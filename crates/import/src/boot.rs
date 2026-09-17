//! Decode original startup textures in Rust into the common KTX2 profile.
use super::*;
use resonance_content::{BootAssets, BootTexture};

pub fn cook(extracted: &Path, output: &Path) -> Result<()> {
    let _publications = crate::publication::Session::start_if_needed(output)?;
    let boot = fs::read(extracted.join("sys/boot.bin"))?;
    ensure!(
        boot.get(..6) == Some(b"GQSEAF") && boot.get(7) == Some(&0),
        "expected GQSEAF revision 0"
    );
    let source = fs::read(extracted.join("files/boot.tpl"))?;
    let decoded = tpl::decode(&source)?;
    ensure!(
        decoded.len() == 4,
        "expected four original startup textures"
    );
    fs::create_dir_all(output.join("boot"))?;
    let mut textures = Vec::new();
    for (index, (width, height, pixels)) in decoded.into_iter().enumerate() {
        let background = pixels[..3].try_into()?;
        let path = format!("boot/{index:02}.ktx2");
        crate::texture::cook(width, height, &pixels, &output.join(&path))?;
        textures.push(BootTexture {
            path,
            width,
            height,
            background,
        });
    }
    let manifest = BootAssets {
        version: 1,
        source_sha256: digest(&source),
        textures,
    };
    manifest.validate()?;
    write_atomic(
        &output.join("boot.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("Cooked four startup logos");
    Ok(())
}
