//! Detection followed by embedding (SPEC §6.15): decode at long edge 1024, detect, keep
//! confident and large-enough faces, embed.

use crate::cluster::l2_normalize;
use crate::detect::{FaceDetector, FaceEmbedder};
use crate::error::Result;
use image::{DynamicImage, RgbImage, imageops::FilterType};
use lb_core::NormRect;
use std::sync::Arc;

/// Long edge, in pixels, images are analysed at.
pub const DETECT_EDGE: u32 = 1024;
/// Detections below this score are dropped.
pub const MIN_SCORE: f32 = 0.8;
/// Faces whose shorter box side is below this many pixels (at [`DETECT_EDGE`]) are dropped.
pub const MIN_FACE_PX: f32 = 24.0;

/// A face that passed the filters, with its identity vector.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundFace {
    /// Box normalized to the image, clamped into it.
    pub rect: NormRect,
    /// Detector confidence.
    pub score: f32,
    /// L2-normalized embedding.
    pub embedding: Vec<f32>,
}

/// A detector and an embedder working together.
#[derive(Clone)]
pub struct FacePipeline {
    detector: Arc<dyn FaceDetector>,
    embedder: Arc<dyn FaceEmbedder>,
}

impl FacePipeline {
    /// Combines a detector and an embedder.
    pub fn new(detector: Arc<dyn FaceDetector>, embedder: Arc<dyn FaceEmbedder>) -> Self {
        Self { detector, embedder }
    }

    /// A pipeline over the [`MockDetector`](crate::MockDetector) and
    /// [`MockEmbedder`](crate::MockEmbedder).
    pub fn mock() -> Self {
        Self::new(Arc::new(crate::MockDetector), Arc::new(crate::MockEmbedder))
    }

    /// Finds and embeds the faces in an upright image. Larger images are shrunk to
    /// [`DETECT_EDGE`] first. Faces are ordered left to right.
    pub fn analyze(&self, img: &DynamicImage) -> Result<Vec<FoundFace>> {
        let long = img.width().max(img.height());
        let rgb: RgbImage = if long > DETECT_EDGE {
            let scale = DETECT_EDGE as f32 / long as f32;
            let w = ((img.width() as f32 * scale).round() as u32).max(1);
            let h = ((img.height() as f32 * scale).round() as u32).max(1);
            img.resize_exact(w, h, FilterType::Triangle).to_rgb8()
        } else {
            img.to_rgb8()
        };
        let (w, h) = (rgb.width() as f32, rgb.height() as f32);
        let mut found = Vec::new();
        for det in self.detector.detect(&rgb)? {
            if det.score < MIN_SCORE || !det.score.is_finite() {
                continue;
            }
            let x0 = det.rect.x.clamp(0.0, 1.0);
            let y0 = det.rect.y.clamp(0.0, 1.0);
            let x1 = (det.rect.x + det.rect.w).clamp(0.0, 1.0);
            let y1 = (det.rect.y + det.rect.h).clamp(0.0, 1.0);
            let rect = NormRect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            };
            if (rect.w * w).min(rect.h * h) < MIN_FACE_PX {
                continue;
            }
            let mut embedding = self
                .embedder
                .embed(&rgb, &crate::Detection { rect, ..det })?
                .to_vec();
            if !l2_normalize(&mut embedding) {
                tracing::warn!("embedder returned a degenerate vector; face skipped");
                continue;
            }
            found.push(FoundFace {
                rect,
                score: det.score,
                embedding,
            });
        }
        found.sort_by(|a, b| a.rect.x.total_cmp(&b.rect.x));
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Detection, EMBEDDING_DIM, draw_mock_face};
    use image::Rgb;

    struct Fixed(Vec<Detection>);
    impl FaceDetector for Fixed {
        fn detect(&self, _: &RgbImage) -> Result<Vec<Detection>> {
            Ok(self.0.clone())
        }
    }
    struct Ones;
    impl FaceEmbedder for Ones {
        fn embed(&self, _: &RgbImage, _: &Detection) -> Result<[f32; EMBEDDING_DIM]> {
            Ok([2.0; EMBEDDING_DIM])
        }
    }

    fn det(x: f32, w: f32, score: f32) -> Detection {
        Detection {
            rect: NormRect { x, y: 0.1, w, h: w },
            score,
            landmarks: None,
        }
    }

    #[test]
    fn low_scores_and_tiny_faces_are_dropped_and_rects_clamped() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(400, 400));
        let pipeline = FacePipeline::new(
            Arc::new(Fixed(vec![
                det(0.6, 0.3, 0.9),
                det(0.1, 0.3, 0.79),
                det(0.3, 0.05, 0.99),
                det(0.05, 0.2, 0.8),
                det(0.85, 0.3, 0.95),
            ])),
            Arc::new(Ones),
        );
        let found = pipeline.analyze(&img).unwrap();
        assert_eq!(found.len(), 3, "score 0.79 and a 20 px face are dropped");
        assert!((found[0].rect.x - 0.05).abs() < 1e-6, "left to right");
        let last = found[2].rect;
        assert!(
            last.x + last.w <= 1.0 + 1e-6,
            "clamped into the image: {last:?}"
        );
        let norm: f32 = found[0].embedding.iter().map(|v| v * v).sum();
        assert!((norm - 1.0).abs() < 1e-4, "embeddings are unit length");
    }

    #[test]
    fn large_images_are_analysed_at_1024() {
        struct Size(std::sync::Mutex<Option<(u32, u32)>>);
        impl FaceDetector for Arc<Size> {
            fn detect(&self, img: &RgbImage) -> Result<Vec<Detection>> {
                if let Ok(mut slot) = self.0.lock() {
                    *slot = Some(img.dimensions());
                }
                Ok(Vec::new())
            }
        }
        let size = Arc::new(Size(std::sync::Mutex::new(None)));
        let pipeline = FacePipeline::new(Arc::new(size.clone()), Arc::new(Ones));
        pipeline
            .analyze(&DynamicImage::ImageRgb8(RgbImage::new(4000, 3000)))
            .unwrap();
        assert_eq!(size.0.lock().map(|s| *s).ok().flatten(), Some((1024, 768)));
    }

    #[test]
    fn mock_pipeline_finds_and_embeds_squares() {
        let mut img = RgbImage::from_pixel(300, 200, Rgb([128, 128, 128]));
        draw_mock_face(&mut img, 1, 30, 30, 60);
        draw_mock_face(&mut img, 5, 180, 60, 60);
        let found = FacePipeline::mock()
            .analyze(&DynamicImage::ImageRgb8(img))
            .unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].embedding.len(), EMBEDDING_DIM);
    }
}
