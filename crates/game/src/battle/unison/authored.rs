//! Imported roles prepare the same actor actions and feedback as legacy families.
use super::*;
use anyhow::ensure;
use resonance_battle::{AttackOrigin, UnisonPlacement, UnisonRole};
use resonance_content::battle_unison::{Callback, Execution, Origin};

fn phase(role: &resonance_content::battle_unison::Role, actor: &Participant<'_>) -> Option<u8> {
    role.phases.get(actor.character.resource_name()?).copied()
}

pub(super) fn has_pair(execution: &Execution, actors: &[Participant<'_>]) -> bool {
    actors.iter().any(|first| {
        phase(&execution.roles[0], first).is_some()
            && actors.iter().any(|second| {
                first.actor != second.actor && phase(&execution.roles[1], second).is_some()
            })
    })
}

fn origin(value: Origin) -> Result<AttackOrigin> {
    Ok(match value {
        Origin::ActorRoot => AttackOrigin::ActorRoot,
        Origin::ActorAim => AttackOrigin::ActorAim,
        Origin::TargetRoot => AttackOrigin::TargetRoot,
        Origin::TargetAim => AttackOrigin::TargetAim,
        Origin::World(position) => {
            ensure!(
                position.iter().all(|v| v.is_finite()),
                "invalid Unison world point"
            );
            AttackOrigin::World(position)
        }
        Origin::TargetAimHeight { minimum_height } => {
            ensure!(minimum_height.is_finite(), "invalid Unison height clamp");
            AttackOrigin::TargetAimHeight { minimum_height }
        }
        Origin::TargetRootOffset { offset, rotated } => {
            ensure!(
                offset.iter().all(|v| v.is_finite()),
                "invalid Unison offset"
            );
            AttackOrigin::TargetRootOffset { offset, rotated }
        }
        Origin::TargetAimOffset { offset, rotated } => {
            ensure!(
                offset.iter().all(|v| v.is_finite()),
                "invalid Unison offset"
            );
            AttackOrigin::TargetAimOffset { offset, rotated }
        }
        Origin::WorldOffset { offset, rotated } => {
            ensure!(
                offset.iter().all(|v| v.is_finite()),
                "invalid Unison offset"
            );
            AttackOrigin::WorldOffset { offset, rotated }
        }
    })
}

pub(super) fn prepare(
    execution: &Execution,
    scene: &Scene,
    bank: &EffectResource,
    participants: &[Participant<'_>],
    resources: &mut Resources<'_>,
    actions: &mut ActionDefinitions,
) -> Result<UnisonCombination> {
    execution.validate()?;
    ensure!(
        has_pair(execution, participants),
        "authored Unison has no distinct participant pair"
    );
    let mut roles: [Vec<UnisonRole>; 2] = Default::default();
    for (index, role) in execution.roles.iter().enumerate() {
        for actor in participants {
            let Some(variant) = phase(role, actor) else {
                continue;
            };
            let mut source = scene
                .actions
                .get(&variant)
                .context("missing authored Unison participant phase")?
                .prepared()?
                .clone();
            action::remove_empty_blends(&mut source, actor.null_motions)?;
            let mut callbacks = Vec::new();
            for (tick, callback) in &role.events {
                let at = action::elapsed(&source, *tick)?;
                ensure!(
                    at <= source.duration,
                    "authored Unison callback exceeds participant duration"
                );
                let event = match callback {
                    Callback::Ordinary(event) => {
                        source.events.push((at, event.clone()));
                        continue;
                    }
                    Callback::Effect {
                        member,
                        origin: at,
                        scale,
                        follow,
                    }
                    | Callback::CommonEffect {
                        member,
                        origin: at,
                        scale,
                        follow,
                    } => {
                        ensure!(
                            !*follow
                                || matches!(
                                    at,
                                    Origin::ActorRoot
                                        | Origin::ActorAim
                                        | Origin::TargetRoot
                                        | Origin::TargetAim
                                ),
                            "Unison offset effects cannot follow an unshifted anchor"
                        );
                        let effect_bank = if matches!(callback, Callback::CommonEffect { .. }) {
                            &resources.common
                        } else {
                            bank
                        };
                        AttackEvent::EffectAt {
                            appearance: resources.prepared.effect_member(effect_bank, *member)?,
                            origin: origin(*at)?,
                            heading: None,
                            scale: *scale,
                            follow: *follow,
                        }
                    }
                    Callback::Projectile {
                        member,
                        hit_variant,
                        origin: at,
                        heading,
                        height,
                    } => AttackEvent::ProjectileAt {
                        definition: family::projectile(
                            scene,
                            usize::from(*member),
                            usize::from(*hit_variant),
                            bank,
                            resources,
                        )?,
                        origin: origin(*at)?,
                        heading: *heading,
                        height: *height,
                    },
                    Callback::Feedback { duration, limit } => AttackEvent::UnisonFeedback {
                        duration: *duration,
                        limit: *limit,
                    },
                    Callback::PositionFromTarget { height, retreat } => {
                        AttackEvent::PositionFromTarget {
                            height: *height,
                            retreat: *retreat,
                        }
                    }
                };
                callbacks.push((at, event));
            }
            source.events.sort_by_key(|(at, _)| *at);
            let mut attack = action::prepare(&source, actor, bank, resources)?;
            attack.events.extend(callbacks);
            attack.events.sort_by_key(|(at, _)| *at);
            let temporary_weapons = if role.temporary_weapons {
                ensure!(
                    !actor.temporary_weapons.is_empty(),
                    "missing authored Unison temporary weapon"
                );
                actor.temporary_weapons.clone()
            } else {
                Vec::new()
            };
            roles[index].push(UnisonRole {
                actor: actor.actor,
                action: actions.insert(ActionDefinition {
                    normal: None,
                    tp_cost: 0,
                    execution: ActionExecution::Attack(attack),
                }),
                placement: role.placement.map(|p| UnisonPlacement {
                    direction: p.direction,
                    distance: p.distance,
                }),
                visible: role.visible,
                temporary_weapons,
            });
        }
    }
    let mut voices = Vec::new();
    for voice in &execution.voices {
        let mut pair = Vec::new();
        for (role, name) in voice.characters.iter().enumerate() {
            if let Some(actor) = participants.iter().find(|actor| {
                actor.character.resource_name() == Some(name.as_str())
                    && phase(&execution.roles[role], actor).is_some()
            }) {
                pair.push(actor.actor);
            }
        }
        if pair.len() == 2 && pair[0] != pair[1] {
            if let Some(sound) = (resources.sound)(voice.sound)? {
                voices.push(([pair[0], pair[1]], pair[usize::from(voice.speaker)], sound));
            }
        }
    }
    Ok(UnisonCombination {
        roles,
        color: execution.color.or(scene.stage_color),
        voices,
    })
}
