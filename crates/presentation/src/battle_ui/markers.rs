//! Target ornaments and stun artwork anchored to the presented actors.
use super::{Batch, BattleFrame};
use anyhow::{Context, Result};
use resonance_battle::{Activity, Control, Side};

pub(super) const COUNT: usize = 6;
const STUN: usize = COUNT - 1;
const TARGET_SIZE: [f32; 2] = [36., 48.];
const STUN_FRAME_TICKS: u64 = 15;

pub(super) fn draw(
    frame: &BattleFrame,
    selection: Option<(resonance_battle::ActorId, resonance_battle::ActorId)>,
) -> Result<[Batch; COUNT]> {
    let mut batches = std::array::from_fn(|_| Batch::default());
    let Some(camera) = frame.camera.filter(|_| frame.recognized_result.is_none()) else {
        return Ok(batches);
    };
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
        let owners = frame
            .actors
            .iter()
            .enumerate()
            .filter_map(|(index, owner)| {
                (owner.side == Side::Party
                    && owner.available()
                    && owner.hp > 0
                    && (owner.control != Control::Auto
                        || frame.target_selector.is_some_and(|id| id.index() == index)
                        || selection.is_some_and(|(owner, _)| owner.index() == index))
                    && selection
                        .filter(|(owner, _)| owner.index() == index)
                        .map(|(_, target)| target)
                        .or_else(|| frame.targets.get(index).copied().flatten())
                        .is_some_and(|id| id.index() == target))
                .then_some(usize::from(owner.control_slot))
            })
            .collect::<Vec<_>>();
        let stunned = actor.activity == Activity::Stunned;
        if owners.is_empty() && !stunned {
            continue;
        }
        let anchor = [actor.position[0], actor.body_top(), actor.position[2]];
        let forward: f32 = (0..3)
            .map(|axis| (anchor[axis] - camera.eye[axis]) * (camera.focus[axis] - camera.eye[axis]))
            .sum();
        if forward <= 0. {
            continue;
        }
        let [x, scene_y] = resonance_battle::project_screen_point(camera, anchor);
        // The battle scene's 448 rows expand to the HUD's 480-row display.
        let y = scene_y * 480. / 448.;
        if !(0. ..=640.).contains(&x) || !(0. ..=480.).contains(&y) {
            continue;
        }
        for (index, &slot) in owners.iter().enumerate() {
            let left = x + (index as f32 - owners.len() as f32 / 2.) * TARGET_SIZE[0];
            let rect = [left, y - 8. - TARGET_SIZE[1], left + TARGET_SIZE[0], y - 8.];
            batches[0].quad(rect, [0., 256., 48., 320.], [1.; 4]);
            batches
                .get_mut(slot + 1)
                .filter(|_| slot < 4)
                .context("invalid battle controller slot")?
                .quad(rect, [48., 256., 96., 320.], [1.; 4]);
        }
        if stunned {
            let v = 25. + ((frame.update / STUN_FRAME_TICKS) % 2) as f32 * 16.;
            batches[STUN].quad(
                [x - 24., y - 22., x + 24., y - 8.],
                [256., v, 304., v + 14.],
                [1.; 4],
            );
        }
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_ui::battle_ui::fixtures;
    use resonance_battle::{ActorId, BattleResult};

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
                actor.control_slot = (index % 4) as u8;
                actor.position = [index as f32 * 40. - 100., 0., 0.];
                actor.activity = Activity::Stunned;
                actor
            })
            .collect();
        frame.targets = vec![None; 6];
        frame.targets[..4].fill(Some(ActorId::from_index(4)?));
        let all = draw(&frame, None)?;
        assert_eq!(all[0].positions.len(), 16, "four target owners");
        assert!(all[1..5].iter().all(|batch| batch.positions.len() == 4));
        assert_eq!(all[STUN].positions.len(), 24, "six stunned actors");
        frame.targets[0] = Some(ActorId::from_index(5)?);
        let switched = draw(&frame, None)?;
        assert!(switched[1] != all[1]);
        assert_eq!(
            draw(
                &frame,
                Some((ActorId::from_index(0)?, ActorId::from_index(4)?))
            )?[1]
                .positions,
            all[1].positions
        );
        frame.actors[0].control = Control::Auto;
        assert!(draw(&frame, None)?[1].indices.is_empty());
        frame.target_selector = Some(ActorId::from_index(0)?);
        assert!(draw(&frame, None)?[1] == switched[1]);
        frame.actors[5].hp = 0;
        assert!(draw(&frame, None)?[1].indices.is_empty());
        frame.recognized_result = Some(BattleResult::Victory);
        assert!(
            draw(&frame, None)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        Ok(())
    }
}
