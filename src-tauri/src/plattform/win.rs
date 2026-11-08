//! Windows: Low-Level-Tastaturhaken auf eigenem Faden, Tastendrücke über SendInput.
//! Die Taste wird über ihren Scancode erkannt — 0x29 ist die Taste unter Escape,
//! egal ob das Layout dort ^ (deutsch) oder ` (US) hat.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::Rueckruf;

/// Kennzeichen an selbst erzeugten Tastendrücken
const EIGEN: usize = 0x464B_4C31;
const UNTER_ESCAPE: u32 = 0x29;

static RUECKRUF: AtomicUsize = AtomicUsize::new(0);
static LAEUFT: AtomicBool = AtomicBool::new(false);
/// Zuletzt gedrückter Scancode, der noch nicht losgelassen wurde: so erkennt man Gedrückthalten
static UNTEN: AtomicUsize = AtomicUsize::new(usize::MAX);

fn gedrueckt(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

unsafe extern "system" fn haken(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let eigen = info.dwExtraInfo == EIGEN;
        let injiziert = info.flags.0 & LLKHF_INJECTED.0 != 0;
        let scan = info.scanCode;
        let nachricht = wparam.0 as u32;
        if nachricht == WM_KEYUP || nachricht == WM_SYSKEYUP {
            if UNTEN.load(Ordering::SeqCst) == scan as usize {
                UNTEN.store(usize::MAX, Ordering::SeqCst);
            }
        } else if (nachricht == WM_KEYDOWN || nachricht == WM_SYSKEYDOWN) && !eigen && !injiziert {
            let gehalten = UNTEN.swap(scan as usize, Ordering::SeqCst) == scan as usize;
            let zusatz = gedrueckt(VK_SHIFT) || gedrueckt(VK_CONTROL) || gedrueckt(VK_MENU)
                || gedrueckt(VK_LWIN) || gedrueckt(VK_RWIN);
            let f = RUECKRUF.load(Ordering::SeqCst);
            if f != 0 {
                let f: Rueckruf = std::mem::transmute(f);
                if f(scan, zusatz, gehalten) {
                    return LRESULT(1); // verschluckt
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

pub fn abgriff_starten(rueckruf: Rueckruf) -> bool {
    RUECKRUF.store(rueckruf as usize, Ordering::SeqCst);
    let (fertig_tx, fertig_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("funkel-tastatur".into())
        .spawn(move || unsafe {
            let modul = GetModuleHandleW(None).ok();
            let ergebnis = SetWindowsHookExW(WH_KEYBOARD_LL, Some(haken), modul.map(Into::into), 0);
            let _ = fertig_tx.send(ergebnis.is_ok());
            if ergebnis.is_err() {
                return;
            }
            LAEUFT.store(true, Ordering::SeqCst);
            // Der Haken lebt nur, solange dieser Faden Nachrichten abholt
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        })
        .ok();
    fertig_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or(false)
}

pub fn abgriff_wach_halten() -> bool {
    LAEUFT.load(Ordering::SeqCst)
}

fn eingabe(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: EIGEN },
        },
    }
}

pub fn taste_senden(scan: u32) {
    let s = scan as u16;
    let folge = [
        eingabe(VIRTUAL_KEY(0), s, KEYEVENTF_SCANCODE),
        eingabe(VIRTUAL_KEY(0), s, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP),
    ];
    unsafe { SendInput(&folge, std::mem::size_of::<INPUT>() as i32) };
}

pub fn einfuegen() {
    let folge = [
        eingabe(VK_CONTROL, 0, KEYBD_EVENT_FLAGS(0)),
        eingabe(VK_V, 0, KEYBD_EVENT_FLAGS(0)),
        eingabe(VK_V, 0, KEYEVENTF_KEYUP),
        eingabe(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ];
    unsafe { SendInput(&folge, std::mem::size_of::<INPUT>() as i32) };
}

/// Windows braucht keine Freigabe für den Tastaturhaken.
pub fn freigabe() -> bool {
    LAEUFT.load(Ordering::SeqCst)
}
pub fn freigabe_anfragen() {}
pub fn freigabe_oeffnen() {}

pub fn standard_taste() -> u32 {
    UNTER_ESCAPE
}

pub fn tasten_name(scan: u32) -> String {
    let mut puffer = [0u16; 64];
    let laenge = unsafe { GetKeyNameTextW((scan << 16) as i32, &mut puffer) };
    if laenge > 0 {
        String::from_utf16_lossy(&puffer[..laenge as usize])
    } else {
        format!("Taste {scan}")
    }
}

pub fn fenster_ueber_allem(fenster: &tauri::WebviewWindow) {
    let _ = fenster.set_ignore_cursor_events(true);
}
