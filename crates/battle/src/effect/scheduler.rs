use super::*;
use resonance_content::battle_effect::*;

#[derive(Debug, Clone, Default)]
pub struct EffectDefinition {
    pub events: Vec<ScheduledEvent>,
    pub particles: BTreeMap<u8, Arc<crate::ParticleDefinition>>,
    pub sounds: BTreeMap<u16, crate::Sound>,
}
impl EffectDefinition {
    pub(crate) fn validate(&self, models: &BTreeMap<u8, crate::PreparedEffectModel>) -> Result<()> {
        for (&member, definition) in &self.particles {
            definition
                .validate()
                .with_context(|| format!("effect particle {member}"))?;
            if let Some(binding) = definition.model {
                validate_model(models, binding, binding.slot)?;
            }
        }
        let mut previous = 0;
        for event in &self.events {
            ensure!(event.at >= previous, "effect schedule is out of order");
            previous = event.at;
            match &event.operation {
                EffectOperation::Unsupported { reason } => anyhow::bail!("{reason}"),
                EffectOperation::Sound { id, .. } => ensure!(
                    *id == 0 || self.sounds.contains_key(id),
                    "unbound effect sound"
                ),
                EffectOperation::Shake { .. } => {}
                EffectOperation::Spawn {
                    particle,
                    blend,
                    palette,
                    birth,
                } => {
                    let definition = self
                        .particles
                        .get(particle)
                        .context("unbound effect particle")?;
                    birth.validate()?;
                    ensure!(blend.is_none_or(|v| v <= 1), "invalid particle blend");
                    ensure!(
                        definition.model.is_none()
                            || (blend.is_none() && palette.is_none() && birth.palette.is_none()),
                        "model appearance override is unsupported"
                    );
                    let geometry = &definition.data.state.geometry;
                    ensure!(
                        birth
                            .size
                            .iter()
                            .chain(&birth.size_velocity)
                            .chain(&birth.size_acceleration)
                            .all(Option::is_none)
                            || matches!(
                                geometry,
                                ParticleGeometry::Size { .. } | ParticleGeometry::Spiral { .. }
                            ),
                        "particle has no size settings"
                    );
                    ensure!(
                        birth.segment_angle_step.is_none()
                            || matches!(
                                geometry,
                                ParticleGeometry::BillboardTrail { .. }
                                    | ParticleGeometry::Spiral { .. }
                            ),
                        "particle has no segment angle"
                    );
                    ensure!(
                        birth.phase.is_none()
                            || matches!(geometry, ParticleGeometry::Ribbon { .. }),
                        "particle has no ribbon phase"
                    );
                    if let Some(model) = birth.model {
                        validate_model(
                            models,
                            definition
                                .model
                                .context("model selection requires a model particle")?,
                            model,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}
fn validate_model(
    models: &BTreeMap<u8, crate::PreparedEffectModel>,
    binding: ModelBinding,
    slot: u8,
) -> Result<()> {
    let model = models
        .get(&slot)
        .with_context(|| format!("unprepared effect model {slot}"))?;
    if let Some(clip) = binding.animation {
        model.validate_clip(clip)?;
    }
    Ok(())
}

impl Active {
    /// Execute due events and return whether delayed work remains.
    pub(super) fn step(&mut self, effects: &mut Effects, cues: &mut Vec<Cue>) -> Result<bool> {
        while let Some(event) = self.definition.events.get(self.next) {
            if event.at > self.age {
                break;
            }
            let index = self.next;
            self.next += 1;
            self.execute(effects, index, cues)?;
        }
        let pending = self.next < self.definition.events.len();
        if pending {
            self.age += 1;
        }
        Ok(pending)
    }
    fn execute(&mut self, effects: &mut Effects, index: usize, cues: &mut Vec<Cue>) -> Result<()> {
        let effect = self.definition.clone();
        let operation = &effect.events[index].operation;
        match operation {
            EffectOperation::Unsupported { reason } => anyhow::bail!("{reason}"),
            EffectOperation::Shake {
                duration,
                amplitude,
            } => cues.push(Cue::Shake {
                duration: *duration,
                amplitude: *amplitude,
            }),
            EffectOperation::Sound { id, priority } => {
                if *id != 0 {
                    cues.push(Cue::Sound {
                        actor: self.actor,
                        sound: effect.sounds[id],
                        position: self.origin,
                        priority: *priority,
                    });
                }
            }
            EffectOperation::Spawn {
                particle,
                blend,
                palette,
                birth,
            } => {
                let definition = &effect.particles[particle];
                let handle = effects.spawn_particle(
                    definition.clone(),
                    self.actor,
                    self.target,
                    self.origin,
                    self.heading,
                )?;
                let Some(handle) = handle else {
                    return Ok(());
                };
                let result = (|| {
                    let particle = effects.particles.get_mut(&handle).unwrap();
                    if blend.is_some() {
                        particle.frame.state.blend = *blend;
                    }
                    if let Some(palette) = palette {
                        particle.frame.state.palettes[0] = *palette;
                    }
                    if definition.data.follow_origin {
                        particle.follow =
                            Some(self.follow.context("particle needs an origin attachment")?);
                    }
                    apply_birth(particle, birth, &mut effects.random);
                    if let Some(model) = birth.model {
                        effects.set_particle_model(handle, model)?;
                    }
                    effects.apply_effect_appearance(handle, self.scale, self.tint)
                })();
                if result.is_err() {
                    effects.particles.remove(&handle);
                }
                result?;
            }
        }
        Ok(())
    }
}

fn unit(random: &mut crate::Random) -> f32 {
    f32::from(random.next_u16()) / 65536.
}
fn sample(range: ValueRange, random: &mut crate::Random) -> f32 {
    range.sample(if range.min == range.max {
        0.
    } else {
        unit(random)
    })
}
fn polar(spread: PolarSpread, random: &mut crate::Random) -> [f32; 3] {
    let jitter = |extent: f32, random: &mut crate::Random| {
        sample(
            ValueRange {
                min: -extent,
                max: extent,
                step: 0.,
            },
            random,
        )
    };
    let radius = spread.radius + jitter(spread.radius_jitter, random);
    let mut angles = spread.angles;
    angles[2] += jitter(spread.angle_jitter, random);
    crate::geometry::polar_point(angles, radius)
}
fn floats<const N: usize>(
    output: &mut [f32; N],
    values: &[Option<ValueRange>; N],
    random: &mut crate::Random,
) {
    for (out, range) in output.iter_mut().zip(values) {
        if let Some(range) = range {
            *out = sample(*range, random);
        }
    }
}
fn words(output: &mut [i16; 4], values: &[Option<ValueRange>; 4], random: &mut crate::Random) {
    for (out, range) in output.iter_mut().zip(values) {
        if let Some(range) = range {
            *out = sample(*range, random)
                .round()
                .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
    }
}
fn byte(value: ValueRange, random: &mut crate::Random) -> u8 {
    sample(value, random).round().clamp(0., 255.) as u8
}
fn apply_birth(
    particle: &mut crate::particle::Particle,
    birth: &ParticleBirth,
    random: &mut crate::Random,
) {
    let state = &mut particle.frame.state;
    if let Some(spread) = birth.offset_spread {
        state.offset = polar(spread, random);
    }
    if let Some(spread) = birth.velocity_spread {
        state.velocity = polar(spread, random);
    }
    floats(&mut state.offset, &birth.offset, random);
    floats(&mut state.velocity, &birth.velocity, random);
    floats(&mut state.angles, &birth.angles, random);
    floats(&mut state.angular_velocity, &birth.angular_velocity, random);
    floats(&mut state.orbit, &birth.orbit, random);
    floats(&mut particle.orbit_velocity, &birth.orbit_velocity, random);
    if birth.relative_yaw {
        state.angles[1] += particle.frame.heading;
    }
    match &mut state.geometry {
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
        } => {
            let size_unit = if birth
                .size
                .iter()
                .chain(&birth.size_velocity)
                .chain(&birth.size_acceleration)
                .flatten()
                .any(|r| r.min != r.max)
            {
                unit(random)
            } else {
                0.
            };
            for (output, ranges) in [
                (value, &birth.size),
                (velocity, &birth.size_velocity),
                (acceleration, &birth.size_acceleration),
            ] {
                for (out, range) in output.iter_mut().zip(ranges) {
                    if let Some(range) = range {
                        *out = range.sample(size_unit);
                    }
                }
            }
        }
        _ => {}
    }
    match &mut state.geometry {
        ParticleGeometry::BillboardTrail {
            segment_angle_step, ..
        }
        | ParticleGeometry::Spiral {
            segment_angle_step, ..
        } => {
            if let Some(range) = birth.segment_angle_step {
                *segment_angle_step = sample(range, random);
            }
        }
        ParticleGeometry::Ribbon { phase, .. } => {
            if let Some(range) = birth.phase {
                *phase = byte(range, random);
            }
        }
        _ => {}
    }
    words(&mut state.colors[0], &birth.color, random);
    words(&mut state.colors[1], &birth.end_color, random);
    words(&mut state.uv, &birth.uv, random);
    for (output, ranges) in [
        (&mut state.brighten, &birth.brighten),
        (&mut particle.fade, &birth.fade),
    ] {
        for (out, range) in output.iter_mut().zip(ranges) {
            if let Some(range) = range {
                *out = byte(*range, random);
            }
        }
    }
    if let Some(range) = birth.brighten_until {
        let duration = sample(range, random).round() as u32;
        state.brighten_until = (duration != 0).then_some(duration);
    }
    if let Some(range) = birth.geometry_count {
        state.geometry_count = byte(range, random);
    }
    if let Some(range) = birth.palette {
        state.palettes[0] = byte(range, random);
    }
    state.cull_back |= birth.cull_back;
    particle.element_tint |= birth.element_tint;
}
