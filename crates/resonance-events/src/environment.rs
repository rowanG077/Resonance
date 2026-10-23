//! Shared cosmetic forces sampled once per simulation update.
use crate::GameWorld;
use resonance_content::{field::FieldWind, random::Random};

pub(crate) fn wind(world: &mut GameWorld, recipe: Option<&FieldWind>) {
    let previous = world
        .environment_wind
        .last_key_value()
        .map(|(_, force)| *force);
    world.environment_wind.retain(|tick, _| {
        world.tick.saturating_sub(*tick) < crate::projectile::CHAIN_HISTORY_TICKS
    });
    let Some(recipe) = recipe else { return };
    let force = if recipe.gate.active(
        world.texture_animation_enabled,
        &world.texture_bindings,
        |id| world.actors.contains_key(&id),
    ) {
        // A cosmetic stream derived from the update never consumes gameplay randomness.
        let mut random = Random::new(u64::from(world.effect_tick));
        std::array::from_fn(|axis| {
            recipe.base[axis]
                + recipe.step[axis] * (random.next_u32() % u32::from(recipe.steps)) as f32
        })
    } else if let Some(previous) = previous {
        // The gate controls new samples; the last force remains active.
        previous
    } else {
        return;
    };
    world.environment_wind.insert(world.tick, force);
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::field::EnvironmentGate;

    #[test]
    fn wind_keeps_bounded_history_and_observes_live_settings() {
        let mut world = GameWorld::default();
        let recipe = FieldWind {
            gate: EnvironmentGate::SettingNonzero(7),
            base: [0., 1.8, -0.8],
            step: [0., 1. / 15., -1. / 30.],
            steps: 32,
        };
        wind(&mut world, Some(&recipe));
        assert!(world.environment_wind.is_empty());
        world.texture_bindings.insert(7, 1);
        for tick in 1..=40 {
            world.tick = tick;
            world.effect_tick = tick;
            wind(&mut world, Some(&recipe));
        }
        assert_eq!(
            world.environment_wind.len(),
            crate::projectile::CHAIN_HISTORY_TICKS as usize
        );
        assert!(
            world
                .environment_wind
                .values()
                .all(|v| v[0] == 0. && (1.8..=4.).contains(&v[1]) && (-1.9..=-0.8).contains(&v[2]))
        );
        let last = world.environment_wind[&world.tick];
        world.texture_bindings.insert(7, 0);
        for _ in 0..40 {
            world.tick += 1;
            wind(&mut world, Some(&recipe));
            assert_eq!(world.environment_wind[&world.tick], last);
        }
        assert_eq!(
            world.environment_wind.len(),
            crate::projectile::CHAIN_HISTORY_TICKS as usize
        );
    }
}
