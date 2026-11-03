//! Steam battle scenery and local effects use the ordinary GC arena documents.
use crate::{
    battle_effects, effect_atlas, output, package::Package, pe, preview_model, source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{
        EffectTexture,
        declaration::Declaration,
        document::{Artwork, Definition as Effects},
        visual::ParticleTexture,
    },
    battle_stage::{
        Fog, TextureAnimation, TextureMotion,
        document::{self, Definition, Layer},
    },
    library::Mappings,
    model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_constants::ArenaId;

fn name(id: u16) -> String {
    ArenaId::try_from(id)
        .ok()
        .and_then(ArenaId::resource_name)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("arena-{id:04}"))
}

fn texture_animation(data: &[u8]) -> Result<TextureAnimation> {
    let motion = match data[0] {
        1 => TextureMotion::Atlas { frames: data[3] },
        2 => TextureMotion::Scroll {
            velocity: [
                f32::from_bits(pe::word(data, 4)?),
                f32::from_bits(pe::word(data, 8)?),
            ],
        },
        kind => anyhow::bail!("unsupported arena texture motion {kind}"),
    };
    Ok(TextureAnimation {
        texture: usize::from(data[1]),
        interval: data[2].max(1),
        motion,
    })
}

fn paired_sprites(
    source: &Source,
    library: &Path,
    bank: &mut Effects,
    resource: &str,
    atlases: &effect_atlas::Directory,
    owner: &str,
    slot: u8,
) -> Result<Vec<EffectTexture>> {
    let bytes = source.require(resource)?;
    let package = Package::parse(&bytes)?;
    ensure!(
        package.len() == 2,
        "paired arena atlas must have color and opacity images"
    );
    let color_name = package.reference(0)?.context("missing arena color atlas")?;
    let alpha_name = package
        .reference(1)?
        .context("missing arena opacity atlas")?;
    let original_dual =
        bank.templates
            .iter()
            .map(|declaration| {
                declaration.particle_visual().ok().is_some_and(|visual| {
                    visual.texture.slot() == Some(slot) && visual.texture.dual()
                })
            })
            .collect::<Vec<_>>();
    let mut alpha_bank = bank.clone();
    let mut color = effect_atlas::cook(
        source,
        library,
        bank,
        color_name,
        atlases.get(color_name),
        &format!("{owner}/color"),
        slot,
    )?;
    let mut alpha = effect_atlas::cook(
        source,
        library,
        &mut alpha_bank,
        alpha_name,
        atlases.get(alpha_name),
        &format!("{owner}/alpha"),
        slot,
    )?;
    ensure!(
        bank.programs == alpha_bank.programs,
        "paired arena atlases rewrite different program UVs"
    );
    // Image deduplication may merge different color and alpha declarations.
    // Intern their binding pair together so both ordinary texture entries use
    // the same palette-set selector in the shared renderer.
    let mut pairs = vec![(vec![None; 256], vec![None; 256])];
    for (index, declaration) in bank.templates.iter_mut().enumerate() {
        let Declaration::Particle { template, visual } = declaration else {
            continue;
        };
        if visual.texture.slot() != Some(slot) {
            continue;
        }
        let other = &alpha_bank.templates[index];
        ensure!(
            template == other.template()?,
            "paired arena atlas frame layouts differ"
        );
        let pair = (
            color.palette_sets[usize::from(visual.texture.palette_set())].clone(),
            alpha.palette_sets[usize::from(other.particle_visual()?.texture.palette_set())].clone(),
        );
        let set = if let Some(set) = pairs.iter().position(|prior| *prior == pair) {
            set
        } else {
            pairs.push(pair);
            pairs.len() - 1
        };
        visual.texture = ParticleTexture::Atlas {
            slot,
            dual: original_dual[index],
            palette_set: set.try_into()?,
        };
    }
    (color.palette_sets, alpha.palette_sets) = pairs.into_iter().unzip();
    Ok(vec![color, alpha])
}

fn effects(
    source: &Source,
    library: &Path,
    package: &Package<'_>,
    atlases: &effect_atlas::Directory,
    residents: &BTreeMap<u8, String>,
    owner: &str,
) -> Result<Option<String>> {
    let Some(resource) = package.reference(9)? else {
        return Ok(None);
    };
    let bytes = source.require(resource)?;
    let mut bank = battle_effects::decode(&bytes, &[], |stride, declaration| {
        if let Declaration::Particle { visual, .. } = declaration
            && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
        {
            *palette_set = stride;
        }
        Ok(())
    })?;
    let slots = bank
        .templates
        .iter()
        .filter_map(|declaration| declaration.particle_visual().ok()?.texture.slot())
        .collect::<BTreeSet<_>>();
    let mut textures = BTreeMap::new();
    for slot in slots {
        let resource = if slot == 9 {
            package
                .reference(10)?
                .context("missing local arena atlas")?
        } else {
            residents
                .get(&slot)
                .map(String::as_str)
                .with_context(|| format!("unbound arena atlas {slot}"))?
        };
        let texture = if slot == 9 && resource.to_ascii_lowercase().ends_with(".pac") {
            paired_sprites(
                source,
                library,
                &mut bank,
                resource,
                atlases,
                &format!("{owner}/effects"),
                slot,
            )?
        } else {
            vec![effect_atlas::cook(
                source,
                library,
                &mut bank,
                resource,
                atlases.get(resource),
                &format!("{owner}/effects"),
                slot,
            )?]
        };
        textures.insert(slot, texture);
    }
    bank.artwork = Some(Artwork::from_models(textures, BTreeMap::new())?);
    battle_effects::bind_scene_opacity(&mut bank)?;
    let path = format!("{owner}/effects/effects.json");
    output::json(&library.join(&path), &bank)?;
    Ok(Some(path))
}

/// Scenery uses the shared GC unlit pass. Steam's baked RGB has half-range
/// modulation; opacity stays ordinary byte alpha. Preserve the source alpha
/// comparison before depth writes, including its cutout edge rejection.
fn normalize_scenery(
    source: &Source,
    library: &Path,
    descriptor: &str,
    scene: &mut resonance_content::ScenePart,
    model: &mut crate::model::Model,
) -> Result<()> {
    let stem = descriptor
        .rsplit_once('.')
        .context("arena model has no extension")?
        .0;
    let parameters = source.require(&format!("{stem}.mxd"))?;
    let (tints, _, _) = crate::model::material_parameters(&parameters, &model.materials)?;
    let cutoffs = crate::model::material_alpha_cutoffs(&parameters, &model.materials)?;
    for (mesh, material) in model
        .meshes
        .iter_mut()
        .filter(|mesh| !mesh.indices.is_empty())
        .zip(&mut scene.materials)
    {
        for color in &mut mesh.colors {
            for channel in &mut color[..3] {
                *channel = (*channel * 2.).min(1.);
            }
        }
        material.alpha_cutoff = cutoffs[usize::from(mesh.material)];
    }
    output::write(&library.join(&scene.mesh), &model.glb(&tints)?)
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    cook_selection(source, library, None)
}

fn cook_selection(source: &Source, library: &Path, selected: Option<u16>) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let atlases = effect_atlas::Directory::read(&image)?;
    let residents = effect_atlas::residents(source, &image)?;
    let directory = image.slice(0x8e6ee8, 97 * 4)?;
    let mut mappings = if selected.is_some() {
        let existing: Mappings =
            serde_json::from_slice(&std::fs::read(library.join(document::MAPPINGS))?)?;
        existing
            .iter()
            .map(|(id, name)| (id, name.to_owned()))
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut absent = Vec::new();
    let mut failures = Vec::new();
    let mut layers_count = 0;
    let mut cooked_count = 0;
    for (id, pointer) in directory.chunks_exact(4).enumerate() {
        if selected.is_some_and(|selected| usize::from(selected) != id) {
            continue;
        }
        let resource = image.text(pe::word(pointer, 0)?)?;
        ensure!(
            resource == format!("BG_{id:03}.PAC"),
            "Steam arena directory changed"
        );
        if !source.contains(resource)? {
            absent.push(id);
            continue;
        }
        let result = (|| -> Result<()> {
            let bytes = source.require(resource)?;
            let package = Package::parse(&bytes)?;
            ensure!(package.len() >= 11, "truncated arena member table");
            let data = source.require(package.reference(0)?.context("missing arena metadata")?)?;
            ensure!(data.len() >= 0x318, "truncated arena metadata");
            let animation_start = usize::from(data[0x304]);
            let scenery_end = if animation_start == 0 {
                package.len()
            } else {
                animation_start
            };
            ensure!(
                (11..=package.len()).contains(&scenery_end)
                    && scenery_end - 11 <= 32
                    && (animation_start == 0 || scenery_end - 11 <= 15),
                "invalid arena scenery directory"
            );
            let name = name(id as u16);
            let owner = format!("battle/arenas/{name}");
            let mut layers = Vec::new();
            let mut primary_joints = Vec::<String>::new();
            for section in (1..5).chain(11..scenery_end) {
                let Some(resource) = package.reference(section)? else {
                    continue;
                };
                let slot = if section < 5 {
                    section - 1
                } else {
                    section - 7
                };
                let label = match slot {
                    0 => "background".into(),
                    1..=3 => format!("foreground-{slot}"),
                    _ => format!("scenery-{:02}", slot - 3),
                };
                let motion = if section < 5 {
                    package.reference(section + 4)?
                } else if animation_start != 0 && data[0x305 + section - 11] != u8::MAX {
                    package.reference(animation_start + usize::from(data[0x305 + section - 11]))?
                } else {
                    None
                };
                let motions = motion
                    .map(|name| vec![(0, name.to_owned())])
                    .unwrap_or_default();
                let (mut scene, mut model) =
                    preview_model::part(source, library, resource, &owner, &label, &motions)
                        .with_context(|| format!("arena {id} layer {label}"))?;
                normalize_scenery(source, library, resource, &mut scene, &mut model)?;
                if slot == 0 {
                    primary_joints = model.rig.joint_names();
                }
                scene.resource = slot.try_into()?;
                scene.autoplay = motion.is_some();
                for material in &mut scene.materials {
                    material.depth_write = slot == 0 || slot >= 3;
                    material.blend = true;
                }
                let settings = 0x2c + slot * 0xa8;
                let (flags, texture_animations) = if section < 5 {
                    let count = usize::from(data[settings + 0xa5]);
                    ensure!(count <= 4, "too many arena UV animations");
                    (
                        data[settings + 0xa4],
                        (0..count)
                            .map(|index| texture_animation(&data[settings + index * 40..]))
                            .collect::<Result<_>>()?,
                    )
                } else {
                    (0, Vec::new())
                };
                ensure!(flags & !3 == 0, "unsupported arena layer flags {flags:#x}");
                let joint = if section >= 11 {
                    let expected = format!("kk{:02x}", section - 11);
                    Some(
                        primary_joints
                            .iter()
                            .find(|joint| joint.eq_ignore_ascii_case(&expected))
                            .with_context(|| {
                                format!("arena {id} has no scenery anchor {expected}")
                            })?
                            .clone(),
                    )
                } else {
                    None
                };
                layers.push(Layer {
                    name: label,
                    model: PreviewPart {
                        scene,
                        animation: motion.map(|_| 0),
                        attached_to: None,
                        additive: flags & 2 != 0,
                        uv_offsets: Vec::new(),
                    },
                    foreground: (1..=3).contains(&slot),
                    joint,
                    minimum_alpha: (slot >= 4 && data[0x2d6] != 0).then(|| data[0x2d8 + slot - 4]),
                    animation_rate: if motion.is_some() && section < 5 {
                        Some(f32::from_bits(pe::word(&data, settings + 0xa0)?))
                    } else {
                        None
                    },
                    texture_animations,
                });
            }
            ensure!(
                layers
                    .first()
                    .is_some_and(|layer| layer.name == "background"),
                "arena has no background"
            );
            let number = |at| -> Result<f32> { Ok(f32::from_bits(pe::word(&data, at)?)) };
            let vector =
                |at| -> Result<[f32; 3]> { Ok([number(at)?, number(at + 4)?, number(at + 8)?]) };
            let color = |at| -> Result<[u8; 4]> { Ok(data[at..at + 4].try_into()?) };
            let recurring = pe::half(&data, 0x2d4)?;
            let effects = effects(source, library, &package, &atlases, &residents, &owner)?;
            ensure!(
                recurring == 0 || effects.is_some(),
                "recurring arena has no effect bank"
            );
            let fog_end = pe::half(&data, 0x302)?;
            let fog = if fog_end == 0 {
                None
            } else {
                Some(Fog {
                    color: color(0x2fc)?,
                    start: f32::from(pe::half(&data, 0x300)?),
                    end: f32::from(fog_end),
                })
            };
            let definition = Definition {
                unsupported_reason: None,
                light_position: vector(0)?,
                chain_acceleration: vector(12)?,
                actor_color: color(24)?,
                translation: vector(28)?,
                yaw_degrees: number(40)?,
                color: color(0x2f8)?,
                camera_pitch_offset: number(0x314)?,
                layers,
                fade_step: if data[0x2d6] == 0 { 24 } else { data[0x2d6] },
                fog,
                effects,
                recurring_interval: (recurring != 0).then_some(recurring),
            };
            layers_count += definition.layers.len();
            output::json(&library.join(&owner).join("arena.json"), &definition)?;
            mappings.insert(id as u32, name);
            cooked_count += 1;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("  Arena {id}: {error:#}");
            failures.push(id);
        }
    }
    output::json(&library.join(document::MAPPINGS), &Mappings::new(mappings)?)?;
    eprintln!(
        "  Battle arenas: {cooked_count} owners, {layers_count} layers; absent source packages: {absent:?}",
    );
    ensure!(
        failures.is_empty(),
        "arenas remain unconverted: {failures:?}"
    );
    ensure!(
        selected.is_none() || cooked_count == 1,
        "selected arena has no source package"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use std::path::PathBuf;

    #[test]
    fn arena_texture_motion_keeps_native_clock_and_scalar_types() -> Result<()> {
        let mut row = [0; 40];
        row[..4].copy_from_slice(&[2, 3, 0, 0]);
        row[4..8].copy_from_slice(&0.125f32.to_le_bytes());
        row[8..12].copy_from_slice(&(-0.25f32).to_le_bytes());
        let animation = texture_animation(&row)?;
        assert_eq!(animation.texture, 3);
        assert_eq!(animation.interval, 1);
        assert_eq!(animation.offset(8), [1., -2.]);
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam and a cooked library; refreshes only the arena category"]
    fn installed_battle_arena_category_cooks() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &root)
    }

    #[test]
    #[ignore = "requires installed Steam and a cooked library; recooks only RESONANCE_TEST_ARENA (default 13)"]
    fn installed_arena_uses_gc_scenery() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let id = std::env::var("RESONANCE_TEST_ARENA")
            .unwrap_or_else(|_| "13".into())
            .parse()?;
        cook_selection(&source, &root, Some(id))?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let stage = document::capture(&mut library.snapshot(), id, &diagnostics, &|| false)?;
        stage.validate()?;
        if id == 13 {
            assert_eq!(stage.layers.len(), 1);
            assert_eq!(stage.light_position, [300., 800., -200.]);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks and prepares all physical arenas"]
    fn installed_battle_environments_cook_and_load_shared_documents() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &root)?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let mappings: Mappings = library.snapshot().json(
            document::MAPPINGS,
            resonance_content::preparation::JSON_LIMIT,
        )?;
        ensure!(mappings.iter().count() == 84, "incomplete arena catalogue");
        let mut layers = 0;
        let mut banks = 0;
        let mut programs = 0;
        for (id, _) in mappings.iter() {
            let mut snapshot = library.snapshot();
            let stage = document::capture(&mut snapshot, id as u16, &diagnostics, &|| false)
                .with_context(|| format!("prepare arena {id}"))?;
            stage.validate()?;
            layers += stage.layers.len();
            if let Some(path) = &stage.effects {
                let bank: Effects =
                    snapshot.json(path, resonance_content::preparation::JSON_LIMIT)?;
                ensure!(
                    !bank.programs.iter().flatten().any(|event| matches!(
                        event.operation,
                        resonance_content::battle_effect::EffectOperation::Unsupported { .. }
                    )),
                    "arena {id} has an unavailable program"
                );
                ensure!(
                    !bank
                        .templates
                        .iter()
                        .any(|declaration| matches!(declaration, Declaration::Unsupported { .. })),
                    "arena {id} has an unavailable template"
                );
                programs += bank.programs.len();
                banks += 1;
                if matches!(id, 18 | 48) {
                    let art = bank.artwork.context("paired arena artwork is absent")?;
                    ensure!(
                        art.textures[&9].len() == 2,
                        "paired arena lost its opacity atlas"
                    );
                    ensure!(
                        bank.templates.iter().any(|declaration| declaration
                            .particle_visual()
                            .ok()
                            .is_some_and(|visual| visual.texture.dual())),
                        "paired arena lost dual sampling"
                    );
                }
            }
            if id == 13 {
                assert_eq!(stage.light_position, [300., 800., -200.]);
                assert_eq!(stage.yaw_degrees, -15.);
            }
        }
        ensure!(
            (layers, banks, programs) == (412, 78, 87),
            "incomplete native arena contents"
        );
        Ok(())
    }
}
