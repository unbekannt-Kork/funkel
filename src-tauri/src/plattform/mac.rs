//! macOS: CGEventTap auf eigenem Faden, Tastendrücke über CGEventPost.
#![allow(non_upper_case_globals, non_snake_case)]
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use super::Rueckruf;

type CGEventRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CGEventSourceRef = *mut c_void;

const kCGSessionEventTap: u32 = 1;
const kCGHIDEventTap: u32 = 0;
const kCGHeadInsertEventTap: u32 = 0;
const kCGEventTapOptionDefault: u32 = 0;
const kCGEventKeyDown: u32 = 10;
const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;
const kCGKeyboardEventAutorepeat: u32 = 8;
const kCGKeyboardEventKeycode: u32 = 9;
const kCGEventSourceUserData: u32 = 42;
const kCGEventFlagMaskShift: u64 = 0x0002_0000;
const kCGEventFlagMaskControl: u64 = 0x0004_0000;
const kCGEventFlagMaskAlternate: u64 = 0x0008_0000;
const kCGEventFlagMaskCommand: u64 = 0x0010_0000;
const kCGEventSourceStateHIDSystemState: i32 = 1;
/// Fn zählt nicht: macOS setzt es bei jeder Funktionstaste (F1–F19) von selbst
const STOERER: u64 = kCGEventFlagMaskShift | kCGEventFlagMaskControl | kCGEventFlagMaskAlternate | kCGEventFlagMaskCommand;
/// Kennzeichen an selbst erzeugten Tastendrücken, damit wir sie nicht wieder abfangen
const EIGEN: i64 = 0x464B_4C31;
const TASTE_V: u16 = 9;

type TapRueckruf = extern "C" fn(*mut c_void, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapRueckruf, user: *mut c_void) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventTapIsEnabled(tap: CFMachPortRef) -> bool;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventCreateKeyboardEvent(source: CGEventSourceRef, key: u16, down: bool) -> CGEventRef;
    fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
    fn CFMachPortIsValid(port: CFMachPortRef) -> bool;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: *const c_void);
    fn CFRunLoopRun();
    fn CFRelease(cf: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn LMGetKbdType() -> u8;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *const c_void;
    fn TISGetInputSourceProperty(quelle: *const c_void, schluessel: *const c_void) -> *const c_void;
    fn UCKeyTranslate(
        layout: *const c_void,
        code: u16,
        aktion: u16,
        zusatz: u32,
        tastatur: u32,
        optionen: u32,
        tot: *mut u32,
        max: usize,
        laenge: *mut usize,
        zeichen: *mut u16,
    ) -> i32;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataGetBytePtr(daten: *const c_void) -> *const c_void;
}

/// Welches Zeichen liefert diese Taste im aktuellen Layout (ohne tote Tasten)?
/// Muss im Hauptfaden laufen.
fn zeichen_von(code: u16) -> Option<char> {
    unsafe {
        let quelle = TISCopyCurrentKeyboardLayoutInputSource();
        if quelle.is_null() {
            return None;
        }
        let daten = TISGetInputSourceProperty(quelle, kTISPropertyUnicodeKeyLayoutData);
        let ergebnis = if daten.is_null() {
            None
        } else {
            let (mut tot, mut laenge, mut puffer) = (0u32, 0usize, [0u16; 4]);
            // Aktion 0 = gedrückt, Option 1 = tote Tasten liefern ihr Zeichen sofort
            let s = UCKeyTranslate(CFDataGetBytePtr(daten), code, 0, 0, LMGetKbdType() as u32, 1, &mut tot, 4, &mut laenge, puffer.as_mut_ptr());
            (s == 0 && laenge > 0).then(|| char::decode_utf16(puffer[..laenge].iter().copied()).next()?.ok()).flatten()
        };
        CFRelease(quelle);
        ergebnis
    }
}

static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static RUECKRUF: AtomicUsize = AtomicUsize::new(0);

extern "C" fn tap_rueckruf(_proxy: *mut c_void, typ: u32, event: CGEventRef, _user: *mut c_void) -> CGEventRef {
    unsafe {
        if typ == kCGEventTapDisabledByTimeout || typ == kCGEventTapDisabledByUserInput {
            // macOS schaltet einen trägen Abgriff ab — sofort wieder an
            let tap = TAP.load(Ordering::SeqCst);
            if !tap.is_null() {
                CGEventTapEnable(tap, true);
            }
            return event;
        }
        if typ != kCGEventKeyDown || CGEventGetIntegerValueField(event, kCGEventSourceUserData) == EIGEN {
            return event;
        }
        let code = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode) as u32;
        let zusatz = CGEventGetFlags(event) & STOERER != 0;
        let gehalten = CGEventGetIntegerValueField(event, kCGKeyboardEventAutorepeat) != 0;
        let f = RUECKRUF.load(Ordering::SeqCst);
        if f == 0 {
            return event;
        }
        let f: Rueckruf = std::mem::transmute(f);
        if f(code, zusatz, gehalten) {
            std::ptr::null_mut() // verschluckt
        } else {
            event
        }
    }
}

pub fn abgriff_starten(rueckruf: Rueckruf) -> bool {
    RUECKRUF.store(rueckruf as usize, Ordering::SeqCst);
    let (fertig_tx, fertig_rx) = std::sync::mpsc::channel();
    // Eigener Faden mit eigener Schleife: jeder Anschlag am ganzen Mac wartet
    // auf diesen Rückruf, er darf nie an der Oberfläche hängen.
    std::thread::Builder::new()
        .name("funkel-tastatur".into())
        .spawn(move || unsafe {
            let tap = CGEventTapCreate(
                kCGSessionEventTap,
                kCGHeadInsertEventTap,
                kCGEventTapOptionDefault,
                1 << kCGEventKeyDown,
                tap_rueckruf,
                std::ptr::null_mut(),
            );
            if tap.is_null() {
                let _ = fertig_tx.send(false);
                return;
            }
            TAP.store(tap, Ordering::SeqCst);
            let quelle = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), quelle, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            let _ = fertig_tx.send(true);
            CFRunLoopRun();
        })
        .ok();
    fertig_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or(false)
}

/// Wächter: abgeschaltet → wieder an. Liefert `false`, wenn der Abgriff fehlt oder ungültig ist.
pub fn abgriff_wach_halten() -> bool {
    let tap = TAP.load(Ordering::SeqCst);
    if tap.is_null() {
        return false;
    }
    unsafe {
        if !CFMachPortIsValid(tap) {
            return false;
        }
        if !CGEventTapIsEnabled(tap) {
            CGEventTapEnable(tap, true);
        }
    }
    true
}

fn senden(code: u16, flags: u64) {
    unsafe {
        let quelle = CGEventSourceCreate(kCGEventSourceStateHIDSystemState);
        for unten in [true, false] {
            let e = CGEventCreateKeyboardEvent(quelle, code, unten);
            if e.is_null() {
                continue;
            }
            CGEventSetIntegerValueField(e, kCGEventSourceUserData, EIGEN);
            if flags != 0 {
                CGEventSetFlags(e, flags);
            }
            CGEventPost(kCGHIDEventTap, e);
            CFRelease(e);
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        if !quelle.is_null() {
            CFRelease(quelle);
        }
    }
}

pub fn taste_senden(code: u32) {
    senden(code as u16, 0);
}

pub fn einfuegen() {
    senden(TASTE_V, kCGEventFlagMaskCommand);
}

/// Kleiner Ton, wenn der Text fertig ist (Systemklang „Glass“, leise)
pub fn ton_fertig() {
    let _ = std::process::Command::new("afplay").args(["-v", "0.35", "/System/Library/Sounds/Glass.aiff"]).spawn();
}

pub fn freigabe() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Zeigt den Systemdialog „Funkel möchte diesen Computer steuern“.
pub fn freigabe_anfragen() {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    unsafe {
        let schluessel = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt as _);
        let optionen = CFDictionary::from_CFType_pairs(&[(schluessel, CFBoolean::true_value())]);
        AXIsProcessTrustedWithOptions(optionen.as_concrete_TypeRef() as *const c_void);
    }
}

pub fn freigabe_oeffnen() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

/// Die Taste unter Escape, aus dem Tastaturlayout gelesen statt geraten:
/// deutsch `^` auf Code 10, US `` ` `` auf Code 50. Die Bauart (ISO/ANSI), die
/// macOS meldet, taugt nicht: eine deutsche Tastatur kam als „ANSI“ an, Funkel
/// hörte auf Code 50 und `^^` tat nichts (08.11.2026). Einmal ermittelt, dann gemerkt.
pub fn standard_taste() -> u32 {
    static TASTE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *TASTE.get_or_init(|| match (zeichen_von(10), zeichen_von(50)) {
        (Some('^' | '`'), _) => 10,
        (_, Some('^' | '`')) => 50,
        _ => 10,
    })
}

/// Ziffern 1–5 der oberen Reihe (für die Schnellwahl der Modi)
pub fn ziffer(code: u32) -> Option<u8> {
    match code {
        18 => Some(1),
        19 => Some(2),
        20 => Some(3),
        21 => Some(4),
        23 => Some(5),
        _ => None,
    }
}

pub fn tasten_name(code: u32) -> String {
    let name = match code {
        10 => "^",
        50 => "`",
        48 => "⇥",
        53 => "esc",
        57 => "⇪",
        63 => "fn",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        99 => "F3",
        100 => "F8",
        101 => "F9",
        103 => "F11",
        105 => "F13",
        107 => "F14",
        109 => "F10",
        111 => "F12",
        113 => "F15",
        118 => "F4",
        120 => "F2",
        122 => "F1",
        _ => return format!("Taste {code}"),
    };
    name.into()
}

/// Über der Menüleiste und Vollbildprogrammen, auf allen Spaces, nie im Weg.
pub fn fenster_ueber_allem(fenster: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let Ok(zeiger) = fenster.ns_window() else { return };
    unsafe {
        let w = &*(zeiger as *mut AnyObject);
        // NSPopUpMenuWindowLevel: über der Menüleiste
        let _: () = msg_send![w, setLevel: 101isize];
        // alle Spaces | stehen bleiben | neben Vollbild | nicht in der Fensterliste
        let _: () = msg_send![w, setCollectionBehavior: (1usize | 16 | 64 | 256)];
        let _: () = msg_send![w, setIgnoresMouseEvents: true];
        let _: () = msg_send![w, setHasShadow: false];
    }
}
