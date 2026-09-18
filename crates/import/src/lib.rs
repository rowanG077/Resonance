//! One-time disc extraction and asset conversion.
mod animation;
mod boot;
mod character;
mod compression;
pub use boot::cook as cook_boot;
mod dol;
pub mod field;
mod field_caption;
mod field_doors;
mod field_effects;
pub use field_effects::cook_all as cook_effects;
mod field_lighting;
pub mod field_preload;
mod field_resources;
mod field_shadow;
pub mod figurines;
mod font;
mod geometry;
pub mod menu;
mod model_preview;
pub mod monsters;
pub use font::cook as cook_font;
mod afs;
mod glow;
pub mod media;
mod model;
mod read;
mod scene;
mod secondary_motion;
mod session;
pub mod skit;
mod texture;
mod texture_animation;
pub mod tpl;

use anyhow::{Context, Result, ensure};
use nod::{
    common::PartitionKind,
    read::{DiscOptions, DiscReader, PartitionOptions},
};
use resonance_content::{CONTENT_VERSION, TitleAssets, TitleTexture};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("path has no parent")?)?;
    if fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    let temp = path.with_extension("partial");
    let mut file = fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}

/// Refresh localized labels and the field manifests that depend on them.
pub fn cook_text(extracted: &Path, output: &Path) -> Result<()> {
    let path = session::cook_text(extracted, output)?;
    field::refresh_shared(output, &[path])
}

/// Extract a disc once. All subsequent conversion uses this filesystem tree.
pub fn extract(disc_path: &Path, output: &Path) -> Result<()> {
    let disc = DiscReader::new(disc_path, &DiscOptions::default())?;
    let header = disc.header();
    ensure!(
        header.game_id_str() == "GQSEAF" && header.disc_version == 0 && header.is_gamecube(),
        "expected GameCube GQSEAF revision 0"
    );
    let mut partition =
        disc.open_partition_kind(PartitionKind::Data, &PartitionOptions::default())?;
    let meta = partition.meta()?;
    let mut inventory = Vec::new();
    for (name, bytes) in [
        ("boot.bin", meta.raw_boot.as_slice()),
        ("bi2.bin", meta.raw_bi2.as_slice()),
        ("apploader.img", meta.raw_apploader.as_ref()),
        ("main.dol", meta.raw_dol.as_ref()),
        ("fst.bin", meta.raw_fst.as_ref()),
    ] {
        write_atomic(&output.join("sys").join(name), bytes)?;
    }
    let fst = meta.fst().map_err(anyhow::Error::msg)?;
    for (_, node, name) in fst.iter() {
        ensure!(
            Path::new(&name)
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "unsafe disc path {name}"
        );
        if !node.is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        partition.open_file(node)?.read_to_end(&mut bytes)?;
        write_atomic(&output.join("files").join(&name), &bytes)?;
        inventory.push(serde_json::json!({"path": name, "size": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes))}));
    }
    write_atomic(
        &output.join("disc.json"),
        &serde_json::to_vec_pretty(
            &serde_json::json!({"game_id": header.game_id_str(), "revision": header.disc_version, "disc": header.disc_num + 1, "files": inventory}),
        )?,
    )?;
    println!(
        "Extracted {} files into {}",
        inventory.len(),
        output.display()
    );
    Ok(())
}

/// Convert title art into lossless runtime textures.
pub fn cook_title(extracted: &Path, output: &Path) -> Result<()> {
    let boot = fs::read(extracted.join("sys/boot.bin"))
        .context("missing extracted sys/boot.bin; run extract first")?;
    ensure!(
        boot.get(..6) == Some(b"GQSEAF") && boot.get(7) == Some(&0),
        "expected GQSEAF revision 0"
    );
    let source = fs::read(extracted.join("files/title.tpl"))?;
    let hash = format!("{:x}", Sha256::digest(&source));
    let decoded = tpl::decode(&source)?;
    let texture_dir = output.join("title");
    fs::create_dir_all(&texture_dir)?;
    let mut textures = Vec::new();
    for (index, (width, height, pixels)) in decoded.into_iter().enumerate() {
        let path = format!("title/{index:02}.ktx2");
        crate::texture::cook(width, height, &pixels, &output.join(&path))?;
        textures.push(TitleTexture {
            index,
            path,
            width,
            height,
        });
    }
    let scene = Some(scene::cook(
        &extracted.join("files/MAP/tit_t00.bin"),
        &extracted.join("sys/main.dol"),
        output,
    )?);
    let manifest = TitleAssets {
        version: CONTENT_VERSION,
        game_id: "GQSEAF".into(),
        revision: 0,
        source_sha256: hash,
        textures,
        scene,
    };
    manifest.validate()?;
    write_atomic(
        &output.join("title.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!(
        "Converted {} title textures into {}",
        manifest.textures.len(),
        output.display()
    );
    Ok(())
}
