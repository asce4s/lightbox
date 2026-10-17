# Lightbox — A Picasa-style Photo Manager in Rust (GPUI Kit)

**Spec version:** 1.0 · **Date:** 2026-10-17 · **Status:** Ready for implementation

> This document is the single source of truth for building Lightbox. It is written to be executed by an autonomous coding agent (Cursor). Read `AGENTS.md` first (how to work), then this file (what to build), then `TASKS.md` (in what order, and how to prove it works).

---

## 0. How to read this spec

- **MUST / SHOULD / MAY** follow RFC 2119 meaning.
- Every feature has **Acceptance Criteria (AC)**. An AC is not done until an automated test proves it. Test names in AC are normative: create a test with that name (or containing it). `TASKS.md` lists additional required tests per task; the union of both lists is binding.
- Code snippets show *intent and shape*. GPUI/GPUI Kit APIs move fast. When a snippet does not compile against the locked version, read the installed crate's source (`~/.cargo/registry/src/*/gpui-kit-*`, `gpui-component-*`, `gpui-pre-*`) or `cargo doc --open`, adapt, and record the deviation in `TASKS.md → Decisions`.
- "Picasa behavior" notes describe the original Google Picasa 3 behavior we emulate. When this spec and Picasa disagree, this spec wins.
- Product name is **Lightbox** (do not ship anything named "Picasa"; it is a Google trademark). Picasa is referenced only as inspiration and as a legacy import format (§6.17).

---

## 1. Product overview

Lightbox is a fast, local-first desktop photo organizer and non-destructive editor. It indexes folders the user chooses, shows every photo and video in one scrollable library, lets the user organize (albums, tags, stars, ratings, captions, people), find (search, filters, timeline, map), fix (one-click and slider edits that never touch the original file), and share (export, slideshow, collage).

### 1.1 Principles (non-negotiable)

1. **Originals are sacred.** Lightbox never modifies, re-encodes, or deletes an original file except through an explicit user file operation (rename, move, delete-to-trash). Edits are stored as data and rendered on demand. A test suite invariant enforces this (§12.6).
2. **Local-first, offline.** No account, no cloud, no telemetry. Network is used only for optional map tiles (§6.14) and optional model download (§6.15), both user-initiated and cached.
3. **Fast at 100k photos.** Virtualized UI, indexed SQLite catalog, background jobs, cached thumbnails. Budgets in §11.
4. **Headless-testable.** All behavior lives in UI-agnostic crates behind a service layer. The GPUI UI is a thin shell over view-models. Everything a user can do in the UI is also exercisable from tests and from the CLI.
5. **Deterministic.** Edit math, collage layouts, grain/noise, and fixtures are seeded and reproducible bit-for-bit on one platform and within tolerance across platforms.

### 1.2 Target platforms

macOS 13+ (Apple Silicon and x86_64), Windows 10/11 x64, Linux x86_64 (X11 and Wayland). CI MUST build and test on all three (§13).

---

## 2. Scope and priorities

| Priority | Meaning |
|---|---|
| **P0** | Core. App is not usable without it. Build first. |
| **P1** | Expected Picasa parity. Build after P0 is green. |
| **P2** | Feature-gated extras (`--features faces`, `video`, `map`). Default-on in release builds, but core tests MUST pass with them off. |
| **P3** | Nice-to-have. Only after everything above is green. |

### 2.1 Feature list

| # | Feature | Priority | Section |
|---|---|---|---|
| F1 | Folder manager (watch / scan once / exclude) | P0 | 6.1 |
| F2 | Scanner, metadata (EXIF/size/date/GPS), corrupt-file tolerance | P0 | 6.2 |
| F3 | Filesystem watcher (add/modify/delete/rename) | P0 | 6.3 |
| F4 | Thumbnail generation + disk cache | P0 | 6.4 |
| F5 | Library grid: grouped by folder or by date, virtualized, multi-select | P0 | 9.3 |
| F6 | Photo viewer with filmstrip, zoom, keyboard nav | P0 | 9.4 |
| F7 | Non-destructive editor: Basic Fixes, Tuning, Effects; undo/redo; revert | P0 | 6.6, 9.5 |
| F8 | Stars, 0–5 ratings, captions, tags | P0 | 6.7 |
| F9 | Albums (virtual collections, ordered) | P0 | 6.7 |
| F10 | Search box + filter bar + query language | P0 | 6.8 |
| F11 | Export (resize, quality, metadata, watermark) | P0 | 6.10 |
| F12 | File ops: rename, move to folder, delete to OS trash, reveal in file manager | P0 | 6.7 |
| F13 | Photo tray (persistent selection holding area) | P1 | 9.3 |
| F14 | Timeline / date-range scrubber | P1 | 9.3 |
| F15 | Import from camera/card/folder (copy + date-based folders + dedupe) | P1 | 6.9 |
| F16 | Duplicate finder (exact + perceptual) | P1 | 6.11 |
| F17 | Slideshow (fullscreen, timing, transitions, captions) | P1 | 6.12 |
| F18 | Collage maker (grid, mosaic, picture pile, frame mosaic) | P1 | 6.13 |
| F19 | Before/after compare in editor | P1 | 9.5 |
| F20 | Light/dark theme, settings screen | P1 | 6.18 |
| F21 | Geotag map view (`map` feature) | P2 | 6.14 |
| F22 | Face detection, clustering, People (`faces` feature) | P2 | 6.15 |
| F23 | Video indexing, thumbnails, playback hand-off (`video` feature) | P2 | 6.16 |
| F24 | Import legacy `.picasa.ini` metadata | P2 | 6.17 |
| F25 | Movie maker (slideshow → MP4 via ffmpeg) | P3 | 6.12.3 |
| F26 | XMP sidecar write-back for stars/ratings/tags/captions | P3 | 6.7.6 |
| F27 | HEIC and camera RAW decoding (`heif`, `raw` features) | P3 | 6.2.3 |

### 2.2 Out of scope

Cloud sync, web albums, accounts, printing, email sending, plugin system, mobile, multi-user catalogs, network shares as watched folders (allowed to *work*, not required to be *watched*).

---

## 3. Technology stack

### 3.1 Toolchain

- Rust **stable**, edition **2024**, MSRV = the version pinned in `rust-toolchain.toml` (use current stable at scaffold time; record it).
- Cargo workspace with `resolver = "3"`; `Cargo.lock` committed; all CI commands use `--locked`.

### 3.2 UI: GPUI Kit

Use the single facade crate **`gpui-kit`** (Longbridge). It re-exports GPUI (`use gpui_kit::*;`), GPUI Base (`gpui_kit::base`), GPUI Component (`gpui_kit::component`), default icon assets (`gpui_kit::assets`), and a headless UI test harness (`gpui_kit::test`, feature `test-support`).

```toml
# workspace Cargo.toml
[workspace.dependencies]
gpui-kit = { version = "0.7", default-features = true }   # pin exact after first successful build

# crates/lb-ui/Cargo.toml
[dependencies]
gpui-kit = { workspace = true }
[dev-dependencies]
gpui-kit = { workspace = true, features = ["test-support"] }
```

Rules:
- **Depend on `gpui-kit` only.** Do not add `gpui`, `gpui-pre-*`, or `gpui-component` directly; they must stay version-locked through the facade.
- Use `gpui_kit::actions!` (not `gpui::actions!`).
- App bootstrap: `gpui_kit::application().run(|cx| { gpui_kit::init(cx); ... })`; wrap root views in `gpui_kit::component::Root`.
- Linux build deps for GPUI (install in CI and document in README): at minimum `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libssl-dev pkg-config clang`. If a build fails on a missing system library, add it to this list and to CI.

Reference bootstrap (from gpui-kit docs):

```rust
use gpui_kit::component::Root;
use gpui_kit::*;

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| MainWindow::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
```

### 3.3 Non-UI crates (use these; add others only with a Decision entry)

| Concern | Crate | Notes |
|---|---|---|
| Errors | `thiserror` (libs), `anyhow` (bins/tests only) | |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` | rolling file logs in data dir |
| Serialization | `serde`, `serde_json` | edit stacks, settings |
| Database | `rusqlite` (features: `bundled`, `functions`, `chrono` or `time`), `rusqlite_migration` | FTS5 available in bundled SQLite |
| Paths | `directories` | data/cache dirs |
| Walk & watch | `walkdir`, `notify`, `notify-debouncer-full` | |
| Hashing | `blake3` | content hash |
| Metadata | `nom-exif` (images + MP4/MOV); fallback `kamadak-exif` | record choice |
| Decoding | `image` (jpeg, png, gif, webp, tiff, bmp) | |
| Resizing | `fast_image_resize` | thumbnails, export |
| Perceptual hash | `image_hasher` | duplicates |
| Text in images | `ab_glyph` + bundled OFL font (e.g. Inter) | watermark, collage captions |
| Parallelism | `rayon`, `crossbeam-channel`, `parking_lot` | |
| Time | `chrono` | |
| Trash | `trash` | OS recycle bin |
| Reveal/open | `opener` | |
| Query parsing | `winnow` | search grammar |
| HTTP (map/models) | `ureq` (rustls) | only in `map`/`faces` features |
| ONNX | `ort` | `faces` feature only |
| CLI | `clap` (derive) | |
| Tests | `tempfile`, `proptest`, `insta`, `assert_cmd`, `predicates`, `pretty_assertions` | |
| Bench | `criterion` | |
| Fixture EXIF writing | `little_exif` (fallback: hand-built APP1 segment, see §12.3) | dev-only |

Optional, feature-gated: `libheif-rs` (`heif`), `rawler` (`raw`, embedded-preview extraction first).

---

## 4. Architecture

### 4.1 Workspace layout

```
lightbox/
├─ Cargo.toml                 # workspace
├─ rust-toolchain.toml
├─ AGENTS.md  SPEC.md  TASKS.md  README.md
├─ crates/
│  ├─ lb-core/        # domain types, ids, errors, settings model. No I/O.
│  ├─ lb-catalog/     # SQLite schema, migrations, repositories, FTS
│  ├─ lb-media/       # decode, orientation, metadata extraction, hashing
│  ├─ lb-imaging/     # edit pipeline (ops + math), compositing, export encode, collage render
│  ├─ lb-thumbs/      # thumbnail generation + content-addressed disk cache
│  ├─ lb-scan/        # folder scanning, watcher, move detection, import
│  ├─ lb-search/      # query language parser → SQL plan
│  ├─ lb-jobs/        # job queue, priorities, cancellation, progress events
│  ├─ lb-faces/       # [feature faces] detector/embedder traits, ONNX impl, clustering
│  ├─ lb-video/       # [feature video] ffprobe/ffmpeg integration
│  ├─ lb-geo/         # [feature map] web-mercator math, tile cache, tile provider trait
│  ├─ lb-picasa/      # .picasa.ini parser + importer
│  ├─ lb-app/         # Service layer: commands, queries, events, view-models. UI-agnostic.
│  ├─ lb-ui/          # GPUI Kit views. Depends on lb-app only (+ gpui-kit).
│  ├─ lb-cli/         # `lightbox-cli` headless commands over lb-app
│  └─ lb-testkit/     # dev-only: fixtures, fakes, invariants, image compare (§12.2)
├─ apps/
│  └─ lightbox/       # GUI binary: wires lb-app + lb-ui
├─ xtask/             # cargo xtask: ci, fixtures, goldens, perf, models
├─ assets/            # icons (SVG), font (OFL), app icon
└─ tests/
   ├─ fixtures/       # small committed fixtures + manifest.toml
   └─ goldens/        # golden PNGs for edit pipeline and collage
```

### 4.2 Dependency rules (enforced by `cargo xtask ci` via `cargo metadata`)

```
lb-core  ←  everything
lb-catalog, lb-media, lb-jobs, lb-search, lb-geo, lb-picasa  →  lb-core only
lb-imaging → lb-core, lb-media
lb-thumbs  → lb-core, lb-media, lb-imaging
lb-scan    → lb-core, lb-catalog, lb-media
lb-faces, lb-video → lb-core, lb-media
lb-app     → all of the above (features forwarded)
lb-ui      → lb-app, lb-core, gpui-kit      (NOT lb-catalog, lb-imaging, …)
lb-cli     → lb-app, lb-core
apps/lightbox → lb-app, lb-ui
lb-testkit → anything; only ever a [dev-dependency]
```

`lb-ui` MUST NOT touch SQLite, files, or image math directly. If a view needs something, add it to `lb-app`.

### 4.3 Runtime model

- **Main thread:** GPUI event loop. Never blocks on I/O or decoding.
- **Catalog writer:** one dedicated thread owns the single write connection (WAL mode). Commands are sent over a channel and acknowledged. Readers use a pool of read-only connections (size = min(4, cores)).
- **Job workers:** `lb-jobs` runs a priority queue on a rayon-backed pool (size = cores − 1, min 1). Priorities: `Interactive` (visible thumbnails, viewer decode) > `Foreground` (user-requested export, import) > `Background` (scan, hashing, faces, duplicates).
- **Events:** `lb-app` publishes `AppEvent`s on a broadcast channel. The UI subscribes and maps events to GPUI `cx.notify()` on the relevant entities. Tests subscribe directly.
- **Deterministic test mode:** `JobQueue::inline()` executes jobs synchronously on the calling thread. All UI tests and most service tests use it.

### 4.4 Paths

| Item | Location |
|---|---|
| Data dir | `directories::ProjectDirs::from("app", "Lightbox", "Lightbox").data_dir()` |
| Catalog | `{data}/catalog.sqlite3` |
| Thumbnail cache | `{cache}/thumbs/` |
| Tile cache | `{cache}/tiles/` |
| Models | `{data}/models/` |
| Logs | `{data}/logs/lightbox.log.YYYY-MM-DD` |
| Override | env `LIGHTBOX_HOME=<dir>` puts data and cache under `<dir>/data` and `<dir>/cache`. **All tests MUST set this to a tempdir.** |

---

## 5. Data model

### 5.1 Identifiers

`PhotoId`, `FolderId`, `AlbumId`, `TagId`, `PersonId`, `FaceId`, `JobId` are `i64` newtypes (`#[repr(transparent)]`, `serde(transparent)`). Never pass raw `i64` across crate boundaries.

### 5.2 Schema (migration `V1__init.sql`)

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE roots (               -- folders configured in Folder Manager
  id           INTEGER PRIMARY KEY,
  path         TEXT NOT NULL UNIQUE,     -- absolute, normalized (§6.1.3)
  mode         TEXT NOT NULL CHECK (mode IN ('watch','once','exclude')),
  added_at     TEXT NOT NULL
);

CREATE TABLE folders (
  id           INTEGER PRIMARY KEY,
  root_id      INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
  path         TEXT NOT NULL UNIQUE,
  parent_id    INTEGER REFERENCES folders(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  caption      TEXT,                     -- folder description (Picasa "folder properties")
  sort_date    TEXT                      -- earliest taken_at in folder; maintained by trigger/job
);

CREATE TABLE photos (
  id            INTEGER PRIMARY KEY,
  folder_id     INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  file_name     TEXT NOT NULL,
  media_type    TEXT NOT NULL CHECK (media_type IN ('image','video')),
  format        TEXT NOT NULL,           -- jpeg|png|gif|webp|tiff|bmp|heif|raw|mp4|mov|...
  file_size     INTEGER NOT NULL,
  file_mtime    INTEGER NOT NULL,        -- unix seconds
  quick_key     TEXT NOT NULL,           -- blake3(size || first 64KiB || last 64KiB), hex
  content_hash  TEXT,                    -- full blake3, hex; NULL until hashed
  width         INTEGER, height INTEGER, -- after EXIF orientation
  orientation   INTEGER NOT NULL DEFAULT 1,
  taken_at      TEXT,                    -- ISO-8601 local w/o tz if unknown; see §6.2.2
  taken_at_src  TEXT CHECK (taken_at_src IN ('exif','video','filename','mtime')),
  camera_make   TEXT, camera_model TEXT, lens TEXT,
  iso INTEGER, f_number REAL, exposure_s REAL, focal_mm REAL,
  gps_lat REAL, gps_lon REAL, gps_alt REAL,
  duration_ms   INTEGER,                 -- video
  caption       TEXT,
  starred       INTEGER NOT NULL DEFAULT 0,
  rating        INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
  hidden        INTEGER NOT NULL DEFAULT 0,
  edit_version  INTEGER NOT NULL DEFAULT 0,   -- increments on every edit-stack change
  phash         INTEGER,                 -- 64-bit perceptual hash
  status        TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','unreadable','missing')),
  error         TEXT,
  indexed_at    TEXT NOT NULL,
  UNIQUE (folder_id, file_name)
);
CREATE INDEX photos_taken_at ON photos(taken_at);
CREATE INDEX photos_quick_key ON photos(quick_key);
CREATE INDEX photos_content_hash ON photos(content_hash);
CREATE INDEX photos_starred ON photos(starred) WHERE starred = 1;

CREATE TABLE edits (
  photo_id    INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
  ops_json    TEXT NOT NULL,             -- EditStack serialized (§6.6)
  redo_json   TEXT NOT NULL DEFAULT '[]',
  updated_at  TEXT NOT NULL
);

CREATE TABLE albums (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL, description TEXT,
  cover_photo_id INTEGER REFERENCES photos(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE album_items (
  album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
  photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
  position REAL NOT NULL,                -- fractional ordering for cheap reorder
  PRIMARY KEY (album_id, photo_id)
);

CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
CREATE TABLE photo_tags (
  photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
  tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (photo_id, tag_id)
);

CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, hidden INTEGER NOT NULL DEFAULT 0);
CREATE TABLE faces (
  id INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
  x REAL NOT NULL, y REAL NOT NULL, w REAL NOT NULL, h REAL NOT NULL,   -- normalized [0,1] in oriented image
  score REAL NOT NULL,
  embedding BLOB,                        -- f32 little-endian, L2-normalized
  cluster_id INTEGER,
  person_id INTEGER REFERENCES people(id) ON DELETE SET NULL,
  source TEXT NOT NULL CHECK (source IN ('detected','manual','picasa'))
);

CREATE TABLE settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);

CREATE TABLE jobs_log (
  id INTEGER PRIMARY KEY, kind TEXT NOT NULL, started_at TEXT, finished_at TEXT,
  status TEXT NOT NULL, detail TEXT
);

CREATE VIRTUAL TABLE photos_fts USING fts5(
  file_name, caption, tags, folder_path, camera, people, album_names,
  content='', tokenize='unicode61 remove_diacritics 2'
);
```

Rules:
- `photos_fts` is a contentless FTS index keyed by `rowid = photos.id`, maintained by `lb-catalog` in the same transaction as any change to its source fields. Provide `Catalog::rebuild_fts()` and a test that it equals incremental maintenance.
- All writes go through `lb-catalog` repository functions; no SQL outside `lb-catalog` (and `lb-search`'s SQL plan output, which `lb-catalog` executes).
- Migrations are append-only. A test opens a V1 database fixture and migrates to latest.

### 5.3 Core domain types (lb-core, abridged)

```rust
pub struct Photo { pub id: PhotoId, pub folder_id: FolderId, pub path: PathBuf, pub media_type: MediaType,
    pub format: Format, pub size: u64, pub width: Option<u32>, pub height: Option<u32>,
    pub taken_at: Option<NaiveDateTime>, pub camera: Option<String>, pub gps: Option<GeoPoint>,
    pub caption: Option<String>, pub starred: bool, pub rating: u8, pub hidden: bool,
    pub edit_version: u32, pub status: PhotoStatus }

pub enum SortOrder { DateTakenAsc, DateTakenDesc, FileNameAsc, FileSizeDesc, ModifiedDesc }
pub enum GroupBy { Folder, Month, Year, None }
pub struct Selection { pub ids: IndexSet<PhotoId>, pub anchor: Option<PhotoId> }
```

---

## 6. Subsystems

### 6.1 Folder manager (F1, P0)

**Picasa behavior:** Tools → Folder Manager lists the filesystem tree; each folder is set to "Scan Always", "Scan Once", or "Remove from Picasa".

- Modes: `watch` (scan now, then watch for changes), `once` (scan now, no watching), `exclude` (never index; removes existing photos under it from catalog — files untouched).
- Modes inherit to subfolders; a nested root overrides its ancestor (most specific path wins).
- Always ignored: hidden files/dirs (dot-prefixed; on Windows also `FILE_ATTRIBUTE_HIDDEN`), `$RECYCLE.BIN`, `.Trash*`, `System Volume Information`, `@eaDir`, `.picasaoriginals` (but see §6.17), our own data/cache dirs.
- Supported extensions (case-insensitive): images `jpg jpeg jpe png gif webp tif tiff bmp`; with `heif`: `heic heif avif`; with `raw`: `cr2 cr3 nef arw dng orf rw2 raf`; with `video`: `mp4 mov m4v avi mkv webm 3gp mts`.

#### 6.1.3 Path normalization
Absolute, `dunce::canonicalize`-style (no `\\?\` prefix on Windows), no trailing separator, symlinks resolved; symlink loops are detected and skipped with a warning. Comparison is case-insensitive on Windows and macOS default volumes (use `to_lowercase` key in a separate column only if needed; record decision).

**AC**
- `folder_manager_watch_scans_and_watches`: adding a `watch` root indexes all fixture images and a file added afterwards appears within 2 s.
- `folder_manager_once_does_not_watch`.
- `folder_manager_exclude_removes_catalog_rows_not_files`: originals hash unchanged.
- `folder_manager_nested_override`: root `/a` watch, `/a/b` exclude → nothing under `/a/b` indexed.
- `scanner_skips_hidden_and_system_dirs`.

### 6.2 Scanner and metadata (F2, P0)

#### 6.2.1 Pipeline
1. Walk roots (`walkdir`, follow links with loop detection). Collect candidate files by extension.
2. For each file: stat → `(size, mtime)`. If a row exists with same path and same `(size, mtime)` → skip (unchanged).
3. Else compute `quick_key`; read metadata (§6.2.2); probe dimensions without full decode where possible (`image::ImageReader::into_dimensions`).
4. Upsert row in batches of 500 per transaction.
5. Enqueue `Background` jobs: thumbnails (128 first), full `content_hash`, `phash`, faces (if enabled).
6. After walk: rows under the root whose file no longer exists → **move detection** (§6.3.2) then mark `missing` (not deleted; user can "Remove missing" in Folder Manager). Missing rows are hidden from the library by default.
7. Emit `AppEvent::ScanProgress { root, done, total_estimate }` at most every 100 ms and `ScanFinished { root, added, updated, missing, errors }`.

Scanning is resumable and idempotent: running it twice yields zero changes the second time (`scan_is_idempotent`).

#### 6.2.2 Metadata extraction
- EXIF: Make, Model, LensModel, ISO, FNumber, ExposureTime, FocalLength, Orientation, DateTimeOriginal (+ OffsetTimeOriginal), GPS (lat/lon/alt with refs), ImageDescription/UserComment → caption only if caption is empty.
- `taken_at` precedence: EXIF DateTimeOriginal → EXIF DateTimeDigitized → video creation_time → filename pattern (`IMG_20240131_235959`, `2024-01-31 23.59.59`, `PXL_20240131_235959123`, `Screenshot_20240131-235959`) → file mtime. Record source in `taken_at_src`. Store as naive local time; if an offset exists, store local time and keep offset in `error`-free side field later if needed (P3).
- Orientation: values 1–8 per EXIF. `width/height` stored **after** orientation (6 and 8 swap).
- GPS: convert DMS rationals to signed decimal degrees; reject (0,0) exactly and out-of-range values.

#### 6.2.3 Decoding (lb-media)
- `decode(path, max_long_edge: Option<u32>) -> Result<DynamicImage>`; applies EXIF orientation; JPEG uses DCT scaling when `max_long_edge` allows (via `image`'s jpeg decoder scale or `zune-jpeg` options) for fast thumbnails.
- Animated GIF/WebP: first frame.
- `heif`/`raw` features: RAW decodes the embedded preview JPEG first; full RAW demosaic is out of scope.
- Every decode is wrapped: panics inside decoders are caught (`std::panic::catch_unwind`) and turned into `MediaError::Corrupt`.

#### 6.2.4 Corrupt and odd files
Zero-byte, truncated, wrong extension, unsupported variant → row with `status='unreadable'` and `error` set; shown in grid with a broken-image placeholder; never crashes scan.

**AC**
- `scan_indexes_basic_fixture_set` (counts per §12.3 manifest).
- `scan_is_idempotent`.
- `exif_orientation_6_swaps_dimensions`.
- `taken_at_precedence` (table-driven over fixtures with/without EXIF, filename patterns).
- `gps_parsed_to_decimal_degrees` (fixture with known coordinates, tolerance 1e-6).
- `corrupt_files_marked_unreadable_without_panic` (zero-byte, truncated JPEG, text-as-.jpg).
- `decoder_panic_is_caught` (inject a panicking decoder via test hook).

### 6.3 Watcher (F3, P0)

#### 6.3.1 Behavior
- `notify` recommended watcher + `notify-debouncer-full` at 500 ms.
- Create/modify → rescan that file. Delete → mark missing (after move detection window). Folder create/delete/rename → rescan subtree.
- Writes from in-progress copies: if size changes between two stats 250 ms apart, re-queue.
- If the watcher reports overflow/rescan-needed, schedule a full root rescan.

#### 6.3.2 Move/rename detection
When a file disappears and within 5 s a new file appears anywhere under any root with equal `quick_key` and size, treat it as a move: update `folder_id/file_name` of the **existing** row (preserving id, stars, tags, albums, edits, faces). Same logic in full rescans (missing row ↔ new file with same `quick_key`).

**AC**
- `watcher_detects_new_file`, `watcher_detects_modified_file_updates_metadata`, `watcher_detects_delete_marks_missing`.
- `rename_preserves_metadata`: star + tag + album + edit a photo, rename it on disk → same `PhotoId`, metadata intact.
- `move_across_folders_preserves_metadata`.
- Watcher tests use real temp dirs with a 10 s timeout and `wait_until` polling (no fixed sleeps > 100 ms).

### 6.4 Thumbnails (F4, P0)

- Sizes: **S=160**, **M=320**, **L=640** px long edge (grid uses S or M depending on zoom; viewer filmstrip uses S; L for previews).
- Rendered **with the current edit stack applied** (edits run on a proxy ≤ 1280 px first, then downscale).
- Cache key: `blake3(quick_key || edit_stack_hash || size)` → `{cache}/thumbs/{size}/{k[0..2]}/{k}.jpg` (JPEG q=82, sRGB). Atomic write (temp file + rename).
- In-memory LRU of decoded `RenderImage`s in `lb-app` (default 400 MB budget, configurable), keyed by `(PhotoId, size, edit_version)`.
- Request API is async and prioritized: visible grid cells request `Interactive`; prefetch ±2 screens at `Foreground`; full library pre-generation of S at `Background`.
- Video thumbnails come from `lb-video` (frame at 10% of duration); unreadable files get a placeholder (no cache entry).
- Cache GC: on startup, if cache > limit (default 2 GB), delete least-recently-accessed files (track atime in a small sidecar index, not filesystem atime).

**AC**
- `thumbnail_respects_orientation`, `thumbnail_long_edge_exact`, `thumbnail_reflects_edits` (cache key changes after edit; image differs), `thumbnail_cache_hit_does_not_decode` (counter hook), `thumbnail_cache_gc_evicts_lru`, `thumbnail_write_is_atomic` (kill-safe: partial temp files ignored).

### 6.5 Job system (lb-jobs)

```rust
pub enum Priority { Interactive, Foreground, Background }
pub struct JobHandle { pub id: JobId, cancel: Arc<AtomicBool> }
pub trait Job: Send + 'static {
    fn kind(&self) -> &'static str;
    fn run(self: Box<Self>, ctx: &JobCtx) -> Result<(), JobError>;   // ctx.is_cancelled(), ctx.progress(f32)
}
pub struct JobQueue { /* pool or inline */ }
impl JobQueue {
    pub fn threaded(workers: usize) -> Self;
    pub fn inline() -> Self;                 // runs on submit; for tests
    pub fn submit(&self, job: impl Job, p: Priority) -> JobHandle;
    pub fn reprioritize(&self, id: JobId, p: Priority);
    pub fn cancel(&self, id: JobId);
    pub fn wait_idle(&self, timeout: Duration) -> bool;
}
```
- Coalescing: submitting a thumbnail job for a key already queued returns the existing handle (and raises priority if higher).
- Panics inside jobs are caught, logged, and reported as `JobError::Panicked`.

**AC**: `jobs_priority_order`, `jobs_cancel_before_start`, `jobs_cancel_cooperative`, `jobs_coalesce_duplicates`, `jobs_panic_is_contained`, `jobs_wait_idle`.

### 6.6 Non-destructive edit pipeline (F7, P0)

#### 6.6.1 Model
```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    // Basic Fixes
    Crop { x: f32, y: f32, w: f32, h: f32 },          // normalized to the image entering this op
    RotateQuarter { turns: i8 },                      // ±1, ±2 (cw positive)
    Straighten { degrees: f32 },                      // [-45, 45]
    RedEye { regions: Vec<NormRect> },
    AutoContrast,
    AutoColor,
    FeelingLucky,                                     // = AutoContrast then AutoColor
    Retouch { spots: Vec<RetouchSpot> },              // P1: clone-heal from source to target circle
    FillLight { amount: f32 },                        // [0, 1]
    // Tuning (one op; sliders edit it in place if it is the last op)
    Tuning { fill_light: f32, highlights: f32, shadows: f32, temperature: f32,
             neutral: Option<[f32; 3]> },
    // Effects
    Sharpen { amount: f32 },                          // [0, 2]
    Sepia,
    BlackAndWhite,
    Warmify,
    FilmGrain { amount: f32 },                        // [0, 1]; seed from photo quick_key
    Tint { color: [f32; 3], preserve: f32 },          // preserve ∈ [0,1]
    Saturation { amount: f32 },                       // [-1, 1]
    SoftFocus { cx: f32, cy: f32, radius: f32, amount: f32 },
    Glow { intensity: f32, radius: f32 },
    FocalBlackAndWhite { cx: f32, cy: f32, radius: f32, sharpness: f32 },
    GraduatedTint { color: [f32; 3], y: f32, feather: f32, amount: f32 },
    Vignette { amount: f32, radius: f32 },            // P1
}
pub struct EditStack { pub version: u32, pub ops: Vec<EditOp> }
```

Rules:
- Ops apply **in stack order** (Picasa behavior). EXIF orientation is always applied before op 0.
- Consecutive slider adjustments to the same op type on the top of the stack **replace** that op rather than pushing a new one (so dragging a slider is one undo step). A slider drag commits on release; during drag, preview only.
- Undo pops to a redo stack; any new op clears redo. "Undo All"/Revert clears ops (keeps redo so Revert is undoable once).
- Every stack change increments `photos.edit_version` and `EditStack.version`, emits `AppEvent::PhotoEdited`.
- `edit_stack_hash = blake3(canonical JSON of ops)` (floats serialized with fixed 6-decimal precision).
- **Validation:** `EditOp::validate()` clamps/rejects out-of-range params; crop must have positive area and lie in [0,1].

#### 6.6.2 Working color model
- Decode to `Rgba32F` with sRGB-encoded values in [0,1] (no color management beyond assuming sRGB; embedded ICC profiles are ignored in P0, converted in P3).
- `luma(r,g,b) = 0.2126 r + 0.7152 g + 0.0722 b` on sRGB-encoded values.
- `to_linear(v)` / `to_srgb(v)` use the exact sRGB piecewise transfer functions.
- All outputs clamped to [0,1] after every op. Alpha preserved.
- Implementation MUST be pure Rust, single-threaded per tile or rayon over rows, no SIMD intrinsics required. Results must be bit-identical between runs on the same machine.

#### 6.6.3 Op definitions (normative math)

Let `v` be a channel value, `L = luma(r,g,b)`, `eps = 1e-6`. "Scale by luma" means: compute target `L'`; if `L > eps` multiply r,g,b by `L'/L`, else set r=g=b=`L'`; clamp.

| Op | Definition |
|---|---|
| RotateQuarter | Exact pixel rotation (no resampling). `turns` mod 4. |
| Crop | Integer pixel crop: `x0=round(x·W)`, `y0=round(y·H)`, `x1=round((x+w)·W)`, `y1=round((y+h)·H)`, clamped, min 1×1. |
| Straighten θ | Rotate image content about its center by θ degrees (**positive θ rotates clockwise**), bilinear sampling, then center-crop by scale `s = min(W/(W·c + H·sn), H/(W·sn + H·c))` where `c=cos|θ|`, `sn=sin|θ|`; output size `round(s·W) × round(s·H)`. No transparent corners. |
| AutoContrast | Compute luma histogram (1024 bins). `lo` = 0.5th percentile, `hi` = 99.5th percentile. If `hi−lo < 0.02` → no-op. Each channel: `v' = (v − lo)/(hi − lo)`. |
| AutoColor | Per channel c: `lo_c`, `hi_c` = 0.5/99.5 percentiles of that channel. Then shift so that each channel's mean of mid-luma pixels (0.25 < L < 0.75) equals their overall mean: `v' = (v − lo_c)/(hi_c − lo_c)`, then `v'' = v' · (m / m_c)` where `m_c` is channel mean over mid-luma pixels after stretch and `m` the mean of the three. Skip a channel if `hi_c − lo_c < 0.02`. |
| FeelingLucky | AutoContrast then AutoColor. |
| FillLight a | Scale by luma with `L' = L + 2a·L·(1−L)²`. |
| Tuning | Apply in this order: neutral → temperature → fill_light → highlights → shadows. **neutral** `[nr,ng,nb]` (sampled sRGB color): gains `g_c = mean(n)/max(n_c, eps)`, normalized so `max(g)=1`, multiply. **temperature** t∈[−1,1]: `r·=1+0.15t`, `b·=1−0.15t`. **fill_light** as FillLight. **highlights** h∈[0,1]: `L' = L + 2h·L²·(1−L)`. **shadows** s∈[0,1]: `L' = L − 2s·L·(1−L)²` (floor 0). |
| Sharpen a | Unsharp mask on each channel: `blur` = separable Gaussian σ=1.0 (kernel radius 3, edge clamp); `v' = v + a·(v − blur)`. |
| BlackAndWhite | r=g=b=L. |
| Sepia | `r' = .393r+.769g+.189b; g' = .349r+.686g+.168b; b' = .272r+.534g+.131b`. |
| Warmify | Tuning(temperature=+0.4) then Saturation(+0.15). |
| Saturation s | `v' = L + (v − L)(1 + s)`. |
| Tint color C, preserve p | `T = C·L` (per channel, using L of the pixel); `v' = p·v + (1−p)·T`. |
| FilmGrain a | Seeded PRNG (`rand_pcg::Pcg32`, seed = first 8 bytes of `quick_key` XOR op index). Per pixel noise `n ~ U(−1,1)`; `v' = v + a·0.12·n·(1 − |2L−1|)` (same n for all channels). |
| SoftFocus | Distance `d` from (cx,cy) in normalized coords (aspect-corrected by long edge). Mask `m = smoothstep(radius, radius+0.25, d)·amount`. `blur` = Gaussian σ = 0.01·long_edge. `v' = mix(v, blur, m)`. |
| Glow i, r | `B` = Gaussian σ = r·0.02·long_edge of `max(0, v−0.5)·2`; screen blend: `v' = 1 − (1−v)(1−i·B)`. |
| FocalBlackAndWhite | `m = smoothstep(radius, radius+sharpness_to_feather, d)` with `feather = 0.3·(1−sharpness)+0.01`; `v' = mix(v, L, m)`. |
| GraduatedTint | `m = amount·(1 − smoothstep(y−feather, y+feather, py))` where py is normalized row; `v' = mix(v, C·L·…)`: use Tint(C, preserve=0) result blended by m. |
| Vignette a, r | `v' = v·(1 − a·smoothstep(r, 1.0, d·√2))`. |
| RedEye | Within each region ellipse: pixels where `r > 1.5·max(g,b)` and `r > 0.2` get `r' = (g+b)/2`. |
| Retouch | For each spot (source center, target center, radius normalized): copy source disk to target with feathered (radius·0.2) alpha blend. |

`smoothstep(e0,e1,x)` is the standard cubic Hermite, clamped.

#### 6.6.4 Rendering API (lb-imaging)
```rust
pub fn render(src: &DynamicImage, stack: &EditStack, ctx: &RenderCtx) -> Rgba32FImage;
pub struct RenderCtx { pub seed: u64, pub target: RenderTarget /* Proxy{max_edge} | Full */ }
```
- Interactive preview renders on a proxy (long edge ≤ 2048 or window size, whichever is smaller) at `Interactive` priority, cancelling the previous render for the same photo.
- Size-dependent params (Gaussian σ, etc.) are defined relative to long edge so proxy and full render look the same.

**AC**
- Per-op golden tests: `golden_<op>_<fixture>` comparing to `tests/goldens/*.png` with tolerance: max abs channel diff ≤ 2/255 and PSNR ≥ 45 dB (cross-platform). Fixtures: `gradient_rgb_512.png`, `portrait_patches_640x480.jpg`, `low_contrast_400.png`.
- Property tests (proptest): `rotate_four_quarters_is_identity`, `crop_full_is_identity`, `ops_never_produce_out_of_range`, `edit_stack_serde_roundtrip`, `stack_hash_stable_across_runs`, `straighten_zero_is_identity`, `auto_contrast_idempotent_within_tolerance`.
- `slider_drag_coalesces_into_one_op`, `undo_redo_semantics`, `revert_is_undoable`, `original_file_unchanged_after_edits`.
- `proxy_and_full_render_similar` (downscaled full vs proxy PSNR ≥ 35 dB).

### 6.7 Organization (F8, F9, F12, P0)

#### 6.7.1 Stars, ratings, captions, hidden
- `starred` toggle (Picasa star), `rating` 0–5 (Lightbox addition), `caption` (≤ 2000 chars, plain text), `hidden` (Picasa "Hide"; hidden photos excluded unless "Show hidden" is on).
- Batch operations apply to the whole selection in one transaction; one undo step (§7.4).

#### 6.7.2 Tags
- Case-insensitive unique names, trimmed, internal whitespace collapsed, max 64 chars, no commas. Autocomplete from existing tags by prefix (top 10 by usage).
- Batch add/remove on selection. Renaming a tag merges if the target exists.

#### 6.7.3 Albums
- Virtual collections; a photo can be in many albums. Ordered (fractional `position`; renormalize when gaps < 1e-9).
- Create, rename, delete (photos untouched), add/remove, reorder by drag, set cover, duplicate album.
- Default album sort: by `sort_order`, then name.

#### 6.7.4 File operations
- **Rename** (single): validates name (no separators, not empty, keeps extension unless explicitly changed, OS-reserved names on Windows rejected). Batch rename with pattern `{name}`, `{n:03}`, `{date:%Y-%m-%d}` (P1).
- **Move to folder**: files moved on disk (same volume: rename; cross-volume: copy + verify hash + delete). Catalog row updated, id preserved.
- **Delete**: moves to OS trash via `trash` crate after a confirmation dialog; row removed. If trash is unavailable, show error; do not hard-delete.
- **Reveal in file manager**, **Open with default app** via `opener`.
- **Locate on disk** for `missing` photos: user picks the new folder; rows re-linked by `quick_key`.
- All file ops are journaled in `jobs_log` and are idempotent on retry.

#### 6.7.5 Folder captions & folder rename
Folder rename renames on disk (with confirmation) and updates descendant paths in one transaction.

#### 6.7.6 XMP sidecar write-back (P3)
Optional setting: write `{file}.xmp` with `xmp:Rating`, `dc:subject` (tags), `dc:description` (caption). Never write into originals.

**AC**
- `star_toggle_batch`, `rating_bounds_enforced`, `caption_saved_and_indexed_in_fts`, `hidden_excluded_by_default`.
- `tag_normalization`, `tag_rename_merges`, `tag_autocomplete_prefix`.
- `album_crud`, `album_reorder_fractional_positions`, `album_delete_keeps_photos`, `album_renormalize_positions`.
- `rename_file_updates_catalog`, `rename_rejects_invalid_names` (table: `""`, `a/b`, `CON` on Windows-only test), `move_cross_folder_preserves_id`, `delete_moves_to_trash` (uses a `TrashBackend` trait; tests use a fake trash that moves into a temp dir), `relink_missing_by_quick_key`.

### 6.8 Search and filters (F10, F14, P0/P1)

#### 6.8.1 Query language (lb-search)
Typing in the search box filters the current view live (debounce 150 ms). Grammar (whitespace-separated terms, implicit AND):

```
query    := term*
term     := ['-'] (filter | phrase | word)
filter   := key ':' value
key      := tag | album | folder | camera | lens | person | type | ext | caption | name
          | rating | date | year | month | starred | hidden | has | is
value    := word | phrase | range | comparison
phrase   := '"' chars '"'
range    := datespec '..' datespec            (either side may be empty)
comparison := ('>' | '>=' | '<' | '<=' | '=') number
datespec := YYYY | YYYY-MM | YYYY-MM-DD
has:     gps | caption | faces | edits | tags
is:      starred | hidden | edited | video | photo | missing | unreadable
type:    photo | video
```
- Bare words and phrases match FTS (prefix match for words ≥ 2 chars: `beac` matches `beach`).
- `rating:>=3`, `date:2023-06..2023-08`, `year:2019`, `camera:"Canon EOS"`, `-tag:work`, `has:gps`, `is:edited`.
- Unknown keys are treated as plain words (no error), but the parser returns diagnostics for the UI to underline.
- Output: `QueryPlan { sql_where: String, params: Vec<SqlValue>, fts: Option<String>, diagnostics: Vec<Diag> }`. SQL is built only with bound parameters (no string interpolation of user values). A test fuzzes the parser with proptest strings and checks it never panics and never emits unbound user text.

#### 6.8.2 Filter bar (UI)
Toggle chips: Starred, Video, Faces, Geotagged, Edited, Rating ≥ N (dropdown), plus Date range (timeline scrubber F14). Chips compose with the text query (AND). The equivalent query text is shown in a tooltip on hover.

**AC**
- `parser_table_tests` (≥ 40 cases with `insta` snapshots of `QueryPlan`).
- `parser_never_panics` (proptest), `parser_params_bound`.
- `search_fts_prefix`, `search_tag_negation`, `search_date_range_inclusive`, `search_rating_comparison`, `search_combines_with_chips`.
- `search_100k_under_budget` (perf, release only, §11).

### 6.9 Import (F15, P1)

- Sources: any folder or removable volume (detect volumes via OS mount points: `/Volumes/*`, `/media/$USER/*`, `/run/media/$USER/*`, drive letters with `DCIM`).
- Import dialog shows thumbnails (generated from source without caching to the main cache), lets the user deselect, pick destination root, folder naming (`{yyyy}/{yyyy-mm-dd}` default, `{yyyy-mm-dd} {title}`), and "skip duplicates" (by `content_hash` against catalog, default on).
- Copy with verification (hash after copy). Optional "delete from source after import" (default off; only after verified copy).
- Imported files are indexed immediately.

**AC**: `import_copies_into_date_folders`, `import_skips_duplicates`, `import_verifies_hash`, `import_delete_source_only_after_verify`, `import_cancel_leaves_no_partial_files`.

### 6.10 Export (F11, P0)

- Inputs: selection or album. Options: destination folder; size (original, long edge 640/1024/1600/2048/custom); JPEG quality 1–100 (default 85); format (JPEG default; PNG; keep original for unedited); apply edits (default on); metadata (keep EXIF minus GPS default; keep all; strip all); watermark text (bottom-right, 3% of long edge font size, 60% opacity white with 1 px dark shadow); filename collision policy (append ` (n)`, overwrite, skip).
- Export edited images with EXIF orientation reset to 1 after baking orientation.
- Metadata copy: copy EXIF block from original using `little_exif` (or img-parts) and patch Orientation, PixelXDimension/YDimension; drop GPS tags when requested.
- Runs as a `Foreground` job with progress and cancel; partial outputs are written to temp names and renamed on success.

**AC**: `export_resizes_long_edge`, `export_applies_edits`, `export_strip_gps`, `export_keep_exif_resets_orientation`, `export_watermark_present` (pixel check in corner region vs non-watermarked), `export_collision_policy`, `export_cancel_cleans_temp`.

### 6.11 Duplicates (F16, P1)

- Exact: same `content_hash`.
- Similar: `phash` (64-bit, `image_hasher` DoubleGradient or Mean on 16×16) Hamming distance ≤ threshold (default 6), grouped via union-find. Use a BK-tree for neighbor search.
- UI: groups with side-by-side compare; actions: keep one (others to trash), star best, add group to album.

**AC**: `exact_duplicates_grouped`, `resized_copy_is_similar`, `different_images_not_similar`, `bk_tree_matches_bruteforce` (proptest).

### 6.12 Slideshow (F17, P1)

- Fullscreen window (GPUI window with `WindowKind`/fullscreen option); black background; photo fitted.
- Controls (auto-hide after 2 s idle): play/pause (Space), prev/next (←/→), exit (Esc), duration slider 1–30 s (default 3), transition (none, crossfade 400 ms, pan-and-zoom/Ken Burns), show captions toggle, shuffle, loop.
- Preloads next 2 images at `Interactive` priority.
- **Slideshow state machine lives in lb-app** (`SlideshowState`) driven by a clock trait so tests advance time deterministically.

#### 6.12.3 Movie maker (P3, `video` feature)
Render slideshow to MP4 via `ffmpeg` CLI: frames piped as raw RGB, H.264 yuv420p, 30 fps, 1080p max; optional audio track from a user-selected file.

**AC**: `slideshow_advances_on_tick`, `slideshow_pause_stops_advance`, `slideshow_loop_and_shuffle_deterministic_with_seed`, `slideshow_skips_unreadable`.

### 6.13 Collage (F18, P1)

- Inputs: 2–200 photos (selection/album/folder). Output size presets (1920×1080, 3000×2000, 4×6 in @300 dpi = 1800×1200, square 2048, custom).
- Layouts:
  - **Grid**: rows/cols auto (`cols = ceil(sqrt(n·aspect))`), center-crop cells, spacing px, background color.
  - **Mosaic**: justified rows (Flickr-style): target row height, scale rows to exact width, last row centered.
  - **Picture pile**: each photo with white border (2% of short edge), random rotation ±15°, random position via seeded PRNG avoiding >60% overlap of centers; drop shadow.
  - **Frame mosaic**: one large center image (first or chosen) surrounded by grid.
- Seed stored with collage settings; "Shuffle" changes seed. Live preview at ≤ 1024 px; final render full size. Output saved as a new JPEG file in a chosen folder (default `{root}/Collages/`), then indexed.

**AC**: `golden_collage_grid_4`, `golden_collage_mosaic_7`, `golden_collage_pile_5_seed_42`, `collage_output_dimensions_exact`, `collage_deterministic_with_seed`, `collage_layout_never_exceeds_canvas` (proptest over n, aspects).

### 6.14 Map and geotags (F21, P2, feature `map`)

- `lb-geo`: Web Mercator math (`lat/lon ↔ tile x/y/zoom ↔ pixel`), tile grid computation for a viewport, marker clustering (grid-based per zoom: 60 px cells).
- `TileProvider` trait: `fn fetch(&self, z,x,y) -> Result<Vec<u8>>`. Default impl: OpenStreetMap standard tiles (`https://tile.openstreetmap.org/{z}/{x}/{y}.png`) with a descriptive `User-Agent` including app name/version and contact URL, max 2 concurrent requests, disk cache with 7-day TTL, and visible "© OpenStreetMap contributors" attribution. Provider URL is configurable in settings. Tests use a `LocalTileProvider` serving generated PNGs.
- Map panel: shows markers for geotagged photos in the current view; clicking a cluster filters the grid to those photos; drag-to-pan, scroll-to-zoom (z 2–18).
- Geotag editing: select photos → "Set location" → click map → writes `gps_lat/lon` to catalog (originals untouched; optional XMP in P3).

**AC**: `mercator_roundtrip` (proptest, tolerance 1e-9), `tiles_for_viewport`, `cluster_counts_sum_to_markers`, `tile_cache_ttl`, `set_location_updates_catalog`, `no_network_in_tests` (provider injection asserted).

### 6.15 Faces and People (F22, P2, feature `faces`)

- Traits in `lb-faces`:
  ```rust
  pub trait FaceDetector: Send + Sync { fn detect(&self, img: &RgbImage) -> Result<Vec<Detection>>; }
  pub trait FaceEmbedder: Send + Sync { fn embed(&self, img: &RgbImage, det: &Detection) -> Result<[f32; 128]>; }
  ```
- Default ONNX implementations with `ort`: detection **YuNet**, recognition **SFace** (OpenCV Zoo). `cargo xtask models` downloads them with SHA-256 verification into the models dir. Verify each model's license before redistribution and record it in `TASKS.md → Decisions`; models are downloaded at runtime by user action, not bundled, unless the license permits.
- Pipeline: decode at long edge 1024 → detect (score ≥ 0.8, min face 24 px) → align by eye landmarks → embed → store.
- Clustering: incremental. New face joins nearest cluster centroid if cosine similarity ≥ 0.55 (tunable), else creates a new cluster. Periodic full re-cluster with agglomerative (average linkage) at the same threshold. Named clusters become `people`.
- UI: People section in sidebar ("Unnamed" first, then by name), face crops grid, name a cluster (autocomplete), merge, remove a face from a person, "not this person", hide person. Manual face add: drag a rectangle in viewer.
- Search `person:"Ana"` uses `people`.

**AC** (with `MockDetector`/`MockEmbedder` driven by fixture manifest): `faces_detected_and_stored_normalized`, `cluster_assigns_similar_embeddings`, `name_cluster_creates_person`, `merge_people`, `remove_face_from_person`, `person_search`. Real-model smoke test `faces_real_model_smoke` is `#[ignore]` unless env `LIGHTBOX_FACE_MODELS` is set; CI runs it in a nightly job only.

### 6.16 Video (F23, P2, feature `video`)

- Requires `ffprobe`/`ffmpeg` on PATH (or configured path). If absent: videos are still indexed by filesystem data, show a generic video placeholder, and Settings shows a warning.
- `ffprobe -v error -print_format json -show_format -show_streams` → duration, dimensions (respect rotation side data), creation_time, GPS (`location` tag ISO 6709).
- Thumbnail: `ffmpeg -ss {10% of duration} -i file -frames:v 1 -vf scale=...` to PNG on stdout.
- Playback: hand off to the OS default player via `opener` (in-app playback out of scope). Grid shows duration badge.

**AC**: `ffprobe_json_parsed` (from recorded JSON fixtures; no binary needed), `video_thumbnail_command_built_correctly`, `video_rotation_swaps_dimensions`, `video_iso6709_location_parsed`; `video_integration_ffmpeg` runs only when ffmpeg is present (CI installs it).

### 6.17 Legacy Picasa import (F24, P2)

Parse `.picasa.ini` (and legacy `Picasa.ini`) files in indexed folders, on user request ("Import Picasa metadata" in Tools menu) and optionally on scan.

Format (INI, sections are file names or special sections):
```
[Picasa]
name=Folder Name
description=Folder caption
[.album:a1b2c3]
name=Summer 2009
token=a1b2c3
date=2009-08-01T10:00:00+02:00
[Contacts2]
4f3e2d1c=Ana;;
[IMG_0001.JPG]
star=yes
caption=At the beach
rotate=rotate(1)
albums=a1b2c3,d4e5f6
keywords=beach,family
filters=crop64=1,30a245ce99b7dbb2;enhance=1;autolight=1;autocolor=1;finetune2=1,0.000000,0.000000,0.190000,fff7f5f3,0.000000;
faces=rect64(3f845bcb59418507),4f3e2d1c;rect64(9eb15e89b6b584c1),ffffffffffffffff
```
Mapping:
- `star=yes` → starred. `caption` → caption (unescape). `keywords` → tags. `albums` tokens → albums by `[.album:token] name`.
- `rotate=rotate(n)` → `RotateQuarter{turns:n}`.
- `filters`: `crop64=1,<hex16>` → Crop (hex is four 16-bit values left, top, right, bottom as fractions of 65535); `enhance=1` → FeelingLucky; `autolight=1` → AutoContrast; `autocolor=1` → AutoColor; `finetune2=1,fill,highlights,shadows,neutralHexARGB,temperature` → Tuning; `bw=1` → BlackAndWhite; `sepia=1` → Sepia; `warm=1` → Warmify; `sat=1,amount` → Saturation; `radblur`, `glow2`, `ansel`, `fill=1,amount` → FillLight; unknown filters are logged and skipped. Order preserved.
- `faces=rect64(hex16),contactId` → face rect (same 4×16-bit encoding) with person from `[Contacts2]` (`name;;`), `source='picasa'`. `ffffffffffffffff` means unnamed.
- Folder `[Picasa] description` → folder caption.
- Import never overwrites non-empty Lightbox values unless "overwrite" is chosen; it is idempotent (running twice changes nothing).
- `.picasaoriginals` folders: if a file exists in both, the one in the parent is a Picasa-saved edit; the original lives in `.picasaoriginals`. Offer "Restore originals view": index the `.picasaoriginals` version and hide the saved copy (P3; default: index parent only).

**AC**: `picasa_ini_parse_fixture` (insta snapshot), `rect64_decode` (table), `crop64_maps_to_crop`, `finetune2_maps_to_tuning`, `picasa_import_idempotent`, `picasa_import_does_not_overwrite_by_default`, `picasa_albums_and_contacts`.

### 6.18 Settings (F20, P1)

Stored in `settings` table as JSON per key; typed via `Settings` struct with defaults and `#[serde(default)]` for forward compatibility.

| Key | Default |
|---|---|
| `theme` | `system` (`light`/`dark`) |
| `grid.thumb_size` | `M` |
| `grid.group_by` | `Folder` |
| `grid.sort` | `DateTakenAsc` within group, groups by date desc |
| `library.show_hidden` | false |
| `library.show_missing` | false |
| `thumbs.cache_limit_mb` | 2048 |
| `thumbs.memory_limit_mb` | 400 |
| `export.*` | as §6.10 |
| `slideshow.*` | as §6.12 |
| `faces.enabled` | false until models present |
| `map.tile_url` | OSM default |
| `video.ffmpeg_path` | auto |

**AC**: `settings_defaults`, `settings_roundtrip`, `settings_unknown_keys_ignored`, `theme_change_emits_event`.

---

## 7. Service layer (lb-app)

The UI and CLI only talk to `lb-app`. Everything is testable with `Lightbox::open_in(tempdir, JobQueue::inline())`.

### 7.1 Facade

```rust
pub struct Lightbox { /* Arc inner */ }
impl Lightbox {
    pub fn open(home: &Path, jobs: JobQueue) -> Result<Self>;
    pub fn events(&self) -> Receiver<AppEvent>;
    pub fn library(&self) -> LibraryService;   // roots, scan, folders
    pub fn photos(&self)  -> PhotoService;     // query, get, star/rate/caption/hide, file ops
    pub fn edits(&self)   -> EditService;      // push/replace/undo/redo/revert, preview render
    pub fn albums(&self)  -> AlbumService;
    pub fn tags(&self)    -> TagService;
    pub fn search(&self)  -> SearchService;    // parse + execute → PhotoQueryResult
    pub fn thumbs(&self)  -> ThumbService;     // request(id,size,prio) -> ThumbTicket; get_cached
    pub fn export(&self)  -> ExportService;
    pub fn import(&self)  -> ImportService;
    pub fn dupes(&self)   -> DuplicateService;
    pub fn slideshow(&self) -> SlideshowService;
    pub fn collage(&self) -> CollageService;
    #[cfg(feature = "faces")] pub fn faces(&self) -> FaceService;
    #[cfg(feature = "map")]   pub fn geo(&self)   -> GeoService;
    pub fn picasa(&self)  -> PicasaImportService;
    pub fn settings(&self) -> SettingsService;
    pub fn history(&self) -> HistoryService;   // app-level undo for organize actions (§7.4)
}
```

### 7.2 Queries for the grid

```rust
pub struct ViewQuery { pub source: ViewSource, pub search: String, pub chips: FilterChips,
                       pub group_by: GroupBy, pub sort: SortOrder, pub show_hidden: bool }
pub enum ViewSource { AllPhotos, Folder(FolderId), Album(AlbumId), Person(PersonId), Starred,
                      RecentlyEdited, Imports(ImportBatchId), Duplicates, Tray }
pub struct PhotoQueryResult { pub groups: Vec<Group>, pub total: usize }
pub struct Group { pub key: GroupKey, pub title: String, pub subtitle: String, pub ids: Vec<PhotoId> }
```
Result contains only ids + group headers. Rows needed for visible cells are fetched in batches via `photos().get_many(&ids)` (≤ 200 per call). This keeps 100k-photo views fast.

### 7.3 Events

```rust
pub enum AppEvent {
    ScanProgress { root: RootId, done: usize, total_estimate: usize },
    ScanFinished { root: RootId, added: usize, updated: usize, missing: usize, errors: usize },
    PhotosChanged { ids: Vec<PhotoId> },          // metadata changed
    PhotosAdded { ids: Vec<PhotoId> }, PhotosRemoved { ids: Vec<PhotoId> },
    PhotoEdited { id: PhotoId, version: u32 },
    ThumbReady { id: PhotoId, size: ThumbSize, version: u32 },
    AlbumsChanged, TagsChanged, PeopleChanged, FoldersChanged,
    JobProgress { id: JobId, kind: &'static str, progress: f32, label: String },
    JobFinished { id: JobId, result: Result<(), String> },
    SettingsChanged { key: String },
    Error { context: String, message: String },
}
```
Events are coalesced per 16 ms for `PhotosChanged`/`ThumbReady` to avoid UI storms.

### 7.4 App-level undo (organize actions)
`HistoryService` records reversible organize commands (star, rate, caption, tag add/remove, album add/remove/reorder, hide). `Ctrl/Cmd+Z` in library context undoes the last organize command; in editor context it undoes the edit stack. File operations (rename/move/delete) are **not** undoable via history (trash is the safety net); the UI says so in the confirmation.

### 7.5 View-models
Each screen has a plain-Rust view-model in `lb-app::vm` (e.g., `LibraryVm`, `ViewerVm`, `EditorVm`, `ExportDialogVm`, `SlideshowVm`). View-models hold UI state (selection, scroll anchor, focused item, dialog fields, validation errors) and expose intent methods (`select_range`, `toggle_star_selected`, `apply_slider`, …). GPUI views render from the VM and forward events to it. **Most UI behavior is tested at the VM level without GPUI**; GPUI tests (§12.5) cover wiring.

**AC**: `vm_selection_click_shift_ctrl` (click, shift-range, ctrl/cmd-toggle, select-all, across groups), `vm_keyboard_navigation_grid` (arrow keys with column count), `vm_editor_slider_preview_then_commit`, `history_undo_star_tag_album`.

---

## 8. CLI (lb-cli → binary `lightbox-cli`)

Same catalog as the GUI (respects `LIGHTBOX_HOME`). Machine-readable output with `--json`.

```
lightbox-cli roots add <path> [--mode watch|once|exclude]
lightbox-cli roots list
lightbox-cli scan [--root <id>] [--wait]
lightbox-cli query "<search query>" [--source all|starred|album:<id>|folder:<id>] [--json]
lightbox-cli star <photo-id>... [--off]
lightbox-cli rate <photo-id> <0-5>
lightbox-cli tag add|remove <tag> <photo-id>...
lightbox-cli album create <name> | add <album-id> <photo-id>... | list
lightbox-cli edit push <photo-id> '<EditOp JSON>' | undo <photo-id> | revert <photo-id> | show <photo-id>
lightbox-cli render <photo-id> <out.png> [--max-edge N]
lightbox-cli export <photo-id>... --to <dir> [--long-edge N] [--quality Q] [--strip-gps]
lightbox-cli collage <layout> <photo-id>... --out <file> [--size WxH] [--seed N]
lightbox-cli dupes [--threshold N] [--json]
lightbox-cli picasa-import <folder> [--overwrite]
lightbox-cli thumbs warm [--size S|M|L]
lightbox-cli doctor          # environment report: GPU, ffmpeg, models, catalog integrity (PRAGMA integrity_check)
```
Exit codes: 0 ok, 1 user error, 2 runtime error.

**AC**: an `assert_cmd` end-to-end test per subcommand (`cli_<subcommand>_e2e`) running against the generated fixture library in a tempdir, asserting JSON output and catalog state.

---

## 9. User interface (lb-ui, GPUI Kit)

### 9.1 Global layout (Library window)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Title bar: [Import] [Slideshow] [Collage] [Export]        [🔍 search……] [⚙]  │
│ Filter chips: ☆Starred  ▶Video  ☺Faces  ⌖Geotagged  ✎Edited  Rating≥[ ]     │
├───────────────┬──────────────────────────────────────────────┬───────────────┤
│ Sidebar       │ Grid (virtualized)                           │ Info / Map    │
│ ▸ Library     │ ┌ Folder or month header ── 34 photos ─────┐ │ panel         │
│   All photos  │ │ ▢ ▢ ▢ ▢ ▢ ▢ ▢ ▢                          │ │ (toggle,      │
│   Starred     │ │ ▢ ▢ ▢                                     │ │  resizable)   │
│   Recently ed.│ ├ Next header ─────────────────────────────┤ │               │
│ ▸ Albums      │ │ ▢ ▢ ▢ ▢ ▢ ▢                               │ │               │
│ ▸ People      │ └──────────────────────────────────────────┘ │               │
│ ▸ Folders     │ Timeline scrubber (dates, draggable)         │               │
│   2024 ▸ …    │                                              │               │
├───────────────┴──────────────────────────────────────────────┴───────────────┤
│ Photo tray: [thumbs of held photos] [Hold ✚] [Clear] │ [★] [⟲][⟳] [Tag…] [Album…] [Share…] │
│ Status bar: 12,431 photos · Scanning 3/7 folders ▮▮▮▯ · Jobs: 2  · Zoom slider │
└──────────────────────────────────────────────────────────────────────────────┘
```

Use GPUI Component: `TitleBar`, `Sidebar`, `Resizable` panels, `Input` (search), `Button`/`ButtonGroup`, `Toggle`/`ToggleGroup` (chips), `Slider` (zoom, editor sliders), `Tabs` (editor), `Popover`/`Dropdown`/`Menu` (context menus), `Modal`/`Dialog` (export, import, confirm), `Notification` (toasts), `Progress`, `Tooltip`, `Badge`, `List`/virtualized list, `ColorPicker` (tint, collage background), `DatePicker` (filters). Use the component names as they exist in the locked `gpui-component` version; adapt names if they differ.

### 9.2 Sidebar
- Sections: Library (All photos, Starred, Recently edited, Imports, Duplicates), Albums (+ New album), People (`faces`), Folders (tree grouped by year of `sort_date`, then folder name; right-click: rename, reveal, folder caption, Folder Manager mode).
- Drop photos on an album to add; on a folder to move (confirm).
- Counts shown as badges.

### 9.3 Grid (F5, F13, F14)
- Virtualized: render only visible rows + 1 screen overscan. Implement as a uniform-height row list where each row is either a group header (fixed height) or a row of N cells; N = floor(width / (cell + gap)). Use GPUI's `uniform_list` or GPUI Component's virtual list (whichever supports variable item kinds; if heights must be uniform, use two row heights via a custom element — record decision).
- Thumbnail zoom slider (96–320 px cell). Cells show: image (aspect-fit, letterboxed), star badge, rating dots on hover, video duration badge, edited badge, unreadable placeholder, selection ring, tray marker.
- Selection: click, Shift+click range (in display order, across groups), Cmd/Ctrl+click toggle, Cmd/Ctrl+A, rubber-band drag selection (P1), Esc clears.
- Context menu: Open, Edit, Star, Rate ▸, Rotate ▸, Add to album ▸, Add tag…, Hold in tray, Export…, Reveal, Rename…, Move to…, Delete…, Hide.
- Keyboard: arrows move focus (wrap rows), Enter opens viewer, Space toggles tray hold, `S` star, `0`–`5` rating, Cmd/Ctrl+R rotate right, Cmd/Ctrl+Shift+R rotate left, Cmd/Ctrl+F focus search, Delete → delete dialog.
- Scroll position anchored to the focused item across re-sorts and window resizes.
- Header shows group title (folder name or "March 2024"), date range, count, folder caption (editable inline on double-click).
- **Photo tray** (Picasa "Photo Tray"): bottom strip; holds a persistent working set across view changes; actions in the tray toolbar apply to tray contents if non-empty, else to grid selection.
- **Timeline scrubber**: horizontal bar of months with density; dragging scrolls the grid to that date; shift-drag sets a date-range filter chip.

### 9.4 Viewer (F6)
- Opens over the library (same window, navigation stack). Fit to window by default; zoom (wheel, pinch, `+`/`-`, `1` = 100%), pan by drag, double-click toggles fit/100%.
- Filmstrip at bottom (current view's ids), arrows to navigate, Home/End.
- Info panel toggle (`I`): file, size, dimensions, date, camera, exposure, GPS (mini-map with `map`), tags, albums, people, caption editor.
- Uses L thumbnail immediately, then proxy render; never blank.
- Esc returns to library with focus on the same photo.

### 9.5 Editor (F7, F19)
- Left panel `Tabs`: **Basic Fixes** (Crop, Straighten, Red-eye, I'm Feeling Lucky, Auto Contrast, Auto Color, Retouch, Fill Light slider), **Tuning** (Fill Light, Highlights, Shadows, Color Temperature sliders, Neutral color picker eyedropper), **Effects** (grid of 12 effect tiles with live mini-previews: Sharpen, Sepia, B&W, Warmify, Film Grain, Tint, Saturation, Soft Focus, Glow, Focal B&W, Graduated Tint, Vignette). Each parametrized effect opens a small param panel with Apply/Cancel.
- Top bar: Undo, Redo, Revert (confirm), Before/After toggle (hold `\`), Export, Back.
- Crop tool: aspect presets (Free, original, 1:1, 4:3, 3:2, 16:9, 5:7, 8:10), draggable handles, rule-of-thirds overlay, Apply/Cancel; keyboard arrows nudge.
- Straighten: slider ±45° with grid overlay and live preview.
- Red-eye: drag rectangles over eyes; Apply.
- Retouch (P1): brush size slider; click target then source.
- Preview renders at proxy resolution at `Interactive` priority; while rendering, previous frame stays visible with a subtle spinner.

### 9.6 Dialogs
Export (§6.10), Import (§6.9), Collage (§6.13, with live preview and layout tabs), Folder Manager (tree with mode per folder), Settings, Confirm delete/move/rename, Duplicate review, Picasa import preview (counts of what will change).

### 9.7 Theming & accessibility
- Light/dark via GPUI Component `Theme`; follow system by default.
- Every interactive element is keyboard reachable with visible focus ring; tooltips include shortcuts.
- Use semantic roles where the component supports them; minimum hit target 24 px.

### 9.8 Test IDs (normative)
Every interactive or asserted element MUST have a stable `ElementId` from `lb_ui::ids` and call `.test_support()` (from `gpui_kit::TestSupportExt`; inert in release builds). Focusable controls call `.test_support()` **before** `.track_focus(&handle)`.

```rust
pub mod ids {
    pub const SEARCH_INPUT: &str = "search-input";
    pub const CHIP_STARRED: &str = "chip-starred";
    pub const CHIP_VIDEO: &str = "chip-video";
    pub const CHIP_FACES: &str = "chip-faces";
    pub const CHIP_GEO: &str = "chip-geo";
    pub const CHIP_EDITED: &str = "chip-edited";
    pub const SIDEBAR: &str = "sidebar";
    pub const SIDEBAR_ALL: &str = "sidebar-all";
    pub const SIDEBAR_STARRED: &str = "sidebar-starred";
    pub const NEW_ALBUM_BUTTON: &str = "new-album";
    pub const GRID: &str = "grid";
    pub const TRAY: &str = "tray";
    pub const TRAY_STAR: &str = "tray-star";
    pub const TRAY_ROTATE_LEFT: &str = "tray-rotate-left";
    pub const TRAY_ROTATE_RIGHT: &str = "tray-rotate-right";
    pub const TRAY_ADD_TAG: &str = "tray-add-tag";
    pub const TRAY_ADD_ALBUM: &str = "tray-add-album";
    pub const TRAY_EXPORT: &str = "tray-export";
    pub const VIEWER: &str = "viewer";
    pub const VIEWER_NEXT: &str = "viewer-next";
    pub const VIEWER_PREV: &str = "viewer-prev";
    pub const EDITOR: &str = "editor";
    pub const EDITOR_TAB_BASIC: &str = "editor-tab-basic";
    pub const EDITOR_TAB_TUNING: &str = "editor-tab-tuning";
    pub const EDITOR_TAB_EFFECTS: &str = "editor-tab-effects";
    pub const EDITOR_LUCKY: &str = "editor-lucky";
    pub const EDITOR_UNDO: &str = "editor-undo";
    pub const EDITOR_REDO: &str = "editor-redo";
    pub const EDITOR_REVERT: &str = "editor-revert";
    pub const SLIDER_FILL_LIGHT: &str = "slider-fill-light";
    pub const SLIDER_HIGHLIGHTS: &str = "slider-highlights";
    pub const SLIDER_SHADOWS: &str = "slider-shadows";
    pub const SLIDER_TEMPERATURE: &str = "slider-temperature";
    pub const EXPORT_DIALOG: &str = "export-dialog";
    pub const EXPORT_CONFIRM: &str = "export-confirm";
    pub const CONFIRM_OK: &str = "confirm-ok";
    pub const CONFIRM_CANCEL: &str = "confirm-cancel";
    // dynamic ids: helpers returning ElementId (e.g. ElementId::NamedInteger)
    pub fn grid_cell(id: PhotoId) -> ElementId;          // "grid-cell", id
    pub fn group_header(idx: usize) -> ElementId;        // "group-header", idx
    pub fn album_item(id: AlbumId) -> ElementId;         // "album-item", id
    pub fn effect_tile(name: &str) -> ElementId;         // "effect-" + name
    pub fn filmstrip_cell(id: PhotoId) -> ElementId;
}
```

---

## 10. Robustness, errors, logging

- Library errors: `thiserror` enums per crate (`CatalogError`, `MediaError`, `ScanError`, `ImagingError`, `ExportError`, …). `lb-app` maps them to user-facing messages (`AppError::user_message()`), shown as toasts; details logged.
- No `unwrap()`/`expect()` in non-test code except for documented invariants (`#![deny(clippy::unwrap_used, clippy::expect_used)]` in library crates; allow in tests and xtask).
- All filesystem writes by Lightbox are atomic (temp + rename) and never target files inside watched roots except explicit outputs (export, collage, import destination, file ops).
- Catalog integrity: run `PRAGMA quick_check` at startup in background; on failure, offer restore from the automatic daily backup (`{data}/backups/catalog-YYYY-MM-DD.sqlite3`, keep 7, via SQLite backup API).
- Crash safety: panic hook logs backtrace and writes `{data}/crash-*.log`; next start shows a notice.
- Logging: `RUST_LOG`-style filter, default `info` (`lightbox=debug` in debug builds); rolling daily files, keep 7.

**AC**: `catalog_backup_rotation`, `catalog_restore_from_backup`, `atomic_write_leaves_no_partial_on_error`, `user_messages_for_all_error_variants` (exhaustive match test).

---

## 11. Performance budgets

Measured by `cargo xtask perf` in **release** on CI runners; failures are reported, and the job fails if a budget is exceeded by > 50% (to absorb runner noise). Synthetic catalogs are generated by inserting rows directly (no image files) unless noted.

| Metric | Budget |
|---|---|
| Open catalog with 100k photos and produce first grid query result (AllPhotos, grouped by month) | ≤ 300 ms |
| Search query (`beach rating:>=3 year:2021`) on 100k rows | ≤ 100 ms |
| `get_many` 200 rows | ≤ 10 ms |
| Scan 2,000 small JPEG fixtures (metadata only, cold) | ≤ 15 s |
| Rescan unchanged 2,000 files | ≤ 2 s |
| Thumbnail S from 12 MP JPEG (DCT-scaled decode) | ≤ 60 ms per image per core |
| Editor proxy render (2048 px, Tuning + Sharpen) | ≤ 120 ms |
| UI: grid scroll frame time with 100k items (measured via a GPUI test that renders 100 frames while scrolling) | p95 ≤ 16 ms on CI (informational on Linux software rendering) |
| Idle memory with 100k-photo catalog, grid open | ≤ 600 MB |

`criterion` benches exist for edit ops, thumbnail generation, query parse, and FTS search.

---

## 12. Testing strategy

### 12.1 Layers

| Layer | Where | Tooling | Runs in CI |
|---|---|---|---|
| Unit | each crate `src/**` | `#[test]`, `proptest` | always |
| Golden image | `lb-imaging/tests`, collage | custom comparator + `tests/goldens` | always |
| Integration (catalog, scan, watcher, services) | `crates/*/tests`, `lb-app/tests` | `tempfile`, inline jobs | always |
| View-model | `lb-app/tests/vm_*.rs` | plain Rust | always |
| CLI e2e | `lb-cli/tests` | `assert_cmd` | always |
| UI integration (GPUI headless) | `lb-ui/tests` | `#[gpui_kit::test]`, `gpui_kit::test::{TestWindowExt, TestAppContextExt}` | always |
| Perf | `xtask perf`, `benches/` | criterion | release job |
| Real model / ffmpeg | `#[ignore]` + env | | nightly |

### 12.2 Test harness crate
Create `crates/lb-testkit` (dev-dependency only):
- `TestHome` (tempdir + `LIGHTBOX_HOME`), `fixture_library("basic") -> (TestHome, Lightbox)` (copies generated fixtures, adds root, scans with inline jobs).
- `assert_originals_unchanged(&home)`: hashes all files under fixture roots against the manifest.
- `FakeTrash`, `FakeClock`, `LocalTileProvider`, `MockDetector`, `MockEmbedder`, `FakeFfmpeg` (records commands, returns canned output).
- `wait_until(timeout, || cond)` for watcher tests.
- Image comparison: `assert_images_close(actual, golden, max_diff, min_psnr)` that on failure writes `target/test-artifacts/<test>/{actual,expected,diff}.png`.

### 12.3 Fixtures
- `cargo xtask fixtures` generates deterministic fixtures into `tests/fixtures/generated/` (gitignored) from `tests/fixtures/manifest.toml`. CI runs it before tests. Generated images are procedural (gradients, color patches, checkerboards, text via `ab_glyph`) so no licensing issues.
- EXIF is written with `little_exif`; if it cannot write a field, build the APP1/TIFF segment by hand (`xtask/src/exif_writer.rs`) — a minimal IFD0 + ExifIFD + GPS IFD writer is ~200 lines and fully under our control.
- **Basic set** (`basic/`), minimum contents:
  - `2021/beach/` 6 JPEGs, EXIF dates 2021-07-01..06, camera "Canon EOS R6", 2 with GPS (Lisbon 38.7223,-9.1393; Porto 41.1579,-8.6291).
  - `2022/city/` 6 JPEGs, orientations 1,3,6,8 + 2 without EXIF but with filename dates (`IMG_20220315_101500.jpg`, `PXL_20220316_101500123.jpg`).
  - `misc/` 1 PNG, 1 GIF (animated, 3 frames), 1 WebP, 1 BMP, 1 TIFF, 1 large 6000×4000 JPEG, 1 tiny 1×1 PNG, 1 unicode name `café ☕ 写真.jpg`, 1 zero-byte `.jpg`, 1 truncated JPEG, 1 text file renamed `.jpg`.
  - `dupes/` exact copy of one beach photo + a 50% resized copy + a recompressed (q=60) copy.
  - `.hidden/` 1 JPEG (must be ignored); `@eaDir/` 1 JPEG (ignored).
  - `picasa/` 4 JPEGs + `.picasa.ini` exercising every mapping in §6.17.
  - `video/` (only if ffmpeg available at fixture time) 2-second MP4 with rotation 90 and `location` tag; otherwise recorded ffprobe JSON fixtures live in `tests/fixtures/ffprobe/*.json` (committed).
- Manifest lists expected counts and metadata for each file; tests read the manifest instead of hard-coding numbers.

### 12.4 Goldens
- Location: `tests/goldens/<suite>/<name>.png`. Regenerate with `cargo xtask goldens --update <suite|name>`.
- **Rule:** golden updates are allowed only when the corresponding spec math changed or a bug was fixed; the agent MUST log the reason in `TASKS.md → Decisions` and visually check the diff image. Never bulk-regenerate to make a failing test pass.

### 12.5 GPUI UI tests
Use the GPUI Kit headless harness. It dispatches real pointer/keyboard events, completes frames, and queries elements registered with `.test_support()`. It does **not** inspect pixels — assert on service state, VM state, element visibility/bounds/focus, and events.

Shape of a UI test (adapt names to the locked version):
```rust
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{TestAppContext, VisualTestContext};
use lb_testkit::fixture_library;
use lb_ui::{ids, LightboxWindow};

#[gpui_kit::test]
async fn ui_star_from_tray_updates_catalog(cx: &mut TestAppContext) {
    let (_home, app) = fixture_library("basic");
    let first = app.photos().all_ids()[0];
    let window = cx.update(|cx| LightboxWindow::open_for_test(app.clone(), cx));

    cx.update_window(window.into(), |_, window, cx| {
        window.click(ids::grid_cell(first), cx);
        window.click(ids::TRAY_STAR, cx);
    }).unwrap();
    cx.run_until_parked();

    assert!(app.photos().get(first).unwrap().starred);
}
```
- `LightboxWindow::open_for_test` opens the main window with test-friendly size (1280×800) and `JobQueue::inline()`.
- Use `TestAppContextExt::wait_for(window, timeout, |window, cx| …)` for async UI conditions (uses the test clock).
- Required UI tests (minimum): `ui_launch_shows_grid_with_groups`, `ui_click_selects_and_shift_click_extends`, `ui_search_filters_grid`, `ui_chip_starred_filters`, `ui_open_viewer_and_navigate`, `ui_editor_lucky_then_undo`, `ui_editor_slider_commits_one_op`, `ui_new_album_and_add_selection`, `ui_tray_hold_and_clear`, `ui_delete_shows_confirm_and_cancels`, `ui_export_dialog_runs_job`, `ui_keyboard_rating_and_star`, `ui_sidebar_navigation_changes_source`, `ui_theme_toggle`.
- If a test needs real pixel rendering, it is out of scope for headless; cover it with a golden test at the `lb-imaging` level instead.

### 12.6 Global invariants (run at end of every integration test via testkit drop guard)
- `assert_originals_unchanged` for fixture roots (except files the test explicitly moved/renamed/deleted, which the test declares).
- `PRAGMA integrity_check` returns `ok`; `PRAGMA foreign_key_check` returns no rows.
- FTS consistency: incremental FTS equals `rebuild_fts()` result for a sample of queries.
- No files left in temp-name patterns (`*.lbtmp`) under fixture roots or cache.

### 12.7 Coverage
`cargo llvm-cov` in CI (Linux job) with thresholds: `lb-core`, `lb-catalog`, `lb-search`, `lb-imaging`, `lb-scan`, `lb-app` ≥ 80% line coverage; workspace ≥ 70%. `lb-ui` excluded from threshold.

---

## 13. CI and quality gates

`cargo xtask ci` is the single command the agent and CI run. It performs, in order, failing fast:
1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
3. Dependency-rule check (§4.2) via `cargo metadata`.
4. `cargo xtask fixtures`
5. `cargo test --workspace --locked` (default features)
6. `cargo test --workspace --all-features --locked`
7. `cargo test -p lb-app --no-default-features --locked` (core without P2 features)
8. `cargo doc --workspace --no-deps --locked` with `RUSTDOCFLAGS="-D warnings"`
9. `cargo deny check` (licenses: allow MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0, OFL-1.1 for fonts; deny GPL/AGPL in the default dependency tree)

GitHub Actions matrix: `ubuntu-latest`, `macos-latest`, `windows-latest`; Linux installs the system packages in §3.2 plus `ffmpeg`; UI tests run under `xvfb-run` on Linux if the headless platform requires a display (record whether it does). Separate jobs: `coverage` (Linux), `perf` (Linux, release, on main only), `nightly` (ignored tests with models + ffmpeg).

Release packaging (P1): macOS `.app` + `.dmg` (unsigned in CI, signing documented), Windows `.msi` via `cargo-wix` or zip, Linux AppImage + `.tar.gz`. App icon from `assets/`.

---

## 14. Milestones (summary; full checklist in TASKS.md)

| M | Name | Contents | Exit criteria |
|---|---|---|---|
| M0 | Scaffold | workspace, toolchain, xtask ci, fixtures, testkit, CI | `cargo xtask ci` green on 3 OS with hello-world window test |
| M1 | Catalog | schema, migrations, repos, FTS, backups | all §5/§10 catalog AC |
| M2 | Media + Scan | decode, metadata, scanner, folder manager | §6.1, §6.2 AC |
| M3 | Watcher | watch, move detection | §6.3 AC |
| M4 | Jobs + Thumbs | job queue, thumbnail cache | §6.4, §6.5 AC |
| M5 | Edit pipeline | ops, goldens, stack semantics | §6.6 AC |
| M6 | Services + CLI | lb-app facade, search, organize, export, VMs, CLI | §6.7, §6.8, §6.10, §7, §8 AC |
| M7 | UI shell | window, sidebar, grid, selection, tray, search, chips | UI tests for library |
| M8 | Viewer + Editor UI | viewer, filmstrip, editor tabs, crop/straighten | UI tests for viewer/editor |
| M9 | P1 features | import, duplicates, slideshow, collage, timeline, settings, theming | §6.9, 6.11–6.13, 6.18 AC |
| M10 | P2 features | map, faces, video, Picasa import | §6.14–6.17 AC |
| M11 | Hardening | perf budgets, coverage, packaging, docs | §11, §12.7, §13 |
| M12 | P3 | movie maker, XMP, HEIC/RAW | respective AC |

---

## 15. Definition of Done (per task)

A task is done only when all of the following hold:
1. Every AC listed for it has a passing automated test with the specified name.
2. `cargo xtask ci` is green locally (and the agent has not disabled, ignored, or weakened any existing test to get there).
3. No new `TODO`/`FIXME` without a linked follow-up task in `TASKS.md`.
4. Public items in library crates have doc comments.
5. `TASKS.md` updated: checkbox ticked, progress log entry, decisions recorded.
6. A single focused commit (or small series) with a conventional-commit message referencing the task ID.
