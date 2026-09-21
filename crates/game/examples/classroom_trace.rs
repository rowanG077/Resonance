//! Device-free development probe of the original classroom event entry.
use anyhow::Result;
use std::{fs, path::Path};
fn main() -> Result<()> {
    let root = Path::new("local/cooked");
    let field: resonance_content::field::FieldAssets =
        serde_json::from_slice(&fs::read(root.join("fields/map-340.json"))?)?;
    let messages = serde_json::from_slice(&fs::read(root.join(&field.messages))?)?;
    if let Some(pc) = std::env::args().nth(1).filter(|a| !a.starts_with("--")) {
        let start = u32::from_str_radix(pc.trim_start_matches("0x"), 16)?;
        let program = symphonia_script::Program::decode(&fs::read(root.join(&field.script.path))?)?;
        for pc in start..start + 200 {
            if let Some((op, _)) = program.instruction(pc) {
                println!("{pc:04x} {op:?}");
            }
        }
        return Ok(());
    }
    let mut events =
        resonance_game::field::start(&fs::read(root.join(&field.script.path))?, messages, &field)?;
    let automatic = std::env::args().any(|a| a == "--auto");
    let interaction = std::env::args().any(|a| a == "--interact");
    let mut interacted = false;
    let mut seen = std::collections::BTreeSet::new();
    for update in 0..if automatic { 20000 } else { 100 } {
        if update % 100 == 0 {
            println!(
                "tick={} actors={} dialogue={} movie={:?} input={}",
                events.tick(),
                events.world.actors.len(),
                events.world.dialogue.len(),
                events.world.movie.as_ref().map(|m| m.resource),
                events.world.input_enabled
            );
        }
        if automatic {
            if let Some(movie) = &events.world.movie
                && movie.operation.is_pending()
            {
                println!("probe completes movie {} without playback", movie.resource);
                movie.operation.complete(None).map_err(anyhow::Error::msg)?;
            }
            for dialogue in events.world.dialogue.values() {
                if seen.insert(dialogue.operation.id()) {
                    println!("dialogue @{}: {:?}", events.tick(), dialogue.body.tokens);
                    if let Some(c) = &events.world.field_camera {
                        println!(
                            "camera position={:?} target={:?} fov={}",
                            c.position,
                            c.target,
                            c.fov_degrees()
                        );
                    }
                    dialogue.operation.advance(1).map_err(anyhow::Error::msg)?;
                } else if dialogue.operation.is_pending() && update % 120 == 0 {
                    dialogue
                        .operation
                        .complete(None)
                        .map_err(anyhow::Error::msg)?;
                }
            }
            for command in events.world.audio_commands.drain(..) {
                println!("audio @{}: {command:?}", update);
            }
            if events.world.input_enabled {
                if interaction && !interacted {
                    anyhow::ensure!(events.interact(305)?, "interaction was not started");
                    interacted = true;
                    println!("probe starts NPC 305 interaction without movement");
                    events.step()?;
                    continue;
                }
                println!("control handed to player at tick {}", events.tick());
                println!(
                    "progression={} camera={:?} triggers={:?}",
                    events.memory().read(0x40, symphonia_script::Width::S32)?,
                    events.world.field_camera,
                    events.world.triggers
                );
                for (id, actor) in &events.world.actors {
                    println!(
                        "actor {id}: position={:?} heading={} resource={:#x}",
                        actor.position, actor.heading, actor.resource
                    );
                }
                return Ok(());
            }
        }
        events.step()?;
    }
    Ok(())
}
