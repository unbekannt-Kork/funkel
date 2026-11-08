//! Mikrofon aufnehmen, solange diktiert wird. Der Ton bleibt im Speicher und
//! wird nach der Erkennung verworfen — nichts landet auf der Platte.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const RATE: u32 = 16_000;

/// Virtuelle Geräte nehmen nie das Mikrofon auf, sondern den Tonausgang.
/// Im Vorgänger schrieb das Diktat so einmal ein Kursvideo mit statt der Stimme.
const VIRTUELL: [&str; 7] = ["blackhole", "loopback", "soundflower", "vb-audio", "cable output", "zoomaudio", "teams audio"];

pub struct Aufnahme {
    stopp: mpsc::Sender<()>,
    ergebnis: mpsc::Receiver<Vec<f32>>,
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
    pub fn starten() -> Result<Self, String> {
        let (stopp_tx, stopp_rx) = mpsc::channel::<()>();
        let (ergebnis_tx, ergebnis_rx) = mpsc::channel();
        let (bereit_tx, bereit_rx) = mpsc::channel::<Result<(), String>>();
        let pegel = Arc::new(AtomicU32::new(0));
        let pegel_faden = pegel.clone();

        // Der Strom lebt auf eigenem Faden: er ist nicht auf jedem System übertragbar
        std::thread::spawn(move || {
            let gesammelt = Arc::new(Mutex::new(Vec::<f32>::new()));
            let start = || -> Result<(cpal::Stream, u32), String> {
                let geraet = geraet_waehlen()?;
                let konfig = geraet.default_input_config().map_err(|e| e.to_string())?;
                let kanaele = konfig.channels() as usize;
                let rate = konfig.sample_rate().0;
                let fehler = |e| eprintln!("Aufnahme: {e}");
                let ziel = gesammelt.clone();
                let pegel = pegel_faden.clone();
                let annehmen = move |mono: &mut dyn Iterator<Item = f32>| {
                    let mut ziel = ziel.lock().unwrap();
                    let vorher = ziel.len();
                    ziel.extend(mono);
                    let neu = &ziel[vorher..];
                    if !neu.is_empty() {
                        let rms = (neu.iter().map(|x| x * x).sum::<f32>() / neu.len() as f32).sqrt();
                        // grob: Raum leiser als −50 dB, Sprache nah am Mikrofon um −30…−15 dB
                        let db = 20.0 * rms.max(1e-6).log10();
                        let wert = ((db + 50.0) / 32.0).clamp(0.0, 1.0);
                        pegel.store(wert.to_bits(), Ordering::Relaxed);
                    }
                };
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
            match start() {
                Ok((strom, rate)) => {
                    let _ = bereit_tx.send(Ok(()));
                    let _ = stopp_rx.recv();
                    drop(strom);
                    let roh = std::mem::take(&mut *gesammelt.lock().unwrap());
                    let _ = ergebnis_tx.send(umrechnen(&roh, rate, RATE));
                }
                Err(e) => {
                    let _ = bereit_tx.send(Err(e));
                }
            }
        });

        bereit_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "Mikrofon antwortet nicht".to_string())??;
        Ok(Self { stopp: stopp_tx, ergebnis: ergebnis_rx, pegel })
    }

    pub fn pegel(&self) -> f32 {
        f32::from_bits(self.pegel.load(Ordering::Relaxed))
    }

    /// Beendet die Aufnahme und liefert Mono-Ton mit 16 kHz.
    pub fn stoppen(self) -> Vec<f32> {
        let _ = self.stopp.send(());
        self.ergebnis.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or_default()
    }
}

/// Lineare Umrechnung auf die Abtastrate, die Whisper erwartet.
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
