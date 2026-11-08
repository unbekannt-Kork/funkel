# Entscheidungen

| Datum | Entscheidung | Grund |
|---|---|---|
| 08.11.2026 | Name „Funkel“ | gewählt aus vier Vorschlägen; Bezug auf den leuchtenden Rand |
| 08.11.2026 | Tauri 2 (Rust + Web) statt Python | ein Code für Mac und Windows, App etwa 7 MB statt eines Python-Bündels |
| 08.11.2026 | whisper.cpp über `whisper-rs`, Metal auf dem Mac | offline, auf Apple-Chips etwa 1,4 s für 8 s Ton (gemessen im Release-Build) |
| 08.11.2026 | Modell large-v3-turbo q5_0 (574 MB) statt voll (1,6 GB) | ein Drittel des Downloads; Probesatz fehlerfrei erkannt |
| 08.11.2026 | Randfenster bleiben ständig offen, leer im Ruhezustand | ein Fenster erst beim Diktat zu zeigen, könnte der Schreibmarke den Fokus nehmen |
| 08.11.2026 | Rand als SVG-Maske je Stufe, überblendet | eine Maske je Stufe statt Neuberechnung in jedem Bild |
| 08.11.2026 | kein `requestAnimationFrame` im Randfenster | WebKit drosselt es in Fenstern, die es für verdeckt hält; das Feld blieb unsichtbar |
| 08.11.2026 | Ereignisse je Fenster (`getCurrentWebviewWindow().listen`) | `listen` aus `api/event` empfängt auch Ereignisse an andere Fenster |
| 08.11.2026 | Fn zählt auf dem Mac nicht als Zusatztaste | macOS setzt es bei jeder F-Taste, F-Tasten wären sonst als Auslöser unbrauchbar |
| 08.11.2026 | Windows: Taste über Scancode 0x29 | die Taste unter Escape, unabhängig vom Layout (`^` oder `` ` ``) |
| 08.11.2026 | Kein Textverlauf, Ton nie auf Platte | Datensparsamkeit (Art. 5 Abs. 1 lit. c DSGVO) |
| 08.11.2026 | MIT-Lizenz, Repo öffentlich | so gewünscht; Whisper und whisper.cpp stehen ebenfalls unter MIT |
| 08.11.2026 | Noch ohne Signatur (Apple, Windows) | kostet Geld (Apple 99 USD/Jahr), Entscheidung steht aus; bis dahin Hinweis im README |
| 08.11.2026 | Ton läuft während der Aufnahme auf die Platte, Text daneben, 7 Tage | keine Zeitgrenze, nichts geht bei Absturz verloren, Grundlage für späteres Umwandeln in andere Modi; ersetzt „kein Verlauf“ |
| 08.11.2026 | Modi: Whisper schreibt immer wortgetreu, das lokale Sprachmodell formt um | eine Erkennung für alle Modi; Umwandeln nachträglich kostet nur 1–3 s |
| 08.11.2026 | Sprachmodell Qwen3-4B-Instruct-2507 (Q4_K_M, 2,5 GB, Apache 2.0) | deutschfähig, auf dem M-Chip 0,9–2,4 s je Modus (docs/beispiele-modi.md) |
| 08.11.2026 | llama.cpp in eigenem Helfer-Prozess (`helfer/`) | whisper.cpp und llama.cpp bringen je ein eigenes ggml mit, in einem Programm kollidieren die Symbole |
| 08.11.2026 | Anweisungen mit festen Regeln und einem Beispiel je Modus, Füllreste per Regel | ohne das vertauschte das Modell die Person („schick ich“ → „schicken Sie“) und erfand Dank |
| 08.11.2026 | Schnellwahl mit Ziffern 1–5 für 4,5 s nach „Fertig“ | Modus wechseln ohne Maus; jede andere Taste beendet die Wahl, damit Tippen nie verschluckt wird |
| 08.11.2026 | Sprechertrennung mit sherpa-onnx im Helfer, direkt über `sherpa-rs-sys` | `sherpa-rs` setzt beide Modelle fest auf einen Kern: 24,4 s für 31 s Gespräch; mit allen Kernen 5,3 s |
| 08.11.2026 | Abschnitte von Whisper bekommen die Person mit der längsten Überlappung | einfach, nachvollziehbar, getestet; Pausen bleiben beim vorigen Sprecher |

