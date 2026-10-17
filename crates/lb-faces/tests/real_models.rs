//! Smoke test of the real YuNet and SFace models. Nightly only: it needs the models from
//! `cargo xtask models`, a copy of ONNX Runtime, and sample photos of faces, none of which are
//! committed.
//!
//! Set `LIGHTBOX_FACE_MODELS` to the models directory; it may also hold `libonnxruntime.so` and
//! the samples `sample1.jpg` (a face) and `sample2.jpg` (a different person).
#![cfg(feature = "onnx")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_faces::{FacePipeline, FoundFace, cosine, load_onnx_pipeline};
use std::path::PathBuf;

fn models_dir() -> Option<PathBuf> {
    std::env::var_os("LIGHTBOX_FACE_MODELS").map(PathBuf::from)
}

fn faces_in(pipeline: &FacePipeline, file: &PathBuf) -> Vec<FoundFace> {
    let img = image::open(file).unwrap();
    pipeline.analyze(&img).unwrap()
}

#[test]
#[ignore = "needs LIGHTBOX_FACE_MODELS (models, ONNX Runtime and sample photos)"]
fn faces_real_model_smoke() {
    let Some(dir) = models_dir() else {
        eprintln!("LIGHTBOX_FACE_MODELS is not set; skipping");
        return;
    };
    let pipeline = load_onnx_pipeline(&dir).unwrap();
    let first = dir.join("sample1.jpg");
    let faces = faces_in(&pipeline, &first);
    assert!(
        !faces.is_empty(),
        "at least one face is found in sample1.jpg"
    );
    let best = faces
        .iter()
        .max_by(|a, b| a.score.total_cmp(&b.score))
        .unwrap();
    eprintln!("best face: score {:.3} rect {:?}", best.score, best.rect);
    assert!(best.score >= 0.8);
    assert!(best.rect.w > 0.05 && best.rect.h > 0.05);
    assert_eq!(best.embedding.len(), 128);
    let norm: f32 = best.embedding.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-3);

    let again = faces_in(&pipeline, &first);
    let same = again
        .iter()
        .map(|f| cosine(&f.embedding, &best.embedding))
        .fold(f32::MIN, f32::max);
    assert!(
        same > 0.99,
        "the same photo gives the same identity: {same}"
    );

    let second = dir.join("sample2.jpg");
    if second.is_file() {
        let others = faces_in(&pipeline, &second);
        for other in &others {
            let sim = cosine(&other.embedding, &best.embedding);
            assert!(sim < 0.55, "different people stay apart: {sim}");
        }
    }
}
