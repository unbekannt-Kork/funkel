// Rand und Feld eines Bildschirms. Rust schickt den Zustand, hier wird nur gezeigt.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

type Zustand =
  | { phase: "hoert"; feld: boolean }
  | { phase: "pegel"; wert: number }
  | { phase: "erkennt"; feld: boolean; anteil: number }
  | { phase: "fertig"; feld: boolean; modi: string[]; aktiv: number }
  | { phase: "aus" }
  | { phase: "fehler"; feld: boolean; text: string };

// Stufen des Rands: sichtbare Breite und Weichzeichner in Pixeln
// Aufnahme: kurz breit, dann schmal im Hintergrund
const STUFEN = { schmal: [7, 12], breit: [11, 18] } as const;
type Stufe = keyof typeof STUFEN;
const RADIUS = 40;
const ANLAUF_MS = 1300; // Start: kurz breit, dann schmal im Hintergrund
const GRUEN_MINDESTENS_MS = 900; // die Erkennung ist oft schneller — so lange bleibt Grün sichtbar
const BIM_MS = 450;
// Grüne Kante: Breite in Pixeln, wächst mit dem Fortschritt
const KANTE_START = 3;
const KANTE_ENDE = 14;
const FERTIG_STEHT_MS = 2500;
const FEHLER_STEHT_MS = 5000;
const WAHL_AB_MS = 900; // erst „✓ Fertig“ lesen lassen, dann die Modi zeigen
const WAHL_MS = 3600; // passt zu WAHL im Rust-Ablauf (4,5 s ab Fertig)

const rand = document.getElementById("rand")!;
const kante = document.getElementById("kante")!;
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
    for (const art of ["kuehl", "warm"]) {
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
// Einstellung „Anzeige“: false = nur das Feld, kein Rahmen
let mitRand = true;
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

// Ein Fortschritt für Kante und Balken: beide gleiten im selben Takt zum Ziel,
// sonst lief der Balken gleichmäßig und die Kante abgebremst (Louis, 08.11.2026:
// „nicht synchronisiert, das triggert“). Zeitgeber statt requestAnimationFrame.
let anteilIst = 0;
let anteilZiel = 0;
let bimZusatz = 0; // Bim: Kante kurz breiter als 100 %
let fortschrittTakt = 0;
// Whisper meldet nur am Anfang jedes 30-s-Fensters; der Rest nach dem Stopp ist kürzer,
// also kam 0 % und nach 1,4 s 100 % — der Balken stand so lange (Louis, 09.11.2026:
// „komische Pause“). Während der Erkennung läuft er darum selbst weiter, erst zügig,
// dann langsamer, nie ganz voll; echte Meldungen schieben ihn nur nach vorn.
const SCHAETZUNG_BIS = 0.95;
const SCHAETZUNG_MS = 1200; // nach 1,4 s etwa zwei Drittel
let schaetzenSeit = 0; // 0 = keine Schätzung

function schaetzung() {
  if (!schaetzenSeit) return 0;
  return SCHAETZUNG_BIS * (1 - Math.exp(-(Date.now() - schaetzenSeit) / SCHAETZUNG_MS));
}

function zeichnen() {
  kanteMaske(KANTE_START + (KANTE_ENDE - KANTE_START) * anteilIst + bimZusatz);
  balken.style.width = `${(anteilIst * 100).toFixed(2)}%`;
}

function kanteMaske(dicke: number) {
  const b = window.innerWidth, h = window.innerHeight, d = dicke, r = 34;
  const innen =
    `M${d + r},${d} H${b - d - r} A${r},${r} 0 0 1 ${b - d},${d + r} V${h - d - r} ` +
    `A${r},${r} 0 0 1 ${b - d - r},${h - d} H${d + r} A${r},${r} 0 0 1 ${d},${h - d - r} ` +
    `V${d + r} A${r},${r} 0 0 1 ${d + r},${d} Z`;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${b}" height="${h}" viewBox="0 0 ${b} ${h}">` +
    `<path fill="#000" fill-rule="evenodd" d="M0,0 H${b} V${h} H0 Z ${innen}"/></svg>`;
  const m = `url("data:image/svg+xml;utf8,${encodeURIComponent(svg)}")`;
  kante.style.setProperty("-webkit-mask-image", m);
  kante.style.setProperty("mask-image", m);
}

/** Fortschritt 0…1 für Kante und Balken; `sofort` springt ohne Gleiten. */
function fortschritt(anteil: number, sofort = false) {
  anteilZiel = Math.max(0, Math.min(1, anteil));
  if (sofort) {
    anteilIst = anteilZiel;
    zeichnen();
    return;
  }
  if (fortschrittTakt) return;
  // Gleichmäßig, höchstens 1,2 % je Takt: Whisper meldet in Sprüngen
  fortschrittTakt = window.setInterval(() => {
    // Während der Schätzung nie rückwärts, auch wenn Whisper danach noch 0 % meldet
    const ziel = Math.max(anteilZiel, schaetzung(), schaetzenSeit ? anteilIst : 0);
    const schritt = Math.sign(ziel - anteilIst) * Math.min(Math.abs(ziel - anteilIst), 0.012);
    anteilIst += schritt;
    zeichnen();
    if (anteilIst === ziel && !schaetzenSeit) {
      clearInterval(fortschrittTakt);
      fortschrittTakt = 0;
    }
  }, 16);
}

function hoert(mitFeld: boolean) {
  alleAbbrechen();
  schaetzenSeit = 0;
  kante.className = "";
  rand.className = mitRand ? "an" : "";
  zeigeStufe("breit");
  spaeter(ANLAUF_MS, () => zeigeStufe("schmal"));
  if (mitFeld) {
    const start = Date.now();
    uhr.textContent = "0:00";
    bimZusatz = 0;
    fortschritt(0, true);
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
    // Farbschein aus, klare grüne Kante an
    rand.className = "";
    bimZusatz = 0;
    fortschritt(0, true);
    schaetzenSeit = Date.now();
    kante.className = mitRand ? "an" : "";
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
  fortschritt(anteil);
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
  schaetzenSeit = 0;
  // Erst gleichmäßig bis 100 % (Balken und Kante zusammen), dann Bim
  fortschritt(1);
  const bisVoll = ((1 - anteilIst) / 0.012) * 16;
  spaeter(Math.max(warten, bisVoll + 40), () => {
    // Kante ganz hoch, einmal aufblitzen mit kleinem Ton, dann weg
    fortschritt(1, true);
    kante.className = mitRand ? "an bim" : "";
    // Bim: kurz deutlich breiter, dann zurück
    spaeter(60, () => ((bimZusatz = 10), zeichnen()));
    spaeter(200, () => ((bimZusatz = 0), zeichnen()));
    if (mitFeld) invoke("ton_fertig").catch(() => {});
    spaeter(BIM_MS, () => (kante.className = "weg"));
    if (!mitFeld) return;
    feld.classList.remove("erkennt", "balken-da", "hoert");
    feld.classList.add("an", "fertig");
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
  schaetzenSeit = 0;
  rand.className = "";
  kante.className = "";
  feld.classList.replace("an", "weg");
}

function fehler(mitFeld: boolean, text: string) {
  alleAbbrechen();
  schaetzenSeit = 0;
  rand.className = "";
  kante.className = "";
  if (!mitFeld) return;
  satz.textContent = text;
  feldSetzen("fehler");
  spaeter(20, () => feld.classList.add("an"));
  spaeter(FEHLER_STEHT_MS, () => feld.classList.replace("an", "weg"));
}

bauen();
window.addEventListener("resize", bauen);

// Vorschau ohne Tauri, zum Prüfen in einem unsichtbaren Fenster:
// overlay.html#vorschau=hoert | erkennt-0.4 | bim
const vorschau = location.hash.match(/^#vorschau=(\w+)(?:-([\d.]+))?/);
if (vorschau) {
  document.body.style.background = "#1c1c22"; // nur zum Ansehen
  // Unsichtbare Fenster halten Übergänge an: in der Vorschau sofort den Endzustand zeigen
  const stil = document.createElement("style");
  stil.textContent = "*{transition:none!important}";
  document.head.appendChild(stil);
  const [, zustand, wert] = vorschau;
  if (zustand === "hoert") hoert(true);
  if (zustand === "erkennt") {
    hoert(true);
    erkennt(true, Number(wert ?? 0));
    erkennt(true, Number(wert ?? 0));
  }
  if (zustand === "wahl") {
    wahlZeigen(["Wortgetreu", "Bereinigt", "Mail", "WhatsApp", "Protokoll"], Number(wert ?? 0));
    feldSetzen("an", "fertig", "zusammen", "wahl");
  }
  if (zustand === "bim") {
    erkennt(true, 1);
    fortschritt(1, true);
    bimZusatz = 10;
    zeichnen();
  }
}

// Nur Ereignisse an dieses Fenster (und an alle): `listen` aus api/event hörte auch
// die der anderen Bildschirme mit, dann stand das Feld überall.
if (!vorschau) getCurrentWebviewWindow().listen<Zustand>("zustand", ({ payload: z }) => {
  if ("rand" in z) mitRand = (z as { rand?: boolean }).rand !== false;
  switch (z.phase) {
    case "hoert": return hoert(z.feld);
    case "pegel": return warme.forEach((w) => (w.style.opacity = String(z.wert)));
    case "erkennt": return erkennt(z.feld, z.anteil);
    case "fertig": return fertig(z.feld, z.modi, z.aktiv);
    case "aus": return aus();
    case "fehler": return fehler(z.feld, z.text);
  }
});
