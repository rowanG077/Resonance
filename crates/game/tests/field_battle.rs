//! Real field scripts exercise the handoff here. Explicit result callbacks stand
//! in for the battle service; this test does not establish combat or fidelity.
mod common;
use anyhow::{Result, ensure};
use resonance_content::{
    field::FieldAssets, menu_data::MenuData, prepared::Files, session::SessionData,
};
use resonance_events::{PersistentState, battle, party::Party};
use resonance_game::field::{FieldEntry, FieldInput, FieldSession};
use std::sync::Arc;

#[test]
#[ignore = "requires the current cooked school grounds; no devices"]
fn original_opening_event_requests_two_battles_and_keeps_its_field_session() -> Result<()> {
    let files = Files::load(
        &common::asset_root(),
        &["fields/map-332.preload.json"],
        &mut Default::default(),
        || false,
    )?;
    let assets: FieldAssets = files.json("fields/map-332.json")?;
    let menus: Arc<MenuData> = Arc::new(files.json("game/menu-data.json")?);
    let mut data: SessionData = files.json("game/session-data.json")?;
    data.ex_skills = Some(Arc::new(menus.ex_skills.clone()));
    let data = Arc::new(data);
    let mut party = Party::new(&data, Default::default())?;
    party.formation = vec![1, 2, 3];
    let mut persistent = PersistentState {
        party: Some(party),
        event_flags: [520].into(),
        ..Default::default()
    };
    persistent
        .memory
        .write(0x40, symphonia_script::Width::S32, 2500)?;
    let mut field = FieldSession::enter(
        &files.read(&assets.script.path)?,
        files.json(&assets.messages)?,
        &assets,
        FieldEntry {
            data: Some(data),
            menu_data: Some(menus),
            persistent,
            text: Arc::new(files.json("game/text.json")?),
            available_fields: [330, 331, 332, 340].into(),
            position: [1968., 1005., 0.],
            heading: 276.,
            ..Default::default()
        },
    )?;
    for _ in 0..1000 {
        if field.player_has_control() {
            break;
        }
        field.step(FieldInput::default())?;
        field.events.world.audio_commands.clear();
    }
    ensure!(field.player_has_control(), "field did not initialize");
    ensure!(
        field.events.trigger(2002, false)?,
        "opening trigger did not start"
    );
    let mut completed = Vec::new();
    for tick in 0..30000 {
        if let Some(request) = field.events.world.battle_request.take() {
            assert_eq!(request.setup.encounter, completed.len() as u16 + 1);
            assert_eq!(request.setup.arena, 13);
            assert_eq!(request.setup.defeat, battle::DefeatPolicy::GameOver);
            assert_eq!(request.setup.music, None);
            let before = field.events.save_progress()?;
            let before_party = serde_json::to_value(&before.party)?;
            let effect_tick = field.effect_clock.tick();
            let play_time = field.play_time;
            let actor_instances: Vec<_> = field
                .events
                .world
                .actors
                .iter()
                .map(|(&id, actor)| (id, actor.instance, actor.position))
                .collect();
            for _ in 0..60 {
                field.step(FieldInput {
                    interact: true,
                    menu: true,
                    direction: [1., 1.],
                    ..Default::default()
                })?;
            }
            assert_eq!(field.events.tick(), before.tick);
            assert_eq!(field.effect_clock.tick(), effect_tick);
            assert_eq!(field.play_time, play_time);
            assert_eq!(
                serde_json::to_value(field.events.world.party.as_ref().unwrap())?,
                before_party
            );
            assert_eq!(field.events.world.random_state, before.random_state);
            assert_eq!(
                field
                    .events
                    .world
                    .actors
                    .iter()
                    .map(|(&id, actor)| (id, actor.instance, actor.position))
                    .collect::<Vec<_>>(),
                actor_instances
            );
            assert!(field.checkpoint().is_err());
            assert!(!field.player_has_control());
            request
                .complete(battle::Outcome::Victory)
                .map_err(anyhow::Error::msg)?;
            assert!(request.complete(battle::Outcome::Victory).is_err());
            assert!(completed.iter().all(|&id| id != request.id()));
            completed.push(request.id());
        }
        if completed.len() == 2 && field.player_has_control() {
            assert_eq!(field.map_id, 332);
            assert_eq!(field.story_progress()?, 3000);
            assert!(field.events.world.battle_request.is_none());
            return Ok(());
        }
        let advance = tick % 10 == 0
            && field
                .dialogue
                .values()
                .any(|page| !page.closed && !page.persistent && page.fully_revealed());
        field.step(FieldInput {
            interact: advance,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        field.events.world.audio_commands.clear();
    }
    anyhow::bail!(
        "opening field event stalled after {} battles",
        completed.len()
    )
}
