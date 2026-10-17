//! Detects libheif when the `heif` feature is on. A missing library does not fail the build:
//! HEIC decoding then reports itself unavailable (`heif_decoder_available`).

fn main() {
    println!("cargo::rustc-check-cfg=cfg(lb_heif)");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    if std::env::var_os("CARGO_FEATURE_HEIF").is_none() {
        return;
    }
    let found = std::process::Command::new("pkg-config")
        .args(["--exists", "libheif"])
        .status()
        .is_ok_and(|status| status.success());
    if found {
        println!("cargo:rustc-cfg=lb_heif");
        println!("cargo:rustc-link-lib=heif");
    }
}
