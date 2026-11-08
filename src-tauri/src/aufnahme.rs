//! Mikrofon aufnehmen, solange diktiert wird — ohne Zeitgrenze. Der Ton geht
//! laufend auf 16 kHz umgerechnet in eine WAV-Datei (`ablage`), im Speicher
//! liegt nur, was seit dem letzten Schreiben kam. So übersteht eine Aufnahme
//! auch einen Absturz, und eine Stunde braucht nicht eine Stunde Arbeitsspeicher.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const RATE: u32 = 16_000;

/// Virtuelle Geräte nehmen nie das Mikrofon auf, sondern den Tonausgang.
/// Im Vorgänger schrieb das Diktat so einmal ein Kursvideo mit statt der Stimme.
const VIRTUELL: [&str; 7] = ["blackhole", "loopback", "soundflower", "vb-audio", "cable output", "zoomaudio", "teams audio"];

/// So oft geht der Puffer auf die Platte
const SCHREIBTAKT: std::time::Duration = std::time::Duration::from_millis(500);

/// Rechnet einen laufenden Strom auf 16 kHz um, Stück für Stück (linear).
pub struct Umrechner {
    schritt: f64,
    pos: f64,
    letzter: f32,
}

impl Umrechner {
    pub fn neu(von: u32) -> Self {
        Self { schritt: von as f64 / RATE as f64, pos: 0.0, letzter: 0.0 }
    }

    pub fn weiter(&mut self, eingang: &[f32], ausgang: &mut Vec<f32>) {
        for &x in eingang {
            while self.pos <= 1.0 {
                ausgang.push(self.letzter + (x - self.letzter) * self.pos as f32);
                self.pos += self.schritt;
            }
            self.pos -= 1.0;
            self.letzter = x;
        }
    }
}

pub struct Aufnahme {
    stopp: mpsc::Sender<()>,
    ergebnis: mpsc::Receiver<Result<(), String>>,
    pub pfad: std::path::PathBuf,
    /// Lautstärke 0…1, für die Farbe des Rands
    pub pegel: Arc<AtomicU32>,
}

fn virtuell(name: &str) -> bool {
    let n = name.to_lowercase();
    VIRTUELL.iter().any(|v| n.contains(v))
}

fn geraet_waehlen() -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if let Some(d) = host.default_input_device() {
        if !d.name().map(|n| virtuell(&n)).unwrap_or(false) {
            return Ok(d);
        }
    }
    host.input_devices()
        .map_err(|e| e.to_string())?
        .find(|d| d.name().map(|n| !virtuell(&n)).unwrap_or(false))
        .ok_or_else(|| "Kein Mikrofon gefunden".into())
}

impl Aufnahme {
    pub fn starten(pfad: std::path::PathBuf) -> Result<Self, String> {
        let (stopp_tx, stopp_rx) = mpsc::channel::<()>();
        let (ergebnis_tx, ergebnis_rx) = mpsc::channel();
        let (bereit_tx, bereit_rx) = mpsc::channel::<Result<(), String>>();
        let pegel = Arc::new(AtomicU32::new(0));
        let pegel_faden = pegel.clone();
        let datei_pfad = pfad.clone();

        // Der Strom lebt auf eigenem Faden: er ist nicht auf jedem System übertragbar
        std::thread::spawn(move || {
            // Nur 16-kHz-Ton seit dem letzten Schreiben
            let gesammelt = Arc::new(Mutex::new(Vec::<f32>::new()));
            let start = || -> Result<(cpal::Stream, u32), String> {
                let geraet = geraet_waehlen()?;
                let konfig = geraet.default_input_config().map_err(|e| e.to_string())?;
                let kanaele = konfig.channels() as usize;
                let rate = konfig.sample_rate().0;
                let fehler = |e| eprintln!("Aufnahme: {e}");
                let ziel = gesammelt.clone();
                let pegel = pegel_faden.clone();
                let mut umrechner = Umrechner::neu(rate);
                let mut mono_puffer = Vec::<f32>::new();
                let annehmen = move |mono: &mut dyn Iterator<Item = f32>| {
                    mono_puffer.clear();
                    mono_puffer.extend(mono);
                    let neu = &mono_puffer;
                    umrechner.weiter(neu, &mut ziel.lock().unwrap());
                    if !neu.is_empty() {
                        let rms = (neu.iter().map(|x| x * x).sum::<f32>() / neu.len() as f32).sqrt();
                        // grob: Raum leiser als −50 dB, Sprache nah am Mikrofon um −30…−15 dB
                        let db = 20.0 * rms.max(1e-6).log10();
                        let wert = ((db + 50.0) / 32.0).clamp(0.0, 1.0);
                        pegel.store(wert.to_bits(), Ordering::Relaxed);
                    }
                };
                let mut annehmen = annehmen;
                let strom = match konfig.sample_format() {
                    cpal::SampleFormat::F32 => geraet.build_input_stream(
                        &konfig.into(),
                        move |d: &[f32], _| annehmen(&mut d.chunks(kanaele).map(|k| k.iter().sum::<f32>() / k.len() as f32)),
                        fehler,
                        None,
                    ),
                    cpal::SampleFormat::I16 => geraet.build_input_stream(
                        &konfig.into(),
                        move |d: &[i16], _| {
                            annehmen(&mut d.chunks(kanaele).map(|k| k.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / k.len() as f32))
                        },
                        fehler,
                        None,
                    ),
                    cpal::SampleFormat::U16 => geraet.build_input_stream(
                        &konfig.into(),
                        move |d: &[u16], _| {
                            annehmen(&mut d.chunks(kanaele).map(|k| {
                                k.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).sum::<f32>() / k.len() as f32
                            }))
                        },
                        fehler,
                        None,
                    ),
                    andere => return Err(format!("Tonformat {andere:?} nicht unterstützt")),
                }
                .map_err(|e| e.to_string())?;
                strom.play().map_err(|e| e.to_string())?;
                Ok((strom, rate))
            };
            let mut datei = match crate::ablage::wav_anlegen(&datei_pfad) {
                Ok(f) => f,
                Err(e) => {
                    let _ = bereit_tx.send(Err(format!("Aufnahme lässt sich nicht speichern: {e}")));
                    return;
                }
            };
            match start() {
                Ok((strom, _rate)) => {
                    let _ = bereit_tx.send(Ok(()));
                    let mut fehler = None;
                    // Bis zum Stopp alle halbe Sekunde auf die Platte
                    while let Err(mpsc::RecvTimeoutError::Timeout) = stopp_rx.recv_timeout(SCHREIBTAKT) {
                        let stueck = std::mem::take(&mut *gesammelt.lock().unwrap());
                        if let Err(e) = crate::ablage::wav_anhaengen(&mut datei, &stueck) {
                            fehler.get_or_insert(e.to_string());
                        }
                    }
                    drop(strom);
                    let rest = std::mem::take(&mut *gesammelt.lock().unwrap());
                    let ergebnis = crate::ablage::wav_anhaengen(&mut datei, &rest)
                        .and_then(|_| crate::ablage::wav_abschliessen(&mut datei))
                        .map_err(|e| e.to_string());
                    let _ = ergebnis_tx.send(match fehler {
                        Some(f) => Err(f),
                        None => ergebnis,
                    });
                }
                Err(e) => {
                    let _ = bereit_tx.send(Err(e));
                }
            }
        });

        bereit_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "Mikrofon antwortet nicht".to_string())??;
        Ok(Self { stopp: stopp_tx, ergebnis: ergebnis_rx, pfad, pegel })
    }

    pub fn pegel(&self) -> f32 {
        f32::from_bits(self.pegel.load(Ordering::Relaxed))
    }

    /// Beendet die Aufnahme, schließt die Datei ab und liefert Mono-Ton mit 16 kHz.
    pub fn stoppen(self) -> Result<Vec<f32>, String> {
        let _ = self.stopp.send(());
        self.ergebnis
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| "Aufnahme reagiert nicht".to_string())??;
        crate::ablage::wav_lesen(&self.pfad).map_err(|e| e.to_string())
    }
}

/// Lineare Umrechnung am Stück — Vergleich für den Test des laufenden Umrechners.
#[cfg(test)]
pub fn umrechnen(ton: &[f32], von: u32, nach: u32) -> Vec<f32> {
    if von == nach || ton.is_empty() {
        return ton.to_vec();
    }
    let schritt = von as f64 / nach as f64;
    let laenge = (ton.len() as f64 / schritt) as usize;
    (0..laenge)
        .map(|i| {
            let pos = i as f64 * schritt;
            let a = pos.floor() as usize;
            let b = (a + 1).min(ton.len() - 1);
            let t = (pos - a as f64) as f32;
            ton[a] * (1.0 - t) + ton[b] * t
        })
        .collect()
}

/// Effektivwert des ganzen Tons — fast null heißt: nichts gehört.
pub fn lautheit(ton: &[f32]) -> f32 {
    if ton.is_empty() {
        return 0.0;
    }
    (ton.iter().map(|x| x * x).sum::<f32>() / ton.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umrechnen_halbiert_bei_doppelter_rate() {
        let ton: Vec<f32> = (0..48_000).map(|i| (i as f32 / 10.0).sin()).collect();
        assert_eq!(umrechnen(&ton, 48_000, 24_000).len(), 24_000);
        assert_eq!(umrechnen(&ton, 48_000, 16_000).len(), 16_000);
    }

    #[test]
    fn laufende_umrechnung_entspricht_der_ganzen() {
        let ton: Vec<f32> = (0..48_000).map(|i| (i as f32 / 30.0).sin()).collect();
        let mut u = Umrechner::neu(48_000);
        let mut aus = Vec::new();
        for stueck in ton.chunks(480) {
            u.weiter(stueck, &mut aus);
        }
        assert!((aus.len() as i64 - 16_000).abs() <= 1, "{}", aus.len());
        let ganz = umrechnen(&ton, 48_000, 16_000);
        assert!((aus[5000] - ganz[4999]).abs() < 0.05 || (aus[5000] - ganz[5000]).abs() < 0.05);
    }

    #[test]
    fn virtuelle_geraete_werden_erkannt() {
        assert!(virtuell("BlackHole 2ch"));
        assert!(virtuell("CABLE Output (VB-Audio Virtual Cable)"));
        assert!(!virtuell("MacBook Pro-Mikrofon"));
    }

    #[test]
    fn stille_ist_leise() {
        assert_eq!(lautheit(&[0.0; 100]), 0.0);
        assert!(lautheit(&[0.5; 100]) > 0.4);
    }
}
