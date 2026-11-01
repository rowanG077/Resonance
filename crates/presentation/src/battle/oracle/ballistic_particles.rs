//! Validate detached random debris using the ordinary particle integrator.
use super::{Trace, compare::equal, exactly_one, optional_field, random_inputs::Pin};
use anyhow::{Context as ContextExt, Result, ensure};
use resonance_battle::{FixtureParticle, ParticleGeometry, ParticleTemplate};
use resonance_content::battle_effect::{
    EffectOperation, ParticleBirth, ScheduledEvent, ValueRange,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

type Key = (u64, String, u64, u64);
pub(super) type Validated = BTreeMap<Key, Lifetime>;
pub(super) struct Lifetime {
    first: usize,
    last: usize,
    ordinal: usize,
    origin: [f32; 3],
    heading: f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    metadata: super::content_evidence::Evidence,
    scheduler: Option<Pin>,
    member: u64,
    program: u64,
}

pub(super) fn validate(source: &Trace, native: &Trace) -> Result<[Validated; 2]> {
    let descriptors: Vec<Descriptor> =
        optional_field(&source.setup, "effect_ballistic_particles")?.unwrap_or_default();
    let mut result = [BTreeMap::new(), BTreeMap::new()];
    for descriptor in descriptors {
        let metadata = super::content_evidence::effect_value(descriptor.metadata.effects()?)?;
        let bank = descriptor.metadata.source_digest()?;
        let raw_bank = descriptor.metadata.source()?;
        if descriptor.scheduler.is_none() {
            for trace in [source, native] {
                let registration = trace.setup["stored_scenes"]["registrations"]
                    .as_array()
                    .context("missing debris scene registrations")?
                    .iter()
                    .find(|r| r["bank_sha256"] == bank)
                    .context("debris metadata is not registered")?;
                super::content_evidence::registered_scene(registration)?;
                let effects = super::content_evidence::Evidence::parse(&registration["effects"])?;
                ensure!(
                    effects.entry == descriptor.metadata.entry,
                    "debris effect entry differs"
                );
                effects.includes(&descriptor.metadata)?;
                descriptor.metadata.includes(&effects)?;
            }
        }
        let declaration = &metadata["actors"][descriptor.member as usize];
        let template: ParticleTemplate = serde_json::from_value(declaration["template"].clone())?;
        ensure!(
            declaration["kind"] == "model_particle"
                && declaration["binding"]["animation"].is_null()
                && template.lifetime.is_some_and(|life| life > 1)
                && !template.origin_motion
                && !template.fixed_orientation
                && template.jerk == [0.; 3]
                && template.state.acceleration[0] == 0.
                && template.state.acceleration[2] == 0.
                && template.state.acceleration[1] <= 0.,
            "unsupported debris declaration"
        );
        let slot = declaration["binding"]["slot"]
            .as_u64()
            .context("missing debris model binding")?;
        let events: Vec<ScheduledEvent> =
            serde_json::from_value(metadata["programs"][descriptor.program as usize].clone())?;
        let mut spawns = Vec::new();
        let mut sequence = Vec::new();
        for event in events {
            match event.operation {
                EffectOperation::Spawn {
                    particle, birth, ..
                } => {
                    if u64::from(particle) == descriptor.member {
                        spawns.push((event.at.max(1), *birth));
                    }
                    sequence.push(u64::from(particle));
                }
                EffectOperation::SetMotion { spawn, .. } => ensure!(
                    sequence.get(spawn) != Some(&descriptor.member),
                    "debris receives later scripted motion"
                ),
                _ => {}
            }
        }
        ensure!(!spawns.is_empty(), "debris has no authored births");
        let program_epoch = descriptor
            .scheduler
            .as_ref()
            .map(|observations| {
                embedded_epoch(source, &descriptor, observations, &raw_bank, &spawns)
            })
            .transpose()?;
        for (side, trace) in [source, native].into_iter().enumerate() {
            validate_run(
                trace,
                bank,
                &descriptor,
                &metadata,
                &template,
                slot,
                &spawns,
                program_epoch,
                &mut result[side],
            )?;
        }
    }
    Ok(result)
}

/// Bind an embedded emitter's execution clock to original observer rows. The same
/// source epoch is used for both runs, so no native time shift can be admitted.
fn embedded_epoch(
    source: &Trace,
    descriptor: &Descriptor,
    observations: &Pin,
    bank: &[u8],
    spawns: &[(u32, ParticleBirth)],
) -> Result<u64> {
    let capture: Value = serde_json::from_slice(
        &serde_json::from_value::<Pin>(source.setup["capture"].clone())?.read_bytes()?,
    )?;
    ensure!(
        capture["combined_observations_sha256"] == observations.sha256,
        "scheduler rows do not belong to the captured source"
    );
    let rows: Vec<Value> = serde_json::from_slice(&observations.read_bytes()?)?;
    let owner = source.setup["owner"]
        .as_u64()
        .context("missing scheduler owner")?;
    let slot = source.setup["source_actor_map"]
        .as_array()
        .context("missing source actor map")?
        .iter()
        .find(|entry| entry["actor"] == owner)
        .context("missing scheduler source actor")?["source_slot"]
        .as_u64()
        .context("missing source actor slot")?;
    let prefix = format!("actor_{slot}_effect_");
    let scalar = |row: &Value, field: &str| {
        row[format!("{prefix}{field}")]
            .as_u64()
            .context("missing embedded scheduler observation")
    };
    let word = |offset: usize| -> Result<u16> {
        Ok(u16::from_be_bytes(
            bank.get(offset..offset + 2)
                .context("truncated original bank")?
                .try_into()?,
        ))
    };
    ensure!(
        bank.get(..4) == Some(b"ef1\0".as_slice())
            && descriptor.program < u64::from(*bank.get(4).context("truncated original bank")?),
        "invalid original scheduler bank"
    );
    let table = usize::from(word(16)?);
    let root =
        i64::from(word(10)?) + i64::from(word(table + descriptor.program as usize * 2)? as i16);
    let signature = descriptor.metadata.source_digest()?;
    let parent = exactly_one(source.events.iter().filter(|event| {
        event["kind"] == "program_started"
            && event["owner"] == owner
            && event["bank"] == signature
            && event["program"] == descriptor.program
    }))
    .context("ambiguous embedded program admission")?;
    let parent = parent["clock"]
        .as_u64()
        .context("missing embedded parent clock")?;
    let start = source.frames[0]["combat_tick"]
        .as_u64()
        .context("missing admission clock")?;
    let mut epoch = None;
    let mut last = None;
    for frame in &source.frames {
        let clock = frame["combat_tick"]
            .as_u64()
            .context("missing source combat clock")?;
        if clock < start + parent {
            continue;
        }
        let row = rows
            .iter()
            .find(|row| {
                row["battle_combat_tick_word"] == frame["combat_tick"]
                    && row["battle_presentation_tick_word"] == frame["presentation_tick"]
            })
            .context("missing exact scheduler row")?;
        let base = scalar(row, "bank")?;
        if i64::try_from(scalar(row, "program")?)? - i64::try_from(base)? != root {
            break;
        }
        let age = (scalar(row, "clock_word")? as u16) as i16;
        let observed = i64::try_from(clock - start + 1)? - i64::from(age);
        if epoch.is_some_and(|epoch| epoch != observed) {
            break;
        }
        epoch = Some(observed);
        last = Some(clock - start);
    }
    let epoch = u64::try_from(epoch.context("no independently observed scheduler epoch")?)?;
    ensure!(
        last.is_some_and(|last| spawns.iter().all(|(at, _)| epoch + u64::from(*at) <= last)),
        "scheduler evidence does not cover all particle births"
    );
    Ok(epoch)
}

fn validate_run(
    trace: &Trace,
    bank: &str,
    descriptor: &Descriptor,
    metadata: &Value,
    template: &ParticleTemplate,
    fixed_slot: u64,
    spawns: &[(u32, ParticleBirth)],
    program_epoch: Option<u64>,
    output: &mut Validated,
) -> Result<()> {
    let owner = trace.setup["owner"]
        .as_u64()
        .context("missing debris owner")?;
    let parents: Vec<_> = trace
        .events
        .iter()
        .filter(|e| {
            e["kind"] == "program_started"
                && e["owner"] == owner
                && e["bank"] == bank
                && e["program"] == descriptor.program
        })
        .collect();
    let parent = parents.first().context("debris has no parent program")?;
    let parent_clock = parent["clock"]
        .as_u64()
        .context("missing debris parent clock")?;
    ensure!(
        parents.iter().all(|p| p["clock"] == parent_clock),
        "debris parent programs are not simultaneous"
    );
    let emission_clock = program_epoch.unwrap_or(parent_clock);
    ensure!(
        parents.len() == 1 || spawns.iter().all(|(_, birth)| birth.angles[1].is_none()),
        "simultaneous debris parents need independently observed fixed yaw"
    );
    let start = trace.frames.first().context("missing debris frames")?["combat_tick"]
        .as_u64()
        .context("missing admission clock")?;
    let parent_frame = trace
        .frames
        .iter()
        .find(|f| f["combat_tick"].as_u64() == Some(start + parent_clock))
        .context("missing parent observation")?;
    let actor = parent_frame["actors"]
        .as_array()
        .context("missing parent actors")?
        .iter()
        .find(|a| a["actor"] == owner)
        .context("missing debris owner pose")?;
    let heading = actor["heading"].as_f64().context("missing owner heading")? as f32;
    let is_debris = |value: &Value| {
        value["owner"] == owner && value["bank"] == bank && value["member"] == descriptor.member
    };
    let mut lives: BTreeMap<Key, Vec<(usize, &Value, &Value)>> = BTreeMap::new();
    for (index, frame) in trace.frames.iter().enumerate() {
        for particle in frame["particles"].as_array().context("missing particles")? {
            if is_debris(particle) && particle["age"].as_u64().is_some_and(|age| age > 0) {
                ensure!(
                    !particle["model"].is_null()
                        && frame["effect_models"]
                            .as_array()
                            .context("missing effect models")?
                            .iter()
                            .filter(|m| is_debris(m) && m["model"] == particle["model"])
                            .count()
                            == 1,
                    "debris particle has no unique visible model"
                );
            }
        }
        for model in frame["effect_models"]
            .as_array()
            .context("missing effect models")?
        {
            if !is_debris(model) {
                continue;
            }
            let particle = exactly_one(
                frame["particles"]
                    .as_array()
                    .context("missing particles")?
                    .iter()
                    .filter(|p| is_debris(p) && p["model"] == model["model"]),
            )
            .context("ambiguous debris particle/model binding")?;
            lives
                .entry(key(model)?)
                .or_default()
                .push((index, model, particle));
        }
    }
    ensure!(
        lives.len() == spawns.len() * parents.len(),
        "debris birth count differs"
    );
    fn canonical_draws(value: &mut Value, draws: &mut BTreeMap<u64, u64>) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(key.as_str(), "draw" | "radius_draw" | "angle_draw")
                        && let Some(original) = value.as_u64()
                    {
                        let next = draws.len() as u64;
                        *value = json!(*draws.entry(original).or_insert(next));
                    } else {
                        canonical_draws(value, draws);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    canonical_draws(value, draws);
                }
            }
            _ => {}
        }
    }
    let recipes: Vec<_> = spawns
        .iter()
        .map(|spawn| {
            let mut recipe = json!(spawn);
            canonical_draws(&mut recipe, &mut BTreeMap::new());
            recipe
        })
        .collect();
    let mut groups = BTreeMap::<u32, Vec<_>>::new();
    for (key, samples) in lives {
        let observed_heading = if parents.len() == 1 {
            heading
        } else {
            let angles: [f32; 3] = serde_json::from_value(samples[0].2["angles"].clone())?;
            angles[1] - template.state.angles[1] - template.state.angular_velocity[1]
        };
        ensure!(
            observed_heading.is_finite(),
            "nonfinite debris parent heading"
        );
        groups
            .entry(observed_heading.to_bits())
            .or_default()
            .push((key, samples));
    }
    ensure!(
        groups.len() == parents.len(),
        "debris parent heading count differs"
    );
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|a, b| f32::from_bits(a.0).total_cmp(&f32::from_bits(b.0)));
    for (group_index, (heading_bits, mut lives)) in groups.into_iter().enumerate() {
        let heading = f32::from_bits(heading_bits);
        ensure!(
            lives.len() == spawns.len(),
            "debris parent schedule is incomplete"
        );
        lives.sort_by_key(|(key, samples)| (samples[0].0, key.3));
        let mut available = vec![true; spawns.len()];
        let validate_lifetime = |samples: &[(usize, &Value, &Value)],
                                 at: u32,
                                 birth: &ParticleBirth|
         -> Result<Lifetime> {
            let (first, model, particle) = samples[0];
            ensure!(
                trace.frames[first]["combat_tick"].as_u64()
                    == Some(start + emission_clock + u64::from(at))
                    && samples.len() == template.lifetime.unwrap() as usize
                    && samples.last().unwrap().0 + 1 < trace.frames.len(),
                "debris schedule or lifetime differs"
            );
            let origin: [f32; 3] = serde_json::from_value(particle["origin"].clone())?;
            let state = hydrate_birth(template, birth, particle, model, heading)?;
            let slot = match birth.model {
                Some(range) => {
                    let observed = particle["model_slot"]
                        .as_u64()
                        .context("random debris mesh was not observed")?;
                    range_check(observed as f32, range)?;
                    observed
                }
                None => fixed_slot,
            };
            ensure!(
                !metadata["art"]["models"][slot.to_string()].is_null(),
                "debris mesh is not in the pinned bank"
            );
            let birth_angles = state.angles;
            let mut motion = FixtureParticle::at_first_frame(template.clone(), state, origin)?;
            for (age, (index, model, observed)) in samples.iter().enumerate() {
                ensure!(
                    *index == first + age
                        && observed["age"] == age + 1
                        && observed["id"] == particle["id"]
                        && observed["initialized"] == true
                        && observed["owner_bone"].is_null()
                        && model["attachment"].is_null()
                        && model["model"]["clip"].is_null()
                        && model["model"]["frame"].is_null()
                        && observed.get("model_slot").is_some()
                            == particle.get("model_slot").is_some()
                        && observed
                            .get("model_slot")
                            .is_none_or(|value| value.as_u64() == Some(slot)),
                    "debris identity, age, binding or motion changed"
                );
                equal(
                    &json!(origin),
                    &observed["origin"],
                    "detached debris origin",
                )?;
                check_frame(model, observed, motion.state(), origin, template)?;
                if age + 1 < samples.len() {
                    motion.advance()?;
                }
            }
            let birth_heading = if birth.angles[1].is_none() {
                birth_angles[1] - template.state.angles[1] - template.state.angular_velocity[1]
            } else {
                heading
            };
            Ok(Lifetime {
                first,
                last: first + samples.len() - 1,
                ordinal: 0,
                origin,
                heading: birth_heading,
            })
        };
        for (key, samples) in lives {
            let mut matches = Vec::new();
            let mut failure = None;
            for (index, (at, birth)) in spawns.iter().enumerate() {
                if !available[index]
                    || trace.frames[samples[0].0]["combat_tick"].as_u64()
                        != Some(start + emission_clock + u64::from(*at))
                {
                    continue;
                }
                match validate_lifetime(&samples, *at, birth) {
                    Ok(life) => matches.push((index, life)),
                    Err(error) => failure = Some(error),
                }
            }
            ensure!(
                !matches.is_empty(),
                "debris has no complete authored lifetime: {:?}",
                failure
            );
            let recipe = &recipes[matches[0].0];
            ensure!(
                matches.iter().all(|(index, _)| recipes[*index] == *recipe),
                "debris matches different authored birth recipes"
            );
            let (index, mut life) = matches.remove(0);
            available[index] = false;
            life.ordinal = group_index * spawns.len() + index;
            ensure!(
                output.insert(key, life).is_none(),
                "duplicate debris normalization"
            );
        }
        ensure!(
            available.iter().all(|value| !value),
            "debris authored birth is missing"
        );
    }
    Ok(())
}

fn hydrate_birth(
    template: &ParticleTemplate,
    birth: &ParticleBirth,
    particle: &Value,
    model: &Value,
    heading: f32,
) -> Result<resonance_battle::ParticleState> {
    let mut state = template.state.clone();
    let offset: [f32; 3] = serde_json::from_value(particle["offset"].clone())?;
    let (angles, scale) = birth_transform(model, particle, birth, template)?;
    let velocity = if birth.offset.iter().flatten().any(|r| r.min != r.max) {
        ensure!(
            birth.velocity_spread.is_none()
                && birth.velocity.iter().flatten().all(|r| r.min == r.max),
            "simultaneous random offset and velocity are not independently observed"
        );
        let velocity: [f32; 3] = std::array::from_fn(|axis| {
            birth.velocity[axis].map_or(state.velocity[axis], |r| r.min)
        });
        let local = rotate(offset, -heading);
        for axis in 0..3 {
            range_check(
                local[axis] - velocity[axis],
                birth.offset[axis].unwrap_or(ValueRange::fixed(state.offset[axis])),
            )?;
        }
        rotate(velocity, heading)
    } else {
        let initial = rotate(
            std::array::from_fn(|axis| birth.offset[axis].map_or(state.offset[axis], |r| r.min)),
            heading,
        );
        std::array::from_fn(|axis| offset[axis] - initial[axis])
    };
    validate_birth(birth, template, velocity, angles, scale)?;
    state.offset = offset;
    state.velocity = std::array::from_fn(|axis| velocity[axis] + state.acceleration[axis]);
    state.angles = angles;
    let ParticleGeometry::Size { value, .. } = &mut state.geometry else {
        anyhow::bail!("particle does not use size geometry");
    };
    *value = scale;
    Ok(state)
}

fn validate_birth(
    birth: &ParticleBirth,
    template: &ParticleTemplate,
    velocity: [f32; 3],
    angles: [f32; 3],
    scale: [f32; 3],
) -> Result<()> {
    let mut supported = ParticleBirth::default();
    supported.random_draws = birth.random_draws;
    supported.offset = birth.offset;
    supported.velocity = birth.velocity;
    supported.angles = birth.angles;
    supported.size = birth.size;
    supported.model = birth.model;
    supported.velocity_spread = birth.velocity_spread;
    ensure!(*birth == supported, "unsupported debris birth modifier");
    ensure!(
        birth.velocity[0].is_none()
            && birth.velocity[2].is_none()
            && template.state.velocity[0] == 0.
            && template.state.velocity[2] == 0.,
        "unsupported directional debris velocity"
    );
    if let Some(spread) = birth.velocity_spread {
        ensure!(
            spread.angles == [90., 0., 0.] && spread.angle_jitter == 360.,
            "unsupported debris velocity spread"
        );
        range_check(
            velocity[0].hypot(velocity[2]),
            ValueRange {
                min: spread.radius - spread.radius_jitter,
                max: spread.radius + spread.radius_jitter,
                step: 0.,
                draw: None,
                reverse: false,
            },
        )?;
    } else {
        equal(
            &json!([velocity[0], velocity[2]]),
            &json!([0., 0.]),
            "deterministic debris horizontal velocity",
        )?;
    }
    range_check(
        velocity[1],
        birth.velocity[1].unwrap_or(ValueRange::fixed(template.state.velocity[1])),
    )?;
    let ParticleGeometry::Size {
        value, velocity, ..
    } = template.state.geometry
    else {
        anyhow::bail!("debris lacks scale");
    };
    for axis in 0..3 {
        if let Some(range) = birth.angles[axis] {
            range_check(
                wrap(angles[axis] - template.state.angular_velocity[axis]),
                range,
            )?;
        } else if axis != 1 {
            equal(
                &json!(wrap(angles[axis] - template.state.angular_velocity[axis])),
                &json!(wrap(template.state.angles[axis])),
                "debris fixed birth angle",
            )?;
        }
        range_check(
            scale[axis] - velocity[axis],
            birth.size[axis].unwrap_or(ValueRange::fixed(value[axis])),
        )?;
        for other in 0..axis {
            if birth.size[axis]
                .is_some_and(|range| range.draw.is_some() && birth.size[other] == Some(range))
            {
                equal(
                    &json!(scale[axis]),
                    &json!(scale[other]),
                    "shared debris size draw",
                )?;
            }
        }
    }
    Ok(())
}

fn range_check(value: f32, range: ValueRange) -> Result<()> {
    range.validate()?;
    ensure!(value.is_finite(), "nonfinite debris birth");
    let nearest = if range.step > 0. {
        range.min + ((value - range.min) / range.step).round() * range.step
    } else {
        value.clamp(range.min, range.max)
    };
    equal(
        &json!(value),
        &json!(nearest.clamp(range.min, range.max)),
        "authored debris birth range",
    )
}
fn wrap(angle: f32) -> f32 {
    (angle + 180.).rem_euclid(360.) - 180.
}
fn rotate([x, y, z]: [f32; 3], heading: f32) -> [f32; 3] {
    let (sin, cos) = heading.to_radians().sin_cos();
    [cos * x + sin * z, y, cos * z - sin * x]
}
fn birth_transform(
    model: &Value,
    particle: &Value,
    birth: &ParticleBirth,
    template: &ParticleTemplate,
) -> Result<([f32; 3], [f32; 3])> {
    let world: [f64; 12] = serde_json::from_value(model["model"]["world"].clone())?;
    let scale: [f32; 3] = std::array::from_fn(|col| {
        (0..3)
            .map(|row| world[row * 4 + col].powi(2))
            .sum::<f64>()
            .sqrt() as f32
    });
    ensure!(
        scale.iter().all(|value| value.is_finite() && *value > 0.),
        "invalid debris birth scale"
    );
    if let Some(angles) = particle.get("angles") {
        return Ok((serde_json::from_value(angles.clone())?, scale));
    }
    let r: [[f64; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|col| world[row * 4 + col] / f64::from(scale[col]))
    });
    let angles = [
        r[2][1].atan2(r[2][2]),
        (-r[2][0]).atan2(r[0][0].hypot(r[1][0])),
        r[1][0].atan2(r[0][0]),
    ]
    .map(|v| v.to_degrees() as f32);
    let alternate = [angles[0] + 180., 180. - angles[1], angles[2] + 180.];
    let angles = exactly_one([angles, alternate].into_iter().map(|a| a.map(wrap)).filter(
        |angles| {
            (0..3).filter(|axis| *axis != 1).all(|axis| {
                let initial = wrap(angles[axis] - template.state.angular_velocity[axis]);
                let range =
                    birth.angles[axis].unwrap_or(ValueRange::fixed(template.state.angles[axis]));
                range_check(initial, range).is_ok()
            })
        },
    ))
    .context("debris birth rotation is not uniquely observed")?;
    Ok((angles, scale))
}
fn check_frame(
    model: &Value,
    particle: &Value,
    state: &resonance_battle::ParticleState,
    origin: [f32; 3],
    template: &ParticleTemplate,
) -> Result<()> {
    equal(&json!(state.offset), &particle["offset"], "debris motion")?;
    if let Some(observed) = particle.get("angles") {
        let actual: [f32; 3] = serde_json::from_value(observed.clone())?;
        equal(
            &json!(state.angles.map(wrap)),
            &json!(actual.map(wrap)),
            "debris angular motion",
        )?;
    }
    let ParticleGeometry::Size { value: scale, .. } = state.geometry else {
        anyhow::bail!("debris size changed");
    };
    let [x, y, z] = state.angles.map(f32::to_radians);
    let rotation = bevy::math::Quat::from_euler(bevy::math::EulerRot::ZYX, z, y, x);
    let mut position: [f32; 3] = std::array::from_fn(|axis| origin[axis] + state.offset[axis]);
    if template.model_ground_relative {
        position[1] = state.offset[1];
    }
    position[1] += template.model_elevation.unwrap_or(0.);
    let world =
        bevy::math::Mat4::from_scale_rotation_translation(scale.into(), rotation, position.into())
            .to_cols_array_2d();
    let expected: [f32; 12] = std::array::from_fn(|index| world[index % 4][index / 4]);
    equal(
        &json!(expected),
        &model["model"]["world"],
        "debris transform",
    )
}
fn key(model: &Value) -> Result<Key> {
    Ok((
        model["owner"].as_u64().context("missing model owner")?,
        model["bank"].as_str().context("missing model bank")?.into(),
        model["member"].as_u64().context("missing model member")?,
        model["ordinal"].as_u64().context("missing model ordinal")?,
    ))
}
pub(super) fn canonicalize(model: &mut Value, validated: &Validated, index: usize) -> Result<()> {
    let Some(life) = validated.get(&key(model)?) else {
        return Ok(());
    };
    ensure!(
        (life.first..=life.last).contains(&index),
        "debris model appeared outside its validated life"
    );
    let [x, y, z] = life.origin;
    model["model"]["world"] = json!([1., 0., 0., x, 0., 1., 0., y, 0., 0., 1., z]);
    model["ordinal"] = json!(life.ordinal);
    model["particle_age"] = json!(index - life.first + 1);
    model["heading"] = json!(life.heading);
    Ok(())
}
