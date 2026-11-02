//! Scenery layers, panorama and cameras written in the same field scene format as GC.
use crate::{model, output, package::Package, pe, source::Source, texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::{DrawStage, document::Scene},
    library::{FIELDS, Mappings},
};
use std::{collections::BTreeMap, fs, path::Path};

const LAYERS: [usize; 4] = [0, 10, 12, 2];

pub(crate) fn cook(source: &Source, library: &Path, selected: Option<u32>) -> Result<()> {
    let fields: Mappings = serde_json::from_slice(&fs::read(library.join(FIELDS))?)?;
    if let Some(id) = selected {
        fields.target(id)?;
    }
    let image = pe::Image::parse(&source.executable)?;
    for (id, name) in fields
        .iter()
        .filter(|(id, _)| selected.is_none_or(|only| only == *id))
    {
        eprintln!("  Field scenery: {name} ({id})");
        let pointer = pe::word(image.slice(0x8cb908 + id * 24, 4)?, 0)?;
        let bytes = source.require(image.text(pointer)?)?;
        let package = Package::parse(&bytes)?;
        let root = format!("fields/{name}");
        let mut parts = Vec::new();
        for section in LAYERS {
            let Some(descriptor) = package.reference(section)? else {
                continue;
            };
            if !source.contains(descriptor)? {
                eprintln!("    Source scenery is absent: {descriptor}");
                continue;
            }
            let label = format!("part-{}", parts.len());
            let (mut part, _model) = model::cook(source, library, descriptor, &root, &label)
                .with_context(|| format!("field {id} scenery section {section} ({descriptor})"))?;
            part.resource = section as u16;
            let stage = DrawStage::scenery(section as u16).context("unknown field layer")?;
            for material in &mut part.materials {
                material.draw_order += stage.offset();
                material.depth_write &= section != 2;
            }
            parts.push(part);
        }
        let panorama = match package.reference(15)? {
            Some(name) => match source.read(name)? {
                Some(bytes) => {
                    let descriptor = texture::Descriptor::read(&bytes)?;
                    let image = descriptor.decode(&source.require(&descriptor.payload)?)?;
                    let path = format!("{root}/textures/panorama.ktx2");
                    output::write(&library.join(&path), &image.ktx2()?)?;
                    Some(path)
                }
                None => {
                    eprintln!("    Source panorama is absent: {name}");
                    None
                }
            },
            None => None,
        };
        let cameras = (16..package.len())
            .filter_map(|slot| match package.reference(slot) {
                Ok(Some(name)) if name.to_ascii_lowercase().ends_with(".cam") => {
                    Some(Ok((slot, name)))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut camera_tracks = BTreeMap::new();
        for (ordinal, &(slot, name)) in cameras.iter().enumerate() {
            let Some(bytes) = source.read(name)? else {
                eprintln!("    Source camera is absent: {name}");
                continue;
            };
            let bytes = camera(&bytes).with_context(|| format!("field {id} camera {name}"))?;
            let path = if cameras.len() == 1 {
                format!("{root}/camera.glb")
            } else {
                format!("{root}/camera-{}.glb", ordinal + 1)
            };
            output::write(&library.join(&path), &bytes)?;
            camera_tracks.insert(0xffee_0000 + (slot - 16) as u32, path);
        }
        let scene = Scene {
            parts,
            panorama,
            water: None,
            environment: Default::default(),
            camera_tracks,
            texture_animations: Vec::new(),
            unbound_geometry: Vec::new(),
            contact_shadow: None,
            toon_ramp: None,
            effects: "fields/settings.json".into(),
            blink: None,
            particles: None,
            overlays: BTreeMap::new(),
            save_point: None,
        };
        output::json(&library.join(&root).join("scene.json"), &scene)?;
    }
    Ok(())
}

pub(crate) fn camera(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.starts_with(b"CAMM") && bytes.len().is_multiple_of(4),
        "unsupported field camera"
    );
    let mut bytes = bytes.to_vec();
    // CAMM's payload consists of 32-bit scalar fields. Reuse the GC converter
    // after changing numeric storage; its output already has the common clock.
    for word in bytes.chunks_exact_mut(4).skip(1) {
        word.reverse();
    }
    resonance_importer::convert_camera(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn little_endian_camera_loads_with_shared_camera_reader() -> Result<()> {
        let mut bytes = b"CAMM".to_vec();
        bytes.extend([72u32, 2].into_iter().flat_map(u32::to_le_bytes));
        for time in [0f32, 10.] {
            bytes.extend(
                [time, time, 2., 3., 0., 0., 0., 1., 0.]
                    .into_iter()
                    .flat_map(f32::to_le_bytes),
            );
        }
        for time in [0f32, 10.] {
            bytes.extend(
                [time, 0., 0., 0., 0.]
                    .into_iter()
                    .flat_map(f32::to_le_bytes),
            );
        }
        let bytes = camera(&bytes)?;
        let motion = resonance_content::camera_motion::CameraMotion::decode(&bytes)?;
        assert_eq!(motion.duration_frames(), 10.);
        let key = motion.sample(5.)?;
        for (actual, expected) in key.position.into_iter().zip([5., 2., 3.]) {
            assert!((actual - expected).abs() < 0.00001);
        }
        Ok(())
    }
}
