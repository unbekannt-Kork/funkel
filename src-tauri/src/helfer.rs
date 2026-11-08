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

pub fn modell_vorhanden() -> bool {
    std::fs::metadata(modell_pfad()).map(|m| m.len() >= MODELL_MINDESTGROESSE).unwrap_or(false)
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
    let mut wache = HELFER.lock().unwrap();
    for versuch in 0..2 {
        if wache.is_none() {
            *wache = Some(starten()?);
        }
        let h = wache.as_mut().unwrap();
        let auftrag = json!({ "modell": modell_pfad(), "anweisung": anweisung, "text": text });
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
        if let Some(t) = antwort["text"].as_str() {
            return Ok(t.to_string());
        }
        return Err(antwort["fehler"].as_str().unwrap_or("unbekannter Fehler").to_string());
    }
    unreachable!()
}
