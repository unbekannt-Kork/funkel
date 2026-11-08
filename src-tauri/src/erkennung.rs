//! Whisper auf dem eigenen Rechner. Das Modell bleibt geladen, damit jedes
//! Diktat sofort erkannt wird.
use std::sync::Mutex;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct Erkenner {
    kontext: Mutex<Option<WhisperContext>>,
}

/// Sätze, die Whisper bei Stille oder Rauschen erfindet (aus Untertiteln gelernt).
/// Besteht der ganze Text nur daraus, wurde nichts gesagt.
const ERFUNDEN: [&str; 8] = [
    "untertitel",
    "vielen dank fürs zuschauen",
    "vielen dank für's zuschauen",
    "thank you for watching",
    "thanks for watching",
    "amara.org",
    "copyright",
    "swr 20",
];

impl Erkenner {
    pub fn neu() -> Self {
        Self { kontext: Mutex::new(None) }
    }

    pub fn bereit(&self) -> bool {
        self.kontext.lock().unwrap().is_some()
    }

    pub fn laden(&self, pfad: &std::path::Path) -> Result<(), String> {
        let pfad = pfad.to_str().ok_or("Pfad ist kein Text")?;
        let kontext = WhisperContext::new_with_params(pfad, WhisperContextParameters::default())
            .map_err(|e| format!("Sprachmodell lässt sich nicht laden: {e}"))?;
        *self.kontext.lock().unwrap() = Some(kontext);
        Ok(())
    }

    /// Erkennt den Ton (16 kHz, mono). `fortschritt` bekommt 0…1.
    pub fn erkennen(
        &self,
        ton: &[f32],
        sprache: &str,
        vorlage: &str,
        fortschritt: impl FnMut(i32) + 'static,
    ) -> Result<String, String> {
        let wache = self.kontext.lock().unwrap();
        let kontext = wache.as_ref().ok_or("Sprachmodell fehlt")?;
        let mut zustand = kontext.create_state().map_err(|e| e.to_string())?;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let kerne = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        p.set_n_threads(kerne.min(8) as i32);
        p.set_language(Some(sprache));
        p.set_translate(false);
        p.set_no_context(true);
        p.set_suppress_blank(true);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        if !vorlage.is_empty() {
            p.set_initial_prompt(vorlage);
        }
        p.set_progress_callback_safe(fortschritt);
        zustand.full(p, ton).map_err(|e| e.to_string())?;
        let mut text = String::new();
        let anzahl = zustand.full_n_segments().map_err(|e| e.to_string())?;
        for i in 0..anzahl {
            text.push_str(&zustand.full_get_segment_text(i).map_err(|e| e.to_string())?);
        }
        Ok(aufraeumen(&text))
    }
}

/// Leerraum glätten und erfundene Sätze verwerfen.
pub fn aufraeumen(text: &str) -> String {
    let glatt = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let klein = glatt.to_lowercase();
    let ohne_zeichen: String = klein.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '.').collect();
    if ERFUNDEN.iter().any(|e| ohne_zeichen.trim().starts_with(e)) && glatt.len() < 80 {
        return String::new();
    }
    glatt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leerraum_wird_geglaettet() {
        assert_eq!(aufraeumen("  Hallo   Welt \n"), "Hallo Welt");
    }

    #[test]
    fn erfundene_untertitel_fliegen_raus() {
        assert_eq!(aufraeumen(" Untertitel im Auftrag des ZDF, 2021"), "");
        assert_eq!(aufraeumen("Vielen Dank fürs Zuschauen!"), "");
    }

    #[test]
    fn echter_text_bleibt() {
        assert_eq!(aufraeumen("Die Untertitel sind zu klein."), "Die Untertitel sind zu klein.");
    }
}

/// Mit echtem Modell und erfundenem Ton: `FUNKEL_TESTTON=datei.wav cargo test --release -- --ignored`
/// (WAV, 16 kHz, mono, 16 Bit — z. B. `say -o datei.wav --data-format=LEI16@16000 "…"`).
#[cfg(test)]
mod mit_modell {
    use super::*;

    fn wav_lesen(pfad: &str) -> Vec<f32> {
        let roh = std::fs::read(pfad).expect("Testton fehlt");
        let daten = roh.windows(4).position(|w| w == b"data").expect("kein data-Block") + 8;
        roh[daten..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
    }

    #[test]
    #[ignore]
    fn erfundener_satz_wird_erkannt() {
        let pfad = std::env::var("FUNKEL_TESTTON").expect("FUNKEL_TESTTON setzen");
        let ton = wav_lesen(&pfad);
        let e = Erkenner::neu();
        e.laden(&crate::modell::pfad()).unwrap();
        let beginn = std::time::Instant::now();
        let text = e.erkennen(&ton, "de", "", |_| {}).unwrap();
        println!("{:.1} s Ton in {:.1} s: {text}", ton.len() as f32 / 16000.0, beginn.elapsed().as_secs_f32());
        let klein = text.to_lowercase();
        assert!(klein.contains("funkel") || klein.contains("probediktat"), "{text}");
        assert!(klein.contains("zehn") || klein.contains("10"), "{text}");
    }
}
