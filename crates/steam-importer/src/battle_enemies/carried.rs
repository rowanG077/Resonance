//! Carried controls and alternate weapons share the ordinary GC artwork owner.
use super::*;
use crate::{
    effect_atlas, equipment, monsters, output, package::Package, preview_model, source::Source,
    texture,
};
use resonance_content::{
    battle_effect::{
        EffectTexture, declaration::Declaration, document::Definition as Effects,
        visual::ParticleTexture,
    },
    battle_model::{
        AttachmentAnimation, TrailTexture,
        document::{
            Model,
            enemy::{Definition as Artwork, Variant},
        },
    },
    font::UiTexture,
    model_preview::PreviewPart,
};
use std::{fs, num::NonZeroU16};

fn controls(settings: &[u8]) -> Result<(Vec<u8>, Option<AttachmentAnimation>)> {
    let mut settings = settings.to_vec();
    ensure!(settings.len() == 64, "invalid carried metadata size");
    let interval = settings[10];
    settings[10] = 0;
    let settings = equipment::metadata(&settings)?;
    let mut animation =
        resonance_importer::decode_equipment_animation(&settings)?.unwrap_or_default();
    animation.effect_every_ticks =
        (interval != 0).then(|| NonZeroU16::new(u16::from(interval) + 1).unwrap());
    Ok((settings, (!animation.is_empty()).then_some(animation)))
}

fn package(bytes: &[u8], slot: u8) -> Result<Option<Package<'_>>> {
    preview_model::member(bytes, 0x160 + usize::from(slot) * 4)?
        .map(Package::parse)
        .transpose()
}

pub(super) fn bind(
    source: &Source,
    library: &Path,
    bytes: &[u8],
    name: &str,
    artwork: &mut Artwork,
) -> Result<()> {
    for (&slot, model) in &artwork.attachments {
        let part = package(bytes, slot)?.context("carried model has no source package")?;
        let (settings, animation) = controls(part.member(0)?.context("missing carried metadata")?)?;
        let mesh = &model
            .layers
            .first()
            .context("carried model has no primary layer")?
            .scene
            .mesh;
        if let Some(trail) =
            resonance_importer::decode_equipment_trail(&fs::read(library.join(mesh))?, &settings)?
        {
            artwork.trails.insert(slot, trail);
        }
        if let Some(animation) = animation {
            artwork.uv_animation.insert(slot, animation);
        }
    }
    let metadata = monsters::metadata(bytes)?;
    let equipped = metadata[0x1f0];
    ensure!(equipped <= 8, "enemy carried model count exceeds directory");
    for slot in equipped..8 {
        let Some(part) = package(bytes, slot)? else {
            continue;
        };
        ensure!(equipped != 0, "alternate weapon has no equipped attachment");
        ensure!(
            part.member(2)?.is_none() && part.member(4)?.is_none(),
            "layered alternate weapon is not decoded"
        );
        let (settings, animation) = controls(
            part.member(0)?
                .context("missing alternate weapon metadata")?,
        )?;
        let body = part
            .reference(1)?
            .context("missing alternate weapon model")?;
        let motions = part
            .reference(3)?
            .map(|name| vec![(0, name.to_owned())])
            .unwrap_or_default();
        let owner = format!("battle/enemies/{name}/weapon-variants/{slot}");
        let (mut scene, _) = preview_model::part(source, library, body, &owner, "body", &motions)?;
        scene.autoplay = !motions.is_empty();
        let trail = resonance_importer::decode_equipment_trail(
            &fs::read(library.join(&scene.mesh))?,
            &settings,
        )?;
        ensure!(trail.is_some(), "alternate weapon has no blade endpoints");
        let model = Model {
            attachment_nodes: Default::default(),
            independent_layers: Default::default(),
            layers: vec![PreviewPart {
                scene,
                animation: (!motions.is_empty()).then_some(0),
                attached_to: None,
                additive: metadata[0x130] & 0x10 != 0,
                uv_offsets: Vec::new(),
            }],
        };
        artwork.weapon_variants.insert(
            slot,
            Variant {
                model,
                trail,
                animation,
            },
        );
    }
    Ok(())
}

/// The effect category owns every atlas binding, including carried ribbons.
/// Read the same native metadata as the artwork owner without requiring that
/// owner to run afterward and repair a freshly published effect document.
pub(crate) fn bind_effects(
    source: &Source,
    library: &Path,
    bytes: &[u8],
    name: &str,
    effects: &mut Effects,
) -> Result<()> {
    let selected = trail_rectangles(source, bytes)?;
    if !selected.is_empty() {
        local_trail_images(source, library, bytes, name, &selected, effects)?;
    }
    Ok(())
}

fn trail_rectangles(source: &Source, bytes: &[u8]) -> Result<BTreeMap<u8, BTreeSet<[i16; 4]>>> {
    let mut selected = BTreeMap::<u8, BTreeSet<[i16; 4]>>::new();
    for slot in 0..8 {
        let Some(part) = package(bytes, slot)? else {
            continue;
        };
        let body = part.reference(1)?.context("carried ribbon has no model")?;
        let rig = crate::model::Rig::read(&source.require(body)?)?;
        let endpoints = rig
            .joint_names()
            .iter()
            .filter(|name| {
                name.as_bytes()
                    .get(..2)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"KI"))
            })
            .count();
        if endpoints < 2 {
            continue;
        }
        ensure!(endpoints <= 3, "too many carried ribbon endpoints");
        let (settings, _) = controls(part.member(0)?.context("missing carried ribbon metadata")?)?;
        let material = resonance_importer::decode_equipment_trail_material(&settings)?;
        if material.texture == Some(TrailTexture::Enemy { slot: 2 }) {
            selected
                .entry(material.palette)
                .or_default()
                .insert(material.uv);
        }
    }
    Ok(selected)
}

fn local_trail_images(
    source: &Source,
    library: &Path,
    package: &[u8],
    name: &str,
    selected: &BTreeMap<u8, BTreeSet<[i16; 4]>>,
    effects: &mut Effects,
) -> Result<()> {
    let owner = format!("battle/enemies/{name}/effects");
    // Previous incremental cooks used set zero for the first repacked particle.
    // Move those explicit bindings together before assigning the shared trail set.
    let migrate = effects.templates.iter().any(|declaration| matches!(declaration,
        Declaration::Particle { visual, .. } if matches!(visual.texture, ParticleTexture::Atlas { slot: 2, palette_set: 0, .. })));
    let textures = effects
        .artwork
        .as_mut()
        .context("enemy effect artwork is missing")?
        .textures
        .entry(2)
        .or_insert_with(|| {
            vec![EffectTexture {
                images: Vec::new(),
                sampler: resonance_content::texture::Sampler {
                    wrap: [resonance_content::TextureWrap::Clamp; 2],
                    ..Default::default()
                },
                palette_sets: vec![vec![None; 256]],
            }]
        });
    let texture = textures
        .first_mut()
        .context("enemy local atlas has no image binding")?;
    if migrate {
        texture.palette_sets.insert(0, vec![None; 256]);
        for declaration in &mut effects.templates {
            if let Declaration::Particle { visual, .. } = declaration
                && let ParticleTexture::Atlas {
                    slot: 2,
                    palette_set,
                    ..
                } = &mut visual.texture
            {
                *palette_set = palette_set
                    .checked_add(1)
                    .context("too many enemy palette sets")?;
            }
        }
    }
    let resource =
        preview_model::reference(package, 0x1d0)?.context("enemy ribbon has no local atlas")?;
    let descriptor = texture::Descriptor::read(&source.require(&resource)?)?;
    let decoded = descriptor.decode(&source.require(&descriptor.payload)?)?;
    let image = crate::pe::Image::parse(&source.executable)?;
    let directory = effect_atlas::Directory::read(&image)?;
    for (&selector, rectangles) in selected {
        let pixels = restore_rectangles(
            &decoded.levels[0],
            directory.get(&resource),
            selector,
            rectangles,
        )?;
        let relative = format!("{owner}/textures/weapon-trail-{selector}.ktx2");
        output::write(
            &library.join(&relative),
            &resonance_asset_writer::ktx2::encode_rgba8(512, 512, &pixels)?,
        )?;
        let page = if let Some(index) = texture
            .images
            .iter()
            .position(|image| image.path == relative)
        {
            index
        } else {
            let index = texture.images.len();
            texture.images.push(UiTexture {
                path: relative,
                width: 512,
                height: 512,
            });
            index
        };
        let set = texture
            .palette_sets
            .first_mut()
            .context("enemy ribbon palette set is absent")?;
        ensure!(
            set.len() == 256,
            "enemy ribbon selectors do not cover shared palette space"
        );
        set[usize::from(selector)] = Some(page);
    }
    Ok(())
}

fn restore_rectangles(
    source: &resonance_asset_writer::texture::Pixels,
    table: Option<&effect_atlas::Table>,
    selector: u8,
    rectangles: &BTreeSet<[i16; 4]>,
) -> Result<Vec<u8>> {
    const SIDE: usize = 512;
    let mut rgba = vec![0; SIDE * SIDE * 4];
    let mut occupied = vec![0u8; SIDE * SIDE];
    for &rect in rectangles {
        let [x, y, width, height] = rect.map(i32::from);
        ensure!(
            width > 0
                && height > 0
                && x >= 0
                && y >= 0
                && x + width <= SIDE as i32
                && y + height <= SIDE as i32,
            "enemy ribbon rectangle exceeds shared atlas"
        );
        let mut quad = [[
            [x as f32, y as f32],
            [(x + width) as f32, y as f32],
            [x as f32, (y + height) as f32],
            [(x + width) as f32, (y + height) as f32],
        ]];
        if let Some(table) = table {
            table.remap(u16::from(selector), &mut quad);
        }
        let mapped = quad[0];
        ensure!(
            mapped
                .iter()
                .flatten()
                .all(|value| value.is_finite() && value.fract() == 0.)
                && mapped[3][0] - mapped[0][0] == width as f32
                && mapped[3][1] - mapped[0][1] == height as f32,
            "enemy ribbon atlas changes texel scale"
        );
        let origin = [mapped[0][0] as i32, mapped[0][1] as i32];
        ensure!(
            origin[0] >= 0
                && origin[1] >= 0
                && origin[0] + width <= source.width as i32
                && origin[1] + height <= source.height as i32,
            "enemy ribbon source rectangle exceeds packed atlas"
        );
        for py in (y - 1).max(0)..=(y + height).min(SIDE as i32 - 1) {
            for px in (x - 1).max(0)..=(x + width).min(SIDE as i32 - 1) {
                let interior = (x..x + width).contains(&px) && (y..y + height).contains(&py);
                let priority = if interior { 2 } else { 1 };
                let sx = (origin[0] + px - x).clamp(0, source.width as i32 - 1) as usize;
                let sy = (origin[1] + py - y).clamp(0, source.height as i32 - 1) as usize;
                let color = &source.rgba[(sy * source.width as usize + sx) * 4..][..4];
                let index = py as usize * SIDE + px as usize;
                let output = &mut rgba[index * 4..][..4];
                ensure!(
                    occupied[index] != 2 || priority != 2 || output == color,
                    "enemy ribbon rectangles have incompatible interiors"
                );
                if priority > occupied[index] {
                    output.copy_from_slice(color);
                    occupied[index] = priority;
                }
            }
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carried_interval_and_alpha_survive_shared_numeric_normalization() -> Result<()> {
        let mut row = vec![0; 64];
        row[10] = 40;
        row[19] = 128;
        row[20..22].copy_from_slice(&192i16.to_le_bytes());
        let (settings, animation) = controls(&row)?;
        assert_eq!(settings[19], 128);
        assert_eq!(i16::from_be_bytes(settings[20..22].try_into()?), 192);
        assert_eq!(animation.unwrap().effect_every_ticks.unwrap().get(), 41);
        row[10] = 0;
        assert!(controls(&row)?.1.is_none());
        Ok(())
    }

    #[test]
    fn ribbon_restoration_keeps_remapped_texels_and_filter_border() -> Result<()> {
        let source = resonance_asset_writer::texture::Pixels {
            width: 8,
            height: 4,
            rgba: (0..4u8)
                .flat_map(|y| (0..8u8).flat_map(move |x| [x, y, 7, x * 10]))
                .collect(),
        };
        let table = effect_atlas::Table {
            rows: vec![effect_atlas::Remap {
                palette: 5,
                source: [1., 1., 3., 3.],
                packed: [4., 1., 6., 3.],
            }],
        };
        let image = restore_rectangles(&source, Some(&table), 5, &BTreeSet::from([[1, 1, 2, 2]]))?;
        assert_eq!(&image[(512 + 1) * 4..][..4], &[4, 1, 7, 40]);
        assert_eq!(&image[..4], &[3, 0, 7, 30]);
        assert!(
            restore_rectangles(
                &source,
                Some(&table),
                5,
                &BTreeSet::from([[1, 1, 2, 2], [2, 1, 2, 2]])
            )
            .is_err()
        );
        Ok(())
    }
}
