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

    /// Erkennt den Ton (16 kHz, mono).
    pub fn erkennen(
        &self,
        ton: &[f32],
        sprache: &str,
        vorlage: &str,
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
        // Kein Temperatur-Rückfall: bei unsicherem Ergebnis rechnete Whisper bis zu
        // fünfmal neu. Das waren die Spitzen von 3–8 s nach dem Stopp (Log bis
        // 13.11.2026: 3,8 s Rest in 6,5 s, 18 s Rest in 8,0 s statt etwa 1 s).
        p.set_temperature_inc(0.0);
        p.set_suppress_blank(true);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        if !vorlage.is_empty() {
            p.set_initial_prompt(vorlage);
        }
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

    /// Deutsch oder Englisch, `None` = noch unsicher. Kostet einen eigenen Durchgang —
    /// deshalb während der Aufnahme: bei „automatisch“ verdoppelte er sonst nach dem
    /// Stopp die Wartezeit (gemessen 09.11.2026: 1,32–1,61 s statt 0,66–0,83 s).
    pub fn sprache_erkennen(&self, ton: &[f32]) -> Option<String> {
        let wache = self.kontext.lock().unwrap();
        let mut zustand = wache.as_ref()?.create_state().ok()?;
        let kerne = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
        zustand.pcm_to_mel(ton, kerne).ok()?;
        let (_, wahrscheinlich) = zustand.lang_detect(0, kerne).ok()?;
        let p = |kuerzel: &str| whisper_rs::get_lang_id(kuerzel).and_then(|id| wahrscheinlich.get(id as usize).copied());
        haupt_sprache(p("de")?, p("en")?).map(str::to_string)
    }
}

/// „Automatisch“ heißt Deutsch oder Englisch. Frei über alle Sprachen hielt Whisper
/// deutsches Diktat für Russisch oder blieb unsicher und rechnete doppelt (Log bis
/// 13.11.2026: 35-mal unentschieden, einmal Russisch). Andere Sprachen fest einstellen.
/// Englisch nur, wenn klar (14.11.2026: nach 3 s fast stiller Luft gewann Englisch
/// knapp, das deutsche Diktat kam als Englisch). Sonst Deutsch, wenn deutlich; sonst
/// unsicher — dann mit mehr Ton noch einmal, am Ende gilt Deutsch.
const ENGLISCH_AB: f32 = 0.6;
const DEUTSCH_AB: f32 = 0.4;

fn haupt_sprache(deutsch: f32, englisch: f32) -> Option<&'static str> {
    if englisch >= ENGLISCH_AB && englisch > deutsch {
        Some("en")
    } else if deutsch >= DEUTSCH_AB && deutsch >= englisch {
        Some("de")
    } else {
        None
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

fn pegel_db(ton: &[f32]) -> f32 {
    if ton.is_empty() {
        return -99.0;
    }
    let mittel = ton.iter().map(|x| x * x).sum::<f32>() / ton.len() as f32;
    10.0 * mittel.max(1e-12).log10()
}

/// Wo das Sprechen endet: Index nach der letzten lauten Stelle plus 0,5 s.
/// In stiller Luft erfindet Whisper Schlusssätze (im Vorgänger am 17.09.2026
/// gemessen: 10 s Ruhe bei −65 dB → „Tschüss, tschüss, tschüss“). Zählt nur, was
/// 300 ms am Stück laut ist; die letzten 0,4 s (Klick der Taste) zählen nicht.
pub fn sprechen_endet(ton: &[f32], rate: usize) -> usize {
    let rahmen = rate / 50; // 20 ms
    let anzahl = ton.len() / rahmen;
    if anzahl < 20 {
        return ton.len();
    }
    let leistung: Vec<f32> = (0..anzahl).map(|i| ton[i * rahmen..(i + 1) * rahmen].iter().map(|x| x * x).sum::<f32>() / rahmen as f32).collect();
    let db: Vec<f32> = (0..anzahl)
        .map(|i| {
            let (von, bis) = (i.saturating_sub(7), (i + 8).min(anzahl));
            let m = leistung[von..bis].iter().sum::<f32>() / (bis - von) as f32;
            10.0 * m.max(1e-12).log10()
        })
        .collect();
    let mut sortiert = db.clone();
    sortiert.sort_by(f32::total_cmp);
    let laut = sortiert[(sortiert.len() * 95 / 100).min(sortiert.len() - 1)];
    let schwelle = (laut - 20.0).max(-50.0);
    match db[..anzahl.saturating_sub(20).max(1)].iter().rposition(|&d| d > schwelle) {
        Some(letzte) => ((letzte + 25) * rahmen).min(ton.len()),
        None => 0,
    }
}

/// Streicht Schlussabschnitte, die im Ton nicht zu hören sind: mehr als 20 dB
/// unter dem Median der übrigen (im Vorgänger gemessen: gesprochen −35 dB,
/// erfundenes „Tschüss“ −65 dB).
pub fn nachlauf_streichen(abschnitte: Vec<Abschnitt>, ton: &[f32], rate: usize) -> Vec<Abschnitt> {
    if abschnitte.len() < 3 {
        return abschnitte;
    }
    let pegel: Vec<f32> = abschnitte
        .iter()
        .map(|a| {
            let von = ((a.von * rate as f32) as usize).min(ton.len());
            let bis = ((a.bis * rate as f32) as usize).clamp(von, ton.len());
            pegel_db(&ton[von..bis])
        })
        .collect();
    let mut sortiert = pegel.clone();
    sortiert.sort_by(f32::total_cmp);
    let grenze = sortiert[sortiert.len() / 2] - 20.0;
    let mut behalten = abschnitte.len();
    while behalten > 2 && pegel[behalten - 1] < grenze {
        behalten -= 1;
    }
    abschnitte.into_iter().take(behalten).collect()
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

    fn laut(sekunden: f32, rate: usize) -> Vec<f32> {
        (0..(sekunden * rate as f32) as usize).map(|i| (i as f32 / 9.0).sin() * 0.3).collect()
    }

    #[test]
    fn stille_am_ende_wird_abgeschnitten() {
        let rate = 16_000;
        let mut ton = laut(3.0, rate);
        ton.extend(vec![0.0005; 10 * rate]); // 10 s fast still
        let ende = sprechen_endet(&ton, rate);
        assert!(ende > 3 * rate && ende < 4 * rate, "Ende bei {:.2} s", ende as f32 / rate as f32);
    }

    #[test]
    fn klick_der_taste_haelt_die_stille_nicht_offen() {
        let rate = 16_000;
        let mut ton = laut(2.0, rate);
        ton.extend(vec![0.0; 5 * rate]);
        ton.extend(laut(0.05, rate)); // Klick ganz am Ende
        ton.extend(vec![0.0; rate / 10]);
        assert!(sprechen_endet(&ton, rate) < 3 * rate);
    }

    #[test]
    fn leiser_schlusssatz_wird_gestrichen() {
        let rate = 16_000;
        let mut ton = laut(6.0, rate);
        ton.extend(vec![0.0003; 2 * rate]);
        let a = |von: f32, bis: f32, t: &str| ab(von, bis, t);
        let abschnitte = vec![a(0.0, 2.0, "Eins."), a(2.0, 4.0, "Zwei."), a(4.0, 6.0, "Drei."), a(6.0, 8.0, "Erfunden.")];
        let rest = nachlauf_streichen(abschnitte, &ton, rate);
        assert_eq!(rest.iter().map(|a| a.text.as_str()).collect::<Vec<_>>(), ["Eins.", "Zwei.", "Drei."]);
    }

    #[test]
    fn automatisch_ist_deutsch_oder_englisch() {
        assert_eq!(haupt_sprache(0.9, 0.05), Some("de"));
        assert_eq!(haupt_sprache(0.1, 0.8), Some("en"));
    }

    #[test]
    fn knappes_englisch_ist_unsicher() {
        // Stille Luft am Anfang: beide klein, Englisch knapp vorn
        assert_eq!(haupt_sprache(0.2, 0.3), None);
        assert_eq!(haupt_sprache(0.35, 0.55), None);
        assert_eq!(haupt_sprache(0.0, 0.0), None);
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
        let (text, _) = e.erkennen(&ton, "de", "").unwrap();
        println!("{:.1} s Ton in {:.1} s: {text}", ton.len() as f32 / 16000.0, beginn.elapsed().as_secs_f32());
        let klein = text.to_lowercase();
        assert!(klein.contains("funkel") || klein.contains("probediktat"), "{text}");
        assert!(klein.contains("zehn") || klein.contains("10"), "{text}");
    }

    #[test]
    #[ignore]
    fn sprache_wird_vorab_erkannt() {
        let pfad = std::env::var("FUNKEL_TESTTON").expect("FUNKEL_TESTTON setzen");
        let ton = wav_lesen(&pfad);
        let e = Erkenner::neu();
        e.laden(&crate::modell::pfad()).unwrap();
        assert_eq!(e.sprache_erkennen(&ton[..ton.len().min(32000)]).as_deref(), Some(std::env::var("FUNKEL_SPRACHE").as_deref().unwrap_or("de")));
        let sprache = e.sprache_erkennen(&ton).unwrap();
        let beginn = std::time::Instant::now();
        let (text, _) = e.erkennen(&ton, &sprache, "").unwrap();
        println!("{:.1} s Ton mit fester Sprache in {:.2} s: {text}", ton.len() as f32 / 16000.0, beginn.elapsed().as_secs_f32());
    }

    /// Leise Luft am Anfang darf nicht als Englisch gelten (14.11.2026)
    #[test]
    #[ignore]
    fn leise_luft_ist_kein_englisch() {
        let e = Erkenner::neu();
        e.laden(&crate::modell::pfad()).unwrap();
        let luft: Vec<f32> = (0..3 * 16000).map(|i| ((i * 7919 % 1000) as f32 / 1000.0 - 0.5) * 0.002).collect();
        let sprache = e.sprache_erkennen(&luft);
        println!("3 s leise Luft: {sprache:?}");
        assert_ne!(sprache.as_deref(), Some("en"));
    }

    /// Erfundenes Gespräch mit zwei Systemstimmen: `FUNKEL_GESPRAECH=datei.wav … -- --ignored protokoll`
    #[test]
    #[ignore]
    fn protokoll_mit_zwei_stimmen() {
        let pfad = std::path::PathBuf::from(std::env::var("FUNKEL_GESPRAECH").expect("FUNKEL_GESPRAECH setzen"));
        let ton = wav_lesen(pfad.to_str().unwrap());
        let e = Erkenner::neu();
        e.laden(&crate::modell::pfad()).unwrap();
        let (_, abschnitte) = e.erkennen(&ton, "de", "").unwrap();
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
