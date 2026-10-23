//! Replay independently attributed cosmetic draws through ordinary effect emission.
use super::{Trace, random_inputs::Pin};
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, EffectAppearance, Effects, FixtureEffectBirth};
use resonance_content::battle_effect::{EffectOperation, SourceBank, declaration::Declaration};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DrawKind {
    SignedRemainder,
    UnsignedRemainder,
    PolarRadius,
    PolarAngle,
}
#[derive(Deserialize)]
struct Draw {
    draw: u32,
    interval_index: usize,
    operation_word_offset: usize,
    kind: DrawKind,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum BirthAttachment {
    Projectile { projectile: u64 },
}
#[derive(Deserialize)]
struct Birth {
    clock: u32,
    program_clock: u32,
    event_age: u32,
    owner: usize,
    program: u16,
    spawn: usize,
    particle: u8,
    model_ordinal: Option<usize>,
    heading: Option<f32>,
    first_draw: usize,
    modifier_offset: usize,
    birth_clock: u32,
    source_particle: u64,
    attachment: Option<BirthAttachment>,
    modifier_words: Vec<u16>,
    draws: Vec<Draw>,
}
#[derive(Deserialize)]
struct Interval {
    clock: u32,
    before: u32,
    after: u32,
    values: Vec<u16>,
}
#[derive(Deserialize)]
struct Proof {
    format: String,
    case: String,
    metadata: Pin,
    archive: Pin,
    bank_offset: usize,
    bank_length: usize,
    source_capture: Pin,
    intervals: Vec<Interval>,
    births: Vec<Birth>,
}

#[derive(Default)]
pub(super) struct Inputs {
    pending: BTreeMap<u32, Vec<FixtureEffectBirth>>,
}

impl Inputs {
    pub(super) fn admit(
        source: &Trace,
        banks: &BTreeMap<resonance_battle::ResourceId, String>,
        effects: &mut Effects,
    ) -> Result<Self> {
        let Some(pin) = source.setup.get("effect_birth_inputs") else {
            return Ok(Self::default());
        };
        let proof: Proof =
            serde_json::from_slice(&serde_json::from_value::<Pin>(pin.clone())?.read_bytes()?)?;
        ensure!(
            proof.format == "source-effect-birth-inputs-v1"
                && proof.case == source.case
                && source.setup["capture"]["sha256"] == proof.source_capture.sha256,
            "unqualified effect birth inputs"
        );
        proof.source_capture.read_bytes()?;
        let metadata: SourceBank = serde_json::from_slice(&proof.metadata.read_bytes()?)?;
        let archive = proof.archive.read_bytes()?;
        let raw = archive
            .get(proof.bank_offset..proof.bank_offset + proof.bank_length)
            .context("effect bank range differs")?;
        ensure!(
            format!("{:x}", Sha256::digest(raw)) == metadata.source_sha256,
            "effect bank fingerprint differs"
        );
        let (&resource, _) = super::exactly_one(
            banks
                .iter()
                .filter(|(_, hash)| **hash == metadata.source_sha256),
        )
        .context("recorded effect bank is not unique")?;
        for interval in &proof.intervals {
            let first_tick = source
                .frames
                .first()
                .context("missing effect source frames")?["presentation_tick"]
                .as_u64()
                .context("missing source presentation clock")?;
            let index = source
                .frames
                .iter()
                .position(|frame| {
                    frame["presentation_tick"].as_u64()
                        == Some(first_tick + u64::from(interval.clock))
                })
                .context("effect interval clock is absent")?;
            ensure!(
                index > 0
                    && source.frames[index - 1]["source_random_state"] == interval.before
                    && source.frames[index]["source_random_state"] == interval.after,
                "effect RNG interval is not witnessed by source frames"
            );
            let mut state = interval.before;
            for value in &interval.values {
                state = state.wrapping_mul(0x41c64e6d).wrapping_add(0x12d687);
                ensure!(
                    (state >> 16) as u16 == *value,
                    "effect RNG interval differs"
                );
            }
            ensure!(state == interval.after, "effect RNG interval is incomplete");
        }
        let mut headings = BTreeMap::<_, BTreeSet<_>>::new();
        for birth in &proof.births {
            if let Some(heading) = birth.heading {
                ensure!(heading.is_finite(), "invalid effect heading input");
                headings
                    .entry((birth.owner, birth.program, birth.program_clock))
                    .or_default()
                    .insert(heading.to_bits());
            }
        }
        let mut result = Self::default();
        let mut scopes = Vec::new();
        for input in proof.births {
            let interval = proof
                .intervals
                .iter()
                .find(|interval| interval.clock == input.clock)
                .context("effect draw has no observed interval")?;
            ensure!(
                source
                    .events
                    .iter()
                    .filter(|event| event["kind"] == "program_started"
                        && event["owner"] == input.owner
                        && event["bank"] == metadata.source_sha256
                        && event["program"] == input.program
                        && event["presentation_clock"] == input.program_clock)
                    .count()
                    == input.heading.map_or(1, |_| {
                        headings[&(input.owner, input.program, input.program_clock)].len()
                    }),
                "effect input source program multiplicity differs"
            );
            let program = metadata
                .programs
                .get(usize::from(input.program))
                .context("missing effect program")?;
            let emissions: Vec<_> = program
                .iter()
                .filter_map(|event| match &event.operation {
                    EffectOperation::Spawn {
                        particle, birth, ..
                    } => Some((event.at, *particle, birth)),
                    _ => None,
                })
                .collect();
            let (age, particle, birth) = emissions
                .get(input.spawn)
                .context("missing effect emission")?;
            ensure!(
                *age == input.event_age
                    && input.program_clock <= input.clock
                    && *particle == input.particle
                    && input.draws.len() == birth.random_draws as usize,
                "effect input emission differs"
            );
            ensure!(
                input.first_draw
                    == emissions
                        .iter()
                        .take(input.spawn)
                        .map(|(_, _, birth)| birth.random_draws as usize)
                        .sum::<usize>(),
                "effect shared draw slot differs"
            );
            let first_tick = source.frames[0]["presentation_tick"].as_u64().unwrap();
            let frame = source
                .frames
                .iter()
                .find(|frame| {
                    frame["presentation_tick"].as_u64()
                        == Some(first_tick + u64::from(input.birth_clock))
                })
                .context("source effect birth clock is absent")?;
            let particle = super::exactly_one(
                frame["particles"]
                    .as_array()
                    .context("missing source particles")?
                    .iter()
                    .filter(|particle| particle["id"] == input.source_particle),
            )
            .filter(|particle| {
                particle["owner"] == input.owner
                    && particle["bank"] == metadata.source_sha256
                    && particle["member"] == input.particle
                    && particle["initialized"] == true
                    && particle["age"] == 1
            })
            .context("effect allocation identity is not observed at birth")?;
            let model = if let Some(ordinal) = input.model_ordinal {
                let observed = super::exactly_one(
                    frame["effect_models"]
                        .as_array()
                        .context("missing effect models")?
                        .iter()
                        .filter(|model| {
                            model["owner"] == input.owner
                                && model["bank"] == metadata.source_sha256
                                && model["member"] == input.particle
                                && model["ordinal"] == ordinal
                                && model["model"] == particle["model"]
                        }),
                )
                .context("effect model birth is not unique")?;
                Some(observed)
            } else {
                ensure!(
                    matches!(
                        metadata.actors.get(usize::from(input.particle)),
                        Some(Declaration::Particle { .. })
                    ) && particle["model"].is_null()
                        && particle["owner_bone"].is_null(),
                    "effect sprite birth differs from its authored declaration"
                );
                None
            };
            let attachment = match input.attachment {
                None => serde_json::Value::Null,
                Some(BirthAttachment::Projectile { projectile }) => {
                    super::exactly_one(
                        frame["projectiles"]
                            .as_array()
                            .context("missing projectile birth observations")?
                            .iter()
                            .filter(|shot| shot["id"] == projectile),
                    )
                    .filter(|shot| {
                        projectile != 0
                            && input.event_age == 0
                            && input.program_clock == input.birth_clock
                            && input.clock == input.birth_clock
                            && shot["owner"] == input.owner
                            && shot["age"] == 1
                            && super::exactly_one(source.events.iter().filter(|event| {
                                event["kind"] == "projectile_started"
                                    && event["projectile"] == projectile
                                    && event["owner"] == input.owner
                                    && event["target"] == shot["target"]
                                    && event["presentation_clock"] == input.birth_clock
                            }))
                            .is_some()
                    })
                    .context("effect attachment lacks a unique owned projectile birth")?;
                    serde_json::json!({"kind": "projectile", "projectile": projectile})
                }
            };
            ensure!(
                model.map_or_else(
                    || attachment.is_null(),
                    |model| model["attachment"] == attachment
                ),
                "effect birth attachment differs"
            );
            let bytes: Vec<_> = input
                .modifier_words
                .iter()
                .flat_map(|word| word.to_be_bytes())
                .collect();
            ensure!(
                raw.get(input.modifier_offset..input.modifier_offset + bytes.len())
                    == Some(bytes.as_slice()),
                "effect modifier bytes differ"
            );
            let mut samples = Vec::new();
            for draw in &input.draws {
                ensure!(
                    draw.draw as usize == input.first_draw + samples.len(),
                    "effect shared draw order differs"
                );
                let value = *interval
                    .values
                    .get(draw.interval_index)
                    .context("missing observed effect draw")?;
                let words = input
                    .modifier_words
                    .get(draw.operation_word_offset..)
                    .context("missing effect random operation")?;
                let (divisor, count, index) = match draw.kind {
                    DrawKind::SignedRemainder | DrawKind::UnsignedRemainder => {
                        let opcode = if matches!(draw.kind, DrawKind::SignedRemainder) {
                            11
                        } else {
                            2
                        };
                        ensure!(
                            words.len() >= 4 && words[0] == opcode && words[3] == 0,
                            "effect remainder operation differs"
                        );
                        let divisor = i32::from(words[2]);
                        ensure!(
                            divisor > 0 && divisor <= i32::from(i16::MAX),
                            "invalid effect divisor"
                        );
                        if matches!(draw.kind, DrawKind::SignedRemainder) {
                            (
                                divisor,
                                2 * divisor - 1,
                                i32::from(value as i16) % divisor + divisor - 1,
                            )
                        } else {
                            (divisor, divisor, i32::from(value) % divisor)
                        }
                    }
                    DrawKind::PolarRadius | DrawKind::PolarAngle => {
                        ensure!(
                            words.len() >= 12 && words[0] == 9,
                            "effect polar operation differs"
                        );
                        let slot = if matches!(draw.kind, DrawKind::PolarRadius) {
                            10
                        } else {
                            11
                        };
                        let divisor = i32::from(words[slot] as i16);
                        ensure!(divisor > 0, "invalid effect polar divisor");
                        (
                            divisor,
                            2 * divisor,
                            i32::from(value as i16) % divisor + divisor,
                        )
                    }
                };
                let ranges: Vec<_> = emissions
                    .iter()
                    .flat_map(|(_, _, birth)| birth.ranges())
                    .filter(|range| range.draw == Some(draw.draw))
                    .collect();
                let spreads: Vec<_> = emissions
                    .iter()
                    .flat_map(|(_, _, birth)| {
                        [(0x34, birth.offset_spread), (0x40, birth.velocity_spread)]
                    })
                    .filter_map(|(target, spread)| spread.map(|spread| (target, spread)))
                    .filter(|(_, spread)| {
                        spread.radius_draw == Some(draw.draw)
                            || spread.angle_draw == Some(draw.draw)
                    })
                    .collect();
                match draw.kind {
                    DrawKind::SignedRemainder | DrawKind::UnsignedRemainder => {
                        ensure!(
                            spreads.is_empty()
                                && ranges
                                    .iter()
                                    .all(|range| range.step > 0.
                                        && range.choice_count() == count as f32),
                            "effect discrete distribution differs"
                        );
                        if ranges.is_empty() {
                            match draw.kind {
                                DrawKind::SignedRemainder => ensure!(
                                    (0x7ff8..=0x7ffb).contains(&words[1]),
                                    "unused effect draw is not a carried scratch value"
                                ),
                                DrawKind::UnsignedRemainder => {
                                    ensure!(
                                        words.get(4..8) == Some(&[3, 0xd4, words[1], 0]),
                                        "unused effect integer has no packed addition"
                                    );
                                    let table = u16::from_be_bytes(
                                        raw.get(8..10)
                                            .context("missing effect declarations")?
                                            .try_into()
                                            .unwrap(),
                                    );
                                    let at = usize::from(table) + usize::from(input.particle) * 352;
                                    let declaration = raw
                                        .get(at..at + 352)
                                        .context("missing effect declaration")?;
                                    let flags =
                                        u32::from_be_bytes(declaration[20..24].try_into().unwrap());
                                    ensure!(
                                        flags & 0x24 == 0
                                            && i32::from(declaration[213]) + divisor - 1 <= 255
                                            && birth.animation.is_none()
                                            && matches!(metadata.actors.get(usize::from(input.particle)),
                                            Some(Declaration::ModelParticle { binding, .. })
                                            if binding.animation.is_none() && !binding.program_animation),
                                        "unused packed effect field can change an active model"
                                    );
                                }
                                _ => unreachable!(),
                            }
                        }
                        samples.push((index as f32 + 0.5) / count as f32);
                    }
                    DrawKind::PolarRadius | DrawKind::PolarAngle => {
                        let scalar = |at: usize| {
                            f32::from_bits(u32::from(words[at]) << 16 | u32::from(words[at + 1]))
                        };
                        ensure!(
                            ranges.is_empty()
                                && !spreads.is_empty()
                                && spreads.iter().all(|(target, spread)| {
                                    let (slot, extent) =
                                        if matches!(draw.kind, DrawKind::PolarRadius) {
                                            (spread.radius_draw, spread.radius_jitter)
                                        } else {
                                            (spread.angle_draw, spread.angle_jitter)
                                        };
                                    *target == words[1]
                                        && slot == Some(draw.draw)
                                        && extent == divisor as f32 * 0.1
                                        && spread.angles == [scalar(2), scalar(4), scalar(6)]
                                        && spread.radius == scalar(8)
                                }),
                            "effect polar distribution differs"
                        );
                        samples.push(index as f32 / count as f32);
                    }
                }
            }
            let appearance = EffectAppearance {
                resource,
                member: input.program,
            };
            let owner = ActorId::from_index(input.owner)?;
            if !scopes.contains(&(owner, appearance, input.particle)) {
                scopes.push((owner, appearance, input.particle));
            }
            result
                .pending
                .entry(input.clock)
                .or_default()
                .push(FixtureEffectBirth {
                    owner,
                    appearance,
                    spawn: input.spawn,
                    particle: input.particle,
                    first_draw: input.first_draw,
                    birth: birth.as_ref().clone(),
                    samples,
                    model_ordinal: input.model_ordinal,
                    heading: input.heading,
                });
        }
        effects.observe_fixture_births(&scopes)?;
        Ok(result)
    }

    pub(super) fn prepare(&mut self, clock: Option<u32>, effects: &mut Effects) -> Result<()> {
        let inputs = clock
            .and_then(|clock| self.pending.remove(&clock))
            .unwrap_or_default();
        effects.fixture_birth_inputs(inputs)
    }

    pub(super) fn finish(&self, effects: &Effects) -> Result<()> {
        effects.finish_fixture_birth_inputs()?;
        ensure!(
            self.pending.is_empty(),
            "effect input clocks were not reached"
        );
        Ok(())
    }
}
