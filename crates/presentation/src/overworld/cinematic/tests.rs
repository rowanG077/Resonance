use super::*;
use resonance_content::{CameraKey, TextureAnimation, overworld::*};

#[test]
fn prepared_actor_tracks_control_layers_materials_opacity_and_uv_clocks() -> Result<()> {
    let actors = [
        (1, CinematicActor::default()),
        (
            2,
            CinematicActor {
                background: true,
                depth_write: Some(false),
                depth_test: Some(false),
                blend: Some(CinematicBlend::Additive),
                opacity: Some(CinematicOpacity { begin: 1, end: 4 }),
                uv: vec![CinematicUv {
                    clock: CinematicClock::Presentation,
                    animation: TextureAnimation {
                        texture: 0,
                        delay_ticks: 0,
                        loop_start: 0,
                        offsets: vec![[0., 0.], [0.5, 0.]],
                    },
                }],
            },
        ),
    ]
    .into();
    let part = resonance_content::ScenePart {
        resource: 0,
        mesh: "fixture/model.glb".into(),
        textures: vec!["fixture/texture.ktx2".into()],
        materials: vec![],
        appearance: None,
        translation: [0.; 3],
        clips: vec![],
        autoplay: false,
        texture_animations: vec![],
        bone_names: vec![],
        material_nodes: vec![],
        outline_color: None,
        secondary_motion: Default::default(),
    };
    let definition = Cinematic {
        world: resonance_content::overworld::World::Sylvarant,
        camera: resonance_content::camera_motion::CameraMotion::from_keys(&[
            CameraKey::looking_at(0., [0.; 3], [0.; 3]),
            CameraKey::looking_at(10., [0.; 3], [0.; 3]),
        ])
        .unwrap(),
        actors: [(1, vec![part.clone()]), (2, vec![part])].into(),
        dialogue: vec![],
        presentation: CinematicPresentation {
            fade: CinematicFade {
                white: false,
                end_rate: 8,
                intro_rate: None,
                flashes: vec![],
            },
            initial_audio: vec![],
            audio: vec![],
            end_audio: vec![],
            actors,
        },
    };
    let mut material = TitleSurface {
        depth_write: true,
        depth_test: true,
        blend: Some(Blend::Alpha),
        ..Default::default()
    };
    definition.validate(|_| true)?;
    surface(&definition, 1, &mut material);
    assert!(material.depth_write && material.depth_test);
    assert_eq!(material.blend, Some(Blend::Alpha));
    surface(&definition, 2, &mut material);
    assert!(!material.depth_write && !material.depth_test);
    assert_eq!(material.blend, Some(Blend::Additive));
    assert!(!background(&definition, 1));
    assert!(background(&definition, 2));
    let order = draw_order(&definition, 2, 7);
    assert_eq!(
        (order.0, order.1, order.2),
        (crate::draw_order::Layer::Background, 2, 7)
    );
    // The rendering samplers are independent of the scene ID and need no files.
    let mut playback = Playback::new(
        513,
        Arc::new(definition),
        resonance_events::SceneDestination {
            map: 1,
            position: [0.; 3],
            heading: 0.,
        },
    )?;
    assert_eq!(uv(&playback, 2, 0, 7), Vec2::new(0.5, 0.));
    assert_eq!(uv(&playback, 2, 1, 7), Vec2::ZERO);
    let mut audio = vec![];
    playback.step(&mut audio)?;
    playback.step(&mut audio)?;
    assert_eq!(opacity(&playback, 2), 85. / 255.);
    assert_eq!(uv(&playback, 2, 0, 7), Vec2::new(0.5, 0.));
    assert_eq!(uv(&playback, 2, 0, 8), Vec2::ZERO);
    playback.step(&mut audio)?;
    playback.step(&mut audio)?;
    assert_eq!(opacity(&playback, 2), 0.);
    assert_eq!(opacity(&playback, 1), 1.);
    Ok(())
}
