//! Opt-in integration evidence. The original script/assets stay in ignored local/.
mod common;
use common::{asset_root, cooked};
use resonance_content::TitleAssets;
use std::fs;
#[test]
#[ignore = "requires locally cooked GQSEAF title assets"]
fn original_title_program_drives_the_scene_for_ten_thousand_updates() {
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
    // These title actors use strict depth testing with depth writes disabled.
    for (id, depth_write) in [
        (2000, true),
        (2001, false),
        (2002, false),
        (2003, true),
        (2004, false),
    ] {
        assert_eq!(runtime.world.actors[&id].depth_write, depth_write);
    }
    for tick in 1..=10000 {
        runtime.step().unwrap();
        assert!(runtime.active_instances() <= 4);
        assert!(runtime.world.particles.len() <= 64);
        match tick {
            120 => {
                let births: Vec<_> = runtime
                    .world
                    .particles
                    .iter()
                    .filter(|p| p.born == tick)
                    .collect();
                // Dolphin title-entry checkpoint: newly allocated particle pair.
                assert_eq!(births[0].position, [240., -575., 1035.]);
                assert_eq!(births[1].position, [117., -1272., -1087.]);
                assert_eq!(births[0].rgba, [8., 8., 16., 130.]);
                assert_eq!(births[1].rgba, [11., 7., 15., 130.]);
            }
            730 => {
                let p = runtime.world.particles.last().unwrap();
                assert_eq!(p.born, 730);
                assert_eq!(p.size, 800.);
                assert_eq!(p.size_delta, 9.);
            }
            761 => {
                let p = runtime.world.particles.last().unwrap();
                assert_eq!(p.born, 761);
                assert_eq!(p.size, 500.);
            }
            841 => assert_eq!(
                runtime.world.actors[&2002].animation.as_ref().unwrap().slot,
                12
            ),
            842 => {
                let a = runtime.world.actors[&2002].animation.as_ref().unwrap();
                assert_eq!(a.slot, 36);
                assert_eq!(a.start_tick, 842);
            }
            1241 => assert_eq!(runtime.world.camera.as_ref().unwrap().resource, 0),
            1242 => {
                let c = runtime.world.camera.as_ref().unwrap();
                assert_eq!(c.resource, 1);
                assert_eq!(c.start_tick, 1242);
            }
            _ => {}
        }
    }
}
