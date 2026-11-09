//! Erkennt schon während der Aufnahme: Stücke von gut 20 s, geschnitten an der
//! leisesten Stelle, damit kein Wort zerteilt wird. Nach dem Stopp bleibt nur der
//! Rest — bei langen Aufnahmen der Unterschied zwischen Sekunden und Minuten.
//! Werte aus dem Vorgänger, dort am 04.11.2026 gemessen: 20 s je Stück war
//! schneller und genauer als 15 s und 30 s.
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::aufnahme::RATE;
use crate::erkennung::{Abschnitt, Erkenner};

const STUECK_S: usize = 20;
const SUCHFENSTER_S: usize = 8;
const TAKT: Duration = Duration::from_millis(1000);

#[derive(Default)]
pub struct Vorab {
    pub abschnitte: Vec<Abschnitt>,
    /// Bis zu diesem Abtastwert ist erkannt
    pub fertig_bis: usize,
    /// Bei „automatisch“: schon während der Aufnahme erkannte Sprache
    pub sprache: Option<String>,
}

/// Ab so viel Ton wird bei „automatisch“ die Sprache bestimmt; unsicher → später
/// noch einmal mit mehr Ton, höchstens so oft
const SPRACHE_AB_S: usize = 2;
const SPRACHE_VERSUCHE: usize = 3;

pub struct Mitlauf {
    stopp: Arc<AtomicBool>,
    faden: JoinHandle<Vorab>,
}

/// Abtastwerte ab Index `ab` aus der wachsenden WAV (Kopf 44 Byte)
fn lesen_ab(pfad: &PathBuf, ab: usize) -> Vec<f32> {
    let Ok(mut f) = fs::File::open(pfad) else { return vec![] };
    if f.seek(SeekFrom::Start(44 + 2 * ab as u64)).is_err() {
        return vec![];
    }
    let mut roh = Vec::new();
    let _ = f.read_to_end(&mut roh);
    let (paare, _) = roh.as_chunks::<2>();
    paare.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect()
}

/// Index der leisesten Zehntelsekunde im Bereich `von..bis`
pub fn leiseste_stelle(ton: &[f32], von: usize, bis: usize) -> usize {
    let fenster = RATE as usize / 10;
    (von..bis.saturating_sub(fenster))
        .step_by(fenster / 2)
        .min_by(|&a, &b| {
            let e = |s: usize| ton[s..s + fenster].iter().map(|x| x * x).sum::<f32>();
            e(a).total_cmp(&e(b))
        })
        .map(|s| s + fenster / 2)
        .unwrap_or(bis)
}

impl Mitlauf {
    pub fn starten(pfad: PathBuf, erkenner: Arc<Erkenner>, sprache: String, vorlage: String) -> Self {
        let stopp = Arc::new(AtomicBool::new(false));
        let halt = stopp.clone();
        let faden = std::thread::spawn(move || {
            let mut vorab = Vorab::default();
            let mut versuche = 0;
            let noetig = (STUECK_S + SUCHFENSTER_S) * RATE as usize;
            while !halt.load(Ordering::SeqCst) {
                std::thread::sleep(TAKT);
                if halt.load(Ordering::SeqCst) || !erkenner.bereit() {
                    continue;
                }
                if sprache == "auto" && vorab.sprache.is_none() && versuche < SPRACHE_VERSUCHE {
                    let bisher = lesen_ab(&pfad, 0);
                    if bisher.len() >= SPRACHE_AB_S * (versuche + 1) * RATE as usize {
                        versuche += 1;
                        vorab.sprache = erkenner.sprache_erkennen(&bisher);
                    }
                }
                let sprache = vorab.sprache.as_deref().unwrap_or(&sprache);
                let neu = lesen_ab(&pfad, vorab.fertig_bis);
                if neu.len() < noetig {
                    continue;
                }
                let schnitt = leiseste_stelle(&neu, STUECK_S * RATE as usize, noetig);
                let versatz = vorab.fertig_bis as f32 / RATE as f32;
                match erkenner.erkennen(&neu[..schnitt], sprache, &vorlage) {
                    Ok((_, abschnitte)) => {
                        vorab.abschnitte.extend(abschnitte.into_iter().map(|a| Abschnitt { von: a.von + versatz, bis: a.bis + versatz, text: a.text }));
                        vorab.fertig_bis += schnitt;
                    }
                    // Ein Fehler hier kostet nur Zeit: nach dem Stopp wird der Rest ab `fertig_bis` erkannt
                    Err(e) => crate::einstellungen::protokoll(&format!("Mitlauf: {e}")),
                }
            }
            vorab
        });
        Self { stopp, faden }
    }

    /// Hält an; wartet, bis ein laufendes Stück fertig ist.
    pub fn stoppen(self) -> Vorab {
        self.stopp.store(true, Ordering::SeqCst);
        self.faden.join().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schnitt_liegt_in_der_pause() {
        let r = RATE as usize;
        // 10 s laut, 0,3 s still bei 12,0 s, dann wieder laut
        let mut ton: Vec<f32> = (0..20 * r).map(|i| (i as f32 / 7.0).sin() * 0.5).collect();
        for x in &mut ton[12 * r..12 * r + 3 * r / 10] {
            *x = 0.0;
        }
        let s = leiseste_stelle(&ton, 10 * r, 18 * r);
        assert!(s >= 12 * r && s <= 12 * r + 3 * r / 10, "Schnitt bei {:.2} s", s as f32 / r as f32);
    }

    /// Lange Aufnahme simulieren: `FUNKEL_LANG=datei.wav cargo test --release -- --ignored mitlauf --nocapture`
    #[test]
    #[ignore]
    fn mitlauf_spart_zeit() {
        let quelle = std::env::var("FUNKEL_LANG").expect("FUNKEL_LANG setzen");
        let ton = crate::ablage::wav_lesen(std::path::Path::new(&quelle)).unwrap();
        let e = Arc::new(Erkenner::neu());
        e.laden(&crate::modell::pfad()).unwrap();
        let vorlage = crate::einstellungen::vorlage("de");
        // Ganz am Stück, zum Vergleich
        let t = std::time::Instant::now();
        let (ganz, _) = e.erkennen(&ton, "de", &vorlage).unwrap();
        let dauer_ganz = t.elapsed().as_secs_f32();
        // Wachsende Datei, etwa 4-mal so schnell wie Sprechen
        let ziel = std::env::temp_dir().join(format!("funkel-mitlauf-{}.wav", std::process::id()));
        let mut datei = crate::ablage::wav_anlegen(&ziel).unwrap();
        let m = Mitlauf::starten(ziel.clone(), e.clone(), "de".into(), vorlage.clone());
        for stueck in ton.chunks(RATE as usize) {
            crate::ablage::wav_anhaengen(&mut datei, stueck).unwrap();
            std::thread::sleep(Duration::from_millis(250));
        }
        let t = std::time::Instant::now();
        let vorab = m.stoppen();
        let rest_ab = vorab.fertig_bis.min(ton.len());
        let versatz = rest_ab as f32 / RATE as f32;
        let (_, rest) = e.erkennen(&ton[rest_ab..], "de", &vorlage).unwrap();
        let mut alle = vorab.abschnitte;
        alle.extend(rest.into_iter().map(|a| Abschnitt { von: a.von + versatz, bis: a.bis + versatz, text: a.text }));
        let dauer_rest = t.elapsed().as_secs_f32();
        let stueckweise = crate::erkennung::text_aus(&alle);
        let woerter = |t: &str| t.to_lowercase().split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).collect::<Vec<_>>();
        let (a, b) = (woerter(&ganz), woerter(&stueckweise));
        let gemeinsam = a.iter().filter(|w| b.contains(w)).count() as f32 / a.len().max(1) as f32;
        println!("{:.0} s Ton: am Stück {dauer_ganz:.1} s, nach dem Stopp nur noch {dauer_rest:.1} s ({versatz:.0} s vorab); gleiche Wörter {:.0} %", ton.len() as f32 / RATE as f32, gemeinsam * 100.0);
        assert!(versatz > 20.0, "nichts vorab erkannt");
        assert!(dauer_rest < dauer_ganz, "nicht schneller");
        assert!(gemeinsam > 0.9, "Text weicht ab:\n{ganz}\n---\n{stueckweise}");
    }
}
