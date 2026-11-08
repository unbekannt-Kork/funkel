//! Liest Aufträge zeilenweise als JSON von stdin, schreibt Antworten als JSON auf stdout.
//! Das Modell bleibt zwischen Aufträgen geladen.
//!
//!   {"modell": "/pfad/modell.gguf", "anweisung": "…", "text": "…"}
//!   → {"text": "…"}  oder  {"fehler": "…"}
//!
//!   {"aufgabe": "sprecher", "wav": "/pfad.wav", "segmentierung": "…onnx", "stimmen": "…onnx",
//!    "anzahl": 0, "schwelle": 0.55}
//!   → {"abschnitte": [{"von": 0.0, "bis": 3.2, "sprecher": 0}, …]}
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
    #[serde(default)]
    aufgabe: String,
    #[serde(default)]
    modell: String,
    #[serde(default)]
    anweisung: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    wav: String,
    #[serde(default)]
    segmentierung: String,
    #[serde(default)]
    stimmen: String,
    /// 0 = selbst erkennen
    #[serde(default)]
    anzahl: i32,
    #[serde(default)]
    schwelle: f32,
}

#[cfg(windows)]
fn sprecher(_: &Auftrag) -> Result<serde_json::Value, String> {
    Err("Sprechertrennung gibt es auf Windows noch nicht".into())
}

/// Wer spricht wann. Werte wie im Vorgänger erprobt: Schwelle 0,55, kleiner = mehr Personen.
/// Direkt über sherpa-onnx, damit beide Modelle alle Rechenkerne nutzen.
#[cfg(not(windows))]
fn sprecher(a: &Auftrag) -> Result<serde_json::Value, String> {
    use sherpa_rs_sys as s;
    use std::ffi::CString;
    let roh = std::fs::read(&a.wav).map_err(|e| e.to_string())?;
    let start = roh.windows(4).position(|w| w == b"data").map(|p| p + 8).unwrap_or(roh.len());
    let (paare, _) = roh[start..].as_chunks::<2>();
    let mut ton: Vec<f32> = paare.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect();
    let kerne = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8) as i32;
    let cpu = CString::new("cpu").unwrap();
    let stimmen = CString::new(a.stimmen.as_str()).map_err(|e| e.to_string())?;
    let segmentierung = CString::new(a.segmentierung.as_str()).map_err(|e| e.to_string())?;
    let einstellung = s::SherpaOnnxOfflineSpeakerDiarizationConfig {
        embedding: s::SherpaOnnxSpeakerEmbeddingExtractorConfig { model: stimmen.as_ptr(), num_threads: kerne, debug: 0, provider: cpu.as_ptr() },
        clustering: s::SherpaOnnxFastClusteringConfig {
            num_clusters: if a.anzahl > 0 { a.anzahl } else { -1 },
            threshold: if a.schwelle > 0.0 { a.schwelle } else { 0.55 },
        },
        min_duration_off: 0.5,
        min_duration_on: 0.3,
        segmentation: s::SherpaOnnxOfflineSpeakerSegmentationModelConfig {
            pyannote: s::SherpaOnnxOfflineSpeakerSegmentationPyannoteModelConfig { model: segmentierung.as_ptr() },
            num_threads: kerne,
            debug: 0,
            provider: cpu.as_ptr(),
        },
    };
    let mut abschnitte = Vec::new();
    unsafe {
        let trenner = s::SherpaOnnxCreateOfflineSpeakerDiarization(&einstellung);
        if trenner.is_null() {
            return Err("Sprechertrennung lässt sich nicht einrichten".into());
        }
        let ergebnis = s::SherpaOnnxOfflineSpeakerDiarizationProcessWithCallback(trenner, ton.as_mut_ptr(), ton.len() as i32, None, std::ptr::null_mut());
        let anzahl = s::SherpaOnnxOfflineSpeakerDiarizationResultGetNumSegments(ergebnis);
        let zeiger = s::SherpaOnnxOfflineSpeakerDiarizationResultSortByStartTime(ergebnis);
        if !zeiger.is_null() && anzahl > 0 {
            for seg in std::slice::from_raw_parts(zeiger, anzahl as usize) {
                abschnitte.push(json!({ "von": seg.start, "bis": seg.end, "sprecher": seg.speaker }));
            }
            s::SherpaOnnxOfflineSpeakerDiarizationDestroySegment(zeiger);
        }
        s::SherpaOnnxOfflineSpeakerDiarizationDestroyResult(ergebnis);
        s::SherpaOnnxDestroyOfflineSpeakerDiarization(trenner);
    }
    Ok(json!({ "abschnitte": abschnitte }))
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
            Ok(a) if a.aufgabe == "sprecher" => sprecher(&a).unwrap_or_else(|e| json!({ "fehler": e })),
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
