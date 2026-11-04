//! Global and local picture resources use the ordinary shared overlay texture catalogue.
use crate::{output, package::Package, source::Source, ui_images};
use anyhow::{Context, Result, ensure};
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
            ui_images::cook(source, library, &name, &path, true)?;
            let mut sampler = OverlayArt::default_sampler();
            if (field == 171 && slot == 23) || ((511..=518).contains(&field) && slot == 22) {
                sampler.wrap = [TextureWrap::Repeat; 2];
            }
            art.textures.push(Some(TextureImage {
                sampler,
                images: vec![path],
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

/// Test-only targeted publication changes overlay bindings without recooking scenes.
#[cfg(test)]
fn publish_story_overlays(source: &Source, library: &Path, fields: &[u32]) -> Result<()> {
    use resonance_content::library::{FIELDS, Mappings};
    let mappings: Mappings = serde_json::from_slice(&std::fs::read(library.join(FIELDS))?)?;
    // Resolve every requested field before producing any output.
    let selected = fields
        .iter()
        .map(|&field| Ok((field, mappings.target(field)?.to_owned())))
        .collect::<Result<Vec<_>>>()?;
    let image = crate::pe::Image::parse(&source.executable)?;
    for (field, name) in selected {
        let pointer = crate::pe::word(image.slice(0x8cb908 + field * 24, 4)?, 0)?;
        let bytes = source.require(image.text(pointer)?)?;
        let package = Package::parse(&bytes)?;
        let overlays = shared(source, library, &package)?;
        if overlays.is_empty() {
            eprintln!(
                "  Story overlay: field {field} does not declare a supported shared picture; skipped"
            );
            continue;
        }
        let path = library.join("fields").join(name).join("scene.json");
        // Preserve all existing fields, including cinematic clip declarations,
        // exactly as JSON values instead of rebuilding a scenery document.
        let mut scene: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        let object = scene
            .as_object_mut()
            .context("field scene is not an object")?;
        let bindings = object
            .entry("overlays")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("field overlay bindings are not an object")?;
        let mut changed = false;
        for (resource, target) in overlays {
            let key = resource.to_string();
            let value = serde_json::Value::String(target);
            if let Some(existing) = bindings.get(&key) {
                ensure!(
                    *existing == value,
                    "field {field} shared overlay {resource} conflicts with an existing binding"
                );
            } else {
                bindings.insert(key, value);
                changed = true;
            }
        }
        if changed {
            output::json(&path, &scene)?;
        }
        eprintln!("  Story overlay: field {field}, new binding {changed}");
    }
    Ok(())
}

#[cfg(test)]
fn story_overlay_fields(selection: Option<&str>) -> Result<Vec<u32>> {
    let fields = selection
        .unwrap_or("535")
        .split(',')
        .map(|item| {
            item.trim()
                .parse::<u32>()
                .context("invalid RESONANCE_STORY_OVERLAY_FIELDS field ID")
        })
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    ensure!(!fields.is_empty(), "empty story overlay field selection");
    Ok(fields.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::field::{
        DrawStage,
        document::{JOINT_MAPPINGS, Scene},
    };
    use std::{fs, path::PathBuf};

    #[test]
    fn story_overlay_selection_is_explicit_and_deduplicated() -> Result<()> {
        assert_eq!(story_overlay_fields(None)?, [535]);
        assert_eq!(story_overlay_fields(Some("535, 6,535"))?, [6, 535]);
        assert!(story_overlay_fields(Some("535,wrong")).is_err());
        assert!(story_overlay_fields(Some("")).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; optional RESONANCE_SALVATION_OUTPUT publishes selected RESONANCE_STORY_OVERLAY_FIELDS (default535)"]
    fn installed_salvation_overlay_and_scenery_aliases_use_existing_field_contracts() -> Result<()>
    {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let temporary = tempfile::tempdir()?;
        crate::field_directory::cook(&source, temporary.path())?;
        crate::field_scene::cook(&source, temporary.path(), Some(535))?;
        crate::field_joints::cook(&source, temporary.path())?;
        let scene_path = "fields/tower-of-salvation-0535/scene.json";
        let scene: Scene = serde_json::from_slice(&fs::read(temporary.path().join(scene_path))?)?;
        let overlay = &scene.overlays[&68611];
        let image =
            resonance_asset_writer::texture::ktx2(&fs::read(temporary.path().join(overlay))?)?;
        let native = ui_images::read(&source, "d_PIC019_PIC000.TOTEXB_D")?;
        let expected = image::imageops::resize(
            &native,
            native.width() / 2,
            native.height() / 2,
            image::imageops::FilterType::Triangle,
        );
        assert_eq!(image[0].rgba, expected.into_raw());
        let aliases: BTreeMap<String, String> =
            serde_json::from_slice(&fs::read(temporary.path().join(JOINT_MAPPINGS))?)?;
        for (resource, labels) in [(0, ["step", "step_br"]), (2, ["yuka", "yuka_br"])] {
            let mut part = scene
                .parts
                .iter()
                .find(|part| part.resource == resource)
                .context("missing scenery part")?
                .clone();
            part.prepare_bones(&fs::read(temporary.path().join(&part.mesh))?)?;
            for label in labels {
                let target = &aliases[label];
                assert_eq!(
                    part.bone_names
                        .iter()
                        .filter(|name| *name == target)
                        .count(),
                    1
                );
            }
            assert!(DrawStage::scenery(resource).is_some());
        }
        let original = fs::read(temporary.path().join(scene_path))?;
        publish_story_overlays(&source, temporary.path(), &[340, 535])?;
        assert_eq!(fs::read(temporary.path().join(scene_path))?, original);
        let mut missing: serde_json::Value = serde_json::from_slice(&original)?;
        missing["overlays"].as_object_mut().unwrap().remove("68611");
        missing["cinematic_preservation_probe"] =
            serde_json::json!({"clips":[{"motion":"unchanged.glb","banks":[123]}]});
        output::json(&temporary.path().join(scene_path), &missing)?;
        publish_story_overlays(&source, temporary.path(), &[340, 535])?;
        let merged: serde_json::Value =
            serde_json::from_slice(&fs::read(temporary.path().join(scene_path))?)?;
        missing["overlays"]["68611"] = serde_json::json!(overlay);
        assert_eq!(merged, missing);
        assert!(
            !temporary
                .path()
                .join("fields/iselia-0340/scene.json")
                .exists()
        );
        let baseline: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../../importer/src/model-joints.json"))?;
        assert_eq!(aliases.len(), baseline.len() + 4);
        for (key, value) in baseline {
            assert_eq!(aliases[&key], value);
        }
        if let Some(output) = std::env::var_os("RESONANCE_SALVATION_OUTPUT") {
            let selection = std::env::var("RESONANCE_STORY_OVERLAY_FIELDS").ok();
            let fields = story_overlay_fields(selection.as_deref())?;
            publish_story_overlays(&source, Path::new(&output), &fields)?;
            if fields.contains(&535) {
                crate::field_joints::cook(&source, Path::new(&output))?;
            }
        }
        Ok(())
    }
}
