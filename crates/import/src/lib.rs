//! One-time disc extraction and asset conversion.
// Asset readers collect byte spans, including single ranges, rather than integers.
#![allow(clippy::single_range_in_vec_init)]

pub mod all_assets;
mod animation;
mod arte;
mod boot;
mod character;
mod character_data;
mod compression;
mod cooked;
pub use boot::cook as cook_boot;
pub mod battle;
mod dol;
mod embedded;
mod event_bank_directory;
pub mod field;
mod field_catalogue;
mod field_doors;
mod field_effects;
mod field_overlay;
mod font_directory;
pub use field_effects::cook_all as cook_effects;
mod field_lighting;
pub mod field_preload;
mod field_resources;
mod field_shadow;
pub mod figurines;
mod font;
mod geometry;
mod item;
pub mod menu;
mod model_preview;
pub mod monsters;
pub use font::cook as cook_font;
mod afs;
mod glow;
pub mod media;
mod model;
mod music_directory;
mod read;
mod rel;
mod resource;
mod scene;
mod secondary_motion;
mod session;
pub mod skit;
mod stream_mixer;
mod texture;
mod texture_animation;
pub mod tpl;
mod voice_directory;

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
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Disc tables use lowercase directories; extraction retains uppercase names.
fn source_path(source: &str) -> Result<String> {
    let path = match source.split_once('/') {
        Some((directory, file)) => format!("{}/{file}", directory.to_ascii_uppercase()),
        None => source.to_owned(),
    };
    resonance_content::validate_asset_path(&path)?;
    Ok(path)
}

/// Unique sibling that preserves the extension expected by external codecs.
pub(crate) fn temporary_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_stem().unwrap_or_default());
    name.push(format!(
        "-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    if let Some(extension) = path.extension() {
        name.push(".");
        name.push(extension);
    }
    path.with_file_name(name)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("path has no parent")?)?;
    if fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    let temp = temporary_path(path);
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

/// The source header, rather than its directory or argument order, identifies a disc.
pub(crate) fn disc_number(extracted: &Path) -> Result<u8> {
    let mut boot = [0; 8];
    fs::File::open(extracted.join("sys/boot.bin"))
        .and_then(|mut file| file.read_exact(&mut boot))
        .with_context(|| format!("read extracted disc identity: {}", extracted.display()))?;
    ensure!(
        &boot[..6] == b"GQSEAF" && boot[6] <= 1 && boot[7] == 0,
        "expected GQSEAF revision 0 disc 1 or 2: {}",
        extracted.display()
    );
    Ok(boot[6] + 1)
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

/// Bind shared title images and prepare the title scene.
pub fn cook_title(output: &Path, disc: u8) -> Result<()> {
    let recipe = scene::title::Recipe::bind(output, disc)?;
    let textures = recipe
        .images
        .bind(output, disc)?
        .standalone_textures()?
        .into_iter()
        .enumerate()
        .map(|(index, texture)| {
            let image = texture.image(0)?;
            Ok(TitleTexture {
                index,
                path: image.path,
                width: image.width,
                height: image.height,
            })
        })
        .collect::<Result<_>>()?;
    let scene = Some(scene::bind_title(output, disc, &recipe)?);
    let manifest = TitleAssets {
        version: CONTENT_VERSION,
        game_id: recipe.game_id,
        revision: recipe.revision,
        source_sha256: recipe.images.sha256,
        textures,
        scene,
    };
    manifest.validate()?;
    write_atomic(
        &output.join("title.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!(
        "Bound {} shared title textures in {}",
        manifest.textures.len(),
        output.display()
    );
    Ok(())
}
