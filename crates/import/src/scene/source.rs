//! Bind decoded model values and publish their final presentation assets.
use super::{binding, decoded::Package, glb::Glb};
use crate::{geometry::DecodedGeometry, texture::Catalogue};
use anyhow::{Context, Result};
use resonance_content::ScenePart;
use std::{collections::BTreeSet, path::Path, sync::Arc};

#[derive(Clone)]
pub(crate) struct Layer {
    pub part: ScenePart,
    pub glb: Arc<Glb>,
}

pub(crate) struct Models<'a> {
    output: &'a Path,
    decoded: &'a Package,
    palettes: BTreeSet<String>,
    layers: Vec<Layer>,
}

impl<'a> Models<'a> {
    pub(crate) fn new(output: &'a Path, decoded: &'a Package) -> Self {
        Self {
            output,
            decoded,
            palettes: BTreeSet::new(),
            layers: Vec::new(),
        }
    }

    pub(crate) fn add(
        &mut self,
        label: &str,
        original: &[u8],
        primary: &[u8],
        configure: impl FnOnce(&DecodedGeometry, &Catalogue, &mut ScenePart, &mut Glb) -> Result<()>,
    ) -> Result<()> {
        let normalized = crate::character::texture_palette(primary, original)?;
        let model = self
            .decoded
            .get(&normalized)
            .with_context(|| format!("model {label} was not supplied by its decode job"))?;
        let textures = &model.textures;
        if self.palettes.insert(textures.source_sha256.clone()) {
            textures.validate()?;
            let mut result = Ok(());
            textures.publish(self.output, |_, written| {
                if result.is_ok() {
                    result = written;
                }
            });
            result?;
        }
        let (mut part, mut glb) = binding::decoded(&model.geometry, &textures.catalogue)?;
        configure(&model.geometry, &textures.catalogue, &mut part, &mut glb)?;
        part.mesh = if model.geometry.gltf == glb.json
            && Arc::ptr_eq(&model.geometry.binary, &glb.binary)
        {
            for motion in &glb.motions {
                crate::write_atomic(&self.output.join(&motion.path), &motion.bytes)?;
            }
            model
                .mesh
                .share(&self.output.join(&model.geometry.scene.mesh))?;
            model.geometry.scene.mesh.clone()
        } else {
            binding::write_mesh(self.output, &glb)?
        };
        self.layers.push(Layer {
            part,
            glb: Arc::new(glb),
        });
        Ok(())
    }

    pub(crate) fn finish(self) -> Vec<Layer> {
        self.layers
    }
}
