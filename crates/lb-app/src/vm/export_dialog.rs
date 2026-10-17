//! State and validation of the export dialog (SPEC §6.10, §9.6).

use crate::error::Result;
use crate::events::AppEvent;
use crate::export::{
    CollisionPolicy, ExportFormat, ExportHandle, ExportOptions, ExportReport, ExportSize,
    MetadataPolicy,
};
use crate::lightbox::Lightbox;
use lb_core::PhotoId;
use std::path::PathBuf;

/// The size presets of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeChoice {
    /// Full resolution.
    Original,
    /// Long edge 640 px.
    Px640,
    /// Long edge 1024 px.
    Px1024,
    /// Long edge 1600 px.
    Px1600,
    /// Long edge 2048 px.
    Px2048,
    /// The number typed in the custom field.
    Custom,
}

/// A field with a validation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportField {
    /// Destination folder.
    Destination,
    /// Custom size.
    CustomSize,
    /// JPEG quality.
    Quality,
    /// Nothing to export.
    Selection,
}

/// A validation problem tied to a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// The field to highlight.
    pub field: ExportField,
    /// What is wrong.
    pub message: String,
}

/// Where the dialog is in its life.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportPhase {
    /// Editing settings.
    Editing,
    /// The job runs.
    Running {
        /// Completion in `0.0..=1.0`.
        progress: f32,
    },
    /// The job ended.
    Done(ExportReport),
}

/// The export dialog's view-model.
pub struct ExportDialogVm {
    lb: Lightbox,
    /// Destination folder text.
    pub dest: String,
    /// Selected size preset.
    pub size: SizeChoice,
    /// Text of the custom long-edge field.
    pub custom_size: String,
    /// JPEG quality as typed, 1 to 100.
    pub quality: u8,
    /// Output format.
    pub format: ExportFormat,
    /// Bake edits into the output.
    pub apply_edits: bool,
    /// Metadata handling.
    pub metadata: MetadataPolicy,
    /// Watermark text; empty for none.
    pub watermark: String,
    /// Collision handling.
    pub collision: CollisionPolicy,
    ids: Vec<PhotoId>,
    handle: Option<ExportHandle>,
    phase: ExportPhase,
}

impl ExportDialogVm {
    /// A dialog for exporting `ids` with the defaults of SPEC §6.10.
    pub fn new(lb: &Lightbox, ids: Vec<PhotoId>) -> Self {
        let d = ExportOptions::new("");
        Self {
            lb: lb.clone(),
            dest: String::new(),
            size: SizeChoice::Original,
            custom_size: String::new(),
            quality: d.quality,
            format: d.format,
            apply_edits: d.apply_edits,
            metadata: d.metadata,
            watermark: String::new(),
            collision: d.collision,
            ids,
            handle: None,
            phase: ExportPhase::Editing,
        }
    }

    /// Photos to export.
    pub fn ids(&self) -> &[PhotoId] {
        &self.ids
    }

    /// Current phase.
    pub fn phase(&self) -> &ExportPhase {
        &self.phase
    }

    /// Every problem with the current settings; empty means the Export button is enabled.
    pub fn errors(&self) -> Vec<FieldError> {
        let mut out = Vec::new();
        let mut err = |field, message: &str| {
            out.push(FieldError {
                field,
                message: message.to_owned(),
            });
        };
        if self.ids.is_empty() {
            err(ExportField::Selection, "Select photos to export.");
        }
        if self.dest.trim().is_empty() {
            err(ExportField::Destination, "Choose a destination folder.");
        }
        if !(1..=100).contains(&self.quality) {
            err(ExportField::Quality, "Quality must be between 1 and 100.");
        }
        if self.size == SizeChoice::Custom && self.custom_px().is_none() {
            err(
                ExportField::CustomSize,
                "Enter a size between 16 and 20000 pixels.",
            );
        }
        out
    }

    fn custom_px(&self) -> Option<u32> {
        self.custom_size
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|n| (16..=20_000).contains(n))
    }

    /// The options the settings describe.
    pub fn options(&self) -> std::result::Result<ExportOptions, Vec<FieldError>> {
        let errors = self.errors();
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut o = ExportOptions::new(PathBuf::from(self.dest.trim()));
        o.size = match self.size {
            SizeChoice::Original => ExportSize::Original,
            SizeChoice::Px640 => ExportSize::LongEdge(640),
            SizeChoice::Px1024 => ExportSize::LongEdge(1024),
            SizeChoice::Px1600 => ExportSize::LongEdge(1600),
            SizeChoice::Px2048 => ExportSize::LongEdge(2048),
            SizeChoice::Custom => ExportSize::LongEdge(self.custom_px().unwrap_or(2048)),
        };
        o.quality = self.quality;
        o.format = self.format;
        o.apply_edits = self.apply_edits;
        o.metadata = self.metadata;
        o.watermark = (!self.watermark.trim().is_empty()).then(|| self.watermark.clone());
        o.collision = self.collision;
        Ok(o)
    }

    /// Starts the export job. Returns `false` (and stays in the editing phase) when the settings
    /// have problems.
    pub fn start(&mut self) -> Result<bool> {
        let Ok(options) = self.options() else {
            return Ok(false);
        };
        let handle = self.lb.export().start(&self.ids, options)?;
        self.phase = ExportPhase::Running { progress: 0.0 };
        self.handle = Some(handle);
        self.poll();
        Ok(true)
    }

    /// Cancels the running job.
    pub fn cancel(&mut self) {
        if let Some(h) = &self.handle {
            h.cancel();
        }
    }

    /// Moves to `Done` when the job ended.
    pub fn poll(&mut self) {
        if let Some(h) = &self.handle
            && h.is_finished()
            && let Some(report) = h.report()
        {
            self.phase = ExportPhase::Done(report);
            self.handle = None;
        }
    }

    /// Updates the progress bar from `JobProgress` events.
    pub fn handle_event(&mut self, event: &AppEvent) {
        if let (
            AppEvent::JobProgress {
                progress,
                kind: "export",
                ..
            },
            ExportPhase::Running { .. },
        ) = (event, &self.phase)
        {
            self.phase = ExportPhase::Running {
                progress: *progress,
            };
        }
        self.poll();
    }
}
