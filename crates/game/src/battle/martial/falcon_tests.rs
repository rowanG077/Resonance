use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, BattleInput, BattlePhase, Cue, TechniqueId};

#[test]
#[ignore = "requires Steam base and isolated native Falcon overlay; CPU preparation/release only"]
fn steam_falcon_authored_action_keeps_native_projectile_clock_and_presentation() -> Result<()> {
    use crate::battle::encounter::{Assets, PrepareOptions};
    use resonance_content::{diagnostics::Diagnostics, library::Library, session::SessionData};
    use resonance_events::{
        battle::{DefeatPolicy, Encounter, Setup},
        party::Party,
    };
    let library = Library::open_paths(
        &std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
        ),
        &[std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_OVERLAY").context("set RESONANCE_TEST_OVERLAY")?,
        )],
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
        .raise_level(&data, 0, 99, None, || growth.next_u32())
        .map_err(anyhow::Error::msg)?;
    party.members[0].techniques = [228].into();
    party.members[0].disabled_techniques.clear();
    party.members[0].shortcuts = [228, 0, 0, 0];
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
        inputs.skill_scenes.contains_key(&228),
        "Falcon skill artwork was not captured"
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
        .prepared_technique(owner, TechniqueId(228))
        .context("Steam Falcon has no executable action")?
        .action;
    core.set_control_mode(owner, resonance_battle::Control::Auto)?;
    ensure!(
        core.queue_technique_target(owner, action, target)?,
        "Steam Falcon public Tech command rejected"
    );
    let mut started = None;
    let mut hits = 0;
    let mut portrait = false;
    let mut ages = Vec::new();
    for _ in 0..600 {
        let mut frame = core.step(BattleInput::default())?;
        for cue in &frame.cues {
            match cue {
                Cue::Started {
                    definition: Some(definition),
                    action: id,
                    ..
                } if *definition == action => started = Some(*id),
                Cue::ProjectileStarted { action: id, .. } if Some(*id) == started => {
                    hits += 1;
                    ages.push(
                        core.action_age(*id)
                            .context("projectile action lost its clock")?,
                    );
                }
                Cue::Cutin {
                    catalogue: TechniqueId(228),
                    duration: 44,
                    ..
                } => portrait = true,
                _ => {}
            }
        }
        let clock = frame.clock;
        core.advance_models(&mut frame, clock, true)?;
        if hits == 4 && portrait {
            break;
        }
    }
    ensure!(
        started.is_some() && portrait && hits == 4,
        "Steam Falcon release missing: started{started:?}, portrait{portrait}, hits{hits}, pending{:?}, owner{:?}, target{:?}, projectile ages{ages:?}",
        core.pending_technique(owner),
        core.actors()[0].position,
        core.actors()[1].position
    );
    // Later launches can wait for contact hitstop in this live target fixture.
    // The source owner test checks all four authored timestamps exactly.
    assert_eq!(&ages[..2], &[32, 106]);
    ensure!(
        !diagnostics.has_errors(),
        "Steam Falcon produced diagnostics"
    );
    Ok(())
}
