//! Liest Aufträge zeilenweise als JSON von stdin, schreibt Antworten als JSON auf stdout.
//! Das Modell bleibt zwischen Aufträgen geladen.
//!
//!   {"modell": "/pfad/modell.gguf", "anweisung": "…", "text": "…"}
//!   → {"text": "…"}  oder  {"fehler": "…"}
use std::io::{BufRead, Write};
use std::num::NonZeroU32;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Auftrag {
    modell: String,
    anweisung: String,
    text: String,
}

const KONTEXT: u32 = 16_384;

fn umformen(backend: &LlamaBackend, modell: &LlamaModel, anweisung: &str, text: &str) -> Result<String, String> {
    let vorlage = modell.chat_template(None).map_err(|e| format!("Vorlage: {e}"))?;
    let nachrichten = vec![
        LlamaChatMessage::new("system".into(), anweisung.into()).map_err(|e| e.to_string())?,
        LlamaChatMessage::new("user".into(), text.into()).map_err(|e| e.to_string())?,
    ];
    let eingabe = modell.apply_chat_template(&vorlage, &nachrichten, true).map_err(|e| format!("Vorlage anwenden: {e}"))?;
    let wortschatz = modell.vocab();
    // Sondertokens der Chat-Vorlage auswerten, BOS steckt schon in der Vorlage
    let zeichen = wortschatz.tokenize(eingabe.as_bytes(), false, true);
    let laenge = (zeichen.len() as u32 * 2 + 512).min(KONTEXT);
    if zeichen.len() as u32 >= laenge {
        return Err("Text zu lang".into());
    }
    let mut kontext = modell
        .new_context(backend, LlamaContextParams::default().with_n_ctx(NonZeroU32::new(laenge)).with_n_batch(laenge))
        .map_err(|e| format!("Kontext: {e}"))?;
    let mut stapel = LlamaBatch::new(laenge as usize, 1);
    let letzter = zeichen.len() - 1;
    for (i, z) in zeichen.iter().enumerate() {
        stapel.add(*z, i as i32, &[0], i == letzter).map_err(|e| e.to_string())?;
    }
    kontext.decode(&mut stapel).map_err(|e| format!("Rechnen: {e}"))?;
    // Gierig: immer das wahrscheinlichste Wort — nichts erfinden, gleiches Ergebnis bei gleichem Text
    let mut auswahl = LlamaSampler::greedy();
    let mut aus: Vec<u8> = Vec::new();
    let mut pos = zeichen.len() as i32;
    while (pos as u32) < laenge {
        let z = auswahl.sample(&kontext, stapel.n_tokens() - 1);
        auswahl.accept(z);
        if wortschatz.is_eog(z) {
            break;
        }
        aus.extend(wortschatz.token_to_piece(z, false, None));
        stapel.clear();
        stapel.add(z, pos, &[0], true).map_err(|e| e.to_string())?;
        pos += 1;
        kontext.decode(&mut stapel).map_err(|e| format!("Rechnen: {e}"))?;
    }
    Ok(String::from_utf8_lossy(&aus).trim().to_string())
}

fn main() {
    llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default().with_logs_enabled(false));
    let backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            println!("{}", json!({ "fehler": format!("llama.cpp: {e}") }));
            return;
        }
    };
    let mut geladen: Option<(String, LlamaModel)> = None;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for zeile in stdin.lock().lines() {
        let Ok(zeile) = zeile else { break };
        if zeile.trim().is_empty() {
            continue;
        }
        let antwort = match serde_json::from_str::<Auftrag>(&zeile) {
            Err(e) => json!({ "fehler": format!("Auftrag: {e}") }),
            Ok(a) => {
                if geladen.as_ref().map(|(p, _)| p != &a.modell).unwrap_or(true) {
                    let param = LlamaModelParams::default().with_n_gpu_layers(1000);
                    geladen = LlamaModel::load_from_file(&backend, &a.modell, &param).ok().map(|m| (a.modell.clone(), m));
                }
                match &geladen {
                    None => json!({ "fehler": "Sprachmodell lässt sich nicht laden" }),
                    Some((_, m)) => match umformen(&backend, m, &a.anweisung, &a.text) {
                        Ok(t) => json!({ "text": t }),
                        Err(e) => json!({ "fehler": e }),
                    },
                }
            }
        };
        let _ = writeln!(stdout, "{antwort}");
        let _ = stdout.flush();
    }
}
