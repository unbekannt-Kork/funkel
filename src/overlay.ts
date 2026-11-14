// Rand und Feld eines Bildschirms. Rust schickt den Zustand, hier wird nur gezeigt.
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

type Zustand =
  | { phase: "hoert"; feld: boolean }
  | { phase: "pegel"; wert: number }
  | { phase: "erkennt"; feld: boolean }
  | { phase: "fertig"; feld: boolean; modi: string[]; aktiv: number }
  | { phase: "aus" }
  | { phase: "fehler"; feld: boolean; text: string };

// Stufen des Rands: sichtbare Breite und Weichzeichner in Pixeln
// Aufnahme: kurz breit, dann schmal im Hintergrund
const STUFEN = { schmal: [7, 12], breit: [11, 18] } as const;
type Stufe = keyof typeof STUFEN;
// Ein Eckenradius für Farbrand und grüne Kante: außen rund, innen gleich weit
// eingerückt (konzentrisch), damit das Band rundum gleich dick ist (13.11.2026:
// außen eckig und innen 33 px rund machte die Ecken dick und „abgeknackt“).
const RADIUS = 28;
const ANLAUF_MS = 1300; // Start: kurz breit, dann schmal im Hintergrund
const FERTIG_STEHT_MS = 2500;
const FEHLER_STEHT_MS = 5000;
const WAHL_AB_MS = 900; // erst „✓ Fertig“ lesen lassen, dann die Modi zeigen
const WAHL_MS = 7100; // passt zu WAHL im Rust-Ablauf (8 s ab Fertig)

const rand = document.getElementById("rand")!;
const kante = document.getElementById("kante")!;
const feld = document.getElementById("feld")!;
const uhr = document.getElementById("uhr")!;
const satz = document.getElementById("satz")!;
const wahl = document.getElementById("wahl")!;

const baender: Record<Stufe, HTMLDivElement> = {} as never;
const warme: HTMLDivElement[] = [];
let stufeJetzt: Stufe | null = null;
let gebaut = "";

/** Maske eines Bands, einmal je Bildschirmgröße in voller Auflösung gezeichnet:
 *  außen der runde Rand, innen weich auslaufend mit demselben Mittelpunkt der Ecken.
 *  Vorher ein SVG mit Weichzeichner: WebKit rechnete es in halber Auflösung (Stufen). */
function maske(breite: number, hoehe: number, band: number, weich: number): string {
  const dpr = window.devicePixelRatio || 1;
  const leinwand = document.createElement("canvas");
  leinwand.width = Math.round(breite * dpr);
  leinwand.height = Math.round(hoehe * dpr);
  const c = leinwand.getContext("2d")!;
  c.scale(dpr, dpr);
  c.beginPath();
  c.roundRect(0, 0, breite, hoehe, RADIUS);
  c.clip();
  // Der Schatten ist der Weichzeichner: die Form liegt links außerhalb, nur ihr
  // Schatten fällt auf die Leinwand. Versatz und Unschärfe gelten in Gerätepixeln.
  const weg = breite + 400;
  c.shadowColor = "#000";
  c.shadowBlur = weich * dpr;
  c.shadowOffsetX = weg * dpr;
  c.beginPath();
  c.rect(-weg - 200, -200, breite + 400, hoehe + 400);
  c.roundRect(band - weg, band, breite - 2 * band, hoehe - 2 * band, Math.max(RADIUS - band, 0));
  c.fill("evenodd");
  return `url("${leinwand.toDataURL("image/png")}")`;
}

function bauen() {
  const breite = window.innerWidth, hoehe = window.innerHeight;
  // Gleiche Größe: nichts tun. Neu bauen nahm dem Rand sonst mitten im Diktat die Sichtbarkeit.
  const schluessel = `${breite}x${hoehe}@${window.devicePixelRatio}`;
  if (schluessel === gebaut || !breite || !hoehe) return;
  gebaut = schluessel;
  rand.replaceChildren();
  warme.length = 0;
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
  zeigeStufe(stufeJetzt, false);
}

/** Neue Stufe sofort darunter, die alte blendet darüber aus: so wird der Rand
 *  beim Wechsel nie dunkler (zwei gegenläufige Überblendungen ließen ihn einbrechen). */
function zeigeStufe(stufe: Stufe | null, weich = true) {
  stufeJetzt = stufe;
  for (const [name, el] of Object.entries(baender)) {
    if (name === stufe) {
      el.style.zIndex = "0";
      el.style.transition = "none";
      el.classList.add("sichtbar");
      void el.offsetWidth;
      el.style.transition = "";
    } else {
      el.style.zIndex = "1";
      if (!weich) el.style.transition = "none";
      el.classList.remove("sichtbar");
      if (!weich) {
        void el.offsetWidth;
        el.style.transition = "";
      }
    }
  }
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

// Nach dem Stopp gibt es keinen Fortschritt zu zeigen, nur „arbeitet“: der Balken
// läuft durch, bis das Ergebnis da ist (Theo-Regel „Lädt“). Vorher lief er nach
// einer geschätzten Dauer und kroch ab 70 % immer langsamer — dauerte die Erkennung
// länger als geschätzt, wirkte Funkel stockend, und der Ton kam erst danach (14.11.2026).

function hoert(mitFeld: boolean) {
  alleAbbrechen();
  kante.className = "";
  rand.className = mitRand ? "an" : "";
  zeigeStufe("breit");
  spaeter(ANLAUF_MS, () => zeigeStufe("schmal"));
  if (mitFeld) {
    const start = Date.now();
    uhr.textContent = "0:00";
    feldSetzen("hoert");
    spaeter(20, () => feld.classList.add("an"));
    uhrTakt = window.setInterval(() => (uhr.textContent = uhrText(Date.now() - start)), 250);
  } else {
    feldSetzen("aus-hier");
  }
}

function erkennt(mitFeld: boolean) {
  alleAbbrechen();
  // Farbschein aus, ruhige grüne Kante an
  rand.className = "";
  kante.className = mitRand ? "an" : "";
  if (!mitFeld) return;
  if (feld.classList.contains("hoert")) {
    feld.classList.remove("hoert");
    feld.classList.add("an", "erkennt");
  } else {
    // Umwandeln aus der Schnellwahl: gleich wieder der Balken
    feldSetzen("an", "erkennt");
  }
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

/** Sofort, ohne Nachlauf: der Text liegt schon in der Zwischenablage, der Ton kam aus Rust. */
function fertig(mitFeld: boolean, modi: string[] = [], aktiv = 0) {
  alleAbbrechen();
  kante.className = mitRand ? "weg" : "";
  if (!mitFeld) return;
  feldSetzen("an", "fertig");
  if (modi.length) {
    wahlZeigen(modi, aktiv);
    spaeter(WAHL_AB_MS, () => feld.classList.add("wahl"));
    spaeter(WAHL_AB_MS + WAHL_MS, () => feld.classList.replace("an", "weg"));
  } else {
    spaeter(FERTIG_STEHT_MS, () => feld.classList.replace("an", "weg"));
  }
}

function aus() {
  alleAbbrechen();
  rand.className = "";
  kante.className = "";
  feld.classList.replace("an", "weg");
}

function fehler(mitFeld: boolean, text: string) {
  alleAbbrechen();
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
// overlay.html#vorschau=hoert | erkennt | fertig | wahl-1
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
    erkennt(true);
  }
  if (zustand === "fertig") fertig(true);
  if (zustand === "wahl") {
    wahlZeigen(["Wortgetreu", "Bereinigt", "Mail", "WhatsApp", "Protokoll"], Number(wert ?? 0));
    feldSetzen("an", "fertig", "wahl");
  }
}

// Nur Ereignisse an dieses Fenster (und an alle): `listen` aus api/event hörte auch
// die der anderen Bildschirme mit, dann stand das Feld überall.
if (!vorschau) getCurrentWebviewWindow().listen<Zustand>("zustand", ({ payload: z }) => {
  if ("rand" in z) mitRand = (z as { rand?: boolean }).rand !== false;
  switch (z.phase) {
    case "hoert": return hoert(z.feld);
    case "pegel": return warme.forEach((w) => (w.style.opacity = String(z.wert)));
    case "erkennt": return erkennt(z.feld);
    case "fertig": return fertig(z.feld, z.modi, z.aktiv);
    case "aus": return aus();
    case "fehler": return fehler(z.feld, z.text);
  }
});
