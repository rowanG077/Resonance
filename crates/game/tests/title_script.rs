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
    assert_eq!(runtime.world.billboards.len(), 2);
    let initial = *runtime.world.billboards.keys().next().unwrap();
    let initial_camera = runtime.world.camera.as_ref().unwrap().resource;
    let (mut moved, mut grew, mut faded, mut changed_camera) = (false, false, false, false);
    for _ in 0..10_000 {
        let previous = runtime.world.billboards.clone();
        runtime.step().unwrap();
        assert!(runtime.active_instances() <= 4);
        assert!(
            runtime.world.billboards.len() <= 64,
            "particle tails accumulated"
        );
        let tick = runtime.tick();
        for (id, particle) in &runtime.world.billboards {
            assert!(particle.position.iter().all(|v| v.is_finite()));
            assert!(particle.size.iter().all(|v| v.is_finite() && *v >= 0.));
            if let Some(before) = previous.get(id) {
                moved |= particle.position != before.position;
                grew |= particle.size[0] > before.size[0];
                faded |= particle.alpha(tick) < before.alpha(tick - 1);
            }
        }
        changed_camera |= runtime.world.camera.as_ref().unwrap().resource != initial_camera;
    }
    assert!(
        moved && grew && faded,
        "title trails must move, grow and fade"
    );
    assert!(changed_camera, "title scene stopped progressing");
    assert!(
        !runtime.world.billboards.contains_key(&initial),
        "initial particle never expired"
    );
}
