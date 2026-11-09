// Rand und Feld eines Bildschirms. Rust schickt den Zustand, hier wird nur gezeigt.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

type Zustand =
  | { phase: "hoert"; feld: boolean }
  | { phase: "pegel"; wert: number }
  | { phase: "erkennt"; feld: boolean; dauer: number }
  | { phase: "fertig"; feld: boolean; modi: string[]; aktiv: number }
  | { phase: "aus" }
  | { phase: "fehler"; feld: boolean; text: string };

// Stufen des Rands: sichtbare Breite und Weichzeichner in Pixeln
// Aufnahme: kurz breit, dann schmal im Hintergrund
const STUFEN = { schmal: [7, 12], breit: [11, 18] } as const;
type Stufe = keyof typeof STUFEN;
const RADIUS = 40;
const ANLAUF_MS = 1300; // Start: kurz breit, dann schmal im Hintergrund
const KANTE_STEHT_MS = 250; // voll und grün kurz stehen lassen, dann ausblenden
// Grüne Kante: Breite in Pixeln, wächst mit dem Fortschritt
const KANTE_START = 3;
const KANTE_ENDE = 14;
const FERTIG_STEHT_MS = 2500;
const FEHLER_STEHT_MS = 5000;
const WAHL_AB_MS = 900; // erst „✓ Fertig“ lesen lassen, dann die Modi zeigen
const WAHL_MS = 7100; // passt zu WAHL im Rust-Ablauf (8 s ab Fertig)

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

// Ein Fortschritt für Kante und Balken, an einer Uhr: beide laufen linear von 0 bis
// 100 % (Louis, 09.11.2026: „gleichmäßig und sauber durchlaufen“, nicht langsam
// anfangen, nicht zurückspringen). Rust schickt die erwartete Dauer; wird die
// Erkennung früher fertig, läuft der Balken im selben Tempo zu Ende, dauert sie
// länger, wartet er kurz vor dem Ende. Zeitgeber statt requestAnimationFrame.
const HALT_VOR_ENDE = 0.97;
let anteilIst = 0;
let laufStart = 0;
let laufDauer = 1;
let laufFertig = false;
let laufTakt = 0;
let wennVoll: (() => void) | null = null;

function zeichnen() {
  kanteMaske(KANTE_START + (KANTE_ENDE - KANTE_START) * anteilIst);
  balken.style.width = `${(anteilIst * 100).toFixed(2)}%`;
}

function laufAnhalten() {
  if (laufTakt) clearInterval(laufTakt);
  laufTakt = 0;
  wennVoll = null;
}

function laufen(dauerMs: number) {
  laufAnhalten();
  anteilIst = 0;
  laufStart = Date.now();
  laufDauer = Math.max(dauerMs, 1);
  laufFertig = false;
  zeichnen();
  laufTakt = window.setInterval(() => {
    const a = (Date.now() - laufStart) / laufDauer;
    anteilIst = Math.min(a, laufFertig ? 1 : HALT_VOR_ENDE);
    zeichnen();
    if (anteilIst >= 1) {
      const danach = wennVoll;
      laufAnhalten();
      danach?.();
    }
  }, 16);
}

/** Erkennung fertig: im selben Tempo bis 100 %, dann `danach`. */
function laufBeenden(danach: () => void) {
  if (!laufTakt) {
    anteilIst = 1;
    zeichnen();
    return danach();
  }
  // Uhr so stellen, dass es ohne Sprung vom jetzigen Stand weitergeht
  laufStart = Date.now() - anteilIst * laufDauer;
  laufFertig = true;
  wennVoll = danach;
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

function hoert(mitFeld: boolean) {
  alleAbbrechen();
  laufAnhalten();
  erkenntLaeuft = false;
  kante.className = "";
  rand.className = mitRand ? "an" : "";
  zeigeStufe("breit");
  spaeter(ANLAUF_MS, () => zeigeStufe("schmal"));
  if (mitFeld) {
    const start = Date.now();
    uhr.textContent = "0:00";
    anteilIst = 0;
    zeichnen();
    feldSetzen("hoert");
    spaeter(20, () => feld.classList.add("an"));
    uhrTakt = window.setInterval(() => (uhr.textContent = uhrText(Date.now() - start)), 250);
  } else {
    feldSetzen("aus-hier");
  }
}

let erkenntLaeuft = false;

function erkennt(mitFeld: boolean, dauer: number) {
  // Nur der erste Aufruf startet; ein zweiter darf den Lauf nicht zurücksetzen
  if (erkenntLaeuft) return;
  erkenntLaeuft = true;
  alleAbbrechen();
  // Farbschein aus, klare grüne Kante an
  rand.className = "";
  kante.className = mitRand ? "an" : "";
  laufen(dauer);
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
  erkenntLaeuft = false;
  alleAbbrechen();
  // Balken und Kante laufen gleichmäßig voll, dann ist die Kante breit und grün und blendet aus
  laufBeenden(() => {
    if (mitFeld) invoke("ton_fertig").catch(() => {});
    spaeter(KANTE_STEHT_MS, () => (kante.className = mitRand ? "weg" : ""));
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
  laufAnhalten();
  erkenntLaeuft = false;
  rand.className = "";
  kante.className = "";
  feld.classList.replace("an", "weg");
}

function fehler(mitFeld: boolean, text: string) {
  alleAbbrechen();
  laufAnhalten();
  erkenntLaeuft = false;
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
// overlay.html#vorschau=hoert | erkennt-0.4 | wahl-1
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
    erkennt(true, 1000);
    laufAnhalten();
    anteilIst = Number(wert ?? 0);
    zeichnen();
  }
  if (zustand === "wahl") {
    wahlZeigen(["Wortgetreu", "Bereinigt", "Mail", "WhatsApp", "Protokoll"], Number(wert ?? 0));
    feldSetzen("an", "fertig", "zusammen", "wahl");
  }
}

// Nur Ereignisse an dieses Fenster (und an alle): `listen` aus api/event hörte auch
// die der anderen Bildschirme mit, dann stand das Feld überall.
if (!vorschau) getCurrentWebviewWindow().listen<Zustand>("zustand", ({ payload: z }) => {
  if ("rand" in z) mitRand = (z as { rand?: boolean }).rand !== false;
  switch (z.phase) {
    case "hoert": return hoert(z.feld);
    case "pegel": return warme.forEach((w) => (w.style.opacity = String(z.wert)));
    case "erkennt": return erkennt(z.feld, z.dauer);
    case "fertig": return fertig(z.feld, z.modi, z.aktiv);
    case "aus": return aus();
    case "fehler": return fehler(z.feld, z.text);
  }
});
