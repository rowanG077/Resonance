//! Continuous field effects use bounded particle arrays and shared sprite draws.
use super::{draw_order::Layer, field_effects::Quad};
use bevy::prelude::*;
use resonance_content::{
    effect::VerticalAnchor,
    field::{AmbientEmission, AmbientParticles},
};
use resonance_events::{
    GameWorld,
    effect::{BillboardEffect, Fade, fire_sprites},
};

#[derive(Default)]
pub(super) struct Ambient {
    emitters: Vec<Emitter>,
    tick: Option<u32>,
}

struct Emitter {
    recipe: AmbientParticles,
    material: usize,
    sprites: Vec<BillboardEffect>,
    snow: Vec<Snowflake>,
    random: u32,
}

#[derive(Clone, Copy, Default)]
struct Snowflake {
    position: [f32; 3],
    speed: u8,
}

impl Ambient {
    pub(super) fn new(recipes: impl IntoIterator<Item = (AmbientParticles, usize)>) -> Self {
        Self {
            emitters: recipes
                .into_iter()
                .enumerate()
                .map(|(index, (recipe, material))| {
                    let (capacity, count) = match recipe.emission {
                        AmbientEmission::Drifting {
                            interval, lifetime, ..
                        } => (usize::from(lifetime).div_ceil(usize::from(interval)) + 1, 0),
                        // Fire emits two layers every fourth tick, alive for 61/31 ticks.
                        AmbientEmission::Fire { .. } => {
                            (61usize.div_ceil(4) + 31usize.div_ceil(4), 0)
                        }
                        AmbientEmission::Snow { count, .. } => (0, usize::from(count)),
                    };
                    Emitter {
                        recipe,
                        material,
                        sprites: Vec::with_capacity(capacity),
                        snow: vec![Snowflake::default(); count],
                        random: index as u32 + 1,
                    }
                })
                .collect(),
            tick: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.tick = None;
        for (index, emitter) in self.emitters.iter_mut().enumerate() {
            emitter.sprites.clear();
            emitter.snow.fill(Snowflake::default());
            emitter.random = index as u32 + 1;
        }
    }

    pub(super) fn render(
        &mut self,
        world: &GameWorld,
        camera: Quat,
        quads: &mut Vec<(Layer, usize, Quad)>,
    ) {
        let tick = world.effect_tick;
        let elapsed = self.tick.map_or(1, |previous| tick.wrapping_sub(previous));
        self.tick = Some(tick);
        let player = world
            .actors
            .get(&world.controlled_actor)
            .map(|a| a.position);
        for emitter in &mut self.emitters {
            let active = emitter.recipe.gate.active(
                world.texture_animation_enabled,
                &world.texture_bindings,
                |id| world.actors.contains_key(&id),
            );
            // A long pause/seek cannot contribute particles older than their lifetime.
            // Bound catch-up by that horizon rather than elapsed session time.
            let history = match emitter.recipe.emission {
                AmbientEmission::Drifting { lifetime, .. } => u32::from(lifetime) + 1,
                AmbientEmission::Fire { .. } => 61,
                AmbientEmission::Snow {
                    height, fall_speed, ..
                } => {
                    ((player.map_or(0., |p| p[2]) + height).max(0.) / f32::from(fall_speed[0]))
                        .ceil() as u32
                        + 1
                }
            };
            if elapsed > history {
                emitter.sprites.clear();
                emitter.snow.fill(Snowflake::default());
            }
            let steps = elapsed.min(history);
            for offset in (0..steps).rev() {
                emitter.step(tick.wrapping_sub(offset), active, player);
            }
            emitter.draw(tick, camera, world.brightness(), quads);
        }
    }
}

impl Emitter {
    fn step(&mut self, tick: u32, active: bool, player: Option<[f32; 3]>) {
        for sprite in &mut self.sprites {
            sprite.step();
        }
        self.sprites.retain(|sprite| {
            tick.wrapping_sub(sprite.born) < sprite.lifetime
                && sprite.size.iter().all(|size| *size >= 0.)
        });
        if !active {
            return;
        }
        match self.recipe.emission {
            AmbientEmission::Drifting {
                interval,
                lifetime,
                position,
                position_step,
                position_steps,
                velocity,
                half_size,
                color,
            } if tick.is_multiple_of(u32::from(interval)) => {
                let lifetime = u32::from(lifetime) + 1;
                let mut sprite = BillboardEffect::default();
                sprite.born = tick;
                sprite.lifetime = lifetime;
                sprite.position = std::array::from_fn(|i| {
                    position[i]
                        + position_step[i]
                            * sample(&mut self.random, u32::from(position_steps[i])) as f32
                });
                sprite.velocity = velocity;
                sprite.size = half_size.map(|half| half * 2.);
                sprite.rgba = color;
                sprite.fade = Fade::tail(lifetime);
                self.sprites.push(sprite);
            }
            AmbientEmission::Fire { position, size } if tick.is_multiple_of(4) => {
                self.sprites
                    .extend(fire_sprites(position, tick, size, false, &mut self.random));
            }
            AmbientEmission::Snow {
                spread,
                height,
                fall_speed,
                ..
            } => {
                let Some(player) = player else { return };
                for flake in &mut self.snow {
                    if flake.speed == 0 {
                        flake.position = [
                            player[0] + sample(&mut self.random, u32::from(spread) * 2 + 2) as f32
                                - f32::from(spread),
                            player[1] + sample(&mut self.random, u32::from(spread) * 2 + 2) as f32
                                - f32::from(spread),
                            player[2] + height,
                        ];
                        flake.speed = fall_speed[0]
                            + sample(
                                &mut self.random,
                                u32::from(fall_speed[1] - fall_speed[0]) + 1,
                            ) as u8;
                    }
                    flake.position[2] -= f32::from(flake.speed);
                    if flake.position[2] < 0. {
                        flake.speed = 0;
                    }
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tick: u32,
        camera: Quat,
        brightness: f32,
        quads: &mut Vec<(Layer, usize, Quad)>,
    ) {
        let sprite = &self.recipe.sprite;
        for particle in &self.sprites {
            let rgb = particle.rgba.map(|v| f32::from(v) * 4. / 255. * brightness);
            quads.push((
                Layer::Effects,
                self.material,
                Quad::new(
                    Vec3::from_array(particle.position),
                    camera * Quat::from_rotation_z(particle.rotation[2].to_radians()),
                    particle.size,
                    sprite.uv_at(tick.wrapping_sub(particle.born)),
                    [rgb[0], rgb[1], rgb[2], particle.alpha(tick) / 255.],
                    VerticalAnchor::Center,
                ),
            ));
        }
        if let AmbientEmission::Snow {
            half_size, color, ..
        } = self.recipe.emission
        {
            let [r, g, b, a] = color.map(|v| f32::from(v) / 255.);
            for flake in self.snow.iter().filter(|flake| flake.speed != 0) {
                quads.push((
                    Layer::Effects,
                    self.material,
                    Quad::new(
                        Vec3::from_array(flake.position),
                        camera,
                        [half_size * 2.; 2],
                        sprite.uv,
                        [r * brightness, g * brightness, b * brightness, a],
                        VerticalAnchor::Center,
                    ),
                ));
            }
        }
    }
}

fn sample(state: &mut u32, count: u32) -> u32 {
    // Xorshift gives independent, reproducible environmental variation.
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state % count
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{effect::SpriteRecipe, field::EnvironmentGate};

    fn recipe(emission: AmbientEmission) -> AmbientParticles {
        AmbientParticles {
            gate: EnvironmentGate::Always,
            sprite: SpriteRecipe {
                texture: "weather.png".into(),
                uv: [0., 0., 1., 1.],
                additive: false,
                frames: Vec::new(),
                repeat: false,
            },
            emission,
        }
    }

    #[test]
    fn emissions_are_bounded_and_gates_stop_births_without_erasing_live_particles() {
        let mut ambient = Ambient::new([(
            recipe(AmbientEmission::Drifting {
                interval: 4,
                lifetime: 12,
                position: [0.; 3],
                position_step: [0.; 3],
                position_steps: [1; 3],
                velocity: [1., 0., 0.],
                half_size: [5.; 2],
                color: [64; 4],
            }),
            0,
        )]);
        let emitter = &mut ambient.emitters[0];
        let capacity = emitter.sprites.capacity();
        for tick in 0..1000 {
            emitter.step(tick, true, None);
        }
        assert_eq!(emitter.sprites.capacity(), capacity);
        assert_eq!(emitter.sprites.len(), 3);
        emitter.step(1000, false, None);
        assert_eq!(emitter.sprites.len(), 3);
        for tick in 1001..1013 {
            emitter.step(tick, false, None);
        }
        assert!(emitter.sprites.is_empty());
        let mut world = GameWorld::default();
        let mut quads = Vec::new();
        world.effect_tick = 2000;
        ambient.render(&world, Quat::IDENTITY, &mut quads);
        world.effect_tick = 1_000_000;
        quads.clear();
        ambient.render(&world, Quat::IDENTITY, &mut quads);
        assert_eq!(ambient.emitters[0].sprites.capacity(), capacity);
        assert_eq!(ambient.emitters[0].sprites.len(), 4);
        assert!(
            ambient.emitters[0]
                .sprites
                .iter()
                .all(|p| p.position[0] <= 12.)
        );
    }

    #[test]
    fn snow_recycles_around_the_current_player_in_one_fixed_buffer() {
        let mut ambient = Ambient::new([(
            recipe(AmbientEmission::Snow {
                count: 4096,
                spread: 20,
                height: 10.,
                fall_speed: [3, 5],
                half_size: 6.,
                color: [255; 4],
                fog_color: [0; 4],
                fog_range: [0., 1000.],
            }),
            0,
        )]);
        let emitter = &mut ambient.emitters[0];
        let allocation = emitter.snow.as_ptr();
        emitter.step(0, true, Some([0.; 3]));
        for tick in 1..20 {
            emitter.step(tick, true, Some([100., 200., 0.]));
        }
        assert_eq!(emitter.snow.as_ptr(), allocation);
        assert_eq!(emitter.snow.len(), 4096);
        assert!(
            emitter
                .snow
                .iter()
                .all(|p| (80. ..=121.).contains(&p.position[0])
                    && (180. ..=221.).contains(&p.position[1])
                    && p.position[2] < 10.)
        );
        ambient.reset();
        assert!(ambient.emitters[0].snow.iter().all(|p| p.speed == 0));
    }
}
