//! Funkel: zweimal eine Taste tippen, sprechen, einmal tippen — der Text liegt
//! in der Zwischenablage. Erkennung offline mit Whisper.
mod ablauf;
mod aufnahme;
mod ausloeser;
mod einstellungen;
mod erkennung;
mod modell;
mod overlay;
mod plattform;

use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use ablauf::Ablauf;
use einstellungen::{protokoll, Einstellungen};

#[derive(Serialize)]
struct Status {
    einstellungen: Einstellungen,
    taste_name: String,
    freigabe: bool,
    modell: bool,
    woerterbuch: String,
}

fn aktuelle_taste(werte: &Einstellungen) -> u32 {
    werte.taste.unwrap_or_else(plattform::standard_taste)
}

#[tauri::command]
fn status(ablauf: State<Arc<Ablauf>>) -> Status {
    let werte = ablauf.einstellungen.lock().unwrap().clone();
    Status {
        taste_name: plattform::tasten_name(aktuelle_taste(&werte)),
        freigabe: plattform::freigabe() && plattform::abgriff_wach_halten(),
        modell: modell::vorhanden(),
        woerterbuch: einstellungen::woerterbuch(),
        einstellungen: werte,
    }
}

#[tauri::command]
fn speichern(app: AppHandle, ablauf: State<Arc<Ablauf>>, einstellungen: Einstellungen) -> Result<(), String> {
    einstellungen::speichern(&einstellungen)?;
    autostart_setzen(&app, einstellungen.autostart);
    ausloeser::taste_setzen(aktuelle_taste(&einstellungen));
    *ablauf.einstellungen.lock().unwrap() = einstellungen;
    Ok(())
}

#[tauri::command]
fn woerterbuch_speichern(text: String) -> Result<(), String> {
    einstellungen::woerterbuch_speichern(&text)
}

#[tauri::command]
fn freigabe_oeffnen() {
    plattform::freigabe_anfragen();
    plattform::freigabe_oeffnen();
}

#[tauri::command]
fn taste_lernen() {
    ausloeser::lernen();
}

#[tauri::command]
fn modell_laden(app: AppHandle, ablauf: State<Arc<Ablauf>>) {
    let erkenner = ablauf.erkenner.clone();
    std::thread::spawn(move || {
        let melder = app.clone();
        let ergebnis = modell::herunterladen(move |geladen, gesamt| {
            let _ = melder.emit("modell-fortschritt", serde_json::json!({ "geladen": geladen, "gesamt": gesamt }));
        })
        .and_then(|_| erkenner.laden(&modell::pfad()));
        if let Err(e) = &ergebnis {
            protokoll(&format!("Modell: {e}"));
        }
        let _ = app.emit("modell-fertig", ergebnis.err());
    });
}

fn autostart_setzen(app: &AppHandle, an: bool) {
    let start = app.autolaunch();
    let ist = start.is_enabled().unwrap_or(false);
    if an && !ist {
        let _ = start.enable();
    } else if !an && ist {
        let _ = start.disable();
    }
}

pub(crate) fn einstellungen_zeigen(app: &AppHandle) {
    if let Some(f) = app.get_webview_window("einstellungen") {
        let _ = f.show();
        let _ = f.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "einstellungen", WebviewUrl::App("einstellungen.html".into()))
        .title("Funkel")
        .inner_size(480.0, 720.0)
        .resizable(true)
        .build();
}

fn menue_bauen(app: &AppHandle) -> tauri::Result<()> {
    let werte = einstellungen::laden();
    let hinweis = MenuItem::with_id(
        app,
        "hinweis",
        format!("Zweimal {} tippen zum Diktieren", plattform::tasten_name(aktuelle_taste(&werte))),
        false,
        None::<&str>,
    )?;
    let einstellungen = MenuItem::with_id(app, "einstellungen", "Einstellungen …", true, None::<&str>)?;
    let beenden = MenuItem::with_id(app, "beenden", "Funkel beenden", true, None::<&str>)?;
    let menue = Menu::with_items(app, &[&hinweis, &PredefinedMenuItem::separator(app)?, &einstellungen, &beenden])?;
    TrayIconBuilder::with_id("funkel")
        .icon(app.default_window_icon().cloned().ok_or(tauri::Error::InvalidIcon(std::io::Error::other("kein Symbol")))?)
        .icon_as_template(false)
        .tooltip("Funkel")
        .menu(&menue)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "einstellungen" => einstellungen_zeigen(app),
            "beenden" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let werte = einstellungen::laden();
            let ablauf = Ablauf::neu(handle.clone(), werte.clone());
            app.manage(ablauf.clone());

            overlay::sicherstellen(&handle);
            menue_bauen(&handle)?;
            autostart_setzen(&handle, werte.autostart);

            let ausloeser_ablauf = ablauf.clone();
            let lern_app = handle.clone();
            let abgriff = ausloeser::starten(
                aktuelle_taste(&werte),
                werte.doppeltipp_ms,
                ausloeser::Rueckrufe {
                    ausloesen: Box::new(move || ausloeser_ablauf.umschalten()),
                    gelernt: Box::new(move |code| {
                        let ablauf = lern_app.state::<Arc<Ablauf>>();
                        let mut w = ablauf.einstellungen.lock().unwrap();
                        w.taste = Some(code);
                        let _ = einstellungen::speichern(&w);
                        let _ = lern_app.emit("status-geaendert", ());
                    }),
                    laeuft: ablauf.laeuft.clone(),
                },
            );
            protokoll(&format!("Start, Tastatur {}", if abgriff { "wird gelesen" } else { "ohne Freigabe" }));

            // Modell im Hintergrund laden, damit das erste Diktat nicht wartet
            if modell::vorhanden() {
                let erkenner = ablauf.erkenner.clone();
                std::thread::spawn(move || {
                    if let Err(e) = erkenner.laden(&modell::pfad()) {
                        protokoll(&e);
                    }
                });
            }
            if !abgriff || !modell::vorhanden() {
                if !abgriff {
                    plattform::freigabe_anfragen();
                }
                einstellungen_zeigen(&handle);
            }
            // Wächter: Abgriff wach halten, neue Bildschirme versorgen
            let waechter = handle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                plattform::abgriff_wach_halten();
                let w = waechter.clone();
                let _ = waechter.run_on_main_thread(move || overlay::sicherstellen(&w));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            status,
            speichern,
            woerterbuch_speichern,
            freigabe_oeffnen,
            taste_lernen,
            modell_laden
        ])
        .build(tauri::generate_context!())
        .expect("Funkel startet nicht")
        .run(|_app, ereignis| {
            // Schließen des Einstellungsfensters beendet Funkel nicht
            if let tauri::RunEvent::ExitRequested { api, code, .. } = ereignis {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
