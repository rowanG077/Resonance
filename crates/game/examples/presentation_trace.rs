//! Silent visual-request inventory using ordinary dialogue reveal/advance.
use anyhow::{Result, ensure};
use resonance_game::field::replay::InputReplay;
use resonance_game::field::{FieldInput, FieldSession};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn main() -> Result<()> {
    let root = Path::new("local/cooked");
    let assets: resonance_content::field::FieldAssets =
        serde_json::from_slice(&fs::read(root.join("fields/iselia-classroom.json"))?)?;
    let messages = serde_json::from_slice(&fs::read(root.join(&assets.messages))?)?;
    let data: std::sync::Arc<resonance_content::session::SessionData> = std::sync::Arc::new(
        serde_json::from_slice(&fs::read(root.join("game/session-data.json"))?)?,
    );
    let mut session = FieldSession::enter(
        &fs::read(root.join(&assets.script.path))?,
        messages,
        &assets,
        resonance_game::field::FieldEntry {
            persistent: resonance_events::PersistentState {
                party: Some(resonance_events::party::Party::new(
                    &data,
                    Default::default(),
                )?),
                ..Default::default()
            },
            data: Some(data),
            ..Default::default()
        },
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
    let hold = std::env::args()
        .nth(1)
        .map(|s| s.parse::<u32>())
        .transpose()?
        .unwrap_or(180);
    for tick in 0..30000 {
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
        let interact = replay_started
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
                return Ok(());
            }
        }
    }
    ensure!(false, "classroom never handed control to the player");
    Ok(())
}
