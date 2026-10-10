//! Decode shared actor profiles and party templates.
use crate::{read::Field, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_conditions::{Condition, ConditionSet};
use resonance_content::battle_profile::{
    Blink, Profile, ProfileTraits, Table, VoicePolicy, VoiceSequence, WeaponStyle,
};
use std::path::Path;

pub(crate) const BYTES: usize = 496;

pub(crate) fn read(
    bytes: &[u8],
    voices: &crate::battle_voice::Source,
    roles: usize,
) -> Result<Profile> {
    let b = bytes
        .get(..BYTES)
        .context("truncated battle actor profile")?;
    let channels = usize::from(b[0xce]);
    ensure!(channels <= 4, "too many actor texture channels");
    let weapons = usize::from(b[0x1e4]);
    ensure!(weapons <= 8, "too many actor weapon slots");
    let texture_channels: Vec<_> = (0..channels)
        .map(|index| resonance_content::battle_profile::TextureChannel {
            texture: b[0xd3 + index],
            frames: b[0xcf + index],
        })
        .collect();
    let blink =
        (u32::read(b, 0x5c)? & 0x8000 != 0 && !texture_channels.is_empty()).then(|| Blink {
            channel: 0,
            frames: [1, 2],
            excluded_expressions: vec![2, 3, 10],
        });
    if let Some(blink) = &blink {
        blink.validate(&texture_channels)?;
    }
    Ok(Profile {
        walk_speed: f32::read(b, 0x20)?,
        run_speed: f32::read(b, 0x24)?,
        turn_ticks: b[0x2c],
        idle_ticks: b[0x94],
        idle_variation: b[0x95],
        initial_motion: b[0x97],
        initial_motion_override: b[0xc1],
        body_alpha: b[0x9b],
        texture_channels,
        blink,
        idle_expression: std::array::from_fn(
            |index| if index < channels { b[0xd7 + index] } else { 0 },
        ),
        rescue_expression: std::array::from_fn(
            |index| if index < channels { b[0xdf + index] } else { 0 },
        ),
        weapon_styles: std::array::from_fn(|slot| {
            let flags = b[0x124 + slot * 24];
            WeaponStyle {
                before_body: flags & 1 != 0,
                toon: flags & 2 != 0,
                additive: flags & 0x10 != 0,
            }
        }),
        initial_conditions: conditions(<[u32; 2]>::read(b, 0x10)?, false)
            .context("initial conditions")?,
        intrinsic_conditions: conditions(<[u32; 2]>::read(b, 0x38)?, false)
            .context("intrinsic conditions")?,
        immunities: conditions(<[u32; 2]>::read(b, 0x18)?, true).context("immunities")?,
        weight: b[0x28],
        species: u16::read(b, 0x2a)?,
        stun_resistance: b[0x2d],
        stagger_threshold: b[0x2e],
        guard_reduction: b[0x2f],
        stagger_ticks: b[0x55],
        guard_pressure_limit: i16::read(b, 0x58)?.max(0) as u32,
        traits: profile_traits(u32::read(b, 0x5c)?, u16::read(b, 0xb4)?),
        armor: b[0x11b],
        center_offset: <[f32; 3]>::read(b, 0x60)?,
        model_scale: f32::read(b, 0x84)?,
        shadow_scale: f32::read(b, 0x88)?,
        shadow_color: <[u8; 4]>::read(b, 0x108)?,
        effect_scale: f32::read(b, 0x8c)?,
        camera_category: b[0xe7],
        voices: voices.actor_voices(u32::read(b, 0x104)?, roles)?,
        death_motion: b[0x9c],
        overlimit_gain: b[0xad],
        initial_overlimit: i16::read(b, 0xae)?,
        ground_offset: f32::read(b, 0x90)?,
        cast_ticks: i16::read(b, 0x56)?,
    })
}

fn conditions([high, low]: [u32; 2], immunity: bool) -> Result<ConditionSet> {
    use Condition::*;
    let mut flags = (u64::from(high) << 32) | u64::from(low);
    let mut conditions = ConditionSet::EMPTY;
    for (mask, condition) in [
        (1, PoisonMild),
        (1 << 1, PoisonSevere),
        (1 << 2, Stun),
        (1 << 3, Paralysis),
        (1 << 4, Weak),
        (1 << 5, Petrified),
        (1 << 7, Curse),
        (1 << 8, ReduceItemEffect),
        (1 << 9, Heavy),
        ((1 << 10) | (1 << 11), PhysicalAffliction),
        (1 << 12, AttackUp),
        (1 << 13, DefenseUp),
        (1 << 14, AccuracyUp),
        (1 << 15, MagicAttackUp),
        (1 << 16, MagicDefenseUp),
        (1 << 17, AttackDown),
        (1 << 18, DefenseDown),
        (1 << 19, AccuracyDown),
        (1 << 20, MagicAttackDown),
        (1 << 21, MagicDefenseDown),
        (1 << 22, EvasionDown),
        (1 << 23, PhysicalProtection),
        (1 << 24, MagicalProtection),
        (1 << 25, Revive),
        (1 << 26, DefenseHalved),
        (1 << 27, RegenerateHp),
        (1 << 28, RegenerateTp),
        (1 << 29, Acuity),
        (1 << 30, Flare),
        (1 << 31, Guard),
        (1 << 32, Quartz),
        (1 << 37, Enchanted),
        (1 << 40, ShortStun),
        (1 << 41, AilmentResistance),
        (1 << 42, KillHpRecovery),
        (1 << 43, KillTpRecovery),
        (1 << 44, TpThird),
        (1 << 45, TpHalf),
        (1 << 47, CastingSpeed),
        (1 << 56, MovementBoost),
    ] {
        if flags & mask == 0 {
            continue;
        }
        ensure!(
            immunity || !matches!(condition, Stun | ShortStun),
            "{condition:?} is only supported as immunity"
        );
        conditions = conditions.union(condition.into());
        flags &= !mask;
    }
    ensure!(flags == 0, "unsupported profile condition flags {flags:#x}");
    Ok(conditions)
}

fn profile_traits(flags: u32, body: u16) -> ProfileTraits {
    ProfileTraits {
        flying: flags & 0x1 != 0,
        hover_bobbing: flags & 0x2 != 0,
        hide_enemy_shadow: flags & 0x8 != 0,
        turning_disabled: flags & 0x80 != 0,
        inactive_item_target: flags & 0x100 != 0,
        knockdown_immune: flags & 0x200 != 0,
        clear_pending_on_hit: flags & 0x400 != 0,
        show_body_attachments: flags & 0x1000 != 0,
        auto_guard_disabled: flags & 0x4000 != 0,
        retain_defeated_body: flags & 0x40000 != 0,
        recover_in_air: flags & 0x100000 != 0,
        launch_immune: flags & 0x2000000 != 0,
        body_motion_disabled: flags & 0x8000000 != 0,
        push_immovable: flags & 0x10000000 != 0,
        unrestricted_arena: flags & 0x40000000 != 0,
        enemy_contact_recovery: flags & 0x80000000 != 0,
        passes_allied_obstacles: body & 0x1 != 0,
        passable_for_allies: body & 0x20 != 0,
        suppress_hurt_motion: body & 0x20 != 0,
        push_obstacle_disabled: body & 0x100 != 0,
        item_target_excluded: body & 0x200 != 0,
        secondary_body_entry: body & 0x400 != 0,
        fixed_height: body & 0x4000 != 0,
    }
}

pub fn publish_party(file: &Path, usual: &[u8], output: &Path, prefix: &str) -> Result<String> {
    let voices = crate::battle_voice::read(usual)?;
    let module = Rel::read(file)?;
    let bytes = module
        .at((5, 0x3d30))?
        .get(..11 * BYTES)
        .context("truncated party profile table")?;
    let table = Table {
        source_sha256: crate::digest(&module.bytes),
        contact_sounds: resonance_content::battle_profile::ContactSounds {
            party: (0..9)
                .map(|index| u16::read(module.at((4, 0x1d30))?, index * 2))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .unwrap(),
            elements: module
                .at((5, 0x1404))?
                .get(..9)
                .context("truncated element contact sounds")?
                .try_into()
                .unwrap(),
        },
        overlimit_voices: (0..9)
            .map(|index| {
                Ok(crate::battle_voice::sound(u16::read(
                    module.at((4, 0x1264))?,
                    index * 2,
                )?))
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        default_strategy: {
            let columns = module
                .at((4, 0x10dc))?
                .get(..30)
                .context("truncated strategy defaults")?;
            std::array::from_fn(|character| {
                std::array::from_fn(|kind| columns[kind * 10 + character])
            })
        },
        companion_policy: resonance_content::battle_profile::CompanionPolicy {
            tp_limits: <[u8; 9]>::read(module.at((4, 0x2860))?, 0)?,
            healing_limits: <[u8; 9]>::read(module.at((4, 0x286c))?, 0)?,
            support_level_limits: <[u8; 9]>::read(module.at((4, 0x2878))?, 0)?.map(|v| v as i8),
        },
        entry: entry(&module)?,
        voice_sequences: (0..10)
            .map(|character| {
                voice_sequence(
                    &module,
                    character,
                    u32::read(bytes, character * BYTES + 0x104)?,
                )
            })
            .collect::<Result<_>>()?,
        death_voice_pairs: module
            .at((4, 0x177c))?
            .get(..28)
            .context("truncated ally death voice pairs")?
            .chunks_exact(2)
            .map(|pair| [pair[0], pair[1]])
            .collect(),
        lethal_rescue_names: [0x1dd0, 0x1de0, 0x1de8, 0x1df4, 0x1e08]
            .map(|offset| module.text((4, offset)))
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        records: bytes
            .chunks_exact(BYTES)
            .map(|bytes| read(bytes, &voices, crate::battle_voice::PARTY_ROLES))
            .collect::<Result<_>>()?,
    };
    let path = format!("{prefix}/party-profiles.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&table)?)?;
    Ok(path)
}

fn entry(module: &Rel) -> Result<resonance_content::battle_profile::Entry> {
    let read = |offset| f32::read(module.at((4, offset))?, 0);
    Ok(resonance_content::battle_profile::Entry {
        replacement_motion_rate: read(0x2a04)?,
        fade_color: <[u8; 3]>::read(module.at((4, 0x148))?, 0)?,
    })
}

fn voice_sequence(module: &Rel, character: usize, base: u32) -> Result<VoicePolicy> {
    use resonance_content::battle_voice::Sound;
    let default = if base == 0 {
        VoiceSequence::default()
    } else {
        let index = u16::try_from(base.checked_add(7).context("chant voice overflow")?)?;
        ensure!(index < 0x8000, "chant voice exceeds source domain");
        VoiceSequence {
            chant: Some(Sound::Stream(index)),
            self_chant: Some(Sound::Stream(index)),
            release: None,
            fallback: Some(Sound::Cue(index + 501)),
        }
    };
    let mut policy = VoicePolicy {
        default,
        techniques: Default::default(),
    };
    let address = (5, 0x59d8 + character * 4);
    if let Some(&pointer) = module.pointers.get(&address) {
        let mut terminated = false;
        for row in module.at(pointer)?.chunks_exact(6) {
            let technique = u16::read(row, 0)?;
            if technique == 0 {
                terminated = true;
                break;
            }
            let chant = crate::battle_voice::sound(u16::read(row, 2)?).or(default.chant);
            policy.techniques.insert(
                technique,
                VoiceSequence {
                    chant,
                    self_chant: if character == 1 {
                        chant
                    } else {
                        default.self_chant
                    },
                    release: crate::battle_voice::sound(u16::read(row, 4)?),
                    fallback: default.fallback,
                },
            );
        }
        ensure!(terminated, "unterminated technique voice table");
    } else {
        ensure!(
            u32::read(module.at(address)?, 0)? == 0,
            "invalid voice table pointer"
        );
    }
    if character == 1 && base != 0 {
        for (technique, stream) in [(268, 232), (269, 231)] {
            policy
                .techniques
                .entry(technique)
                .or_insert(default)
                .fallback = Some(Sound::Stream(stream));
        }
    }
    Ok(policy)
}

#[cfg(test)]
mod tests;
