//! Settings shared by party templates and enemy packages.
use super::*;
use crate::battle::{
    actions::Rel,
    embedded::{self, Layout, SETTINGS_BYTES as BYTES},
};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PartySettings {
    pub character: u8,
    pub settings: ActorSettings,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ActorSettings {
    pub combat: Combat,
    pub casting: Casting,
    pub rewards: Rewards,
    pub model: Model,
    pub appearance: Appearance,
    pub effects: Effects,
    pub body_trail: BodyTrail,
    pub book_preview: BookPreview,
    pub camera: Camera,
    pub unused_storage: [crate::read::Storage; 2],
    pub attachments: [Attachment; 8],
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Combat {
    pub attack_element: u8,
    pub physical_affinity: u8,
    pub element_affinities: [u8; 9],
    pub condition_flags: [u32; 2],
    pub condition_immunity: [u32; 2],
    pub walk_speed: f32,
    pub run_speed: f32,
    pub weight: u8,
    pub family: u16,
    pub turn_divisor: u8,
    pub stun_resistance: u8,
    pub stagger_threshold: u8,
    pub guard_reduction: u8,
    pub intrinsic_conditions: [u32; 2],
    pub stagger_ticks: u8,
    pub guard_pressure_limit: u16,
    pub flags: u32,
    pub poise: u8,
    pub variant_count: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Rewards {
    pub experience: u32,
    pub gald: u32,
    pub drops: [u16; 2],
    pub steal: u16,
    pub drop_chances: [u8; 2],
    pub steal_chance: u8,
    pub grade: i16,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Casting {
    pub base_ticks: i16,
    pub loop_start: u8,
    pub animation_rate: f32,
    pub command_index: i16,
    pub effect_interval: u8,
    pub resume_start: u8,
    pub resume_blend_ticks: u8,
    pub resume_loop_start: u8,
    pub stored_recovery_clip: u8,
    pub chant_looping: bool,
    pub release_looping: bool,
    pub stored_release_looping: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Model {
    pub target_bone: u8,
    pub effect_offset: [f32; 3],
    pub stun_offset: [f32; 3],
    pub target_bone_offset: [f32; 3],
    pub model_scale: f32,
    pub shadow_scale: f32,
    pub effect_scale: f32,
    pub ground_offset: f32,
    pub idle_delay: u8,
    pub idle_jitter: u8,
    pub initial_clip: u8,
    pub unused_reset_argument: u8,
    pub entry_command_index: i16,
    pub entry_timeout_ticks: u8,
    pub alpha: u8,
    pub death_clip: u8,
    pub guard_effect: u8,
    pub head_bone: u8,
    pub body_flags: u16,
    pub secondary_body: u8,
    pub secondary_clip: u8,
    pub fixed_initial_clip: u8,
    pub shadow_color: [u8; 4],
    pub attachment_count: u8,
    pub auxiliary_instance_count: u8,
    pub effect_model_count: u8,
    pub attachment_resource_count: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Appearance {
    pub channel_count: u8,
    pub atlas_frames: [u8; 4],
    pub atlas_textures: [u8; 4],
    pub idle_face: [u8; 4],
    pub attack_face: [u8; 4],
    pub paralysis_face: [u8; 4],
    pub stun_face: [u8; 4],
    pub appearance_texture: u8,
    pub appearance_rows: u8,
    pub uv_channels: [UvChannel; 2],
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct UvChannel {
    pub mode: BodyUv,
    pub texture: u8,
    pub frame_count_or_u_step: i8,
    pub frame_delay_or_v_step: i8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Effects {
    pub overlimit_bone: u8,
    pub overlimit_rate: u8,
    pub overlimit_initial: u16,
    pub overlimit_scale: f32,
    pub opening_warning: u8,
    pub locomotion_effect_period: u8,
    pub locomotion_effect: u8,
    pub idle: Periodic<u8>,
    pub periodic: Periodic<u16>,
    pub periodic_channels: [Periodic<u8>; 6],
    pub voice_bank: u8,
    pub voice_base: u32,
    pub victory_voice: u16,
    pub death_voice: u16,
    pub opening_voice: u16,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Periodic<T> {
    pub interval: T,
    pub effect: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BodyTrail {
    pub rgba: [u8; 4],
    pub uv: [i16; 4],
    pub texture_bank: i8,
    pub palette: u8,
    pub render_flags: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BookPreview {
    pub scale: f32,
    pub elevation: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Camera {
    pub minimum_distance: i16,
    pub yaw_offset_degrees: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Attachment {
    pub draw_flags: u8,
}

pub(crate) fn cook_party(
    file: &Path,
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Option<Vec<String>>> {
    let Some((module, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    let offset = layout.party_settings;
    let length = layout
        .party_settings_end
        .checked_sub(offset)
        .context("reversed party settings table")?;
    ensure!(
        length > 0 && length % BYTES == 0,
        "incomplete party settings table"
    );
    let rel = Rel::read(file)?;
    let table = rel
        .at((5, offset))?
        .get(..length)
        .context("truncated party settings table")?;
    let mut characters = Vec::new();
    let mut records = Vec::new();
    for (index, bytes) in table.chunks_exact(BYTES).enumerate() {
        let character = index + 1;
        let missing = unresolved(bytes);
        characters.push(PartySettings {
            character: character.try_into()?,
            settings: ActorSettings::read(bytes)?,
        });
        records.push(json!({
            "character": character, "offset": offset + index * BYTES,
            "unresolved": missing.iter().map(|&at| json!({"offset": at, "value": bytes[at]})).collect::<Vec<_>>()
        }));
        if !missing.is_empty() {
            report(
                &format!("files/{module}/party-settings/{character}"),
                Err(anyhow::anyhow!(
                    "unresolved nonzero actor fields {missing:x?}"
                )),
            );
        }
    }
    embedded::write(
        file,
        output,
        "battle-party-settings",
        &serde_json::to_value(&characters)?,
        json!({
            "section": 5, "offset": offset, "end": layout.party_settings_end, "stride": BYTES, "records": records,
        }),
    )
    .map(Some)
}

pub(super) fn unresolved(bytes: &[u8]) -> Vec<usize> {
    const KNOWN: &[std::ops::Range<usize>] = &[
        0..11,
        0x10..0x29,
        0x2a..0x31,
        0x38..0x5a,
        0x5c..0xb6,
        0xbc..0xf8,
        0xfa..0x102,
        0x104..0x110,
        0x110..0x124,
        0x124..0x125,
        0x13c..0x13d,
        0x154..0x155,
        0x16c..0x16d,
        0x184..0x185,
        0x19c..0x19d,
        0x1b4..0x1b5,
        0x1cc..0x1cd,
        0x1e4..0x1e5,
        0x1e6..0x1e9,
        0x1ea..0x1ed,
    ];
    bytes
        .iter()
        .enumerate()
        .filter_map(|(at, &value)| {
            (value != 0 && !KNOWN.iter().any(|range| range.contains(&(at % BYTES)))).then_some(at)
        })
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BodyUv {
    Disabled,
    Frames,
    Scroll,
}

pub(super) fn parse(b: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(ActorSettings::read(b)?)?)
}

impl ActorSettings {
    pub(crate) fn read(b: &[u8]) -> Result<Self> {
        ensure!(b.len() >= BYTES, "truncated actor settings");
        ensure!(
            b[0x30] & !7 == 0,
            "unresolved casting motion flags {:#x}",
            b[0x30]
        );
        let vector =
            |at| -> Result<_> { Ok([float(b, at)?, float(b, at + 4)?, float(b, at + 8)?]) };
        let uv = |i: usize| -> Result<_> {
            Ok(UvChannel {
                mode: match b[0xfa + i] {
                    0 => BodyUv::Disabled,
                    1 => BodyUv::Frames,
                    2 => BodyUv::Scroll,
                    other => bail!("unknown actor body UV mode {other}"),
                },
                texture: b[0xfc + i],
                frame_count_or_u_step: b[0xfe + i] as i8,
                frame_delay_or_v_step: b[0x100 + i] as i8,
            })
        };
        Ok(Self {
            combat: Combat {
                attack_element: b[0],
                physical_affinity: b[1],
                element_affinities: b[2..11].try_into()?,
                condition_flags: [word(b, 0x10)?, word(b, 0x14)?],
                condition_immunity: [word(b, 0x18)?, word(b, 0x1c)?],
                walk_speed: float(b, 0x20)?,
                run_speed: float(b, 0x24)?,
                weight: b[0x28],
                family: half(b, 0x2a)?,
                turn_divisor: b[0x2c],
                stun_resistance: b[0x2d],
                stagger_threshold: b[0x2e],
                guard_reduction: b[0x2f],
                intrinsic_conditions: [word(b, 0x38)?, word(b, 0x3c)?],
                stagger_ticks: b[0x55],
                guard_pressure_limit: half(b, 0x58)?,
                flags: word(b, 0x5c)?,
                poise: b[0x11b],
                variant_count: b[0x1e7],
            },
            rewards: Rewards {
                experience: word(b, 0x40)?,
                gald: word(b, 0x44)?,
                drops: [half(b, 0x48)?, half(b, 0x4a)?],
                steal: half(b, 0x4c)?,
                drop_chances: b[0x4e..0x50].try_into()?,
                steal_chance: b[0x50],
                grade: half(b, 0x1ea)? as i16,
            },
            casting: Casting {
                base_ticks: half(b, 0x56)? as i16,
                loop_start: b[0x9d],
                animation_rate: float(b, 0xa0)?,
                command_index: half(b, 0xec)? as i16,
                effect_interval: b[0x96],
                resume_start: b[0xa4],
                resume_blend_ticks: b[0xa5],
                resume_loop_start: b[0xa8],
                stored_recovery_clip: b[0xa6],
                chant_looping: b[0x30] & 4 == 0,
                release_looping: b[0x30] & 2 != 0,
                stored_release_looping: b[0x30] & 1 != 0,
            },
            model: Model {
                target_bone: b[0x51],
                effect_offset: vector(0x60)?,
                stun_offset: vector(0x6c)?,
                target_bone_offset: vector(0x78)?,
                model_scale: float(b, 0x84)?,
                shadow_scale: float(b, 0x88)?,
                effect_scale: float(b, 0x8c)?,
                ground_offset: float(b, 0x90)?,
                idle_delay: b[0x94],
                idle_jitter: b[0x95],
                initial_clip: b[0x97],
                unused_reset_argument: b[0x99],
                entry_command_index: half(b, 0xf2)? as i16,
                entry_timeout_ticks: b[0x98],
                alpha: b[0x9b],
                death_clip: b[0x9c],
                guard_effect: b[0x9e],
                head_bone: b[0x9f],
                body_flags: half(b, 0xb4)?,
                secondary_body: b[0xbc],
                secondary_clip: b[0xbd],
                fixed_initial_clip: b[0xc1],
                shadow_color: b[0x108..0x10c].try_into()?,
                attachment_count: b[0x1e4],
                auxiliary_instance_count: b[0x1e6],
                effect_model_count: b[0x1e8],
                attachment_resource_count: b[0x1ec],
            },
            appearance: Appearance {
                channel_count: b[0xce],
                atlas_frames: b[0xcf..0xd3].try_into()?,
                atlas_textures: b[0xd3..0xd7].try_into()?,
                idle_face: b[0xd7..0xdb].try_into()?,
                attack_face: b[0xdb..0xdf].try_into()?,
                paralysis_face: b[0xdf..0xe3].try_into()?,
                stun_face: b[0xe3..0xe7].try_into()?,
                appearance_texture: b[0xee],
                appearance_rows: b[0xef],
                uv_channels: [uv(0)?, uv(1)?],
            },
            effects: Effects {
                overlimit_bone: b[0xac],
                overlimit_rate: b[0xad],
                overlimit_initial: half(b, 0xae)?,
                overlimit_scale: float(b, 0xb0)?,
                opening_warning: b[0xe7],
                locomotion_effect_period: b[0xea],
                locomotion_effect: b[0xeb],
                idle: Periodic {
                    interval: b[0xe8],
                    effect: b[0xe9],
                },
                periodic: Periodic {
                    interval: half(b, 0xbe)?,
                    effect: b[0xc0],
                },
                periodic_channels: std::array::from_fn(|i| Periodic {
                    interval: b[0xc2 + i],
                    effect: b[0xc8 + i],
                }),
                voice_bank: b[0xa9],
                voice_base: word(b, 0x104)?,
                victory_voice: half(b, 0xaa)?,
                death_voice: half(b, 0xf4)?,
                opening_voice: half(b, 0xf6)?,
            },
            body_trail: BodyTrail {
                rgba: b[0x10c..0x110].try_into()?,
                uv: [
                    half(b, 0x110)? as i16,
                    half(b, 0x112)? as i16,
                    half(b, 0x114)? as i16,
                    half(b, 0x116)? as i16,
                ],
                texture_bank: b[0x118] as i8,
                palette: b[0x119],
                render_flags: b[0x11a],
            },
            book_preview: BookPreview {
                scale: float(b, 0x11c)?,
                elevation: float(b, 0x120)?,
            },
            camera: Camera {
                minimum_distance: half(b, 0xf0)? as i16,
                yaw_offset_degrees: b[0xa7],
            },
            unused_storage: [0x52..0x55, 0x9a..0x9b].map(|range| crate::read::Storage {
                offset: range.start,
                bytes: b[range].to_vec(),
            }),
            attachments: std::array::from_fn(|i| Attachment {
                draw_flags: b[0x124 + i * 24],
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    fn unused_settings_are_retained_without_relaxing_other_fields() -> Result<()> {
        let mut bytes = [0; BYTES];
        bytes[0x52..0x55].copy_from_slice(&[255, 128, 1]);
        bytes[0x9a] = 254;
        assert_eq!(
            parse(&bytes)?["unused_storage"],
            json!([
                {"offset":0x52,"bytes":[255,128,1]}, {"offset":0x9a,"bytes":[254]}
            ])
        );
        assert!(unresolved(&bytes).is_empty());
        for offset in [0x31, 0x5a, 0xb6] {
            bytes[offset] = 1;
            assert_eq!(unresolved(&bytes), [offset]);
            bytes[offset] = 0;
        }
        bytes[0xfa] = 2;
        bytes[0xfe] = 0x80;
        bytes[0x100] = 0xff;
        bytes[0xec..0xee].copy_from_slice(&(-2_i16).to_be_bytes());
        bytes[0x1ea..0x1ec].copy_from_slice(&(-5_i16).to_be_bytes());
        bytes[0x124 + 7 * 24] = 7;
        let settings = ActorSettings::read(&bytes)?;
        let restored: ActorSettings = serde_json::from_slice(&serde_json::to_vec(&settings)?)?;
        assert_eq!(serde_json::to_value(&restored)?, parse(&bytes)?);
        assert_eq!(restored.casting.command_index, -2);
        assert_eq!(restored.rewards.grade, -5);
        assert_eq!(
            restored.appearance.uv_channels[0].frame_count_or_u_step,
            -128
        );
        assert_eq!(restored.appearance.uv_channels[0].frame_delay_or_v_step, -1);
        assert_eq!(restored.attachments[7].draw_flags, 7);
        Ok(())
    }

    #[test]
    #[ignore = "requires original battle modules; no media conversion"]
    fn original_party_settings_share_enemy_layout_across_all_modules() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let output = crate::temporary_path(&std::env::temp_dir().join("party-settings"));
        let mut shared = BTreeSet::new();
        let mut original = None;
        for disc in [1, 2] {
            let files = local.join(format!("extracted/disc{disc}/files"));
            let mut modules = 0;
            for file in fs::read_dir(&files)? {
                let file = file?.path();
                let mut missing = Vec::new();
                let Some(paths) = cook_party(&file, &output, &mut |path, result| {
                    missing.push((path.to_owned(), result.unwrap_err().to_string()));
                })?
                else {
                    continue;
                };
                modules += 1;
                assert!(missing.is_empty(), "{missing:?}");
                let manifest: Value = serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
                assert_eq!(manifest["source_sha256"], crate::digest(&fs::read(&file)?));
                let offset = manifest["offset"].as_u64().unwrap() as usize;
                let end = manifest["end"].as_u64().unwrap() as usize;
                let rel = Rel::read(&file)?;
                assert_eq!(end - offset, 11 * BYTES);
                assert!(rel.local_targets().contains(&(5, offset)));
                assert!(rel.local_targets().contains(&(5, end)));
                let table = &rel.at((5, offset))?[..end - offset];
                assert_eq!(table.iter().filter(|&&value| value != 0).count(), 670);
                if let Some(original) = &original {
                    assert_eq!(table, original);
                } else {
                    original = Some(table.to_vec());
                }
                let data: Vec<Value> = serde_json::from_slice(&fs::read(output.join(&paths[0]))?)?;
                assert_eq!(data.len(), 11);
                for (index, bytes) in table.chunks_exact(BYTES).enumerate() {
                    assert_eq!(manifest["records"][index]["offset"], offset + index * BYTES);
                    assert_eq!(manifest["records"][index]["unresolved"], json!([]));
                    assert_eq!(data[index]["character"], index + 1);
                    // Compare through the same serialized representation as the file.
                    let expected: Value =
                        serde_json::from_slice(&serde_json::to_vec(&parse(bytes)?)?)?;
                    assert_eq!(data[index]["settings"], expected);
                    assert!(unresolved(bytes).is_empty());
                    assert_eq!(
                        data[index]["settings"]["unused_storage"],
                        json!([
                            {"offset":0x52,"bytes":&bytes[0x52..0x55]},
                            {"offset":0x9a,"bytes":&bytes[0x9a..0x9b]}
                        ])
                    );
                    assert!(parse(&bytes[..BYTES - 1]).is_err());
                }
                if file.file_name().unwrap() == "US_r_Top2Btl.rel" {
                    fs::write(
                        output.join("sources.json"),
                        serde_json::to_vec(&json!({
                            format!("disc{disc}/US_r_Top2Btl.rel"): paths
                        }))?,
                    )?;
                    let mut cooked: Vec<PartySettings> =
                        crate::cooked::Source::open(&output, disc, "US_r_Top2Btl.rel")?
                            .embedded("battle-party-settings", "US_r_Top2Btl.rel")?;
                    let traits =
                        crate::battle::party_traits(&cooked, &[1, 2, 3, 4, 5, 6, 7, 8, 9])?;
                    for row in &cooked[..9] {
                        let bytes = &table[usize::from(row.character - 1) * BYTES..];
                        assert_eq!(
                            traits[&row.character].movement.walk_speed,
                            float(bytes, 0x20)?
                        );
                        assert_eq!(
                            traits[&row.character].casting_base,
                            half(bytes, 0x56)? as i16
                        );
                        assert_eq!(
                            traits[&row.character].defense.quarter_damage,
                            half(bytes, 0xb4)? & 0x3000 != 0
                        );
                    }
                    cooked[0].settings.combat.walk_speed = 17.;
                    assert_eq!(
                        crate::battle::party_traits(&cooked, &[1])?[&1]
                            .movement
                            .walk_speed,
                        17.
                    );
                    assert!(crate::battle::party_traits(&cooked[..1], &[2]).is_err());
                }
                shared.insert(paths[0].clone());
            }
            assert_eq!(modules, 7);
        }
        assert_eq!(shared.len(), 1);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}
