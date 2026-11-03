use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, BattleInput, BattlePhase, Cue, TechniqueId};

#[test]
#[ignore = "requires refreshed GC Grand Fall party rules owner; CPU preparation/release only"]
fn gc_grand_fall_prepares_and_releases_its_falling_skill_projectile() -> Result<()> {
    use crate::battle::encounter::{Assets, PrepareOptions};
    use resonance_content::{diagnostics::Diagnostics, library::Library, session::SessionData};
    use resonance_events::{
        battle::{DefeatPolicy, Encounter, Setup},
        party::Party,
    };
    let library = Library::open(
        &std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_GC_ASSETS").context("set RESONANCE_TEST_GC_ASSETS")?,
        ),
        &[],
    )?;
    let diagnostics = Diagnostics::new(true);
    let files =
        resonance_content::preparation::capture(&library, &[], || false, diagnostics.clone())?;
    let data = SessionData::load(&files)?;
    let menus = data.rules.as_deref().context("missing menu rules")?;
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![2];
    let mut growth = resonance_battle::Random::new(0x3053);
    party
        .raise_level(&data, 1, 60, None, || growth.next_u32())
        .map_err(anyhow::Error::msg)?;
    party.members[1].techniques = [51].into();
    party.members[1].disabled_techniques.clear();
    party.members[1].shortcuts = [51, 0, 0, 0];
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
        inputs.skill_scenes.contains_key(&51),
        "Grand Fall skill artwork was not captured"
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
        .prepared_technique(owner, TechniqueId(51))
        .context("GC Grand Fall has no executable action")?
        .action;
    core.set_control_mode(owner, resonance_battle::Control::Auto)?;
    ensure!(
        core.queue_technique_target(owner, action, target)?,
        "GC Grand Fall public Tech command rejected"
    );
    let mut started = None;
    let mut hits = 0;
    let mut birth = false;
    for _ in 0..600 {
        let mut frame = core.step(BattleInput::default())?;
        for cue in &frame.cues {
            match cue {
                Cue::Started {
                    definition: Some(definition),
                    action: id,
                    ..
                } if *definition == action => started = Some(*id),
                Cue::ProjectileStarted { action: id, .. } if Some(*id) == started => hits += 1,
                Cue::InitializedEffect(request) | Cue::Effect(request)
                    if request.owner == owner
                        && request.appearance.member == 2
                        && request.origin[1] == 450. =>
                {
                    birth = true
                }
                _ => {}
            }
        }
        let clock = frame.clock;
        core.advance_models(&mut frame, clock, true)?;
        if hits > 0 && birth {
            break;
        }
    }
    ensure!(
        started.is_some() && birth && hits > 0,
        "GC Grand Fall release missing: started{started:?}, birth{birth}, hits{hits}, pending{:?}, owner{:?}, target{:?}",
        core.pending_technique(owner),
        core.actors()[0].position,
        core.actors()[1].position
    );
    ensure!(
        !diagnostics.has_errors(),
        "GC Grand Fall produced diagnostics"
    );
    Ok(())
}
