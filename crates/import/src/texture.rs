//! Shared lossless KTX2 profile for decoded GameCube textures.
use anyhow::{Context, Result, ensure};
use std::{path::Path, process::Command};

pub(crate) fn cook(ktx: &Path, png: &Path, output: &Path) -> Result<()> {
    let status = Command::new(ktx)
        .args([
            "create",
            "--format",
            "R8G8B8A8_UNORM",
            "--assign-tf",
            "linear",
            "--zstd",
            "9",
        ])
        .arg(png)
        .arg(output)
        .status()
        .context("could not launch ktx; use nix develop")?;
    ensure!(
        status.success(),
        "texture conversion failed: {}",
        png.display()
    );
    Ok(())
}
