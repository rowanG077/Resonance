use super::*;

#[test]
#[ignore = "requires prepared field/menu assets; no output devices"]
fn offline_restore_preserves_artes_while_user_config_applies_explicit_preferences() -> Result<()> {
    let root = asset_root();
    let package = Arc::new(FieldPackage::prepare(
        &root,
        332,
        &mut Default::default(),
        || false,
    )?);
    let files = package.files.clone();
    let data = resonance_content::session::SessionData::load(&files)?;
    let mut checkpoint = field_checkpoint(&files)?;
    let party = &mut checkpoint.progress.party;
    party.settings.preferences.learn_both_arte_paths = true;
    party.settings.preferences.learn_elemental_artes = true;
    party.settings.preferences.battle_rank = 2;
    party.members[0]
        .techniques
        .extend(&data.characters[0].allowed_techniques);
    let mut pruned = party.clone();
    pruned.forget_opposite_path_artes();
    let opposite = *party.members[0]
        .techniques
        .difference(&pruned.members[0].techniques)
        .next()
        .context("fixture has no opposite-path arte")?;
    party.members[0].shortcuts[0] = opposite;
    party.members[0].disabled_techniques.insert(opposite);
    party.members[0].technique_uses.insert(opposite, 37);

    let mut session = Session::load_prepared(package, Some(checkpoint.clone()), None)?;
    session.field_mut().menu = Some(resonance_game::menu::Menu::new(
        resonance_game::menu::Page::Main,
        Some(resonance_game::Checkpoint::Field(checkpoint)),
        false,
    ));
    let snapshot = |session: &Session| -> Result<serde_json::Value> {
        Ok(serde_json::to_value((
            &session.events().world.party,
            &session.menu().unwrap().checkpoint,
        ))?)
    };
    let before = snapshot(&session)?;
    let offline = crate::config::Preferences::default();
    offline.apply(&mut session, false);
    assert_eq!(snapshot(&session)?, before);
    // Returning from a battle or another scene must preserve the same settings.
    offline.apply(&mut session, false);
    assert_eq!(snapshot(&session)?, before);

    let directory = tempfile::tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        "learn_both_arte_paths = false\nlearn_elemental_artes = true\nbattle_rank = 0\n",
    )?;
    let configured = crate::config::Preferences::load(config)?;
    configured.apply(&mut session, false);
    for party in [
        session.events().world.party.as_ref().unwrap(),
        &session
            .menu()
            .unwrap()
            .checkpoint
            .as_ref()
            .unwrap()
            .progress()
            .party,
    ] {
        assert!(!party.settings.preferences.learn_both_arte_paths);
        assert_eq!(party.settings.preferences.battle_rank, 2);
        assert!(!party.members[0].techniques.contains(&opposite));
        assert!(!party.members[0].disabled_techniques.contains(&opposite));
        assert_eq!(party.members[0].shortcuts[0], 0);
        assert_eq!(party.members[0].technique_uses[&opposite], 37);
    }
    Ok(())
}
