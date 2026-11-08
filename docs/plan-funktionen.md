# Plan: Funkel 0.2 — Modi, Protokolle, nichts geht verloren

Stand 08.11.2026. Ziel in einem Satz: **Nach dem Sprechen wählt man, was aus dem
Gesagten wird — wortgetreu, bereinigt, Mail, WhatsApp oder Protokoll mit mehreren
Personen — auch nachträglich, ohne Zeitgrenze, und keine Aufnahme geht verloren.**

Alles bleibt lokal: Whisper für die Sprache, ein kleines lokales Sprachmodell für das
Umformulieren, sherpa-onnx für die Sprechertrennung.

## Schritte, je mit Abnahme

Stand 08.11.2026: alle fünf umgesetzt und mit Tests ohne Bildschirm nachgewiesen (siehe docs/entscheidungen.md).

1. **Aufnahmen sichern, keine Zeitgrenze** — Ton geht während der Aufnahme
   laufend auf die Platte, der Text danach ebenfalls; nach einem Absturz meldet
   Funkel beim Start „Aufnahme von 13:40 (2:13 min) nicht fertig — wiederherstellen?“.
   Verlauf der letzten Texte im Einstellungsfenster. Aufbewahrung einstellbar,
   Standard 7 Tage.
   *Abnahme:* Aufnahme läuft, Prozess wird hart beendet, nach Neustart wird sie
   erkannt und wiederhergestellt; 60 min Aufnahme ohne Abbruch und ohne
   wachsenden Speicher.
2. **Modi und Schnellwahl** — vor oder nach dem Diktat wählbar: *wortgetreu*
   (jedes Wort, auch „äh“), *bereinigt* (Füllwörter weg, Sätze glatt, Inhalt
   unverändert), *Mail*, *WhatsApp*, *Protokoll*. Nach dem Fertig zeigt das Feld
   kurz die Modi; ein Klick oder eine Zifferntaste wandelt dieselbe Aufnahme um.
   *Abnahme:* ein erfundenes Diktat in allen fünf Modi, Ergebnisse im Repo.
3. **Lokales Sprachmodell** für bereinigt, Mail, WhatsApp, Protokoll —
   llama.cpp mit einem kleinen Modell, einmaliger Download wie Whisper.
   *Abnahme:* deutsche Beispieltexte, kein Inhalt erfunden, unter 5 s für eine
   Minute Diktat auf dem M-Chip.
4. **Protokoll mit mehreren Personen** — Sprechertrennung (sherpa-onnx),
   Namen zuweisbar, Ergebnis als Protokoll mit Teilnehmern, Punkten, Aufgaben.
   *Abnahme:* erfundenes Gespräch mit zwei Stimmen richtig getrennt.
5. **Lange Aufnahmen schnell** — Erkennung in Stücken schon während der
   Aufnahme (wie im Protokollanten), nach dem Stopp nur der Rest.
   *Abnahme:* 30 min Aufnahme, Text nach dem Stopp in unter 3 s.

## Datenschutz

Ton und Text liegen dafür befristet auf der Platte (`…/funkel/aufnahmen/`),
nur auf dem eigenen Rechner, Löschung nach der eingestellten Frist automatisch.
Eintrag in `docs/datenschutz.md` mit Schritt 1.
