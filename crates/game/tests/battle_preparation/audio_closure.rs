use anyhow::{Context, Result, ensure};
use resonance_battle::SoundBinding;
use resonance_content::{
    menu_data::MenuData,
    prepared::{Cache, Files},
    session::SessionData,
};
use resonance_game::{
    battle::{
        encounter::{Assets, Inputs, PrepareOptions},
        voice::Sound,
    },
    field::FieldCheckpoint,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    sync::Arc,
};

#[test]
#[ignore = "requires current structural/audio publication and an actual opening checkpoint; no activation or devices"]
fn maintained_opening_audio_covers_real_cold_bindings() -> Result<()> {
    let checkpoint = std::env::var_os("RESONANCE_BATTLE_CHECKPOINT")
        .context("set RESONANCE_BATTLE_CHECKPOINT to the actual opening FieldCheckpoint")?;
    let checkpoint: FieldCheckpoint = serde_json::from_slice(&fs::read(checkpoint)?)?;
    ensure!(
        checkpoint.map_id == 332,
        "checkpoint is outside the opening route"
    );
    let requirements: BTreeMap<String, BTreeSet<u16>> = serde_json::from_slice(&fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/battle/audio-requirements.json"),
    )?)?;
    ensure!(
        requirements.len() == 3
            && ["sounds", "streams", "music"]
                .iter()
                .all(|key| requirements.get(*key).is_some_and(|ids| !ids.is_empty())),
        "maintained opening audio requirements are incomplete"
    );
    let root = super::common::asset_root();
    let mut cache = Cache::default();
    let retained = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let menus: MenuData = retained.json("game/menu-data.json")?;
    let mut data: SessionData = retained.json("game/session-data.json")?;
    data.ex_skills = Some(Arc::new(menus.ex_skills.clone()));
    let mut party = checkpoint.progress.party;
    party.bind_ex_skills(&data);
    party.validate(&data)?;
    let before = serde_json::to_value(&party)?;
    let options = || -> Result<_> {
        Ok(PrepareOptions {
            random_seed: 0x2345,
            map: checkpoint.map_id.try_into()?,
            world_music: *checkpoint
                .progress
                .script_globals
                .get(0x50 / 4)
                .context("missing world selection")?,
            story: *checkpoint
                .progress
                .script_globals
                .get(0x40 / 4)
                .context("missing story progress")?,
            overlimit_boost: false,
        })
    };
    let mut scripts = symphonia_script_tools::PreparationCache::default();
    for encounter in [1, 2] {
        let setup = resonance_events::battle::Setup {
            encounter,
            arena: 13,
            music: None,
            defeat: resonance_events::battle::DefeatPolicy::GameOver,
        };
        let inputs = Inputs::load(&root, &retained, &menus, &party, setup, &mut cache, || {
            false
        })?;
        let mut actual = BTreeSet::new();
        let first = inputs.prepare(&menus, &party, options()?, &mut scripts, |request| {
            let (resource, index, key) = match request {
                Sound::Cue(index) => (0, index, "sounds"),
                Sound::Stream(index) => (1, index, "streams"),
            };
            ensure!(
                requirements[key].contains(&index),
                "unlisted opening {key} ID{index}"
            );
            actual.insert((resource, index));
            Ok(SoundBinding { resource, index })
        })?;
        ensure!(
            requirements["music"].contains(&first.music),
            "unlisted selected battle music"
        );
        drop(first);
        drop(inputs);
        let assets = Assets::load(&root, &retained, &menus, &party, setup, &mut cache, || {
            false
        })?;
        let mut rebound = BTreeSet::new();
        let prepared = assets.prepare(&menus, &party, options()?, &mut scripts, |request| {
            let (resource, index, present) = match request {
                Sound::Cue(index) => (
                    0,
                    index,
                    assets
                        .audio
                        .assets
                        .sounds
                        .contains_key(&i16::try_from(index)?),
                ),
                Sound::Stream(index) => (
                    1,
                    index,
                    assets.audio.assets.voices.contains_key(&u32::from(index)),
                ),
            };
            ensure!(
                present,
                "missing published battle binding {resource}:{index}"
            );
            rebound.insert((resource, index));
            Ok(SoundBinding { resource, index })
        })?;
        assert_eq!(actual, rebound);
        for cue in [1, 2, 3, 4, 6, 7, 38, 80] {
            ensure!(
                requirements["sounds"].contains(&cue)
                    && assets.audio.assets.sounds.contains_key(&(cue as i16)),
                "unprepared UI/result cue {cue}"
            );
        }
        for music in [prepared.music, 95, 96] {
            ensure!(
                requirements["music"].contains(&music)
                    && assets.audio.assets.music.contains_key(&(music as i16)),
                "unprepared battle/result music {music}"
            );
        }
    }
    assert_eq!(serde_json::to_value(&party)?, before);
    Ok(())
}
