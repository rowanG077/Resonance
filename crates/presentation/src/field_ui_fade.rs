//! Composite the field fade before drawing dialogue.
use super::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

// Ordinary overlays occupy 2..3, action hints 3..3.01, dialogue starts at10.
const DEPTH: f32 = 4.;

pub(super) fn surface(
    materials: &mut Assets<Surface>,
    images: &mut Assets<Image>,
) -> (Handle<Image>, Handle<Surface>) {
    // Reuse the UI's ordinary alpha blend with the same solid image as the HUD.
    let image = images.add(Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![255; 4],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    ));
    let material = materials.add(Surface {
        source: image.clone(),
        sampling: image.clone(),
        frame_mask: image.clone(),
        color_mask: image.clone(),
        coverage: Coverage::default(),
        opaque: false,
        additive: false,
        red_channel: false,
    });
    (image, material)
}

fn batch(world: &resonance_events::GameWorld, resolution: super::super::Resolution) -> Batch {
    let mut batch = Batch::default();
    let (alpha, white) = world.fade.as_ref().map_or((255, false), |fade| {
        (fade.alpha(world.tick) as u8, fade.white)
    });
    if alpha != 0 {
        let color = if white { 1. } else { 0. };
        batch.quad(
            resolution.ui_rect(),
            [0., 0., 1., 1.],
            [color, color, color, f32::from(alpha) / 255.],
        );
    }
    batch
}

impl Artwork {
    pub(super) fn prepare_fade(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if self.fade_layer.is_some() {
            return;
        }
        let mesh = meshes
            .add(batch(&resonance_events::GameWorld::default(), self.resolution).mesh([1, 1]));
        let material = self.fade_surface.clone();
        let entity = commands
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0., 0., DEPTH),
                Visibility::Hidden,
            ))
            .id();
        self.fade_layer = Some(Layer {
            entity,
            mesh,
            material,
            uploaded: None,
            visible: false,
        });
    }

    pub(super) fn render_fade(
        &mut self,
        world: &resonance_events::GameWorld,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let layer = self
            .fade_layer
            .as_mut()
            .context("field fade was not prepared")?;
        layer.update_mesh(batch(world, self.resolution), [1, 1], meshes)?;
        layer.show(true, commands);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_return_fade_blends_completed_color_without_tinting_dialogue() {
        let mut world = resonance_events::GameWorld::default();
        world.fade = Some(resonance_events::Fade {
            start_tick: 20440,
            duration: 20,
            from: 255.,
            to: -1.,
            white: false,
        });
        let resolution = super::super::super::Resolution::default();
        // Sample the fade on three consecutive return updates.
        for (tick, alpha) in [(20440, 242_u8), (20441, 229), (20442, 216)] {
            world.tick = tick;
            let drawn = batch(&world, resolution);
            assert_eq!(drawn.colors, vec![[0., 0., 0., f32::from(alpha) / 255.]; 4]);
            assert_eq!(
                drawn.positions,
                vec![
                    [-320., 240., 0.],
                    [320., 240., 0.],
                    [320., -240., 0.],
                    [-320., -240., 0.]
                ]
            );
            // The circle's additive passes have already saturated in the scene
            // target. Applying this layer preserves neutral saturated white.
            let completed = [1.5_f32, 1.5, 3.];
            let composed = completed.map(|v| v.min(1.) * (1. - drawn.colors[0][3]));
            assert_eq!(composed, [1. - f32::from(alpha) / 255.; 3]);
        }
        world.tick = 20482;
        assert!(batch(&world, resolution).indices.is_empty());
        world.fade.as_mut().unwrap().white = true;
        world.tick = 20441;
        assert_eq!(
            batch(&world, resolution).colors,
            vec![[1., 1., 1., 229. / 255.]; 4]
        );
        world.fade = None;
        assert_eq!(batch(&world, resolution).colors, vec![[0., 0., 0., 1.]; 4]);
        // Existing UI ownership: scene overlays precede the fade; dialogue follows.
        assert!((3.01..10.).contains(&DEPTH));
    }
}
