//! Target owners and stun labels projected from the current actor geometry.
use super::{Batch, BattleFrame, BitmapFont, text};
use anyhow::Result;
use resonance_battle::{Activity, Control, Side};

const OWNER_COLORS: [[u8; 4]; 4] = [
    [96, 115, 128, 255],
    [128, 92, 92, 255],
    [96, 128, 100, 255],
    [128, 115, 80, 255],
];

pub(super) fn draw(
    frame: &BattleFrame,
    font: &BitmapFont,
    panels: &mut Batch,
    text: &mut Batch,
) -> Result<()> {
    let Some(camera) = frame.camera.filter(|_| frame.recognized_result.is_none()) else {
        return Ok(());
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
        let owners: Vec<_> = frame
            .actors
            .iter()
            .enumerate()
            .filter_map(|(index, owner)| {
                (owner.side == Side::Party
                    && owner.available()
                    && owner.hp > 0
                    && (owner.control != Control::Auto
                        || frame.target_selector.is_some_and(|id| id.index() == index))
                    && frame
                        .targets
                        .get(index)
                        .copied()
                        .flatten()
                        .is_some_and(|id| id.index() == target))
                .then_some(owner.control_slot)
            })
            .collect();
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
        let [x, y] = resonance_battle::project_screen_point(camera, anchor);
        if !(0. ..=640.).contains(&x) || !(0. ..=448.).contains(&y) {
            continue;
        }
        for (index, &slot) in owners.iter().enumerate() {
            let left = x + (index as f32 - owners.len() as f32 / 2.) * 28.;
            label(
                panels,
                text,
                font,
                &format!("P{}", slot + 1),
                [left, y - 22., 24.],
                OWNER_COLORS[usize::from(slot)],
            )?;
        }
        if stunned {
            label(
                panels,
                text,
                font,
                "STUN",
                [x - 24., y - if owners.is_empty() { 22. } else { 42. }, 48.],
                [128, 115, 80, 255],
            )?;
        }
    }
    Ok(())
}

fn label(
    panels: &mut Batch,
    text: &mut Batch,
    font: &BitmapFont,
    value: &str,
    [x, y, width]: [f32; 3],
    color: [u8; 4],
) -> Result<()> {
    panels.quad(
        [x, y, x + width, y + 16.],
        [0.5; 4],
        [0.04, 0.05, 0.08, 0.9],
    );
    text::fit(text, font, value, [x + 3., y + 2., width - 6., 12.], color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_ui::battle_ui::{fixtures, selector};
    use resonance_battle::{ActorId, BattleResult};

    #[test]
    #[ignore = "requires current dialogue font; CPU marker drawing only"]
    fn full_roster_labels_follow_selection_and_dismiss_with_results() -> Result<()> {
        let font = fixtures::dialogue_font()?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.camera = Some(resonance_battle::CameraPose {
            eye: [0., 150., 2000.],
            focus: [0., 40., 0.],
            radius: 2000.,
            ..frame.camera.unwrap()
        });
        frame.actors = (0..12)
            .map(|index| {
                let mut actor = crate::test_support::actor(
                    if index < 4 { Side::Party } else { Side::Enemy },
                    100,
                    20,
                );
                actor.control = if index < 4 {
                    Control::Manual
                } else {
                    Control::Enemy
                };
                actor.control_slot = (index % 4) as u8;
                actor.position = [
                    (index % 4) as f32 * 60. - 90.,
                    40.,
                    (index / 4) as f32 * 40.,
                ];
                resonance_battle::ActorFrame {
                    state: actor,
                    activity: Activity::Stunned,
                }
            })
            .collect();
        frame.targets = vec![None; frame.actors.len()];
        frame.targets[..4].fill(Some(ActorId::from_index(4)?));
        let render = |frame: &BattleFrame| -> Result<[Batch; 2]> {
            let mut panels = Batch::default();
            let mut text = Batch::default();
            draw(frame, &font, &mut panels, &mut text)?;
            Ok([panels, text])
        };
        let all = render(&frame)?;
        assert_eq!(
            all[0].positions.len(),
            16 * 4,
            "four owners and twelve stun labels"
        );
        assert!(!all[1].indices.is_empty());
        frame.actors[11].activity = Activity::Idle;
        let recovered = render(&frame)?;
        assert_eq!(recovered[0].positions.len(), all[0].positions.len() - 4);
        assert!(recovered[1].indices.len() < all[1].indices.len());
        frame.actors[11].activity = Activity::Stunned;
        assert!(
            render(&frame)? == all,
            "stun labels need no body or particle artwork"
        );
        frame.targets[0] = Some(ActorId::from_index(5)?);
        let switched = render(&frame)?;
        assert!(switched[0] != all[0], "target changes draw immediately");
        assert_eq!(switched[0].positions.len(), all[0].positions.len());
        frame.actors[0].control = Control::Auto;
        let automatic = render(&frame)?;
        assert!(automatic[0].positions.len() < switched[0].positions.len());
        frame.target_selector = Some(ActorId::from_index(0)?);
        assert!(
            render(&frame)? == switched,
            "an active selector shows its owner"
        );
        frame.actors[4].hp = 0;
        assert!(render(&frame)?[0].positions.len() < switched[0].positions.len());
        let outline = selector::draw(&frame, None, &font, true)?;
        assert!(!outline[0].indices.is_empty() && outline[1].indices.is_empty());
        frame.scanned_enemies = 1 << 5;
        frame.actors[5].hp = i32::MAX;
        frame.actors[5].equipment.max_hp = i32::MAX;
        frame.actors[5].tp = u16::MAX;
        frame.actors[5].equipment.max_tp = u16::MAX;
        for x in [-4000., 0., 4000.] {
            frame.actors[5].position[0] = x;
            let scanned = selector::draw(&frame, None, &font, true)?;
            assert!(scanned[0].positions.len() > outline[0].positions.len());
            assert!(!scanned[1].indices.is_empty());
            assert!(
                scanned
                    .iter()
                    .flat_map(|batch| &batch.positions)
                    .all(|p| (-312. ..=312.).contains(&p[0]) && (-140. ..=232.).contains(&p[1]))
            );
            assert!(selector::draw(&frame, None, &font, true)? == scanned);
        }
        frame.target_selector = None;
        assert!(
            selector::draw(&frame, None, &font, true)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        let command = resonance_game::battle::command::Frame {
            actor: ActorId::from_index(0)?,
            selected: resonance_game::battle::command::Command::Items,
            enabled: u8::MAX,
            controller: 0,
            connected: [false; 4],
            escape_requested: false,
            view: resonance_game::battle::command::View::Enemy {
                target: ActorId::from_index(5)?,
            },
        };
        assert!(
            !selector::draw(&frame, Some(&command), &font, true)?[1]
                .indices
                .is_empty()
        );
        let during_full_scan = selector::draw(&frame, Some(&command), &font, false)?;
        assert!(during_full_scan[1].indices.is_empty());
        assert_eq!(
            during_full_scan[0].positions.len(),
            outline[0].positions.len()
        );
        frame.target_selector = Some(ActorId::from_index(0)?);
        let strip = resonance_game::battle::command::Frame {
            view: resonance_game::battle::command::View::Strip,
            ..command.clone()
        };
        assert!(
            selector::draw(&frame, Some(&strip), &font, true)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        frame.actors[5].hp = 0;
        frame.actors[5].availability = resonance_battle::ActorAvailability::Dead;
        assert!(
            !selector::draw(&frame, Some(&command), &font, true)?[0]
                .indices
                .is_empty(),
            "the command owns eligibility, including scan-selectable inactive enemies"
        );
        frame.recognized_result = Some(BattleResult::Victory);
        assert!(
            selector::draw(&frame, Some(&command), &font, true)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        assert!(render(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        frame.recognized_result = None;
        frame.camera = None;
        assert!(
            selector::draw(&frame, Some(&command), &font, true)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        assert!(render(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        Ok(())
    }
}
