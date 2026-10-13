//! Battle arena resources.
use crate::{
    field::MapArchive,
    model_preview::{self, Layer},
    read::{f32 as float, u16 as half, u32 as word},
    rel::Rel,
    scene::decoded::Package,
    source_assets::{Sources, read_range},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_scene::Texture,
    battle_stage::{Fog, SceneryFade, Stage, TextureAnimation, TextureMotion},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, ops::Range, path::Path};

const SCENERY_FRAMES_PER_UPDATE: f32 = 0.5;
const LAYER_ADDITIVE: u8 = 0b10;
const LAYER_FLAGS: u8 = 0b11;

/// Publish a selected arena using the ordinary shared model conversion.
pub fn publish(extracted: &Path, arena: u16, output: &Path) -> Result<String> {
    publish_source(
        extracted,
        &Sources::read(extracted)?,
        arena,
        output,
        "battle",
    )
}

pub(crate) fn publish_source(
    extracted: &Path,
    sources: &Sources,
    arena: u16,
    output: &Path,
    prefix: &str,
) -> Result<String> {
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let table = module
        .at((5, 0x3b90))?
        .get(..0x188)
        .context("truncated stage directory")?;
    let bytes = read_range(
        &extracted.join("files").join(&sources.stages),
        range(table, arena)?,
    )?;
    let archive = MapArchive::decode(&bytes)?;
    let path = format!("{prefix}/stages/{arena:03}.json");
    let stage = read(
        &archive,
        &format!("{prefix}/stages/{arena:03}.effects.json"),
        output,
    )?;
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&stage)?)?;
    Ok(path)
}

fn range(table: &[u8], arena: u16) -> Result<Range<usize>> {
    ensure!(arena <= i8::MAX as u16, "invalid stage index {arena}");
    let index = usize::from(arena) * 4;
    let start = word(table, index)? as usize;
    ensure!(arena == 0 || start != 0, "stage {arena} has no package");
    let end = table
        .get(index + 4..)
        .context("stage directory has no range end")?
        .chunks_exact(4)
        .map(|row| word(row, 0))
        .find(|value| !matches!(value, Ok(0)))
        .context("stage directory has no range end")?? as usize;
    let size = end
        .checked_sub(start)
        .filter(|&n| n != 0)
        .context("invalid stage range")?;
    let size = size.checked_add(31).context("stage range overflow")? & !31;
    Ok(start..start.checked_add(size).context("stage range overflow")?)
}

/// Publish every physical arena. Unsupported presentation remains explicit in
/// its descriptor and the returned pending map; malformed packages still fail.
pub fn publish_all(
    extracted: &Path,
    output: &Path,
) -> Result<(Vec<String>, BTreeMap<u16, String>)> {
    publish_all_source(extracted, &Sources::read(extracted)?, output, "battle")
}

pub(crate) fn publish_all_source(
    extracted: &Path,
    sources: &Sources,
    output: &Path,
    prefix: &str,
) -> Result<(Vec<String>, BTreeMap<u16, String>)> {
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let table = module
        .at((5, 0x3b90))?
        .get(..0x188)
        .context("truncated stage directory")?;
    let mut paths = Vec::new();
    let mut pending = BTreeMap::new();
    for arena in 0..table.len() / 4 - 1 {
        if arena != 0 && word(table, arena * 4)? == 0 {
            continue;
        }
        let arena = u16::try_from(arena)?;
        let bytes = read_range(
            &extracted.join("files").join(&sources.stages),
            range(table, arena)?,
        )?;
        let archive = MapArchive::decode(&bytes)?;
        let path = format!("{prefix}/stages/{arena:03}.json");
        let stage = read(
            &archive,
            &format!("{prefix}/stages/{arena:03}.effects.json"),
            output,
        )?;
        if let Some(reason) = &stage.unsupported_reason {
            pending.insert(arena, reason.clone());
        }
        crate::write_atomic(&output.join(&path), &serde_json::to_vec(&stage)?)?;
        paths.push(path);
    }
    Ok((paths, pending))
}

fn read(archive: &MapArchive, effects_path: &str, output: &Path) -> Result<Stage> {
    ensure!(archive.sections.len() >= 11, "invalid stage section count");
    let data = archive.section(0)?;
    ensure!(data.len() >= 0x318, "truncated stage metadata");
    let recurring_interval = half(data, 0x2d4)?;
    let mut unsupported = Vec::new();
    let vector = |offset| -> Result<[f32; 3]> {
        Ok([
            float(data, offset)?,
            float(data, offset + 4)?,
            float(data, offset + 8)?,
        ])
    };
    let color = |offset| -> [u8; 4] { data[offset..offset + 4].try_into().unwrap() };
    let mut decoded = Package::default();
    let mut layers = BTreeMap::new();
    let mut scenery_bones = BTreeMap::new();
    let mut animation_rates = BTreeMap::new();
    let mut texture_animations = BTreeMap::<u8, Vec<TextureAnimation>>::new();
    let animation_start = usize::from(data[0x304]);
    let scenery_end = if animation_start == 0 {
        archive.sections.len()
    } else {
        animation_start
    };
    ensure!(
        (11..=archive.sections.len()).contains(&scenery_end)
            && scenery_end - 11 <= 32
            && (animation_start == 0 || scenery_end - 11 <= 15),
        "invalid stage scenery directory"
    );
    let primary = (scenery_end > 11)
        .then(|| {
            crate::battle_model::rig(archive.section(1)?, crate::battle_model::RigKind::Effect)
        })
        .transpose()?;
    // Four main layers precede effects and textures. Scenery models end where
    // the optional animation bank begins.
    for section in (1..5).chain(11..scenery_end) {
        let Some(model) = archive.optional_section(section) else {
            continue;
        };
        let slot = if section < 5 {
            section - 1
        } else {
            section - 7
        };
        let slot_key = u8::try_from(slot).context("too many stage layers")?;
        if section >= 11 {
            let name = format!("kk{:02x}", section - 11);
            let primary = primary.as_ref().context("scenery has no primary model")?;
            let bone = primary
                .skeleton
                .bones
                .iter()
                .position(|bone| bone.name == name)
                .with_context(|| format!("missing scenery attachment {name}"))?;
            scenery_bones.insert(slot_key, u16::try_from(bone)?);
        }
        let flags = if section < 5 {
            let settings = 0x2c + slot * 0xa8;
            let count = usize::from(data[settings + 0xa5]);
            ensure!(count <= 4, "too many stage texture animations");
            if count != 0 {
                let animations = (0..count)
                    .map(|index| texture_animation(&data[settings + index * 40..]))
                    .collect::<Result<_>>()?;
                texture_animations.insert(slot_key, animations);
            }
            data[settings + 0xa4]
        } else {
            0
        };
        if flags & !LAYER_FLAGS != 0 {
            unsupported.push(format!("layer {slot} rendering flags {flags:#04x}"));
        }
        let additive = flags & LAYER_ADDITIVE != 0;
        let animation_bytes = if section < 5 {
            archive.optional_section(section + 4)
        } else if animation_start == 0 {
            None
        } else {
            let clip = data[0x305 + section - 11];
            if clip == u8::MAX {
                None
            } else {
                Some(archive.section(animation_start + usize::from(clip))?)
            }
        };
        let animation = animation_bytes
            .map(|bytes| decoded.decode_animation(bytes, || crate::animation::read_member(bytes)))
            .transpose()?;
        if animation.is_some() {
            let rate = if section < 5 {
                float(data, 0x2c + slot * 0xa8 + 0xa0)?
            } else {
                SCENERY_FRAMES_PER_UPDATE
            };
            animation_rates.insert(slot_key, rate);
        }
        let mut parts = Vec::new();
        model_preview::layers(
            Layer {
                model,
                outline: None,
                animation: animation.as_deref(),
                attached_to: None,
                // Stage blend does not change the model's culling policy.
                additive: false,
            },
            &mut parts,
            &format!("battle/stage/{slot}"),
            output,
            &mut decoded,
        )?;
        let mut layer = parts.pop().context("missing stage model")?;
        ensure!(parts.is_empty(), "unexpected stage model layers");
        layer.additive = additive;
        layer.scene.resource = slot as u16;
        for material in &mut layer.scene.materials {
            // Fixed models use this depth mask and blend function.
            material.depth_write = slot == 0 || slot >= 3;
            material.blend = true;
        }
        layers.insert(slot_key, layer);
    }
    let mut files = crate::battle_model::files(layers.values().map(|p| &p.scene), output)?;
    let mut effect_textures = archive
        .optional_section(10)
        .map(|bytes| crate::battle_effect::art::textures_from_source(bytes, output))
        .transpose()?
        .map(|textures| BTreeMap::from([(9, textures)]))
        .unwrap_or_default();
    let mut effect_files = BTreeMap::new();
    for image in effect_textures
        .values()
        .flatten()
        .flat_map(|t| &t.effect.images)
    {
        effect_files.insert(
            image.path.clone(),
            File {
                sha256: crate::media::hash_file(&output.join(&image.path))?,
                bytes: std::fs::metadata(output.join(&image.path))?.len(),
                roles: [Role::Texture].into(),
            },
        );
    }
    files.extend(effect_files.clone());
    let effects = archive
        .optional_section(9)
        .map(|bytes| -> Result<_> {
            let (mut bank, strides) = crate::battle_effect::read_with_palettes(bytes)?;
            if recurring_interval != 0 {
                use resonance_content::battle_effect::{EffectOperation, visual::ParticleShape};
                match bank.program(1) {
                    Err(error) => unsupported.push(format!("recurring effects: {error:#}")),
                    Ok(program) => {
                        for event in program {
                            if let EffectOperation::Spawn { particle, .. } = event.operation
                                && let Ok(visual) =
                                    bank.actors[usize::from(particle)].particle_visual()
                                && let ParticleShape::Unsupported { reason } = &visual.geometry
                            {
                                unsupported.push(format!(
                                    "recurring effect particle {particle} geometry: {reason}"
                                ));
                            }
                        }
                    }
                }
            }
            crate::battle_effect::art::bind_palettes(&mut bank, &strides, &mut effect_textures)?;
            bank.art = Some(resonance_content::battle_effect::Art {
                source_sha256: archive.source_sha256.clone(),
                textures: effect_textures
                    .iter()
                    .map(|(&slot, textures)| {
                        (slot, textures.iter().map(|t| t.effect.clone()).collect())
                    })
                    .collect(),
                models: BTreeMap::new(),
                files: effect_files,
            });
            let bytes = serde_json::to_vec(&bank)?;
            crate::write_atomic(&output.join(effects_path), &bytes)?;
            files.insert(
                effects_path.into(),
                File {
                    sha256: crate::digest(&bytes),
                    bytes: bytes.len() as u64,
                    roles: [Role::Data].into(),
                },
            );
            Ok(effects_path.to_owned())
        })
        .transpose()?;
    let textures = effect_textures
        .into_values()
        .flatten()
        .map(|texture| Texture {
            images: texture.effect.images,
            sampler: texture.effect.sampler,
        })
        .collect();
    let fog = Fog {
        color: color(0x2fc),
        start: f32::from(half(data, 0x300)?),
        end: f32::from(half(data, 0x302)?),
    };
    let stage = Stage {
        source_sha256: archive.source_sha256.clone(),
        unsupported_reason: (!unsupported.is_empty()).then(|| unsupported.join("; ")),
        light_position: vector(0)?,
        chain_acceleration: vector(12)?,
        actor_color: color(24),
        translation: vector(28)?,
        yaw_degrees: float(data, 40)?,
        color: color(0x2f8),
        camera_pitch_offset: float(data, 0x314)?,
        layers,
        scenery_fade: (data[0x2d6] != 0 && !scenery_bones.is_empty()).then(|| SceneryFade {
            step: data[0x2d6],
            minimum_alpha: scenery_bones
                .keys()
                .map(|&slot| (slot, data[0x2d8 + usize::from(slot - 4)]))
                .collect(),
        }),
        scenery_bones,
        animation_rates,
        texture_animations,
        fog: (fog.end != 0.).then_some(fog),
        effects,
        recurring_interval: (recurring_interval != 0).then_some(recurring_interval),
        textures,
        files,
    };
    stage.validate()?;
    Ok(stage)
}

fn texture_animation(bytes: &[u8]) -> Result<TextureAnimation> {
    let motion = match bytes[0] {
        1 => TextureMotion::Atlas { frames: bytes[3] },
        2 => TextureMotion::Scroll {
            velocity: [float(bytes, 4)?, float(bytes, 8)?],
        },
        kind => anyhow::bail!("unknown stage texture motion {kind}"),
    };
    Ok(TextureAnimation {
        texture: usize::from(bytes[1]),
        interval: bytes[2].max(1),
        motion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_texture_channel_preserves_selector_and_rejects_unknown_motion() -> Result<()> {
        // Arena 3 keeps selector 4 even though its primary model has images 0–3.
        let mut bytes = [0; 40];
        bytes[..4].copy_from_slice(&[2, 4, 2, 0]);
        let animation = texture_animation(&bytes)?;
        assert_eq!(animation.texture, 4);
        assert_eq!(animation.interval, 2);
        bytes[0] = 0;
        assert!(texture_animation(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn selected_range_skips_holes_and_preserves_read_alignment() {
        let table = [0_u32, 64, 0, 0, 97, 160]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        assert_eq!(range(&table, 0).unwrap(), 0..64);
        assert_eq!(range(&table, 1).unwrap(), 64..128);
        assert!(range(&table, 2).is_err());
        assert!(range(&table, 5).is_err());
        assert!(range(&table, 128).is_err());
        let reversed = [0_u32, 64, 32]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        assert!(range(&reversed, 1).is_err());
    }

    #[test]
    #[ignore = "requires both extracted original discs; publishes only arena 13"]
    fn original_school_arena_preserves_models_metadata_and_dependencies() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let output = tempfile::tempdir()?;
            let path = publish(&local.join(format!("disc{disc}")), 13, output.path())?;
            let bytes = std::fs::read(output.path().join(path))?;
            let stage: Stage = serde_json::from_slice(&bytes)?;
            stage.validate()?;
            assert_eq!(
                stage.source_sha256,
                "783d4bc91d71c2e6caff80f0980fea9d8e1cbf4b850024ba365a973d311bcb3c"
            );
            assert_eq!(stage.layers.keys().copied().collect::<Vec<_>>(), [0]);
            assert_eq!(stage.light_position, [300., 800., -200.]);
            assert_eq!(stage.chain_acceleration, [0.; 3]);
            assert_eq!(stage.translation, [0.; 3]);
            assert_eq!(stage.yaw_degrees, -15.);
            assert_eq!(stage.camera_pitch_offset, -1.);
            assert_eq!(stage.actor_color, [64, 64, 64, 255]);
            assert_eq!(stage.color, [64, 64, 64, 255]);
            assert!(!stage.layers[&0].scene.materials.is_empty());
            assert!(
                stage.layers[&0]
                    .scene
                    .materials
                    .iter()
                    .all(|m| m.depth_write && m.blend)
            );
            assert!(stage.effects.is_some());
            assert!(!stage.textures.is_empty());
            for (path, file) in &stage.files {
                let bytes = std::fs::read(output.path().join(path))?;
                assert_eq!(file.sha256, crate::digest(&bytes));
                assert_eq!(file.bytes, bytes.len() as u64);
            }
            if let Some(previous) = &previous {
                assert_eq!(&bytes, previous);
            }
            previous = Some(bytes);
        }
        Ok(())
    }
}
