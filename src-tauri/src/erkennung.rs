//! Whisper auf dem eigenen Rechner. Das Modell bleibt geladen, damit jedes
//! Diktat sofort erkannt wird.
use std::sync::Mutex;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Ein Stück erkannter Text mit Zeit in Sekunden ab Beginn der Aufnahme
#[derive(Clone, Debug, PartialEq)]
pub struct Abschnitt {
    pub von: f32,
    pub bis: f32,
    pub text: String,
}

pub struct Erkenner {
    kontext: Mutex<Option<WhisperContext>>,
}

/// Fügt Abschnitte zu einem Text zusammen und räumt auf.
pub fn text_aus(abschnitte: &[Abschnitt]) -> String {
    aufraeumen(&abschnitte.iter().map(|a| a.text.as_str()).collect::<Vec<_>>().join(" "))
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
    ) -> Result<(String, Vec<Abschnitt>), String> {
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
        let mut abschnitte = Vec::new();
        let anzahl = zustand.full_n_segments().map_err(|e| e.to_string())?;
        for i in 0..anzahl {
            let stueck = zustand.full_get_segment_text(i).map_err(|e| e.to_string())?;
            // Zeiten in Hundertstelsekunden
            let von = zustand.full_get_segment_t0(i).unwrap_or(0) as f32 / 100.0;
            let bis = zustand.full_get_segment_t1(i).unwrap_or(0) as f32 / 100.0;
            text.push_str(&stueck);
            abschnitte.push(Abschnitt { von, bis, text: stueck.trim().to_string() });
        }
        Ok((aufraeumen(&text), abschnitte))
    }
}

/// Ordnet jedem Abschnitt die Person zu, die in seiner Zeit am längsten spricht,
/// und fasst aufeinanderfolgende Abschnitte derselben Person zusammen.
/// `sprecher`: (von, bis, nummer ab 0)
pub fn mit_sprechern(abschnitte: &[Abschnitt], sprecher: &[(f32, f32, i32)]) -> String {
    let mut zeilen: Vec<(i32, String)> = Vec::new();
    for a in abschnitte.iter().filter(|a| !a.text.is_empty()) {
        let wer = sprecher
            .iter()
            .map(|&(von, bis, nr)| (nr, (bis.min(a.bis) - von.max(a.von)).max(0.0)))
            .filter(|&(_, dauer)| dauer > 0.0)
            .fold(std::collections::HashMap::<i32, f32>::new(), |mut summe, (nr, dauer)| {
                *summe.entry(nr).or_default() += dauer;
                summe
            })
            .into_iter()
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(nr, _)| nr)
            // Kein Sprecher erkannt (z. B. Pause): beim vorigen bleiben
            .or(zeilen.last().map(|z| z.0))
            .unwrap_or(0);
        match zeilen.last_mut() {
            Some((nr, text)) if *nr == wer => {
                text.push(' ');
                text.push_str(&a.text);
            }
            _ => zeilen.push((wer, a.text.clone())),
        }
    }
    zeilen.iter().map(|(nr, text)| format!("Sprecher {}: {text}", nr + 1)).collect::<Vec<_>>().join("\n\n")
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

    fn ab(von: f32, bis: f32, text: &str) -> Abschnitt {
        Abschnitt { von, bis, text: text.into() }
    }

    #[test]
    fn sprecher_werden_zugeordnet_und_zusammengefasst() {
        let abschnitte = [ab(0.0, 2.0, "Hallo."), ab(2.0, 4.0, "Wie geht's?"), ab(4.5, 6.0, "Gut, danke."), ab(6.0, 7.0, "Und dir?")];
        let sprecher = [(0.0, 4.2, 0), (4.4, 7.0, 1)];
        assert_eq!(mit_sprechern(&abschnitte, &sprecher), "Sprecher 1: Hallo. Wie geht's?\n\nSprecher 2: Gut, danke. Und dir?");
    }

    #[test]
    fn pause_bleibt_beim_vorigen_sprecher() {
        let abschnitte = [ab(0.0, 2.0, "Eins."), ab(10.0, 11.0, "Zwei.")];
        assert_eq!(mit_sprechern(&abschnitte, &[(0.0, 2.0, 1)]), "Sprecher 2: Eins. Zwei.");
    }

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
        let (text, _) = e.erkennen(&ton, "de", "", |_| {}).unwrap();
        println!("{:.1} s Ton in {:.1} s: {text}", ton.len() as f32 / 16000.0, beginn.elapsed().as_secs_f32());
        let klein = text.to_lowercase();
        assert!(klein.contains("funkel") || klein.contains("probediktat"), "{text}");
        assert!(klein.contains("zehn") || klein.contains("10"), "{text}");
    }

    /// Erfundenes Gespräch mit zwei Systemstimmen: `FUNKEL_GESPRAECH=datei.wav … -- --ignored protokoll`
    #[test]
    #[ignore]
    fn protokoll_mit_zwei_stimmen() {
        let pfad = std::path::PathBuf::from(std::env::var("FUNKEL_GESPRAECH").expect("FUNKEL_GESPRAECH setzen"));
        let ton = wav_lesen(pfad.to_str().unwrap());
        let e = Erkenner::neu();
        e.laden(&crate::modell::pfad()).unwrap();
        let (_, abschnitte) = e.erkennen(&ton, "de", "", |_| {}).unwrap();
        let beginn = std::time::Instant::now();
        let sprecher = crate::helfer::sprecher(&pfad, 0).expect("Sprechertrennung");
        let zahl = sprecher.iter().map(|s| s.2).collect::<std::collections::HashSet<_>>().len();
        let text = mit_sprechern(&abschnitte, &sprecher);
        println!("Sprecher getrennt in {:.1} s, {zahl} Personen:\n{text}\n", beginn.elapsed().as_secs_f32());
        assert_eq!(zahl, 2, "zwei Stimmen erwartet");
        assert_eq!(text.matches("Sprecher ").count(), 5, "fünf Wortwechsel erwartet:\n{text}");
        let protokoll = crate::modus::nachputzen(
            &crate::helfer::umformen(&crate::modus::Modus::Protokoll.anweisung().unwrap(), &text).unwrap(),
        );
        println!("{protokoll}");
        let klein = protokoll.to_lowercase();
        assert!(klein.contains("freitag") && klein.contains("montag"), "{protokoll}");
    }
}
