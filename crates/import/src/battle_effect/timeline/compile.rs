//! Resolve bounded schedules and arithmetic into particle settings.
use super::*;

pub(super) fn timeline(input: InputTimeline, actors: &[Declaration]) -> Vec<ScheduledEvent> {
    let mut expanded = Vec::new();
    for (index, event) in input.events.into_iter().enumerate() {
        if let Some(repeat) = event.repeat {
            for emission in 0..repeat.count {
                let at = event.at + u32::from(emission) * repeat.interval;
                if at < input.end {
                    expanded.push((at, index, event.operation.clone()));
                }
            }
        } else {
            expanded.push((event.at, index, event.operation));
        }
    }
    expanded.sort_by_key(|(at, index, _)| (*at, *index));
    let mut compiler = Compiler::default();
    let mut events = Vec::new();
    for (at, _, operation) in expanded {
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
    scalars: [Value; 4],
    integers: [Value; 4],
}
impl Default for Compiler {
    fn default() -> Self {
        Self {
            scalars: [Value::constant(0.); 4],
            integers: [Value::constant(0.); 4],
        }
    }
}
impl Compiler {
    fn operation(
        &mut self,
        operation: InputOperation,
        actors: &[Declaration],
    ) -> Result<EffectOperation> {
        Ok(match operation {
            InputOperation::Spawn {
                particle,
                blend,
                palette,
                edits,
            } => EffectOperation::Spawn {
                particle,
                blend,
                palette,
                birth: Box::new(self.birth(edits, actors.get(usize::from(particle)))?),
            },
            InputOperation::Sound { id, priority } => EffectOperation::Sound { id, priority },
            InputOperation::Shake {
                duration,
                amplitude,
            } => EffectOperation::Shake {
                duration,
                amplitude,
            },
        })
    }
    fn birth(
        &mut self,
        edits: Vec<InputEdit>,
        declaration: Option<&Declaration>,
    ) -> Result<ParticleBirth> {
        let mut birth = ParticleBirth::default();
        for edit in edits {
            match edit {
                InputEdit::Scalar {
                    target,
                    arithmetic: operation,
                    value,
                } => {
                    let value = match value {
                        InputScalarValue::Literal { value } => Value::constant(value),
                        InputScalarValue::Scratch { index } => self.scalars[usize::from(index)],
                        InputScalarValue::Random { range, scale } => {
                            let extent = (f32::from(range) * scale).abs();
                            Value {
                                range: ValueRange {
                                    min: -extent,
                                    max: extent,
                                    step: 0.,
                                },
                                heading: 0.,
                            }
                        }
                    };
                    if let InputScalarTarget::Scratch { index } = target {
                        let slot = &mut self.scalars[usize::from(index)];
                        *slot = arithmetic(*slot, value, operation)?;
                    } else {
                        let yaw = target
                            == (InputScalarTarget::Vector {
                                vector: VectorTarget::Angles,
                                axis: 1,
                            });
                        let heading = if yaw && birth.relative_yaw { 1. } else { 0. };
                        let polar = matches!(target,
                            InputScalarTarget::Vector { vector: VectorTarget::Offset, .. } if birth.offset_spread.is_some())
                            || matches!(target,
                            InputScalarTarget::Vector { vector: VectorTarget::Velocity, .. } if birth.velocity_spread.is_some());
                        let slot = scalar_slot(&mut birth, target);
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
                            value.heading == 0. || (value.heading == 1. && yaw),
                            "unsupported relative particle angle"
                        );
                        *slot = Some(value.range);
                        if yaw {
                            birth.relative_yaw = value.heading == 1.;
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
                            },
                            heading: 0.,
                        },
                    };
                    if let InputIntegerTarget::Scratch { index } = target {
                        let slot = &mut self.integers[usize::from(index)];
                        *slot = arithmetic(*slot, value, operation)?;
                    } else if target == InputIntegerTarget::Model {
                        ensure!(
                            operation == Arithmetic::Set,
                            "model selection must be explicit"
                        );
                        ensure!(
                            value.fixed()
                                && value.range.min.fract() == 0.
                                && (0. ..=255.).contains(&value.range.min),
                            "particle model needs a fixed valid identity"
                        );
                        birth.model = Some(value.range.min as u8);
                    } else {
                        let slot = integer_slot(&mut birth, target);
                        let before = match *slot {
                            Some(range) => Value { range, heading: 0. },
                            None if operation == Arithmetic::Set => Value::constant(0.),
                            None => Value::constant(initial_integer(
                                declaration.context("missing particle declaration")?,
                                target,
                            )?),
                        };
                        *slot = Some(arithmetic(before, value, operation)?.range);
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
                    });
                    match target {
                        VectorTarget::Offset => {
                            birth.offset_spread = spread;
                            birth.offset = [None; 3];
                        }
                        VectorTarget::Velocity => {
                            birth.velocity_spread = spread;
                            birth.velocity = [None; 3];
                        }
                        _ => bail!("invalid polar destination"),
                    }
                }
                InputEdit::CullBack => birth.cull_back = true,
                InputEdit::ElementTint => birth.element_tint = true,
            }
        }
        birth.validate()?;
        Ok(birth)
    }
}

fn scalar_slot(birth: &mut ParticleBirth, target: InputScalarTarget) -> &mut Option<ValueRange> {
    match target {
        InputScalarTarget::Vector { vector, axis } => {
            let vector = match vector {
                VectorTarget::Offset => &mut birth.offset,
                VectorTarget::Velocity => &mut birth.velocity,
                VectorTarget::Angles => &mut birth.angles,
                VectorTarget::AngularVelocity => &mut birth.angular_velocity,
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
        InputIntegerTarget::GeometryCount => &mut birth.geometry_count,
        InputIntegerTarget::Phase => &mut birth.phase,
        InputIntegerTarget::Palette => &mut birth.palette,
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
            let step = if left.fixed() {
                right.range.step
            } else if right.fixed() || left.range.step == right.range.step {
                left.range.step
            } else {
                0.
            };
            Value {
                range: ValueRange {
                    min: left.range.min + right.range.min,
                    max: left.range.max + right.range.max,
                    step,
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
                VectorTarget::Offset => &state.offset,
                VectorTarget::Velocity => &state.velocity,
                VectorTarget::Angles => &state.angles,
                VectorTarget::AngularVelocity => &state.angular_velocity,
                VectorTarget::Orbit => &state.orbit,
                VectorTarget::OrbitVelocity => &data.orbit_velocity,
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
                        VectorTarget::Size => value,
                        VectorTarget::SizeVelocity => velocity,
                        VectorTarget::SizeAcceleration => acceleration,
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
        InputIntegerTarget::GeometryCount => f32::from(state.geometry_count),
        InputIntegerTarget::Palette => f32::from(state.palettes[0]),
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
    fn repeats_are_bounded_and_keep_authored_order() {
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
        assert_eq!(sounds, [(0, 1), (0, 2), (1, 1), (1, 3)]);
    }

    #[test]
    fn arithmetic_becomes_birth_ranges_and_discrete_texture_choices() -> Result<()> {
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
        assert_eq!(
            birth.offset[0],
            Some(ValueRange {
                min: -6.,
                max: 14.,
                step: 0.
            })
        );
        let uv = birth.uv[0].unwrap();
        assert_eq!(
            [0., 0.25, 0.5, 0.99].map(|unit| uv.sample(unit)),
            [0., 6., 12., 18.]
        );
        assert_eq!(birth.model, Some(7));
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
        Ok(())
    }
}
