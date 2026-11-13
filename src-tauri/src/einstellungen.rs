//! Einstellungen, Wörterbuch und Pfade. Gespeichert als JSON im Konfigurationsordner.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Einstellungen {
    /// Tastencode des Auslösers; `None` = die Taste unter Escape
    pub taste: Option<u32>,
    /// Text nach dem Diktat sofort einfügen (sonst nur Zwischenablage)
    pub einfuegen: bool,
    /// `auto`, `de` oder `en`
    pub sprache: String,
    pub autostart: bool,
    /// Zeitfenster für den Doppeltipp
    pub doppeltipp_ms: u64,
    /// Ton und Text so viele Tage behalten (0 = für immer)
    pub aufbewahren_tage: u32,
    /// Was aus dem Gesagten wird
    pub modus: crate::modus::Modus,
    /// Personen im Protokoll; 0 = selbst erkennen
    pub personen: i32,
    /// Kleiner Ton, wenn der Text fertig ist
    pub ton: bool,
    /// `beides` (Rahmen und Feld), `rahmen`, `feld` oder `nichts`
    pub anzeige: String,
    /// Nach dem Diktat die Modi zur Schnellwahl einblenden (Ziffern 1–5)
    pub modi_zeigen: bool,
}

impl Default for Einstellungen {
    fn default() -> Self {
        Self { taste: None, einfuegen: true, sprache: "auto".into(), autostart: true, doppeltipp_ms: 550, aufbewahren_tage: 7, modus: Default::default(), personen: 0, ton: true, anzeige: "beides".into(), modi_zeigen: true }
    }
}

pub fn konfig_ordner() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("funkel")
}

pub fn daten_ordner() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("funkel")
}

fn datei() -> PathBuf {
    konfig_ordner().join("einstellungen.json")
}

fn woerterbuch_datei() -> PathBuf {
    konfig_ordner().join("woerterbuch.txt")
}

pub fn laden() -> Einstellungen {
    fs::read_to_string(datei())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn speichern(werte: &Einstellungen) -> Result<(), String> {
    fs::create_dir_all(konfig_ordner()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(werte).map_err(|e| e.to_string())?;
    fs::write(datei(), text).map_err(|e| e.to_string())
}

pub fn woerterbuch() -> String {
    fs::read_to_string(woerterbuch_datei()).unwrap_or_default()
}

pub fn woerterbuch_speichern(text: &str) -> Result<(), String> {
    fs::create_dir_all(konfig_ordner()).map_err(|e| e.to_string())?;
    fs::write(woerterbuch_datei(), text).map_err(|e| e.to_string())
}

/// Vorlage für Whisper: Begriffe aus dem Wörterbuch, dahinter Füllwörter,
/// damit wortgetreu mitgeschrieben wird. Begriffe zuerst — Whisper nimmt nur
/// etwa 224 Marken, der Rest fällt weg.
pub fn vorlage(sprache: &str) -> String {
    let mut teile = vec![begriffe()];
    // Füllwörter als Stilvorbild nur für die eigene Sprache, sonst zieht die Vorlage
    // Whisper in diese Sprache
    if matches!(sprache, "auto" | "de") {
        teile.push(crate::modus::STIL_DE.into());
    }
    if matches!(sprache, "auto" | "en") {
        teile.push(crate::modus::STIL_EN.into());
    }
    let text = teile.into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
    text.chars().take(900).collect()
}

fn begriffe() -> String {
    let begriffe: Vec<String> = woerterbuch()
        .lines()
        .map(str::trim)
        .filter(|z| !z.is_empty() && !z.starts_with('#'))
        .map(String::from)
        .collect();
    if begriffe.is_empty() {
        String::new()
    } else {
        format!("{}.", begriffe.join(", "))
    }
}

/// Protokoll ohne Inhalte: nur Abläufe und Fehler, nie diktierter Text.
pub fn protokoll(zeile: &str) {
    use std::io::Write;
    let _ = fs::create_dir_all(daten_ordner());
    let stempel = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(daten_ordner().join("funkel.log")) {
        let _ = writeln!(f, "{stempel}  {zeile}");
    }
    eprintln!("{zeile}");
}

#[cfg(test)]
mod tests_vorlage {
    use crate::modus::{STIL_DE, STIL_EN};

    #[test]
    fn fremde_sprache_bekommt_keine_deutschen_fuellwoerter() {
        let fr = super::vorlage("fr");
        assert!(!fr.contains(STIL_DE) && !fr.contains(STIL_EN));
        assert!(super::vorlage("de").contains(STIL_DE));
        assert!(super::vorlage("en").contains(STIL_EN) && !super::vorlage("en").contains(STIL_DE));
    }
}
