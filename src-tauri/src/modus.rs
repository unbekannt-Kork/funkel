//! Was aus dem Gesagten wird. Whisper schreibt immer wortgetreu mit; die
//! übrigen Modi formen diesen Rohtext mit dem lokalen Sprachmodell um.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Modus {
    /// Jedes Wort, auch „äh“ und Abbrüche
    #[default]
    Wortgetreu,
    /// Füllwörter und Versprecher weg, Inhalt unverändert
    Bereinigt,
    Mail,
    Whatsapp,
    /// Gespräch mit zwei oder mehr Personen
    Protokoll,
}

pub const ALLE: [Modus; 5] = [Modus::Wortgetreu, Modus::Bereinigt, Modus::Mail, Modus::Whatsapp, Modus::Protokoll];

/// Füllwörter in der Vorlage bringen Whisper dazu, sie mitzuschreiben statt
/// glattzubügeln (übernommen aus dem Vorgänger, dort seit 10.09.2026 erprobt).
pub const STIL_DE: &str = "Ähm, also, ja, äh, ich mein, ne? Genau, ähm, warte mal, äh, also so ungefähr, ja genau, mhm, öhm, halt, quasi, sozusagen, weißt du, ne.";
pub const STIL_EN: &str = "Um, so, yeah, uh, I mean, right? Exactly, um, hold on, uh, like, sort of, you know, I guess, kind of, well, anyway, yeah.";

const GRUNDREGELN: &str = "Du bearbeitest ein gesprochenes Diktat, das automatisch mitgeschrieben wurde.
Regeln, ohne Ausnahme:
- Erfinde nichts dazu: keine Dankesformeln, keine Begründungen, keine Inhalte, die nicht gesagt wurden.
- Lass keine Aussage weg.
- Die sprechende Person bleibt dieselbe: aus „ich“ wird nie „wir“ oder „Sie“, aus „Ihnen“ nie „mir“.
- Zahlen, Uhrzeiten, Daten, Namen und Fachbegriffe übernimmst du genau so, wie sie gemeint sind.
- Füllwörter sind: äh, ähm, öhm, also, halt, quasi, sozusagen, ne, genau, ja (als Einstieg), das wär's eigentlich.
- Behalte die Sprache des Diktats bei.
- Antworte nur mit dem fertigen Text, ohne Einleitung, ohne Erklärung, ohne Anführungszeichen.";

impl Modus {
    pub fn name(self) -> &'static str {
        match self {
            Modus::Wortgetreu => "Wortgetreu",
            Modus::Bereinigt => "Bereinigt",
            Modus::Mail => "Mail",
            Modus::Whatsapp => "WhatsApp",
            Modus::Protokoll => "Protokoll",
        }
    }

    pub fn aus_nummer(n: u8) -> Option<Modus> {
        ALLE.get(n.checked_sub(1)? as usize).copied()
    }

    /// Anweisung an das Sprachmodell; `None` = Rohtext unverändert.
    pub fn anweisung(self) -> Option<String> {
        let auftrag = match self {
            Modus::Wortgetreu => return None,
            Modus::Bereinigt => "Aufgabe: Bereinige den Text. Entferne nur Füllwörter, Wiederholungen, Versprecher und \
abgebrochene Satzanfänge, setze Satzzeichen und Großschreibung, bilde Absätze bei Themenwechseln. \
Behalte Wortwahl und Satzbau sonst bei.

Beispiel:
Diktat: Ähm, also ich, äh, ich komm dann, ähm, am Freitag um drei vorbei, genau, und bring, äh, die Pläne mit.
Ergebnis: Ich komme dann am Freitag um drei vorbei und bringe die Pläne mit.",
            Modus::Mail => "Aufgabe: Forme den Text in eine E-Mail um. Anrede nur, wenn ein Empfänger genannt wird \
(z. B. „Guten Tag Frau Beispiel,“), dann kurze klare Absätze in der Ich-Form, am Ende nur „Viele Grüße“. \
Keine Betreffzeile, außer sie wurde diktiert.

Beispiel:
Diktat: Ähm, hallo Herr Muster, also ich, äh, ich komm dann am Freitag um drei vorbei, genau, und bring die Pläne mit.
Ergebnis: Guten Tag Herr Muster,

ich komme am Freitag um drei vorbei und bringe die Pläne mit.

Viele Grüße",
            Modus::Whatsapp => "Aufgabe: Forme den Text in eine kurze Chat-Nachricht in der Ich-Form um: locker, direkt, \
kurze Sätze, höchstens zwei kurze Absätze, keine Grußformel am Ende, keine Emojis.

Beispiel:
Diktat: Ähm, hallo Herr Muster, also ich, äh, ich komm dann am Freitag um drei vorbei, genau, und bring die Pläne mit.
Ergebnis: Hallo Herr Muster, ich komme Freitag um drei vorbei und bringe die Pläne mit.",
            Modus::Protokoll => "Aufgabe: Erstelle aus dem Gespräch ein Protokoll in Markdown mit diesen Abschnitten:
**Teilnehmer** — so wie im Text bezeichnet (Sprecher 1, Sprecher 2 oder genannte Namen).
**Themen** — Stichpunkte.
**Ergebnisse** — was entschieden oder festgestellt wurde; Zusagen so vorsichtig wie gesagt („versucht“ bleibt „versucht“).
**Aufgaben** — jede zugesagte oder angekündigte Aufgabe jeder Person, je Zeile: wer — was — bis wann.
Lass einen Abschnitt weg, wenn nichts dazu gesagt wurde.",
        };
        Some(format!("{GRUNDREGELN}\n\n{auftrag}"))
    }
}

/// Feste Nachbearbeitung nach dem Sprachmodell: Füllreste, die das Modell gern
/// stehen lässt, am Satzende („…, genau.“, „…, ne?“) und Schlusssätze wie
/// „Das wär's eigentlich.“ Regeln statt Modell, damit es immer greift.
pub fn nachputzen(text: &str) -> String {
    const SCHLUSS: [&str; 6] = ["das wär's eigentlich", "das wärs eigentlich", "das wäre es eigentlich", "das war's eigentlich", "das wär's", "das war's"];
    const ANHAENGSEL: [&str; 4] = [", genau", ", ne", ", oder so", ", ja"];
    let mut zeilen = Vec::new();
    for zeile in text.lines() {
        let mut saetze: Vec<String> = Vec::new();
        for satz in zeile.split_inclusive(['.', '!', '?']) {
            let roh = satz.trim();
            let ohne = roh.trim_end_matches(['.', '!', '?', ',']).trim().to_lowercase();
            let ohne = ohne.trim_end_matches(", ne").trim_end_matches(" ne");
            if SCHLUSS.contains(&ohne) {
                continue;
            }
            let mut t = satz.to_string();
            for a in ANHAENGSEL {
                for ende in [".", "!", "?"] {
                    t = t.replace(&format!("{a}{ende}"), ende);
                }
            }
            saetze.push(t);
        }
        zeilen.push(saetze.concat().trim_end().to_string());
    }
    zeilen.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn fuellreste_am_satzende_fliegen_raus() {
        assert_eq!(
            nachputzen("Die Unterlagen schick ich Ihnen bis Montag, genau. Das wär's eigentlich, ne."),
            "Die Unterlagen schick ich Ihnen bis Montag."
        );
        assert_eq!(nachputzen("Kommst du, ne? Gut."), "Kommst du? Gut.");
        assert_eq!(nachputzen("Hallo,\n\nich komme.\n\nViele Grüße"), "Hallo,\n\nich komme.\n\nViele Grüße");
    }

    #[test]
    fn genau_mitten_im_satz_bleibt() {
        assert_eq!(nachputzen("Das ist genau richtig."), "Das ist genau richtig.");
    }

    use super::*;

    #[test]
    fn nummern_eins_bis_fuenf() {
        assert_eq!(Modus::aus_nummer(1), Some(Modus::Wortgetreu));
        assert_eq!(Modus::aus_nummer(5), Some(Modus::Protokoll));
        assert_eq!(Modus::aus_nummer(0), None);
        assert_eq!(Modus::aus_nummer(6), None);
    }

    #[test]
    fn wortgetreu_braucht_kein_sprachmodell() {
        assert!(Modus::Wortgetreu.anweisung().is_none());
        assert!(ALLE[1..].iter().all(|m| m.anweisung().unwrap().contains("Erfinde nichts")));
    }

    #[test]
    fn name_in_den_einstellungen() {
        assert_eq!(serde_json::to_string(&Modus::Whatsapp).unwrap(), "\"whatsapp\"");
    }
}

/// Mit echtem Sprachmodell: `cargo test --release -- --ignored modi_mit_modell --nocapture`
/// Schreibt die Ergebnisse nach docs/beispiele-modi.md.
#[cfg(test)]
mod mit_modell {
    use super::*;

    const DIKTAT: &str = "Ja, also, ähm, hallo Frau Beispiel, äh, ich wollte nur kurz sagen, dass wir, ähm, \
den Termin am Dienstag, also am zwölften, äh, auf zehn Uhr verschieben müssen, weil, ähm, weil der Kollege \
da noch, äh, beim Kunden ist. Und, ähm, die Unterlagen schick ich Ihnen dann, äh, bis Montag, genau. \
Ja, das wär's eigentlich, ne.";

    const GESPRAECH: &str = "Sprecher 1: Also, ähm, wir müssen über das Angebot für die Firma Muster reden. \
Sprecher 2: Ja, äh, ich hab das durchgerechnet, wir liegen bei zwölftausend Euro. \
Sprecher 1: Okay, das ist zu hoch, ähm, kannst du das bis Freitag auf zehntausend drücken? \
Sprecher 2: Ich versuch's, ich schick dir das dann bis Freitag. \
Sprecher 1: Gut, und ich ruf den Kunden am Montag an.";

    #[test]
    #[ignore]
    fn modi_mit_modell() {
        let mut doku = String::from("# Beispiele: ein erfundenes Diktat in allen Modi\n\nErzeugt von `modi_mit_modell` (lokales Sprachmodell, gierig, ohne Zufall).\n");
        for m in ALLE {
            let eingabe = if m == Modus::Protokoll { GESPRAECH } else { DIKTAT };
            let beginn = std::time::Instant::now();
            let text = match m.anweisung() {
                None => eingabe.to_string(),
                Some(a) => nachputzen(&crate::helfer::umformen(&a, eingabe).expect("Umformen")),
            };
            let sek = beginn.elapsed().as_secs_f32();
            println!("== {} ({sek:.1} s)\n{text}\n", m.name());
            doku.push_str(&format!("\n## {} ({sek:.1} s)\n\n{}\n", m.name(), text));
            let klein = text.to_lowercase();
            if m != Modus::Protokoll {
                assert!(klein.contains("10") || klein.contains("zehn"), "Uhrzeit fehlt: {text}");
                assert!(klein.contains("montag"), "Montag fehlt: {text}");
            } else {
                assert!(klein.contains("freitag") && (klein.contains("10.000") || klein.contains("zehntausend") || klein.contains("10 000")), "{text}");
            }
            if matches!(m, Modus::Bereinigt | Modus::Mail | Modus::Whatsapp) {
                assert!(!klein.contains("ähm") && !klein.contains(" äh"), "Füllwörter übrig: {text}");
                // Die Person bleibt: „ich schick Ihnen“ darf nicht zu „Sie schicken“ werden
                assert!(klein.contains("ich") || klein.contains("schicke"), "Ich-Form verloren: {text}");
                assert!(!klein.contains("schicken sie") && !klein.contains("sie schicken"), "Person vertauscht: {text}");
                assert!(!klein.contains("dank"), "Dank erfunden: {text}");
                assert!(!klein.contains("genau.") && !klein.contains("eigentlich"), "Füllrest: {text}");
            }
            if m == Modus::Protokoll {
                assert!(klein.matches("montag").count() >= 1 && klein.contains("anruf"), "Aufgabe Anruf fehlt: {text}");
            }
        }
        doku.push_str(&format!("\n## Eingabe\n\n{DIKTAT}\n\n**Gespräch (für Protokoll):** {GESPRAECH}\n"));
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/beispiele-modi.md"), doku).unwrap();
    }
}
