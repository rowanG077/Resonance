//! A captured actor follows one body attachment until release or interruption.
#[cfg(test)]
use crate::ResourceId;
use crate::{ActionId, ActorId, Battle, MeleeAnchor, state::ActorTask};
use anyhow::{Context, Result, ensure};

const INTERRUPTED_HITSTUN: u16 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Link {
    captor: ActorId,
    action: ActionId,
    anchor: MeleeAnchor,
    hidden: bool,
    release_kind: crate::RecoilKind,
    release_hitstun: Option<u16>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct State {
    held_by: Option<Link>,
    held_victim: Option<ActorId>,
}

impl State {
    pub(crate) fn retain_recoil(&mut self, kind: crate::RecoilKind) {
        if let Some(link) = &mut self.held_by {
            link.release_kind = kind;
        }
    }
}

impl Battle {
    pub(crate) fn held_victim(&self, captor: ActorId) -> Option<ActorId> {
        self.runtime[captor.index()].capture.held_victim
    }

    pub(crate) fn is_captured(&self, victim: ActorId) -> bool {
        self.runtime[victim.index()].capture.held_by.is_some()
    }

    #[cfg(feature = "test-support")]
    pub fn fixture_capture(&self, victim: ActorId) -> Option<(ActorId, bool)> {
        self.runtime[victim.index()]
            .capture
            .held_by
            .map(|link| (link.captor, link.hidden))
    }

    pub(crate) fn capture_contact(
        &mut self,
        captor: ActorId,
        victim: ActorId,
        action: ActionId,
        anchor: MeleeAnchor,
    ) -> Result<()> {
        if self.held_victim(captor).is_some()
            || self.is_captured(victim)
            || !self.runtime[captor.index()]
                .task()
                .action()
                .is_some_and(|(id, sequence)| *id == action && sequence.running())
        {
            return Ok(());
        }
        ensure!(
            captor != victim
                && matches!(self.runtime[victim.index()].task(), ActorTask::Hurt { .. }),
            "capture requires an initialized hurt reaction"
        );
        let models = self
            .models
            .as_ref()
            .context("capture has no model playback")?;
        let body = models
            .definition(captor)
            .context("capture has no body model")?;
        ensure!(
            anchor.resource == body.resource,
            "capture requires a body attachment"
        );
        models.anchor(captor, anchor)?;
        let link = Link {
            captor,
            action,
            anchor,
            hidden: false,
            release_kind: self.actors[victim.index()].reaction.recoil.kind,
            release_hitstun: None,
        };
        self.runtime[captor.index()].capture.held_victim = Some(victim);
        self.runtime[victim.index()].capture.held_by = Some(link);
        self.actors[victim.index()].reaction.recoil.kind = crate::RecoilKind::Normal;
        Ok(())
    }

    /// Use the completed model pose. Neither hurt timing nor velocity advances here.
    pub(crate) fn follow_capture(
        &mut self,
        victim: ActorId,
        placements: &[Option<resonance_content::animation::Matrix>],
    ) -> Result<bool> {
        let Some(link) = self.runtime[victim.index()].capture.held_by else {
            return Ok(false);
        };
        ensure!(
            self.held_victim(link.captor) == Some(victim),
            "nonreciprocal capture"
        );
        if !self.actors[link.captor.index()].available()
            || !self.runtime[link.captor.index()]
                .task()
                .action()
                .is_some_and(|(id, sequence)| *id == link.action && sequence.running())
        {
            self.interrupt_capture(link.captor);
            return Ok(false);
        }
        let placement = placements
            .get(link.captor.index())
            .copied()
            .flatten()
            .context("capture lost captor placement")?;
        let mut position = self
            .models
            .as_ref()
            .context("capture lost model playback")?
            .anchor_at(link.captor, link.anchor, Some(placement))?;
        position[1] -= self.actors[victim.index()].body.center_offset[1];
        self.actors[victim.index()].position = position;
        if let Some(hitstun) = link.release_hitstun {
            self.release_captured(link.captor, hitstun);
        }
        Ok(true)
    }

    pub(crate) fn captured_visibility(&mut self, captor: ActorId, visible: bool) {
        if let Some(victim) = self.held_victim(captor)
            && let Some(link) = &mut self.runtime[victim.index()].capture.held_by
        {
            link.hidden = !visible;
        }
    }

    pub(crate) fn capture_hidden(&self, victim: ActorId) -> bool {
        self.runtime[victim.index()]
            .capture
            .held_by
            .is_some_and(|link| link.hidden)
    }

    pub(crate) fn captured_velocity(
        &mut self,
        captor: ActorId,
        forward: Option<f32>,
        vertical: Option<f32>,
    ) {
        let Some(victim) = self.held_victim(captor) else {
            return;
        };
        let movement = &mut self.actors[victim.index()].movement;
        if let Some(value) = forward {
            movement.forward = value;
        }
        if let Some(value) = vertical {
            movement.vertical = value;
        }
    }

    /// Finish following this update's attachment before applying the release impulse.
    pub(crate) fn request_capture_release(&mut self, captor: ActorId, hitstun: u16) {
        if let Some(victim) = self.held_victim(captor)
            && let Some(link) = &mut self.runtime[victim.index()].capture.held_by
        {
            link.release_hitstun = Some(hitstun);
        }
    }

    fn release_captured(&mut self, captor: ActorId, hitstun: u16) {
        let Some(victim) = self.runtime[captor.index()].capture.held_victim.take() else {
            return;
        };
        let link = self.runtime[victim.index()].capture.held_by.take();
        let origin = self.actors[captor.index()].position;
        let actor = &mut self.actors[victim.index()];
        if let Some(link) = link {
            actor.reaction.recoil.kind = link.release_kind;
        }
        actor.reaction.direction =
            crate::distance::planar_direction(actor.position, origin, actor.reaction.direction);
        actor.movement.acceleration = 0.;
        actor.movement.gravity = crate::movement::GRAVITY;
        self.set_task(
            victim.index(),
            ActorTask::Hurt {
                remaining: u32::from(hitstun),
            },
        );
        // Clamp the release point without integrating the authored impulse twice.
        self.constrain_actor(victim.index(), None);
    }

    pub(crate) fn interrupt_capture(&mut self, captor: ActorId) {
        self.release_captured(captor, INTERRUPTED_HITSTUN);
    }

    pub(crate) fn release_capture_links(&mut self, actor: ActorId) {
        self.interrupt_capture(actor);
        if let Some(link) = self.runtime[actor.index()].capture.held_by {
            self.interrupt_capture(link.captor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionKey, AttackEvent, BattleInput, ContactSource, Cue, Side};
    use resonance_content::animation::{
        Bone, Track, Transform, TransformChannels, VectorCurve, VectorInterpolation,
    };
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn capture_requires_a_hurt_contact_and_preserves_the_victim_until_release() -> Result<()> {
        let victim = ActorId(0);
        let captor = ActorId(1);
        for (guarded, reflected) in [(true, false), (false, false), (false, true)] {
            let mut target = crate::tests::actor(Side::Party);
            target.control = crate::Control::Manual;
            target.guard.active = guarded;
            target.guard.break_pressure = 10;
            target.facing_direction = [0., 0., -1.];
            target.position = [20., 0., 0.];
            target.body.collider = Some(crate::Collider::sphere(60.));
            target.body.center_offset[1] = 12.;
            let owner = crate::tests::actor(Side::Enemy);
            let mut model = crate::tests::model_definition(7, [(0, 10.)]);
            model.skeleton.bones.push(Bone {
                name: "hold".into(),
                parent: Some(0),
                bind_channels: TransformChannels(8),
                bind: Transform {
                    // Model Z is battle height; model Y points along negative battle Z.
                    translation: [20., 0., 50.],
                    ..Default::default()
                },
            });
            model.motions.get_mut(&0).unwrap().tracks.push(Track {
                bone: 1,
                bind_channels: TransformChannels(8),
                period_frames: 10.,
                times: vec![0., 10.],
                translation: Some(VectorCurve {
                    interpolation: VectorInterpolation::Linear,
                    values: vec![[20., 0., 50.], [40., 0., 50.]],
                    incoming: vec![],
                    outgoing: vec![],
                    ease: vec![],
                }),
                scale: None,
                rotation: None,
                euler_degrees: None,
                matrices: None,
            });
            let mut victim_model = crate::tests::model_definition(8, [(0, 10.)]);
            victim_model.skeleton.bones.push(Bone {
                name: "reception".into(),
                parent: Some(0),
                bind_channels: TransformChannels(8),
                bind: Transform {
                    translation: [0., 0., 12.],
                    ..Default::default()
                },
            });
            victim_model.reception_spheres = Some(vec![crate::BodySphere {
                bone: 1,
                radius: 0.,
            }]);
            victim_model
                .weapons
                .push(Arc::new(crate::WeaponDefinition::new(
                    crate::WeaponSlot(0),
                    0,
                    vec![crate::WeaponLayerDefinition {
                        resource: ResourceId(9),
                        skeleton: victim_model.skeleton.clone(),
                        motions: BTreeMap::new(),
                        playback: crate::WeaponPlayback::Rigid,
                        secondary_motion: vec![],
                        texture_animation: None,
                    }],
                    vec![],
                )?));
            let mut action = crate::tests::action(30);
            crate::tests::attack_mut(&mut action).events.push((
                0,
                AttackEvent::Contact {
                    duration: 1,
                    definition: Arc::new(crate::MeleeDefinition {
                        volume: crate::MeleeVolume::Attached {
                            anchors: vec![MeleeAnchor {
                                resource: ResourceId(7),
                                bone: 1,
                            }],
                            shape: crate::HitShape::Sphere,
                            radius: 60.,
                            half_height: 0.,
                        },
                        trail: None,
                        hit: crate::HitRule {
                            power: crate::Power::Fixed(5),
                            arte: true,
                            prevents_defeat: true,
                            reaction: crate::ReactionRule {
                                capture: true,
                                hitstun: 12,
                                ..Default::default()
                            },
                            ..crate::tests::hit()
                        },
                    }),
                },
            ));
            crate::tests::attack_mut(&mut action)
                .events
                .push((1, AttackEvent::CapturedVisibility(false)));
            let AttackEvent::Contact { definition, .. } =
                &crate::tests::attack_mut(&mut action).events[0].1
            else {
                unreachable!()
            };
            let mut terminal_contact = (**definition).clone();
            terminal_contact.hit.reaction.capture = false;
            terminal_contact.hit.reaction.recoil.launch = true;
            // Narrow enough that the previous update's reception point misses.
            let crate::MeleeVolume::Attached { radius, .. } = &mut terminal_contact.volume else {
                unreachable!()
            };
            *radius = 0.25;
            crate::tests::attack_mut(&mut action).events.push((
                2,
                AttackEvent::Contact {
                    duration: 1,
                    definition: Arc::new(terminal_contact),
                },
            ));
            crate::tests::attack_mut(&mut action)
                .events
                .push((3, AttackEvent::ReleaseCaptured { hitstun: 30 }));
            let mut prepared = crate::PreparedBattle::new(
                vec![(target, Default::default()), (owner, Default::default())],
                vec![action].into(),
                1,
            )?;
            crate::tests::assign_action(&mut prepared, captor.index(), ActionKey(0));
            let mut battle = prepared.finish()?;
            battle.set_models(crate::Models::new(
                battle.actors(),
                vec![Some(Arc::new(victim_model)), Some(Arc::new(model))],
                BTreeMap::new(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )?);
            let frame = battle.step(BattleInput::from(crate::ActionRequest {
                actor: captor,
                target: victim,
                action: ActionKey(0),
            }))?;
            assert!(frame.cues.iter().any(|cue| matches!(cue,
                Cue::Hit { source: ContactSource::Melee { .. }, actor, .. } if *actor == victim
            )));
            assert_eq!(battle.is_captured(victim), !guarded);
            let visible = |battle: &mut Battle, expected: bool| -> Result<()> {
                let mut frame = battle.snapshot();
                battle.advance_models(&mut frame, crate::BattleClock::Held, false)?;
                assert_eq!(
                    frame
                        .models
                        .iter()
                        .find(|model| model.actor == victim)
                        .unwrap()
                        .visible,
                    expected
                );
                assert_eq!(
                    frame
                        .weapons
                        .iter()
                        .find(|weapon| weapon.owner == victim)
                        .unwrap()
                        .visible,
                    expected
                );
                Ok(())
            };
            if guarded {
                battle.captured_visibility(captor, false);
                visible(&mut battle, true)?;
                continue;
            }
            assert_eq!(battle.held_victim(captor), Some(victim));
            let before = match battle.runtime[victim.index()].task() {
                ActorTask::Hurt { remaining } => *remaining,
                _ => panic!("capture did not retain hurt"),
            };
            battle.actors[victim.index()].hit_stop = 4;
            battle.captured_velocity(captor, Some(6.), Some(16.));
            let anchor = MeleeAnchor {
                resource: ResourceId(7),
                bone: 1,
            };
            let previous_anchor = battle.models.as_ref().unwrap().anchor(captor, anchor)?;
            battle.step(BattleInput::default())?;
            let current_anchor = battle.models.as_ref().unwrap().anchor(captor, anchor)?;
            assert!(current_anchor[0] > previous_anchor[0]);
            let actor = &battle.actors[victim.index()];
            assert_eq!(
                actor.position,
                [
                    current_anchor[0],
                    current_anchor[1] - 12.,
                    current_anchor[2]
                ]
            );
            assert_eq!(
                (
                    actor.hit_stop,
                    actor.movement.forward,
                    actor.movement.vertical
                ),
                (4, 6., 16.)
            );
            assert!(
                matches!(battle.runtime[victim.index()].task(), ActorTask::Hurt { remaining } if *remaining == before)
            );
            assert!(battle.is_captured(victim));
            visible(&mut battle, false)?;
            if reflected {
                let actor = &mut battle.actors[victim.index()];
                actor.equipment.reaction_ex.reflect_damage = true;
                actor.equipment.luck = u16::MAX;
            }
            battle.step(BattleInput::default())?;
            let actor = &battle.actors[victim.index()];
            assert_eq!(actor.reaction.combo_hits, 2);
            if reflected {
                assert_eq!(actor.reaction.recoil.kind, crate::RecoilKind::Launched);
                assert_eq!((actor.movement.forward, actor.movement.vertical), (0., 0.));
                assert!(!battle.is_captured(victim));
                assert_eq!(battle.held_victim(captor), None);
                assert_eq!(battle.activity(captor), crate::Activity::Hurt);
                visible(&mut battle, true)?;
                continue;
            }
            assert_eq!(actor.reaction.recoil.kind, crate::RecoilKind::Normal);
            assert_eq!((actor.movement.forward, actor.movement.vertical), (6., 16.));
            let position = actor.position;
            visible(&mut battle, false)?;
            battle.captured_visibility(captor, true);
            visible(&mut battle, true)?;
            battle.captured_visibility(captor, false);
            battle.step(BattleInput::default())?;
            visible(&mut battle, true)?;
            assert!(!battle.is_captured(victim));
            assert_eq!(battle.held_victim(captor), None);
            let release_anchor = battle.models.as_ref().unwrap().anchor(captor, anchor)?;
            assert_ne!(battle.actors[victim.index()].position, position);
            assert_eq!(
                battle.actors[victim.index()].position,
                [
                    release_anchor[0],
                    release_anchor[1] - 12.,
                    release_anchor[2]
                ]
            );
            assert_eq!(battle.actors[victim.index()].movement.vertical, 16.);
            assert_eq!(
                battle.actors[victim.index()].reaction.recoil.kind,
                crate::RecoilKind::Launched
            );
            let action = *battle.runtime[captor.index()].task().action().unwrap().0;
            battle.capture_contact(
                captor,
                victim,
                action,
                MeleeAnchor {
                    resource: ResourceId(7),
                    bone: 1,
                },
            )?;
            battle.captured_visibility(captor, false);
            visible(&mut battle, false)?;
            battle.enter_death(captor, &mut Vec::new());
            visible(&mut battle, true)?;
            assert!(!battle.is_captured(victim));
            assert_eq!(battle.held_victim(captor), None);
        }
        Ok(())
    }
}
