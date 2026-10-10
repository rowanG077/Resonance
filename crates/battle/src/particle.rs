//! Particle motion and lifetime.
use crate::{ActorId, BattleFrame, Effects, geometry::rotate};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{UvAnimation, UvChange};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParticleId(pub(crate) i32);

pub use resonance_content::battle_effect::{ParticleGeometry, ParticleState, ParticleTemplate};

#[derive(Debug, Clone)]
pub struct ParticleDefinition {
    pub resource: u32,
    pub member: u16,
    pub model: Option<resonance_content::battle_effect::ModelBinding>,
    pub data: ParticleTemplate,
}
impl ParticleDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        self.data.validate()?;
        ensure!(
            self.model.is_some()
                || (!self.data.follow_orientation && self.data.model_elevation.is_none()),
            "particle model placement requires a model binding"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParticleFrame {
    pub id: ParticleId,
    pub owner: ActorId,
    pub draw_after: Option<ActorId>,
    pub resource: u32,
    pub member: u16,
    pub origin: [f32; 3],
    pub heading: f32,
    pub state: ParticleState,
    pub model: Option<crate::EffectModelFrame>,
}

pub(crate) struct Particle {
    pub definition: Arc<ParticleDefinition>,
    pub frame: ParticleFrame,
    pub follow: Option<crate::EffectFollow>,

    pub model: Option<crate::PreparedEffectModel>,
    pub model_secondary: Vec<resonance_content::secondary_motion::Simulation>,
    pub(crate) orbit_velocity: [f32; 3],
    pub(crate) element_tint: bool,
    /// Completed motion updates; zero also means the particle is not yet visible.
    pub(crate) age: u32,
    pub(crate) fade: [u8; 4],
    uv_frame: usize,
    uv_elapsed: u32,
    uv_scroll: [i32; 2],
}

impl Particle {
    fn step(&mut self, followed_origin: Option<[f32; 3]>) -> Result<()> {
        let fresh = self.age == 0;
        if fresh {
            let state = &mut self.frame.state;
            state.offset = rotate(state.offset, self.frame.heading);
            state.velocity = rotate(state.velocity, self.frame.heading);
            state.acceleration = rotate(state.acceleration, self.frame.heading);
            if !self.definition.data.gradient {
                state.colors[1] = state.colors[0];
            }
        }
        self.advance_uv(fresh)?;
        let state = &mut self.frame.state;
        add(&mut state.offset, state.velocity);
        add(&mut state.velocity, state.acceleration);
        if !matches!(state.geometry, ParticleGeometry::BillboardTrail { .. }) {
            add(&mut state.acceleration, self.definition.data.jerk);
        }
        let previous_origin = self.frame.origin;
        if let Some(origin) = followed_origin {
            self.frame.origin = origin;
        }
        if !self.definition.data.follow_origin
            && let Some(restitution) = self.definition.data.ground_restitution
        {
            let y = self.frame.origin[1] + state.offset[1];
            if y < 0. {
                state.offset[1] -= y;
                state.velocity[1] *= -restitution;
            }
        }
        if self.definition.data.follow_orientation {
            let delta: [f32; 3] =
                std::array::from_fn(|i| self.frame.origin[i] - previous_origin[i]);
            if delta != [0.; 3] {
                let [x, y, z] = crate::distance::normalize(delta);
                state.angles[0] = y.clamp(-1., 1.).acos().to_degrees() + 180.;
                state.angles[1] = x.atan2(z).to_degrees();
            }
        } else {
            add(&mut state.angles, state.angular_velocity);
            if self.definition.data.linear_orbit
                || matches!(state.geometry, ParticleGeometry::Quad { .. })
            {
                add(&mut state.orbit, self.orbit_velocity);
            } else {
                state.orbit[0] += state.orbit[1];
            }
        }
        let accelerates = self
            .definition
            .data
            .geometry_acceleration_until
            .is_none_or(|until| self.age < until);
        match &mut state.geometry {
            ParticleGeometry::BillboardTrail {
                radius,
                radius_velocity,
                ..
            } => {
                *radius += *radius_velocity;
            }
            ParticleGeometry::Ribbon { .. } => {}
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
                add(value, *velocity);
                if accelerates {
                    add(velocity, *acceleration);
                }
            }
            ParticleGeometry::Quad { vertices, velocity } => {
                if accelerates {
                    for (value, velocity) in vertices.iter_mut().zip(velocity) {
                        add(value, *velocity);
                    }
                }
            }
        }
        for color in &mut state.colors {
            for (i, component) in color.iter_mut().enumerate() {
                if state.brighten_until.is_some_and(|until| self.age < until) {
                    *component =
                        (i32::from(*component) + i32::from(state.brighten[i])).clamp(0, 255) as i16;
                } else if self.age >= self.definition.data.fade_from {
                    *component =
                        (i32::from(*component) - i32::from(self.fade[i])).clamp(0, 255) as i16;
                }
            }
        }
        if let ParticleGeometry::Ribbon {
            phase,
            phase_period,
            ..
        } = &mut state.geometry
            && *phase_period != 0
            && self
                .age
                .is_multiple_of(u32::from(phase_period.unsigned_abs()))
        {
            *phase = phase.wrapping_add(1);
        }
        self.age = self.age.saturating_add(1);
        ensure!(state.finite(), "particle motion overflow");
        if let Some(model) = &mut self.model {
            model.advance()?;
            self.frame.model = Some(model.sample(
                &self.frame,
                &mut self.model_secondary,
                self.definition.data.model_elevation,
            )?);
        }
        Ok(())
    }
}

impl Particle {
    fn advance_uv(&mut self, fresh: bool) -> Result<()> {
        let Some(animation) = &self.definition.data.uv_animation else {
            return Ok(());
        };
        let state = &mut self.frame.state;
        if fresh {
            if let UvAnimation::Frames { frames, .. } = animation {
                apply_uv(state, frames[0].change);
            }
            return Ok(());
        }
        self.uv_elapsed = self.uv_elapsed.saturating_add(1);
        match animation {
            UvAnimation::Frames { frames, loop_start } => {
                if self.uv_elapsed >= frames[self.uv_frame].duration {
                    let next = if self.uv_frame + 1 < frames.len() {
                        self.uv_frame + 1
                    } else if let Some(start) = loop_start {
                        *start
                    } else {
                        return Ok(());
                    };
                    self.uv_frame = next;
                    self.uv_elapsed = 0;
                    apply_uv(state, frames[next].change);
                }
            }
            UvAnimation::Scroll {
                origin,
                step,
                interval,
            } => {
                if self.uv_elapsed >= *interval {
                    self.uv_elapsed = 0;
                    for axis in 0..2 {
                        let extent = i32::from(state.uv[axis + 2]);
                        ensure!(extent > 0, "nonpositive particle UV scroll extent");
                        self.uv_scroll[axis] =
                            (self.uv_scroll[axis] + i32::from(step[axis])).rem_euclid(extent);
                        state.uv[axis] = (i32::from(origin[axis]) + self.uv_scroll[axis])
                            .try_into()
                            .context("particle UV scroll exceeds coordinate range")?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn apply_uv(state: &mut ParticleState, change: UvChange) {
    match change {
        UvChange::Rectangle { rect } => state.uv = rect,
        UvChange::Palette { index } => state.palettes[0] = index,
    }
}

fn add(value: &mut [f32; 3], delta: [f32; 3]) {
    for i in 0..3 {
        value[i] += delta[i];
    }
}

pub(crate) fn apply_scale(state: &mut ParticleState, scale: f32) -> Result<()> {
    let multiply = |v: &mut [f32; 3]| v.iter_mut().for_each(|x| *x *= scale);
    multiply(&mut state.offset);
    match &mut state.geometry {
        ParticleGeometry::BillboardTrail {
            size,
            radius,
            radius_velocity,
            segment_size_step,
            ..
        } => {
            size.iter_mut().for_each(|v| *v *= scale);
            *radius *= scale;
            *radius_velocity *= scale;
            multiply(segment_size_step);
        }
        ParticleGeometry::Ribbon {
            length,
            width,
            jitter,
            ..
        } => {
            *length *= scale;
            *width *= scale;
            *jitter *= scale;
        }
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
            multiply(value);
            multiply(velocity);
            multiply(acceleration);
        }
        ParticleGeometry::Quad { vertices, velocity } => {
            for vector in vertices.iter_mut().chain(velocity.iter_mut()) {
                multiply(vector);
            }
            multiply(&mut state.orbit);
        }
    }
    ensure!(state.finite(), "particle scale overflow");
    Ok(())
}

impl Effects {
    pub(crate) fn spawn_particle(
        &mut self,
        definition: Arc<ParticleDefinition>,
        owner: ActorId,
        target: ActorId,
        origin: [f32; 3],
        heading: f32,
    ) -> Result<Option<ParticleId>> {
        if self.particles.len() >= crate::MAX_PARTICLES {
            return Ok(None);
        }
        let id = ParticleId(self.next_particle);
        self.next_particle = self
            .next_particle
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("battle particle handle exhausted"))?;
        let mut state = definition.data.state.clone();
        if definition.model.is_some() {
            state.angles[1] += heading;
        }
        let model = definition
            .model
            .map(|binding| {
                let mut model = self
                    .banks
                    .get(&definition.resource)
                    .and_then(|bank| bank.models.get(&binding.slot))
                    .ok_or_else(|| anyhow::anyhow!("unprepared particle model"))?
                    .clone();
                if let Some(animation) = binding.animation {
                    model.play_repeating(animation, binding.repeat)?;
                }
                Ok::<_, anyhow::Error>(model)
            })
            .transpose()?;
        let fade = definition.data.fade;
        let frame = ParticleFrame {
            id,
            owner,
            draw_after: definition.data.draw_after_target.then_some(target),
            resource: definition.resource,
            member: definition.member,
            origin,
            heading,
            state,
            model: None,
        };
        self.particles.insert(
            id,
            Particle {
                orbit_velocity: definition.data.orbit_velocity,
                element_tint: definition.data.element_tint,
                model,
                model_secondary: Vec::new(),

                definition,
                frame,
                follow: None,
                age: 0,
                fade,
                uv_frame: 0,
                uv_elapsed: 0,
                uv_scroll: [0; 2],
            },
        );
        Ok(Some(id))
    }

    pub(crate) fn set_particle_model(&mut self, handle: ParticleId, model: u8) -> Result<()> {
        let particle = self
            .particles
            .get(&handle)
            .context("stale particle handle")?;
        let binding = particle
            .definition
            .model
            .context("particle has no model binding")?;
        let bank = particle.frame.resource;
        let mut model_state = self
            .banks
            .get(&bank)
            .and_then(|bank| bank.models.get(&model))
            .context("unprepared particle model")?
            .clone();
        if let Some(clip) = binding.animation {
            model_state.play_repeating(clip, binding.repeat)?;
        }
        let particle = self.particles.get_mut(&handle).unwrap();
        particle.model = Some(model_state);
        particle.model_secondary.clear();
        Ok(())
    }

    pub(crate) fn apply_effect_appearance(
        &mut self,
        handle: ParticleId,
        scale: f32,
        tint: crate::effect::EffectTint,
    ) -> Result<()> {
        let particle = self
            .particles
            .get_mut(&handle)
            .context("stale particle handle")?;
        let state = &mut particle.frame.state;
        apply_scale(state, scale)?;
        if tint.enabled && particle.element_tint {
            state.palettes[0] = tint.palette;
            state.colors[1][..3].copy_from_slice(&tint.rgb.map(i16::from));
        }
        Ok(())
    }

    pub(crate) fn advance_particles(&mut self, frame: &BattleFrame) -> Result<()> {
        let ids: Vec<_> = self.particles.keys().copied().collect();
        for id in ids {
            let origin = self.particles[&id]
                .follow
                .and_then(|follow| follow.position(frame));
            let particle = self.particles.get_mut(&id).unwrap();
            if particle
                .definition
                .data
                .lifetime
                .is_some_and(|duration| particle.age >= duration)
            {
                self.particles.remove(&id);
            } else if let Err(error) = particle.step(origin) {
                self.particles.remove(&id);
                self.diagnostics.report("battle particle", error)?;
            }
        }
        Ok(())
    }

    pub fn frames(&self) -> Vec<ParticleFrame> {
        self.particles
            .values()
            .filter(|p| p.age > 0)
            .map(|p| p.frame.clone())
            .collect()
    }
}

#[cfg(test)]
mod uv_tests;

#[cfg(test)]
mod model_follow_tests;
