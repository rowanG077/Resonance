//! Native actor profile bytes directly into editable battle profiles.
use crate::{dol, native_archive};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_conditions::{Condition, ConditionSet},
    battle_profile::{
        Casting, Expression, ExpressionTexture, FacialTexture, HeadAnchor, HostedBody, Profile,
        ProfileTraits, RecurringEffect, Shadow, TargetAnchor, TargetOrigin, WeaponStyle,
    },
    battle_voice::{Sound, Voices},
};

fn half(b: &[u8], at: usize) -> Result<u16> {
    dol::u16(b, at)
}
fn signed(b: &[u8], at: usize) -> Result<i16> {
    Ok(half(b, at)? as i16)
}
fn word(b: &[u8], at: usize) -> Result<u32> {
    dol::u32(b, at)
}
fn float(b: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_bits(word(b, at)?))
}
fn vector(b: &[u8], at: usize) -> Result<[f32; 3]> {
    Ok([float(b, at)?, float(b, at + 4)?, float(b, at + 8)?])
}

fn conditions(high: u32, low: u32, immunity: bool) -> Result<ConditionSet> {
    use Condition::*;
    let mut flags = (u64::from(high) << 32) | u64::from(low);
    let mut result = ConditionSet::EMPTY;
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
        (1 << 33, PhysicalBarrier),
        (1 << 34, MagicalBarrier),
        (1 << 35, HolySong),
        (1 << 36, RecoveryDown),
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
        result = result.union(condition.into());
        flags &= !mask;
    }
    ensure!(flags == 0, "unsupported profile condition flags {flags:#x}");
    Ok(result)
}

fn traits(flags: u32, body: u16) -> ProfileTraits {
    ProfileTraits {
        hosts_participants: body & 1 != 0,
        flying: flags & 0x1 != 0,
        hover_bobbing: flags & 0x2 != 0,
        turning_disabled: flags & 0x80 != 0,
        inactive_item_target: flags & 0x100 != 0,
        knockdown_immune: flags & 0x200 != 0,
        clear_pending_on_hit: flags & 0x400 != 0,
        show_body_attachments: flags & 0x1000 != 0,
        auto_guard_disabled: flags & 0x4000 != 0,
        retain_defeated_body: flags & 0x40000 != 0,
        wait_for_defeat_animation: flags & 0x200000 != 0,
        hit_shake_disabled: flags & 0x80000 != 0,
        recover_in_air: flags & 0x100000 != 0,
        launch_immune: flags & 0x2000000 != 0,
        immune_to_unison: flags & 0x4000000 != 0,
        body_motion_disabled: flags & 0x8000000 != 0,
        push_immovable: flags & 0x10000000 != 0,
        unrestricted_arena: flags & 0x40000000 != 0,
        enemy_contact_recovery: flags & 0x80000000 != 0,
        passes_allied_obstacles: body & 1 != 0,
        passable_for_allies: body & 0x20 != 0,
        suppress_hurt_motion: body & 0x20 != 0,
        push_obstacle_disabled: body & 0x100 != 0,
        item_target_excluded: body & 0x200 != 0,
        secondary_body_entry: body & 0x400 != 0,
        fixed_height: body & 0x4000 != 0,
    }
}

fn voices(usual: &[u8], base: u32, party: bool) -> Result<Voices> {
    let streams =
        native_archive::member(usual, 11, 14)?.context("missing battle voice stream table")?;
    let durations =
        native_archive::member(usual, 12, 14)?.context("missing battle voice duration table")?;
    ensure!(
        !durations.is_empty()
            && durations.len().is_multiple_of(2)
            && durations.len() / 2 <= streams.len() * 8
            && durations.len() / 2 <= 32768,
        "invalid battle voice data"
    );
    let base = usize::try_from(base)?;
    let count = if party { 58 } else { 10 };
    let end = base
        .checked_add(count)
        .context("actor voice range overflow")?;
    ensure!(end <= durations.len() / 2, "missing enemy voices");
    let line = |at: usize| -> Option<Sound> {
        if at >= count {
            return None;
        }
        let index = base + at;
        Some(if streams[index / 8] & (1 << (index % 8)) != 0 {
            Sound::Stream(index as u16)
        } else {
            Sound::Cue(index as u16 + 501)
        })
    };
    Ok(Voices {
        hurt: [line(3), line(4)],
        defeat: line(6),
        critical: line(8),
        guard: line(9),
        ally_defeated: [line(10), line(11)],
        unison_start: line(12),
        technique_command: line(13),
        item_use: line(14),
        acquisition: line(15),
        entry: [line(16), line(17), line(18)],
        major_enemy: line(19),
        outnumbered: line(20),
        weaker_enemies: line(21),
        stronger_enemies: line(22),
        repeated_encounter: line(23),
        victory: (28..38)
            .step_by(2)
            .filter_map(|i| line(i).zip(line(i + 1)).map(|(a, b)| [a, b]))
            .collect(),
        interrupted_cast: line(49),
        escape_request: line(51),
        escape_success: line(52),
        escape_cancel: line(53),
        taunt: line(56),
        contact_recovery: line(57),
    })
}

fn category<T: Copy>(items: &[T], index: usize, label: &str) -> Result<T> {
    items
        .get(index)
        .copied()
        .with_context(|| format!("unknown enemy {label} {index}"))
}

/// `data` is the profile section of one native enemy package. The resident
/// BTLusual archive is only needed for the 76 enemies with a nonzero voice base.
pub(crate) fn profile(data: &[u8], nodes: &[String], usual: Option<&[u8]>) -> Result<Profile> {
    read_profile(data, Some(nodes), usual)
}

pub(crate) fn party_profile(data: &[u8], usual: &[u8]) -> Result<Profile> {
    let mut profile = read_profile(data, None, Some(usual))?;
    let anchors = crate::world_gpl::animation_target_bindings(vec![
        (0, profile.head_anchor.bone),
        (1, profile.overlimit_bone),
    ]);
    profile.head_anchor.bone = anchors[0].1.clone();
    profile.overlimit_bone = anchors[1].1.clone();
    Ok(profile)
}

fn read_profile(data: &[u8], nodes: Option<&[String]>, usual: Option<&[u8]>) -> Result<Profile> {
    use resonance_content::battle_profile::{RecoilWeight, Size, Species};
    let b = data.get(..496).context("truncated actor profile")?;
    let party = nodes.is_none();
    let nodes = nodes.unwrap_or(&[]);
    let node = |index: u8| -> Result<String> {
        Ok(nodes
            .get(usize::from(index))
            .or_else(|| nodes.first())
            .context("enemy profile has no body joints")?
            .clone())
    };
    let head = HeadAnchor {
        bone: if party {
            "Bone_atama".into()
        } else {
            node(b[0x9f])?
        },
        offset: vector(b, 0x6c)?,
    };
    ensure!(!party || b[0x51] == 0, "unsupported party target anchor");
    let target = if b[0x51] == 0 {
        TargetAnchor::Reference(TargetOrigin::Center)
    } else {
        let joint = HeadAnchor {
            bone: node(b[0x51])?,
            offset: vector(b, 0x78)?,
        };
        if joint == head {
            TargetAnchor::Reference(TargetOrigin::Head)
        } else {
            TargetAnchor::Joint(joint)
        }
    };
    // Unlike the two anchors, this index must exist: its burst is attached to
    // the selected rig joint at runtime.
    let overlimit_bone = if party {
        "Bone_sebone01".into()
    } else {
        nodes
            .get(usize::from(b[0xac]))
            .context("enemy Over Limit joint is missing")?
            .clone()
    };
    let channels = usize::from(b[0xce]);
    ensure!(channels <= 4, "too many enemy expression channels");
    let weapons = usize::from(b[0x1e4]);
    ensure!(weapons <= 8, "too many enemy weapon slots");
    ensure!(
        b[0xeb] == 0 || b[0xea] != 0,
        "invalid movement effect interval"
    );
    let flags = word(b, 0x5c)?;
    let expressions = (0..channels)
        .map(|index| Expression {
            texture: match (party, index) {
                (true, 0) => ExpressionTexture::Role(FacialTexture::Eyes),
                (true, 1) => ExpressionTexture::Role(FacialTexture::Mouth),
                _ => ExpressionTexture::Slot(usize::from(b[0xd3 + index])),
            },
            frames: b[0xcf + index],
            idle: b[0xd7 + index],
            active: b[0xdb + index],
            rescue: b[0xdf + index],
            blink: (index == 0 && flags & 0x8000 != 0).then_some([1, 2]),
            blink_exclude: vec![2, 3, 10],
        })
        .collect();
    let body = half(b, 0xb4)?;
    let color: [u8; 4] = b[0x108..0x10c].try_into()?;
    let shadow = (color[3] != 0 && (party || flags & 8 == 0))
        .then(|| float(b, 0x88).map(|scale| Shadow { scale, color }))
        .transpose()?;
    let rate = float(b, 0xa0)?;
    let voice_base = word(b, 0x104)?;
    let profile = Profile {
        walk_speed: float(b, 0x20)?,
        run_speed: float(b, 0x24)?,
        turn_ticks: b[0x2c],
        idle_ticks: b[0x94],
        idle_variation: b[0x95],
        initial_motion: b[0x97],
        initial_motion_override: b[0xc1],
        body_alpha: b[0x9b],
        expressions,
        weapon_styles: std::array::from_fn(|slot| {
            let flags = b[0x124 + slot * 24];
            WeaponStyle {
                before_body: flags & 1 != 0,
                toon: flags & 2 != 0,
                additive: flags & 0x10 != 0,
            }
        }),
        initial_conditions: conditions(word(b, 0x10)?, word(b, 0x14)?, false)?,
        intrinsic_conditions: conditions(word(b, 0x38)?, word(b, 0x3c)?, false)?,
        immunities: conditions(word(b, 0x18)?, word(b, 0x1c)?, true)?,
        recoil_weight: category(
            &[
                RecoilWeight::Normal,
                RecoilWeight::Light,
                RecoilWeight::Heavy,
                RecoilWeight::Grounded,
            ],
            usize::from(b[0x28]),
            "recoil weight",
        )?,
        species: category(
            &[
                Species::Unknown,
                Species::Plant,
                Species::Undead,
                Species::Beast,
                Species::Magical,
                Species::Machine,
                Species::Human,
                Species::Bird,
                Species::Insect,
                Species::Aquatic,
                Species::Immaterial,
                Species::Dragon,
                Species::Angel,
            ],
            usize::from(half(b, 0x2a)?),
            "species",
        )?,
        stun_resistance: b[0x2d],
        stagger_threshold: b[0x2e],
        guard_reduction: b[0x2f],
        stagger_ticks: b[0x55],
        guard_pressure_limit: signed(b, 0x58)?.max(0) as u32,
        traits: traits(flags, body),
        hosted_body: (body & 0x20 != 0).then_some(HostedBody {
            release_on_hurt: body & 0x40 != 0,
            release_below_half_health: body & 0x2000 != 0,
            release_on_defeat: body & 0x1000 != 0,
            launch_on_release: body & 0x80 != 0,
        }),
        armor: b[0x11b],
        center_offset: vector(b, 0x60)?,
        target_anchor: target,
        head_anchor: head,
        model_scale: float(b, 0x84)?,
        shadow,
        effect_scale: float(b, 0x8c)?,
        passive_effects: std::iter::once((half(b, 0xbe)?, b[0xc0]))
            .chain((0..6).map(|slot| (u16::from(b[0xc2 + slot]), b[0xc8 + slot])))
            .filter(|(interval, _)| *interval != 0)
            .map(|(interval, program)| RecurringEffect {
                program: u16::from(program),
                interval,
            })
            .collect(),
        idle_effect: (b[0xe8] != 0).then_some(RecurringEffect {
            program: u16::from(b[0xe9]),
            interval: u16::from(b[0xe8]),
        }),
        movement_effect: (b[0xeb] != 0).then_some(RecurringEffect {
            program: u16::from(b[0xeb]),
            interval: u16::from(b[0xea]),
        }),
        size: category(
            &[Size::Small, Size::Medium, Size::Large, Size::Huge],
            usize::from(b[0xe7]),
            "size",
        )?,
        voices: (voice_base != 0)
            .then(|| {
                voices(
                    usual.context("voiced enemy needs resident battle archive")?,
                    voice_base,
                    party,
                )
            })
            .transpose()?,
        death_motion: b[0x9c],
        overlimit_bone,
        overlimit_scale: float(b, 0xb0)?,
        overlimit_gain: b[0xad],
        initial_overlimit: signed(b, 0xae)?,
        ground_offset: float(b, 0x90)?,
        casting: Casting {
            base_ticks: signed(b, 0x56)?,
            chant_once: b[0x30] & 4 != 0,
            chant_loop_start: f32::from(b[0x9d]) * rate.abs(),
            recovery_motion: (b[0xa6] != 0).then_some(b[0xa6]),
            opening_clip: 13,
            opening_start: 0.,
            opening_end: None,
            opening_rate: 0.5,
            opening_blend: 4,
            opening_repeat: b[0x30] & 1 != 0,
            release_clip: 12,
            release_start: f32::from(b[0xa4]) * rate,
            release_end: None,
            release_rate: rate,
            release_blend: b[0xa5],
            release_repeat: b[0x30] & 2 != 0,
            release_loop_start: f32::from(b[0xa8]) * rate,
        },
    };
    profile.blinks()?;
    Ok(profile)
}
