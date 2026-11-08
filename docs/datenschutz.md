# Datenschutz

Funkel verarbeitet Sprache ausschließlich auf dem Rechner, auf dem es läuft.

| Daten | Zweck | Wo | Wie lange |
|---|---|---|---|
| Ton vom Mikrofon | Text erkennen | nur im Arbeitsspeicher | bis die Erkennung fertig ist, dann verworfen; nie auf der Platte |
| erkannter Text | einfügen | Zwischenablage des Systems | bis etwas anderes kopiert wird; Funkel speichert keinen Verlauf |
| Einstellungen, Wörterbuch | Bedienung | Konfigurationsordner (`…/funkel/`) | bis zum Löschen durch die Person |
| Protokoll (`funkel.log`) | Fehler finden | Datenordner (`…/funkel/`) | wächst mit; enthält nur Abläufe und Zeiten, **nie Text oder Ton** |

**Netzwerk:** Ein einziger Abruf — das Sprachmodell wird einmalig von
`huggingface.co` geladen (Hugging Face Inc., USA). Dabei sieht Hugging Face die
IP-Adresse. Wer das vermeiden will, legt die Modelldatei
`ggml-large-v3-turbo-q5_0.bin` selbst in den Ordner `…/funkel/modelle/`.
Danach baut Funkel keine Verbindung mehr auf.

**Keine Dritten:** kein Konto, keine Telemetrie, keine Analyse, kein Server.
Damit entsteht keine Auftragsverarbeitung (Art. 28 DSGVO) durch Funkel.

**Löschen:** App entfernen und die Ordner `funkel` im Konfigurations- und
Datenordner löschen (Mac: `~/Library/Application Support/funkel`, Windows:
`%APPDATA%\funkel`).
