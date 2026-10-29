use super::*;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::RunSystemOnce;
use bevy::render::render_resource::{Extent3d, TextureDimension};

#[test]
fn title_drawing_follows_named_choices_positions_and_actual_image_sizes() -> Result<()> {
    let mut app = App::new();
    app.add_message::<AppExit>();
    app.insert_resource(Menu(TitleState {
        selected: 1,
        opacity: 200,
        ..default()
    }));
    app.init_resource::<Assets<Image>>()
        .init_resource::<Assets<TitleText>>()
        .init_resource::<Assets<TitleOutput>>()
        .init_resource::<movie::Playback>()
        .init_resource::<boot::Playback>();
    app.insert_resource(crate::diagnostics::Diagnostics(
        resonance_content::diagnostics::Diagnostics::new(true),
    ));
    let images: Vec<_> = [[152, 32], [180, 40], [512, 208]]
        .into_iter()
        .map(|[width, height]| {
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_fill(
                    Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    &[255; 4],
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                ))
        })
        .collect();
    let material = app
        .world_mut()
        .resource_mut::<Assets<TitleText>>()
        .add(TitleText {
            source: images[0].clone(),
            opacity_pulse: Vec4::ZERO,
        });
    let logo_material = app
        .world_mut()
        .resource_mut::<Assets<TitleText>>()
        .add(TitleText {
            source: images[2].clone(),
            opacity_pulse: Vec4::ZERO,
        });
    let row = app
        .world_mut()
        .spawn((
            TitleQuad {
                index: 0,
                selected_index: Some(1),
                row: Some(1),
                top: 300.,
            },
            MeshMaterial2d(material.clone()),
            Transform::default(),
        ))
        .id();
    let logo = app
        .world_mut()
        .spawn((
            TitleQuad {
                index: 2,
                selected_index: None,
                row: None,
                top: 24.,
            },
            MeshMaterial2d(logo_material),
            Transform::default(),
        ))
        .id();
    app.insert_resource(Art {
        manifest: Vec::new(),
        images: images.clone(),
    });
    app.world_mut().run_system_once(layout).unwrap();
    let transform = app.world().get::<Transform>(row).unwrap();
    assert_eq!(transform.scale, Vec3::new(180., 40., 1.));
    assert_eq!(transform.translation.y, -80.);
    let transform = app.world().get::<Transform>(logo).unwrap();
    assert_eq!(transform.scale, Vec3::new(512., 208., 1.));
    assert_eq!(transform.translation.y, 112.);
    let materials = app.world().resource::<Assets<TitleText>>();
    assert_eq!(materials.get(&material).unwrap().source, images[1]);
    assert_eq!(
        materials.get(&material).unwrap().opacity_pulse.x,
        200. / 255.
    );
    app.world_mut().resource_mut::<Menu>().0.selected = 0;
    app.world_mut().run_system_once(layout).unwrap();
    assert_eq!(
        app.world().get::<Transform>(row).unwrap().scale,
        Vec3::new(152., 32., 1.)
    );
    assert_eq!(
        app.world().get::<Transform>(row).unwrap().translation.y,
        -76.
    );
    assert_eq!(
        app.world()
            .resource::<Assets<TitleText>>()
            .get(&material)
            .unwrap()
            .source,
        images[0]
    );
    Ok(())
}
