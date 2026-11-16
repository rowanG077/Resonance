//! Opt-in, asset-backed filmstrip of the production renderer and finisher clock.
use super::*;
use crate::field_ui::resident::scenario::Scenario;
use bevy::time::TimeUpdateStrategy;
use resonance_battle::{BattleInput, FatalColor, FatalEnemyRules, FatalOutcome};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32},
    },
};
#[path = "fatal_preview_movie.rs"]
mod movie;

fn capture_revision() -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for source in [
        include_str!("fatal_preview.rs"),
        include_str!("fatal_preview_movie.rs"),
        include_str!("feedback.rs"),
        include_str!("../battle_view/fatal.rs"),
        include_str!("../battle_view.rs"),
        include_str!("../battle_view/stage_color.rs"),
        include_str!("../battle_ui/fatal_strike.rs"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../battle/src/model.rs"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../battle/src/model/player.rs"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../battle/src/fatal_strike/scene.rs"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../game/src/battle/fatal_strike.rs"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../game/src/battle/fatal_strike/mage.rs"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../game/src/battle/fatal_strike/rig.rs"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../game/src/battle/fatal_strike/weapon.rs"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../battle/src/model/fatal.rs"
        )),
    ] {
        hash.update(source.as_bytes());
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn stage(owner: &mut Owner, character: u8, color: FatalColor) -> Result<ActorId> {
    let Phase::Scene(scene) = &mut owner.phase else {
        anyhow::bail!("battle is not ready")
    };
    let core = &mut scene.core;
    core.suppress_automatic_decisions();
    let performer = core.actor_ids().next().context("missing performer")?;
    let target = core.target(performer).context("missing target")?;
    for id in core.actor_ids().collect::<Vec<_>>() {
        if core.actors()[id.index()].side == Side::Enemy {
            core.set_actor_vitals(id, 2000, 2000, 999, 999)?;
            core.set_fixture_neutral_control(id)?;
        } else {
            let actor = &core.actors()[id.index()];
            let (hp, tp) = (actor.equipment.max_hp, actor.equipment.max_tp);
            core.set_actor_vitals(id, hp, hp, tp, tp)?;
        }
    }
    // Entry may have admitted an enemy attack before neutral control was set.
    // Settle that fixture state before the recorded input begins.
    let interrupt = core
        .snapshot()
        .actions
        .iter()
        .filter(|(_, actor, _)| core.actors()[actor.index()].side == Side::Enemy)
        .map(|(action, _, _)| *action)
        .collect();
    let mut frame = core.step(BattleInput {
        interrupt,
        ..Default::default()
    })?;
    let clock = frame.clock;
    core.advance_models(&mut frame, clock, false)?;
    // An entry action can already be queued without appearing in actions yet.
    // Drain it before filming, using neutral input and the normal recovery path.
    for tick in 0..180 {
        let mut frame = core.step(BattleInput::default())?;
        let clock = frame.clock;
        core.advance_models(&mut frame, clock, false)?;
        if tick >= 20
            && core
                .actor_ids()
                .all(|actor| core.activity(actor) == resonance_battle::Activity::Idle)
        {
            break;
        }
    }
    let [x, _, z] = core.actors()[target.index()].position;
    let distance = if matches!(character, 2 | 3 | 4) {
        300.
    } else {
        60.
    };
    core.restore_fixture_placement(performer, [x - distance, 0., z], Some(90.))?;
    core.restore_fixture_target(performer, target)?;
    core.hold_event_boss_hp(None)?;
    core.configure_fatal_enemy(
        target,
        FatalEnemyRules {
            capacity: [1; 3],
            outcome: FatalOutcome::Defeat,
            vulnerability: std::array::from_fn(|i| if i == color.index() { 100 } else { 0 }),
        },
    )?;
    scene.published_tick = None;
    Ok(performer)
}

#[test]
#[ignore = "requires installed assets and GPU; writes a full cinematic recording"]
fn capture_fatal_strike_filmstrip() -> Result<()> {
    let output = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_FATAL_CAPTURE").context("set RESONANCE_FATAL_CAPTURE")?,
    );
    let character = std::env::var("RESONANCE_FATAL_CHARACTER")
        .unwrap_or_else(|_| "1".into())
        .parse()?;
    let color = match std::env::var("RESONANCE_FATAL_COLOR").as_deref() {
        Ok("red") => FatalColor::Red,
        Ok("green") => FatalColor::Green,
        _ => FatalColor::Blue,
    };
    let mut scenario = Scenario::boot("1280x720".parse().map_err(anyhow::Error::msg)?, |_| {})?;
    scenario.enter(332)?;
    record_variation(&mut scenario, character, color, &output, false, false)
}

#[test]
#[ignore = "requires installed assets, GPU and ffmpeg; records all 27 variations"]
fn capture_all_fatal_strike_variations() -> Result<()> {
    let output = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_FATAL_CAPTURE").context("set RESONANCE_FATAL_CAPTURE")?,
    );
    std::fs::create_dir_all(&output)?;
    let mut scenario = Scenario::boot("1280x720".parse().map_err(anyhow::Error::msg)?, |_| {})?;
    scenario.enter(332)?;
    let mut catalogue = Vec::new();
    let characters = std::env::var("RESONANCE_FATAL_CHARACTERS")
        .unwrap_or_else(|_| "1,2,3,4,5,6,7,8,9".into())
        .split(',')
        .map(str::parse::<u8>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let expected = characters.len() * 3;
    for character in characters {
        let name = format!("{:?}", Character::try_from(character)?);
        for color in FatalColor::ALL {
            let folder = format!(
                "{character:02}-{}-{}",
                name.to_ascii_lowercase(),
                format!("{color:?}").to_ascii_lowercase()
            );
            let clip = output.join(&folder);
            let current = std::fs::read(clip.join("flow.json"))
                .ok()
                .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok())
                .is_some_and(|flow| flow["capture_revision"] == capture_revision());
            if !current || !clip.join("video-silent.mp4").exists() {
                eprintln!("FATAL_VARIATION_START {character} {name} {color:?}");
                record_variation(&mut scenario, character, color, &clip, true, true)?;
                eprintln!("FATAL_VARIATION_READY {character} {name} {color:?}");
            }
            catalogue.push(serde_json::json!({ "character": character, "name": name, "color": format!("{color:?}"), "folder": folder }));
            std::fs::write(
                output.join("catalogue.json"),
                serde_json::to_vec_pretty(&catalogue)?,
            )?;
        }
    }
    ensure!(
        catalogue.len() == expected,
        "incomplete variation catalogue"
    );
    Ok(())
}

fn record_variation(
    scenario: &mut Scenario,
    character: u8,
    color: FatalColor,
    output: &Path,
    streaming: bool,
    compact: bool,
) -> Result<()> {
    std::fs::create_dir_all(output)?;
    {
        let mut session = scenario
            .app
            .world_mut()
            .resource_mut::<crate::new_game::Session>();
        let party = session
            .events_mut()
            .world
            .party
            .as_mut()
            .context("missing party")?;
        party.formation = vec![character];
        party.field_leader = character;
        party.settings.battle_controls = [1; 4];
        party.settings.preferences.fatal_strikes = true;
        party.settings.preferences.keymap = Default::default();
        party.settings.preferences.button_map = [0, 1, 2, 3, 4, 5, 6];
        party.settings.preferences.volumes.music = 96;
        party.settings.preferences.volumes.battle_effects = 115;
        party.settings.preferences.volumes.battle_voice = 115;
        for member in &mut party.members {
            member.arte_bindings.clear();
        }
        if matches!(character, 3 | 4) {
            // A caster's projectile earns any vulnerable formula through the
            // normal magic contact rules, without depending on melee reach.
            let member = usize::from(character - 1);
            let arte = if character == 3 { 66 } else { 113 };
            party.members[member].techniques.insert(arte);
            party
                .assign_arte_binding(
                    member,
                    arte,
                    resonance_content::input::InputChord(vec![
                        resonance_content::input::PhysicalInput::Key("KeyJ".into()),
                    ]),
                )
                .map_err(anyhow::Error::msg)?;
        }
    }
    let request = scenario.battle()?;
    scenario
        .app
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ));
    scenario.wait("battle entry did not reach combat", |app| {
        matches!(&app.world().resource::<Owner>().phase, Phase::Scene(scene) if scene.core.phase() == resonance_battle::BattlePhase::Combat)
    })?;
    stage(
        &mut scenario.app.world_mut().resource_mut::<Owner>(),
        character,
        color,
    )?;
    // Drain entry choreography through the complete host lifecycle as well as
    // the core. A queued entry attack can otherwise displace the staged pair.
    for _ in 0..180 {
        scenario.update()?;
    }
    let performer = stage(
        &mut scenario.app.world_mut().resource_mut::<Owner>(),
        character,
        color,
    )?;
    let target = match &scenario.app.world().resource::<Owner>().phase {
        Phase::Scene(scene) => scene.core.target(performer).context("missing victim")?,
        _ => anyhow::bail!("missing battle"),
    };
    let movie = streaming.then(|| movie::Movie::new(output)).transpose()?;
    let mut pcm = Vec::new();
    let mut opened_at = None;
    let mut activated_at = None;
    let mut impact_at = None;
    let mut completed_at = None;
    let mut events = Vec::new();
    let mut live_opening = None;
    let mut live_generation = None;
    let mut vanished = false;
    let pad = scenario.app.world_mut().spawn(Gamepad::default()).id();
    let every_frame = std::env::var_os("RESONANCE_FATAL_CAPTURE_EVERY_FRAME").is_some();
    let mut frames = 0;
    // Stored spells include their native chanting and scene-transition clocks.
    // Leave room for those to finish before filming the earned opening.
    for tick in 0_u64..1200 {
        {
            let mut keys = scenario
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>();
            keys.release_all();
            if matches!(character, 3 | 4)
                && live_opening.is_none()
                && activated_at.is_none()
                && tick >= 40
                && (tick - 40) % 120 == 0
            {
                keys.press(KeyCode::KeyJ);
            }
        }
        {
            let mut controller = scenario
                .app
                .world_mut()
                .get_mut::<Gamepad>(pad)
                .context("missing capture controller")?;
            controller.digital_mut().reset_all();
            controller.analog_mut().set(GamepadAxis::LeftStickX, 0.);
            controller.analog_mut().set(GamepadAxis::LeftStickY, 0.);
            if !matches!(character, 3 | 4)
                && live_opening.is_none()
                && activated_at.is_none()
                && tick >= 40
            {
                if (tick - 40) % 24 != 23 {
                    controller.digital_mut().press(GamepadButton::South);
                }
                // Semi-auto approaches keep every character's own weapon reach;
                // full directional input selects the matching normal attack.
                let (axis, value) = match color {
                    FatalColor::Blue => (GamepadAxis::LeftStickY, 1.),
                    FatalColor::Red => (GamepadAxis::LeftStickX, 1.),
                    FatalColor::Green => (GamepadAxis::LeftStickY, -1.),
                };
                controller.analog_mut().set(axis, value);
            }
            // A single press can occur during a native landing/return state.
            // Retry ordinary button taps within the live window, and earn a
            // fresh formula if it expires before admission becomes possible.
            if activated_at.is_none()
                && live_opening.is_some_and(|at| tick >= at + 42 && (tick - at - 42) % 2 == 0)
            {
                controller.digital_mut().press(GamepadButton::RightTrigger2);
            }
        }
        scenario
            .app
            .insert_resource(TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP,
            ));
        if let Some(movie) = &movie {
            movie.request(&mut scenario.app, tick);
        }
        scenario.update_recorded(&mut pcm, tick)?;
        if tick.is_multiple_of(120) {
            let owner = scenario.app.world().resource::<Owner>();
            if let Phase::Scene(scene) = &owner.phase {
                eprintln!(
                    "FATAL_CAPTURE_TICK {character} {color:?} {tick} {:?} hp={} target_hp={}",
                    scene.core.activity(performer),
                    scene.core.actors()[performer.index()].hp,
                    scene.core.actors()[target.index()].hp
                );
            }
            std::fs::write(
                output.join("state-latest.json"),
                serde_json::to_vec(&owner.diagnostic())?,
            )?;
        }
        {
            let owner = scenario.app.world().resource::<Owner>();
            let Phase::Scene(scene) = &owner.phase else {
                anyhow::bail!("battle retired before the full capture")
            };
            let frame = scene.frame.as_ref().context("missing presented battle")?;
            let opening = frame
                .fatal_strikes
                .as_ref()
                .and_then(|fatal| fatal.targets.iter().find(|entry| entry.actor == target))
                .and_then(|target| target.opening);
            if let Some(opening) = opening {
                if live_generation != Some(opening.generation) {
                    live_generation = Some(opening.generation);
                    live_opening = Some(tick);
                    if activated_at.is_none() {
                        opened_at = Some(tick);
                    }
                }
            } else {
                live_opening = None;
                live_generation = None;
            }
            if frame
                .fatal_strikes
                .as_ref()
                .is_some_and(|fatal| fatal.scene.is_some())
            {
                activated_at.get_or_insert(tick);
            }
            if scene
                .core
                .ledger()
                .fatal_strikes
                .iter()
                .any(|success| success.target == target && success.defeated)
            {
                impact_at.get_or_insert(tick);
                ensure!(
                    scene.core.actors()[target.index()].hp == 0,
                    "finisher did not kill its victim"
                );
            }
            if activated_at.is_some()
                && frame
                    .fatal_strikes
                    .as_ref()
                    .is_none_or(|fatal| fatal.scene.is_none())
            {
                completed_at.get_or_insert(tick);
            }
            vanished |= impact_at.is_some()
                && frame
                    .models
                    .iter()
                    .any(|model| model.actor == target && !model.visible);
            for cue in &frame.cues {
                if matches!(
                    cue,
                    resonance_battle::Cue::Sound { .. }
                        | resonance_battle::Cue::GlobalSound { .. }
                        | resonance_battle::Cue::Voice { .. }
                        | resonance_battle::Cue::FatalOpening { .. }
                        | resonance_battle::Cue::FatalImpact(_)
                ) {
                    events.push(serde_json::json!({ "frame": tick, "cue": format!("{cue:?}") }));
                }
            }
        }
        if !streaming && (every_frame || tick.is_multiple_of(12)) {
            crate::new_game_capture::screenshot_held(
                &mut scenario.app,
                output.join(format!("frame.{tick:04}.png")),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicU32::new(0)),
            )?;
        }
        frames = tick + 1;
        let voices_finished = scenario
            .app
            .world()
            .resource::<crate::battle_audio::Playback>()
            .busy_voices()
            == resonance_battle::ActorSet::default();
        if completed_at.is_some_and(|at| tick >= at + if compact { 20 } else { 180 })
            && voices_finished
        {
            break;
        }
    }
    ensure!(
        opened_at.is_some()
            && activated_at.is_some()
            && impact_at.is_some()
            && completed_at.is_some()
            && vanished,
        "incomplete fatal-strike flow: opening={opened_at:?}, activation={activated_at:?}, kill={impact_at:?}, completion={completed_at:?}, vanished={vanished}"
    );
    ensure!(
        events.iter().any(|event| event["cue"]
            .as_str()
            .is_some_and(|cue| cue.starts_with("Voice"))),
        "recording has no battle voices"
    );
    ensure!(
        pcm.len() == (frames * u64::from(resonance_playback::SOURCE_RATE) / 60 * 2) as usize,
        "audio/video durations differ"
    );
    if let Some(movie) = movie {
        movie.finish(scenario, frames)?;
    }
    let mut wave = hound::WavWriter::create(
        output.join("audio.wav"),
        hound::WavSpec {
            channels: 2,
            sample_rate: resonance_playback::SOURCE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut peak = 0_f32;
    for sample in pcm {
        peak = peak.max(sample.abs());
        wave.write_sample((sample * 32768.).round().clamp(-32768., 32767.) as i16)?;
    }
    wave.finalize()?;
    ensure!(peak > 0.02, "recorded audio is silent");
    std::fs::write(
        output.join("flow.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({ "capture_revision":capture_revision(), "character":character,"color":format!("{color:?}"), "frames": frames, "fps": 60, "sample_rate": resonance_playback::SOURCE_RATE, "opened_at": opened_at, "activated_at": activated_at, "impact_at": impact_at, "completed_at": completed_at, "enemy_vanished": vanished, "audio_peak": peak, "events": events }),
        )?,
    )?;
    scenario.app.world_mut().despawn(pad);
    scenario.return_from_battle(request)?;
    Ok(())
}
