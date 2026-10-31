//! The two stored battle skill packages, directly from the native archive.
//! Grand Fall has a model and projectile contacts; Falcon's Crest additionally
//! has a cut-in, a weapon and a larger effect bank.
use crate::{
    battle_effect_recipes, battle_equipment, battle_projectiles, battle_scene_package, dol,
    rel::Rel, world_gpl, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{
        EffectOperation, ScheduledEvent, UvAnimation, UvChange,
        declaration::Declaration,
        document::{Artwork, Definition},
        visual::ParticleTexture,
    },
    battle_model::document::{Model, WeaponDefinition},
    battle_projectile::DamageKind,
    battle_scene::{Scene, SceneProjectile, Texture},
    font::UiTexture,
    model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::BattleSkillId;

fn package(root: &Path, id: u16) -> Result<(Vec<u8>, Rel)> {
    // The technique table selects physical slots 1 and 2 for these two skills.
    // The REL contains the final native archive boundaries, including slot 0.
    let slot = match id {
        228 => 1,
        51 => 2,
        _ => anyhow::bail!("battle skill {id} has no stored package"),
    };
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let directory = module.slice(5, 0x11c4, 16)?;
    let start = dol::u32(directory, slot * 4)? as usize;
    let archive = fs::read(root.join("disc1/files/BTL/BTLskill.dat"))?;
    let end = dol::u32(directory, (slot + 1) * 4)? as usize;
    ensure!(
        start < end && end <= archive.len(),
        "skill package exceeds archive"
    );
    Ok((archive[start..end].to_vec(), module))
}

fn models(
    package: &[u8],
    owner: &str,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<BTreeMap<u8, Model>> {
    let mut models = BTreeMap::new();
    for slot in 0..10 {
        let field = 12 + slot * 4;
        let Some(source) = battle_scene_package::member(package, field)? else {
            continue;
        };
        // Motion and outline members need a separate rigged-model converter.
        for extra in [
            52 + slot * 4,
            92 + slot * 16,
            96 + slot * 16,
            100 + slot * 16,
            104 + slot * 16,
        ] {
            ensure!(
                battle_scene_package::member(package, extra)?.is_none(),
                "skill model {slot} has motion or outline artwork"
            );
        }
        let start = dol::u32(source, 0)? as usize;
        let end = dol::u32(source, 4)? as usize;
        let tpl = source
            .get(start..end)
            .context("skill model palette exceeds model")?;
        let stem = format!("{owner}/models/model-{slot}-layer-0");
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for index in 0..dol::u32(tpl, 4)? as usize {
            let (image, transparent) = world_texture::tpl_texture(tpl, index)?;
            let path = format!("{stem}-texture-{index}.ktx2");
            files.push((path.clone(), image));
            paths.push(path);
            alpha.push(transparent);
        }
        let (glb, mut scene) = world_gpl::static_model(source, paths, alpha)?;
        let path = format!("{stem}.glb");
        scene.mesh = path.clone();
        files.push((path, glb));
        models.insert(
            slot as u8,
            Model {
                attachment_nodes: Default::default(),
                independent_layers: Default::default(),
                layers: vec![PreviewPart {
                    scene,
                    animation: None,
                    attached_to: None,
                    additive: false,
                    uv_offsets: Vec::new(),
                }],
            },
        );
    }
    Ok(models)
}

fn projectiles(package: &[u8], module: &Rel) -> Result<Vec<SceneProjectile>> {
    let Some(rows) = battle_scene_package::member(package, 0xfc)? else {
        return Ok(Vec::new());
    };
    let length = rows.len() / 400 * 400;
    let start = dol::u32(package, 0xfc)? as usize;
    ensure!(
        length != 0
            && (length == rows.len()
                || start + rows.len() == (start + length).next_multiple_of(32)),
        "misaligned skill projectiles"
    );
    let parameters = battle_scene_package::member(package, 0x100)?
        .context("skill projectiles lack contact parameters")?;
    let start = dol::u32(parameters, 0)? as usize;
    let end = dol::u32(parameters, 4)? as usize;
    let rules = parameters
        .get(start..end)
        .context("skill contact rules exceed parameters")?;
    ensure!(
        rules.len().is_multiple_of(28),
        "misaligned skill contact rules"
    );
    let reset_scale = f32::from_bits(dol::u32(module.slice(4, 0xce8, 4)?, 0)?);
    ensure!(reset_scale.is_finite(), "invalid projectile reset scale");
    rows[..length]
        .chunks_exact(400)
        .map(|row| -> Result<_> {
            let kind = match row[17] {
                0 => DamageKind::Slash,
                1 => DamageKind::Thrust,
                2 => DamageKind::Magic,
                value => anyhow::bail!("unknown skill projectile damage kind {value}"),
            };
            let mut projectile = battle_projectiles::scene_record(row, reset_scale)?;
            // Native response state can call into unprepared per-instance state.
            if projectile.motion.response.is_some() && row[0x14] != 0 {
                projectile.unsupported_reason =
                    Some("projectile response requires unsupported instance state".into());
            }
            let mut hit_errors = BTreeMap::new();
            let hits = rules
                .chunks_exact(28)
                .enumerate()
                .map(
                    |(index, rule)| match battle_projectiles::scene_hit(rule, row[19]) {
                        Ok(mut hit) => {
                            hit.kind = kind;
                            Some(hit)
                        }
                        Err(error) => {
                            hit_errors.insert(index, format!("{error:#}"));
                            None
                        }
                    },
                )
                .collect::<Vec<_>>();
            let selected = usize::from(row[18]);
            let hit = hits.get(selected).copied().flatten();
            let unsupported_reason = hit.is_none().then(|| {
                format!(
                    "contact rule {selected}: {}",
                    hit_errors
                        .get(&selected)
                        .map(String::as_str)
                        .unwrap_or("missing contact rule")
                )
            });
            Ok(SceneProjectile {
                projectile,
                hit,
                hits,
                hit_errors,
                unsupported_reason,
            })
        })
        .collect()
}

fn effects(
    package: &[u8],
    owner: &str,
    models: BTreeMap<u8, Model>,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<Definition> {
    let source =
        battle_scene_package::member(package, 4)?.context("skill effect bank is absent")?;
    let native = battle_effect_recipes::NativeBank::read(source)?;
    let mut programs = (0..native.program_count())
        .map(|index| {
            native.simple_program(index).or_else(|error| {
                // The authored file keeps a visible unsupported operation;
                // a failed native effect is never silently discarded.
                Ok(vec![ScheduledEvent {
                    at: 0,
                    operation: EffectOperation::Unsupported {
                        reason: format!("effect program {index}: {error:#}"),
                    },
                }])
            })
        })
        .collect::<Result<Vec<_>>>()?;
    for (index, program) in programs.iter_mut().enumerate() {
        let unsupported = program.iter().find_map(|event| {
            let EffectOperation::Spawn { particle, .. } = &event.operation else {
                return None;
            };
            match native.simple_particle(*particle) {
                Ok(Declaration::Particle { .. } | Declaration::ModelParticle { .. }) => None,
                Ok(_) => Some(format!(
                    "effect declaration {particle}: unsupported declaration"
                )),
                Err(error) => Some(format!("effect declaration {particle}: {error:#}")),
            }
        });
        if let Some(reason) = unsupported {
            *program = vec![ScheduledEvent {
                at: 0,
                operation: EffectOperation::Unsupported {
                    reason: format!("effect program {index}: {reason}"),
                },
            }];
        }
    }
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
                    range.min.round().clamp(0., 255.) as u8
                        ..=range.max.round().clamp(0., 255.) as u8,
                );
            }
        }
    }
    let mut strides = BTreeSet::new();
    let mut requested = BTreeSet::new();
    for (actor, mut selectors) in used {
        let declaration = native.simple_particle(actor)?;
        let Declaration::Particle {
            visual, template, ..
        } = &declaration
        else {
            // Model particles reference the scene's separately converted GPL.
            ensure!(
                matches!(declaration, Declaration::ModelParticle { .. }),
                "skill effect needs a specialized declaration"
            );
            continue;
        };
        match visual.texture {
            ParticleTexture::Atlas { slot: 6, .. } => {
                let stride = native.actor(actor)?[6];
                strides.insert(stride);
                selectors.extend(template.state.palettes);
                if let Some(UvAnimation::Frames { frames, .. }) = &template.uv_animation {
                    for frame in frames {
                        if let UvChange::Palette { index } = frame.change {
                            selectors.insert(index);
                        }
                    }
                }
                requested.extend(selectors.into_iter().map(|selector| (stride, selector)));
            }
            ParticleTexture::Untextured => {}
            _ => anyhow::bail!("skill effect uses a non-scene texture atlas"),
        }
    }
    let tpl = battle_scene_package::member(package, 8)?.context("skill atlas is absent")?;
    let (textures, sets, images) =
        battle_scene_package::atlas(tpl, owner, 6, &strides, &requested)?;
    let (template_names, declarations) =
        battle_effect_recipes::name_programs(&native, &mut programs, &[], |actor, declaration| {
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
            {
                *palette_set = *sets
                    .get(&native.actor(actor)?[6])
                    .context("skill effect palette binding is absent")?;
            }
            Ok(())
        })?;
    files.extend(images);
    Ok(Definition {
        programs,
        template_names,
        templates: declarations,
        artwork: Some(Artwork::from_models(
            BTreeMap::from([(6, textures)]),
            models,
        )?),
    })
}

fn cutin(
    package: &[u8],
    owner: &str,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<Option<Texture>> {
    let Some(tpl) = battle_scene_package::member(package, 0x104)? else {
        return Ok(None);
    };
    ensure!(
        dol::u32(tpl, 0)? == 0x0020_af30 && dol::u32(tpl, 4)? == 1,
        "unsupported skill cut-in"
    );
    let descriptor = dol::u32(tpl, dol::u32(tpl, 8)? as usize)? as usize;
    let (image, _) = world_texture::tpl_texture(tpl, 0)?;
    let path = format!("{owner}/cutin/palette-0.ktx2");
    files.push((path.clone(), image));
    Ok(Some(Texture {
        images: vec![UiTexture {
            path,
            width: u32::from(dol::u16(tpl, descriptor + 2)?),
            height: u32::from(dol::u16(tpl, descriptor)?),
        }],
        sampler: world_texture::sampler(tpl, descriptor)?,
    }))
}

fn weapon(
    package: &[u8],
    owner: &str,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<Option<WeaponDefinition>> {
    let Some(source) = battle_scene_package::member(package, 0x108)? else {
        return Ok(None);
    };
    let (weapon, media) =
        battle_equipment::convert_package(source, &format!("{owner}/weapon"), None, None)?;
    files.extend(media);
    Ok(Some(weapon))
}

/// Cook one selected native stored skill package to its editable scene and media.
pub fn cook_battle_skill_package(root: &Path, library: &Path, id: u16) -> Result<()> {
    let name = BattleSkillId::try_from(id)?
        .resource_name()
        .context("battle skill has no readable name")?;
    let owner = format!("battle/skills/{name}");
    let (package, module) = package(root, id)?;
    let mut files = Vec::new();
    let models = models(&package, &owner, &mut files)?;
    let effects = effects(&package, &owner, models, &mut files)?;
    let parameters = battle_scene_package::member(&package, 0x100)?
        .context("skill parameter block is absent")?;
    let scene: Scene<WeaponDefinition> = Scene {
        effects: format!("{owner}/effects.json"),
        cutin: cutin(&package, &owner, &mut files)?,
        weapon: weapon(&package, &owner, &mut files)?,
        projectiles: projectiles(&package, &module)?,
        actions: Default::default(),
        actions_error: None,
        duration: dol::u16(parameters, 16)?,
        stage_color: None,
        camera: None,
        strike_points: Vec::new(),
        recovery_radius: None,
        spell_recipe: None,
    };
    for field in [0x10c, 0x110] {
        ensure!(
            battle_scene_package::member(&package, field)?.is_none(),
            "skill {id} has unsupported auxiliary member {field:#x}"
        );
    }
    files.push((
        format!("{owner}/scene.json"),
        resonance_asset_writer::json::encode(&scene)?,
    ));
    files.push((
        format!("{owner}/effects.json"),
        resonance_asset_writer::json::encode(&effects)?,
    ));
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
