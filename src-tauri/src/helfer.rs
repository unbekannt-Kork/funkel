//! Der Helfer-Prozess mit dem lokalen Sprachmodell (llama.cpp). Er wird beim
//! ersten Umformen gestartet und bleibt mit geladenem Modell am Leben.
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::einstellungen::daten_ordner;

pub const MODELL_NAME: &str = "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
pub const MODELL_QUELLE: &str =
    "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
pub const MODELL_MINDESTGROESSE: u64 = 2_000_000_000;

pub fn modell_pfad() -> PathBuf {
    daten_ordner().join("modelle").join(MODELL_NAME)
}

/// Sprechertrennung (sherpa-onnx): Segmentierung (pyannote 3.0) und Stimmabdruck (3D-Speaker)
pub const SEGMENTIERUNG: (&str, &str, u64) = (
    "segmentierung-pyannote-3.0.onnx",
    "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
    5_000_000,
);
pub const STIMMEN: (&str, &str, u64) = (
    "stimmen-3dspeaker.onnx",
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2netv2_sv_zh-cn_16k-common.onnx",
    60_000_000,
);

fn vorhanden(name: &str, mindest: u64) -> bool {
    std::fs::metadata(daten_ordner().join("modelle").join(name)).map(|m| m.len() >= mindest).unwrap_or(false)
}

/// Alles, was die Modi brauchen: Sprachmodell und beide Modelle der Sprechertrennung
pub fn modell_vorhanden() -> bool {
    vorhanden(MODELL_NAME, MODELL_MINDESTGROESSE) && vorhanden(SEGMENTIERUNG.0, SEGMENTIERUNG.2) && vorhanden(STIMMEN.0, STIMMEN.2)
}

/// Was noch zu laden ist: (Quelle, Ziel, Mindestgröße)
pub fn fehlende_modelle() -> Vec<(&'static str, PathBuf, u64)> {
    [(MODELL_NAME, MODELL_QUELLE, MODELL_MINDESTGROESSE), SEGMENTIERUNG, STIMMEN]
        .into_iter()
        .filter(|(name, _, mindest)| !vorhanden(name, *mindest))
        .map(|(name, quelle, mindest)| (quelle, daten_ordner().join("modelle").join(name), mindest))
        .collect()
}

/// Neben dem Programm (im App-Bündel), sonst aus dem Entwicklungsordner.
fn programm() -> PathBuf {
    let name = if cfg!(windows) { "funkel-helfer.exe" } else { "funkel-helfer" };
    if let Some(p) = std::env::var_os("FUNKEL_HELFER") {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../helfer/target/release").join(name))
}

struct Laufend {
    _kind: Child,
    ein: ChildStdin,
    aus: BufReader<ChildStdout>,
}

static HELFER: Mutex<Option<Laufend>> = Mutex::new(None);

fn starten() -> Result<Laufend, String> {
    let mut kind = Command::new(programm())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Helfer startet nicht: {e}"))?;
    let ein = kind.stdin.take().ok_or("kein stdin")?;
    let aus = BufReader::new(kind.stdout.take().ok_or("kein stdout")?);
    Ok(Laufend { _kind: kind, ein, aus })
}

/// Formt `text` nach `anweisung` um. Blockiert, bis das Ergebnis da ist.
pub fn umformen(anweisung: &str, text: &str) -> Result<String, String> {
    if !modell_vorhanden() {
        return Err("Sprachmodell für die Modi fehlt — in den Einstellungen laden".into());
    }
    let antwort = fragen(json!({ "modell": modell_pfad(), "anweisung": anweisung, "text": text }))?;
    antwort["text"].as_str().map(String::from).ok_or_else(|| "keine Antwort".into())
}

/// Wer spricht wann: (von, bis, Nummer ab 0). `anzahl` 0 = selbst erkennen.
pub fn sprecher(wav: &std::path::Path, anzahl: i32) -> Result<Vec<(f32, f32, i32)>, String> {
    let modelle = daten_ordner().join("modelle");
    let antwort = fragen(json!({
        "aufgabe": "sprecher", "wav": wav, "anzahl": anzahl,
        "segmentierung": modelle.join(SEGMENTIERUNG.0), "stimmen": modelle.join(STIMMEN.0),
    }))?;
    Ok(antwort["abschnitte"]
        .as_array()
        .ok_or("keine Abschnitte")?
        .iter()
        .map(|a| (a["von"].as_f64().unwrap_or(0.0) as f32, a["bis"].as_f64().unwrap_or(0.0) as f32, a["sprecher"].as_i64().unwrap_or(0) as i32))
        .collect())
}

fn fragen(auftrag: Value) -> Result<Value, String> {
    let mut wache = HELFER.lock().unwrap();
    for versuch in 0..2 {
        if wache.is_none() {
            *wache = Some(starten()?);
        }
        let h = wache.as_mut().unwrap();
        let mut zeile = String::new();
        let ok = writeln!(h.ein, "{auftrag}").is_ok() && h.ein.flush().is_ok() && h.aus.read_line(&mut zeile).is_ok_and(|n| n > 0);
        if !ok {
            // Helfer ist abgestürzt: einmal neu starten
            *wache = None;
            if versuch == 1 {
                return Err("Helfer antwortet nicht".into());
            }
            continue;
        }
        let antwort: Value = serde_json::from_str(&zeile).map_err(|e| e.to_string())?;
        if let Some(f) = antwort["fehler"].as_str() {
            return Err(f.to_string());
        }
        return Ok(antwort);
    }
    unreachable!()
}
