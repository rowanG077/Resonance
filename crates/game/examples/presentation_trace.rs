//! Silent visual-request inventory using ordinary dialogue reveal/advance.
use anyhow::{Result, ensure};
use resonance_game::field::replay::InputReplay;
use resonance_game::field::{FieldCheckpoint, FieldEntry, FieldInput, FieldSession};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn main() -> Result<()> {
    let root = Path::new("local/cooked");
    let argument = |name| -> Result<Option<String>> {
        std::env::args()
            .position(|a| a == name)
            .map(|i| {
                std::env::args()
                    .nth(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("{name} requires a value"))
            })
            .transpose()
    };
    // This diagnostic accepts a raw checkpoint or a persistence envelope. The
    // player owns save identity validation; the trace exercises field entry.
    let checkpoint: Option<FieldCheckpoint> = argument("--load")?
        .map(|path| -> Result<_> {
            let json: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
            Ok(serde_json::from_value(
                json.get("state").unwrap_or(&json).clone(),
            )?)
        })
        .transpose()?;
    let field_path = |map| match map {
        340 => "fields/iselia-classroom.json".to_owned(),
        _ => format!("fields/map-{map}.json"),
    };
    let assets: resonance_content::field::FieldAssets = serde_json::from_slice(&fs::read(
        root.join(field_path(checkpoint.as_ref().map_or(340, |c| c.map_id))),
    )?)?;
    let messages = serde_json::from_slice(&fs::read(root.join(&assets.messages))?)?;
    let data: std::sync::Arc<resonance_content::session::SessionData> = std::sync::Arc::new(
        serde_json::from_slice(&fs::read(root.join("game/session-data.json"))?)?,
    );
    let entry = if let Some(checkpoint) = checkpoint {
        checkpoint.entry(&assets, data.clone(), [330, 332, 340].into())?
    } else {
        FieldEntry {
            persistent: resonance_events::PersistentState {
                party: Some(resonance_events::party::Party::new(
                    &data,
                    Default::default(),
                )?),
                ..Default::default()
            },
            data: Some(data.clone()),
            available_fields: [330, 332, 340].into(),
            ..Default::default()
        }
    };
    let mut session = FieldSession::enter(
        &fs::read(root.join(&assets.script.path))?,
        messages,
        &assets,
        entry,
    )?;
    let mut pages = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let exit = std::env::args().any(|a| a == "--exit");
    let stay = std::env::args().any(|a| a == "--stay");
    let actor_trace = std::env::args()
        .position(|a| a == "--actor")
        .map(|index| {
            std::env::args()
                .nth(index + 1)
                .ok_or_else(|| anyhow::anyhow!("--actor requires an actor id"))?
                .parse::<i32>()
                .map_err(anyhow::Error::from)
        })
        .transpose()?;
    let replay: Option<InputReplay> = std::env::args()
        .position(|a| a == "--replay")
        .map(|index| -> Result<_> {
            let path = std::env::args()
                .nth(index + 1)
                .ok_or_else(|| anyhow::anyhow!("--replay requires a JSON path"))?;
            Ok(serde_json::from_slice(&fs::read(path)?)?)
        })
        .transpose()?;
    if let Some(replay) = &replay {
        replay.validate()?;
    }
    let mut replay_started = None;
    let mut control = None;
    let mut exit_started = false;
    let follow = std::env::args().any(|a| a == "--follow");
    let output = argument("--output")?;
    let confirmed = argument("--cross")?.is_none();
    let mut trigger = argument(if confirmed { "--trigger" } else { "--cross" })?
        .map(|s| s.parse::<u32>())
        .transpose()?;
    let hold = std::env::args()
        .nth(1)
        .filter(|s| !s.starts_with('-'))
        .map(|s| s.parse::<u32>())
        .transpose()?
        .unwrap_or(180);
    for tick in 0..30000 {
        if trigger.is_some() && session.checkpoint().is_ok() {
            let key = trigger.take().unwrap();
            ensure!(
                session.events.trigger(key, confirmed)?,
                "field trigger {key} is unavailable"
            );
        }
        if let Some(movie) = &session.events.world.movie
            && movie.operation.is_pending()
        {
            // The inventory does not render or decode a movie.
            movie.operation.complete(None).map_err(anyhow::Error::msg)?;
        }
        if replay_started.is_none()
            && replay
                .as_ref()
                .is_some_and(|replay| replay.matches(&session))
        {
            replay_started = Some(session.events.tick());
            println!("input replay anchored at {}", session.events.tick());
        }
        let mut interact = replay_started
            .and_then(|start| {
                replay
                    .as_ref()
                    .unwrap()
                    .accept_at(session.events.tick() + 1 - start)
            })
            .unwrap_or_else(|| {
                session.dialogue.values().any(|p| {
                    if p.closed || p.persistent || p.visible != p.current().glyphs.len() {
                        return false;
                    }
                    tick - *pages.entry((p.operation.id(), p.page)).or_insert(tick) >= hold
                })
            });
        if exit && session.events.world.input_enabled && tick % 30 == 0 {
            interact = true;
        }
        let choosing = stay
            && session
                .events
                .world
                .choices
                .values()
                .any(|c| c.operation.is_pending() && c.selected_line < c.last_line);
        let direction = if choosing {
            [0., -1.]
        } else if exit && session.events.world.input_enabled {
            let age = tick - *control.get_or_insert(tick);
            if age < 138 {
                [-1., 0.]
            } else if age < 228 {
                [0., 1.]
            } else {
                [-1., 0.]
            }
        } else {
            [0., 0.]
        };
        session.step(FieldInput {
            interact: interact && !choosing,
            direction,
            ..Default::default()
        })?;
        let w = &mut session.events.world;
        if let Some(actor) = actor_trace.and_then(|id| w.actors.get(&id)) {
            println!(
                "actor_trace {}",
                serde_json::json!({
                    "tick":w.tick,"actor":actor_trace,"position":actor.position,
                    "heading":actor.heading,"target_heading":actor.target_heading,
                    "destination":actor.motion.as_ref().map(|m|m.target),
                    "speed":actor.motion.as_ref().map(|m|m.speed),
                    "animation":actor.animation.as_ref().map(|a|serde_json::json!({"slot":a.slot,"sample":a.sample(w.tick,0,a.duration_ticks as f32)}))
                })
            );
        }
        if control.is_some() && !w.input_enabled {
            exit_started = true;
        }
        if control.is_some() && tick % 30 == 0 {
            println!(
                "tick {tick}: controlled {:?}; input={}",
                w.actors[&w.controlled_actor].position, w.input_enabled
            );
        }
        for (&id, e) in &w.emotes {
            if seen.insert(format!("emote:{id}:{}", e.start_tick)) {
                println!("tick {tick}: emote {id}: {e:?}");
            }
        }
        for (&id, e) in &w.billboards {
            if seen.insert(format!("billboard:{id}")) {
                println!("tick {tick}: billboard {id}: {e:?}");
            }
        }
        for (index, point) in w.save_points.iter().enumerate() {
            if seen.insert(format!("save-point:{}:{index}", session.map_id)) {
                println!("tick {tick}: save-point {point:?}");
            }
        }
        for (&id, a) in &w.actors {
            if matches!(id, 2..=4 | 100)
                && let Some(animation) = &a.animation
                && seen.insert(format!("animation:{id}:{}", animation.start_tick))
            {
                println!("tick {tick}: actor {id} animation {animation:?}");
            }
            if let Some(attachment) = &a.attachment
                && seen.insert(format!("attachment:{id}:{attachment:?}"))
            {
                println!(
                    "tick {tick}: actor {id} resource {} attachment {attachment:?}",
                    a.resource
                );
            }
            for (&slot, adjustment) in &a.appearance.bone_adjustments {
                if seen.insert(format!("bone:{id}:{slot}:{}", adjustment.start_tick)) {
                    println!("tick {tick}: actor {id} bone {slot}: {adjustment:?}");
                }
            }
        }
        for (&slot, d) in &w.dialogue {
            if seen.insert(format!("dialogue:{}", d.operation.id())) {
                println!("tick {tick}: dialogue {slot}: {:?}", d.body.tokens);
            }
        }
        for command in w.audio_commands.drain(..) {
            println!("tick {tick}: audio {command:?}");
        }
        if let Some(transition) = w.field_transition.clone() {
            println!("tick {tick}: field transition {transition:?}");
            ensure!(
                follow,
                "transition requested; use --follow to enter the next field"
            );
            let assets: resonance_content::field::FieldAssets =
                serde_json::from_slice(&fs::read(root.join(field_path(transition.map)))?)?;
            let next = FieldSession::enter(
                &fs::read(root.join(&assets.script.path))?,
                serde_json::from_slice(&fs::read(root.join(&assets.messages))?)?,
                &assets,
                FieldEntry {
                    play_time: session.play_time,
                    persistent: session.events.persistent_state()?,
                    data: Some(data.clone()),
                    available_fields: [330, 332, 340].into(),
                    position: transition.position,
                    heading: transition.heading,
                    camera: transition.camera.clone(),
                    ..Default::default()
                },
            )?;
            session.events.cancel();
            session = next;
            continue;
        }
        let w = &session.events.world;
        if w.input_enabled && seen.insert(format!("control-camera:{}", session.map_id)) {
            println!(
                "field {} control camera {:?}",
                session.map_id,
                w.field_camera
                    .as_ref()
                    .map(|c| c.settings(w.controlled_actor))
            );
        }
        if trigger.is_some() {
            continue;
        }
        if w.input_enabled {
            println!(
                "tick {tick}: player control; render settings {:?}",
                w.render_settings
            );
            if !exit
                || exit_started
                    && session
                        .events
                        .memory()
                        .read(0x40, symphonia_script::Width::S32)?
                        != 1000
            {
                if let Some(path) = &output {
                    let Ok(checkpoint) = session.checkpoint() else {
                        continue;
                    };
                    fs::write(path, serde_json::to_vec_pretty(&checkpoint)?)?;
                }
                return Ok(());
            }
        }
    }
    ensure!(false, "field never handed control to the player");
    Ok(())
}
