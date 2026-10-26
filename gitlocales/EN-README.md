[🇷🇺 Русский](RU-README.md) · [🇬🇧 English](EN-README.md) · [🇪🇸 Español](ES-README.md) · [🇩🇪 Deutsch](DE-README.md) · [🇫🇷 Français](FR-README.md) · [🇨🇳 中文](ZH-README.md)

---

# PlicZip

An archiver with its own `.plz` format: index at the end of the file, deflate/brotli compression, BLAKE3 on every file and block, password encryption, multi-volume archives, safe extraction. Written in Rust, with no C dependencies.

```text
$ pliczip a backup.plz ./documents -x9
$ pliczip a secret.plz ./documents --password=secret --volume-size 700M
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./restored
```

> Paths in the examples are placeholders — substitute your own directories. If a path
> doesn't exist, PlicZip says so plainly: `pliczip: error: path not found: ".\photos"`.

## Building

```
cargo build --release
# CLI:  target\release\pliczip.exe (2.7 MB)
# GUI:  target\release\pliczip-gui.exe (14.6 MB)
```

Pure Rust only: `flate2` (miniz_oxide), `brotli`, `chacha20poly1305`, `argon2` — no C code. That's why the build needs no MSVC `cl.exe` and works in paths with Cyrillic characters. The GUI uses egui/eframe, also with no C dependencies.

## Commands

| Command | Alias | What it does |
|---|---|---|
| `add <archive> <paths...>` | `a` | pack files and directories |
| `extract <archive>` | `x` | unpack |
| `list <archive>` | `l` | show contents |
| `test <archive>` | `t` | verify integrity without writing anything |
| `info <archive>` | `i` | archive information |

### Packing

```
pliczip a backup.plz ./documents ./notes     # your own directories, not placeholders
pliczip a -x9 backup.plz ./sources           # maximum compression
pliczip a --solid backup.plz ./sources       # glue small files together
pliczip a -x0 backup.plz ./video             # no compression, just a container
pliczip a -c "before vacation" backup.plz .  # with a comment
pliczip a backup.plz src --exclude "**/*.tmp" --exclude "target"
pliczip a src.plz src --include "**/*.rs"    # only the files you need
```

A directory is stored under its own name: `a x.plz photos` produces `photos/...` inside, just like 7-Zip.

### Compression levels

| Level ↕▾ | Method ↕▾ | When to use ↕▾ |
|---|---|---|
| −`-x0` | no compression | already compressed (video, jpg, other archives) |
| −`-x1..4` | deflate | fast, moderate |
| `-x5` (default) | brotli q5 | reasonable balance |
| `-x9..11` | brotli q9..q11 | slow, but text/code/logs compress noticeably better |
⚙

If compression doesn't shrink a block, it's stored as-is — the archive doesn't bloat.

### Filters

`--exclude` and `--include` take masks and can be repeated:

- `*` — any characters except `/`;
- `?` — exactly one character except `/`;
- `**` — any characters, including `/`.

Exclusion always beats inclusion. An excluded directory is not traversed at all: `--exclude target` on a large tree saves time, not just space.

### Progress

Packing, extraction, and verification show progress on a single stderr line:

```
 compress 812.4 MiB / 1.42 GiB  57%  186 MiB/s  remaining 4.3s
```

Speed and remaining time are computed over a five-second sliding window, not over the whole run: on mixed data, an average lies by orders of magnitude — the first seconds go to indexing and small files, then gigabytes follow. `-q` disables progress.

## Password protection

```
pliczip a secret.plz ./documents -p                  # asks for the password twice
pliczip a secret.plz ./documents --password="long password"
pliczip x secret.plz -o ./out --password="long password"
pliczip t secret.plz            # asks for the password interactively
```

- `-p` without a value prompts interactively — the password never lands in
shell history or the process list;
- `--password=…` only with an equals sign — otherwise `-p backup.plz` would
swallow the archive name as a password;
- `--password-file pw.txt` — the password is read from the file's first line;
- `PLICZIP_PASSWORD` — environment variable, handy for scripts.
- the key is derived via **Argon2id** (64 MiB, 3 passes) from a random salt;
- data and file names are encrypted with **ChaCha20-Poly1305** in 1 MiB chunks;
- the index is encrypted too, so even the file list is invisible without the password;
- a wrong password and a corrupted archive are indistinguishable from the message — no oracle;
- the password is not recoverable. Lose it — lose the archive.

Chunking matters: a 20 GB file is encrypted and decrypted as a stream, without occupying 20 GB of RAM.

## Volumes

```
pliczip a backup.plz ./photos --volume-size 700M   # produces backup.plz.001, backup.plz.002, …
pliczip x backup.plz -o ./out      # reads all volumes in sequence
```

Volumes are a plain byte split; the format inside is unchanged. The reader doesn't need to know the volume size — it concatenates files in order. Pointing at `.001` is enough; the rest is found automatically.

Lose a middle volume and the archive won't unpack: an explicit error is better than a silently incomplete result.

## Extraction

```
pliczip x backup.plz                     # into the current directory
pliczip x backup.plz -o ./restored
pliczip x backup.plz --skip-existing     # don't touch existing files
pliczip x backup.plz --no-verify         # skip hash verification (faster)
```

By default, checksums are verified and extraction stops with an error on mismatch. Modification times and unix permissions are restored (on Windows, the read-only flag).

## Machine output

`l`, `i`, and `t` understand `--json`:

```
pliczip l backup.plz --json
pliczip i backup.plz --json
pliczip t backup.plz --json     # {"ok": true, ...} or {"ok": false, "error": ...}
```

## GUI

```
pliczip-gui                    # empty window
pliczip-gui backup.plz         # open an archive (this is how double-click works)
```

An egui/eframe window, a separate binary in the same workspace, sharing the library with the CLI.

```
┌───────────────────────────────────────────────────────────┐
│ File  View  Help                                          │
├───────────────────────────────────────────────────────────┤
│ + Add │ Extract │ Verify │ Modify   [search]              │
├──────────────────────────────────────────┬────────────────┤
│ NAME        SIZE   IN ARCHIVE  METHOD …  │ SUMMARY        │
│ data/                                 —  │ Format  PLZ2   │
│   docs/readme.md        1.6 KiB  28 B brotli │ VOLUME      │
│   photo/IMG_0001.jpg    7.3 KiB  38 B brotli │ Before      │
│                                          │ Savings 99%    │
│                                          │ ┌───────────┐  │
│                                          │ │✓ Integrity │  │
│                                          │ │ confirmed  │  │
│                                          │ └───────────┘  │
├──────────────────────────────────────────┴────────────────┤
│ Done · 3 blocks · brotli/store · normal   8.9 KiB → 85 B  │
└───────────────────────────────────────────────────────────┘
```

What it can do:

- **opening** — via the menu, double-clicking a `.plz` (path as argument), or
drag-and-drop into the window; an encrypted archive prompts for a password;
- **contents table** — names, sizes, share in the archive, method, date;
search by name;
- **summary and integrity badge** — green "confirmed", red
"corrupted", gray "not checked": status is always visible, not only on
demand;
- **archive creation** — drag files in, a "fast / balanced / maximum" slider
with an explanation of consequences, solid mode, password, volume size,
comment;
- **extraction** — choose a directory, hash verification on by default;
- **progress with honest forecasting** — speed and remaining time are
computed over the last seconds, not averaged from the start;
- **settings** — theme (dark/light) and accent (brass/steel/green)
saved to `%APPDATA%/pliczip/config.json`.

Not yet done: modifying an existing archive (the "Modify" button).

### Explorer context menu (Windows)

Enable it in the GUI: **File → Add to Explorer context menu** (click again to remove). For installers, the same without a window:

```text
pliczip-gui --register-shell     # add; exit code 1 means an error
pliczip-gui --unregister-shell   # remove
```

A **PlicZip** submenu appears:

| Where | Item | What it does |
|---|---|---|
| `.plz` archive | double click / "Open in PlicZip" | opens the archive window |
| `.plz` archive | Extract here | into the archive's folder |
| `.plz` archive | Extract to a folder named after the archive | `backup.plz` → `backup\` |
| `.plz` archive | Test integrity | full check, badge in the window |
| files and folders | Add to archive… | create dialog with the selected items |
| files and folders | Compress to .plz | immediately, default settings, next to the source; an existing name is never overwritten — `name (2).plz` |

Encrypted archives ask for the password first. With several items selected, Explorer starts one process per item; they agree through a temp folder and a single window opens with the full list.

Everything is written to `HKCU\Software\Classes` — no admin rights, other users are not affected; removal deletes only PlicZip's own keys. On Windows 11 the items live under "Show more options" (Shift+F10); the top-level modern menu needs an `IExplorerCommand` COM handler and an MSIX package — that's the next step.

(Menu captions are currently in Russian, like the rest of the GUI.)

## Integrity

Four levels, all on BLAKE3:

```
file  ──hash──┐
              ├── block ──hash──┐
other files ──┘                 ├── index ──hash──┐
                                ┘                 ├── footer
index length ─────────────────────────────────────┘
```

- **file** — catches content corruption even if a block decompressed;
- **block** — catches corruption of the whole compressed stream;
- **index** — protects the file list: without it, files could be dropped from the
archive and it would still look "valid";
- **signature at the start and end** — catches a truncated file immediately, before parsing.

Encrypted archives add an AEAD tag on every chunk, and block hashes are computed on decrypted data — so verification works there too.

`pliczip t` verifies everything without creating a single file. Exit code `1` means corruption, `2` means other errors.

## Extraction safety

| Attack ↕▾ | Defense ↕▾ |
|---|---|
| −`../../secret` (zip-slip) | path policy: `..`, `.`, and empty segments are forbidden in the format |
| −absolute paths `/etc/…`, `C:\…` | forbidden at index validation |
| NTFS ADS (`file.txt:hidden`) | the `:` character is forbidden |
| reserved names (`CON`, `NUL`, `aux.txt`) | forbidden |
| `Report.txt` + `report.txt` | case-insensitive uniqueness |
| symlink `docs -> /etc` and writing through it | symlinks are created last, never pointing outward |
| a pre-planted directory symlink | path is checked after resolving the parent |
| a "bomb" of gigabytes | write size is bounded by the index, plus a compression-ratio limit |
⚙

Path validation is done twice: on write (so no junk is created) and on read (so no foreign junk is unpacked).

## Benchmarks

126 MiB of mixed data (76 MiB of text + 50 MiB incompressible), this machine, `--release`:

| Mode ↕▾ | Archive ↕▾ | Of original ↕▾ | Packing speed ↕▾ |
|---|---|---|---|
| −`-x1` deflate | 50.7 MiB | 40.2 % | 227 MiB/s |
| `-x5` brotli | 50.0 MiB | 39.6 % | 185 MiB/s |
| `-x9` brotli | 50.0 MiB | 39.6 % | 58 MiB/s |
⚙

2000 files of 8 KB each (33 MiB) — this is where solid mode matters:

| Mode ↕▾ | Archive ↕▾ | Compression ↕▾ | Time ↕▾ |
|---|---|---|---|
| −normal `-x5` | 0.72 MiB | 45× | 1.48 s |
| −`--solid -x5` | 0.32 MiB | 101× | 0.45 s |
| −`--solid -x9` | 0.32 MiB | 103× | 1.99 s |
⚙

Extraction is around 700 MiB/s; integrity verification is at about the same speed.

Encryption adds 16 bytes per megabyte of data and about 0.5 s for key derivation from the password (once per archive).

## Format

Full specification: [docs/format.md](https://docs/format.md).

```
"PLZ1" | data blocks | index (CBOR) | index length u64 | BLAKE3 of index | "PLZ1"
"PLZE" | KDF parameters | data blocks | index | footer          (encrypted)
```

The index at the end — like 7z: it allows writing the archive as a stream without knowing compressed sizes in advance. The cost is that reading requires `seek`, so the archive can't yet be read from a pipe.

The version 2 reader opens version 1 archives too — the format is backward compatible.

## Design

Identity and interface: [`design/pliczip-design.html`](https://design/pliczip-design.html) — a self-contained mockup (mark, palette, typography, CLI, main window, creation dialog, errors, ideas). Opens in a browser; the theme toggles with a button in the top right or with the `#light` link.

Decisions and rationale — in [`docs/design.md`](https://docs/design.md), the mark — [`design/mark.svg`](https://design/mark.svg), snapshots — `design/preview*.png`.

Principles in brief: the lightning bolt as a metaphor for the name, brass as the only accent, green and red reserved for integrity, and a permanent "Integrity confirmed" badge in the main window — because that is the one thing that essentially distinguishes PlicZip from 7-Zip.

## What's missing

An honest list, so you don't go looking:

- **recovery record** — a damaged sector can't be recovered (planned; this is a
difference from 7z and zip);
- **deduplication** — identical files are still stored separately, though the
hashes for it already exist;
- **stdin reading** — because of the index at the end;
- **non-UTF-8 names** — a path that can't be represented as UTF-8 isn't archived;
- **archive modification** — files can't be added to or removed from a
finished archive (the "Modify" button in the GUI is empty for now);
- **Linux/macOS file manager integration** — the context menu is Windows Explorer only, and on Windows 11 only in the classic menu;
- **archive digital signature**;
- **name-only encryption** without encrypting data — deliberately not done: it
creates a false sense of protection.

## Architecture

```
src/
  format.rs    container: signatures, index structures, footer, validation
  crypto.rs    Argon2id, ChaCha20-Poly1305, streaming encryptor and decryptor
  compress.rs  deflate/brotli, method selection, hashing reader
  safety.rs    path policy: normalization, zip-slip, Windows names
  glob.rs      --exclude / --include masks
  source.rs    byte source: single file or set of volumes
  writer.rs    filesystem traversal, block planning, parallel compression, volumes
  reader.rs    open, list, extract, integrity verification
  util.rs      sizes, progress with ETA, pluralization, UTF-8 in Windows console
  main.rs      CLI on clap
gui/
  main.rs      entry point, window launch
  app.rs       state: open archive, dialogs, background operations
  views.rs     main window: menu, toolbar, table, summary, status bar
  dialogs.rs   creation, extraction, password, error, progress
  theme.rs     palette from docs/design.md, accents
  config.rs    settings in JSON
  icon.rs      application mark, assembled in code
  progress.rs  progress channel and remaining-time estimation
tests/
  cli.rs       end-to-end tests through real processes
  security.rs  hand-assembled malicious archives
docs/format.md specification
design/        identity and interface mockup
legacy/        the YZ01 format prototype the project grew from
```

Progress goes through `ProgressSink`: the library reports progress, the CLI prints a line to stderr, the GUI fills a bar and estimates remaining time. That's why the window doesn't block during packing.

Memory is bounded: blocks up to 32 MiB are assembled in memory and compressed in parallel (rayon); anything larger is compressed as a stream straight to file. Same for encryption: 1 MiB chunks. A tens-of-gigabytes archive is packed and unpacked without occupying the same amount of memory.

## Development

```
cargo test --workspace   # unit, CLI, security, GUI, doc-test
cargo build --release
```

The library part is available separately:

```
use pliczip::prelude::*;

create(&archive, &[input], &CreateOptions::default())?;
let mut ar = Archive::open_with(&archive, Some("password"))?;
ar.extract(&ExtractOptions { output: dir, ..Default::default() })?;
```

License: MIT OR Apache-2.0.

