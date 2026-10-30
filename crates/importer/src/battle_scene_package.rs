//! Stored spells directly to scene rules, effect recipes and editable art.
//! Only native package shapes with direct conversions are admitted here.
#[path = "battle_scene_actions.rs"]
mod actions;

use crate::{
    battle_effect_recipes, battle_equipment, battle_projectiles, character_models, dol, rel::Rel,
    world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    EffectOperation, EffectTexture, UvAnimation, UvChange,
    declaration::Declaration,
    document::{Artwork, Definition},
    visual::ParticleTexture,
};
use resonance_content::battle_projectile::DamageKind;
use resonance_content::battle_scene::{
    CameraBounds, PatternContact, PatternOrigin, PatternSpell, Scene, SceneProjectile, SpellRecipe,
    Texture,
};
use resonance_content::font::UiTexture;
use resonance_content::model_preview::PreviewPart;
use resonance_content::{CullFace, SceneClip};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::BattleSceneId;

pub(crate) fn member<'a>(package: &'a [u8], field: usize) -> Result<Option<&'a [u8]>> {
    let offsets = (4..0x114)
        .step_by(4)
        .map(|at| dol::u32(package, at).map(|offset| offset as usize))
        .collect::<Result<Vec<_>>>()?;
    let start = offsets[(field - 4) / 4];
    if start == 0 {
        return Ok(None);
    }
    ensure!(
        start >= 0x114 && start < package.len(),
        "invalid scene member pointer"
    );
    let end = offsets
        .into_iter()
        .filter(|&offset| offset > start && offset <= package.len())
        .min()
        .unwrap_or(package.len());
    Ok(Some(
        package
            .get(start..end)
            .context("scene member exceeds package")?,
    ))
}

fn package(root: &Path, module: &Rel, id: u16) -> Result<Vec<u8>> {
    let archive = fs::read(root.join("disc1/files/BTL/BTLmagic.dat"))?;
    let offsets = module.slice(5, 0xfe0, 0x1e4)?;
    let slot = usize::from(id - 200);
    let start = dol::u32(offsets, slot * 4)? as usize;
    let end = ((slot + 1)..0x1e4 / 4)
        .map(|at| dol::u32(offsets, at * 4))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|offset| offset as usize)
        .find(|&offset| offset > start)
        .context("stored scene has no package end")?;
    ensure!(
        start < end && end <= archive.len(),
        "stored scene package exceeds archive"
    );
    Ok(archive[start..end].to_vec())
}

fn stationary_projectiles(
    package: &[u8],
    parameters: &[u8],
    module: &Rel,
    id: u16,
) -> Result<Vec<SceneProjectile>> {
    let rows = member(package, 0xfc)?.context("stationary scene has no projectile rows")?;
    let count = rows.len() / 400;
    let row_start = dol::u32(package, 0xfc)? as usize;
    let row_end = dol::u32(package, 0x100)? as usize;
    ensure!(
        count > 0
            && (rows.len() == count * 400
                || row_start + count * 400 == row_end
                || (row_start + count * 400).next_multiple_of(32) == row_end),
        "projectile member is not 400-byte rows with package alignment"
    );
    let start = dol::u32(parameters, 0)? as usize;
    let end = dol::u32(parameters, 4)? as usize;
    let rules = parameters
        .get(start..end)
        .context("scene contact rules exceed parameter block")?;
    ensure!(
        rules.len().is_multiple_of(28),
        "misaligned scene contact rules"
    );
    let reset_scale = f32::from_bits(dol::u32(module.slice(4, 0xce8, 4)?, 0)?);
    let element_sounds = module.slice(5, 0x1404, 9)?;
    ensure!(
        reset_scale.is_finite(),
        "invalid native projectile reset scale"
    );
    rows[..count * 400]
        .chunks_exact(400)
        .enumerate()
        .map(|(index, row)| {
            let kind = match row[17] {
                0 => DamageKind::Slash,
                1 => DamageKind::Thrust,
                2 => DamageKind::Magic,
                _ => anyhow::bail!("unsupported stationary projectile damage kind {}", row[17]),
            };
            let mut projectile = battle_projectiles::scene_record(row, reset_scale)?;
            if id == 205 && index == 1 && row[0x14] == 1 && row[0x8c..0x90] != [0; 4] {
                projectile.unsupported_reason =
                    Some("projectile response requires unsupported instance state".into());
            }
            let hits = rules
                .chunks_exact(28)
                .map(|rule| {
                    let mut hit = battle_projectiles::scene_hit(rule, row[19])
                        .with_context(|| format!("stationary scene hit rule {rule:02x?}"))?;
                    hit.kind = kind;
                    if (1..=8).contains(&rule[2])
                        && hit.sound == Some(u16::from(element_sounds[usize::from(rule[2])]))
                    {
                        hit.sound = None;
                    }
                    Ok(Some(hit))
                })
                .collect::<Result<Vec<_>>>()?;
            // These callbacks use the second hit rule for their third carrier.
            let selected = if id == 213 {
                index
            } else if id == 286 && index >= 2 {
                index - 1
            } else if id == 215 && index == 1
                || matches!(id, 206 | 207 | 219 | 230..=232 | 253 | 283) && index == 2
            {
                1
            } else {
                usize::from(row[18])
            };
            let hit = if hits.is_empty() && selected == 0
                || matches!(id, 305..=308 | 311) && hits.len() == 1 && selected == 1
            {
                None
            } else {
                hits.get(selected)
                    .cloned()
                    .context("scene projectile selects a missing contact rule")?
            };
            Ok(SceneProjectile {
                projectile,
                hit,
                hits,
                hit_errors: BTreeMap::new(),
                unsupported_reason: None,
            })
        })
        .collect()
}

fn native_camera(
    module: &Rel,
    color_offset: usize,
    camera_offset: usize,
) -> Result<(Option<[u8; 3]>, Option<CameraBounds>)> {
    let color = module.slice(4, color_offset, 4)?;
    ensure!(color[3] == 255, "invalid native stage tint alpha");
    let settings = module.slice(4, camera_offset, 8)?;
    let bounds = CameraBounds {
        minimum_distance: f32::from_bits(dol::u32(settings, 0)?),
        minimum_elevation: f32::from_bits(dol::u32(settings, 4)?),
    };
    bounds.validate()?;
    Ok((Some(color[..3].try_into()?), Some(bounds)))
}

fn fixed_camera(
    color: [u8; 3],
    distance: f32,
    elevation: f32,
) -> (Option<[u8; 3]>, Option<CameraBounds>) {
    (
        Some(color),
        Some(CameraBounds {
            minimum_distance: distance,
            minimum_elevation: elevation,
        }),
    )
}

// Ray's callback has one fixed nine-point schedule; the point table and heading
// are native REL data. Keep these small callback constants beside their reader.
fn ray_recipe(module: &Rel) -> Result<SpellRecipe> {
    let contacts = module
        .slice(4, 0x7dac, 9 * 12)?
        .chunks_exact(12)
        .enumerate()
        .map(|(index, row)| {
            let mut offset = [0.; 3];
            for (axis, value) in offset.iter_mut().enumerate() {
                *value = f32::from_bits(dol::u32(row, axis * 4)?);
                ensure!(value.is_finite(), "non-finite Ray contact point");
            }
            Ok(PatternContact {
                tick: 60 + u16::try_from(index)? * 8,
                projectile: 1,
                hit_variant: 0,
                effect_member: 2,
                offset,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let recipe = PatternSpell {
        lifetime: 230,
        origin: PatternOrigin::SceneGround { toward_caster: 1. },
        heading_offset: f32::from_bits(dol::u32(module.slice(4, 0x7e28, 4)?, 0)?),
        startup_member: 1,
        startup_deferred: true,
        contacts,
    };
    recipe.validate()?;
    Ok(SpellRecipe::FixedPattern(recipe))
}

fn scene_cutin(package: &[u8], owner: &str) -> Result<(Texture, Vec<u8>)> {
    let tpl = member(package, 0x104)?.context("scene cut-in is absent")?;
    ensure!(
        dol::u32(tpl, 0)? == 0x0020_af30 && dol::u32(tpl, 4)? == 1,
        "unsupported scene cut-in"
    );
    let descriptor = dol::u32(tpl, dol::u32(tpl, 8)? as usize)? as usize;
    let (image, _) = world_texture::tpl_texture(tpl, 0)?;
    let path = format!("{owner}/cutin/palette-0.ktx2");
    Ok((
        Texture {
            images: vec![UiTexture {
                path,
                width: u32::from(dol::u16(tpl, descriptor + 2)?),
                height: u32::from(dol::u16(tpl, descriptor)?),
            }],
            sampler: world_texture::sampler(tpl, descriptor)?,
        },
        image,
    ))
}

fn static_scene_models(
    package: &[u8],
    owner: &str,
    id: u16,
) -> Result<(BTreeMap<u8, serde_json::Value>, Vec<(String, Vec<u8>)>)> {
    if id == 237 {
        return nurse_models(package, owner);
    }
    if matches!(id, 282 | 284..=293) {
        return summon_models(package, owner, id);
    }
    let mut artwork = BTreeMap::new();
    let mut files = Vec::new();
    let mut images = Vec::<(Vec<u8>, String)>::new();
    let mut meshes = Vec::<(Vec<u8>, String)>::new();
    let mut known = Vec::<(&[u8], serde_json::Value)>::new();
    for slot in 0..10_u8 {
        let Some(model) = member(package, 12 + usize::from(slot) * 4)? else {
            continue;
        };
        if let Some((_, document)) = known.iter().find(|(source, _)| *source == model) {
            artwork.insert(slot, document.clone());
            continue;
        }
        let start = dol::u32(model, 0)? as usize;
        let end = dol::u32(model, 4)? as usize;
        let tpl = model
            .get(start..end)
            .context("scene model TPL exceeds package")?;
        ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "scene model lacks TPL");
        let stem = format!("{owner}/models/model-{slot}-layer-0");
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for index in 0..dol::u32(tpl, 4)? as usize {
            let (image, transparent) = world_texture::tpl_texture(tpl, index)?;
            let path = if let Some((_, path)) = images.iter().find(|(old, _)| old == &image) {
                path.clone()
            } else {
                let path = format!("{stem}-texture-{index}.ktx2");
                images.push((image.clone(), path.clone()));
                files.push((path.clone(), image));
                path
            };
            paths.push(path.clone());
            alpha.push(transparent);
        }
        let (glb, mut scene) = world_gpl::static_model(model, paths, alpha)?;
        scene.mesh = if let Some((_, path)) = meshes.iter().find(|(old, _)| old == &glb) {
            path.clone()
        } else {
            let path = format!("{stem}.glb");
            meshes.push((glb.clone(), path.clone()));
            files.push((path.clone(), glb));
            path
        };
        let document = resonance_content::battle_model::document::Model {
            attachment_nodes: BTreeMap::new(),
            independent_layers: BTreeSet::new(),
            layers: vec![PreviewPart {
                scene,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: Vec::new(),
            }],
        };
        let document = serde_json::to_value(document)?;
        known.push((model, document.clone()));
        artwork.insert(slot, document);
    }
    Ok((artwork, files))
}

/// Summon bodies, outlines and portal motions use the same native slot indices.
fn summon_models(
    package: &[u8],
    owner: &str,
    id: u16,
) -> Result<(BTreeMap<u8, serde_json::Value>, Vec<(String, Vec<u8>)>)> {
    let (mut artwork, mut files) = static_scene_models(package, owner, 0)?;
    for slot in 0..10_u8 {
        let outline = member(package, 0x34 + usize::from(slot) * 4)?;
        let motions = (0..4)
            .map(|index| member(package, 0x5c + usize::from(slot) * 16 + index * 4))
            .collect::<Result<Vec<_>>>()?;
        if outline.is_none() && motions.iter().all(Option::is_none) {
            continue;
        }
        let body = member(package, 0xc + usize::from(slot) * 4)?
            .context("summon outline or motion has no body")?;
        let mut document: resonance_content::battle_model::document::Model =
            serde_json::from_value(artwork.remove(&slot).context("summon model is absent")?)?;
        let primary = &mut document.layers[0].scene;
        for material in &mut primary.materials {
            if material.color.is_none() && material.multiply.is_none() {
                material.blend = false;
            }
        }
        let mesh = files
            .iter()
            .find(|(path, _)| path == &primary.mesh)
            .map(|(_, bytes)| bytes.clone())
            .context("summon mesh is absent")?;
        primary.prepare_bones(&mesh)?;
        let bindings = world_gpl::animation_bindings(body)?;
        let source_names = bindings
            .iter()
            .map(|(_, name)| name.clone())
            .collect::<Vec<_>>();
        let json_len =
            u32::from_le_bytes(mesh.get(12..16).context("short summon GLB")?.try_into()?) as usize;
        let gltf: serde_json::Value = serde_json::from_slice(
            mesh.get(20..20 + json_len)
                .context("short summon GLB JSON")?,
        )?;
        let dynamics = character_models::character_dynamics::bind(
            owner,
            &gltf,
            &source_names,
            &primary.bone_names,
        )?;
        primary.secondary_motion = dynamics.clone();
        let stem = format!("{owner}/models/model-{slot}-layer-0");
        for (index, motion) in motions.into_iter().enumerate() {
            let Some(motion) = motion else { continue };
            ensure!(dol::u32(motion, 0)? == 0x007b_7960, "invalid summon motion");
            let shared = match (id, slot, index) {
                (284, 0, 0) => Some("meshes/figurines/efreet-0080/animations/idle.glb"),
                (284, 0, 1) => Some("meshes/monsters/efreet-0197/animations/motion-32.glb"),
                (286, 0, 0) => Some("meshes/monsters/sephie-0205/animations/motion-32.glb"),
                (286, 1, 0) => Some("meshes/monsters/yutis-0206/animations/motion-30.glb"),
                (286, 2, 0) => Some("meshes/monsters/fairess-0207/animations/motion-31.glb"),
                (287, 0, 0) => Some("meshes/monsters/gnome-0196/animations/motion-0.glb"),
                (287, 0, 1) => Some("meshes/monsters/gnome-0196/animations/motion-36.glb"),
                (287, 0, 2) => Some("meshes/monsters/gnome-0196/animations/motion-37.glb"),
                (288, 0, 0) => Some("meshes/monsters/celsius-0199/animations/motion-0.glb"),
                (288, 0, 1) => Some("meshes/monsters/celsius-0199/animations/motion-40.glb"),
                (288, 0, 2) => Some("meshes/monsters/celsius-0199/animations/motion-41.glb"),
                (289, 0, 0) => Some("meshes/monsters/volt-0198/animations/motion-0.glb"),
                (289, 0, 1) => Some("meshes/monsters/volt-0198/animations/motion-31.glb"),
                (289, 1, 0) => {
                    Some("battle/enemies/volt-0198/attachment-0-layer-0-animation-0.glb")
                }
                (290, 0, 0) => Some("meshes/figurines/luna-0084/animations/idle.glb"),
                (290, 0, 1) => Some("meshes/monsters/luna-0200/animations/motion-34.glb"),
                (291, 0, 0) => Some("meshes/figurines/shadow-0086/animations/idle.glb"),
                (291, 0, 1) => Some("meshes/monsters/shadow-0202/animations/motion-12.glb"),
                (292, 0, 0) => Some("meshes/monsters/maxwell-0203/animations/motion-0.glb"),
                (292, 0, 1) => Some("meshes/monsters/maxwell-0203/animations/motion-34.glb"),
                (293, 0, 0) => Some("meshes/monsters/origin-0204/animations/motion-0.glb"),
                (293, 0, 1) => Some("meshes/monsters/origin-0204/animations/motion-12.glb"),
                _ => None,
            };
            let path = if let Some(shared) = shared {
                shared.to_owned()
            } else {
                let path = format!("{stem}-animation-{index}.glb");
                files.push((
                    path.clone(),
                    world_motion::encode_bound(motion, &mesh, &bindings)
                        .with_context(|| format!("summon {id} body motion {slot}/{index}"))?,
                ));
                path
            };
            primary.clips.push(SceneClip {
                motion: path,
                joints: BTreeMap::new(),
                resource_slot: index as u16,
                duration_seconds: 0.,
                animation_resource: None,
                pose_chains: Vec::new(),
            });
        }
        if let Some(outline) = outline {
            let owns_palette = dol::u32(outline, 0)? != 0;
            let source = if owns_palette {
                outline.to_vec()
            } else {
                world_gpl::with_primary_palette(body, outline)?
            };
            let tpl_owner = if owns_palette { outline } else { body };
            let start = dol::u32(tpl_owner, 0)? as usize;
            let end = dol::u32(tpl_owner, 4)? as usize;
            let tpl = tpl_owner
                .get(start..end)
                .context("summon TPL exceeds model")?;
            let mut paths = if owns_palette {
                Vec::new()
            } else {
                primary.textures.clone()
            };
            let mut alpha = Vec::new();
            for index in 0..dol::u32(tpl, 4)? as usize {
                let (image, transparent) = world_texture::tpl_texture(tpl, index)?;
                if owns_palette {
                    let path = if let Some((path, _)) =
                        files.iter().find(|(_, previous)| previous == &image)
                    {
                        path.clone()
                    } else {
                        let path =
                            format!("{owner}/models/model-{slot}-layer-1-texture-{index}.ktx2");
                        files.push((path.clone(), image));
                        path
                    };
                    paths.push(path);
                }
                alpha.push(transparent);
            }
            let (outline_mesh, mut secondary) = world_gpl::static_model(&source, paths, alpha)?;
            secondary.mesh = format!("{owner}/models/model-{slot}-layer-1.glb");
            secondary.resource = 1;
            secondary.outline_color = Some([0, 0, 0, 127]);
            secondary.secondary_motion = dynamics;
            let outline_bindings = (id == 285 && slot == 0)
                .then(|| world_gpl::animation_bindings(&source))
                .transpose()?;
            secondary.clips = primary
                .clips
                .iter()
                .enumerate()
                .map(|(index, clip)| -> Result<SceneClip> {
                    let mut clip = clip.clone();
                    if id == 285 && slot == 0 && matches!(index, 0 | 2) {
                        let motion = member(package, 0x5c + index * 4)?
                            .context("Undine outline motion is absent")?;
                        let path = format!("{stem}-animation-{index}-variant-2.glb");
                        files.push((
                            path.clone(),
                            world_motion::encode_bound(
                                motion,
                                &outline_mesh,
                                outline_bindings
                                    .as_ref()
                                    .context("Undine outline rig is absent")?,
                            )
                            .with_context(|| format!("Undine outline motion {index}"))?,
                        ));
                        clip.motion = path;
                        return Ok(clip);
                    }
                    clip.motion = match (id, slot, index) {
                        (285, 0, 1) => "meshes/figurines/undine-0076/animations/idle.glb",
                        (286, 0, 0) => {
                            "meshes/monsters/sephie-0205/animations/part-1-motion-32.glb"
                        }
                        (289, 0, 0) => "meshes/figurines/volt-0082/animations/idle.glb",
                        (289, 0, 1) => "meshes/monsters/volt-0198/animations/part-1-motion-31.glb",
                        (292, 0, 0) => {
                            "meshes/monsters/maxwell-0203/animations/part-1-motion-0.glb"
                        }
                        (292, 0, 1) => {
                            "meshes/monsters/maxwell-0203/animations/part-1-motion-34.glb"
                        }
                        (293, 0, 0) => "meshes/monsters/origin-0204/animations/part-1-motion-0.glb",
                        (293, 0, 1) => {
                            "meshes/monsters/origin-0204/animations/part-1-motion-12.glb"
                        }
                        _ => &clip.motion,
                    }
                    .to_owned();
                    Ok(clip)
                })
                .collect::<Result<Vec<_>>>()?;
            for material in &mut secondary.materials {
                material.cull = CullFace::Front;
                material.blend = true;
                material.draw_order += 65536;
            }
            files.push((secondary.mesh.clone(), outline_mesh));
            document.layers.push(PreviewPart {
                scene: secondary,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: Vec::new(),
            });
        }
        artwork.insert(slot, serde_json::to_value(document)?);
    }
    Ok((artwork, files))
}

/// Nurse stores four references to one rig, outline and performance. Publish
/// those media once; the four effect slots still bind to the same editable model.
fn nurse_models(
    package: &[u8],
    owner: &str,
) -> Result<(BTreeMap<u8, serde_json::Value>, Vec<(String, Vec<u8>)>)> {
    let body = member(package, 0xc)?.context("Nurse body is absent")?;
    let outline = member(package, 0x34)?.context("Nurse outline is absent")?;
    let motion = member(package, 0x5c)?.context("Nurse motion is absent")?;
    for slot in 1..4 {
        ensure!(
            member(package, 0xc + slot * 4)? == Some(body)
                && member(package, 0x34 + slot * 4)? == Some(outline)
                && member(package, 0x5c + slot * 16)? == Some(motion),
            "Nurse model slots have distinct media"
        );
    }
    ensure!(dol::u32(motion, 0)? == 0x007b_7960, "invalid Nurse motion");
    let start = dol::u32(body, 0)? as usize;
    let end = dol::u32(body, 4)? as usize;
    let tpl = body.get(start..end).context("Nurse TPL exceeds body")?;
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "Nurse body lacks TPL");
    let stem = format!("{owner}/models/model-0-layer-0");
    let mut files = Vec::new();
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    for index in 0..dol::u32(tpl, 4)? as usize {
        let (image, transparent) = world_texture::tpl_texture(tpl, index)?;
        let path = format!("{stem}-texture-{index}.ktx2");
        files.push((path.clone(), image));
        paths.push(path);
        alpha.push(transparent);
    }
    let (mesh, mut primary) = world_gpl::static_model(body, paths.clone(), alpha.clone())?;
    primary.mesh = format!("{stem}.glb");
    for material in &mut primary.materials {
        if material.color.is_none() && material.multiply.is_none() {
            material.blend = false;
        }
    }
    primary.prepare_bones(&mesh)?;
    let bindings = world_gpl::animation_bindings(body)?;
    let source_names = bindings
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    let json_len =
        u32::from_le_bytes(mesh.get(12..16).context("short Nurse GLB")?.try_into()?) as usize;
    let gltf: serde_json::Value = serde_json::from_slice(
        mesh.get(20..20 + json_len)
            .context("short Nurse GLB JSON")?,
    )?;
    let dynamics = character_models::character_dynamics::bind(
        "nurse",
        &gltf,
        &source_names,
        &primary.bone_names,
    )?;
    primary.secondary_motion = dynamics.clone();
    let animation = world_motion::encode_bound(motion, &mesh, &bindings)?;
    let motion_path = format!("{stem}-animation-0.glb");
    let clip = SceneClip {
        motion: motion_path.clone(),
        joints: BTreeMap::new(),
        resource_slot: 0,
        duration_seconds: 0.,
        animation_resource: None,
        pose_chains: character_models::pose_chains(
            motion,
            &bindings,
            &primary.bone_names,
            &dynamics,
        )?,
    };
    primary.clips.push(clip.clone());
    files.push((primary.mesh.clone(), mesh));
    files.push((motion_path, animation));

    let normalized = world_gpl::with_primary_palette(body, outline)?;
    let (outline_mesh, mut secondary) = world_gpl::static_model(&normalized, paths, alpha)?;
    secondary.mesh = format!("{owner}/models/model-0-layer-1.glb");
    secondary.resource = 1;
    secondary.outline_color = Some([0, 0, 0, 127]);
    secondary.secondary_motion = dynamics;
    secondary.clips.push(clip);
    for material in &mut secondary.materials {
        material.cull = CullFace::Front;
        material.blend = true;
        material.draw_order += 65536;
    }
    files.push((secondary.mesh.clone(), outline_mesh));
    let document = resonance_content::battle_model::document::Model {
        attachment_nodes: BTreeMap::new(),
        independent_layers: BTreeSet::new(),
        layers: [primary, secondary]
            .into_iter()
            .map(|scene| PreviewPart {
                scene,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: Vec::new(),
            })
            .collect(),
    };
    let value = serde_json::to_value(document)?;
    Ok(((0..4).map(|slot| (slot, value.clone())).collect(), files))
}

/// Read scene rules for supported native member layouts.
/// Other owners must publish the referenced effect bank and atlas before the
/// scene is playable.
pub(crate) fn read_simple(root: &Path, id: u16) -> Result<(Scene, Vec<u8>)> {
    let name = BattleSceneId::try_from(id)?
        .resource_name()
        .context("stored battle scene has no name")?;
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let bytes = package(root, &module, id)?;
    ensure!(
        member(&bytes, 4)?.is_some() && member(&bytes, 8)?.is_some(),
        "stored scene lacks effect bank or atlas"
    );
    let model_slots = (0..10)
        .map(|slot| member(&bytes, 12 + slot * 4))
        .collect::<Result<Vec<_>>>()?;
    let model_count = model_slots
        .iter()
        .take_while(|entry| entry.is_some())
        .count();
    ensure!(
        model_slots[model_count..].iter().all(Option::is_none),
        "scene {id} has sparse model slots"
    );
    let has_projectiles = member(&bytes, 0xfc)?.is_some();
    for field in (12..0x100).step_by(4).chain((0x104..0x114).step_by(4)) {
        if has_projectiles && field == 0xfc
            || matches!(id, 280 | 296 | 301 | 312 | 314) && field == 0x104
            || (12..12 + model_count * 4).contains(&field)
            || id == 237
                && ((0x34..=0x40).contains(&field) || matches!(field, 0x5c | 0x6c | 0x7c | 0x8c))
            || id == 282 && matches!(field, 0x34 | 0x5c | 0x60 | 0x64)
            || id == 291 && matches!(field, 0x34 | 0x5c | 0x60)
            || matches!(id, 284 | 285 | 287 | 288) && matches!(field, 0x34 | 0x5c | 0x60 | 0x64)
            || id == 290 && matches!(field, 0x34 | 0x5c | 0x60)
            || matches!(id, 292 | 293) && matches!(field, 0x34 | 0x5c | 0x60)
            || id == 286 && matches!(field, 0x34 | 0x38 | 0x3c | 0x5c | 0x6c | 0x7c)
            || id == 289 && matches!(field, 0x34 | 0x5c | 0x60 | 0x6c)
        {
            continue;
        }
        ensure!(
            member(&bytes, field)?.is_none(),
            "scene {id} has additional native member {field:#x}"
        );
    }
    let parameters = member(&bytes, 0x100)?.context("scene lacks parameter block")?;
    let duration = dol::u16(parameters, 16)?;
    let (stage_color, camera) = match id {
        221 => native_camera(&module, 0x5948, 0x594c)?,
        201 => fixed_camera([16, 16, 24], 2950.0, 8.0),
        202 => fixed_camera([0, 0, 0], 2700.0, 13.0),
        217 => fixed_camera([16, 16, 24], 2700.0, 13.0),
        215 => fixed_camera([24, 20, 20], 2450.0, 12.0),
        218 => fixed_camera([8, 8, 16], 3450.0, 18.0),
        223 => fixed_camera([32, 24, 24], 2850.0, 14.5),
        228 => fixed_camera([32, 32, 32], 2800.0, 15.5),
        230 => fixed_camera([16, 16, 24], 2700.0, 14.0),
        232 => fixed_camera([12, 12, 12], 2950.0, 18.0),
        231 => fixed_camera([24, 16, 24], 2700.0, 13.0),
        260 => fixed_camera([0, 0, 0], 2450.0, 16.0),
        206 => fixed_camera([20, 16, 16], 2900.0, 15.5),
        253 => fixed_camera([12, 12, 12], 2700.0, 15.5),
        261 => fixed_camera([16, 16, 16], 2450.0, 8.0),
        283 => fixed_camera([48, 48, 48], 2700.0, 15.5),
        203 => native_camera(&module, 0x5a20, 0x5a24)?,
        205 => native_camera(&module, 0x5140, 0x5144)?,
        213 => native_camera(&module, 0x4730, 0x4734)?,
        214 => fixed_camera([16, 12, 12], 2600.0, 18.0),
        219 => fixed_camera([16, 16, 24], 2650.0, 18.0),
        222 => fixed_camera([16, 16, 32], 2700.0, 8.0),
        226 => fixed_camera([24, 16, 16], 2400.0, 8.0),
        229 => fixed_camera([16, 16, 16], 2950.0, 14.0),
        263 => fixed_camera([0, 0, 0], 2450.0, 8.0),
        251 => native_camera(&module, 0x6990, 0x6998)?,
        265 => native_camera(&module, 0x54f0, 0x54f4)?,
        266 => (None, None),
        268 => fixed_camera([0, 0, 0], 2700.0, 8.0),
        227 => fixed_camera([16, 16, 16], 2950.0, 18.0),
        207 => fixed_camera([32, 20, 20], 2650.0, 13.0),
        209 => fixed_camera([16, 24, 24], 2700.0, 13.0),
        211 => fixed_camera([20, 24, 24], 2400.0, 8.0),
        224 => fixed_camera([16, 16, 16], 2450.0, 14.5),
        255 => fixed_camera([12, 12, 16], 3100.0, 15.5),
        210 => fixed_camera([16, 20, 16], 3400.0, 15.5),
        233 => fixed_camera([16, 16, 16], 4200.0, 15.5),
        252 => native_camera(&module, 0x7da8, 0x7e20)?,
        256 => fixed_camera([12, 16, 12], 3100.0, 15.5),
        267 => fixed_camera([0, 0, 0], 2700.0, 8.0),
        269 => fixed_camera([0, 0, 0], 3450.0, 12.0),
        270 => fixed_camera([0, 0, 0], 3400.0, 11.5),
        278 => fixed_camera([48, 48, 48], 2250.0, 8.0),
        280 => fixed_camera([0, 0, 0], 2750.0, 16.0),
        282 => fixed_camera([64, 64, 64], 1950.0, 8.0),
        290 => native_camera(&module, 0x8668, 0x8750)?,
        292 => native_camera(&module, 0x9418, 0x94dc)?,
        293 => fixed_camera([16, 16, 16], 3150.0, 19.5),
        284 => fixed_camera([24, 16, 16], 3150.0, 15.5),
        285 => fixed_camera([16, 16, 24], 3350.0, 15.5),
        286 => fixed_camera([16, 24, 16], 2950.0, 15.5),
        287 => fixed_camera([20, 20, 16], 3150.0, 15.5),
        288 => fixed_camera([16, 16, 24], 3150.0, 17.0),
        289 => fixed_camera([16, 16, 20], 3200.0, 16.5),
        291 => fixed_camera([8, 8, 8], 3150.0, 15.5),
        296 => fixed_camera([0, 0, 0], 2450.0, 12.0),
        301..=318 => (None, None),
        240 => fixed_camera([16, 16, 16], 3200.0, 15.5),
        237 | 238 | 257 => fixed_camera([16, 16, 16], 1950.0, 8.0),
        241 => fixed_camera([16, 16, 16], 1950.0, 13.0),
        243 => fixed_camera([20, 20, 16], 2950.0, 13.0),
        244 => fixed_camera([20, 16, 20], 2450.0, 8.0),
        246 => fixed_camera([20, 16, 16], 2950.0, 13.0),
        247 => fixed_camera([20, 16, 16], 2450.0, 8.0),
        249 => fixed_camera([16, 16, 20], 2950.0, 13.0),
        250 => fixed_camera([20, 20, 32], 2450.0, 10.5),
        258 => fixed_camera([16, 16, 16], 3050.0, 12.0),
        259 => fixed_camera([12, 12, 20], 2700.0, 13.0),
        _ => anyhow::bail!("scene {id} needs additional scene-setting interpretation"),
    };
    let projectiles = if has_projectiles {
        stationary_projectiles(&bytes, parameters, &module, id)?
    } else {
        Vec::new()
    };
    let owner = format!("battle/scenes/{name}");
    let strike_points = if let Some((offset, count)) = match id {
        269 | 290 | 296 => Some((0x866c, 17)),
        233 | 292 => Some((0x941c, 14)),
        _ => None,
    } {
        module
            .slice(4, offset, count * 12)?
            .chunks_exact(12)
            .map(|row| {
                let mut point = [0.; 3];
                for (axis, coordinate) in point.iter_mut().enumerate() {
                    *coordinate = f32::from_bits(dol::u32(row, axis * 4)?);
                    ensure!(coordinate.is_finite(), "non-finite strike point");
                }
                Ok(point)
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let scene: Scene = Scene {
        effects: format!("{owner}/effects.json"),
        cutin: matches!(id, 280 | 296)
            .then(|| scene_cutin(&bytes, &owner).map(|(texture, _)| texture))
            .transpose()?,
        weapon: None,
        projectiles,
        actions: if matches!(id, 301..=318) {
            actions::read(parameters)?
        } else {
            Default::default()
        },
        actions_error: None,
        duration,
        stage_color,
        camera,
        strike_points,
        recovery_radius: match id {
            240 => Some(f32::from_bits(dol::u32(module.slice(4, 0x4d2c, 4)?, 0)?)),
            255 => Some(f32::from_bits(dol::u32(module.slice(4, 0x5af4, 4)?, 0)?)),
            256 => Some(f32::from_bits(dol::u32(module.slice(4, 0x5b5c, 4)?, 0)?)),
            258 => Some(f32::from_bits(dol::u32(module.slice(4, 0x555c, 4)?, 0)?)),
            _ => None,
        },
        spell_recipe: (id == 252).then(|| ray_recipe(&module)).transpose()?,
    };
    Ok((scene, bytes))
}

pub fn cook_battle_scene_parameters(root: &Path, library: &Path, id: u16) -> Result<()> {
    let name = BattleSceneId::try_from(id)?
        .resource_name()
        .context("stored battle scene has no name")?;
    let (scene, _) = read_simple(root, id)?;
    let output = library.join(format!("battle/scenes/{name}/scene.json"));
    ensure!(!output.exists(), "battle scene parameters already exist");
    crate::output::json(&output, &scene)?;
    Ok(())
}

struct AtlasEntry {
    effect: EffectTexture,
    capacity: usize,
    pages: BTreeMap<usize, usize>,
}

impl AtlasEntry {
    fn selectors(&self, stride: usize) -> Vec<Option<usize>> {
        let step = if stride == 0 { self.capacity } else { stride };
        if step == 0 {
            return vec![Some(0); 256];
        }
        (0..256)
            .map(|index| self.pages.get(&(index * step)).copied())
            .collect()
    }
}

pub(crate) fn atlas(
    tpl: &[u8],
    owner: &str,
    slot: u8,
    strides: &BTreeSet<u8>,
    requested: &BTreeSet<(u8, u8)>,
) -> Result<(Vec<EffectTexture>, BTreeMap<u8, u8>, Vec<(String, Vec<u8>)>)> {
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "scene atlas is not a TPL");
    let count = dol::u32(tpl, 4)? as usize;
    let table = dol::u32(tpl, 8)? as usize;
    let mut files = Vec::<(String, Vec<u8>)>::new();
    let mut entries = Vec::new();
    for index in 0..count {
        let descriptor = dol::u32(tpl, table + index * 8)? as usize;
        let width = u32::from(dol::u16(tpl, descriptor + 2)?);
        let height = u32::from(dol::u16(tpl, descriptor)?);
        let capacity = world_texture::tpl_palette_capacity(tpl, index)?;
        let colors = world_texture::tpl_palette_entries(tpl, index)?;
        let pages = if capacity == 0 {
            1
        } else {
            colors.div_ceil(capacity).max(1)
        };
        let mut images = Vec::<UiTexture>::new();
        let mut offsets = BTreeMap::new();
        for page in 0..pages {
            let offset = page * capacity;
            let (pixels, _) = world_texture::tpl_texture_palette(tpl, index, offset)?;
            let path =
                if let Some((path, _)) = files.iter().find(|(_, previous)| previous == &pixels) {
                    path.clone()
                } else {
                    let path = format!("{owner}/textures/atlas-{slot}-{index}-palette-{page}.ktx2");
                    files.push((path.clone(), pixels));
                    path
                };
            let image = images
                .iter()
                .position(|entry| entry.path == path)
                .unwrap_or_else(|| {
                    images.push(UiTexture {
                        path,
                        width,
                        height,
                    });
                    images.len() - 1
                });
            offsets.insert(offset, image);
        }
        // The default selector table describes the native full-width pages.
        // Actor-specific tail windows added below must not become defaults.
        let default = if capacity == 0 {
            vec![Some(0); 256]
        } else {
            (0..256)
                .map(|selector| offsets.get(&(selector * capacity)).copied())
                .collect()
        };
        if capacity != 0 {
            let window = colors.min(capacity);
            for &stride in strides {
                if stride == 0 {
                    continue;
                }
                let stride = usize::from(stride);
                for selector in 0..=u8::MAX {
                    let offset = usize::from(selector) * stride;
                    if offset + window > colors {
                        break;
                    }
                    if offsets.contains_key(&offset) {
                        continue;
                    }
                    let (pixels, _) = world_texture::tpl_texture_palette(tpl, index, offset)?;
                    let path = if let Some((path, _)) =
                        files.iter().find(|(_, previous)| previous == &pixels)
                    {
                        path.clone()
                    } else {
                        let path = format!(
                            "{owner}/textures/atlas-{slot}-{index}-palette-{}.ktx2",
                            images.len()
                        );
                        files.push((path.clone(), pixels));
                        path
                    };
                    let image = images
                        .iter()
                        .position(|entry| entry.path == path)
                        .unwrap_or_else(|| {
                            images.push(UiTexture {
                                path,
                                width,
                                height,
                            });
                            images.len() - 1
                        });
                    offsets.insert(offset, image);
                }
            }
        }
        for &(stride, selector) in requested {
            if stride == 0 {
                continue;
            }
            let offset = usize::from(stride) * usize::from(selector);
            if offsets.contains_key(&offset) {
                continue;
            }
            let (pixels, _) = world_texture::tpl_texture_palette_explicit(tpl, index, offset)?;
            let path =
                if let Some((path, _)) = files.iter().find(|(_, previous)| previous == &pixels) {
                    path.clone()
                } else {
                    let path = format!(
                        "{owner}/textures/atlas-{slot}-{index}-palette-{}.ktx2",
                        images.len()
                    );
                    files.push((path.clone(), pixels));
                    path
                };
            let image = images
                .iter()
                .position(|entry| entry.path == path)
                .unwrap_or_else(|| {
                    images.push(UiTexture {
                        path,
                        width,
                        height,
                    });
                    images.len() - 1
                });
            offsets.insert(offset, image);
        }
        let entry = AtlasEntry {
            effect: EffectTexture {
                images,
                sampler: world_texture::sampler(tpl, descriptor)?,
                palette_sets: vec![default],
            },
            capacity,
            pages: offsets,
        };
        entries.push(entry);
    }
    ensure!(!entries.is_empty(), "scene atlas has no textures");
    let mut sets = BTreeMap::new();
    for &stride in strides {
        let maps = entries
            .iter()
            .map(|entry| entry.selectors(usize::from(stride)))
            .collect::<Vec<_>>();
        let existing = (0..entries[0].effect.palette_sets.len()).find(|&set| {
            entries
                .iter()
                .zip(&maps)
                .all(|(entry, map)| entry.effect.palette_sets[set] == *map)
        });
        let set = existing.unwrap_or(entries[0].effect.palette_sets.len());
        if existing.is_none() {
            for (entry, map) in entries.iter_mut().zip(maps) {
                entry.effect.palette_sets.push(map);
            }
        }
        sets.insert(stride, u8::try_from(set)?);
    }
    Ok((
        entries.into_iter().map(|entry| entry.effect).collect(),
        sets,
        files,
    ))
}

/// Convert a complete supported spell package: scene rules, effect programs,
/// indexed atlas images, projectiles and static models as present.
pub fn cook_battle_simple_scene(root: &Path, library: &Path, id: u16) -> Result<()> {
    let name = BattleSceneId::try_from(id)?
        .resource_name()
        .context("stored battle scene has no name")?;
    let owner = format!("battle/scenes/{name}");
    let (scene, package) = read_simple(root, id)?;
    let weapon = if matches!(id, 301 | 312 | 314) {
        let source = member(&package, 0x104)?.context("combination weapon is absent")?;
        Some(battle_equipment::convert_package(
            source,
            &format!("{owner}/weapon"),
            None,
            None,
        )?)
    } else {
        None
    };
    let scene = scene.with_weapon(weapon.as_ref().map(|(weapon, _)| weapon.clone()));
    let effects = member(&package, 4)?.context("scene effect bank is absent")?;
    let native = battle_effect_recipes::NativeBank::read(effects)?;
    let mut programs = (0..native.program_count())
        .map(|index| native.simple_program(index))
        .collect::<Result<Vec<_>>>()?;
    let mut used = BTreeMap::<u8, BTreeSet<u8>>::new();
    for event in programs.iter().flatten() {
        if let EffectOperation::Spawn {
            particle,
            palette,
            birth,
            ..
        } = &event.operation
        {
            let selectors = used.entry(*particle).or_default();
            selectors.extend(palette);
            for range in [birth.palette, birth.alpha_palette].into_iter().flatten() {
                selectors.extend(
                    range.min.round().clamp(0.0, 255.0) as u8
                        ..=range.max.round().clamp(0.0, 255.0) as u8,
                );
            }
        }
    }
    let mut strides = BTreeSet::new();
    let mut requested = BTreeSet::new();
    let mut resident_slots = BTreeSet::new();
    for (actor, mut selectors) in used {
        let declaration = native.simple_particle(actor)?;
        let Declaration::Particle { visual, .. } = &declaration else {
            if matches!(declaration, Declaration::ModelParticle { .. }) {
                continue;
            }
            anyhow::bail!("scene {id} needs a specialized effect declaration");
        };
        match visual.texture {
            ParticleTexture::Atlas { slot: 6, .. } => {
                let stride = native.actor(actor)?[6];
                strides.insert(stride);
                if let Declaration::Particle { template, .. } = &declaration {
                    selectors.extend(template.state.palettes);
                    if let Some(UvAnimation::Frames { frames, .. }) = &template.uv_animation {
                        for frame in frames {
                            if let UvChange::Palette { index } = frame.change {
                                selectors.insert(index);
                            }
                        }
                    }
                }
                requested.extend(selectors.into_iter().map(|selector| (stride, selector)));
            }
            ParticleTexture::Untextured => {}
            ParticleTexture::Atlas { slot, .. } if slot <= 1 => {
                resident_slots.insert((slot, native.actor(actor)?[6]));
            }
            ref texture => anyhow::bail!("scene {id} uses a non-scene effect atlas {texture:?}"),
        }
    }
    let resident_sets = if resident_slots.is_empty() {
        BTreeMap::new()
    } else {
        battle_effect_recipes::resident_palette_sets(root)?
    };
    for binding in resident_slots {
        ensure!(
            resident_sets.contains_key(&binding),
            "resident scene palette binding {binding:?} is absent"
        );
    }
    let tpl = member(&package, 8)?.context("scene effect atlas is absent")?;
    let (textures, sets, images) = atlas(tpl, &owner, 6, &strides, &requested)?;
    let (models, model_files) = static_scene_models(&package, &owner, id)?;
    let (template_names, declarations) =
        battle_effect_recipes::name_programs(&native, &mut programs, &[], |actor, declaration| {
            if let Declaration::ModelParticle { binding, .. } = declaration {
                ensure!(
                    models.contains_key(&binding.slot),
                    "scene model binding is absent"
                );
            }
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas {
                    slot, palette_set, ..
                } = &mut visual.texture
            {
                let stride = native.actor(actor)?[6];
                *palette_set = match *slot {
                    6 => *sets
                        .get(&stride)
                        .context("scene effect palette binding is absent")?,
                    slot => *resident_sets
                        .get(&(slot, stride))
                        .context("resident scene palette binding is absent")?,
                };
            }
            Ok(())
        })?;
    let definition = Definition {
        programs,
        template_names,
        templates: declarations,
        artwork: Some(Artwork {
            textures: BTreeMap::from([(6, textures)]),
            models,
        }),
    };
    let mut files = vec![
        (
            format!("{owner}/scene.json"),
            resonance_asset_writer::json::encode(&scene)?,
        ),
        (
            format!("{owner}/effects.json"),
            resonance_asset_writer::json::encode(&definition)?,
        ),
    ];
    if matches!(id, 280 | 296) {
        let (_, image) = scene_cutin(&package, &owner)?;
        files.push((format!("{owner}/cutin/palette-0.ktx2"), image));
    }
    files.extend(images);
    files.extend(model_files);
    if let Some((_, media)) = weapon {
        files.extend(media);
    }
    for (path, _) in &files {
        ensure!(
            !library.join(path).exists(),
            "scene output already exists: {path}"
        );
    }
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
