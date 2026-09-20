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
pub(crate) use boot::cook as cook_boot;
mod afs;
mod dol;
mod embedded;
mod event_bank_directory;
pub mod field;
mod field_catalogue;
mod field_doors;
mod field_effects;
mod field_lighting;
mod field_overlay;
mod field_preload;
mod field_resources;
mod field_shadow;
mod figurines;
mod font;
mod font_directory;
mod geometry;
mod glow;
mod item;
pub mod media;
pub mod menu;
mod model;
mod model_behavior;
mod model_preview;
mod monsters;
mod music_directory;
mod publication;
mod read;
mod rel;
mod resource;
mod scene;
mod secondary_motion;
mod session;
mod shared;
mod skit;
pub(crate) mod source_assets;
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
    io::Read,
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
    publication::File::write(path, bytes).map(|_| ())
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

/// Decode original title resources and publish their completed runtime assets.
pub(crate) fn cook_title(extracted: &Path, output: &Path) -> Result<()> {
    let _publications = publication::Session::start_if_needed(output)?;
    disc_number(extracted)?;
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let recipe = scene::title::Recipe::read(extracted, &executable)?;
    let images = recipe.images.textures(extracted)?;
    let textures = images
        .catalogue
        .textures
        .iter()
        .enumerate()
        .map(|(index, texture)| {
            let image = texture.as_ref().context("invalid title image")?.image(0)?;
            Ok(TitleTexture {
                index,
                path: image.path,
                width: image.width,
                height: image.height,
            })
        })
        .collect::<Result<_>>()?;
    images.write(output)?;
    let scene = Some(scene::bind_title(extracted, output, &recipe, &executable)?);
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
