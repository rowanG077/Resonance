//! Admit authored initial rotations while retaining each instance's later transform.
use super::Trace;
use anyhow::{Context as ContextExt, Result, ensure};
use resonance_content::battle_effect::ValueRange;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Key = (u64, String, u64, u64);
pub(super) type Rotations = BTreeMap<Key, Rotation>;

#[derive(Default)]
pub(super) struct Rotation {
    initial: [f64; 3],
    offset: [f64; 3],
    ordinal: u64,
    motion: Option<Motion>,
    spin: Option<Spin>,
    relative: Option<Relative>,
    scaled_pitch: bool,
}

struct Relative {
    basis: [[f64; 3]; 3],
    frames: BTreeMap<usize, u64>,
}

struct Spin {
    first: [f64; 3],
    velocity: [f64; 3],
    frames: BTreeMap<usize, (u64, [f64; 3])>,
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
    fixed_model: Option<u64>,
    template: Template,
}

pub(super) fn validate(source: &Trace, native: &Trace) -> Result<[Rotations; 2]> {
    let descriptors: Vec<Pitch> =
        super::optional_field(&source.setup, "effect_random_pitch")?.unwrap_or_default();
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
                            ordinal: pitch.ordinal,
                            ..Rotation::default()
                        }
                    )
                    .is_none(),
                "duplicate random-pitch model binding"
            );
        }
    }
    let yaws: Vec<Yaw> =
        super::optional_field(&source.setup, "effect_random_yaw")?.unwrap_or_default();
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
        let mut modifier = vec![11, 0x5c, 1800, 0];
        if let Some(slot) = yaw.fixed_model {
            ensure!(slot <= 255, "invalid fixed yaw model slot");
            modifier.extend([0, 0x7ffc, slot as u16, 0, 10, 0xd4, 0x7ffc, 0]);
        }
        modifier.push(u16::MAX);
        ensure!(
            !yaw.template.path.is_empty()
                && digest(&yaw.template.sha256)
                && digest(&yaw.bank)
                && yaw.member == yaw.template.member
                && yaw.first_step.is_finite()
                && yaw.program_clock <= yaw.birth_clock
                && yaw.template.modifier_words == modifier,
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
        if let Some(slot) = yaw.fixed_model {
            yaw_model_binding(source, &id, source_birth, slot, true)?;
        }
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
            if yaw.fixed_model.is_some_and(|slot| {
                yaw_model_binding(native, native_id, birth, slot, false).is_err()
            }) {
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
    validate_relative(source, native, &mut result)?;
    match_detached_lifetimes(source, native, &mut result)?;
    Ok(result)
}

/// Allocation order is not visible identity. Reuse the admitted-birth lifetime
/// matcher without removing any transform for unqualified detached models.
/// A failed family remains untouched so the ordinary frame comparison reports
/// its geometry, missing instance or retirement difference.
fn match_detached_lifetimes(
    source: &Trace,
    native: &Trace,
    result: &mut [Rotations; 2],
) -> Result<()> {
    let mut families = BTreeSet::new();
    for trace in [source, native] {
        for frame in &trace.frames {
            for model in frame["effect_models"]
                .as_array()
                .context("missing effect models")?
            {
                if model.get("attachment").is_some_and(Value::is_null) {
                    let id = key(model)?;
                    families.insert((id.0, id.1, id.2));
                }
            }
        }
    }
    let families: Vec<_> = families.into_iter().collect();
    let observed = [births(source, &families)?, births(native, &families)?];
    let unchanged = |ordinal| Rotation {
        ordinal,
        ..Rotation::default()
    };
    for family in families {
        let eligible = |side: usize| {
            observed[side]
                .iter()
                .filter(|(id, birth)| {
                    (id.0, id.1.as_str(), id.2) == (family.0, family.1.as_str(), family.2)
                        && birth.model.get("attachment").is_some_and(Value::is_null)
                        && !result[side].contains_key(*id)
                })
                .collect::<Vec<_>>()
        };
        let sources = eligible(0);
        let natives = eligible(1);
        if sources.is_empty() || sources.len() != natives.len() {
            continue;
        }
        let candidates: Vec<Vec<_>> = sources
            .iter()
            .map(|(id, birth)| {
                natives
                    .iter()
                    .filter_map(|(other, other_birth)| {
                        let rotation = unchanged(id.3);
                        same_history(
                            [source, native],
                            [id, other],
                            [birth, other_birth],
                            [&rotation, &rotation],
                        )
                        .is_ok()
                        .then(|| ((*other).clone(), rotation))
                    })
                    .collect()
            })
            .collect();
        let mut assigned = BTreeMap::new();
        if !(0..candidates.len())
            .all(|index| assign_birth(index, &candidates, &mut assigned, &mut BTreeSet::new()))
        {
            continue;
        }
        for (id, _) in &sources {
            result[0].insert((*id).clone(), unchanged(id.3));
        }
        for (id, index) in assigned {
            result[1].insert(id, unchanged(sources[index].0.3));
        }
    }
    Ok(())
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
    // Old normalized traces retained matrices but omitted these deterministic
    // fields. This declaration is allowed only for an unchanging authored
    // orientation and model slot; the observed matrix still proves its basis.
    static_binding: Option<StaticBinding>,
    angular_velocity: Option<[f64; 3]>,
    template: Template,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticBinding {
    angles: [f64; 3],
    model: u64,
}

fn validate_choices(source: &Trace, native: &Trace, result: &mut [Rotations; 2]) -> Result<()> {
    let choices: Vec<Choices> =
        super::optional_field(&source.setup, "effect_random_births")?.unwrap_or_default();
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
        if let Some(binding) = &choice.static_binding {
            ensure!(
                choice.angles.iter().all(Option::is_none)
                    && choice.model.min == choice.model.max
                    && f64::from(choice.model.min) == binding.model as f64
                    && binding.angles.iter().all(|angle| angle.is_finite()),
                "authored static binding contains a random field"
            );
        }
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
                && (choice.angles[1].is_none()
                    || choice.first_step[2] == 0.
                    || choice.angular_velocity.is_some())
                && choice
                    .angular_velocity
                    .is_none_or(|velocity| velocity.iter().all(|v| v.is_finite()))
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
        let source_rotation = choice_rotation(source, &source_id, source_birth, &choice, true)?;
        let mut candidates: Vec<_> = observed[1]
            .iter()
            .filter(|(id, birth)| {
                id.0 == choice.owner
                    && id.1 == choice.bank
                    && id.2 == choice.member
                    && birth.clock == choice.birth_clock
            })
            .filter_map(|(id, birth)| {
                choice_rotation(native, id, birth, &choice, false)
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelativeTemplate {
    metadata: super::content_evidence::Evidence,
    member: u64,
    program: u64,
}

fn validate_relative(source: &Trace, native: &Trace, result: &mut [Rotations; 2]) -> Result<()> {
    let templates: Vec<RelativeTemplate> =
        super::optional_field(&source.setup, "effect_relative_rotations")?.unwrap_or_default();
    for template in templates {
        let metadata = super::content_evidence::effect_value(
            template.metadata.registered_effect(&source.setup)?,
        )?;
        let bank = template.metadata.source_digest()?;
        ensure!(digest(bank), "invalid relative rotation bank");
        let declaration = &metadata["actors"][template.member as usize];
        let slot = declaration["binding"]["slot"]
            .as_u64()
            .context("missing fixed model slot")?;
        let particle: resonance_content::battle_effect::ParticleTemplate =
            serde_json::from_value(declaration["template"].clone())?;
        let resonance_content::battle_effect::ParticleGeometry::Size {
            value: scale,
            velocity,
            acceleration,
        } = &particle.state.geometry
        else {
            anyhow::bail!("relative rotation requires size geometry");
        };
        let scale = (*scale).map(f64::from);
        let program = metadata["programs"][template.program as usize]
            .as_array()
            .context("missing relative rotation program")?;
        let [emission] = program.as_slice() else {
            anyhow::bail!("relative rotation has additional operations");
        };
        let birth = &emission["operation"]["birth"];
        ensure!(
            declaration["kind"] == "model_particle"
                && declaration["binding"]["animation"].is_null()
                && !metadata["art"]["models"][slot.to_string()].is_null()
                && particle.follow_origin
                && !particle.follow_orientation
                && scale.iter().all(|value| value.is_finite() && *value > 0.)
                && *velocity == [0.; 3]
                && *acceleration == [0.; 3]
                && emission["at"] == 0
                && emission["operation"]["kind"] == "spawn"
                && emission["operation"]["particle"] == template.member
                && birth.as_object().is_some_and(|fields| fields.len() == 2)
                && birth["random_draws"] == 3,
            "unsupported relative rotation recipe"
        );
        for axis in 0..3 {
            let range = &birth["angles"][axis];
            let min = range["min"]
                .as_f64()
                .context("missing random angle minimum")?;
            let max = range["max"]
                .as_f64()
                .context("missing random angle maximum")?;
            ensure!(
                min <= -179.9
                    && max >= 179.9
                    && min >= -360.
                    && max <= 360.
                    && range["draw"] == axis,
                "birth does not independently randomize every rotation axis"
            );
        }
        let owner = source.setup["owner"]
            .as_u64()
            .context("missing relative rotation owner")?;
        let families = [(owner, bank.to_owned(), template.member)];
        let observed = [births(source, &families)?, births(native, &families)?];
        ensure!(
            !observed[0].is_empty() && observed[0].len() == observed[1].len(),
            "relative model instance count differs"
        );
        let mut used = BTreeSet::new();
        for (id, birth) in &observed[0] {
            let candidates: Vec<_> = observed[1]
                .iter()
                .filter(|(other, candidate)| {
                    !used.contains(*other)
                        && candidate.clock == birth.clock
                        && candidate.model["attachment"] == birth.model["attachment"]
                })
                .collect();
            let [(native_id, native_birth)] = candidates.as_slice() else {
                anyhow::bail!("relative model lacks a unique projectile binding");
            };
            let mut rotations = [
                relative_rotation(source, id, birth, template.program, slot, scale)?,
                relative_rotation(
                    native,
                    native_id,
                    native_birth,
                    template.program,
                    slot,
                    scale,
                )?,
            ];
            rotations[1].ordinal = id.3;
            same_history(
                [source, native],
                [id, native_id],
                [birth, native_birth],
                [&rotations[0], &rotations[1]],
            )?;
            let [source_rotation, native_rotation] = rotations;
            ensure!(
                result[0].insert(id.clone(), source_rotation).is_none()
                    && result[1]
                        .insert((*native_id).clone(), native_rotation)
                        .is_none(),
                "relative rotation already normalized"
            );
            used.insert((*native_id).clone());
        }
    }
    Ok(())
}

fn relative_rotation(
    trace: &Trace,
    id: &Key,
    birth: &Birth<'_>,
    program: u64,
    slot: u64,
    scale: [f64; 3],
) -> Result<Rotation> {
    ensure!(
        birth.model["attachment"]["kind"] == "projectile"
            && trace
                .events
                .iter()
                .any(|event| event["kind"] == "program_started"
                    && event["owner"] == id.0
                    && event["bank"] == id.1
                    && event["program"] == program
                    && event["clock"] == birth.clock),
        "relative rotation is not bound to its projectile program"
    );
    let mut relative = Relative {
        basis: rotation_basis(birth.model, scale)?,
        frames: BTreeMap::new(),
    };
    let mut particle_id = None;
    for index in birth.first_frame..=birth.last_frame {
        let frame = &trace.frames[index];
        let model = frame["effect_models"]
            .as_array()
            .context("missing relative models")?
            .iter()
            .find(|model| key(model).ok().as_ref() == Some(id))
            .context("relative model disappeared")?;
        rotation_basis(model, scale)?;
        ensure!(
            model["attachment"] == birth.model["attachment"]
                && model["model"]["clip"].is_null()
                && model["model"]["frame"].is_null(),
            "relative model binding changed"
        );
        let particles: Vec<_> = frame["particles"]
            .as_array()
            .context("missing relative particles")?
            .iter()
            .filter(|particle| particle_matches(particle, model))
            .collect();
        let [particle] = particles.as_slice() else {
            anyhow::bail!("relative model particle is ambiguous");
        };
        let age = particle["age"]
            .as_u64()
            .context("missing relative model age")?;
        let actual_id = particle["id"]
            .as_u64()
            .context("missing relative particle identity")?;
        ensure!(
            particle["initialized"] == true
                && age == (index - birth.first_frame + 1) as u64
                && particle_id.is_none_or(|id| id == actual_id)
                && particle
                    .get("model_slot")
                    .is_none_or(|observed| observed.as_u64() == Some(slot)),
            "relative model age, identity or fixed mesh differs"
        );
        particle_id = Some(actual_id);
        relative.frames.insert(index, age);
    }
    Ok(Rotation {
        ordinal: id.3,
        relative: Some(relative),
        ..Rotation::default()
    })
}

fn rotation_basis(model: &Value, scale: [f64; 3]) -> Result<[[f64; 3]; 3]> {
    let world = world(model)?;
    let basis: [[f64; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|column| world[row * 4 + column] / scale[column])
    });
    for first in 0..3 {
        for second in 0..3 {
            let dot: f64 = (0..3)
                .map(|row| basis[row][first] * basis[row][second])
                .sum();
            super::compare::equal(
                &Value::from(dot),
                &Value::from(if first == second { 1. } else { 0. }),
                "relative model positive scale and orthogonality",
            )?;
        }
    }
    let determinant = basis[0][0] * (basis[1][1] * basis[2][2] - basis[1][2] * basis[2][1])
        - basis[0][1] * (basis[1][0] * basis[2][2] - basis[1][2] * basis[2][0])
        + basis[0][2] * (basis[1][0] * basis[2][1] - basis[1][1] * basis[2][0]);
    super::compare::equal(
        &Value::from(determinant),
        &Value::from(1.),
        "relative model handedness",
    )?;
    Ok(basis)
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
    source: bool,
) -> Result<Rotation> {
    let first = &trace.frames[birth.first_frame];
    let candidates: Vec<_> = first["particles"]
        .as_array()
        .context("missing particles")?
        .iter()
        .filter(|particle| particle_matches(particle, birth.model))
        .collect();
    ensure!(
        candidates.len() == 1,
        "model birth lacks a unique particle identity"
    );
    let particle = candidates[0];
    let particle_id = particle["id"]
        .as_u64()
        .context("missing particle identity")?;
    let angles: [f64; 3] = if source && particle.get("angles").is_none() {
        choice
            .static_binding
            .as_ref()
            .context("missing actual birth angles")?
            .angles
    } else {
        serde_json::from_value(particle["angles"].clone())?
    };
    let offset: [f64; 3] = serde_json::from_value(particle["offset"].clone())?;
    let delta = std::array::from_fn::<_, 3, _>(|axis| offset[axis] - choice.first_offset[axis]);
    let (sin, cos) = choice.heading.to_radians().sin_cos();
    let local = [
        cos * delta[0] - sin * delta[2],
        delta[1],
        sin * delta[0] + cos * delta[2],
    ];
    let mut rotation = Rotation {
        ordinal: choice.ordinal,
        spin: choice.angular_velocity.map(|velocity| Spin {
            first: angles,
            velocity,
            frames: BTreeMap::new(),
        }),
        scaled_pitch: choice.angles[0].is_some(),
        ..Rotation::default()
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
    let slot = if source && particle.get("model_slot").is_none() {
        choice
            .static_binding
            .as_ref()
            .context("missing actual model choice")?
            .model
    } else {
        particle["model_slot"]
            .as_u64()
            .context("missing actual model choice")?
    };
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
                && (particle["model_slot"].as_u64() == Some(slot)
                    || (source
                        && particle.get("model_slot").is_none()
                        && choice
                            .static_binding
                            .as_ref()
                            .is_some_and(|binding| binding.model == slot)))
                && particle_matches(particle, model)
                && model["model"]["clip"].is_null()
                && model["model"]["frame"].is_null()
                && if choice.origin.is_some() {
                    model["attachment"].is_null()
                } else {
                    model["attachment"]["kind"] == "projectile"
                },
            "random model binding changed"
        );
        if let Some(binding) = &choice.static_binding {
            if let Some(observed) = particle.get("angles") {
                super::compare::equal(
                    observed,
                    &serde_json::to_value(binding.angles)?,
                    "authored static model angles",
                )?;
            }
            positive_scale(model, binding.angles)?;
        }
        if let Some(spin) = &mut rotation.spin {
            let age = particle["age"].as_u64().context("missing model spin age")?;
            let previous = spin.frames.last_key_value().map(|(_, (age, _))| *age);
            ensure!(
                previous.map_or(age == 1, |previous| age == previous || age == previous + 1),
                "model spin age is discontinuous"
            );
            let angles: [f64; 3] = serde_json::from_value(particle["angles"].clone())?;
            if choice.origin.is_some() {
                let expected = std::array::from_fn::<_, 3, _>(|axis| {
                    spin.first[axis] + spin.velocity[axis] * (age - 1) as f64
                });
                super::compare::equal(
                    &particle["angles"],
                    &serde_json::to_value(expected)?,
                    "authored model spin",
                )?;
            }
            // Projectile contact and rebound can change angular velocity. Keep
            // every observed angle so the comparison retains those responses.
            positive_scale(model, angles)?;
            spin.frames.insert(index, (age, angles));
        } else if rotation.scaled_pitch {
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

fn yaw_model_binding(
    trace: &Trace,
    id: &Key,
    birth: &Birth<'_>,
    slot: u64,
    source: bool,
) -> Result<()> {
    let mut particle_id = None;
    for index in birth.first_frame..=birth.last_frame {
        let frame = &trace.frames[index];
        let model = frame["effect_models"]
            .as_array()
            .context("missing yaw models")?
            .iter()
            .find(|model| key(model).ok().as_ref() == Some(id))
            .context("fixed yaw model disappeared")?;
        let particles: Vec<_> = frame["particles"]
            .as_array()
            .context("missing yaw particles")?
            .iter()
            .filter(|particle| particle_matches(particle, model))
            .collect();
        let [particle] = particles.as_slice() else {
            anyhow::bail!("fixed yaw model lacks a unique particle");
        };
        let observed_id = particle["id"].as_u64().context("missing yaw particle id")?;
        ensure!(
            particle_id.is_none_or(|id| id == observed_id)
                && particle["initialized"] == true
                && particle["age"] == index - birth.first_frame + 1
                && (particle["model_slot"].as_u64() == Some(slot)
                    || (source && particle.get("model_slot").is_none())),
            "fixed yaw model identity, age or authored slot differs"
        );
        particle_id = Some(observed_id);
    }
    Ok(())
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
        ordinal: yaw.ordinal,
        ..Rotation::default()
    };
    let mut model = model.clone();
    apply(&mut model, &rotation, 0)?;
    let canonical = world(&model)?;
    let [sx, sy, sz] = [0, 1, 2].map(|column| column_length(&initial_world, column));
    ensure!(
        [sx, sy, sz].into_iter().all(|scale| scale > 0.),
        "random-yaw model has nonpositive scale"
    );
    let (sine, cosine) = yaw.first_step.to_radians().sin_cos();
    let expected = [
        cosine * sx,
        -sine * sy,
        0.,
        initial_world[3],
        0.,
        0.,
        sz,
        initial_world[7],
        -sine * sx,
        -cosine * sy,
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
    if let Some(relative) = &rotation.relative {
        let original = world;
        for row in 0..3 {
            for column in 0..3 {
                world[row * 4 + column] = (0..3)
                    .map(|axis| relative.basis[axis][row] * original[axis * 4 + column])
                    .sum();
            }
        }
        model["rotation_age"] = (*relative
            .frames
            .get(&frame)
            .context("missing relative rotation age")?)
        .into();
    }
    if let Some(spin) = &rotation.spin {
        let (age, observed) = spin.frames.get(&frame).context("missing model spin age")?;
        let angles = std::array::from_fn(|axis| observed[axis] - rotation.initial[axis]);
        let basis = angle_basis(angles);
        // Euler increments on multiple axes do not commute with a constant
        // inverse birth rotation. Every observed basis was independently
        // checked against its retained angles above; retain its column scales.
        for column in 0..3 {
            let scale = column_length(&world, column);
            for row in 0..3 {
                world[row * 4 + column] = basis[row][column] * scale;
            }
        }
        model["rotation_age"] = (*age).into();
    }
    // Pitch rotates between the Y/Z basis vectors, not their scale magnitudes.
    // Positive scales were independently checked against retained particle angles.
    let [sy, sz] = if rotation.scaled_pitch {
        [1, 2].map(|column| column_length(&world, column))
    } else {
        [1.; 2]
    };
    // Inverse birth roll and yaw act on the left; inverse pitch acts on the right.
    // Only constant birth choices are removed, never the subsequent motion.
    for (angle, first, second) in if rotation.spin.is_none() {
        [(rotation.initial[2], 0, 4), (rotation.initial[1], 8, 0)]
    } else {
        [(0., 0, 4), (0., 8, 0)]
    } {
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
    let (sine, cosine) = if rotation.spin.is_none() {
        rotation.initial[0]
    } else {
        0.
    }
    .to_radians()
    .sin_cos();
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

fn angle_basis(angles: [f64; 3]) -> [[f64; 3]; 3] {
    let [(sx, cx), (sy, cy), (sz, cz)] = angles.map(|angle| angle.to_radians().sin_cos());
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

fn column_length(world: &[f64; 12], column: usize) -> f64 {
    (0..3)
        .map(|row| world[row * 4 + column].powi(2))
        .sum::<f64>()
        .sqrt()
}

fn particle_matches(particle: &Value, model: &Value) -> bool {
    ["owner", "bank", "member", "model"]
        .iter()
        .all(|field| particle[*field] == model[*field])
}

fn positive_scale(model: &Value, angles: [f64; 3]) -> Result<()> {
    let basis = angle_basis(angles);
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

#[cfg(test)]
mod fixed_yaw_tests {
    use super::*;
    use serde_json::json;

    fn trace(initial: f64, source: bool) -> Trace {
        let bank = "a".repeat(64);
        let frames = (0..3)
            .map(|frame| {
                let basis = angle_basis([-90., initial + 0.25 * (frame + 1) as f64, 0.]);
                let world: [f64; 12] = std::array::from_fn(|index| {
                    if index % 4 == 3 {
                        0.
                    } else {
                        basis[index / 4][index % 4] * [1.1, 1.1, 1.][index % 4]
                    }
                });
                let model = json!({"owner":0,"bank":bank,"member":2,"ordinal":1,
                "attachment":null,"model":{"clip":null,"frame":null,"world":world}});
                let mut particle = json!({"id":1,"owner":0,"bank":bank,"member":2,
                "initialized":true,"age":frame+1,"model":model["model"],"model_slot":2});
                if source {
                    particle.as_object_mut().unwrap().remove("model_slot");
                }
                json!({"combat_tick":frame,"effect_models":if frame<2 {vec![model]} else {vec![]},
                "particles":if frame<2 {vec![particle]} else {vec![]}})
            })
            .collect();
        Trace {
            case: "fixed-yaw".into(),
            setup: json!({"effect_random_yaw":[{"owner":0,"bank":bank,"member":2,"ordinal":1,
                "birth_clock":0,"program_clock":0,"first_step":0.25,"fixed_model":2,
                "template":{"path":"authored.bank","sha256":bank,"member":2,"program":1,
                    "modifier_words":[11,92,1800,0,0,32764,2,0,10,212,32764,0,65535]}}]}),
            frames,
            events: vec![
                json!({"kind":"program_started","owner":0,"bank":bank,"program":1,"clock":0}),
            ],
            assessment: Value::Null,
            incomplete_observations: vec![],
        }
    }

    #[test]
    fn fixed_yaw_models_require_authored_slot_and_stable_particle_identity() {
        assert!(validate(&trace(35., true), &trace(-80., false)).is_ok());
        for (field, value) in [
            ("model_slot", json!(3)),
            ("id", json!(2)),
            ("age", json!(3)),
        ] {
            let mut wrong = trace(-80., false);
            wrong.frames[1]["particles"][0][field] = value;
            assert!(validate(&trace(35., true), &wrong).is_err());
        }
        let mut missing = trace(-80., false);
        missing.frames[0]["particles"][0]
            .as_object_mut()
            .unwrap()
            .remove("model_slot");
        assert!(validate(&trace(35., true), &missing).is_err());
        let mut wrong_source = trace(35., true);
        wrong_source.frames[0]["particles"][0]["model_slot"] = json!(3);
        assert!(validate(&wrong_source, &trace(-80., false)).is_err());
        let mut wrong_recipe = trace(35., true);
        wrong_recipe.setup["effect_random_yaw"][0]["template"]["modifier_words"][6] = json!(3);
        assert!(validate(&wrong_recipe, &trace(-80., false)).is_err());
    }
}

#[cfg(test)]
mod relative_tests {
    use super::*;
    use serde_json::json;

    fn model(angles: [f64; 3], x: f64) -> Value {
        let basis = angle_basis(angles);
        let world: [f64; 12] = std::array::from_fn(|index| {
            if index % 4 == 3 {
                if index == 3 { x } else { 0. }
            } else {
                basis[index / 4][index % 4]
            }
        });
        json!({"owner":0,"bank":"test","member":90,"ordinal":1,
            "attachment":{"kind":"projectile","projectile":1},
            "model":{"clip":null,"frame":null,"world":world}})
    }

    #[test]
    fn relative_birth_rotation_keeps_contact_spin_changes_and_placement() {
        let births = [model([25., 60., -30.], 4.), model([-10., 15., 80.], 4.)];
        let rotations = births.each_ref().map(|birth| Rotation {
            ordinal: 1,
            relative: Some(Relative {
                basis: rotation_basis(birth, [1.; 3]).unwrap(),
                frames: BTreeMap::from([(0, 1), (1, 2)]),
            }),
            ..Rotation::default()
        });
        for change in [5., -6.] {
            let mut frames = [
                model([25. + change, 60., -30.], 10.),
                model([-10. + change, 15., 80.], 10.),
            ];
            for side in 0..2 {
                apply(&mut frames[side], &rotations[side], 1).unwrap();
            }
            super::super::compare::equal(&frames[0], &frames[1], "relative rotation").unwrap();
            frames[1]["model"]["world"][3] = json!(11.);
            assert!(
                super::super::compare::equal(&frames[0], &frames[1], "relative placement").is_err()
            );
        }
        let mut wrong = model([-4., 15., 80.], 10.); // Wrong +6 spin instead of +5.
        let mut expected = model([30., 60., -30.], 10.);
        apply(&mut wrong, &rotations[1], 1).unwrap();
        apply(&mut expected, &rotations[0], 1).unwrap();
        assert!(super::super::compare::equal(&expected, &wrong, "relative spin").is_err());
    }

    #[test]
    fn observed_projectile_spin_keeps_contact_reversal_and_rejects_wrong_motion() {
        let initial = [[40., 20., 0.], [-70., -30., 0.]];
        let changes = [[2., 3., 0.], [4., 6., 0.], [-2., 9., 0.]];
        let mut rotations = initial.map(|initial| Rotation {
            initial,
            ordinal: 1,
            scaled_pitch: true,
            spin: Some(Spin {
                first: std::array::from_fn(|axis| initial[axis] + changes[0][axis]),
                velocity: [2., 3., 0.],
                frames: changes
                    .iter()
                    .enumerate()
                    .map(|(frame, change)| {
                        (
                            frame,
                            (
                                frame as u64 + 1,
                                std::array::from_fn(|axis| initial[axis] + change[axis]),
                            ),
                        )
                    })
                    .collect(),
            }),
            ..Rotation::default()
        });
        for frame in 0..changes.len() {
            let mut models = std::array::from_fn::<_, 2, _>(|side| {
                let angles = rotations[side].spin.as_ref().unwrap().frames[&frame].1;
                let model = model(angles, frame as f64);
                positive_scale(&model, angles).unwrap();
                model
            });
            for side in 0..2 {
                apply(&mut models[side], &rotations[side], frame).unwrap();
            }
            super::super::compare::equal(&models[0], &models[1], "projectile spin").unwrap();
        }
        let expected_angles = rotations[0].spin.as_ref().unwrap().frames[&2].1;
        let wrong = rotations[1]
            .spin
            .as_mut()
            .unwrap()
            .frames
            .get_mut(&2)
            .unwrap();
        wrong.1[0] += 8.; // Continuing +2 instead of reversing to -6 at contact.
        let mut models = [model(expected_angles, 2.), model(wrong.1, 2.)];
        for side in 0..2 {
            apply(&mut models[side], &rotations[side], 2).unwrap();
        }
        assert!(super::super::compare::equal(&models[0], &models[1], "contact spin").is_err());
    }

    #[test]
    fn relative_basis_rejects_scale_shear_and_reflection() {
        let base = model([0.; 3], 0.);
        assert!(rotation_basis(&base, [1.; 3]).is_ok());
        for (element, value) in [(0, 2.), (1, 0.2), (0, -1.)] {
            let mut wrong = base.clone();
            wrong["model"]["world"][element] = json!(value);
            assert!(rotation_basis(&wrong, [1.; 3]).is_err());
        }
    }
}
