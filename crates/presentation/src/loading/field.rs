//! Fixed field-effect and shadow resources, prepared through the live GPU observer.
use super::*;
use crate::{field_effects, field_view::SharedShadows, gpu_prepare, materials::TitleSurface};

pub(super) struct Warm {
    report: gpu_prepare::Shared,
    images: Vec<Handle<Image>>,
    bindings: Vec<(Handle<Mesh>, Handle<TitleSurface>)>,
    since: Instant,
}

pub(super) fn abort(world: &mut World) {
    world.remove_resource::<field_effects::Shared>();
    world.remove_resource::<SharedShadows>();
}

impl Warm {
    pub(super) fn new(world: &mut World, common: &startup::Common) -> Result<Option<Self>> {
        let Some(defaults) = common
            .field
            .as_ref()
            .and_then(|field| field.defaults.as_ref())
        else {
            return Ok(None);
        };
        let server = world.resource::<AssetServer>().clone();
        let (effects, shadows) = world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            let mut surfaces = world.resource_mut::<Assets<TitleSurface>>();
            Ok::<_, anyhow::Error>((
                field_effects::Shared::new(
                    &defaults.settings,
                    &server,
                    &mut meshes,
                    &mut surfaces,
                    &common.files,
                )?,
                SharedShadows::new(
                    &defaults.settings.contact_shadow,
                    &server,
                    &mut meshes,
                    &mut surfaces,
                ),
            ))
        })?;
        let images = effects
            .images()
            .chain(std::iter::once(shadows.image()))
            .cloned()
            .collect();
        let bindings = effects
            .bindings()
            .chain(std::iter::once(shadows.binding()))
            .collect();
        world.insert_resource(effects);
        world.insert_resource(shadows);
        Ok(Some(Self {
            report: gpu_prepare::Shared::new(world),
            images,
            bindings,
            since: Instant::now(),
        }))
    }

    pub(super) fn update(&mut self, world: &mut World) -> Result<bool> {
        let mut report = self.report.0.lock().unwrap();
        if let Some(error) = &report.error {
            anyhow::bail!("shared field graphics: {error}");
        }
        if report.completed.load(Ordering::Acquire) {
            return Ok(true);
        }
        anyhow::ensure!(
            self.since.elapsed().as_secs() < 120,
            "shared field graphics timed out"
        );
        if report.armed {
            return Ok(false);
        }
        let server = world.resource::<AssetServer>();
        let images = world.resource::<Assets<Image>>();
        let mut ready = true;
        for image in &self.images {
            ready &= crate::field_view::image_ready(server, images, image)?;
        }
        if !ready {
            return Ok(false);
        }
        let samples = world
            .query_filtered::<&Msaa, With<crate::FieldCamera>>()
            .single(world)
            .map_or(1, Msaa::samples);
        for (mesh, material) in &self.bindings {
            report.prepare(
                gpu_prepare::View::Field,
                mesh.id(),
                material.id().untyped(),
                false,
                Some(samples),
            );
        }
        report.images.extend(self.images.iter().map(Handle::id));
        report.armed = true;
        Ok(false)
    }
}
