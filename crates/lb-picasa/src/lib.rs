//! Legacy `.picasa.ini` parser (SPEC §6.17). The importer that applies the parsed data to the
//! catalog lives in `lb-app`.
#![warn(missing_docs)]

mod filters;
mod ini;
mod model;
mod rect;

pub use filters::{FilterOutcome, map_filters};
pub use model::{
    AlbumDef, FaceRef, FileEntry, FolderMeta, PICASA_INI_NAMES, PicasaIni, parse_picasa_ini,
    unescape_caption,
};
pub use rect::{decode_hex_u64, decode_rect64};
