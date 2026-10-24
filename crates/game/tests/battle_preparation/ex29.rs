use super::{opening_options, opening_setup};
use anyhow::{Context, Result, ensure};
use resonance_battle::Cue;
use resonance_battle::TechniqueId;
use resonance_content::{menu_data::MenuData, prepared::Cache};
use resonance_events::{battle::DefeatPolicy, party::Party};
use std::sync::Arc;

/// A saved EX29 gem/skill row must retain Fire Ball's spell-charge definition
/// even when the spell is dormant. The real learning owner activates the
/// prepared action first; the real battle callback then stores the native
/// Fire Ball on a fresh Manual attack.
#[test]
#[ignore = "requires current party EX29/common50/Fire Ball publications; CPU preparation and store execution"]
fn cooked_dormant_ex29_fire_ball_acquires_and_stores_with_prepared_charge() -> Result<()> {
    for (character, catalogue) in [(3, 66), (6, 213), (9, 213)] {
        stores_dormant_fire_ball(character, catalogue)
            .with_context(|| format!("Spell Charge for character {character}"))?;
    }
    Ok(())
}

fn stores_dormant_fire_ball(character: u8, catalogue: u16) -> Result<()> {
    use resonance_battle::{BattleInput, ControlInput, Element};

    let root = super::common::asset_root();
    let mut cache = Cache::default();
    let retained = resonance_content::preparation::capture(
        &resonance_content::library::Library::open(&root, &[])?,
        &[],
        &mut cache,
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let menus: MenuData = resonance_content::menu_data::MenuData::load(&retained)?;
    let mut data: resonance_content::session::SessionData =
        retained.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![character];
    party.field_leader = character;
    // Party defaults slot0 to SemiAuto; this fixture explicitly exercises the
    // Manual fresh-Attack store route.
    party.settings.battle_controls[0] = 0;
    party.items.clear();
    for member in &mut party.members {
        member.assist_shortcuts = [None; 2];
    }
    let member_index = usize::from(character - 1);
    let member = &mut party.members[member_index];
    member.ex_gems = [0, 0, 0, 4];
    member.ex_skills = [0, 0, 0, 29];
    member.techniques.clear();
    member.disabled_techniques.clear();
    member.shortcuts = [0; 4];
    party.validate(&data)?;

    let (assets, prepared) = {
        let mut snapshot = (&retained)
            .library()
            .context("named library is required")?
            .snapshot();
        let catalogue = resonance_content::battle_audio::document::Catalogue::capture(
            &mut snapshot,
            &(|| false),
        )?;
        let inputs = resonance_game::battle::encounter::Inputs::capture(
            &mut snapshot,
            &menus,
            &data,
            &party,
            opening_setup(1, DefeatPolicy::GameOver),
            &mut cache,
            (&retained).diagnostics().clone(),
            &(|| false),
        )?;
        let (assets, prepared, _) =
            inputs.prepare_named(&menus, opening_options(0x2345), &catalogue)?;
        Ok::<_, anyhow::Error>((assets, prepared))
    }?;
    let element = usize::from(assets.catalogue.definition(usize::from(catalogue))?.element);
    assert_eq!(element, 3);

    let actor = prepared.results.actors[0].0;
    assert!(
        prepared
            .core
            .technique_is_current(actor, TechniqueId(catalogue))
            == Some(false),
        "Fire Ball must remain dormant until the live learning owner acquires it"
    );

    // Dormant spells must retain their storage artwork and elemental palette,
    // even before learning activates the action.
    let common = prepared
        .effects
        .iter()
        .find(|effect| {
            effect.source.document.as_deref()
                == Some(resonance_content::battle_effect::document::COMMON)
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
    super::enter_battle(&mut active)?;
    assert_eq!(
        active.technique_is_current(actor, TechniqueId(catalogue)),
        Some(false)
    );
    assert!(
        active
            .shortcuts(actor)
            .context("prepared actor shortcuts before acquisition")?
            .iter()
            .all(Option::is_none)
    );
    let random = active.random_state();
    let action = active.record_technique_acquisition(actor, TechniqueId(catalogue))?;
    assert_eq!(active.random_state(), random);
    assert_eq!(
        active.technique_is_current(actor, TechniqueId(catalogue)),
        Some(true)
    );
    let shortcuts = active
        .shortcuts(actor)
        .context("prepared actor shortcuts after acquisition")?;
    assert_eq!(shortcuts[0], Some(TechniqueId(catalogue)));
    let event = active
        .technique_acquisitions()
        .last()
        .context("acquisition event")?;
    ensure!(
        (event.actor, event.catalogue, event.action) == (actor, TechniqueId(catalogue), action),
        "live acquisition published the wrong dormant action"
    );
    assert!(!party.members[member_index].techniques.contains(&catalogue));
    assert!(!active.is_diagnostic());

    // Entry setup already supplies a retained movement direction. Exercise
    // public movement input before selecting the newly acquired shortcut.
    let mut orient = ControlInput::neutral(actor);
    orient.stick = [30, 0];
    active.step(BattleInput::from(orient))?;
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
    active.step(BattleInput::from(select))?;
    assert!(!active.is_diagnostic());
    let mut ready = false;
    for _ in 0..1024 {
        let mut control = ControlInput::neutral(actor);
        control.technique.held = true;
        control.attack.held = ready;
        control.attack.pressed = ready;
        let frame = active.step(BattleInput::from(control))?;
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
