# Lightbox

Lightbox is a fast, local-first photo manager and non-destructive editor in the spirit of Picasa,
written in Rust with [GPUI Kit](https://crates.io/crates/gpui-kit). It indexes the folders you
choose, shows every photo and video in one scrollable library, lets you organize (stars, ratings,
captions, tags, albums, people), search, fix (edits are stored as data and never touch the
original file), and share (export, slideshow, collage).

- **Originals are sacred.** Files are only changed by explicit file operations (rename, move,
  delete to the trash). Edits are rendered on demand.
- **Local-first.** No account, no cloud, no telemetry. The network is used only for optional map
  tiles and the optional face-model download, and only when you ask.
- **Fast at 100k photos.** Virtualized grid, indexed SQLite catalog, background jobs, cached
  thumbnails (budgets in `SPEC.md` §11, checked by `cargo xtask perf`).
- **Headless-testable.** All behavior lives in UI-agnostic crates; the GUI is a thin shell, and
  everything is also available from `lightbox-cli`.

The full product specification is `SPEC.md`; `TASKS.md` records progress, decisions and the
current status of every task.

## Contents

- [Status](#status)
- [Install](#install)
  - [Download a release](#download-a-release)
  - [Arch Linux](#arch-linux)
  - [What you get](#what-you-get)
  - [Code signing](#code-signing)
- [Build from source](#build-from-source)
- [Data](#data)
- [Use](#use)
- [Develop](#develop)
- [License](#license)

## Status

The services, view-models and command line cover every feature of the specification and are
tested end to end. Some dialogs of the GUI are not drawn yet and are available through
`lightbox-cli` (or the service layer) only: import from a card or folder, duplicates review,
collage, batch rename, the map panel, settings sections other than Photo viewer & file types,
the People face grid with naming and merging, and the Picasa import preview.
`TASKS.md` (Decisions) lists what is deferred and why. The title-bar gear opens
**Photo viewer & file types**. The theme button still cycles the colour scheme.

## Install

### Download a release

Download the archive for your system from the
[releases page](https://github.com/asce4s/lightbox/releases). Each release also has a
`SHA256SUMS` file.

| System | Artifact | Run |
|---|---|---|
| Linux x86_64 | `lightbox-<version>-linux-x86_64.AppImage` or `.tar.gz` | `chmod +x` the AppImage, or run `./lightbox` from the archive |
| macOS 13+ | `lightbox-<version>-macos-<arch>.dmg` or `-app.tar.gz` | drag `Lightbox.app` to Applications |
| Windows 10/11 x64 | `lightbox-<version>-windows-x86_64.zip` | run `lightbox.exe` |

The AppImage bundles the libraries the binary links, except glibc, libstdc++, libgcc and the
Vulkan loader. It still needs a Vulkan driver and the usual font, Wayland and X11 libraries
(`fontconfig`, `freetype`, `libxkbcommon`, `wayland`, `libxcb`, `libX11`).

### Arch Linux

```sh
yay -S lightbox-git
# or
paru -S lightbox-git
```

[`lightbox-git`](https://aur.archlinux.org/packages/lightbox-git) builds the current git revision.
`ffmpeg` is optional and enables video thumbnails.

### What you get

The archives also contain `lightbox-cli`, the command-line companion.

`ffmpeg` and `ffprobe` on the `PATH` enable video thumbnails and metadata (video support works
without them in a reduced form). Face recognition needs the models from `cargo xtask models` and
an ONNX Runtime library (`ORT_DYLIB_PATH`; see [Data](#data)).

### Code signing

Builds are unsigned. On macOS, right-click the app and choose Open the first time (or run
`xattr -dr com.apple.quarantine Lightbox.app`). To sign and notarize your own build, sign
`Lightbox.app` with `codesign --deep --options runtime --sign "Developer ID Application: ..."`,
then `xcrun notarytool submit` the `.dmg` and `xcrun stapler staple` it. On Windows, sign
`lightbox.exe` with `signtool sign` before zipping if you need SmartScreen reputation.

## Build from source

### Toolchain

Rust 1.99 (pinned by `rust-toolchain.toml`; `rustup` installs it on first use).

### System packages

| System | Install |
|---|---|
| Debian / Ubuntu | see below |
| Fedora | `sudo dnf install libxkbcommon-devel libxkbcommon-x11-devel wayland-devel vulkan-loader-devel libxcb-devel fontconfig-devel freetype-devel openssl-devel pkgconf clang` |
| macOS | Xcode command line tools (`xcode-select --install`); `brew install ffmpeg` for video |
| Windows | Visual Studio Build Tools (MSVC, Windows SDK); `winget install ffmpeg` for video |

Debian / Ubuntu:

```sh
sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libssl-dev pkg-config clang
```

Add `ffmpeg` for video, and `libheif-dev` plus `libheif-plugin-libde265` for the `heif` feature.

### Commands

```sh
cargo run -p lightbox --release          # the app
cargo run -p lb-cli -- --help            # the CLI (binary name: lightbox-cli)
cargo xtask package                      # release archive for this OS in dist/
```

### Features

Optional features of `lb-app` (all on by default): `faces`, `video`, `map`. Core code builds and
passes its tests without them (`cargo test -p lb-app --no-default-features`). The `onnx` feature
adds the real face-detection networks and is off by default.

## Data

`LIGHTBOX_HOME` moves everything under one folder (`<home>/data`, `<home>/cache`); without it the
platform's per-user data and cache directories are used.

| What | Location |
|---|---|
| Catalog (SQLite), settings, logs, backups, models | `<data>/catalog.sqlite3`, `<data>/logs`, `<data>/backups`, `<data>/models` |
| Thumbnails and map tiles | `<cache>/thumbs`, `<cache>/tiles` |
| Face models directory override | `LIGHTBOX_FACE_MODELS` |
| ONNX Runtime library | `ORT_DYLIB_PATH`, a `libonnxruntime` next to the models, or the system path |

## Use

### Library

1. Add the folders you want in the library (Folder Manager): **watch** (kept in sync), **scan
   once**, or **exclude**.
2. Browse the grid grouped by folder or date, drag-select, hold photos in the tray, and use the
   timeline scrubber to jump around.
3. Open a photo for the viewer (zoom, filmstrip, info panel) and `E` for the editor: Basic Fixes
   (I'm Feeling Lucky, auto contrast and color, crop, straighten, red eye, fill light), Tuning
   sliders, and Effects. Undo, redo and Revert never change the file.
4. Organize with stars, ratings, captions, tags and albums; find photos with the search box
   (`beach rating:>=3 year:2021 camera:canon -tag:work is:starred`) and the filter chips.
5. Export (size, quality, metadata, watermark), start a slideshow, or build a collage.

### Keyboard shortcuts

`Cmd` on macOS, `Ctrl` elsewhere.

| Where | Keys | Action |
|---|---|---|
| Library | Arrow keys, `Home`, `End` | Move focus (hold `Shift` to extend the selection) |
| Library | `Enter` | Open the focused photo |
| Library | `Space` | Hold or release the focused photo in the tray |
| Library | `S` | Toggle star |
| Library | `0` to `5` | Set rating |
| Library | `E` | Open the editor |
| Library | `Cmd+A` | Select all |
| Library | `Cmd+R` / `Cmd+Shift+R` | Rotate right / left |
| Library | `Cmd+Z` | Undo the last organize action |
| Library | `Cmd+F` | Focus the search box |
| Library | `Delete` / `Backspace` | Delete (asks first) |
| Library | `Esc` | Clear the selection |
| Viewer | `Left` / `Right`, `Home`, `End` | Previous / next / first / last photo |
| Viewer | `+` / `-`, `1` | Zoom in / out, toggle 100% |
| Viewer | `I` | Info panel |
| Viewer | `E` | Open the editor |
| Viewer | `Esc` | Back to the library |
| Editor | `Cmd+Z`, `Cmd+Shift+Z` or `Cmd+Y` | Undo / redo |
| Editor | `\` (hold) | Show the original (before / after) |
| Editor | `Enter` / `Esc` | Apply / cancel the open tool, `Esc` again leaves the editor |
| Editor, crop tool | Arrow keys (`Shift` = 10 px) | Nudge the crop |
| Slideshow | `Space`, `Left` / `Right`, `Esc` | Play or pause, previous / next, exit |

### Default photo viewer

#### Register

Starting Lightbox registers it as an *available* viewer for the picture types it can open. It
does not change the app that currently opens those files.

#### Make default

Becoming the default is a separate step: the gear in the title bar opens
**Photo viewer & file types**, where you tick the types and choose **Make Lightbox the default
photo viewer**. The same action is on the first-run banner ("Open photos from your file manager
with Lightbox?"). **Not now** hides that banner for good. **Restore previous defaults** puts
back the handlers recorded before that change, when the operating system let Lightbox read them.

#### Per system

What the button can do depends on the system:

| System | Available (every launch) | Default |
|---|---|---|
| Linux | `app.lightbox.Viewer.desktop` with a `MimeType` line, plus an icon | writes `mimeapps.list` (and an existing desktop-specific list) under `$XDG_CONFIG_HOME` |
| Windows | per-user ProgID and Open With entries under `HKCU` | opens Settings → Default Apps; Windows does not let an app write the default itself |
| macOS | document types in `Info.plist` (`LSHandlerRank` Alternate) | the installed app opens the system confirmation; a dev build cannot |

Flatpak and Snap builds are out of scope: those packages hand associations to the portal, and
this desktop file is not how they register types.

#### Unregister

Uninstall (or moving the binary) should drop the entries Lightbox added:

```sh
lightbox --unregister-file-types
```

On Linux that removes the viewer desktop file, the icon, and mimeapps lines that point at
Lightbox. On Windows it deletes only the keys recorded in Lightbox's association manifest.
On macOS there is nothing to remove at uninstall time; document types leave with the `.app`.

### Command line

`lightbox-cli` works on the same library as the app (`LIGHTBOX_HOME` applies). Add `--json` for
machine-readable output. Exit codes: 0 success, 1 bad input, 2 runtime failure.

```sh
lightbox-cli roots add ~/Pictures --mode watch
lightbox-cli scan
lightbox-cli query "beach rating:>=3 year:2021"
lightbox-cli star 12 13 14
lightbox-cli rate 12 5
lightbox-cli tag add holiday 12 13
lightbox-cli xmp on                       # keep {file}.xmp sidecars current (also: xmp off, xmp write <ids>)
lightbox-cli album create "Summer"        # also: album add <album-id> <photo-ids>, album list
lightbox-cli edit push 12 '{"op":"sepia"}'  # also: edit show, undo, revert
lightbox-cli render 12 preview.png --max-edge 1280
lightbox-cli export 12 13 --to ~/Exports --long-edge 2048 --quality 85
lightbox-cli dupes                        # exact and similar photos
lightbox-cli collage grid 12 13 14 --out collage.jpg --size 1920x1080
lightbox-cli movie 12 13 14 --out show.mp4 --seconds 3 --fps 30   # slideshow MP4 (needs ffmpeg)
lightbox-cli thumbs warm --size M
lightbox-cli picasa-import ~/Pictures     # legacy .picasa.ini metadata (--overwrite, --dry-run)
lightbox-cli doctor                       # environment report and catalog integrity check
```

`lightbox-cli <command> --help` lists every option.

## Develop

### Architecture

```text
                 ┌────────────────────────────┐   ┌──────────────┐
                 │ apps/lightbox (GUI binary) │   │  lb-cli      │
                 └──────────────┬─────────────┘   └──────┬───────┘
                                │                        │
                       ┌────────▼────────┐               │
                       │     lb-ui       │ gpui-kit      │
                       │  (GPUI views)   │               │
                       └────────┬────────┘               │
                                │ view-models, events    │
                       ┌────────▼────────────────────────▼───────┐
                       │                lb-app                   │
                       │ services · queries · events · vm::*     │
                       └─┬──────┬──────┬──────┬──────┬──────┬────┘
                         │      │      │      │      │      │
              ┌──────────▼┐ ┌───▼───┐ ┌▼─────┐│┌─────▼──┐ ┌─▼────────────────┐
              │lb-catalog │ │lb-scan│ │lb-   │││lb-     │ │ optional features│
              │ SQLite,   │ │ walk, │ │search│││imaging │ │ lb-faces lb-video│
              │ FTS, repos│ │ watch │ │      │││edits,  │ │ lb-geo  lb-picasa│
              └─────┬─────┘ └───┬───┘ └──────┘││collage │ └─────────┬────────┘
                    │           │      ┌──────▼┴┐ └───┬───┘           │
                    │           │      │lb-thumbs│    │               │
                    │           │      │ + jobs  │    │               │
                    │           │      └────┬────┘    │               │
                    └───────────┴───────────┴────┬────┴───────────────┘
                                                 │
                                    ┌────────────▼───────────┐
                                    │ lb-media   (decode,    │
                                    │ metadata, hashing)     │
                                    │ lb-core    (domain)    │
                                    └────────────────────────┘
```

- `lb-core` holds domain types, ids, errors and the edit-stack model; it does no I/O.
- `lb-catalog` is the only crate that contains SQL (schema, migrations, repositories, full-text
  search). `lb-search` turns the query language into a plan for it.
- `lb-media`, `lb-imaging` and `lb-thumbs` decode, edit (`EditStack` rendered on demand), and cache
  thumbnails; `lb-scan` walks and watches folders; `lb-jobs` runs prioritized, cancellable jobs.
- `lb-app` is the UI-agnostic service layer and holds the view-models (`lb_app::vm`) that the GUI
  renders. `lb-ui` depends only on `lb-app`, `lb-core` and `gpui-kit`.
- `lb-faces`, `lb-video`, `lb-geo` and `lb-picasa` are optional or leaf crates, each depending only
  on `lb-core` and `lb-media`. `lb-testkit` is a dev-dependency only.

The dependency rules are enforced by `cargo xtask ci` (SPEC §4.2).

### Checks

```sh
cargo xtask ci          # everything CI runs: fmt, clippy, dependency rules, fixtures, tests
                        # (default, all features, lb-app without features), docs, cargo-deny
cargo xtask fixtures    # regenerate the test fixtures
cargo xtask goldens     # run the golden-image tests (add --update <name> to rewrite one)
cargo xtask perf        # release-mode performance budgets (add --bench for criterion benches)
cargo xtask coverage    # line-coverage thresholds (needs cargo-llvm-cov)
cargo xtask models      # download and verify the face-detection models
cargo xtask package     # build the release archive for this OS
```

Tests that need the network or real models are `#[ignore]`d and run only in the nightly workflow.
`AGENTS.md` describes how the project is built and tested; keep `TASKS.md` up to date.

## License

MIT, see `LICENSE`. Dependencies are limited to permissive licenses (checked by `cargo deny`).
The face models (YuNet, MIT; SFace, Apache-2.0) are downloaded on request, never bundled.
