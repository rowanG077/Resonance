//! Resolve bounded schedules and arithmetic into particle settings.
use super::*;

pub(super) fn timeline(input: InputTimeline, actors: &[Declaration]) -> Vec<ScheduledEvent> {
    let mut expanded = Vec::new();
    for (index, event) in input.events.into_iter().enumerate() {
        if let Some(repeat) = event.repeat {
            for emission in 0..repeat.count {
                let at = event.at + u32::from(emission) * repeat.interval;
                if at < input.end {
                    expanded.push((at, true, index, event.operation.clone()));
                }
            }
        } else {
            expanded.push((event.at, false, index, event.operation));
        }
    }
    // Due commands update shared settings before repeated emissions use them.
    expanded.sort_by_key(|(at, repeated, index, _)| (*at, *repeated, *index));
    let mut compiler = Compiler::default();
    let mut events = Vec::new();
    for (at, _, _, operation) in expanded {
        match compiler.operation(operation, actors) {
            Ok(operation) => events.push(ScheduledEvent { at, operation }),
            Err(error) => {
                events.clear();
                events.push(ScheduledEvent {
                    at,
                    operation: EffectOperation::Unsupported {
                        reason: error.to_string(),
                    },
                });
                break;
            }
        }
    }
    events
}

#[derive(Clone, Copy)]
struct Value {
    range: ValueRange,
    heading: f32,
}
impl Value {
    fn constant(value: f32) -> Self {
        Self {
            range: ValueRange::fixed(value),
            heading: 0.,
        }
    }
    fn fixed(self) -> bool {
        self.range.min == self.range.max && self.heading == 0.
    }
}
struct Compiler {
    // A halfword write makes a scalar opaque until its next complete assignment.
    scalars: [Option<Value>; 4],
    integers: [Value; 4],
    draws: u32,
    spawns: usize,
    retained_spawns: Vec<usize>,
    previous_birth: Option<ParticleBirth>,
}
impl Default for Compiler {
    fn default() -> Self {
        Self {
            scalars: [Some(Value::constant(0.)); 4],
            integers: [Value::constant(0.); 4],
            draws: 0,
            spawns: 0,
            retained_spawns: Vec::new(),
            previous_birth: None,
        }
    }
}
impl Compiler {
    fn draw(&mut self) -> Result<u32> {
        let id = self.draws;
        self.draws = id
            .checked_add(1)
            .context("too many particle random draws")?;
        Ok(id)
    }
    fn operation(
        &mut self,
        operation: InputOperation,
        actors: &[Declaration],
    ) -> Result<EffectOperation> {
        Ok(match operation {
            InputOperation::Spawn {
                particle,
                origin_bone,
                inherit_previous,
                retained,
                blend,
                palette,
                edits,
            } => {
                let declaration = actors.get(usize::from(particle));
                let previous = if inherit_previous {
                    ensure!(
                        matches!(declaration, Some(Declaration::Particle { .. })),
                        "inherited grouped birth requires a sprite declaration"
                    );
                    Some(
                        self.previous_birth
                            .take()
                            .context("grouped birth has no predecessor")?,
                    )
                } else {
                    None
                };
                let mut birth = self.birth_from(edits, declaration, previous)?;
                birth.origin_bone = origin_bone;
                if let Some(Declaration::ModelParticle { visual, .. }) = declaration
                    && matches!(blend.unwrap_or(visual.blend), 1 | 2)
                    && let Some(animation) = &mut birth.animation
                {
                    // Blended models take their first animation sample on the birth update.
                    animation.frame += animation.rate;
                }
                if let Some(Declaration::ModelParticle { binding, .. }) = declaration
                    && let Some(attachment) = binding.attachment
                {
                    birth.model_parent = Some(ModelParent {
                        spawn: *self
                            .retained_spawns
                            .get(usize::from(attachment.retained))
                            .context("model parent has no earlier retained birth")?,
                        joint: attachment.joint,
                    });
                }
                if retained {
                    self.retained_spawns.push(self.spawns);
                }
                self.spawns += 1;
                self.previous_birth = Some(birth.clone());
                EffectOperation::Spawn {
                    particle,
                    blend,
                    palette,
                    birth: Box::new(birth),
                }
            }
            InputOperation::SetMotion { retained, motion } => EffectOperation::SetMotion {
                spawn: *self
                    .retained_spawns
                    .get(usize::from(retained))
                    .context("delayed particle edit has no retained birth")?,
                motion,
            },
            InputOperation::Sound { id, priority } => EffectOperation::Sound { id, priority },
            InputOperation::Notice { text } => EffectOperation::Notice { text },
            InputOperation::StageTint { color, duration } => {
                EffectOperation::StageTint { color, duration }
            }
            InputOperation::Shake {
                duration,
                amplitude,
            } => EffectOperation::Shake {
                duration,
                amplitude,
            },
        })
    }
    #[cfg(test)]
    fn birth(
        &mut self,
        edits: Vec<InputEdit>,
        declaration: Option<&Declaration>,
    ) -> Result<ParticleBirth> {
        self.birth_from(edits, declaration, None)
    }

    fn birth_from(
        &mut self,
        edits: Vec<InputEdit>,
        declaration: Option<&Declaration>,
        previous: Option<ParticleBirth>,
    ) -> Result<ParticleBirth> {
        let inherited = previous.is_some();
        let mut birth = previous.unwrap_or_default();
        // Stored births include the final lifetime visit. Restore the authored
        // duration before inherited edits; it is normalized again below.
        if let Some(lifetime) = &mut birth.lifetime
            && lifetime.min > 0.
        {
            lifetime.min -= 1.;
            lifetime.max -= 1.;
        }
        let first_draw = self.draws;
        let mut animated_model = None;
        let mut animation_byte = None;
        for edit in edits {
            match edit {
                InputEdit::Forward { .. } => {
                    bail!("forward motion requires an initialized particle")
                }
                InputEdit::ModelAnimation(animation) => {
                    ensure!(
                        animated_model.replace(animation.model).is_none(),
                        "multiple model animation starts"
                    );
                    birth.animation = Some(animation);
                }
                InputEdit::HeldModelFrame { scalar } => {
                    let Some(Declaration::ModelParticle { binding, .. }) = declaration else {
                        bail!("held model frame has no model declaration");
                    };
                    let frame = self.scalars[usize::from(scalar)]
                        .context("held model frame reads an incomplete scalar")?;
                    ensure!(frame.fixed(), "held model frame must be fixed at import");
                    ensure!(
                        animated_model.replace(binding.slot).is_none(),
                        "multiple model animation starts"
                    );
                    let animation = ModelAnimation {
                        model: binding.slot,
                        clip: binding.animation.context("held model frame has no clip")?,
                        frame: frame.range.min,
                        blend_ticks: 0,
                        rate: 0.,
                        repeat: binding.repeat,
                    };
                    animation.validate()?;
                    birth.animation = Some(animation);
                }
                InputEdit::Scalar {
                    target,
                    arithmetic: operation,
                    value,
                } => {
                    let value = match value {
                        InputScalarValue::Literal { value } => Value::constant(value),
                        InputScalarValue::Scratch { index } => self.scalars[usize::from(index)]
                            .context("particle reads scalar scratch after a halfword write")?,
                        InputScalarValue::Random { range, scale } => {
                            let extent = (f32::from(range).abs() - 1.) * scale.abs();
                            let (min, max, step) = match range {
                                0 => (
                                    f32::from(i16::MIN) * scale,
                                    f32::from(i16::MAX) * scale,
                                    scale,
                                ),
                                1 | -1 => (0., 0., 0.),
                                _ => (-extent, extent, scale.abs()),
                            };
                            Value {
                                range: ValueRange {
                                    min,
                                    max,
                                    step,
                                    draw: Some(self.draw()?),
                                    reverse: false,
                                },
                                heading: 0.,
                            }
                        }
                    };
                    if let InputScalarTarget::Scratch { index } = target {
                        let slot = &mut self.scalars[usize::from(index)];
                        let before = if operation == Arithmetic::Set {
                            Value::constant(0.)
                        } else {
                            slot.context(
                                "particle arithmetic reads scalar scratch after a halfword write",
                            )?
                        };
                        *slot = Some(arithmetic(before, value, operation)?);
                    } else {
                        let yaw = target
                            == (InputScalarTarget::Vector {
                                vector: VectorTarget::Angles,
                                axis: 1,
                            });
                        let heading = if yaw { birth.yaw_heading } else { 0. };
                        let polar = matches!(target,
                            InputScalarTarget::Vector { vector: VectorTarget::Offset, .. } if birth.offset_spread.is_some())
                            || matches!(target,
                            InputScalarTarget::Vector { vector: VectorTarget::Velocity, .. } if birth.velocity_spread.is_some());
                        if inherited && let Some(field) = sum_field(target, declaration) {
                            let initial = initial_scalar(
                                declaration.context("missing particle declaration")?,
                                target,
                            )?;
                            if add_sum(&mut birth, field, initial, value, &operation)? {
                                continue;
                            }
                        }
                        let slot = scalar_slot(&mut birth, target, declaration);
                        let before = match *slot {
                            Some(range) => Value { range, heading },
                            None if operation == Arithmetic::Set => Value::constant(0.),
                            None => {
                                ensure!(!polar, "particle setting depends on polar coordinates");
                                initial_scalar(
                                    declaration.context("missing particle declaration")?,
                                    target,
                                )?
                            }
                        };
                        let value = arithmetic(before, value, operation)?;
                        ensure!(
                            value.heading == 0. || yaw,
                            "unsupported relative particle angle"
                        );
                        *slot = Some(value.range);
                        if yaw {
                            birth.yaw_heading = value.heading;
                        }
                    }
                }
                InputEdit::Integer {
                    target,
                    arithmetic: operation,
                    value,
                } => {
                    let value = match value {
                        InputIntegerValue::Literal { value } => Value::constant(f32::from(value)),
                        InputIntegerValue::Scratch { index } => self.integers[usize::from(index)],
                        InputIntegerValue::Random { range } => Value {
                            range: ValueRange {
                                min: 0.,
                                max: f32::from(range - 1),
                                step: 1.,
                                draw: Some(self.draw()?),
                                reverse: false,
                            },
                            heading: 0.,
                        },
                    };
                    if let InputIntegerTarget::ScalarHigh { index } = target {
                        // The operand draw is retained above. Discarding the resulting
                        // bits is valid only if no later birth reads them before overwrite.
                        self.scalars[usize::from(index)] = None;
                        continue;
                    }
                    if target == InputIntegerTarget::RollHigh {
                        ensure!(
                            operation == Arithmetic::Add
                                && value.fixed()
                                && value.range.draw.is_none()
                                && value.range.min.fract() == 0.
                                && (i16::MIN as f32..=i16::MAX as f32).contains(&value.range.min),
                            "roll halfword addition requires a fixed integer"
                        );
                        let before = match birth.angles[2] {
                            Some(range) => Value { range, heading: 0. },
                            None => initial_scalar(
                                declaration.context("missing particle declaration")?,
                                InputScalarTarget::Vector {
                                    vector: VectorTarget::Angles,
                                    axis: 2,
                                },
                            )?,
                        };
                        ensure!(
                            before.fixed() && before.range.draw.is_none(),
                            "roll halfword addition requires a fixed angle"
                        );
                        let bits = before.range.min.to_bits();
                        let high = u16::try_from((bits >> 16) as i32 + value.range.min as i32)
                            .context("roll halfword addition requires wrapping")?;
                        let angle = f32::from_bits(u32::from(high) << 16 | bits & 0xffff);
                        ensure!(
                            angle == 0. || angle.is_normal(),
                            "roll halfword addition produces a non-normal angle"
                        );
                        birth.angles[2] = Some(ValueRange::fixed(angle));
                        continue;
                    }
                    if let InputIntegerTarget::ModelAndAnimation { animation } = target {
                        ensure!(
                            matches!(declaration, Some(Declaration::ModelParticle { binding, .. })
                                if binding.animation.is_none() && !binding.program_animation),
                            "packed model edit requires an unused animation byte"
                        );
                        let result = arithmetic(
                            animation_byte.unwrap_or(Value::constant(f32::from(animation))),
                            value,
                            operation,
                        )?;
                        ensure!(
                            result.range.min >= 0. && result.range.max <= f32::from(u8::MAX),
                            "packed model edit crosses a model boundary"
                        );
                        // The low byte is inactive. Preserve its arithmetic and draw
                        // order so subsequent additions still detect carry correctly.
                        animation_byte = Some(result);
                        continue;
                    }
                    if target == InputIntegerTarget::PalettePair {
                        let state = &declaration
                            .context("palette edit has no particle")?
                            .template()?
                            .state;
                        let alpha = birth
                            .alpha_palette
                            .unwrap_or(ValueRange::fixed(f32::from(state.palettes[1])));
                        let result = arithmetic(
                            Value {
                                range: alpha,
                                heading: 0.,
                            },
                            value,
                            operation,
                        )?
                        .range;
                        ensure!(
                            result.min >= 0. && result.max <= 255.,
                            "packed palette edit crosses a color palette boundary"
                        );
                        birth.alpha_palette = Some(result);
                        continue;
                    }
                    if target == InputIntegerTarget::AttachmentIndex {
                        ensure!(
                            operation == Arithmetic::Set
                                && value.fixed()
                                && value.range.min >= 0.
                                && value.range.min <= f32::from(u8::MAX)
                                && value.range.min.fract() == 0.,
                            "attachment selector requires a fixed byte"
                        );
                        let declaration = declaration.context("attachment has no particle")?;
                        let template = declaration.template()?;
                        if matches!(declaration, Declaration::Particle { .. })
                            && (template.owner_bone.is_some() != template.follow_bone.is_some())
                        {
                            birth.attachment_bone = Some(value.range.min as u8);
                        } else {
                            ensure!(
                                value.range.min < 16. && template.state.effect_group.is_some(),
                                "effect group requires a fixed prepared group"
                            );
                            birth.effect_group = Some(value.range.min as u8);
                        }
                        continue;
                    }
                    if let InputIntegerTarget::Scratch { index } = target {
                        let slot = &mut self.integers[usize::from(index)];
                        *slot = arithmetic(*slot, value, operation)?;
                    } else {
                        if inherited && let InputIntegerTarget::Uv { component } = target {
                            let initial = Value::constant(initial_integer(
                                declaration.context("missing particle declaration")?,
                                target,
                            )?);
                            if add_sum(
                                &mut birth,
                                BirthField::Uv(component),
                                initial,
                                value,
                                &operation,
                            )? {
                                continue;
                            }
                        }
                        let slot = integer_slot(&mut birth, target);
                        let before = match *slot {
                            Some(range) => Value { range, heading: 0. },
                            None if operation == Arithmetic::Set => Value::constant(0.),
                            None => Value::constant(initial_integer(
                                declaration.context("missing particle declaration")?,
                                target,
                            )?),
                        };
                        let divide = operation == Arithmetic::Divide;
                        let mut range = arithmetic(before, value, operation)?.range;
                        if divide {
                            ensure!(
                                range.min == range.max,
                                "varying integer division is unsupported"
                            );
                            range.min = range.min.trunc();
                            range.max = range.min;
                        }
                        let maximum = match target {
                            InputIntegerTarget::Lifetime => Some(i16::MAX as f32),
                            InputIntegerTarget::FadeFrom => Some(255.),
                            _ => None,
                        };
                        if let Some(maximum) = maximum {
                            ensure!(
                                range.min >= 0. && range.max <= maximum,
                                "particle duration arithmetic requires wrapping"
                            );
                        }
                        *slot = Some(range);
                    }
                }
                InputEdit::Polar {
                    target,
                    angles,
                    radius,
                    radius_jitter,
                    angle_jitter,
                } => {
                    let spread = Some(PolarSpread {
                        angles,
                        radius,
                        radius_jitter: f32::from(radius_jitter).abs() * 0.1,
                        angle_jitter: f32::from(angle_jitter).abs() * 0.1,
                        radius_draw: (radius_jitter != 0).then(|| self.draw()).transpose()?,
                        angle_draw: (angle_jitter != 0).then(|| self.draw()).transpose()?,
                    });
                    match target {
                        VectorTarget::Offset => {
                            birth.offset_spread = spread;
                            birth.offset = [None; 3];
                            birth
                                .sums
                                .retain(|sum| !matches!(sum.field, BirthField::Offset(_)));
                        }
                        VectorTarget::Velocity => {
                            birth.velocity_spread = spread;
                            birth.velocity = [None; 3];
                            birth
                                .sums
                                .retain(|sum| !matches!(sum.field, BirthField::Velocity(_)));
                        }
                        _ => bail!("invalid polar destination"),
                    }
                }
                InputEdit::CullBack => birth.cull_back = true,
                InputEdit::ElementTint => birth.element_tint = true,
                InputEdit::RibbonDepth => birth.ribbon_depth = true,
            }
        }
        if let Some(model) = animated_model {
            let Some(Declaration::ModelParticle { binding, .. }) = declaration else {
                bail!("model animation start has no model declaration");
            };
            let choice = *birth
                .model
                .get_or_insert(ValueRange::fixed(f32::from(binding.slot)));
            ensure!(
                choice.min == f32::from(model) && choice.max == choice.min,
                "model animation start selects a different particle model"
            );
        }
        if let Some(lifetime) = &mut birth.lifetime {
            ensure!(
                lifetime.min > 0. || lifetime.max == 0.,
                "random lifetime mixes disabled and finite durations"
            );
            if lifetime.min > 0. {
                lifetime.min += 1.;
                lifetime.max += 1.;
            }
        }
        birth.random_draws = self.draws - first_draw;
        birth.validate()?;
        Ok(birth)
    }
}

fn sum_field(target: InputScalarTarget, declaration: Option<&Declaration>) -> Option<BirthField> {
    match target {
        InputScalarTarget::Vector {
            vector: VectorTarget::Offset,
            axis,
        } => Some(BirthField::Offset(axis)),
        InputScalarTarget::Vector {
            vector: VectorTarget::Velocity,
            axis,
        } => Some(BirthField::Velocity(axis)),
        InputScalarTarget::Vector {
            vector: VectorTarget::Size,
            axis,
        } if declaration
            .and_then(|value| value.template().ok())
            .is_some_and(|template| {
                matches!(
                    template.state.geometry,
                    ParticleGeometry::Size { .. } | ParticleGeometry::Spiral { .. }
                )
            }) =>
        {
            Some(BirthField::Size(axis))
        }
        _ => None,
    }
}

fn sum_slot(birth: &mut ParticleBirth, field: BirthField) -> &mut Option<ValueRange> {
    match field {
        BirthField::Offset(axis) => &mut birth.offset[usize::from(axis)],
        BirthField::Velocity(axis) => &mut birth.velocity[usize::from(axis)],
        BirthField::Size(axis) => &mut birth.size[usize::from(axis)],
        BirthField::Uv(axis) => &mut birth.uv[usize::from(axis)],
    }
}

fn add_sum(
    birth: &mut ParticleBirth,
    field: BirthField,
    initial: Value,
    value: Value,
    operation: &Arithmetic,
) -> Result<bool> {
    let index = birth.sums.iter().position(|sum| sum.field == field);
    if *operation == Arithmetic::Set {
        if let Some(index) = index {
            birth.sums.remove(index);
        }
        return Ok(false);
    }
    let before = sum_slot(birth, field).unwrap_or(initial.range);
    let independent = before.min != before.max
        && value.range.min != value.range.max
        && before.draw != value.range.draw;
    if index.is_none() && !independent {
        return Ok(false);
    }
    ensure!(
        matches!(operation, Arithmetic::Add | Arithmetic::Subtract)
            && initial.heading == 0.
            && value.heading == 0.,
        "particle sum requires additive scalar settings"
    );
    let term = if *operation == Arithmetic::Subtract {
        arithmetic(Value::constant(0.), value, Arithmetic::Subtract)?.range
    } else {
        value.range
    };
    let index = index.unwrap_or_else(|| {
        birth.sums.push(BirthSum {
            field,
            terms: vec![before],
        });
        birth.sums.len() - 1
    });
    birth.sums[index].terms.push(term);
    *sum_slot(birth, field) = None;
    Ok(true)
}

fn scalar_slot<'a>(
    birth: &'a mut ParticleBirth,
    target: InputScalarTarget,
    declaration: Option<&Declaration>,
) -> &'a mut Option<ValueRange> {
    match target {
        InputScalarTarget::Vector { vector, axis } => {
            if declaration.is_some_and(|declaration| {
                declaration.template().is_ok_and(|template| {
                    matches!(template.state.geometry, ParticleGeometry::Quad { .. })
                })
            }) {
                let vertex = match vector {
                    VectorTarget::Size => Some(0),
                    VectorTarget::SizeVelocity => Some(1),
                    VectorTarget::SizeAcceleration => Some(2),
                    _ => None,
                };
                if let Some(vertex) = vertex {
                    return &mut birth.vertices[vertex][usize::from(axis)];
                }
            }
            let vector = match vector {
                VectorTarget::Offset => &mut birth.offset,
                VectorTarget::Velocity => &mut birth.velocity,
                VectorTarget::Acceleration => &mut birth.acceleration,
                VectorTarget::Angles => &mut birth.angles,
                VectorTarget::AngularVelocity => &mut birth.angular_velocity,
                VectorTarget::SegmentOffset => &mut birth.segment_offset,
                VectorTarget::Orbit => &mut birth.orbit,
                VectorTarget::OrbitVelocity => &mut birth.orbit_velocity,
                VectorTarget::Size => &mut birth.size,
                VectorTarget::SizeVelocity => &mut birth.size_velocity,
                VectorTarget::SizeAcceleration => &mut birth.size_acceleration,
            };
            &mut vector[usize::from(axis)]
        }
        InputScalarTarget::SegmentAngleStep => &mut birth.segment_angle_step,
        InputScalarTarget::Scratch { .. } => unreachable!(),
    }
}
fn integer_slot(birth: &mut ParticleBirth, target: InputIntegerTarget) -> &mut Option<ValueRange> {
    match target {
        InputIntegerTarget::Color { color: 0, channel } => &mut birth.color[usize::from(channel)],
        InputIntegerTarget::Color { channel, .. } => &mut birth.end_color[usize::from(channel)],
        InputIntegerTarget::Uv { component } => &mut birth.uv[usize::from(component)],
        InputIntegerTarget::Brighten { channel } => &mut birth.brighten[usize::from(channel)],
        InputIntegerTarget::Fade { channel } => &mut birth.fade[usize::from(channel)],
        InputIntegerTarget::BrightenUntil => &mut birth.brighten_until,
        InputIntegerTarget::Lifetime => &mut birth.lifetime,
        InputIntegerTarget::FadeFrom => &mut birth.fade_from,
        InputIntegerTarget::GeometryCount => &mut birth.geometry_count,
        InputIntegerTarget::Phase => &mut birth.phase,
        InputIntegerTarget::Palette => &mut birth.palette,
        InputIntegerTarget::Model => &mut birth.model,
        _ => unreachable!(),
    }
}

fn arithmetic(left: Value, right: Value, operation: Arithmetic) -> Result<Value> {
    let scale = |value: Value, factor: f32| {
        let a = value.range.min * factor;
        let b = value.range.max * factor;
        Value {
            range: ValueRange {
                min: a.min(b),
                max: a.max(b),
                step: value.range.step * factor.abs(),
                draw: value.range.draw,
                reverse: value.range.reverse ^ factor.is_sign_negative(),
            },
            heading: value.heading * factor,
        }
    };
    let value = match operation {
        Arithmetic::Set => right,
        Arithmetic::Add | Arithmetic::Subtract => {
            let right = scale(
                right,
                if operation == Arithmetic::Subtract {
                    -1.
                } else {
                    1.
                },
            );
            let left_varies = left.range.min != left.range.max;
            let right_varies = right.range.min != right.range.max;
            ensure!(
                !left_varies
                    || !right_varies
                    || (left.range.draw.is_some() && left.range.draw == right.range.draw),
                "summing independent particle variations is unsupported"
            );
            let step = if !left_varies {
                right.range.step
            } else if !right_varies {
                left.range.step
            } else {
                let signed = |range: ValueRange| {
                    if range.reverse {
                        -range.step
                    } else {
                        range.step
                    }
                };
                (signed(left.range) + signed(right.range)).abs()
            };
            let ends = |range: ValueRange| {
                if range.reverse {
                    [range.max, range.min]
                } else {
                    [range.min, range.max]
                }
            };
            let a = ends(left.range);
            let b = ends(right.range);
            let start = a[0] + b[0];
            let end = a[1] + b[1];
            Value {
                range: ValueRange {
                    min: start.min(end),
                    max: start.max(end),
                    step,
                    draw: if left_varies {
                        left.range.draw
                    } else if right_varies {
                        right.range.draw
                    } else {
                        None
                    },
                    reverse: start > end,
                },
                heading: left.heading + right.heading,
            }
        }
        Arithmetic::Multiply if right.fixed() => scale(left, right.range.min),
        Arithmetic::Multiply if left.fixed() => scale(right, left.range.min),
        Arithmetic::Divide if right.fixed() && right.range.min != 0. => {
            scale(left, 1. / right.range.min)
        }
        _ => bail!("nonlinear particle variation is unsupported"),
    };
    value.range.validate()?;
    ensure!(value.heading.is_finite(), "nonfinite particle heading");
    Ok(value)
}

fn initial_scalar(declaration: &Declaration, target: InputScalarTarget) -> Result<Value> {
    let data = declaration.template()?;
    let state = &data.state;
    let value = match target {
        InputScalarTarget::SegmentAngleStep => match &state.geometry {
            ParticleGeometry::BillboardTrail {
                segment_angle_step, ..
            }
            | ParticleGeometry::Spiral {
                segment_angle_step, ..
            } => *segment_angle_step,
            _ => bail!("particle has no segment angle"),
        },
        InputScalarTarget::Vector { vector, axis } => {
            let values = match vector {
                VectorTarget::Offset => state.offset,
                VectorTarget::Velocity => state.velocity,
                VectorTarget::Acceleration => state.acceleration,
                VectorTarget::Angles => state.angles,
                VectorTarget::AngularVelocity => state.angular_velocity,
                VectorTarget::SegmentOffset => match &declaration.particle_visual()?.geometry {
                    visual::ParticleShape::Spiral { segment_offset, .. } => *segment_offset,
                    _ => bail!("particle has no spiral segment displacement"),
                },
                VectorTarget::Orbit => state.orbit,
                VectorTarget::OrbitVelocity => data.orbit_velocity,
                other => match &state.geometry {
                    ParticleGeometry::Size {
                        value,
                        velocity,
                        acceleration,
                    }
                    | ParticleGeometry::Spiral {
                        value,
                        velocity,
                        acceleration,
                        ..
                    } => match other {
                        VectorTarget::Size => *value,
                        VectorTarget::SizeVelocity => *velocity,
                        VectorTarget::SizeAcceleration => *acceleration,
                        _ => unreachable!(),
                    },
                    ParticleGeometry::BillboardTrail { size, radius, .. }
                        if other == VectorTarget::Size =>
                    {
                        [size[0], size[1], *radius]
                    }
                    ParticleGeometry::BillboardTrail {
                        radius_velocity, ..
                    } if other == VectorTarget::SizeVelocity && axis == 2 => {
                        [0., 0., *radius_velocity]
                    }
                    ParticleGeometry::Ribbon { length, width, .. }
                        if other == VectorTarget::Size && axis < 2 =>
                    {
                        [*length, *width, 0.]
                    }
                    ParticleGeometry::Quad { vertices, .. } => match other {
                        VectorTarget::Size => vertices[0],
                        VectorTarget::SizeVelocity => vertices[1],
                        VectorTarget::SizeAcceleration => vertices[2],
                        _ => unreachable!(),
                    },
                    _ => bail!("particle has no size vector"),
                },
            };
            values[usize::from(axis)]
        }
        InputScalarTarget::Scratch { .. } => bail!("scratch value has no particle template"),
    };
    let mut value = Value::constant(value);
    if matches!(declaration, Declaration::ModelParticle { .. })
        && target
            == (InputScalarTarget::Vector {
                vector: VectorTarget::Angles,
                axis: 1,
            })
    {
        value.heading = 1.;
    }
    Ok(value)
}
fn initial_integer(declaration: &Declaration, target: InputIntegerTarget) -> Result<f32> {
    let data = declaration.template()?;
    let state = &data.state;
    Ok(match target {
        InputIntegerTarget::Color { color, channel } => {
            f32::from(state.colors[usize::from(color)][usize::from(channel)])
        }
        InputIntegerTarget::Uv { component } => f32::from(state.uv[usize::from(component)]),
        InputIntegerTarget::Brighten { channel } => f32::from(state.brighten[usize::from(channel)]),
        InputIntegerTarget::Fade { channel } => f32::from(data.fade[usize::from(channel)]),
        InputIntegerTarget::BrightenUntil => state.brighten_until.unwrap_or(0) as f32,
        InputIntegerTarget::Lifetime => data.lifetime.map_or(0, |count| count - 1) as f32,
        InputIntegerTarget::FadeFrom => data.fade_from as f32,
        InputIntegerTarget::GeometryCount => f32::from(state.geometry_count),
        InputIntegerTarget::Palette => f32::from(state.palettes[0]),
        InputIntegerTarget::Model => match declaration {
            Declaration::ModelParticle { binding, .. } => f32::from(binding.slot),
            _ => bail!("model selection requires a model particle"),
        },
        InputIntegerTarget::Phase => match state.geometry {
            ParticleGeometry::Ribbon { phase, .. } => f32::from(phase),
            _ => bail!("particle has no ribbon phase"),
        },
        _ => bail!("value has no particle template"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_birth_edits_preserve_vertices_ribbon_depth_and_integer_fade() -> Result<()> {
        let mut row = [0; 352];
        row[0] = 15;
        row[50] = 255;
        row[44..48].fill(5);
        row[180..184].copy_from_slice(&300_f32.to_be_bytes());
        row[192..196].copy_from_slice(&240_f32.to_be_bytes());
        let declaration = crate::battle_effect::declaration::read(&row, &[])?;
        let words = [
            13,
            0xb4,
            0x42c8,
            0,
            0,
            0,
            13,
            0xc0,
            0x42c8,
            0,
            0,
            0,
            25,
            0x2c,
            2,
            0,
            10,
            3,
            0,
            0,
            u16::MAX,
        ];
        let birth = Compiler::default().birth(
            edits_for(&words, Some(&declaration), None)?,
            Some(&declaration),
        )?;
        assert_eq!(birth.vertices[0][1], Some(ValueRange::fixed(200.)));
        assert_eq!(birth.vertices[1][1], Some(ValueRange::fixed(140.)));
        assert_eq!(birth.fade[0], Some(ValueRange::fixed(2.)));
        assert_eq!(birth.palette, Some(ValueRange::fixed(0.)));
        assert!(
            birth
                .size
                .iter()
                .chain(&birth.size_velocity)
                .all(Option::is_none)
        );

        row[0] = 18;
        row[18] = 8;
        row[176..180].copy_from_slice(&500_f32.to_be_bytes());
        let declaration = crate::battle_effect::declaration::read(&row, &[])?;
        let words = [13, 0xb0, 0x42c8, 0, 0, 0, 21, 0x14, 0, 0x20, u16::MAX];
        let birth = Compiler::default().birth(
            edits_for(&words, Some(&declaration), None)?,
            Some(&declaration),
        )?;
        assert!(birth.ribbon_depth);
        assert_eq!(birth.size[0], Some(ValueRange::fixed(400.)));
        Ok(())
    }

    #[test]
    fn signed_scalar_zero_range_and_auxiliary_preserve_one_draw() -> Result<()> {
        let birth = |range, auxiliary| {
            Compiler::default().birth(edits(&[11, 0x5c, range, auxiliary, u16::MAX])?, None)
        };
        let signed = birth(0, 0x7ff9)?;
        assert_eq!(signed, birth(0, 0)?);
        assert_eq!(signed.random_draws, 1);
        let yaw = signed.angles[1].unwrap();
        assert_eq!(yaw.step, 0.1);
        assert_eq!(yaw.sample(0.), -3276.8);
        assert!((yaw.sample(0.99999) - 3276.7).abs() < 0.001);
        let unit = birth(1, 0)?;
        assert_eq!(unit.random_draws, 1);
        assert_eq!(unit.angles[1].unwrap().sample(0.75), 0.);
        for range in [1800, (-1800_i16) as u16] {
            let pitch = birth(range, 0)?.angles[1].unwrap();
            assert_eq!(pitch.step, 0.1);
            assert!((pitch.min + 179.9).abs() < 0.0001);
            assert!((pitch.max - 179.9).abs() < 0.0001);
            for unit in [0., 0.123, 0.5, 0.99999] {
                let angle = pitch.sample(unit);
                assert!((pitch.min..=pitch.max).contains(&angle));
                assert!((angle * 10. - (angle * 10.).round()).abs() < 0.001);
            }
        }
        Ok(())
    }

    #[test]
    fn spiral_displacement_and_packed_alpha_palette_keep_shared_draws() -> Result<()> {
        let mut row = [0; 352];
        row[0] = 10;
        row[3] = 1;
        row[4] = 1;
        row[18] = 8;
        row[50] = 255;
        row[116..120].copy_from_slice(&2.5_f32.to_be_bytes());
        let declaration = crate::battle_effect::declaration::read(&row, &[])?;
        let birth = Compiler::default().birth(
            edits(&[
                11,
                0x7ff8,
                10,
                0,
                8,
                0x44,
                0,
                0,
                0x7ff8,
                0,
                8,
                0x74,
                0,
                0,
                0x7ff8,
                0,
                2,
                0x7ffc,
                4,
                0,
                3,
                3,
                0x7ffc,
                0,
                u16::MAX,
            ])?,
            Some(&declaration),
        )?;
        assert_eq!(birth.random_draws, 2);
        assert_eq!(
            birth.velocity[1].unwrap().draw,
            birth.segment_offset[1].unwrap().draw
        );
        for unit in [0., 0.5, 0.99] {
            assert_eq!(
                birth.segment_offset[1].unwrap().sample(unit)
                    - birth.velocity[1].unwrap().sample(unit),
                2.5
            );
        }
        assert!(birth.palette.is_none());
        let alpha = birth.alpha_palette.unwrap();
        assert_eq!(
            (alpha.min, alpha.max, alpha.step, alpha.draw),
            (1., 4., 1., Some(1))
        );
        row[4] = 255;
        let overflowing = crate::battle_effect::declaration::read(&row, &[])?;
        assert!(
            Compiler::default()
                .birth(edits(&[3, 3, 1, 0, u16::MAX])?, Some(&overflowing))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn dead_scalar_halfword_writes_preserve_draws_and_reject_later_reads() -> Result<()> {
        let mut bytes = [0; 352];
        bytes[0] = 8;
        bytes[16..18].copy_from_slice(&50_i16.to_be_bytes());
        bytes[18] = 8;
        bytes[19] = 1;
        bytes[49] = 18;
        bytes[50] = 255;
        let declaration = crate::battle_effect::declaration::read(&bytes, &[])?;
        let mut compiler = Compiler::default();
        let records: &[&[u16]] = &[
            &[2, 0x7ffc, 15, 0],
            &[3, 0x10, 0x7ffc, 0],
            &[12, 0x31, 0x7ffc, 0],
            &[2, 0x7ff8, 2, 0],
            &[12, 0x12, 0x7ffc, 0],
            &[u16::MAX],
        ];
        let birth = compiler.birth(edits(&records.concat())?, Some(&declaration))?;
        assert_eq!(birth.random_draws, 2);
        for (range, minimum) in [
            (birth.lifetime.unwrap(), 51.),
            (birth.fade_from.unwrap(), 18.),
            (birth.geometry_count.unwrap(), 8.),
        ] {
            assert_eq!(range.draw, Some(0));
            assert_eq!(
                [range.min, range.max, range.step],
                [minimum, minimum + 14., 1.]
            );
        }
        // Scratch remains opaque across births, for both operand and destination reads.
        for words in [
            [7, 0x34, 0, 0, 0x7ff8, 0, u16::MAX],
            [7, 0x7ff8, 0, 0, 0x7ff8, 0, u16::MAX],
            [8, 0x7ff8, 0x3f80, 0, 0, 0, u16::MAX],
        ] {
            assert!(compiler.birth(edits(&words)?, None).is_err());
        }
        let next = compiler.birth(
            edits(&[11, 0x7ff8, 100, 0, 7, 0x34, 0, 0, 0x7ff8, 0, u16::MAX])?,
            None,
        )?;
        assert_eq!(next.random_draws, 1);
        assert_eq!(next.offset[0].unwrap().draw, Some(2));
        Ok(())
    }

    #[test]
    fn trail_radius_velocity_adds_to_its_template_without_spatial_size_motion() -> Result<()> {
        let mut bytes = [0; 352];
        bytes[0] = 8;
        bytes[19] = 1;
        bytes[50] = 255;
        bytes[196..200].copy_from_slice(&1.5_f32.to_be_bytes());
        let declaration = crate::battle_effect::declaration::read(&bytes, &[])?;
        let birth = Compiler::default().birth(
            edits(&[8, 0xc4, 0x4040, 0, 0, 0, u16::MAX])?,
            Some(&declaration),
        )?;
        assert_eq!(
            birth.size_velocity,
            [None, None, Some(ValueRange::fixed(4.5))]
        );
        assert!(
            Compiler::default()
                .birth(
                    edits(&[8, 0xbc, 0x4040, 0, 0, 0, u16::MAX])?,
                    Some(&declaration),
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn repeats_follow_due_commands_and_stay_bounded() {
        let sound = |id| InputOperation::Sound { id, priority: 0 };
        let result = timeline(
            InputTimeline {
                events: vec![
                    InputEvent {
                        at: 0,
                        repeat: Some(Repeat {
                            count: 5,
                            interval: 1,
                        }),
                        operation: sound(1),
                    },
                    InputEvent {
                        at: 0,
                        repeat: None,
                        operation: sound(2),
                    },
                    InputEvent {
                        at: 1,
                        repeat: None,
                        operation: sound(3),
                    },
                ],
                end: 2,
            },
            &[],
        );
        let sounds: Vec<_> = result
            .iter()
            .map(|event| match event.operation {
                EffectOperation::Sound { id, .. } => (event.at, id),
                _ => panic!("sound expected"),
            })
            .collect();
        assert_eq!(sounds, [(0, 2), (0, 1), (1, 3), (1, 1)]);
    }

    #[test]
    fn arithmetic_becomes_birth_ranges_and_discrete_texture_choices() -> Result<()> {
        let acceleration =
            Compiler::default().birth(edits(&[7, 0x50, 0x3f80, 0, 0, 0, u16::MAX])?, None)?;
        assert_eq!(
            acceleration.acceleration,
            [None, Some(ValueRange::fixed(1.)), None]
        );
        let birth = Compiler::default().birth(
            vec![
                InputEdit::Scalar {
                    target: InputScalarTarget::Scratch { index: 0 },
                    arithmetic: Arithmetic::Set,
                    value: InputScalarValue::Random {
                        range: 100,
                        scale: 0.1,
                    },
                },
                InputEdit::Scalar {
                    target: InputScalarTarget::Scratch { index: 0 },
                    arithmetic: Arithmetic::Add,
                    value: InputScalarValue::Literal { value: 4. },
                },
                InputEdit::Scalar {
                    target: InputScalarTarget::Vector {
                        vector: VectorTarget::Offset,
                        axis: 0,
                    },
                    arithmetic: Arithmetic::Set,
                    value: InputScalarValue::Scratch { index: 0 },
                },
                InputEdit::Integer {
                    target: InputIntegerTarget::Scratch { index: 0 },
                    arithmetic: Arithmetic::Set,
                    value: InputIntegerValue::Random { range: 4 },
                },
                InputEdit::Integer {
                    target: InputIntegerTarget::Scratch { index: 0 },
                    arithmetic: Arithmetic::Multiply,
                    value: InputIntegerValue::Literal { value: 6 },
                },
                InputEdit::Integer {
                    target: InputIntegerTarget::Uv { component: 0 },
                    arithmetic: Arithmetic::Set,
                    value: InputIntegerValue::Scratch { index: 0 },
                },
                InputEdit::Integer {
                    target: InputIntegerTarget::Model,
                    arithmetic: Arithmetic::Set,
                    value: InputIntegerValue::Literal { value: 7 },
                },
            ],
            None,
        )?;
        let offset = birth.offset[0].unwrap();
        // Signed variation uses discrete tenths, then shifts by four units.
        for (unit, expected) in [(0., -5.9), (0.001, -5.9), (0.5, 4.), (0.999, 13.9)] {
            assert!((offset.sample(unit) - expected).abs() < 0.00001);
        }
        assert_eq!(offset.draw, Some(0));
        let uv = birth.uv[0].unwrap();
        assert_eq!(
            [0., 0.25, 0.5, 0.99].map(|unit| uv.sample(unit)),
            [0., 6., 12., 18.]
        );
        assert_eq!(birth.model, Some(ValueRange::fixed(7.)));
        assert_eq!(birth.random_draws, 2);
        assert_eq!(uv.draw, Some(1));
        let inverted = arithmetic(
            Value {
                range: uv,
                heading: 0.,
            },
            Value::constant(-2.),
            Arithmetic::Multiply,
        )?
        .range;
        assert_eq!(inverted.draw, uv.draw);
        for unit in [0., 0.25, 0.5, 0.99] {
            assert_eq!(inverted.sample(unit), uv.sample(unit) * -2.);
        }
        for value in [-1, 256] {
            assert!(
                Compiler::default()
                    .birth(
                        vec![InputEdit::Integer {
                            target: InputIntegerTarget::Model,
                            arithmetic: Arithmetic::Set,
                            value: InputIntegerValue::Literal { value }
                        }],
                        None
                    )
                    .is_err()
            );
        }
        let roll_birth = |bits: u32, operand: u16| -> Result<ParticleBirth> {
            let mut row = [0; 352];
            row[0] = 12;
            row[2] = 255;
            row[50] = 255;
            row[0x60..0x64].copy_from_slice(&bits.to_be_bytes());
            let declaration = crate::battle_effect::declaration::read(&row, &[])?;
            Compiler::default().birth(
                edits(&[0, 0x7ffc, 10, 0, 3, 0x60, operand, 0, u16::MAX])?,
                Some(&declaration),
            )
        };
        for (before, operand, expected) in [
            (0, 0, 0),
            (0x4234_0000, 0, 0x4234_0000),
            (0x4234_0000, 1, 0x4235_0000),
            (0x4234_1234, 1, 0x4235_1234),
        ] {
            let birth = roll_birth(before, operand)?;
            assert_eq!(birth.angles[2].unwrap().min.to_bits(), expected);
            assert_eq!(birth.random_draws, 0);
        }
        for (before, operand) in [(0, 1), (0x7f7f_0000, 1), (0xff7f_0000, 256)] {
            assert!(roll_birth(before, operand).is_err());
        }
        Ok(())
    }

    #[test]
    fn fake_model_choice_uses_the_authored_integer_draw() -> Result<()> {
        let records: &[&[u16]] = &[
            &[11, 0x5c, 900, 0],
            &[11, 0x58, 3600, 0],
            &[2, 0x7ffc, 4, 0],
            &[10, 0xd4, 0x7ffc, 0],
            &[u16::MAX],
        ];
        let birth = Compiler::default().birth(edits(&records.concat())?, None)?;
        let model = birth.model.unwrap();
        assert_eq!(birth.random_draws, 3);
        assert_eq!(model.draw, Some(2));
        assert_eq!(model.step, 1.);
        assert_eq!(
            [0., 0.249, 0.25, 0.5, 0.75, 0.999].map(|unit| model.sample(unit)),
            [0., 0., 1., 2., 3., 3.]
        );
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../content/tests/fixtures/projectile-model-follow.json"
        ))?;
        let mut declaration: Declaration =
            serde_json::from_value(fixture["files"]["effects/throw/actors.json"][0].clone())?;
        let birth = Compiler::default().birth(
            edits(&[2, 0x7ffc, 3, 0, 12, 0xd4, 0x7ffc, 0, u16::MAX])?,
            Some(&declaration),
        )?;
        let model = birth.model.unwrap();
        assert_eq!(birth.random_draws, 1);
        assert_eq!(model.draw, Some(0));
        assert_eq!([0., 0.5, 0.99].map(|unit| model.sample(unit)), [1., 2., 3.]);
        let Declaration::ModelParticle { template, .. } = &mut declaration else {
            panic!("model particle");
        };
        template.state.angles[1] = -10.;
        for factor in [-1., 0., 1.] {
            let target = InputScalarTarget::Vector {
                vector: VectorTarget::Angles,
                axis: 1,
            };
            let birth = Compiler::default().birth(
                vec![
                    InputEdit::Scalar {
                        target,
                        arithmetic: Arithmetic::Multiply,
                        value: InputScalarValue::Literal { value: factor },
                    },
                    InputEdit::Scalar {
                        target,
                        arithmetic: Arithmetic::Add,
                        value: InputScalarValue::Literal { value: 5. },
                    },
                ],
                Some(&declaration),
            )?;
            assert_eq!(birth.angles[1], Some(ValueRange::fixed(-10. * factor + 5.)));
            assert_eq!(birth.yaw_heading, factor);
            assert_eq!(birth.random_draws, 0);
        }
        Ok(())
    }

    #[test]
    fn arena_scratch_draws_stay_shared_and_repeated_emissions_get_fresh_draws() -> Result<()> {
        let mut settings: Vec<_> = [
            (VectorTarget::Offset, 0, 2800.),
            (VectorTarget::Offset, 2, 0.),
            (VectorTarget::Velocity, 1, 0.),
            (VectorTarget::Size, 0, 32.),
            (VectorTarget::Size, 1, 32.),
        ]
        .into_iter()
        .map(|(vector, axis, value)| InputEdit::Scalar {
            target: InputScalarTarget::Vector { vector, axis },
            arithmetic: Arithmetic::Set,
            value: InputScalarValue::Literal { value },
        })
        .collect();
        // Arena 31: vertical drift, a shared X/Z displacement, then a shared size.
        let records: &[&[u16]] = &[
            &[11, 0x7ff9, 10, 0],
            &[8, 0x44, 0, 0, 0x7ff9, 0],
            &[11, 0x7ff8, 2500, 0],
            &[8, 0x34, 0, 0, 0x7ff8, 0],
            &[8, 0x3c, 0, 0, 0x7ff8, 0],
            &[11, 0x7ffa, 80, 0],
            &[8, 0xb4, 0, 0, 0x7ffa, 0],
            &[8, 0xb0, 0, 0, 0x7ffa, 0],
            &[u16::MAX],
        ];
        settings.extend(edits(&records.concat())?);
        let mut compiler = Compiler::default();
        for emission in 0..2 {
            let birth = compiler.birth(settings.clone(), None)?;
            assert_eq!(birth.random_draws, 3);
            assert_eq!(birth.velocity[1].unwrap().draw, Some(emission * 3));
            let x = birth.offset[0].unwrap();
            let z = birth.offset[2].unwrap();
            assert_eq!(x.draw, Some(emission * 3 + 1));
            assert_eq!(x.draw, z.draw);
            assert_eq!(birth.size[0], birth.size[1]);
            assert_eq!(birth.size[0].unwrap().draw, Some(emission * 3 + 2));
            for unit in [0., 0.25, 0.5, 0.99] {
                assert!((x.sample(unit) - 2800. - z.sample(unit)).abs() < 0.001);
            }
        }
        let reused = compiler.birth(edits(&[7, 0x34, 0, 0, 0x7ff8, 0, u16::MAX])?, None)?;
        assert_eq!(reused.random_draws, 0);
        assert_eq!(reused.offset[0].unwrap().draw, Some(4));
        Ok(())
    }
}
