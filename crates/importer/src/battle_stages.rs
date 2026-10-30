//! One native battle-stage package directly into an editable arena and media.
use crate::{
    battle_effect_recipes, compression, dol, rel::Rel, world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip,
    battle_effect::{
        EffectTexture,
        declaration::Declaration,
        document::{Artwork, Definition as EffectDefinition},
        visual::ParticleTexture,
    },
    battle_stage::{
        Fog, TextureAnimation, TextureMotion,
        document::{Definition, Layer},
    },
    font::UiTexture,
    model_preview::PreviewPart,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use symphonia_constants::ArenaId;

fn texture_animation(bytes: &[u8]) -> Result<TextureAnimation> {
    let motion = match bytes[0] {
        1 => TextureMotion::Atlas { frames: bytes[3] },
        2 => TextureMotion::Scroll {
            velocity: [
                f32::from_bits(dol::u32(bytes, 4)?),
                f32::from_bits(dol::u32(bytes, 8)?),
            ],
        },
        kind => anyhow::bail!("unsupported stage texture motion {kind}"),
    };
    Ok(TextureAnimation {
        texture: usize::from(bytes[1]),
        interval: bytes[2].max(1),
        motion,
    })
}

fn stage_directory(root: &Path) -> Result<Vec<u8>> {
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    Ok(module.slice(5, 0x3b90, 0x188)?.to_vec())
}

fn native_stage(root: &Path, id: u16) -> Result<Vec<u8>> {
    ensure!(id <= 127, "invalid battle stage {id}");
    let disc = root.join("disc1/files");
    let table = stage_directory(root)?;
    let at = usize::from(id) * 4;
    ensure!(at + 8 <= table.len(), "battle stage {id} exceeds directory");
    let start = dol::u32(&table, at)? as usize;
    ensure!(id == 0 || start != 0, "battle stage {id} has no package");
    let end = table[at + 4..]
        .chunks_exact(4)
        .map(|row| dol::u32(row, 0))
        .find(|value| !matches!(value, Ok(0)))
        .context("stage directory has no range end")?? as usize;
    ensure!(end > start, "invalid battle-stage range");
    let size = (end - start + 31) & !31;
    let mut archive = fs::File::open(disc.join("BTL/BTLbg.dat"))?;
    archive.seek(SeekFrom::Start(start as u64))?;
    let mut packed = vec![0; size];
    archive.read_exact(&mut packed)?;
    compression::cabinet(&packed)
}

fn section(package: &[u8], index: usize) -> Result<Option<&[u8]>> {
    let count = dol::u32(package, 0)? as usize;
    ensure!(
        count >= 11 && count <= 64 && index < count,
        "invalid stage section table"
    );
    let start = dol::u32(package, 4 + index * 4)? as usize;
    if start == 0 {
        return Ok(None);
    }
    let end = (0..count)
        .map(|other| dol::u32(package, 4 + other * 4).map(|value| value as usize))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&other| other > start)
        .min()
        .unwrap_or(package.len());
    ensure!(
        start >= 4 + count * 4 && start < end,
        "invalid stage section {index}"
    );
    Ok(Some(
        package
            .get(start..end)
            .context("stage section exceeds package")?,
    ))
}

/// Convert the arena's local effect bank and atlas together. Program and
/// particle decoding is shared with the other native battle-effect banks.
fn stage_effect_bank(
    bank: &[u8],
    atlas: Option<&[u8]>,
    base: &str,
) -> Result<Vec<(String, Vec<u8>)>> {
    let bank = battle_effect_recipes::NativeBank::read(bank)
        .with_context(|| format!("arena effect bank header {:?}", &bank[..bank.len().min(48)]))?;
    let mut programs = (0..bank.program_count())
        .map(|index| bank.simple_program(index))
        .collect::<Result<Vec<_>>>()?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut textures = BTreeMap::new();
    let mut palette_capacities = Vec::new();
    if let Some(atlas) = atlas {
        let tpl = if atlas.starts_with(&[0, 0x20, 0xaf, 0x30]) {
            atlas.to_vec()
        } else {
            compression::decode(atlas)?
        };
        let count = dol::u32(&tpl, 4)? as usize;
        let table = dol::u32(&tpl, 8)? as usize;
        let mut entries = Vec::new();
        for atlas_index in 0..count {
            let descriptor = dol::u32(&tpl, table + atlas_index * 8)? as usize;
            let width = u32::from(dol::u16(&tpl, descriptor + 2)?);
            let height = u32::from(dol::u16(&tpl, descriptor)?);
            let capacity = world_texture::tpl_palette_capacity(&tpl, atlas_index)?;
            palette_capacities.push(capacity);
            let colors = world_texture::tpl_palette_entries(&tpl, atlas_index)?;
            let pages = if capacity == 0 {
                1
            } else {
                colors.div_ceil(capacity).max(1)
            };
            let mut images = Vec::new();
            let mut choices = Vec::new();
            for page in 0..pages {
                let (image, _) =
                    world_texture::tpl_texture_palette(&tpl, atlas_index, page * capacity)?;
                let path = if let Some((path, _)) = files.iter().find(|(_, old)| *old == image) {
                    path.clone()
                } else {
                    let path = format!("{base}/textures/atlas-9-{atlas_index}-palette-{page}.ktx2");
                    files.push((path.clone(), image));
                    path
                };
                let index = images
                    .iter()
                    .position(|item: &UiTexture| item.path == path)
                    .unwrap_or_else(|| {
                        images.push(UiTexture {
                            path,
                            width,
                            height,
                        });
                        images.len() - 1
                    });
                choices.push(Some(index));
            }
            if capacity == 0 {
                choices.resize(256, Some(0));
            } else {
                choices.resize(256, None);
            }
            entries.push(EffectTexture {
                images,
                sampler: world_texture::sampler(&tpl, descriptor)?,
                palette_sets: vec![choices],
            });
        }
        textures.insert(9, entries);
    }
    let (template_names, templates) =
        battle_effect_recipes::name_programs(&bank, &mut programs, &[], |actor, declaration| {
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas {
                    slot, palette_set, ..
                } = &mut visual.texture
            {
                if *slot == 9 {
                    ensure!(
                        textures.contains_key(slot),
                        "arena particle {actor} refers to a missing local atlas"
                    );
                    let stride = usize::from(bank.actor(actor)?[6]);
                    ensure!(
                        stride == 0
                            || palette_capacities
                                .iter()
                                .all(|&capacity| capacity == 0 || capacity == stride),
                        "arena particle {actor} uses an unconverted palette binding"
                    );
                    *palette_set = 0;
                } else {
                    ensure!(
                        *slot < 5 && *palette_set == 0,
                        "arena particle {actor} refers to an unsupported shared atlas {slot}"
                    );
                }
            }
            Ok(())
        })?;
    let definition = EffectDefinition {
        programs,
        template_names,
        templates,
        artwork: Some(Artwork {
            textures,
            models: BTreeMap::new(),
        }),
    };
    files.push((
        format!("{base}/effects.json"),
        resonance_asset_writer::json::encode(&definition)?,
    ));
    Ok(files)
}

fn convert(root: &Path, id: u16, name: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let package = native_stage(root, id)?;
    let data = section(&package, 0)?.context("stage metadata is missing")?;
    ensure!(data.len() >= 0x318, "truncated stage metadata");
    let count = dol::u32(&package, 0)? as usize;
    let animation_start = usize::from(data[0x304]);
    let scenery_end = if animation_start == 0 {
        count
    } else {
        animation_start
    };
    ensure!(
        (11..=count).contains(&scenery_end)
            && scenery_end - 11 <= 32
            && (animation_start == 0 || scenery_end - 11 <= 15),
        "stage {id} has an invalid scenery directory"
    );
    let effect_bank = section(&package, 9)?;
    let effect_atlas = section(&package, 10)?;
    ensure!(
        effect_bank.is_some() || effect_atlas.is_none(),
        "stage {id} has an atlas without an effect bank"
    );
    let base = format!("battle/arenas/{name}");
    let mut files = Vec::new();
    if let Some(bank) = effect_bank {
        files.extend(stage_effect_bank(bank, effect_atlas, &base)?);
    }
    let mut layers = Vec::new();
    let mut palettes: Vec<(Vec<u8>, Vec<String>, Vec<bool>)> = Vec::new();
    let mut palette_owner: Option<(&[u8], Vec<String>, Vec<bool>)> = None;
    let mut primary_joints = Vec::new();
    let mut foreground = 0;
    // Native section order is background, foregrounds, then scenery. The
    // document presents scenery before foregrounds so the foreground draws last.
    for index in std::iter::once(1).chain(11..scenery_end).chain(2..=4) {
        let Some(model) = section(&package, index)? else {
            continue;
        };
        let slot = if index < 5 { index - 1 } else { index - 7 };
        let layer_name = match slot {
            0 => "background".to_owned(),
            1..=3 => {
                foreground += 1;
                format!("foreground-{foreground}")
            }
            _ => format!("scenery-{:02}", slot - 3),
        };
        let owned_palette = dol::u32(model, 0)? != 0;
        if owned_palette {
            let start = dol::u32(model, 0)? as usize;
            let end = dol::u32(model, 4)? as usize;
            let tpl = model
                .get(start..end)
                .context("stage palette exceeds model")?;
            ensure!(
                dol::u32(tpl, 0)? == 0x0020_af30,
                "stage palette is not a TPL"
            );
            let (paths, alpha) = if let Some((_, paths, alpha)) =
                palettes.iter().find(|(seen, _, _)| seen == tpl)
            {
                (paths.clone(), alpha.clone())
            } else {
                let mut paths = Vec::new();
                let mut alpha = Vec::new();
                for texture_index in 0..dol::u32(tpl, 4)? as usize {
                    let (texture, transparent) = world_texture::tpl_texture(tpl, texture_index)?;
                    let path = if let Some((path, _)) = files
                        .iter()
                        .find(|(path, old)| path.contains("-texture-") && *old == texture)
                    {
                        path.clone()
                    } else {
                        let path = format!("{base}/{layer_name}-texture-{texture_index}.ktx2");
                        files.push((path.clone(), texture));
                        path
                    };
                    paths.push(path);
                    alpha.push(transparent);
                }
                palettes.push((tpl.to_vec(), paths.clone(), alpha.clone()));
                (paths, alpha)
            };
            palette_owner = Some((model, paths, alpha));
        }
        let (owner, paths, alpha) = palette_owner
            .as_ref()
            .context("stage layer has no palette")?;
        let source = if owned_palette {
            model.to_vec()
        } else {
            world_gpl::with_primary_palette(owner, model)?
        };
        let (mesh, mut scene) = world_gpl::static_model(&source, paths.clone(), alpha.clone())?;
        let existing_mesh = files
            .iter()
            .find(|(path, bytes)| {
                path.ends_with(".glb") && !path.ends_with("-motion.glb") && *bytes == mesh
            })
            .map(|(path, _)| path.clone());
        scene.mesh = existing_mesh
            .clone()
            .unwrap_or_else(|| format!("{base}/{layer_name}.glb"));
        scene.resource = slot as u16;
        for material in &mut scene.materials {
            material.blend = true;
            material.depth_write = slot == 0 || slot >= 3;
        }
        if slot == 0 {
            primary_joints = scene.bone_names.clone();
        }
        let animation = if index < 5 {
            section(&package, index + 4)?
        } else if animation_start != 0 && data[0x305 + index - 11] != u8::MAX {
            section(
                &package,
                animation_start + usize::from(data[0x305 + index - 11]),
            )?
        } else {
            None
        };
        let animation_rate = if let Some(animation) = animation {
            let path = format!("{base}/{layer_name}-motion.glb");
            files.push((path.clone(), world_motion::encode(animation, &mesh)?));
            scene.clips.push(SceneClip {
                motion: path,
                joints: BTreeMap::new(),
                resource_slot: 0,
                duration_seconds: 0.,
                animation_resource: None,
                pose_chains: Vec::new(),
            });
            scene.autoplay = true;
            let rate = if index < 5 {
                f32::from_bits(dol::u32(data, 0x2c + slot * 0xa8 + 0xa0)?)
            } else {
                0.5
            };
            (rate != 0.5).then_some(rate)
        } else {
            None
        };
        if existing_mesh.is_none() {
            files.push((scene.mesh.clone(), mesh));
        }
        let (flags, texture_animations) = if index < 5 {
            let settings = 0x2c + slot * 0xa8;
            let count = usize::from(data[settings + 0xa5]);
            ensure!(count <= 4, "stage {id} has too many texture motions");
            let animations = (0..count)
                .map(|at| texture_animation(&data[settings + at * 40..]))
                .collect::<Result<Vec<_>>>()?;
            (data[settings + 0xa4], animations)
        } else {
            (0, Vec::new())
        };
        ensure!(flags & !0b11 == 0, "stage {id} has unsupported layer flags");
        let joint = (slot >= 4).then(|| format!("kk{:02x}", index - 11));
        if let Some(name) = &joint {
            ensure!(
                primary_joints.contains(name),
                "stage {id} lacks scenery joint {name}"
            );
        }
        layers.push(Layer {
            name: layer_name,
            model: PreviewPart {
                scene,
                animation: animation.map(|_| 0),
                attached_to: None,
                additive: flags & 0b10 != 0,
                uv_offsets: Vec::new(),
            },
            foreground: (1..=3).contains(&slot),
            joint,
            minimum_alpha: (slot >= 4 && data[0x2d6] != 0).then_some(data[0x2d8 + slot - 4]),
            animation_rate,
            texture_animations,
        });
    }
    ensure!(
        !layers.is_empty() && layers[0].name == "background",
        "stage has no background"
    );
    let number = |at| -> Result<f32> { Ok(f32::from_bits(dol::u32(data, at)?)) };
    let vector = |at| -> Result<[f32; 3]> { Ok([number(at)?, number(at + 4)?, number(at + 8)?]) };
    let color = |at| -> Result<[u8; 4]> { Ok(data[at..at + 4].try_into()?) };
    let fog_end = dol::u16(data, 0x302)?;
    let recurring = dol::u16(data, 0x2d4)?;
    ensure!(
        recurring == 0 || effect_bank.is_some(),
        "stage {id} has recurring effects without a local bank"
    );
    let fog = if fog_end != 0 {
        Some(Fog {
            color: color(0x2fc)?,
            start: f32::from(dol::u16(data, 0x300)?),
            end: f32::from(fog_end),
        })
    } else {
        None
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
        effects: effect_bank.map(|_| format!("{base}/effects.json")),
        recurring_interval: (recurring != 0).then_some(recurring),
    };
    files.push((
        format!("{base}/arena.json"),
        resonance_asset_writer::json::encode(&definition)?,
    ));
    Ok(files)
}

/// Write the native arena directory's known names as editable ID mappings.
pub fn cook_battle_stage_mappings(root: &Path, library: &Path) -> Result<()> {
    let table = stage_directory(root)?;
    let physical = (0..table.len() / 4 - 1)
        .filter(|&id| id == 0 || dol::u32(&table, id * 4).is_ok_and(|at| at != 0))
        .collect::<Vec<_>>();
    let known = ArenaId::ALL
        .iter()
        .map(|id| usize::from(id.raw()))
        .collect::<Vec<_>>();
    ensure!(
        physical == known,
        "native arena directory differs from known arena IDs"
    );
    let mappings = ArenaId::ALL
        .iter()
        .map(|id| (id.raw().to_string(), id.resource_name().unwrap().to_owned()))
        .collect::<BTreeMap<_, _>>();
    let output = library.join("battle/arenas/mappings.json");
    ensure!(!output.exists(), "arena mappings already exist");
    crate::output::json(&output, &mappings)?;
    Ok(())
}

/// Convert one selected native arena using its known publication name.
pub fn cook_battle_stage(root: &Path, library: &Path, id: u16) -> Result<()> {
    let name = ArenaId::try_from(id)?
        .resource_name()
        .context("battle stage has no publication name")?;
    let files = convert(root, id, name)?;
    ensure!(
        files.iter().all(|(path, _)| !library.join(path).exists()),
        "battle stage already exists"
    );
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
