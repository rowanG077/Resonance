use super::*;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
};
use resonance_asset_writer::texture::Pixels;
use resonance_content::texture::{Mips, VERSION};

#[test]
fn ui_and_scene_preparation_preserve_authored_levels_color_and_sampler() -> Result<()> {
    let definition = Definition {
        version: VERSION,
        image: "wood.png".into(),
        color_space: ColorSpace::Srgb,
        mips: Mips::Authored {
            levels: vec!["wood-mip-1.png".into()],
        },
    };
    let base = Pixels {
        width: 2,
        height: 2,
        rgba: vec![37; 16],
    };
    let mip = Pixels {
        width: 1,
        height: 1,
        rgba: vec![17, 31, 59, 127],
    };
    let files: BTreeMap<String, Vec<u8>> = [
        ("wood.texture.json".into(), serde_json::to_vec(&definition)?),
        ("wood.png".into(), base.png()?),
        ("wood-mip-1.png".into(), mip.png()?),
    ]
    .into();
    let settings = ImageLoaderSettings {
        is_srgb: false, // The explicit descriptor owns color interpretation.
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::nearest()
        }),
        asset_usage: RenderAssetUsages::MAIN_WORLD,
        ..default()
    };
    let image = prepare(&definition, &settings, |path| {
        files.get(path).context("missing fixture")
    })?;
    assert_eq!(
        image.texture_descriptor.format,
        TextureFormat::Rgba8UnormSrgb
    );
    assert_eq!(image.texture_descriptor.mip_level_count, 2);
    assert_eq!([image.width(), image.height()], [2, 2]);
    assert_eq!(
        image.data.as_ref().unwrap(),
        &[base.rgba, mip.rgba].concat()
    );
    assert!(matches!(&image.sampler, ImageSampler::Descriptor(sampler)
        if sampler.address_mode_u == ImageAddressMode::Repeat));
    let ui = decode("wood.texture.json", &settings, &|path| {
        files.get(path).cloned().context("missing fixture")
    })?;
    assert_eq!(ui.data, image.data);
    assert_eq!(
        ui.texture_descriptor.mip_level_count,
        image.texture_descriptor.mip_level_count
    );
    let mut invalid = settings.clone();
    invalid.texture_format = Some(TextureFormat::R8Unorm);
    assert!(
        prepare(&definition, &invalid, |path| files
            .get(path)
            .context("missing fixture"))
        .is_err()
    );
    Ok(())
}

#[test]
fn descriptor_loader_keeps_nested_pngs_in_the_selected_asset_source() -> Result<()> {
    use bevy::asset::{
        AssetPlugin, AssetServer, LoadState,
        io::{
            AssetSourceBuilder,
            memory::{Dir, MemoryAssetReader},
        },
    };
    use std::{
        path::Path,
        time::{Duration, Instant},
    };
    let directory = Dir::default();
    directory.insert_asset_text(
        Path::new("wood.texture.json"),
        r#"{"version":1,"image":"wood.png","color_space":"linear","mips":{"kind":"generate"}}"#,
    );
    directory.insert_asset(
        Path::new("wood.png"),
        Pixels {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
        }
        .png()?,
    );
    let application = |directory: Dir| {
        let mut app = App::new();
        app.register_asset_source(
            "texture-fixture",
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: directory.clone(),
                })
            }),
        );
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Image>();
        install(&mut app);
        app
    };
    let load = |app: &mut App| -> Result<Handle<Image>> {
        let handle: Handle<Image> = app
            .world()
            .resource::<AssetServer>()
            .load("texture-fixture://wood.texture.json");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.update();
            if app
                .world()
                .resource::<Assets<Image>>()
                .contains(handle.id())
            {
                return Ok(handle);
            }
            if let Some(LoadState::Failed(error)) = app
                .world()
                .resource::<AssetServer>()
                .get_load_state(handle.id())
            {
                anyhow::bail!("texture fixture failed: {error}");
            }
            ensure!(Instant::now() < deadline, "texture fixture load timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let mut app = application(directory.clone());
    let handle = load(&mut app)?;
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&handle)
        .unwrap();
    assert_eq!(image.texture_descriptor.mip_level_count, 2);
    assert_eq!(&image.data.as_ref().unwrap()[8..], [128, 128, 128, 255]);
    assert_eq!(
        load(&mut app)?.id(),
        handle.id(),
        "unchanged warm loads share the image"
    );
    drop(app);

    // The descriptor and path stay identical. Restarting must prepare the edited
    // PNG and generated mips, with no author-maintained cache key or checksum.
    directory.insert_asset(
        Path::new("wood.png"),
        Pixels {
            width: 4,
            height: 2,
            rgba: [37, 83, 211, 255].repeat(8),
        }
        .png()?,
    );
    let mut restarted = application(directory);
    let handle = load(&mut restarted)?;
    let image = restarted
        .world()
        .resource::<Assets<Image>>()
        .get(&handle)
        .unwrap();
    assert_eq!([image.width(), image.height()], [4, 2]);
    assert_eq!(image.texture_descriptor.mip_level_count, 3);
    assert_eq!(image.data.as_ref().unwrap(), &[37, 83, 211, 255].repeat(11));
    Ok(())
}
