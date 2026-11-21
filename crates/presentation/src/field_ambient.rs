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
    future: Vec<Emitter>,
    predicted: Option<(u32, u64)>,
}

#[derive(Clone)]
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
                            interval_ticks: interval,
                            lifetime_ticks: lifetime,
                            ..
                        } => (((lifetime - 1) as usize).div_ceil(interval as usize) + 1, 0),
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
            future: Vec::new(),
            predicted: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.tick = None;
        self.future.clear();
        self.predicted = None;
        for (index, emitter) in self.emitters.iter_mut().enumerate() {
            emitter.sprites.clear();
            emitter.snow.fill(Snowflake::default());
            emitter.random = index as u32 + 1;
        }
    }

    pub(super) fn render(
        &mut self,
        world: &GameWorld,
        next: Option<(&GameWorld, f32, u64)>,
        camera: Quat,
        quads: &mut Vec<(Layer, usize, Quad)>,
    ) {
        self.advance(world);
        if let Some((next, _, revision)) = next {
            let key = (world.effect_tick, revision);
            if self.predicted != Some(key) {
                self.future.clone_from(&self.emitters);
                // Swap only the disposable arrays; committed emitters and RNG never advance.
                std::mem::swap(&mut self.emitters, &mut self.future);
                let tick = self.tick;
                self.advance(next);
                self.tick = tick;
                std::mem::swap(&mut self.emitters, &mut self.future);
                self.predicted = Some(key);
            }
        } else {
            self.predicted = None;
        }
        for (index, emitter) in self.emitters.iter().enumerate() {
            emitter.draw(
                world.effect_tick,
                camera,
                world.brightness(),
                next.map(|(world, alpha, _)| {
                    (
                        &self.future[index],
                        world.effect_tick,
                        world.brightness(),
                        alpha,
                    )
                }),
                quads,
            );
        }
    }
    fn advance(&mut self, world: &GameWorld) {
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
                AmbientEmission::Drifting { lifetime_ticks, .. } => lifetime_ticks,
                AmbientEmission::Fire { .. } => 61,
                AmbientEmission::Snow {
                    spawn_height: height,
                    fall_speed_per_tick: fall_speed,
                    ..
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
                interval_ticks: interval,
                lifetime_ticks: lifetime,
                spawn_position: position,
                spawn_step: position_step,
                spawn_counts: position_steps,
                velocity_per_tick: velocity,
                size,
                color,
            } if tick.is_multiple_of(u32::from(interval)) => {
                let mut sprite = BillboardEffect::default();
                sprite.born = tick;
                sprite.lifetime = lifetime;
                sprite.position = std::array::from_fn(|i| {
                    position[i]
                        + position_step[i]
                            * sample(&mut self.random, u32::from(position_steps[i])) as f32
                });
                sprite.velocity = velocity;
                sprite.size = size;
                sprite.rgba = color;
                sprite.fade = Fade::tail(lifetime);
                self.sprites.push(sprite);
            }
            AmbientEmission::Fire { position, size } if tick.is_multiple_of(4) => {
                self.sprites
                    .extend(fire_sprites(position, tick, size, false, &mut self.random));
            }
            AmbientEmission::Snow {
                horizontal_offsets,
                spawn_height: height,
                fall_speed_per_tick: fall_speed,
                ..
            } => {
                let Some(player) = player else { return };
                for flake in &mut self.snow {
                    if flake.speed == 0 {
                        flake.position = [
                            player[0]
                                + sample(
                                    &mut self.random,
                                    (i64::from(horizontal_offsets[1])
                                        - i64::from(horizontal_offsets[0])
                                        + 1) as u32,
                                ) as f32
                                + horizontal_offsets[0] as f32,
                            player[1]
                                + sample(
                                    &mut self.random,
                                    (i64::from(horizontal_offsets[1])
                                        - i64::from(horizontal_offsets[0])
                                        + 1) as u32,
                                ) as f32
                                + horizontal_offsets[0] as f32,
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
        next: Option<(&Self, u32, f32, f32)>,
        quads: &mut Vec<(Layer, usize, Quad)>,
    ) {
        let sprite = &self.recipe.sprite;
        let (future, next_tick, next_brightness, alpha) = next
            .map_or((&[][..], tick, brightness, 0.), |(e, t, b, a)| {
                (e.sprites.as_slice(), t, b, a)
            });
        let brightness = brightness + (next_brightness - brightness) * alpha;
        let endpoint = |p: &BillboardEffect, tick: u32| {
            let [r, g, b, _] = p.rgba.map(|c| f32::from(c) * 4. / 255. * brightness);
            Quad::new(
                Vec3::from_array(p.position),
                camera * Quat::from_rotation_z(p.rotation[2].to_radians()),
                p.size,
                sprite.uv_at(tick.wrapping_sub(p.born)),
                [r, g, b, p.alpha(tick) / 255.],
                VerticalAnchor::Center,
            )
        };
        for (current, next) in sprite_pairs(&self.sprites, future) {
            let quad = Quad::between(
                current.map(|p| endpoint(p, tick)),
                next.map(|p| endpoint(p, next_tick)),
                alpha,
            );
            quads.push((Layer::Effects, self.material, quad));
        }
        if let AmbientEmission::Snow { size, color, .. } = self.recipe.emission {
            let [r, g, b, a] = color.map(|v| f32::from(v) / 255.);
            for (index, flake) in self.snow.iter().enumerate() {
                let future = next
                    .and_then(|(next, _, _, _)| next.snow.get(index))
                    .unwrap_or(flake);
                let position = match (flake.speed != 0, future.speed != 0) {
                    (true, true) => Vec3::from_array(flake.position)
                        .lerp(Vec3::from_array(future.position), alpha),
                    (true, false) => Vec3::from_array(flake.position),
                    (false, true) => Vec3::from_array(future.position),
                    (false, false) => continue,
                };
                let opacity = if flake.speed == 0 {
                    alpha
                } else if future.speed == 0 {
                    1. - alpha
                } else {
                    1.
                };
                quads.push((
                    Layer::Effects,
                    self.material,
                    Quad::new(
                        position,
                        camera,
                        [size; 2],
                        sprite.uv,
                        [r * brightness, g * brightness, b * brightness, a * opacity],
                        VerticalAnchor::Center,
                    ),
                ));
            }
        }
    }
}

// Emitters retain chronological birth order. Fire's two layers have distinct lifetimes.
fn sprite_pairs<'a>(
    from: &'a [BillboardEffect],
    to: &'a [BillboardEffect],
) -> impl Iterator<Item = (Option<&'a BillboardEffect>, Option<&'a BillboardEffect>)> {
    let (mut a, mut b) = (from.iter().peekable(), to.iter().peekable());
    std::iter::from_fn(move || match (a.peek(), b.peek()) {
        (Some(x), Some(y)) if x.born == y.born && x.lifetime == y.lifetime => {
            Some((a.next(), b.next()))
        }
        (Some(x), Some(y)) if (x.born.wrapping_sub(y.born) as i32) > 0 => Some((None, b.next())),
        (Some(_), _) => Some((a.next(), None)),
        (_, Some(_)) => Some((None, b.next())),
        _ => None,
    })
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
    fn speculative_weather_reuses_its_endpoint_and_keeps_live_particles_and_rng() {
        let mut ambient = Ambient::new([(
            recipe(AmbientEmission::Fire {
                position: [0.; 3],
                size: 20.,
            }),
            0,
        )]);
        let mut source = GameWorld::default();
        source.effect_tick = 4;
        let mut next = GameWorld::default();
        next.effect_tick = 5;
        let mut quads = Vec::new();
        ambient.render(&source, Some((&next, 0.25, 1)), Quat::IDENTITY, &mut quads);
        let random = ambient.emitters[0].random;
        let next_address = ambient.future[0].sprites.as_ptr();
        assert_eq!(ambient.emitters[0].sprites[0].position, [0.; 3]);
        let fire = &ambient.emitters[0].sprites[0];
        let next_position = fire.velocity;
        assert_eq!(ambient.future[0].sprites[0].position, next_position);
        let width = 2. * (fire.size[0] / 2.).trunc();
        assert_eq!(fire.size[0], width);
        let calls = resonance_simulation_memory::system_calls();
        for alpha in [0.1, 0.5, 0.9] {
            quads.clear();
            ambient.render(&source, Some((&next, alpha, 1)), Quat::IDENTITY, &mut quads);
            let [left, right, ..] = quads[0].2.positions().map(Vec3::from_array);
            assert!((left.distance(right) - (width - 2. * alpha)).abs() < 1e-5);
            assert_eq!(ambient.future[0].sprites.as_ptr(), next_address);
            assert_eq!(ambient.emitters[0].random, random);
            assert_eq!(ambient.emitters[0].sprites[0].position, [0.; 3]);
        }
        assert_eq!(resonance_simulation_memory::system_calls(), calls);
        quads.clear();
        ambient.render(&next, None, Quat::IDENTITY, &mut quads);
        assert_eq!(ambient.emitters[0].sprites[0].position, next_position);
    }

    #[test]
    fn emissions_are_bounded_and_gates_stop_births_without_erasing_live_particles() {
        let mut ambient = Ambient::new([(
            recipe(AmbientEmission::Drifting {
                interval_ticks: 4,
                lifetime_ticks: 13,
                spawn_position: [0.; 3],
                spawn_step: [0.; 3],
                spawn_counts: [1; 3],
                velocity_per_tick: [1., 0., 0.],
                size: [10.; 2],
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
        ambient.render(&world, None, Quat::IDENTITY, &mut quads);
        world.effect_tick = 1_000_000;
        quads.clear();
        ambient.render(&world, None, Quat::IDENTITY, &mut quads);
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
    fn atmosphere_duration_size_and_discrete_samples_keep_previous_appearance() {
        let mut ambient = Ambient::new([(
            recipe(AmbientEmission::Drifting {
                interval_ticks: 8,
                lifetime_ticks: 121,
                spawn_position: [1024., -2184., 253.],
                spawn_step: [-124., 0., 32.],
                spawn_counts: [8, 1, 8],
                velocity_per_tick: [0., 32., 0.],
                size: [2048., 896.],
                color: [63, 51, 31, 128],
            }),
            0,
        )]);
        let emitter = &mut ambient.emitters[0];
        let mut random = 1;
        let mut births = Vec::new();
        for tick in 0..500 {
            if tick % 8 == 0 {
                births.push((
                    tick,
                    [
                        1024. - 124. * sample(&mut random, 8) as f32,
                        -2184. + 0. * sample(&mut random, 1) as f32,
                        253. + 32. * sample(&mut random, 8) as f32,
                    ],
                ));
            }
            emitter.step(tick, true, None);
            let live: Vec<_> = births
                .iter()
                .filter(|(born, _)| tick - born <= 120)
                .collect();
            assert_eq!(emitter.sprites.len(), live.len());
            for (actual, (born, position)) in emitter.sprites.iter().zip(live) {
                assert_eq!(
                    actual.position,
                    [
                        position[0],
                        position[1] + 32. * (tick - born) as f32,
                        position[2]
                    ]
                );
                assert_eq!(actual.size, [2048., 896.]);
                assert_eq!(actual.lifetime, 121);
                let mut previous = BillboardEffect::default();
                previous.born = *born;
                previous.rgba = [63, 51, 31, 128];
                previous.fade = Fade::tail(121);
                assert_eq!(actual.alpha(tick), previous.alpha(tick));
            }
        }
        assert_eq!(emitter.random, random);
        let mut ambient = Ambient::new([(
            recipe(AmbientEmission::Snow {
                count: 4096,
                horizontal_offsets: [-1023, 1024],
                spawn_height: 800.,
                fall_speed_per_tick: [3, 18],
                size: 12.,
                color: [240, 240, 255, 112],
                fog_color: [0, 0, 31, 16],
                fog_range: [100., 10000.],
            }),
            0,
        )]);
        let emitter = &mut ambient.emitters[0];
        emitter.step(0, true, Some([137., -218., 40.]));
        let mut random = 1;
        for flake in &emitter.snow {
            let x = 137. + sample(&mut random, 1023 * 2 + 2) as f32 - 1023.;
            let y = -218. + sample(&mut random, 1023 * 2 + 2) as f32 - 1023.;
            let speed = 3 + sample(&mut random, 16) as u8;
            assert_eq!(flake.position, [x, y, 840. - f32::from(speed)]);
            assert_eq!(flake.speed, speed);
        }
        assert_eq!(emitter.random, random);
    }

    #[test]
    fn snow_recycles_around_the_current_player_in_one_fixed_buffer() {
        let mut ambient = Ambient::new([(
            recipe(AmbientEmission::Snow {
                count: 4096,
                horizontal_offsets: [-20, 21],
                spawn_height: 10.,
                fall_speed_per_tick: [3, 5],
                size: 12.0,
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
