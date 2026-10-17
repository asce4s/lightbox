//! `cargo xtask models`: downloads the face models with SHA-256 verification (SPEC §6.15).

use anyhow::{Context, Result, bail};
use lb_faces::{MODELS, ModelSpec, model_path, sha256_hex, verify_model};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where models go: `--dir`, `LIGHTBOX_FACE_MODELS`, `LIGHTBOX_HOME/data/models`, or the
/// per-user data directory.
pub fn target_dir(arg: Option<&str>) -> Result<PathBuf> {
    if let Some(dir) = arg {
        return Ok(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::var_os("LIGHTBOX_FACE_MODELS").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    if let Some(home) = std::env::var_os("LIGHTBOX_HOME").filter(|d| !d.is_empty()) {
        return Ok(Path::new(&home).join("data").join("models"));
    }
    let dirs = directories::ProjectDirs::from("app", "Lightbox", "Lightbox")
        .context("no home directory could be determined")?;
    Ok(dirs.data_dir().join("models"))
}

/// Fetches the bytes behind a URL.
pub type Fetch<'a> = dyn Fn(&str) -> Result<Vec<u8>> + 'a;

/// Downloads `spec` into `dir` unless a file with the right checksum is already there. The
/// download goes to a temporary file that is renamed only after the checksum matched. Returns
/// whether a download happened.
pub fn ensure_model(dir: &Path, spec: &ModelSpec, fetch: &Fetch<'_>) -> Result<bool> {
    if verify_model(dir, spec).is_ok() {
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    let bytes = fetch(spec.url).with_context(|| format!("downloading {}", spec.url))?;
    let digest = sha256_hex(&bytes[..])?;
    if digest != spec.sha256 {
        bail!(
            "{} has checksum {digest}, expected {}; refusing to install it",
            spec.file,
            spec.sha256
        );
    }
    let target = model_path(dir, spec);
    let temp = target.with_extension("onnx.part");
    std::fs::write(&temp, &bytes)?;
    std::fs::rename(&temp, &target)?;
    Ok(true)
}

fn http_fetch(url: &str) -> Result<Vec<u8>> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| anyhow::anyhow!("request failed: {e}"))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(256 * 1024 * 1024)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Runs the subcommand.
pub fn run(dir_arg: Option<&str>) -> Result<()> {
    let dir = target_dir(dir_arg)?;
    println!("models directory: {}", dir.display());
    for spec in MODELS {
        let downloaded = ensure_model(&dir, &spec, &http_fetch)?;
        println!(
            "{} {} ({}, license {})",
            if downloaded {
                "downloaded"
            } else {
                "verified  "
            },
            spec.file,
            spec.name,
            spec.license
        );
    }
    println!(
        "ONNX Runtime is not bundled: set ORT_DYLIB_PATH or place libonnxruntime next to the models."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn fake_spec(bytes: &[u8]) -> ModelSpec {
        let digest: &'static str = Box::leak(sha256_hex(bytes).unwrap().into_boxed_str());
        ModelSpec {
            name: "fake",
            file: "fake.onnx",
            url: "https://example.invalid/fake.onnx",
            sha256: digest,
            license: "MIT",
        }
    }

    #[test]
    fn download_is_verified_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let spec = fake_spec(b"model bytes");
        let calls = Cell::new(0);
        let fetch = |_: &str| {
            calls.set(calls.get() + 1);
            Ok(b"model bytes".to_vec())
        };
        assert!(ensure_model(dir.path(), &spec, &fetch).unwrap());
        assert_eq!(
            std::fs::read(dir.path().join("fake.onnx")).unwrap(),
            b"model bytes"
        );
        assert!(
            !ensure_model(dir.path(), &spec, &fetch).unwrap(),
            "already verified"
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn corrupt_downloads_are_rejected_and_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let spec = fake_spec(b"good");
        let err = ensure_model(dir.path(), &spec, &|_: &str| Ok(b"evil".to_vec())).unwrap_err();
        assert!(err.to_string().contains("checksum"));
        assert!(!dir.path().join("fake.onnx").exists());
        assert!(!dir.path().join("fake.onnx.part").exists());
    }

    #[test]
    fn a_damaged_existing_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let spec = fake_spec(b"good");
        std::fs::write(dir.path().join("fake.onnx"), b"damaged").unwrap();
        assert!(ensure_model(dir.path(), &spec, &|_: &str| Ok(b"good".to_vec())).unwrap());
        assert_eq!(
            std::fs::read(dir.path().join("fake.onnx")).unwrap(),
            b"good"
        );
    }

    #[test]
    fn explicit_directory_wins() {
        assert_eq!(target_dir(Some("/x/y")).unwrap(), PathBuf::from("/x/y"));
    }
}
