//! Escape voice bindings for every playable character.
use super::*;
use battle::model::ModelSource;
use resonance_content::{
    battle_profile, menu_data::MenuData, prepared::Cache, session::SessionData,
};
use resonance_events::party::Party;

#[test]
#[ignore = "requires current party profiles and escape voices; CPU only"]
fn all_party_escape_voices_are_prepared() -> Result<()> {
    let root = common::asset_root();
    let mut cache = Cache::default();
    let files = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let menus: MenuData = files.json("game/menu-data.json")?;
    let mut data: SessionData = files.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default())?;
    for member in &mut party.members {
        member.ex_skills = [0; 4];
        member.compound_ex_skills.clear();
        member.recent_compound_ex_skills.clear();
    }
    let sources: Vec<_> = (1..=9).map(ModelSource::Party).collect();
    let files = battle::model::load_files(&root, files, &sources, &mut cache, || false)?;
    let profiles: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    let prepared = resonance_battle::PreparedBattle::new(
        vec![(actor(), Default::default())],
        Default::default(),
        1,
    )?;
    let actor_id = prepared.actor_ids().next().unwrap();
    let resolve = |sound| Ok(Some(sound));
    let resolver = battle::voice::Resolver::new(&files, &profiles, []);
    for (index, &source) in sources.iter().enumerate() {
        let roster = [(actor_id, source)];
        for allowed in [false, true] {
            let definition = battle::escape::prepare(
                &resolver,
                roster.iter().copied(),
                &party,
                &menus,
                allowed,
                -8,
                resolve,
            )?;
            assert_eq!(definition.allowed, allowed);
            assert_eq!(definition.level_difference, -8);
            assert_eq!(definition.actors.len(), 1);
            let binding = &definition.actors[0];
            assert_eq!(binding.actor, actor_id);
            let voices = profiles.records[index].voices.as_ref().unwrap();
            for (sound, binding) in [
                (voices.escape_request, binding.request),
                (voices.escape_success, binding.success),
                (voices.escape_cancel, binding.cancel),
            ] {
                assert_eq!(binding, Some(sound.context("missing Escape voice")?));
            }
        }
    }
    assert!(!files.diagnostics().has_errors());
    let roster = [(actor_id, ModelSource::Party(9))];
    for bad_level in [-9, 9] {
        assert!(
            battle::escape::prepare(
                &resolver,
                roster.iter().copied(),
                &party,
                &menus,
                true,
                bad_level,
                resolve
            )
            .is_err()
        );
    }
    party.members[8].ex_skills[0] = 6;
    assert!(
        battle::escape::prepare(
            &resolver,
            roster.iter().copied(),
            &party,
            &menus,
            true,
            0,
            resolve
        )
        .is_ok()
    );
    Ok(())
}
