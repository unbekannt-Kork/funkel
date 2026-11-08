//! Bereit → hört zu → erkennt → fertig. Hier laufen Taste, Aufnahme,
//! Erkennung, Zwischenablage und Anzeige zusammen.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::aufnahme::{self, Aufnahme};
use crate::einstellungen::{self, protokoll, Einstellungen};
use crate::erkennung::Erkenner;
use crate::modus::{self, Modus};
use crate::{ablage, ausloeser, helfer, modell, overlay, plattform};

/// Kürzer ist ein versehentlicher Doppeltipp, kein Diktat
const MINDESTDAUER: Duration = Duration::from_millis(400);
/// Leiser als das ist Stille (Effektivwert)
const STILLE: f32 = 0.002;
/// So lange nach „Fertig“ wählen die Ziffern 1–5 einen anderen Modus
const WAHL: Duration = Duration::from_millis(4500);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Bereit,
    Hoert,
    Erkennt,
}

pub struct Ablauf {
    app: AppHandle,
    phase: Mutex<Phase>,
    aufnahme: Mutex<Option<(Aufnahme, Instant)>>,
    feld: Mutex<String>,
    pub erkenner: Arc<Erkenner>,
    /// Für den Abgriff: läuft gerade eine Aufnahme? Dann beendet ein einzelner Tipp.
    pub laeuft: Arc<AtomicBool>,
    pub einstellungen: Mutex<Einstellungen>,
    /// Letztes Diktat: Aufnahme und wortgetreuer Rohtext — Grundlage für das Umwandeln
    letzte: Mutex<Option<(std::path::PathBuf, String)>>,
}

impl Ablauf {
    pub fn neu(app: AppHandle, werte: Einstellungen) -> Arc<Self> {
        Arc::new(Self {
            app,
            phase: Mutex::new(Phase::Bereit),
            aufnahme: Mutex::new(None),
            feld: Mutex::new(String::new()),
            erkenner: Arc::new(Erkenner::neu()),
            laeuft: Arc::new(AtomicBool::new(false)),
            einstellungen: Mutex::new(werte),
            letzte: Mutex::new(None),
        })
    }

    /// Doppeltipp oder Tipp: starten, beenden oder (während der Erkennung) übergehen.
    pub fn umschalten(self: &Arc<Self>) {
        let phase = *self.phase.lock().unwrap();
        match phase {
            Phase::Bereit => self.starten(),
            Phase::Hoert => self.beenden(),
            Phase::Erkennt => protokoll("Taste übergangen: voriger Text wird noch erkannt"),
        }
    }

    fn starten(self: &Arc<Self>) {
        ausloeser::wahl_schliessen();
        overlay::sicherstellen(&self.app);
        let feld = overlay::feld_fenster(&self.app);
        *self.feld.lock().unwrap() = feld.clone();
        match Aufnahme::starten(ablage::neue_aufnahme()) {
            Ok(a) => {
                *self.aufnahme.lock().unwrap() = Some((a, Instant::now()));
                *self.phase.lock().unwrap() = Phase::Hoert;
                self.laeuft.store(true, Ordering::SeqCst);
                overlay::senden(&self.app, &feld, |f| json!({ "phase": "hoert", "feld": f }));
                protokoll(&format!("Aufnahme gestartet, Feld auf {feld}"));
                let ich = self.clone();
                std::thread::spawn(move || ich.pegel_melden());
            }
            Err(e) => self.fehler(&format!("Mikrofon: {e}")),
        }
    }

    fn pegel_melden(&self) {
        while *self.phase.lock().unwrap() == Phase::Hoert {
            if let Some((a, _)) = self.aufnahme.lock().unwrap().as_ref() {
                overlay::pegel(&self.app, a.pegel());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn beenden(self: &Arc<Self>) {
        let Some((aufnahme, seit)) = self.aufnahme.lock().unwrap().take() else { return };
        *self.phase.lock().unwrap() = Phase::Erkennt;
        self.laeuft.store(false, Ordering::SeqCst);
        let feld = self.feld.lock().unwrap().clone();
        if seit.elapsed() < MINDESTDAUER {
            let pfad = aufnahme.pfad.clone();
            let _ = aufnahme.stoppen();
            let _ = ablage::text_speichern(&pfad, ""); // versehentlich, nicht wiederherstellen
            self.zurueck();
            overlay::senden(&self.app, &feld, |_| json!({ "phase": "aus" }));
            return;
        }
        overlay::senden(&self.app, &feld, |f| json!({ "phase": "erkennt", "feld": f, "anteil": 0.0 }));
        let ich = self.clone();
        std::thread::spawn(move || {
            let pfad = aufnahme.pfad.clone();
            match aufnahme.stoppen() {
                Ok(ton) => ich.erkennen(ton, &pfad, feld),
                Err(e) => ich.fehler(&format!("Aufnahme: {e} — der Ton liegt in {}", pfad.display())),
            }
        });
    }

    /// Eine liegengebliebene Aufnahme (Absturz, Abbruch) nachträglich erkennen.
    pub fn wiederherstellen(self: &Arc<Self>, pfad: std::path::PathBuf) -> Result<(), String> {
        {
            let mut phase = self.phase.lock().unwrap();
            if *phase != Phase::Bereit {
                return Err("Gerade läuft ein Diktat".into());
            }
            *phase = Phase::Erkennt;
        }
        let ton = ablage::wav_lesen(&pfad).map_err(|e| e.to_string())?;
        overlay::sicherstellen(&self.app);
        let feld = overlay::feld_fenster(&self.app);
        *self.feld.lock().unwrap() = feld.clone();
        overlay::senden(&self.app, &feld, |f| json!({ "phase": "erkennt", "feld": f, "anteil": 0.0 }));
        protokoll(&format!("Wiederherstellen: {}", pfad.display()));
        let ich = self.clone();
        std::thread::spawn(move || ich.erkennen(ton, &pfad, feld));
        Ok(())
    }

    fn erkennen(self: &Arc<Self>, ton: Vec<f32>, pfad: &std::path::Path, feld: String) {
        let sekunden = ton.len() as f32 / aufnahme::RATE as f32;
        if aufnahme::lautheit(&ton) < STILLE {
            let _ = ablage::text_speichern(pfad, "");
            return self.fehler("Nichts gehört — Mikrofon stumm oder falsch gewählt?");
        }
        if !self.erkenner.bereit() {
            if !modell::vorhanden() {
                crate::einstellungen_zeigen(&self.app);
                return self.fehler("Sprachmodell fehlt — in den Einstellungen laden");
            }
            if let Err(e) = self.erkenner.laden(&modell::pfad()) {
                return self.fehler(&e);
            }
        }
        let werte = self.einstellungen.lock().unwrap().clone();
        let app = self.app.clone();
        let feld_fortschritt = feld.clone();
        let beginn = Instant::now();
        // Whisper bis 90 % des Balkens, das Umformen den Rest
        let anteil_whisper = if werte.modus.anweisung().is_some() { 0.9 } else { 1.0 };
        let ergebnis = self.erkenner.erkennen(&ton, &werte.sprache, &einstellungen::vorlage(&werte.sprache), move |prozent| {
            let anteil = prozent as f32 / 100.0 * anteil_whisper;
            overlay::senden(&app, &feld_fortschritt, |f| json!({ "phase": "erkennt", "feld": f, "anteil": anteil }));
        });
        match ergebnis {
            Ok(roh) if roh.is_empty() => {
                let _ = ablage::text_speichern(pfad, "");
                self.fehler("Nichts erkannt")
            }
            Ok(roh) => {
                protokoll(&format!("Erkannt: {sekunden:.1} s Ton in {:.1} s", beginn.elapsed().as_secs_f32()));
                let _ = ablage::roh_speichern(pfad, &roh);
                *self.letzte.lock().unwrap() = Some((pfad.to_path_buf(), roh.clone()));
                self.abliefern(pfad, &roh, werte.modus, werte.einfuegen, &feld);
            }
            Err(e) => self.fehler(&e),
        }
    }

    /// Rohtext im Modus umformen, ablegen, in die Zwischenablage, Fertig zeigen.
    fn abliefern(&self, pfad: &std::path::Path, roh: &str, modus: Modus, einfuegen: bool, feld: &str) {
        let text = match modus.anweisung() {
            None => roh.to_string(),
            Some(anweisung) => {
                overlay::senden(&self.app, feld, |f| json!({ "phase": "erkennt", "feld": f, "anteil": 0.92 }));
                let beginn = Instant::now();
                match helfer::umformen(&anweisung, roh) {
                    Ok(t) if !t.is_empty() => {
                        protokoll(&format!("{} in {:.1} s", modus.name(), beginn.elapsed().as_secs_f32()));
                        modus::nachputzen(&t)
                    }
                    Ok(_) => roh.to_string(),
                    Err(e) => {
                        // Lieber den Rohtext als nichts
                        protokoll(&format!("{}: {e} — Rohtext geliefert", modus.name()));
                        roh.to_string()
                    }
                }
            }
        };
        if let Err(e) = ablage::text_speichern(pfad, &text) {
            protokoll(&format!("Text nicht gespeichert: {e}"));
        }
        // Nur für Prüfläufe: FUNKEL_PROBE=/pfad schreibt den Text zusätzlich dorthin
        if let Some(probe) = std::env::var_os("FUNKEL_PROBE") {
            let _ = std::fs::write(probe, &text);
        }
        if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
            return self.fehler(&format!("Zwischenablage: {e}"));
        }
        if einfuegen {
            std::thread::sleep(Duration::from_millis(80));
            plattform::einfuegen();
        }
        let namen: Vec<&str> = modus::ALLE.iter().map(|m| m.name()).collect();
        let aktiv = modus::ALLE.iter().position(|m| *m == modus).unwrap_or(0);
        overlay::senden(&self.app, feld, |f| json!({ "phase": "fertig", "feld": f, "modi": namen, "aktiv": aktiv }));
        ausloeser::wahl_oeffnen(WAHL);
        self.zurueck();
    }

    /// Schnellwahl oder Menü: das letzte Diktat in einem anderen Modus.
    pub fn umwandeln(self: &Arc<Self>, modus: Modus) {
        let Some((pfad, roh)) = self.letzte.lock().unwrap().clone() else { return };
        {
            let mut phase = self.phase.lock().unwrap();
            if *phase != Phase::Bereit {
                return;
            }
            *phase = Phase::Erkennt;
        }
        let feld = self.feld.lock().unwrap().clone();
        overlay::senden(&self.app, &feld, |f| json!({ "phase": "erkennt", "feld": f, "anteil": 0.5 }));
        // Schon eingefügter Text würde doppelt stehen: umgewandelt nur in die Zwischenablage
        self.abliefern(&pfad, &roh, modus, false, &feld);
    }

    /// Die gerade laufende Aufnahme — sie ist kein Absturz.
    pub fn laufende(&self) -> Option<std::path::PathBuf> {
        self.aufnahme.lock().unwrap().as_ref().map(|(a, _)| a.pfad.clone())
    }

    fn zurueck(&self) {
        *self.phase.lock().unwrap() = Phase::Bereit;
        self.laeuft.store(false, Ordering::SeqCst);
    }

    fn fehler(&self, text: &str) {
        protokoll(&format!("Fehler: {text}"));
        if let Some((a, _)) = self.aufnahme.lock().unwrap().take() {
            let _ = a.stoppen();
        }
        self.zurueck();
        let feld = self.feld.lock().unwrap().clone();
        overlay::senden(&self.app, &feld, |f| json!({ "phase": "fehler", "feld": f, "text": text }));
        let _ = self.app.emit("status-geaendert", ());
    }
}
