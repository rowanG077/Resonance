//! Editable repetitions expand once during preparation. Runtime schedules keep
//! their original order and absolute sample/particle references.
use crate::battle_effect::{EffectOperation, ScheduledEvent};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

const MAX_EVENTS: usize = 65_536;
fn one() -> u32 {
    1
}
fn is_one(value: &u32) -> bool {
    *value == 1
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum Entry<P = u8> {
    Group(Group<P>),
    Event(Event<P>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Group<P = u8> {
    #[serde(default, skip_serializing_if = "is_zero")]
    at: u32,
    repeat: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    every: u32,
    events: Vec<Event<P>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Event<P = u8> {
    #[serde(default, skip_serializing_if = "is_zero")]
    at: u32,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    repeat: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    every: u32,
    #[serde(flatten)]
    operation: EffectOperation<P>,
}

impl<P> Entry<P> {
    fn map_particle<T>(self, map: &impl Fn(P) -> Result<T>) -> Result<Entry<T>> {
        let event = |e: Event<P>| -> Result<Event<T>> {
            Ok(Event {
                at: e.at,
                repeat: e.repeat,
                every: e.every,
                operation: e.operation.map_particle(map)?,
            })
        };
        Ok(match self {
            Self::Event(e) => Entry::Event(event(e)?),
            Self::Group(g) => Entry::Group(Group {
                at: g.at,
                repeat: g.repeat,
                every: g.every,
                events: g.events.into_iter().map(event).collect::<Result<_>>()?,
            }),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Programs(Vec<Vec<Entry<String>>>);
impl Programs {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn from_runtime(programs: &[Vec<ScheduledEvent>], names: &[String]) -> Result<Self> {
        let mut result = Vec::new();
        for program in programs {
            let events = localize(program)?;
            result.push(
                compact(&events)
                    .into_iter()
                    .map(|entry| {
                        entry.map_particle(&|id: u8| {
                            names
                                .get(usize::from(id))
                                .cloned()
                                .context("effect birth has no template")
                        })
                    })
                    .collect::<Result<_>>()?,
            );
        }
        Ok(Self(result))
    }
    pub fn prepare(self, names: &[String]) -> Result<Vec<Vec<ScheduledEvent>>> {
        self.0
            .into_iter()
            .map(|entries| {
                let entries = entries
                    .into_iter()
                    .map(|e| {
                        e.map_particle(&|name: String| {
                            let index = names
                                .iter()
                                .position(|n| *n == name)
                                .with_context(|| format!("unknown effect template {name}"))?;
                            Ok(u8::try_from(index)?)
                        })
                    })
                    .collect::<Result<_>>()?;
                expand(entries)
            })
            .collect()
    }
}

fn localize(program: &[ScheduledEvent]) -> Result<Vec<ScheduledEvent>> {
    ensure!(
        program.len() <= MAX_EVENTS,
        "effect program has too many events"
    );
    ensure!(
        program.windows(2).all(|p| p[0].at <= p[1].at),
        "unordered effect program"
    );
    let mut events = program.to_vec();
    let mut counters = Counters::default();
    for event in &mut events {
        counters.references(&mut event.operation, true)?;
    }
    Ok(events)
}

#[derive(Default)]
struct Counters {
    draws: u32,
    spawns: usize,
}

impl Counters {
    fn references(&mut self, operation: &mut EffectOperation, localize: bool) -> Result<()> {
        let previous = |spawn: &mut usize| -> Result<()> {
            ensure!(
                !localize || *spawn < self.spawns,
                "particle target is not an earlier birth"
            );
            ensure!(localize || *spawn != 0, "previous_spawn must be at least 1");
            *spawn = self
                .spawns
                .checked_sub(*spawn)
                .context("particle target precedes this program")?;
            Ok(())
        };
        match operation {
            EffectOperation::Spawn { birth, .. } => {
                let end = self
                    .draws
                    .checked_add(birth.random_draws)
                    .context("too many effect random samples")?;
                let base = i64::from(self.draws);
                birth.map_draws(|draw| {
                    let absolute = if localize {
                        draw
                    } else {
                        base.checked_add(draw)
                            .context("effect random sample overflow")?
                    };
                    ensure!(
                        absolute >= 0 && absolute < i64::from(end),
                        "unbound effect random sample"
                    );
                    Ok(if localize { absolute - base } else { absolute })
                })?;
                if let Some(parent) = &mut birth.model_parent {
                    previous(&mut parent.spawn)?;
                }
                self.draws = end;
                self.spawns += 1;
            }
            EffectOperation::SetMotion { spawn, .. } => previous(spawn)?,
            _ => {}
        }
        Ok(())
    }
}

fn tick(at: u32, iteration: u32, every: u32) -> Result<u32> {
    every
        .checked_mul(iteration)
        .and_then(|offset| at.checked_add(offset))
        .context("effect repetition exceeds the timeline")
}

fn append(event: &Event, base: u32, output: &mut Vec<ScheduledEvent>) -> Result<()> {
    ensure!(event.repeat > 0, "effect repeat must be positive");
    ensure!(
        output.len() + event.repeat as usize <= MAX_EVENTS,
        "effect program has too many events"
    );
    let at = base.checked_add(event.at).context("effect time overflow")?;
    for iteration in 0..event.repeat {
        output.push(ScheduledEvent {
            at: tick(at, iteration, event.every)?,
            operation: event.operation.clone(),
        });
    }
    Ok(())
}

pub(super) fn expand(entries: Vec<Entry>) -> Result<Vec<ScheduledEvent>> {
    let mut events = Vec::new();
    for entry in entries {
        match entry {
            Entry::Event(event) => append(&event, 0, &mut events)?,
            Entry::Group(group) => {
                ensure!(
                    group.repeat > 0 && !group.events.is_empty(),
                    "empty effect repetition"
                );
                let per_iteration = group
                    .events
                    .iter()
                    .try_fold(0u64, |sum, event| sum.checked_add(u64::from(event.repeat)))
                    .context("effect repetition is too large")?;
                let total = per_iteration
                    .checked_mul(u64::from(group.repeat))
                    .context("effect repetition is too large")?;
                ensure!(
                    total <= (MAX_EVENTS - events.len()) as u64,
                    "effect program has too many events"
                );
                for iteration in 0..group.repeat {
                    let at = tick(group.at, iteration, group.every)?;
                    for event in &group.events {
                        append(event, at, &mut events)?;
                    }
                }
            }
        }
    }
    // Stable sorting keeps authored order for simultaneous events, including
    // bursts and overlapping repetitions. References follow this final order.
    events.sort_by_key(|event| event.at);
    let mut counters = Counters::default();
    for event in &mut events {
        counters.references(&mut event.operation, false)?;
    }
    Ok(events)
}

/// Find adjacent, equal blocks with evenly shifted times. This is writer-only;
/// loading never searches for patterns or examines unrelated asset files.
fn compact(events: &[ScheduledEvent]) -> Vec<Entry> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < events.len() {
        let (mut length, mut count, mut interval) = (1, 1, 0);
        let mut saving = 0;
        for candidate in 1..=(events.len() - start) / 2 {
            if events[start].operation != events[start + candidate].operation {
                continue;
            }
            let every = events[start + candidate].at - events[start].at;
            let pattern = &events[start..start + candidate];
            let mut repetitions = 1;
            while start + (repetitions + 1) * candidate <= events.len() {
                let next =
                    &events[start + repetitions * candidate..start + (repetitions + 1) * candidate];
                if !pattern.iter().zip(next).all(|(a, b)| {
                    a.operation == b.operation
                        && u64::from(a.at) + repetitions as u64 * u64::from(every)
                            == u64::from(b.at)
                }) {
                    break;
                }
                repetitions += 1;
            }
            let removed = (repetitions - 1) * candidate;
            if removed > saving {
                (length, count, interval, saving) = (candidate, repetitions, every, removed);
            }
        }
        let first = &events[start];
        if length == 1 {
            result.push(Entry::Event(Event {
                at: first.at,
                repeat: count as u32,
                every: interval,
                operation: first.operation.clone(),
            }));
        } else {
            result.push(Entry::Group(Group {
                at: first.at,
                repeat: count as u32,
                every: interval,
                events: burst_runs(&events[start..start + length], first.at),
            }));
        }
        start += length * count;
    }
    result
}

/// Keep repeated operations inside a block concise, without nested groups.
fn burst_runs(events: &[ScheduledEvent], base: u32) -> Vec<Event> {
    let mut result = Vec::new();
    let mut index = 0;
    while index < events.len() {
        let event = &events[index];
        let every = events.get(index + 1).map_or(0, |next| next.at - event.at);
        let mut count = 1;
        while let Some(next) = events.get(index + count) {
            if next.operation != event.operation
                || u64::from(next.at) != u64::from(event.at) + count as u64 * u64::from(every)
            {
                break;
            }
            count += 1;
        }
        result.push(Event {
            at: event.at - base,
            repeat: count as u32,
            every: if count > 1 { every } else { 0 },
            operation: event.operation.clone(),
        });
        index += count;
    }
    result
}

#[cfg(test)]
mod tests;
