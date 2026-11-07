//! Global and local picture resources use the ordinary shared overlay texture catalogue.
use crate::{output, package::Package, source::Source, ui_images};
use anyhow::{Result, ensure};
use resonance_content::{TextureWrap, effect::OverlayArt, physical_scene::TextureImage};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(
    source: &Source,
    library: &Path,
    package: &Package<'_>,
    field: u32,
    root: &str,
) -> Result<BTreeMap<i32, String>> {
    let mut overlays = shared(source, library, package)?;
    for slot in 16..package.len() {
        let Some(name) = package.reference(slot)? else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let names = if lower.ends_with(".totexb_d") {
            vec![Some(name.to_owned())]
        } else if lower.ends_with(".pac") {
            let bytes = source.require(name)?;
            let bank = Package::parse(&bytes)?;
            let names = (0..bank.len())
                .map(|index| Ok(bank.reference(index)?.map(str::to_owned)))
                .collect::<Result<Vec<_>>>()?;
            if !names
                .iter()
                .flatten()
                .any(|name| name.to_ascii_lowercase().ends_with(".totexb_d"))
                || !names
                    .iter()
                    .flatten()
                    .all(|name| name.to_ascii_lowercase().ends_with(".totexb_d"))
            {
                continue;
            }
            names
        } else {
            continue;
        };
        let resource = 0xffee0000u32 + (slot - 16) as u32;
        let owner = format!("{root}/overlays/local-{:02}", slot - 16);
        let mut art = OverlayArt::default();
        for (index, name) in names.into_iter().enumerate() {
            let Some(name) = name else {
                art.textures.push(None);
                continue;
            };
            let path = format!("{owner}/texture-{index}.ktx2");
            let image = ui_images::cook(source, library, &name, &path, true)?;
            let mut sampler = OverlayArt::default_sampler();
            if (field == 171 && slot == 23) || ((511..=518).contains(&field) && slot == 22) {
                sampler.wrap = [TextureWrap::Repeat; 2];
            }
            art.textures.push(Some(TextureImage {
                sampler,
                images: vec![path],
                size: Some([image.width, image.height]),
            }));
        }
        art.validate()?;
        let path = format!("{owner}/overlay.json");
        output::json(&library.join(&path), &art)?;
        overlays.insert(resource as i32, path);
    }
    // Steam omits the old night-title lettering texture but still creates its
    // overlay handle. The shared startup UI owns that lettering in both imports.
    if field == 531 {
        let resource = 0xffee0003u32 as i32;
        ensure!(
            !overlays.contains_key(&resource),
            "night title menu unexpectedly has artwork"
        );
        let path = format!("{root}/overlays/local-03/transparent.ktx2");
        output::write(
            &library.join(&path),
            &resonance_asset_writer::ktx2::encode_rgba8(1, 1, &[0; 4])?,
        )?;
        overlays.insert(resource, path);
    }
    Ok(overlays)
}

/// Only the proven global picture is selected, using its original requested ID.
/// The ordinary scenario decoder and resource directory retain source identity.
fn shared(source: &Source, library: &Path, package: &Package<'_>) -> Result<BTreeMap<i32, String>> {
    let Some(native) = package.reference(6)? else {
        return Ok(BTreeMap::new());
    };
    let (bytes, encoding) = source.scenario(native)?;
    let bytes = crate::script::normalize(&bytes, encoding)?;
    let (declared, _) = crate::field_actors::declarations(&bytes)?;
    let mut overlays = BTreeMap::new();
    if declared.contains(&68611) {
        let resources = crate::resources::Resources::read(source)?;
        let name = resources.loaded(68611)?;
        ensure!(
            name.eq_ignore_ascii_case("d_PIC019_PIC000.TOTEXB_D"),
            "shared radial-burst source changed"
        );
        let path = "effects/overlays/radial-burst.ktx2";
        ui_images::cook(source, library, name, path, true)?;
        overlays.insert(68611, path.into());
    }
    Ok(overlays)
}
