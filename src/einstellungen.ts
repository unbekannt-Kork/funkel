import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Einstellungen = {
  taste: number | null;
  einfuegen: boolean;
  sprache: string;
  autostart: boolean;
  aufbewahren_tage: number;
  modus: string;
  personen: number;
  ton: boolean;
  anzeige: string;
};
type Eintrag = { pfad: string; zeit: string; sekunden: number; text: string | null };
type Status = {
  einstellungen: Einstellungen;
  taste_name: string;
  freigabe: boolean;
  modell: boolean;
  sprachmodell: boolean;
  woerterbuch: string;
  verlauf: Eintrag[];
};

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
let werte: Einstellungen;

function marke(id: string, gut: boolean) {
  $(id).classList.toggle("gut", gut);
}

async function laden() {
  const s = await invoke<Status>("status");
  werte = s.einstellungen;
  marke("freigabe-marke", s.freigabe);
  $("freigabe-text").textContent = s.freigabe
    ? "wird gelesen"
    : "Freigabe fehlt — ohne sie sieht Funkel die Taste nicht";
  $("freigabe-knopf").hidden = s.freigabe;
  marke("modell-marke", s.modell);
  $("modell-text").textContent = s.modell ? "geladen, läuft offline" : "noch nicht geladen (einmalig, danach offline)";
  $("modell-knopf").hidden = s.modell;
  marke("sprachmodell-marke", s.sprachmodell);
  if ($("sprachmodell-fortschritt").hidden)
    $("sprachmodell-text").textContent = s.sprachmodell
      ? "Sprachmodell geladen: bereinigt, Mail, WhatsApp, Protokoll"
      : "für alles außer wortgetreu: lokales Sprachmodell laden (einmalig)";
  $("sprachmodell-knopf").hidden = s.sprachmodell;
  $<HTMLSelectElement>("modus").value = werte.modus;
  $<HTMLSelectElement>("personen").value = String(werte.personen);
  $("taste-name").textContent = s.taste_name;
  $<HTMLInputElement>("einfuegen").checked = werte.einfuegen;
  $<HTMLInputElement>("ton").checked = werte.ton;
  $<HTMLSelectElement>("anzeige").value = werte.anzeige;
  $<HTMLSelectElement>("sprache").value = werte.sprache;
  $<HTMLInputElement>("autostart").checked = werte.autostart;
  // Nicht überschreiben, während jemand tippt
  if (document.activeElement?.id !== "woerterbuch") $<HTMLTextAreaElement>("woerterbuch").value = s.woerterbuch;
  $<HTMLSelectElement>("aufbewahren").value = String(werte.aufbewahren_tage);
  verlaufZeigen(s.verlauf);
}

function dauer(s: number) {
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function eintrag(e: Eintrag, knopf: string, tun: (b: HTMLButtonElement) => void) {
  const zeile = document.createElement("div");
  zeile.className = "eintrag";
  const wann = document.createElement("span");
  wann.className = "wann";
  wann.textContent = `${e.zeit} · ${dauer(e.sekunden)}`;
  const inhalt = document.createElement("span");
  inhalt.className = "inhalt";
  inhalt.textContent = e.text ?? "Aufnahme ohne Text — nach Absturz oder Abbruch";
  const b = document.createElement("button");
  b.textContent = knopf;
  b.addEventListener("click", () => tun(b));
  zeile.append(wann, inhalt, b);
  return zeile;
}

let letzterVerlauf = "";
function verlaufZeigen(liste: Eintrag[]) {
  const schluessel = JSON.stringify(liste);
  if (schluessel === letzterVerlauf) return;
  letzterVerlauf = schluessel;
  const liegen = liste.filter((e) => e.text === null && e.sekunden > 0);
  $("liegen-teil").hidden = liegen.length === 0;
  $("liegen").replaceChildren(
    ...liegen.map((e) =>
      eintrag(e, "Wiederherstellen", (b) => {
        b.disabled = true;
        b.textContent = "erkennt …";
        invoke("wiederherstellen", { pfad: e.pfad }).catch((f) => (b.textContent = String(f)));
      }),
    ),
  );
  const fertige = liste.filter((e) => e.text);
  if (fertige.length)
    $("verlauf").replaceChildren(
      ...fertige.map((e) =>
        eintrag(e, "Kopieren", (b) => {
          invoke("kopieren", { text: e.text });
          b.textContent = "kopiert";
          setTimeout(() => (b.textContent = "Kopieren"), 1500);
        }),
      ),
    );
}

async function speichern() {
  werte.einfuegen = $<HTMLInputElement>("einfuegen").checked;
  werte.ton = $<HTMLInputElement>("ton").checked;
  werte.anzeige = $<HTMLSelectElement>("anzeige").value;
  werte.sprache = $<HTMLSelectElement>("sprache").value;
  werte.autostart = $<HTMLInputElement>("autostart").checked;
  werte.modus = $<HTMLSelectElement>("modus").value;
  werte.personen = Number($<HTMLSelectElement>("personen").value);
  werte.aufbewahren_tage = Number($<HTMLSelectElement>("aufbewahren").value);
  await invoke("speichern", { einstellungen: werte });
}

$("ordner-knopf").addEventListener("click", () => invoke("ordner_oeffnen"));
$("sprachmodell-knopf").addEventListener("click", () => {
  $<HTMLButtonElement>("sprachmodell-knopf").disabled = true;
  $("sprachmodell-fortschritt").hidden = false;
  invoke("sprachmodell_laden");
});
listen<{ geladen: number; gesamt: number }>("sprachmodell-fortschritt", ({ payload }) => {
  const anteil = payload.gesamt ? payload.geladen / payload.gesamt : 0;
  $("sprachmodell-balken").style.width = `${Math.round(anteil * 100)}%`;
  $("sprachmodell-text").textContent = `${Math.round(payload.geladen / 1e6)} von ${Math.round(payload.gesamt / 1e6)} MB`;
});
listen<string | null>("sprachmodell-fertig", ({ payload: fehler }) => {
  $("sprachmodell-fortschritt").hidden = true;
  $<HTMLButtonElement>("sprachmodell-knopf").disabled = false;
  if (fehler) $("sprachmodell-text").textContent = `Laden fehlgeschlagen: ${fehler}`;
  else laden();
});
for (const id of ["anzeige", "ton", "einfuegen", "sprache", "autostart", "aufbewahren", "modus", "personen"]) $(id).addEventListener("change", speichern);
$("woerterbuch").addEventListener("change", () =>
  invoke("woerterbuch_speichern", { text: $<HTMLTextAreaElement>("woerterbuch").value }),
);
$("freigabe-knopf").addEventListener("click", () => invoke("freigabe_oeffnen"));
$("modell-knopf").addEventListener("click", () => {
  $<HTMLButtonElement>("modell-knopf").disabled = true;
  $("modell-fortschritt").hidden = false;
  invoke("modell_laden");
});
$("taste-knopf").addEventListener("click", () => {
  $("taste-name").textContent = "jetzt drücken …";
  invoke("taste_lernen");
});

listen<{ geladen: number; gesamt: number }>("modell-fortschritt", ({ payload }) => {
  const anteil = payload.gesamt ? payload.geladen / payload.gesamt : 0;
  $("modell-balken").style.width = `${Math.round(anteil * 100)}%`;
  $("modell-text").textContent = `${Math.round(payload.geladen / 1e6)} von ${Math.round(payload.gesamt / 1e6)} MB`;
});
listen<string | null>("modell-fertig", ({ payload: fehler }) => {
  $("modell-fortschritt").hidden = true;
  $<HTMLButtonElement>("modell-knopf").disabled = false;
  if (fehler) $("modell-text").textContent = `Laden fehlgeschlagen: ${fehler}`;
  else laden();
});
listen("status-geaendert", () => laden());

laden();
// Die Freigabe wird außerhalb erteilt — ab und zu nachsehen
setInterval(laden, 3000);
