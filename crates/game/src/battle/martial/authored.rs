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
fn pose_completion(start: u16, pose: ActionPose, end: f32, controls: &[(u16, f32)]) -> Result<u16> {
    ensure!(
        !pose.repeat && pose.rate.is_finite() && pose.rate >= 0.,
        "pose continuation requires a finite forward motion"
    );
    ensure!(
        end.is_finite() && pose.start.is_finite() && end >= pose.start,
        "invalid continued martial motion length"
    );
    let (mut frame, mut rate) = (pose.start, pose.rate);
    let mut controls = controls
        .iter()
        .map(|&(at, rate)| (u32::from(at) + 1, rate))
        .filter(|&(at, _)| at > u32::from(start))
        .peekable();
    // A replacement retains its starting sample. On each later update, rate
    // events apply before the model advances; reaching the exact end still
    // displays that sample, and only crossing it completes the pose. Accumulate
    // frame-by-frame here as the model clock does, including f32 rounding.
    for at in u32::from(start) + 1..=u32::from(u16::MAX) {
        while controls.peek().is_some_and(|&(change, _)| change == at) {
            rate = controls.next().unwrap().1;
            ensure!(
                rate.is_finite() && rate >= 0.,
                "pose continuation requires finite forward rate controls"
            );
        }
        if rate == 0. && controls.peek().is_none() {
            bail!("continued martial pose remains held without a forward rate");
        }
        frame += rate;
        if frame > end {
            return Ok(at as u16);
        }
    }
    bail!("martial pose continuation exceeds timeline limit")
}

fn resolve_pose_sequence(
    attack: &mut Attack,
    motion_duration: impl Fn(u16) -> Option<f32>,
) -> Result<Option<Vec<(u16, u8)>>> {
    let Some(sequence) = attack.pose_sequence.take() else {
        return Ok(None);
    };
    let mut rate_controls: Vec<_> = attack
        .events
        .iter()
        .filter_map(|(at, event)| match event {
            Event::AnimationRate(rate) => Some((*at, *rate)),
            _ => None,
        })
        .collect();
    rate_controls.sort_by_key(|(at, _)| *at);
    let mut previous: Option<(u16, ActionPose)> = None;
    let mut holds = Vec::new();
    for step in sequence {
        let at = match step.start {
            PoseStart::At(at) => at,
            PoseStart::AfterPrevious => {
                let (start, pose) = previous.context("pose continuation has no preceding pose")?;
                let motion_end =
                    motion_duration(pose.clip).context("missing continued martial motion")?;
                // The previous clip's final sample is displayed before its successor.
                pose_completion(start, pose, pose.end.unwrap_or(motion_end), &rate_controls)?
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
        *at = if matches!(
            event,
            Event::Contact(_) | Event::Projectile { .. } | Event::EmitProjectile { .. }
        ) {
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
    Ok(Some(holds))
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
        attack: resonance_content::battle_martial::Attack,
    ) -> Result<PreparedAttack> {
        self.prepare_attack_presentation(resources, model, attack, Vec::new())
    }

    pub(super) fn prepare_asset_attack(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
        action: resonance_content::battle_martial::AuthoredAction,
    ) -> Result<PreparedAttack> {
        self.prepare_attack_presentation(resources, model, action.attack, action.presentation)
    }

    fn prepare_attack_presentation(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
        mut attack: resonance_content::battle_martial::Attack,
        mut presentation: Vec<(u16, resonance_content::battle_martial::Presentation)>,
    ) -> Result<PreparedAttack> {
        let model = model.context("missing martial action model")?;
        let holds = resolve_pose_sequence(&mut attack, |clip| {
            model
                .motions
                .get(&clip)
                .map(|motion| motion.duration_frames)
        })?;
        if let Some(holds) = holds {
            for (at, _) in &mut presentation {
                if *at != 0 {
                    *at = gameplay_tick(*at, 0, &holds)?;
                }
            }
        }
        let presentation =
            self.prepare_presentation(resources, model, presentation, attack.duration)?;
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
        let effect_bank = if attack.skill_effects {
            resources
                .skill_scenes
                .get(&self.catalogue)
                .context("martial action has no stored skill artwork")?
        } else {
            resources
                .authored_effects
                .get(&self.catalogue)
                .unwrap_or(&resources.techniques)
        }
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
        if let Some(member) = attack.effect.filter(|_| !attack.skill_effects) {
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
                        && let Some(trail) = crate::battle::normal::contact_trail(contact)?
                    {
                        events.push((*at, trail));
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
                Event::EmitProjectile {
                    definition,
                    hit,
                    feedback,
                } => {
                    let local = resources.skill_scenes.get(&self.catalogue).cloned();
                    AttackEvent::Projectile(super::sword::projectile_with_local(
                        resources,
                        definition,
                        hit,
                        *feedback,
                        recoil,
                        local.as_ref(),
                    )?)
                }
                Event::Sound(cue) => AttackEvent::Sound((resources.sound)(Sound::Cue(*cue))?),
                Event::HealOtherAllies { radius, percent } => AttackEvent::HealOtherAllies {
                    radius: *radius,
                    percent: *percent,
                },
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
                Event::VoiceChoice(choice) => AttackEvent::VoiceChoice {
                    choice: resources.voices.choice(
                        crate::battle::model::ModelSource::Party(self.character),
                        *choice,
                        &mut resources.sound,
                    )?,
                    priority: resonance_battle::VoicePriority::Action,
                },
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
        let mut prepared = PreparedAttack {
            contact_recovery: None,
            opening: None,
            events,
            chain_at: attack.chain_at,
            chain_retry_at: None,
            end_at: attack.duration,
            recovery: attack.recovery,
        };
        if let Some(member) = attack.effect.filter(|_| attack.skill_effects) {
            prepared.events.push((
                0,
                AttackEvent::InitializedActionEffect {
                    appearance: resources.prepared.effect_member(&effect_bank, member)?,
                    origin: resonance_battle::AttackOrigin::ActorRoot,
                },
            ));
        }
        prepared.events.extend(presentation);
        prepared.events.sort_by_key(|(at, _)| *at);
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_martial::PoseStep;

    #[test]
    fn continuation_keeps_intervening_held_and_restored_source_rates() -> Result<()> {
        let pose = ActionPose {
            clip: 69,
            start: 0.,
            end: None,
            blend: 0,
            rate: 0.75,
            repeat: false,
        };
        assert_eq!(pose_completion(90, pose, 20., &[])?, 117);
        assert_eq!(
            pose_completion(90, pose, 20., &[(92, 0.05), (165, 0.4)])?,
            203
        );
        // Later controls belong to the next pose and must not lengthen this one.
        assert_eq!(pose_completion(90, pose, 20., &[(150, 0.01)])?, 117);
        assert_eq!(
            pose_completion(90, pose, 20., &[(92, 0.), (165, 0.4)])?,
            212
        );
        assert!(pose_completion(90, pose, 20., &[(92, 0.)]).is_err());
        assert!(pose_completion(90, pose, 20., &[(92, -1.)]).is_err());
        Ok(())
    }

    #[test]
    fn continuation_applies_rate_changes_before_their_model_sample() -> Result<()> {
        let pose = ActionPose {
            clip: 0,
            start: 0.,
            end: Some(1.),
            blend: 0,
            rate: 0.5,
            repeat: false,
        };
        // The real Model clock counterpart is covered in battle/model/tests.rs.
        // Start sample 0; tick 1 -> .5; tick 2 applies the hold before advancing.
        assert!(pose_completion(0, pose, 1., &[(1, 0.)]).is_err());
        assert_eq!(pose_completion(0, pose, 1., &[(1, 0.), (4, 0.5)])?, 6);
        // Holding at the would-be completion update retains the final sample.
        assert!(pose_completion(0, pose, 1., &[(2, 0.)]).is_err());
        assert_eq!(pose_completion(0, pose, 1., &[])?, 3);
        Ok(())
    }

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
            skill_effects: false,
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
