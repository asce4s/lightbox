//! The ONNX models Lightbox uses and their checksums (SPEC §6.15). Models are downloaded on
//! request by `cargo xtask models`; they are never committed or bundled.

use crate::error::{FaceError, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// A downloadable model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Human-readable name.
    pub name: &'static str,
    /// File name inside the models directory.
    pub file: &'static str,
    /// Where it is downloaded from.
    pub url: &'static str,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: &'static str,
    /// License of the model.
    pub license: &'static str,
}

/// YuNet face detector (OpenCV Zoo, 2023 March, MIT).
pub const DETECTOR_MODEL: ModelSpec = ModelSpec {
    name: "YuNet face detector",
    file: "face_detection_yunet_2023mar.onnx",
    url: "https://github.com/opencv/opencv_zoo/raw/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
    sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
    license: "MIT",
};

/// SFace face recognizer (OpenCV Zoo, 2021 December, Apache-2.0).
pub const EMBEDDER_MODEL: ModelSpec = ModelSpec {
    name: "SFace face recognizer",
    file: "face_recognition_sface_2021dec.onnx",
    url: "https://github.com/opencv/opencv_zoo/raw/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx",
    sha256: "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
    license: "Apache-2.0",
};

/// Every model the face feature needs.
pub const MODELS: [ModelSpec; 2] = [DETECTOR_MODEL, EMBEDDER_MODEL];

/// Lowercase hex SHA-256 of the bytes of `reader`.
pub fn sha256_hex(mut reader: impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Path of `spec` inside `dir`.
pub fn model_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(spec.file)
}

/// Checks that `spec` exists in `dir` and has the expected checksum; returns its path.
pub fn verify_model(dir: &Path, spec: &ModelSpec) -> Result<PathBuf> {
    let path = model_path(dir, spec);
    let digest = sha256_hex(std::fs::File::open(&path)?)?;
    if digest != spec.sha256 {
        return Err(FaceError::Checksum {
            name: spec.file.to_owned(),
        });
    }
    Ok(path)
}

/// Whether every model is in `dir` with the right checksum.
pub fn models_present(dir: &Path) -> bool {
    MODELS.iter().all(|m| verify_model(dir, m).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_input() {
        assert_eq!(
            sha256_hex(&b"abc"[..]).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(&b""[..]).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn verify_rejects_missing_and_modified_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            verify_model(dir.path(), &DETECTOR_MODEL),
            Err(FaceError::Io(_))
        ));
        std::fs::write(model_path(dir.path(), &DETECTOR_MODEL), b"not a model").unwrap();
        assert!(matches!(
            verify_model(dir.path(), &DETECTOR_MODEL),
            Err(FaceError::Checksum { .. })
        ));
        assert!(!models_present(dir.path()));
    }

    #[test]
    fn specs_are_well_formed() {
        for m in MODELS {
            assert_eq!(m.sha256.len(), 64);
            assert!(
                m.sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
            assert!(m.url.starts_with("https://"));
            assert!(m.url.ends_with(m.file));
        }
    }
}
