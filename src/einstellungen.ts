import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Einstellungen = {
  taste: number | null;
  einfuegen: boolean;
  sprache: string;
  autostart: boolean;
};
type Status = {
  einstellungen: Einstellungen;
  taste_name: string;
  freigabe: boolean;
  modell: boolean;
  woerterbuch: string;
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
  $("taste-name").textContent = s.taste_name;
  $<HTMLInputElement>("einfuegen").checked = werte.einfuegen;
  $<HTMLSelectElement>("sprache").value = werte.sprache;
  $<HTMLInputElement>("autostart").checked = werte.autostart;
  $<HTMLTextAreaElement>("woerterbuch").value = s.woerterbuch;
}

async function speichern() {
  werte.einfuegen = $<HTMLInputElement>("einfuegen").checked;
  werte.sprache = $<HTMLSelectElement>("sprache").value;
  werte.autostart = $<HTMLInputElement>("autostart").checked;
  await invoke("speichern", { einstellungen: werte });
}

for (const id of ["einfuegen", "sprache", "autostart"]) $(id).addEventListener("change", speichern);
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
