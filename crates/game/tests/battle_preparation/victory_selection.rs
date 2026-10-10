use super::*;
use resonance_battle::{ActorAvailability, BattleResult, Cue, Sound};
use resonance_content::{
    arte, battle_audio::Audio, diagnostics::Diagnostics, menu_data::MenuData, prepared::Cache,
    session::SessionData,
};
use resonance_events::{
    battle::{DefeatPolicy, Setup},
    party::Party,
};
use resonance_game::battle::{
    encounter::{Assets, PrepareOptions},
    lifecycle::victory_selection,
    party::Character,
    results,
};

/// Real result resources exercise selected group playback and persistence with
/// a knocked-out participant. Combat recognition has native lifecycle coverage.
#[test]
#[ignore = "requires prepared battle and audio assets; no devices"]
fn fallen_companion_results_place_speak_and_preserve_party_state() -> Result<()> {
    let root = common::asset_root();
    let mut cache = Cache::default();
    let retained = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let menus: Arc<MenuData> = Arc::new(retained.json("game/menu-data.json")?);
    let mut data: SessionData = retained.json("game/session-data.json")?;
    data.rules = Some(menus.clone());
    let data = Arc::new(data);
    let catalogue: Arc<arte::Catalogue> = Arc::new(retained.json(arte::PATH)?);
    let mut party = Party::new(&data, Default::default())?;
    party.formation = vec![Character::Raine as u8, Character::Sheena as u8];
    party.field_leader = Character::Raine as u8;
    party.settings.battle_controls = [2; 4];
    for member in &mut party.members {
        member.disabled_techniques = member.techniques.clone();
    }
    let raine = usize::from(Character::Raine as u8 - 1);
    party.members[raine].hp = 0;
    party.validate(&data)?;
    let assets = Assets::load(
        &root,
        &retained,
        &menus,
        &data,
        &party,
        Setup {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(1),
            arena: 13,
            music: None,
            defeat: DefeatPolicy::GameOver,
        },
        &mut cache,
        || false,
    )?;
    let audio = assets.audio.as_ref().context("missing battle audio")?;
    let prepared = assets.prepare(
        &menus,
        PrepareOptions {
            random_seed: 4,
            map: 332,
            world_music: 0,
            story: 2500,
            story3: false,
            colette_state: 0,
            devils_arms_unlocked: false,
            victory_story_flags: [true, true],
            overlimit_boost: false,
        },
        |sound| bind(audio, sound),
    )?;
    let group = 2; // Raine's fallen-companion dialogue.
    let expected = prepared
        .results
        .groups
        .iter()
        .find(|row| row.id == group)
        .unwrap();
    let expected_actor = prepared
        .results
        .actors
        .iter()
        .find(|&&(_, character)| character == expected.leader)
        .unwrap()
        .0;
    let expected_voice = expected.voice;
    let mut actors = prepared.core.actors().to_vec();
    for actor in &mut actors {
        if actor.side == Side::Enemy {
            actor.hp = 0;
        }
    }
    let mut live = resonance_battle::PreparedBattle::new(
        (actors)
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )?
    .finish()?;
    let diagnostics = Diagnostics::new(true);
    live.set_diagnostics(diagnostics.clone());
    assert_eq!(live.recognize_result(), Some(BattleResult::Victory));
    let mut candidate = results::Candidate::new(
        prepared.results,
        live.actors(),
        party,
        Default::default(),
        data,
        menus,
        catalogue,
    )?;
    let context = candidate.victory_context(&live)?;
    assert!(
        context
            .party
            .iter()
            .any(|member| member.character == Character::Raine && member.dead)
    );
    let selection = victory_selection::select(&context, 0);
    assert_eq!(selection.group, group);
    candidate.select_victory(&live, group, selection.pose)?;
    assert_eq!(candidate.selection().unwrap().actor, expected_actor);
    assert_eq!(
        live.actors()[expected_actor.index()].availability,
        ActorAvailability::Dead
    );
    live.retire_combat()?;
    candidate.prepare_results(&mut live)?;
    let leader = &live.actors()[expected_actor.index()];
    assert_eq!(leader.hp, 0);
    assert_eq!(leader.availability, ActorAvailability::Active);
    let mut spoken = false;
    for _ in 0..600 {
        let frame = live.step(BattleInput::default())?;
        for cue in frame.cues {
            if let Cue::Voice { actor, sound, .. } = cue {
                spoken |= actor == expected_actor && sound == expected_voice;
            }
        }
        if spoken {
            break;
        }
    }
    assert!(spoken);
    candidate.accept_victory()?;
    let outcome = live
        .finish_result()?
        .outcome
        .context("missing victory outcome")?;
    let result = candidate.finish(&live, &outcome)?;
    assert_ne!(result.party.battles.victory_groups & (1 << group), 0);
    assert_eq!(result.party.members[raine].hp, 0);
    assert!(!diagnostics.has_errors());
    Ok(())
}

fn bind(audio: &Audio, sound: Sound) -> Result<Option<Sound>> {
    Ok(match sound {
        Sound::Cue(index) => {
            anyhow::ensure!(
                audio.assets.sounds.contains_key(&i16::try_from(index)?),
                "unpublished cue {index}"
            );
            Some(Sound::Cue(index))
        }
        Sound::Stream(index) => {
            anyhow::ensure!(
                audio.assets.voices.contains_key(&u32::from(index)),
                "unpublished voice {index}"
            );
            Some(Sound::Stream(index))
        }
    })
}
