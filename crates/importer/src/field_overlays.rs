//! Five reusable field overlay images that are not part of the named UI art.
use crate::{dol, field_actors, ui_controls, world_texture};
use anyhow::{Result, ensure};
use serde_json::json;
use std::{fs, path::Path};

fn write(library: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let target = library.join(path);
    if target.is_file() {
        ensure!(
            fs::read(target)? == bytes,
            "conflicting overlay artwork {path}"
        );
    } else {
        crate::output::write(&target, bytes)?;
    }
    Ok(())
}

fn image(library: &Path, name: &str, source: &[u8], repeat: bool) -> Result<()> {
    ensure!(
        dol::u32(source, 0)? == 0x0020_af30,
        "{name} is not a native TPL"
    );
    ensure!(
        dol::u32(source, 4)? == 1,
        "{name} is not a single overlay image"
    );
    let (bytes, _) = world_texture::tpl_texture(source, 0)?;
    let path = format!("effects/overlays/{name}.ktx2");
    write(library, &path, &bytes)?;
    if repeat {
        let document = json!({"entries":[path],"sampler":{"wrap":["repeat","repeat"]}});
        write(
            library,
            &format!("effects/overlays/{name}.json"),
            &resonance_asset_writer::json::encode(&document)?,
        )?;
    }
    Ok(())
}

pub fn cook_field_overlay_media(extracted: &Path, library: &Path) -> Result<()> {
    for (id, name, repeat) in [(68611, "radial-burst", false), (68614, "ornate-ring", true)] {
        let native = field_actors::read_shared_resource(extracted, id)?;
        image(library, name, &native, repeat)?;
    }
    let flames = ui_controls::field_archive(extracted, 171)?;
    image(
        library,
        "flames",
        &ui_controls::local_texture(&flames, 23)?,
        true,
    )?;
    let gold = ui_controls::field_archive(extracted, 511)?;
    for (section, name, repeat) in [(17, "gold-glow", false), (22, "gold-light-strip", true)] {
        image(
            library,
            name,
            &ui_controls::local_texture(&gold, section)?,
            repeat,
        )?;
    }
    Ok(())
}
