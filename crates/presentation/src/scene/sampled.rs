//! Sampler variants share one uploaded texture while keeping independent views.
use super::*;
use bevy::{
    asset::{RenderAssetUsages, StrongHandle},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    render::{
        Render, RenderApp, RenderSystems,
        erased_render_asset::prepare_erased_assets,
        render_asset::{RenderAssets, prepare_assets},
        render_resource::{Sampler, TextureViewDescriptor},
        renderer::RenderDevice,
        texture::GpuImage,
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, Weak},
};

#[derive(Resource, Default, Clone)]
struct Views(Arc<Mutex<HashMap<AssetId<Image>, View>>>);
struct View {
    source: Handle<Image>,
    handle: Weak<StrongHandle>,
    view: TextureViewDescriptor<'static>,
    sampler: ImageSamplerDescriptor,
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<SampledImages>();
    let views = app.world().resource::<SampledImages>().views.clone();
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.insert_resource(views).add_systems(Render,
            prepare_views
                .in_set(RenderSystems::PrepareAssets)
                .after(prepare_assets::<GpuImage>)
                .before(prepare_erased_assets::<MeshMaterial3d<TitleSurface>>)
                .before(prepare_erased_assets::<MeshMaterial3d<crate::model_preview::material::Surface>>),
        );
    }
}

fn prepare_views(
    views: Res<Views>,
    mut images: ResMut<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    mut previous: Local<HashSet<AssetId<Image>>>,
    mut samplers: Local<Vec<(ImageSamplerDescriptor, Sampler)>>,
) {
    profile_span!("sampler views");
    if device.is_changed() {
        samplers.clear();
    }
    let mut views = views.0.lock().unwrap();
    views.retain(|_, view| view.handle.strong_count() > 0);
    for id in previous.drain() {
        if !views.contains_key(&id) {
            images.remove(id);
        }
    }
    for (&id, view) in views.iter() {
        previous.insert(id);
        let Some(source) = images.get(&view.source) else {
            continue;
        };
        if images
            .get(id)
            .is_some_and(|image| image.texture.id() == source.texture.id())
        {
            continue;
        }
        let texture = source.texture.clone();
        let texture_descriptor = source.texture_descriptor.clone();
        let texture_view = texture.create_view(&view.view);
        // Samplers describe filtering, not image storage. Share their GPU
        // identity across textures so bindless tables can reuse the same slot.
        let sampler = if let Some((_, sampler)) = samplers
            .iter()
            .find(|(descriptor, _)| descriptor == &view.sampler)
        {
            sampler.clone()
        } else {
            let sampler = device.create_sampler(&view.sampler.as_wgpu());
            samplers.push((view.sampler.clone(), sampler.clone()));
            sampler
        };
        images.insert(
            id,
            GpuImage {
                texture,
                texture_view,
                sampler,
                texture_descriptor,
                texture_view_descriptor: Some(view.view.clone()),
                had_data: false,
            },
        );
    }
}

#[derive(Resource, Default, Clone)]
pub(crate) struct SampledImages {
    lookup: Arc<Mutex<Lookup>>,
    views: Views,
}
#[derive(Default)]
struct Lookup {
    images: HashMap<SampleKey, Weak<StrongHandle>>,
    hits: u64,
    misses: u64,
}
impl SampledImages {
    pub(crate) fn stats(&self) -> (u64, u64) {
        let lookup = self.lookup.lock().unwrap();
        (lookup.hits, lookup.misses)
    }

    #[cfg(test)]
    pub(crate) fn retained_for(
        &self,
        sources: impl Iterator<Item = AssetId<Image>>,
    ) -> Vec<Handle<Image>> {
        let sources: std::collections::HashSet<_> = sources.collect();
        self.lookup
            .lock()
            .unwrap()
            .images
            .iter()
            .filter(|(key, _)| sources.contains(&key.0))
            .filter_map(|(_, image)| image.upgrade())
            .map(Handle::Strong)
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn prune(&mut self) {
        self.lookup
            .lock()
            .unwrap()
            .images
            .retain(|_, image| image.strong_count() > 0);
        self.views
            .0
            .lock()
            .unwrap()
            .retain(|_, view| view.handle.strong_count() > 0);
    }
}
type SampleKey = (AssetId<Image>, u8, u8, bool, bool, bool);

pub(crate) fn sampled_image(
    binding: Option<(Handle<Image>, TextureBinding)>,
    images: &mut Assets<Image>,
    cache: &mut SampledImages,
) -> Option<Handle<Image>> {
    binding.map(|(handle, binding)| {
        let key = (
            handle.id(),
            binding.wrap_u as u8,
            binding.wrap_v as u8,
            binding.nearest_min,
            binding.nearest_mag,
            binding.mipmapped,
        );
        let mut lookup = cache.lookup.lock().unwrap();
        if let Some(cached) = lookup.images.get(&key).and_then(Weak::upgrade) {
            lookup.hits += 1;
            return Handle::Strong(cached);
        }
        lookup.misses += 1;
        lookup.images.retain(|_, image| image.strong_count() > 0);
        // A sampler variant needs metadata and a GPU view, never another copy
        // of the pixels. The render system aliases the original GPU texture.
        let source = images.get(&handle).expect("loaded texture");
        let mut image = Image {
            data: None,
            data_order: source.data_order,
            texture_descriptor: source.texture_descriptor.clone(),
            texture_view_descriptor: source.texture_view_descriptor.clone(),
            asset_usage: RenderAssetUsages::MAIN_WORLD,
            sampler: ImageSampler::Default,
            copy_on_resize: false,
        };
        // Only materials selecting mip filtering may sample authored mip levels.
        let view = image.texture_view_descriptor.get_or_insert_default();
        view.base_mip_level = 0;
        view.mip_level_count = (!binding.mipmapped).then_some(1);
        let wrap = |mode| match mode {
            TextureWrap::Clamp => ImageAddressMode::ClampToEdge,
            TextureWrap::Repeat => ImageAddressMode::Repeat,
            TextureWrap::Mirror => ImageAddressMode::MirrorRepeat,
        };
        let filter = |nearest| {
            if nearest {
                ImageFilterMode::Nearest
            } else {
                ImageFilterMode::Linear
            }
        };
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: wrap(binding.wrap_u),
            address_mode_v: wrap(binding.wrap_v),
            min_filter: filter(binding.nearest_min),
            mag_filter: filter(binding.nearest_mag),
            mipmap_filter: ImageFilterMode::Linear,
            ..ImageSamplerDescriptor::linear()
        });
        let view = image.texture_view_descriptor.clone().unwrap();
        let ImageSampler::Descriptor(sampler) = &image.sampler else {
            unreachable!()
        };
        let sampler = sampler.clone();
        let derived = images.add(image);
        if let Handle::Strong(strong) = &derived {
            lookup.images.insert(key, Arc::downgrade(strong));
            cache.views.0.lock().unwrap().insert(
                derived.id(),
                View {
                    source: handle,
                    handle: Arc::downgrade(strong),
                    view,
                    sampler,
                },
            );
        }
        derived
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        renderer::RenderQueue,
    };

    #[test]
    #[ignore = "requires a GPU; verifies sampler views share the source texture and release retired bindings"]
    fn sampler_views_share_gpu_storage_and_retire_independently() -> anyhow::Result<()> {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::gilrs::GilrsPlugin>(),
        );
        let began = std::time::Instant::now();
        while app.plugins_state() == bevy::app::PluginsState::Adding {
            anyhow::ensure!(began.elapsed().as_secs() < 60, "GPU setup timed out");
            bevy::tasks::tick_global_task_pools_on_main_thread();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app.finish();
        app.cleanup();
        let device = app.world().resource::<RenderDevice>().clone();
        let queue = app.world().resource::<RenderQueue>();
        let mut source = Image::new_fill(
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255, 0, 0, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        source.texture_descriptor.mip_level_count = 2;
        source
            .data
            .as_mut()
            .unwrap()
            .extend_from_slice(&[0, 255, 0, 255]);
        let upload = |image: &Image| {
            let texture = device.create_texture_with_data(
                queue,
                &image.texture_descriptor,
                image.data_order,
                image.data.as_ref().unwrap(),
            );
            GpuImage {
                texture_view: texture.create_view(&default()),
                texture,
                sampler: device.create_sampler(&ImageSamplerDescriptor::linear().as_wgpu()),
                texture_descriptor: image.texture_descriptor.clone(),
                texture_view_descriptor: None,
                had_data: true,
            }
        };
        let first = upload(&source);
        let replacement = upload(&source);
        let mut images = Assets::<Image>::default();
        let source = images.add(source);
        let mut cache = SampledImages::default();
        let binding = TextureBinding {
            texture: 0,
            wrap_u: TextureWrap::Clamp,
            wrap_v: TextureWrap::Clamp,
            nearest_min: false,
            nearest_mag: false,
            mipmapped: false,
            scale: TextureBinding::unscaled(),
        };
        let a = sampled_image(
            Some((source.clone(), binding.clone())),
            &mut images,
            &mut cache,
        )
        .unwrap();
        let second_image = images.get(&source).unwrap().clone();
        let second_upload = upload(&second_image);
        let second_source = images.add(second_image);
        let same_sampler = sampled_image(
            Some((second_source.clone(), binding.clone())),
            &mut images,
            &mut cache,
        )
        .unwrap();
        let b = sampled_image(
            Some((
                source.clone(),
                TextureBinding {
                    wrap_u: TextureWrap::Repeat,
                    mipmapped: true,
                    ..binding
                },
            )),
            &mut images,
            &mut cache,
        )
        .unwrap();
        let mut gpu = RenderAssets::<GpuImage>::default();
        gpu.insert(source.id(), first);
        gpu.insert(second_source.id(), second_upload);
        let mut world = World::new();
        world.insert_resource(cache.views.clone());
        world.insert_resource(gpu);
        world.insert_resource(device);
        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.add_systems(prepare_views);
        schedule.run(&mut world);
        {
            let gpu = world.resource::<RenderAssets<GpuImage>>();
            let source = gpu.get(&source).unwrap();
            let a = gpu.get(&a).unwrap();
            let b = gpu.get(&b).unwrap();
            assert_eq!(source.texture.id(), a.texture.id());
            assert_eq!(source.texture.id(), b.texture.id());
            assert_ne!(a.texture_view.id(), b.texture_view.id());
            assert_ne!(a.sampler.id(), b.sampler.id());
            let other = gpu.get(&same_sampler).unwrap();
            assert_ne!(a.texture.id(), other.texture.id());
            assert_eq!(a.sampler.id(), other.sampler.id());
            assert_eq!(
                a.texture_view_descriptor.as_ref().unwrap().mip_level_count,
                Some(1)
            );
            assert_eq!(
                b.texture_view_descriptor.as_ref().unwrap().mip_level_count,
                None
            );
        }
        world
            .resource_mut::<RenderAssets<GpuImage>>()
            .insert(source.id(), replacement);
        schedule.run(&mut world);
        assert_eq!(
            world
                .resource::<RenderAssets<GpuImage>>()
                .get(&source)
                .unwrap()
                .texture
                .id(),
            world
                .resource::<RenderAssets<GpuImage>>()
                .get(&b)
                .unwrap()
                .texture
                .id()
        );
        let retired = a.id();
        drop(a);
        drop(same_sampler);
        schedule.run(&mut world);
        assert!(
            world
                .resource::<RenderAssets<GpuImage>>()
                .get(retired)
                .is_none()
        );
        assert!(world.resource::<RenderAssets<GpuImage>>().get(&b).is_some());
        assert!(
            world
                .resource::<RenderAssets<GpuImage>>()
                .get(&source)
                .is_some()
        );
        let views = cache.views.0.lock().unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views.get(&b.id()).unwrap().source.id(), source.id());
        Ok(())
    }
}
