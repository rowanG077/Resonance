use anyhow::{Context, Result, ensure};
use resonance_battle::Cue;
use resonance_content::{
    menu_data::MenuData,
    prepared::{Cache, Files},
};
use resonance_events::{
    battle::{DefeatPolicy, Setup},
    party::Party,
};
use resonance_game::battle::encounter::{Assets, PrepareOptions};
use std::sync::Arc;

/// A saved EX29 gem/skill row must retain Fire Ball's spell-charge definition
/// even when catalogue66 is dormant. The real learning owner activates the
/// prepared action first; the real battle callback then stores the native
/// Fire Ball on a fresh Manual attack.
#[test]
#[ignore = "requires current cooked Genis EX29/common50/Fire Ball publications; CPU preparation and store execution"]
fn cooked_dormant_ex29_fire_ball_acquires_and_stores_with_prepared_charge() -> Result<()> {
    use resonance_battle::{BattleInput, BattlePhase, ControlInput, Element};

    let root = super::common::asset_root();
    let mut cache = Cache::default();
    let retained = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let menus: MenuData = retained.json("game/menu-data.json")?;
    let mut data: resonance_content::session::SessionData =
        retained.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![3];
    party.field_leader = 3;
    // Party defaults slot0 to SemiAuto; this fixture explicitly exercises the
    // Manual fresh-Attack store route.
    party.settings.battle_controls[0] = 0;
    party.items.clear();
    for member in &mut party.members {
        member.assist_shortcuts = [None; 2];
    }
    let genis = &mut party.members[2];
    genis.ex_gems = [0, 0, 0, 4];
    genis.ex_skills = [0, 0, 0, 29];
    genis.techniques.clear();
    genis.disabled_techniques.clear();
    genis.shortcuts = [0; 4];
    party.validate(&data)?;
    assert!(!party.members[2].techniques.contains(&66));
    assert_eq!(party.members[2].ex_gems[3], 4);
    assert_eq!(party.members[2].ex_skills[3], 29);

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
    let catalogue = &assets.catalogue;
    let element = usize::from(catalogue.definition(66)?.element);
    assert_eq!(element, 3);
    let prepared = assets.prepare(
        &menus,
        PrepareOptions {
            devils_arms_unlocked: false,
            victory_story_flags: [false; 2],
            random_seed: 0x2345,
            map: 332,
            world_music: 0,
            story: 2500,
            story3: false,
            colette_state: 0,
            overlimit_boost: false,
        },
        |request| Ok(Some(request)),
    )?;
    let actor = prepared.results.actors[0].0;
    assert!(
        prepared.core.technique_is_current(actor, 66) == Some(false),
        "Fire Ball must remain dormant until the live learning owner acquires it"
    );

    // This render assertion proves the dormant preparation retained the
    // common50 dependency without confusing program50 with a particle member:
    // both common6 and common50 emit particle82, while the latter is selected
    // only when spell_charge_slots is populated.
    let common = prepared
        .effects
        .iter()
        .find(|effect| {
            effect.source.source_sha256 == assets.common_effects.as_ref().unwrap().source_sha256
        })
        .context("prepared common effect bank")?;
    let common50 = common.source.program(50)?;
    ensure!(
        common50
            .iter()
            .filter_map(|event| match event.operation {
                resonance_content::battle_effect::EffectOperation::Spawn { particle, .. } =>
                    Some(u16::from(particle)),
                _ => None,
            })
            .all(|id| common.members.contains(&id)),
        "dormant EX29 omitted common50's reachable particle closure"
    );
    ensure!(
        common
            .palettes
            .get(&82)
            .is_some_and(|pairs| pairs.contains(&[9, 1])),
        "dormant EX29 omitted Fire palette capacity for common50 particle82"
    );

    // record_technique_acquisition is the sole live membership transition;
    // no saved-party mutation or synthetic action handle is used here.
    let mut active = prepared.core;
    assert!(
        active
            .actors()
            .iter()
            .all(|actor| actor.equipment.base_element != Some(Element::Fire))
    );
    for _ in 0..600 {
        if active.phase() == BattlePhase::Combat {
            break;
        }
        active.step(BattleInput::default())?;
    }
    ensure!(
        active.phase() == BattlePhase::Combat,
        "encounter entry did not complete before dormant acquisition"
    );
    assert_eq!(active.technique_is_current(actor, 66), Some(false));
    assert!(
        active
            .shortcuts(actor)
            .context("prepared actor shortcuts before acquisition")?
            .iter()
            .all(|&catalogue| catalogue == 0)
    );
    let random = active.random_state();
    let action = active.record_technique_acquisition(actor, 66)?;
    assert_eq!(active.random_state(), random);
    assert_eq!(active.technique_is_current(actor, 66), Some(true));
    let shortcuts = active
        .shortcuts(actor)
        .context("prepared actor shortcuts after acquisition")?;
    assert_eq!(shortcuts[0], 66);
    let event = active
        .technique_acquisitions()
        .last()
        .context("acquisition event")?;
    ensure!(
        (event.actor, event.catalogue, event.action) == (actor, 66, action),
        "live acquisition published the wrong dormant action"
    );
    assert!(!party.members[2].techniques.contains(&66));
    assert!(!active.is_diagnostic());

    // Entry setup already supplies a retained movement direction. Exercise
    // public movement input before selecting the newly acquired shortcut.
    let mut orient = ControlInput::neutral(actor);
    orient.stick = [30, 0];
    active.step(BattleInput {
        controllers: vec![orient],
        ..Default::default()
    })?;
    ensure!(
        active.actors()[actor.index()]
            .movement
            .direction
            .iter()
            .any(|value| value.abs() > 0.01),
        "public movement input did not establish a retained cast direction"
    );

    // Select the acquired row through the same public shortcut path used by
    // gameplay. Keep Technique held for its cast delay; only after the
    // callback is delayed/at clock zero send a fresh Attack edge.
    let mut select = ControlInput::neutral(actor);
    select.technique.pressed = true;
    select.technique.held = true;
    active.step(BattleInput {
        controllers: vec![select],
        ..Default::default()
    })?;
    assert!(!active.is_diagnostic());
    let mut ready = false;
    for _ in 0..1024 {
        let mut control = ControlInput::neutral(actor);
        control.technique.held = true;
        control.attack.held = ready;
        control.attack.pressed = ready;
        let frame = active.step(BattleInput {
            controllers: vec![control],
            ..Default::default()
        })?;
        ensure!(
            !active.is_diagnostic(),
            "fault-tolerant battle hid a cast failure"
        );
        if active.actors()[actor.index()].stored_spell.is_some() {
            ensure!(
                frame.cues.iter().any(|cue| matches!(
                    cue,
                    Cue::ExSkillLabel { actor: owner, .. } if *owner == actor
                )),
                "stored Fire Ball omitted its EX skill label cue"
            );
            ensure!(
                frame.cues.iter().any(|cue| matches!(cue,
                    Cue::Casting { actor: owner, action: stored, phase: resonance_battle::CastPhase::Stored }
                        if *owner == actor && *stored == action
                )),
                "stored spell omitted its presentation event"
            );
            let feedback = prepared
                .feedback
                .casting
                .get(&action)
                .context("stored spell feedback")?;
            assert_eq!(feedback.stored_effect.resource, common.resource);
            assert_eq!(feedback.stored_effect.member, 50);
            break;
        }
        ready = matches!(
            active.activity(actor),
            resonance_battle::Activity::Casting { held: true }
        ) || active.casting_remaining(actor) == Some(0);
    }
    ensure!(
        active.actors()[actor.index()].stored_spell == Some(action),
        "dormant EX29 Fire Ball never reached the retained-spell store callback"
    );
    Ok(())
}
