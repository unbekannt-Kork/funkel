//! Ein durchsichtiges Fenster je Bildschirm für Rand und Feld. Die Fenster
//! bleiben ständig offen und sind leer, solange nichts passiert: ein Fenster
//! erst beim Diktat zu zeigen, könnte der Schreibmarke den Fokus nehmen.
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::plattform;

const PRAEFIX: &str = "rand-";

/// Was gezeigt wird: `beides`, `rahmen`, `feld` oder `nichts` (aus den Einstellungen)
static ANZEIGE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn anzeige_setzen(art: &str) {
    *ANZEIGE.lock().unwrap() = art.to_string();
}

fn bildschirme(app: &AppHandle) -> Vec<tauri::Monitor> {
    app.available_monitors().unwrap_or_default()
}

/// Je Bildschirm genau ein Fenster in seiner Größe. Neu angesteckte Bildschirme
/// bekommen eins, abgezogene verlieren ihres.
pub fn sicherstellen(app: &AppHandle) {
    let schirme = bildschirme(app);
    for (i, schirm) in schirme.iter().enumerate() {
        let name = format!("{PRAEFIX}{i}");
        let pos = *schirm.position();
        let groesse = *schirm.size();
        if let Some(f) = app.get_webview_window(&name) {
            let passt = f.outer_position().ok() == Some(pos) && f.outer_size().ok() == Some(groesse);
            if !passt {
                let _ = f.set_position(PhysicalPosition::new(pos.x, pos.y));
                let _ = f.set_size(PhysicalSize::new(groesse.width, groesse.height));
            }
            continue;
        }
        let massstab = schirm.scale_factor();
        let fenster = WebviewWindowBuilder::new(app, &name, WebviewUrl::App("overlay.html".into()))
            .title("Funkel")
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .focused(false)
            .position(pos.x as f64 / massstab, pos.y as f64 / massstab)
            .inner_size(groesse.width as f64 / massstab, groesse.height as f64 / massstab)
            .build();
        match fenster {
            Ok(f) => {
                let _ = f.set_position(PhysicalPosition::new(pos.x, pos.y));
                let _ = f.set_size(PhysicalSize::new(groesse.width, groesse.height));
                let _ = f.set_ignore_cursor_events(true);
                plattform::fenster_ueber_allem(&f);
            }
            Err(e) => crate::einstellungen::protokoll(&format!("Randfenster {i}: {e}")),
        }
    }
    for (name, f) in app.webview_windows() {
        if let Some(nr) = name.strip_prefix(PRAEFIX).and_then(|n| n.parse::<usize>().ok()) {
            if nr >= schirme.len() {
                let _ = f.close();
            }
        }
    }
}

/// Das Fenster des Bildschirms, auf dem die Maus steht: dort erscheint das Feld.
pub fn feld_fenster(app: &AppHandle) -> String {
    let maus = app.cursor_position().ok();
    let schirme = bildschirme(app);
    let nr = maus
        .and_then(|m| {
            schirme.iter().position(|s| {
                let (p, g) = (s.position(), s.size());
                m.x >= p.x as f64 && m.x < (p.x + g.width as i32) as f64 && m.y >= p.y as f64 && m.y < (p.y + g.height as i32) as f64
            })
        })
        .unwrap_or(0);
    format!("{PRAEFIX}{nr}")
}

/// Schickt den Zustand an alle Randfenster; `mit_feld(true)` für das Fenster mit dem Feld.
pub fn senden(app: &AppHandle, feld: &str, zustand: impl Fn(bool) -> Value) {
    let art = ANZEIGE.lock().unwrap().clone();
    let (mit_feld, mit_rand) = sichtbar(&art);
    for name in app.webview_windows().keys().filter(|n| n.starts_with(PRAEFIX)) {
        let mut wert = zustand(name == feld && mit_feld);
        if let Some(objekt) = wert.as_object_mut() {
            objekt.insert("rand".into(), Value::Bool(mit_rand));
            // Der Ton kommt aus dem Fenster des Feldes, auch wenn das Feld verborgen ist
            objekt.insert("hier".into(), Value::Bool(name == feld));
        }
        let _ = app.emit_to(name.as_str(), "zustand", wert);
    }
}

/// (Feld, Rahmen) je Einstellung „Anzeige“
fn sichtbar(art: &str) -> (bool, bool) {
    match art {
        "rahmen" => (false, true),
        "feld" => (true, false),
        "nichts" => (false, false),
        _ => (true, true),
    }
}

pub fn pegel(app: &AppHandle, wert: f32) {
    let _ = app.emit("zustand", json!({ "phase": "pegel", "wert": wert }));
}

#[cfg(test)]
mod tests {
    use super::sichtbar;

    #[test]
    fn anzeige_stufen() {
        assert_eq!(sichtbar("beides"), (true, true));
        assert_eq!(sichtbar("rahmen"), (false, true));
        assert_eq!(sichtbar("feld"), (true, false));
        assert_eq!(sichtbar("nichts"), (false, false));
        assert_eq!(sichtbar(""), (true, true), "alte Einstellungen ohne Wert zeigen alles");
    }
}
