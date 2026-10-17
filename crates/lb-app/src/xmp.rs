//! XMP sidecar write-back (SPEC §6.7.6, feature F26).
//!
//! With the `xmp.write_sidecars` setting on, every change to a photo's rating, tags or caption
//! rewrites `{file}.xmp` next to the photo. Originals are never opened for writing.

use crate::atomic::write_bytes_atomic;
use crate::error::Result;
use crate::lightbox::Lightbox;
use crate::settings::XmpSettings;
use lb_catalog::Catalog;
use lb_core::PhotoId;
use std::path::{Path, PathBuf};

/// What a sidecar write produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XmpReport {
    /// Sidecar files written.
    pub written: Vec<PathBuf>,
    /// Photos whose sidecar could not be written, with the reason.
    pub failed: Vec<(PhotoId, String)>,
}

/// Writes XMP sidecar files.
#[derive(Clone)]
pub struct XmpService {
    lb: Lightbox,
}

impl Lightbox {
    /// XMP sidecar write-back.
    pub fn xmp(&self) -> XmpService {
        XmpService { lb: self.clone() }
    }
}

impl XmpService {
    /// Writes the sidecar of every photo in `ids` now, whether or not the setting is on.
    pub fn write(&self, ids: &[PhotoId]) -> Result<XmpReport> {
        write_sidecars(self.lb.catalog(), ids)
    }

    /// The sidecar path of the photo file `photo`: the full file name plus `.xmp`.
    pub fn sidecar_path(photo: &Path) -> PathBuf {
        sidecar_path(photo)
    }
}

/// `photo.jpg` becomes `photo.jpg.xmp`.
pub(crate) fn sidecar_path(photo: &Path) -> PathBuf {
    let mut name = photo.file_name().unwrap_or_default().to_os_string();
    name.push(".xmp");
    photo.with_file_name(name)
}

/// Whether the user turned sidecar write-back on.
pub(crate) fn enabled(catalog: &Catalog) -> bool {
    catalog
        .get_setting("xmp")
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<XmpSettings>(&json).ok())
        .is_some_and(|s| s.write_sidecars)
}

/// Writes the sidecar of each photo in `ids`; missing photos are ignored.
pub(crate) fn write_sidecars(catalog: &Catalog, ids: &[PhotoId]) -> Result<XmpReport> {
    let mut report = XmpReport::default();
    for &id in ids {
        let Some(photo) = catalog.get_photo(id)? else {
            continue;
        };
        let tags: Vec<String> = catalog
            .tags_for_photo(id)?
            .into_iter()
            .map(|t| t.name)
            .collect();
        let xmp = render(photo.rating, &tags, photo.caption.as_deref());
        let dest = sidecar_path(&photo.path);
        match write_bytes_atomic(&dest, xmp.as_bytes()) {
            Ok(()) => report.written.push(dest),
            Err(e) => report.failed.push((id, e.to_string())),
        }
    }
    Ok(report)
}

/// The sidecar after a change: failures are logged, never raised, so a read-only photo folder
/// does not break rating or tagging.
pub(crate) fn sync_after_change(catalog: &Catalog, ids: &[PhotoId]) {
    if ids.is_empty() || !enabled(catalog) {
        return;
    }
    match write_sidecars(catalog, ids) {
        Ok(report) => {
            for (id, why) in &report.failed {
                tracing::warn!(photo = id.get(), error = %why, "xmp sidecar not written");
            }
        }
        Err(e) => tracing::warn!(error = %e, "xmp sidecars not written"),
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\t' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// The XMP packet for a photo: `xmp:Rating`, `dc:subject` (tags) and `dc:description` (caption).
pub(crate) fn render(rating: u8, tags: &[String], caption: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Lightbox\">\n");
    out.push_str(" <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    out.push_str("  <rdf:Description rdf:about=\"\"\n");
    out.push_str("    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n");
    out.push_str("    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n");
    out.push_str(&format!("    xmp:Rating=\"{rating}\">\n"));
    if !tags.is_empty() {
        out.push_str("   <dc:subject>\n    <rdf:Bag>\n");
        for tag in tags {
            out.push_str(&format!("     <rdf:li>{}</rdf:li>\n", escape(tag)));
        }
        out.push_str("    </rdf:Bag>\n   </dc:subject>\n");
    }
    if let Some(caption) = caption.filter(|c| !c.is_empty()) {
        out.push_str("   <dc:description>\n    <rdf:Alt>\n");
        out.push_str(&format!(
            "     <rdf:li xml:lang=\"x-default\">{}</rdf:li>\n",
            escape(caption)
        ));
        out.push_str("    </rdf:Alt>\n   </dc:description>\n");
    }
    out.push_str("  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_keeps_the_full_file_name() {
        assert_eq!(
            sidecar_path(Path::new("/a/b/IMG_1.jpg")),
            Path::new("/a/b/IMG_1.jpg.xmp")
        );
    }

    #[test]
    fn rendering_escapes_markup_and_drops_control_characters() {
        let xml = render(2, &["a&b".into()], Some("x<y>\u{1}\n"));
        assert!(xml.contains("<rdf:li>a&amp;b</rdf:li>"));
        assert!(xml.contains("x&lt;y&gt;\n"));
        assert!(!xml.contains('\u{1}'));
    }
}
