[🇷🇺 Русский](RU-README.md) · [🇬🇧 English](EN-README.md) · [🇪🇸 Español](ES-README.md) · [🇩🇪 Deutsch](DE-README.md) · [🇫🇷 Français](FR-README.md) · [🇨🇳 中文](ZH-README.md)

---

# PlicZip

Ein Archivierer mit eigenem `.plz`-Format: Index am Dateiende, deflate/brotli-Kompression, BLAKE3 für jede Datei und jeden Block, Passwortverschlüsselung, mehrteilige Archive, sichere Entpackung. In Rust geschrieben, ohne C-Abhängigkeiten.

```text
$ pliczip a backup.plz ./dokumente -x9
$ pliczip a geheim.plz ./dokumente --password=geheim --volume-size 700M
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./wiederhergestellt
```

> Die Pfade in den Beispielen sind Platzhalter — ersetze sie durch deine eigenen Verzeichnisse. Existiert ein Pfad
> nicht, sagt PlicZip es direkt: `pliczip: error: Pfad nicht gefunden: ".\fotos"`.

## Build

```
cargo build --release
# CLI:  target\release\pliczip.exe (2.7 MB)
# GUI:  target\release\pliczip-gui.exe (14.6 MB)
```

Nur reines Rust: `flate2` (miniz_oxide), `brotli`, `chacha20poly1305`, `argon2` — kein C-Code. Deshalb benötigt der Build kein MSVC-`cl.exe` und funktioniert auch in Pfaden mit kyrillischen Zeichen. Die GUI basiert auf egui/eframe, ebenfalls ohne C-Abhängigkeiten.

## Befehle

| Befehl | Alias | Was er tut |
|---|---|---|
| `add <archiv> <pfade...>` | `a` | Dateien und Verzeichnisse packen |
| `extract <archiv>` | `x` | entpacken |
| `list <archiv>` | `l` | Inhalt anzeigen |
| `test <archiv>` | `t` | Integrität prüfen, ohne etwas zu schreiben |
| `info <archiv>` | `i` | Archivinformationen |

### Packen

```
pliczip a backup.plz ./dokumente ./notizen     # eigene Verzeichnisse, keine Platzhalter
pliczip a -x9 backup.plz ./quellen             # maximale Kompression
pliczip a --solid backup.plz ./quellen         # kleine Dateien zusammenkleben
pliczip a -x0 backup.plz ./video               # keine Kompression, nur Container
pliczip a -c "vor dem Urlaub" backup.plz .     # mit Kommentar
pliczip a backup.plz src --exclude "**/*.tmp" --exclude "target"
pliczip a src.plz src --include "**/*.rs"      # nur die benötigten Dateien
```

Ein Verzeichnis wird unter seinem eigenen Namen gespeichert: `a x.plz photos` erzeugt innen `photos/...`, wie bei 7-Zip.

### Kompressionsstufen

| Stufe | Methode | Wann verwenden |
|---|---|---|
| `-x0` | keine Kompression | bereits komprimiert (Video, JPG, andere Archive) |
| `-x1..4` | deflate | schnell, mäßig |
| `-x5` (Standard) | brotli q5 | vernünftiger Kompromiss |
| `-x9..11` | brotli q9..q11 | langsam, aber Text/Code/Logs komprimieren deutlich besser |

Wenn die Kompression einen Block nicht verkleinert, wird er unverändert gespeichert — das Archiv wird nicht aufgebläht.

### Filter

`--exclude` und `--include` akzeptieren Masken und können wiederholt werden:

- `*` — beliebige Zeichen außer `/`;
- `?` — genau ein Zeichen außer `/`;
- `**` — beliebige Zeichen, einschließlich `/`.

Ausschluss gewinnt immer gegen Einschluss. Ein ausgeschlossenes Verzeichnis wird überhaupt nicht durchlaufen: `--exclude target` auf einem großen Baum spart Zeit, nicht nur Platz.

### Fortschritt

Packen, Entpacken und Prüfen zeigen den Fortschritt in einer einzigen Zeile auf stderr:

```
 kompression 812.4 MiB / 1.42 GiB  57%  186 MiB/s  verbleibend 4.3s
```

Geschwindigkeit und Restzeit werden über ein gleitendes Fenster von fünf Sekunden berechnet, nicht über die gesamte Laufzeit: Bei gemischten Daten täuscht der Durchschnitt erheblich — die ersten Sekunden gehen für die Indexierung und kleine Dateien drauf, dann kommen Gigabytes. `-q` schaltet den Fortschritt ab.

## Passwortschutz

```
pliczip a geheim.plz ./dokumente -p                  # fragt zweimal nach dem Passwort
pliczip a geheim.plz ./dokumente --password="langes passwort"
pliczip x geheim.plz -o ./out --password="langes passwort"
pliczip t geheim.plz            # fragt interaktiv nach dem Passwort
```

- `-p` ohne Wert fragt interaktiv ab — das Passwort landet weder in der
Shell-Historie noch in der Prozessliste;
- `--password=…` nur mit Gleichheitszeichen — sonst würde `-p backup.plz` den
Archivnamen als Passwort verschlucken;
- `--password-file pw.txt` — Passwort aus der ersten Zeile der Datei;
- `PLICZIP_PASSWORD` — Umgebungsvariable, praktisch für Skripte.
- der Schlüssel wird über **Argon2id** (64 MiB, 3 Durchläufe) aus einem zufälligen Salt abgeleitet;
- Daten und Dateinamen werden mit **ChaCha20-Poly1305** in 1-MiB-Stücken verschlüsselt;
- der Index wird ebenfalls verschlüsselt, sodass selbst die Dateiliste ohne Passwort unsichtbar ist;
- falsches Passwort und beschädigtes Archiv sind an der Meldung nicht unterscheidbar — kein Orakel;
- das Passwort ist nicht wiederherstellbar. Verloren — Archiv verloren.

Stückelung ist wichtig: Eine 20-GB-Datei wird im Strom ver- und entschlüsselt, ohne 20 GB RAM zu belegen.

## Volumes

```
pliczip a backup.plz ./fotos --volume-size 700M   # erzeugt backup.plz.001, backup.plz.002, …
pliczip x backup.plz -o ./out      # liest alle Volumes der Reihe nach
```

Volumes sind ein einfaches Byte-Splitting; das Format im Inneren bleibt unverändert. Der Leser muss die Volume-Größe nicht kennen — er verkettet die Dateien der Reihe nach. Es reicht, `.001` anzugeben; der Rest wird automatisch gefunden.

Geht ein mittleres Volume verloren, lässt sich das Archiv nicht entpacken: ein expliziter Fehler ist besser als ein stillschweigend unvollständiges Ergebnis.

## Entpacken

```
pliczip x backup.plz                     # ins aktuelle Verzeichnis
pliczip x backup.plz -o ./wiederhergestellt
pliczip x backup.plz --skip-existing     # vorhandene Dateien nicht anfassen
pliczip x backup.plz --no-verify         # ohne Hash-Prüfung (schneller)
```

Standardmäßig werden Prüfsummen verifiziert, und bei Nichtübereinstimmung bricht das Entpacken mit Fehler ab. Änderungszeiten und Unix-Rechte werden wiederhergestellt (unter Windows das Nur-Lesen-Flag).

## Maschinenlesbare Ausgabe

`l`, `i` und `t` verstehen `--json`:

```
pliczip l backup.plz --json
pliczip i backup.plz --json
pliczip t backup.plz --json     # {"ok": true, ...} oder {"ok": false, "error": ...}
```

## Grafische Oberfläche

```
pliczip-gui                    # leeres Fenster
pliczip-gui backup.plz         # ein Archiv öffnen (so funktioniert der Doppelklick)
```

Ein egui/eframe-Fenster, separates Binary im selben Workspace, teilt die Bibliothek mit der CLI.

```
┌───────────────────────────────────────────────────────────┐
│ Datei  Ansicht  Hilfe                                     │
├───────────────────────────────────────────────────────────┤
│ + Hinzufügen │ Entpacken │ Prüfen │ Ändern   [suchen]     │
├──────────────────────────────────────────┬────────────────┤
│ NAME        GRÖSSE IM ARCHIV  METHODE …  │ ÜBERSICHT      │
│ data/                                 —  │ Format  PLZ2   │
│   docs/readme.md        1.6 KiB  28 B brotli │ VOLUMEN     │
│   photo/IMG_0001.jpg    7.3 KiB  38 B brotli │ Vorher      │
│                                          │ Ersparnis 99% │
│                                          │ ┌───────────┐  │
│                                          │ │✓ Integrität│ │
│                                          │ │ bestätigt │  │
│                                          │ └───────────┘  │
├──────────────────────────────────────────┴────────────────┤
│ Fertig · 3 Blöcke · brotli/store · normal  8.9 KiB → 85 B │
└───────────────────────────────────────────────────────────┘
```

Was es kann:

- **Öffnen** — über das Menü, Doppelklick auf `.plz` (Pfad als Argument) oder
Drag-and-drop ins Fenster; ein verschlüsseltes Archiv fragt nach dem Passwort;
- **Inhaltstabelle** — Namen, Größen, Anteil im Archiv, Methode, Datum;
Suche nach Namen;
- **Übersicht und Integritätsplakette** — grün „bestätigt", rot
„beschädigt", grau „nicht geprüft": der Status ist immer sichtbar, nicht erst
auf Anfrage;
- **Archiverstellung** — Dateien hineinziehen, Schieberegler „schnell / ausgewogen /
maximal" mit Erklärung der Folgen, solid, Passwort, Volume-Größe, Kommentar;
- **Entpacken** — Verzeichnisauswahl, Hash-Prüfung standardmäßig an;
- **Fortschritt mit ehrlicher Prognose** — Geschwindigkeit und Restzeit werden
über die letzten Sekunden berechnet, nicht ab Beginn gemittelt;
- **Einstellungen** — Theme (dunkel/hell) und Akzent (Messing/Stahl/Grün)
gespeichert in `%APPDATA%/pliczip/config.json`.

Noch nicht umgesetzt: Ändern eines bestehenden Archivs (Schaltfläche „Ändern"), `.plz`-Verknüpfung mit dem Explorer und Kontextmenü — das ist der nächste Block.

## Integrität

Vier Ebenen, alle mit BLAKE3:

```
datei  ──hash──┐
               ├── block ──hash──┐
andere dateien ┘                 ├── index ──hash──┐
                                 ┘                ├── fuß
index-länge ──────────────────────────────────────┘
```

- **Datei** — erkennt Inhaltsbeschädigung, auch wenn ein Block entpackt wurde;
- **Block** — erkennt Beschädigung des gesamten komprimierten Stroms;
- **Index** — schützt die Dateiliste: ohne ihn könnten Dateien aus dem Archiv
entfernt werden und es bliebe „gültig";
- **Signatur am Anfang und Ende** — erkennt eine abgeschnittene Datei sofort, vor dem Parsen.

In verschlüsselten Archiven kommt ein AEAD-Tag pro Stück hinzu, und Block-Hashes werden über die entschlüsselten Daten berechnet — die Prüfung funktioniert also auch dort.

`pliczip t` prüft alles, ohne eine einzige Datei zu erzeugen. Exit-Code `1` bedeutet Beschädigung, `2` andere Fehler.

## Entpackungssicherheit

| Angriff | Schutz |
|---|---|
| `../../secret` (zip-slip) | Pfadrichtlinie: `..`, `.`, leere Segmente im Format verboten |
| absolute Pfade `/etc/…`, `C:\…` | verboten bei der Indexvalidierung |
| NTFS-ADS (`file.txt:hidden`) | Zeichen `:` verboten |
| reservierte Namen (`CON`, `NUL`, `aux.txt`) | verboten |
| `Report.txt` + `report.txt` | Eindeutigkeit ohne Beachtung der Groß-/Kleinschreibung |
| Link `docs -> /etc` und Schreiben darüber | Links werden zuletzt erstellt, nie nach außen |
| vorab platzierter Verzeichnis-Link | Pfad wird nach Auflösung des Elternverzeichnisses geprüft |
| „Bombe" auf Gigabytes | Schreibmenge durch Index begrenzt, plus Kompressionsverhältnis-Limit |

Die Pfadvalidierung erfolgt zweimal: beim Schreiben (damit kein Müll entsteht) und beim Lesen (damit kein fremder Müll entpackt wird).

## Messungen

126 MiB gemischte Daten (76 MiB Text + 50 MiB inkompressibel), diese Maschine, `--release`:

| Modus | Archiv | Vom Original | Packgeschwindigkeit |
|---|---|---|---|
| `-x1` deflate | 50.7 MiB | 40.2 % | 227 MiB/s |
| `-x5` brotli | 50.0 MiB | 39.6 % | 185 MiB/s |
| `-x9` brotli | 50.0 MiB | 39.6 % | 58 MiB/s |

2000 Dateien à 8 KB (33 MiB) — hier entscheidet der Solid-Modus:

| Modus | Archiv | Kompression | Zeit |
|---|---|---|---|
| normal `-x5` | 0.72 MiB | 45× | 1.48 s |
| `--solid -x5` | 0.32 MiB | 101× | 0.45 s |
| `--solid -x9` | 0.32 MiB | 103× | 1.99 s |

Entpacken liegt bei rund 700 MiB/s; die Integritätsprüfung ist etwa gleich schnell.

Verschlüsselung fügt 16 Byte pro Megabyte Daten hinzu und etwa 0.5 s für die Schlüsselableitung aus dem Passwort (einmal pro Archiv).

## Format

Byte-genaues Layout: Moduldokumentation in [`src/format.rs`](../src/format.rs).

```
"PLZ1" | Datenblöcke | Index (CBOR) | Indexlänge u64 | BLAKE3 des Index | "PLZ1"
"PLZE" | KDF-Parameter | Datenblöcke | Index | Fuß            (verschlüsselt)
```

Index am Ende — wie bei 7z: ermöglicht das Schreiben des Archivs im Strom, ohne komprimierte Größen im Voraus zu kennen. Der Preis: Lesen erfordert `seek`, deshalb lässt sich das Archiv noch nicht aus einer Pipe lesen.

Der Leser der Version 2 öffnet auch Archive der Version 1 — das Format ist abwärtskompatibel.

## Design

Prinzipien kurz: der Blitz als Metapher des Namens, Messing als einziger Akzent, Grün und Rot für Integrität reserviert, und eine dauerhafte Plakette „Integrität bestätigt" im Hauptfenster — weil genau das PlicZip wesentlich von 7-Zip unterscheidet.

## Was noch fehlt

Eine ehrliche Liste, damit du nicht suchen musst:

- **recovery record** — ein beschädigter Sektor lässt sich nicht wiederherstellen (geplant; das ist der
Unterschied zu 7z und zip);
- **Deduplizierung** — identische Dateien werden noch separat gespeichert, obwohl die
Hashes dafür bereits vorhanden sind;
- **Lesen aus stdin** — wegen des Index am Ende;
- **Nicht-UTF-8-Namen** — ein Pfad, der nicht als UTF-8 darstellbar ist, wird nicht archiviert;
- **Archivänderung** — Dateien können einem fertigen Archiv weder hinzugefügt noch
daraus entfernt werden (die Schaltfläche „Ändern" in der GUI ist vorerst leer);
- **Explorer-Integration** — `.plz`-Verknüpfung und Kontextmenü fehlen noch;
- **digitale Signatur** des Archivs;
- **Verschlüsselung nur der Namen** ohne Datenverschlüsselung — bewusst nicht gemacht: erzeugt
ein falsches Gefühl von Schutz.

## Aufbau

```
src/
  format.rs    Container: Signaturen, Indexstrukturen, Fuß, Validierung
  crypto.rs    Argon2id, ChaCha20-Poly1305, Strom-Ver- und Entschlüsseler
  compress.rs  deflate/brotli, Methodenauswahl, hashender Leser
  safety.rs    Pfadrichtlinie: Normalisierung, zip-slip, Windows-Namen
  glob.rs      --exclude / --include Masken
  source.rs    Bytequelle: einzelne Datei oder Menge von Volumes
  writer.rs    FS-Durchlauf, Blockplanung, parallele Kompression, Volumes
  reader.rs    Öffnen, Auflisten, Entpacken, Integritätsprüfung
  util.rs      Größen, Fortschritt mit ETA, Pluralisierung, UTF-8 in Windows-Konsole
  main.rs      CLI auf clap
gui/
  main.rs      Einstiegspunkt, Fensterstart
  app.rs       Zustand: geöffnetes Archiv, Dialoge, Hintergrundoperationen
  views.rs     Hauptfenster: Menü, Werkzeugleiste, Tabelle, Übersicht, Statusleiste
  dialogs.rs   Erstellung, Entpacken, Passwort, Fehler, Fortschritt
  theme.rs     Palette, Akzente
  config.rs    Einstellungen in JSON
  icon.rs      Anwendungszeichen, im Code zusammengesetzt
  progress.rs  Fortschrittskanal und Restzeitschätzung
tests/
  cli.rs       End-to-End-Tests über echte Prozesse
  security.rs  manuell zusammengesetzte bösartige Archive
legacy/        Prototyp des Formats YZ01, aus dem das Projekt entstand
```

Fortschritt läuft über `ProgressSink`: die Bibliothek meldet, die CLI druckt eine Zeile auf stderr, die GUI füllt einen Balken und schätzt die Restzeit. Deshalb blockiert das Fenster während des Packens nicht.

Speicher ist begrenzt: Blöcke bis 32 MiB werden im Speicher zusammengesetzt und parallel komprimiert (rayon); alles Größere wird im Strom direkt in die Datei komprimiert. Ebenso bei der Verschlüsselung: 1-MiB-Stücke. Ein Archiv von Dutzenden Gigabytes wird gepackt und entpackt, ohne ebenso viel Speicher zu belegen.

## Entwicklung

```
cargo test --workspace   # 104 Tests: Unit, CLI, Sicherheit, GUI, Doctest
cargo build --release
```

Der Bibliotheksteil ist separat verfügbar:

```
use pliczip::prelude::*;

create(&archive, &[input], &CreateOptions::default())?;
let mut ar = Archive::open_with(&archive, Some("passwort"))?;
ar.extract(&ExtractOptions { output: dir, ..Default::default() })?;
```

Lizenz: [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).

