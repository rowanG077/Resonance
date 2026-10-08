//! Opt-in title playback using locally cooked assets.
mod common;
use common::{asset_root, cooked};
use resonance_content::TitleAssets;
use std::fs;
#[test]
#[ignore = "requires locally cooked GQSEAF title assets"]
fn title_program_keeps_the_scene_and_particles_running() {
    let root = asset_root();
    let assets: TitleAssets = cooked("title.json");
    assets.validate().unwrap();
    let scene = assets.scene.unwrap();
    let mut runtime = resonance_game::title_events::start(
        &fs::read(root.join(&scene.script.path)).unwrap(),
        &scene,
        |path| {
            Ok(std::sync::Arc::new(
                resonance_content::animation::Motion::decode(&fs::read(root.join(path))?)?,
            ))
        },
    )
    .unwrap();
    assert_eq!(runtime.world.actors.len(), 6);
    // Overlay property 8 changes target opacity, independently of actor visibility.
    assert!(runtime.world.actors[&1000].visible);
    assert_eq!(runtime.world.overlays[&1000].rgba[3], 0);
    assert_eq!(runtime.world.particles.len(), 2);
    let initial = runtime.world.particles[0].clone();
    let initial_camera = runtime.world.camera.as_ref().unwrap().resource;
    let (mut moved, mut grew, mut faded, mut changed_camera) = (false, false, false, false);
    for _ in 0..10_000 {
        runtime.step().unwrap();
        assert!(runtime.active_instances() <= 4);
        assert!(
            runtime.world.particles.len() <= 64,
            "particle tails accumulated"
        );
        let tick = runtime.tick();
        for particle in &runtime.world.particles {
            let (position, size, rgba) = particle.sample(tick);
            assert!(position.iter().chain(&rgba).all(|v| v.is_finite()));
            assert!(size.is_finite() && size >= 0.);
            moved |= position != particle.position;
            grew |= size > particle.size;
            faded |= rgba[3] < particle.rgba[3];
        }
        changed_camera |= runtime.world.camera.as_ref().unwrap().resource != initial_camera;
    }
    assert!(
        moved && grew && faded,
        "title trails must move, grow and fade"
    );
    assert!(changed_camera, "title scene stopped progressing");
    assert!(
        runtime
            .world
            .particles
            .iter()
            .all(|p| p.handle != initial.handle),
        "initial particle never expired"
    );
}
