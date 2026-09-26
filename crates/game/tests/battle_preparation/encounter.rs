use anyhow::Result;
use resonance_content::{
    battle_formation::PATH,
    field_preload::Manifest,
    menu_data::MenuData,
    prepared::{Cache, Files},
};
use resonance_game::battle::encounter::enemies;

#[test]
#[ignore = "requires the complete current cooked library; no devices"]
fn cooked_formations_select_enemy_requirements_from_a_verified_field_snapshot() -> Result<()> {
    let root = super::common::asset_root();
    let manifest: Manifest = super::common::cooked("fields/map-340.preload.json");
    assert!(manifest.files.contains_key(PATH));
    let files = Files::load(
        &root,
        &["fields/map-340.preload.json"],
        &mut Cache::default(),
        || false,
    )?;
    let menu: MenuData = files.json("game/menu-data.json")?;
    menu.validate()?;
    let first = enemies(&files, &menu.monsters, 1)?;
    assert_eq!(first.resources[0].id, 36);
    assert_eq!(first.spawns[0].variant, 0);
    assert_eq!(first.resources[0].statistics[0].hp, 800);
    let second = enemies(&files, &menu.monsters, 2)?;
    assert_eq!(
        second
            .resources
            .iter()
            .map(|monster| monster.id)
            .collect::<Vec<_>>(),
        [49, 36]
    );
    assert_eq!(second.spawns[0].variant, 1);
    assert_eq!(second.spawns[0].position, Some([300, 0]));
    assert_eq!(second.spawns[1].position, Some([500, 300]));
    assert_eq!(second.resources[0].statistics[1].hp, 320);
    for id in 0..1000 {
        let result = enemies(&files, &menu.monsters, id);
        if id == 90 {
            assert_eq!(
                result.err().unwrap().to_string(),
                "formation 90 requires missing enemy 191 variant 3"
            );
        } else {
            result?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires current opening battle publications; CPU preparation only"]
fn complete_opening_candidates_are_atomic_and_replay_the_same_prepared_generation() -> Result<()> {
    use resonance_battle::{Battle, BattleInput, SoundBinding};
    use resonance_events::{
        battle::{DefeatPolicy, Setup},
        party::Party,
    };
    use resonance_game::battle::{
        encounter::{Assets, PrepareOptions},
        voice::Sound,
    };
    use std::sync::Arc;

    let root = super::common::asset_root();
    let mut file_cache = Cache::default();
    let retained = Files::load(
        &root,
        &["fields/map-332.preload.json"],
        &mut file_cache,
        || false,
    )?;
    let menus: MenuData = retained.json("game/menu-data.json")?;
    let mut data: resonance_content::session::SessionData =
        retained.json("game/session-data.json")?;
    data.ex_skills = Some(Arc::new(menus.ex_skills.clone()));
    let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
    party.formation = vec![1, 2, 3];
    for (index, level) in [(0, 3), (1, 1), (2, 2)] {
        party
            .raise_level(&data, index, level, None, || 7)
            .map_err(anyhow::Error::msg)?;
    }
    let before = serde_json::to_value(&party)?;
    let mut scripts = symphonia_script_tools::PreparationCache::default();
    for formation in [1, 2] {
        let mut assets = Assets::load(
            &root,
            &retained,
            &menus,
            &party,
            Setup {
                encounter: formation,
                arena: 13,
                defeat: DefeatPolicy::GameOver,
                music: None,
            },
            &mut file_cache,
            || false,
        )?;
        let options = || PrepareOptions {
            random_seed: 0x2345,
            map: 332,
            world_music: 0,
            story: 2500,
            overlimit_boost: false,
        };
        let audio = &assets.audio;
        let sound = |request| -> Result<_> {
            let (resource, index) = match request {
                Sound::Cue(index) => {
                    anyhow::ensure!(
                        audio.assets.sounds.contains_key(&i16::try_from(index)?),
                        "unpublished battle cue {index}"
                    );
                    (0, index)
                }
                Sound::Stream(index) => {
                    anyhow::ensure!(
                        audio.assets.voices.contains_key(&u32::from(index)),
                        "unpublished battle voice {index}"
                    );
                    (1, index)
                }
            };
            Ok(SoundBinding { resource, index })
        };
        let prepared = assets.prepare(&menus, &party, options(), &mut scripts, sound)?;
        assert_eq!(prepared.characters, [1, 2, 3]);
        assert_eq!(prepared.results.formation, formation);
        assert_eq!(prepared.results.enemies.len(), usize::from(formation));
        let mut first = Battle::new(prepared.core.clone());
        let mut second = Battle::new(prepared.core);
        assert_eq!(first.snapshot(), second.snapshot());
        assert_eq!(first.actors().len(), 3 + usize::from(formation));
        assert_eq!(first.actors()[3].position[0], 300.);
        assert_eq!(first.actors()[3].hp, if formation == 1 { 800 } else { 320 });
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
        assert_eq!(serde_json::to_value(&party)?, before);
        assets
            .inputs
            .files
            .bytes
            .remove("scripts/battle/normal_lloyd.sym");
        assert!(
            assets
                .prepare(&menus, &party, options(), &mut scripts, |_| Ok(
                    SoundBinding {
                        resource: 0,
                        index: 0
                    }
                ))
                .is_err()
        );
        assert_eq!(serde_json::to_value(&party)?, before);
        assert!(retained.read("scripts/battle/normal_lloyd.sym").is_ok());
    }
    Ok(())
}
