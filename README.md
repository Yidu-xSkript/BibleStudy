# Bible Study

A modern desktop Bible-study app in Rust, inspired by BibleWorks 8. It is a personal, non-commercial project.

**It contains no Bible texts, lexicons or other BibleWorks content.** It reads the modules from *your own* BibleWorks 8 installation at runtime (read-only, never copied) and keeps your notes and lists in its own data folder.

## What it does

- **Read** English, Greek (Byzantine, NA27, LXX and others) and Hebrew (WTT, with optional cantillation), with verse-aligned side-by-side comparison across different versifications.
- **Search** English texts by word, phrase, prefix (`love*`) or Strong's number (`G25`, `H7225`).
- **Analyze** any Greek or Hebrew word: parsing, lemma, Strong's definition, and the full lexicons (Thayer, Friberg, Gingrich, Liddell-Scott, Louw-Nida, BDB, Holladay).
- **Notes** on verses and chapters, **verse lists** (with BibleWorks `.vls` import/export and list comparison) and **word lists** (frequency lists with part-of-speech filters).
- Light and dark themes.

## Layout

| Path | What |
| --- | --- |
| `core/` | Library: module readers, Greek/Hebrew decoders, versification and verse maps, morphology, lexicons, notes/lists. All logic and tests live here. |
| `app/` | Tauri shell: thin commands over `core`. |
| `ui/` | The interface: plain HTML, CSS and JavaScript, no build step. |
| `PRODUCT.md` | Product and design brief. |

## Build and run (Windows)

Requires Rust and, as set up here, the GNU toolchain with MinGW (the MSVC toolchain needs the Windows SDK):

```powershell
.\run.ps1
```

`run.ps1` puts `C:\msys64\mingw64\bin` on `PATH` and runs `cargo run -p bible-study`. `rust-toolchain.toml` pins the GNU toolchain.

By default the app looks for BibleWorks in `C:\Program Files (x86)\BibleWorks 8`; set `BW8_DIR` to use another folder. User data goes to `%APPDATA%\local.biblestudy.app` (override with `BS_DATA_DIR`).

## Tests

```powershell
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
cargo test --workspace
```

Tests that need real modules read your BibleWorks install and skip themselves when it isn't present.
