//! Typed application settings stored as JSON per section in the catalog (SPEC §6.18).

use crate::error::Result;
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use lb_core::{GroupBy, SortOrder};
use serde::{Deserialize, Serialize};

/// Which colour scheme the window uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// Follow the operating system.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// Grid thumbnail size preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThumbChoice {
    /// Small.
    S,
    /// Medium.
    #[default]
    M,
    /// Large.
    L,
}

impl ThumbChoice {
    /// Cell size in pixels the grid starts with.
    pub fn zoom_px(self) -> u32 {
        match self {
            Self::S => 112,
            Self::M => 160,
            Self::L => 256,
        }
    }
}

/// Grid arrangement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridSettings {
    /// Initial cell size.
    pub thumb_size: ThumbChoice,
    /// Header grouping.
    pub group_by: GroupBy,
    /// Order inside the result.
    pub sort: SortOrder,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            thumb_size: ThumbChoice::M,
            group_by: GroupBy::Month,
            sort: SortOrder::DateTakenDesc,
        }
    }
}

/// What the library shows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySettings {
    /// Include hidden photos.
    pub show_hidden: bool,
    /// Include photos whose file is missing.
    pub show_missing: bool,
}

/// Cache limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThumbSettings {
    /// Disk cache limit in MB.
    pub cache_limit_mb: u32,
    /// In-memory cache limit in MB.
    pub memory_limit_mb: u32,
}

impl Default for ThumbSettings {
    fn default() -> Self {
        Self {
            cache_limit_mb: 2048,
            memory_limit_mb: 400,
        }
    }
}

/// Defaults of the export dialog (SPEC §6.10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    /// JPEG quality 1 to 100.
    pub quality: u8,
    /// Long edge in pixels, `None` for the original size.
    pub long_edge: Option<u32>,
    /// Bake edits into the output.
    pub apply_edits: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            quality: 85,
            long_edge: None,
            apply_edits: true,
        }
    }
}

/// Slideshow transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// Cut.
    None,
    /// 400 ms crossfade.
    #[default]
    Crossfade,
    /// Slow pan and zoom.
    KenBurns,
}

/// Slideshow defaults (SPEC §6.12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideshowSettings {
    /// Seconds per photo, 1 to 30.
    pub duration_secs: u32,
    /// Transition between photos.
    pub transition: Transition,
    /// Show captions.
    pub show_captions: bool,
    /// Random order.
    pub shuffle: bool,
    /// Start over after the last photo.
    pub loop_forever: bool,
}

impl Default for SlideshowSettings {
    fn default() -> Self {
        Self {
            duration_secs: 3,
            transition: Transition::Crossfade,
            show_captions: false,
            shuffle: false,
            loop_forever: true,
        }
    }
}

/// Face recognition.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FacesSettings {
    /// Detect faces; stays off until the models are present.
    pub enabled: bool,
}

/// Map provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    /// Tile URL template.
    pub tile_url: String,
}

impl Default for MapSettings {
    fn default() -> Self {
        Self {
            tile_url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".to_owned(),
        }
    }
}

/// Video tooling.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    /// Path of `ffmpeg`; `None` searches `PATH`.
    pub ffmpeg_path: Option<String>,
}

/// XMP sidecar write-back (SPEC §6.7.6).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct XmpSettings {
    /// Write `{file}.xmp` next to a photo whenever its rating, tags or caption change.
    pub write_sidecars: bool,
}

/// Every setting with its default. Sections are stored under their field name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Settings {
    /// Colour scheme.
    pub theme: ThemeChoice,
    /// Grid arrangement.
    pub grid: GridSettings,
    /// Library visibility.
    pub library: LibrarySettings,
    /// Cache limits.
    pub thumbs: ThumbSettings,
    /// Export defaults.
    pub export: ExportSettings,
    /// Slideshow defaults.
    pub slideshow: SlideshowSettings,
    /// Face recognition.
    pub faces: FacesSettings,
    /// Map provider.
    pub map: MapSettings,
    /// Video tooling.
    pub video: VideoSettings,
    /// XMP sidecars.
    pub xmp: XmpSettings,
}

/// The stored section names, in the order of [`Settings`].
pub const SETTINGS_KEYS: [&str; 10] = [
    "theme",
    "grid",
    "library",
    "thumbs",
    "export",
    "slideshow",
    "faces",
    "map",
    "video",
    "xmp",
];

impl Settings {
    fn section(&self, key: &str) -> Option<serde_json::Result<String>> {
        Some(match key {
            "theme" => serde_json::to_string(&self.theme),
            "grid" => serde_json::to_string(&self.grid),
            "library" => serde_json::to_string(&self.library),
            "thumbs" => serde_json::to_string(&self.thumbs),
            "export" => serde_json::to_string(&self.export),
            "slideshow" => serde_json::to_string(&self.slideshow),
            "faces" => serde_json::to_string(&self.faces),
            "map" => serde_json::to_string(&self.map),
            "video" => serde_json::to_string(&self.video),
            "xmp" => serde_json::to_string(&self.xmp),
            _ => return None,
        })
    }

    fn apply(&mut self, key: &str, json: &str) -> bool {
        fn parse<T: serde::de::DeserializeOwned>(json: &str, slot: &mut T) -> bool {
            match serde_json::from_str(json) {
                Ok(v) => {
                    *slot = v;
                    true
                }
                Err(_) => false,
            }
        }
        match key {
            "theme" => parse(json, &mut self.theme),
            "grid" => parse(json, &mut self.grid),
            "library" => parse(json, &mut self.library),
            "thumbs" => parse(json, &mut self.thumbs),
            "export" => parse(json, &mut self.export),
            "slideshow" => parse(json, &mut self.slideshow),
            "faces" => parse(json, &mut self.faces),
            "map" => parse(json, &mut self.map),
            "video" => parse(json, &mut self.video),
            "xmp" => parse(json, &mut self.xmp),
            _ => true,
        }
    }
}

/// Reads and writes [`Settings`].
#[derive(Clone)]
pub struct SettingsService {
    lb: Lightbox,
}

impl Lightbox {
    /// Typed settings stored in the catalog.
    pub fn settings(&self) -> SettingsService {
        SettingsService { lb: self.clone() }
    }
}

/// Reads every section from `catalog`; see [`SettingsService::load`].
pub(crate) fn load_settings(catalog: &lb_catalog::Catalog) -> Result<Settings> {
    let mut settings = Settings::default();
    for (key, json) in catalog.list_settings()? {
        if !settings.apply(&key, &json) {
            tracing::warn!(%key, "unreadable setting ignored");
        }
    }
    Ok(settings)
}

impl SettingsService {
    /// Loads every section. Missing sections, unknown keys and unreadable values fall back to
    /// the defaults so an older or newer file never prevents startup.
    pub fn load(&self) -> Result<Settings> {
        load_settings(self.lb.catalog())
    }

    /// Stores the sections that differ from what is stored and emits one
    /// [`AppEvent::SettingsChanged`] per changed section.
    pub fn save(&self, settings: &Settings) -> Result<()> {
        let current = self.load()?;
        for key in SETTINGS_KEYS {
            let (Some(Ok(new)), Some(Ok(old))) = (settings.section(key), current.section(key))
            else {
                continue;
            };
            if new != old {
                self.lb.catalog().set_setting(key, &new)?;
                self.lb.emit(AppEvent::SettingsChanged {
                    key: key.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Applies `change` to the stored settings and saves the result.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut settings = self.load()?;
        change(&mut settings);
        self.save(&settings)?;
        Ok(settings)
    }

    /// Chooses the colour scheme.
    pub fn set_theme(&self, theme: ThemeChoice) -> Result<()> {
        self.update(|s| s.theme = theme).map(|_| ())
    }
}
