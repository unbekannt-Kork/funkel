//! Aufnahmen und Texte auf der Platte: Ton läuft während der Aufnahme laufend
//! in eine WAV-Datei, der Text kommt danach daneben. Stürzt etwas ab, bleibt die
//! WAV ohne Text liegen und wird beim nächsten Start zum Wiederherstellen angeboten.
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDateTime};
use serde::Serialize;

use crate::aufnahme::RATE;
use crate::einstellungen::daten_ordner;

const ZEITFORMAT: &str = "%Y-%m-%d_%H-%M-%S";

pub fn ordner() -> PathBuf {
    daten_ordner().join("aufnahmen")
}

#[derive(Serialize, Clone, Debug)]
pub struct Eintrag {
    pub pfad: String,
    /// „08.11. 13:40“
    pub zeit: String,
    pub sekunden: u32,
    /// `None` = nicht fertig erkannt (Absturz oder Abbruch)
    pub text: Option<String>,
}

pub fn neue_aufnahme() -> PathBuf {
    ordner().join(format!("{}.wav", Local::now().format(ZEITFORMAT)))
}

fn text_pfad(wav: &Path) -> PathBuf {
    wav.with_extension("txt")
}

// -- WAV: 16 kHz, mono, 16 Bit ---------------------------------------------------

/// Schreibt den Kopf mit Länge 0; `abschliessen` trägt die Längen nach.
/// Bricht das Programm vorher ab, liest `lesen` trotzdem alles bis zum Dateiende.
pub fn wav_anlegen(pfad: &Path) -> std::io::Result<fs::File> {
    fs::create_dir_all(pfad.parent().unwrap())?;
    let mut f = fs::File::create(pfad)?;
    let mut kopf = Vec::with_capacity(44);
    kopf.extend_from_slice(b"RIFF");
    kopf.extend_from_slice(&36u32.to_le_bytes());
    kopf.extend_from_slice(b"WAVEfmt ");
    kopf.extend_from_slice(&16u32.to_le_bytes());
    kopf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    kopf.extend_from_slice(&1u16.to_le_bytes()); // mono
    kopf.extend_from_slice(&RATE.to_le_bytes());
    kopf.extend_from_slice(&(RATE * 2).to_le_bytes());
    kopf.extend_from_slice(&2u16.to_le_bytes());
    kopf.extend_from_slice(&16u16.to_le_bytes());
    kopf.extend_from_slice(b"data");
    kopf.extend_from_slice(&0u32.to_le_bytes());
    f.write_all(&kopf)?;
    Ok(f)
}

pub fn wav_anhaengen(f: &mut fs::File, ton: &[f32]) -> std::io::Result<()> {
    let mut roh = Vec::with_capacity(ton.len() * 2);
    for x in ton {
        roh.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    f.write_all(&roh)?;
    f.sync_data()
}

pub fn wav_abschliessen(f: &mut fs::File) -> std::io::Result<()> {
    let laenge = f.seek(SeekFrom::End(0))? as u32;
    f.seek(SeekFrom::Start(4))?;
    f.write_all(&(laenge - 8).to_le_bytes())?;
    f.seek(SeekFrom::Start(40))?;
    f.write_all(&(laenge - 44).to_le_bytes())?;
    f.sync_all()
}

/// Alle Abtastwerte hinter dem `data`-Kopf bis zum Dateiende — auch bei
/// unvollständigem Kopf nach einem Absturz.
pub fn wav_lesen(pfad: &Path) -> std::io::Result<Vec<f32>> {
    let roh = fs::read(pfad)?;
    let start = roh.windows(4).position(|w| w == b"data").map(|p| p + 8).unwrap_or(roh.len());
    let (paare, _) = roh[start..].as_chunks::<2>();
    Ok(paare.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect())
}

fn sekunden(pfad: &Path) -> u32 {
    fs::metadata(pfad).map(|m| (m.len().saturating_sub(44) / (RATE as u64 * 2)) as u32).unwrap_or(0)
}

// -- Text und Verlauf ----------------------------------------------------------

/// Wortgetreuer Rohtext neben dem Ergebnis: daraus entstehen alle Modi.
pub fn roh_speichern(wav: &Path, text: &str) -> std::io::Result<()> {
    fs::write(wav.with_extension("roh.txt"), text)
}

pub fn text_speichern(wav: &Path, text: &str) -> std::io::Result<()> {
    fs::write(text_pfad(wav), text)
}

fn zeit_von(pfad: &Path) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(pfad.file_stem()?.to_str()?, ZEITFORMAT).ok()
}

/// Neueste zuerst, höchstens `hoechstens`. `ausser` = die gerade laufende Aufnahme.
/// Texte werden nur für diese gelesen: das Einstellungsfenster fragt alle 3 s, und
/// im Ordner lagen 1291 Dateien (14.11.2026).
pub fn eintraege(ausser: Option<&Path>, hoechstens: usize) -> Vec<Eintrag> {
    let Ok(dateien) = fs::read_dir(ordner()) else { return vec![] };
    let mut wavs: Vec<PathBuf> = dateien
        .filter_map(|d| d.ok().map(|d| d.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "wav") && Some(p.as_path()) != ausser)
        .filter(|p| zeit_von(p).is_some())
        .collect();
    wavs.sort();
    wavs.reverse();
    wavs.into_iter()
        .take(hoechstens)
        .map(|p| Eintrag {
            zeit: zeit_von(&p).map(|z| z.format("%d.%m. %H:%M").to_string()).unwrap_or_default(),
            sekunden: sekunden(&p),
            text: fs::read_to_string(text_pfad(&p)).ok(),
            pfad: p.to_string_lossy().into_owned(),
        })
        .collect()
}

pub fn unfertige(ausser: Option<&Path>) -> Vec<Eintrag> {
    eintraege(ausser, usize::MAX).into_iter().filter(|e| e.text.is_none() && e.sekunden > 0).collect()
}

/// Löscht Ton und Text, die älter als `tage` sind (Datensparsamkeit). 0 = nie.
pub fn aufraeumen(tage: u32) -> usize {
    if tage == 0 {
        return 0;
    }
    let grenze = Local::now().naive_local() - chrono::Duration::days(tage as i64);
    let mut weg = 0;
    for e in eintraege(None, usize::MAX) {
        let p = PathBuf::from(&e.pfad);
        if zeit_von(&p).is_some_and(|z| z < grenze) {
            let _ = fs::remove_file(text_pfad(&p));
            let _ = fs::remove_file(p.with_extension("roh.txt"));
            if fs::remove_file(&p).is_ok() {
                weg += 1;
            }
        }
    }
    weg
}

/// Für Meldungen: „13:40, 2:13 min“
pub fn kurz(e: &Eintrag) -> String {
    let uhr = e.zeit.split(' ').nth(1).unwrap_or(&e.zeit);
    format!("{uhr}, {}:{:02} min", e.sekunden / 60, e.sekunden % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wegwerf() -> PathBuf {
        let d = std::env::temp_dir().join(format!("funkel-test-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn wav_hin_und_zurueck() {
        let p = wegwerf().join("2026-01-01_10-00-00.wav");
        let mut f = wav_anlegen(&p).unwrap();
        let ton: Vec<f32> = (0..16_000).map(|i| (i as f32 / 50.0).sin() * 0.5).collect();
        wav_anhaengen(&mut f, &ton[..8000]).unwrap();
        wav_anhaengen(&mut f, &ton[8000..]).unwrap();
        wav_abschliessen(&mut f).unwrap();
        let zurueck = wav_lesen(&p).unwrap();
        assert_eq!(zurueck.len(), 16_000);
        assert!((zurueck[100] - ton[100]).abs() < 0.001);
        assert_eq!(sekunden(&p), 1);
    }

    #[test]
    fn nach_absturz_ist_der_ton_noch_lesbar() {
        // Kopf mit Länge 0, kein Abschluss — wie nach einem harten Abbruch
        let p = wegwerf().join("2026-01-01_11-00-00.wav");
        let mut f = wav_anlegen(&p).unwrap();
        wav_anhaengen(&mut f, &[0.25; 3200]).unwrap();
        drop(f);
        assert_eq!(wav_lesen(&p).unwrap().len(), 3200);
    }

    #[test]
    fn zeit_aus_dem_dateinamen() {
        let z = zeit_von(Path::new("/x/2026-11-08_13-40-12.wav")).unwrap();
        assert_eq!(z.format("%d.%m. %H:%M").to_string(), "08.11. 13:40");
        assert!(zeit_von(Path::new("/x/irgendwas.wav")).is_none());
    }
}
