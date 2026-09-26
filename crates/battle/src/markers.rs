//! Original world-space target pointers and stun captions. Their clocks belong
//! to fixed battle visits; repeated presentation snapshots never advance them.
use crate::{Actor, ActorId, Battle, BattlePhase, Control, Side};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetMarkerFrame {
    pub owner: ActorId,
    pub target: ActorId,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub trail: u8,
    pub phase: u16,
    pub control_slot: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StunMarkerFrame {
    pub actor: ActorId,
    pub position: [f32; 3],
    pub phase: u16,
}

#[derive(Debug, Clone)]
pub(crate) struct TargetMarker {
    position: [f32; 3],
    direction: [f32; 3],
    trail: u8,
    phase: u16,
}

/// 1FEB4 initialization also applies to the no-bone branch of 1FF48. The
/// profile center is scaled twice in separate operations, without yaw rotation.
pub(crate) fn initial_position(actor: &Actor) -> [f32; 3] {
    std::array::from_fn(|i| {
        actor.position[i] + (actor.body.center_offset[i] * actor.body.scale) * 2.
    })
}

impl TargetMarker {
    pub(crate) fn new(target: &Actor) -> Self {
        Self {
            position: initial_position(target),
            direction: [0.; 3],
            trail: 0,
            phase: 0,
        }
    }

    fn follow(&mut self, position: [f32; 3]) {
        let difference = std::array::from_fn(|i| position[i] - self.position[i]);
        if crate::distance::length(difference) <= 50. {
            self.position = position;
            self.trail = self.trail.saturating_sub(1);
        } else {
            self.direction = crate::distance::normalize(difference);
            self.position = std::array::from_fn(|i| self.position[i] + self.direction[i] * 50.);
            self.trail = (self.trail + 1).min(4);
        }
    }
}

impl Battle {
    /// 1C40 draws pointers (145C/51038) before HUD smoothing (70AE4).
    pub(crate) fn advance_target_markers(&mut self) {
        // 51038 advances the phase before drawing the preceding position,
        // direction and trail. Later selector/actor visits must not replace
        // this draw's target or visibility either.
        self.advance_marker_phases();
        self.drawn_target_markers = self.target_marker_frames();
        for index in 0..self.actors.len() {
            if self.actors[index].side != Side::Party {
                continue;
            }
            let Some(target) = self.target(ActorId(index as u8)) else {
                continue;
            };
            let actor = &self.actors[target.index()];
            let position = self.models[target.index()].as_ref().map_or_else(
                || initial_position(actor),
                |model| model.target_marker_position(actor),
            );
            self.target_markers[index].follow(position);
        }
    }

    /// 51038 advances every party owner's phase when its active enemy target is
    /// visited, including Auto owners whose pointer itself is not drawn.
    fn advance_marker_phases(&mut self) {
        if self.phase() != BattlePhase::Combat {
            return;
        }
        for index in 0..self.actors.len() {
            if self.actors[index].side != Side::Party {
                continue;
            }
            if let Some(target) = self.target(ActorId(index as u8))
                && self.actors[target.index()].side == Side::Enemy
                && self.actors[target.index()].available()
            {
                let marker = &mut self.target_markers[index];
                marker.phase = (marker.phase + 1) % 180;
            }
        }
    }

    pub(crate) fn target_marker_frames(&self) -> Vec<TargetMarkerFrame> {
        if self.phase() != BattlePhase::Combat {
            return Vec::new();
        }
        // 145C visits enemy roster order, and 51038 then visits party owners.
        let mut frames = Vec::new();
        for (target, actor) in self.actors.iter().enumerate() {
            if actor.side != Side::Enemy || !actor.available() {
                continue;
            }
            for (owner, actor) in self.actors.iter().enumerate() {
                if actor.side != Side::Party
                    || actor.control == Control::Auto
                    || self.target(ActorId(owner as u8)) != Some(ActorId(target as u8))
                {
                    continue;
                }
                let marker = &self.target_markers[owner];
                frames.push(TargetMarkerFrame {
                    owner: ActorId(owner as u8),
                    target: ActorId(target as u8),
                    position: marker.position,
                    direction: marker.direction,
                    trail: marker.trail,
                    phase: marker.phase,
                    control_slot: actor.hud.control_slot,
                });
            }
        }
        frames
    }

    pub(crate) fn stun_marker_frames(&self) -> Vec<StunMarkerFrame> {
        if !self.actors_visible {
            return Vec::new();
        }
        self.models
            .iter()
            .flatten()
            .filter_map(|model| {
                let actor = &self.actors[model.shown.actor.index()];
                if actor.activity != crate::Activity::Stunned {
                    return None;
                }
                let binding = model.definition.stun.as_ref()?;
                let mut visible = true;
                let mut tint = actor.body.tint;
                self.death_appearance(model.shown.actor, &mut visible, &mut tint);
                if !visible {
                    return None;
                }
                let origin = model.bone_position(binding.head);
                let offset = crate::geometry::rotate(
                    binding.offset.map(|v| v * actor.body.scale),
                    actor.heading,
                );
                Some(StunMarkerFrame {
                    actor: model.shown.actor,
                    position: std::array::from_fn(|i| origin[i] + offset[i]),
                    phase: actor.hud.phase,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battle(entry: bool) -> Battle {
        let mut party = crate::tests::actor(Side::Party);
        party.position[0] = -300.;
        let prepared = crate::tests::prepared(
            "pub task run() { battle::finish(); }",
            vec![party, crate::tests::actor(Side::Enemy)],
            0,
        );
        let mut battle = Battle::new(prepared);
        if entry {
            let mut camera = crate::camera::Camera::new(
                crate::CameraDefinition {
                    leader: ActorId(0),
                    target: ActorId(1),
                    stage_pitch: -1.,
                    adaptive: true,
                    initial: crate::CameraPose {
                        eye: [0., 120., 1750.],
                        focus: [0., 120., 0.],
                        pitch: 0.,
                        yaw: 85.,
                        radius: 4200.,
                    },
                },
                &battle.actors,
            )
            .unwrap();
            camera
                .initialize_entry(
                    &battle.actors,
                    crate::EntryCamera {
                        initial_yaw: 85.,
                        radius: 4200.,
                        focus_x: 700.,
                        focus_speed_scale: 1. / 59.,
                    },
                )
                .unwrap();
            battle.camera = Some(camera);
        }
        battle
    }

    #[test]
    fn drawing_keeps_previous_follow_sample_in_combat_and_entry() {
        for entry in [false, true] {
            let mut battle = battle(entry);
            let initial = battle.snapshot().target_markers[0];
            assert_eq!(
                (initial.position, initial.trail, initial.phase),
                ([0.; 3], 0, 0)
            );
            battle.actors[1].position = [300., 0., 0.];

            let first = battle.step(crate::BattleInput::default()).unwrap();
            assert_eq!(
                first.target_markers,
                [TargetMarkerFrame {
                    phase: 1,
                    ..initial
                }]
            );
            let followed = battle.target_markers[0].clone();
            assert_eq!(followed.position, [f32::from_bits(0x4247ffff), 0., 0.]);
            assert_eq!(followed.trail, 1);
            assert_eq!(battle.snapshot().target_markers, first.target_markers);
            assert_eq!(battle.snapshot().target_markers, first.target_markers);

            let second = battle.step(crate::BattleInput::default()).unwrap();
            assert_eq!(
                second.target_markers,
                [TargetMarkerFrame {
                    position: followed.position,
                    direction: followed.direction,
                    trail: followed.trail,
                    phase: 2,
                    ..initial
                }]
            );
            assert_eq!(battle.target_markers[0].trail, 2);
            assert_ne!(battle.target_markers[0].position, followed.position);
            assert_eq!(battle.entry_pending(), entry);
        }
    }

    #[test]
    fn menu_pause_holds_both_drawn_and_next_marker_samples() {
        for entry in [false, true] {
            let mut battle = battle(entry);
            battle.actors[1].position = [300., 0., 0.];
            let before = battle.step(crate::BattleInput::default()).unwrap();
            let next = battle.target_marker_frames();
            for _ in 0..2 {
                let held = battle
                    .step(crate::BattleInput {
                        menu_open: true,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(held.target_markers, before.target_markers);
                assert_eq!(battle.target_marker_frames(), next);
                assert_eq!(battle.snapshot().target_markers, before.target_markers);
            }
            let resumed = battle.step(crate::BattleInput::default()).unwrap();
            assert_eq!(
                resumed.target_markers,
                [TargetMarkerFrame {
                    phase: 2,
                    ..next[0]
                }]
            );
            assert_eq!(battle.entry_pending(), entry);
        }
    }

    #[test]
    fn target_switch_smooths_fifty_units_and_keeps_four_trailing_samples() {
        let mut marker = TargetMarker::new(&crate::tests::actor(Side::Enemy));
        // Original SDK reciprocal-square-root refinement rounds the first
        // normalized X to0x3f7fffff, so the first50-unit step is0x4247ffff.
        let positions = [0x4247ffff, 0x42c80000, 0x43160000, 0x43480000, 0x437a0000];
        for (index, bits) in positions.into_iter().enumerate() {
            marker.follow([300., 0., 0.]);
            assert_eq!(marker.position, [f32::from_bits(bits), 0., 0.]);
            assert_eq!(marker.trail, (index as u8 + 1).min(4));
        }
        marker.follow([300., 0., 0.]);
        assert_eq!(marker.position, [300., 0., 0.]);
        assert_eq!(marker.trail, 3);
        marker.follow([300., 0., 0.]);
        assert_eq!(marker.trail, 2);
    }
}
