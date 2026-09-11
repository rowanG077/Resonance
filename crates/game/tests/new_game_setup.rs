//! Original setup scenario; all retail dependencies remain in ignored cooked assets.
mod common;
use common::{asset_root, cooked};
use resonance_content::{field::FieldAssets, session::SessionData};
use resonance_events::{
    Actor, EventRuntime, GameWorld, ResourceLibrary, camera::CameraRig, dialogue::ChoiceExit,
    party::Party,
};
use std::{fs, sync::Arc};
use symphonia_script::Program;
use symphonia_script_vm::Memory;

#[test]
#[ignore = "requires locally cooked GQSEAF setup scenario/session data; no devices"]
fn original_setup_initializes_party_and_both_settings_routes_reach_classroom() {
    let root = asset_root();
    let assets: FieldAssets = cooked("fields/new-game-setup.json");
    assets.validate().unwrap();
    let data: SessionData = cooked("game/session-data.json");
    let data = Arc::new(data);
    let program =
        Arc::new(Program::decode(&fs::read(root.join(&assets.script.path)).unwrap()).unwrap());
    let resources = Arc::new(ResourceLibrary {
        messages: cooked(&assets.messages),
        session_data: Some(data.clone()),
        fields: [340].into(),
        ..Default::default()
    });
    for change_settings in [false, true] {
        let mut world = GameWorld::default();
        world.controlled_actor = 1;
        world.actors.insert(1, Actor::new(1, [0.; 3]));
        world.field_camera = Some(CameraRig::default());
        world.party = Some(Party::new(&data, Default::default()).unwrap());
        let mut events =
            EventRuntime::with_state(program.clone(), resources.clone(), world, Memory::default())
                .unwrap();
        let selections = if change_settings {
            &[0, 1, 2, 0, 0][..]
        } else {
            &[1][..]
        };
        let mut selected = 0;
        for _ in 0..2000 {
            if events.world.field_transition.is_some() {
                break;
            }
            for dialogue in events.world.dialogue.values() {
                if !dialogue.operation.progress().ready {
                    dialogue.operation.advance(1).unwrap();
                }
            }
            for (&slot, choice) in &mut events.world.choices {
                if choice.operation.is_pending() {
                    choice.selected_line = selections[selected];
                    selected += 1;
                    choice.finish(ChoiceExit::Confirm).unwrap();
                    events.world.dialogue[&slot]
                        .operation
                        .complete(None)
                        .unwrap();
                }
            }
            events.world.audio_commands.clear();
            events.step().unwrap();
        }
        assert_eq!(selected, selections.len());
        let transition = events
            .world
            .field_transition
            .as_ref()
            .expect("setup did not request the classroom");
        assert_eq!(transition.map, 340);
        assert_eq!(transition.position, [-719., -371., 0.]);
        assert_eq!(transition.heading, 0.);
        let party = events.world.party.as_ref().unwrap();
        assert_eq!(party.gald, 500);
        assert_eq!(
            party.members.iter().map(|m| m.level).collect::<Vec<_>>(),
            [3, 1, 2, 7, 1, 1, 1, 1, 4]
        );
        assert_eq!(party.items.get(&1), Some(&3));
        assert_eq!(party.items.get(&3), Some(&1));
        assert_eq!(party.items.get(&11), Some(&1));
        assert_eq!(party.items.get(&121), Some(&3));
        assert_eq!(party.members[0].equipment[0], 135);
        assert!(party.members[8].techniques.contains(&34));
        assert_eq!(party.settings.preferences.rumble, !change_settings);
        assert_eq!(party.settings.preferences.stereo, !change_settings);
        assert_eq!(
            party.settings.battle_controls[0],
            u8::from(!change_settings)
        );
        let old = transition.operation.clone();
        events.cancel();
        assert!(old.complete(None).is_err());
        assert!(events.world.field_transition.is_none());
    }
}
