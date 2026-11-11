//! Five reusable field overlay images that are not part of the named UI art.
use crate::{dol, field_actors, ui_controls, world_texture};
use anyhow::{Result, ensure};
use resonance_content::{effect::OverlayArt, physical_scene::TextureImage};
use std::path::Path;

/// An overlay's coordinate space stays independent of its stored image size.
pub(crate) fn image(path: impl Into<String>, size: [u32; 2]) -> TextureImage {
    TextureImage {
        sampler: OverlayArt::default_sampler(),
        images: vec![path.into()],
        size: Some(size),
    }
}

/// A texture and the one-image catalogue beside it, as library files.
pub(crate) fn image_files(texture: TextureImage, ktx: Vec<u8>) -> Result<[(String, Vec<u8>); 2]> {
    let path = texture.images[0].clone();
    let document = OverlayArt {
        textures: vec![Some(texture)],
    };
    Ok([
        (
            path.replace(".ktx2", ".json"),
            resonance_asset_writer::json::encode(&document)?,
        ),
        (path, ktx),
    ])
}

pub(crate) fn overlay_files(
    name: &str,
    source: &[u8],
    repeat: bool,
) -> Result<[(String, Vec<u8>); 2]> {
    ensure!(
        dol::u32(source, 0)? == 0x0020_af30,
        "{name} is not a native TPL"
    );
    ensure!(
        dol::u32(source, 4)? == 1,
        "{name} is not a single overlay image"
    );
    let mut texture = image(format!("effects/overlays/{name}.ktx2"), [0; 2]);
    if repeat {
        texture.sampler.wrap = [resonance_content::TextureWrap::Repeat; 2];
    }
    let (ktx, size) = world_texture::tpl_interface_texture(source, 0, texture.sampler.wrap)?;
    texture.size = Some(size);
    image_files(texture, ktx)
}

fn cook_image(library: &Path, name: &str, source: &[u8], repeat: bool) -> Result<()> {
    for (path, bytes) in overlay_files(name, source, repeat)? {
        crate::output::write(&library.join(path), &bytes)?;
    }
    Ok(())
}

pub fn cook_field_overlay_media(extracted: &Path, library: &Path) -> Result<()> {
    for (id, name, repeat) in [(68611, "radial-burst", false), (68614, "ornate-ring", true)] {
        let native = field_actors::read_shared_resource(extracted, id)?;
        cook_image(library, name, &native, repeat)?;
    }
    let flames = ui_controls::field_archive(extracted, 171)?;
    cook_image(
        library,
        "flames",
        &ui_controls::local_texture(&flames, 23)?,
        true,
    )?;
    let gold = ui_controls::field_archive(extracted, 511)?;
    for (section, name, repeat) in [(17, "gold-glow", false), (22, "gold-light-strip", true)] {
        cook_image(
            library,
            name,
            &ui_controls::local_texture(&gold, section)?,
            repeat,
        )?;
    }
    Ok(())
}
