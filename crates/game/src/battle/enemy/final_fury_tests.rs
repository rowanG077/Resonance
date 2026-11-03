use super::*;

#[test]
#[ignore = "requires current source-cooked enemy226, martial and skill200 owners"]
fn native_final_fury_enemy_prepares_its_explicit_skill_artwork() -> Result<()> {
    use crate::battle::encounter::{Assets, PrepareOptions};
    use resonance_content::{diagnostics::Diagnostics, library::Library, session::SessionData};
    use resonance_events::{
        battle::{DefeatPolicy, Encounter, Setup},
        party::Party,
    };
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
    );
    let library = Library::open(&root, &[])?;
    let diagnostics = Diagnostics::new(true);
    let files =
        resonance_content::preparation::capture(&library, &[], || false, diagnostics.clone())?;
    let session = SessionData::load(&files)?;
    let menus = session.rules.as_deref().context("missing menu rules")?;
    let mut party = Party::new(&session, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1];
    party.members[0].techniques = [1].into();
    party.members[0].disabled_techniques.clear();
    party.members[0].shortcuts = [1, 0, 0, 0];
    let inputs = Assets::capture(
        &mut library.snapshot(),
        menus,
        &session,
        &party,
        Setup {
            route: [0; 5],
            encounter: Encounter::Formation(42),
            arena: 13,
            music: None,
            defeat: DefeatPolicy::ResumeEvent,
        },
        diagnostics.clone(),
        &|| false,
    )?;
    anyhow::ensure!(
        inputs.skill_scenes.contains_key(&200),
        "enemy Final Fury did not capture skill200"
    );
    let prepared = inputs.prepare(
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
    let actor = prepared
        .core
        .actors()
        .iter()
        .position(|actor| actor.side == resonance_battle::Side::Enemy)
        .context("missing Convict actor")?;
    let choices = prepared
        .core
        .enemy_choices(resonance_battle::ActorId::from_index(actor)?)
        .context("missing Convict choices")?;
    let action = choices[6].action;
    anyhow::ensure!(
        prepared.feedback.action_catalogues[&action] == resonance_battle::TechniqueId(200),
        "Final Fury lost its action catalogue"
    );
    anyhow::ensure!(
        !diagnostics.has_errors(),
        "Final Fury enemy preparation produced diagnostics"
    );
    Ok(())
}
