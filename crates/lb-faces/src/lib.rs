//! Face detection and clustering traits, mocks and optional ONNX implementation (SPEC §6.15).
#![warn(missing_docs)]

mod align;
mod cluster;
mod detect;
mod error;
mod mock;
mod models;
#[cfg(feature = "onnx")]
mod onnx;
mod pipeline;

pub use align::{ALIGNED_EDGE, align_face};
pub use cluster::{
    ClusterIndex, DEFAULT_THRESHOLD, agglomerate, cosine, dot, l2_normalize, stable_labels,
};
pub use detect::{Detection, EMBEDDING_DIM, FaceDetector, FaceEmbedder, Landmarks};
pub use error::{FaceError, Result};
pub use mock::{MOCK_PALETTE, MockDetector, MockEmbedder, draw_mock_face};
pub use models::{
    DETECTOR_MODEL, EMBEDDER_MODEL, MODELS, ModelSpec, model_path, models_present, sha256_hex,
    verify_model,
};
#[cfg(feature = "onnx")]
pub use onnx::{OnnxDetector, OnnxEmbedder, load_onnx_pipeline};
pub use pipeline::{DETECT_EDGE, FacePipeline, FoundFace, MIN_FACE_PX, MIN_SCORE};
