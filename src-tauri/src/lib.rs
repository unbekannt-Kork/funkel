//! Funkel: zweimal eine Taste tippen, sprechen, einmal tippen — der Text liegt
//! in der Zwischenablage. Erkennung offline mit Whisper.
mod ablage;
mod ablauf;
mod aufnahme;
mod ausloeser;
mod einstellungen;
mod erkennung;
mod helfer;
mod mitlauf;
mod modell;
mod modus;
mod overlay;
mod plattform;

use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::MacosLauncher;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_autostart::ManagerExt;

use ablauf::Ablauf;
use einstellungen::{protokoll, Einstellungen};

#[derive(Serialize)]
struct Status {
    verlauf: Vec<ablage::Eintrag>,
    einstellungen: Einstellungen,
    taste_name: String,
    freigabe: bool,
    modell: bool,
    sprachmodell: bool,
    woerterbuch: String,
}

fn aktuelle_taste(werte: &Einstellungen) -> u32 {
    werte.taste.unwrap_or_else(plattform::standard_taste)
}

#[tauri::command]
fn status(ablauf: State<Arc<Ablauf>>) -> Status {
    let werte = ablauf.einstellungen.lock().unwrap().clone();
    let laufende = ablauf.laufende();
    let mut verlauf = ablage::eintraege(laufende.as_deref());
    verlauf.truncate(30);
    Status {
        verlauf,
        taste_name: plattform::tasten_name(aktuelle_taste(&werte)),
        freigabe: plattform::freigabe() && plattform::abgriff_wach_halten(),
        modell: modell::vorhanden(),
        sprachmodell: helfer::modell_vorhanden(),
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
fn wiederherstellen(ablauf: State<Arc<Ablauf>>, pfad: String) -> Result<(), String> {
    // Nur Dateien aus dem eigenen Ordner
    let p = std::path::PathBuf::from(&pfad);
    if !p.starts_with(ablage::ordner()) {
        return Err("Unbekannte Datei".into());
    }
    ablauf.inner().clone().wiederherstellen(p)
}

/// Aus dem Randfenster im Moment des Aufblitzens, damit Bild und Ton zusammenfallen
#[tauri::command]
fn ton_fertig(ablauf: State<Arc<Ablauf>>) {
    if ablauf.einstellungen.lock().unwrap().ton {
        plattform::ton_fertig();
    }
}

#[tauri::command]
fn kopieren(text: String) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string())
}

#[tauri::command]
fn ordner_oeffnen() {
    let ordner = ablage::ordner();
    let _ = std::fs::create_dir_all(&ordner);
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&ordner).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(&ordner).spawn();
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

#[tauri::command]
fn sprachmodell_laden(app: AppHandle) {
    std::thread::spawn(move || {
        let melder = app.clone();
        // Nacheinander; der Balken zeigt das größte (Sprachmodell), die kleinen laufen schnell durch
        let ergebnis = helfer::fehlende_modelle().into_iter().try_for_each(|(quelle, ziel, mindest)| {
            let melder = melder.clone();
            modell::laden(quelle, &ziel, mindest, move |geladen, gesamt| {
                let _ = melder.emit("sprachmodell-fortschritt", serde_json::json!({ "geladen": geladen, "gesamt": gesamt }));
            })
        });
        if let Err(e) = &ergebnis {
            protokoll(&format!("Sprachmodell: {e}"));
        }
        let _ = app.emit("sprachmodell-fertig", ergebnis.err());
    });
}

/// Mac: eigener LaunchAgent mit KeepAlive — nach Absturz startet Funkel nach 10 s neu,
/// „Funkel beenden“ im Menü (sauberes Ende) bleibt beendet.
#[cfg(target_os = "macos")]
fn autostart_setzen(_app: &AppHandle, an: bool) {
    let ordner = dirs::home_dir().unwrap_or_default().join("Library/LaunchAgents");
    let datei = ordner.join("de.theostudio.funkel.plist");
    // Alten Eintrag des Autostart-Plugins ablösen, sonst starten zwei
    let _ = std::fs::remove_file(ordner.join("Funkel.plist"));
    if !an {
        let _ = std::fs::remove_file(&datei);
        return;
    }
    let Ok(programm) = std::env::current_exe() else { return };
    let inhalt = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>de.theostudio.funkel</string>
  <key>ProgramArguments</key><array><string>{}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
        programm.display()
    );
    if std::fs::read_to_string(&datei).ok().as_deref() != Some(inhalt.as_str()) {
        let _ = std::fs::create_dir_all(&ordner);
        let _ = std::fs::write(&datei, inhalt);
    }
}

#[cfg(not(target_os = "macos"))]
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
        // Nur einmal: zwei Funkel hörten beide auf die Taste. Ein zweiter Start öffnet die Einstellungen.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| einstellungen_zeigen(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let werte = einstellungen::laden();
            let weg = ablage::aufraeumen(werte.aufbewahren_tage);
            if weg > 0 {
                protokoll(&format!("{weg} alte Aufnahme(n) nach {} Tagen gelöscht", werte.aufbewahren_tage));
            }
            let liegen = ablage::unfertige(None);
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
                    umwandeln: {
                        let a = ablauf.clone();
                        Box::new(move |n| {
                            if let Some(m) = modus::Modus::aus_nummer(n) {
                                a.umwandeln(m);
                            }
                        })
                    },
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
            let taste = aktuelle_taste(&werte);
            protokoll(&format!(
                "Start, Taste {} (Code {taste}), Tastatur {}",
                plattform::tasten_name(taste),
                if abgriff { "wird gelesen" } else { "ohne Freigabe" }
            ));

            // Modell im Hintergrund laden, damit das erste Diktat nicht wartet
            if modell::vorhanden() {
                let erkenner = ablauf.erkenner.clone();
                std::thread::spawn(move || {
                    if let Err(e) = erkenner.laden(&modell::pfad()) {
                        protokoll(&e);
                    }
                });
            }
            if !liegen.is_empty() {
                protokoll(&format!("{} Aufnahme(n) nicht fertig, zuletzt {}", liegen.len(), ablage::kurz(&liegen[0])));
            }
            if !abgriff || !modell::vorhanden() || !liegen.is_empty() {
                if !abgriff {
                    plattform::freigabe_anfragen();
                }
                einstellungen_zeigen(&handle);
            }
            // Wächter: Abgriff wach halten, neue Bildschirme versorgen
            let waechter = handle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                // Freigabe erst nach dem Start erteilt: jetzt anhängen, ohne Neustart
                if !plattform::abgriff_wach_halten() && plattform::freigabe() && ausloeser::neu_anhaengen() {
                    protokoll("Freigabe erteilt, Tastatur wird gelesen");
                    let _ = waechter.emit("status-geaendert", ());
                }
                let w = waechter.clone();
                let _ = waechter.run_on_main_thread(move || overlay::sicherstellen(&w));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            status,
            speichern,
            wiederherstellen,
            kopieren,
            ton_fertig,
            ordner_oeffnen,
            woerterbuch_speichern,
            freigabe_oeffnen,
            taste_lernen,
            modell_laden,
            sprachmodell_laden
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
