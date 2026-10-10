use super::*;
use resonance_battle::{ActorId, Cue, Effects};
use resonance_content::{
    diagnostics::Diagnostics, menu_data::MenuData, prepared::Cache, session::SessionData,
};
use resonance_events::{
    battle::{DefeatPolicy, Setup},
    party::{Party, Poison},
};
use resonance_game::battle::encounter::{Assets, PrepareOptions};

#[test]
#[ignore = "requires current opening encounter and poison effect publications; no devices"]
fn saved_poison_emits_scene_feedback_that_expires() -> Result<()> {
    let root = common::asset_root();
    let mut cache = Cache::default();
    let retained = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let menus: MenuData = retained.json("game/menu-data.json")?;
    let mut data: SessionData = retained.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default())?;
    party.formation = vec![1, 2, 3];
    party.members[0].ailments.poison = Poison::Mild;
    party.settings.battle_controls = [0; 4];
    let assets = Assets::load(
        &root,
        &retained,
        &menus,
        &data,
        &party,
        Setup {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(1),
            arena: 13,
            defeat: DefeatPolicy::GameOver,
            music: None,
        },
        &mut cache,
        || false,
    )?;
    let prepared = assets.prepare(
        &menus,
        PrepareOptions {
            devils_arms_unlocked: false,
            victory_story_flags: [false; 2],
            random_seed: 17,
            map: 332,
            world_music: 0,
            story: 2500,
            story3: false,
            colette_state: 0,
            overlimit_boost: false,
        },
        |request| Ok(Some(request)),
    )?;
    let diagnostics = Diagnostics::new(true);
    let mut effects = Effects::new(
        prepared.effect_banks,
        Some(prepared.poison_effect),
        1,
        diagnostics.clone(),
    )?;
    let mut battle = prepared.core;
    let owner = ActorId::from_index(0)?;
    let pulse = Cue::PoisonPulse { actor: owner };
    let mut emitted = false;
    for _ in 0..1200 {
        let frame = battle.step(BattleInput::default())?;
        effects.advance(&frame, false)?;
        if frame.cues.contains(&pulse) {
            let actor = &frame.actors[0];
            assert!(
                effects
                    .frames()
                    .iter()
                    .any(|particle| particle.owner == owner
                        && particle.resource == prepared.poison_effect.resource
                        && particle.origin
                            == [actor.position[0], actor.body_top(), actor.position[2]]),
                "prepared poison puff did not spawn above its owner"
            );
            emitted = true;
            break;
        }
    }
    assert!(emitted, "saved poison did not produce a pulse");
    let frame = battle.snapshot();
    for _ in 0..120 {
        effects.advance(&frame, false)?;
    }
    assert!(effects.frames().is_empty(), "poison puff did not expire");
    assert!(!diagnostics.has_errors());
    Ok(())
}
