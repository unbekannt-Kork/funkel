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
// Ein Eckenradius für Farbrand und grüne Kante: außen rund, innen gleich weit
// eingerückt (konzentrisch), damit das Band rundum gleich dick ist (13.11.2026:
// außen eckig und innen 33 px rund machte die Ecken dick und „abgeknackt“).
const RADIUS = 28;
const ANLAUF_MS = 1300; // Start: kurz breit, dann schmal im Hintergrund
const KANTE_STEHT_MS = 250; // voll und grün kurz stehen lassen, dann ausblenden
// Grüne Kante: Breite in Pixeln, wächst mit dem Fortschritt
const KANTE_START = 3;
const KANTE_ENDE = 14;
const FERTIG_STEHT_MS = 2500;
const FEHLER_STEHT_MS = 5000;
const WAHL_AB_MS = 900; // erst „✓ Fertig“ lesen lassen, dann die Modi zeigen
const WAHL_MS = 7100; // passt zu WAHL im Rust-Ablauf (8 s ab Fertig)
const PUNKT_ZUM_BALKEN_MS = 320; // passt zu --wandel in overlay.css

const rand = document.getElementById("rand")!;
const kante = document.getElementById("kante")!;
const feld = document.getElementById("feld")!;
const uhr = document.getElementById("uhr")!;
const balken = document.getElementById("balken")!;
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
// Dieses Fenster gehört zum Bildschirm des Diktats (für den Ton, auch ohne Feld)
let hier = false;

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

// Ein Fortschritt für Kante und Balken, an einer Uhr (Louis, 09.11.2026: „gleichmäßig
// und sauber durchlaufen“, nicht langsam anfangen, nicht zurückspringen). Rust schickt
// die erwartete Dauer. Bis 70 % läuft der Balken gleichmäßig, danach ohne Knick
// immer langsamer Richtung 99 %, bleibt aber nie stehen (13.11.2026: das Anhalten bei
// 97 % wirkte wie ein Hänger). Ist die Erkennung fertig, läuft er zügig voll.
// Zeitgeber statt requestAnimationFrame.
const GLEICHMAESSIG_BIS = 0.7;
const SCHLUSS_MS = [180, 420] as const;
let anteilIst = 0;
let laufStart = 0;
let laufDauer = 1;
let laufTakt = 0;
let wennVoll: (() => void) | null = null;
let endeStart = 0;
let endeVon = 0;
let endeDauer = 0;

/** Anteil nach `t` ms bei erwarteter Dauer `d`: gleichmäßig, dann ohne Knick abflachend. */
function verlauf(t: number, d: number) {
  const knick = GLEICHMAESSIG_BIS * d;
  if (t <= knick) return t / d;
  const rest = 0.99 - GLEICHMAESSIG_BIS;
  const x = t - knick;
  // Steigung am Knick gleich 1/d: tau = rest * d
  return GLEICHMAESSIG_BIS + rest * (x / (x + rest * d));
}

function zeichnen() {
  kanteDicke(KANTE_START + (KANTE_ENDE - KANTE_START) * anteilIst);
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
  endeStart = 0;
  zeichnen();
  laufTakt = window.setInterval(() => {
    const jetzt = Date.now();
    if (endeStart) {
      // Schluss: vom jetzigen Stand gleichmäßig auf 100 %
      anteilIst = Math.min(1, endeVon + (1 - endeVon) * ((jetzt - endeStart) / endeDauer));
    } else {
      anteilIst = verlauf(jetzt - laufStart, laufDauer);
    }
    zeichnen();
    if (anteilIst >= 1) {
      const danach = wennVoll;
      laufAnhalten();
      danach?.();
    }
  }, 16);
}

/** Erkennung fertig: zügig und ohne Sprung bis 100 %, dann `danach`. */
function laufBeenden(danach: () => void) {
  if (!laufTakt) {
    anteilIst = 1;
    zeichnen();
    return danach();
  }
  endeVon = anteilIst;
  endeStart = Date.now();
  // Nicht langsamer als bisher, aber auch kein Schnappen
  endeDauer = Math.min(SCHLUSS_MS[1], Math.max(SCHLUSS_MS[0], (1 - anteilIst) * laufDauer));
  wennVoll = danach;
}

/** Grüne Kante: nur eine Zahl je Bild, die Form zeichnet CSS (gleicher Radius wie der Farbrand) */
function kanteDicke(dicke: number) {
  kante.style.setProperty("--dicke", `${dicke.toFixed(2)}px`);
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
      // Der Punkt ist dann am Balkenanfang angekommen und verschwindet im Balken
      spaeter(PUNKT_ZUM_BALKEN_MS, () => feld.classList.add("balken-da"));
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
    if (hier) invoke("ton_fertig").catch(() => {});
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
  if (zustand === "kante") {
    erkennt(true, 1000);
    laufAnhalten();
    anteilIst = Number(wert ?? 0);
    zeichnen();
  }
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
  if ("hier" in z) hier = (z as { hier?: boolean }).hier === true;
  switch (z.phase) {
    case "hoert": return hoert(z.feld);
    case "pegel": return warme.forEach((w) => (w.style.opacity = String(z.wert)));
    case "erkennt": return erkennt(z.feld, z.dauer);
    case "fertig": return fertig(z.feld, z.modi, z.aktiv);
    case "aus": return aus();
    case "fehler": return fehler(z.feld, z.text);
  }
});
