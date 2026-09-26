//! Two real opening battles and the suspended original field event. The headless
//! service acknowledges prepared UI/audio without presenting them: this proves
//! gameplay/lifecycle/commit integration, not image or audio timing fidelity.
mod common;
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Battle, BattleInput, BattleResult, ButtonInput, Control, ControlInput, Cue, SoundBinding,
    VoiceId,
};
use resonance_content::{
    arte,
    battle_audio::Audio,
    field::FieldAssets,
    menu_data::MenuData,
    prepared::{Cache, Files},
    session::SessionData,
};
use resonance_events::{battle::Request, party::Party};
use resonance_game::{
    battle::{encounter, lifecycle, results, voice::Sound},
    field::{FieldCheckpoint, FieldInput, FieldSession},
};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use symphonia_script_tools::PreparationCache;

#[test]
#[ignore = "requires current opening publications and paired checkpoint; real combat, no devices"]
fn opening_combats_commit_once_and_resume_the_original_event_through_both_results() -> Result<()> {
    let root = common::asset_root();
    let mut cache = Cache::default();
    let retained = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let menus: Arc<MenuData> = Arc::new(retained.json("game/menu-data.json")?);
    let mut data: SessionData = retained.json("game/session-data.json")?;
    data.ex_skills = Some(Arc::new(menus.ex_skills.clone()));
    let data = Arc::new(data);
    let catalogue: Arc<arte::Catalogue> = Arc::new(retained.json(arte::PATH)?);
    let mut field = opening_field(&retained, menus.clone(), data.clone())?;
    let initial_total = field
        .events
        .world
        .party
        .as_ref()
        .context("missing party")?
        .battles
        .total;
    ensure!(
        field.events.trigger(2002, false)?,
        "opening event did not start"
    );
    let mut scripts = PreparationCache::default();
    let mut commits: Vec<(Request, results::Completed)> = Vec::new();

    for tick in 0..30_000 {
        if let Some(request) = field.events.world.battle_request.take() {
            let formation = u16::try_from(commits.len() + 1)?;
            ensure!(formation <= 2, "unexpected third opening battle");
            assert_eq!(request.setup.encounter, formation);
            assert_eq!(request.setup.arena, 13);
            assert_eq!(
                request.setup.defeat,
                resonance_events::battle::DefeatPolicy::GameOver
            );
            let frozen = Frozen::read(&field)?;
            // A delayed first callback must not publish its already committed
            // candidate while the second native operation is pending.
            for (old_request, old_result) in &commits {
                assert!(
                    copy_completed(old_result)
                        .commit(&mut field.events.world, old_request)
                        .is_err()
                );
                frozen.assert_held(&field)?;
                assert!(request.is_pending());
            }
            let party = field
                .events
                .world
                .party
                .as_ref()
                .context("missing battle party")?
                .clone();
            let mut assets = encounter::Assets::load(
                &root,
                &retained,
                &menus,
                &party,
                request.setup,
                &mut cache,
                || false,
            )?;
            // Deterministic independent battle-clock inputs. These are fixture
            // seeds, not claimed to match a Dolphin presentation or OS clock.
            let world_music = field
                .events
                .memory()
                .read(0x50, symphonia_script::Width::S32)?;
            let story = field.story_progress()?;
            let options = || encounter::PrepareOptions {
                random_seed: if formation == 1 { 0x2345 } else { 0x3456 },
                map: 332,
                world_music,
                story,
                overlimit_boost: false,
            };
            let script = assets
                .inputs
                .files
                .bytes
                .remove("scripts/battle/normal_lloyd.sym")
                .context("missing maintained normal source")?;
            let failed = assets.prepare(&menus, &party, options(), &mut scripts, |sound| {
                bind_sound(&assets.audio, sound)
            });
            assert!(
                failed.is_err(),
                "missing normal source activated an encounter"
            );
            frozen.assert_held(&field)?;
            assert!(request.is_pending());
            assets
                .inputs
                .files
                .bytes
                .insert("scripts/battle/normal_lloyd.sym".into(), script);
            let prepared = assets.prepare(&menus, &party, options(), &mut scripts, |sound| {
                bind_sound(&assets.audio, sound)
            })?;
            frozen.assert_held(&field)?;
            let completed = fight(
                prepared,
                &assets.audio,
                &mut field,
                &frozen,
                party.clone(),
                data.clone(),
                menus.clone(),
                catalogue.clone(),
            )?;
            assert_eq!(completed.result, BattleResult::Victory);
            assert_eq!(completed.party.battles.total, initial_total + formation);
            assert_eq!(completed.party.battles.previous_formation, Some(formation));
            for &character in party.formation.iter().take(4) {
                let index = usize::from(character - 1);
                assert_eq!(
                    completed.party.battles.participation[index],
                    party.battles.participation[index] + 1
                );
            }
            let expected_party = serde_json::to_value(&completed.party)?;
            let expected_random = completed.libc_seed;
            let duplicate = copy_completed(&completed);
            let retained_result = copy_completed(&completed);
            completed.commit(&mut field.events.world, &request)?;
            assert!(!request.is_pending());
            assert_eq!(
                serde_json::to_value(field.events.world.party.as_ref().unwrap())?,
                expected_party
            );
            assert_eq!(field.events.world.random_state, expected_random);
            assert!(duplicate.commit(&mut field.events.world, &request).is_err());
            assert_eq!(
                serde_json::to_value(field.events.world.party.as_ref().unwrap())?,
                expected_party
            );
            assert_eq!(field.events.world.random_state, expected_random);
            eprintln!(
                "opening formation {formation} committed: {}",
                serde_json::json!({
                    "gald": retained_result.party.gald,
                    "battle_rng": retained_result.battle_random_state,
                    "field_rng": retained_result.libc_seed,
                    "members": retained_result.party.members.iter().take(3).map(|m| serde_json::json!({
                        "level":m.level,"experience":m.experience,"hp":m.hp,"tp":m.tp,
                        "techniques":m.techniques,"titles":m.titles
                    })).collect::<Vec<_>>()
                })
            );
            commits.push((request, retained_result));
        }
        if commits.len() == 2 && field.player_has_control() {
            assert_eq!(field.map_id, 332);
            assert_eq!(field.story_progress()?, 3000);
            assert!(!field.events.battle_pending());
            assert!(field.events.world.battle_request.is_none());
            assert_ne!(commits[0].0.id(), commits[1].0.id());
            assert_eq!(
                field.events.world.party.as_ref().unwrap().battles.total,
                initial_total + 2
            );
            return Ok(());
        }
        let interact = tick % 10 == 0
            && field
                .dialogue
                .values()
                .any(|page| !page.closed && !page.persistent && page.fully_revealed());
        field.step(FieldInput {
            interact,
            accelerate_dialogue: true,
            ..Default::default()
        })?;
        field.events.world.audio_commands.clear();
    }
    anyhow::bail!(
        "opening event stalled after {} committed combats",
        commits.len()
    )
}

fn opening_field(
    files: &Files,
    menus: Arc<MenuData>,
    data: Arc<SessionData>,
) -> Result<FieldSession> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/battle-rewrite/stage3-opening-checkpoint.json");
    let mut checkpoint: FieldCheckpoint = serde_json::from_slice(&std::fs::read(path)?)?;
    // This paired fixture omits battle history. Its pinned original savestate
    // cde34b48… contains previous formation 0 (entry-voice-history-01 audit).
    // Supplement this observation only here; arbitrary legacy saves stay unknown.
    assert_eq!(checkpoint.progress.party.battles.previous_formation, None);
    checkpoint.progress.party.battles.previous_formation = Some(0);
    assert_eq!(checkpoint.map_id, 332);
    let assets: FieldAssets = files.json("fields/map-332.json")?;
    let mut entry = checkpoint.entry(&assets, data, [330, 331, 332, 340].into())?;
    entry.menu_data = Some(menus);
    entry.text = Arc::new(files.json("game/text.json")?);
    let mut field = FieldSession::enter(
        &files.read(&assets.script.path)?,
        files.json(&assets.messages)?,
        &assets,
        entry,
    )?;
    for _ in 0..120 {
        let controlled = field.player_has_control();
        field.step(FieldInput::default())?;
        field.events.world.audio_commands.clear();
        if controlled && field.player_has_control() {
            assert_eq!(field.story_progress()?, 2500);
            return Ok(field);
        }
    }
    anyhow::bail!("paired opening checkpoint did not initialize")
}

struct Frozen {
    progress: serde_json::Value,
    effect_tick: u32,
    actors: Vec<(i32, u64, [f32; 3])>,
}
impl Frozen {
    fn read(field: &FieldSession) -> Result<Self> {
        Ok(Self {
            progress: serde_json::to_value(field.events.save_progress()?)?,
            effect_tick: field.effect_clock.tick(),
            actors: field
                .events
                .world
                .actors
                .iter()
                .map(|(&id, actor)| (id, actor.instance, actor.position))
                .collect(),
        })
    }
    fn assert_held(&self, field: &FieldSession) -> Result<()> {
        assert_eq!(
            serde_json::to_value(field.events.save_progress()?)?,
            self.progress
        );
        assert_eq!(field.effect_clock.tick(), self.effect_tick);
        assert_eq!(
            field
                .events
                .world
                .actors
                .iter()
                .map(|(&id, actor)| (id, actor.instance, actor.position))
                .collect::<Vec<_>>(),
            self.actors
        );
        assert!(!field.player_has_control());
        assert!(field.checkpoint().is_err());
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)] // The retained field and its immutable candidate inputs stay separate.
fn fight(
    prepared: encounter::Prepared,
    audio: &Audio,
    field: &mut FieldSession,
    frozen: &Frozen,
    party: Party,
    data: Arc<SessionData>,
    menus: Arc<MenuData>,
    catalogue: Arc<arte::Catalogue>,
) -> Result<results::Completed> {
    let leader = prepared.actors[0].actor;
    let mut lifecycle = prepared.lifecycle.start()?;
    let mut battle = Battle::new(prepared.core);
    assert_eq!(battle.actors()[leader.index()].control, Control::SemiAuto);
    assert!(
        battle.actors()[1..3]
            .iter()
            .all(|actor| actor.control == Control::Auto)
    );
    let mut candidate = results::Candidate::new(
        prepared.results,
        party.clone(),
        field.events.world.random_state,
        data,
        menus,
        catalogue,
    )?;
    frozen.assert_held(field)?;
    let mut display = HeadlessAcknowledgements {
        audio,
        requested_music: None,
        tp_recovered: false,
        operations: BTreeMap::new(),
    };
    let mut voices: Vec<VoiceId> = Vec::new();
    let mut starts = [0u32; 3];
    let mut hit_count = 0;
    let mut attacked = [false; 3];
    for tick in 0..18_000u32 {
        // Mapped neutral A presses only. Approach, targeting, buffering and
        // companion decisions remain the production controls/AI's responsibility.
        let mut control = ControlInput::neutral(leader);
        control.attack = ButtonInput {
            held: tick % 8 < 2,
            pressed: tick % 8 == 0,
            released: tick % 8 == 2,
        };
        let frame = lifecycle.step(
            &mut battle,
            lifecycle::Input {
                battle: BattleInput {
                    controllers: vec![control],
                    voices_finished: std::mem::take(&mut voices),
                    ..Default::default()
                },
                confirm: tick % 12 == 0,
                ..Default::default()
            },
            &mut candidate.services(&mut display),
        )?;
        if frame.recognized_result.is_none() {
            for (slot, actor) in frame.actors.iter().take(3).enumerate() {
                attacked[slot] |= matches!(
                    actor.activity,
                    resonance_battle::Activity::Action { .. }
                        | resonance_battle::Activity::Casting { .. }
                );
            }
        }
        for cue in &frame.cues {
            match cue {
                Cue::Voice {
                    playback, sound, ..
                } => {
                    validate_binding(audio, *sound)?;
                    voices.push(*playback);
                }
                Cue::VoiceStopped { playback } => voices.retain(|voice| voice != playback),
                Cue::Started { actor, .. } if actor.index() < 3 => starts[actor.index()] += 1,
                Cue::Hit { actor, result, .. } if actor.index() >= 3 && result.hp_change < 0 => {
                    hit_count += 1
                }
                Cue::Sound { sound, .. } => validate_binding(audio, *sound)?,
                _ => {}
            }
        }
        // Even aggressive field input cannot run the suspended caller; the
        // host separately counts the one presented battle visit as play time.
        field.step(FieldInput {
            interact: true,
            menu: true,
            direction: [1., 1.],
            ..Default::default()
        })?;
        field.play_time.advance();
        if tick % 60 == 0 {
            frozen.assert_held(field)?;
        }
        if let Some(outcome) = &frame.outcome {
            ensure!(
                outcome.result == BattleResult::Victory,
                "ordinary opening inputs ended with {:?}; vitals={:?}",
                outcome.result,
                battle
                    .actors()
                    .iter()
                    .map(|a| (a.hp, a.tp))
                    .collect::<Vec<_>>()
            );
            frozen.assert_held(field)?;
            assert!(
                hit_count > 0 && attacked.into_iter().all(|attack| attack),
                "real attacks were not observed: {starts:?}, hits={hit_count}"
            );
            for operation in ["select", "rewards", "tp", "perform", "accept"] {
                assert_eq!(
                    display.operations.get(operation),
                    Some(&1),
                    "operation {operation}"
                );
            }
            let rewards = candidate.results().context("victory has no result state")?;
            let expected_gald = party
                .gald
                .saturating_add(rewards.rewards.gald)
                .min(99_999_999);
            let completed = candidate.finish(&battle, outcome)?;
            assert_eq!(completed.party.gald, expected_gald);
            assert_eq!(completed.battle_random_state, battle.random_state());
            return Ok(completed);
        }
    }
    anyhow::bail!(
        "opening combat/lifecycle stalled: phase={:?}, vitals={:?}, starts={starts:?}, operations={:?}",
        battle.phase(),
        battle
            .actors()
            .iter()
            .map(|a| (a.hp, a.tp, a.activity))
            .collect::<Vec<_>>(),
        display.operations
    )
}

/// No rasterizer, synthesis or guessed voice duration. Only already prepared
/// resource readiness is acknowledged; voices complete on the next test visit.
/// Result cards are acknowledged after the real source age150 TP operation so
/// the test cannot shortcut required candidate mutations by confirming early.
struct HeadlessAcknowledgements<'a> {
    audio: &'a Audio,
    requested_music: Option<u16>,
    tp_recovered: bool,
    operations: BTreeMap<&'static str, u32>,
}
impl results::Presentation for HeadlessAcknowledgements<'_> {
    fn observations(&self) -> lifecycle::Observations {
        lifecycle::Observations {
            music_ready: self.requested_music.is_some(),
            performance_ready: true,
            results_ready: self.tp_recovered,
            ..Default::default()
        }
    }
    fn request(
        &mut self,
        kind: lifecycle::RequestKind,
        _selection: Option<&results::Selection>,
        result: Option<&results::Results>,
        _battle: &mut Battle,
    ) -> Result<Vec<Cue>> {
        use lifecycle::RequestKind::*;
        let key = match kind {
            SelectVictory { .. } => Some("select"),
            ConstructRewards => {
                ensure!(result.is_some(), "unconstructed rewards acknowledged");
                Some("rewards")
            }
            RecoverTp => {
                self.tp_recovered = true;
                Some("tp")
            }
            PerformVictory => Some("perform"),
            AcceptVictory => Some("accept"),
            RequestMusic { track } => {
                ensure!(
                    self.audio.assets.music.contains_key(&i16::try_from(track)?),
                    "unprepared lifecycle music {track}"
                );
                self.requested_music = Some(track);
                None
            }
            PlayMusic { track, .. } => {
                ensure!(
                    self.requested_music == Some(track),
                    "unrequested lifecycle music"
                );
                None
            }
            DefeatNotice | EscapeNotice | RecordEscape => {
                anyhow::bail!("opening integration reached nonvictory lifecycle {kind:?}")
            }
            _ => None,
        };
        if let Some(key) = key {
            *self.operations.entry(key).or_default() += 1;
        }
        Ok(vec![])
    }
}
fn bind_sound(audio: &Audio, sound: Sound) -> Result<SoundBinding> {
    let binding = match sound {
        Sound::Cue(index) => SoundBinding { resource: 0, index },
        Sound::Stream(index) => SoundBinding { resource: 1, index },
    };
    validate_binding(audio, binding)?;
    Ok(binding)
}
fn validate_binding(audio: &Audio, sound: SoundBinding) -> Result<()> {
    ensure!(
        match sound.resource {
            0 => audio
                .assets
                .sounds
                .contains_key(&i16::try_from(sound.index)?),
            1 => audio.assets.voices.contains_key(&u32::from(sound.index)),
            _ => false,
        },
        "unprepared integration sound {sound:?}"
    );
    Ok(())
}
fn copy_completed(completed: &results::Completed) -> results::Completed {
    results::Completed {
        party: completed.party.clone(),
        libc_seed: completed.libc_seed,
        result: completed.result,
        battle_random_state: completed.battle_random_state,
    }
}
