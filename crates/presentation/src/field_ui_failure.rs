//! Retry notice composed from the current scene's already resident bitmap font.
use super::*;

#[derive(Resource)]
struct Notice(Entity);

pub(crate) fn update(world: &mut World) {
    if !world.contains_resource::<crate::new_game::TransitionFailure>() {
        if let Some(notice) = world.remove_resource::<Notice>() {
            world.despawn(notice.0);
        }
        return;
    }
    if world.contains_resource::<Notice>() {
        return;
    }
    let result = (|| -> Result<()> {
        let font: BitmapFont = world
            .resource::<crate::loading::Resident>()
            .files
            .read()
            .unwrap()
            .as_ref()
            .context("scene font inventory is missing")?
            .json("fonts/dialogue.json")?;
        let image = world
            .resource::<AssetServer>()
            .get_handle::<Image>(&font.texture)
            .context("scene font is not resident")?;
        let mut batch = Batch::default();
        batch.quad([36., 160., 604., 302.], [0.5; 4], [0., 0., 0., 0.92]);
        for (text, y, size) in [
            ("Unable to enter this area.", 181., 23.),
            ("Enter / A: Retry", 225., 19.),
            ("F9: Load quicksave", 257., 19.),
        ] {
            skit::centered(&mut batch, &font, text, y, size, 1., false)?;
        }
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(batch.mesh([font.width, font.height]));
        let material = world.resource_mut::<Assets<Surface>>().add(Surface {
            source: image.clone(),
            sampling: image.clone(),
            frame_mask: image.clone(),
            color_mask: image,
            coverage: Coverage::default(),
            opaque: false,
            additive: false,
            red_channel: false,
        });
        let entity = world
            .spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_xyz(0., 0., 20.),
            ))
            .id();
        world.insert_resource(Notice(entity));
        Ok(())
    })();
    if let Err(error) = result {
        // The window title still carries the full error and recovery controls.
        warn!("Could not display area retry notice: {error:#}");
    }
}
