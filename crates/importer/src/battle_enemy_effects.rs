//! Enemy-local effect recipes and artwork, directly from one native package.
use crate::{
    battle_effect_recipes::{self, NativeBank},
    battle_enemy_media, battle_scene_package, dol,
    enemy_package::EnemyPackages,
    world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace, SceneClip,
    battle_effect::{
        EffectOperation, UvAnimation, UvChange,
        declaration::Declaration,
        document::{Artwork, Definition},
        visual::ParticleTexture,
    },
    battle_model::document::Model,
    model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_constants::MonsterId;

fn model_palette(model: &[u8]) -> Result<&[u8]> {
    let start = dol::u32(model, 0)? as usize;
    let end = dol::u32(model, 4)? as usize;
    let tpl = model
        .get(start..end)
        .context("effect model palette exceeds source")?;
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "effect model lacks TPL");
    Ok(tpl)
}

fn add_file(files: &mut BTreeMap<String, Vec<u8>>, path: String, bytes: Vec<u8>) -> Result<String> {
    if let Some((old, _)) = files.iter().find(|(_, old)| *old == &bytes) {
        return Ok(old.clone());
    }
    ensure!(
        files.insert(path.clone(), bytes).is_none(),
        "duplicate enemy effect media"
    );
    Ok(path)
}

fn model_artwork(
    package: &[u8],
    owner: &str,
    used: &BTreeSet<u8>,
    attached_motions: &BTreeMap<u8, BTreeSet<u8>>,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<u8, Model>> {
    let profile = usize::from(dol::u16(package, 4)?);
    let count = usize::from(
        *package
            .get(profile + 0x1e8)
            .context("enemy effect model count is absent")?,
    );
    ensure!(count <= 6, "enemy effect model count exceeds pointer table");
    let mut models = BTreeMap::new();
    for &slot in used {
        // Some native programs select a model slot without storing artwork for
        // it. Keep the editable binding visible; never fabricate media.
        if usize::from(slot) >= count {
            continue;
        }
        let field = 0x180 + usize::from(slot) * 4;
        let Some(body) = battle_enemy_media::member(package, field)? else {
            continue;
        };
        let tpl = model_palette(body)?;
        let stem = format!("{owner}/models/model-{slot}-layer-0");
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for index in 0..dol::u32(tpl, 4)? as usize {
            let (image, transparent) = world_texture::tpl_texture(tpl, index)?;
            paths.push(add_file(
                files,
                format!("{stem}-texture-{index}.ktx2"),
                image,
            )?);
            alpha.push(transparent);
        }
        let (glb, mut scene) = world_gpl::static_model(body, paths.clone(), alpha.clone())?;
        scene.mesh = add_file(files, format!("{stem}.glb"), glb.clone())?;
        for material in &mut scene.materials {
            if material.color.is_none() && material.multiply.is_none() {
                material.blend = false;
            }
        }
        let motions = std::iter::once((0, 0x1b0 + usize::from(slot) * 4)).chain(
            attached_motions
                .get(&slot)
                .into_iter()
                .flatten()
                .map(|&motion| (motion, 0x20 + usize::from(motion) * 4)),
        );
        for (motion_slot, field) in motions {
            if let Some(motion) = battle_enemy_media::member(package, field)? {
                ensure!(
                    dol::u32(motion, 0)? == 0x007b_7960,
                    "invalid enemy effect motion"
                );
                let bindings = world_gpl::animation_bindings(body)?;
                let animation = world_motion::encode_bound(motion, &glb, &bindings)?;
                let path = add_file(
                    files,
                    format!("{stem}-animation-{motion_slot}.glb"),
                    animation,
                )?;
                scene.clips.push(SceneClip {
                    motion: path,
                    joints: BTreeMap::new(),
                    resource_slot: motion_slot.into(),
                    duration_seconds: 0.,
                    animation_resource: None,
                    pose_chains: Vec::new(),
                });
            }
        }
        let mut layers = vec![PreviewPart {
            scene: scene.clone(),
            animation: None,
            attached_to: None,
            additive: false,
            uv_offsets: Vec::new(),
        }];
        if let Some(outline) = battle_enemy_media::member(package, 0x198 + usize::from(slot) * 4)? {
            let source = if dol::u32(outline, 0)? == 0 {
                world_gpl::with_primary_palette(body, outline)?
            } else {
                outline.to_vec()
            };
            let own_palette = dol::u32(outline, 0)? != 0;
            let (outline_paths, outline_alpha) = if own_palette {
                let tpl = model_palette(&source)?;
                let mut paths = Vec::new();
                let mut alpha = Vec::new();
                for index in 0..dol::u32(tpl, 4)? as usize {
                    let (image, transparent) = world_texture::tpl_texture(tpl, index)?;
                    paths.push(add_file(
                        files,
                        format!("{owner}/models/model-{slot}-layer-1-texture-{index}.ktx2"),
                        image,
                    )?);
                    alpha.push(transparent);
                }
                (paths, alpha)
            } else {
                (paths, alpha)
            };
            let (outline_glb, mut secondary) =
                world_gpl::static_model(&source, outline_paths, outline_alpha)?;
            secondary.mesh = add_file(
                files,
                format!("{owner}/models/model-{slot}-layer-1.glb"),
                outline_glb,
            )?;
            secondary.resource = 1;
            secondary.clips = scene.clips.clone();
            secondary.outline_color = Some([0, 0, 0, 127]);
            for material in &mut secondary.materials {
                material.cull = CullFace::Front;
                material.blend = true;
                material.draw_order += 65536;
            }
            layers.push(PreviewPart {
                scene: secondary,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: Vec::new(),
            });
        }
        models.insert(
            slot,
            Model {
                attachment_nodes: BTreeMap::new(),
                independent_layers: BTreeSet::new(),
                layers,
            },
        );
    }
    Ok(models)
}

fn requested_palettes(
    bank: &NativeBank<'_>,
    programs: &[Vec<resonance_content::battle_effect::ScheduledEvent>],
) -> Result<BTreeMap<u8, (BTreeSet<u8>, BTreeSet<(u8, u8)>)>> {
    let mut selectors = BTreeMap::<u8, BTreeSet<u8>>::new();
    for event in programs.iter().flatten() {
        if let EffectOperation::Spawn {
            particle,
            palette,
            birth,
            ..
        } = &event.operation
        {
            let selected = selectors.entry(*particle).or_default();
            selected.extend(palette);
            for range in [birth.palette, birth.alpha_palette].into_iter().flatten() {
                selected.extend(
                    range.min.round().clamp(0., 255.) as u8
                        ..=range.max.round().clamp(0., 255.) as u8,
                );
            }
        }
    }
    let mut requested = BTreeMap::<u8, (BTreeSet<u8>, BTreeSet<(u8, u8)>)>::new();
    for (actor, mut selected) in selectors {
        if let Declaration::Particle { visual, template } =
            bank.simple_particle(actor).with_context(|| {
                format!(
                    "enemy effect actor {actor}, kind {}, flags {:#x}, control {:?}",
                    bank.actor(actor).map(|row| row[0]).unwrap_or(255),
                    bank.actor(actor)
                        .map(|row| dol::u32(row, 20).unwrap_or(0))
                        .unwrap_or(0),
                    bank.actor(actor).map(|row| &row[140..150]).unwrap_or(&[])
                )
            })?
        {
            let ParticleTexture::Atlas { slot, .. } = visual.texture else {
                continue;
            };
            selected.extend(template.state.palettes);
            if let Some(UvAnimation::Frames { frames, .. }) = template.uv_animation {
                for frame in frames {
                    if let UvChange::Palette { index } = frame.change {
                        selected.insert(index);
                    }
                }
            }
            let stride = bank.actor(actor)?[6];
            let entry = requested.entry(slot).or_default();
            entry.0.insert(stride);
            entry
                .1
                .extend(selected.into_iter().map(|selector| (stride, selector)));
        }
    }
    Ok(requested)
}

fn convert(
    package: &[u8],
    owner: &str,
    resident_sets: &BTreeMap<(u8, u8), u8>,
) -> Result<(Definition, BTreeMap<String, Vec<u8>>)> {
    ensure!(package.starts_with(b"em8\0"), "invalid enemy package");
    let mut files = BTreeMap::new();
    let bank = battle_enemy_media::member(package, 0x1cc)?
        .map(NativeBank::read)
        .transpose()?;
    let mut programs = if let Some(bank) = &bank {
        let body =
            battle_enemy_media::member(package, 0x18)?.context("enemy effects have no body rig")?;
        let bones = world_gpl::animation_bindings(body)?
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>();
        (0..bank.program_count())
            .map(|index| {
                bank.grouped_program(index, &bones)
                    .with_context(|| format!("effect program {index}"))
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let mut textures = BTreeMap::new();
    let mut sets = BTreeMap::new();
    let mut used_models = BTreeSet::new();
    let requests = if let Some(bank) = &bank {
        let requested = requested_palettes(bank, &programs)?;
        for event in programs.iter().flatten() {
            if let EffectOperation::Spawn { particle, .. } = event.operation {
                if let Declaration::ModelParticle { binding, .. } =
                    bank.simple_particle(particle)
                        .with_context(|| format!("enemy effect actor {particle}"))?
                {
                    used_models.insert(binding.slot);
                }
            }
        }
        requested
    } else {
        BTreeMap::new()
    };
    for (slot, field) in [(2, 0x1d0), (14, 0x1d4)] {
        let selected = requests.get(&slot);
        if let Some(tpl) = battle_enemy_media::member(package, field)? {
            let empty_strides = BTreeSet::new();
            let empty_selectors = BTreeSet::new();
            let (strides, requested) = selected
                .map(|(strides, selectors)| (strides, selectors))
                .unwrap_or((&empty_strides, &empty_selectors));
            let (images, palette_sets, paths) =
                battle_scene_package::atlas(tpl, owner, slot, strides, requested)?;
            textures.insert(slot, images);
            sets.extend(
                palette_sets
                    .into_iter()
                    .map(|(stride, set)| ((slot, stride), set)),
            );
            for (path, bytes) in paths {
                files.insert(path, bytes);
            }
        } else {
            ensure!(selected.is_none(), "enemy effect atlas {slot} is absent");
        }
    }
    let profile = usize::from(dol::u16(package, 4)?);
    let count = usize::from(
        *package
            .get(profile + 0x1e8)
            .context("enemy effect model count is absent")?,
    );
    ensure!(count <= 6, "enemy effect model count exceeds pointer table");
    used_models.extend((0..count).map(|slot| slot as u8));
    let attached = usize::from(
        *package
            .get(profile + 0x1e6)
            .context("enemy attachment count is absent")?,
    );
    let mut attached_motions = BTreeMap::<u8, BTreeSet<u8>>::new();
    if attached > 0 {
        let rows = battle_enemy_media::member(package, 0x1d8)?
            .context("enemy attached model table is absent")?;
        for row in rows
            .get(..attached * 40)
            .context("truncated enemy attached models")?
            .chunks_exact(40)
        {
            used_models.insert(row[0]);
            if row[1] != 0 {
                attached_motions.entry(row[0]).or_default().insert(row[1]);
            }
        }
    }
    let models = model_artwork(package, owner, &used_models, &attached_motions, &mut files)?;
    let (template_names, templates) = if let Some(bank) = &bank {
        battle_effect_recipes::name_programs(bank, &mut programs, &[], |actor, declaration| {
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas {
                    slot, palette_set, ..
                } = &mut visual.texture
            {
                let stride = bank.actor(actor)?[6];
                *palette_set = *sets
                    .get(&(*slot, stride))
                    .or_else(|| resident_sets.get(&(*slot, stride)))
                    .with_context(|| {
                        format!("enemy effect palette binding {slot}/{stride} is absent")
                    })?;
            }
            Ok(())
        })?
    } else {
        (Vec::new(), Vec::new())
    };
    let artwork = if textures.is_empty() && models.is_empty() {
        None
    } else {
        Some(Artwork::from_models(textures, models)?)
    };
    Ok((
        Definition {
            programs,
            template_names,
            templates,
            artwork,
        },
        files,
    ))
}

pub fn cook_battle_enemy_effects(root: &Path, library: &Path, id: u8) -> Result<()> {
    cook_battle_enemy_effects_group(root, library, &[id])
}

fn redirect_paths(value: &mut serde_json::Value, paths: &BTreeMap<String, String>) {
    match value {
        serde_json::Value::String(path) => {
            if let Some(owner) = paths.get(path) {
                *path = owner.clone();
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redirect_paths(value, paths);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                redirect_paths(value, paths);
            }
        }
        _ => {}
    }
}

/// Write each completed owner directly, reusing equal media from earlier owners.
pub fn cook_battle_enemy_effects_group(root: &Path, library: &Path, ids: &[u8]) -> Result<()> {
    ensure!(!ids.is_empty(), "no enemy effects selected");
    let mut packages = EnemyPackages::open(&root.join("disc1"))?;
    let resident_sets = battle_effect_recipes::resident_palette_sets(root)?;
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ensure!(
        ids.windows(2).all(|pair| pair[0] != pair[1]),
        "enemy effect ID selected twice"
    );
    let mut equal_media = BTreeMap::<Vec<u8>, String>::new();
    let total = ids.len();
    for (index, id) in ids.into_iter().enumerate() {
        eprintln!(
            "  [{}/{} jobs] Enemy effects: {} ({id})",
            index + 1,
            total,
            MonsterId::try_from(id)?
                .resource_name()
                .unwrap_or("unnamed")
        );
        let name = MonsterId::try_from(id)?
            .resource_name()
            .context("enemy has no name")?;
        let owner = format!("battle/enemies/{name}/effects");
        let package = packages.read(id)?;
        let (mut definition, media) = convert(&package, &owner, &resident_sets)
            .with_context(|| format!("enemy {id} effects"))?;
        let mut redirects = BTreeMap::new();
        for (path, bytes) in media {
            if let Some(existing) = equal_media.get(&bytes) {
                redirects.insert(path, existing.clone());
            } else {
                crate::output::write(&library.join(&path), &bytes)?;
                equal_media.insert(bytes, path);
            }
        }
        if let Some(artwork) = &mut definition.artwork {
            for atlas in artwork.textures.values_mut() {
                for texture in atlas {
                    for image in &mut texture.images {
                        if let Some(owner) = redirects.get(&image.path) {
                            image.path = owner.clone();
                        }
                    }
                }
            }
            for model in artwork.models.values_mut() {
                redirect_paths(model, &redirects);
            }
        }
        crate::output::json(&library.join(format!("{owner}/effects.json")), &definition)?;
    }
    Ok(())
}
