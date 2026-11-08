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
