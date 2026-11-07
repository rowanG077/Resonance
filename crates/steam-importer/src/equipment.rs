//! Steam weapon packages directly into the shared GC equipment documents.
use crate::{
    actor_model, animation::Animation, model, output, package::Package, pe, preview_model,
    source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace,
    battle_model::document::{self, Equipment, EquipmentKind, Model},
    model_preview::PreviewPart,
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::ItemId;

fn identity(slot: u16) -> (u16, EquipmentKind) {
    match slot {
        0..=138 => (slot + 135, EquipmentKind::Weapon),
        139..=149 => (slot + 217, EquipmentKind::Shield),
        _ => (slot + 382, EquipmentKind::Weapon),
    }
}

fn name(item: u16) -> Result<String> {
    if item == 531 {
        return Ok("blue-shield-prop".into());
    }
    if item > 531 {
        return Ok(format!("equipment-{item:04}"));
    }
    Ok(ItemId::try_from(item)?
        .resource_name()
        .context("unnamed equipment item")?
        .into())
}

/// Only numeric halfwords change; frame sequences remain byte arrays.
pub(crate) fn metadata(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() == 64 && bytes[10] == 0 && bytes[12] == 0,
        "unsupported equipment appearance controls"
    );
    let mut row = bytes.to_vec();
    for at in (0x14..0x1c).step_by(2) {
        row[at..at + 2].reverse();
    }
    for index in 0..2 {
        if bytes[index] != 0 && matches!(bytes[4 + index], 2 | 5) {
            let at = 28 + index * 2;
            row[at..at + 2].reverse();
        }
    }
    Ok(row)
}

pub(crate) fn package(
    source: &Source,
    library: &Path,
    resource: &str,
    owner: &str,
    kind: EquipmentKind,
    owner_motions: &[(u16, String)],
) -> Result<Equipment> {
    package_bytes(
        source,
        library,
        &source.require(resource)?,
        owner,
        kind,
        owner_motions,
    )
}

/// Temporary scene weapons embed the part directory and metadata, while their
/// geometry and motion entries still reference the normal Steam resources.
pub(crate) fn embedded(
    source: &Source,
    library: &Path,
    bytes: &[u8],
    owner: &str,
) -> Result<document::WeaponDefinition> {
    Ok(package_bytes(source, library, bytes, owner, EquipmentKind::Weapon, &[])?.into())
}

fn package_bytes(
    source: &Source,
    library: &Path,
    bytes: &[u8],
    owner: &str,
    kind: EquipmentKind,
    owner_motions: &[(u16, String)],
) -> Result<Equipment> {
    let directory = Package::parse(bytes)?;
    let inline = directory
        .member(0)?
        .is_some_and(|value| value.len() == 64 && value[19] == 255);
    let first = if inline {
        None
    } else {
        directory.reference(0)?
    };
    let children = if first.is_some_and(|name| name.to_ascii_lowercase().ends_with(".pac")) {
        ensure!(directory.len() <= 4, "too many equipment parts");
        (0..directory.len())
            .filter_map(|slot| match directory.reference(slot) {
                Ok(Some(name)) => Some(Ok((slot as u8, Some(name.to_owned())))),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![(0, None)]
    };
    let mut equipment = Equipment {
        kind,
        parts: BTreeMap::new(),
        trails: BTreeMap::new(),
        uv_animation: BTreeMap::new(),
    };
    let mut seen = BTreeMap::<String, u8>::new();
    for (slot, resource) in children {
        let key = resource.as_deref().unwrap_or("$embedded");
        if let Some(&previous) = seen.get(key) {
            equipment
                .parts
                .insert(slot, equipment.parts[&previous].clone());
            if let Some(trail) = equipment.trails.get(&previous).cloned() {
                equipment.trails.insert(slot, trail);
            }
            if let Some(animation) = equipment.uv_animation.get(&previous).cloned() {
                equipment.uv_animation.insert(slot, animation);
            }
            continue;
        }
        let child = resource
            .as_deref()
            .map(|name| source.require(name))
            .transpose()?;
        let bytes = child.as_deref().unwrap_or(bytes);
        let part = Package::parse(bytes)?;
        ensure!(
            (5..=7).contains(&part.len()) && part.member(4)?.is_none(),
            "invalid equipment part directory"
        );
        let reference = |index| {
            if index < part.len() {
                part.reference(index)
            } else {
                Ok(None)
            }
        };
        let raw = part.member(0)?.context("missing equipment metadata")?;
        let settings = if raw.len() == 64 && raw[19] == 255 {
            metadata(raw)?
        } else {
            metadata(
                &source.require(reference(0)?.context("missing equipment metadata reference")?)?,
            )?
        };
        ensure!(
            settings[19] == 255,
            "unsupported inventory equipment trail alpha"
        );
        if let Some(animation) = resonance_importer::decode_equipment_animation(&settings)? {
            equipment.uv_animation.insert(slot, animation);
        }
        let mut layers = Vec::new();
        let mut primary_rig = None;
        let mut independent_layers = std::collections::BTreeSet::new();
        for (member, motion_member, outline, additive) in [
            (1, 3, false, false),
            (2, 3, true, false),
            (5, 6, false, true),
        ] {
            let Some(body) = reference(member)? else {
                continue;
            };
            let motion = reference(motion_member)?;
            ensure!(
                additive || owner_motions.is_empty() || motion.is_none(),
                "equipment has both local and owner-linked motion"
            );
            let mut motions = if additive || owner_motions.is_empty() {
                motion
                    .map(|name| vec![(0, name.to_owned())])
                    .unwrap_or_default()
            } else {
                owner_motions.to_vec()
            };
            if !additive && !owner_motions.is_empty() {
                // Genis's motion bank also animates his body. Publish only
                // clips that actually animate this weapon, rather than baking
                // a rest-only weapon clip for every unrelated body action.
                let rig = model::Rig::read(&source.require(body)?)?;
                motions = motions
                    .into_iter()
                    .filter_map(|(slot, name)| {
                        let used = (|| -> Result<bool> {
                            let animation = Animation::read(&source.require(&name)?)?;
                            Ok(animation.targets.iter().any(|target| {
                                matches!(target.kind, 3 | 0x10014 | 0x20003)
                                    && rig
                                        .animation_bindings
                                        .contains(&(target.kind, target.binding))
                            }))
                        })();
                        match used {
                            Ok(true) => Some(Ok((slot, name))),
                            Ok(false) => None,
                            Err(error) => Some(Err(error)),
                        }
                    })
                    .collect::<Result<_>>()?;
            }
            let layer = u16::try_from(layers.len())?;
            let (mut scene, _) = preview_model::part_on_rig(
                source,
                library,
                body,
                owner,
                &format!("part-{slot}-layer-{layer}"),
                &motions,
                if outline { primary_rig.as_ref() } else { None },
            )?;
            if layer == 0 {
                primary_rig = Some(resonance_content::animation::Skeleton::from_glb(
                    &fs::read(library.join(&scene.mesh))?,
                )?);
            }
            if outline {
                actor_model::style_outline(&mut scene);
            }
            scene.resource = layer;
            scene.autoplay = motion.is_some();
            for material in &mut scene.materials {
                if additive {
                    material.blend = true;
                    material.depth_write = false;
                    material.cull = CullFace::None;
                } else if !outline {
                    material.blend = material.color.is_some() || material.multiply.is_some();
                }
                if !outline {
                    material.draw_order +=
                        u32::from(layer) * resonance_content::field::MODEL_DRAW_SPAN;
                }
            }
            if additive {
                independent_layers.insert(layer);
            }
            if layer == 0 {
                if let Some(trail) = resonance_importer::decode_equipment_trail(
                    &fs::read(library.join(&scene.mesh))?,
                    &settings,
                )? {
                    equipment.trails.insert(slot, trail);
                }
            }
            layers.push(PreviewPart {
                scene,
                animation: motion.map(|_| 0),
                attached_to: None,
                additive,
                uv_offsets: Vec::new(),
            });
        }
        ensure!(!layers.is_empty(), "equipment has no model layers");
        equipment.parts.insert(
            slot,
            Model {
                layers,
                attachment_nodes: Default::default(),
                independent_layers,
            },
        );
        seen.insert(key.to_owned(), slot);
    }
    Ok(equipment)
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    for (address, bytes) in [
        (0x47cd15, &[0xba, 0x13, 2, 0, 0][..]),
        (0x47cd1f, &[0x81, 0xc1, 0x82, 0xfe, 0xff, 0xff]),
        (0x47cd33, &[0x81, 0xc1, 0x27, 0xff, 0xff, 0xff]),
        (0x47cd3b, &[0x81, 0xc1, 0x79, 0xff, 0xff, 0xff]),
        (0x47cd4b, &[0x8b, 0x14, 0x8d, 0, 0x38, 0x8e, 0]),
        (0x47cca0, &[0xb9, 0x18, 2, 0, 0]),
    ] {
        ensure!(
            image.slice(address, bytes.len())? == bytes,
            "Steam equipment selector changed at {address:#x}"
        );
    }
    // Genis weapon channels share the owner's native motion bank, starting at slot 60.
    let name_ptr = pe::word(image.slice(0x9191c0 + 2 * 7 * 4, 4)?, 0)?;
    let bytes = source.require(image.text(name_ptr)?)?;
    let genis = Package::parse(&bytes)?;
    let owner_motions = (62..genis.len())
        .filter_map(|member| match genis.reference(member) {
            Ok(Some(name)) => Some(Ok(((member - 2) as u16, name.to_owned()))),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;
    resonance_importer::cook_battle_trails(Path::new(""), library)?;
    let mut mappings = BTreeMap::new();
    let mut prepared = 0;
    for slot in 0..155u16 {
        let filename = image.text(pe::word(
            image.slice(0x8e3800 + u32::from(slot) * 4, 4)?,
            0,
        )?)?;
        if !source.contains(filename)? {
            eprintln!("  Equipment source slot {slot} is absent: {filename}");
            continue;
        }
        let (item, kind) = identity(slot);
        let name = name(item)?;
        let owner = format!("meshes/equipment/{name}");
        let motions =
            if item < 531 && image.slice(0x8d00c8 + u32::from(item) * 60 + 0x1a, 1)?[0] == 17 {
                owner_motions.as_slice()
            } else {
                &[]
            };
        let definition = package(source, library, filename, &owner, kind, motions)
            .with_context(|| format!("equipment {item} ({filename})"))?;
        output::json(&library.join(&owner).join("equipment.json"), &definition)?;
        ensure!(
            mappings.insert(u32::from(item), Some(name)).is_none(),
            "duplicate equipment item"
        );
        if slot == 149 {
            let prop = Equipment {
                kind: EquipmentKind::Weapon,
                ..definition
            };
            output::json(
                &library.join("meshes/equipment/blue-shield-prop/equipment.json"),
                &prop,
            )?;
            mappings.insert(531, Some("blue-shield-prop".into()));
        }
        prepared += 1;
    }
    for (item, row) in image
        .slice(0x8d00c8, 531 * 60)?
        .chunks_exact(60)
        .enumerate()
    {
        if matches!(row[0x1a], 32..=34) {
            ensure!(
                mappings.insert(item as u32, None).is_none(),
                "nonvisual equipment has a model"
            );
        }
    }
    output::json(&library.join(document::EQUIPMENT), &mappings)?;
    eprintln!(
        "  Equipment: {prepared} source packages and {} bindings",
        mappings.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scroll_offsets_and_frame_sequences_keep_their_storage_types() -> Result<()> {
        let mut row = vec![0; 64];
        row[19] = 255;
        row[0] = 1;
        row[4] = 2;
        row[28..30].copy_from_slice(&(-120i16).to_le_bytes());
        row[20..22].copy_from_slice(&448i16.to_le_bytes());
        let decoded = metadata(&row)?;
        assert_eq!(i16::from_be_bytes(decoded[28..30].try_into()?), -120);
        assert_eq!(i16::from_be_bytes(decoded[20..22].try_into()?), 448);
        row[4] = 4;
        row[28..32].copy_from_slice(&[3, 2, 1, 0]);
        assert_eq!(&metadata(&row)?[28..32], &[3, 2, 1, 0]);
        Ok(())
    }
}
