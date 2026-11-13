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
| 08.11.2026 | Erkennung während der Aufnahme in Stücken von gut 20 s, Schnitt an der leisesten Stelle | 76 s Ton: nach dem Stopp 1,1 s statt 3,8 s, Wörter 100 % gleich (Test `mitlauf_spart_zeit`) |
| 08.11.2026 | Aufnahme: Farbschein wie Apple Intelligence; Erkennung: klare grüne Kante, eine Farbe, wächst mit dem Fortschritt; Ende: Aufblitzen und kleiner Ton | Louis: der grüne Verlauf war „verwirrend, weil man nicht rafft, wann das der Fall ist“ |
| 08.11.2026 | Kante als SVG-Maske, Breite per Zeitgeber | ein Schatten mit 6000 px Ausdehnung zeichnet WebKit nicht; `requestAnimationFrame` ist im Randfenster gedrosselt |
| 08.11.2026 | Stille am Ende wird vor Whisper abgeschnitten, unhörbare Schlussabschnitte gestrichen | Whisper erfand Schlusssätze in der Stille; Regeln und Schwellen aus dem Vorgänger (gemessen 17.09.2026) |
| 08.11.2026 | Anzeige wählbar: Rahmen und Feld (Standard), nur Rahmen, nur Feld | Louis: „umschalten können … aber die Kombination, wie wir es jetzt haben, finde ich für mich gut“ |
| 08.11.2026 | Modus-Auswahl im Stil des „Hört zu“-Felds, aktiver Modus mit grünem Punkt | ein Design statt zweier |
| 08.11.2026 | Eigenes Signier-Zertifikat „Funkel Lokal“ plus Mikrofon-Berechtigung (audio-input) | Freigaben bleiben über Updates; die gehärtete Laufzeit liefert ohne Berechtigung nur Stille |
| 08.11.2026 | Windows vorerst ohne Sprechertrennung | sherpa-onnx statisch (/MT) und llama.cpp (/MD) linken dort nicht zusammen; dreimal versucht, abgegrenzt; Protokoll läuft dort ohne Sprecher. Offen |
| 09.11.2026 | Balken und Kante schätzen während der Erkennung selbst weiter (bis 95 %, Zeitkonstante 1,2 s), Whisper-Meldungen schieben nur nach vorn | Whisper meldet nur je 30-s-Fenster; der Rest nach dem Stopp ist kürzer, der Balken stand 1,4 s auf 0 % (Louis: „komische Pause“) |
| 09.11.2026 | Bei Sprache „automatisch“ wird die Sprache während der Aufnahme bestimmt (ab 2 s Ton, nur bei ≥ 80 % Sicherheit, höchstens drei Versuche) | die automatische Erkennung im Rest verdoppelte die Wartezeit nach dem Stopp: 1,32–1,61 s statt 0,66–0,83 s (gemessen, gleicher Text) |
| 09.11.2026 | Kein verkürztes Tonfenster (`audio_ctx`) | gemessen: verfälschte Wörter („Nagen“ statt „Morgen“) und Absturz; Flash-Attention brachte nur 0,05 s |
| 09.11.2026 | Balken und Kante laufen linear an einer Uhr über die erwartete Dauer aus Rust; kein Aufblitzen, kein Nachziehen am Ende | Louis: „gleichmäßig und sauber durchlaufen“; ersetzt die Schätzung vom selben Tag. Zurückspringen kam von `erkennt()`, das bei jeder Meldung neu startete |
| 09.11.2026 | Neues Logo: dunkle Kachel, scharfer Farbrahmen, roter Punkt; in der Menüleiste einfarbige Vorlage (Mac) | Louis: altes Logo „eine Katastrophe“; der verschwommene Ring war in der Menüleiste ein Farbfleck |
| 09.11.2026 | Klick auf das Menüleisten-Symbol öffnet Funkel, Rechtsklick das Menü | Louis will „oben draufklicken“ können |
| 09.11.2026 | Einstellungen im Aufbau der macOS-Systemeinstellungen: Gruppentitel über der Karte, gleich hohe Zeilen, einheitliche Feldbreite, Schalter | Louis: „Spacing … total beschissen“ |
| 09.11.2026 | 26 Sprachen; Füllwort-Vorlage nur für Deutsch/Englisch | mehr Sprachen gewünscht; deutsche Füllwörter in der Vorlage hätten andere Sprachen Richtung Deutsch gezogen |
| 09.11.2026 | Modus-Auswahl steht nach dem Einfügen 8 s statt 4,5 s | Louis: soll nach dem Einfügen „nicht direkt verpissen“; jede andere Taste beendet die Wahl weiterhin |
| 13.11.2026 | Wächter hängt den Tastatur-Abgriff neu an, wenn das System frische Anschläge meldet, beim Abgriff aber seit 30 s keiner ankam (nicht bei sicherer Eingabe) | Funkel reagierte am 12./13.11. eine Nacht lang nicht auf `^^`, obwohl der Abgriff laut System eingeschaltet war; nur ein Neustart half |
| 13.11.2026 | Schalter „Modi nach dem Diktat“ und Anzeige-Stufe „nichts“; Ton kommt aus dem Fenster des Diktats, auch ohne Feld | Louis will die Einblendung unten abschalten können; bei „nur Rahmen“ fehlte bisher der Ton |
| 13.11.2026 | Farbrand und grüne Kante mit einem Radius (28 px), konzentrisch; Maske des Farbrands einmal je Bildschirmgröße per Canvas in voller Auflösung, Kante per CSS | außen eckig und innen 33 px rund machte die Ecken dick und „abgeknackt“; das SVG mit Weichzeichner rechnete WebKit in halber Auflösung; die Kante baute je Bild eine neue Maske |
| 13.11.2026 | Balken bis 70 % gleichmäßig, danach ohne Knick abflachend Richtung 99 %, nie stehend; am Ende 180–420 ms bis voll; Schätzung wortgetreu 1,1 s statt 0,9 s | Louis: nach Stopp „wartet er kurz, bis er dann durchgeht“ — der Balken hielt bei 97 % an |
