//! Resolve scene participant tracks through the ordinary attack owner.
use super::*;
use resonance_content::{
    battle_action::{ActionPose, ContactAttachments},
    battle_normal::Event,
};

pub(super) fn prepare<S: FnMut(Sound) -> Result<Option<Sound>>>(
    source: &resonance_content::battle_martial::Attack,
    actor: &Participant<'_>,
    bank: &EffectResource,
    resources: &mut Resources<'_, S>,
) -> Result<resonance_battle::PreparedAttack> {
    let recoil = resources.recoil;
    let mut model = actor.model.clone();
    for weapon in &actor.temporary_weapons {
        model.weapons.retain(|old| old.slot != weapon.slot);
        model.weapons.push(Arc::clone(weapon));
    }
    let pose = |source: ActionPose| {
        prepare_pose(
            source,
            crate::battle::model::motion(Some(&model), source.clip),
            actor.null_motions,
        )
    };
    let mut events = vec![(0, AttackEvent::Braking(0.55))];
    if let Some(member) = source.effect {
        let appearance = resources.prepared.effect_member(bank, member)?;
        events.push((
            0,
            AttackEvent::ActionEffect {
                appearance,
                origin: resonance_battle::AttackOrigin::ActorRoot,
            },
        ));
    }
    for &(at, ref event) in &source.events {
        let event = match event {
            Event::Pose(value) => {
                let Some(pose) = pose(*value)? else { continue };
                AttackEvent::Pose(pose)
            }
            Event::Contact(contact) => {
                let mut hit = crate::battle::hit::authored(&contact.hit, recoil)?;
                if let Some(sound) = contact.hit.sound {
                    hit.sound = (resources.sound)(Sound::Cue(sound))?;
                }
                if let Some(member) = contact.hit.effect {
                    hit.effect = Some(resources.prepared.effect_member(bank, member)?);
                }
                let volume = if matches!(contact.attachments, ContactAttachments::BodyCenter {}) {
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
            Event::Sound(sound) => AttackEvent::Sound((resources.sound)(Sound::Cue(*sound))?),
            Event::Voice(sound) => {
                let Some(sound) = resources.voices.absolute(
                    crate::battle::model::ModelSource::Party(actor.character),
                    *sound,
                    &mut resources.sound,
                )?
                else {
                    continue;
                };
                AttackEvent::Voice {
                    sound,
                    priority: resonance_battle::VoicePriority::Action,
                }
            }
            Event::Effect(member) => AttackEvent::ActionEffect {
                appearance: resources.prepared.effect_member(bank, *member)?,
                origin: resonance_battle::AttackOrigin::ActorRoot,
            },
            Event::Throw { .. } | Event::Projectile { .. } => {
                anyhow::bail!("unsupported combination participant emission")
            }
            other => crate::battle::normal::movement_event(other)?,
        };
        events.push((at, event));
    }
    if let Some(flight) = &source.flight {
        events.push((
            source.duration,
            AttackEvent::Land {
                pose: pose(flight.finish_pose)?,
                effect: None,
                settle: None,
                recovery_pose: None,
            },
        ));
    } else if let Some(pose) = pose(ActionPose {
        clip: 0,
        start: 0.,
        end: None,
        rate: 0.5,
        blend: 8,
        repeat: true,
    })? {
        events.push((source.duration, AttackEvent::Pose(pose)));
    }
    events.sort_by_key(|(at, _)| *at);
    Ok(resonance_battle::PreparedAttack {
        events,
        recovery: source.recovery,
        ..resonance_battle::PreparedAttack::new(source.duration)
    })
}

/// Remove only waits for motions explicitly absent from this participant's bank.
pub(super) fn remove_empty_blends(
    source: &mut resonance_content::battle_martial::Attack,
    null_motions: &BTreeSet<u16>,
) -> Result<()> {
    let mut pose_holds = Vec::new();
    let mut command_holds = Vec::new();
    let mut command_overlap = 0u16;
    for &(at, ref event) in &source.events {
        let Event::Pose(pose) = event else { continue };
        // Later blend admissions already advance the command clock by one.
        let command_blend = if at == 0 {
            pose.blend
        } else {
            pose.blend.saturating_sub(1)
        };
        if empty_pose(*pose, null_motions) {
            pose_holds.push((at, pose.blend));
            command_holds.push((
                at.checked_sub(command_overlap)
                    .context("invalid combination pose age")?,
                command_blend,
            ));
        }
        command_overlap = command_overlap
            .checked_add(u16::from(pose.blend - command_blend))
            .context("combination interval overflow")?;
    }
    if pose_holds.is_empty() {
        return Ok(());
    }
    for (at, event) in &mut source.events {
        let holds = if matches!(
            event,
            Event::Pose(_) | Event::AnimationRate(_) | Event::Contact(_) | Event::Projectile { .. }
        ) {
            &pose_holds
        } else {
            &command_holds
        };
        *at = remove_holds(*at, holds)?;
        if let Event::Pose(pose) = event
            && empty_pose(*pose, null_motions)
        {
            pose.blend = 0;
        }
    }
    // The terminal update is inclusive; its authored deadline was one tick later.
    let deadline = source
        .duration
        .checked_add(1)
        .context("combination deadline overflow")?;
    source.duration = remove_holds(deadline, &command_holds)?
        .checked_sub(1)
        .context("empty combination duration")?;
    if let Some(at) = &mut source.chain_at {
        *at = remove_holds(*at, &command_holds)?;
    }
    if let Some(flight) = &mut source.flight {
        flight.gravity_at = remove_holds(flight.gravity_at, &command_holds)?;
    }
    source.events.sort_by_key(|(at, _)| *at);
    Ok(())
}

fn remove_holds(at: u16, holds: &[(u16, u8)]) -> Result<u16> {
    holds
        .iter()
        .filter(|(start, _)| at > *start)
        .try_fold(at, |remaining, (_, blend)| {
            remaining
                .checked_sub(u16::from(*blend))
                .context("invalid combination event age")
        })
}

pub(super) fn remove_empty_opening_blend(
    source: &mut resonance_content::battle_martial::Attack,
    null_motions: &BTreeSet<u16>,
) -> Result<()> {
    let hold = source
        .events
        .iter()
        .find_map(|(at, event)| match event {
            Event::Pose(pose) if *at == 0 && empty_pose(*pose, null_motions) => {
                Some(u16::from(pose.blend.saturating_sub(1)))
            }
            _ => None,
        })
        .unwrap_or(0);
    for (at, _) in &mut source.events {
        if *at != 0 {
            *at = at.checked_sub(hold).context("invalid Unison opening age")?;
        }
    }
    // The opening owner has a fixed deadline, independent of the pose blend.
    Ok(())
}

fn empty_pose(pose: ActionPose, null_motions: &BTreeSet<u16>) -> bool {
    pose.clip < 100 && null_motions.contains(&pose.clip)
}

fn prepare_pose(
    source: ActionPose,
    motion: Option<resonance_battle::MotionBinding>,
    null_motions: &BTreeSet<u16>,
) -> Result<Option<resonance_battle::AttackPose>> {
    let null = empty_pose(source, null_motions);
    anyhow::ensure!(
        !null || motion.is_none(),
        "combination motion {} is both present and authored empty",
        source.clip
    );
    if null {
        return Ok(None);
    }
    Ok(Some(resonance_battle::AttackPose {
        motion: motion.with_context(|| format!("missing combination motion {}", source.clip))?,
        start: source.start,
        end: source.end,
        rate: source.rate,
        blend: source.blend,
        repeat: source.repeat,
    }))
}

/// Callback clocks pause for each admitted pose blend, like the action tracks.
pub(super) fn elapsed(
    source: &resonance_content::battle_martial::Attack,
    tick: u16,
) -> Result<u16> {
    let mut holds = 0u16;
    for (at, event) in &source.events {
        if let Event::Pose(pose) = event {
            let authored = at
                .checked_sub(holds)
                .context("invalid combination pose age")?;
            if authored >= tick {
                break;
            }
            holds = holds
                .checked_add(u16::from(pose.blend))
                .context("combination blend interval overflow")?;
        }
    }
    tick.checked_add(holds)
        .context("combination callback age overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_authored_empty_body_poses_leave_the_current_animation_running() -> Result<()> {
        let pose = ActionPose {
            clip: 46,
            start: 0.,
            end: None,
            rate: 0.5,
            blend: 4,
            repeat: false,
        };
        let nulls = BTreeSet::from([46, 100]);
        assert!(prepare_pose(pose, None, &nulls)?.is_none());
        assert!(prepare_pose(pose, None, &BTreeSet::new()).is_err());
        let motion = resonance_battle::MotionBinding {
            model: resonance_battle::ResourceId(1),
            clip: 46,
        };
        assert!(prepare_pose(pose, Some(motion), &nulls).is_err());
        assert_eq!(
            prepare_pose(pose, Some(motion), &BTreeSet::new())?
                .unwrap()
                .motion,
            motion
        );
        assert!(prepare_pose(ActionPose { clip: 100, ..pose }, None, &nulls).is_err());

        let mut track = resonance_content::battle_martial::Attack {
            duration: 169,
            chain_at: None,
            recovery: 20,
            effect: None,
            events: vec![
                (0, Event::Pose(ActionPose { clip: 0, ..pose })),
                (19, Event::Pose(ActionPose { clip: 61, ..pose })),
                (19, Event::PassThrough(true)),
                (26, Event::Forward(5.)),
                (
                    31,
                    Event::Pose(ActionPose {
                        clip: 61,
                        blend: 0,
                        ..pose
                    }),
                ),
                (32, Event::Sound(60)),
                (63, Event::Pose(ActionPose { clip: 60, ..pose })),
                (67, Event::Braking(0.5)),
                (
                    95,
                    Event::Pose(ActionPose {
                        clip: 60,
                        blend: 0,
                        ..pose
                    }),
                ),
            ],
            flight: None,
            pose_sequence: None,
            callback_hits: Default::default(),
            hammer: None,
            steal: None,
            fierce_fang: None,
            hurricane_hit: None,
        };
        assert_eq!(elapsed(&track, 70)?, 82);
        remove_empty_blends(&mut track, &BTreeSet::from([61]))?;
        assert_eq!(
            track.events.iter().map(|(at, _)| *at).collect::<Vec<_>>(),
            [0, 19, 19, 23, 27, 29, 59, 64, 91]
        );
        assert_eq!((track.duration, elapsed(&track, 70)?), (166, 78));
        assert!(matches!(&track.events[1].1, Event::Pose(pose) if pose.blend == 0));

        track.duration = 63;
        track.events = vec![(0, Event::Pose(pose)), (4, Event::Forward(5.))];
        remove_empty_blends(&mut track, &nulls)?;
        assert_eq!((track.duration, track.events[1].0), (59, 0));

        track.duration = 30;
        track.events = vec![
            (0, Event::Pose(pose)),
            (
                0,
                Event::Trail {
                    slot: 0,
                    duration: 90,
                },
            ),
            (11, Event::Forward(4.)),
        ];
        remove_empty_opening_blend(&mut track, &nulls)?;
        assert_eq!(
            track.events.iter().map(|(at, _)| *at).collect::<Vec<_>>(),
            [0, 0, 8]
        );
        assert_eq!(track.duration, 30);
        Ok(())
    }
}
