//! World landmark models, skies and footstep artwork.
use super::*;
use symphonia_constants::{SylvarantMarkerId, TetheAllaMarkerId};

fn model(source: &Source, library: &Path, resource: &str, owner: &str, sky: bool) -> Result<()> {
    let bytes = source.require(resource)?;
    let package = Package::parse(&bytes)?;
    ensure!(
        matches!(package.len(), 1 | 19),
        "unexpected world visual package"
    );
    let motions = if package.len() == 19 {
        package
            .reference(2)?
            .map(|name| vec![(0, name.to_owned())])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let body = package.reference(0)?.context("world visual has no body")?;
    let (mut part, _) = preview_model::part(source, library, body, owner, "body", &motions)?;
    part.autoplay = !part.clips.is_empty();
    if sky {
        for material in &mut part.materials {
            material.depth_write = false;
        }
    }
    output::json(
        &library.join(owner).join("model.json"),
        &document::Model { parts: vec![part] },
    )
}

pub(super) fn cook(source: &Source, library: &Path, image: &pe::Image<'_>) -> Result<()> {
    let mut mappings = BTreeMap::new();
    for (world, (name, address)) in [("sylvarant", 0x86ff6c), ("tethe-alla", 0x86ff74)]
        .into_iter()
        .enumerate()
    {
        let bytes = source.require(image.text(address)?)?;
        let directory = Package::parse(&bytes)?;
        ensure!(directory.len() == 17, "world marker directory changed");
        for slot in 0..directory.len() {
            let Some(resource) = directory.reference(slot)? else {
                continue;
            };
            let id = slot as u8 + 1;
            let marker = if world == 0 {
                SylvarantMarkerId::try_from(id)
                    .ok()
                    .and_then(SylvarantMarkerId::resource_name)
            } else {
                TetheAllaMarkerId::try_from(id)
                    .ok()
                    .and_then(TetheAllaMarkerId::resource_name)
            }
            .map(str::to_owned)
            .unwrap_or_else(|| format!("marker-{id:03}"));
            let target = format!("{name}/{marker}");
            model(
                source,
                library,
                resource,
                &format!("world/markers/{target}"),
                false,
            )?;
            mappings.insert(world as u32 * 256 + u32::from(id), target);
        }
    }
    output::json(&library.join(document::MARKERS), &Mappings::new(mappings)?)?;
    for (name, address) in [("normal", 0x86fee4), ("late", 0x86fed8)] {
        model(
            source,
            library,
            image.text(address)?,
            &format!("world/skies/{name}"),
            true,
        )?;
    }
    for index in 0..4 {
        crate::ui_images::cook(
            source,
            library,
            "SAND_PIC000.TOTEXB_D",
            &format!("world/effects/dust-{index}.ktx2"),
            false,
        )?;
    }
    eprintln!("  World markers/skies and shared footstep artwork cooked");
    Ok(())
}
