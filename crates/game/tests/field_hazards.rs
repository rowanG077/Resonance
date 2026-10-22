//! Room scripts own hazard recovery; exercise them through ordinary field updates.
mod common;

use common::{asset_root, cooked};
use resonance_content::{field::FieldAssets, session::SessionData};
use resonance_events::{PersistentState, input::Button, party::Party};
use resonance_game::field::{FieldEntry, FieldInput, FieldSession};
use std::{fs, sync::Arc};

fn room(map: u32, position: [f32; 3]) -> FieldSession {
    let root = asset_root();
    let assets: FieldAssets = cooked(format!("fields/map-{map}.json"));
    assert!(
        resonance_game::field::navigation::WalkMesh::new(&assets.ground)
            .unwrap()
            .height(position, 32.)
            .is_some_and(|height| (height - position[2]).abs() <= 32.),
        "field {map}: fixture must start on walkable ground"
    );
    let mut data: SessionData = cooked("game/session-data.json");
    data.rules = Some(Arc::new(cooked("game/menu-data.json")));
    let data = Arc::new(data);
    let mut entry = FieldEntry {
        data: Some(data.clone()),
        persistent: PersistentState {
            party: Some(Party::new(&data, Default::default()).unwrap()),
            ..Default::default()
        },
        position,
        available_fields: (251..=266).collect(),
        attachments: resonance_game::field::attachments::prepare(&assets, |path| {
            Ok(fs::read(root.join(path))?.into())
        })
        .unwrap(),
        ..Default::default()
    };
    entry
        .persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 12_203_000)
        .unwrap();
    let mut session = FieldSession::enter(
        &fs::read(root.join(&assets.script)).unwrap(),
        cooked(&assets.messages),
        &assets,
        entry,
    )
    .unwrap();
    for _ in 0..60 {
        step(&mut session, FieldInput::default());
    }
    assert!(session.player_has_control());
    session
}

fn step(session: &mut FieldSession, input: FieldInput) {
    session.step(input).unwrap();
    session.events.world.audio_commands.clear();
}

fn direction(session: &FieldSession, [x, y]: [f32; 2]) -> [f32; 2] {
    let camera = session.events.world.field_camera.as_ref().unwrap();
    let angle =
        -(camera.target[0] - camera.position[0]).atan2(camera.target[1] - camera.position[1]);
    [
        angle.cos() * x + angle.sin() * y,
        -angle.sin() * x + angle.cos() * y,
    ]
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn renegade_lasers_present_both_directions_every_update() {
    for (map, position, count, reversed) in [
        (257, [0., 575., 0.], 2, false),
        (265, [-600., 2000., 1.], 5, true),
    ] {
        let mut session = room(map, position);
        for _ in 0..10 {
            step(&mut session, FieldInput::default());
            let world = &session.events.world;
            let beams: Vec<_> = world
                .billboards
                .values()
                .filter(|effect| effect.recipe == 70 && effect.born == world.tick)
                .collect();
            assert_eq!(
                beams.len(),
                count,
                "field {map}: all laser births must reach presentation"
            );
            assert!(beams.iter().all(|effect| effect.alpha(world.tick) > 0.));
            assert_eq!(
                beams
                    .iter()
                    .any(|effect| effect.size.iter().any(|size| *size < 0.)),
                reversed,
                "field {map}: beam direction coverage"
            );
        }
    }
}

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn renegade_laser_hit_resets_the_grabbed_block_and_returns_control() {
    for keep_accept_held in [false, true] {
        let mut session = room(257, [0., 575., 0.]);
        let block = 3102;
        let instance = session.events.world.actors[&block].instance;
        step(
            &mut session,
            FieldInput {
                held_buttons: [Button::Accept].into(),
                pressed_buttons: [Button::Accept].into(),
                ..Default::default()
            },
        );
        assert_eq!(session.events.world.grabbed_block, Some(block));
        let mut hit = false;
        for _ in 0..80 {
            let direction = direction(&session, [0., -1.]);
            step(
                &mut session,
                FieldInput {
                    held_buttons: [Button::Accept].into(),
                    direction,
                    ..Default::default()
                },
            );
            if session.events.world.mapped_input_disabled {
                hit = true;
                break;
            }
        }
        assert!(
            hit,
            "pushing across the beam must trigger the room's hit script"
        );
        for _ in 0..400 {
            step(
                &mut session,
                FieldInput {
                    held_buttons: if keep_accept_held {
                        [Button::Accept].into()
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            );
        }
        let world = &session.events.world;
        let reset = &world.actors[&block];
        assert_ne!(reset.instance, instance);
        assert!(reset.visible && reset.pushable);
        assert_eq!(reset.opacity, 255);
        assert!((reset.position[2] - 1.).abs() < 0.01);
        assert_eq!(reset.position[..2], [0., 900.]);
        assert_eq!(world.grabbed_block, None);
        assert!(session.player_has_control());
        let player = world.actors[&world.controlled_actor].position;
        assert!(
            player[1] > 510.,
            "hit recovery must clear the beam: {player:?}"
        );
        let checkpoint = session.checkpoint().expect("recovered room can be saved");
        assert_eq!(checkpoint.map_id, 257);

        // Walk back to the replacement and move it using the same controls.
        for _ in 0..100 {
            let world = &session.events.world;
            if world.actors[&world.controlled_actor].position[1] >= 775. {
                break;
            }
            let direction = direction(&session, [0., 1.]);
            step(
                &mut session,
                FieldInput {
                    direction,
                    ..Default::default()
                },
            );
        }
        step(
            &mut session,
            FieldInput {
                held_buttons: [Button::Accept].into(),
                pressed_buttons: [Button::Accept].into(),
                ..Default::default()
            },
        );
        assert_eq!(session.events.world.grabbed_block, Some(block));
        for _ in 0..50 {
            let direction = direction(&session, [0., 1.]);
            step(
                &mut session,
                FieldInput {
                    held_buttons: [Button::Accept].into(),
                    direction,
                    ..Default::default()
                },
            );
        }
        assert_eq!(
            session.events.world.actors[&block].position[..2],
            [0., 1050.]
        );
        step(&mut session, FieldInput::default());
        assert!(session.player_has_control());
    }
}
