//! Target ornaments anchored to the presented actors.
use super::{ActorMarkers, Batch, BattleFrame, hud_color};
use anyhow::{Context, Result};
use bevy::prelude::Vec3;
use resonance_battle::{ActorAvailability, Control, Side, conditions::Condition};

const BLINK_PERIOD: u16 = 180;
const BLINK_CELL_TICKS: u16 = 4;
const FOLLOW_STEP: f32 = 50.;
const TRAIL_LIMIT: u8 = 4;
const TRAIL_SPACING: f32 = 35.;
const MARKER_SIZE: [f32; 2] = [48., 64.];
const PARALYSIS_CELL_TICKS: u16 = 15;
const PARALYSIS_SIZE: [f32; 2] = [96., 32.];

#[derive(Clone, Copy, Default)]
struct Track {
    position: Option<Vec3>,
    direction: Vec3,
    trails: u8,
    blink: u16,
}

impl Track {
    fn follow(&mut self, anchor: Vec3, elapsed: u32) {
        let Some(position) = &mut self.position else {
            self.position = Some(anchor);
            return;
        };
        self.blink = ((u32::from(self.blink) + elapsed) % u32::from(BLINK_PERIOD)) as u16;
        for _ in 0..elapsed {
            let delta = anchor - *position;
            if delta.length() <= FOLLOW_STEP {
                *position = anchor;
                self.trails = self.trails.saturating_sub(1);
            } else {
                self.direction = delta.normalize();
                *position += self.direction * FOLLOW_STEP;
                self.trails = (self.trails + 1).min(TRAIL_LIMIT);
            }
        }
    }

    fn cell(self) -> f32 {
        if self.blink < BLINK_PERIOD - 2 * BLINK_CELL_TICKS {
            0.
        } else if self.blink < BLINK_PERIOD - BLINK_CELL_TICKS {
            1.
        } else {
            2.
        }
    }
}

#[derive(Default)]
pub(super) struct Markers {
    tracks: [Track; resonance_battle::PARTY_CAPACITY],
}

pub(super) const LABELS: usize = 5;
pub(super) const PARALYSIS: usize = LABELS + 1;
pub(super) const COUNT: usize = PARALYSIS + 1;
const CONTROLLER_COLORS: [[u8; 4]; 4] = [
    [128, 64, 64, 255],
    [64, 64, 128, 255],
    [64, 128, 64, 255],
    [128, 128, 64, 255],
];

fn billboard(
    camera: resonance_battle::CameraPose,
    anchor: [f32; 3],
    size: [f32; 2],
    offset_x: f32,
) -> Option<[f32; 4]> {
    let eye = Vec3::from_array(camera.eye);
    let forward = (Vec3::from_array(camera.focus) - eye).normalize_or_zero();
    let depth = (Vec3::from_array(anchor) - eye).dot(forward);
    if depth <= 0. {
        return None;
    }
    let scale = 240.
        / (depth
            * (resonance_battle::VERTICAL_FOV_DEGREES / 2.)
                .to_radians()
                .tan());
    let [mut x, scene_y] = resonance_battle::project_screen_point(camera, anchor);
    // Scene projection has 448 rows; HUD geometry has 480.
    let y = scene_y * 480. / 448.;
    x += offset_x * scale;
    let [width, height] = size.map(|value| value * scale / 2.);
    Some([x - width, y - height, x + width, y + height])
}

impl Markers {
    pub(super) fn draw(
        &mut self,
        frame: &BattleFrame,
        selection: Option<(resonance_battle::ActorId, resonance_battle::ActorId)>,
        anchors: &[ActorMarkers],
        elapsed: u32,
    ) -> Result<[Batch; COUNT]> {
        let mut batches = std::array::from_fn(|_| Batch::default());
        let Some(camera) = frame.camera else {
            self.tracks = Default::default();
            return Ok(batches);
        };
        let controllers = frame
            .actors
            .iter()
            .filter(|actor| {
                actor.side == Side::Party
                    && actor.availability != ActorAvailability::Absent
                    && actor.control != Control::Auto
            })
            .count();
        let mut active = [false; resonance_battle::PARTY_CAPACITY];
        for (target, actor) in frame.actors.iter().enumerate() {
            if !actor.available()
                || actor.hp <= 0
                || frame
                    .models
                    .iter()
                    .any(|model| model.actor.index() == target && !model.visible)
            {
                continue;
            }
            let Some(markers) = anchors.get(target) else {
                continue;
            };
            if actor.conditions.effective().contains(Condition::Paralysis)
                && let Some(mut head) = markers.head
            {
                let cell = f32::from(markers.effect_tick / PARALYSIS_CELL_TICKS % 2);
                head[1] += 8. + 4. * cell;
                if let Some(rect) = billboard(camera, head, PARALYSIS_SIZE, 0.) {
                    let v = 25. + 16. * cell;
                    batches[PARALYSIS].quad(
                        rect,
                        [256., v, 304., v + 14.],
                        hud_color([128, 128, 128, 255]),
                    );
                }
            }
            if frame.recognized_result.is_some() {
                continue;
            }
            let Some(anchor) = markers.target else {
                continue;
            };
            let owners = || {
                frame
                    .actors
                    .iter()
                    .enumerate()
                    .filter_map(|(index, owner)| {
                        let selected_target = selection
                            .filter(|(selected_owner, _)| selected_owner.index() == index)
                            .map(|(_, target)| target)
                            .or_else(|| frame.targets.get(index).copied().flatten());
                        (owner.side == Side::Party
                            && owner.availability != ActorAvailability::Absent
                            && selected_target.is_some_and(|id| id.index() == target))
                        .then_some((index, owner))
                    })
            };
            let shared = controllers >= 2 && owners().count() >= 2;
            for (index, (actor_index, owner)) in owners().enumerate() {
                let slot = owner.control_slot.index();
                let party_index = frame.actors[..actor_index]
                    .iter()
                    .filter(|actor| actor.side == Side::Party)
                    .count();
                let track = self
                    .tracks
                    .get_mut(party_index)
                    .context("too many battle party actors")?;
                active[party_index] = true;
                track.follow(Vec3::from_array(anchor), elapsed);
                if owner.control != Control::Auto {
                    let origin = track.position.unwrap();
                    for trail in 0..=track.trails {
                        let distance = f32::from(trail);
                        let position = origin - track.direction * TRAIL_SPACING * distance;
                        let width = MARKER_SIZE[0] - 2. * distance;
                        let height = MARKER_SIZE[1] - 3. * distance;
                        let opacity = if trail == 0 { 240 } else { 248 - 40 * trail };
                        let Some(rect) = billboard(
                            camera,
                            (position + Vec3::Y * height / 2.).to_array(),
                            [width, height],
                            0.,
                        ) else {
                            continue;
                        };
                        let color = hud_color([128, 128, 128, opacity]);
                        batches[0].quad(rect, [0., 256., 48., 320.], color);
                        let u = 48. * (1. + track.cell());
                        batches[slot + 1].quad(rect, [u, 256., u + 48., 320.], color);
                    }
                }
                if shared {
                    let Some(rect) = billboard(
                        camera,
                        [anchor[0], anchor[1] + 12. + index as f32 * 24., anchor[2]],
                        [36., 24.],
                        22.5,
                    ) else {
                        continue;
                    };
                    let v = 240. + slot as f32 * 24.;
                    batches[LABELS].quad(
                        rect,
                        [480., v, 512., v + 24.],
                        hud_color(CONTROLLER_COLORS[slot]),
                    );
                }
            }
        }
        for (track, active) in self.tracks.iter_mut().zip(active) {
            if !active {
                *track = Track::default();
            }
        }
        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_ui::battle_ui::fixtures;
    use resonance_battle::{Activity, ActorId, BattleResult};

    #[test]
    fn markers_follow_target_selection_and_dismiss_with_results() -> Result<()> {
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.camera = Some(resonance_battle::CameraPose {
            eye: [0., 150., 2000.],
            focus: [0., 40., 0.],
            radius: 2000.,
            ..frame.camera.unwrap()
        });
        frame.actors = (0..6)
            .map(|index| {
                let mut actor = fixtures::actor(Default::default());
                actor.side = if index < 4 { Side::Party } else { Side::Enemy };
                actor.control = if index < 4 {
                    Control::Manual
                } else {
                    Control::Enemy
                };
                actor.control_slot = resonance_battle::ControllerPort::ALL[index % 4];
                actor.position = [index as f32 * 40. - 100., 0., 0.];
                actor.activity = Activity::Stunned;
                actor
            })
            .collect();
        frame.targets = vec![None; 6];
        frame.targets[..4].fill(Some(ActorId::from_index(4)?));
        let anchors: Vec<_> = frame
            .actors
            .iter()
            .map(|actor| ActorMarkers {
                target: Some([actor.position[0], actor.body_top(), actor.position[2]]),
                ..Default::default()
            })
            .collect();
        let draw = |frame: &BattleFrame, selection, anchors: &[ActorMarkers]| {
            Markers::default().draw(frame, selection, anchors, 0)
        };
        let all = draw(&frame, None, &anchors)?;
        assert!(
            draw(&frame, None, &[])?
                .iter()
                .all(|batch| batch.indices.is_empty()),
            "missing posed anchors do not invent marker positions"
        );
        assert_eq!(all[0].positions.len(), 16, "four target owners");
        assert!(all[1..5].iter().all(|batch| batch.positions.len() == 4));
        assert_eq!(all[LABELS].positions.len(), 16, "shared-target labels");
        assert_eq!(all[1].positions, all[2].positions, "markers overlap");
        frame.actors[1].hp = 0;
        frame.actors[1].availability = ActorAvailability::Dead;
        assert!(
            draw(&frame, None, &anchors)? == all,
            "KO owners retain their target"
        );
        frame.targets[0] = Some(ActorId::from_index(5)?);
        let switched = draw(&frame, None, &anchors)?;
        assert!(switched[1] != all[1]);
        let mut moving = Markers::default();
        let mut earlier = frame.clone();
        earlier.targets[0] = Some(ActorId::from_index(4)?);
        let mut separated = anchors.clone();
        separated[5].target.as_mut().unwrap()[0] += 600.;
        moving.draw(&earlier, None, &separated, 0)?;
        let trail = moving.draw(&frame, None, &separated, 1)?;
        assert!(
            trail[1].positions.len() > 4,
            "target changes leave trailing ornaments"
        );
        assert!(
            moving.draw(&frame, None, &separated, 0)? == trail,
            "drawing does not age motion"
        );
        let blink = moving.draw(&frame, None, &separated, 171)?;
        assert_eq!(
            blink[1].positions.len(),
            4,
            "trails settle without retaining copies"
        );
        assert!(blink[1].uv != trail[1].uv[..4]);
        let closed = moving.draw(&frame, None, &separated, 4)?;
        assert!(closed[1].uv != blink[1].uv);
        let open = moving.draw(&frame, None, &separated, 4)?;
        assert!(
            open[1].uv == all[1].uv,
            "the blink returns to its ordinary cell"
        );
        let selecting = draw(
            &frame,
            Some((ActorId::from_index(0)?, ActorId::from_index(4)?)),
            &anchors,
        )?;
        assert!(
            selecting == all,
            "the command target retains its controller marker"
        );
        frame.target_selector = Some(ActorId::from_index(0)?);
        assert!(
            draw(&frame, None, &anchors)? == switched,
            "inspection retains all target owners"
        );
        frame.actors[0].control = Control::Auto;
        assert!(draw(&frame, None, &anchors)?[1].indices.is_empty());
        frame.actors[1..4]
            .iter_mut()
            .for_each(|actor| actor.control = Control::Auto);
        frame.actors[0].control = Control::Manual;
        assert!(
            draw(&frame, None, &anchors)?[LABELS].indices.is_empty(),
            "one controller does not need player labels"
        );
        frame.actors[5].hp = 0;
        assert!(draw(&frame, None, &anchors)?[1].indices.is_empty());
        frame.targets.fill(None);
        frame.target_selector = None;
        assert!(
            draw(&frame, None, &anchors)?
                .iter()
                .all(|batch| batch.indices.is_empty()),
            "stun is represented by scene birds, without a lightning HUD marker"
        );
        frame.recognized_result = Some(BattleResult::Victory);
        assert!(
            draw(&frame, None, &anchors)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        Ok(())
    }
}
