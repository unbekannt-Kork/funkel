//! Andere Systeme: noch nicht unterstützt, aber baubar.
use super::Rueckruf;

pub fn abgriff_starten(_: Rueckruf) -> bool { false }
pub fn abgriff_wach_halten() -> bool { false }
pub fn taste_senden(_: u32) {}
pub fn einfuegen() {}
pub fn ton_fertig() {}
pub fn freigabe() -> bool { false }
pub fn freigabe_anfragen() {}
pub fn freigabe_oeffnen() {}
pub fn standard_taste() -> u32 { 0 }
pub fn ziffer(_: u32) -> Option<u8> { None }
pub fn tasten_name(code: u32) -> String { format!("Taste {code}") }
pub fn fenster_ueber_allem(_: &tauri::WebviewWindow) {}
