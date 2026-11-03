use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, BattleInput, BattlePhase, Cue, TechniqueId};

#[test]
#[ignore = "requires current Steam30 authored owner and GC104 reference; CPU preparation/release only"]
fn sword_rain_tempest_uses_source_action_and_releases_through_public_commands() -> Result<()> {
    use crate::battle::{
        encounter::{Assets, PrepareOptions},
        party::Character,
    };
    use resonance_content::{
        arte, diagnostics::Diagnostics, library::Library, session::SessionData,
    };
    use resonance_events::{
        battle::{DefeatPolicy, Encounter, Setup},
        party::Party,
    };
    let gc = Library::open(
        &std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_GC_ASSETS").context("set RESONANCE_TEST_GC_ASSETS")?,
        ),
        &[],
    )?;
    let gcfiles =
        resonance_content::preparation::capture(&gc, &[], || false, Diagnostics::new(true))?;
    let gcrules = arte::Catalogue::load(&gcfiles)?;
    ensure!(
        gcrules.definition(30)?.action.is_none(),
        "GC30 gained an authored action"
    );
    ensure!(
        super::technique(Character::Lloyd, 30).is_none(),
        "GC30 legacy unsupported set changed"
    );
    let library = Library::open(
        &std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
        ),
        &[],
    )?;
    let diagnostics = Diagnostics::new(true);
    let files =
        resonance_content::preparation::capture(&library, &[], || false, diagnostics.clone())?;
    let data = SessionData::load(&files)?;
    let menus = data.rules.as_deref().context("missing menu rules")?;
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1];
    let mut growth = resonance_battle::Random::new(0x3053);
    party
        .raise_level(&data, 0, 53, None, || growth.next_u32())
        .map_err(anyhow::Error::msg)?;
    party.members[0].techniques = [30].into();
    party.members[0].disabled_techniques.clear();
    party.members[0].shortcuts = [30, 0, 0, 0];
    let inputs = Assets::capture(
        &mut library.snapshot(),
        menus,
        &data,
        &party,
        Setup {
            route: [0; 5],
            encounter: Encounter::Formation(1),
            arena: 13,
            music: None,
            defeat: DefeatPolicy::ResumeEvent,
        },
        diagnostics.clone(),
        &|| false,
    )?;
    ensure!(
        inputs.catalogue.definition(30)?.action.is_some(),
        "Steam30 has no authored action"
    );
    let mut prepared = inputs.prepare(
        menus,
        PrepareOptions {
            hunting_symbol: None,
            event_flags: Default::default(),
            random_seed: 1234,
            map: 332,
            world_music: 0,
            story: 2500,
            colette_state: 0,
            victory_story_flags: [false; 2],
            devils_arms_unlocked: false,
            overlimit_boost: false,
        },
        |request| Ok(Some(request)),
    )?;
    let core = &mut prepared.core;
    let owner = ActorId::from_index(0)?;
    let target = ActorId::from_index(1)?;
    core.suppress_automatic_decisions();
    for _ in 0..600 {
        if core.phase() == BattlePhase::Combat && !core.is_paused() {
            break;
        }
        let mut frame = core.step(BattleInput::default())?;
        let clock = frame.clock;
        core.advance_models(&mut frame, clock, true)?;
    }
    ensure!(
        core.phase() == BattlePhase::Combat,
        "battle entry never finished"
    );
    let action = core
        .prepared_technique(owner, TechniqueId(30))
        .context("Steam30 has no executable action")?
        .action;
    core.set_control_mode(owner, resonance_battle::Control::Auto)?;
    ensure!(
        core.queue_technique_target(owner, action, target)?,
        "Steam30 public Tech command rejected"
    );
    let mut started = None;
    let mut hits = 0;
    let mut startup = false;
    for _ in 0..600 {
        let mut frame = core.step(BattleInput::default())?;
        for cue in &frame.cues {
            match cue {
                Cue::Started {
                    definition: Some(definition),
                    action: id,
                    ..
                } if *definition == action => started = Some(*id),
                Cue::ActionEffect {
                    action: id,
                    request,
                } if Some(*id) == started && request.appearance.member == 141 => startup = true,
                Cue::Hit {
                    source: resonance_battle::ContactSource::Melee { action: id, .. },
                    ..
                } if Some(*id) == started => hits += 1,
                _ => {}
            }
        }
        let clock = frame.clock;
        core.advance_models(&mut frame, clock, true)?;
        if hits > 0 && startup {
            break;
        }
    }
    ensure!(
        started.is_some() && startup && hits > 0,
        "Steam30 release missing: started{started:?}, startup{startup}, hits{hits}, pending{:?}, owner{:?}, target{:?}",
        core.pending_technique(owner),
        core.actors()[0].position,
        core.actors()[1].position
    );
    ensure!(!diagnostics.has_errors(), "Steam30 produced diagnostics");
    Ok(())
}
