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
use crate::{modell, overlay, plattform};

/// Kürzer ist ein versehentlicher Doppeltipp, kein Diktat
const MINDESTDAUER: Duration = Duration::from_millis(400);
/// Leiser als das ist Stille (Effektivwert)
const STILLE: f32 = 0.002;

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
        overlay::sicherstellen(&self.app);
        let feld = overlay::feld_fenster(&self.app);
        *self.feld.lock().unwrap() = feld.clone();
        match Aufnahme::starten() {
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
            aufnahme.stoppen();
            self.zurueck();
            overlay::senden(&self.app, &feld, |_| json!({ "phase": "aus" }));
            return;
        }
        overlay::senden(&self.app, &feld, |f| json!({ "phase": "erkennt", "feld": f, "anteil": 0.0 }));
        let ich = self.clone();
        std::thread::spawn(move || ich.erkennen(aufnahme, feld));
    }

    fn erkennen(self: &Arc<Self>, aufnahme: Aufnahme, feld: String) {
        let ton = aufnahme.stoppen();
        let sekunden = ton.len() as f32 / aufnahme::RATE as f32;
        if aufnahme::lautheit(&ton) < STILLE {
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
        let ergebnis = self.erkenner.erkennen(&ton, &werte.sprache, &einstellungen::vorlage(), move |prozent| {
            let anteil = prozent as f32 / 100.0;
            overlay::senden(&app, &feld_fortschritt, |f| json!({ "phase": "erkennt", "feld": f, "anteil": anteil }));
        });
        match ergebnis {
            Ok(text) if text.is_empty() => self.fehler("Nichts erkannt"),
            Ok(text) => {
                protokoll(&format!("Erkannt: {sekunden:.1} s Ton in {:.1} s", beginn.elapsed().as_secs_f32()));
                // Nur für Prüfläufe: FUNKEL_PROBE=/pfad schreibt den Text zusätzlich dorthin
                if let Some(probe) = std::env::var_os("FUNKEL_PROBE") {
                    let _ = std::fs::write(probe, &text);
                }
                if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
                    return self.fehler(&format!("Zwischenablage: {e}"));
                }
                if werte.einfuegen {
                    std::thread::sleep(Duration::from_millis(80));
                    plattform::einfuegen();
                }
                overlay::senden(&self.app, &feld, |f| json!({ "phase": "fertig", "feld": f }));
                self.zurueck();
            }
            Err(e) => self.fehler(&e),
        }
    }

    fn zurueck(&self) {
        *self.phase.lock().unwrap() = Phase::Bereit;
        self.laeuft.store(false, Ordering::SeqCst);
    }

    fn fehler(&self, text: &str) {
        protokoll(&format!("Fehler: {text}"));
        if let Some((a, _)) = self.aufnahme.lock().unwrap().take() {
            a.stoppen();
        }
        self.zurueck();
        let feld = self.feld.lock().unwrap().clone();
        overlay::senden(&self.app, &feld, |f| json!({ "phase": "fehler", "feld": f, "text": text }));
        let _ = self.app.emit("status-geaendert", ());
    }
}
