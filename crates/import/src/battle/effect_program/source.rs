//! Test helper for collecting the production event decoder output.
use super::*;

pub(crate) use super::events::{Command, Event};

pub(crate) fn program(bytes: &[u8], id: u8) -> Result<Vec<Event>> {
    let mut rows = super::events::Timeline::new(program_timeline(bytes, id)?);
    let mut events = Vec::new();
    for _ in 0..RECORD_LIMIT {
        let event = rows.next()?;
        ensure!(
            !matches!(event.command, Command::Repeat { .. }),
            "nested effect repeat command"
        );
        if let Command::Emit { actor, .. } = event.command {
            let actors = usize::from(half(bytes, 12)? - half(bytes, 8)?) / ACTOR_BYTES;
            ensure!(
                usize::from(actor) < actors,
                "effect emission references an absent actor"
            );
        }
        let end = event.repeat.is_none() && matches!(event.command, Command::End { .. });
        events.push(event);
        if end {
            return Ok(events);
        }
    }
    bail!("effect program exceeds record limit")
}
