//! Fourteen world cutscenes directly from disc 1's FIELD/e00.d–e13.d.
use crate::{
    dol, native_archive::member, rel::Rel, world_cinematic_rules, world_gpl, world_motion,
    world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, CameraKey, SceneClip,
    camera_motion::CameraMotion,
    library::Mappings,
    overworld::document::{self, Model, Presentation},
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
struct Names {
    name: String,
    actors: BTreeMap<u8, String>,
}

fn clip(path: String, slot: u16) -> SceneClip {
    SceneClip {
        motion: path,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: None,
        pose_chains: Vec::new(),
    }
}

pub fn camera(source: &[u8]) -> Result<Vec<u8>> {
    ensure!(source.starts_with(b"CAMM"), "missing cinematic CAMM camera");
    let count = dol::u32(source, 8)? as usize;
    let target = dol::u32(source, 4)? as usize + 12;
    ensure!(
        count >= 2 && count <= 10_000,
        "invalid cinematic camera key count"
    );
    let mut keys = Vec::with_capacity(count);
    for index in 0..count {
        let position = 12 + index * 36;
        let aim = target + index * 20;
        let time = f32::from_bits(dol::u32(source, position)?);
        ensure!(
            time == f32::from_bits(dol::u32(source, aim)?),
            "camera position and target times differ"
        );
        let point = |at| -> Result<[f32; 3]> {
            Ok([
                f32::from_bits(dol::u32(source, at)?),
                f32::from_bits(dol::u32(source, at + 4)?),
                f32::from_bits(dol::u32(source, at + 8)?),
            ])
        };
        keys.push(CameraKey {
            time,
            position: point(position + 4)?,
            target: point(aim + 4)?,
            orientation: [
                f32::from_bits(dol::u32(source, position + 16)?),
                f32::from_bits(dol::u32(source, position + 20)?),
                f32::from_bits(dol::u32(source, position + 24)?),
                f32::from_bits(dol::u32(source, position + 28)?),
            ],
        });
    }
    CameraMotion::from_keys(&keys)?.encode()
}

/// The output set is collected before writing so an unsupported native asset
/// cannot leave a half-published cinematic owner.
pub fn cook_world_cinematics(root: &Path, library: &Path, selected: Option<u16>) -> Result<()> {
    let rel = Rel::read(fs::read(root.join("disc1/files/US_r_Top2field.rel"))?)?;
    let rules = world_cinematic_rules::read(&rel)?;
    let names: BTreeMap<u16, Names> =
        serde_json::from_str(include_str!("world-cinematic-names.json"))?;
    ensure!(
        names.keys().copied().eq(513..=526),
        "incomplete cinematic names"
    );
    let native_slots = rel.slice(5, 0x168, 8)?;
    let motion_slots = (0..4)
        .map(|index| {
            Ok(usize::from(
                dol::u16(native_slots, index * 2)?
                    .checked_sub(1)
                    .context("invalid world motion slot")?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let mut artwork = Vec::<(String, Vec<u8>)>::new();
    let mut mappings = BTreeMap::new();

    for (index, (id, names)) in names.into_iter().enumerate() {
        if selected.is_some_and(|wanted| wanted != id) {
            continue;
        }
        let rule = rules.get(&id).context("missing world cinematic rule")?;
        ensure!(
            names
                .actors
                .keys()
                .all(|actor| rule.presentation.actors.contains_key(actor)),
            "cinematic actor lacks source presentation rule"
        );
        let archive = fs::read(root.join(format!("disc1/files/FIELD/e{index:02}.d")))?;
        let count = dol::u32(&archive, 0)? as usize;
        let owner = format!("world/cinematics/{}", names.name);
        let camera_source = member(&archive, 0, count)?.context("missing cinematic camera")?;
        let camera_path = format!("{owner}/camera.glb");
        files.insert(camera_path.clone(), camera(camera_source)?);
        let mut actors = BTreeMap::new();

        for (slot, name) in names.actors {
            let package = member(&archive, usize::from(slot), count)?
                .with_context(|| format!("missing cinematic {} actor {slot}", names.name))?;
            let package_count = dol::u32(package, 0)? as usize;
            ensure!(package_count == 19, "unexpected cinematic actor package");
            let resource =
                member(package, 0, package_count)?.context("missing cinematic actor body")?;
            ensure!(
                dol::u32(resource, 0x20)? == 0x005b_bc61,
                "cinematic actor has no GPL model"
            );
            let start = dol::u32(resource, 0)? as usize;
            let end = dol::u32(resource, 4)? as usize;
            let tpl = resource
                .get(start..end)
                .context("cinematic texture catalogue exceeds model")?;
            ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing cinematic TPL");
            let mut textures = Vec::new();
            let mut alpha = Vec::new();
            for texture in 0..dol::u32(tpl, 4)? as usize {
                let candidate = format!(
                    "world/cinematics/artwork/{}-{name}-{}.ktx2",
                    names.name,
                    texture + 1
                );
                let (bytes, transparent) = world_texture::tpl_texture(tpl, texture)?;
                let path = if let Some((path, _)) =
                    artwork.iter().find(|(_, existing)| *existing == bytes)
                {
                    path.clone()
                } else {
                    artwork.push((candidate.clone(), bytes));
                    candidate
                };
                textures.push(path);
                alpha.push(transparent);
            }
            let (glb, mut part) = world_gpl::static_model(resource, textures, alpha)
                .with_context(|| format!("convert cinematic {} actor {name}", names.name))?;
            if matches!(id, 514 | 515) && slot == 1 {
                ensure!(
                    part.textures.len() == 9,
                    "Thoda scene has unexpected Lloyd appearance atlas"
                );
                part.appearance = Some(AppearanceTextures {
                    eyes: Some(7),
                    mouth: Some(6),
                    variant: None,
                    costume: None,
                });
                part.secondary_motion = world_gpl::thoda_cape(resource)?;
            }
            let mesh = format!("{owner}/{name}.glb");
            files.insert(mesh.clone(), glb.clone());
            part.mesh = mesh;
            for (motion_index, &native_slot) in motion_slots.iter().enumerate() {
                if native_slot >= package_count {
                    continue;
                }
                if let Some(source) = member(package, native_slot, package_count)? {
                    let path = format!("{owner}/{name}-motion-{motion_index}.glb");
                    files.insert(
                        path.clone(),
                        world_motion::encode(source, &glb).with_context(|| {
                            format!(
                                "convert cinematic {} actor {name} motion {motion_index}",
                                names.name
                            )
                        })?,
                    );
                    let mut bound = clip(path, motion_index as u16);
                    if matches!(id, 514 | 515) && slot == 1 {
                        bound.pose_chains = vec!["cape-left".into(), "cape-right".into()];
                    }
                    part.clips.push(bound);
                }
            }
            let path = format!("{owner}/{name}.json");
            files.insert(
                path.clone(),
                resonance_asset_writer::json::encode(&Model { parts: vec![part] })?,
            );
            actors.insert(slot, path);
        }
        files.insert(
            format!("{owner}/cinematic.json"),
            resonance_asset_writer::json::encode(&Presentation {
                world: rule.world,
                presentation: rule.presentation.clone(),
                camera: camera_path,
                actors,
                dialogue: rule.dialogue.clone(),
            })?,
        );
        mappings.insert(u32::from(id), names.name);
    }
    files.extend(artwork);
    ensure!(!mappings.is_empty(), "unknown world cinematic ID");
    if selected.is_none() {
        files.insert(
            document::CINEMATICS.into(),
            resonance_asset_writer::json::encode(&Mappings::new(mappings)?)?,
        );
    }
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
