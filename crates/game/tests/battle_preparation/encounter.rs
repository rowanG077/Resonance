use anyhow::{Context, Result};
use resonance_content::{
    menu_data::MenuData,
    prepared::{Cache, Files},
};

#[test]
#[ignore = "requires current opening battle publications; CPU preparation only"]
fn complete_opening_candidates_are_atomic_and_replay_the_same_prepared_generation() -> Result<()> {
    use resonance_battle::{BattleInput, Sound};
    use resonance_events::{
        battle::{DefeatPolicy, Setup},
        party::Party,
    };
    use resonance_game::battle::encounter::{Assets, PrepareOptions};
    use std::sync::Arc;

    let root = super::common::asset_root();
    let mut file_cache = Cache::default();
    let mut retained = Files::load(
        &root,
        &["fields/map-332.preload.json"],
        &mut file_cache,
        || false,
    )?;
    let mut menus: MenuData = retained.json("game/menu-data.json")?;
    for path in [
        resonance_content::menu_data::MANUAL_PATH,
        resonance_content::menu_data::FIGURINES_PATH,
        resonance_content::menu_data::SYNOPSIS_PATH,
        resonance_content::menu_data::CUSTOMIZE_PATH,
        resonance_content::menu_data::RENAME_PATH,
    ] {
        retained.remove(path);
    }
    retained.insert(
        resonance_content::menu_data::MANUAL_PATH.into(),
        b"{".as_slice().into(),
    );
    menus.presentation.world_map = None;
    assert!(menus.validate().is_err());
    menus.validate_gameplay()?;
    let mut data: resonance_content::session::SessionData =
        retained.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1, 2, 3];
    for (index, level) in [(0, 3), (1, 1), (2, 2)] {
        party
            .raise_level(&data, index, level, None, || 7)
            .map_err(anyhow::Error::msg)?;
    }

    let mut invalid = menus.clone();
    invalid.presentation.monsters.as_mut().unwrap().records[36]
        .statistics
        .clear();
    assert!(
        resonance_game::battle::encounter::Inputs::load(
            &root,
            &retained,
            &invalid,
            &data,
            &party,
            Setup {
                route: [0; 5],
                encounter: resonance_events::battle::Encounter::Formation(1),
                arena: 13,
                defeat: DefeatPolicy::GameOver,
                music: None
            },
            &mut file_cache,
            || false,
        )
        .is_err(),
        "selected enemy data must still validate"
    );
    for formation in [1, 2] {
        let mut assets = Assets::load(
            &root,
            &retained,
            &menus,
            &data,
            &party,
            Setup {
                route: [0; 5],
                encounter: resonance_events::battle::Encounter::Formation(formation),
                arena: 13,
                defeat: DefeatPolicy::GameOver,
                music: None,
            },
            &mut file_cache,
            || false,
        )?;
        let options = || PrepareOptions {
            devils_arms_unlocked: false,
            victory_story_flags: [false; 2],
            random_seed: 0x2345,
            map: 332,
            world_music: 0,
            story: 2500,
            story3: false,
            colette_state: 0,
            overlimit_boost: false,
        };
        let audio = assets.audio.as_ref().context("missing battle audio")?;
        let sound = |request| -> Result<_> {
            match request {
                Sound::Cue(index) => {
                    anyhow::ensure!(
                        audio.assets.sounds.contains_key(&i16::try_from(index)?),
                        "unpublished battle cue {index}"
                    );
                }
                Sound::Stream(index) => {
                    anyhow::ensure!(
                        audio.assets.voices.contains_key(&u32::from(index)),
                        "unpublished battle voice {index}"
                    );
                }
            };
            Ok(Some(request))
        };
        if formation == 1 {
            let mut changed = menus.clone();
            changed.items.clear();
            let error = assets
                .prepare(&changed, options(), sound)
                .err()
                .expect("changed selected equipment must be checked before deriving stats");
            assert!(error.to_string().contains("missing equipped item"));
        }
        let prepared = assets.prepare(&menus, options(), sound)?;
        assert_eq!(prepared.characters, [1, 2, 3]);
        assert_eq!(prepared.results.formation, formation);
        assert_eq!(prepared.results.enemies.len(), usize::from(formation));
        let mut first = prepared.core;
        let mut second = assets.prepare(&menus, options(), sound)?.core;
        assert_eq!(first.snapshot(), second.snapshot());
        assert_eq!(first.actors().len(), 3 + usize::from(formation));
        // The held snapshot must not consume one of the registered world visits.
        let seed = first.random_state();
        let _ = first.snapshot();
        assert_eq!(first.random_state(), seed);
        for _ in 0..240 {
            assert_eq!(
                first.step(BattleInput::default())?,
                second.step(BattleInput::default())?
            );
        }
        assets
            .inputs
            .files
            .remove(resonance_content::battle_projectile::PATH);
        assert!(assets.prepare(&menus, options(), |_| Ok(None)).is_err());
        assert!(
            retained
                .read(resonance_content::battle_projectile::PATH)
                .is_ok()
        );
    }
    Ok(())
}
