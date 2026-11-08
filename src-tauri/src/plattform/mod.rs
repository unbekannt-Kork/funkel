//! Was je Betriebssystem anders ist: Tastatur abgreifen, Taste senden,
//! Einfügen, Freigaben, Fenster über allem.
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;

#[cfg(not(any(target_os = "macos", windows)))]
mod sonst;
#[cfg(not(any(target_os = "macos", windows)))]
pub use sonst::*;

/// Rückruf des Abgriffs: (Tastencode, mit Zusatztaste, gehalten) → verschlucken?
pub type Rueckruf = fn(u32, bool, bool) -> bool;
