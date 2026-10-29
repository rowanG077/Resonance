//! One native terrain package directly into its final tile files.
use crate::{dol, world_gpl, world_terrain_collision, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::document::Terrain;
use std::{
    fs,
    path::{Component, Path},
};

pub fn cook_world_terrain_tile(
    source: &Path,
    library: &Path,
    owner: &str,
    alternate: bool,
) -> Result<()> {
    ensure!(
        !owner.is_empty()
            && Path::new(owner)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "terrain owner must be a relative library path"
    );
    let tile = fs::read(source)?;
    let mut base_textures = Vec::new();
    if alternate {
        let filename = source
            .file_name()
            .and_then(|name| name.to_str())
            .context("alternate terrain source has no filename")?;
        let stem = filename
            .strip_suffix("a.dat")
            .context("alternate terrain source must end in a.dat")?;
        let base = fs::read(source.with_file_name(format!("{stem}.dat")))?;
        let mut part = 0;
        for layer in [0, 2] {
            if dol::u32(&base, 4 + layer * 4)? == 0 {
                continue;
            }
            for index in 0..world_texture::texture_count(&base, layer)? {
                let path = format!("{owner}/part-{part}-texture-{index}.ktx2");
                base_textures.push((path, world_texture::texture(&base, layer, index)?.0));
            }
            part += 1;
        }
    }
    let prefix = if alternate { "alternate-" } else { "" };
    let mut files = Vec::<(String, Vec<u8>)>::new();
    let collision = format!("{owner}/{prefix}collision.glb");
    files.push((
        collision.clone(),
        world_terrain_collision::collision(&tile)?,
    ));
    let mut parts = Vec::new();
    for layer in [0, 2] {
        if dol::u32(&tile, 4 + layer * 4)? == 0 {
            continue;
        }
        let index = parts.len();
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for texture in 0..world_texture::texture_count(&tile, layer)? {
            let (bytes, transparent) = world_texture::texture(&tile, layer, texture)?;
            let path = if let Some((path, _)) = base_textures
                .iter()
                .find(|(_, original)| original == &bytes)
            {
                ensure!(
                    library.join(path).exists(),
                    "missing base terrain texture {path}"
                );
                path.clone()
            } else if let Some((path, _)) = files
                .iter()
                .find(|(path, original)| path.ends_with(".ktx2") && original == &bytes)
            {
                path.clone()
            } else {
                let path = format!("{owner}/{prefix}part-{index}-texture-{texture}.ktx2");
                files.push((path.clone(), bytes));
                path
            };
            paths.push(path);
            alpha.push(transparent);
        }
        let (glb, mut part) = world_gpl::visual(&tile, layer, paths, alpha)?;
        let mesh = format!("{owner}/{prefix}part-{index}.glb");
        files.push((mesh.clone(), glb));
        part.mesh = mesh;
        parts.push(part);
    }
    let document = if alternate {
        "alternate.json"
    } else {
        "terrain.json"
    };
    files.push((
        format!("{owner}/{document}"),
        resonance_asset_writer::json::encode(&Terrain { collision, parts })?,
    ));
    ensure!(
        files.iter().all(|(path, _)| !library.join(path).exists()),
        "terrain output already exists"
    );
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
