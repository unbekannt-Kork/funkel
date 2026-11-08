//! Sprachmodell einmalig herunterladen. Danach läuft alles offline.
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::einstellungen::daten_ordner;

/// large-v3-turbo, auf 5 Bit verdichtet: fast dieselbe Güte wie das volle Modell
/// (1,6 GB), aber ein Drittel so groß und schneller geladen.
pub const NAME: &str = "ggml-large-v3-turbo-q5_0.bin";
const QUELLE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";
/// Unter dieser Größe ist die Datei sicher unvollständig
const MINDESTGROESSE: u64 = 500_000_000;

pub fn pfad() -> PathBuf {
    daten_ordner().join("modelle").join(NAME)
}

pub fn vorhanden() -> bool {
    fs::metadata(pfad()).map(|m| m.len() >= MINDESTGROESSE).unwrap_or(false)
}

/// Lädt das Modell; `fortschritt(geladen, gesamt)` etwa jedes Prozent.
pub fn herunterladen(mut fortschritt: impl FnMut(u64, u64)) -> Result<(), String> {
    let ziel = pfad();
    fs::create_dir_all(ziel.parent().unwrap()).map_err(|e| e.to_string())?;
    let teil = ziel.with_extension("teil");
    let antwort = ureq::get(QUELLE).call().map_err(|e| format!("Download: {e}"))?;
    let gesamt: u64 = antwort.header("Content-Length").and_then(|l| l.parse().ok()).unwrap_or(0);
    let mut leser = antwort.into_reader();
    let mut datei = fs::File::create(&teil).map_err(|e| e.to_string())?;
    let mut puffer = vec![0u8; 1 << 16];
    let (mut geladen, mut gemeldet) = (0u64, 0u64);
    loop {
        let n = leser.read(&mut puffer).map_err(|e| format!("Download: {e}"))?;
        if n == 0 {
            break;
        }
        datei.write_all(&puffer[..n]).map_err(|e| e.to_string())?;
        geladen += n as u64;
        if gesamt == 0 || geladen - gemeldet > gesamt / 100 {
            fortschritt(geladen, gesamt);
            gemeldet = geladen;
        }
    }
    datei.flush().map_err(|e| e.to_string())?;
    if geladen < MINDESTGROESSE || (gesamt > 0 && geladen != gesamt) {
        let _ = fs::remove_file(&teil);
        return Err("Download unvollständig".into());
    }
    fs::rename(&teil, &ziel).map_err(|e| e.to_string())?;
    fortschritt(geladen, gesamt);
    Ok(())
}
