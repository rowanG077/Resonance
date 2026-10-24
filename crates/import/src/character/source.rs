//! Actor-specific appearance and clip bindings on the shared source-model DAG.
use super::{Clip, appearance};
use crate::{
    all_assets::geometry::{is_model, model_selectors},
    model_preview::{bind_clips, style},
    scene::source::Models,
};
use anyhow::{Context, Result};
use resonance_content::ScenePart;
use std::path::Path;

fn layers(package: &[u8]) -> Result<Vec<&[u8]>> {
    let sections = if is_model(package) {
        vec![Some(0..package.len())]
    } else {
        crate::field::sections(package)?
    };
    sections
        .first()
        .and_then(Option::as_ref)
        .context("missing actor primary layer")?;
    Ok(sections
        .into_iter()
        .take(2)
        .flatten()
        .map(|range| &package[range])
        .collect())
}

pub(super) fn unbound(
    output: &Path,
    package: &[u8],
    decoded: &crate::scene::decoded::Package,
) -> Result<Option<Vec<crate::named::PhysicalScene>>> {
    let layers = layers(package)?;
    let models = layers
        .iter()
        .map(|layer| {
            let normalized = super::texture_palette(layers[0], layer)?;
            decoded
                .get(&normalized)
                .context("field geometry was not supplied by its decode job")
        })
        .collect::<Result<Vec<_>>>()?;
    if !models.iter().any(|model| model.requires_palette()) {
        return Ok(None);
    }
    Ok(Some(
        models
            .into_iter()
            .map(|model| model.prepare_scene(output))
            .collect::<Result<_>>()?,
    ))
}

pub(crate) fn cook_parts(
    output: &Path,
    name: &str,
    package: &[u8],
    clips: &[Clip<'_>],
    decoded: &crate::scene::decoded::Package,
) -> Result<Vec<ScenePart>> {
    let layers = layers(package)?;
    let primary = layers[0];
    let mut models = Models::new(output, decoded);
    for (index, original) in layers.into_iter().enumerate() {
        models.add(
            &format!("{name}/{index}"),
            original,
            primary,
            move |geometry, textures, part, glb| {
                part.resource = index.try_into()?;
                style(part, index != 0, false);
                if !clips.is_empty() {
                    bind_clips(
                        part,
                        glb,
                        geometry
                            .bindings
                            .as_ref()
                            .context("animated model lacks motion bindings")?,
                        None,
                        clips,
                    )?;
                }
                let selectors = model_selectors(original, textures.textures.len().try_into()?)?
                    .map(|index| index.map(usize::from));
                part.appearance = Some(appearance(index, selectors, textures)?);
                Ok(())
            },
        )?;
    }
    Ok(models
        .finish()
        .into_iter()
        .map(|layer| layer.part)
        .collect())
}

#[test]
#[ignore = "requires original disc 1; compares native preparation with current named model files"]
fn original_party_geometry_dag_never_reopens_cooked_intermediates() -> Result<()> {
    use crate::{all_assets::geometry, field::sections, resource::PartyResource};
    use std::fs;
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let files = extracted.join("files");
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let resources = crate::resource::read(&executable)?;
    let source_path = |path: &str| -> Result<_> {
        Ok(files.join(crate::field_resources::resolve_path(&files, path)?))
    };
    for id in [1, 3] {
        let bytes = fs::read(source_path(resources.party(PartyResource::Body, id, 0)?)?)?;
        // Field motion banks are raw offset directories; battle banks use cabinets.
        let bank = fs::read(source_path(resources.field_motion(id)?)?)?;
        let (slot, range) = sections(&bank)?
            .iter()
            .enumerate()
            .skip(2)
            .find_map(|(index, section)| section.as_ref().map(|range| (index, range.clone())))
            .context("missing representative party animation")?;
        let clip = super::decode_clip(&bank[range])?;
        let actual_root = tempfile::tempdir()?;
        let mut decoded = crate::scene::decoded::Package::default();
        for (name, bytes) in [("body", bytes.as_slice()), ("motion", bank.as_slice())] {
            let mut failures = Vec::new();
            assert!(geometry::cook(
                bytes,
                name,
                actual_root.path(),
                None,
                geometry::Input::File,
                &mut decoded,
                &mut |path, result| {
                    if let Err(error) = result {
                        failures.push(format!("{path}: {error:#}"));
                    }
                }
            ));
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }
        let authored = decoded.animation(&clip)?;
        assert!(std::sync::Arc::ptr_eq(
            &authored,
            &decoded.animation(&clip)?
        ));
        let clips = [Clip {
            slot: (4 + slot * 4).try_into()?,
            resource: None,
            animation: &authored,
        }];
        // Native binding must work after all physical metadata is removed.
        // Mesh/texture payloads remain converter scratch owned by the decode DAG.
        fs::remove_dir_all(actual_root.path().join("body"))?;
        fs::remove_dir_all(actual_root.path().join("motion"))?;
        let actual = cook_parts(actual_root.path(), "party", &bytes, &clips, &decoded)?;
        let output = tempfile::tempdir()?;
        let mut publisher = crate::named::Publisher::new(actual_root.path(), output.path())?;
        let model_path = publisher.model(
            "models/party",
            resonance_content::field::document::Model {
                version: resonance_content::field::document::VERSION,
                parts: actual.clone(),
                collision: super::collision(&bytes)?,
            },
        )?;
        publisher.descriptor("GQSEAF", 0)?;
        let mut snapshot =
            resonance_content::library::Library::open(output.path(), &[])?.snapshot();
        let mut current: resonance_content::field::document::Model =
            snapshot.json(&model_path, 64 * 1024 * 1024)?;
        current.validate()?;
        for part in &mut current.parts {
            snapshot.read(&part.mesh, 128 * 1024 * 1024)?;
            for texture in &part.textures {
                snapshot.read(texture, 64 * 1024 * 1024)?;
            }
            for clip in &mut part.clips {
                let motion = resonance_content::animation::Motion::decode(
                    &snapshot.read(&clip.motion, 64 * 1024 * 1024)?,
                )?;
                motion.validate_bones(part.bone_names.len())?;
                clip.duration_seconds =
                    motion.duration_frames / resonance_content::animation::FRAME_HZ;
            }
        }
        crate::named::equivalent_assets(
            &actual,
            &current.parts,
            actual_root.path(),
            output.path(),
            &mut Default::default(),
        )?;
        assert_eq!(
            serde_json::to_value(&current.collision)?,
            serde_json::to_value(super::collision(&bytes)?)?
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires original disc 1; checks resources whose palette is supplied by native callers"]
fn original_caller_palettes_remain_explicit_without_discarding_geometry() -> Result<()> {
    use crate::all_assets::{geometry, physical_scene::TextureSource};
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let output = tempfile::tempdir()?;
    for id in [125, 492, 493, 494, 495, 496, 498, 499, 516, 518] {
        let archive =
            crate::field::MapArchive::open(&crate::field::source_for_id(&extracted, id)?)?;
        let package = archive.section(16)?;
        let mut decoded = crate::scene::decoded::Package::default();
        let mut failures = Vec::new();
        assert!(geometry::cook(
            package,
            "source",
            output.path(),
            None,
            geometry::Input::Member,
            &mut decoded,
            &mut |path, result| {
                if let Err(error) = result {
                    failures.push(format!("{path}: {error:#}"));
                }
            }
        ));
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        let geometry = unbound(output.path(), package, &decoded)?
            .context("caller palette requirement was lost")?;
        assert_eq!(geometry.len(), 1);
        let scene = &geometry[0].scene;
        assert!(matches!(scene.textures, TextureSource::Caller));
        assert!(
            scene
                .draws
                .iter()
                .any(|draw| draw.mesh.is_some() && draw.recipe.textures().next().is_some())
        );
        assert!(output.path().join(&scene.mesh).is_file());
        assert!(geometry[0].textures.is_none());
    }
    Ok(())
}
