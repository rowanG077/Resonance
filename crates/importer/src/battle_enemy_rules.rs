//! Ordinary enemy decisions, contacts and shots from a native enemy package.
//!
//! Native variants without a direct editable rule remain explicitly gated.
use crate::{
    battle_enemy_media, battle_enemy_profile, battle_enemy_projectiles, battle_equipment, dol,
    enemy_package::EnemyPackages, rel::Rel,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AtlasChannel, SceneClip,
    animation::{Motion, Skeleton, Transform, euler_rotation},
    battle_action::{
        ActionHit, Condition, EnemyPredicate, EnemyRequirements, HitCondition, HitElement, Power,
        TargetPolicy,
    },
    battle_model::{
        AttachedModel, AttachmentPlacement,
        document::{Model, enemy::Secondary},
    },
    battle_projectile::DamageKind,
    battle_voice::Sound,
    model_preview::ModelPreview,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::MonsterId;

fn byte(data: &[u8], at: usize) -> Result<u8> {
    data.get(at).copied().context("truncated enemy byte")
}
fn half(data: &[u8], at: usize) -> Result<u16> {
    dol::u16(data, at)
}
fn signed(data: &[u8], at: usize) -> Result<i16> {
    Ok(half(data, at)? as i16)
}
fn word(data: &[u8], at: usize) -> Result<u32> {
    dol::u32(data, at)
}
fn float(data: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_bits(word(data, at)?))
}
fn rounded(value: f32) -> f32 {
    (value * 1000.).round() / 1000.
}

fn section(data: &[u8], field: usize) -> Result<&[u8]> {
    let start = usize::from(half(data, field)?);
    ensure!(start >= 0x1e8, "invalid enemy section");
    let mut end = data.len();
    for at in (4..20).step_by(2) {
        let other = usize::from(half(data, at)?);
        if other > start {
            end = end.min(other);
        }
    }
    for at in (0x18..0x1e8).step_by(4) {
        let other = word(data, at)? as usize;
        if other > start {
            end = end.min(other);
        }
    }
    data.get(start..end)
        .context("enemy section exceeds package")
}

fn joint_names(library: &Path, name: &str) -> Result<Vec<String>> {
    let path = library.join(format!("meshes/monsters/{name}/meshes/body.glb"));
    let glb =
        fs::read(&path).with_context(|| format!("read {} before enemy rules", path.display()))?;
    ensure!(
        glb.starts_with(b"glTF") && word_le(&glb, 16)? == 0x4e4f_534a,
        "invalid enemy body GLB"
    );
    let json_size = word_le(&glb, 12)? as usize;
    let document: Value =
        serde_json::from_slice(glb.get(20..20 + json_size).context("truncated body GLB")?)?;
    document["nodes"]
        .as_array()
        .context("body GLB has no nodes")?
        .iter()
        .map(|node| {
            Ok(node["name"]
                .as_str()
                .context("unnamed body joint")?
                .to_owned())
        })
        .collect()
}

fn word_le(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        data.get(at..at + 4)
            .context("truncated GLB word")?
            .try_into()?,
    ))
}

fn native_sound(encoded: u16) -> Option<Sound> {
    match encoded {
        0 => None,
        value if value & 0x8000 != 0 => Some(Sound::Stream(value & 0x7fff)),
        value => Some(Sound::Cue(value + 501)),
    }
}

pub(crate) fn artwork(package: &[u8], library: &Path, name: &str) -> Result<Value> {
    let model = format!("meshes/monsters/{name}/model.json");
    let preview: ModelPreview = serde_json::from_slice(&fs::read(library.join(&model))?)?;
    preview.validate_definition()?;
    let profile = section(package, 4)?;
    let secondary_body = if half(profile, 0xb4)? & 0x8400 != 0 {
        let layers = 1 + usize::from(word(package, 0x198)? != 0);
        let motion_offset = u16::from(byte(profile, 0xbd)?);
        let parts = preview
            .parts
            .iter()
            .filter(|part| (2..2 + layers as u16).contains(&part.scene.resource))
            .cloned()
            .collect::<Vec<_>>();
        ensure!(
            parts.len() == layers
                && parts
                    .iter()
                    .all(|part| part.animation == Some(motion_offset)),
            "enemy secondary body preview is incomplete"
        );
        Some(Secondary {
            model_slot: byte(profile, 0xbc)?,
            motion_offset,
            model: Model {
                attachment_nodes: BTreeMap::new(),
                independent_layers: Default::default(),
                layers: parts,
            },
        })
    } else {
        None
    };
    let secondary_end = 2 + secondary_body
        .as_ref()
        .map_or(0, |body| body.model.layers.len()) as u16;
    let attached_models = {
        let count = usize::from(byte(profile, 0x1e6)?);
        let start = word(package, 0x1d8)? as usize;
        ensure!(
            count == 0 || start >= 0x1e8,
            "enemy attached-model table is absent"
        );
        let rows = package
            .get(start..start + count * 40)
            .context("truncated attached-model table")?;
        let model_scale = float(profile, 0x84)?;
        rows.chunks_exact(40)
            .map(|row| {
                ensure!(
                    matches!(row[3], 0 | 0x10 | 0x70 | 0x80)
                        && row[0] < byte(profile, 0x1e8)?
                        && row[1] < 80,
                    "unsupported attached-model controller"
                );
                let vec3 = |at| -> Result<[f32; 3]> {
                    Ok([float(row, at)?, float(row, at + 4)?, float(row, at + 8)?])
                };
                let mut transform = Transform {
                    translation: vec3(4)?,
                    rotation: euler_rotation(vec3(16)?),
                    scale: vec3(28)?.map(|value| value * model_scale),
                };
                let placement = if row[3] & 0x80 != 0 {
                    ensure!(
                        byte(section(package, 12)?, 8)? == 16,
                        "unsupported independent attached model"
                    );
                    transform.translation = [0.; 3];
                    transform.rotation = euler_rotation([-90., 0., 0.]);
                    AttachmentPlacement::Independent
                } else {
                    AttachmentPlacement::Bone(row[2])
                };
                transform.validate()?;
                Ok(AttachedModel {
                    model_slot: row[0],
                    motion_slot: (row[1] != 0).then_some(row[1]),
                    placement,
                    visible: row[2] != 0,
                    additive: row[3] & 0x10 != 0,
                    inherit_tint: row[3] & 0x20 == 0,
                    wing_layers: row[3] & 0x40 != 0,
                    transform,
                })
            })
            .collect::<Result<Vec<_>>>()?
    };
    let body_glb = fs::read(library.join(format!("meshes/monsters/{name}/meshes/body.glb")))?;
    let mut attachment_nodes = BTreeMap::new();
    for bone in Skeleton::from_glb(&body_glb)?.bones {
        let bytes = bone.name.as_bytes();
        if bytes.len() >= 4 && bytes[..2].eq_ignore_ascii_case(b"kk") {
            let slot = bytes[3]
                .checked_sub(b'0')
                .context("invalid enemy kk slot")?;
            ensure!(slot < 8, "enemy kk slot exceeds attachment table");
            attachment_nodes.insert(slot, bone.name);
        }
    }
    let mut attachments = BTreeMap::new();
    for mut part in preview
        .parts
        .into_iter()
        .filter(|part| part.scene.resource >= secondary_end && part.attached_to.is_some())
    {
        let node = part.attached_to.as_deref().unwrap();
        let slot = node
            .strip_prefix("kk")
            .and_then(|suffix| suffix.get(..2))
            .and_then(|digits| digits.parse::<u8>().ok())
            .context("enemy attachment has no native kk slot")?;
        ensure!(
            attachment_nodes.get(&slot).is_some_and(|name| name == node),
            "enemy attachment slot {slot} has no matching body anchor"
        );
        let model = attachments.entry(slot).or_insert_with(|| Model {
            attachment_nodes: BTreeMap::new(),
            independent_layers: Default::default(),
            layers: Vec::new(),
        });
        if !model.layers.is_empty() {
            model.independent_layers.insert(part.scene.resource);
        }
        let layer = model.layers.len();
        let motion =
            format!("battle/enemies/{name}/attachment-{slot}-layer-{layer}-animation-0.glb");
        if part.scene.clips.iter().any(|clip| clip.resource_slot == 0) {
            part.animation = Some(0);
        } else if library.join(&motion).exists() {
            part.scene.clips.push(SceneClip {
                motion,
                joints: BTreeMap::new(),
                resource_slot: 0,
                duration_seconds: 0.,
                animation_resource: None,
                pose_chains: Vec::new(),
            });
            part.animation = Some(0);
        }
        model.layers.push(part);
    }
    let mut trails = BTreeMap::new();
    let mut uv_animation = BTreeMap::new();
    for (&slot, model) in &attachments {
        let carried = battle_enemy_media::member(package, 0x160 + usize::from(slot) * 4)?
            .context("enemy attachment package is missing")?;
        let sections = word(carried, 0)? as usize;
        ensure!(
            matches!(sections, 5 | 6),
            "invalid enemy attachment package"
        );
        let start = word(carried, 4)? as usize;
        ensure!(
            start >= 4 + sections * 4,
            "invalid enemy attachment metadata"
        );
        let end = (1..sections)
            .map(|index| word(carried, 4 + index * 4))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|offset| offset as usize)
            .filter(|&offset| offset > start)
            .min()
            .unwrap_or(carried.len());
        let metadata = carried
            .get(start..end)
            .filter(|data| data.len() >= 0x1c)
            .context("enemy attachment metadata is missing")?;
        let mesh = &model
            .layers
            .first()
            .context("enemy attachment has no layer")?
            .scene
            .mesh;
        let glb = fs::read(library.join(mesh))?;
        if let Some(trail) = battle_equipment::trail(&glb, metadata)? {
            trails.insert(slot, trail);
        }
        let mut animation = battle_equipment::texture_animation(metadata)?.unwrap_or_default();
        animation.effect_every_ticks = (metadata[10] != 0)
            .then(|| std::num::NonZeroU16::new(u16::from(metadata[10]) + 1).unwrap());
        if !animation.is_empty() {
            uv_animation.insert(slot, animation);
        }
    }
    Ok(serde_json::from_slice(&serde_json::to_vec(
        &resonance_content::battle_model::document::enemy::Definition {
            model,
            attachment_nodes,
            attachments,
            secondary_body,
            body_variant: (byte(profile, 0xef)? != 0).then_some(AtlasChannel {
                texture: usize::from(byte(profile, 0xee)?),
                frames: byte(profile, 0xef)?,
            }),
            attached_models,
            uv_animation,
            weapon_variants: battle_enemy_media::variant_artwork(package, library, name)?,
            trails,
        },
    )?)?)
}

fn pose_value(row: &[u8], blend: u8) -> Result<Value> {
    let rate = float(row, 8)?;
    Ok(json!({
        "clip":format!("slot-{}", byte(row, 2)?),
        "rate":rate,
        "blend":blend,
        "repeat":byte(row, 6)? & 0x40 != 0,
        "start":f32::from(byte(row, 4)?) * rate.abs(),
        "end":(byte(row, 5)? != 0).then(|| f32::from(row[5]) * rate.abs())
    }))
}

fn poses(data: &[u8], start: u16, library: &Path, name: &str) -> Result<Vec<(u16, Value, u8)>> {
    let source = section(data, 18)?;
    let body_motion_disabled = word(section(data, 4)?, 0x5c)? & 0x0800_0000 != 0;
    let mut cursor = usize::from(start) * 12;
    if source.get(cursor + 6..cursor + 8) == Some(&[0, 5])
        && source.get(cursor + 18..cursor + 20) == Some(&[1, 7])
    {
        let mut events = Vec::new();
        let mut delay = 0_u16;
        loop {
            if signed(source, cursor)? == -2 {
                break;
            }
            let first = source
                .get(cursor..cursor + 12)
                .context("truncated partial pose")?;
            let covered = source
                .get(cursor + 12..cursor + 24)
                .context("truncated covered pose")?;
            let body = source
                .get(cursor + 24..cursor + 36)
                .context("truncated body pose")?;
            ensure!(
                signed(first, 0)? >= 0
                    && first[0..2] == covered[0..2]
                    && first[0..2] == body[0..2]
                    && first[6..8] == [0, 5]
                    && covered[6..8] == [1, 7]
                    && body[6..8] == [6, 255]
                    && first[2] == covered[2]
                    && first[2] < 254
                    && body[2] < 254
                    && first[3] == 1
                    && covered[3] == 1
                    && body[3] > 0,
                "unsupported partial enemy pose group"
            );
            let at = (signed(first, 0)? as u16)
                .checked_add(delay)
                .context("partial pose time overflow")?;
            let body_at = at.checked_add(2).context("partial pose time overflow")?;
            events.push((
                at,
                json!({"partial_pose":{"first_bone":5,"pose":pose_value(first, 0)?}}),
                first[3],
            ));
            events.push((body_at, json!({"pose_keeping_parts":pose_value(body, if body[3] == 1 { 0 } else { body[3] })?}), body[3]));
            delay = delay
                .checked_add(3 + u16::from(body[3] - 1))
                .context("partial pose time overflow")?;
            cursor += 36;
        }
        return Ok(events);
    }
    let mut result: Vec<(u16, Value, u8)> = Vec::new();
    let mut animation_delay = 0_u16;
    loop {
        let row = source
            .get(cursor..cursor + 12)
            .context("truncated enemy pose")?;
        let at = signed(row, 0)?;
        if at == -2 && !result.is_empty() {
            break;
        }
        if at == -2 {
            ensure!(
                byte(row, 2)? < 254
                    && matches!(byte(row, 6)? & !0x40, 0 | 2)
                    && byte(row, 7)? != 255,
                "special initial enemy pose: {row:02x?}"
            );
            result.push((
                0,
                json!({"partial_pose":{"first_bone":byte(row, 7)?,"pose":pose_value(row, byte(row, 3)?)?}}),
                byte(row, 3)?,
            ));
            break;
        }
        let at = if result.is_empty() {
            0
        } else if at == -3 {
            let (previous_at, previous, previous_blend) = result.last().unwrap();
            let pose = previous
                .get("pose")
                .context("enemy pose continuation needs a body motion")?;
            let clip = pose["clip"]
                .as_str()
                .and_then(|clip| clip.strip_prefix("slot-"))
                .context("enemy pose continuation needs a numbered motion")?
                .parse::<u16>()?;
            let rate = pose["rate"].as_f64().context("enemy pose rate is absent")? as f32;
            ensure!(
                rate > 0. && !pose["repeat"].as_bool().unwrap_or(true),
                "enemy pose continuation needs a finite forward motion"
            );
            let start = pose["start"]
                .as_f64()
                .context("enemy pose start is absent")? as f32;
            let end = if let Some(end) = pose["end"].as_f64() {
                end as f32
            } else {
                let preview: ModelPreview = serde_json::from_slice(&fs::read(
                    library.join(format!("meshes/monsters/{name}/model.json")),
                )?)?;
                let motion = preview
                    .parts
                    .iter()
                    .find(|part| part.scene.resource == 0)
                    .and_then(|part| {
                        part.scene
                            .clips
                            .iter()
                            .find(|motion| motion.resource_slot == clip)
                    })
                    .context("missing continued enemy motion")?;
                Motion::decode(&fs::read(library.join(&motion.motion))?)?.duration_frames
            };
            let duration = (end - start) / rate;
            ensure!(
                duration.is_finite() && duration >= 0. && duration < f32::from(u16::MAX),
                "invalid continued enemy motion length"
            );
            previous_at
                .checked_add(duration.floor() as u16)
                .and_then(|at| {
                    at.checked_add(1 + u16::from(result.len() > 1 && *previous_blend > 0))
                })
                .context("enemy pose continuation exceeds timeline limit")?
        } else {
            ensure!(at >= 0, "special enemy pose control: {row:02x?}");
            (at as u16)
                .checked_add(animation_delay)
                .context("enemy pose time overflow")?
        };
        if byte(row, 2)? == 254 {
            ensure!(
                row[3..8].iter().all(|&value| value == 0),
                "special enemy animation rate: {row:02x?}"
            );
            result.push((at, json!({"animation_rate":float(row, 8)?}), 0));
            cursor += 12;
            continue;
        }
        if matches!(byte(row, 6)? & !0x40, 0 | 2) && byte(row, 7)? != 255 {
            ensure!(
                result.is_empty() && signed(source, cursor + 12)? == -2,
                "unsupported multi-row partial enemy pose"
            );
            result.push((at, json!({"partial_pose":{"first_bone":byte(row, 7)?,"pose":pose_value(row, byte(row, 3)?)?}}), byte(row, 3)?));
            break;
        }
        ensure!(
            byte(row, 6)? & !0x40 == 8 && matches!(byte(row, 7)?, 0 | 255),
            "special enemy pose needs a decoder: {row:02x?}"
        );
        let clip = byte(row, 2)?;
        let member = 0x20 + usize::from(clip) * 4;
        ensure!(member < 0x1dc, "enemy pose exceeds motion table");
        let blend = if word(data, member)? == 0 || body_motion_disabled && clip < 30 {
            0
        } else {
            byte(row, 3)?
        };
        result.push((at, json!({"pose":pose_value(row, blend)?}), blend));
        if result.len() > 1 && blend > 0 {
            animation_delay += 1;
        }
        cursor += 12;
    }
    Ok(result)
}

fn elapsed(at: u16, holds: &[(u16, u8)]) -> u16 {
    at + holds
        .iter()
        .filter(|(start, _)| *start < at)
        .map(|(_, ticks)| u16::from(*ticks))
        .sum::<u16>()
}

fn physical_feedback(data: &[u8], hit: &mut ActionHit) -> Result<()> {
    let Some(effect) = hit.effect else {
        return Ok(());
    };
    // Shared hit rules may address zero-program aliases beyond the declared
    // member; verify those against the original bounded package tail.
    let start = word(data, 0x1cc)? as usize;
    ensure!(start >= 0x1e8, "physical contact has no enemy effect bank");
    let bank = data
        .get(start..)
        .context("physical effect bank exceeds package")?;
    if effect < u16::from(byte(bank, 4)?) {
        return Ok(());
    }
    let table = usize::from(half(bank, 16)?);
    let offset = signed(bank, table + usize::from(effect) * 2)?;
    ensure!(
        offset == signed(bank, table)?,
        "physical contact effect is not an empty alias"
    );
    let program = usize::try_from(i32::from(half(bank, 10)?) + i32::from(offset))?;
    ensure!(
        bank.get(program..program + 6) == Some(&[0, 0, 254, 0, 0, 0]),
        "physical contact aliases a nonempty effect"
    );
    hit.effect = Some(0);
    Ok(())
}

fn commands(
    data: &[u8],
    start: u16,
    mut deadline: u16,
    holds: &[(u16, u8)],
    opening_hold: u16,
) -> Result<(Vec<(u16, Value)>, u16)> {
    let source = section(data, 14)?;
    let mut cursor = usize::from(start) * 2;
    let mut events = Vec::new();
    let mut reached = 0_u16;
    loop {
        let authored_at = signed(source, cursor)?;
        if authored_at == -1 {
            break;
        }
        let at = u16::try_from(authored_at)
            .context("enemy command loop needs a decoder")?
            .max(reached);
        reached = at;
        let opcode = signed(source, cursor + 2)?;
        if opcode == 17 {
            let extension = half(source, cursor + 4)?;
            ensure!(
                extension <= i16::MAX as u16
                    && extension == half(source, cursor + 6)?
                    && at <= deadline,
                "enemy deadline extension needs a decoder"
            );
            deadline = deadline
                .checked_add(extension)
                .context("enemy deadline extension overflow")?;
            cursor += 8;
            continue;
        }
        let (event, width) = match opcode {
            -5 | -3 => {
                let start = at;
                ensure!(start <= deadline, "enemy hit wait exceeds its deadline");
                let end = deadline
                    .checked_add(elapsed(start, holds) - start)
                    .context("enemy hit wait deadline overflow")?;
                let event = if opcode == -5 {
                    json!({"wait_for_effective_hit":{"deadline":end}})
                } else {
                    json!({"wait_for_hit":{"deadline":end}})
                };
                (event, 4)
            }
            0 => (
                json!({"forward":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            1 => (
                json!({"vertical":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            2 => (
                json!({"acceleration":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            3 => (
                json!({"gravity":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            5 => {
                let enabled = half(source, cursor + 4)?;
                ensure!(enabled <= 1, "enemy body collision mode needs a decoder");
                let duration = u8::try_from(half(source, cursor + 6)?)?;
                (
                    json!({"pass_through":{"enabled":enabled != 0,"duration":duration}}),
                    8,
                )
            }
            7 => {
                let [slot, visible] = half(source, cursor + 4)?.to_be_bytes();
                ensure!(slot < 8 && visible <= 1, "invalid enemy weapon visibility");
                let profile = section(data, 4)?;
                if slot >= byte(profile, 0x1e4)? && word(data, 0x160 + usize::from(slot) * 4)? == 0
                {
                    cursor += 6;
                    continue;
                }
                (
                    json!({"weapon_visibility":{"slot":slot,"visible":visible != 0}}),
                    6,
                )
            }
            9 => (
                json!({"braking":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            12 => {
                let enabled = half(source, cursor + 4)?;
                let duration = half(source, cursor + 6)?;
                ensure!(
                    (enabled, duration) == (0, 0)
                        || (enabled == 1 && (1..=i16::MAX as u16).contains(&duration)),
                    "enemy reaction armor mode needs a decoder"
                );
                (json!({"reaction_armor":duration}), 8)
            }
            13 => (
                json!({"trail":{"slot":u8::try_from(half(source, cursor + 4)?)?,"duration":half(source, cursor + 6)?}}),
                8,
            ),
            14 => {
                let amount = signed(source, cursor + 6)?;
                let mask = (u64::from(word(source, cursor + 8)?) << 32)
                    | u64::from(word(source, cursor + 12)?);
                let event = match mask {
                    4 => json!("stun"),
                    0x1000 | 0x2000 if amount >= 0 => json!({"stat_up":{
                        "condition":if mask == 0x1000 { "attack_up" } else { "defense_up" },
                        "amount":amount
                    }}),
                    _ => anyhow::bail!("enemy condition command needs a decoder"),
                };
                (event, 16)
            }
            18 => {
                ensure!(
                    half(source, cursor + 4)? == 0,
                    "enemy captured release mode needs a decoder"
                );
                let hitstun = half(source, cursor + 6)?;
                ensure!(
                    hitstun <= i16::MAX as u16,
                    "enemy captured hitstun needs a decoder"
                );
                (json!({"release_captured":{"hitstun":hitstun}}), 8)
            }
            19 => {
                let visible = half(source, cursor + 4)?;
                ensure!(
                    visible <= 1 && half(source, cursor + 6)? == 0,
                    "enemy captured visibility mode needs a decoder"
                );
                (json!({"captured_visibility":visible != 0}), 8)
            }
            20 => (
                json!({"captured_forward":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            21 => (
                json!({"captured_vertical":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            23 => {
                let enabled = half(source, cursor + 4)?;
                let duration = half(source, cursor + 6)?;
                ensure!(
                    enabled <= 1 && duration <= i16::MAX as u16,
                    "invalid enemy secondary wind"
                );
                (
                    json!({"secondary_wind":{"enabled":enabled != 0,"duration":duration}}),
                    8,
                )
            }
            25 => {
                let rate = float(source, cursor + 8)?;
                let repeat = word(source, cursor + 12)?;
                ensure!(rate > 0. && repeat <= 1, "invalid enemy carried pose");
                (
                    json!({"carried_pose":{
                        "slot":u8::try_from(half(source, cursor + 4)?)?,
                        "clip":half(source, cursor + 6)?,
                        "rate":rate,
                        "repeat":repeat != 0
                    }}),
                    16,
                )
            }
            26 => {
                ensure!(
                    half(source, cursor + 6)? == 1,
                    "enemy common effect attachment needs a decoder"
                );
                (json!({"common_effect":half(source, cursor + 4)?}), 8)
            }
            27 => {
                let priority = half(source, cursor + 6)?;
                ensure!(
                    matches!(priority, 2 | 3),
                    "enemy voice priority needs a decoder"
                );
                let encoded = half(source, cursor + 4)?;
                let sound = if encoded == 0 {
                    Value::Null
                } else if encoded & 0x8000 != 0 {
                    json!({"stream":encoded & 0x7fff})
                } else {
                    json!({"cue":encoded + 501})
                };
                (
                    json!({"voice":{"sound":sound,"announcement":priority == 3}}),
                    8,
                )
            }
            28 => {
                ensure!(
                    half(source, cursor + 6)? == 0,
                    "positional enemy sound needs a decoder"
                );
                (json!({"sound":half(source, cursor + 4)?}), 8)
            }
            29 => {
                ensure!(
                    matches!(half(source, cursor + 10)?, 0 | 64 | 255),
                    "enemy tint mode needs a decoder"
                );
                let channels = [4, 6, 8]
                    .map(|at| -> Result<u8> { Ok(u8::try_from(half(source, cursor + at)?)?) })
                    .into_iter()
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                (json!({"tint":channels}), 12)
            }
            // Native command 31 marks the actor's complete reward row excluded.
            31 => (json!({"defeat":{"rewards":false}}), 4),
            38 | 39 => {
                ensure!(
                    half(source, cursor + 6)? == 0,
                    "enemy spell release modifier needs a decoder"
                );
                (
                    json!({"release_spell":{
                        "technique":half(source, cursor + 4)?,
                        "slot":if opcode == 38 { "primary" } else { "secondary" }
                    }}),
                    8,
                )
            }
            41 => (
                json!({"turn_motion":f32::from(signed(source, cursor + 4)?)/10.}),
                6,
            ),
            43 | 44 => {
                ensure!(
                    half(source, cursor + 6)? == 0,
                    "enemy item modifier needs a decoder"
                );
                (
                    json!({"use_item":{"item":half(source, cursor + 4)?,"party":opcode == 44}}),
                    8,
                )
            }
            45 => {
                ensure!(
                    half(source, cursor + 10)? == 0,
                    "enemy position mode needs a decoder"
                );
                (
                    json!({"set_position":[signed(source, cursor + 4)?,signed(source, cursor + 6)?,signed(source, cursor + 8)?]}),
                    12,
                )
            }
            46 => {
                ensure!(
                    half(source, cursor + 4)? == 0 && half(source, cursor + 10)? == 0,
                    "enemy target position mode needs a decoder"
                );
                (
                    json!({"position_from_target":{
                        "height":signed(source, cursor + 6)?,
                        "retreat":signed(source, cursor + 8)?
                    }}),
                    12,
                )
            }
            47 => (
                json!({"offset_position":{
                    "height":signed(source, cursor + 6)?,
                    "retreat":signed(source, cursor + 8)?
                }}),
                12,
            ),
            48 => {
                ensure!(
                    half(source, cursor + 4)? == 0,
                    "enemy facing mode needs a decoder"
                );
                (json!("face_target"), 6)
            }
            _ => anyhow::bail!("enemy command {opcode} needs a decoder"),
        };
        let tick = if at == 0 && matches!(opcode, 0 | 1 | 2 | 3 | 5 | 12 | 25 | 26 | 29) {
            opening_hold
        } else {
            elapsed(at, holds)
        };
        events.push((tick, event));
        cursor += width;
    }
    Ok((events, deadline))
}

fn contact_recovery(data: &[u8], row: &[u8]) -> Result<Option<Value>> {
    if word(row, 8)? & 0x100 == 0 {
        return Ok(None);
    }
    let profile = section(data, 4)?;
    let expressions = profile
        .get(0xd7..0xdb)
        .context("truncated enemy expressions")?;
    let pose = if byte(row, 53)? == 0 {
        Value::Null
    } else {
        json!({"clip":format!("slot-{}", byte(row, 53)?)})
    };
    let mut movement: [Option<f32>; 2] = [None, None];
    let mut braking = None;
    let mut pass_through_ticks = 0;
    let mut voice = None;
    let source = section(data, 14)?;
    let mut cursor = usize::from(half(row, 54)?) * 2;
    if cursor != 0 {
        loop {
            let at = signed(source, cursor)?;
            if at == -1 {
                break;
            }
            ensure!(at == 0, "delayed enemy contact recovery needs a decoder");
            let opcode = half(source, cursor + 2)?;
            match opcode {
                0 | 1 => {
                    movement[usize::from(opcode)] =
                        Some(f32::from(signed(source, cursor + 4)?) / 10.);
                    cursor += 6;
                }
                9 => {
                    braking = Some(f32::from(signed(source, cursor + 4)?) / 10.);
                    cursor += 6;
                }
                5 => {
                    ensure!(
                        half(source, cursor + 4)? == 1,
                        "enemy contact recovery collision mode needs a decoder"
                    );
                    pass_through_ticks = u8::try_from(half(source, cursor + 6)?)?;
                    ensure!(
                        pass_through_ticks > 0,
                        "untimed enemy recovery collision bypass needs a decoder"
                    );
                    cursor += 8;
                }
                27 => {
                    ensure!(
                        half(source, cursor + 6)? == 2,
                        "enemy recovery voice priority needs a decoder"
                    );
                    voice = sound(half(source, cursor + 4)?);
                    cursor += 8;
                }
                _ => anyhow::bail!("enemy contact recovery command {opcode} needs a decoder"),
            }
        }
    }
    Ok(Some(json!({
        "pose":pose,
        "movement":movement,
        "braking":braking,
        "pass_through_ticks":pass_through_ticks,
        "voice":voice,
        "expressions":expressions
    })))
}

fn intern(definitions: &mut serde_json::Map<String, Value>, kind: &str, value: Value) -> String {
    if let Some((name, _)) = definitions.iter().find(|(_, old)| **old == value) {
        return name.clone();
    }
    let name = format!("{kind}-{}", definitions.len() + 1);
    definitions.insert(name.clone(), value);
    name
}

fn requirements(row: &[u8], flags: u32) -> Result<EnemyRequirements> {
    let count = byte(row, 51)?;
    let target = half(row, 12)?;
    const ELIGIBILITY: u32 = 1
        | 2
        | 4
        | 8
        | 0x10
        | 0x4000
        | 0x800
        | 0x2000
        | 0x8000
        | 0x10000
        | 0x20000
        | 0x80000
        | 0x100000
        | 0x200000
        | 0x400000
        | 0x800000
        | 0x1000000
        | 0x10000000
        | 0x20000000
        | 0x40000000
        | 0x80000000;
    ensure!(
        flags & !(ELIGIBILITY | 0x20 | 0x40 | 0x100 | 0x200 | 0x0200_0000) == 0,
        "unknown enemy eligibility flags {flags:#x}"
    );
    ensure!(
        flags & (0x800000 | 0x1000000) == 0 || count == 0,
        "nonzero nearby or targeting count needs a decoder"
    );
    ensure!(
        flags & 0x10000 == 0 || matches!(target, 3 | 5),
        "enemy target state {target} needs a decoder"
    );
    let mut predicates = Vec::new();
    if flags & 0x800 != 0 {
        predicates.push(EnemyPredicate::AllyAbsent(u16::from(count)));
    }
    if flags & 0x40000000 != 0 {
        predicates.push(EnemyPredicate::DoNotRepeat);
    }
    if flags & 0x90000000 != 0 {
        ensure!(target <= i16::MAX as u16, "negative origin distance");
        predicates.push(EnemyPredicate::OriginDistance {
            minimum: (flags & 0x10000000 != 0).then_some(target),
            maximum: (flags & 0x80000000 != 0).then_some(target),
        });
    }
    if flags & 0xa000 != 0 {
        ensure!(count <= i8::MAX as u8, "negative opponent count");
        predicates.push(EnemyPredicate::OpponentCount {
            minimum: (flags & 0x2000 != 0).then_some(count),
            maximum: (flags & 0x8000 != 0).then_some(count),
        });
    }
    ensure!(
        flags & 0x4000 == 0 || count <= i8::MAX as u8,
        "negative maximum enemy count"
    );
    if flags & 0x10000 != 0 && target == 3 {
        predicates.push(EnemyPredicate::TargetAttacking);
    }
    let difficulty_start = if flags & 8 != 0 {
        2
    } else if flags & 4 != 0 {
        1
    } else {
        0
    };
    let difficulty_end = if flags & 0x20000000 != 0 { 0 } else { 2 };
    ensure!(
        difficulty_start <= difficulty_end,
        "empty enemy difficulty range"
    );
    Ok(EnemyRequirements {
        difficulty: difficulty_start..=difficulty_end,
        hp_percent: if flags & 1 != 0 {
            Some(25)
        } else if flags & 2 != 0 {
            Some(50)
        } else if flags & 0x100000 != 0 {
            Some(75)
        } else {
            None
        },
        max_enemies: (flags & 0x4000 != 0).then_some(count),
        priority: flags & 0x80000 != 0,
        once_per_battle: flags & 0x10 != 0,
        target_guarding: flags & 0x10000 != 0 && target == 5,
        followup_only: flags & 0x200000 != 0,
        followup_requires_hit: flags & 0x400000 != 0,
        predicates,
    })
}

fn sound(encoded: u16) -> Option<Sound> {
    match encoded {
        0 => None,
        value if value & 0x8000 != 0 => Some(Sound::Stream(value & 0x7fff)),
        value => Some(Sound::Cue(value + 501)),
    }
}

fn choice(
    row: &[u8],
    flags: u32,
    requirements: EnemyRequirements,
    execution: Value,
) -> Result<Value> {
    let approach = if byte(row, 48)? == 0 {
        json!(if flags & 0x40 != 0 { "run" } else { "walk" })
    } else {
        let speed = float(row, 40)?;
        let rate = float(row, 44)?;
        ensure!(speed > 0. && rate > 0., "invalid enemy custom approach");
        json!({"custom":{"clip":"walk","rate":rate,"speed":speed}})
    };
    let target = byte(row, 1)?;
    let target_policy = (target != 0)
        .then(|| TargetPolicy::try_from(target))
        .transpose()?;
    let native_weight = byte(row, 0)?;
    ensure!(
        native_weight <= i8::MAX as u8 || requirements.priority,
        "negative enemy action weight without priority"
    );
    let weight = if native_weight > i8::MAX as u8 {
        0
    } else {
        native_weight
    };
    let spell = half(row, 64)? >= 200;
    let cast_ticks = if spell {
        Some(half(row, 14)?).filter(|&ticks| ticks != 0)
    } else {
        None
    };
    let cast_armor = spell.then(|| byte(row, 49)).transpose()?;
    Ok(json!({
        "weight":weight,
        "required_flag":(half(row, 58)? != 0).then_some(half(row, 58)?),
        "voices":{"begin":sound(half(row, 60)?),"release":sound(half(row, 62)?)},
        "chain_group":byte(row, 32)?,
        "followup_chance":(byte(row, 50)? as i8).max(0) as u8,
        "approach":approach,
        "requirements":requirements,
        "target_policy":target_policy,
        "target_allies":flags & 0x200 != 0 && target != 0,
        "range":[signed(row, 16)?,signed(row, 18)?],
        "approach_range":signed(row, 20)?,
        "approach_minimum":signed(row, 22)?,
        "guard_chance":byte(row, 35)? as i8,
        "tp":byte(row, 52)?,
        "return_to_formation":flags & 0x20 == 0,
        "run_return":flags & 0x60 == 0x40,
        "cast_ticks":cast_ticks,
        "cast_armor":cast_armor,
        "execution":execution
    }))
}

fn damage_kind(value: u8) -> Result<DamageKind> {
    Ok(match value {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        2 => DamageKind::Magic,
        other => anyhow::bail!("enemy damage kind {other} needs a decoder"),
    })
}

pub(crate) fn hit_rule(
    source: &[u8],
    index: u8,
    recoil: u8,
    kind: DamageKind,
) -> Result<ActionHit> {
    let at = usize::from(index) * 28;
    let hit = source.get(at..at + 28).context("missing enemy hit rule")?;
    ensure!(
        hit[24..].iter().all(|&byte| byte == 0),
        "special enemy hit rule {index}: {hit:02x?} needs a decoder"
    );
    let flags = half(hit, 0)?;
    let element = match hit[2] {
        0 => HitElement::Inherited,
        10 => HitElement::Neutral,
        index @ 1..=8 => {
            HitElement::Element(resonance_content::menu_data::Element::ALL[usize::from(index - 1)])
        }
        other => anyhow::bail!("enemy hit element {other} needs a decoder"),
    };
    let condition = match word(hit, 8)? {
        0 => None,
        bits => Some(match bits {
            1 => Condition::PoisonMild,
            2 => Condition::PoisonSevere,
            8 => Condition::Paralysis,
            16 => Condition::Weak,
            32 => Condition::Petrified,
            128 => Condition::Curse,
            256 => Condition::ReduceItemEffect,
            512 => Condition::Heavy,
            0x40000 => Condition::DefenseDown,
            0x80000 => Condition::AccuracyDown,
            other => anyhow::bail!("enemy hit condition {other:#x} needs a decoder"),
        }),
    }
    .map(|condition| HitCondition {
        condition,
        chance: hit[12],
        value: hit[23] as i8,
    });
    let power = match byte(hit, 13)? {
        0 | 2 => Power::Normal,
        1 => Power::Percent(half(hit, 14)?),
        3 => Power::Fixed(half(hit, 14)?),
        4 => Power::AdjustAfterContact(signed(hit, 14)?),
        other => anyhow::bail!("enemy hit condition {other:#x} needs a decoder"),
    };
    Ok(ActionHit {
        kind,
        effect: (hit[22] != 0).then_some(u16::from(hit[22])),
        effect_on_guard: flags & 0x800 != 0,
        suppress_guard_sound: flags & 0x8000 != 0,
        capture: flags & 0x10 != 0,
        sound: (half(hit, 16)? != 0).then_some(half(hit, 16)?),
        power,
        element,
        arte: flags & 0x20 != 0,
        prevents_defeat: flags & 1 != 0,
        finisher_grade: flags & 0x200 != 0,
        hp_drain: flags & 0x400 != 0,
        suppress_reaction: flags & 0x4000 != 0,
        force_reaction: flags & 0x2000 != 0,
        alternate_motion: flags & 2 != 0,
        knock_down: flags & 4 != 0,
        launch: flags & 8 != 0,
        hits_down: flags & 0x40 != 0,
        breaks_guard: flags & 0x80 != 0,
        unbreakable_guard: flags & 0x1000 != 0,
        lift_guard: flags & 0x100 != 0,
        guard_pressure: hit[7],
        bypass_guard: false,
        hitstun: hit[3],
        cooldown: hit[4],
        stun_chance: hit[5],
        stagger: hit[6],
        armor_damage: hit[20],
        recoil,
        recoil_delay: hit[21],
        condition,
    })
}

fn actions(
    data: &[u8],
    library: &Path,
    name: &str,
    common: Option<&[u8]>,
    id: u8,
) -> Result<(Value, Value, Value, Value)> {
    let source = section(data, 10)?;
    let policy = section(data, 12)?;
    ensure!(
        !source.is_empty()
            && source.len().is_multiple_of(68)
            && policy.len() >= 26
            // Selector 25's active REL callback initializes its state, then
            // delegates to the common scheduler; back-row choices stay data.
            && matches!(policy[8], 0..=13 | 15..=27)
            && usize::from(policy[23]) == source.len() / 68
            && policy[24] <= 4
            && policy[11] == 0
            && policy[22] == 0
            && policy[25] == 0,
        "special enemy action policy needs a decoder: selector {}, reserved [{}, {}, {}], rows {}/{}, back row {}",
        policy.get(8).copied().unwrap_or(255),
        policy.get(11).copied().unwrap_or(255),
        policy.get(22).copied().unwrap_or(255),
        policy.get(25).copied().unwrap_or(255),
        policy.get(23).copied().unwrap_or(255),
        source.len() / 68,
        policy.get(24).copied().unwrap_or(255)
    );
    let back_row = (0..usize::from(policy[24]))
        .map(|index| json!({"action":policy[14 + index],"weight":policy[18 + index]}))
        .collect::<Vec<_>>();
    let hit = section(data, 8)?;
    ensure!(
        !hit.is_empty() && hit.len().is_multiple_of(28),
        "invalid enemy hit-rule table"
    );
    let contact_source = section(data, 16)?;
    let mut rows = Vec::new();
    let mut attacks = serde_json::Map::new();
    let mut contacts = serde_json::Map::new();
    let mut projectiles = serde_json::Map::new();
    let mut weapon_change = [53, 145];
    let mut station_change = 60;
    for (index, row) in source.chunks_exact(68).enumerate() {
        let flags = word(row, 8)?;
        ensure!(
            matches!(byte(row, 48)?, 0 | 1),
            "special enemy action {index} needs a decoder: flags {:#x}, byte1 {}, approach {}, bytes51/52 [{}, {}], tail {:02x?}",
            flags,
            byte(row, 1)?,
            byte(row, 48)?,
            byte(row, 51)?,
            byte(row, 52)?,
            &row[58..66]
        );
        let requirements = requirements(row, flags)?;
        if flags & 0x0200_0000 != 0 {
            rows.push(choice(row, flags, requirements, json!("leave_arena"))?);
            continue;
        }
        let technique = half(row, 64)?;
        if technique >= 200 {
            rows.push(choice(
                row,
                flags,
                requirements,
                json!({"spell":technique}),
            )?);
            continue;
        }
        let common = if technique != 0 {
            let common = common.context("physical enemy arte needs the native common directory")?;
            Some(
                crate::native_archive::member(
                    common,
                    usize::from(technique),
                    word(common, 0)? as usize,
                )?
                .context("missing physical enemy arte")?,
            )
        } else {
            None
        };
        let hit = if let Some(common) = common {
            common
                .get(word(common, 0)? as usize..word(common, 4)? as usize)
                .context("physical enemy hit rules exceed their native member")?
        } else {
            hit
        };
        let mut pose_rows = poses(data, half(row, 24)?, library, name)?;
        if common.is_some() {
            let mut retained = Vec::new();
            for entry in pose_rows {
                if let Some(clip) = entry.1.get("pose").and_then(|pose| pose["clip"].as_str()) {
                    let slot = clip
                        .strip_prefix("slot-")
                        .context("physical pose needs a numbered slot")?
                        .parse::<usize>()?;
                    if word(data, 0x20 + slot * 4)? == 0 {
                        continue;
                    }
                }
                retained.push(entry);
            }
            pose_rows = retained;
        }
        let partial_opening = pose_rows
            .first()
            .is_some_and(|(_, event, _)| event.get("partial_pose").is_some());
        let admission_only = pose_rows.first().is_some_and(|(at, event, blend)| {
            *at == 0
                && *blend == 0
                && (event.get("pose").is_some() || event.get("partial_pose").is_some())
        });
        let mut holds: Vec<_> = pose_rows
            .iter()
            .enumerate()
            .map(|(index, (at, _, blend))| {
                (
                    *at,
                    if index == 0 {
                        *blend
                    } else {
                        blend.saturating_sub(1)
                    },
                )
            })
            .collect();
        if admission_only {
            // An unblended opening pose still consumes one action-admission update.
            holds[0].1 = 1;
        }
        let delayed_opening =
            common.is_some() && pose_rows.first().is_none_or(|(at, _, _)| *at > 0);
        if delayed_opening {
            if let Some((_, hold)) = holds.first_mut() {
                *hold = hold.saturating_sub(1);
            }
            holds.insert(0, (0, 1));
        }
        if partial_opening {
            // Partial events have their own animation clock. Only body selections
            // hold the gameplay clock, at the pre-hold native tick.
            let mut gameplay_delay = 0_u16;
            holds = vec![(0, 1)];
            for (at, event, blend) in &pose_rows {
                if event.get("pose_keeping_parts").is_some() {
                    let hold = blend.saturating_sub(1);
                    holds.push((
                        at.checked_sub(gameplay_delay)
                            .context("partial pose hold precedes gameplay")?,
                        hold,
                    ));
                    gameplay_delay = gameplay_delay
                        .checked_add(u16::from(hold))
                        .context("partial pose hold overflow")?;
                }
            }
        }
        let opening_hold = if partial_opening {
            u8::from(pose_rows[0].2 > 0)
        } else if admission_only {
            0
        } else {
            holds
                .first()
                .filter(|(at, _)| *at == 0)
                .map_or(0, |(_, hold)| *hold)
        };
        let (mut events, native_duration) = commands(
            data,
            half(row, 26)?,
            half(row, 14)?,
            &holds,
            u16::from(opening_hold),
        )?;
        if delayed_opening {
            for (at, _) in &mut events {
                if *at == 0 {
                    *at = 1;
                }
            }
        }
        if policy[8] == 9 && index == 2 {
            weapon_change = weapon_change.map(|at| elapsed(at, &holds));
        }
        if policy[8] == 11 && index == 2 {
            station_change = elapsed(station_change, &holds);
        }
        let duration = elapsed(native_duration, &holds)
            .checked_sub(u16::from(technique != 0))
            .context("empty physical enemy arte duration")?;
        if byte(row, 34)? != 0 {
            events.push((u16::from(delayed_opening), json!({"effect":byte(row, 34)?})));
        }
        for (at, event, _) in &pose_rows {
            let tick = (if partial_opening {
                *at
            } else {
                elapsed(*at, &holds)
            })
            .checked_add(u16::from(event.get("animation_rate").is_some()))
            .context("enemy animation rate exceeds timeline limit")?;
            events.push((tick, event.clone()));
        }
        let mut cursor = usize::from(half(row, 28)?) * 32;
        let mut next_contact_at = 0;
        let mut contact_delay = 0;
        while signed(contact_source, cursor)? != -1 {
            let contact = contact_source
                .get(cursor..cursor + 32)
                .context("truncated enemy contact")?;
            let authored_at = u16::try_from(signed(contact, 0)?)?;
            let contact_at = authored_at.max(next_contact_at);
            let count = usize::from(byte(contact, 3)?);
            if byte(contact, 2)? == 254 {
                let mut definition =
                    battle_enemy_projectiles::projectile(data, contact, hit, library, name)?;
                if common.is_some() {
                    physical_feedback(data, &mut definition.hit)?;
                }
                let name = intern(
                    &mut projectiles,
                    "projectile",
                    serde_json::to_value(definition)?,
                );
                events.push((
                    if contact_at + contact_delay == 0 {
                        u16::from(opening_hold)
                    } else {
                        elapsed(contact_at + contact_delay, &holds)
                    },
                    json!({"projectile":name}),
                ));
                next_contact_at = contact_at + 1;
                cursor += 32;
                continue;
            }
            ensure!(
                (1..=4).contains(&count)
                    && byte(contact, 16)? <= 4
                    && byte(contact, 19)? <= 1
                    && half(contact, 22)? == 0,
                "special enemy contact needs a decoder: {contact:02x?}"
            );
            let attachments = match byte(contact, 4)? {
                254 => {
                    ensure!(count == 1, "enemy body-bone contact has multiple groups");
                    json!({"body_bone":byte(contact, 5)?})
                }
                255 => {
                    ensure!(count == 1, "enemy body-center contact has multiple groups");
                    json!({"body_center":{}})
                }
                _ => {
                    json!({"groups":contact.get(4..4 + count).context("truncated enemy contact groups")?})
                }
            };
            let source_duration = u16::try_from(byte(contact, 2)? as i8)? + 1;
            let end = authored_at + source_duration;
            let at = contact_at + contact_delay;
            let start = if at == 0 {
                u16::from(opening_hold)
            } else {
                elapsed(at, &holds)
            };
            let duration = elapsed(at + end - contact_at, &holds)
                .min(duration.saturating_add(1))
                .checked_sub(start)
                .context("empty translated enemy contact")?;
            let mut hit = hit_rule(
                hit,
                byte(contact, 18)?,
                byte(contact, 20)?,
                damage_kind(byte(contact, 17)?)?,
            )?;
            hit.bypass_guard = byte(contact, 19)? == 0;
            if common.is_some() {
                physical_feedback(data, &mut hit)?;
            }
            let shape = match byte(contact, 16)? {
                0 => json!({"kind":"box"}),
                1 => json!({"kind":"cylinder"}),
                2 => json!({"kind":"ground_circle"}),
                3 => json!({"kind":"ring","width":float(contact, 28)?}),
                4 => json!({"kind":"sphere"}),
                _ => unreachable!(),
            };
            let mut definition = json!({
                "duration":duration,
                "shape":shape,
                "radius":rounded(float(contact, 8)?),
                "height":rounded(float(contact, 12)?),
                "hit":hit
            });
            definition
                .as_object_mut()
                .context("invalid contact definition")?
                .extend(
                    attachments
                        .as_object()
                        .context("invalid contact attachments")?
                        .clone(),
                );
            let name = intern(&mut contacts, "contact", definition);
            events.push((
                start,
                if contact_at > authored_at {
                    json!({"continue_contact":name})
                } else {
                    json!({"contact":name})
                },
            ));
            next_contact_at = end - 1;
            contact_delay += 1;
            cursor += 32;
        }
        if policy[8] == 18 && (2..=5).contains(&index) {
            events.extend((20..=88).step_by(4).map(|at| (
                elapsed(at, &holds)-1,
                json!({"carried_effect":{"slot":0,"bone":0,"member":if index < 4 {8} else {7}}})
            )));
        }
        if policy[8] == 17 && index == 4 {
            let template = battle_enemy_media::member(data, 0x1c8)?
                .and_then(|table| table.get(400..800))
                .context("missing ground eruption template")?;
            let hit = hit_rule(hit, 0, template[0x13], damage_kind(template[0x11])?)?;
            let shots = [3, 4]
                .into_iter()
                .map(|effect| -> Result<String> {
                    let definition = battle_enemy_projectiles::with_model_effect(
                        data, 1, effect, library, name,
                    )?;
                    Ok(intern(
                        &mut projectiles,
                        "projectile",
                        json!({"definition":definition,"hit":hit}),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            // The native callback observes age 26 after this update's action work.
            let at = elapsed(26, &holds)
                .checked_sub(1)
                .context("invalid eruption tick")?;
            ensure!(at < duration, "ground eruption exceeds attack duration");
            events.push((
                at,
                json!({"ground_projectiles":{"projectiles":shots,"retreat":225.}}),
            ));
        }
        if policy[8] == 23 && index == 5 {
            let at = elapsed(120, &holds)
                .checked_sub(1)
                .context("invalid affliction tick")?;
            ensure!(at < duration, "opponent affliction exceeds attack duration");
            events.push((at, json!({"afflict_opponents":{
                "groups":[["curse","paralysis","poison_severe"],["attack_down","defense_down","weak"]],
                "dispel":true
            }})));
        }
        events.sort_by_key(|(at, _)| *at);
        let recovery_ticks = if let Some(common) = common {
            let variant = resonance_content::battle_martial::Binding::enemy(technique).variant;
            u8::try_from(half(common, 18 + usize::from(variant) * 28)?)?
        } else {
            byte(row, 2)?
        };
        let recovery_pose = if common.is_some() || byte(row, 3)? == 0 {
            json!({"clip":"idle","blend":recovery_ticks,"repeat":true})
        } else {
            json!({"clip":format!("slot-{}", byte(row, 3)?),"rate":float(row, 4)?,"blend":0})
        };
        let chain_tick = half(row, 30)?;
        let chain_at = (byte(row, 50)? as i8 > 0 && chain_tick <= half(row, 14)?)
            .then(|| elapsed(chain_tick, &holds));
        let attack = json!({
            "duration":duration,
            "chain_at":chain_at,
            "recovery_ticks":recovery_ticks,
            "recovery_pose":recovery_pose,
            "vulnerable":[
                elapsed(half(row, 36)?, &holds) + u16::from((partial_opening || delayed_opening) && half(row, 36)? == 0),
                elapsed(half(row, 38)?, &holds) + u16::from((partial_opening || delayed_opening) && half(row, 38)? == 0)
            ],
            "contact_recovery":contact_recovery(data, row)?,
            "armor":byte(row, 49)?,
            "events":events
        });
        let attack_name = intern(&mut attacks, "attack", attack);
        let execution = if technique == 0 {
            json!({"attack":attack_name})
        } else {
            json!({"physical":{"technique":technique,"attack":attack_name}})
        };
        rows.push(choice(row, flags, requirements, execution)?);
    }
    let mut decision = json!({"back_row":back_row});
    match policy[8] {
        9 => {
            decision["event_hp_hold"] = json!(true);
            decision["behavior"] = json!({"replace_weapon":{"action":2,"hide_at":weapon_change[0],"replace_at":weapon_change[1],"source":1,"destination":0,"disabled_action":0}});
        }
        11 => {
            decision["behavior"] = json!({"three_stations":{"change_at":station_change,"stations":[
                {"position":[1650.,0.,0.],"heading":-90.},{"position":[-825.,0.,1428.],"heading":150.},{"position":[-825.,0.,-1428.],"heading":30.}
            ]}})
        }
        18 | 24 => {
            let seles = policy[8] == 18;
            let groups = if seles {
                json!([[[0, 0], [1, 1]], [[3, 0], [1, 1]], [[2, 0], [1, 1]]])
            } else {
                json!([[[0, 0], [1, 1]], [[2, 0]], [[3, 0], [3, 1]], [[4, 0]]])
            };
            let selected = (0..rows.len())
                .map(|index| {
                    if seles {
                        Some(match index {
                            2 | 3 => 1,
                            4 | 5 => 2,
                            _ => 0,
                        })
                    } else {
                        match index {
                            2 | 3 | 5 => Some(0),
                            6 => Some(1),
                            9 => Some(2),
                            7 | 8 => Some(3),
                            _ => None,
                        }
                    }
                })
                .collect::<Vec<_>>();
            decision["behavior"] = json!({"action_weapons":{"groups":groups,"actions":selected,
                "idle_group":if seles {json!(0)} else {Value::Null},"effect":if seles {6} else {2},
                "anchors":if seles {vec![0]} else {vec![0,1]},
                "drop_on_hurt":if seles {json!({"slot":1,"hp_percent":50,"upward_speed":10.,"gravity":-0.9,"pitch":-90.})} else {Value::Null}
            }});
        }
        _ => {}
    }
    if policy[8] == 26 {
        decision["normalize_affinities"] = json!([
            {"flags":[311],"elements":["fire","ice"]},
            {"flags":[312],"elements":["water","lightning"]},
            {"flags":[313],"elements":["wind","earth"]},
            {"flags":[311,312,313],"neutral":true}
        ]);
    }
    if id == 165 {
        decision["fixed_hp"] = json!(255);
    }
    if policy[8] == 7 {
        decision["behavior"] = json!({"transformation":{
            "reveal_hp_percent":75,"reveal_armor":1,
            "revealed_bones":["sc_Bone_ude_L","sc_Bone_ude_R","sc_Bone_kata_L","sc_Bone_kata_R","sc_Bone_body","sc_Bone_ashi_L","sc_Bone_ashi_R","sc_Bone_koshi"],
            "airborne_hp_percent":50,"airborne_recover_in_air":true,"airborne_walk_speed":6.5,
            "airborne_motions":(0..30).map(|clip| [format!("slot-{clip}"),format!("slot-{}",clip+30)]).collect::<Vec<_>>(),
            "hover_height":25.,"initial_actions":{"start":0,"end":5},"final_actions":{"start":5,"end":9}
        }});
    }
    if policy[8] == 13 {
        ensure!(
            (208..=210).contains(&id),
            "unknown guardian model-parts owner"
        );
        let mut scales = vec![json!(["kk06_Hane", 0.5])];
        if id != 209 {
            scales.extend([json!(["Bone21_BArm_L", 0.]), json!(["Bone21_BArm_R", 0.])]);
        }
        if id != 208 {
            scales.extend([json!(["Bone11_FArm_L", 0.]), json!(["Bone11_FArm_R", 0.])]);
        }
        decision["behavior"] = json!({"model_parts":{"bone_scales":scales}});
    }
    if policy[8] == 19 {
        decision["behavior"] = json!({"retreat":{"hp_percent":50,"after_ticks":3600}});
    }
    if policy[8] == 17 {
        decision["behavior"] = json!({"health_stage":{
            "hp_percent":50,"common_motion_offset":30,"initial_station":{"position":[850.,0.,0.],"heading":-90.},
            "initial_actions":{"start":0,"end":5},"final_actions":{"start":5,"end":9}
        }});
    }
    if matches!(policy[8], 21 | 23) {
        decision["behavior"] = json!({"health_recoil":{
            "hp_percent":50,"knockdown_immune":true,"clear_pending_on_hit":true
        }});
    }
    if policy[8] == 5 {
        // Active US_r REL initializes the primary carried model's clock once.
        decision["behavior"] = json!({"carried_animation_rate":{"slot":0,"rate":0.15}});
    }
    if policy[8] == 3 {
        // Active US_r REL callback: low HP adopts motion 45 and clears RUN_APPROACH.
        let mut walk = json!({"hp_percent":25,"speed":2.83,"motion_alias":["walk","slot-45"]});
        if name == "botta-0229" {
            walk["last_enemy_hard_cast_ticks"] = json!(0);
        }
        decision["walk_override"] = walk;
    }
    if policy[8] == 27 {
        // The native callback clears the battle menu's item-enable bit.
        decision["items_forbidden"] = json!(true);
    }
    Ok((
        json!({"rows":rows,"policy":decision}),
        Value::Object(attacks),
        Value::Object(contacts),
        Value::Object(projectiles),
    ))
}

fn enemy_entrance(
    selector: u8,
    data: &[u8],
    metadata: &[u8],
    fallback: Value,
    profile: &resonance_content::battle_profile::Profile,
) -> Result<Value> {
    let camera = |radius: f32, height: f32, yaw: f32, elevation: f32| {
        json!({
            "radius":radius,"height_radius":1500.,"focus_height":height,"focus_height_step":0.,
            "yaw":yaw,"yaw_step":0.,"elevation":elevation,"elevation_step":0.,"minimum_elevation":elevation
        })
    };
    let mut entrance =
        json!({"ticks":0,"gravity":0.,"flash":null,"launches":[],"cues":[],"camera":null});
    match selector {
        2 => {
            entrance["ticks"] = json!(360);
            entrance["show_party"] = json!(true);
            entrance["show_hud"] = json!(true);
            entrance["cues"] = json!([[280,{"feedback":{"duration":175,"limit":8,"alpha":160}}]]);
            let mut cam = camera(2000., 80., 0., 70.);
            cam["height_radius"] = json!(1000.);
            cam["yaw_step"] = json!(0.46);
            cam["maximum_yaw"] = json!(165.);
            cam["elevation_step"] = json!(-0.175);
            cam["minimum_elevation"] = json!(10.);
            entrance["camera"] = cam;
        }
        10 => {
            entrance["ticks"] = json!(570);
            entrance["show_party"] = json!(true);
            entrance["show_hud"] = json!(true);
            entrance["flash"] = json!({"tick":220,"color":[128,128,128],"step":16,"hold":4});
            entrance["cues"] = json!([
                [1,{"height":100.}],[10,{"stage_tint":{"color":[28,28,28],"duration":540,"step":2}}],
                [10,{"effect":1}],[220,{"shake":{"duration":30,"amplitude":24}}],
                [220,{"feedback":{"duration":160,"limit":6,"alpha":160}}],
                [240,{"pose":{"clip":"slot-44","rate":0.5,"repeat":false,"blend":0}}],
                [240,{"vertical":-0.4}],[280,{"voice":{"stream":2077}}],
                [490,{"pose":{"clip":"slot-45","rate":0.3,"repeat":false,"blend":8}}]
            ]);
            let mut cam = camera(1750., 120., 180., 22.);
            cam["height_radius"] = json!(1000.);
            cam["yaw_step"] = json!(0.01);
            cam["elevation_step"] = json!(-0.01);
            cam["minimum_elevation"] = json!(18.5);
            entrance["camera"] = cam;
        }
        12 => {
            entrance["ticks"] = json!(300);
            entrance["gravity"] = json!(-1.);
            entrance["show_party"] = json!(true);
            entrance["show_hud"] = json!(true);
            let commands = section(data, 14)?;
            let mut cursor = usize::from(half(metadata, 0xf2)?) * 2;
            let mut launches = Vec::new();
            while half(commands, cursor)? != u16::MAX {
                ensure!(
                    half(commands, cursor + 2)? == 1,
                    "unsupported entrance movement command"
                );
                launches.push((
                    half(commands, cursor)?,
                    f32::from(signed(commands, cursor + 4)?) / 10.,
                ));
                cursor += 6;
            }
            entrance["launches"] = json!(launches);
            entrance["cues"] = json!([
                [1,{"height":1000.}],[1,{"pose":{"clip":"slot-36","rate":0.5,"repeat":false,"blend":4}}],
                [1,{"expression":4}],[10,{"effect":1}],
                [86,{"pose":{"clip":"slot-37","rate":0.4,"repeat":false,"blend":4}}],
                [86,{"shake":{"duration":45,"amplitude":16}}],[86,{"sound":{"cue":93}}],
                [86,{"expression":1}],[86,{"vertical":14.}],[104,{"voice":{"stream":2075}}],
                [134,{"pose":{"clip":"taunt","rate":0.4,"repeat":false,"blend":4}}],
                [134,{"expression":0}],[196,{"pose":{"clip":"idle","rate":0.5,"repeat":true,"blend":4}}]
            ]);
            let mut cam = camera(1750., 120., 180., 20.);
            cam["height_radius"] = json!(1200.);
            cam["yaw_step"] = json!(0.01);
            cam["elevation_step"] = json!(-0.01);
            cam["minimum_elevation"] = json!(16.5);
            entrance["camera"] = cam;
        }
        26 => {
            entrance["ticks"] = json!(210);
            entrance["cues"] = json!([[210, "dissolve"]]);
            let mut cam = camera(
                1950.,
                profile.center_offset[1] * profile.model_scale - 20.,
                180.,
                9.,
            );
            cam["focus_height_step"] = json!(0.1);
            entrance["camera"] = cam;
        }
        21 => {
            let opening = camera(1150., 75., -170., 5.);
            entrance["ticks"] = json!(715);
            entrance["camera"] = opening.clone();
            entrance["cues"] = json!([
                [25,{"voice":{"stream":2047}}],
                [40,{"pose":{"clip":"taunt","rate":0.2,"repeat":false,"blend":10}}],
                [300,"dissolve"],[320,{"party_voice":{"stream":2048}}],[320,"party_active_expression"],
                [530,{"voice":{"stream":2049}}],[550,"dissolve"],
                [650,{"feedback":{"duration":50,"limit":16,"alpha":160}}]
            ]);
            entrance["staging"] = json!({"character":1,
                "party_positions":[[-300.,0.,0.],[-450.,0.,200.],[-450.,0.,100.],[-600.,0.,-200.]],
                "enemy_position":[300.,0.,0.],"hide_party_at":550,
                "shots":[
                    [0,{"party":true,"camera":opening,"lateral":20.,"lateral_step":-0.25}],
                    [301,{"party":false,"camera":camera(1300.,100.,-10.,5.),"lateral":19.75,"lateral_step":-0.25}],
                    [551,{"party":false,"camera":camera(850.,100.,170.,5.),"lateral":0.,"lateral_step":0.}]
                ]
            });
        }
        15 | 22 => {
            let duel = selector == 15;
            entrance["ticks"] = json!(if duel { 430 } else { 660 });
            entrance["letterbox"] = json!(if duel { 64 } else { 0 });
            let opening = camera(
                1150.,
                if duel { 100. } else { 75. },
                -170.,
                if duel { 7.5 } else { 5. },
            );
            entrance["camera"] = opening.clone();
            entrance["cues"] = if duel {
                json!([
                    [5,{"pose":{"clip":"slot-23","rate":0.65,"repeat":false,"blend":10}}],
                    [30,{"voice":{"stream":2008}}],[150,"dissolve"],[165,{"voice":{"stream":2009}}],
                    [165,{"party_pose":{"clip":"slot-100","rate":0.5,"repeat":false,"blend":10}}],
                    [165,"party_active_expression"],[210,{"sound":{"cue":60}}],[430,"dissolve"]
                ])
            } else {
                json!([
                    [2,{"pose":{"clip":"idle","rate":0.4,"repeat":true,"blend":8}}],
                    [25,{"voice":{"stream":1998}}],[210,"dissolve"],[220,{"party_voice":{"stream":1999}}],
                    [220,"party_active_expression"],[330,{"voice":{"stream":2000}}],[330,"active_expression"],
                    [330,"dissolve"],[450,{"pose":{"clip":"taunt","rate":0.3,"repeat":false,"blend":8}}],
                    [660,"dissolve"]
                ])
            };
            let mut shots =
                vec![json!([0,{"party":true,"camera":opening,"lateral":20.,"lateral_step":-0.25}])];
            shots.push(json!([if duel {151} else {211},{"party":false,
                "camera":camera(if duel {1150.} else {1000.},100.,-10.,if duel {7.5} else {5.}),
                "lateral":19.75,"lateral_step":-0.25}]));
            if !duel {
                shots.push(
                    json!([331,{"party":false,"camera":camera(700.,100.,170.,5.),
                    "lateral":0.,"lateral_step":0.,"vertical":-49.75,"vertical_step":0.25}]),
                );
                shots.push(json!([530,{"party":false,"camera":camera(700.,100.,170.,5.),"lateral":0.,"lateral_step":0.}]));
            }
            entrance["staging"] = json!({"character":1,
                "party_positions":if duel {vec![[-300.,0.,0.]]} else {vec![[-300.,0.,0.],[-450.,0.,200.],[-450.,0.,100.],[-600.,0.,-200.]]},
                "enemy_position":[300.,0.,0.],"hide_party_at":if duel {Value::Null} else {json!(330)},
                "fallback":if duel {Value::Null} else {fallback},"shots":shots
            });
        }
        _ => anyhow::bail!("enemy entrance selector {selector} needs a decoder"),
    }
    Ok(json!({"entrance":entrance}))
}

/// Publish one ordinary enemy's editable rules; its body GLB must already be cooked.
pub fn cook_battle_enemy_rules(root: &Path, library: &Path, id: u8) -> Result<()> {
    let name = MonsterId::try_from(id)?
        .resource_name()
        .context("enemy has no name")?;
    let mut packages = EnemyPackages::open(&root.join("disc1"))?;
    let data = packages.read(id)?;
    ensure!(data.starts_with(b"em8\0"), "invalid enemy package");
    let strategy = section(&data, 6)?;
    let row = byte(strategy, 2)?;
    let (entry_row, guard_recovery_bonus) = match row {
        0 => {
            let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
            let default = module.slice(4, 0x10ef, 1)?[0];
            (
                "random",
                resonance_content::battle_recoil::strategy_guard_recovery(default),
            )
        }
        1 => ("front", 0),
        2 => ("middle", 5),
        3 => ("back", 25),
        _ => anyhow::bail!("enemy entry strategy {row} needs a decoder"),
    };
    let target_strategy = TargetPolicy::try_from(byte(strategy, 0)?)?;
    let nodes = joint_names(library, name)?;
    let profile_source = section(&data, 4)?;
    let selector = byte(section(&data, 12)?, 8)?;
    let physical = section(&data, 10)?.chunks_exact(68).any(|row| {
        let technique = u16::from_be_bytes([row[64], row[65]]);
        (1..200).contains(&technique)
    });
    let usual = (physical
        || word(profile_source, 0x104)? != 0
        || matches!(
            selector,
            2 | 3
                | 6
                | 8
                | 10
                | 12
                | 13
                | 15
                | 16
                | 18
                | 19
                | 20
                | 21
                | 22
                | 23
                | 24
                | 25
                | 26
                | 27
        ))
    .then(|| fs::read(root.join("disc1/files/BTL/BTLusual.dat")))
    .transpose()?;
    let common = usual
        .as_deref()
        .map(|usual| crate::native_archive::member(usual, 8, 14))
        .transpose()?
        .flatten();
    let (actions, attacks, contacts, projectiles) = actions(&data, library, name, common, id)?;
    let mut profile = battle_enemy_profile::profile(profile_source, &nodes, usual.as_deref())?;
    let presentation = if matches!(
        selector,
        2 | 3 | 6 | 8 | 10 | 12 | 13 | 15 | 16 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27
    ) {
        if let Some(sound) = native_sound(half(profile_source, 0xf4)?) {
            profile.voices.get_or_insert_with(Default::default).defeat = Some(sound);
        }
        let sound = profile.voices.as_ref().and_then(|voices| voices.defeat);
        let durations = crate::native_archive::member(
            usual
                .as_deref()
                .context("enemy presentation needs battle voices")?,
            12,
            14,
        )?
        .context("missing battle voice durations")?;
        let timed = |sound: Option<Sound>| -> Result<Value> {
            let ticks = if let Some(sound) = sound {
                let index = match sound {
                    Sound::Stream(index) => index,
                    Sound::Cue(cue) => cue.checked_sub(501).context("invalid battle voice cue")?,
                };
                half(durations, usize::from(index) * 2)?
            } else {
                0
            };
            Ok(json!({"sound":sound,"ticks":ticks}))
        };
        let opening = if matches!(selector, 2 | 10 | 12 | 15 | 21 | 22 | 26) {
            enemy_entrance(
                selector,
                &data,
                profile_source,
                timed(native_sound(half(profile_source, 0xf6)?))?,
                &profile,
            )?
        } else if matches!(selector, 3 | 6 | 8 | 19 | 20 | 23 | 24) {
            json!({"voice":timed(native_sound(half(profile_source, 0xf6)?))?})
        } else {
            Value::Null
        };
        json!({"opening":opening,"defeat":timed(sound)?})
    } else {
        Value::Null
    };
    let count = (word(&data, 0x14)? as usize).max(31);
    ensure!(count <= (0x160 - 0x20) / 4, "enemy motion table is invalid");
    let mut null_motions = Vec::new();
    for slot in 0..count {
        if word(&data, 0x20 + slot * 4)? == 0 {
            null_motions.push(slot);
        }
    }
    let rules = json!({
        "null_motions":null_motions,
        "profile":profile,
        "weapon_visibility":match id {
            105 => [("1".into(),json!("alternating"))].into_iter().collect(),
            208 | 209 => (0..4).map(|slot| (slot.to_string(), json!("hidden"))).collect::<serde_json::Map<_,_>>(),
            236 | 237 | 238 => [("0".into(),json!("hidden"))].into_iter().collect(),
            _ => serde_json::Map::new()
        },
        "presentation":presentation,
        "actions":actions,
        "attacks":attacks,
        "contacts":contacts,
        "projectiles":projectiles,
        "target_strategy":target_strategy,
        "entry_row":entry_row,
        "guard_recovery_bonus":guard_recovery_bonus
    });
    let compact: resonance_content::battle_enemy::rules::Rules = serde_json::from_value(rules)?;
    compact.clone().into_definition()?;

    let output = library.join(format!("battle/enemies/{name}/enemy.json"));
    let entry = resonance_content::battle_enemy::document::Entry {
        rules: compact,
        artwork: artwork(&data, library, name)?,
        effects: Some(format!("battle/enemies/{name}/effects/effects.json")),
    };
    crate::output::json(&output, &entry)?;
    Ok(())
}
