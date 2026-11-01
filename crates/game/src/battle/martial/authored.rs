//! Bind the party action table to the encounter's model, sound, and effect banks.
use super::{Character, Sequence, Sound, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result, bail, ensure};
use resonance_battle::{AttackEvent, AttackPose, PreparedAttack};
use resonance_content::{
    battle_action::{ActionPose, ContactAttachments},
    battle_martial::{Attack, PoseStart, gameplay_tick},
    battle_normal::Event,
};
use std::sync::Arc;

/// Flatten completion-relative poses once the character's motion lengths are known.
fn resolve_pose_sequence(
    attack: &mut Attack,
    motion_duration: impl Fn(u16) -> Option<f32>,
) -> Result<()> {
    let Some(sequence) = attack.pose_sequence.take() else {
        return Ok(());
    };
    let mut previous: Option<(u16, ActionPose)> = None;
    let mut holds = Vec::new();
    for step in sequence {
        let at = match step.start {
            PoseStart::At(at) => at,
            PoseStart::AfterPrevious => {
                let (start, pose) = previous.context("pose continuation has no preceding pose")?;
                ensure!(
                    !pose.repeat && pose.rate.is_finite() && pose.rate > 0.,
                    "pose continuation requires a finite forward motion"
                );
                let motion_end =
                    motion_duration(pose.clip).context("missing continued martial motion")?;
                let duration = (pose.end.unwrap_or(motion_end) - pose.start) / pose.rate;
                ensure!(
                    duration.is_finite() && duration >= 0. && duration < f32::from(u16::MAX),
                    "invalid continued martial motion length"
                );
                // The previous clip's final sample is displayed before its successor.
                start
                    .checked_add(duration.floor() as u16)
                    .and_then(|at| at.checked_add(1))
                    .context("martial pose continuation exceeds timeline limit")?
            }
        };
        ensure!(
            previous.is_none_or(|(start, _)| at >= start),
            "martial pose sequence moves backwards"
        );
        holds.push((at, step.pose.blend));
        attack.events.push((at, Event::Pose(step.pose)));
        previous = Some((at, step.pose));
    }
    let elapsed = |at: u16, pose_clock: bool| -> Result<u16> {
        if !pose_clock && at != 0 {
            return gameplay_tick(at, 0, &holds);
        }
        let hold: u32 = holds
            .iter()
            .filter(|(start, _)| *start < at)
            .map(|(_, blend)| u32::from(*blend))
            .sum();
        (u32::from(at) + hold)
            .try_into()
            .context("martial timeline exceeds limit")
    };
    let mut preceding_contacts = 0;
    for (at, event) in &mut attack.events {
        *at = if matches!(event, Event::Contact(_) | Event::Projectile { .. }) {
            let elapsed = gameplay_tick(*at, preceding_contacts, &holds)?;
            if matches!(event, Event::Contact(_)) {
                preceding_contacts = preceding_contacts
                    .checked_add(1)
                    .context("too many martial contact rows")?;
            }
            elapsed
        } else if *at == 0
            && matches!(
                event,
                Event::Forward(_) | Event::Vertical(_) | Event::Gravity(_)
            )
        {
            holds.first().map_or(0, |(_, blend)| u16::from(*blend))
        } else {
            elapsed(
                *at,
                matches!(event, Event::Pose(_) | Event::AnimationRate(_)),
            )?
        };
        if matches!(event, Event::AnimationRate(_)) {
            *at = at
                .checked_add(1)
                .context("martial animation rate exceeds limit")?;
        }
    }
    attack.duration = elapsed(attack.duration, false)?
        .checked_sub(1)
        .context("empty martial action")?;
    attack.chain_at = attack
        .chain_at
        .map(|at| elapsed(at, false))
        .transpose()?
        .filter(|at| *at <= attack.duration);
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}

impl Technique {
    pub(super) fn authored_timeline(
        self,
        table: &resonance_content::battle_profile::Table,
    ) -> Result<Attack> {
        table
            .martial_attack(self.catalogue, self.character as u8)
            .cloned()
    }

    pub(super) fn prepare_authored(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let attack = self.authored_timeline(resources.voices.party_table())?;
        self.prepare_authored_attack(resources, model, attack)
    }

    pub(super) fn prepare_authored_attack(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
        mut attack: resonance_content::battle_martial::Attack,
    ) -> Result<PreparedAttack> {
        let model = model.context("missing martial action model")?;
        resolve_pose_sequence(&mut attack, |clip| {
            model
                .motions
                .get(&clip)
                .map(|motion| motion.duration_frames)
        })?;
        if self.sequence != Sequence::Asset
            && matches!(
                (self.character, self.catalogue),
                (Character::Lloyd, 20 | 22) | (Character::Regal, 191)
            )
        {
            let contact = attack
                .events
                .iter_mut()
                .find_map(|(_, event)| match event {
                    Event::Contact(contact) => Some(contact),
                    _ => None,
                })
                .context("martial action has no opening contact")?;
            let ContactAttachments::BodyBone(bone) = &mut contact.attachments else {
                bail!("martial opening contact is not attached to the body");
            };
            *bone = model
                .skeleton
                .bones
                .iter()
                .position(|bone| bone.name == "shoulder-right")
                .context("martial right shoulder is missing")?
                .try_into()
                .context("martial shoulder index exceeds limit")?;
        }
        let effect_bank = resources
            .authored_effects
            .get(&self.catalogue)
            .unwrap_or(&resources.techniques)
            .clone();
        let recoil = resources.recoil;
        let pose = |source: ActionPose| -> Result<AttackPose> {
            crate::battle::model::action_pose(Some(model), source)
                .context("missing martial action pose")
        };
        // Grounded arte movement loses this speed each gameplay tick.
        const GROUND_BRAKING: f32 = 0.55;
        let mut events = vec![
            (0, AttackEvent::Notice { duration: 90 }),
            (0, AttackEvent::Braking(GROUND_BRAKING)),
        ];
        if let Some(member) = attack.effect {
            let appearance = resources.prepared.effect_member(&effect_bank, member)?;
            events.push((
                0,
                AttackEvent::ActionEffect {
                    appearance,
                    origin: resonance_battle::AttackOrigin::ActorRoot,
                },
            ));
        }
        for (at, event) in &attack.events {
            let event = match event {
                Event::Pose(source) => {
                    if !model.motions.contains_key(&source.clip) {
                        if resources
                            .party_null_motions
                            .get(&self.character)
                            .is_some_and(|slots| slots.contains(&source.clip))
                        {
                            // An authored empty slot keeps the current pose.
                            continue;
                        }
                    }
                    AttackEvent::Pose(pose(*source)?)
                }
                Event::Contact(contact) => {
                    let mut hit = crate::battle::hit::authored(&contact.hit, recoil)?;
                    if let Some(sound) = hit.sound {
                        hit.sound = (resources.sound)(sound)?;
                    }
                    if let Some(member) = contact.hit.effect {
                        hit.effect = Some(resources.prepared.effect_member(&effect_bank, member)?);
                    }
                    if matches!(self.sequence, Sequence::FierceDemonFang)
                        && let ContactAttachments::Groups(groups) = &contact.attachments
                        && let Some(&slot) = groups.first()
                    {
                        // Contact duration includes its final update. The ribbon
                        // remains active for four updates per authored contact tick.
                        const RIBBON_UPDATES_PER_CONTACT_TICK: u16 = 4;
                        let duration = contact
                            .duration
                            .checked_sub(1)
                            .and_then(|ticks| ticks.checked_mul(RIBBON_UPDATES_PER_CONTACT_TICK))
                            .context("Fierce Demon Fang ribbon duration exceeds limit")?;
                        events.push((
                            *at,
                            AttackEvent::Trail {
                                slot: resonance_battle::WeaponSlot(slot),
                                duration,
                            },
                        ));
                    }
                    let volume = if matches!(contact.attachments, ContactAttachments::BodyCenter {})
                    {
                        resonance_battle::MeleeVolume::BodyCenter {
                            shape: contact.shape,
                            radius: contact.radius,
                            half_height: contact.height,
                        }
                    } else {
                        resonance_battle::MeleeVolume::Attached {
                            anchors: model.contact_anchors(&contact.attachments)?,
                            shape: contact.shape,
                            radius: contact.radius,
                            half_height: contact.height,
                        }
                    };
                    AttackEvent::Contact {
                        definition: Arc::new(resonance_battle::MeleeDefinition {
                            hit,
                            volume,
                            trail: None,
                        }),
                        duration: contact.duration,
                    }
                }
                Event::Projectile {
                    member,
                    hit,
                    feedback,
                } => {
                    let row = resources
                        .projectiles
                        .records
                        .get(usize::from(*member))
                        .context("missing martial projectile artwork")?
                        .clone();
                    let hit = hit.as_ref().context("missing martial projectile hit")?;
                    AttackEvent::Projectile(super::sword::projectile(
                        resources, &row, hit, *feedback, recoil,
                    )?)
                }
                Event::Sound(cue) => AttackEvent::Sound((resources.sound)(Sound::Cue(*cue))?),
                Event::Voice(voice) => {
                    let Some(line) = resources.voices.absolute(
                        crate::battle::model::ModelSource::Party(self.character),
                        *voice,
                        &mut resources.sound,
                    )?
                    else {
                        continue;
                    };
                    AttackEvent::Voice {
                        sound: line,
                        priority: resonance_battle::VoicePriority::Action,
                    }
                }
                Event::Effect(member) => AttackEvent::ActionEffect {
                    appearance: resources.prepared.effect_member(&effect_bank, *member)?,
                    origin: resonance_battle::AttackOrigin::ActorRoot,
                },
                Event::Throw { .. } => {
                    bail!("unsupported martial action event")
                }
                other => crate::battle::normal::movement_event(other)?,
            };
            events.push((*at, event));
        }
        let already_idle = attack
            .events
            .iter()
            .rev()
            .find(|(at, event)| {
                *at <= attack.duration && matches!(event, Event::Pose(_) | Event::AnimationRate(_))
            })
            .is_some_and(|(_, event)| {
                matches!(event, Event::Pose(pose)
                    if pose.clip == 0 && pose.start == 0. && pose.end.is_none()
                        && pose.rate == 0.5)
            });
        if !already_idle {
            events.push((
                attack.duration,
                AttackEvent::Pose(pose(ActionPose {
                    clip: 0,
                    start: 0.,
                    end: None,
                    rate: 0.5,
                    blend: attack
                        .recovery
                        .try_into()
                        .context("martial recovery exceeds limit")?,
                    repeat: true,
                })?),
            ));
        }
        events.sort_by_key(|(at, _)| *at);
        Ok(PreparedAttack {
            contact_recovery: None,
            opening: None,
            events,
            chain_at: attack.chain_at,
            end_at: attack.duration,
            recovery: attack.recovery,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_martial::PoseStep;

    #[test]
    fn consecutive_kicks_follow_clip_completion_and_blends() -> Result<()> {
        let pose = |clip, blend, start, end, rate| ActionPose {
            clip,
            start,
            end,
            blend,
            rate,
            repeat: false,
        };
        let mut attack = Attack {
            duration: 75,
            chain_at: Some(65),
            recovery: 10,
            effect: None,
            events: vec![(0, Event::Gravity(0.1)), (46, Event::Forward(7.))],
            pose_sequence: Some(vec![
                PoseStep {
                    start: PoseStart::At(0),
                    pose: pose(34, 4, 0., None, 0.5),
                },
                PoseStep {
                    start: PoseStart::AfterPrevious,
                    pose: pose(35, 0, 0., Some(10.), 0.5),
                },
                PoseStep {
                    start: PoseStart::AfterPrevious,
                    pose: pose(44, 4, 10., None, 0.45),
                },
            ]),
            flight: None,
            hammer: None,
            steal: None,
            fierce_fang: None,
            hurricane_hit: None,
            callback_hits: Default::default(),
        };
        resolve_pose_sequence(&mut attack, |clip| match clip {
            34 => Some(12.),
            35 => Some(22.),
            _ => None,
        })?;
        assert_eq!(
            attack
                .events
                .iter()
                .filter_map(|(at, event)| { matches!(event, Event::Pose(_)).then_some(*at) })
                .collect::<Vec<_>>(),
            [0, 29, 50]
        );
        assert_eq!(attack.duration, 81);
        assert!(
            attack
                .events
                .iter()
                .any(|(at, event)| *at == 50 && matches!(event, Event::Forward(7.)))
        );
        assert!(
            attack
                .events
                .iter()
                .any(|(at, event)| *at == 4
                    && matches!(event, Event::Gravity(value) if *value == 0.1))
        );
        assert!(attack.pose_sequence.is_none());
        // Re-preparation must not count the same blend holds twice.
        resolve_pose_sequence(&mut attack, |_| None)?;
        assert_eq!(attack.duration, 81);
        Ok(())
    }
}
