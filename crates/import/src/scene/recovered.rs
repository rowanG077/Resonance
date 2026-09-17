//! Immutable package outputs shared by physical publication and scene binding jobs.
use crate::{geometry::DecodedGeometry, texture::Decoded};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn animation(
    bytes: &[u8],
    recovered: Option<&RecoveredModels>,
) -> anyhow::Result<Arc<crate::animation::AuthoredAnimation>> {
    match recovered {
        Some(recovered) => recovered.animation(bytes),
        None => crate::animation::read_member(bytes).map(Arc::new),
    }
}

pub(crate) fn textures(
    bytes: &[u8],
    recovered: Option<&RecoveredModels>,
) -> anyhow::Result<Arc<Decoded>> {
    match recovered {
        Some(recovered) => recovered.textures(bytes),
        None => crate::texture::decode_source(bytes).map(Arc::new),
    }
}

#[derive(Clone)]
pub(crate) struct Model {
    pub geometry: Arc<DecodedGeometry>,
    pub textures: Arc<Decoded>,
    pub mesh: Option<crate::publication::File>,
}

impl Model {
    pub(crate) fn requires_palette(&self) -> bool {
        matches!(
            self.geometry.scene.textures,
            crate::all_assets::physical_scene::TextureSource::Caller
        ) && self
            .geometry
            .scene
            .draws
            .iter()
            .any(|draw| draw.mesh.is_some() && draw.recipe.textures().next().is_some())
    }

    /// Retain the physical recipe without inventing the caller's material bindings.
    pub(crate) fn publish_scene(
        &self,
        output: &std::path::Path,
        files: &mut std::collections::BTreeSet<String>,
    ) -> anyhow::Result<String> {
        use anyhow::Context;
        let mut scene = self.geometry.scene.clone();
        self.mesh
            .as_ref()
            .context("physical mesh publication is missing")?
            .share(&output.join(&scene.mesh))?;
        files.insert(scene.mesh.clone());
        files.extend(crate::all_assets::physical_scene::publish_nodes(
            output,
            &scene.bone_names,
        )?);
        if let crate::all_assets::physical_scene::TextureSource::Local { catalogue } =
            &mut scene.textures
        {
            self.textures.validate()?;
            let mut result = Ok(());
            self.textures.publish(output, |_, published| {
                if result.is_ok() {
                    result = published;
                }
            });
            result?;
            *catalogue = format!("textures/{}/textures.json", self.textures.source_sha256);
            crate::write_atomic(
                &output.join(&*catalogue),
                &serde_json::to_vec(&self.textures.catalogue)?,
            )?;
            files.insert(catalogue.clone());
            files.extend(
                self.textures
                    .catalogue
                    .textures
                    .iter()
                    .flatten()
                    .flat_map(|texture| texture.images.iter().cloned()),
            );
        }
        let bytes = serde_json::to_vec(&scene)?;
        let path = format!("geometry/{}.json", crate::digest(&bytes));
        crate::write_atomic(&output.join(&path), &bytes)?;
        files.insert(path.clone());
        Ok(path)
    }
}

#[derive(Default)]
pub(crate) struct RecoveredModels {
    models: BTreeMap<String, Model>,
    palettes: BTreeMap<String, Arc<Decoded>>,
    animations: BTreeMap<String, Arc<crate::animation::AuthoredAnimation>>,
    sources: BTreeMap<String, Arc<Vec<u8>>>,
}

impl RecoveredModels {
    /// Dependency values share their buffers; the DAG controls their lifetime and admission.
    pub(crate) fn extend(&mut self, other: &Self) {
        self.models.extend(other.models.clone());
        self.palettes.extend(other.palettes.clone());
        self.animations.extend(other.animations.clone());
        self.sources.extend(other.sources.clone());
    }

    pub(crate) fn remember_source(&mut self, relative: &str, bytes: Arc<Vec<u8>>) {
        self.sources.insert(relative.to_ascii_lowercase(), bytes);
    }

    pub(crate) fn source(&self, relative: &str) -> anyhow::Result<Arc<Vec<u8>>> {
        use anyhow::Context;
        self.sources
            .get(&relative.to_ascii_lowercase())
            .cloned()
            .with_context(|| format!("source {relative} was not supplied by its read job"))
    }

    pub(crate) fn decode_animation(
        &mut self,
        bytes: &[u8],
        decode: impl FnOnce() -> anyhow::Result<crate::animation::AuthoredAnimation>,
    ) -> anyhow::Result<Arc<crate::animation::AuthoredAnimation>> {
        let key = crate::digest(bytes);
        if let Some(animation) = self.animations.get(&key) {
            return Ok(Arc::clone(animation));
        }
        let animation = Arc::new(decode()?);
        self.animations.insert(key, Arc::clone(&animation));
        Ok(animation)
    }

    pub(crate) fn animation(
        &self,
        bytes: &[u8],
    ) -> anyhow::Result<Arc<crate::animation::AuthoredAnimation>> {
        use anyhow::Context;
        let bytes = crate::compression::payload(bytes.to_vec())?;
        self.animations
            .get(&crate::digest(&bytes))
            .cloned()
            .context("animation was not supplied by its decode job")
    }

    pub(crate) fn remember(
        &mut self,
        normalized: &[u8],
        geometry: crate::geometry::DecodedGeometry,
        textures: Arc<Decoded>,
        mesh: Option<crate::publication::File>,
    ) {
        let key = crate::digest(normalized);
        if self.models.contains_key(&key) {
            return;
        }
        let textures = self.remember_textures(textures);
        self.models.insert(
            key,
            Model {
                geometry: Arc::new(geometry),
                textures,
                mesh,
            },
        );
    }

    pub(crate) fn get(&self, normalized: &[u8]) -> Option<&Model> {
        // Normalization embeds the selected external palette. This map only
        // contains the ordinary physical geometry interpretation, before binding.
        self.models.get(&crate::digest(normalized))
    }

    pub(crate) fn remember_textures(&mut self, textures: Arc<Decoded>) -> Arc<Decoded> {
        Arc::clone(
            self.palettes
                .entry(textures.source_sha256.clone())
                .or_insert(textures),
        )
    }

    pub(crate) fn textures(&self, bytes: &[u8]) -> anyhow::Result<Arc<Decoded>> {
        use anyhow::Context;
        self.palettes
            .get(&crate::digest(bytes))
            .cloned()
            .context("textures were not supplied by their decode job")
    }

    pub(crate) fn decode_textures(&self, bytes: &[u8]) -> anyhow::Result<Arc<Decoded>> {
        let hash = crate::digest(bytes);
        match self.palettes.get(&hash) {
            Some(textures) => Ok(Arc::clone(textures)),
            None => Ok(Arc::new(crate::texture::decode(
                bytes,
                &format!("textures/{hash}"),
            )?)),
        }
    }

    /// A pruned physical writer may still have consumers needing original typed inputs.
    #[cfg(test)]
    pub(crate) fn decode(&mut self, original: &[u8], primary: &[u8]) -> anyhow::Result<()> {
        use anyhow::Context;
        let normalized = crate::character::texture_palette(primary, original)?;
        if self.get(&normalized).is_some() {
            return Ok(());
        }
        let palette = normalized
            .get(
                crate::read::u32(&normalized, 0)? as usize
                    ..crate::read::u32(&normalized, 4)? as usize,
            )
            .context("model palette exceeds source")?;
        let textures = self.decode_textures(palette)?;
        textures.validate()?;
        let geometry = crate::geometry::decode_section_with_alpha(
            &normalized,
            crate::all_assets::physical_scene::TextureSource::Local {
                catalogue: format!("textures/{}/textures.json", textures.source_sha256),
            },
            textures.base_alpha()?,
        )?;
        self.remember(&normalized, geometry, textures, None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result, ensure};
    use std::{fs, path::Path};

    #[test]
    fn package_dependencies_share_buffers_and_release_inputs() -> Result<()> {
        let geometry = || DecodedGeometry {
            scene: crate::all_assets::physical_scene::Scene {
                mesh: String::new(),
                bone_names: Vec::new(),
                draws: Vec::new(),
                textures: crate::all_assets::physical_scene::TextureSource::Caller,
            },
            gltf: serde_json::json!({}),
            binary: Arc::new(vec![0]),
            bindings: None,
            model_name: None,
        };
        let mut palette = vec![0; 96];
        for (offset, value) in [
            (0, 0x20_af30_u32),
            (4, 1),
            (8, 12),
            (12, 20),
            (20, 0x0008_0008),
            (28, 64),
        ] {
            palette[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        let mut recovered = RecoveredModels::default();
        assert!(recovered.textures(&palette).is_err());
        let textures = recovered.decode_textures(&palette)?;
        let pixels = Arc::downgrade(&textures);
        recovered.remember(
            b"normalized model and selected palette",
            geometry(),
            textures,
            None,
        );
        let shared = recovered.textures(&palette)?;
        assert!(Arc::ptr_eq(&shared, &pixels.upgrade().unwrap()));
        recovered.remember(
            b"different geometry, same palette",
            geometry(),
            shared,
            None,
        );
        assert_eq!(recovered.palettes.len(), 1);
        // Identical pixels with a different sampler are a different interpretation.
        palette[35] = 1;
        let changed = recovered.decode_textures(&palette)?;
        assert!(!Arc::ptr_eq(&changed, &pixels.upgrade().unwrap()));
        let model = Arc::downgrade(
            &recovered
                .get(b"normalized model and selected palette")
                .unwrap()
                .geometry,
        );
        assert!(
            recovered
                .get(b"normalized model and different palette")
                .is_none()
        );
        let mut dependency = RecoveredModels::default();
        dependency.extend(&recovered);
        drop(recovered);
        assert!(model.upgrade().is_some() && pixels.upgrade().is_some());
        drop(dependency);
        assert!(model.upgrade().is_none() && pixels.upgrade().is_none());
        Ok(())
    }

    #[test]
    #[ignore = "requires original disc 1; validates physical/named model sharing without playback"]
    fn original_package_reuses_geometry_pixels_and_palette_context() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let resources = crate::resource::read(&executable)?;
        let path = resources.party(crate::resource::PartyResource::Body, 1, 0)?;
        let bytes = fs::read(
            extracted
                .join("files")
                .join(crate::field_resources::resolve_path(
                    &extracted.join("files"),
                    path,
                )?),
        )?;
        let sections = crate::field::sections(&bytes)?;
        let primary = &bytes[sections[0].clone().context("missing body")?];
        let physical = tempfile::tempdir()?;
        let named = tempfile::tempdir()?;
        let mut recovered = RecoveredModels::default();
        let mut inputs = Vec::new();
        for (index, section) in sections.iter().take(2).enumerate() {
            let Some(section) = section else { continue };
            let original = &bytes[section.clone()];
            let normalized = crate::character::texture_palette(primary, original)?;
            let mut failures = Vec::new();
            ensure!(
                crate::all_assets::geometry::cook_recovered(
                    &normalized,
                    &format!("part-{index}"),
                    physical.path(),
                    None,
                    crate::all_assets::geometry::Input::File,
                    &mut recovered,
                    &mut |path, result| {
                        if let Err(error) = result {
                            failures.push(format!("{path}: {error:#}"));
                        }
                    }
                ),
                "unrecognized original model"
            );
            ensure!(failures.is_empty(), "{}", failures.join("\n"));
            ensure!(
                recovered.get(&normalized).is_some(),
                "original model was not retained"
            );
            inputs.push((index, original, normalized));
        }
        assert_eq!(inputs.len(), 2, "fixture needs the body and outline");
        let body = recovered.get(&inputs[0].2).unwrap();
        let outline = recovered.get(&inputs[1].2).unwrap();
        assert!(!Arc::ptr_eq(&body.geometry, &outline.geometry));
        assert!(Arc::ptr_eq(&body.textures, &outline.textures));
        let mut models =
            crate::scene::source::Models::with_recovered(named.path(), Some(&recovered));
        for (index, original, normalized) in &inputs {
            let expected = Arc::clone(&recovered.get(normalized).unwrap().geometry.binary);
            models.add(
                &format!("part-{index}"),
                original,
                primary,
                move |geometry, _, _, _| {
                    ensure!(
                        Arc::ptr_eq(&geometry.binary, &expected),
                        "named model decoded geometry again"
                    );
                    Ok(())
                },
            )?;
        }
        let layers = models.finish()?;
        for ((index, _, normalized), layer) in inputs.iter().zip(&layers) {
            let source = recovered.get(normalized).unwrap();
            assert_eq!(
                fs::read(named.path().join(&layer.part.mesh))?,
                fs::read(physical.path().join(&source.geometry.scene.mesh))?
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(
                    fs::metadata(named.path().join(&layer.part.mesh))?.ino(),
                    fs::metadata(physical.path().join(&source.geometry.scene.mesh))?.ino()
                );
            }
            for texture in source.textures.catalogue.textures.iter().flatten() {
                for path in &texture.images {
                    assert_eq!(
                        fs::read(named.path().join(path))?,
                        fs::read(physical.path().join(format!(
                            "part-{index}/palettes/{}",
                            path.rsplit('/').next().unwrap()
                        )))?
                    );
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        assert_eq!(
                            fs::metadata(named.path().join(path))?.ino(),
                            fs::metadata(physical.path().join(format!(
                                "part-{index}/palettes/{}",
                                path.rsplit('/').next().unwrap()
                            )))?
                            .ino()
                        );
                    }
                }
            }
        }
        let mut changed =
            crate::scene::source::Models::with_recovered(named.path(), Some(&recovered));
        changed.add("changed", primary, primary, |_, _, _, glb| {
            glb.json["asset"]["generator"] = "transformed variant".into();
            Ok(())
        })?;
        assert_ne!(changed.finish()?.remove(0).part.mesh, layers[0].part.mesh);
        let missing = RecoveredModels::default();
        let mut strict = crate::scene::source::Models::with_recovered(named.path(), Some(&missing));
        assert!(
            strict
                .add("missing", primary, primary, |_, _, _, _| Ok(()))
                .is_err()
        );
        let geometry = Arc::downgrade(&recovered.get(&inputs[0].2).unwrap().geometry);
        let textures = Arc::downgrade(&recovered.get(&inputs[0].2).unwrap().textures);
        drop(recovered);
        assert!(geometry.upgrade().is_none() && textures.upgrade().is_none());
        // A pruned physical writer can supply decoded inputs without a mesh file.
        let mut empty = RecoveredModels::default();
        empty.decode(primary, primary)?;
        assert!(empty.get(&inputs[0].2).unwrap().mesh.is_none());
        let mut fallback = crate::scene::source::Models::with_recovered(named.path(), Some(&empty));
        fallback.add("fallback", primary, primary, |_, _, _, _| Ok(()))?;
        assert_eq!(fallback.finish()?.remove(0).part.mesh, layers[0].part.mesh);
        Ok(())
    }
}
