//! Admit authored initial rotations while retaining each instance's later transform.
use super::Trace;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::ValueRange;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Key = (u64, String, u64, u64);
pub(super) type Rotations = BTreeMap<Key, Rotation>;

pub(super) struct Rotation {
    initial: [f64; 3],
    offset: [f64; 3],
    ordinal: u64,
    motion: Option<Motion>,
    scaled_pitch: bool,
}

struct Motion {
    vertical_velocity: f64,
    frames: BTreeMap<usize, u64>,
}

#[derive(Deserialize)]
struct Template {
    path: String,
    sha256: String,
    member: u64,
    program: u64,
    modifier_words: Vec<u16>,
}

#[derive(Deserialize)]
struct Pitch {
    owner: u64,
    bank: String,
    member: u64,
    ordinal: u64,
    birth_clock: u64,
    heading: f64,
    first_step: f64,
    template: Template,
}

#[derive(Deserialize)]
struct Yaw {
    owner: u64,
    bank: String,
    member: u64,
    ordinal: u64,
    birth_clock: u64,
    program_clock: u64,
    first_step: f64,
    template: Template,
}

pub(super) fn validate(source: &Trace, native: &Trace) -> Result<[Rotations; 2]> {
    let descriptors: Vec<Pitch> = source
        .setup
        .get("effect_random_pitch")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let mut result = [BTreeMap::new(), BTreeMap::new()];
    for pitch in descriptors {
        let words = &pitch.template.modifier_words;
        ensure!(
            !pitch.template.path.is_empty()
                && digest(&pitch.template.sha256)
                && digest(&pitch.bank)
                && pitch.member == pitch.template.member
                && pitch.heading.is_finite()
                && pitch.first_step.is_finite()
                && words.len() == 5
                && words[0] == 11
                && words[1] == 0x58
                && words[3] == 0
                && words[4] == u16::MAX,
            "random pitch lacks an independently bound scalar modifier"
        );
        let extent = (i32::from(words[2] as i16).abs() - 1) as f64 * 0.1;
        ensure!(
            (0. ..180.).contains(&extent),
            "random pitch cannot be resolved from one matrix turn"
        );
        let id = (pitch.owner, pitch.bank.clone(), pitch.member, pitch.ordinal);
        for (side, trace) in [source, native].into_iter().enumerate() {
            let start = trace.frames.first().context("missing admission frame")?["combat_tick"]
                .as_u64()
                .context("missing admission clock")?;
            let (frame, model) = trace
                .frames
                .iter()
                .find_map(|frame| {
                    frame["effect_models"].as_array()?.iter().find_map(|model| {
                        (key(model).ok().as_ref() == Some(&id)).then_some((frame, model))
                    })
                })
                .context("random-pitch model was not observed")?;
            ensure!(
                frame["combat_tick"]
                    .as_u64()
                    .and_then(|clock| clock.checked_sub(start))
                    == Some(pitch.birth_clock)
                    && trace.events.iter().any(|event| {
                        event["kind"] == "program_started"
                            && event["owner"] == pitch.owner
                            && event["bank"] == pitch.bank
                            && event["program"] == pitch.template.program
                            && event["clock"] == pitch.birth_clock
                    }),
                "random-pitch birth clock or program differs"
            );
            let world = world(model)?;
            let (sine, cosine) = pitch.heading.to_radians().sin_cos();
            let observed = (sine * world[1] + cosine * world[9])
                .atan2(world[5])
                .to_degrees();
            let initial = (observed - pitch.first_step + 180.).rem_euclid(360.) - 180.;
            ensure!(
                initial.abs() <= extent + 0.001
                    && (initial * 10. - (initial * 10.).round()).abs() <= 0.01,
                "initial model pitch {initial} is outside the authored tenth-degree choices"
            );
            ensure!(
                result[side]
                    .insert(
                        id.clone(),
                        Rotation {
                            initial: [initial, 0., 0.],
                            offset: [0.; 3],
                            ordinal: pitch.ordinal,
                            motion: None,
                            scaled_pitch: false,
                        }
                    )
                    .is_none(),
                "duplicate random-pitch model binding"
            );
        }
    }
    let yaws: Vec<Yaw> = source
        .setup
        .get("effect_random_yaw")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let families: Vec<_> = yaws
        .iter()
        .map(|yaw| (yaw.owner, yaw.bank.clone(), yaw.member))
        .collect();
    let births = [births(source, &families)?, births(native, &families)?];
    ensure!(
        births.iter().all(|births| births.len() == yaws.len()),
        "random-yaw declarations do not cover every observed instance"
    );
    for yaw in yaws {
        ensure!(
            !yaw.template.path.is_empty()
                && digest(&yaw.template.sha256)
                && digest(&yaw.bank)
                && yaw.member == yaw.template.member
                && yaw.first_step.is_finite()
                && yaw.program_clock <= yaw.birth_clock
                && yaw.template.modifier_words == [11, 0x5c, 1800, 0, u16::MAX],
            "random yaw lacks an independently bound scalar modifier"
        );
        for trace in [source, native] {
            ensure!(
                trace.events.iter().any(|event| {
                    event["kind"] == "program_started"
                        && event["owner"] == yaw.owner
                        && event["bank"] == yaw.bank
                        && event["program"] == yaw.template.program
                        && event["clock"] == yaw.program_clock
                }),
                "random-yaw parent program differs"
            );
        }
        let id = (yaw.owner, yaw.bank.clone(), yaw.member, yaw.ordinal);
        let source_birth = births[0]
            .get(&id)
            .context("random-yaw source model was not observed")?;
        ensure!(
            source_birth.clock == yaw.birth_clock,
            "random-yaw source birth clock differs"
        );
        let source_rotation = yaw_rotation(source_birth.model, &yaw)?;
        let mut expected = source_birth.model.clone();
        apply(&mut expected, &source_rotation, 0)?;
        let mut candidates = Vec::new();
        for (native_id, birth) in &births[1] {
            if native_id.0 != yaw.owner
                || native_id.1 != yaw.bank
                || native_id.2 != yaw.member
                || birth.clock != yaw.birth_clock
                || result[1].contains_key(native_id)
            {
                continue;
            }
            let rotation = yaw_rotation(birth.model, &yaw)?;
            let mut observed = birth.model.clone();
            apply(&mut observed, &rotation, 0)?;
            if super::compare::equal(&expected, &observed, "random-yaw birth").is_ok() {
                candidates.push((native_id.clone(), rotation));
            }
        }
        ensure!(
            candidates.len() == 1,
            "random-yaw birth owner={} member={} ordinal={} clock={} has {} native matches",
            yaw.owner,
            yaw.member,
            yaw.ordinal,
            yaw.birth_clock,
            candidates.len()
        );
        let (native_id, rotation) = candidates.pop().unwrap();
        ensure!(
            result[0].insert(id, source_rotation).is_none(),
            "duplicate random-yaw source binding"
        );
        result[1].insert(native_id, rotation);
    }
    validate_choices(source, native, &mut result)?;
    Ok(result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choices {
    owner: u64,
    bank: String,
    member: u64,
    ordinal: u64,
    birth_clock: u64,
    program_clock: u64,
    heading: f64,
    origin: Option<[f64; 3]>,
    first_step: [f64; 3],
    first_offset: [f64; 3],
    offsets: [Option<ValueRange>; 3],
    angles: [Option<ValueRange>; 3],
    angle_additions: Option<[f32; 3]>,
    vertical_velocity: Option<ValueRange>,
    model: ValueRange,
    lifetime: Option<u64>,
    template: Template,
}

fn validate_choices(source: &Trace, native: &Trace, result: &mut [Rotations; 2]) -> Result<()> {
    let choices: Vec<Choices> = source
        .setup
        .get("effect_random_births")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let families: Vec<_> = choices
        .iter()
        .map(|choice| (choice.owner, choice.bank.clone(), choice.member))
        .collect();
    let observed = [births(source, &families)?, births(native, &families)?];
    ensure!(
        observed.iter().all(|births| births.len() == choices.len()),
        "random birth descriptors do not cover every instance"
    );
    let mut bindings = Vec::new();
    for choice in choices {
        choice.model.validate()?;
        ensure!(
            choice.model.draw.is_none() && !choice.model.reverse,
            "shared or reversed model choices are unsupported"
        );
        ensure!(
            !choice.template.path.is_empty()
                && digest(&choice.template.sha256)
                && digest(&choice.bank)
                && choice.template.member == choice.member
                && choice.program_clock <= choice.birth_clock
                && choice.heading.is_finite()
                && choice
                    .first_step
                    .iter()
                    .chain(&choice.first_offset)
                    .all(|x| x.is_finite())
                && choice.offsets[1].is_none()
                && (choice.origin.is_some() || choice.offsets.iter().all(Option::is_none))
                && (choice.angles[1].is_none() || choice.first_step[2] == 0.)
                && choice.lifetime.is_none_or(|lifetime| lifetime > 0),
            "unsupported random model birth"
        );
        let mut modifiers = Vec::new();
        for (base, ranges) in [(0x34, &choice.offsets), (0x58, &choice.angles)] {
            for (axis, range) in ranges.iter().enumerate() {
                if let Some(range) = range {
                    range.validate()?;
                    ensure!(
                        range.draw.is_none() && !range.reverse,
                        "shared or reversed scalar choices are unsupported"
                    );
                    let divisor = (f64::from(range.max) * 10.).round() + 1.;
                    ensure!(
                        range.min == -range.max
                            && range.step == 0.1
                            && (2. ..=if base == 0x58 { 3600. } else { 1800. }).contains(&divisor),
                        "unsupported scalar birth choices"
                    );
                    modifiers.push(vec![11, base + axis as u16 * 4, divisor as u16, 0]);
                }
            }
        }
        if let Some(additions) = choice.angle_additions {
            for (axis, addition) in additions.into_iter().enumerate() {
                ensure!(addition.is_finite(), "invalid fixed angle addition");
                if addition != 0. {
                    let bits = addition.to_bits();
                    modifiers.push(vec![
                        8,
                        0x58 + axis as u16 * 4,
                        (bits >> 16) as u16,
                        bits as u16,
                        0,
                        0,
                    ]);
                }
            }
        }
        if let Some(range) = choice.vertical_velocity {
            range.validate()?;
            let divisor = (f64::from(range.max) * 10.).round() + 1.;
            ensure!(
                choice.origin.is_some()
                    && choice.offsets.iter().all(Option::is_none)
                    && range.draw.is_none()
                    && !range.reverse
                    && range.min == -range.max
                    && range.step == 0.1
                    && (2. ..=1800.).contains(&divisor),
                "unsupported vertical birth velocity"
            );
            modifiers.push(vec![11, 0x44, divisor as u16, 0]);
        }
        let model = choice.model;
        ensure!(
            model.min >= 0.
                && model.max <= 255.
                && model.min.fract() == 0.
                && model.max.fract() == 0.
                && (model.min == model.max || model.step == 1.),
            "invalid model choices"
        );
        if model.min != model.max {
            modifiers.push(vec![
                2,
                0x7ffc,
                (model.max - model.min) as u16 + 1,
                0,
                12,
                0xd4,
                0x7ffc,
                0,
            ]);
        } else {
            let assignment = vec![0, 0x7ffc, model.min as u16, 0, 10, 0xd4, 0x7ffc, 0];
            if choice
                .template
                .modifier_words
                .windows(assignment.len())
                .any(|words| words == assignment.as_slice())
            {
                modifiers.push(assignment);
            }
        }
        // Each block writes a distinct field with an independent draw. Their
        // authored order may differ; the model draw and assignment stay atomic.
        // Raw particle angles, rather than Euler decomposition, admit full turns.
        let mut words = choice.template.modifier_words.as_slice();
        while words != [u16::MAX] {
            // A remainder modulo one is always zero. Admit its additions only
            // to known floating-point fields; other operations still match below.
            if let Some(mut remaining) = words.strip_prefix(&[11, 0x7ff8, 1, 0]) {
                let before = remaining.len();
                while let [8, target, 0, 0, 0x7ff8, 0, rest @ ..] = remaining {
                    ensure!(
                        matches!(
                            *target,
                            0x34 | 0x38
                                | 0x3c
                                | 0x40
                                | 0x44
                                | 0x48
                                | 0x58
                                | 0x5c
                                | 0x60
                                | 0xb0
                                | 0xb4
                                | 0xb8
                        ),
                        "zero birth addition targets an unsupported field"
                    );
                    remaining = rest;
                }
                ensure!(remaining.len() < before, "zero birth draw has no additions");
                words = remaining;
                continue;
            }
            let (index, length) = modifiers
                .iter()
                .enumerate()
                .find_map(|(index, modifier)| {
                    if words.starts_with(modifier) {
                        return Some((index, modifier.len()));
                    }
                    if modifier.len() == 4 && modifier[0] == 11 {
                        let addition =
                            [11, 0x7ff8, modifier[2], 0, 8, modifier[1], 0, 0, 0x7ff8, 0];
                        if words.starts_with(&addition) {
                            return Some((index, addition.len()));
                        }
                    }
                    if modifier.len() == 8 && modifier[0] == 2 {
                        if model.min == 0. {
                            let mut assignment = modifier.clone();
                            assignment[4] = 10;
                            if words.starts_with(&assignment) {
                                return Some((index, assignment.len()));
                            }
                        }
                        let assignment = [
                            2,
                            0x7ffc,
                            modifier[2],
                            0,
                            3,
                            0x7ffc,
                            model.min as u16,
                            0,
                            10,
                            0xd4,
                            0x7ffc,
                            0,
                        ];
                        if words.starts_with(&assignment) {
                            return Some((index, assignment.len()));
                        }
                    }
                    None
                })
                .context("birth choices differ from the independently bound modifier")?;
            modifiers.remove(index);
            words = &words[length..];
        }
        ensure!(
            modifiers.is_empty(),
            "birth modifier omitted an authored choice"
        );
        for trace in [source, native] {
            ensure!(
                trace
                    .events
                    .iter()
                    .any(|event| event["kind"] == "program_started"
                        && event["owner"] == choice.owner
                        && event["bank"] == choice.bank
                        && event["program"] == choice.template.program
                        && event["clock"] == choice.program_clock),
                "random birth parent program differs"
            );
        }
        let source_id = (
            choice.owner,
            choice.bank.clone(),
            choice.member,
            choice.ordinal,
        );
        let source_birth = observed[0]
            .get(&source_id)
            .context("source birth ordinal differs")?;
        ensure!(
            source_birth.clock == choice.birth_clock,
            "source birth clock differs"
        );
        let source_rotation = choice_rotation(source, &source_id, source_birth, &choice)?;
        let mut candidates: Vec<_> = observed[1]
            .iter()
            .filter(|(id, birth)| {
                id.0 == choice.owner
                    && id.1 == choice.bank
                    && id.2 == choice.member
                    && birth.clock == choice.birth_clock
            })
            .filter_map(|(id, birth)| {
                choice_rotation(native, id, birth, &choice)
                    .ok()
                    .map(|rotation| (id.clone(), rotation))
            })
            .collect();
        if candidates.len() > 1 {
            // Detached simultaneous births have no semantic allocation order.
            // Qualify one fixed mapping using every frame, never a new pairing
            // per frame. Projectile-following ambiguity still needs its own
            // independently qualified trajectory normalization.
            ensure!(choice.origin.is_some(), "ambiguous attached random birth");
            candidates.retain(|(id, rotation)| {
                same_history(
                    [source, native],
                    [&source_id, id],
                    [source_birth, &observed[1][id]],
                    [&source_rotation, rotation],
                )
                .is_ok()
            });
        }
        ensure!(
            !candidates.is_empty(),
            "random birth has no matching lifetime"
        );
        ensure!(
            result[0].insert(source_id, source_rotation).is_none(),
            "duplicate random birth binding"
        );
        bindings.push(candidates);
    }
    let mut assigned = BTreeMap::new();
    for index in 0..bindings.len() {
        ensure!(
            assign_birth(index, &bindings, &mut assigned, &mut BTreeSet::new()),
            "random birth histories have no one-to-one lifetime matching"
        );
    }
    for (id, index) in assigned {
        let candidate = bindings[index]
            .iter()
            .position(|(key, _)| key == &id)
            .unwrap();
        let (_, rotation) = bindings[index].swap_remove(candidate);
        ensure!(
            result[1].insert(id, rotation).is_none(),
            "duplicate random birth binding"
        );
    }
    Ok(())
}

fn assign_birth(
    index: usize,
    candidates: &[Vec<(Key, Rotation)>],
    assigned: &mut BTreeMap<Key, usize>,
    visited: &mut BTreeSet<Key>,
) -> bool {
    for (id, _) in &candidates[index] {
        if !visited.insert(id.clone()) {
            continue;
        }
        if assigned
            .get(id)
            .copied()
            .is_none_or(|previous| assign_birth(previous, candidates, assigned, visited))
        {
            assigned.insert(id.clone(), index);
            return true;
        }
    }
    false
}

fn same_history(
    traces: [&Trace; 2],
    ids: [&Key; 2],
    births: [&Birth<'_>; 2],
    rotations: [&Rotation; 2],
) -> Result<()> {
    ensure!(
        births[0].first_frame == births[1].first_frame
            && births[0].last_frame == births[1].last_frame,
        "random model birth or retirement differs"
    );
    for frame in births[0].first_frame..=births[0].last_frame {
        let mut models = [Value::Null, Value::Null];
        for side in 0..2 {
            models[side] = traces[side].frames[frame]["effect_models"]
                .as_array()
                .context("missing models")?
                .iter()
                .find(|model| key(model).ok().as_ref() == Some(ids[side]))
                .context("random model disappeared")?
                .clone();
            apply(&mut models[side], rotations[side], frame)?;
        }
        super::compare::equal(&models[0], &models[1], "random model lifetime")?;
    }
    Ok(())
}

fn admit(value: f64, range: ValueRange) -> Result<()> {
    range.validate()?;
    let (min, max, step) = (
        f64::from(range.min),
        f64::from(range.max),
        f64::from(range.step),
    );
    if min == max {
        super::compare::equal(&Value::from(value), &Value::from(min), "fixed birth value")?;
    } else {
        ensure!(
            step > 0. && value.is_finite(),
            "invalid discrete birth choice"
        );
        let index = (value - min) / step;
        let nearest = index.round();
        ensure!(
            nearest >= 0.
                && min + nearest * step <= max + step * 0.01
                && (index - nearest).abs() <= 0.01,
            "birth value {value} is outside authored discrete choices"
        );
    }
    Ok(())
}

fn choice_rotation(
    trace: &Trace,
    id: &Key,
    birth: &Birth<'_>,
    choice: &Choices,
) -> Result<Rotation> {
    let first = &trace.frames[birth.first_frame];
    let candidates: Vec<_> = first["particles"]
        .as_array()
        .context("missing particles")?
        .iter()
        .filter(|particle| {
            ["owner", "bank", "member", "model"]
                .iter()
                .all(|key| particle[*key] == birth.model[*key])
        })
        .collect();
    ensure!(
        candidates.len() == 1,
        "model birth lacks a unique particle identity"
    );
    let particle = candidates[0];
    let particle_id = particle["id"]
        .as_u64()
        .context("missing particle identity")?;
    let angles: [f64; 3] = serde_json::from_value(particle["angles"].clone())?;
    let offset: [f64; 3] = serde_json::from_value(particle["offset"].clone())?;
    let delta = std::array::from_fn::<_, 3, _>(|axis| offset[axis] - choice.first_offset[axis]);
    let (sin, cos) = choice.heading.to_radians().sin_cos();
    let local = [
        cos * delta[0] - sin * delta[2],
        delta[1],
        sin * delta[0] + cos * delta[2],
    ];
    let mut rotation = Rotation {
        initial: [0.; 3],
        offset: [0.; 3],
        ordinal: choice.ordinal,
        motion: None,
        scaled_pitch: choice.angles[0].is_some(),
    };
    let base_angles = [0., choice.heading, 0.];
    let mut admitted_offset = [0.; 3];
    for axis in 0..3 {
        if let Some(range) = choice.angles[axis] {
            let initial = angles[axis] - choice.first_step[axis];
            admit(initial, range)?;
            rotation.initial[axis] = initial;
        } else {
            super::compare::equal(
                &Value::from(angles[axis]),
                &Value::from(base_angles[axis] + choice.first_step[axis]),
                "deterministic birth angle",
            )?;
        }
        if axis == 1
            && let Some(range) = choice.vertical_velocity
        {
            admit(local[axis], range)?;
            rotation.motion = Some(Motion {
                vertical_velocity: local[axis],
                frames: BTreeMap::new(),
            });
        } else if let Some(range) = choice.offsets[axis] {
            admit(local[axis], range)?;
            admitted_offset[axis] = local[axis];
        } else {
            super::compare::equal(
                &Value::from(local[axis]),
                &Value::from(0.),
                "deterministic birth offset",
            )?;
        }
    }
    rotation.offset = [
        cos * admitted_offset[0] + sin * admitted_offset[2],
        0.,
        cos * admitted_offset[2] - sin * admitted_offset[0],
    ];
    let slot = particle["model_slot"]
        .as_u64()
        .context("missing actual model choice")?;
    admit(slot as f64, choice.model)?;
    ensure!(
        choice
            .lifetime
            .is_none_or(|lifetime| birth.last_frame - birth.first_frame + 1 == lifetime as usize),
        "random model lifetime differs"
    );
    for (index, frame) in trace
        .frames
        .iter()
        .enumerate()
        .take(birth.last_frame + 1)
        .skip(birth.first_frame)
    {
        let model = frame["effect_models"]
            .as_array()
            .context("missing models")?
            .iter()
            .find(|model| key(model).ok().as_ref() == Some(id))
            .context("random model disappeared")?;
        let particles: Vec<_> = frame["particles"]
            .as_array()
            .context("missing particles")?
            .iter()
            .filter(|particle| particle["id"].as_u64() == Some(particle_id))
            .collect();
        ensure!(
            particles.len() == 1,
            "random model identity changed during its lifetime"
        );
        let particle = particles[0];
        ensure!(
            particle["initialized"] == true
                && particle["model_slot"].as_u64() == Some(slot)
                && ["owner", "bank", "member", "model"]
                    .iter()
                    .all(|field| particle[*field] == model[*field])
                && model["model"]["clip"].is_null()
                && model["model"]["frame"].is_null()
                && if choice.origin.is_some() {
                    model["attachment"].is_null()
                } else {
                    model["attachment"]["kind"] == "projectile"
                },
            "random model binding changed"
        );
        if rotation.scaled_pitch {
            positive_scale(model, serde_json::from_value(particle["angles"].clone())?)?;
        }
        if let Some(origin) = choice.origin {
            super::compare::equal(
                &particle["origin"],
                &serde_json::to_value(origin)?,
                "retained model origin",
            )?;
        }
        if let Some(motion) = &mut rotation.motion {
            let age = particle["age"]
                .as_u64()
                .context("missing particle motion age")?;
            let previous = motion.frames.last_key_value().map(|(_, age)| *age);
            ensure!(
                previous.map_or(age == 1, |previous| age == previous || age == previous + 1),
                "particle motion age is discontinuous"
            );
            super::compare::equal(
                &particle["offset"][1],
                &Value::from(choice.first_offset[1] + motion.vertical_velocity * age as f64),
                "retained vertical birth velocity",
            )?;
            motion.frames.insert(index, age);
        }
    }
    Ok(rotation)
}

struct Birth<'a> {
    clock: u64,
    first_frame: usize,
    last_frame: usize,
    model: &'a Value,
}

fn births<'a>(
    trace: &'a Trace,
    families: &[(u64, String, u64)],
) -> Result<BTreeMap<Key, Birth<'a>>> {
    let mut births: BTreeMap<Key, Birth<'a>> = BTreeMap::new();
    if families.is_empty() {
        return Ok(births);
    }
    let start = trace.frames.first().context("missing admission frame")?["combat_tick"]
        .as_u64()
        .context("missing admission clock")?;
    for (index, frame) in trace.frames.iter().enumerate() {
        for model in frame["effect_models"]
            .as_array()
            .context("missing effect models")?
        {
            let id = key(model)?;
            if !families
                .iter()
                .any(|family| id.0 == family.0 && id.1 == family.1 && id.2 == family.2)
            {
                continue;
            }
            if let Some(birth) = births.get_mut(&id) {
                ensure!(
                    birth.last_frame + 1 == index,
                    "random-yaw identity duplicated or reappeared after retirement"
                );
                birth.last_frame = index;
            } else {
                let clock = frame["combat_tick"]
                    .as_u64()
                    .and_then(|clock| clock.checked_sub(start))
                    .context("invalid model birth clock")?;
                births.insert(
                    id,
                    Birth {
                        clock,
                        first_frame: index,
                        last_frame: index,
                        model,
                    },
                );
            }
        }
    }
    Ok(births)
}

fn yaw_rotation(model: &Value, yaw: &Yaw) -> Result<Rotation> {
    ensure!(
        model.get("attachment").is_some_and(Value::is_null),
        "random-yaw model is not detached"
    );
    let initial_world = world(model)?;
    let observed = (-initial_world[8]).atan2(initial_world[0]).to_degrees();
    let initial = (observed - yaw.first_step + 180.).rem_euclid(360.) - 180.;
    ensure!(
        initial.abs() <= 179.901 && (initial * 10. - (initial * 10.).round()).abs() <= 0.01,
        "initial model yaw {initial} is outside the authored tenth-degree choices"
    );
    let rotation = Rotation {
        initial: [0., initial, 0.],
        offset: [0.; 3],
        ordinal: yaw.ordinal,
        motion: None,
        scaled_pitch: false,
    };
    let mut model = model.clone();
    apply(&mut model, &rotation, 0)?;
    let canonical = world(&model)?;
    let scale = initial_world[0].hypot(initial_world[8]);
    ensure!(scale > 0., "random-yaw model has nonpositive scale");
    let (sine, cosine) = yaw.first_step.to_radians().sin_cos();
    let expected = [
        cosine * scale,
        -sine * scale,
        0.,
        initial_world[3],
        0.,
        0.,
        scale,
        initial_world[7],
        -sine * scale,
        -cosine * scale,
        0.,
        initial_world[11],
    ];
    super::compare::equal(
        &serde_json::to_value(expected)?,
        &serde_json::to_value(canonical)?,
        "random-yaw birth pitch, roll or scale",
    )?;
    Ok(rotation)
}

/// Remove only the admitted constant draw, and retain the birth-time identity
/// mapping for the full lifetime. Translation and later angular motion stay strict.
pub(super) fn canonicalize(model: &mut Value, rotations: &Rotations, frame: usize) -> Result<()> {
    let Some(rotation) = rotations.get(&key(model)?) else {
        return Ok(());
    };
    apply(model, rotation, frame)
}

fn apply(model: &mut Value, rotation: &Rotation, frame: usize) -> Result<()> {
    let mut world = world(model)?;
    // Pitch rotates between the Y/Z basis vectors, not their scale magnitudes.
    // Positive scales were independently checked against retained particle angles.
    let [sy, sz] = if rotation.scaled_pitch {
        [1, 2].map(|column| {
            (0..3)
                .map(|row| world[row * 4 + column].powi(2))
                .sum::<f64>()
                .sqrt()
        })
    } else {
        [1.; 2]
    };
    // Inverse birth roll and yaw act on the left; inverse pitch acts on the right.
    // Only constant birth choices are removed, never the subsequent motion.
    for (angle, first, second) in [(rotation.initial[2], 0, 4), (rotation.initial[1], 8, 0)] {
        let (sine, cosine) = angle.to_radians().sin_cos();
        for column in 0..3 {
            let a = world[first + column];
            let b = world[second + column];
            world[first + column] = cosine * a + sine * b;
            world[second + column] = cosine * b - sine * a;
        }
    }
    if let Some(motion) = &rotation.motion {
        let age = motion
            .frames
            .get(&frame)
            .context("missing vertical motion age")?;
        world[7] -= motion.vertical_velocity * *age as f64;
        model["motion_age"] = (*age).into();
    }
    let (sine, cosine) = rotation.initial[0].to_radians().sin_cos();
    for row in 0..3 {
        let y = world[row * 4 + 1] / sy;
        let z = world[row * 4 + 2] / sz;
        world[row * 4 + 1] = (cosine * y - sine * z) * sy;
        world[row * 4 + 2] = (sine * y + cosine * z) * sz;
        world[row * 4 + 3] -= rotation.offset[row];
    }
    model["model"]["world"] = serde_json::to_value(world)?;
    model["ordinal"] = rotation.ordinal.into();
    Ok(())
}

fn positive_scale(model: &Value, angles: [f64; 3]) -> Result<()> {
    let [(sx, cx), (sy, cy), (sz, cz)] = angles.map(|angle| angle.to_radians().sin_cos());
    let basis = [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ];
    let world = world(model)?;
    for column in 0..3 {
        let scale = (0..3)
            .map(|row| world[row * 4 + column] * basis[row][column])
            .sum::<f64>();
        ensure!(
            scale.is_finite() && scale > 0.,
            "ambiguous random-pitch scale"
        );
        for row in 0..3 {
            super::compare::equal(
                &Value::from(world[row * 4 + column]),
                &Value::from(scale * basis[row][column]),
                "random-pitch matrix differs from retained angles",
            )?;
        }
    }
    Ok(())
}

fn key(model: &Value) -> Result<Key> {
    Ok((
        model["owner"].as_u64().context("missing model owner")?,
        model["bank"].as_str().context("missing model bank")?.into(),
        model["member"].as_u64().context("missing model member")?,
        model["ordinal"].as_u64().context("missing model ordinal")?,
    ))
}

fn world(model: &Value) -> Result<[f64; 12]> {
    let world: [f64; 12] = serde_json::from_value(model["model"]["world"].clone())?;
    ensure!(
        world.iter().all(|value| value.is_finite()),
        "invalid model world"
    );
    Ok(world)
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
