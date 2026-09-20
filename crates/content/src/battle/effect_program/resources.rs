//! Finite resource choices from the original integer scratch instructions.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct MaterialChoices {
    pub palettes: BTreeSet<u16>,
    pub blends: BTreeSet<Blend>,
}

impl MaterialChoices {
    pub(super) fn validate(&self, actor: &EffectActor) -> Result<()> {
        ensure!(
            self.blends
                .iter()
                .all(|blend| *blend == actor.blend || actor.blend_variants.contains(blend)),
            "unprepared effect blend"
        );
        ensure!(
            self.palettes.iter().all(|index| actor
                .palette
                .as_ref()
                .is_some_and(|palette| palette.materials.contains_key(index))),
            "unprepared effect palette"
        );
        Ok(())
    }
}

pub fn blend_choices(initial: Blend, modifiers: &[Modifier]) -> Result<BTreeSet<Blend>> {
    let mut choices = BTreeSet::from([initial]);
    blend_step(initial, modifiers, &mut choices)?;
    Ok(choices)
}

fn blend_step(
    mut current: Blend,
    modifiers: &[Modifier],
    choices: &mut BTreeSet<Blend>,
) -> Result<Blend> {
    for &modifier in modifiers {
        if let Modifier::Byte {
            field: ByteField::Blend,
            operation,
            value,
        } = modifier
        {
            current = current.modified(operation, value)?;
            choices.insert(current);
        }
    }
    Ok(current)
}

/// Group size belongs to the live owner's skeleton. Prepare the finite closure
/// over successful births, without inventing a maximum number of matching bones.
pub fn group_materials(actor: &EffectActor, modifiers: &[Modifier]) -> Result<MaterialChoices> {
    let mut result = MaterialChoices::default();
    let mut blend = actor.blend;
    let mut seen = BTreeSet::new();
    while seen.insert(blend) {
        result.blends.insert(blend);
        blend = blend_step(blend, modifiers, &mut result.blends)?;
    }
    if let Some(palette) = &actor.palette {
        let initial = palette.alpha.map_or(i16::from(palette.index), |alpha| {
            i16::from_be_bytes([palette.index, alpha])
        });
        let mut seen = BTreeSet::from([initial]);
        let mut pending = vec![initial];
        while let Some(before) = pending.pop() {
            let (color, alpha) = if palette.alpha.is_some() {
                let [color, alpha] = before.to_be_bytes();
                (color, Some(alpha))
            } else {
                (before as u8, None)
            };
            let (visited, _, after) =
                resource_choices(color, alpha, ByteField::Palette, modifiers)?;
            result.palettes.extend(visited);
            for next in after {
                if seen.insert(next) {
                    pending.push(next);
                }
            }
        }
    }
    Ok(result)
}

/// Finite timeline order bounds retained writes; old states remain possible because
/// the slot can refer to another live instance of the same authored actor.
pub fn retained_materials(
    program: &EffectProgram,
    content: &BattleEffectPrograms,
) -> Result<BTreeMap<EffectId, MaterialChoices>> {
    if !program.emissions.iter().any(|emission| matches!(&emission.command,
        EffectCommand::ModifyRetained { modifiers, .. } if modifiers.iter().any(Modifier::writes_material_selection))) {
        return Ok(BTreeMap::new());
    }
    let mut calls = Vec::new();
    for (index, emission) in program.emissions.iter().enumerate() {
        if let Some(repeat) = emission.repeat {
            for iteration in 0..repeat.count {
                let tick =
                    u32::from(emission.tick) + u32::from(iteration) * u32::from(repeat.interval);
                // The End command is processed before the repeat list.
                if tick >= u32::from(program.end_tick) {
                    break;
                }
                calls.push((tick, true, index, iteration));
            }
        } else {
            calls.push((u32::from(emission.tick), false, index, 0));
        }
    }
    calls.sort_unstable();
    let mut choices = BTreeMap::<EffectId, MaterialChoices>::new();
    for (_, _, index, _) in calls {
        match &program.emissions[index].command {
            EffectCommand::Particle {
                actor,
                attachment,
                modifiers,
            } => {
                let actor = content
                    .actor(*actor)
                    .context("missing material effect actor")?;
                if !actor.retained {
                    continue;
                }
                let states = choices.entry(actor.id).or_default();
                if matches!(attachment, Attachment::BoneGroup(_)) {
                    let group = group_materials(actor, modifiers)?;
                    states.blends.extend(group.blends);
                    states.palettes.extend(group.palettes);
                    continue;
                }
                states.blends.extend(blend_choices(actor.blend, modifiers)?);
                if let Some(palette) = &actor.palette {
                    states.palettes.extend(palette_indices(
                        palette.index,
                        palette.alpha,
                        modifiers,
                    )?);
                }
            }
            EffectCommand::ModifyRetained { modifiers, .. } => {
                for (id, states) in &mut choices {
                    let actor = content.actor(*id).unwrap();
                    for blend in states.blends.clone() {
                        states.blends.extend(blend_choices(blend, modifiers)?);
                    }
                    if let Some(palette) = &actor.palette {
                        for index in states.palettes.clone() {
                            let (color, alpha) = if palette.alpha.is_some() {
                                let [color, alpha] = index.to_be_bytes();
                                (color, Some(alpha))
                            } else {
                                (index as u8, None)
                            };
                            states
                                .palettes
                                .extend(palette_indices(color, alpha, modifiers)?);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(choices)
}

type IntegerRanges = [Option<BTreeSet<i16>>; 4];

fn values(v: IntegerValue, slots: &IntegerRanges) -> Result<Option<BTreeSet<i16>>> {
    Ok(match v {
        IntegerValue::Constant(v) => Some(BTreeSet::from([v])),
        IntegerValue::Temporary(i) => slots
            .get(usize::from(i))
            .context("invalid resource scratch slot")?
            .clone(),
    })
}

fn scratch_write(
    slots: &mut IntegerRanges,
    modifier: Modifier,
    use_preconditions: bool,
) -> Result<bool> {
    match modifier {
        Modifier::RequireFreshIntegers if use_preconditions => {
            *slots = std::array::from_fn(|_| Some(BTreeSet::from([0])));
        }
        Modifier::RequireIntegerRange { index, min, max } if use_preconditions => {
            modifier.validate()?;
            slots[usize::from(index)] = Some((min..=max).collect());
        }
        Modifier::RandomInteger {
            field: IntegerField::Temporary(i),
            modulus,
        } => {
            *slots
                .get_mut(usize::from(i))
                .context("invalid resource scratch slot")? = (1..=256)
                .contains(&modulus)
                .then(|| (0..modulus).map(|n| n as i16).collect());
        }
        Modifier::Integer {
            field: IntegerField::Temporary(i),
            operation,
            value,
        } => {
            let right = values(value, slots)?;
            let left = slots
                .get(usize::from(i))
                .context("invalid resource scratch slot")?
                .clone();
            slots[usize::from(i)] = match (operation, left, right) {
                (Arithmetic::Set, _, right) => right,
                (_, Some(left), Some(right)) => {
                    let mut out = BTreeSet::new();
                    for a in left {
                        for &b in &right {
                            out.insert(
                                operation
                                    .integer(a, b)
                                    .context("resource scratch division by zero")?,
                            );
                        }
                    }
                    (out.len() <= 256).then_some(out)
                }
                _ => None,
            };
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Unmodified, unrepeated births leave the program's initialized integer scratch intact.
pub fn fresh_integer_scratch(prior: &[EffectEmission]) -> bool {
    prior.iter().all(|emission| {
        emission.repeat.is_none()
            && matches!(&emission.command, EffectCommand::Particle { modifiers, .. } if modifiers.is_empty())
    })
}

/// Derive inherited scratch from actual preceding synchronous births, ignoring
/// their claimed preconditions. Unknown scheduling or mutations break the proof.
pub fn integer_birth_ranges(prior: &[EffectEmission], tick: u16) -> Result<[Option<[i16; 2]>; 4]> {
    let mut slots = IntegerRanges::default();
    for e in prior {
        match &e.command {
            EffectCommand::Particle {
                attachment,
                modifiers,
                ..
            } if e.tick == tick
                && e.repeat.is_none()
                && !matches!(attachment, Attachment::BoneGroup(_)) =>
            {
                for &m in modifiers {
                    scratch_write(&mut slots, m, false)?;
                }
            }
            EffectCommand::Sound { .. } if e.tick == tick && e.repeat.is_none() => {}
            _ => slots = Default::default(),
        }
    }
    Ok(slots.map(|values| {
        values.and_then(|v| {
            let range = [*v.first()?, *v.last()?];
            (i32::from(range[1]) - i32::from(range[0]) < 256).then_some(range)
        })
    }))
}

pub(super) fn validate_birth_ranges(
    prior: &[EffectEmission],
    tick: u16,
    modifiers: &[Modifier],
) -> Result<()> {
    let ranges = integer_birth_ranges(prior, tick)?;
    for &m in modifiers {
        if let Modifier::RequireIntegerRange { index, min, max } = m {
            m.validate()?;
            ensure!(
                ranges[usize::from(index)] == Some([min, max]),
                "integer scratch range is not proved by preceding births"
            );
        }
    }
    Ok(())
}

/// Resource choices require finite instructions or checked timeline preconditions.
/// Independent sets conservatively include correlations between scratch operands.
pub fn resource_indices(
    initial: u8,
    field: ByteField,
    modifiers: &[Modifier],
) -> Result<BTreeSet<u8>> {
    ensure!(
        field != ByteField::ModelIndex || !modifiers.iter().any(Modifier::writes_model_selection),
        "packed model selection requires the authored animation selector"
    );
    ensure!(
        field != ByteField::Palette || !modifiers.iter().any(Modifier::writes_palette_selection),
        "packed palette selection requires the authored alpha selector"
    );
    Ok(resource_choices(initial, None, field, modifiers)?
        .0
        .into_iter()
        .map(|v| v as u8)
        .collect())
}

/// Both source selector bytes participate in halfword arithmetic; only the high byte selects geometry.
pub fn model_indices(initial: u8, selector: u8, modifiers: &[Modifier]) -> Result<BTreeSet<u8>> {
    Ok(
        resource_choices(initial, Some(selector), ByteField::ModelIndex, modifiers)?
            .0
            .into_iter()
            .map(|v| v as u8)
            .collect(),
    )
}

/// A group copies the preceding successful birth before applying its modifiers.
/// Each edge records the ordinary animation's source and the final visible model.
pub fn group_model_choices(
    initial: u8,
    selector: u8,
    modifiers: &[Modifier],
) -> Result<BTreeSet<(u8, u8)>> {
    let initial = i16::from_be_bytes([initial, selector]);
    let mut seen = BTreeSet::from([initial]);
    let mut pending = vec![initial];
    let mut edges = BTreeSet::new();
    while let Some(before) = pending.pop() {
        let [model, selector] = before.to_be_bytes();
        for after in resource_choices(model, Some(selector), ByteField::ModelIndex, modifiers)?.2 {
            edges.insert((model, after.to_be_bytes()[0]));
            if seen.insert(after) {
                pending.push(after);
            }
        }
    }
    Ok(edges)
}

/// The visible model is rebound after the complete modifier stream.
pub fn selected_model_index(initial: u8, selector: u8, modifiers: &[Modifier]) -> Result<u8> {
    let choices = resource_choices(initial, Some(selector), ByteField::ModelIndex, modifiers)?.1;
    ensure!(
        choices.len() == 1,
        "external animation requires one resolved model controller"
    );
    Ok(*choices.first().unwrap() as u8)
}

pub fn palette_indices(
    initial: u8,
    alpha: Option<u8>,
    modifiers: &[Modifier],
) -> Result<BTreeSet<u16>> {
    ensure!(
        alpha.is_some() || !modifiers.iter().any(Modifier::writes_palette_selection),
        "packed palette selection requires the authored alpha selector"
    );
    Ok(resource_choices(initial, alpha, ByteField::Palette, modifiers)?.0)
}

fn resource_choices(
    initial: u8,
    selector: Option<u8>,
    field: ByteField,
    modifiers: &[Modifier],
) -> Result<(BTreeSet<u16>, BTreeSet<u16>, BTreeSet<i16>)> {
    ensure!(
        matches!(field, ByteField::Palette | ByteField::ModelIndex),
        "not an effect resource field"
    );
    let packed = field == ByteField::ModelIndex || selector.is_some();
    let initial_value = if packed {
        i16::from_be_bytes([initial, selector.unwrap_or(0)])
    } else {
        i16::from(initial)
    };
    let key = |value: i16| {
        if field == ByteField::ModelIndex {
            u16::from(value.to_be_bytes()[0])
        } else if packed {
            value as u16
        } else {
            u16::from(value as u8)
        }
    };
    let mut indices = BTreeSet::from([key(initial_value)]);
    if !modifiers.iter().any(|m| {
        matches!(m,Modifier::Byte{field:f,..}if *f==field)
            || (field == ByteField::ModelIndex && m.writes_model_selection())
            || (field == ByteField::Palette && m.writes_palette_selection())
    }) {
        return Ok((indices.clone(), indices, BTreeSet::from([initial_value])));
    }
    let mut integers = IntegerRanges::default();
    let mut current = BTreeSet::from([initial_value]);
    let index = |value: i16| {
        if packed {
            value.to_be_bytes()[0]
        } else {
            value as u8
        }
    };
    for m in modifiers {
        if scratch_write(&mut integers, *m, true)? {
            continue;
        }
        match *m {
            Modifier::Byte {
                field: f,
                operation,
                value,
            } if f == field => {
                let right = values(value, &integers)?
                    .context("effect resource depends on unbounded or uninitialized scratch")?;
                let mut next = BTreeSet::new();
                for &a in &current {
                    for &b in &right {
                        let selected = operation
                            .byte(index(a), b as u8)
                            .context("effect resource division by zero")?;
                        next.insert(if packed {
                            i16::from_be_bytes([selected, a.to_be_bytes()[1]])
                        } else {
                            i16::from(selected)
                        });
                    }
                }
                current = next;
            }
            Modifier::Integer {
                field: selection,
                operation,
                value,
            } if matches!(
                (field, selection),
                (ByteField::ModelIndex, IntegerField::ModelSelection)
                    | (ByteField::Palette, IntegerField::PaletteSelection)
            ) =>
            {
                let right = values(value, &integers)?
                    .context("effect resource depends on unbounded or uninitialized scratch")?;
                let mut next = BTreeSet::new();
                for &a in &current {
                    for &b in &right {
                        next.insert(
                            operation
                                .integer(a, b)
                                .context("effect resource division by zero")?,
                        );
                    }
                }
                current = next;
            }
            Modifier::RandomInteger {
                field: selection,
                modulus,
            } if matches!(
                (field, selection),
                (ByteField::ModelIndex, IntegerField::ModelSelection)
                    | (ByteField::Palette, IntegerField::PaletteSelection)
            ) =>
            {
                ensure!(
                    (1..=256).contains(&modulus),
                    "unbounded packed resource selection"
                );
                current = (0..modulus).map(|n| n as i16).collect();
            }
            _ => continue,
        }
        ensure!(
            current.len() <= 256,
            "unbounded effect resource combinations"
        );
        let selected = current.iter().map(|&v| key(v)).collect::<BTreeSet<_>>();
        ensure!(
            field != ByteField::ModelIndex || selected.iter().all(|&i| i < 128),
            "effect model index exceeds supported range"
        );
        indices.extend(selected);
    }
    Ok((indices, current.iter().copied().map(key).collect(), current))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_palette_requires_an_ordered_bounded_producer() {
        let random = Modifier::RandomInteger {
            field: IntegerField::Temporary(3),
            modulus: 8,
        };
        let mut producer = EffectEmission {
            tick: 0,
            repeat: None,
            command: EffectCommand::Particle {
                actor: EffectId {
                    bank: crate::battle::effects::EffectBank::Magic(111),
                    id: 4,
                },
                attachment: Attachment::Emitter,
                modifiers: vec![random],
            },
        };
        let claim = Modifier::RequireIntegerRange {
            index: 3,
            min: 0,
            max: 7,
        };
        let palette = Modifier::Byte {
            field: ByteField::Palette,
            operation: Arithmetic::Add,
            value: IntegerValue::Temporary(3),
        };
        let consumer = [claim, palette];
        validate_birth_ranges(&[producer.clone()], 0, &consumer).unwrap();
        assert_eq!(
            palette_indices(8, None, &consumer).unwrap(),
            (8..16).collect()
        );
        assert!(palette_indices(8, None, &[palette]).is_err());
        assert!(validate_birth_ranges(&[], 0, &consumer).is_err());
        assert!(validate_birth_ranges(&[producer.clone()], 1, &consumer).is_err());
        assert!(
            validate_birth_ranges(
                &[producer.clone()],
                0,
                &[Modifier::RequireIntegerRange {
                    index: 3,
                    min: 0,
                    max: 8
                }]
            )
            .is_err()
        );
        producer.repeat = Some(Repeat {
            count: 2,
            interval: 1,
        });
        assert!(validate_birth_ranges(&[producer.clone()], 0, &consumer).is_err());
        producer.repeat = None;
        let EffectCommand::Particle { attachment, .. } = &mut producer.command else {
            unreachable!()
        };
        *attachment = Attachment::BoneGroup(0);
        assert!(validate_birth_ranges(&[producer.clone()], 0, &consumer).is_err());
        let EffectCommand::Particle {
            modifiers,
            attachment,
            ..
        } = &mut producer.command
        else {
            unreachable!()
        };
        *attachment = Attachment::Emitter;
        // A claim cannot act as its own producer or conceal an unbounded write.
        *modifiers = vec![claim];
        assert!(validate_birth_ranges(&[producer.clone()], 0, &consumer).is_err());
        let EffectCommand::Particle { modifiers, .. } = &mut producer.command else {
            unreachable!()
        };
        *modifiers = vec![
            random,
            Modifier::RandomInteger {
                field: IntegerField::Temporary(3),
                modulus: 257,
            },
        ];
        assert!(validate_birth_ranges(&[producer], 0, &consumer).is_err());
    }
    #[test]
    fn random_resources_keep_source_arithmetic_and_reject_inherited_scratch() {
        let random = Modifier::RandomInteger {
            field: IntegerField::Temporary(0),
            modulus: 4,
        };
        let increment = Modifier::Integer {
            field: IntegerField::Temporary(0),
            operation: Arithmetic::Add,
            value: IntegerValue::Constant(1),
        };
        let model = Modifier::Byte {
            field: ByteField::ModelIndex,
            operation: Arithmetic::Set,
            value: IntegerValue::Temporary(0),
        };
        assert_eq!(
            resource_indices(1, ByteField::ModelIndex, &[random, increment, model]).unwrap(),
            BTreeSet::from([1, 2, 3, 4])
        );
        assert!(resource_indices(1, ByteField::ModelIndex, &[model]).is_err());
        let unknown = Modifier::RandomInteger {
            field: IntegerField::Temporary(0),
            modulus: 257,
        };
        let reset = Modifier::Integer {
            field: IntegerField::Temporary(0),
            operation: Arithmetic::Set,
            value: IntegerValue::Constant(3),
        };
        assert_eq!(
            resource_indices(1, ByteField::ModelIndex, &[unknown, reset, model]).unwrap(),
            BTreeSet::from([1, 3])
        );
        let palette = Modifier::Byte {
            field: ByteField::Palette,
            operation: Arithmetic::Add,
            value: IntegerValue::Temporary(0),
        };
        assert_eq!(
            resource_indices(1, ByteField::Palette, &[random, palette]).unwrap(),
            BTreeSet::from([1, 2, 3, 4])
        );
        assert_eq!(
            resource_indices(254, ByteField::Palette, &[random, palette]).unwrap(),
            BTreeSet::from([0, 1, 254, 255])
        );
        let packed = Modifier::Integer {
            field: IntegerField::PaletteSelection,
            operation: Arithmetic::Add,
            value: IntegerValue::Temporary(0),
        };
        assert_eq!(
            palette_indices(1, Some(1), &[random, packed]).unwrap(),
            BTreeSet::from([0x101, 0x102, 0x103, 0x104])
        );
        assert_eq!(
            palette_indices(255, Some(254), &[random, packed]).unwrap(),
            BTreeSet::from([0xfffe, 0xffff, 0, 1])
        );
        assert!(palette_indices(1, None, &[random, packed]).is_err());
        assert!(palette_indices(1, Some(1), &[packed]).is_err());
        assert!(
            palette_indices(
                1,
                Some(1),
                &[Modifier::Integer {
                    field: IntegerField::PaletteSelection,
                    operation: Arithmetic::Divide,
                    value: IntegerValue::Constant(0)
                }]
            )
            .is_err()
        );
        assert!(
            palette_indices(
                1,
                Some(1),
                &[Modifier::Integer {
                    field: IntegerField::PaletteSelection,
                    operation: Arithmetic::Set,
                    value: IntegerValue::Temporary(4)
                }]
            )
            .is_err()
        );
        let legacy: Palette = serde_json::from_str(r#"{"index":1,"materials":{"1":7}}"#).unwrap();
        assert_eq!((legacy.alpha, legacy.material()), (None, Some(7)));
        assert!(
            resource_indices(
                127,
                ByteField::ModelIndex,
                &[
                    random,
                    Modifier::Byte {
                        field: ByteField::ModelIndex,
                        operation: Arithmetic::Add,
                        value: IntegerValue::Temporary(0)
                    }
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn packed_selection_requires_bounded_scratch_and_keeps_signed_halfword_arithmetic() {
        let packed = |operation, value| Modifier::Integer {
            field: IntegerField::ModelSelection,
            operation,
            value,
        };
        let random = Modifier::RandomInteger {
            field: IntegerField::Temporary(0),
            modulus: 3,
        };
        let add = packed(Arithmetic::Add, IntegerValue::Temporary(0));
        assert_eq!(
            model_indices(4, 0, &[random, add]).unwrap(),
            BTreeSet::from([4])
        );
        assert_eq!(
            model_indices(4, 255, &[random, add]).unwrap(),
            BTreeSet::from([4, 5])
        );
        assert!(model_indices(4, 0, &[add]).is_err());
        assert!(resource_indices(4, ByteField::ModelIndex, &[random, add]).is_err());
        assert!(model_indices(127, 255, &[random, add]).is_err());
        let constant = |op, value| packed(op, IntegerValue::Constant(value));
        let ops = [
            constant(Arithmetic::Set, 0x0400),
            constant(Arithmetic::Subtract, 1),
            constant(Arithmetic::Multiply, 2),
            constant(Arithmetic::Divide, 2),
        ];
        assert_eq!(
            model_indices(4, 0, &ops).unwrap(),
            BTreeSet::from([3, 4, 7])
        );
        assert_eq!(selected_model_index(4, 0, &ops).unwrap(), 3);
        assert_eq!(Arithmetic::Divide.integer(-1025, 2), Some(-512));
        assert_eq!(Arithmetic::Multiply.integer(-32768, -1), Some(-32768));
        assert_eq!(Arithmetic::Divide.integer(-32768, -1), Some(-32768));
        assert!(model_indices(4, 0, &[constant(Arithmetic::Divide, 0)]).is_err());
        assert!(
            model_indices(
                4,
                0,
                &[Modifier::RandomInteger {
                    field: IntegerField::ModelSelection,
                    modulus: 257
                }]
            )
            .is_err()
        );
        assert_eq!(
            model_indices(
                4,
                0,
                &[Modifier::RandomInteger {
                    field: IntegerField::ModelSelection,
                    modulus: 3
                }]
            )
            .unwrap(),
            BTreeSet::from([0, 4])
        );
    }
}
