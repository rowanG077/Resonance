use super::*;
use resonance_battle::{ActorId, Cue, Effects};
use resonance_content::{
    diagnostics::Diagnostics, menu_data::MenuData, prepared::Cache, session::SessionData,
};
use resonance_events::{
    battle::DefeatPolicy,
    party::{Party, Poison},
};
use resonance_game::battle::encounter::Assets;

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
    let (_, prepared) = Assets::load(
        &root,
        &retained,
        &menus,
        &data,
        &party,
        opening_setup(1, DefeatPolicy::GameOver),
        opening_options(17),
        &mut cache,
        || false,
    )?;

    let diagnostics = Diagnostics::new(true);
    let mut effects = Effects::new(
        prepared.effect_banks,
        Some(prepared.poison_effect),
        1,
        diagnostics.clone(),
    )?
    .with_head_anchors(prepared.head_anchors);
    let mut battle = prepared.core;
    let owner = ActorId::from_index(0)?;
    let pulse = Cue::PoisonPulse { actor: owner };
    let mut pulsed = false;
    let mut emitted = false;
    for _ in 0..1200 {
        let mut frame = battle.step(BattleInput::default())?;
        let clock = frame.clock;
        battle.advance_models(&mut frame, clock, true)?;
        effects.advance(&frame, false)?;
        pulsed |= frame.cues.contains(&pulse);
        if pulsed {
            let actor = &frame.actors[0];
            emitted = effects.frames().iter().any(|particle| {
                particle.owner == owner
                    && particle.resource == prepared.poison_effect.resource
                    && particle.origin[1] > actor.position[1]
            });
            if emitted {
                break;
            }
        }
    }
    assert!(
        pulsed && emitted,
        "saved poison did not produce visible feedback"
    );
    let mut frame = battle.snapshot();
    let clock = frame.clock;
    battle.advance_models(&mut frame, clock, false)?;
    for _ in 0..120 {
        effects.advance(&frame, false)?;
    }
    assert!(effects.frames().is_empty(), "poison puff did not expire");
    assert!(!diagnostics.has_errors());
    Ok(())
}
