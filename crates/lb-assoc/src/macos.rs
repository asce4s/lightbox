//! macOS document types and the bundle-only executor (SPEC-VIEWER §V10.4).

use crate::{AssocError, AssocStatus, FormatAssoc, MakeDefaultResult, Result};

/// Bundle id shared with the package `Info.plist`.
pub const BUNDLE_ID: &str = "app.lightbox.Lightbox";

/// `CFBundleDocumentTypes` entries, one dict per UTI.
pub fn document_types_xml(formats: &[FormatAssoc]) -> String {
    let mut out = String::from("  <key>CFBundleDocumentTypes</key>\n  <array>\n");
    let mut seen = Vec::new();
    for fmt in formats {
        if seen.contains(&fmt.uti) {
            continue;
        }
        seen.push(fmt.uti);
        out.push_str("    <dict>\n");
        out.push_str(&format!(
            "      <key>CFBundleTypeName</key><string>{}</string>\n",
            xml_escape(fmt.label)
        ));
        out.push_str("      <key>CFBundleTypeRole</key><string>Viewer</string>\n");
        out.push_str("      <key>LSHandlerRank</key><string>Alternate</string>\n");
        out.push_str(&format!(
            "      <key>LSItemContentTypes</key><array><string>{}</string></array>\n",
            xml_escape(fmt.uti)
        ));
        out.push_str("    </dict>\n");
    }
    out.push_str("  </array>\n");
    out
}

/// Full `Info.plist` for the macOS bundle, including document types for `formats`.
pub fn info_plist(version: &str, formats: &[FormatAssoc]) -> String {
    let types = document_types_xml(formats);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Lightbox</string>
  <key>CFBundleDisplayName</key><string>Lightbox</string>
  <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
  <key>CFBundleExecutable</key><string>lightbox</string>
  <key>CFBundleIconFile</key><string>icon.png</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
{types}</dict>
</plist>
"#
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Whether `current_exe` lives inside a `.app` bundle.
pub fn current_is_bundled() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .is_some_and(|text| {
            text.contains(".app/Contents/MacOS") || text.contains(".app\\Contents\\MacOS")
        })
}

/// Runtime association for macOS. Availability comes from the bundle's `Info.plist`.
#[derive(Debug, Clone, Copy)]
pub struct MacosAssoc {
    /// `true` when this process is the installed `.app`.
    pub bundled: bool,
}

impl MacosAssoc {
    /// Detects a bundle from the current executable path.
    pub fn current() -> Self {
        Self {
            bundled: current_is_bundled(),
        }
    }

    /// Development builds report every type as unregistered.
    pub fn status(&self, formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
        let status = if self.bundled {
            AssocStatus::Available
        } else {
            AssocStatus::Unregistered
        };
        Ok(formats
            .iter()
            .copied()
            .map(|fmt| (fmt, status.clone()))
            .collect())
    }

    /// No-op. Launch Services reads the document types the package already wrote.
    pub fn register_available(&self, _formats: &[FormatAssoc]) -> Result<()> {
        tracing::info!("macOS availability is the Info.plist document types");
        Ok(())
    }

    /// AppKit is not linked. The Settings screen explains that the installed app confirms this.
    pub fn make_default(&self, _formats: &[FormatAssoc]) -> Result<MakeDefaultResult> {
        tracing::info!(
            bundled = self.bundled,
            "macOS default handler is not linked in this build"
        );
        Err(AssocError::Unavailable)
    }

    /// macOS uninstall leaves Launch Services to drop the bundle. Nothing to delete.
    pub fn unregister(&self) -> Result<()> {
        Ok(())
    }
}
