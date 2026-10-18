# Bible Study

A modern desktop Bible-study app in Rust, inspired by BibleWorks 8. It is a personal, non-commercial project.

**It contains no Bible texts, lexicons or other BibleWorks content.** It reads the modules from *your own* BibleWorks 8 installation at runtime (read-only, never copied) and keeps your notes and lists in its own data folder.

## What it does

- **Read** English, Greek (Byzantine, NA27, LXX and others) and Hebrew (WTT, with optional cantillation), with verse-aligned side-by-side comparison across different versifications.
- **Search** English, Greek and Hebrew (see below).
- **Analyze** any Greek or Hebrew word: parsing, lemma, Strong's definition, and the full lexicons (Thayer, Friberg, Gingrich, Liddell-Scott, Louw-Nida, BDB, Holladay).
- **Cross-references** from the Treasury of Scripture Knowledge for any verse in any version (Refs tab): grouped by phrase, one click to follow, optional verse text in the version you are reading, chapter outlines, and "save as verse list".
- **Notes** on verses and chapters, **verse lists** (with BibleWorks `.vls` import/export and list comparison) and **word lists** (frequency lists with part-of-speech filters).
- Light and dark themes.

## Search syntax

Open the search box with <kbd>Ctrl</kbd>+<kbd>K</kbd> (or `/`). It works the same in every version:

| Type | Finds |
| --- | --- |
| `love*` `wom?n` `*belie*` | wildcards: `*` any run of letters, `?` one letter, anywhere in a word |
| `"so loved"` | a phrase of consecutive words |
| `faith hope` | verses containing all the words (start with `/` for any of them, `'` for one phrase) |
| `G25` `H7225` | a Strong's number |

In Greek and Hebrew versions, also:

| Type | Finds |
| --- | --- |
| `λόγος` `בראשית` | a word form. Accents, breathings, vowel points and cantillation are ignored unless you type them. The **αβ** button opens a letter keypad. |
| `logos` | Latin letters are read as Beta Code (Greek) or Westminster letters (Hebrew) |
| `ἀγαπάω@v*` `ברא@vq*` | lemma plus morphology code (either side may be `*`; the code has an implied `*` at the end). `ἀγαπάω@*` finds every form. |

Hebrew prefixes (ה, ו, ב …) stay attached to the word, so use `lemma@*` to find all forms. A lemma search includes homonyms; the Westminster homonym code narrows it (`רעה@*Ha*` is the first).

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
