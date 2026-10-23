//! Decode enemy actions into semantic timelines during cooking.
use crate::read::Field;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_action::*;

/// Pose offsets count updates at the selected speed, not fixed clip frames.
pub(crate) fn action_pose(row: &[u8]) -> Result<ActionPose> {
    let rate = f32::read(row, 8)?;
    Ok(ActionPose {
        clip: u16::from(row[2]),
        start: f32::from(row[4]) * rate.abs(),
        end: (row[5] != 0).then_some(f32::from(row[5]) * rate.abs()),
        rate,
        blend: row[3],
        repeat: row[6] & 0x40 != 0,
    })
}

fn section(bytes: &[u8], field: usize) -> Result<(usize, &[u8])> {
    let start = usize::from(u16::read(bytes, field)?);
    ensure!(start >= 488, "invalid enemy action section");
    let mut end = bytes.len();
    for offset in (4..20)
        .step_by(2)
        .map(|at| u16::read(bytes, at).map(usize::from))
        .chain(
            (24..488)
                .step_by(4)
                .map(|at| u32::read(bytes, at).map(|v| v as usize)),
        )
    {
        let offset = offset?;
        if offset > start {
            end = end.min(offset);
        }
    }
    Ok((
        start,
        bytes
            .get(start..end)
            .context("enemy action section outside package")?,
    ))
}

const STAY_AFTER_ACTION: u32 = 0x20;
const RUN_APPROACH: u32 = 0x40;
const CONTACT_RECOVERY: u32 = 0x100;
const TARGET_ALLIES: u32 = 0x200;
const LEAVE_ARENA: u32 = 0x0200_0000;

fn requirements(flags: u32, count_limit: u8, target_state: u16) -> Result<EnemyRequirements> {
    const NORMAL_ONLY: u32 = 0x2000_0000;
    const HARD_OR_MANIA: u32 = 4;
    const MANIA_ONLY: u32 = 8;
    const HP_THREE_QUARTERS: u32 = 0x10_0000;
    const HP_HALF: u32 = 2;
    const HP_QUARTER: u32 = 1;
    const PRIORITY: u32 = 0x8_0000;
    const ONCE_PER_BATTLE: u32 = 0x10;
    const MAX_ENEMIES: u32 = 0x4000;
    const ALLY_ABSENT: u32 = 0x800;
    const MIN_OPPONENTS: u32 = 0x2000;
    const MAX_OPPONENTS: u32 = 0x8000;
    const MIN_ORIGIN_DISTANCE: u32 = 0x1000_0000;
    const MAX_ORIGIN_DISTANCE: u32 = 0x8000_0000;
    const DO_NOT_REPEAT: u32 = 0x4000_0000;
    const TARGET_STATE: u32 = 0x1_0000;
    const FOLLOWUP_ONLY: u32 = 0x20_0000;
    const FOLLOWUP_HIT: u32 = 0x40_0000;
    const MIN_NEARBY_OPPONENTS: u32 = 0x80_0000;
    const MIN_TARGETING_OPPONENTS: u32 = 0x100_0000;
    // Accepted input metadata that does not restrict action selection.
    const UNUSED_INPUT_FLAG: u32 = 0x2_0000;
    ensure!(
        flags
            & !(NORMAL_ONLY
                | HARD_OR_MANIA
                | MANIA_ONLY
                | HP_THREE_QUARTERS
                | HP_HALF
                | HP_QUARTER
                | PRIORITY
                | ONCE_PER_BATTLE
                | MAX_ENEMIES
                | ALLY_ABSENT
                | MIN_OPPONENTS
                | MAX_OPPONENTS
                | MIN_ORIGIN_DISTANCE
                | MAX_ORIGIN_DISTANCE
                | DO_NOT_REPEAT
                | TARGET_STATE
                | FOLLOWUP_ONLY
                | FOLLOWUP_HIT
                | MIN_NEARBY_OPPONENTS
                | MIN_TARGETING_OPPONENTS
                | UNUSED_INPUT_FLAG
                | STAY_AFTER_ACTION
                | RUN_APPROACH
                | CONTACT_RECOVERY
                | TARGET_ALLIES
                | LEAVE_ARENA)
            == 0,
        "unsupported enemy eligibility flags {flags:#x}"
    );
    // Every authored use has a zero minimum, so it imposes no restriction.
    ensure!(
        flags & (MIN_NEARBY_OPPONENTS | MIN_TARGETING_OPPONENTS) == 0 || count_limit == 0,
        "nonzero enemy nearby or targeting count is not prepared"
    );
    ensure!(
        flags & TARGET_STATE == 0 || matches!(target_state, 3 | 5),
        "unsupported enemy target state {target_state}"
    );
    let mut predicates = Vec::new();
    if flags & ALLY_ABSENT != 0 {
        predicates.push(EnemyPredicate::AllyAbsent(u16::from(count_limit)));
    }
    if flags & DO_NOT_REPEAT != 0 {
        predicates.push(EnemyPredicate::DoNotRepeat);
    }
    if flags & (MIN_ORIGIN_DISTANCE | MAX_ORIGIN_DISTANCE) != 0 {
        ensure!(
            target_state <= i16::MAX as u16,
            "negative enemy origin distance"
        );
        predicates.push(EnemyPredicate::OriginDistance {
            minimum: (flags & MIN_ORIGIN_DISTANCE != 0).then_some(target_state),
            maximum: (flags & MAX_ORIGIN_DISTANCE != 0).then_some(target_state),
        });
    }
    if flags & (MIN_OPPONENTS | MAX_OPPONENTS) != 0 {
        ensure!(
            count_limit <= i8::MAX as u8,
            "negative enemy opponent count"
        );
        predicates.push(EnemyPredicate::OpponentCount {
            minimum: (flags & MIN_OPPONENTS != 0).then_some(count_limit),
            maximum: (flags & MAX_OPPONENTS != 0).then_some(count_limit),
        });
    }
    if flags & TARGET_STATE != 0 && target_state == 3 {
        predicates.push(EnemyPredicate::TargetAttacking);
    }
    Ok(EnemyRequirements {
        target_guarding: flags & TARGET_STATE != 0 && target_state == 5,
        followup_only: flags & FOLLOWUP_ONLY != 0,
        followup_requires_hit: flags & FOLLOWUP_HIT != 0,
        difficulty: (if flags & MANIA_ONLY != 0 {
            2
        } else {
            u8::from(flags & HARD_OR_MANIA != 0)
        })..=(if flags & NORMAL_ONLY != 0 { 0 } else { 2 }),
        hp_percent: if flags & HP_QUARTER != 0 {
            Some(25)
        } else if flags & HP_HALF != 0 {
            Some(50)
        } else if flags & HP_THREE_QUARTERS != 0 {
            Some(75)
        } else {
            None
        },
        priority: flags & PRIORITY != 0,
        once_per_battle: flags & ONCE_PER_BATTLE != 0,
        max_enemies: (flags & MAX_ENEMIES != 0)
            .then(|| u8::try_from(count_limit as i8).context("negative enemy count limit"))
            .transpose()?,
        predicates,
    })
}

fn action(
    row: &[u8],
    package: &ActionPackage<'_>,
    policy_ticks: &mut [u16],
) -> Result<EnemyAction> {
    let mut unsupported_reason = None;
    let required_flag = u16::read(row, 58)?;
    let flags = u32::read(row, 8)?;
    let approach = approach(row, flags).unwrap_or_else(|error| {
        unsupported_reason.get_or_insert_with(|| error.to_string());
        EnemyApproach::default()
    });
    let requirements = requirements(flags, row[51], u16::read(row, 12)?).unwrap_or_else(|error| {
        unsupported_reason.get_or_insert_with(|| error.to_string());
        EnemyRequirements::default()
    });
    let weight = u8::try_from(row[0] as i8).unwrap_or_else(|_| {
        if !requirements.priority {
            unsupported_reason.get_or_insert_with(|| "negative enemy action weight".into());
        }
        // Priority choices bypass the weighted pool.
        0
    });
    let target_policy = if row[1] == 0 {
        None
    } else {
        match TargetPolicy::try_from(row[1]) {
            Ok(policy) => Some(policy),
            Err(error) => {
                unsupported_reason.get_or_insert_with(|| error.to_string());
                None
            }
        }
    };
    let execution = if flags & LEAVE_ARENA != 0 {
        Some(EnemyExecution::LeaveArena)
    } else {
        match package.execution(row, policy_ticks) {
            Ok(execution) => Some(execution),
            Err(error) => {
                unsupported_reason.get_or_insert_with(|| error.to_string());
                None
            }
        }
    };
    Ok(EnemyAction {
        weight,
        required_flag: (required_flag != 0).then_some(required_flag),
        cast_ticks: matches!(execution, Some(EnemyExecution::Spell(_)))
            .then(|| u16::read(row, 14))
            .transpose()?
            .filter(|&duration| duration != 0),
        cast_armor: matches!(execution, Some(EnemyExecution::Spell(_))).then_some(row[49]),
        voices: EnemyActionVoices {
            begin: crate::battle_voice::sound(u16::read(row, 60)?),
            release: crate::battle_voice::sound(u16::read(row, 62)?),
        },
        chain_group: row[32],
        followup_chance: (row[50] as i8).max(0) as u8,
        target_policy,
        target_allies: flags & TARGET_ALLIES != 0 && row[1] != 0,
        requirements,
        return_to_formation: flags & STAY_AFTER_ACTION == 0,
        run_return: flags & (STAY_AFTER_ACTION | RUN_APPROACH) == RUN_APPROACH,
        approach,
        range: <[i16; 2]>::read(row, 16)?,
        approach_range: i16::read(row, 20)?,
        approach_minimum: i16::read(row, 22)?,
        guard_chance: row[35] as i8,
        tp: row[52],
        execution,
        unsupported_reason,
    })
}

fn approach(row: &[u8], flags: u32) -> Result<EnemyApproach> {
    Ok(if row[48] != 0 {
        let speed = f32::read(row, 40)?;
        let rate = f32::read(row, 44)?;
        ensure!(speed > 0. && rate > 0., "invalid enemy custom approach");
        EnemyApproach::Custom {
            clip: u16::from(row[48]),
            rate,
            speed,
        }
    } else if flags & RUN_APPROACH != 0 {
        EnemyApproach::Run
    } else {
        EnemyApproach::Walk
    })
}

pub(crate) fn read(
    bytes: &[u8],
    projectiles: &[crate::battle_projectile::EnemyProjectile],
    common_actions: &[u8],
) -> Result<EnemyActions> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy action package");
    let package = ActionPackage::new(bytes, projectiles, common_actions)?;
    let (_, policy_source) = section(bytes, 12)?;
    ensure!(
        policy_source.len() >= 26 && policy_source[24] <= 4,
        "invalid enemy policy"
    );
    // The station controller changes position at authored action age sixty.
    // Compile it with the selected action's other times, including blend holds.
    let mut station_change_at = 60;
    let mut weapon_change_at = [53, 145];
    let (_, source) = section(bytes, 10)?;
    ensure!(source.len().is_multiple_of(68), "misaligned enemy actions");
    let mut rows = source
        .chunks_exact(68)
        .enumerate()
        .map(|(index, row)| {
            let mut afflict_at = 120;
            let mut carried_effect_ticks = if policy_source[8] == 18 && (2..=5).contains(&index) {
                (20..=88).step_by(4).collect::<Vec<u16>>()
            } else {
                Vec::new()
            };
            let policy_ticks = match (policy_source[8], index) {
                (9, 2) => &mut weapon_change_at[..],
                (11, 2) => std::slice::from_mut(&mut station_change_at),
                (18, 2..=5) => &mut carried_effect_ticks,
                (23, 5) => std::slice::from_mut(&mut afflict_at),
                _ => &mut [],
            };
            let mut row = action(row, &package, policy_ticks)?;
            if policy_source[8] == 23 && index == 5 {
                let Some(EnemyExecution::Attack(attack)) = &mut row.execution else {
                    anyhow::bail!("opponent affliction requires an attack timeline");
                };
                attack.events.push((
                    // The affliction follows the action clock's update.
                    afflict_at - 1,
                    EnemyEvent::AfflictOpponents {
                        groups: vec![
                            vec![
                                Condition::Curse,
                                Condition::Paralysis,
                                Condition::PoisonSevere,
                            ],
                            vec![
                                Condition::AttackDown,
                                Condition::DefenseDown,
                                Condition::Weak,
                            ],
                        ],
                        dispel: true,
                    },
                ));
                attack.events.sort_by_key(|(at, _)| *at);
            }
            if !carried_effect_ticks.is_empty() {
                let Some(EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. }) =
                    &mut row.execution
                else {
                    anyhow::bail!("carried effect requires an attack timeline");
                };
                attack
                    .events
                    .extend(carried_effect_ticks.into_iter().map(|at| {
                        (
                            // Policy effects run after this update advances the action clock.
                            at - 1,
                            EnemyEvent::CarriedEffect {
                                slot: WeaponSlot(0),
                                bone: 0,
                                member: if index < 4 { 8 } else { 7 },
                            },
                        )
                    }));
                attack.events.sort_by_key(|(at, _)| *at);
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    let source = policy_source;
    if source[8] == 17 {
        // This controller releases two ground tentacles on its elapsed action
        // clock. Placement is a native policy parameter, independent of the
        // zero-offset projectile templates and their baked contact paths.
        const TENTACLE_RELEASE_TICK: u16 = 30;
        const TENTACLE_RETREAT: f32 = 225.;
        let row = rows.get_mut(4).context("missing tentacle action")?;
        let prepared = (|| -> Result<()> {
            let Some(EnemyExecution::Attack(attack)) = &mut row.execution else {
                anyhow::bail!("tentacle release requires an attack timeline");
            };
            ensure!(
                TENTACLE_RELEASE_TICK < attack.duration,
                "tentacle release exceeds attack duration"
            );
            let event = package.ground_projectiles(1, [3, 4], TENTACLE_RETREAT)?;
            // Policy callbacks run after this update advances the action clock.
            attack.events.push((TENTACLE_RELEASE_TICK - 1, event));
            attack.events.sort_by_key(|(at, _)| *at);
            Ok(())
        })();
        if let Err(error) = prepared {
            row.unsupported_reason
                .get_or_insert_with(|| format!("tentacle release: {error:#}"));
        }
    }
    let policy = EnemyPolicy {
        back_row: (0..usize::from(source[24]))
            .map(|index| EnemyBackRow {
                action: source[14 + index],
                weight: source[18 + index],
            })
            .collect(),
        walk_override: (source[8] == 3).then_some(EnemyWalkOverride {
            hp_percent: 25,
            speed: 2.83,
            motion_alias: (1, 45),
        }),
        behavior: match source[8] {
            5 => Some(EnemyBehavior::CarriedAnimationRate {
                slot: WeaponSlot(0),
                rate: 0.15,
            }),
            9 => Some(EnemyBehavior::ReplaceWeapon {
                action: 2,
                hide_at: weapon_change_at[0],
                replace_at: weapon_change_at[1],
                source: WeaponSlot(1),
                destination: WeaponSlot(0),
                disabled_action: 0,
            }),
            7 => Some(EnemyBehavior::Transformation {
                reveal_hp_percent: 75,
                revealed_bones: [
                    "sc_Bone_ude_L",
                    "sc_Bone_ude_R",
                    "sc_Bone_kata_L",
                    "sc_Bone_kata_R",
                    "sc_Bone_body",
                    "sc_Bone_ashi_L",
                    "sc_Bone_ashi_R",
                    "sc_Bone_koshi",
                ]
                .map(str::to_owned)
                .to_vec(),
                airborne_hp_percent: 50,
                airborne_walk_speed: 6.5,
                airborne_motions: (0..30).map(|clip| (clip, clip + 30)).collect(),
                hover_height: 25.,
                initial_actions: 0..5,
                final_actions: 5..9,
            }),
            11 => Some(EnemyBehavior::ThreeStations {
                change_at: station_change_at,
                stations: [
                    EnemyStation {
                        position: [1650., 0., 0.],
                        heading: -90.,
                    },
                    EnemyStation {
                        position: [-825., 0., 1428.],
                        heading: 150.,
                    },
                    EnemyStation {
                        position: [-825., 0., -1428.],
                        heading: 30.,
                    },
                ],
            }),
            // Controller 16 initializes the independent attached-model record.
            16 => None,
            17 => Some(EnemyBehavior::HealthStage {
                hp_percent: 50,
                initial_station: EnemyStation {
                    position: [850., 0., 0.],
                    heading: -90.,
                },
                initial_actions: 0..5,
                final_actions: 5..9,
            }),
            19 => Some(EnemyBehavior::Retreat {
                hp_percent: 50,
                after_ticks: 60 * 60,
            }),
            18 => Some(EnemyBehavior::ActionWeapons {
                groups: vec![
                    vec![(0, WeaponSlot(0)), (1, WeaponSlot(1))],
                    vec![(3, WeaponSlot(0)), (1, WeaponSlot(1))],
                    vec![(2, WeaponSlot(0)), (1, WeaponSlot(1))],
                ],
                actions: (0..rows.len())
                    .map(|row| {
                        Some(match row {
                            2 | 3 => 1,
                            4 | 5 => 2,
                            _ => 0,
                        })
                    })
                    .collect(),
                idle_group: Some(0),
                effect: 6,
                anchors: vec![0],
                drop_on_hurt: Some(WeaponDrop {
                    slot: WeaponSlot(1),
                    hp_percent: 50,
                    upward_speed: 10.,
                    gravity: -0.9,
                    pitch: -90.,
                }),
            }),
            24 => Some(EnemyBehavior::ActionWeapons {
                groups: vec![
                    vec![(0, WeaponSlot(0)), (1, WeaponSlot(1))],
                    vec![(2, WeaponSlot(0))],
                    vec![(3, WeaponSlot(0)), (3, WeaponSlot(1))],
                    vec![(4, WeaponSlot(0))],
                ],
                actions: (0..rows.len())
                    .map(|row| match row {
                        2 | 3 | 5 => Some(0),
                        6 => Some(1),
                        9 => Some(2),
                        7 | 8 => Some(3),
                        _ => None,
                    })
                    .collect(),
                idle_group: None,
                effect: 2,
                anchors: vec![0, 1],
                drop_on_hurt: None,
            }),
            _ => None,
        },
        items_forbidden: source[8] == 27,
        event_hp_hold: source[8] == 9,
        unsupported_reason: if !matches!(source[8], 0..=13 | 15..=27)
            || source[11] != 0
            || source[22] != 0
            || source[25] != 0
        {
            Some("enemy native/counter/overlimit decisions are not prepared".into())
        } else if usize::from(source[23]) != rows.len() {
            Some("enemy action/decision row count differs".into())
        } else {
            None
        },
    };
    Ok(EnemyActions { rows, policy })
}

mod timeline;
use timeline::ActionPackage;
pub(crate) use timeline::{contact, damage_kind, hit, hit_rule};

#[cfg(test)]
mod tests;
