//! Shared original-model jobs; transformations see typed data and writers own output paths.
use super::{binding, glb::Glb};
use crate::{
    all_assets::{
        physical_scene::TextureSource,
        pool::{Dag, Output},
    },
    geometry::{DecodedGeometry, decode_section_with_alpha},
    texture::{Catalogue, Decoded},
};
use anyhow::{Context, Result};
use resonance_content::ScenePart;
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Clone)]
pub(crate) struct Layer {
    pub part: ScenePart,
    pub glb: Arc<Glb>,
    source: Option<(Arc<DecodedGeometry>, crate::publication::File)>,
}

pub(crate) struct Models<'a> {
    graph: Dag<'a, ()>,
    palettes: BTreeMap<String, (Output<Arc<Decoded>>, Output<()>)>,
    layers: Vec<(String, Output<Layer>, Output<()>)>,
    output: &'a Path,
    recovered: Option<&'a super::recovered::RecoveredModels>,
}

impl<'a> Models<'a> {
    pub(crate) fn new(output: &'a Path) -> Self {
        Self {
            graph: Dag::new(),
            palettes: BTreeMap::new(),
            layers: Vec::new(),
            output,
            recovered: None,
        }
    }

    pub(crate) fn with_recovered(
        output: &'a Path,
        recovered: Option<&'a super::recovered::RecoveredModels>,
    ) -> Self {
        Self {
            recovered,
            ..Self::new(output)
        }
    }

    pub(crate) fn add(
        &mut self,
        label: &str,
        original: &'a [u8],
        primary: &'a [u8],
        configure: impl Fn(&DecodedGeometry, &Catalogue, &mut ScenePart, &mut Glb) -> Result<()>
        + Sync
        + 'a,
    ) -> Result<()> {
        let normalized = Arc::new(crate::character::texture_palette(primary, original)?);
        let recovered = self
            .recovered
            .map(|models| {
                models
                    .get(&normalized)
                    .with_context(|| format!("model {label} was not supplied by its decode job"))
            })
            .transpose()?;
        let source = recovered.and_then(|model| {
            model
                .mesh
                .clone()
                .map(|file| (Arc::clone(&model.geometry), file))
        });
        let palette =
            crate::read::u32(&normalized, 0)? as usize..crate::read::u32(&normalized, 4)? as usize;
        let directory = format!("textures/{}", crate::digest(&normalized[palette.clone()]));
        let output = self.output;
        let (textures, texture_publication) =
            *self.palettes.entry(directory.clone()).or_insert_with(|| {
                let source = Arc::clone(&normalized);
                let palette_name = directory.clone();
                let recovered = recovered.map(|model| Arc::clone(&model.textures));
                let textures = self
                    .graph
                    .add(format!("decode/{directory}"), [], move |_, _| {
                        let decoded = match &recovered {
                            Some(decoded) => Arc::clone(decoded),
                            None => Arc::new(crate::texture::decode(
                                &source[palette.clone()],
                                &palette_name,
                            )?),
                        };
                        decoded.validate()?;
                        Ok(decoded)
                    });
                let publication = self.graph.add(
                    format!("publish/{directory}"),
                    [textures.dependency()],
                    move |_, inputs| {
                        let textures = inputs.get(textures)?;
                        let mut result = Ok(());
                        textures.publish(output, |_, written| {
                            if result.is_ok() {
                                result = written;
                            }
                        });
                        result
                    },
                );
                (textures, publication)
            });
        let recovered = recovered.map(|model| Arc::clone(&model.geometry));
        let geometry = self.graph.add(
            format!("decode/{label}"),
            [textures.dependency()],
            move |_, inputs| {
                if let Some(model) = &recovered {
                    return Ok(Arc::clone(model));
                }
                Ok(Arc::new(decode_section_with_alpha(
                    &normalized,
                    TextureSource::Local {
                        catalogue: format!("{directory}/textures.json"),
                    },
                    inputs.get(textures)?.base_alpha()?,
                )?))
            },
        );
        let layer = self.graph.add(
            format!("bind/{label}"),
            [geometry.dependency(), textures.dependency()],
            move |_, inputs| {
                let geometry = inputs.get(geometry)?;
                let textures = inputs.get(textures)?;
                let (mut part, mut glb) = binding::decoded(&geometry, &textures.catalogue)?;
                configure(&geometry, &textures.catalogue, &mut part, &mut glb)?;
                Ok(Layer {
                    part,
                    glb: Arc::new(glb),
                    source: source.clone(),
                })
            },
        );
        self.layers
            .push((label.to_owned(), layer, texture_publication));
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<Vec<Layer>> {
        let output = self.output;
        let publications: Vec<_> = self
            .layers
            .into_iter()
            .map(|(label, layer, textures)| {
                self.graph.add(
                    format!("publish/{label}"),
                    [layer.dependency(), textures.dependency()],
                    move |_, inputs| {
                        let layer = inputs.get(layer)?;
                        let mut published = (*layer).clone();
                        published.part.mesh = if let Some((source, file)) = &layer.source
                            && source.gltf == layer.glb.json
                            && Arc::ptr_eq(&source.binary, &layer.glb.binary)
                        {
                            for motion in &layer.glb.motions {
                                crate::write_atomic(&output.join(&motion.path), &motion.bytes)?;
                            }
                            file.share(&output.join(&source.scene.mesh))?;
                            source.scene.mesh.clone()
                        } else {
                            binding::write_mesh(output, &layer.glb)?
                        };
                        published.source = None;
                        Ok(published)
                    },
                )
            })
            .collect();
        let mut layers = BTreeMap::new();
        // Package callers already bound concurrency; this graph never adds a worker fanout.
        let statuses = self.graph.run(
            1,
            || (),
            |completion| {
                for (index, &publication) in publications.iter().enumerate() {
                    if let Some(Ok(layer)) = completion.get(publication) {
                        layers.insert(index, (*layer).clone());
                    }
                }
            },
        )?;
        for status in statuses {
            status?;
        }
        Ok(layers.into_values().collect())
    }
}
