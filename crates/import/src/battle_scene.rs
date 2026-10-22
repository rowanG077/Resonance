//! Stored spell and skill models, artwork and animation packages.
use crate::{
    battle_model::{RigKind, files, rig},
    character::Clip,
    model_preview::{Layer, PointerMembers, layers_with_clips},
    rel::Rel,
    scene::decoded::Package,
    source_assets::{Sources, physical_ranges, read_range},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_model::ModelPart,
    battle_scene::{Scene, SceneProjectile, Texture},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, fs, path::Path};

mod catalogue;
pub use catalogue::{Publication, publish_all};
pub(crate) use catalogue::{catalogue_paths, publish_all_source};

/// Publish the selected scene through the same path used by full cooking.
pub fn publish(extracted: &Path, technique: u16, output: &Path) -> Result<String> {
    publish_source(
        extracted,
        &Sources::read(extracted)?,
        technique,
        output,
        "battle",
    )
}

/// Publish one catalogue-selected skill scene without cooking unrelated resources.
pub fn publish_skill(extracted: &Path, catalogue: u16, output: &Path) -> Result<String> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let imported = crate::arte::read(&executable)?;
    let package = *imported
        .skill_packages
        .get(&catalogue)
        .context("arte has no stored skill package")?;
    publish_skill_source(
        extracted,
        &Sources::read_with(extracted, &executable)?,
        catalogue,
        package,
        output,
    )
}

pub(crate) fn publish_skill_source(
    extracted: &Path,
    sources: &Sources,
    catalogue: u16,
    package: u16,
    output: &Path,
) -> Result<String> {
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let mut shared = crate::battle_effect::SharedPalettes::read(&usual, output)?;
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let offsets = module
        .at((5, 0x11c4))?
        .get(..4 * 4)
        .context("truncated skill directory")?
        .chunks_exact(4)
        .map(|row| crate::read::u32(row, 0))
        .collect::<Result<Vec<_>>>()?;
    let archive = extracted.join("files").join(&sources.skill);
    let range = physical_ranges(&offsets, 1, fs::metadata(&archive)?.len())?
        .into_iter()
        .find_map(|(slot, range)| (slot == package).then_some(range))
        .context("skill package selector has no physical range")?;
    let bytes = read_range(&archive, range)?;
    let path = resonance_content::battle_scene::skill_path(catalogue);
    let scene = read(
        &bytes,
        &format!("battle/scenes/skills/{catalogue:03}.effects.json"),
        output,
        None,
        &crate::battle_profile::element_contact_sounds(&module)?,
        &mut shared,
    )?;
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&scene)?)?;
    Ok(path)
}

pub(crate) fn publish_source(
    extracted: &Path,
    sources: &Sources,
    technique: u16,
    output: &Path,
    prefix: &str,
) -> Result<String> {
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let mut shared = crate::battle_effect::SharedPalettes::read(&usual, output)?;
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let archive = extracted.join("files").join(&sources.magic);
    let range = magic_ranges(&module, &archive)?
        .into_iter()
        .find_map(|(id, range)| (id == technique).then_some(range))
        .context("spell has no physical package")?;
    let bytes = read_range(&archive, range)?;
    let path = format!("{prefix}/scenes/{technique:03}.json");
    let mut scene = read(
        &bytes,
        &format!("{prefix}/scenes/{technique:03}.effects.json"),
        output,
        Some(technique),
        &crate::battle_profile::element_contact_sounds(&module)?,
        &mut shared,
    )?;
    scene_settings(&module, technique, &mut scene)?;
    scene_actions(
        &bytes,
        technique,
        &crate::battle_profile::element_contact_sounds(&module)?,
        &mut scene,
    )?;
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&scene)?)?;
    Ok(path)
}

fn magic_ranges(module: &Rel, archive: &Path) -> Result<Vec<(u16, std::ops::Range<usize>)>> {
    let offsets = module
        .at((5, 0xfe0))?
        .get(..0x1e4)
        .context("truncated spell directory")?
        .chunks_exact(4)
        .map(|row| crate::read::u32(row, 0))
        .collect::<Result<Vec<_>>>()?;
    // The first stored slot (scene 201) starts at byte zero; later zero entries
    // denote resident spells without stored packages.
    Ok(physical_ranges(&offsets, 1, fs::metadata(archive)?.len())?
        .into_iter()
        .map(|(slot, range)| (slot + 200, range))
        .collect())
}

fn scene_settings(module: &Rel, spell: u16, scene: &mut Scene) -> Result<()> {
    scene.stage_color = None;
    scene.camera = None;
    scene.recovery_radius = match spell {
        240 => Some(0x4d2c),
        255 => Some(0x5af4),
        256 => Some(0x5b5c),
        258 => Some(0x555c),
        _ => None,
    }
    .map(|offset| -> Result<_> {
        let radius = crate::read::f32(module.at((4, offset))?, 0)?;
        ensure!(radius.is_finite() && radius > 0., "invalid recovery radius");
        Ok(radius)
    })
    .transpose()?;
    scene.strike_points = match spell {
        252 => Some((0x7dac, 9)),
        269 | 290 | 296 => Some((0x866c, 17)),
        233 | 292 => Some((0x941c, 14)),
        _ => None,
    }
    .map(|(offset, count)| -> Result<_> {
        module
            .at((4, offset))?
            .get(..count * 12)
            .context("truncated scene strike pattern")?
            .chunks_exact(12)
            .map(|row| {
                let point = [
                    crate::read::f32(row, 0)?,
                    crate::read::f32(row, 4)?,
                    crate::read::f32(row, 8)?,
                ];
                ensure!(
                    point.into_iter().all(f32::is_finite),
                    "invalid scene strike point"
                );
                Ok(point)
            })
            .collect::<Result<Vec<_>>>()
    })
    .transpose()?
    .unwrap_or_default();
    let observed = match spell {
        201 => Some(([16, 16, 24], 2950.0, 8.0)),
        202 => Some(([0, 0, 0], 2700.0, 13.0)),
        206 => Some(([20, 16, 16], 2900.0, 15.5)),
        207 => Some(([32, 20, 20], 2650.0, 13.0)),
        209 => Some(([16, 24, 24], 2700.0, 13.0)),
        210 => Some(([16, 20, 16], 3400.0, 15.5)),
        211 => Some(([20, 24, 24], 2400.0, 8.0)),
        214 => Some(([16, 12, 12], 2600.0, 18.0)),
        217 => Some(([16, 16, 24], 2700.0, 13.0)),
        218 => Some(([8, 8, 16], 3450.0, 18.0)),
        219 => Some(([16, 16, 24], 2650.0, 18.0)),
        222 => Some(([16, 16, 32], 2700.0, 8.0)),
        223 => Some(([32, 24, 24], 2850.0, 14.5)),
        224 => Some(([16, 16, 16], 2450.0, 14.5)),
        226 => Some(([24, 16, 16], 2400.0, 8.0)),
        227 => Some(([16, 16, 16], 2950.0, 18.0)),
        228 => Some(([32, 32, 32], 2800.0, 15.5)),
        229 => Some(([16, 16, 16], 2950.0, 14.0)),
        230 => Some(([16, 16, 24], 2700.0, 14.0)),
        231 => Some(([24, 16, 24], 2700.0, 13.0)),
        232 => Some(([12, 12, 12], 2950.0, 18.0)),
        233 => Some(([16, 16, 16], 4200.0, 15.5)),
        240 => Some(([16, 16, 16], 3200.0, 15.5)),
        241 => Some(([16, 16, 16], 1950.0, 13.0)),
        243 => Some(([20, 20, 16], 2950.0, 13.0)),
        244 => Some(([20, 16, 20], 2450.0, 8.0)),
        246 => Some(([20, 16, 16], 2950.0, 13.0)),
        247 => Some(([20, 16, 16], 2450.0, 8.0)),
        249 => Some(([16, 16, 20], 2950.0, 13.0)),
        250 => Some(([20, 20, 32], 2450.0, 10.5)),
        252 => Some(([12, 12, 12], 3550.0, 11.5)),
        253 => Some(([12, 12, 12], 2700.0, 15.5)),
        255 => Some(([12, 12, 16], 3100.0, 15.5)),
        256 => Some(([12, 16, 12], 3100.0, 15.5)),
        258 => Some(([16, 16, 16], 3050.0, 12.0)),
        259 => Some(([12, 12, 20], 2700.0, 13.0)),
        260 => Some(([0, 0, 0], 2450.0, 16.0)),
        261 => Some(([16, 16, 16], 2450.0, 8.0)),
        263 => Some(([0, 0, 0], 2450.0, 8.0)),
        267 => Some(([0, 0, 0], 2700.0, 8.0)),
        268 => Some(([0, 0, 0], 2700.0, 8.0)),
        269 => Some(([0, 0, 0], 3450.0, 12.0)),
        270 => Some(([0, 0, 0], 3400.0, 11.5)),
        278 => Some(([48, 48, 48], 2250.0, 8.0)),
        280 => Some(([0, 0, 0], 2750.0, 16.0)),
        282 => Some(([64, 64, 64], 1950.0, 8.0)),
        283 => Some(([48, 48, 48], 2700.0, 15.5)),
        284 => Some(([24, 16, 16], 3150.0, 15.5)),
        285 => Some(([16, 16, 24], 3350.0, 15.5)),
        286 => Some(([16, 24, 16], 2950.0, 15.5)),
        287 => Some(([20, 20, 16], 3150.0, 15.5)),
        288 => Some(([16, 16, 24], 3150.0, 17.0)),
        289 => Some(([16, 16, 20], 3200.0, 16.5)),
        291 => Some(([8, 8, 8], 3150.0, 15.5)),
        293 => Some(([16, 16, 16], 3150.0, 19.5)),
        296 => Some(([0, 0, 0], 2450.0, 12.0)),
        _ => None,
    };
    if let Some((color, minimum_distance, minimum_elevation)) = observed {
        scene.stage_color = Some(color);
        scene.camera = Some(resonance_content::battle_scene::CameraBounds {
            minimum_distance,
            minimum_elevation,
        });
        return Ok(());
    }
    let (color_offset, camera_offset) = match spell {
        203 => (0x5a20, 0x5a24),
        205 => (0x5140, 0x5144),
        213 => (0x4730, 0x4734),
        215 => (0x6098, 0x609c),
        221 => (0x5948, 0x594c),
        237 => (0x36c8, 0x36cc),
        238 => (0x4f20, 0x4f24),
        251 => (0x6990, 0x6998),
        257 => (0x6270, 0x6274),
        265 => (0x54f0, 0x54f4),
        290 => (0x8668, 0x8750),
        292 => (0x9418, 0x94dc),
        _ => return Ok(()),
    };
    let color = module
        .at((4, color_offset))?
        .get(..4)
        .context("truncated scene color")?;
    ensure!(color[3] == 255, "unsupported scene tint alpha");
    let settings = module.at((4, camera_offset))?;
    let camera = resonance_content::battle_scene::CameraBounds {
        minimum_distance: crate::read::f32(settings, 0)?,
        minimum_elevation: crate::read::f32(settings, 4)?,
    };
    camera.validate()?;
    scene.stage_color = Some(color[..3].try_into()?);
    scene.camera = Some(camera);
    Ok(())
}

/// Reuse scene artwork and complete palette windows required by refreshed effects.
fn scene_effect_metadata(
    bytes: &[u8],
    output: &Path,
    scene: &mut Scene,
    shared: &mut crate::battle_effect::SharedPalettes,
) -> Result<()> {
    let members = PointerMembers::new(bytes, 4..0x114)?;
    let path = output.join(&scene.effects);
    let cached: resonance_content::battle_effect::SourceBank =
        serde_json::from_slice(&fs::read(&path)?)?;
    let (mut bank, strides) = crate::battle_effect::read_with_palettes(
        members.model(4)?.context("missing scene effects")?,
    )?;
    ensure!(
        bank.source_sha256 == cached.source_sha256 && bank.actors.len() == cached.actors.len(),
        "cached scene effect declarations differ from source"
    );
    let mut art = cached
        .art
        .context("cached scene effect artwork is missing")?;
    ensure!(
        scene.source_sha256 == crate::digest(bytes) && art.source_sha256 == scene.source_sha256,
        "cached scene effect artwork differs from source"
    );
    let mut textures = BTreeMap::from([(
        6,
        members
            .model(8)?
            .map(|bytes| {
                crate::battle_effect::art::cached_bank_textures(
                    bytes,
                    &bank,
                    &strides,
                    6,
                    art.textures
                        .get(&6)
                        .context("cached scene texture slot is missing")?,
                    output,
                )
            })
            .transpose()?
            .unwrap_or_default(),
    )]);
    crate::battle_effect::art::bind_palettes(&mut bank, &strides, &mut textures)?;
    shared.bind(&mut bank, &strides)?;
    let textures: Vec<_> = textures
        .remove(&6)
        .unwrap()
        .into_iter()
        .map(|texture| texture.effect)
        .collect();
    for image in textures.iter().flat_map(|texture| &texture.images) {
        if !art.files.contains_key(&image.path) {
            art.files.insert(
                image.path.clone(),
                File {
                    sha256: crate::media::hash_file(&output.join(&image.path))?,
                    bytes: fs::metadata(output.join(&image.path))?.len(),
                    roles: [Role::Texture].into(),
                },
            );
        }
    }
    scene.textures = textures
        .iter()
        .map(|texture| Texture {
            images: texture.images.clone(),
            sampler: texture.sampler.clone(),
        })
        .collect();
    scene.files.extend(art.files.clone());
    art.textures.insert(6, textures);
    bank.art = Some(art);
    let encoded = serde_json::to_vec(&bank)?;
    let file = scene
        .files
        .get_mut(&scene.effects)
        .context("missing scene effect identity")?;
    file.sha256 = crate::digest(&encoded);
    file.bytes = encoded.len() as u64;
    crate::write_atomic(&path, &encoded)
}

/// Refresh small participant tracks without converting the scene's artwork again.
fn scene_actions(bytes: &[u8], spell: u16, sounds: &[u8; 9], scene: &mut Scene) -> Result<()> {
    if !(301..=318).contains(&spell) {
        return Ok(());
    }
    let decoded = (|| -> Result<_> {
        let members = PointerMembers::new(bytes, 4..0x114)?;
        let mut parameters = members
            .model(0x100)?
            .context("missing combination actions")?
            .to_vec();
        let rules = contact_rules(&parameters, sounds, 6)?;
        let start = crate::read::u32(&parameters, 0)? as usize;
        parameters[start..start + rules.len()].copy_from_slice(&rules);
        crate::battle_martial::combined(&parameters)
    })();
    match decoded {
        Ok(actions) => {
            scene.actions = actions;
            scene.actions_error = None;
        }
        Err(error) => {
            scene.actions.clear();
            scene.actions_error = Some(format!("{error:#}"));
        }
    }
    Ok(())
}

fn read(
    bytes: &[u8],
    effects_path: &str,
    output: &Path,
    spell: Option<u16>,
    element_sounds: &[u8; 9],
    shared: &mut crate::battle_effect::SharedPalettes,
) -> Result<Scene> {
    let members = PointerMembers::new(bytes, 4..0x114)?;
    // These optional members have no consumer in the current stored scene.
    for field in [0x10c, 0x110] {
        ensure!(
            members.model(field)?.is_none(),
            "unsupported stored scene member {field:#x}"
        );
    }
    let (mut effects, strides) = crate::battle_effect::read_with_palettes(
        members.model(4)?.context("missing scene effects")?,
    )?;
    let mut effect_textures = BTreeMap::from([(
        6,
        members
            .model(8)?
            .map(|bytes| {
                crate::battle_effect::art::bank_textures(bytes, &effects, &strides, 6, output)
            })
            .transpose()?
            .unwrap_or_default(),
    )]);
    crate::battle_effect::art::bind_palettes(&mut effects, &strides, &mut effect_textures)?;
    shared.bind(&mut effects, &strides)?;
    let effect_textures: BTreeMap<_, _> = effect_textures
        .into_iter()
        .map(|(slot, textures)| {
            (
                slot,
                textures
                    .into_iter()
                    .map(|texture| texture.effect)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let textures = effect_textures[&6]
        .iter()
        .map(|texture| Texture {
            images: texture.images.clone(),
            sampler: texture.sampler.clone(),
        })
        .collect::<Vec<_>>();
    let mut decoded = Package::default();
    let mut models = BTreeMap::new();
    for slot in 0..10 {
        let model = members.model(12 + slot * 4)?;
        let outline = members.model(52 + slot * 4)?;
        let motions = (0..4)
            .map(|motion| members.animation(92 + slot * 16 + motion * 4, &mut decoded))
            .collect::<Result<Vec<_>>>()?;
        let Some(model) = model else {
            ensure!(
                outline.is_none() && motions.iter().all(Option::is_none),
                "scene animation or outline has no model"
            );
            continue;
        };
        let clips = motions
            .iter()
            .enumerate()
            .filter_map(|(slot, motion)| {
                motion.as_ref().map(|animation| Clip {
                    slot: slot as u16,
                    resource: None,
                    animation,
                })
            })
            .collect::<Vec<_>>();
        let mut layers = Vec::new();
        layers_with_clips(
            Layer {
                model,
                outline,
                animation: None,
                attached_to: None,
                additive: false,
            },
            &mut layers,
            &format!("battle/scene/{slot}"),
            &clips,
            output,
            &mut decoded,
        )?;
        models.insert(
            slot as u8,
            ModelPart {
                rig: rig(model, RigKind::Effect)?,
                layer_skeletons: Default::default(),
                layers,
            },
        );
    }
    let mut cutin_source = members.model(0x104)?;
    let mut weapon_source = members.model(0x108)?;
    // Auxiliary artwork can be a TPL cut-in or a counted attachment package.
    if cutin_source.is_some_and(|bytes| matches!(crate::read::u32(bytes, 0), Ok(5..=7))) {
        ensure!(
            weapon_source.is_none(),
            "scene declares multiple attachments"
        );
        weapon_source = cutin_source.take();
    }
    let cutin = cutin_source
        .map(|bytes| -> Result<_> {
            let mut textures = crate::battle_effect::art::textures_from_source(bytes, output)?;
            ensure!(textures.len() == 1, "scene cut-in must contain one texture");
            let texture = textures.remove(0).effect;
            ensure!(
                texture.images.len() == 1,
                "scene cut-in must contain one image"
            );
            Ok(Texture {
                images: texture.images,
                sampler: texture.sampler,
            })
        })
        .transpose()?;
    let weapon = weapon_source
        .map(|bytes| {
            crate::battle_model::weapon::read_package(
                bytes,
                &crate::digest(bytes),
                &[],
                output,
                &mut decoded,
            )
        })
        .transpose()?;
    let mut files = files(
        models
            .values()
            .flat_map(|model| model.layers.iter().map(|layer| &layer.scene)),
        output,
    )?;
    for image in textures.iter().flat_map(|texture| &texture.images) {
        files.insert(
            image.path.clone(),
            File {
                sha256: crate::media::hash_file(&output.join(&image.path))?,
                bytes: std::fs::metadata(output.join(&image.path))?.len(),
                roles: [Role::Texture].into(),
            },
        );
    }
    effects.art = Some(resonance_content::battle_effect::Art {
        source_sha256: crate::digest(bytes),
        textures: effect_textures,
        models: models.clone(),
        files: files.clone(),
    });
    let effect_bytes = serde_json::to_vec(&effects)?;
    crate::write_atomic(&output.join(effects_path), &effect_bytes)?;
    files.insert(
        effects_path.into(),
        File {
            sha256: crate::digest(&effect_bytes),
            bytes: effect_bytes.len() as u64,
            roles: [Role::Data].into(),
        },
    );
    if let Some(cutin) = &cutin {
        for image in &cutin.images {
            files.insert(
                image.path.clone(),
                File {
                    sha256: crate::media::hash_file(&output.join(&image.path))?,
                    bytes: fs::metadata(output.join(&image.path))?.len(),
                    roles: [Role::Texture].into(),
                },
            );
        }
    }
    if let Some(weapon) = &weapon {
        files.extend(weapon.files.clone());
    }
    let parameters = members.model(0x100)?;
    let projectiles = projectiles(bytes, spell, element_sounds)?;
    Ok(Scene {
        source_sha256: crate::digest(bytes),
        effects: effects_path.into(),
        textures,
        models,
        cutin,
        weapon,
        projectiles,
        actions: BTreeMap::new(),
        actions_error: None,
        stage_color: None,
        camera: None,
        strike_points: Vec::new(),
        recovery_radius: None,
        duration: parameters
            .map(|bytes| crate::read::u16(bytes, 16))
            .transpose()?
            .unwrap_or(0),
        files,
    })
}

fn projectiles(
    bytes: &[u8],
    spell: Option<u16>,
    element_sounds: &[u8; 9],
) -> Result<Vec<SceneProjectile>> {
    let members = PointerMembers::new(bytes, 4..0x114)?;
    let parameters = members.model(0x100)?;
    members
        .model(0xfc)?
        .map(|rows| -> Result<_> {
            let parameters = parameters.context("scene projectiles have no contact rules")?;
            const SPELL_EFFECT_BANK: u8 = 6;
            const SKILL_EFFECT_BANK: u8 = 8;
            let effect_bank = if spell.is_some() {
                SPELL_EFFECT_BANK
            } else {
                SKILL_EFFECT_BANK
            };
            let rules = contact_rules(parameters, element_sounds, effect_bank)?;
            let start = crate::read::u32(bytes, 0xfc)? as usize;
            crate::battle_projectile::read_package_member(rows, start)?
                .records
                .into_iter()
                .zip(rows.chunks_exact(400))
                .enumerate()
                .map(|(index, (projectile, row))| {
                    use resonance_content::battle_projectile::DamageKind;
                    let kind = match row[17] {
                        0 => Ok(DamageKind::Slash),
                        1 => Ok(DamageKind::Thrust),
                        2 => Ok(DamageKind::Magic),
                        kind => Err(format!("unknown scene damage kind {kind}")),
                    };
                    let selected = if matches!(spell, Some(213 | 215)) {
                        index
                    } else if matches!(spell, Some(232 | 253 | 283 | 286)) {
                        index.saturating_sub(1)
                    } else if matches!(spell, Some(206 | 207 | 219 | 230 | 231)) && index == 2 {
                        1
                    } else {
                        usize::from(row[18])
                    };
                    let mut hit_errors = BTreeMap::new();
                    let hits = (0..rules.len() / 28)
                        .map(|rule| {
                            let decoded = (|| -> Result<_> {
                                let kind = *kind
                                    .as_ref()
                                    .map_err(|reason| anyhow::anyhow!(reason.clone()))?;
                                crate::battle_action::hit_rule(
                                    &rules,
                                    rule.try_into()?,
                                    kind,
                                    row[19],
                                )
                            })();
                            match decoded {
                                Ok(hit) => Some(hit),
                                Err(error) => {
                                    hit_errors.insert(rule, format!("{error:#}"));
                                    None
                                }
                            }
                        })
                        .collect::<Vec<_>>();
                    let hit = hits.get(selected).cloned().flatten();
                    let unsupported_reason = hit.is_none().then(|| {
                        let reason = hit_errors
                            .get(&selected)
                            .map(String::as_str)
                            .unwrap_or("missing contact rule");
                        format!("contact rule {selected}: {reason}")
                    });
                    Ok(SceneProjectile {
                        projectile,
                        hit,
                        hits,
                        hit_errors,
                        unsupported_reason,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

/// Ordinary contact feedback already emits the authored elemental default.
/// Remove only that redundant request and retain distinct cues for the contact.
fn contact_rules(parameters: &[u8], element_sounds: &[u8; 9], effect_bank: u8) -> Result<Vec<u8>> {
    let start = crate::read::u32(parameters, 0)? as usize;
    let end = crate::read::u32(parameters, 4)? as usize;
    let mut rules = parameters
        .get(start..end)
        .context("scene contact rules outside member")?
        .to_vec();
    ensure!(
        rules.len().is_multiple_of(28),
        "misaligned scene contact rules"
    );
    for row in rules.chunks_exact_mut(28) {
        // Contact effects are resolved against this scene's prepared bank.
        if row[22] != 0 && row[24] == effect_bank {
            row[24] = 0;
        }
        if (1..=8).contains(&row[2])
            && crate::read::u16(row, 16)? == u16::from(element_sounds[usize::from(row[2])])
        {
            row[16..18].fill(0);
        }
    }
    Ok(rules)
}

#[cfg(test)]
#[test]
fn stored_contacts_deduplicate_element_cues_and_keep_distinct_sounds() -> Result<()> {
    use resonance_content::battle_projectile::DamageKind;
    let mut parameters = [0; 36];
    parameters[..4].copy_from_slice(&8_u32.to_be_bytes());
    parameters[4..8].copy_from_slice(&36_u32.to_be_bytes());
    let mut sounds = [0; 9];
    sounds[2] = 37;
    for (element, cue, sound) in [
        (2, 0_u16, None),
        (2, 37, None),
        (2, 38, Some(38)),
        (0, 37, Some(37)),
        (10, 37, Some(37)),
    ] {
        parameters[8 + 2] = element;
        parameters[8 + 16..8 + 18].copy_from_slice(&cue.to_be_bytes());
        let rules = contact_rules(&parameters, &sounds, 6)?;
        assert_eq!(
            crate::battle_action::hit_rule(&rules, 0, DamageKind::Magic, 0)?.sound,
            sound
        );
    }
    parameters[8 + 16..8 + 18].fill(0);
    parameters[8 + 22] = 4;
    for (bank, expected) in [(0, true), (6, true), (8, false)] {
        parameters[8 + 24] = bank;
        let rules = contact_rules(&parameters, &sounds, 6)?;
        let result = crate::battle_action::hit_rule(&rules, 0, DamageKind::Magic, 0);
        assert_eq!(result.is_ok(), expected);
        if let Ok(hit) = result {
            assert_eq!(hit.effect, Some(4));
        }
    }
    Ok(())
}
