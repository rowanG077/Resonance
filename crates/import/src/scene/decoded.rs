//! Immutable package outputs shared by physical publication and scene binding jobs.
use crate::{geometry::DecodedGeometry, texture::Decoded};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub(crate) struct Model {
    pub geometry: Arc<DecodedGeometry>,
    pub textures: Arc<Decoded>,
    pub mesh: crate::publication::File,
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
        let mut scene = self.geometry.scene.clone();
        self.mesh.share(&output.join(&scene.mesh))?;
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

#[derive(Clone, Default)]
pub(crate) struct Package {
    models: BTreeMap<String, Model>,
    palettes: BTreeMap<String, Arc<Decoded>>,
    animations: BTreeMap<String, Arc<crate::animation::AuthoredAnimation>>,
    sources: BTreeMap<String, Arc<Vec<u8>>>,
}

impl Package {
    #[cfg(test)]
    pub(crate) fn field_dependencies(
        extracted: &std::path::Path,
        output: &std::path::Path,
        catalogue: &crate::resource::Catalogue,
        declarations: &std::collections::BTreeSet<u32>,
    ) -> anyhow::Result<Self> {
        use crate::resource::PartyResource;
        let mut paths = declarations
            .iter()
            .map(|&id| catalogue.source(id))
            .collect::<anyhow::Result<std::collections::BTreeSet<_>>>()?;
        for id in 1..=catalogue.party_bodies.len() as u8 {
            paths.insert(catalogue.party(PartyResource::Body, id, 0)?);
            paths.insert(catalogue.field_motion(id)?);
            paths.insert(catalogue.field_service(id)?);
        }
        let mut package = Self::default();
        let files = extracted.join("files");
        for path in paths {
            let bytes = Arc::new(std::fs::read(
                files.join(crate::field_resources::resolve_path(&files, path)?),
            )?);
            package.extend(&Self::cook(
                &bytes,
                &format!("assets/{}", crate::digest(&bytes)),
                output,
                crate::all_assets::geometry::Input::File,
            )?);
            package.remember_source(path, bytes);
        }
        Ok(package)
    }

    pub(crate) fn cook(
        bytes: &[u8],
        name: &str,
        output: &std::path::Path,
        input: crate::all_assets::geometry::Input,
    ) -> anyhow::Result<Self> {
        let mut package = Self::default();
        let mut failures = Vec::new();
        anyhow::ensure!(
            crate::all_assets::geometry::cook(
                bytes,
                name,
                output,
                None,
                input,
                &mut package,
                &mut |path, result| {
                    if let Err(error) = result {
                        failures.push(format!("{path}: {error:#}"));
                    }
                },
            ),
            "unrecognized model package {name}"
        );
        anyhow::ensure!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(package)
    }
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

    pub(crate) fn get(&self, normalized: &[u8]) -> Option<&Model> {
        // Normalization embeds the selected external palette. This map only
        // contains the ordinary physical geometry interpretation, before binding.
        self.models.get(&crate::digest(normalized))
    }

    pub(crate) fn textures(&self, bytes: &[u8]) -> anyhow::Result<Arc<Decoded>> {
        use anyhow::Context;
        self.palettes
            .get(&crate::digest(bytes))
            .cloned()
            .context("textures were not supplied by their decode job")
    }

    pub(crate) fn decode_textures(&mut self, bytes: &[u8]) -> anyhow::Result<&mut Arc<Decoded>> {
        let hash = crate::digest(bytes);
        match self.palettes.entry(hash.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
            std::collections::btree_map::Entry::Vacant(entry) => Ok(entry.insert(Arc::new(
                crate::texture::decode(bytes, &format!("textures/{hash}"))?,
            ))),
        }
    }

    pub(crate) fn decode_model(
        &mut self,
        original: &[u8],
        primary: &[u8],
        output: &std::path::Path,
    ) -> anyhow::Result<&Model> {
        use anyhow::Context;
        let normalized = crate::character::texture_palette(primary, original)?;
        let key = crate::digest(&normalized);
        if !self.models.contains_key(&key) {
            let palette = normalized
                .get(
                    crate::read::u32(&normalized, 0)? as usize
                        ..crate::read::u32(&normalized, 4)? as usize,
                )
                .context("model palette exceeds source")?;
            let textures = Arc::clone(self.decode_textures(palette)?);
            let source =
                if crate::read::u32(original, 0)? == 0 && crate::read::u32(palette, 4)? == 0 {
                    crate::all_assets::physical_scene::TextureSource::Caller
                } else {
                    crate::all_assets::physical_scene::TextureSource::Local {
                        catalogue: format!("textures/{}/textures.json", textures.source_sha256),
                    }
                };
            let (geometry, mesh) = crate::all_assets::physical_scene::cook_decoded(
                &normalized,
                output,
                source,
                textures.base_alpha().ok(),
            )?;
            self.models.insert(
                key.clone(),
                Model {
                    geometry: Arc::new(geometry),
                    textures,
                    mesh,
                },
            );
        }
        Ok(&self.models[&key])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result, ensure};
    use std::{fs, path::Path};

    #[test]
    fn package_dependencies_share_buffers_and_release_inputs() -> Result<()> {
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
        let mut decoded = Package::default();
        assert!(decoded.textures(&palette).is_err());
        let textures = decoded.decode_textures(&palette)?;
        let pixels = Arc::downgrade(textures);
        let shared = decoded.textures(&palette)?;
        assert!(Arc::ptr_eq(&shared, &pixels.upgrade().unwrap()));
        drop(shared);
        assert_eq!(decoded.palettes.len(), 1);
        // Identical pixels with a different sampler are a different interpretation.
        palette[35] = 1;
        let changed = decoded.decode_textures(&palette)?;
        assert!(!Arc::ptr_eq(changed, &pixels.upgrade().unwrap()));
        let mut dependency = Package::default();
        dependency.extend(&decoded);
        drop(decoded);
        assert!(pixels.upgrade().is_some());
        drop(dependency);
        assert!(pixels.upgrade().is_none());
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
        let mut decoded = Package::default();
        let mut inputs = Vec::new();
        for (index, section) in sections.iter().take(2).enumerate() {
            let Some(section) = section else { continue };
            let original = &bytes[section.clone()];
            let normalized = crate::character::texture_palette(primary, original)?;
            let mut failures = Vec::new();
            ensure!(
                crate::all_assets::geometry::cook(
                    &normalized,
                    &format!("part-{index}"),
                    physical.path(),
                    None,
                    crate::all_assets::geometry::Input::File,
                    &mut decoded,
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
                decoded.get(&normalized).is_some(),
                "original model was not retained"
            );
            inputs.push((index, original, normalized));
        }
        assert_eq!(inputs.len(), 2, "fixture needs the body and outline");
        let body = decoded.get(&inputs[0].2).unwrap();
        let outline = decoded.get(&inputs[1].2).unwrap();
        assert!(!Arc::ptr_eq(&body.geometry, &outline.geometry));
        assert!(Arc::ptr_eq(&body.textures, &outline.textures));
        let mut models = crate::scene::source::Models::new(named.path(), &decoded);
        for (index, original, normalized) in &inputs {
            let expected = Arc::clone(&decoded.get(normalized).unwrap().geometry.binary);
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
        let layers = models.finish();
        for ((index, _, normalized), layer) in inputs.iter().zip(&layers) {
            let source = decoded.get(normalized).unwrap();
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
        let mut changed = crate::scene::source::Models::new(named.path(), &decoded);
        changed.add("changed", primary, primary, |_, _, _, glb| {
            glb.json["asset"]["generator"] = "transformed variant".into();
            Ok(())
        })?;
        assert_ne!(changed.finish().remove(0).part.mesh, layers[0].part.mesh);
        let missing = Package::default();
        let mut strict = crate::scene::source::Models::new(named.path(), &missing);
        assert!(
            strict
                .add("missing", primary, primary, |_, _, _, _| Ok(()))
                .is_err()
        );
        let geometry = Arc::downgrade(&decoded.get(&inputs[0].2).unwrap().geometry);
        let textures = Arc::downgrade(&decoded.get(&inputs[0].2).unwrap().textures);
        drop(decoded);
        assert!(geometry.upgrade().is_none() && textures.upgrade().is_none());
        Ok(())
    }
}
