//! Common particle motion and lifetime. Original record decoding stays in import.
use crate::{ActionId, ActorId, Battle, Cue, geometry::rotate};
use anyhow::{Result, ensure};
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
        self.data.validate()
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
    pub age: i16,
    pub state: ParticleState,
    pub model: Option<crate::EffectModelFrame>,
}

pub(crate) struct Particle {
    pub definition: Arc<ParticleDefinition>,
    pub action: Option<ActionId>,
    pub frame: ParticleFrame,
    pub follow: Option<crate::effect::Follow>,
    pub scene: Option<ActionId>,
    pub model: Option<u8>,
    pub own_model: Option<crate::PreparedEffectModel>,
    pub(crate) orbit_velocity: [f32; 3],
    age: i16,
    fade_age: i16,
    uv_row: usize,
    uv_age: u8,
    uv_scroll: [i16; 2],
    pub(crate) initialized: bool,
    retiring: bool,
}

impl Particle {
    fn step(&mut self, hold_uv: bool) -> Result<()> {
        let fresh = !self.initialized;
        if fresh {
            let state = &mut self.frame.state;
            state.offset = rotate(state.offset, self.frame.heading);
            state.velocity = rotate(state.velocity, self.frame.heading);
            state.acceleration = rotate(state.acceleration, self.frame.heading);
            if !self.definition.data.gradient {
                state.colors[1] = state.colors[0];
            }
            if let Some(row) = self.definition.data.uv_track.first()
                && row.timing & 128 == 0
            {
                apply_uv(state, row);
            }
            self.initialized = true;
        }
        let state = &mut self.frame.state;
        add(&mut state.offset, state.velocity);
        add(&mut state.velocity, state.acceleration);
        if !matches!(state.geometry, ParticleGeometry::BillboardTrail { .. }) {
            add(&mut state.acceleration, self.definition.data.jerk);
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
        add(&mut state.angles, state.angular_velocity);
        if self.definition.data.linear_orbit
            || matches!(state.geometry, ParticleGeometry::Quad { .. })
        {
            add(&mut state.orbit, self.orbit_velocity);
        } else {
            state.orbit[0] += state.orbit[1];
        }
        let accelerates = self.definition.data.geometry_acceleration_until == 0
            || self.age < self.definition.data.geometry_acceleration_until;
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
                if self.fade_age < i16::from(state.brighten_until) {
                    *component = component
                        .wrapping_add(i16::from(state.brighten[i]))
                        .min(255);
                } else if self.fade_age >= i16::from(self.definition.data.fade_from) {
                    *component = component
                        .wrapping_sub(i16::from(self.definition.data.fade[i]))
                        .max(0);
                }
            }
        }
        let track = &self.definition.data.uv_track;
        if !track.is_empty() {
            let row = &track[self.uv_row];
            if row.timing & 128 != 0 {
                if self.uv_age as i8 >= (row.timing & 127) as i8 {
                    self.uv_age = 0;
                    for i in 0..2 {
                        self.uv_scroll[i] = self.uv_scroll[i].wrapping_add(row.values[i + 2]);
                        if self.uv_scroll[i] >= state.uv[i + 2] {
                            self.uv_scroll[i] = self.uv_scroll[i].wrapping_sub(state.uv[i + 2]);
                        }
                        state.uv[i] = row.values[i].wrapping_add(self.uv_scroll[i]);
                    }
                }
            } else if self.uv_age as i8 >= row.timing as i8 {
                self.uv_age = 0;
                self.uv_row += 1;
                if track[self.uv_row].timing == 255 {
                    self.uv_row = usize::from(track[self.uv_row].control);
                }
                apply_uv(state, &track[self.uv_row]);
            }
        }
        if let ParticleGeometry::Ribbon {
            phase,
            phase_period,
            ..
        } = &mut state.geometry
            && *phase_period != 0
            && i32::from(self.age) % i32::from(*phase_period) == 0
        {
            *phase = phase.wrapping_add(1);
        }
        self.frame.age = self.age;
        self.retiring = !fresh
            && self.definition.data.lifetime != 0
            && self.age >= self.definition.data.lifetime;
        self.age = self.age.wrapping_add(1);
        self.fade_age = self.fade_age.wrapping_add(1);
        if !hold_uv {
            self.uv_age = self.uv_age.wrapping_add(1);
        }
        ensure!(state.finite(), "particle motion overflow");
        Ok(())
    }
}

fn apply_uv(state: &mut ParticleState, row: &resonance_content::battle_effect::UvRecord) {
    if row.values[0] == -32000 {
        state.palettes[0] = row.values[1] as u8;
    } else {
        state.uv = row.values;
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
        } => {
            multiply(value);
            multiply(velocity);
            multiply(acceleration);
        }
        ParticleGeometry::Quad { vertices, .. } => {
            // 418B4 scales only the first three vertices, then the orbit vector.
            // The fourth vertex and quad velocities retain their input values.
            for vertex in &mut vertices[..3] {
                multiply(vertex);
            }
            multiply(&mut state.orbit);
        }
    }
    ensure!(state.finite(), "particle scale overflow");
    Ok(())
}

impl Battle {
    pub(crate) fn spawn_particle(
        &mut self,
        definition: Arc<ParticleDefinition>,
        action: Option<ActionId>,
        owner: ActorId,
        target: ActorId,
        origin: [f32; 3],
        heading: f32,
    ) -> Result<Option<ParticleId>> {
        if !self.object_available() {
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
        let own_model = definition
            .model
            .filter(|model| !model.shared)
            .map(|binding| {
                let mut model = self
                    .prepared
                    .effects
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
        let frame = ParticleFrame {
            id,
            owner,
            draw_after: definition.data.draw_after_target.then_some(target),
            resource: definition.resource,
            member: definition.member,
            origin,
            heading,
            age: 0,
            state,
            model: None,
        };
        self.particles.insert(
            id,
            Particle {
                orbit_velocity: definition.data.orbit_velocity,
                model: definition.model.map(|binding| binding.slot),
                own_model,
                scene: None,
                definition,
                action,
                frame,
                follow: None,
                age: 0,
                fade_age: 0,
                uv_row: 0,
                uv_age: 0,
                uv_scroll: [0; 2],
                initialized: false,
                retiring: false,
            },
        );
        Ok(Some(id))
    }

    pub(crate) fn particle(
        &mut self,
        handle: i32,
        action: ActionId,
    ) -> Result<&mut ParticleState, String> {
        Ok(&mut self.owned_particle(handle, action)?.frame.state)
    }

    pub(crate) fn apply_effect_appearance(
        &mut self,
        handle: i32,
        action: ActionId,
        scale: f32,
        tint: crate::effect::EffectTint,
    ) -> Result<(), String> {
        let particle = self.owned_particle(handle, action)?;
        let state = &mut particle.frame.state;
        apply_scale(state, scale).map_err(|e| e.to_string())?;
        if tint.enabled && particle.definition.data.element_tint {
            state.palettes[0] = tint.palette;
            state.colors[1][..3].copy_from_slice(&tint.rgb.map(i16::from));
        }
        Ok(())
    }

    pub(crate) fn owned_particle(
        &mut self,
        handle: i32,
        action: ActionId,
    ) -> Result<&mut Particle, String> {
        let particle = self
            .particles
            .get_mut(&ParticleId(handle))
            .ok_or("stale particle handle")?;
        if particle.action != Some(action) {
            return Err("particle belongs to another effect".into());
        }
        Ok(particle)
    }

    pub(crate) fn advance_particles(&mut self, late: bool, cues: &mut Vec<Cue>) -> Result<()> {
        // 413B8/123C8 append particles; 40210/12470 prepend effect programs.
        // Each group's effects run first, then particles in creation order,
        // including particles appended by that same group's effect callbacks.
        let ids: Vec<_> = self
            .particles
            .iter()
            .filter(|(_, p)| p.definition.data.late == late)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let origin = self.particles[&id]
                .follow
                .and_then(|follow| self.follow_position(follow));
            let particle = self.particles.get_mut(&id).unwrap();
            if particle.retiring {
                self.particles.remove(&id);
                cues.push(Cue::ParticleExpired { particle: id });
            } else {
                if let Some(origin) = origin {
                    particle.frame.origin = origin;
                }
                if !particle.initialized {
                    cues.push(Cue::ParticleStarted {
                        particle: id,
                        action: particle.action,
                    });
                }
                let hold_uv = self.actors[particle.frame.owner.index()]
                    .reaction
                    .unflinching;
                if let Err(error) = particle.step(hold_uv) {
                    self.diagnostics.report("battle particle", error)?;
                    self.diagnostic = true;
                    self.particles.remove(&id);
                    cues.push(Cue::ParticleExpired { particle: id });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn particle_frames(&self) -> Vec<ParticleFrame> {
        self.particles
            .values()
            .filter(|p| p.initialized)
            .map(|p| p.frame.clone())
            .collect()
    }
}

#[cfg(test)]
mod attachment_tests;
#[cfg(test)]
mod uv_tests;
