//! Bind all texture banks independently of the scripts that display them.
use crate::{all_assets::MemberKind, field_resources, texture, write_atomic};
use anyhow::{Context, Result};
use resonance_content::{
    effect::{OverlayArt, OverlayTexture},
    font::UiTexture,
};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub(crate) fn cook(
    resources: &mut field_resources::binding::Resources<'_>,
    physical: &crate::scene::binding::Map<'_>,
    output: &Path,
    textures: &BTreeMap<u32, Arc<texture::Decoded>>,
) -> Result<(BTreeMap<i32, String>, Vec<String>)> {
    let banks = bind(resources, physical, output, textures)?;
    let mut overlays = BTreeMap::new();
    let mut files = Vec::new();
    for (resource, bank) in banks {
        files.extend(
            bank.textures
                .iter()
                .flat_map(|texture| texture.images.iter().map(|image| image.path.clone())),
        );
        let bytes = serde_json::to_vec(&bank)?;
        let path = format!("fields/overlays/{}.json", crate::digest(&bytes));
        write_atomic(&output.join(&path), &bytes)?;
        files.push(path.clone());
        overlays.insert(resource, path);
    }
    files.sort();
    files.dedup();
    Ok((overlays, files))
}

fn bind(
    resources: &mut field_resources::binding::Resources<'_>,
    physical: &crate::scene::binding::Map<'_>,
    output: &Path,
    sources: &BTreeMap<u32, Arc<texture::Decoded>>,
) -> Result<BTreeMap<i32, OverlayArt>> {
    let decoded = resources.package();
    let mut textures = sources
        .iter()
        .map(|(&id, decoded)| Ok((id, publish(decoded, output)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    // Map-local texture handles index the resource tail, independent of which
    // branch creates an overlay. Bind every TPL, including unused entries.
    for (index, kind) in physical.sections() {
        if index >= 16 && kind == MemberKind::Texture {
            textures.insert(
                0xffee0000 | (index as u32 - 16),
                publish(
                    decoded.textures(physical.source_section(index)?)?.as_ref(),
                    output,
                )?,
            );
        }
    }
    let banks = textures
        .into_iter()
        .map(|(resource, textures)| {
            (
                resource as i32,
                OverlayArt {
                    textures: textures
                        .into_iter()
                        .map(|texture| OverlayTexture {
                            images: texture
                                .images
                                .into_iter()
                                .map(|path| UiTexture {
                                    path,
                                    width: texture.dimensions[0].into(),
                                    height: texture.dimensions[1].into(),
                                })
                                .collect(),
                            sampler: texture.sampler,
                        })
                        .collect(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for bank in banks.values() {
        bank.validate()?;
    }
    Ok(banks)
}

fn publish(decoded: &texture::Decoded, output: &Path) -> Result<Vec<texture::Texture>> {
    decoded.validate()?;
    let mut result = Ok(());
    decoded.publish(output, |_, written| {
        if result.is_ok() {
            result = written;
        }
    });
    result?;
    decoded
        .catalogue
        .textures
        .iter()
        .cloned()
        .map(|texture| texture.context("missing overlay texture"))
        .collect()
}
