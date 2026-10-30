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
        let mut draws = 0_u32;
        let mut spawned = Vec::new();
        let mut model_births = Vec::new();
        let mut model_selections = Vec::new();
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
                EffectOperation::StageTint { duration, .. } => {
                    ensure!(*duration > 0, "zero stage tint duration")
                }
                EffectOperation::Notice { text } => {
                    ensure!(!text.is_empty(), "empty effect notice")
                }
                EffectOperation::SetMotion { spawn, motion } => {
                    let MotionChange {
                        offset,
                        size,
                        animation,
                        velocity,
                        velocity_add,
                        acceleration,
                        angular_velocity,
                        forward_motion,
                        velocity_scale,
                        size_velocity,
                        size_acceleration,
                    } = motion.as_ref();
                    if let Some(animation) = animation {
                        animation.validate()?;
                        ensure!(
                            model_selections.get(*spawn) == Some(&Some((true, animation.model))),
                            "delayed animation requires the selected program-owned model"
                        );
                        models
                            .get(&animation.model)
                            .context("unprepared animated model")?
                            .validate_clip(animation.clip)?;
                    }
                    let geometry = spawned
                        .get(*spawn)
                        .context("size edit has no earlier birth")?;
                    ensure!(
                        (size
                            .iter()
                            .chain(size_velocity)
                            .chain(size_acceleration)
                            .all(Option::is_none))
                            || matches!(
                                geometry,
                                ParticleGeometry::Size { .. } | ParticleGeometry::Spiral { .. }
                            ),
                        "size edit requires size geometry"
                    );
                    ensure!(
                        offset
                            .iter()
                            .chain(size)
                            .chain(velocity)
                            .chain(acceleration)
                            .chain(angular_velocity)
                            .chain(size_velocity)
                            .chain(size_acceleration)
                            .flatten()
                            .chain(velocity_scale.iter().flatten())
                            .chain(velocity_add.iter().flatten())
                            .chain(
                                forward_motion
                                    .iter()
                                    .flat_map(|motion| [&motion.velocity, &motion.acceleration])
                                    .flatten()
                            )
                            .all(|value| value.is_finite()),
                        "nonfinite particle motion edit"
                    );
                }
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
                    ensure!(
                        birth
                            .sums
                            .iter()
                            .all(|sum| !matches!(sum.field, BirthField::Size(_))
                                || matches!(
                                    definition.data.state.geometry,
                                    ParticleGeometry::Size { .. } | ParticleGeometry::Spiral { .. }
                                )),
                        "summed particle size requires size geometry"
                    );
                    ensure!(
                        birth.attachment_bone.is_none()
                            || (definition.model.is_none()
                                && birth.origin_bone.is_none()
                                && (definition.data.owner_bone.is_some()
                                    != definition.data.follow_bone.is_some())),
                        "bone override requires an existing particle attachment"
                    );
                    ensure!(
                        birth.origin_bone.is_none()
                            || (definition.model.is_none_or(|binding| {
                                binding.animation.is_none()
                                    && !binding.program_animation
                                    && binding.attachment.is_none()
                            }) && birth.model_parent.is_none()
                                && !definition.data.follow_origin
                                && !definition.data.follow_orientation
                                && (definition.data.follow_bone.is_none()
                                    || (definition.model.is_some()
                                        && definition.data.follow_bone == birth.origin_bone))
                                && definition.data.owner_bone.is_none()),
                        "bone birth requires a detached particle or a static model with matching bone follow"
                    );
                    ensure!(
                        definition
                            .model
                            .and_then(|binding| binding.attachment)
                            .map(|attachment| attachment.joint)
                            == birth.model_parent.map(|parent| parent.joint),
                        "model attachment has no matching parent binding"
                    );
                    if let Some(parent) = birth.model_parent {
                        ensure!(
                            model_births.get(parent.spawn) == Some(&true),
                            "model attachment requires an earlier model birth"
                        );
                    }
                    model_births.push(definition.model.is_some());
                    model_selections.push(definition.model.and_then(|binding| {
                        let choice = birth
                            .model
                            .unwrap_or(ValueRange::fixed(f32::from(binding.slot)));
                        (choice.min == choice.max)
                            .then_some((binding.program_animation, choice.min as u8))
                    }));
                    ensure!(
                        birth.effect_group.is_none()
                            || definition.data.state.effect_group.is_some(),
                        "effect group has no prepared binding"
                    );
                    draws = draws
                        .checked_add(birth.random_draws)
                        .context("too many particle random draws")?;
                    ensure!(
                        birth
                            .draw_references()
                            .all(|draw| draw >= 0 && draw < i64::from(draws)),
                        "unbound particle random draw"
                    );
                    ensure!(blend.is_none_or(|v| v <= 1), "invalid particle blend");
                    ensure!(
                        definition.model.is_none()
                            || (blend.is_none()
                                && palette.is_none()
                                && birth.palette.is_none()
                                && birth.alpha_palette.is_none()),
                        "model appearance override is unsupported"
                    );
                    let geometry = &definition.data.state.geometry;
                    spawned.push(geometry.clone());
                    ensure!(
                        birth.vertices.iter().flatten().all(Option::is_none)
                            || matches!(geometry, ParticleGeometry::Quad { .. }),
                        "vertex edits require a quad"
                    );
                    ensure!(
                        !birth.ribbon_depth || matches!(geometry, ParticleGeometry::Ribbon { .. }),
                        "depth modifier requires a ribbon"
                    );
                    ensure!(
                        birth.segment_offset.iter().all(Option::is_none)
                            || matches!(geometry, ParticleGeometry::Spiral { .. }),
                        "particle has no spiral segment offset"
                    );
                    ensure!(
                        birth.size.iter().all(Option::is_none)
                            || matches!(
                                geometry,
                                ParticleGeometry::Size { .. }
                                    | ParticleGeometry::Spiral { .. }
                                    | ParticleGeometry::BillboardTrail { .. }
                            )
                            || (matches!(geometry, ParticleGeometry::Ribbon { .. })
                                && birth.size[2].is_none()),
                        "particle has no size settings"
                    );
                    ensure!(
                        birth
                            .size_velocity
                            .iter()
                            .chain(&birth.size_acceleration)
                            .all(Option::is_none)
                            || matches!(
                                geometry,
                                ParticleGeometry::Size { .. } | ParticleGeometry::Spiral { .. }
                            )
                            || (matches!(geometry, ParticleGeometry::BillboardTrail { .. })
                                && birth.size_velocity[..2]
                                    .iter()
                                    .chain(&birth.size_acceleration)
                                    .all(Option::is_none)),
                        "particle has no size motion settings"
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
                    if let Some(animation) = birth.animation {
                        let binding = definition
                            .model
                            .context("model animation requires a model particle")?;
                        let held_default = definition.data.owner_bone.is_some()
                            && binding.animation == Some(animation.clip)
                            && binding.slot == animation.model
                            && binding.repeat
                            && animation.repeat
                            && binding.attachment.is_none()
                            && animation.rate == 0.;
                        ensure!(
                            binding.program_animation || held_default,
                            "unbound program-owned model animation"
                        );
                    }
                    if let Some(choice) = birth.model {
                        let binding = definition
                            .model
                            .context("model selection requires a model particle")?;
                        for slot in (choice.min as u8..=choice.max as u8)
                            .step_by(choice.step.max(1.) as usize)
                        {
                            validate_model(models, binding, slot)?;
                            if let Some(animation) = birth.animation {
                                ensure!(
                                    animation.model == slot,
                                    "model animation selects a different model"
                                );
                                models[&slot].validate_clip(animation.clip)?;
                            }
                        }
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
    models
        .get(&slot)
        .with_context(|| format!("unprepared effect model {slot}"))?;
    if let Some(clip) = binding.animation {
        let source = models
            .get(&binding.slot)
            .context("unprepared declaration model")?;
        source.validate_clip(clip)?;
    }
    Ok(())
}

impl Active {
    /// Execute due events and return whether delayed work remains.
    pub(super) fn step(
        &mut self,
        effects: &mut Effects,
        frame: &BattleFrame,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        while let Some(event) = self.definition.events.get(self.next) {
            if event.at > self.age {
                break;
            }
            let index = self.next;
            self.next += 1;
            self.execute(effects, frame, index, cues)?;
        }
        let pending = self.next < self.definition.events.len();
        if pending {
            self.age += 1;
        }
        Ok(pending)
    }
    fn execute(
        &mut self,
        effects: &mut Effects,
        frame: &BattleFrame,
        index: usize,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let effect = self.definition.clone();
        let operation = &effect.events[index].operation;
        match operation {
            EffectOperation::Unsupported { reason } => anyhow::bail!("{reason}"),
            EffectOperation::Notice { text } => cues.push(Cue::EffectNotice {
                actor: self.actor,
                text: text.clone(),
                duration: 90,
            }),
            EffectOperation::Shake {
                duration,
                amplitude,
            } => cues.push(Cue::Shake {
                duration: *duration,
                amplitude: *amplitude,
            }),
            EffectOperation::StageTint { color, duration } => {
                cues.push(Cue::StageTint(crate::StageTint {
                    color: *color,
                    duration: *duration,
                }));
            }
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
            EffectOperation::SetMotion { spawn, motion } => {
                let MotionChange {
                    offset,
                    size,
                    animation,
                    velocity,
                    velocity_add,
                    acceleration,
                    angular_velocity,
                    forward_motion,
                    velocity_scale,
                    size_velocity,
                    size_acceleration,
                } = motion.as_ref();
                if let Some(handle) = self.spawned[*spawn]
                    && let Some(particle) = effects.particles.get_mut(&handle)
                {
                    assign(&mut particle.frame.state.offset, offset);
                    if let Some(animation) = animation {
                        particle
                            .model
                            .as_mut()
                            .context("delayed animation has no model")?
                            .play(*animation)?;
                    }
                    assign(&mut particle.frame.state.acceleration, acceleration);
                    assign(&mut particle.frame.state.angular_velocity, angular_velocity);
                    if let Some(motion) = forward_motion {
                        if let Some(value) = motion.velocity {
                            particle.frame.state.velocity =
                                crate::geometry::rotate([0., 0., value], particle.frame.heading);
                        }
                        if let Some(value) = motion.acceleration {
                            particle.frame.state.acceleration =
                                crate::geometry::rotate([0., 0., value], particle.frame.heading);
                        }
                    }
                    for ((current, assigned), scale) in particle
                        .frame
                        .state
                        .velocity
                        .iter_mut()
                        .zip(velocity)
                        .zip(velocity_scale.unwrap_or([1.; 3]))
                    {
                        if let Some(value) = assigned {
                            *current = *value;
                        }
                        *current *= scale;
                    }
                    if let Some(add) = velocity_add {
                        for (current, add) in particle.frame.state.velocity.iter_mut().zip(add) {
                            *current += add;
                        }
                    }
                    let (current_size, current_velocity, current_acceleration) =
                        match &mut particle.frame.state.geometry {
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
                            } => (value, velocity, acceleration),
                            _ => return Ok(()),
                        };
                    assign(current_size, size);
                    assign(current_velocity, size_velocity);
                    assign(current_acceleration, size_acceleration);
                }
            }
            EffectOperation::Spawn {
                particle,
                blend,
                palette,
                birth,
            } => {
                let definition = &effect.particles[particle];
                let bone_anchor = |bone| match self.follow {
                    Some(EffectFollow::Weapon {
                        actor, resource, ..
                    }) => EffectFollow::Weapon {
                        actor,
                        resource,
                        bone: Some(bone),
                    },
                    _ => EffectFollow::Bone {
                        actor: self.actor,
                        bone,
                    },
                };
                let bone_follow = definition
                    .data
                    .follow_bone
                    .map(|bone| bone_anchor(birth.attachment_bone.unwrap_or(bone)));
                let origin =
                    if let Some(follow) = birth.origin_bone.map(bone_anchor).or(bone_follow) {
                        (if bone_follow.is_some() {
                            follow.particle_position(frame)
                        } else {
                            follow.position(frame)
                        })
                        .context("particle attachment has no owner bone pose")?
                    } else {
                        self.origin
                    };
                #[cfg(feature = "test-support")]
                let recorded = effects.recorded_births.take(self, *particle, birth)?;
                #[cfg(feature = "test-support")]
                let first_draw = self.samples.len();
                for _ in 0..birth.random_draws {
                    self.samples.push(unit(&mut effects.random));
                }
                #[cfg(feature = "test-support")]
                if let Some(recorded) = &recorded {
                    self.samples[first_draw..].copy_from_slice(&recorded.samples);
                }
                let handle = effects.spawn_particle(
                    definition.clone(),
                    self.actor,
                    self.target,
                    origin,
                    self.heading,
                )?;
                self.spawned.push(Some(handle));
                #[cfg(feature = "test-support")]
                if let Some(recorded) = recorded {
                    if let Some(ordinal) = recorded.model_ordinal {
                        effects.recorded_births.identities.insert(handle, ordinal);
                    }
                }
                let result = (|| {
                    let particle = effects.particles.get_mut(&handle).unwrap();
                    if particle.frame.owner_bone.is_some()
                        && let Some(bone) = birth.attachment_bone
                    {
                        particle.frame.owner_bone = Some(bone);
                    }
                    if blend.is_some() {
                        particle.frame.state.blend = *blend;
                    }
                    if let Some(palette) = palette {
                        particle.frame.state.palettes[0] = *palette;
                    }
                    if bone_follow.is_some() {
                        particle.follow = bone_follow;
                    } else if definition.data.follow_origin {
                        particle.follow = self.follow;
                    }
                    apply_birth(particle, birth, &mut effects.random, &self.samples);
                    let model = particle.model.is_some() || birth.model.is_some();
                    if definition.data.follow_orientation
                        && let Some(direction) = self.direction
                        && (model || direction[0] != 0. || direction[2] != 0.)
                        && crate::distance::length(direction) > 0.1
                    {
                        let [pitch, yaw] = if model {
                            crate::particle::model_follow_angles(direction)
                        } else {
                            let [x, y, z] = crate::distance::normalize(direction);
                            [
                                -y.clamp(-1., 1.).asin().to_degrees(),
                                x.atan2(z).to_degrees(),
                            ]
                        };
                        particle.frame.state.angles[0] = pitch;
                        particle.frame.state.angles[1] = yaw;
                    }
                    if let Some(choice) = birth.model {
                        let slot = BirthSamples {
                            random: &mut effects.random,
                            shared: &self.samples,
                        }
                        .sample(choice) as u8;
                        effects.set_particle_model(handle, slot, birth.animation)?;
                    }
                    if let Some(parent) = birth.model_parent {
                        let parent_handle = self.spawned[parent.spawn];
                        let Some(parent_handle) = parent_handle else {
                            effects.particles.remove(&handle);
                            *self.spawned.last_mut().unwrap() = None;
                            return Ok(());
                        };
                        effects.attach_particle_model(handle, parent_handle, parent.joint)?;
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

fn assign<const N: usize>(output: &mut [f32; N], values: &[Option<f32>; N]) {
    for (output, value) in output.iter_mut().zip(values) {
        if let Some(value) = value {
            *output = *value;
        }
    }
}

fn unit(random: &mut crate::Random) -> f32 {
    f32::from(random.next_u16()) / 65536.
}
struct BirthSamples<'a> {
    random: &'a mut crate::Random,
    shared: &'a [f32],
}
impl BirthSamples<'_> {
    fn sample(&mut self, range: ValueRange) -> f32 {
        let value = match range.draw {
            Some(index) => self.shared[index as usize],
            None if range.min == range.max => 0.,
            None => unit(self.random),
        };
        range.sample(value)
    }
}
fn polar(spread: PolarSpread, samples: &mut BirthSamples<'_>) -> [f32; 3] {
    let mut jitter = |extent: f32, draw| {
        samples.sample(ValueRange {
            min: -extent,
            max: extent,
            step: 0.,
            draw,
            reverse: false,
        })
    };
    let radius = spread.radius + jitter(spread.radius_jitter, spread.radius_draw);
    let mut angles = spread.angles;
    angles[2] += jitter(spread.angle_jitter, spread.angle_draw);
    crate::geometry::polar_point(angles, radius)
}
fn floats<const N: usize>(
    output: &mut [f32; N],
    values: &[Option<ValueRange>; N],
    samples: &mut BirthSamples<'_>,
) {
    for (out, range) in output.iter_mut().zip(values) {
        if let Some(range) = range {
            *out = samples.sample(*range);
        }
    }
}
fn words(output: &mut [i16; 4], values: &[Option<ValueRange>; 4], samples: &mut BirthSamples<'_>) {
    for (out, range) in output.iter_mut().zip(values) {
        if let Some(range) = range {
            *out = samples
                .sample(*range)
                .round()
                .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
    }
}
fn byte(value: ValueRange, samples: &mut BirthSamples<'_>) -> u8 {
    samples.sample(value).round().clamp(0., 255.) as u8
}
fn apply_birth(
    particle: &mut crate::particle::Particle,
    birth: &ParticleBirth,
    random: &mut crate::Random,
    shared: &[f32],
) {
    let mut samples = BirthSamples { random, shared };
    let state = &mut particle.frame.state;
    if let Some(spread) = birth.offset_spread {
        state.offset = polar(spread, &mut samples);
    }
    if let Some(spread) = birth.velocity_spread {
        state.velocity = polar(spread, &mut samples);
    }
    floats(&mut state.offset, &birth.offset, &mut samples);
    floats(&mut state.velocity, &birth.velocity, &mut samples);
    floats(&mut state.acceleration, &birth.acceleration, &mut samples);
    floats(&mut state.angles, &birth.angles, &mut samples);
    floats(
        &mut state.angular_velocity,
        &birth.angular_velocity,
        &mut samples,
    );
    for (offset, value) in state.segment_offset.iter_mut().zip(birth.segment_offset) {
        if let Some(value) = value {
            *offset = Some(samples.sample(value));
        }
    }
    floats(&mut state.orbit, &birth.orbit, &mut samples);
    floats(
        &mut particle.orbit_velocity,
        &birth.orbit_velocity,
        &mut samples,
    );
    state.angles[1] += birth.yaw_heading * particle.frame.heading;
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
            floats(value, &birth.size, &mut samples);
            floats(velocity, &birth.size_velocity, &mut samples);
            floats(acceleration, &birth.size_acceleration, &mut samples);
        }
        ParticleGeometry::BillboardTrail {
            size,
            radius,
            radius_velocity,
            ..
        } => {
            floats(size, &[birth.size[0], birth.size[1]], &mut samples);
            if let Some(range) = birth.size[2] {
                *radius = samples.sample(range);
            }
            if let Some(range) = birth.size_velocity[2] {
                *radius_velocity = samples.sample(range);
            }
        }
        ParticleGeometry::Ribbon { length, width, .. } => {
            if let Some(range) = birth.size[0] {
                *length = samples.sample(range);
            }
            if let Some(range) = birth.size[1] {
                *width = samples.sample(range);
            }
        }
        ParticleGeometry::Quad { vertices, .. } => {
            for (vertex, settings) in vertices.iter_mut().zip(&birth.vertices) {
                floats(vertex, settings, &mut samples);
            }
        }
    }
    match &mut state.geometry {
        ParticleGeometry::BillboardTrail {
            segment_angle_step, ..
        }
        | ParticleGeometry::Spiral {
            segment_angle_step, ..
        } => {
            if let Some(range) = birth.segment_angle_step {
                *segment_angle_step = samples.sample(range);
            }
        }
        ParticleGeometry::Ribbon { phase, .. } => {
            if let Some(range) = birth.phase {
                *phase = byte(range, &mut samples);
            }
        }
        _ => {}
    }
    words(&mut state.colors[0], &birth.color, &mut samples);
    words(&mut state.colors[1], &birth.end_color, &mut samples);
    words(&mut state.uv, &birth.uv, &mut samples);
    for (output, ranges) in [
        (&mut state.brighten, &birth.brighten),
        (&mut particle.fade, &birth.fade),
    ] {
        for (out, range) in output.iter_mut().zip(ranges) {
            if let Some(range) = range {
                *out = byte(*range, &mut samples);
            }
        }
    }
    if let Some(range) = birth.brighten_until {
        let duration = samples.sample(range).round() as u32;
        state.brighten_until = (duration != 0).then_some(duration);
    }
    if birth.lifetime.is_some() || birth.fade_from.is_some() {
        let data = &mut std::sync::Arc::make_mut(&mut particle.definition).data;
        if let Some(range) = birth.lifetime {
            let duration = samples.sample(range).round() as u32;
            data.lifetime = (duration != 0).then_some(duration);
        }
        if let Some(range) = birth.fade_from {
            data.fade_from = u32::from(byte(range, &mut samples));
        }
    }
    if let Some(range) = birth.geometry_count {
        state.geometry_count = byte(range, &mut samples);
    }
    for (palette, range) in state
        .palettes
        .iter_mut()
        .zip([birth.palette, birth.alpha_palette])
    {
        if let Some(range) = range {
            *palette = byte(range, &mut samples);
        }
    }
    if let Some(group) = birth.effect_group {
        state.effect_group = Some(group);
    }
    for sum in &birth.sums {
        let mut terms = sum.terms.iter();
        let first = samples.sample(*terms.next().unwrap());
        let value = terms.fold(first, |value, term| value + samples.sample(*term));
        match sum.field {
            BirthField::Offset(axis) => state.offset[usize::from(axis)] = value,
            BirthField::Velocity(axis) => state.velocity[usize::from(axis)] = value,
            BirthField::Uv(axis) => state.uv[usize::from(axis)] = value.round() as i16,
            BirthField::Size(axis) => match &mut state.geometry {
                ParticleGeometry::Size { value: size, .. }
                | ParticleGeometry::Spiral { value: size, .. } => size[usize::from(axis)] = value,
                _ => unreachable!("validated particle sum geometry"),
            },
        }
    }
    state.cull_back |= birth.cull_back;
    state.ribbon_depth |= birth.ribbon_depth;
    particle.element_tint |= birth.element_tint;
}
