//! Formats Lightbox can offer to the operating system (SPEC-VIEWER §V10.2).

use crate::FormatAssoc;

#[derive(Clone, Copy)]
enum Group {
    Base,
    Heif,
    Raw,
}

struct Row {
    ext: &'static str,
    mime: &'static str,
    uti: &'static str,
    label: &'static str,
    group: Group,
}

const ROWS: &[Row] = &[
    Row {
        ext: "jpg",
        mime: "image/jpeg",
        uti: "public.jpeg",
        label: "JPEG image",
        group: Group::Base,
    },
    Row {
        ext: "jpeg",
        mime: "image/jpeg",
        uti: "public.jpeg",
        label: "JPEG image",
        group: Group::Base,
    },
    Row {
        ext: "jpe",
        mime: "image/jpeg",
        uti: "public.jpeg",
        label: "JPEG image",
        group: Group::Base,
    },
    Row {
        ext: "png",
        mime: "image/png",
        uti: "public.png",
        label: "PNG image",
        group: Group::Base,
    },
    Row {
        ext: "gif",
        mime: "image/gif",
        uti: "com.compuserve.gif",
        label: "GIF image",
        group: Group::Base,
    },
    Row {
        ext: "webp",
        mime: "image/webp",
        uti: "org.webmproject.webp",
        label: "WebP image",
        group: Group::Base,
    },
    Row {
        ext: "bmp",
        mime: "image/bmp",
        uti: "com.microsoft.bmp",
        label: "BMP image",
        group: Group::Base,
    },
    Row {
        ext: "tif",
        mime: "image/tiff",
        uti: "public.tiff",
        label: "TIFF image",
        group: Group::Base,
    },
    Row {
        ext: "tiff",
        mime: "image/tiff",
        uti: "public.tiff",
        label: "TIFF image",
        group: Group::Base,
    },
    Row {
        ext: "heic",
        mime: "image/heic",
        uti: "public.heic",
        label: "HEIC image",
        group: Group::Heif,
    },
    Row {
        ext: "heif",
        mime: "image/heif",
        uti: "public.heif",
        label: "HEIF image",
        group: Group::Heif,
    },
    Row {
        ext: "avif",
        mime: "image/avif",
        uti: "public.avif",
        label: "AVIF image",
        group: Group::Heif,
    },
    Row {
        ext: "dng",
        mime: "image/x-adobe-dng",
        uti: "com.adobe.raw-image",
        label: "DNG image",
        group: Group::Raw,
    },
    Row {
        ext: "cr2",
        mime: "image/x-canon-cr2",
        uti: "com.canon.cr2-raw-image",
        label: "Canon CR2 image",
        group: Group::Raw,
    },
    Row {
        ext: "cr3",
        mime: "image/x-canon-cr3",
        uti: "com.canon.cr3-raw-image",
        label: "Canon CR3 image",
        group: Group::Raw,
    },
    Row {
        ext: "nef",
        mime: "image/x-nikon-nef",
        uti: "com.nikon.raw-image",
        label: "Nikon NEF image",
        group: Group::Raw,
    },
    Row {
        ext: "arw",
        mime: "image/x-sony-arw",
        uti: "com.sony.arw-raw-image",
        label: "Sony ARW image",
        group: Group::Raw,
    },
    Row {
        ext: "orf",
        mime: "image/x-olympus-orf",
        uti: "com.olympus.or-raw-image",
        label: "Olympus ORF image",
        group: Group::Raw,
    },
    Row {
        ext: "rw2",
        mime: "image/x-panasonic-rw2",
        uti: "com.panasonic.rw2-raw-image",
        label: "Panasonic RW2 image",
        group: Group::Raw,
    },
    Row {
        ext: "raf",
        mime: "image/x-fuji-raf",
        uti: "com.fuji.raw-image",
        label: "Fujifilm RAF image",
        group: Group::Raw,
    },
];

/// Formats this build can decode. `heif` and `raw` follow the decoder features.
pub fn formats(heif: bool, raw: bool) -> Vec<FormatAssoc> {
    ROWS.iter()
        .filter(|row| match row.group {
            Group::Base => true,
            Group::Heif => heif,
            Group::Raw => raw,
        })
        .map(|row| FormatAssoc {
            ext: row.ext,
            mime: row.mime,
            uti: row.uti,
            label: row.label,
        })
        .collect()
}

/// JPEG, PNG, GIF, WebP, BMP and TIFF. HEIF and RAW stay out until the build can decode them.
pub fn base_formats() -> Vec<FormatAssoc> {
    formats(false, false)
}

/// ProgID family shared by extensions of one format (`jpg`/`jpeg`/`jpe` → `jpeg`).
pub fn progid_family(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" | "jpe" => "jpeg",
        "tif" | "tiff" => "tiff",
        "png" => "png",
        "gif" => "gif",
        "webp" => "webp",
        "bmp" => "bmp",
        "heic" => "heic",
        "heif" => "heif",
        "avif" => "avif",
        "dng" => "dng",
        "cr2" => "cr2",
        "cr3" => "cr3",
        "nef" => "nef",
        "arw" => "arw",
        "orf" => "orf",
        "rw2" => "rw2",
        "raf" => "raf",
        _ => "image",
    }
}

/// MIME types in first-seen order, one entry per type.
pub fn mime_types(formats: &[FormatAssoc]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for fmt in formats {
        if !out.contains(&fmt.mime) {
            out.push(fmt.mime);
        }
    }
    out
}
