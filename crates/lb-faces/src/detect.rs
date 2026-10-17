//! The detector and embedder interfaces.

use crate::error::Result;
use image::RgbImage;
use lb_core::NormRect;

/// Length of a face embedding.
pub const EMBEDDING_DIM: usize = 128;

/// Right eye, left eye, nose tip, right and left mouth corner, normalized to the image.
pub type Landmarks = [(f32, f32); 5];

/// One face found in an image. Coordinates are normalized to the image that was analysed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    /// Face box.
    pub rect: NormRect,
    /// Confidence in `0..=1`.
    pub score: f32,
    /// Eye, nose and mouth positions when the detector provides them.
    pub landmarks: Option<Landmarks>,
}

/// Finds faces in an upright RGB image.
pub trait FaceDetector: Send + Sync {
    /// All faces the detector believes are in `img`, in no particular order.
    fn detect(&self, img: &RgbImage) -> Result<Vec<Detection>>;
}

/// Turns a detected face into an identity vector.
pub trait FaceEmbedder: Send + Sync {
    /// The embedding of the face `det` in `img`. It need not be normalized.
    fn embed(&self, img: &RgbImage, det: &Detection) -> Result<[f32; EMBEDDING_DIM]>;
}
