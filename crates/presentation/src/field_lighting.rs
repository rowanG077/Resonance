//! Live actor positions drive per-vertex scenery lighting.
use super::super::materials::SceneryLighting;
use bevy::prelude::*;
use resonance_content::field::FieldEnvironment;
use resonance_events::GameWorld;

pub(super) fn resolve(
    environment: &FieldEnvironment,
    world: &GameWorld,
    actor: i32,
) -> SceneryLighting {
    let Some(recipe) = &environment.lights else {
        return SceneryLighting::default();
    };
    if recipe.target.resolve(&world.texture_bindings) != actor
        || !recipe.gate.active(
            world.texture_animation_enabled,
            &world.texture_bindings,
            |id| world.actors.contains_key(&id),
        )
    {
        return SceneryLighting::default();
    }
    let mut lighting = SceneryLighting {
        ambient: Vec3::from_array(recipe.ambient.map(|value| f32::from(value) / 255.)).extend(1.),
        color: Vec3::from_array(recipe.color.map(|value| f32::from(value) / 255.)).extend(0.),
        attenuation: Vec3::from_array(recipe.attenuation).extend(0.),
        ..Default::default()
    };
    for (position, light) in lighting.positions.iter_mut().zip(&recipe.lights) {
        if let Some(source) = world
            .actors
            .get(&light.actor.resolve(&world.texture_bindings))
        {
            *position = (Vec3::from_array(source.position) + Vec3::Z * light.height)
                .extend(light.intensity);
        }
    }
    lighting
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::field::{EnvironmentGate, RenderValue, SceneryLight};

    #[test]
    fn lights_follow_live_bindings_and_clear_when_the_gate_actor_leaves() {
        let environment = FieldEnvironment {
            lights: Some(resonance_content::field::SceneryLighting {
                gate: EnvironmentGate::ActorExists(RenderValue::Setting(7)),
                target: RenderValue::Fixed(999_996),
                ambient: [50, 60, 70],
                color: [255, 159, 159],
                attenuation: [1., 0.002, 0.],
                lights: vec![
                    SceneryLight {
                        actor: RenderValue::Setting(6),
                        height: 200.,
                        intensity: 1.,
                    },
                    SceneryLight {
                        actor: RenderValue::Setting(7),
                        height: 200.,
                        intensity: 2.,
                    },
                ],
            }),
            ..Default::default()
        };
        let mut world = GameWorld::default();
        world.texture_bindings.extend([(6, 20), (7, 30)]);
        world.insert_actor(30, resonance_events::Actor::new(0, [10., 20., 30.]));
        let light = resolve(&environment, &world, 999_996);
        assert_eq!(light.positions[0], Vec4::ZERO);
        assert_eq!(light.positions[1], Vec4::new(10., 20., 230., 2.));
        assert_eq!(
            resolve(&environment, &world, 30),
            SceneryLighting::default()
        );
        world.actors.get_mut(&30).unwrap().position[0] = 80.;
        assert_eq!(resolve(&environment, &world, 999_996).positions[1].x, 80.);
        world.actors.remove(&30);
        assert_eq!(
            resolve(&environment, &world, 999_996),
            SceneryLighting::default()
        );
    }
}
