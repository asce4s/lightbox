# Lightbox Photo Viewer — Spec

**Spec version:** 1.0 · **Date:** 2026-10-17 · **Status:** Ready for implementation · **Milestone:** M13 in `TASKS.md`

> Extends `SPEC.md`. It **supersedes SPEC.md §9.4** (Viewer). Everything in `SPEC.md` (principles, crate rules, testing, Definition of Done) still applies. Read `AGENTS.md` first.
>
> This spec is written against the code base as of commit `7764b68` (M0–M12 done). It names existing types (`ViewerVm`, `LightboxWindow`, `Route::Viewer`, `Stage`, `EditService::preview_file`, `Clock`, `FakeClock`, `ids!`, `xtask/src/package.rs`). If a name has moved, follow the code and record the mapping in `TASKS.md → Decisions`.

---

## V0. What we are building

Picasa had two ways of looking at one photo:

1. **Picasa Photo Viewer.** A fast, borderless viewer that opened when you double-clicked an image in Explorer. The desktop showed through behind it, darkened. The photo zoomed in with a short animation, a floating toolbar and thumbnail strip sat at the bottom, and clicking outside the photo closed it.
2. **Opening a photo from the library**, which took over the window.

Lightbox gets one viewer that does both:

| Mode | Opened by | Window | Background |
|---|---|---|---|
| **Overlay** | Double-click / Enter on a grid thumbnail (and filmstrip, search results, albums…) | Drawn on top of the library window | "Ambient" blur of the photo itself, fading in over the library |
| **Standalone** | The OS (Explorer, Finder, Files…) opening an image with Lightbox, or `lightbox view <files>` | Its own borderless window covering one display | The desktop, blurred and darkened (where the OS supports it), else ambient |

Both modes share one implementation: the **viewer stage** (image + backdrop + chrome + motion), the **view-model**, and the **loader**.

And Lightbox can **register itself as the default image viewer** on Windows, macOS and Linux, only when the user asks it to.

### V0.1 Feature ids (added to SPEC.md §2.1)

| # | Feature | Priority |
|---|---|---|
| F28 | Viewer overlay with ambient backdrop and open/close/navigate animations | P0 |
| F29 | Standalone viewer mode (`lightbox view`), single instance, works for files outside the library | P0 |
| F30 | Register as default image viewer (per OS), with restore | P0 |
| F31 | Animated GIF/WebP playback, copy image to clipboard, transparency checkerboard | P1 |
| F32 | Tiled full-resolution zoom for very large images (> 100 MP) | P2 |

### V0.2 Non-goals

Printing, wallpaper setting, sharing/upload, in-viewer video playback (videos still hand off to the OS player as today), editing inside the standalone viewer (the "Edit" button opens the library editor), true OS fullscreen spaces as the default (available via `F`/`F11`).

---

## V1. Architecture

### V1.1 Where the code goes

```
lb-core      + natural_cmp (filename natural sort), RectF helpers if missing
lb-imaging   + ambient.rs           ambient backdrop renderer (pure, golden-tested)
lb-app       + viewer/              UI-agnostic viewer logic
               ├─ entry.rs          ViewerEntry, NavSet (library ids, folder siblings, explicit files)
               ├─ transform.rs      fit rect, zoom-at-anchor, pan clamp
               ├─ motion.rs         transitions + easing, sampled from Clock::elapsed()
               ├─ loader.rs         progressive load (L0..L3), neighbour preload, memory budget
               ├─ actions.rs        what is allowed for catalog vs non-catalog files
               └─ service.rs        ViewerService (proxies for paths, EXIF for paths, add folder)
             ~ vm/viewer.rs         ViewerVm generalised to ViewerEntry + transform + motion
             + instance.rs          single-instance server/client (wraps lb-ipc)
             + assoc.rs             AssocService (wraps lb-assoc; status, register, make default, restore)
lb-ipc       NEW  local-socket single-instance protocol (→ lb-core only)
lb-assoc     NEW  file-association plans + per-OS executors (→ lb-core only)
lb-ui        ~ viewer.rs            split into ViewerStage (shared) + overlay host
             + viewer_window.rs     standalone ViewerWindow
             + settings_viewer.rs   "Photo viewer & file types" settings overlay + first-run banner
             ~ lib.rs               run(app, Launch) instead of run(app)
apps/lightbox ~ main.rs             parse launch args, single instance, viewer-only start
xtask        ~ package.rs           Info.plist document types, viewer .desktop entry
```

Dependency rules (add to SPEC.md §4.2 and to the `xtask ci` checker):

```
lb-ipc, lb-assoc → lb-core only
lb-app           → lb-ipc, lb-assoc (plus existing)
lb-ui            → unchanged (lb-app, lb-core, gpui-kit)
apps/lightbox    → unchanged (lb-app, lb-ui)
```

New third-party crates (pin exact, check with `cargo deny`; record in Decisions):

| Crate | Use | Scope |
|---|---|---|
| `interprocess` | local sockets / named pipes | lb-ipc |
| `winreg` | HKCU registry writes | lb-assoc, `cfg(windows)` |
| `windows` (feature `Win32_UI_Shell`) | `SHChangeNotify` | lb-assoc, `cfg(windows)` |
| `objc2`, `objc2-foundation`, `objc2-app-kit` | `NSWorkspace` default handler APIs, reduce-motion/transparency flags | lb-assoc, `cfg(target_os = "macos")` |
| `core-foundation` | `LSCopyDefaultRoleHandlerForContentType` fallback | lb-assoc, macOS |
| `percent-encoding` (or `url`) | `file://` URL → path | lb-app |

Natural sort is implemented in `lb-core` (no crate).

### V1.2 Key types (lb-app)

```rust
/// One thing the viewer can show.
pub struct ViewerEntry {
    pub path: PathBuf,              // always set
    pub photo: Option<PhotoId>,     // Some when the file is an indexed, non-missing catalog photo
    pub media_type: MediaType,      // from catalog, else from extension/sniffing
}

pub enum NavSource {
    /// The current library view (grid order), exactly as the grid shows it.
    Library { ids: Vec<PhotoId> },
    /// All supported media in `dir` (non-recursive, hidden files skipped), sorted by `sort`.
    Folder { dir: PathBuf, sort: FolderSort },
    /// Exactly these files, in this order (multi-file open from a file manager).
    Files(Vec<PathBuf>),
}

pub enum FolderSort { NameNatural, DateModified, DateTaken }   // default NameNatural

pub struct NavSet { /* entries + index + source; supports refresh on fs changes */ }

pub enum ViewerMode { Overlay, Standalone }

pub struct RectF { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }   // logical px, window space
```

`ViewerVm` changes (existing API kept where possible; record signature changes in Decisions):
- `open(lb, ids, start)` stays for the library and builds `NavSource::Library`.
- New `open_paths(lb, paths: Vec<PathBuf>, settings) -> Result<ViewerVm, ViewerOpenError>`: one path → `NavSource::Folder` of its parent, starting at that file; several → `NavSource::Files`.
- `current()` becomes `current_entry() -> &ViewerEntry`; keep `current_photo() -> Option<PhotoId>` for library callers. Tests that called `current()` are updated mechanically (allowed: it is a signature change, not weaker behaviour).
- Zoom and pan move into `ViewTransform` (§V5). `ZoomMode` stays as the label/intent (`Fit`, `Actual`, `Factor`).

---

## V2. Entry points and launch

### V2.1 From the library (overlay)

| Gesture | Result |
|---|---|
| Double-click a grid cell | Open overlay on that photo, animating from that cell (§V4.2) |
| `Enter` on the focused cell | Same |
| Single click | Selects (unchanged). Setting `viewer.single_click_opens` (default `false`) makes a plain single click (no modifiers, no drag) open the viewer, like a web gallery |
| Videos | Unchanged: handed to the OS player, no overlay |
| Filmstrip / timeline / search results / album / person views | Same as grid: the NavSet is the current view order |

### V2.2 From the OS or a terminal (standalone)

Command-line grammar of the `lightbox` GUI binary (parsed in `apps/lightbox`, pure function `parse_launch(args, platform) -> Launch`, unit-tested):

```
lightbox                         → Launch::Library
lightbox --library               → Launch::Library
lightbox view <path>...          → Launch::View { paths }
lightbox <path>...               → Launch::View { paths }   (paths that exist and are files)
lightbox file:///…               → Launch::View             (percent-decoded file URLs)
lightbox view --perf-json <path> → Launch::View { perf: true }   (§V12)
```
- macOS adds `-psn_*` arguments when launched by Finder on old systems; ignore them.
- Relative paths are resolved against the process working directory. Directories are accepted: `lightbox view <dir>` opens the first supported file of that folder in natural order.
- Paths that do not exist or are not supported images/videos still open the viewer and show an error card (§V9), so the user sees *why* nothing opened.

**macOS:** Finder does not pass files in `argv`; it sends an "open documents" Apple Event. GPUI exposes this as `Application::on_open_urls(FnMut(Vec<String>))` (file URLs). Register it **before** `run`. If the app was launched by such an event, the event arrives shortly after launch: on macOS, when `argv` has no paths, wait up to **250 ms** after finish-launching for `on_open_urls` before deciding to open the library window. If URLs arrive, open only the viewer. Also handle `on_reopen` (Dock click): show the library window.

### V2.3 Single instance (lb-ipc)

Exactly one Lightbox GUI process runs per user and data directory. It may own a library window, a viewer window, or both.

1. On start, `apps/lightbox` asks `lb_app::instance::connect(paths)`.
2. If a server answers, send the launch as a message, wait for the acknowledgement (≤ 1 s), exit with code 0. The running instance opens the viewer (or raises the library window) and activates itself.
3. If nobody answers, become the server (`bind`) and continue starting up. If `bind` fails because another process won the race, retry `connect` up to 3 times, 50 ms apart.

Requirements:
- Endpoint name derived from the data dir: `lightbox-{first 16 hex of blake3(canonical data dir)}`, so `LIGHTBOX_HOME` tests are isolated. Unix: a socket in `$XDG_RUNTIME_DIR` (Linux) or `$TMPDIR` (macOS), mode `0600`; a stale socket file (connect refused) is removed and re-bound. Windows: a named pipe restricted to the current user.
- Protocol: newline-delimited JSON, versioned.
  ```json
  {"v":1,"cmd":"open_viewer","paths":["/abs/a.jpg"],"cwd":"/abs"}
  {"v":1,"cmd":"show_library"}
  → {"v":1,"ok":true}   or   {"v":1,"ok":false,"error":"..."}
  ```
  Unknown `v` or `cmd` → `ok:false`; the client then falls back to starting normally only if the server is gone. Messages > 1 MiB are rejected.
- The server runs on a background thread and forwards launches to the GPUI foreground via a channel. The UI drains it in its existing event pump (`LightboxWindow::pump`) and, when no library window exists, in the viewer window's pump.

### V2.4 Viewer-only start (fast path)

When the process starts with `Launch::View`:
- Open the catalog normally (`Lightbox::open_default`). It is fast (measured in M11), and it lets catalog photos show stars and edits. **Do not** call `library().rescan_all()` and **do not** start watchers. Thumbnail and decode jobs run as usual.
- Open only the `ViewerWindow` (§V3.2). No library window.
- If the user opens the library from the viewer ("Open in Lightbox", §V8), or a later launch asks for it, open the library window and **then** start the startup rescan and watchers exactly as `main.rs` does today.
- `QuitMode`: the process exits when its last window closes (existing behaviour).

`lb_ui::run(app)` becomes `lb_ui::run(app, launch: Launch, instance: InstanceServer)`. `apps/lightbox` stays thin.

---

## V3. Windows and layers

### V3.1 Overlay (inside the library window)

`Route::Viewer` no longer replaces the library. It draws the library **underneath** and the viewer stage **on top** (an absolutely positioned layer filling the window):

```
LightboxWindow root
├─ library chrome + grid + tray …           (drawn while backdrop_opacity < 1, or while closing)
└─ ViewerStage (absolute, inset 0)
   ├─ backdrop layer                         (VIEWER_BACKDROP; ambient image + tint; click → close)
   ├─ outgoing image (during navigate)
   ├─ image (VIEWER_IMAGE)                   (absolutely positioned at the presented rect)
   ├─ error card / spinner (when applicable)
   └─ chrome: top bar, bottom toolbar, filmstrip, info panel   (fade with chrome_opacity)
```
- Once the open transition has finished and the backdrop is fully opaque, the library layer may be skipped from rendering (optimisation). It must be drawn again from the first frame of a close.
- Keyboard focus moves to the stage while open, and back to the grid on close (existing `ui_viewer_escape_returns_focus` keeps passing).
- The editor and slideshow routes keep working from the viewer (`E`, `Enter`/`F5`). Returning from the editor returns to the overlay without replaying the open animation.

### V3.2 Standalone `ViewerWindow`

A second GPUI window type, `lb_ui::ViewerWindow`, whose root is a `ViewerStage` in `ViewerMode::Standalone`.

`WindowOptions`:
- Borderless: no title bar and no window decorations. Not resizable by the user; it covers a display.
- Bounds: the full bounds of the display under the mouse cursor if the platform layer exposes the cursor position, else the display of the most recently active Lightbox window, else the primary display. On macOS, if the menu bar cannot be covered at a normal window level, use the display's visible bounds. Do not use an OS fullscreen space by default (it animates and switches spaces); `F`/`F11` toggles real fullscreen via `Window::toggle_fullscreen`.
- `window_background`: per backdrop mode (§V6). `Blurred` for `desktop_blur`, `Opaque` otherwise.
- Focused and activated on open (`activate_window`, app activation).
- One viewer window per process. A new `open_viewer` launch **replaces** its NavSet and plays the standalone open transition again (§V4.3). It does not open a second window.
- Window/app title: `"{file name} — Lightbox"` (task switcher, accessibility).

Closing the stage (Esc, ×, backdrop click) plays the close transition, then closes the window.

### V3.3 Stage geometry

All geometry is in logical pixels in **stage space** (overlay: window space; standalone: window space of the viewer window). It is derived from the stage size and fixed chrome sizes, as M8 already does (no layout measurement needed). The only measured value is the source thumbnail rect (§V4.2).

```
Stage: W × H
Chrome (overlay and standalone):
  TOP_BAR      = 48 px  (file name, counter, close ×)
  TOOLBAR      = 56 px  (floating pill, centred, 12 px above bottom or above filmstrip)
  FILMSTRIP    = 84 px  (existing FILMSTRIP_HEIGHT), toggle `viewer.show_filmstrip` (default true)
  INFO_PANEL   = 280 px (existing INFO_WIDTH), slides in from the right when open

Fit area = stage minus insets:
  top    = TOP_BAR
  bottom = TOOLBAR + 12 + (FILMSTRIP if shown) + 8
  left   = M,  right = M + (INFO_PANEL if open)
  where M = max(24, round(0.03 · min(W, H)))
```
The fit area does **not** change when the chrome auto-hides. The photo must not jump when the mouse stops moving.

---

## V4. Motion

Motion is the point of this feature. It must be smooth, interruptible, and deterministic in tests.

### V4.1 Model (lb-app `viewer::motion`)

Do **not** use GPUI's `with_animation`. It runs on wall time, and `gpui-kit`'s TESTING.md notes that a test-clock wait is not an animation clock. Motion is a pure function of time owned by the VM:

```rust
pub struct Presented {
    pub image: Option<LayerState>,     // current photo
    pub outgoing: Option<LayerState>,  // previous photo during a navigate transition
    pub backdrop_opacity: f32,         // 0..1
    pub backdrop_crossfade: f32,       // 0..1 from previous ambient to current ambient
    pub chrome_opacity: f32,           // 0..1 (product of transition and auto-hide)
}
pub struct LayerState { pub rect: RectF, pub opacity: f32, pub corner_radius: f32 }

impl ViewerVm {
    pub fn presented(&self, now: Duration) -> Presented;   // now = Clock::elapsed()
    pub fn is_animating(&self, now: Duration) -> bool;
}
```
- The view renders `presented(clock.elapsed())` every frame and calls `window.request_animation_frame()` while `is_animating`. When idle it requests nothing, so CPU use is ~0.
- The VM receives the clock from `Lightbox::clock()`, so `FakeClock` drives UI tests: advance, `render_frame`, assert bounds.
- **Interruption:** any new transition starts from the *currently presented* state sampled at the moment of the input, never from the previous target. There are no visual jumps. `Presented` is continuous across retargets (tested).

### V4.2 Transitions (normative values)

Easing functions (t ∈ [0,1]): `out_cubic(t) = 1 − (1 − t)³`, `in_out_cubic(t) = t < .5 ? 4t³ : 1 − (−2t + 2)³ / 2`, `out_quad(t) = 1 − (1 − t)²`, `in_cubic(t) = t³`. Rect interpolation is component-wise on (x, y, w, h).

| Transition | Duration | Image | Backdrop | Chrome |
|---|---|---|---|---|
| **Open from thumbnail** (overlay) | 260 ms | rect: source thumbnail rect → fit rect, `out_cubic`; corner radius 6 → 0; opacity 1 | opacity 0 → 1 over the first 200 ms, `out_quad` | 0 → 1 from 160 ms to 260 ms, linear |
| **Open, no source rect** (standalone, or source not visible) | 220 ms | scale 0.92 → 1 about the fit-rect centre, opacity 0 → 1, `out_cubic` | 0 → 1 over 180 ms, `out_quad` | 0 → 1 from 120 ms to 220 ms |
| **Close to thumbnail** (overlay, cell visible) | 220 ms | rect: presented → cell rect, `in_out_cubic`; corner radius 0 → 6 | 1 → 0, linear | 1 → 0 over the first 80 ms |
| **Close, no target** | 160 ms | scale 1 → 0.96, opacity 1 → 0, `in_cubic` | 1 → 0, linear | 1 → 0 over the first 80 ms |
| **Navigate** next (dir=+1) / prev (dir=−1) | 200 ms | outgoing: translate x 0 → −dir·0.06·W, opacity 1 → 0; incoming: from +dir·0.06·W → 0, opacity 0 → 1; `out_cubic` | ambient crossfade 0 → 1 over 320 ms, linear | unchanged |
| **Zoom** (wheel, `+`/`-`, buttons) | 140 ms | scale interpolated in log space, `out_quad`; offset recomputed every frame so the anchor point stays fixed (§V5.2) | — | — |
| **Fit ↔ 100 %** (double-click, `1`, buttons) | 180 ms | as Zoom, anchor = click point (double-click) or stage centre | — | — |
| **Info panel toggle / window resize** | 200 ms | rect → new fit rect, `out_cubic` (only when zoom mode is Fit) | — | — |
| **Chrome auto-hide** | fade out 200 ms after 2.5 s without pointer movement or key press; fade in 120 ms | — | — | as stated |
| **Quality swap** (L0/L1 → L2 → L3, §V7) | 120 ms | crossfade between the two sources at the same rect | — | — |
| **End bounce** (next at last item, `viewer.wrap = false`) | 160 ms | translate x by −dir·12 px and back, `out_quad` | — | — |

**Source thumbnail rect (overlay open and close).** This is the rect of the *image content* inside the grid cell, not the cell: the thumbnail is aspect-fitted (`ObjectFit::Contain`) in the cell, so compute the contain-fit of the photo's aspect inside the cell rect. The cell rect in window space = grid origin (already measured with the `canvas` element used by the rubber band, Decision M9.8) + `GridLayout` row top − scroll offset + column offset. Add `GridLayout::cell_rect(group, index) -> RectF` and `LibraryVm::thumbnail_rect(photo, grid_origin, scroll) -> Option<RectF>` (None when the cell is not on screen). The same is done for filmstrip cells (`filmstrip_cell` ids) when the viewer was opened from somewhere else.

**On close**, if the photo now shown differs from the one opened (user navigated) and its cell is off-screen, scroll the grid **without animation** so the cell is visible (existing `reveal_focused`), compute its rect, then play "close to thumbnail". If it still cannot be found (filtered out, deleted), play "close, no target".

**Rapid navigation.** When a navigate input arrives less than 150 ms after the previous one (key repeat, fast clicks), skip the slide. The new photo appears immediately at its fit rect, and only the ambient crossfade runs (shortened to 120 ms). Intermediate photos are never fully decoded (§V7.3).

### V4.3 Reduced motion

Motion level = `viewer.motion` setting (`auto` | `full` | `reduced` | `off`), where `auto` follows `App::reduce_motion()` (GPUI) / the OS preference.
- `reduced`: every geometric transition (open, close, navigate, zoom, bounce) becomes a 120 ms opacity crossfade at the final rect. Chrome fades are kept.
- `off`: all durations 0.

### V4.4 Frame budget

During any transition the main thread does no I/O, no decode, no image resize and no allocation proportional to image size. It only samples `presented` and lays out a handful of absolutely positioned elements. Target 60 fps at 4K. Decode and ambient rendering run on the background executor or job queue (§V7).

---

## V5. Fit, zoom and pan (lb-app `viewer::transform`)

### V5.1 Fit

`image` = displayed pixel size of the current entry (after EXIF orientation and the edit stack: the existing `viewer_image_size` logic). `area` = fit area (§V3.3).

```
s_fit = min(area.w / image.w, area.h / image.h)
if !viewer.enlarge_small (default false): s_fit = min(s_fit, 1.0)     // small photos stay 1:1, like Picasa
fit_rect = image scaled by s_fit, centred in area, then rounded to whole logical pixels
```

### V5.2 Transform

`ViewTransform { scale: f32, offset: (f32, f32) }` maps image pixel p to stage point `offset + p·scale`. In Fit mode it equals the fit rect.

- **Zoom at anchor** a (stage point) to scale s': `offset' = a − (a − offset)·(s'/s)`. The image point under the cursor stays under the cursor. Tested with proptest.
- **Limits:** `s_min = s_fit`. You cannot zoom out past fit; zooming out to `s_fit` snaps back to Fit mode and recentres. `s_max = 16.0` (existing `ZOOM_LIMITS.1`). Step factor 1.25 (existing `ZOOM_STEP`).
- **Pan clamp:** on each axis, if the scaled image is smaller than the stage on that axis, centre it in the fit area; otherwise clamp the offset so the image edge never leaves the stage edge (`offset.x ∈ [W − image.w·s, 0]`).
- Drag pans only when zoomed beyond fit. A press/release that moves more than 4 px is a drag, not a click, so it never closes the viewer or toggles zoom.

### V5.3 Input mapping

| Input | Action |
|---|---|
| Mouse wheel, `viewer.wheel = zoom` (default, Picasa behaviour) | Zoom at cursor. `ScrollDelta::Lines(d)`: one step per 1.0 of `d.y` (accumulate fractions). `ScrollDelta::Pixels(d)`: continuous, `factor = exp(−d.y / 300)` |
| Mouse wheel, `viewer.wheel = navigate` | Wheel down = next, up = prev (one per notch, 120 ms debounce) |
| `Ctrl`/`Cmd` + wheel | Always zoom at cursor |
| Trackpad horizontal swipe (Pixels, \|dx\| > 2·\|dy\|, accumulated ≥ 120 px, not zoomed) | Next / prev |
| Pinch / magnify gesture | Zoom at gesture centre, if the locked GPUI version exposes such an event; otherwise skip and record in Decisions |
| Double-click on image | Toggle Fit ↔ 100 % anchored at the click point |
| Click on backdrop (outside the image rect and outside chrome) | Close |
| Click on image | Nothing |
| Drag (zoomed) | Pan |

Keyboard (stage focused):

| Key | Action |
|---|---|
| `→`, `Space`, `PageDown` | Next |
| `←`, `Shift+Space`, `PageUp` | Previous |
| `Home` / `End` | First / last |
| `Esc` | Close (exits real fullscreen first if active) |
| `+` / `=` / `-` | Zoom in / out at stage centre |
| `0` | Fit |
| `1` | Toggle Fit ↔ 100 % |
| `Ctrl/Cmd+R`, `Ctrl/Cmd+Shift+R` | Rotate right / left |
| `S` | Toggle star (catalog photos) |
| `Shift+0` … `Shift+5` | Set rating 0–5 (catalog photos). If GPUI reports these as symbols (`)`, `!`, …) on some keyboard layouts, map by key code and record it |
| `I` | Info panel |
| `E` | Edit in library editor (catalog photos) |
| `Enter`, `F5` | Slideshow from the current photo over the same NavSet |
| `F`, `F11` | Toggle real fullscreen (standalone) |
| `Delete` | Delete to trash (with confirm) |
| `Ctrl/Cmd+C` | Copy image (P1, §V8) |
| `P` | Pause/resume animated GIF/WebP (P1) |

The grid's existing `0`–`5` rating keys stay unchanged in the library. In the viewer, `0` is Fit (Picasa/most viewers), so ratings move to `Shift+digit`.

---

## V6. Backdrop

### V6.1 Modes

Setting `viewer.backdrop`: `auto` | `desktop_blur` | `ambient` | `dim`.

| Mode | Overlay | Standalone |
|---|---|---|
| `desktop_blur` | not available (GPUI cannot blur in-window content) → `ambient` | Window background `Blurred` + tint layer `rgba(0,0,0,0.55)` |
| `ambient` | Ambient texture (§V6.2) fading in over the library | Same, opaque window |
| `dim` | Solid `rgba(0,0,0,0.88)` over the library | Opaque `#0b0b0b` |

`auto` resolves to:
- Overlay → `ambient`.
- Standalone → `desktop_blur` on macOS and Windows, `ambient` on Linux. **But** use `ambient` if the OS "reduce transparency" preference is on (macOS `NSWorkspace.accessibilityDisplayShouldReduceTransparency`; Windows `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\EnableTransparency = 0`).
- The agent MUST check on each OS (CI screenshots are not available, so do it manually or with a recorded note) whether `WindowBackgroundAppearance::Blurred` actually blurs on that platform with the locked GPUI version. Record the result per OS in Decisions. If it does not blur on a platform, change `auto` for that platform to `ambient` (and keep `MicaAltBackdrop` as the Windows 11 alternative if `Blurred` is unsupported there).

In `desktop_blur` the OS blur appears with the window (it cannot be animated). The tint and the image animate as in §V4.2.

### V6.2 Ambient backdrop (lb-imaging `ambient`, normative)

A soft, darkened, colour-rich blur derived from the photo itself, the same idea as album-art backgrounds. It is cheap because it starts from the small thumbnail that is already cached.

```rust
pub struct AmbientParams { pub tex_long_edge: u32 /* 64 */, pub sigma: f32 /* 3.0 */,
    pub saturation: f32 /* +0.25 */, pub gain: f32 /* 0.40 */, pub lift: f32 /* 0.04 */,
    pub vignette: f32 /* 0.35 */, pub out_scale: u32 /* 4 */ }
pub fn render_ambient(thumb: &RgbaImage, stage: (u32, u32), p: &AmbientParams) -> RgbaImage;
```
Steps (sRGB-encoded values in [0,1], as SPEC.md §6.6.2):
1. **Cover-crop** `thumb` to the stage aspect ratio (centre crop).
2. **Downscale** with a box (area-average) filter to long edge `tex_long_edge` (64). The short edge is `max(1, round(64 · short/long))`.
3. **Blur:** separable Gaussian, σ = 3.0 px, radius 9, edge clamp (reuse lb-imaging's blur).
4. **Grade:** saturation `v' = L + (v − L)(1 + 0.25)`, then `v'' = lift + gain·v'` (= 0.04 + 0.40·v'). Clamp.
5. **Vignette:** multiply by `1 − vignette · smoothstep(0.55, 1.0, d)`, where d = distance from the centre normalised so that the corners are 1.
6. **Upscale** with Catmull-Rom bicubic to `ceil(stage / out_scale)` (¼ of the stage size), then quantise to RGBA8 with a 4×4 ordered (Bayer) dither of amplitude 1/255 to avoid banding in dark gradients. Alpha = 255.
7. The UI draws the result stretched to the full stage (`ObjectFit::Fill`). The GPU's bilinear 4× magnification of an already-smooth image is invisible.

Rules:
- Source = the S or M thumbnail of the entry *with edits applied* (existing thumb cache). For non-catalog files, the L0/L1 image of the loader (§V7).
- Output is cached like the existing previews (file-backed so `img(path)` caching stays correct, Decision M8): `{cache}/viewer/ambient/{blake3(source key ‖ tex size ‖ params version)}.png`. Prune to 64 files (LRU by access tick, like the thumb cache).
- Stage sizes are bucketed to multiples of 32 px before rendering, and regeneration after a window resize waits for 150 ms of no further resizing (the old texture is stretched meanwhile).
- Computed on the background executor at Interactive priority. Until it is ready, the backdrop shows `dim` colour. It is normally ready before the 200 ms backdrop fade ends; the crossfade hides the swap.
- Brightness cap: no output pixel channel above `lift + gain + dither` (0.44 + 1/255), so white text/icons on it always have enough contrast. This is tested.
- Performance: ≤ 8 ms for a 4K stage on one core (release); this is tested as a perf check.

---

## V7. Loading pipeline (lb-app `viewer::loader`)

### V7.1 Levels

| Level | Source | When | Used for |
|---|---|---|---|
| **L0** | Thumbnail already in memory/disk cache (grid's S/M/L) | Instantly (overlay); catalog files in standalone | The texture that animates during open/close. **No I/O on the main thread** to obtain it: overlay uses the cached path the grid already drew |
| **L1** | Embedded EXIF JPEG thumbnail (≥ 160 px), read with the existing EXIF reader | Standalone, non-catalog files, within ~50 ms | First image shown in standalone when L0 is missing |
| **L2** | Screen proxy: decode with DCT scaling (`jpeg-decoder` `scale()`, existing) to the smallest scale ≥ fit size × display scale factor, then Lanczos to exactly that pixel size; apply the edit stack for catalog photos (existing `EditService::preview_file` / `Stage` path; for non-catalog files a new `ViewerService::proxy_for_path(path, edge)` writes `{cache}/viewer/files/{quick_key}-{edge}.jpg`) | Immediately after open/navigate | The normal sharp image |
| **L3** | Full resolution (existing `ZOOM_EDGE` 8192 render, up to the image size) | Only when zoom scale × image size exceeds the L2 pixel size | Zoomed views |

- Swaps between levels use the 120 ms quality crossfade at the same rect. The displayed rect never changes because of a level swap (all levels share the oriented aspect ratio).
- If L0 and L1 are both unavailable and L2 is not ready after **120 ms**, show the spinner (`VIEWER_LOADING`) centred in the fit area. The open transition still runs, with the backdrop only.
- Cache key for catalog photos is the existing `frame_key(id, stack, edge)`. For files it is `(quick_key, edge)`. Changing the file (mtime/size) changes `quick_key`.

### V7.2 Neighbour preload

After the current L2 is shown, preload L2 for index ±1, then ±2 (direction of travel first). Preloads are `Foreground` priority, the current photo is `Interactive`. Navigating cancels preloads that are now outside ±2. Navigating to a preloaded neighbour shows L2 on the first frame (no spinner, no decode).

### V7.3 Rapid navigation

While navigation inputs arrive faster than 150 ms apart, request only L0/L1 for the photos passed through. Start the L2 decode for a photo only once it has stayed current for 120 ms.

### V7.4 Memory

The viewer keeps at most 512 MB of decoded image memory (L2/L3, counted as w·h·4), evicting the farthest from the current index first. This budget is separate from the grid's thumbnail memory budget.

---

## V8. Chrome and actions

### V8.1 Layout

```
┌────────────────────────────────────────────────────────────────────────────┐
│  IMG_2041.jpg · 12 / 48 · 3 Jul 2021 14:02                            ✕   │ ← top bar (gradient scrim)
│                                                                            │
│                   ┌──────────────────────────────┐                          │
│                   │                              │                          │
│                   │            photo             │   (drop shadow:          │
│                   │                              │    0 12 40 rgba(0,0,0,.5))│
│                   └──────────────────────────────┘                          │
│                                                                            │
│      ╭──────────────────────────────────────────────────────────────╮      │
│      │ ◀  ▶  ▷  │  ⟲  ⟳  │  ☆  │  ⤢ Fit  1:1  100%  │  ⓘ  ✎  🗑  ⧉ │      │ ← floating toolbar
│      ╰──────────────────────────────────────────────────────────────╯      │
│  ▢ ▢ ▢ ▢ ▢ [▣] ▢ ▢ ▢ ▢ ▢                                                    │ ← filmstrip (centred on current)
└────────────────────────────────────────────────────────────────────────────┘
```
- The top bar and filmstrip sit on a subtle vertical gradient scrim (`rgba(0,0,0,0.45)` → transparent) so they stay legible over any backdrop. The toolbar is a rounded pill (radius 14) with `rgba(20,20,20,0.72)` fill and a 1 px `rgba(255,255,255,0.08)` border.
- Toolbar groups: navigation (prev, next, slideshow) · rotate (left, right) · star (+ rating on hover) · zoom (fit, 1:1, zoom label) · info, edit, delete, open-in-Lightbox / add-folder.
- Each button has a tooltip with its shortcut. Disabled buttons are visibly dimmed and do nothing when clicked (existing Decision: tests assert "click does nothing").
- Chrome is always dark-themed, whatever the library theme, because it sits on a dark backdrop. Text `#f2f2f2`, secondary `#b0b0b0`.
- The info panel (existing content: file, size, dimensions, date, camera, exposure, GPS, tags, albums, people, caption) slides in from the right over the backdrop. It uses a translucent dark panel and works for non-catalog files: EXIF is read via lb-media for the path, and catalog-only fields are hidden.

### V8.2 What each entry can do

| Action | Catalog photo (`entry.photo = Some`) | File outside the library |
|---|---|---|
| Navigate, zoom, pan, info, slideshow | ✓ | ✓ |
| Rotate | Pushes `RotateQuarter` to the edit stack (non-destructive, existing EditService); thumbnail and L2 update | **View-only** rotation for this session. A hint chip: "Rotation isn't saved — add this folder to Lightbox to keep edits" |
| Star / rating | ✓ (existing PhotoService, history/undo) | Disabled. Tooltip: "Add this folder to Lightbox to star photos" |
| Edit (`E`) | Opens library window + editor on this photo | Disabled (same tooltip) |
| Delete | Confirm, then existing delete-to-trash | Confirm, then `TrashBackend` on the path. Never hard-delete |
| Open in Lightbox | Opens/raises the library window with this photo focused | Replaced by **Add folder to Lightbox ▾** (Watch folder / Scan once): adds the parent folder as a root with that mode (existing FolderManager), scans it, then the entry resolves to a catalog photo and catalog actions enable without reopening |
| Copy image (P1) | Edited L2/L3 to clipboard (if the locked GPUI clipboard supports images; else copy the file path and record) | Same, unedited |
| Reveal in folder, Open with default app | ✓ (existing `Opener`) | ✓ |

Resolving an entry: at open and on each navigation, `photos().find_by_path(path)` (add it if missing; it must use an index). Photos with status `missing`/`unreadable` count as not catalog for actions, but the file is still shown if it decodes.

---

## V9. Edge cases (all required)

| Case | Behaviour |
|---|---|
| File missing / permission denied / corrupt / unsupported | Error card in the fit area (`VIEWER_ERROR_CARD`): icon, file name, one-line reason, "Show in folder". Navigation continues. The ambient backdrop uses `dim` |
| Non-image argument (e.g. `.txt`) | Same error card: "Lightbox can't open this kind of file" |
| Current file deleted/renamed on disk while viewing (standalone Folder source watches the dir; Library source uses `PhotosRemoved`) | Advance to the next entry (or previous if it was last), with a toast. If none remain, close |
| New file appears in the folder | Inserted in sort order. The current index stays on the same file |
| File modified while viewing | `quick_key` changes, so reload L2 (crossfade) |
| Very small image (e.g. 32×32) | Shown 1:1 (not enlarged unless `enlarge_small`). Open/close animations still map to/from the thumbnail rect |
| Very large image (> 100 MP) | L2 as usual. L3 capped at 8192 px long edge (existing), with a one-time toast "Shown at reduced resolution". Tiles are F32/P2 |
| Animated GIF/WebP | P0: first frame (existing decode). P1 (F31): play frames with their delays, loop, `P` pauses |
| Transparent PNG/WebP | Drawn over a subtle checkerboard (8 px squares, `#2e2e2e`/`#3a3a3a`) inside the image rect (P1; P0 draws on the backdrop directly) |
| Video entry in a Folder/Files NavSet | Poster frame (existing video thumbnail) with a ▶ badge; click/Enter opens the OS player |
| Window resized, display changed, DPI changed | Recompute fit (animated per §V4.2 when in Fit mode). Re-request L2 at the new pixel size after 150 ms of no further resizing |
| Paths with spaces, unicode, very long names, non-UTF-8 on Linux | Work end-to-end (OsString internally, lossy only for display) |
| Network share / slow disk | Spinner after 120 ms. Navigation stays responsive, and pending decodes are cancelled on navigate |
| Library catalog needs a migration or is locked/corrupt | Standalone still opens files as non-catalog entries and logs a warning. Never block viewing on the catalog |
| Second launch with new files while a close animation runs | Cancel the close and present the new NavSet with "open, no source rect" |

---

## V10. Default image viewer registration (lb-assoc)

### V10.1 Principles

1. **Never change the user's defaults without an explicit click.** At first run, Lightbox may silently register itself as *available*: listed in "Open with" on Windows, a document-type handler with rank `Alternate` on macOS, a `.desktop` entry with `MimeType=` on Linux. Becoming *the default* happens only from the Settings button or the first-run banner.
2. **Reversible.** Store what the previous defaults were (where the OS lets us read them) and offer "Restore previous defaults".
3. **Plans are data.** Each OS backend has a pure function that builds the list of changes (`AssocPlan`) from inputs (exe path, formats, existing state). A thin executor applies it. Plans are snapshot-tested on every OS. Executors are tested on their own OS in CI against sandboxed locations.
4. **Per-user only.** No admin rights, no system-wide writes.

```rust
pub struct AppInstall { pub exe: PathBuf, pub bundle_id: String /* app.lightbox.Lightbox */,
                        pub display_name: String /* "Lightbox Photo Viewer" */, pub icon: Option<PathBuf> }
pub struct FormatAssoc { pub ext: &'static str, pub mime: &'static str, pub uti: &'static str, pub label: &'static str }
pub enum AssocStatus { Default, Available, OtherDefault(String) /* app name if known */, Unregistered }

pub trait AssocBackend {
    fn status(&self, formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>>;
    fn register_available(&self, app: &AppInstall, formats: &[FormatAssoc]) -> Result<()>;   // silent, idempotent
    fn make_default(&self, app: &AppInstall, formats: &[FormatAssoc]) -> Result<MakeDefaultOutcome>;
    fn restore_previous(&self, saved: &SavedDefaults) -> Result<()>;
    fn unregister(&self, app: &AppInstall) -> Result<()>;
}
pub enum MakeDefaultOutcome { Done, UserConfirmationShown, OpenedSystemSettings }
```

### V10.2 Formats

Only formats the build can decode are registered (`lb_media::heif_decoder_available()`, `raw_decoder_available()`):

| ext | MIME | UTI |
|---|---|---|
| jpg, jpeg, jpe | image/jpeg | public.jpeg |
| png | image/png | public.png |
| gif | image/gif | com.compuserve.gif |
| webp | image/webp | org.webmproject.webp |
| bmp | image/bmp | com.microsoft.bmp |
| tif, tiff | image/tiff | public.tiff |
| heic, heif (`heif`) | image/heic, image/heif | public.heic, public.heif |
| avif (`heif`) | image/avif | public.avif |
| dng (`raw`) | image/x-adobe-dng | com.adobe.raw-image |
| cr2, cr3, nef, arw, orf, rw2, raf (`raw`) | image/x-canon-cr2, image/x-canon-cr3, image/x-nikon-nef, image/x-sony-arw, image/x-olympus-orf, image/x-panasonic-rw2, image/x-fuji-raf | com.canon.cr2-raw-image, com.canon.cr3-raw-image, com.nikon.raw-image, com.sony.arw-raw-image, com.olympus.or-raw-image, com.panasonic.rw2-raw-image, com.fuji.raw-image |

RAW formats are off by default in the Settings type list (users often want their RAW tool there).

### V10.3 Windows (HKCU)

`register_available` writes (one ProgID per format family):
```
HKCU\Software\Classes\Lightbox.Image.<fmt>
    (Default)                = "<Label> (Lightbox)"
    FriendlyTypeName         = "<Label>"
    DefaultIcon\(Default)    = "\"<exe>\",0"
    shell\open\FriendlyAppName = "Lightbox Photo Viewer"
    shell\open\command\(Default) = "\"<exe>\" view \"%1\""
HKCU\Software\Classes\.<ext>\OpenWithProgids   value "Lightbox.Image.<fmt>" = "" (REG_NONE)
HKCU\Software\Classes\Applications\lightbox.exe
    FriendlyAppName = "Lightbox Photo Viewer"
    SupportedTypes   values ".jpg" = "" …
    shell\open\command\(Default) = "\"<exe>\" view \"%1\""
HKCU\Software\Lightbox\Capabilities
    ApplicationName        = "Lightbox Photo Viewer"
    ApplicationDescription = "View and organise photos"
    FileAssociations       values ".jpg" = "Lightbox.Image.jpeg" …
HKCU\Software\RegisteredApplications   value "Lightbox" = "Software\Lightbox\Capabilities"
HKCU\Software\Lightbox\AssocManifest   (list of every key/value we created, for clean unregister)
```
Then `SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, null, null)`.

`make_default`: Windows 8+ protects the per-type `UserChoice` key with a hash. Apps **must not** write it. Instead, open the Default Apps settings page for Lightbox: `ms-settings:defaultapps?registeredAppUser=Lightbox` where supported (Windows 11), else `ms-settings:defaultapps`. Return `OpenedSystemSettings`. The UI then explains: "Choose Lightbox Photo Viewer for the types you want."

`status` reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.<ext>\UserChoice\ProgId` (read-only) and compares it with our ProgIDs.

`restore_previous` cannot set `UserChoice` either: it unregisters our ProgIDs (Windows then asks the user next time) and opens the same settings page.

`exe` is the current executable path (`std::env::current_exe`). A portable zip install re-registers on each start if the exe path changed (idempotent; it updates paths).

### V10.4 macOS

`xtask/src/package.rs::info_plist` adds `CFBundleDocumentTypes`, one dict per format group:
```xml
<dict>
  <key>CFBundleTypeName</key><string>JPEG image</string>
  <key>CFBundleTypeRole</key><string>Viewer</string>
  <key>LSHandlerRank</key><string>Alternate</string>
  <key>LSItemContentTypes</key><array><string>public.jpeg</string></array>
</dict>
```
This makes Lightbox appear in Finder's "Open With" once the app is in `/Applications` or `~/Applications` and has been launched once (LaunchServices registration). `register_available` may additionally call `LSRegisterURL(bundle_url, true)` to speed this up.

`make_default`: for each UTI, `NSWorkspace.setDefaultApplication(at:toOpenContentType:completionHandler:)` (macOS 12+). The system shows its own confirmation, so return `UserConfirmationShown`. On macOS 11 fall back to `LSSetDefaultRoleHandlerForContentType(uti, kLSRolesViewer, bundle_id)`. Before changing, read the current handler (`NSWorkspace.urlForApplication(toOpen:)` / `LSCopyDefaultRoleHandlerForContentType`) and save it for restore.

Development builds (`cargo run`, no bundle) cannot register: `status` returns `Unregistered`, and the Settings section shows "Available in the installed app" with the buttons disabled.

File opens arrive via `on_open_urls` (§V2.2).

### V10.5 Linux (freedesktop)

`register_available`:
- Write `$XDG_DATA_HOME/applications/app.lightbox.Viewer.desktop` (default `~/.local/share/applications`) atomically:
  ```ini
  [Desktop Entry]
  Type=Application
  Name=Lightbox Photo Viewer
  GenericName=Image Viewer
  Comment=View and organise photos
  Exec=<exec> view %F
  Icon=app.lightbox.Lightbox
  Terminal=false
  Categories=Graphics;Viewer;Photography;
  MimeType=image/jpeg;image/png;image/gif;image/webp;image/bmp;image/tiff;
  StartupNotify=true
  StartupWMClass=lightbox
  ```
  `<exec>` = `$APPIMAGE` if set (AppImage), else `current_exe()`, quoted per the Desktop Entry spec (wrap in `"` when it has spaces; escape `"`, `` ` ``, `$`, `\`; double `%`).
- Install the icon to `$XDG_DATA_HOME/icons/hicolor/512x512/apps/app.lightbox.Lightbox.png` (from the packaging icon).
- Run `update-desktop-database "$XDG_DATA_HOME/applications"` if it is on `PATH` (ignore absence).
- The existing library `.desktop` (xtask) stays; it gains no `MimeType`.

`make_default`: edit `$XDG_CONFIG_HOME/mimeapps.list` (default `~/.config`) ourselves, preserving all other content. Under `[Default Applications]` set `<mime>=app.lightbox.Viewer.desktop;` for each selected type. If a desktop-specific file `$XDG_CONFIG_HOME/$XDG_CURRENT_DESKTOP-mimeapps.list` exists (lower-cased, `:`-separated desktops), update it too, because it takes precedence. Save the previous values for restore. Return `Done`. Do not rely on `xdg-mime` (keeps behaviour deterministic and testable). The INI editor keeps comments, ordering and unknown sections intact.

`status` resolves per the XDG MIME apps spec: desktop-specific list, then `mimeapps.list`, in `$XDG_CONFIG_HOME` then `$XDG_CONFIG_DIRS`.

Flatpak/Snap packaging is out of scope (portals handle associations there); document it in the README.

### V10.6 UX

**Settings → "Photo viewer & file types"** (an in-view overlay like the other dialogs, opened from a new gear button `SETTINGS_BUTTON` in the title bar; this is the first section of the Settings dialog, which M9.1 deferred):
- Viewer options: backdrop (`auto`/`desktop_blur`/`ambient`/`dim`), motion (`auto`/`full`/`reduced`/`off`), mouse wheel (`zoom`/`navigate`), enlarge small photos, wrap at ends, show filmstrip, folder sort order, single click opens.
- File types: a checkbox list (like Picasa's "File Types" options tab) with status per type: "Default", "Available", "Default: <other app>", "Not registered".
- **[Make Lightbox the default photo viewer]** applies `make_default` to the checked types. After an `OpenedSystemSettings` or `UserConfirmationShown` outcome, show a one-line explanation and refresh the status when the window regains focus.
- **[Restore previous defaults]** is enabled when saved defaults exist.

**First-run banner** (installed builds only, i.e. when `AppInstall` is valid; never in tests or dev builds unless forced): a dismissible bar in the library: "Open photos from your file manager with Lightbox?" **[Make default]** **[Not now]**. "Not now" sets `viewer.default_prompt_dismissed = true`; it never shows again. "Make default" behaves like the Settings button with the default type selection (all non-RAW types).

**Unregister on uninstall:** document in the README that `lightbox --unregister-file-types` removes everything we added (Windows manifest keys, Linux desktop file and mimeapps entries pointing to us, macOS nothing needed). Add the flag to `parse_launch`.

---

## V11. Settings (new `viewer` section)

Stored like the other sections (Decision M9.1: one JSON value per section, defaults never stored).

```rust
pub struct ViewerSettings {
    pub backdrop: BackdropMode,          // Auto
    pub motion: MotionLevel,             // Auto
    pub wheel: WheelAction,              // Zoom
    pub enlarge_small: bool,             // false
    pub wrap: bool,                      // false
    pub show_filmstrip: bool,            // true
    pub folder_sort: FolderSort,         // NameNatural
    pub single_click_opens: bool,        // false
    pub file_types: Vec<String>,         // selected extensions; default = all non-RAW supported
    pub saved_defaults: Option<SavedDefaults>,   // for restore (per OS payload)
    pub default_prompt_dismissed: bool,  // false
}
```

---

## V12. Performance budgets (added to SPEC.md §11 and `xtask perf`)

| Metric | Budget |
|---|---|
| Overlay: main-thread work between the double-click and the first animated frame | no file I/O, no decode (asserted by test), ≤ 4 ms |
| Overlay: L2 visible for a 24 MP JPEG | ≤ 150 ms after open (8-core machine) |
| Navigate to a preloaded neighbour | L2 on the first frame, 0 decodes |
| Standalone, instance already running: launch → first frame with image (L1 or L2) | ≤ 150 ms |
| Standalone, cold process start: launch → first frame (backdrop) | ≤ 300 ms; image (L1/L2) ≤ 500 ms |
| Ambient render, 4K stage | ≤ 8 ms on one core |
| Transition frame CPU time (sampling + layout) | ≤ 4 ms p95 |
| Viewer decoded-image memory | ≤ 512 MB |

`lightbox view --perf-json <file>` prints `{"process_start_ms", "first_frame_ms", "first_image_ms", "l2_ms"}` to stdout after L2 is shown, then exits. `xtask perf` runs it on a generated 24 MP JPEG (under `xvfb-run` on Linux CI; informational on CI, enforced locally). Unit-level budgets (ambient, no-blocking-I/O, neighbour instant) are ordinary tests as in Decision M11.1.

---

## V13. Element ids (add to `lb_ui::ids`)

Existing ids are kept with the same meaning, except as noted:

```
VIEWER_IMAGE          "viewer-image"           now the photo's own rect (was the whole image area)
VIEWER_BACK           "viewer-back"            now the × close button (top right)
VIEWER_PREV/NEXT/EDIT/INFO/INFO_PANEL/FILMSTRIP/ZOOM_LABEL   unchanged
new:
VIEWER_STAGE          "viewer-stage"           stage root (focus target)
VIEWER_BACKDROP       "viewer-backdrop"        full-stage backdrop hit area (click closes)
VIEWER_TITLE          "viewer-title"           file name (aria label = name)
VIEWER_COUNTER        "viewer-counter"         "12 / 48"
VIEWER_TOOLBAR        "viewer-toolbar"
VIEWER_PLAY           "viewer-play"
VIEWER_ROTATE_LEFT    "viewer-rotate-left"
VIEWER_ROTATE_RIGHT   "viewer-rotate-right"
VIEWER_STAR           "viewer-star"
VIEWER_ZOOM_FIT       "viewer-zoom-fit"
VIEWER_ZOOM_ACTUAL    "viewer-zoom-actual"
VIEWER_DELETE         "viewer-delete"
VIEWER_OPEN_LIBRARY   "viewer-open-library"
VIEWER_ADD_FOLDER     "viewer-add-folder"
VIEWER_HINT           "viewer-hint"            the "not saved" chip for non-catalog rotation
VIEWER_LOADING        "viewer-loading"
VIEWER_ERROR_CARD     "viewer-error-card"
VIEWER_WINDOW         "viewer-window"          root of the standalone window
SETTINGS_BUTTON       "settings-button"
SETTINGS_VIEWER       "settings-viewer"
SETTINGS_MAKE_DEFAULT "settings-make-default"
SETTINGS_RESTORE_DEFAULTS "settings-restore-defaults"
DEFAULT_VIEWER_BANNER "default-viewer-banner"
BANNER_MAKE_DEFAULT   "banner-make-default"
BANNER_DISMISS        "banner-dismiss"
fn assoc_type(ext: &str) -> ElementId           "assoc-type" + ext
```
The image element's `bounds()` in UI tests must equal `Presented.image.rect` (± 0.5 px): it is the hook that makes the animation testable headlessly. To click the backdrop in tests, use `click_at(VIEWER_BACKDROP, point(px(8.), px(8.)))` (a corner, away from the image).

---

## V14. Tests (acceptance criteria)

All names are normative (SPEC.md §0). Tests that involve the clock use `FakeClock` and never sleep.

**Transform (lb-app, unit + proptest)**
- `viewer_fit_rect_respects_insets_and_aspect` (proptest: rect inside fit area, aspect preserved within 1 px, centred)
- `viewer_small_image_not_enlarged`, `viewer_enlarge_small_setting`
- `viewer_zoom_anchor_point_stays_fixed` (proptest: image point under anchor identical before/after, 1e-3 px)
- `viewer_zoom_out_snaps_to_fit`, `viewer_pan_clamped_to_edges`, `viewer_drag_threshold_is_not_click`
- `viewer_wheel_lines_and_pixels_mapping`, `viewer_wheel_navigate_setting`, `viewer_swipe_navigates_when_not_zoomed`

**Motion (lb-app)**
- `motion_easing_endpoints_and_monotonic` (all easing fns: f(0)=0, f(1)=1, monotonic)
- `viewer_open_from_thumbnail_interpolates_rect` (t=0 → source rect, t=260 ms → fit rect, midpoint per `out_cubic`)
- `viewer_open_without_source_scales_and_fades`
- `viewer_close_returns_to_visible_thumbnail_rect`, `viewer_close_fades_when_thumbnail_offscreen`
- `viewer_navigate_slides_and_crossfades_ambient`
- `viewer_retarget_is_continuous` (proptest: interrupting any transition at any time; presented state at the input instant equals the first sample of the new transition)
- `viewer_rapid_navigation_skips_slide`
- `viewer_reduced_motion_uses_crossfade`, `viewer_motion_off_is_instant`
- `viewer_chrome_autohides_after_idle_and_returns_on_move`
- `viewer_not_animating_when_idle` (no animation frames requested after the transition ends)

**Navigation set and entries (lb-app)**
- `natural_sort_table` (lb-core: `img2` < `IMG10`, unicode, leading zeros, extensions)
- `viewer_folder_siblings_natural_sort_skip_hidden_and_unsupported`
- `viewer_multi_file_open_limits_navigation`
- `viewer_directory_argument_opens_first_file`
- `viewer_entry_resolves_catalog_photo_by_path`, `viewer_find_by_path_uses_index`
- `viewer_folder_change_inserts_new_sibling`, `viewer_current_file_deleted_advances`
- `viewer_add_folder_enables_catalog_actions`

**Loader (lb-app, with a counting fake decoder)**
- `viewer_progressive_load_order` (L0/L1 before L2; L3 only when zoomed past L2)
- `viewer_open_has_no_blocking_io` (zero decodes or file reads on the calling thread during `open` and the first `presented`)
- `viewer_preloads_neighbors_then_instant_navigation`
- `viewer_rapid_navigation_skips_intermediate_decodes`
- `viewer_memory_budget_evicts_farthest`
- `viewer_proxy_for_path_cached_by_quick_key`
- `viewer_missing_file_shows_error_and_navigation_continues`, `viewer_non_image_argument_shows_error`
- `viewer_unicode_and_space_paths`

**Ambient (lb-imaging)**
- `golden_ambient_portrait_1280x800`, `golden_ambient_landscape_1280x800` (goldens per SPEC.md §12.4)
- `ambient_deterministic`, `ambient_cover_crop_matches_stage_aspect`, `ambient_brightness_cap`, `ambient_4k_under_budget` (perf check per Decision M11.1)

**Actions (lb-app)**
- `viewer_rotate_catalog_pushes_edit_op`, `viewer_rotate_file_is_view_only`
- `viewer_star_disabled_for_non_catalog`, `viewer_delete_file_uses_trash_backend`

**Launch, single instance (apps/lightbox, lb-app, lb-ipc)**
- `launch_args_parse_table` (all rows of §V2.2, `-psn_` ignored, file URLs, relative paths, `--unregister-file-types`)
- `instance_second_launch_forwards_paths`, `instance_isolated_per_home`, `instance_stale_socket_recovered`, `instance_protocol_rejects_unknown_version`, `instance_race_one_server_wins`
- `view_launch_does_not_scan_or_watch`, `open_library_from_viewer_starts_scan`
- `macos_open_urls_route_to_viewer` (handler-level unit test, all OSes; URL decoding)

**Associations (lb-assoc)**
- `assoc_formats_follow_decoder_capabilities`
- `windows_assoc_plan_snapshot` (insta), `windows_unregister_removes_only_manifest_entries`
- `windows_registry_roundtrip_hkcu` (`#[cfg(windows)]`, test ProgID prefix `LightboxTest`, cleans up)
- `macos_info_plist_document_types_snapshot`, `macos_info_plist_lints` (`#[cfg(target_os = "macos")]`, runs `plutil -lint`)
- `linux_desktop_entry_snapshot`, `linux_exec_quoting_table`
- `mimeapps_list_edit_preserves_other_content`, `mimeapps_desktop_specific_list_updated`, `mimeapps_restore_previous_default`, `linux_status_resolution_order`
- `linux_register_in_temp_xdg_home` (integration: temp `XDG_DATA_HOME`/`XDG_CONFIG_HOME`)
- `default_viewer_requires_explicit_action` (first run registers availability only; no default changes)

**UI (lb-ui, `#[gpui_kit::test]`)**
- `ui_double_click_opens_overlay_from_thumbnail_rect` (VIEWER_IMAGE bounds at t=0 ≈ cell content rect; at t=260 ms ≈ fit rect)
- `ui_overlay_draws_library_beneath_until_opaque`
- `ui_viewer_backdrop_click_closes`, `ui_viewer_click_on_image_does_not_close`, `ui_viewer_drag_pan_does_not_close`
- `ui_viewer_close_animates_back_to_cell`, `ui_viewer_close_after_navigation_reveals_cell`
- `ui_viewer_arrow_keys_navigate_with_slide`, `ui_viewer_toolbar_star_and_rotate`, `ui_viewer_toolbar_autohides`
- `ui_viewer_window_opens_path_outside_library` (standalone window: title, counter, sibling navigation, star disabled, add-folder present)
- `ui_viewer_window_replaces_navset_on_second_launch`
- `ui_viewer_error_card_for_missing_file`
- `ui_settings_viewer_section_and_make_default` (with a fake `AssocBackend` injected through `LightboxOptions`)
- `ui_first_run_banner_dismiss_persists`
- Existing `ui_open_viewer_and_navigate` and `ui_viewer_escape_returns_focus` keep passing. Update their mechanics only if needed (e.g. advance the FakeClock past the open transition) and record it.

**Perf** — the §V12 table via `xtask perf`.

---

## V15. Implementation order

See `TASKS.md` M13. Build the pure pieces first (transform, motion, natural sort, ambient, NavSet, loader), then the overlay, then launch and single instance, then the standalone window, then associations and settings. After each step the app must still work, and the existing viewer behaviour stays available until the overlay replaces it.
