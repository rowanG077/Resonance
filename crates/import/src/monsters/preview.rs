use super::*;
use crate::{
    model_preview::{Layer, layers},
    read::f32 as float,
};
use resonance_content::model_preview::{ModelPreview, NodeScale};

fn resource(bytes: &[u8], offset: usize) -> Result<&[u8]> {
    ensure!(offset > 0, "missing preview resource");
    bytes
        .get(offset..)
        .context("preview resource exceeds package")
}

fn optional(bytes: &[u8], offset: usize) -> Result<Option<&[u8]>> {
    (offset != 0).then(|| resource(bytes, offset)).transpose()
}

pub(super) fn cook(bytes: &[u8], metadata: &[u8], id: u8, output: &Path) -> Result<ModelPreview> {
    let at = |offset| -> Result<&[u8]> { resource(bytes, word(bytes, offset)? as usize) };
    let maybe =
        |offset| -> Result<Option<&[u8]>> { optional(bytes, word(bytes, offset)? as usize) };
    let mut parts = Vec::new();
    layers(
        Layer {
            model: at(0x18)?,
            outline: maybe(0x1c)?,
            animation: maybe(0x20)?,
            attached_to: None,
            additive: false,
        },
        &mut parts,
        &format!("monsters/{id:03}"),
        output,
    )?;
    let bones = parts[0].scene.bone_names.clone();
    let bone = |prefix: &str| -> Result<String> {
        bones
            .iter()
            .find(|name| name.starts_with(prefix))
            .cloned()
            .with_context(|| format!("missing preview attachment bone {prefix}"))
    };
    let flags = half(metadata, 0xb4)?;
    if flags & 0x8400 != 0 {
        layers(
            Layer {
                model: at(0x180)?,
                outline: maybe(0x198)?,
                animation: maybe(0x20 + usize::from(metadata[0xbd]) * 4)?,
                attached_to: (flags & 0x8000 != 0).then(|| bone("pa00")).transpose()?,
                additive: false,
            },
            &mut parts,
            &format!("monsters/{id:03}"),
            output,
        )?;
    }
    let count = usize::from(metadata[0x1e4]);
    ensure!(count <= 8, "too many enemy attachments");
    let mut extra = None;
    for i in 0..count {
        let record = at(0x160 + i * 4)?;
        layers(
            Layer {
                model: resource(record, word(record, 8)? as usize)?,
                outline: None,
                animation: None,
                attached_to: Some(bone(&format!("kk0{i}"))?),
                additive: metadata[0x124 + i * 24] & 0x10 != 0,
            },
            &mut parts,
            &format!("monsters/{id:03}"),
            output,
        )?;
        if i == 0 && word(record, 0)? >= 6 {
            extra = optional(record, word(record, 0x18)? as usize)?;
        }
    }
    if let Some(model) = extra {
        layers(
            Layer {
                model,
                outline: None,
                animation: None,
                attached_to: Some(bone("kk00")?),
                additive: true,
            },
            &mut parts,
            &format!("monsters/{id:03}"),
            output,
        )?;
    }
    let mut node_scales = Vec::new();
    match id {
        236..=238 => node_scales.push(NodeScale {
            bone: bone("kk00")?,
            scale: [0.; 3],
            unless_flag: None,
        }),
        208..=210 => node_scales.push(NodeScale {
            bone: bone("kk06_Hane")?,
            scale: [0.5; 3],
            unless_flag: None,
        }),
        191 => {
            for (index, flag) in [(0x46, 0x93), (0x20, 0x94), (0x3f, 0x94)] {
                node_scales.push(NodeScale {
                    bone: bones
                        .get(index)
                        .context("missing Sword Dancer body part")?
                        .clone(),
                    scale: [0.; 3],
                    unless_flag: Some(flag),
                });
            }
        }
        _ => {}
    }
    Ok(ModelPreview {
        scale: float(metadata, 0x11c)?,
        elevation: float(metadata, 0x120)?,
        parts,
        hidden_geometry: bones
            .into_iter()
            .filter(|name| {
                let prefix = name.get(..2).unwrap_or("");
                prefix.eq_ignore_ascii_case("kk") || prefix.eq_ignore_ascii_case("pa")
            })
            .collect(),
        node_scales,
    })
}
