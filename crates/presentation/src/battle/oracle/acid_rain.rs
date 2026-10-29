//! Check each rain drop and its floor callback before comparing stochastic splashes.
use super::{Trace, compare::equal};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    EffectOperation, ParticleBirth, ParticleTemplate, ScheduledEvent,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const RAIN_BANK: &str = "7fd0598c911b5fe5467c822140897fe8d65108592ade8a89dc65b40230bd5e82";
const RAIN_PROGRAM: u64 = 1;
const FLOOR_PROGRAM: u64 = 2;
type Clock = (u64, u64);

/// Only independently validated floor callbacks are removed. The parent program,
/// drop declarations, debuffs, actor state, payment and action completion remain.
pub(super) fn retain_gameplay(
    source: &Trace,
    native: &Trace,
    events: &mut [Vec<Value>; 2],
) -> Result<()> {
    let Some(registrations) = source.setup["stored_scenes"]["registrations"].as_array() else {
        return Ok(());
    };
    let registrations: Vec<_> = registrations
        .iter()
        .filter(|entry| entry["bank_sha256"] == RAIN_BANK)
        .collect();
    if registrations.is_empty() {
        return Ok(());
    }
    ensure!(registrations.len() == 1, "ambiguous rain registration");
    let registration = registrations[0];
    ensure!(
        native.setup["stored_scenes"]["registrations"]
            .as_array()
            .is_some_and(|entries| entries.contains(registration)),
        "rain registration differs"
    );
    let (_, bank) = super::content_evidence::registered_scene(registration)?;
    let metadata = super::content_evidence::effect_value(bank)?;
    let evidence = super::content_evidence::Evidence::parse(&registration["effects"])?;
    let contract = Contract::read(
        &metadata,
        evidence.current_template(2)?,
        evidence.current_template(0)?,
    )?;
    let owner = integer(&source.setup, "owner")?;
    ensure!(native.setup["owner"] == owner, "rain owner differs");
    for trace in [source, native] {
        validate_run(trace, owner, &contract)?;
    }
    for events in events {
        events.retain(|event| {
            !(event["owner"] == owner
                && event["bank"] == RAIN_BANK
                && (event["kind"] == "program_started" && event["program"] == FLOOR_PROGRAM
                    || event["kind"] == "declaration_started"
                        && event["member"] == contract.splash))
        });
    }
    Ok(())
}

struct Contract {
    drop: u64,
    splash: u64,
    emissions: BTreeMap<u64, usize>,
    bounds: [[f64; 3]; 3],
    velocity: [f64; 3],
    drop_lifetime: usize,
    splash_lifetime: usize,
    splash_offset: [f64; 3],
}

impl Contract {
    fn read(metadata: &Value, drop_member: u64, splash_member: u64) -> Result<Self> {
        let actors = &metadata["actors"];
        let drop: ParticleTemplate =
            serde_json::from_value(actors[drop_member as usize]["template"].clone())?;
        let splash: ParticleTemplate =
            serde_json::from_value(actors[splash_member as usize]["template"].clone())?;
        let floor: Vec<ScheduledEvent> =
            serde_json::from_value(metadata["programs"][FLOOR_PROGRAM as usize].clone())?;
        ensure!(
            actors[drop_member as usize]["kind"] == "particle"
                && actors[splash_member as usize]["kind"] == "particle"
                && drop.floor_effect == Some(FLOOR_PROGRAM as u16)
                && !drop.follow_origin
                && !drop.follow_orientation
                && drop.state.acceleration == [0.; 3]
                && drop.jerk == [0.; 3]
                && splash.state.velocity == [0.; 3]
                && splash.state.acceleration == [0.; 3]
                && floor
                    == vec![ScheduledEvent {
                        at: 0,
                        operation: EffectOperation::Spawn {
                            particle: splash_member as u8,
                            blend: None,
                            palette: None,
                            birth: Box::new(ParticleBirth::default())
                        }
                    }],
            "unsupported rain callback recipe"
        );
        let program = metadata["programs"][RAIN_PROGRAM as usize]
            .as_array()
            .context("missing rain program")?;
        let offsets =
            &program.first().context("empty rain program")?["operation"]["birth"]["offset"];
        let mut bounds = [[0.; 3]; 3];
        for (axis, bound) in bounds.iter_mut().enumerate() {
            *bound = ["min", "max", "step"]
                .map(|field| offsets[axis][field].as_f64().unwrap_or(f64::NAN));
            ensure!(
                bound.iter().all(|x| x.is_finite()) && bound[0] <= bound[1] && bound[2] > 0.,
                "invalid rain birth range"
            );
        }
        let mut emissions = BTreeMap::new();
        for event in program {
            let operation = &event["operation"];
            let birth = &operation["birth"];
            ensure!(
                operation["kind"] == "spawn"
                    && operation["particle"] == drop_member
                    && birth.as_object().is_some_and(|object| object.len() == 2)
                    && birth["random_draws"] == 3,
                "unsupported rain emission"
            );
            for (axis, bound) in bounds.iter().enumerate() {
                ensure!(["min", "max", "step"].into_iter().zip(bound).all(|(field, value)| birth["offset"][axis][field].as_f64() == Some(*value)), "rain birth ranges change within program");
            }
            // The first scheduler visit executes declarations at both ages 0 and 1.
            *emissions.entry(integer(event, "at")?.max(1)).or_default() += 1;
        }
        let velocity = drop.state.velocity.map(f64::from);
        ensure!(velocity[1] < 0., "rain does not fall");
        Ok(Self {
            drop: drop_member,
            splash: splash_member,
            emissions,
            bounds,
            velocity,
            drop_lifetime: drop.lifetime.context("missing drop lifetime")? as usize,
            splash_lifetime: splash.lifetime.context("missing splash lifetime")? as usize,
            splash_offset: splash.state.offset.map(f64::from),
        })
    }
}

struct Sample<'a> {
    clock: Clock,
    particle: &'a Value,
}

fn validate_run(trace: &Trace, owner: u64, contract: &Contract) -> Result<()> {
    let first = trace.frames.first().context("rain has no frames")?;
    let start = (
        integer(first, "combat_tick")?,
        integer(first, "presentation_tick")?,
    );
    let parent: Vec<_> = trace
        .events
        .iter()
        .filter(|event| {
            event["bank"] == RAIN_BANK
                && event["owner"] == owner
                && event["kind"] == "program_started"
                && event["program"] == RAIN_PROGRAM
        })
        .collect();
    ensure!(
        parent.len() == 1,
        "rain must have one observed parent program"
    );
    let parent = event_clock(parent[0])?;
    let admission = trace
        .frames
        .iter()
        .find(|frame| frame["combat_tick"].as_u64() == Some(start.0 + parent.0))
        .context("rain parent frame is missing")?;
    let actor = admission["actors"]
        .as_array()
        .context("missing rain actors")?
        .iter()
        .find(|actor| actor["actor"] == owner)
        .context("missing rain owner")?;
    let (sin, cos) = actor["heading"]
        .as_f64()
        .context("missing rain heading")?
        .to_radians()
        .sin_cos();
    let velocity = [
        cos * contract.velocity[0] + sin * contract.velocity[2],
        contract.velocity[1],
        cos * contract.velocity[2] - sin * contract.velocity[0],
    ];
    let mut lives = BTreeMap::<u64, Vec<Sample<'_>>>::new();
    for frame in &trace.frames {
        let clock = (
            integer(frame, "combat_tick")?
                .checked_sub(start.0)
                .context("rain clock reversed")?,
            integer(frame, "presentation_tick")?
                .checked_sub(start.1)
                .context("rain display clock reversed")?,
        );
        for particle in frame["particles"]
            .as_array()
            .context("missing rain particles")?
        {
            if particle["bank"] != RAIN_BANK {
                continue;
            }
            ensure!(
                particle["owner"] == owner
                    && (particle["member"] == contract.drop
                        || particle["member"] == contract.splash)
                    && particle["model"].is_null()
                    && particle["owner_bone"].is_null(),
                "unexpected rain particle binding"
            );
            lives
                .entry(integer(particle, "id")?)
                .or_default()
                .push(Sample { clock, particle });
        }
    }
    let mut births = BTreeMap::<Clock, usize>::new();
    let mut crossings = BTreeMap::<Clock, Vec<[f64; 3]>>::new();
    let mut splashes = Vec::new();
    for life in lives.values() {
        let initialized: Vec<_> = life
            .iter()
            .filter(|sample| sample.particle["initialized"] == true)
            .collect();
        let first = initialized
            .first()
            .context("rain particle never initialized")?;
        let member = integer(first.particle, "member")?;
        ensure!(
            life.len() - initialized.len() <= 1
                && life.iter().all(|sample| sample.particle["member"] == member
                    && (sample.particle["initialized"] == true
                        || sample.particle["age"] == 0 && sample.clock.0 + 1 == first.clock.0)),
            "rain particle admission differs"
        );
        let lifetime = if member == contract.drop {
            contract.drop_lifetime
        } else {
            contract.splash_lifetime
        };
        ensure!(
            lifetime > 0 && initialized.len() == lifetime,
            "rain particle lifetime differs"
        );
        let origin = vector(&first.particle["origin"])?;
        let offset = vector(&first.particle["offset"])?;
        if member == contract.drop {
            *births.entry(first.clock).or_default() += 1;
            equal(&json!(origin), &json!([0., 0., 0.]), "rain emission origin")?;
            let birth = [
                offset[0] - velocity[0],
                offset[1] - velocity[1],
                offset[2] - velocity[2],
            ];
            let local = [
                cos * birth[0] - sin * birth[2],
                birth[1],
                cos * birth[2] + sin * birth[0],
            ];
            for (value, [min, max, step]) in local.into_iter().zip(contract.bounds) {
                equal(
                    &json!(value),
                    &json!(value.clamp(min, max)),
                    "rain birth range",
                )?;
                equal(
                    &json!(value),
                    &json!(min + ((value - min) / step).round() * step),
                    "rain birth step",
                )?;
            }
        } else {
            splashes.push((first.clock, origin));
        }
        let mut crossed = false;
        for (age, sample) in initialized.iter().enumerate() {
            ensure!(
                sample.clock == (first.clock.0 + age as u64, first.clock.1 + age as u64)
                    && sample.particle["age"] == age + 1,
                "rain particle clock differs"
            );
            equal(
                &sample.particle["origin"],
                &json!(origin),
                "rain particle origin",
            )?;
            let expected = if member == contract.drop {
                std::array::from_fn(|axis| offset[axis] + velocity[axis] * age as f64)
            } else {
                contract.splash_offset
            };
            equal(
                &sample.particle["offset"],
                &json!(expected),
                "rain particle motion",
            )?;
            let position = vector(&sample.particle["offset"])?;
            if member == contract.drop && !crossed && origin[1] + position[1] < 0. {
                crossings
                    .entry(sample.clock)
                    .or_default()
                    .push(std::array::from_fn(|axis| origin[axis] + position[axis]));
                crossed = true;
            }
        }
    }
    let expected_births: BTreeMap<_, _> = contract
        .emissions
        .iter()
        .map(|(age, count)| ((parent.0 + age, parent.1 + age), *count))
        .collect();
    ensure!(births == expected_births, "rain drop emissions differ");
    let mut programs = BTreeMap::<Clock, usize>::new();
    let mut declarations = BTreeSet::new();
    let mut declaration_count = 0;
    for event in &trace.events {
        if event["bank"] != RAIN_BANK || event["owner"] != owner {
            continue;
        }
        if event["kind"] == "program_started" && event["program"] == FLOOR_PROGRAM {
            *programs.entry(event_clock(event)?).or_default() += 1;
        }
        if event["kind"] == "declaration_started" && event["member"] == contract.splash {
            declarations.insert(event_clock(event)?);
            declaration_count += 1;
        }
    }
    ensure!(
        programs
            == crossings
                .iter()
                .map(|(clock, points)| (*clock, points.len()))
                .collect(),
        "rain floor callbacks differ from strict crossings"
    );
    ensure!(
        declarations == programs.keys().copied().collect()
            && declaration_count == declarations.len(),
        "rain splash declarations differ"
    );
    for (clock, origin) in splashes {
        let candidates = crossings
            .get_mut(&clock)
            .context("rain splash without floor crossing")?;
        let index = candidates
            .iter()
            .position(|point| equal(&json!(point), &json!(origin), "rain splash position").is_ok())
            .context("rain splash origin differs from floor crossing")?;
        candidates.swap_remove(index);
    }
    ensure!(
        crossings.values().all(Vec::is_empty),
        "rain crossing has no splash"
    );
    Ok(())
}

fn integer(value: &Value, key: &str) -> Result<u64> {
    value[key]
        .as_u64()
        .with_context(|| format!("missing rain {key}"))
}
fn vector(value: &Value) -> Result<[f64; 3]> {
    let values: [f64; 3] = serde_json::from_value(value.clone())?;
    ensure!(
        values.iter().all(|value| value.is_finite()),
        "nonfinite rain position"
    );
    Ok(values)
}
fn event_clock(event: &Value) -> Result<Clock> {
    Ok((
        integer(event, "clock")?,
        integer(event, "presentation_clock")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    const DROP: u64 = 2;
    const SPLASH: u64 = 0;

    fn fixture() -> (Trace, Contract) {
        let contract = Contract {
            drop: DROP,
            splash: SPLASH,
            emissions: BTreeMap::from([(1, 2)]),
            bounds: [[0., 0., 1.], [780., 845., 65.], [0., 0., 1.]],
            velocity: [0., -65., 0.],
            drop_lifetime: 13,
            splash_lifetime: 25,
            splash_offset: [0., 1., 0.],
        };
        let frames = (0..39)
            .map(|clock| {
                let mut particles = Vec::new();
                if (1..=13).contains(&clock) {
                    for (id, height) in [(1, 780.), (2, 845.)] {
                        particles.push(json!({"id":id,"owner":0,"bank":RAIN_BANK,"member":DROP,
                        "initialized":true,"age":clock,"origin":[0.,0.,0.],
                        "offset":[0.,height-65.*clock as f64,0.],"owner_bone":null,"model":null}));
                    }
                }
                if (13..38).contains(&clock) {
                    particles.push(json!({"id":3,"owner":0,"bank":RAIN_BANK,"member":SPLASH,
                    "initialized":true,"age":clock-12,"origin":[0.,-65.,0.],
                    "offset":[0.,1.,0.],"owner_bone":null,"model":null}));
                }
                json!({"combat_tick":clock,"presentation_tick":clock,"particles":particles,
                "actors":[{"actor":0,"heading":0.}]})
            })
            .collect();
        let events = vec![
            json!({"kind":"program_started","owner":0,"bank":RAIN_BANK,"program":RAIN_PROGRAM,"clock":0,"presentation_clock":0}),
            json!({"kind":"program_started","owner":0,"bank":RAIN_BANK,"program":FLOOR_PROGRAM,"clock":13,"presentation_clock":13}),
            json!({"kind":"declaration_started","owner":0,"bank":RAIN_BANK,"member":SPLASH,"clock":13,"presentation_clock":13}),
        ];
        (
            Trace {
                case: "rain".into(),
                setup: Value::Null,
                frames,
                events,
                assessment: Value::Null,
                incomplete_observations: vec![],
            },
            contract,
        )
    }

    #[test]
    fn rain_callbacks_require_strict_crossings_and_complete_particle_lifetimes() {
        let (trace, contract) = fixture();
        assert!(validate_run(&trace, 0, &contract).is_ok());
        for mutation in 0..6 {
            let (mut trace, contract) = fixture();
            match mutation {
                0 => {
                    trace.events.remove(1);
                }
                1 => trace.events.push(trace.events[1].clone()), // The other drop reaches zero, not below it.
                2 => trace.frames[5]["particles"][0]["offset"][1] = json!(0.),
                3 => trace.frames[13]["particles"][2]["origin"][0] = json!(5.),
                4 => {
                    trace.frames[37]["particles"] = json!([]);
                }
                5 => {
                    // Omit a whole drop, including its birth and retirement.
                    for frame in &mut trace.frames {
                        frame["particles"]
                            .as_array_mut()
                            .unwrap()
                            .retain(|particle| particle["id"] != 2);
                    }
                }
                _ => unreachable!(),
            }
            assert!(
                validate_run(&trace, 0, &contract).is_err(),
                "accepted mutation {mutation}"
            );
        }
    }
}
