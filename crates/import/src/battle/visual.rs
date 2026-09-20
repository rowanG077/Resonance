//! Battle packages share the ordinary model, texture and skeletal decoders.
pub(in crate::battle) mod all;
mod attachments;
#[cfg(test)]
mod attack_groups_tests;
pub(super) mod binding;
#[cfg(test)]
mod body_geometry_tests;
mod costumes;
mod effects;
pub(super) use costumes::validate_martial;
#[cfg(test)]
mod guardian_arena_tests;
mod initial_pose;
pub(super) mod party;
mod pow_blade;
#[cfg(test)]
mod target_anchor_tests;
mod trail_bones;
mod trails;
#[cfg(test)]
mod variant_texture_tests;
mod victory;
use crate::{
    compression, digest,
    field::{MapArchive, sections},
    read::{f32 as float, u16 as half, u32 as word},
    scene::{PartSource, SourceClip, cook_part},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::effect_program::BattleEffectPrograms,
    battle::visual::{
        ArenaVisuals, BoneVolume, ModelVisuals, Rig, Shadow, TextureLayer, TrailVisual,
        VisualAssets, VolumeKind, WeaponMotionLink, WeaponVisuals,
    },
    model_preview::{ModelPreview, PreviewPart},
};
use std::{fs, path::Path};

#[cfg(test)]
pub(super) fn preflight_effect_models(
    extracted: &Path,
    effects: &BattleEffectPrograms,
) -> Result<()> {
    effects::preflight_magic(extracted, effects)
}

pub(crate) fn cook(
    extracted: &Path,
    output: &Path,
    arenas: &[u16],
    party: &[u8],
    enemies: &[u8],
    weapons: &[u16],
    effect_programs: &BattleEffectPrograms,
) -> Result<VisualAssets> {
    let disc = crate::disc_number(extracted)?;
    let sources = super::all::Sources::cooked(output, disc)?;
    let arenas = binding::arenas(output, disc, &sources, arenas)?;
    let enemy_visuals = binding::enemies(output, disc, &sources, enemies)?;
    let (toon_ramp, shadow_texture) = binding::textures(output, disc, &sources)?;
    let (titles, equipment_owners) = binding::party_metadata(output, disc)?;
    let weapons = binding::weapons(output, disc, &sources, weapons)?;
    let mut visuals = VisualAssets {
        pow_devastation: Default::default(),
        pow_weapons: binding::pow_weapons(output, disc, &sources, party)?,
        arenas,
        party: binding::party(
            output,
            disc,
            &sources,
            party,
            &titles,
            &weapons,
            &equipment_owners,
        )?,
        enemies: enemy_visuals,
        toon_ramp,
        shadow_texture,
        effect_models: binding::effect_packages(output, disc, &sources, effect_programs)?,
        weapons,
    };
    if let Some(pow) = visuals
        .pow_weapons
        .get(&resonance_content::battle::unison::PowWeapon::Devastation)
    {
        for (&item, original) in &visuals.weapons {
            let owners = weapon_owners(&equipment_owners, item);
            if owners & (1 << 6) != 0 {
                visuals
                    .pow_devastation
                    .insert(item, pow_blade::preserve_secondary(original, pow)?);
            }
        }
    }
    visuals.validate()?;
    Ok(visuals)
}

fn excluded_bones(files: &Path) -> Result<Vec<String>> {
    let relocated = super::actions::Rel::read(&files.join("US_r_Top2Btl.rel"))?;
    (0..6)
        .map(|i| {
            let bytes = relocated.at(relocated.pointer(4, 0x106c + i * 4)?)?;
            let end = bytes
                .iter()
                .position(|&b| b == 0)
                .context("unterminated bone classifier")?;
            Ok(std::str::from_utf8(&bytes[..end])?.to_owned())
        })
        .collect()
}

fn shadow_texture(directory: &[u8], output: &Path) -> Result<String> {
    let banks = sections(directory)?;
    let bank = &directory[banks[4].clone().context("missing battle texture bank")?];
    let textures = sections(bank)?;
    let tpl = &bank[textures[0]
        .clone()
        .context("missing battle common textures")?];
    // The loader binds this one-image TPL to runtime texture bank 10.
    let texture = crate::tpl::parse_tpl(tpl)?
        .into_iter()
        .next()
        .context("missing battle shadow texture")?;
    let rgba = crate::tpl::decode_texture(tpl, &texture)?;
    let png = crate::temporary_path(&output.join("intermediate/battle/shadow.png"));
    fs::create_dir_all(png.parent().unwrap())?;
    let path = "battle/shadow.ktx2";
    fs::create_dir_all(output.join("battle"))?;
    image::save_buffer(
        &png,
        &rgba,
        u32::from(texture.width),
        u32::from(texture.height),
        image::ColorType::Rgba8,
    )?;
    crate::texture::cook_png(&png, &output.join(path))?;
    fs::remove_file(png)?;
    Ok(path.into())
}

/// Read the REL's arena offset table, then decode only the selected cabinet.
/// Executable bytes never enter the cooked content.
fn arena_range(rel: &[u8], id: u16) -> Result<std::ops::Range<usize>> {
    indexed_range(rel, 0x3b90, 98, usize::from(id))
}

fn indexed_range(
    rel: &[u8],
    table: usize,
    offsets: usize,
    id: usize,
) -> Result<std::ops::Range<usize>> {
    let data = rel_data(rel)?;
    let table = data
        .get(table..table + offsets * 4)
        .context("missing battle arena catalog")?;
    ensure!(id + 1 < offsets, "resource ID exceeds catalog");
    let start = word(table, id * 4)? as usize;
    ensure!(start > 0 || id == 0, "missing indexed package");
    let end = (id + 1..offsets)
        .map(|i| word(table, i * 4))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .find(|&offset| offset != 0)
        .context("unterminated arena range")? as usize;
    ensure!(end > start, "reversed arena range");
    Ok(start..end)
}

fn rel_data(rel: &[u8]) -> Result<&[u8]> {
    const DATA_SECTION: usize = 5;
    ensure!(
        word(rel, 0)? == 1 && word(rel, 12)? > DATA_SECTION as u32,
        "unsupported battle module"
    );
    let at = word(rel, 16)? as usize + DATA_SECTION * 8;
    let start = (word(rel, at)? & !3) as usize;
    let size = word(rel, at + 4)? as usize;
    rel.get(start..start.checked_add(size).context("REL section overflow")?)
        .context("battle data section exceeds module")
}

fn weapon_owners(owners: &[u16], item: u16) -> u16 {
    // Battle-only models (IDs528+) have no equipment ownership record.
    owners.get(usize::from(item)).copied().unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn weapon(
    rel: &[u8],
    bytes: &[u8],
    executable: &[u8],
    files: &Path,
    id: u16,
    owners: u16,
    trails: &mut trails::Cooker,
    output: &Path,
) -> Result<WeaponVisuals> {
    const GENIS: u8 = 3;
    const SHEENA: u8 = 5;
    let mirrored = owners & (1 << (GENIS - 1)) != 0;
    let owner = mirrored
        .then(|| party::archive(executable, files, GENIS, 0))
        .transpose()?;
    let motion_link = owner.as_ref().map(costumes::weapon_link);
    let archive = weapon_archive(rel, bytes, id)?;
    let packages = weapon_packages(&archive)?;
    let mut slots = std::collections::BTreeMap::new();
    let mut rigs = std::collections::BTreeMap::new();
    let mut styles = std::collections::BTreeMap::new();
    for (slot, package) in packages {
        let ranges = sections(package)?;
        ensure!(
            (5..=7).contains(&ranges.len()),
            "unsupported weapon layer package"
        );
        let at = |index: usize| {
            ranges
                .get(index)
                .and_then(Option::as_ref)
                .map(|r| &package[r.clone()])
        };
        let mut parts = Vec::new();
        let clips = if let Some(owner) = &owner {
            costumes::weapon_clips(owner)
        } else {
            at(3)
                .map(|bytes| SourceClip {
                    slot: 0,
                    bytes,
                    resource: None,
                })
                .into_iter()
                .collect::<Vec<_>>()
        };
        let rig = rig(
            at(1).context("weapon model is missing")?,
            &clips,
            RigKind::Weapon,
        )
        .with_context(|| format!("weapon {id} slot {slot} rig"))?;
        let bones = attachments::trail_bones(&rig)?;
        if bones.len() >= 2 {
            styles.insert(
                slot,
                TrailVisual {
                    bones,
                    style: trails.cook(
                        trails::Recipe::PartyWeapon(at(0).context("weapon recipe is missing")?),
                        None,
                    )?,
                },
            );
        }
        rigs.insert(slot, rig);
        crate::model_preview::layers_with_clips(
            crate::model_preview::Layer {
                model: at(1).context("weapon model is missing")?,
                outline: at(2),
                animation: if mirrored { None } else { at(3) },
                attached_to: None,
                additive: false,
            },
            &mut parts,
            &format!("battle/weapons/{id}/{slot}"),
            if mirrored { &clips } else { &[] },
            output,
        )?;
        if let Some(model) = at(5) {
            crate::model_preview::layers(
                crate::model_preview::Layer {
                    model,
                    outline: None,
                    animation: at(6),
                    attached_to: None,
                    additive: false,
                },
                &mut parts,
                &format!("battle/weapons/{id}/{slot}"),
                output,
            )?;
        }
        slots.insert(
            slot,
            ModelPreview {
                scale: 1.,
                elevation: 0.,
                parts,
                hidden_geometry: Vec::new(),
                node_scales: Vec::new(),
            },
        );
    }
    let weapon = WeaponVisuals {
        source_sha256: archive.source_sha256,
        slots,
        rigs,
        trails: styles,
        motion_link,
    };
    if owners & (1 << (SHEENA - 1)) != 0 {
        // Each card package is carried at two independently animated body anchors.
        let count = rel_data(rel)?
            .get(0x3d30 + usize::from(SHEENA - 1) * 496 + 0x1e4)
            .copied()
            .context("missing carried card count")?;
        weapon.with_instances(&(0..count).map(|slot| slot >> 1).collect::<Vec<_>>())
    } else {
        Ok(weapon)
    }
}

fn weapon_archive(rel: &[u8], bytes: &[u8], id: u16) -> Result<MapArchive> {
    let index = match id {
        528.. => id - 379,
        356..=366 => id - 217,
        135..=355 => id - 135,
        _ => anyhow::bail!("invalid battle weapon {id}"),
    };
    MapArchive::decode(
        bytes
            .get(indexed_range(rel, 0x6e8, 158, usize::from(index))?)
            .context("weapon exceeds archive")?,
    )
}

fn weapon_packages(archive: &MapArchive) -> Result<Vec<(u8, &[u8])>> {
    let nested = sections(archive.section(0)?).is_ok();
    Ok(if nested {
        archive
            .sections
            .iter()
            .enumerate()
            .filter_map(|(slot, range)| {
                range
                    .as_ref()
                    .map(|r| (slot as u8, &archive.bytes[r.clone()]))
            })
            .collect::<Vec<_>>()
    } else {
        vec![(0, archive.bytes.as_slice())]
    })
}

fn arena(rel: &[u8], bytes: &[u8], id: u16, output: &Path) -> Result<ArenaVisuals> {
    let archive = MapArchive::decode(
        bytes
            .get(arena_range(rel, id)?)
            .context("arena exceeds background archive")?,
    )?;
    let settings = crate::texture_animation::ArenaSettings::read(archive.section(0)?)?;
    let mut parts = Vec::new();
    let mut animation_rates = Vec::new();
    let mut uv_channels = Vec::new();
    for index in 0..4 {
        let Some(source) = archive.optional_section(index + 1) else {
            continue;
        };
        let (scene, _, _) = cook_part(
            PartSource {
                name: &format!("battle/arenas/{id}/{index}"),
                source,
                resource: index as u16,
                draw_order: index as u32,
                depth_write: !matches!(index, 1 | 2),
                translation: [0.; 3],
                autoplay: archive.optional_section(index + 5),
                animation_slots: &[],
                clip_prefix: "arena",
                extra_clips: &[],
                shared_clips: &[],
                texture_animations: Vec::new(),
            },
            output,
        )
        .with_context(|| format!("arena {id} layer {index}"))?;
        parts.push(PreviewPart {
            animation: None,
            scene,
            attached_to: None,
            additive: settings.layers[index].additive(),
            uv_offsets: Vec::new(),
        });
        animation_rates.push(settings.layers[index].animation_rate()?);
        uv_channels.push(settings.layers[index].channels()?);
    }
    let scenery = settings.scenery(archive.sections.len())?;
    if !scenery.is_empty() {
        let skeleton = super::pose::rig_skeleton(archive.section(1)?)?;
        for binding in scenery {
            let Some(source) = archive.optional_section(binding.model) else {
                continue;
            };
            let slot = binding.model - 11;
            let tag = format!("kk0{slot:x}");
            let anchor = skeleton
                .bones
                .iter()
                .find(|bone| bone.name.contains(&tag))
                .with_context(|| format!("arena {id} scenery {slot} has no {tag} anchor"))?;
            let animation = binding
                .animation_section(archive.sections.len())?
                .and_then(|section| archive.optional_section(section));
            crate::model_preview::layers(
                crate::model_preview::Layer {
                    model: source,
                    outline: None,
                    animation,
                    attached_to: Some(anchor.name.clone()),
                    additive: false,
                },
                &mut parts,
                &format!("battle/arenas/{id}/scenery/{slot}"),
                output,
            )?;
            animation_rates.push(1.);
            uv_channels.push(Vec::new());
        }
    }
    let yaw_degrees = settings.yaw_degrees.finite()?;
    let camera_pitch_offset = settings.camera_pitch_offset.finite()?;
    let translation = settings.translation()?;
    let ambient = settings.ambient;
    let actor_ambient = Some(settings.actor_color[..3].try_into()?);
    let light_position = settings.light_position()?;
    Ok(ArenaVisuals {
        source_sha256: archive.source_sha256,
        model: ModelPreview {
            scale: 1.,
            elevation: 0.,
            parts,
            hidden_geometry: Vec::new(),
            node_scales: Vec::new(),
        },
        yaw_degrees,
        camera_pitch_offset,
        translation,
        animation_rates,
        uv_channels,
        ambient,
        actor_ambient,
        light_position,
    })
}

#[allow(clippy::too_many_arguments)]
fn party_model(
    rel: &[u8],
    victory_archive: &Path,
    id: u8,
    namespace: &str,
    model: &[u8],
    archive: &MapArchive,
    excluded: &[String],
    output: &Path,
) -> Result<ModelVisuals> {
    // Full-detail bodies retain the selected costume's independent motion archive.
    // The battle motion table starts at archive member 2; holes retain their IDs.
    let victory = victory::Package::open(victory_archive, id)?;
    let mut clips = archive
        .sections
        .iter()
        .enumerate()
        .skip(2)
        .filter_map(|(member, range)| {
            range.as_ref().map(|r| SourceClip {
                slot: (member - 2) as u16,
                bytes: &archive.bytes[r.clone()],
                resource: None,
            })
        })
        .collect::<Vec<_>>();
    clips.extend(victory.clips());
    let parts = crate::character::cook_parts(namespace, model, &clips, &[], output)?;
    let hidden_geometry = parts[0]
        .bone_names
        .iter()
        .filter(|name| name.starts_with("kk"))
        .cloned()
        .collect();
    let metadata = &rel_data(rel)?[0x3d30 + (usize::from(id) - 1) * 496..];
    let layers = texture_layers(metadata, Some(&model[sections(model)?[0].clone().unwrap()]))?;
    let rig = rig(
        &model[sections(model)?[0]
            .clone()
            .context("missing party primary model")?],
        &clips,
        RigKind::Actor,
    )?;
    let (volumes, bounds_joints) = body_geometry(&rig, float(metadata, 0x84)?, excluded)?;
    Ok(ModelVisuals {
        model_sha256: digest(model),
        animation_sha256: archive.source_sha256.clone(),
        authored_motions: Some(party::motion_table(archive)?),
        target_anchor: target_anchor(metadata, &rig)?,
        initial_pose: initial_pose::metadata(metadata, &rig)?,
        paired_body: None,
        rig,
        volumes,
        alpha: metadata[0x9b],
        shadow: Some(shadow(metadata)?),
        bounds_joints,
        texture_layers: layers,
        variant_texture: None,
        victory: victory.bindings,
        attachments: Default::default(),
        trails: Default::default(),
        model: ModelPreview {
            scale: 1.,
            elevation: 0.,
            hidden_geometry,
            node_scales: Vec::new(),
            parts: parts
                .into_iter()
                .map(|scene| PreviewPart {
                    animation: None,
                    scene,
                    attached_to: None,
                    additive: false,
                    uv_offsets: Vec::new(),
                })
                .collect(),
        },
    })
}

fn enemy_model(
    directory: &[u8],
    archive: &[u8],
    id: u8,
    excluded: &[String],
    trails: &mut trails::Cooker,
    output: &Path,
) -> Result<ModelVisuals> {
    let table = word(directory, 0x2c)? as usize;
    let start = word(directory, table + usize::from(id) * 4)? as usize;
    let end = word(directory, table + (usize::from(id) + 1) * 4)? as usize;
    let packed = archive.get(start..end).context("enemy exceeds archive")?;
    let bytes = compression::decode(packed)?;
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(&bytes, 4)?)..)
        .context("enemy metadata exceeds package")?;
    ensure!(metadata.len() >= 0x1e8, "truncated enemy metadata");
    let clips = enemy_clips(&bytes)?;
    let mut model = crate::monsters::preview::with_clips(
        &bytes,
        metadata,
        id,
        &format!("battle/enemies/{id:03}"),
        &clips,
        output,
    )?;
    crate::monsters::preview::publish(&model, metadata, id, output)?;
    let rig = rig(
        &bytes[word(&bytes, 0x18)? as usize..],
        &clips,
        RigKind::Actor,
    )?;
    let paired_body = initial_pose::paired(&bytes, metadata, &clips, &rig)?;
    let initial_pose = initial_pose::metadata(metadata, &rig)?;
    // Catalogue fit settings are not combat transforms. The battle constructor
    // sets all three model axes from the enemy descriptor's body scale.
    model.scale = float(metadata, 0x84)?;
    model.elevation = 0.;
    let attachments = attachments::cook(
        &bytes,
        metadata,
        &rig,
        &mut model,
        &format!("battle/enemies/{id:03}"),
        output,
        trails,
        enemy_atlas(&bytes, &format!("enemy-{id}"))?,
    )?;
    // fn_1_44388 installs KI body ribbons after carried-model initialization.
    // Body endpoints and the metadata material are independent of KK packages.
    let body_trails = trail_bones::cook(
        &rig.skeleton,
        metadata,
        trails,
        enemy_atlas(&bytes, &format!("enemy-{id}"))?,
    )?;
    let (volumes, bounds_joints) = body_geometry(&rig, float(metadata, 0x84)?, excluded)?;
    Ok(ModelVisuals {
        model_sha256: digest(packed),
        animation_sha256: digest(packed),
        authored_motions: None,
        target_anchor: target_anchor(metadata, &rig)?,
        initial_pose,
        paired_body,
        rig,
        volumes,
        alpha: metadata[0x9b],
        shadow: (word(metadata, 0x5c)? & 8 == 0)
            .then(|| shadow(metadata))
            .transpose()?,
        bounds_joints,
        texture_layers: texture_layers(metadata, None)?,
        variant_texture: variant_texture(metadata)?,
        victory: Default::default(),
        attachments,
        trails: body_trails,
        model,
    })
}

fn enemy_atlas<'a>(bytes: &'a [u8], key: &'a str) -> Result<Option<trails::Atlas<'a>>> {
    let offset = word(bytes, 0x1d0)? as usize;
    if offset == 0 {
        return Ok(None);
    }
    Ok(Some(trails::Atlas {
        kind: 2,
        key,
        bytes: bytes.get(offset..).context("enemy atlas exceeds package")?,
    }))
}

/// Collision labels and targeting indices belong to the source rig, not scene display names.
fn body_geometry(
    rig: &Rig,
    scale: f32,
    excluded: &[String],
) -> Result<(Vec<BoneVolume>, Vec<u16>)> {
    let names = rig
        .skeleton
        .bones
        .iter()
        .map(|bone| bone.name.clone())
        .collect::<Vec<_>>();
    Ok((volumes(&names, scale)?, bounds_joints(&names, excluded)?))
}

fn bounds_joints(bones: &[String], excluded: &[String]) -> Result<Vec<u16>> {
    bones
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let volume = match VolumeBone::parse(name) {
                Ok(volume) => volume,
                Err(error) => return Some(Err(error)),
            };
            let prefix = name.get(..2).unwrap_or("").to_ascii_uppercase();
            let eligible = match (volume, prefix.as_str()) {
                (Some(volume), _) => volume.0 & 0x210 == 0,
                (_, "AT" | "KK" | "PA" | "AB" | "EF" | "KI" | "NS") => false,
                _ => !excluded.iter().any(|pattern| name.contains(pattern)),
            };
            eligible.then(|| Ok(u16::try_from(index)?))
        })
        .collect()
}

fn target_anchor(
    metadata: &[u8],
    rig: &Rig,
) -> Result<resonance_content::battle::visual::TargetAnchor> {
    let selected = *metadata.get(0x51).context("missing battle target joint")?;
    let bone = (selected != 0).then_some(if usize::from(selected) < rig.skeleton.bones.len() {
        u16::from(selected)
    } else {
        0
    });
    let scale = float(metadata, 0x84)? * if bone.is_none() { 2. } else { 1. };
    let at = if bone.is_some() { 0x78 } else { 0x60 };
    let offset = [
        float(metadata, at)?,
        float(metadata, at + 4)?,
        float(metadata, at + 8)?,
    ]
    .map(|value| value * scale);
    Ok(resonance_content::battle::visual::TargetAnchor { bone, offset })
}

fn shadow(metadata: &[u8]) -> Result<Shadow> {
    Ok(Shadow {
        scale: float(metadata, 0x88)?,
        color: metadata[0x108..0x10b].try_into()?,
    })
}

/// A persistent body-atlas row is independent of the animated expression channels.
fn variant_texture(metadata: &[u8]) -> Result<Option<TextureLayer>> {
    let row = metadata
        .get(0xee..0xf0)
        .context("truncated enemy atlas variant")?;
    Ok((row[1] != 0).then_some(TextureLayer {
        texture: u16::from(row[0]),
        frames: row[1],
    }))
}

fn texture_layers(metadata: &[u8], primary: Option<&[u8]>) -> Result<Vec<TextureLayer>> {
    ensure!(metadata[0xce] <= 4, "too many battle atlas channels");
    (0..usize::from(metadata[0xce]))
        .map(|i| {
            let texture = if let Some(primary) = primary.filter(|_| i < 2) {
                primary[12 + i]
                    .checked_sub(1)
                    .context("missing battle atlas channel")?
            } else {
                metadata[0xd3 + i]
            };
            Ok(TextureLayer {
                texture: u16::from(texture),
                frames: metadata[0xcf + i],
            })
        })
        .collect()
}

enum RigKind {
    Actor,
    Weapon,
}

fn rig(resource: &[u8], clips: &[SourceClip<'_>], kind: RigKind) -> Result<Rig> {
    let source = super::pose::RigSource::read(resource)?;
    source.require_names()?;
    let mut rig = Rig {
        skeleton: source.skeleton,
        motions: clips
            .iter()
            .map(|clip| {
                source
                    .bindings
                    .motion(clip.bytes)
                    .map(|motion| (clip.slot, motion))
            })
            .collect::<Result<_>>()?,
        // Body contacts use a cleared twelve-slot table; an empty slot is authored data.
        attack_groups: match kind {
            RigKind::Actor => (0..12).map(|group| (group, Vec::new())).collect(),
            RigKind::Weapon => Default::default(),
        },
        effect_groups: Default::default(),
        weapon_bones: Default::default(),
    };
    for (index, bone) in rig.skeleton.bones.iter().enumerate() {
        let name = bone.name.as_bytes();
        let Some(prefix) = name.get(..2) else {
            continue;
        };
        let attack = prefix.eq_ignore_ascii_case(b"at");
        if matches!(kind, RigKind::Actor) {
            // Group membership is a flag in the classification word. MO/DM labels
            // can carry into that flag too; EF labels need not contain a digit.
            let flags = if prefix.eq_ignore_ascii_case(b"ef") {
                (i16::from(rig_tag_byte(&bone.name, 3)? as i8) + 0x250) as u16
            } else {
                VolumeBone::parse(&bone.name)?.map_or(0, |bone| bone.0)
            };
            if flags & 0x80 != 0 {
                rig.effect_groups
                    .entry((flags & 15) as u8)
                    .or_default()
                    .push(index as u16);
            }
        }
        if attack || prefix.eq_ignore_ascii_case(b"kk") {
            let column = if attack && matches!(kind, RigKind::Weapon) {
                2
            } else {
                3
            };
            let slot = rig_tag_byte(&bone.name, column)?.wrapping_sub(b'0') as i8;
            let slot = u8::try_from(slot)
                .with_context(|| format!("negative battle rig slot in {}", bone.name))?;
            if attack {
                rig.attack_groups
                    .entry(slot)
                    .or_default()
                    .push(index as u16);
            } else {
                rig.weapon_bones.insert(slot, index as u16);
            }
        }
    }
    rig.validate()?;
    Ok(rig)
}

fn rig_tag_byte(name: &str, column: usize) -> Result<u8> {
    let tag = name
        .as_bytes()
        .get(..=column)
        .with_context(|| format!("truncated battle rig tag {name}"))?;
    // Escaped source bytes are safe in a label suffix, but cannot be read as tags.
    ensure!(
        tag.is_ascii() && !tag.contains(&b'\\'),
        "escaped battle rig tag {name}"
    );
    Ok(tag[column])
}

/// The label suffix is byte arithmetic, not a decimal grammar. Carrying into
/// the flag bits matters: `mon_hair01` becomes 0x9e, with no collision sphere
/// and with the targeting-bounds exclusion bit set.
#[derive(Clone, Copy)]
struct VolumeBone(u16);

impl VolumeBone {
    fn parse(bone: &str) -> Result<Option<Self>> {
        let name = bone.as_bytes();
        let Some(prefix) = name.get(..2) else {
            return Ok(None);
        };
        let (base, extra) = if prefix.eq_ignore_ascii_case(b"mo") {
            (0x60, 0)
        } else if prefix.eq_ignore_ascii_case(b"dm") {
            (0x20, 0x200)
        } else {
            return Ok(None);
        };
        ensure!(name.is_ascii(), "non-ASCII battle volume bone {bone}");
        // Rig labels escape source bytes. Numeric-prefix escapes need decoding before
        // classification; suffix escapes have no effect on the original four-byte test.
        ensure!(
            !name.iter().take(4).any(|&b| b == b'\\'),
            "escaped battle volume prefix {bone}"
        );
        let mut value = name
            .get(2)
            .context("truncated battle volume bone")?
            .wrapping_sub(b'0');
        if let Some(&digit) = name.get(3).filter(|digit| digit.is_ascii_digit()) {
            value = value.wrapping_mul(10).wrapping_add(digit - b'0');
        }
        Ok(Some(Self((u16::from(value) + base) | extra)))
    }

    fn kind(self) -> Result<Option<VolumeKind>> {
        Ok(match self.0 & 0x60 {
            0 => None,
            0x20 => Some(VolumeKind::Hurt),
            0x60 => Some(VolumeKind::BodyAndHurt),
            _ => anyhow::bail!("body-only battle volume requires a collision consumer"),
        })
    }
}

/// Named model spheres become explicit data; runtime never parses bone labels.
fn volumes(bones: &[String], scale: f32) -> Result<Vec<BoneVolume>> {
    const RADIUS_UNIT: f32 = 10.;
    let mut volumes = Vec::new();
    for bone in bones {
        let Some(flags) = VolumeBone::parse(bone)? else {
            continue;
        };
        let Some(kind) = flags
            .kind()
            .with_context(|| format!("battle volume bone {bone}"))?
        else {
            continue;
        };
        volumes.push(BoneVolume {
            bone: bone.clone(),
            radius: f32::from(flags.0 & 15) * RADIUS_UNIT * scale,
            kind,
        });
    }
    Ok(volumes)
}

fn enemy_clips(bytes: &[u8]) -> Result<Vec<SourceClip<'_>>> {
    let count = word(bytes, 0x14)?.max(31) as usize;
    ensure!(
        count <= (0x160 - 0x20) / 4,
        "enemy motion count overlaps attachments"
    );
    let mut clips = Vec::new();
    for index in 0..count {
        let offset = word(bytes, 0x20 + index * 4)? as usize;
        if offset == 0 {
            continue;
        }
        let clip = bytes
            .get(offset..)
            .context("enemy motion exceeds package")?;
        ensure!(
            clip.starts_with(&0x007b7960u32.to_be_bytes()),
            "invalid enemy motion {index}"
        );
        clips.push(SourceClip {
            slot: index as u16,
            bytes: clip,
            resource: None,
        });
    }
    Ok(clips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires original battle arenas; metadata and texture headers only"]
    fn original_arena_uv_channels_preserve_authored_and_unused_selectors() -> Result<()> {
        use crate::texture_animation::{ArenaSettings, ArenaUvLayer, arena_layer, arena_records};
        use resonance_content::battle::visual::{ArenaUvChannel, ArenaUvMode};
        use std::io::{Read, Seek, SeekFrom};
        let mut first_disc_storage = None;
        let mut first_disc_settings = None;
        for disc in ["disc1", "disc2"] {
            let files = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../local/extracted")
                .join(disc)
                .join("files");
            let rel = fs::read(files.join("US_r_Top2Btl.rel"))?;
            let data = rel_data(&rel)?;
            let offsets = (0..98)
                .map(|i| word(data, 0x3b90 + i * 4))
                .collect::<Result<Vec<_>>>()?;
            let mut file = fs::File::open(files.join("BTL/BTLbg.dat"))?;
            let ranges = super::super::all::physical_ranges(&offsets, 0, file.metadata()?.len())?;
            assert_eq!(ranges.len(), 86);
            assert_eq!(u64::from(*offsets.last().unwrap()), file.metadata()?.len());
            let selectors: Vec<_> = offsets[..offsets.len() - 1]
                .iter()
                .enumerate()
                .filter_map(|(id, &start)| (id == 0 || start != 0).then_some(id as u16))
                .collect();
            assert_eq!(
                ranges.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
                selectors
            );
            let mut modes = [0; 4];
            let mut unused = Vec::new();
            let mut inactive_count = 0;
            let mut nonzero_inactive = Vec::new();
            let mut storage = Vec::new();
            let mut complete_settings = Vec::new();
            let mut sizes = std::collections::BTreeMap::<usize, usize>::new();
            let mut inactive_alpha = 0;
            let mut inactive_motion = 0;
            for (id, range) in ranges {
                file.seek(SeekFrom::Start(range.start as u64))?;
                let mut packed = vec![0; range.len()];
                file.read_exact(&mut packed)?;
                let archive = MapArchive::decode(&packed)?;
                let metadata = archive.section(0)?;
                let settings = ArenaSettings::read(metadata)?;
                let json = serde_json::to_value(&settings)?;
                assert_eq!(json["minimum_alpha"].as_array().unwrap().len(), 32);
                assert_eq!(json["motion_offsets"].as_array().unwrap().len(), 15);
                let settings: ArenaSettings = serde_json::from_value(json)?;
                assert_eq!(
                    settings.source_bytes()?,
                    metadata,
                    "arena {id} complete settings"
                );
                complete_settings.push(metadata.to_vec());
                *sizes.entry(metadata.len()).or_default() += 1;
                assert_eq!(
                    settings.yaw_degrees.finite()?.to_bits(),
                    word(metadata, 40)?
                );
                assert_eq!(
                    settings.camera_pitch_offset.finite()?.to_bits(),
                    word(metadata, 788)?
                );
                assert_eq!(
                    settings.light_position()?.map(f32::to_bits),
                    [word(metadata, 0)?, word(metadata, 4)?, word(metadata, 8)?]
                );
                assert_eq!(
                    settings.translation()?.map(f32::to_bits),
                    [
                        word(metadata, 28)?,
                        word(metadata, 32)?,
                        word(metadata, 36)?
                    ]
                );
                assert_eq!(settings.actor_color, metadata[24..28]);
                assert_eq!(settings.ambient, metadata[760..764]);
                let scenery = settings.scenery(archive.sections.len())?;
                let motion_start = usize::from(metadata[772]);
                let model_end = if motion_start == 0 {
                    archive.sections.len()
                } else {
                    motion_start
                };
                assert_eq!(scenery.len(), model_end - 11);
                for (slot, binding) in scenery.iter().enumerate() {
                    assert_eq!(binding.model, 11 + slot);
                    assert_eq!(settings.minimum_alpha[slot], metadata[728 + slot]);
                    if archive.optional_section(binding.model).is_none() {
                        continue;
                    }
                    // Original admitted scenery uses the same binding as the previous
                    // preparer. Synthetic tests separately prove other signed selectors.
                    let old_animation = if motion_start == 0 || (metadata[773 + slot] as i8) < 0 {
                        None
                    } else {
                        Some(motion_start + usize::from(metadata[773 + slot]))
                    };
                    assert_eq!(
                        binding.animation_section(archive.sections.len())?,
                        old_animation
                    );
                }
                inactive_alpha += settings.minimum_alpha[scenery.len()..]
                    .iter()
                    .filter(|&&value| value != 0)
                    .count();
                let active_motion = if motion_start == 0 { 0 } else { scenery.len() };
                inactive_motion += settings.motion_offsets[active_motion..]
                    .iter()
                    .filter(|&&value| value != 0)
                    .count();
                for layer in 0..4 {
                    let physical = arena_records(metadata, layer)?;
                    let json = serde_json::to_value(&physical)?;
                    assert_eq!(json["slots"].as_array().unwrap().len(), 4);
                    let physical: ArenaUvLayer = serde_json::from_value(json)?;
                    let start = 44 + layer * 168;
                    let source = &metadata[start..start + 168];
                    assert_eq!(physical.source_bytes()?, source);
                    storage.extend_from_slice(source);
                    assert_eq!(physical.active_count, source[165]);
                    let active = usize::from(physical.active_count);
                    for slot in active..4 {
                        inactive_count += 1;
                        if source[slot * 40..(slot + 1) * 40]
                            .iter()
                            .any(|&byte| byte != 0)
                        {
                            nonzero_inactive.push((id, layer, slot));
                        }
                    }
                    // Compare the shared projection against each native field, including
                    // unused finite operands, so existing serialized runtime rows stay exact.
                    let expected = source[..active * 40]
                        .chunks_exact(40)
                        .map(|row| -> Result<_> {
                            Ok(ArenaUvChannel {
                                mode: match row[0] {
                                    1 => ArenaUvMode::Frames,
                                    2 => ArenaUvMode::Scroll,
                                    3 => ArenaUvMode::Oscillate,
                                    _ => ArenaUvMode::Disabled,
                                },
                                texture: u16::from(row[1]),
                                interval: row[2],
                                frames: row[3],
                                speed: [float(row, 4)?, float(row, 8)?],
                                angular_speed: [float(row, 12)?, float(row, 16)?],
                                initial_tick: row[20],
                                initial_frame: row[21],
                                initial_offset: [float(row, 24)?, float(row, 28)?],
                                initial_angle: [float(row, 32)?, float(row, 36)?],
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let channels = arena_layer(metadata, layer)?;
                    assert_eq!(channels, settings.layers[layer].channels()?);
                    assert_eq!(settings.layers[layer].additive(), source[164] & 2 != 0);
                    assert_eq!(
                        settings.layers[layer].animation_rate()?.to_bits(),
                        word(source, 160)?
                    );
                    assert_eq!(
                        serde_json::to_vec(&channels)?,
                        serde_json::to_vec(&expected)?
                    );
                    assert_eq!(channels, physical.channels()?);
                    for channel in &channels {
                        modes[match channel.mode {
                            ArenaUvMode::Disabled => 0,
                            ArenaUvMode::Frames => 1,
                            ArenaUvMode::Scroll => 2,
                            ArenaUvMode::Oscillate => 3,
                        }] += 1;
                    }
                    if channels.is_empty() {
                        continue;
                    }
                    // Authored UV rows can outlive the optional model layer.
                    // Keep and sample those rows even when nothing draws them.
                    let texture_count = match archive.optional_section(layer + 1) {
                        Some(source) => {
                            let tpl = source
                                .get(word(source, 0)? as usize..word(source, 4)? as usize)
                                .context("arena texture bounds")?;
                            crate::tpl::parse_tpl(tpl)?.len()
                        }
                        None => 0,
                    };
                    for channel in channels {
                        // These are material selectors, not image references.
                        // The draw consumer never reads an unmatched channel.
                        if usize::from(channel.texture) >= texture_count {
                            unused.push((id, layer, channel.texture));
                        }
                        ensure!(
                            [0, 1, 2, 256, 36001]
                                .into_iter()
                                .all(|tick| channel.offset(tick).into_iter().all(f32::is_finite)),
                            "arena {id} layer {layer}: invalid sampled UV"
                        );
                    }
                }
            }
            assert_eq!(modes, [0, 30, 38, 0]);
            assert_eq!(inactive_count, 1308);
            assert_eq!(nonzero_inactive.len(), 16);
            for example in [
                (1, 0, 0),
                (2, 0, 0),
                (3, 1, 0),
                (13, 0, 0),
                (13, 1, 0),
                (13, 2, 0),
            ] {
                assert!(
                    nonzero_inactive.contains(&example),
                    "lost inactive arena row {example:?}"
                );
            }
            assert_eq!(storage.len(), 86 * 4 * 168);
            if let Some(first) = &first_disc_storage {
                assert_eq!(&storage, first);
            } else {
                first_disc_storage = Some(storage);
            }
            if let Some(first) = &first_disc_settings {
                assert_eq!(&complete_settings, first);
            } else {
                first_disc_settings = Some(complete_settings);
            }
            eprintln!(
                "{disc} arena metadata sizes={sizes:?}, nonzero inactive alpha={inactive_alpha}, nonzero inactive motion offsets={inactive_motion}"
            );
            assert!(
                unused.contains(&(3, 0, 4)),
                "original unused arena selector was lost"
            );
        }
        Ok(())
    }

    #[test]
    fn volume_label_arithmetic_controls_both_spheres_and_bounds() {
        let names =
            ["mo07_Bone_koshi", "mon_hair01", "Mo62", "DM3", "Bone_head"].map(str::to_owned);
        for name in [&names[1], &names[2]] {
            assert_eq!(VolumeBone::parse(name).unwrap().unwrap().0, 0x9e);
        }
        let spheres = volumes(&names, 0.5).unwrap();
        assert_eq!(spheres.len(), 2);
        assert_eq!(
            (&spheres[0].bone, spheres[0].kind, spheres[0].radius),
            (&names[0], VolumeKind::BodyAndHurt, 35.)
        );
        assert_eq!(
            (&spheres[1].bone, spheres[1].kind, spheres[1].radius),
            (&names[3], VolumeKind::Hurt, 15.)
        );
        assert_eq!(bounds_joints(&names, &[]).unwrap(), [0, 4]);
        assert!(volumes(&["mo".into()], 1.).is_err());
        assert!(volumes(&["moé".into()], 1.).is_err());
        assert!(
            volumes(&["DM32".into()], 1.)
                .unwrap_err()
                .to_string()
                .contains("DM32")
        );
    }

    #[test]
    #[ignore = "requires original extracted GameCube records; parses the rig without texture cooking"]
    fn original_undine_rig_keeps_hair_but_binds_only_the_authored_body_sphere() {
        let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
        let rel = fs::read(files.join("US_r_Top2Btl.rel")).unwrap();
        let text = (word(&rel, word(&rel, 16).unwrap() as usize + 8).unwrap() & !3) as usize;
        // Suffix byte3 selects the decimal pair; byte2 itself is not digit-checked.
        // The result narrows to u8 before adding the collision/bounds flags.
        for (offset, instruction) in [
            (0x1be18, 0x889e0003),
            (0x1be20, 0x2c000030),
            (0x1be28, 0x2c000039),
            (0x1be30, 0x887e0002),
            (0x1be38, 0x1c00000a),
            (0x1be44, 0x5400063e),
            (0x1be4c, 0x887e0002),
            (0x1be50, 0x3803ffd0),
            (0x1be54, 0x5400063e),
            (0x1be60, 0x38040060),
            (0x1bf0c, 0x38040020),
            (0x1bf10, 0x60000200),
        ] {
            assert_eq!(word(&rel, text + offset).unwrap(), instruction);
        }
        let usual = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
        let enemies = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
        let table = word(&usual, 0x2c).unwrap() as usize;
        let start = word(&usual, table + 195 * 4).unwrap() as usize;
        let end = word(&usual, table + 196 * 4).unwrap() as usize;
        let package = compression::decode(&enemies[start..end]).unwrap();
        let metadata = half(&package, 4).unwrap() as usize;
        let scale = float(&package, metadata + 0x84).unwrap();
        assert_eq!(scale, 1.);
        let rig = rig(
            &package[word(&package, 0x18).unwrap() as usize..],
            &[],
            RigKind::Actor,
        )
        .unwrap();
        let names = rig
            .skeleton
            .bones
            .iter()
            .map(|bone| bone.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 74);
        assert_eq!(names[7], "mon_hair01");
        assert_eq!(rig.attack_groups[&0], [45, 46, 47]);
        let spheres = volumes(&names, scale).unwrap();
        assert_eq!(spheres.len(), 1);
        assert_eq!(spheres[0].bone, names[1]);
        assert_eq!(spheres[0].bone, "mo07_Bone_koshi");
        assert_eq!(spheres[0].radius, 70.);
        assert_eq!(spheres[0].kind, VolumeKind::BodyAndHurt);
        let bounds = bounds_joints(&names, &[]).unwrap();
        assert!(bounds.contains(&1));
        let decorative = names
            .iter()
            .enumerate()
            .filter(|(_, name)| name.starts_with("mon_"))
            .collect::<Vec<_>>();
        assert_eq!(decorative.len(), 26);
        for (index, name) in decorative {
            assert_eq!(VolumeBone::parse(name).unwrap().unwrap().0, 0x9e);
            assert!(!bounds.contains(&(index as u16)));
        }
    }

    #[test]
    fn enemy_motion_indices_keep_holes_and_reject_non_animation_resources() {
        let mut bytes = vec![0; 0x240];
        bytes[0x14..0x18].copy_from_slice(&3u32.to_be_bytes());
        bytes[0x20..0x24].copy_from_slice(&0x200u32.to_be_bytes());
        bytes[0x28..0x2c].copy_from_slice(&0x220u32.to_be_bytes());
        for offset in [0x200, 0x220] {
            bytes[offset..offset + 4].copy_from_slice(&0x007b7960u32.to_be_bytes());
        }
        assert_eq!(
            enemy_clips(&bytes)
                .unwrap()
                .iter()
                .map(|c| c.slot)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        bytes[0x220] = 1;
        assert!(enemy_clips(&bytes).is_err());
        bytes[0x14..0x18].copy_from_slice(&81u32.to_be_bytes());
        assert!(enemy_clips(&bytes).is_err());
    }
}
