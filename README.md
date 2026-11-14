# Funkel

**Zweimal tippen, sprechen, einmal tippen — der Text ist da.** Diktieren in
jedem Programm, auf Mac und Windows, komplett offline.

*Dictate into any app on macOS and Windows: double-tap a key, speak, tap once.
Speech recognition runs locally with Whisper — nothing leaves your computer.*

- **Offline:** Whisper (large-v3-turbo) erkennt auf dem eigenen Rechner. Kein
  Konto, kein Server, kein Ton und kein Text verlässt den Rechner.
- **Man sieht, dass es zuhört:** Ein weicher Farbrand läuft um den Bildschirm,
  unten erscheint ein kleines Feld mit Laufzeit. Bei der Erkennung wird der Rand
  grün und das Feld zum Balken, am Ende steht „✓ Fertig“.
- **Deutsch und Englisch** werden automatisch erkannt, andere Sprachen fest einstellbar.
- **Wörterbuch** für Namen und Fachbegriffe, damit sie richtig geschrieben werden.

## Installieren

Unter [Releases](../../releases) liegen:

| System | Datei |
|---|---|
| Mac mit Apple-Chip (M1 und neuer), macOS 13+ | `Funkel_x.y.z_aarch64.dmg` |
| Windows 10/11 | `Funkel_x.y.z_x64-setup.exe` |

Beim ersten Start lädt Funkel einmalig das Sprachmodell (574 MB). Danach läuft
alles offline.

**Mac:** Die App ist noch nicht bei Apple beglaubigt. Beim ersten Öffnen
deshalb Rechtsklick auf Funkel → *Öffnen* → *Öffnen*. Danach zwei Freigaben,
beide nur einmal: *Bedienungshilfen* (damit Funkel die Taste sieht) und
*Mikrofon*.

**Windows:** Der Installer ist noch nicht signiert. Erscheint „Der Computer
wurde durch Windows geschützt“: *Weitere Informationen* → *Trotzdem ausführen*.

## Bedienen

| | |
|---|---|
| Diktat starten | zweimal die Taste unter Escape (`^` auf deutschen, `` ` `` auf US-Tastaturen) |
| Diktat beenden | einmal dieselbe Taste |
| Ergebnis | Text in der Zwischenablage und sofort an der Schreibmarke eingefügt |

Ein einzelner Tipp schreibt weiter das normale Zeichen. Taste, Sprache,
Einfügen, Autostart und Wörterbuch stehen in den Einstellungen (Symbol in der
Menüleiste bzw. im Infobereich).

## Entwickeln

Voraussetzungen: Node 22+, Rust (stable), CMake. Auf Windows zusätzlich
LLVM/Clang (für whisper.cpp).

```sh
npm install
npm run tauri dev        # starten
npm run pruefen          # Typen, Build, Tests, Clippy
npm run tauri build      # .app/.dmg bzw. .exe/.msi bauen
```

Test der Erkennung mit echtem Modell und erfundenem Ton (Mac):

```sh
say -o /tmp/probe.wav --data-format=LEI16@16000 "Morgen um zehn Uhr treffen wir uns."
FUNKEL_TESTTON=/tmp/probe.wav cargo test --release --manifest-path src-tauri/Cargo.toml -- --ignored
```

## Ausliefern

Ein Tag `vX.Y.Z` baut über GitHub Actions `.dmg` und Windows-Installer und legt
sie als Release-Entwurf ab (`.github/workflows/veroeffentlichen.yml`).

## Aufbau

```
overlay.html, src/overlay.*        Rand und Feld, ein durchsichtiges Fenster je Bildschirm
einstellungen.html, src/einstell…  Einstellungsfenster
src-tauri/src/ausloeser.rs         Doppeltipp-Logik (getestet)
src-tauri/src/plattform/           Tastatur, Einfügen, Freigaben: mac.rs, win.rs
src-tauri/src/aufnahme.rs          Mikrofon, nie virtuelle Geräte, Ton nur im Speicher
src-tauri/src/erkennung.rs         Whisper, Modell bleibt geladen
src-tauri/src/modell.rs            einmaliger Download des Modells
src-tauri/src/ablauf.rs            bereit → hört zu → erkennt → fertig
```

Datenschutz: [docs/datenschutz.md](docs/datenschutz.md) ·
Entscheidungen: [docs/entscheidungen.md](docs/entscheidungen.md)

## Lizenz

MIT. Das Sprachmodell (OpenAI Whisper, über whisper.cpp) steht ebenfalls unter MIT.
