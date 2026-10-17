//! ONNX Runtime implementations of the detector (YuNet) and embedder (SFace), SPEC §6.15.
//!
//! ONNX Runtime itself is loaded dynamically at run time: from `ORT_DYLIB_PATH`, from a
//! `libonnxruntime` file next to the models, or from the system library path.

use crate::align::{ALIGNED_EDGE, align_face};
use crate::detect::{Detection, EMBEDDING_DIM, FaceDetector, FaceEmbedder, Landmarks};
use crate::error::{FaceError, Result};
use crate::models::{DETECTOR_MODEL, EMBEDDER_MODEL, verify_model};
use crate::pipeline::FacePipeline;
use image::{RgbImage, imageops::FilterType};
use lb_core::NormRect;
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Side of YuNet's fixed square input.
const DETECTOR_INPUT: usize = 640;
/// Candidates below this score are discarded before non-maximum suppression.
const CANDIDATE_SCORE: f32 = 0.6;
/// Boxes overlapping a better box by more than this fraction are suppressed.
const NMS_IOU: f32 = 0.3;
const STRIDES: [usize; 3] = [8, 16, 32];

fn model_error(e: impl std::fmt::Display) -> FaceError {
    FaceError::Model(e.to_string())
}

fn session(env: &Environment, path: &Path) -> Result<Session> {
    Session::builder(env)
        .map_err(model_error)?
        .commit_from_file(path)
        .map_err(model_error)
}

fn lock(session: &Mutex<Session>) -> Result<std::sync::MutexGuard<'_, Session>> {
    session
        .lock()
        .map_err(|_| FaceError::Model("the model session is poisoned".into()))
}

/// YuNet face detector.
pub struct OnnxDetector {
    session: Mutex<Session>,
}

impl OnnxDetector {
    /// Loads the model file at `path`.
    pub fn load(env: &Environment, path: &Path) -> Result<Self> {
        Ok(Self {
            session: Mutex::new(session(env, path)?),
        })
    }
}

struct Candidate {
    rect: [f32; 4],
    score: f32,
    landmarks: [(f32, f32); 5],
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn non_max_suppression(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Candidate> = Vec::new();
    for c in candidates {
        if kept.iter().all(|k| iou(&k.rect, &c.rect) <= NMS_IOU) {
            kept.push(c);
        }
    }
    kept
}

/// Decodes one stride's outputs into candidates in the 640x640 input frame.
fn decode_stride(
    stride: usize,
    cls: &[f32],
    obj: &[f32],
    bbox: &[f32],
    kps: &[f32],
    out: &mut Vec<Candidate>,
) {
    let cols = DETECTOR_INPUT / stride;
    let rows = DETECTOR_INPUT / stride;
    let s = stride as f32;
    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            let (Some(cls), Some(obj)) = (cls.get(i), obj.get(i)) else {
                return;
            };
            let score = (cls.clamp(0.0, 1.0) * obj.clamp(0.0, 1.0)).sqrt();
            if score < CANDIDATE_SCORE {
                continue;
            }
            let (Some(b), Some(k)) = (bbox.get(i * 4..i * 4 + 4), kps.get(i * 10..i * 10 + 10))
            else {
                return;
            };
            let cx = (c as f32 + b[0]) * s;
            let cy = (r as f32 + b[1]) * s;
            let w = b[2].exp() * s;
            let h = b[3].exp() * s;
            let landmarks: [(f32, f32); 5] =
                std::array::from_fn(|n| ((k[2 * n] + c as f32) * s, (k[2 * n + 1] + r as f32) * s));
            out.push(Candidate {
                rect: [cx - w / 2.0, cy - h / 2.0, w, h],
                score,
                landmarks,
            });
        }
    }
}

impl FaceDetector for OnnxDetector {
    fn detect(&self, img: &RgbImage) -> Result<Vec<Detection>> {
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }
        let scale = DETECTOR_INPUT as f32 / w.max(h) as f32;
        let nw = ((w as f32 * scale).round() as u32).clamp(1, DETECTOR_INPUT as u32);
        let nh = ((h as f32 * scale).round() as u32).clamp(1, DETECTOR_INPUT as u32);
        let resized = image::imageops::resize(img, nw, nh, FilterType::Triangle);
        let plane = DETECTOR_INPUT * DETECTOR_INPUT;
        let mut data = vec![0f32; 3 * plane];
        for (x, y, px) in resized.enumerate_pixels() {
            let at = y as usize * DETECTOR_INPUT + x as usize;
            for c in 0..3 {
                data[c * plane + at] = f32::from(px[2 - c]);
            }
        }
        let input = Tensor::from_array(([1usize, 3, DETECTOR_INPUT, DETECTOR_INPUT], data))
            .map_err(model_error)?;
        let mut session = lock(&self.session)?;
        let outputs = session.run(ort::inputs![input]).map_err(model_error)?;
        let mut candidates = Vec::new();
        for stride in STRIDES {
            let get = |name: &str| -> Result<Vec<f32>> {
                let key = format!("{name}_{stride}");
                let value = outputs
                    .get(key.as_str())
                    .ok_or_else(|| FaceError::Model(format!("the model has no output {key}")))?;
                let (_, data) = value.try_extract_tensor::<f32>().map_err(model_error)?;
                Ok(data.to_vec())
            };
            decode_stride(
                stride,
                &get("cls")?,
                &get("obj")?,
                &get("bbox")?,
                &get("kps")?,
                &mut candidates,
            );
        }
        let (fw, fh) = (nw as f32, nh as f32);
        Ok(non_max_suppression(candidates)
            .into_iter()
            .map(|c| {
                let landmarks: Landmarks = c.landmarks.map(|(x, y)| (x / fw, y / fh));
                Detection {
                    rect: NormRect {
                        x: c.rect[0] / fw,
                        y: c.rect[1] / fh,
                        w: c.rect[2] / fw,
                        h: c.rect[3] / fh,
                    },
                    score: c.score,
                    landmarks: Some(landmarks),
                }
            })
            .collect())
    }
}

/// SFace face recognizer.
pub struct OnnxEmbedder {
    session: Mutex<Session>,
}

impl OnnxEmbedder {
    /// Loads the model file at `path`.
    pub fn load(env: &Environment, path: &Path) -> Result<Self> {
        Ok(Self {
            session: Mutex::new(session(env, path)?),
        })
    }
}

impl FaceEmbedder for OnnxEmbedder {
    fn embed(&self, img: &RgbImage, det: &Detection) -> Result<[f32; EMBEDDING_DIM]> {
        let aligned = align_face(img, det);
        let edge = ALIGNED_EDGE as usize;
        let plane = edge * edge;
        let mut data = vec![0f32; 3 * plane];
        for (x, y, px) in aligned.enumerate_pixels() {
            let at = y as usize * edge + x as usize;
            for c in 0..3 {
                data[c * plane + at] = f32::from(px[c]);
            }
        }
        let input = Tensor::from_array(([1usize, 3, edge, edge], data)).map_err(model_error)?;
        let mut session = lock(&self.session)?;
        let outputs = session.run(ort::inputs![input]).map_err(model_error)?;
        let value = outputs
            .get("fc1")
            .ok_or_else(|| FaceError::Model("the model has no output fc1".into()))?;
        let (_, data) = value.try_extract_tensor::<f32>().map_err(model_error)?;
        let mut out = [0f32; EMBEDDING_DIM];
        if data.len() != EMBEDDING_DIM {
            return Err(FaceError::Model(format!(
                "the embedding has {} values instead of {EMBEDDING_DIM}",
                data.len()
            )));
        }
        out.copy_from_slice(data);
        Ok(out)
    }
}

fn runtime_library(dir: &Path) -> Option<PathBuf> {
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        return None;
    }
    let names = if cfg!(windows) {
        &["onnxruntime.dll"][..]
    } else if cfg!(target_os = "macos") {
        &["libonnxruntime.dylib"][..]
    } else {
        &["libonnxruntime.so"][..]
    };
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Loads both models from `dir` after verifying their checksums and returns a ready pipeline.
pub fn load_onnx_pipeline(dir: &Path) -> Result<FacePipeline> {
    let detector_path = verify_model(dir, &DETECTOR_MODEL)?;
    let embedder_path = verify_model(dir, &EMBEDDER_MODEL)?;
    let builder = match runtime_library(dir) {
        Some(lib) => ort::init_from(&lib).map_err(model_error)?,
        None => ort::init(),
    };
    let env = builder.with_telemetry(false).build().map_err(model_error)?;
    let detector = OnnxDetector::load(&env, &detector_path)?;
    let embedder = OnnxEmbedder::load(&env, &embedder_path)?;
    Ok(FacePipeline::new(
        Arc::new(KeepAlive {
            _env: env.clone(),
            inner: detector,
        }),
        Arc::new(KeepAlive {
            _env: env,
            inner: embedder,
        }),
    ))
}

struct KeepAlive<T> {
    _env: Environment,
    inner: T,
}

impl<T: FaceDetector> FaceDetector for KeepAlive<T> {
    fn detect(&self, img: &RgbImage) -> Result<Vec<Detection>> {
        self.inner.detect(img)
    }
}

impl<T: FaceEmbedder> FaceEmbedder for KeepAlive<T> {
    fn embed(&self, img: &RgbImage, det: &Detection) -> Result<[f32; EMBEDDING_DIM]> {
        self.inner.embed(img, det)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(x: f32, y: f32, w: f32, h: f32, score: f32) -> Candidate {
        Candidate {
            rect: [x, y, w, h],
            score,
            landmarks: [(0.0, 0.0); 5],
        }
    }

    #[test]
    fn nms_keeps_the_best_of_overlapping_boxes() {
        let kept = non_max_suppression(vec![
            cand(10.0, 10.0, 50.0, 50.0, 0.7),
            cand(12.0, 12.0, 50.0, 50.0, 0.9),
            cand(300.0, 300.0, 40.0, 40.0, 0.65),
        ]);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.9).abs() < 1e-6);
        assert!((iou(&[0.0, 0.0, 10.0, 10.0], &[0.0, 0.0, 10.0, 10.0]) - 1.0).abs() < 1e-6);
        assert_eq!(iou(&[0.0, 0.0, 1.0, 1.0], &[5.0, 5.0, 1.0, 1.0]), 0.0);
    }

    #[test]
    fn decode_places_boxes_on_the_stride_grid() {
        let cells = (DETECTOR_INPUT / 32) * (DETECTOR_INPUT / 32);
        let mut cls = vec![0.0; cells];
        let mut obj = vec![0.0; cells];
        let mut bbox = vec![0.0; cells * 4];
        let kps = vec![0.5; cells * 10];
        let at = 2 * (DETECTOR_INPUT / 32) + 3;
        cls[at] = 1.0;
        obj[at] = 0.81;
        bbox[at * 4] = 0.5;
        bbox[at * 4 + 1] = 0.5;
        bbox[at * 4 + 2] = 1.0f32.ln() + 1.0;
        bbox[at * 4 + 3] = 1.0f32.ln() + 1.0;
        let mut out = Vec::new();
        decode_stride(32, &cls, &obj, &bbox, &kps, &mut out);
        assert_eq!(out.len(), 1);
        let c = &out[0];
        assert!((c.score - 0.9).abs() < 1e-6);
        let side = std::f32::consts::E * 32.0;
        assert!((c.rect[2] - side).abs() < 1e-3);
        assert!((c.rect[0] + c.rect[2] / 2.0 - 3.5 * 32.0).abs() < 1e-3);
        assert!((c.rect[1] + c.rect[3] / 2.0 - 2.5 * 32.0).abs() < 1e-3);
        assert!((c.landmarks[0].0 - 3.5 * 32.0).abs() < 1e-3);
    }
}
