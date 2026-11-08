// Rand und Feld eines Bildschirms. Rust schickt den Zustand, hier wird nur gezeigt.
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

type Zustand =
  | { phase: "hoert"; feld: boolean }
  | { phase: "pegel"; wert: number }
  | { phase: "erkennt"; feld: boolean; anteil: number }
  | { phase: "fertig"; feld: boolean; modi: string[]; aktiv: number }
  | { phase: "aus" }
  | { phase: "fehler"; feld: boolean; text: string };

// Stufen des Rands: sichtbare Breite und Weichzeichner in Pixeln
const STUFEN = { schmal: [6, 10], breit: [9, 14], bluete: [22, 18] } as const;
type Stufe = keyof typeof STUFEN;
const RADIUS = 40;
const ANLAUF_MS = 1300; // Start: kurz breit, dann schmal im Hintergrund
const GRUEN_MINDESTENS_MS = 900; // die Erkennung ist oft schneller — so lange bleibt Grün sichtbar
const BLUETE_MS = 500;
const FERTIG_STEHT_MS = 2500;
const FEHLER_STEHT_MS = 5000;
const WAHL_AB_MS = 900; // erst „✓ Fertig“ lesen lassen, dann die Modi zeigen
const WAHL_MS = 3600; // passt zu WAHL im Rust-Ablauf (4,5 s ab Fertig)

const rand = document.getElementById("rand")!;
const feld = document.getElementById("feld")!;
const uhr = document.getElementById("uhr")!;
const balken = document.getElementById("balken")!;
const satz = document.getElementById("satz")!;
const wahl = document.getElementById("wahl")!;

const baender: Record<Stufe, HTMLDivElement> = {} as never;
const warme: HTMLDivElement[] = [];

/** Maske eines Bands: Bildschirm minus innen abgerundetes Rechteck, weich gezeichnet.
 *  Außen reicht die Fläche über den Rand hinaus, damit der Weichzeichner dort nicht ausfranst. */
function maske(breite: number, hoehe: number, band: number, weich: number): string {
  const r = Math.max(RADIUS - band, 8);
  const x = band, y = band, w = breite - 2 * band, h = hoehe - 2 * band;
  const innen =
    `M${x + r},${y} H${x + w - r} A${r},${r} 0 0 1 ${x + w},${y + r} V${y + h - r} ` +
    `A${r},${r} 0 0 1 ${x + w - r},${y + h} H${x + r} A${r},${r} 0 0 1 ${x},${y + h - r} ` +
    `V${y + r} A${r},${r} 0 0 1 ${x + r},${y} Z`;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${breite}" height="${hoehe}" viewBox="0 0 ${breite} ${hoehe}">` +
    `<filter id="w" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="${weich / 2}"/></filter>` +
    `<path filter="url(#w)" fill="#000" fill-rule="evenodd" d="M-60,-60 H${breite + 60} V${hoehe + 60} H-60 Z ${innen}"/>` +
    `</svg>`;
  return `url("data:image/svg+xml;utf8,${encodeURIComponent(svg)}")`;
}

function bauen() {
  rand.replaceChildren();
  warme.length = 0;
  const breite = window.innerWidth, hoehe = window.innerHeight;
  rand.style.setProperty("--seite", `${Math.ceil(Math.hypot(breite, hoehe))}px`);
  for (const stufe of Object.keys(STUFEN) as Stufe[]) {
    const [band, weich] = STUFEN[stufe];
    const el = document.createElement("div");
    el.className = "band";
    const m = maske(breite, hoehe, band, weich);
    el.style.setProperty("-webkit-mask-image", m);
    el.style.setProperty("mask-image", m);
    for (const art of ["kuehl", "warm", "gruen"]) {
      const kegel = document.createElement("div");
      kegel.className = `kegel ${art}`;
      if (art === "warm") warme.push(kegel);
      el.appendChild(kegel);
    }
    rand.appendChild(el);
    baender[stufe] = el;
  }
}

function zeigeStufe(stufe: Stufe | null) {
  for (const [name, el] of Object.entries(baender)) el.classList.toggle("sichtbar", name === stufe);
}

// -- Zeitgeber, die beim nächsten Zustand verfallen -------------------------
let zeitgeber: number[] = [];
function spaeter(ms: number, f: () => void) {
  zeitgeber.push(window.setTimeout(f, ms));
}
function alleAbbrechen() {
  zeitgeber.forEach(clearTimeout);
  zeitgeber = [];
  if (uhrTakt) clearInterval(uhrTakt);
  uhrTakt = 0;
}

let uhrTakt = 0;
let erkenntSeit = 0;

function uhrText(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// Kein requestAnimationFrame: WebKit drosselt es in Fenstern, die es für verdeckt
// hält — das durchsichtige Randfenster gilt dafür, und das Feld blieb unsichtbar.
function feldSetzen(...klassen: string[]) {
  feld.className = "";
  void feld.offsetWidth; // Animationen neu starten
  feld.classList.add(...klassen);
}

function hoert(mitFeld: boolean) {
  alleAbbrechen();
  rand.className = "an";
  zeigeStufe("breit");
  spaeter(ANLAUF_MS, () => zeigeStufe("schmal"));
  if (mitFeld) {
    const start = Date.now();
    uhr.textContent = "0:00";
    balken.style.width = "0";
    feldSetzen("hoert");
    spaeter(20, () => feld.classList.add("an"));
    uhrTakt = window.setInterval(() => (uhr.textContent = uhrText(Date.now() - start)), 250);
  } else {
    feldSetzen("aus-hier");
  }
}

function erkennt(mitFeld: boolean, anteil: number) {
  if (!rand.classList.contains("erkennt")) {
    alleAbbrechen();
    erkenntSeit = Date.now();
    rand.className = "an erkennt";
    zeigeStufe("breit");
    if (mitFeld) {
      if (feld.classList.contains("hoert")) {
        feld.classList.remove("hoert");
        feld.classList.add("an", "erkennt");
        spaeter(420, () => feld.classList.add("balken-da"));
      } else {
        // Umwandeln aus der Schnellwahl: gleich wieder der Balken
        feldSetzen("an", "erkennt", "balken-da");
      }
    }
  }
  if (mitFeld) balken.style.width = `${Math.round(Math.max(0, Math.min(1, anteil)) * 100)}%`;
}

function wahlZeigen(modi: string[], aktiv: number) {
  wahl.replaceChildren(
    ...modi.map((name, i) => {
      const k = document.createElement("span");
      k.className = "wahl-knopf" + (i === aktiv ? " aktiv" : "");
      const taste = document.createElement("kbd");
      taste.textContent = String(i + 1);
      k.append(taste, name);
      return k;
    }),
  );
}

function fertig(mitFeld: boolean, modi: string[] = [], aktiv = 0) {
  const warten = Math.max(0, GRUEN_MINDESTENS_MS - (Date.now() - erkenntSeit));
  alleAbbrechen();
  spaeter(warten, () => {
    // Rand: auf die grünste Stufe hoch, einmal weit, dann verglimmen
    rand.className = "an fertig";
    zeigeStufe("bluete");
    spaeter(BLUETE_MS, () => (rand.className = "weg fertig"));
    if (!mitFeld) return;
    feld.classList.remove("erkennt", "balken-da", "hoert");
    feld.classList.add("an", "fertig");
    balken.style.width = "100%";
    spaeter(180, () => feld.classList.add("zusammen"));
    if (modi.length) {
      wahlZeigen(modi, aktiv);
      spaeter(WAHL_AB_MS, () => feld.classList.add("wahl"));
      spaeter(WAHL_AB_MS + WAHL_MS, () => feld.classList.replace("an", "weg"));
    } else {
      spaeter(FERTIG_STEHT_MS, () => feld.classList.replace("an", "weg"));
    }
  });
}

function aus() {
  alleAbbrechen();
  rand.className = "";
  feld.classList.replace("an", "weg");
}

function fehler(mitFeld: boolean, text: string) {
  alleAbbrechen();
  rand.className = "";
  if (!mitFeld) return;
  satz.textContent = text;
  feldSetzen("fehler");
  spaeter(20, () => feld.classList.add("an"));
  spaeter(FEHLER_STEHT_MS, () => feld.classList.replace("an", "weg"));
}

bauen();
window.addEventListener("resize", bauen);

// Nur Ereignisse an dieses Fenster (und an alle): `listen` aus api/event hörte auch
// die der anderen Bildschirme mit, dann stand das Feld überall.
getCurrentWebviewWindow().listen<Zustand>("zustand", ({ payload: z }) => {
  switch (z.phase) {
    case "hoert": return hoert(z.feld);
    case "pegel": return warme.forEach((w) => (w.style.opacity = String(z.wert)));
    case "erkennt": return erkennt(z.feld, z.anteil);
    case "fertig": return fertig(z.feld, z.modi, z.aktiv);
    case "aus": return aus();
    case "fehler": return fehler(z.feld, z.text);
  }
});
