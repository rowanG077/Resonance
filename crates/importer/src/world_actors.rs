//! World-actor models from the single disc-1 common archive.
use crate::{
    compression, dol,
    native_archive::{member, read_named},
    text, world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, CullFace, SceneClip,
    field::document::Model as CharacterModel,
    library::Mappings,
    overworld::document::{self, Model},
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::WorldActorId;

const OWNERS: [(usize, WorldActorId); 20] = [
    (0, WorldActorId::EnemySymbol0),
    (1, WorldActorId::EnemySymbol1),
    (2, WorldActorId::Rheaird),
    (3, WorldActorId::Noishe),
    (4, WorldActorId::Ship),
    (5, WorldActorId::FieldPoint),
    (6, WorldActorId::RheairdExhaust),
    (7, WorldActorId::RheairdRiderLloyd),
    (8, WorldActorId::RheairdRiderColette),
    (9, WorldActorId::RheairdRiderGenis),
    (10, WorldActorId::RheairdRiderRaine),
    (11, WorldActorId::RheairdRiderSheena),
    (12, WorldActorId::RheairdRiderZelos),
    (13, WorldActorId::RheairdRiderPresea),
    (14, WorldActorId::RheairdRiderRegal),
    (15, WorldActorId::RheairdRiderKratos),
    (16, WorldActorId::ShipWake),
    (17, WorldActorId::ShipTravelEffect),
    (18, WorldActorId::FlightBarrier),
    (19, WorldActorId::WorldPortal),
];

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

pub fn cook_world_actors_common(root: &Path, library: &Path) -> Result<()> {
    let source = fs::read(root.join("disc1/files/FIELD/enemy.d"))?;
    let archive = compression::cabinet(&source)?;
    let mut files = BTreeMap::new();
    for (slot, actor) in OWNERS {
        let name = actor.resource_name().context("unnamed world actor")?;
        let package =
            member(&archive, slot, 21)?.with_context(|| format!("missing world actor {name}"))?;
        let nested = matches!(slot, 0 | 1 | 2 | 3 | 4 | 5 | 6 | 16 | 17 | 19);
        let resource = if nested {
            member(package, 0, if slot <= 1 { 31 } else { 19 })?
                .context("missing world actor body")?
        } else {
            package
        };
        ensure!(
            dol::u32(resource, 0x20)? == 0x005b_bc61,
            "{name} is not a direct GPL model"
        );
        let start = dol::u32(resource, 0)? as usize;
        let end = dol::u32(resource, 4)? as usize;
        let tpl = resource
            .get(start..end)
            .context("actor TPL exceeds model")?;
        ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing actor TPL");
        let count = dol::u32(tpl, 4)? as usize;
        let base = format!("world/actors/{name}");
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for index in 0..count {
            let path = format!("{base}/textures/part-0-texture-{index}.ktx2");
            let (texture, transparent) = world_texture::tpl_texture(tpl, index)
                .with_context(|| format!("convert {name} texture {index}"))?;
            files.insert(path.clone(), texture);
            paths.push(path);
            alpha.push(transparent);
        }
        let mut parts = Vec::new();
        let mut glbs = Vec::new();
        for index in 0..if matches!(slot, 0 | 1 | 3) { 2 } else { 1 } {
            let normalized = if index == 0 {
                std::borrow::Cow::Borrowed(resource)
            } else {
                let secondary = member(package, 1, if slot <= 1 { 31 } else { 19 })?
                    .context("missing world actor outline")?;
                std::borrow::Cow::Owned(world_gpl::with_primary_palette(resource, secondary)?)
            };
            let (glb, mut part) =
                world_gpl::static_model(&normalized, paths.clone(), alpha.clone())
                    .with_context(|| format!("convert world actor {name} part {index}"))?;
            let mesh = format!("{base}/meshes/part-{index}.glb");
            part.mesh = mesh.clone();
            if index == 1 {
                part.resource = 1;
                part.outline_color = Some([0, 0, 0, 127]);
                for material in &mut part.materials {
                    material.cull = CullFace::Front;
                    material.blend = true;
                    material.draw_order += 65536;
                }
            } else {
                let costume = resource.get(0x0f).and_then(|value| value.checked_sub(1));
                part.appearance = Some(AppearanceTextures {
                    eyes: None,
                    mouth: None,
                    variant: None,
                    costume: costume.map(usize::from),
                });
            }
            if slot == 3 {
                part.secondary_motion = world_gpl::noishe_hair(&normalized)?;
                if index == 0 {
                    part.appearance = Some(AppearanceTextures {
                        eyes: Some(0),
                        mouth: None,
                        variant: None,
                        costume: None,
                    });
                }
            }
            files.insert(mesh, glb.clone());
            glbs.push(glb);
            parts.push(part);
        }
        let motion_slots: &[usize] = match slot {
            0 | 1 => &[2, 8, 9, 19],
            3 => &[2, 8, 9],
            4 => &[2, 8],
            16 => &[2],
            17 => &[2],
            19 => &[2],
            _ => &[],
        };
        for (index, &motion_slot) in motion_slots.iter().enumerate() {
            let motion = member(package, motion_slot, if slot <= 1 { 31 } else { 19 })?
                .with_context(|| format!("missing world actor {name} motion {index}"))?;
            let path = format!("{base}/animations/motion-{index}.glb");
            files.insert(path.clone(), world_motion::encode(motion, &glbs[0])?);
            for part in &mut parts {
                part.clips.push(clip(path.clone(), index as u16));
            }
        }
        if slot == 4 {
            member(package, 9, 19)?.context("missing ship motion alias")?;
            parts[0]
                .clips
                .push(clip(format!("{base}/animations/motion-1.glb"), 2));
        }
        if matches!(slot, 5 | 6) {
            let motion = member(package, 2, 19)?.context("missing world actor idle motion")?;
            let path = format!("{base}/animations/idle.glb");
            files.insert(path.clone(), world_motion::encode(motion, &glbs[0])?);
            parts[0].clips.push(clip(path, 0));
        }
        files.insert(
            format!("{base}/model.json"),
            resonance_asset_writer::json::encode(&Model { parts })?,
        );
    }
    files.insert(
        document::ACTORS.into(),
        resonance_asset_writer::json::encode(&Mappings::new(
            WorldActorId::ALL
                .into_iter()
                .map(|id| (u32::from(id.raw()), id.resource_name().unwrap().to_owned()))
                .collect(),
        )?)?,
    );
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}

/// Party bodies and ordinary motions belong to the shared character owner.
/// The world adds only its two mount motions and a binding document.
pub fn cook_world_actors_party(root: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let source = fs::read(root.join("disc1/files/FIELD/enemy.d"))?;
    let archive = compression::cabinet(&source)?;
    let mounts = member(&archive, 20, 21)?.context("missing world mount motion bank")?;
    let mut files = BTreeMap::new();
    for (index, id) in (1_u16..=9).enumerate() {
        let actor = WorldActorId::try_from(id)?;
        let name = actor.resource_name().context("unnamed party world actor")?;
        let base = format!("world/actors/{name}");
        let character = format!("meshes/characters/{name}");
        let model: CharacterModel = serde_json::from_slice(
            &fs::read(library.join(format!("{character}/model.json")))
                .with_context(|| format!("missing shared character model {name}"))?,
        )?;
        ensure!(
            !model.parts.is_empty(),
            "empty shared character model {name}"
        );
        let body = fs::read(library.join(&model.parts[0].mesh))
            .with_context(|| format!("missing shared character body {name}"))?;
        let body_pointer = dol::u32(dol::slice(&exe, 0x801f_ace4 + index as u32 * 20, 4)?, 0)?;
        let body_source = read_named(
            &root.join("disc1/files"),
            &text::string(&exe, body_pointer)?,
        )?;
        let native_body = member(&body_source, 0, 2)?
            .with_context(|| format!("missing native character body {name}"))?;
        let bindings = world_gpl::animation_bindings(native_body)?;

        // The executable names one native field-motion package per character.
        // Its selected members establish which of the four common travel slots exist.
        let row = dol::slice(&exe, 0x8017_e4c0 + index as u32 * 12, 12)?;
        let end = row
            .iter()
            .position(|&byte| byte == 0)
            .context("unterminated field motion name")?;
        let filename = std::str::from_utf8(&row[..end])?;
        let motions = read_named(&root.join("disc1/files"), filename)?;
        let mut parts = model.parts;
        for part in &mut parts {
            let shared = std::mem::take(&mut part.clips);
            for (slot, (native, shared_slot)) in [(2, 12), (8, 36), (9, 40), (19, 80)]
                .into_iter()
                .enumerate()
            {
                if member(&motions, native, 31)?.is_none() {
                    continue;
                }
                let mut clip = shared
                    .iter()
                    .find(|clip| {
                        clip.resource_slot == shared_slot && clip.animation_resource.is_none()
                    })
                    .with_context(|| {
                        format!("missing shared character {name} motion {shared_slot}")
                    })?
                    .clone();
                clip.resource_slot = slot as u16;
                part.clips.push(clip);
            }
            for slot in 0..2 {
                part.clips.push(clip(
                    format!("{base}/animations/motion-{}.glb", slot + 4),
                    (slot + 4) as u16,
                ));
            }
        }
        for slot in 0..2 {
            let motion = member(mounts, index * 2 + slot, 18)?
                .with_context(|| format!("missing {name} mount motion {slot}"))?;
            files.insert(
                format!("{base}/animations/motion-{}.glb", slot + 4),
                world_motion::encode_bound(&compression::cabinet(motion)?, &body, &bindings)?,
            );
        }
        files.insert(
            format!("{base}/model.json"),
            resonance_asset_writer::json::encode(&Model { parts })?,
        );
    }
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
