//! Doppeltipp auf eine Taste — beim Beenden reicht ein Tipp.
//!
//! Übernommen aus dem Vorgänger (ein internes Diktierwerkzeug), wo
//! jede Regel an einem echten Fehler gelernt wurde:
//! * Der erste Tipp wird zurückgehalten und nachgeschickt, wenn kein zweiter
//!   kommt. Durchlassen geht nicht: `^` ist auf deutschen Tastaturen eine tote
//!   Taste, eine Rücktaste träfe das Zeichen davor.
//! * Läuft eine Aufnahme, beendet ein einzelner Tipp sie, ohne Zeitfenster.
//! * Nach Start und Stopp werden Tipps 1,6 s lang verschluckt: wer zum Beenden
//!   aus Gewohnheit zweimal tippt, bekommt sonst ein `^` in den Text.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::plattform;

pub const TOTZEIT: Duration = Duration::from_millis(1600);

#[derive(Debug, PartialEq)]
pub enum Entscheid {
    /// Verschlucken, nichts auslösen
    Schlucken,
    /// Verschlucken und Aufnahme starten oder beenden
    Ausloesen,
    /// Verschlucken; kommt binnen Zeitfenster kein zweiter Tipp, Taste mit dieser Nummer nachschicken
    Zurueckhalten(u64),
}

/// Reine Logik, ohne Betriebssystem — damit testbar.
pub struct Doppeltipp {
    pub fenster: Duration,
    letzter: Option<Instant>,
    tot_bis: Option<Instant>,
    nummer: u64,
}

impl Doppeltipp {
    pub fn neu(fenster: Duration) -> Self {
        Self { fenster, letzter: None, tot_bis: None, nummer: 0 }
    }

    pub fn tipp(&mut self, jetzt: Instant, laeuft: bool) -> Entscheid {
        if self.tot_bis.is_some_and(|t| jetzt < t) {
            return Entscheid::Schlucken;
        }
        self.nummer += 1;
        let zweiter = self.letzter.is_some_and(|l| jetzt.duration_since(l) <= self.fenster);
        if laeuft || zweiter {
            self.letzter = None;
            self.tot_bis = Some(jetzt + TOTZEIT);
            return Entscheid::Ausloesen;
        }
        self.letzter = Some(jetzt);
        Entscheid::Zurueckhalten(self.nummer)
    }

    /// Ist der zurückgehaltene Tipp `nummer` noch der letzte? Dann nachschicken.
    pub fn nachschicken(&mut self, nummer: u64) -> bool {
        if nummer == self.nummer && self.letzter.is_some() {
            self.letzter = None;
            true
        } else {
            false
        }
    }
}

struct Abgriff {
    logik: Doppeltipp,
    taste: u32,
    lernen: bool,
}

pub struct Rueckrufe {
    pub ausloesen: Box<dyn Fn() + Send + Sync>,
    /// Ziffer 1–5 während der Schnellwahl nach einem Diktat
    pub umwandeln: Box<dyn Fn(u8) + Send + Sync>,
    pub gelernt: Box<dyn Fn(u32) + Send + Sync>,
    pub laeuft: Arc<AtomicBool>,
}

static ABGRIFF: OnceLock<Mutex<Abgriff>> = OnceLock::new();
static RUECKRUFE: OnceLock<Rueckrufe> = OnceLock::new();
/// Bis wann Ziffern 1–5 einen Modus wählen statt getippt zu werden
static WAHL_BIS: Mutex<Option<Instant>> = Mutex::new(None);

pub fn wahl_oeffnen(dauer: Duration) {
    *WAHL_BIS.lock().unwrap() = Some(Instant::now() + dauer);
}

pub fn wahl_schliessen() {
    *WAHL_BIS.lock().unwrap() = None;
}

/// Hängt den Abgriff an. `false` = keine Freigabe (Mac: Bedienungshilfen).
pub fn starten(taste: u32, fenster_ms: u64, rueckrufe: Rueckrufe) -> bool {
    let _ = ABGRIFF.set(Mutex::new(Abgriff {
        logik: Doppeltipp::neu(Duration::from_millis(fenster_ms.max(250))),
        taste,
        lernen: false,
    }));
    let _ = RUECKRUFE.set(rueckrufe);
    plattform::abgriff_starten(taste_gedrueckt)
}

/// Erneut anhängen, z. B. nachdem die Freigabe erst nach dem Start erteilt wurde.
pub fn neu_anhaengen() -> bool {
    ABGRIFF.get().is_some() && plattform::abgriff_starten(taste_gedrueckt)
}

pub fn taste_setzen(taste: u32) {
    if let Some(a) = ABGRIFF.get() {
        a.lock().unwrap().taste = taste;
    }
}

pub fn lernen() {
    if let Some(a) = ABGRIFF.get() {
        a.lock().unwrap().lernen = true;
    }
}

/// Rückruf aus dem Abgriff des Betriebssystems. Muss schnell sein: das System
/// wartet mit jedem Anschlag darauf. Rückgabe `true` = Anschlag verschlucken.
fn taste_gedrueckt(code: u32, mit_zusatztaste: bool, gehalten: bool) -> bool {
    let (Some(abgriff), Some(rueck)) = (ABGRIFF.get(), RUECKRUFE.get()) else { return false };
    // Schnellwahl: nur unmittelbar nach einem Diktat, jede andere Taste beendet sie
    {
        let mut wahl = WAHL_BIS.lock().unwrap();
        if wahl.is_some_and(|bis| Instant::now() < bis) && !gehalten {
            *wahl = None;
            if let (false, Some(n)) = (mit_zusatztaste, plattform::ziffer(code)) {
                std::thread::spawn(move || (RUECKRUFE.get().unwrap().umwandeln)(n));
                return true;
            }
        } else {
            *wahl = None;
        }
    }
    let mut a = abgriff.lock().unwrap();
    if a.lernen {
        if mit_zusatztaste {
            return false;
        }
        a.lernen = false;
        a.taste = code;
        drop(a);
        (rueck.gelernt)(code);
        return true;
    }
    if code != a.taste || mit_zusatztaste || gehalten {
        return false;
    }
    let entscheid = a.logik.tipp(Instant::now(), rueck.laeuft.load(Ordering::SeqCst));
    let fenster = a.logik.fenster;
    let taste = a.taste;
    drop(a);
    match entscheid {
        Entscheid::Schlucken => true,
        Entscheid::Ausloesen => {
            std::thread::spawn(|| (RUECKRUFE.get().unwrap().ausloesen)());
            true
        }
        Entscheid::Zurueckhalten(nummer) => {
            std::thread::spawn(move || {
                std::thread::sleep(fenster);
                let faellig = ABGRIFF.get().unwrap().lock().unwrap().logik.nachschicken(nummer);
                if faellig {
                    plattform::taste_senden(taste);
                }
            });
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn zwei_tipps_im_fenster_loesen_aus() {
        let mut d = Doppeltipp::neu(ms(550));
        let t = Instant::now();
        assert!(matches!(d.tipp(t, false), Entscheid::Zurueckhalten(_)));
        assert_eq!(d.tipp(t + ms(300), false), Entscheid::Ausloesen);
    }

    #[test]
    fn einzelner_tipp_wird_nachgeschickt() {
        let mut d = Doppeltipp::neu(ms(550));
        let Entscheid::Zurueckhalten(n) = d.tipp(Instant::now(), false) else { panic!() };
        assert!(d.nachschicken(n));
        assert!(!d.nachschicken(n), "nur einmal");
    }

    #[test]
    fn zu_langsam_ist_kein_doppeltipp() {
        let mut d = Doppeltipp::neu(ms(550));
        let t = Instant::now();
        d.tipp(t, false);
        assert!(matches!(d.tipp(t + ms(900), false), Entscheid::Zurueckhalten(_)));
    }

    #[test]
    fn waehrend_der_aufnahme_beendet_ein_tipp() {
        let mut d = Doppeltipp::neu(ms(550));
        assert_eq!(d.tipp(Instant::now(), true), Entscheid::Ausloesen);
    }

    #[test]
    fn nach_dem_ausloesen_wird_verschluckt() {
        let mut d = Doppeltipp::neu(ms(550));
        let t = Instant::now();
        d.tipp(t, true);
        assert_eq!(d.tipp(t + ms(800), true), Entscheid::Schlucken);
        assert_eq!(d.tipp(t + ms(1700), true), Entscheid::Ausloesen);
    }

    #[test]
    fn ausgeloester_doppeltipp_schickt_nichts_nach() {
        let mut d = Doppeltipp::neu(ms(550));
        let t = Instant::now();
        let Entscheid::Zurueckhalten(n) = d.tipp(t, false) else { panic!() };
        d.tipp(t + ms(200), false);
        assert!(!d.nachschicken(n));
    }
}
