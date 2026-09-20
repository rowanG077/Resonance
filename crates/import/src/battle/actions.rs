//! Decode action tables and relocations once; runtime consumes named JSON records.
#[cfg(test)]
use super::action_program::ENEMY_ACTION_BYTES as ACTION_BYTES;
use super::action_program::{
    EnemyActionRecord, HIT_BYTES, HIT_RULE_BYTES as RULE_BYTES, HitRecord, HitRuleRecord,
};
use super::animation_table::ROW_BYTES as ANIMATION_BYTES;
pub(crate) use super::animation_table::selected_at as animations_at;
pub(in crate::battle) use bundle::Bundle;
use bundle::Tables;
mod acid_rain;
mod air_thrust;
pub(super) mod all;
mod aqua_edge;
mod binding;
mod bundle;
mod earth_field;
mod elemental_parameters;
mod enemy;
mod enemy_parameters;
mod eruption;
mod explosion;
mod flame_lance;
mod freeze_lancer;
mod genis_final;
mod ground_pulse;
mod ground_summon;
mod ice_tornado;
mod icicle;
mod lance;
mod lightning;
mod martial;
mod martial_parameters;
mod normals;
mod nurse;
mod orb;
mod ordinary_parameters;
#[cfg(test)]
mod position_tests;
mod prism;
mod ray;
mod recovery;
mod recovery_parameters;
mod recovery_pose;
mod spiral_flare;
mod spread;
mod stalagmite;
mod stone_blast;
mod stored_parameters;
mod stored_resume;
mod summon;
mod summon_parameters;
mod thunder_arrow;
mod water;
mod wind_blade;
mod wind_field;
use crate::{
    arte::Definition,
    read::{f32 as float, u16 as half, u32 as word},
};
#[cfg(test)]
use crate::{compression, dol};
use anyhow::{Context, Result, bail, ensure};
pub(crate) use elemental_parameters::cook as cook_elemental_spell_parameters;
pub(crate) use enemy_parameters::cook as cook_enemy_parameters;
pub(crate) use martial_parameters::cook as cook_martial_parameters;
pub(crate) use normals::cook as cook_normal_actions;
#[cfg(test)]
pub(super) use normals::party as party_actions;
pub(crate) use ordinary_parameters::cook as cook_ordinary_spell_parameters;
pub(crate) use recovery_parameters::cook as cook_recovery_parameters;
use resonance_content::battle::{actions::*, effects::ProjectileRecipe};
#[cfg(test)]
use std::fs;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub(crate) use stored_parameters::cook as cook_stored_spell_parameters;
pub(crate) use summon_parameters::cook as cook_summon_parameters;

const DATA: usize = 5;
const LIMIT: usize = 256;

pub(super) use crate::rel::Rel;

pub(crate) fn cook_selected(
    extracted: &Path,
    data: &Path,
    catalogue: &crate::arte::Catalogue,
    motion: &super::motion::MotionTables,
    selection: &super::CookSelection,
    monsters: &[u8],
    shared_contact: &mut impl FnMut() -> Result<ProjectileRecipe>,
) -> Result<BattleActions> {
    all::Cooker::read(extracted, catalogue)?.selected(
        data,
        motion,
        selection,
        monsters,
        shared_contact,
    )
}

fn martial_chains(
    catalogue: &crate::arte::Catalogue,
    parameters: &martial::ChainParameters,
    selected: &[u16],
) -> Result<resonance_content::battle::chains::MartialChains> {
    use resonance_content::battle::chains::{ChainArte, ChainElement, MartialChains};
    let chains = MartialChains {
        artes: selected
            .iter()
            .copied()
            .map(|id| {
                let row = catalogue.definition(usize::from(id))?;
                Ok((
                    id,
                    ChainArte {
                        upgrades: [row.technical_successor as u16, row.strike_successor as u16],
                        airborne: row.flags & 0x40 != 0,
                        element: match row.element {
                            0 => ChainElement::Inherit,
                            10 => ChainElement::Neutral,
                            id => ChainElement::Element(
                                *resonance_content::menu_data::Element::ALL
                                    .get(usize::from(id - 1))
                                    .context("invalid chain element")?,
                            ),
                        },
                    },
                ))
            })
            .collect::<Result<_>>()?,
        colors: std::array::from_fn(|i| parameters.colors[i][..3].try_into().unwrap()),
        ground_height: parameters.ground_height,
        aerial_height: parameters.aerial_height,
        regal_aerial_height: parameters.regal_aerial_height,
    };
    chains.validate()?;
    Ok(chains)
}

#[cfg(test)]
fn enemy_actions(bytes: &[u8], monster: u8) -> Result<EnemyActions> {
    prepared_enemy_actions(&binding::Records::read(bytes)?, monster)
}

fn prepared_enemy_actions(records: &binding::Records, monster: u8) -> Result<EnemyActions> {
    ensure!(
        records.rows.len() <= usize::from(u8::MAX) + 1,
        "invalid enemy action rows"
    );
    let actions = records
        .rows
        .iter()
        .enumerate()
        .map(|(id, row)| {
            let (commands, loop_commands) = records.commands.select(row.command_index as i16)?;
            let [range_min, range_max] = row.range;
            Ok(EnemyAction {
                id: id as u8,
                approach: Some(enemy::approach(row)?),
                contact_recovery: enemy::prepared_contact_recovery(row, records)?,
                selection: EnemySelection {
                    weight: row.weight,
                    target_policy: row.target_policy,
                    requirements: row.requirements,
                    target_state: row.target_state,
                    range: [range_min, if range_max == 0 { 2500 } else { range_max }],
                    approach_range: row.approach_range,
                    approach_minimum: row.approach_minimum,
                    required_story_flag: nonzero(row.required_story_flag),
                    required_monster: row.required_monster,
                },
                combo_at: row.combo_at,
                followup_group: row.followup_group,
                followup_chance: row.followup_chance,
                stagger_threshold: row.stagger_threshold,
                guard_chance: row.guard_chance,
                vulnerable: row.vulnerable,
                resource_decrement: row.resource_decrement,
                technique: nonzero(row.native_technique),
                effect: (row.effect != 0).then_some(row.effect),
                recovery: Recovery {
                    duration: u16::from(row.recovery_ticks),
                    animation: (row.recovery_clip != 0).then_some(row.recovery_clip),
                    rate: row.recovery_rate.finite()?,
                },
                action: Action {
                    duration: row.duration,
                    tp: row.tp,
                    animations: records.animations(row.animation_index)?,
                    commands,
                    loop_commands,
                    hits: records.hits(row.hit_index)?,
                },
            })
        })
        .collect::<Result<_>>()?;
    Ok(EnemyActions {
        absent_movement_motions: [
            EnemyMovementMotion::Walk,
            EnemyMovementMotion::Run,
            EnemyMovementMotion::Stop,
        ]
        .into_iter()
        .map(|motion| Ok((motion, records.motion_absent(motion.clip())?)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|(motion, absent)| absent.then_some(motion))
        .collect(),
        casting: BTreeMap::new(),
        monster,
        locomotion: Some({
            let metadata = &records.settings;
            EnemyLocomotion {
                turn_divisor: metadata.combat.turn_divisor,
                unrestricted_range: metadata.combat.flags & 0x80 != 0,
                bobs: metadata.combat.flags & 2 != 0,
                effect: if metadata.effects.locomotion_effect == 0 {
                    None
                } else {
                    let period = metadata.effects.locomotion_effect_period;
                    ensure!(period != 0, "enemy locomotion effect has zero period");
                    Some(EnemyLocomotionEffect {
                        period,
                        id: metadata.effects.locomotion_effect,
                    })
                },
            }
        }),
        actions,
        policy: None,
        native_tp: BTreeMap::new(),
    })
}

fn nonzero(value: u16) -> Option<u16> {
    (value != 0).then_some(value)
}

pub(super) fn commands(bytes: &[u8]) -> Result<(Vec<TimedCommand>, bool)> {
    let (commands, loops, _) = commands_with_tail(bytes)?;
    Ok((commands, loops))
}

pub(super) fn commands_with_tail(bytes: &[u8]) -> Result<(Vec<TimedCommand>, bool, &[u8])> {
    use super::action_program::{Record, record};
    let (mut cursor, mut out) = (0, Vec::new());
    for _ in 0..LIMIT {
        let offset = cursor;
        let step = match record(bytes, &mut cursor)? {
            Record::Command(command) => command,
            Record::End { loops } => return Ok((out, loops, &bytes[cursor..])),
        };
        out.push(
            lower_command(&step).map_err(|error| anyhow::anyhow!("{error:#} at {offset:#x}"))?,
        );
    }
    bail!("action program exceeds bounded command limit")
}

pub(super) fn lower_command(step: &super::action_program::Command) -> Result<TimedCommand> {
    step.validate()?;
    use resonance_content::battle::action_program::CommandKind as K;
    let arg = |index: usize| step.operands[index];
    let scalar = || f32::from(arg(0) as i16) * 0.1;
    let command = match step.kind {
        K::Reserved => ActionCommand::Noop,
        K::CommonImpactFlash => ActionCommand::ImpactFlash,
        K::TextureVariant => ActionCommand::TextureVariant(arg(0) as u8),
        K::ModelTransform => {
            let slot = u8::try_from(arg(0) as i16).context("negative bone-scale slot")?;
            ensure!(slot < 8, "bone-scale slot outside controller");
            ActionCommand::BoneScale {
                slot,
                bone: u16::try_from(arg(1) as i16).context("negative bone-scale bone")?,
                duration: arg(2) as i16,
                scale: f32::from(arg(3) as i16) * 0.1,
            }
        }
        K::CommonEffect => ActionCommand::CommonEffect {
            id: arg(0) as u8,
            origin: if arg(1) == 0 {
                EffectOrigin::Root
            } else {
                EffectOrigin::Body
            },
        },
        K::WaitHit => ActionCommand::WaitHit,
        K::WaitActionResult => ActionCommand::WaitActionResult,
        K::Withdraw | K::WithdrawAndRemove => ActionCommand::Defeat {
            rewards: step.kind == K::Withdraw,
        },
        K::SetActorAmbientColor => {
            ActionCommand::AmbientColor([arg(0) as u8, arg(1) as u8, arg(2) as u8])
        }
        K::DamagePower => ActionCommand::DamagePower(arg(0)),
        K::RecoverHp | K::RecoverTp => ActionCommand::Recover {
            resource: if step.kind == K::RecoverHp {
                resonance_content::battle::actions::RecoveryResource::Hp
            } else {
                resonance_content::battle::actions::RecoveryResource::Tp
            },
            percent: arg(0) as i16,
        },
        K::ForwardSpeed => ActionCommand::ForwardSpeed(scalar()),
        K::VerticalSpeed => ActionCommand::VerticalSpeed(scalar()),
        K::AdvancePosition => ActionCommand::AdvancePosition(scalar()),
        K::ExtendAction => ActionCommand::ExtendAction(arg(0) as i16),
        K::ReleaseCapturedTarget => ActionCommand::ReleaseHeld {
            hitstun: arg(1) as i16,
        },
        K::CapturedTargetForwardSpeed => ActionCommand::HeldForwardSpeed(scalar()),
        K::CapturedTargetVerticalSpeed => ActionCommand::HeldVerticalSpeed(scalar()),
        K::CapturedTargetVisibility => ActionCommand::HeldVisibility(arg(0) & 1 != 0),
        K::ForwardAcceleration => ActionCommand::ForwardAcceleration(scalar()),
        K::Gravity => ActionCommand::Gravity(scalar()),
        K::Reverse => ActionCommand::Reverse,
        K::SetActorCollisionMode => ActionCommand::BodyPush {
            enabled: arg(0) & 1 == 0,
            restore_after: arg(1) as u8,
        },
        K::ActorMotionScale => ActionCommand::ForwardDeceleration(scalar()),
        K::TextureLayers => {
            let [a, b] = [arg(0).to_be_bytes(), arg(1).to_be_bytes()];
            ActionCommand::TextureLayers([a[0], a[1], b[0], b[1]])
        }
        K::AttachmentVisibility => {
            let packed = arg(0);
            ensure!(packed as u8 <= 1, "invalid attachment visibility");
            ActionCommand::AttachmentVisibility {
                slot: (packed >> 8) as u8,
                visible: packed as u8 != 0,
            }
        }
        K::SetActorProtection => ActionCommand::Poise {
            enabled: arg(0) != 0,
            duration: arg(1) as i16,
        },
        K::AttachmentTrail => ActionCommand::AttachmentTrail {
            slot: arg(0),
            ticks: arg(1),
        },
        K::SetActorAttackMode => ActionCommand::SecondaryWind {
            enabled: arg(0) & 1 != 0,
            duration: arg(1) as i16,
        },
        K::Voice => ActionCommand::Voice {
            id: arg(0),
            priority: arg(1).try_into().context("invalid voice priority")?,
        },
        K::Sound => ActionCommand::Sound(arg(0)),
        K::CameraMotion => ActionCommand::CameraBounds {
            duration: arg(0) as i16,
            distance: f32::from(arg(1) as i16) * 0.1,
            elevation: if arg(2) == 0 {
                8.
            } else {
                f32::from(arg(2) as i16) * 0.1
            },
        },
        K::ApplyConditionAndTransition => ActionCommand::ApplyConditions {
            conditions: resonance_content::battle::conditions::ScriptConditions::new(
                step.operands[2..6]
                    .iter()
                    .fold(0u64, |mask, &word| (mask << 16) | u64::from(word)),
            )?,
            strength: arg(1) as i16,
        },
        K::PlayerCameraMotion => ActionCommand::PlayerCameraMotion {
            duration: arg(0) as i16,
            amount: f32::from(arg(1) as i16) * 0.1,
        },
        K::CastPrimaryTechnique | K::CastSecondaryTechnique => ActionCommand::CastNative {
            native_id: arg(0),
            slot: if step.kind == K::CastPrimaryTechnique {
                resonance_content::battle::action_program::CastSlot::Primary
            } else {
                resonance_content::battle::action_program::CastSlot::Secondary
            },
        },
        K::SetPosition => ActionCommand::SetPosition([arg(0) as i16, arg(1) as i16, arg(2) as i16]),
        K::PositionFromTarget => ActionCommand::PositionFromTarget {
            height: arg(1) as i16,
            retreat: arg(2) as i16,
        },
        K::OffsetPosition => ActionCommand::OffsetPosition {
            height: arg(1) as i16,
            retreat: arg(2) as i16,
        },
        K::TurnMotion => ActionCommand::TurnMotion(scalar().to_radians()),
        K::TurnHeading => ActionCommand::TurnHeading(scalar().to_radians()),
        // The native handler replaces the preliminary angle adjustment.
        K::FaceTargetDirection => ActionCommand::FaceTargetDirection,
        K::RandomVoice => ActionCommand::RandomVoice {
            first: arg(0),
            second: arg(1),
            priority: arg(2).try_into().context("invalid voice priority")?,
            first_percent: arg(3),
        },
        kind => bail!("unsupported battle action command {}", kind as i16),
    };
    Ok(TimedCommand {
        tick: step.tick,
        command,
    })
}

#[test]
fn common_effect_command_keeps_unsigned_id_and_nonzero_body_selection() {
    let flash = [0u16, 22, 1, 28, 60, 0, 0xffff]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    assert!(matches!(
        commands(&flash).unwrap().0.as_slice(),
        [
            TimedCommand {
                tick: 0,
                command: ActionCommand::ImpactFlash
            },
            TimedCommand {
                tick: 1,
                command: ActionCommand::Sound(60)
            },
        ]
    ));
    let words = [5_u16, 26, 18, 1, 6, 26, 0x112, 0, 7, 26, 18, 0xffff, 0xffff];
    let bytes = words
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    let (decoded, loops) = commands(&bytes).unwrap();
    assert!(!loops);
    assert_eq!(
        decoded.iter().map(|step| step.tick).collect::<Vec<_>>(),
        [5, 6, 7]
    );
    for (index, step) in decoded.iter().enumerate() {
        let ActionCommand::CommonEffect { id, origin } = step.command else {
            panic!()
        };
        assert_eq!(id, 18);
        assert_eq!(matches!(origin, EffectOrigin::Root), index == 1);
    }
    assert!(commands(&bytes[..7]).is_err());
    let actions = BattleActions {
        chains: None,
        party: vec![],
        enemies: vec![],
        projectiles: vec![],
        techniques: vec![],
    };
    let normal = NormalAction {
        selection: 0,
        combo: ComboWindow {
            first: 0,
            second: None,
            buffer_until: 0,
            allowed_directions: 0,
            fallback: None,
        },
        recovery: Recovery {
            duration: 0,
            animation: None,
            rate: 1.,
        },
        effect: None,
        reach: 0,
        airborne_reach: 0,
        action: Action {
            duration: 10,
            tp: 0,
            animations: Default::default(),
            commands: decoded
                .into_iter()
                .chain(commands(&flash).unwrap().0)
                .collect(),
            loop_commands: false,
            hits: vec![],
        },
    };
    let mut actions = actions;
    actions.party.push(PartyActions {
        character: 1,
        normal: vec![normal],
    });
    let dependencies = crate::battle::selection::Dependencies::actions(&actions).unwrap();
    assert!(
        dependencies
            .programs
            .contains(&resonance_content::battle::effects::EffectId {
                bank: resonance_content::battle::effects::EffectBank::Common,
                id: 13,
            })
    );
    assert!(
        dependencies
            .programs
            .contains(&resonance_content::battle::effects::EffectId {
                bank: resonance_content::battle::effects::EffectBank::Common,
                id: 18
            })
    );
}

pub(super) fn hits(bytes: &[u8], rules: &[u8]) -> Result<Vec<HitWindow>> {
    let mut out = Vec::new();
    for i in 0..LIMIT {
        let row = bytes
            .get(i * HIT_BYTES..(i + 1) * HIT_BYTES)
            .context("unterminated hit windows")?;
        match HitRecord::read(row)?.lower(rules)? {
            Some(hit) => out.push(hit),
            None => return Ok(out),
        }
    }
    bail!("hit program exceeds bounded record limit")
}

pub(super) fn member(bytes: &[u8], index: usize) -> Result<&[u8]> {
    let count = word(bytes, 0)? as usize;
    ensure!(
        count < 4096 && index < count,
        "invalid battle table member {index}"
    );
    let start = word(bytes, 4 + index * 4)? as usize;
    ensure!(
        start >= 4 + count * 4,
        "missing battle table member {index}"
    );
    let end = (0..count)
        .map(|i| word(bytes, 4 + i * 4).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&v| v > start)
        .min()
        .unwrap_or(bytes.len());
    bytes
        .get(start..end)
        .context("battle table member outside archive")
}

#[cfg(test)]
pub(super) fn source_contact(rel: &Rel, usual: &[u8]) -> Result<ProjectileRecipe> {
    use resonance_content::battle::effects::{EffectBank, EffectId};
    super::effects::projectile(
        member(usual, 7)?
            .get(400..800)
            .context("missing source contact projectile")?,
        EffectId {
            bank: EffectBank::Techniques,
            id: 1,
        },
        super::motion::read(rel, &super::embedded::Layout::RETAIL)?
            .projectile_velocity_reset_scale()?,
    )
}

#[cfg(test)]
fn technique_actions(
    extracted: &Path,
    rel: &Rel,
    usual: &[u8],
    techniques: &[u16],
) -> Result<Vec<TechniqueAction>> {
    let catalogue = crate::arte::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let natives = action_bundle_ids(&catalogue, techniques)?
        .into_iter()
        .collect::<Vec<_>>();
    let tables = Tables::original(extracted, rel, usual, &natives)?;
    prepare_technique_actions(&catalogue, rel, &tables, techniques, &mut || {
        source_contact(rel, usual)
    })
}

fn action_bundle_ids(
    catalogue: &crate::arte::Catalogue,
    techniques: &[u16],
) -> Result<BTreeSet<u16>> {
    let mut bundles = BTreeSet::new();
    for &id in techniques {
        let native = catalogue.definition(usize::from(id))?.native_id as u16;
        if matches!(native, 0..=224 | 226..=233 | 236 | 251..=253 | 278 | 283..=293) {
            bundles.insert(native);
        }
    }
    Ok(bundles)
}

fn prepare_technique_actions(
    catalogue: &crate::arte::Catalogue,
    rel: &Rel,
    tables: &Tables,
    techniques: &[u16],
    shared_contact: &mut impl FnMut() -> Result<ProjectileRecipe>,
) -> Result<Vec<TechniqueAction>> {
    techniques
        .iter()
        .copied()
        .map(|technique| {
            let definition = catalogue.definition(usize::from(technique))?;
            let native_id = definition.native_id as u16;
            let casters =
                |release| recovery::casters(catalogue, tables, technique, definition, release);
            let bundle = || tables.bundle(native_id);
            let program = match native_id {
                0..=199 => {
                    let source = bundle()?;
                    let mut variants = Vec::new();
                    for (variant, descriptor) in source.phases.iter().enumerate() {
                        if descriptor.duration == 0 {
                            continue;
                        }
                        variants.push(TechniquePhase {
                            alternate: martial::alternate(&tables.martial, native_id),
                            caption: martial::caption(&tables.martial, native_id, variant as u8)?,
                            variant: variant as u8,
                            callback: martial::callback(
                                &tables.martial,
                                rel,
                                native_id,
                                source,
                                variant as u8,
                            )?,
                            recovery_ticks: descriptor.recovery_ticks,
                            buffer_until: descriptor.buffer_until,
                            combo_at: descriptor.combo_at,
                            effect: descriptor.startup_effect,
                            action: source.action(variant, definition.tp_cost)?,
                        });
                    }
                    martial::hammer_variants(&tables.martial, native_id, &mut variants)?;
                    martial::magic_guard_variants(&tables.martial, native_id, &mut variants)?;
                    if matches!(native_id, 43 | 44) {
                        martial::steal_continuations(source, &mut variants)?;
                    }
                    ensure!(!variants.is_empty(), "technique has no authored phase");
                    TechniqueProgram::Martial { variants }
                }
                220 => TechniqueProgram::Icicle {
                    casters: casters(recovery::Release::Ordinary)?,
                    recipe: icicle::cook(
                        &tables.ordinary.icicle,
                        bundle()?,
                        definition,
                        shared_contact()?,
                    )?,
                },
                212 => TechniqueProgram::StoneBlast {
                    casters: casters(recovery::Release::Ordinary)?,
                    recipe: stone_blast::cook(
                        &tables.ordinary.stone_blast,
                        bundle()?,
                        definition,
                        shared_contact()?,
                    )?,
                },
                213 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::EarthField {
                        casters,
                        resume,
                        recipe: stalagmite::cook(tables, definition)?,
                    }
                }
                216..=219 => lightning::cook(catalogue, tables, technique, definition)?,
                221 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::IceTornado {
                        casters,
                        resume,
                        recipe: ice_tornado::cook(tables, definition)?,
                    }
                }
                222 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::FreezeLancer {
                        casters,
                        resume,
                        recipe: freeze_lancer::cook(tables, definition)?,
                    }
                }
                227 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::ThunderArrow {
                        casters,
                        resume,
                        recipe: thunder_arrow::cook(tables, definition)?,
                    }
                }
                202 | 203 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::Water {
                        casters,
                        resume,
                        recipe: water::cook(tables, definition)?,
                    }
                }
                205..=207 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::FireField {
                        casters,
                        resume,
                        recipe: match native_id {
                            205 => eruption::cook(tables, definition)?,
                            206 => explosion::cook(tables, definition)?,
                            207 => flame_lance::cook(tables, definition)?,
                            _ => unreachable!(),
                        },
                    }
                }
                208 => TechniqueProgram::WindBlade {
                    casters: casters(recovery::Release::Ordinary)?,
                    recipe: wind_blade::cook(
                        &tables.ordinary.wind_blade,
                        bundle()?,
                        definition,
                        shared_contact()?,
                    )?,
                },
                214 | 215 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::EarthField {
                        casters,
                        resume,
                        recipe: earth_field::cook(tables, definition)?,
                    }
                }
                210 | 211 => {
                    let casters = casters(recovery::Release::Stored)?;
                    let resume = stored_resume::shared(tables, &casters)?;
                    TechniqueProgram::WindField {
                        casters,
                        resume,
                        recipe: wind_field::cook(tables, definition)?,
                    }
                }
                209 => TechniqueProgram::AirThrust {
                    casters: casters(recovery::Release::Stored)?,
                    resume: stored_resume::character(tables, 3)?,
                    recipe: air_thrust::cook(tables, definition)?,
                },
                200 => {
                    ensure!(
                        definition.flags == 0x00444186,
                        "unexpected Aqua Edge technique flags"
                    );
                    TechniqueProgram::AquaEdge {
                        casters: casters(recovery::Release::Ordinary)?,
                        recipe: aqua_edge::recipe(bundle()?)?,
                    }
                }
                201 => TechniqueProgram::Spread {
                    casters: casters(recovery::Release::Stored)?,
                    resume: stored_resume::character(tables, 3)?,
                    recipe: spread::cook(tables, definition)?,
                },
                284..=293 => {
                    ensure!(
                        tables.actor(5)?.casting.resume_loop_start == 0,
                        "unsupported summon concluding animation"
                    );
                    let casters = casters(recovery::Release::Summon)?;
                    let resume = AnimationCommand::Play {
                        clip: 12,
                        blend: 4,
                        start: 0,
                        end: None,
                        layer: 8,
                        looping: false,
                        mirror: false,
                        resource: -1,
                        rate: tables.summons.resume_rate,
                    };
                    if matches!(native_id, 284..=289 | 291 | 293) {
                        TechniqueProgram::GroundSummon {
                            casters,
                            resume,
                            recipe: ground_summon::cook(tables, technique, definition)?,
                        }
                    } else {
                        TechniqueProgram::Summon {
                            casters,
                            resume,
                            recipe: summon::cook(tables, definition)?,
                        }
                    }
                }
                265 => acid_rain::cook(catalogue, tables, technique, definition)?,
                226 => spiral_flare::cook(catalogue, tables, technique, definition)?,
                223 | 224 | 228 | 229 => {
                    ground_pulse::cook(catalogue, tables, technique, definition)?
                }
                230 | 231 | 233 => genis_final::cook(catalogue, tables, technique, definition)?,
                232 => prism::cook(catalogue, tables, technique, definition)?,
                252 => ray::cook(catalogue, tables, technique, definition)?,
                253 | 283 => lance::cook(catalogue, tables, technique, definition)?,
                251 | 278 => orb::cook(catalogue, tables, technique, definition)?,
                237 => nurse::cook(catalogue, tables, technique, definition)?,
                236 | 238 | 257 => recovery::cook(catalogue, tables, technique, definition)?,
                _ => {
                    ensure!(
                        native_id == 204 && definition.flags & 1 == 0,
                        "unsupported native spell"
                    );
                    let source = bundle()?;
                    let rule = source.rule(0)?;
                    let parameters = &tables.ordinary.fire_ball;
                    let (cast_commands, loops) = tables.programs.commands()?;
                    TechniqueProgram::FireBall {
                        tp: definition.tp_cost,
                        cast_time_adjustment: definition.cast_time_adjustment,
                        voices: tables.voices.selected(3, native_id)?,
                        casting: tables.programs.animation()?,
                        cast_commands,
                        loop_cast_commands: loops,
                        cast_pulse: if definition.flags & 0x00400000 != 0 {
                            3
                        } else if definition.flags & 0x00800000 != 0 {
                            4
                        } else {
                            5
                        },
                        release: {
                            let casting = &tables.actor(3)?.casting;
                            AnimationCommand::Play {
                                clip: 12,
                                blend: 4,
                                start: 0,
                                end: None,
                                layer: 8,
                                looping: casting.release_looping,
                                mirror: false,
                                resource: -1,
                                rate: casting.animation_rate,
                            }
                        },
                        lifetime: source.phases[0].duration,
                        rule,
                        effect: 23,
                        height_scale: parameters.height_scale,
                        height_offset: parameters.height_offset,
                        emissions: parameters.emissions.to_vec(),
                    }
                }
            };
            Ok(TechniqueAction {
                technique,
                native_id,
                properties: technique_properties(definition)?,
                program,
            })
        })
        .collect()
}

pub(super) fn technique_properties(definition: &Definition) -> Result<TechniqueProperties> {
    let flags = definition.flags;
    let range = definition.approach_range() as f32;
    let ranged = flags & 2 != 0;
    Ok(TechniqueProperties {
        menu_target: if flags & 0x8_0000 != 0 {
            TechniqueMenuTarget::Ally
        } else if flags & 0x4_0000 != 0 {
            TechniqueMenuTarget::Enemy
        } else if flags & 0x10_0000 != 0 {
            TechniqueMenuTarget::User
        } else {
            TechniqueMenuTarget::Unavailable
        },
        magic: flags & 0x80 != 0,
        cast_time_reducible: flags & 0x6000_0020 == 0,
        ranged: flags & 2 != 0,
        damage: flags & 0x100 != 0,
        hp_recovery: flags & 0x200 != 0,
        condition_change: flags & 0x400 != 0,
        incapacitated_target: flags & 0x2000 != 0,
        physical_condition_recovery: flags & 0x8000 != 0,
        magical_condition_recovery: flags & 0x10000 != 0,
        utility: flags & 0x20_0000 != 0,
        target_condition_mask: definition.target_condition_mask,
        target_preference: match definition.target_preference {
            0 | 1 => TechniqueTargetPreference::Individual,
            2 => TechniqueTargetPreference::Either,
            3 => TechniqueTargetPreference::Group,
            value => bail!("unsupported technique target preference {value}"),
        },
        tactic: match definition.learning_route {
            0 => TechniqueTactic::Any,
            1 => TechniqueTactic::Weakened,
            2 => TechniqueTactic::Guarding,
            3 => TechniqueTactic::Special,
            value => bail!("unsupported technique tactic {value}"),
        },
        recovery_ticks: definition.recovery_ticks.try_into()?,
        approach: TechniqueApproach {
            maximum: if ranged && range >= 1000. {
                8000.
            } else {
                range
            },
            ai_minimum: if !ranged {
                0.
            } else if range < 1000. {
                range - 100.
            } else {
                500.
            },
            short_weapon_penalty: !ranged,
        },
        chain: match flags & 0x3c {
            4 => TechniqueChain::First,
            8 => TechniqueChain::Second,
            0x10 => TechniqueChain::Third,
            0x20 => TechniqueChain::Finisher,
            0 => match flags & 0x0e00_0000 {
                0x0200_0000 => TechniqueChain::Ground,
                0x0400_0000 => TechniqueChain::Launch,
                0x0800_0000 => TechniqueChain::Aerial,
                0 => TechniqueChain::None,
                value => bail!("unsupported aerial technique chain flags {value:#x}"),
            },
            value => bail!("unsupported technique chain flags {value:#x}"),
        },
    })
}

pub(super) fn hit_rule(rule: &[u8]) -> Result<HitRule> {
    HitRuleRecord::read(rule)?.lower()
}

#[cfg(test)]
mod tests {

    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_enemy_guard_consumers_and_signed_action_rows() {
        use sha2::{Digest, Sha256};
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        // Bind the complete impact, controller, entry and shared-clock bodies.
        for (start, bytes, expected) in [
            (
                0x61578,
                0x1ef8,
                "51c9c35ab83848b9259277ea6d8072703e77f307301d308a8f5fe9c4911cf185",
            ),
            (
                0x3bdf8,
                0x1904,
                "017faaa16af6e2975570b14d3fc9d2f37444ddb2b4b6274e3a30488db63f2b65",
            ),
            (
                0x2a540,
                0x1d0,
                "d71bad8e661e0a65d25ed81b7eb3e6ba7220e93171a2208a6084f48770ea0ad4",
            ),
            (
                0x2f284,
                0x504,
                "8ef4e027237babe0b240750c97f3fd67ce2fff47cc0dc75e02cf1242ebf1b128",
            ),
            (
                0x2a710,
                0x2bc,
                "9cebf592f0a55d9a83807799e37f8e3b4d5a2a4e3fa28e14ccceb19586f02fc5",
            ),
            (
                0x2503c,
                0x178c,
                "8089d5af08e81a7c0dcd2ca6be3991d8a17ca0069254cc859aed4076e2e9ff98",
            ),
        ] {
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(&rel.at((1, start)).unwrap()[..bytes])
                ),
                expected
            );
        }
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
        let table = word(&usual, 0x2c).unwrap() as usize;
        for (monster, offset, tick) in [(2, 132, 360), (11, 204, 21), (165, 0, 1800)] {
            let bytes = crate::battle::archive_directories::enemy_package(
                &extracted.join("files/BTL/BTLenemy.dat"),
                &usual,
                monster,
            )
            .unwrap();
            let start = usize::from(half(&bytes, 14).unwrap()) + offset;
            let (commands, looping) = commands(&bytes[start..]).unwrap();
            assert!(!looping);
            assert!(matches!(commands.last(), Some(TimedCommand {
                tick: actual,
                command: ActionCommand::Defeat { rewards: false },
            }) if *actual == tick));
            if monster == 11 {
                assert_eq!(commands.len(), 12);
                for (index, step) in commands[..10].iter().enumerate() {
                    assert_eq!(step.tick, index as u16 * 2);
                    assert!(matches!(step.command, ActionCommand::AmbientColor(color)
                        if color == [64 + (index % 5) as u8 * 16, 64, 64]));
                }
                assert!(matches!(
                    commands[10],
                    TimedCommand {
                        tick: 20,
                        command: ActionCommand::CommonEffect {
                            id: 14,
                            origin: EffectOrigin::Body
                        }
                    }
                ));
                enemy_actions(&bytes, monster as u8).unwrap();
            } else {
                assert_eq!(commands.len(), 1);
            }
        }
        for (monster, expected) in [
            (
                36,
                &[
                    (0, [0, 12]),
                    (15, [0, 12]),
                    (33, [0, 12]),
                    (50, [0, 12]),
                    (0, [0, 12]),
                ][..],
            ),
            (49, &[(25, [1, 4]), (25, [1, 4])][..]),
            (182, &[(25, [0, 8]); 5][..]),
            (205, &[(25, [0, 8]); 7][..]),
        ] {
            let start = word(&usual, table + usize::from(monster) * 4).unwrap() as usize;
            let end = word(&usual, table + (usize::from(monster) + 1) * 4).unwrap() as usize;
            let bytes = compression::decode(&archive[start..end]).unwrap();
            let actions = enemy_actions(&bytes, monster).unwrap();
            assert_eq!(
                actions
                    .actions
                    .iter()
                    .map(|row| (row.guard_chance, row.vulnerable))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
    use super::*;

    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_casting_ex_bindings_and_consumers() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        for (address, expected) in [
            (0x39d6c, 0x3c606000),
            (0x39d70, 0x809d0034),
            (0x39d74, 0x38030020),
            (0x39d78, 0x7c800039),
            (0x39d7c, 0x40820294),
            (0x39db0, 0x3880001b),
            (0x39db4, 0x4bfe2ab9),
            (0x39dc8, 0xa8630056),
            (0x39dcc, 0x7c631670),
            (0x39dd4, 0x7c030050),
            (0x39dd8, 0xb01c01be),
            (0x3a000, 0x2c000001),
            (0x3a004, 0x4181000c),
            (0x3a008, 0x38000001),
            (0x3a00c, 0xb01c01be),
            (0x3a084, 0x3800001e),
            (0x3a088, 0xb01c01be),
            (0x627f8, 0x88170282),
            (0x627fc, 0x540007ff),
            (0x62800, 0x40820168),
            (0x6291c, 0x3880006f),
            (0x62920, 0x4bfb9e2d),
            (0x6292c, 0x881701b0),
            (0x62930, 0x2800000c),
            (0x62934, 0x40820008),
            (0x62938, 0x639c0002),
            (0x38c68, 0xa81a01be),
            (0x38c6c, 0x2c000000),
            (0x38c70, 0x418100f8),
            (0x38c74, 0x881a1046),
            (0x38c78, 0x2800000c),
            (0x38c7c, 0x418200ec),
            (0x38c8c, 0x3860001a),
            (0x38c98, 0x987a01b0),
            (0x38d28, 0x3800000c),
            (0x38d30, 0x981a01b0),
            (0x39120, 0x38000016),
            (0x39128, 0x981a01b0),
        ] {
            assert_eq!(
                word(rel.at((1, address)).unwrap(), 0).unwrap(),
                expected,
                "source {address:#x}"
            );
        }
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        for (character, base, required) in [
            (3, 80, &[25, 27, 28, 29][..]),
            (4, 70, &[27, 32, 28, 33][..]),
            (6, 75, &[42, 29, 27][..]),
            (9, 70, &[42, 29, 27][..]),
        ] {
            let traits = rel
                .at((DATA, 0x3d30 + (character as usize - 1) * 0x1f0))
                .unwrap();
            assert_eq!(half(traits, 0x56).unwrap(), base);
            let choices =
                crate::dol::slice(&executable, 0x80208dd0 + (character - 1) * 16, 16).unwrap();
            assert!(choices.contains(&27));
            let table =
                crate::dol::slice(&executable, 0x80208e60 + (character - 1) * 196, 196).unwrap();
            assert_eq!(word(table, 0).unwrap(), 24);
            let row = table[4..]
                .chunks_exact(8)
                .find(|row| half(row, 0).unwrap() == 111)
                .unwrap();
            assert_eq!(usize::from(half(row, 2).unwrap()), required.len());
            assert_eq!(&row[4..4 + required.len()], required);
        }
        let sheena = crate::dol::slice(&executable, 0x80208dd0 + 4 * 16, 16).unwrap();
        assert!(sheena[12..].contains(&27));
        for (menu, expected) in [
            (66, true),
            (67, true),
            (68, true),
            (69, true),
            (214, true),
            (238, false),
            (244, false),
        ] {
            let row = crate::dol::slice(&executable, 0x80202f90 + menu * 88, 88).unwrap();
            let definition = Definition::decode(&executable, row).unwrap();
            assert_eq!(
                technique_properties(&definition)
                    .unwrap()
                    .cast_time_reducible,
                expected,
                "menu {menu}"
            );
        }
    }

    #[test]
    fn casting_reduction_eligibility_preserves_each_original_exclusion_bit() {
        for flags in [0, 0x0044_018b, 0x20, 0x2000_0000, 0x4000_0000, 0x2084_0191] {
            let mut row = [0u8; 88];
            row[0x34..0x38].copy_from_slice(&u32::to_be_bytes(flags));
            let definition = Definition::decode(&[], &row).unwrap();
            let properties = super::technique_properties(&definition).unwrap();
            assert_eq!(
                properties.cast_time_reducible,
                matches!(flags, 0 | 0x0044_018b)
            );
        }
    }

    #[test]
    fn secondary_wind_preserves_the_native4_track_and_signed_timer() {
        let bytes: Vec<_> = [0u16, 27, 0x803e, 2, 4, 0, 80, 20, 23, 1, 40, 0xffff]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect();
        let (track, looping) = commands(&bytes).unwrap();
        assert!(!looping);
        assert!(matches!(
            track.as_slice(),
            [
                TimedCommand {
                    tick: 0,
                    command: ActionCommand::Voice {
                        id: 0x803e,
                        priority: 2
                    }
                },
                TimedCommand {
                    tick: 4,
                    command: ActionCommand::ForwardSpeed(8.)
                },
                TimedCommand {
                    tick: 20,
                    command: ActionCommand::SecondaryWind {
                        enabled: true,
                        duration: 40
                    }
                },
            ]
        ));
        for (flags, enabled) in [(2, false), (3, true)] {
            let bytes: Vec<_> = [0u16, 23, flags, 0xffff, 0xffff]
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect();
            assert!(matches!(commands(&bytes).unwrap().0[0].command,
                ActionCommand::SecondaryWind { enabled: value, duration: -1 } if value == enabled));
            assert!(commands(&bytes[..7]).is_err());
        }
    }

    #[test]
    fn poise_command_preserves_unsigned_enable_signed_duration_and_next_record() {
        for (enabled, duration) in [(0u16, -1i16), (1, 150), (2, -1)] {
            let bytes: Vec<_> = [41, 12, enabled, duration as u16, 44, 28, 93, 0, 0xffff]
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect();
            let (track, loops) = commands(&bytes).unwrap();
            assert!(!loops);
            assert!(matches!(track.as_slice(), [
                TimedCommand { tick: 41, command: ActionCommand::Poise { enabled: actual, duration: ticks } },
                TimedCommand { tick: 44, command: ActionCommand::Sound(93) },
            ] if *actual == (enabled != 0) && *ticks == duration));
            assert!(commands(&bytes[..7]).is_err());
        }
    }

    #[test]
    fn camera_color_power_recovery_and_defeat_commands_preserve_values_and_operand_widths() {
        use resonance_content::battle::actions::RecoveryResource;
        for (duration, distance, elevation) in [(30i16, 15000i16, 0i16), (-7, -120, -90)] {
            let bytes = [0, 10, duration, distance, elevation, 1, 28, 60, 0, -1]
                .into_iter()
                .flat_map(i16::to_be_bytes)
                .collect::<Vec<_>>();
            let (track, _) = super::commands(&bytes).unwrap();
            assert!(matches!(track.as_slice(), [
                TimedCommand { command: ActionCommand::CameraBounds { duration: d, distance: x, elevation: y }, .. },
                TimedCommand { command: ActionCommand::Sound(60), .. },
            ] if *d == duration && *x == f32::from(distance) * 0.1
                && *y == if elevation == 0 { 8. } else { f32::from(elevation) * 0.1 }));
        }
        for mask in [0u64, 4, 0x20, 0x209, 1 << 32] {
            let mut bytes = [0u16, 14, 0xabcd, (-7i16) as u16]
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>();
            bytes.extend(mask.to_be_bytes());
            bytes.extend([1i16, 28, 60, 0, -1].into_iter().flat_map(i16::to_be_bytes));
            if mask == 1 << 32 {
                assert!(super::commands(&bytes).is_err());
            } else {
                let (track, _) = super::commands(&bytes).unwrap();
                assert!(matches!(track.as_slice(), [
                    TimedCommand { command: ActionCommand::ApplyConditions { conditions, strength: -7 }, .. },
                    TimedCommand { command: ActionCommand::Sound(60), .. },
                ] if conditions.bits() == mask));
            }
            assert!(super::commands(&bytes[..15]).is_err());
        }
        for (duration, scale) in [(30i16, 15i16), (0, -10), (-7, i16::MIN)] {
            let bytes = [0i16, 32, 7, 14, duration, scale, 1, 28, 60, 0, -1]
                .into_iter()
                .flat_map(i16::to_be_bytes)
                .collect::<Vec<_>>();
            let (track, loops) = commands(&bytes).unwrap();
            assert!(!loops);
            assert!(matches!(track.as_slice(), [
                TimedCommand { tick: 0, command: ActionCommand::BoneScale {
                    slot: 7, bone: 14, duration: actual_duration, scale: actual_scale,
                } },
                TimedCommand { tick: 1, command: ActionCommand::Sound(60) },
            ] if *actual_duration == duration && *actual_scale == f32::from(scale) * 0.1));
        }
        for (slot, bone) in [(-1i16, 0i16), (8, 0), (0, -1)] {
            let bytes = [0i16, 32, slot, bone, 20, 10, -1]
                .into_iter()
                .flat_map(i16::to_be_bytes)
                .collect::<Vec<_>>();
            assert!(commands(&bytes).is_err());
        }
        for value in [0u16, 4, 255, 0x101, u16::MAX] {
            let bytes = [
                0, 35, value, 0xabcd, 1, 33, 1, 2, 3, 4, 5, 6, 7, 8, 2, 28, 60, 0, 0xffff,
            ]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
            let (track, loops) = commands(&bytes).unwrap();
            assert!(!loops);
            assert!(matches!(track.as_slice(), [
                TimedCommand { tick: 0, command: ActionCommand::TextureVariant(actual) },
                TimedCommand { tick: 1, command: ActionCommand::Noop },
                TimedCommand { tick: 2, command: ActionCommand::Sound(60) },
            ] if *actual == value as u8));
        }
        for (opcode, resource) in [(15, RecoveryResource::Hp), (16, RecoveryResource::Tp)] {
            for percent in [0i16, 30, -10, i16::MIN, i16::MAX] {
                let bytes = [0, opcode, percent as u16, 0xabcd, 1, 28, 60, 0, 0xffff]
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect::<Vec<_>>();
                let (track, loops) = commands(&bytes).unwrap();
                assert!(!loops);
                assert!(matches!(track.as_slice(), [
                    TimedCommand { command: ActionCommand::Recover { resource: actual, percent: value }, .. },
                    TimedCommand { command: ActionCommand::Sound(60), .. }
                ] if *actual == resource && *value == percent));
            }
        }
        let bytes: Vec<_> = [40i16, 37, -12, -125, 77, 44, 28, 60, 0, -1, 0]
            .into_iter()
            .flat_map(i16::to_be_bytes)
            .collect();
        let (commands, looping) = commands(&bytes).unwrap();
        assert!(!looping);
        assert!(matches!(
            commands.as_slice(),
            [
                TimedCommand {
                    tick: 40,
                    command: ActionCommand::PlayerCameraMotion {
                        duration: -12,
                        amount: -12.5,
                    }
                },
                TimedCommand {
                    tick: 44,
                    command: ActionCommand::Sound(60)
                },
            ]
        ));
        for percent in [0u16, 125, u16::MAX] {
            let bytes = [0, 24, percent, 0xabcd, 1, 28, 60, 0, 0xffff]
                .into_iter()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>();
            let (track, loops) = super::commands(&bytes).unwrap();
            assert!(!loops);
            assert!(matches!(track.as_slice(), [
                TimedCommand { command: ActionCommand::DamagePower(value), .. },
                TimedCommand { command: ActionCommand::Sound(60), .. }
            ] if *value == percent));
            let mut command = super::super::action_program::Command {
                tick: 0,
                kind: resonance_content::battle::action_program::CommandKind::DamagePower,
                operands: vec![percent, 0xabcd],
            };
            let restored = serde_json::from_value(serde_json::to_value(&command).unwrap()).unwrap();
            assert!(matches!(super::lower_command(&restored).unwrap().command,
                ActionCommand::DamagePower(value) if value == percent));
            command.operands.pop();
            assert!(super::lower_command(&command).is_err());
        }
        for (kind, rewards) in [(30, true), (31, false)] {
            let bytes: Vec<_> = [21i16, kind, -1]
                .into_iter()
                .flat_map(i16::to_be_bytes)
                .collect();
            let (commands, looping) = super::commands(&bytes).unwrap();
            assert!(!looping);
            assert!(matches!(commands.as_slice(), [TimedCommand {
                tick: 21,
                command: ActionCommand::Defeat { rewards: actual },
            }] if *actual == rewards));
        }
        let bytes: Vec<_> = [0u16, 29, 0xffff, 0x180, 0x140, 0xabcd, 2, 31, 0xffff]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect();
        let (commands, looping) = super::commands(&bytes).unwrap();
        assert!(!looping);
        assert!(matches!(
            commands.as_slice(),
            [
                TimedCommand {
                    tick: 0,
                    command: ActionCommand::AmbientColor([255, 128, 64]),
                },
                TimedCommand {
                    tick: 2,
                    command: ActionCommand::Defeat { rewards: false },
                }
            ]
        ));
    }
}

fn required_techniques(
    catalogue: &crate::arte::Catalogue,
    selected: &[u16],
    enemies: &[EnemyActions],
) -> Result<Vec<u16>> {
    let mut ids: BTreeSet<u16> = selected.iter().copied().collect();
    for binding in enemies.iter().flat_map(EnemyActions::native_spells) {
        ensure!(
            binding.supported(),
            "unsupported enemy native spell binding {binding:?}"
        );
        let native = binding.native_id();
        let id = catalogue
            .definitions
            .iter()
            .position(|row| row.native_id as u16 == native)
            .context("enemy spell has no original menu binding")?;
        ids.insert(id.try_into()?);
    }
    Ok(ids.into_iter().collect())
}

#[test]
fn enemy205_action4_turn_keeps_the_six_byte_command_and_signed_angle() {
    // Original action4: voice5, sound15, absolute position15, motion turn15.
    let words = [
        5u16, 27, 34076, 2, 15, 28, 136, 0, 15, 45, 0, 80, 0, 0, 15, 41, 1800, 0xffff,
    ];
    let bytes = words
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    let (program, loops) = commands(&bytes).unwrap();
    assert!(!loops);
    assert!(matches!(program.as_slice(), [
        TimedCommand { tick: 5, command: ActionCommand::Voice { id: 34076, priority: 2 } },
        TimedCommand { tick: 15, command: ActionCommand::Sound(136) },
        TimedCommand { tick: 15, command: ActionCommand::SetPosition([0, 80, 0]) },
        TimedCommand { tick: 15, command: ActionCommand::TurnMotion(angle) },
    ] if *angle == std::f32::consts::PI));
    let bytes = [15i16, 41, -900, 16, 4, -1]
        .into_iter()
        .flat_map(i16::to_be_bytes)
        .collect::<Vec<_>>();
    let (program, loops) = commands(&bytes).unwrap();
    assert!(!loops);
    assert!(matches!(program.as_slice(), [
        TimedCommand { tick: 15, command: ActionCommand::TurnMotion(angle) },
        TimedCommand { tick: 16, command: ActionCommand::Reverse },
    ] if *angle == -std::f32::consts::FRAC_PI_2));
    assert!(commands(&[0, 15, 0, 41, 7]).is_err());
    let bytes: Vec<_> = [0i16, 8, -125, 1, 40, -900, 2, 48, 1234, 3, 8, 20, -1]
        .into_iter()
        .flat_map(i16::to_be_bytes)
        .collect();
    assert!(matches!(commands(&bytes).unwrap().0.as_slice(), [
        TimedCommand { tick: 0, command: ActionCommand::AdvancePosition(-12.5) },
        TimedCommand { tick: 1, command: ActionCommand::TurnHeading(angle) },
        TimedCommand { tick: 2, command: ActionCommand::FaceTargetDirection },
        TimedCommand { tick: 3, command: ActionCommand::AdvancePosition(2.) },
    ] if *angle == -std::f32::consts::FRAC_PI_2));
    for opcode in [8i16, 40, 48] {
        let bytes: Vec<_> = [0i16, opcode]
            .into_iter()
            .flat_map(i16::to_be_bytes)
            .collect();
        assert!(commands(&bytes).is_err());
    }
}

#[test]
#[ignore = "requires the original extracted disc; no asset encoding"]
fn original_enemy_target_facing_command_tracks() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    for monster in [130, 131] {
        let start = word(&usual, table + monster * 4).unwrap() as usize;
        let end = word(&usual, table + (monster + 1) * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        let rows = usize::from(half(&bytes, 10).unwrap());
        let end = usize::from(half(&bytes, 12).unwrap());
        let bank = usize::from(half(&bytes, 14).unwrap());
        let mut facing_tracks = 0;
        for row in bytes[rows..end].chunks_exact(ACTION_BYTES) {
            let offset = usize::from(half(row, 0x1a).unwrap()) * 2;
            let (track, loops) = commands(&bytes[bank + offset..]).unwrap();
            for step in track {
                if matches!(step.command, ActionCommand::FaceTargetDirection) {
                    assert_eq!(step.tick, 278);
                    assert!(!loops);
                    facing_tracks += 1;
                }
            }
        }
        assert!(
            facing_tracks > 0,
            "enemy {monster} lost target-facing action"
        );
    }
}
