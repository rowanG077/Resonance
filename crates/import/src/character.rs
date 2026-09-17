//! Character mesh layers and animation tables, resolved offline from their
//! original files. Runtime assets contain no relocations or resource pointers.
#[cfg(test)]
use crate::scene::binding;
use crate::{
    all_assets::{MemberKind, geometry::is_model},
    animation::AuthoredAnimation,
    field_resources::binding::Resources,
};
use anyhow::{Context, Result, ensure};
use resonance_content::field::ActorAssets;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::Path,
    sync::Arc,
};
mod source;
pub(crate) use source::cook_parts as cook_source_parts;

/// The actor package reserves its final two slots for optional collision meshes.
pub(crate) fn collision_slot(index: usize, bytes: &[u8]) -> bool {
    matches!(index, 29 | 30) && !crate::animation::is_animation(bytes)
}

fn word(data: &[u8], at: usize) -> Result<usize> {
    Ok(crate::read::u32(data, at)? as usize)
}

/// Count followed by offset/length pairs; missing entries fall back to entry zero.
#[cfg(test)]
pub(crate) fn archive_entry(data: &[u8], index: usize) -> Result<&[u8]> {
    let count = word(data, 0)?;
    ensure!(
        count <= 65536 && index < count && 4 + count * 8 <= data.len(),
        "invalid resource archive index"
    );
    let entry = if word(data, 8 + index * 8)? == 0 {
        0
    } else {
        index
    };
    let start = word(data, 4 + entry * 8)?;
    let size = word(data, 8 + entry * 8)?;
    ensure!(
        start >= 4 + count * 8 && size > 0,
        "invalid resource archive payload"
    );
    data.get(start..start.checked_add(size).context("resource range overflow")?)
        .context("resource archive payload exceeds file")
}

#[cfg(test)]
fn section<'a>(bytes: &'a [u8], ranges: &[Option<Range<usize>>], index: usize) -> Result<&'a [u8]> {
    let range = ranges
        .get(index)
        .and_then(Option::as_ref)
        .context("missing character layer")?;
    Ok(&bytes[range.clone()])
}

/// A field's model bank starts with an offset table. Its first section maps
/// model indices to script IDs; remaining sections are complete actor packages.
#[cfg(test)]
fn field_models(bytes: &[u8]) -> Result<Vec<(u16, &[u8])>> {
    let mut ids = std::collections::BTreeSet::new();
    field_model_entries(bytes)?
        .into_iter()
        .map(|(id, range)| {
            ensure!(
                id > 9 && id != 24 && ids.insert(id),
                "duplicate or reserved field model ID {id}"
            );
            Ok((id, &bytes[range]))
        })
        .collect()
}

/// Physical field model entries, independent of runtime actor admission.
/// The first offset addresses IDs; subsequent offsets address model packages.
pub(crate) fn field_model_count(bytes: &[u8]) -> Result<usize> {
    let count = word(bytes, 0)?;
    ensure!(
        count > 0 && count <= bytes.len().saturating_sub(4) / 4,
        "invalid field model count"
    );
    let ids = word(bytes, 4)? & !3;
    ensure!(
        (count == 1 && ids == 0)
            || (ids >= 4 + count * 4
                && bytes
                    .get(ids..)
                    .is_some_and(|ids| ids.len() >= (count - 1) * 2)),
        "invalid field model ID table"
    );
    Ok(count)
}

pub(crate) fn field_model_entries(bytes: &[u8]) -> Result<Vec<(u16, Range<usize>)>> {
    let count = field_model_count(bytes)?;
    let header = 4 + count * 4;
    let offsets = (0..count)
        .map(|i| word(bytes, 4 + i * 4).map(|v| v & !3))
        .collect::<Result<Vec<_>>>()?;
    if count == 1 && offsets[0] == 0 {
        return Ok(Vec::new());
    }
    ensure!(
        offsets.iter().all(|v| (header..bytes.len()).contains(v)),
        "invalid field model offset"
    );
    let end = |start| {
        offsets
            .iter()
            .copied()
            .filter(|v| *v > start)
            .min()
            .unwrap_or(bytes.len())
    };
    let names = &bytes[offsets[0]..end(offsets[0])];
    ensure!((count - 1) * 2 <= names.len(), "truncated field model IDs");
    offsets
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, &offset)| Ok((crate::read::u16(names, (i - 1) * 2)?, offset..end(offset))))
        .collect()
}

/// A secondary mesh layer can share the primary layer's texture palette.
/// Assemble an offline source view for the existing geometry decoder; all GPL
/// and model-relative offsets remain unchanged.
pub(crate) fn texture_palette<'a>(primary: &[u8], secondary: &'a [u8]) -> Result<Cow<'a, [u8]>> {
    if word(secondary, 0)? != 0 {
        return Ok(Cow::Borrowed(secondary));
    }
    const EMPTY_PALETTE: [u8; 12] = [0, 0x20, 0xaf, 0x30, 0, 0, 0, 0, 0, 0, 0, 12];
    let texture = if word(primary, 0)? == 0 {
        // Color-only meshes and attachment placeholders need no texture table.
        // A textured draw without a real palette still fails material binding.
        &EMPTY_PALETTE[..]
    } else {
        primary
            .get(word(primary, 0)?..word(primary, 4)?)
            .context("invalid primary texture palette")?
    };
    let end = match word(secondary, 4)? {
        // Bare untextured geometry has no trailing palette or model boundary.
        0 => secondary.len(),
        end => end,
    };
    ensure!(
        (0x20..=secondary.len()).contains(&end),
        "invalid secondary geometry range"
    );
    let mut data = secondary[..end].to_vec();
    data.extend(texture);
    data.extend(&secondary[end..]);
    data[..4].copy_from_slice(&(end as u32).to_be_bytes());
    data[4..8].copy_from_slice(&((end + texture.len()) as u32).to_be_bytes());
    Ok(Cow::Owned(data))
}

#[derive(Default)]
pub(crate) struct Sources {
    packages: BTreeMap<u32, Vec<u8>>,
    pub(crate) animations: Vec<(u32, Arc<AuthoredAnimation>)>,
    pub(crate) textures: BTreeMap<u32, Arc<crate::texture::Decoded>>,
}

#[derive(Default)]
pub(crate) struct Prepared {
    pub actors: Vec<ActorAssets>,
    pub unbound: Vec<resonance_content::field::UnboundGeometry>,
    pub files: BTreeSet<String>,
}

impl Prepared {
    fn add(
        &mut self,
        binder: &Binder<'_>,
        id: u32,
        name: &str,
        model: &[u8],
        animation: &[u8],
        extra: &[Clip<'_>],
    ) -> Result<()> {
        if let Some(geometry) =
            source::unbound(binder.output, id, model, binder.recovered, &mut self.files)?
        {
            self.unbound.push(geometry);
        } else {
            self.actors
                .push(binder.cook(id, name, model, animation, extra)?);
        }
        Ok(())
    }
}

impl Sources {
    pub(crate) fn read(resources: &mut Resources<'_>, declared: &BTreeSet<u32>) -> Result<Self> {
        let mut sources = Self::default();
        for &id in declared {
            sources.insert(id, resources.resource(id)?, resources.recovered())?;
        }
        Ok(sources)
    }

    pub(crate) fn include_field(
        &mut self,
        field: &crate::scene::binding::Map<'_>,
        recovered: Option<&crate::scene::recovered::RecoveredModels>,
    ) -> Result<()> {
        for (id, bytes) in field.models()? {
            self.insert(u32::from(id), bytes.to_vec(), recovered)?;
        }
        Ok(())
    }

    fn insert(
        &mut self,
        id: u32,
        bytes: Vec<u8>,
        recovered: Option<&crate::scene::recovered::RecoveredModels>,
    ) -> Result<()> {
        if word(&bytes, 0)? == 31 || is_model(&bytes) {
            ensure!(
                self.packages.insert(id, bytes).is_none(),
                "duplicate model resource {id:#x}"
            );
        } else if word(&bytes, 0)? == 0x0020af30 {
            ensure!(
                self.textures
                    .insert(id, crate::scene::recovered::textures(&bytes, recovered)?)
                    .is_none(),
                "duplicate texture resource {id:#x}"
            );
        } else {
            let animation =
                crate::scene::recovered::animation(&bytes, recovered).with_context(|| {
                    format!("field resource {id:#x} is not a model, texture or animation")
                })?;
            self.animations.push((id, animation));
        }
        Ok(())
    }
}

pub(crate) fn cook_field(
    output: &Path,
    map_id: u32,
    physical: &crate::scene::binding::Map<'_>,
    sources: &Sources,
    shared: &[(u32, Arc<AuthoredAnimation>)],
    original: &mut Resources<'_>,
    declarations: &crate::field_resources::Declarations,
) -> Result<Prepared> {
    let recovered = original.recovered();
    let binder = Binder {
        output,
        recovered,
        shared,
    };
    let resources = original.catalogue;
    let mut model_resources = declarations.resources.clone();
    for id in sources.textures.keys() {
        model_resources.remove(id);
    }
    let mut assets = Prepared::default();
    // Scripts choose actor and animation handles independently. Prepare every
    // pairing instead of assigning a guessed owner to a shared clip.
    for id in 1..=resources.party_bodies.len() as u32 {
        let model_path = resources.party(crate::resource::PartyResource::Body, id as u8, 0)?;
        let model = original.source(model_path)?;
        let animation_path = resources.field_motion(id as u8)?;
        let animation = original.source(animation_path)?;
        let service = original.source(resources.field_service(id as u8)?)?;
        let sections = crate::field::sections(&service)?;
        let doors = [20, 24]
            .into_iter()
            .map(|slot| -> Result<_> {
                let range = sections
                    .get((slot - 4) / 4)
                    .and_then(Option::as_ref)
                    .context("missing door animation")?;
                Ok((
                    slot as u16,
                    crate::scene::recovered::animation(&service[range.clone()], recovered)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let extra: Vec<_> = doors
            .iter()
            .map(|(slot, animation)| Clip {
                slot: *slot,
                resource: Some(resonance_content::field::DOOR_MOTION_RESOURCE_BASE + id),
                animation,
            })
            .collect();
        assets.add(
            &binder,
            id,
            &format!("party-{id}"),
            &model,
            &animation,
            &extra,
        )?;
    }
    for (&id, bytes) in &sources.packages {
        ensure!(
            !assets.actors.iter().any(|a| a.resource == id)
                && !assets.unbound.iter().any(|a| a.resource == id),
            "resource {id:#x} conflicts with an actor binding"
        );
        let index = id & 0xffff;
        let name = match id >> 16 {
            2 => format!("npc-{index}"),
            _ => format!("resource-{id}"),
        };
        assets.add(&binder, id, &name, bytes, bytes, &[])?;
    }
    // MAP-local model handles address slots starting at section 16.
    for (index, kind) in physical.sections().filter(|(index, ..)| *index >= 16) {
        if !matches!(kind, MemberKind::Actor | MemberKind::Model) {
            continue;
        }
        let bytes = physical.source_section(index)?;
        assets.add(
            &binder,
            0xffee0000 + (index - 16) as u32,
            &format!("field-{map_id}-object-{}", index - 16),
            bytes,
            bytes,
            &[],
        )?;
    }
    if declarations.save_point {
        let bytes = original.source(&resources.save_point)?;
        assets.add(
            &binder,
            resonance_content::field::SAVE_POINT_RESOURCE,
            "save-point",
            &bytes,
            &bytes,
            &[],
        )?;
    }
    crate::field_resources::validate_cooked(
        &model_resources,
        assets
            .actors
            .iter()
            .flat_map(|actor| {
                std::iter::once(actor.resource).chain(
                    actor
                        .parts
                        .iter()
                        .flat_map(|part| &part.clips)
                        .filter_map(|clip| clip.animation_resource),
                )
            })
            .chain(assets.unbound.iter().map(|geometry| geometry.resource)),
    )?;
    Ok(assets)
}

#[cfg(test)]
fn decode_clip(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.get(..4) == Some(&0x007b7960u32.to_be_bytes()) {
        Ok(bytes.to_vec())
    } else {
        crate::compression::decode(bytes)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Clip<'a> {
    pub slot: u16,
    pub resource: Option<u32>,
    pub animation: &'a AuthoredAnimation,
}

fn unique_clips<'a>(clips: impl IntoIterator<Item = Clip<'a>>) -> Result<Vec<Clip<'a>>> {
    let mut unique = BTreeMap::<_, Clip<'a>>::new();
    for clip in clips {
        let key = (clip.resource, clip.slot);
        if let Some(previous) = unique.get(&key) {
            ensure!(
                serde_json::to_vec(previous.animation)? == serde_json::to_vec(clip.animation)?,
                "conflicting animation {key:?}"
            );
        } else {
            unique.insert(key, clip);
        }
    }
    Ok(unique.into_values().collect())
}

struct Binder<'a> {
    output: &'a Path,
    recovered: Option<&'a crate::scene::recovered::RecoveredModels>,
    shared: &'a [(u32, Arc<AuthoredAnimation>)],
}

impl Binder<'_> {
    fn cook(
        &self,
        id: u32,
        name: &str,
        model: &[u8],
        animation: &[u8],
        extra: &[Clip<'_>],
    ) -> Result<ActorAssets> {
        let Self {
            output,
            recovered,
            shared,
        } = *self;
        let sections = if is_model(animation) {
            Vec::new()
        } else {
            crate::field::sections(animation)?
        };
        let local = sections
            .into_iter()
            .enumerate()
            .skip(2)
            .filter_map(|(index, range)| range.map(|range| (index, range)))
            .filter(|(index, range)| !collision_slot(*index, &animation[range.clone()]))
            .map(|(index, range)| {
                Ok((
                    (4 + index * 4) as u16,
                    crate::scene::recovered::animation(&animation[range], recovered)
                        .with_context(|| format!("actor {name} animation slot {index}"))?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let clips = unique_clips(
            local
                .iter()
                .map(|(slot, animation)| Clip {
                    slot: *slot,
                    resource: None,
                    animation,
                })
                .chain(extra.iter().copied())
                .chain(shared.iter().map(|(id, animation)| Clip {
                    slot: 12,
                    resource: Some(*id),
                    animation,
                })),
        )?;
        let parts = cook_source_parts(output, name, model, &clips, recovered)?;
        println!(
            "Cooked {name}: {} clips across {} mesh layers",
            clips.len(),
            parts.len()
        );
        Ok(ActorAssets {
            resource: id,
            // Optional accessories whose names start with “kk” begin hidden.
            hidden_nodes: parts[0]
                .bone_names
                .iter()
                .enumerate()
                .filter(|(_, name)| name.starts_with("kk"))
                .map(|(index, _)| index as u16)
                .collect(),
            parts,
        })
    }
}

/// Bind published model layers and authored clips; no source conversion occurs here.
#[cfg(test)]
pub(crate) fn cook_parts(
    root: &Path,
    name: &str,
    package_directory: &str,
    clips: &[Clip<'_>],
) -> Result<Vec<resonance_content::ScenePart>> {
    #[derive(serde::Deserialize)]
    struct Selectors {
        texture_indices_by_slot: [Option<usize>; 14],
    }
    let layers = crate::field_resources::binding::members(root, package_directory)?;
    ensure!(
        layers.first().is_some_and(Option::is_some),
        "missing character primary layer"
    );
    let mut parts = Vec::new();
    for (index, directory) in layers.iter().take(2).enumerate() {
        let Some(directory) = directory else { continue };
        let (mut part, mut glb) = binding::model(root, directory)
            .with_context(|| format!("{name} reference layer {index}"))?;
        part.resource = index.try_into()?;
        crate::model_preview::style(&mut part, index != 0, false);
        if !clips.is_empty() {
            let model = binding::bindings(root, directory, &part)?;
            crate::model_preview::bind_clips(&mut part, &mut glb, &model, None, clips)?;
        }
        part.secondary_motion = binding::secondary_motion(root, directory, &glb, &part)?;
        part.mesh = binding::write_mesh(root, &glb)?;
        parts.push(part);
    }
    for (index, (part, directory)) in parts
        .iter_mut()
        .zip(layers.into_iter().flatten())
        .enumerate()
    {
        let selectors = format!("{directory}/texture-selectors.json");
        let channels = if root.join(&selectors).try_exists()? {
            binding::read::<Selectors>(root, &selectors)?.texture_indices_by_slot
        } else {
            [None; 14]
        };
        let textures = crate::texture::read(&root.join(&directory).join("palettes/textures.json"))?;
        part.appearance = Some(appearance(index, channels, &textures)?);
    }
    Ok(parts)
}

fn appearance(
    index: usize,
    channels: [Option<usize>; 14],
    textures: &crate::texture::Catalogue,
) -> Result<resonance_content::AppearanceTextures> {
    let variant = channels[2]
        .map(|texture| -> Result<_> {
            let [width, height] = textures
                .textures
                .get(texture)
                .and_then(Option::as_ref)
                .context("character atlas channel exceeds textures")?
                .dimensions;
            let frames = match (width == 128, height) {
                (true, 256) | (false, 512) => 2,
                (true, 512) | (false, 1024) => 4,
                _ => return Ok(None),
            };
            Ok(Some(resonance_content::AtlasChannel { texture, frames }))
        })
        .transpose()?
        .flatten();
    Ok(resonance_content::AppearanceTextures {
        eyes: (index == 0).then_some(channels[0]).flatten(),
        mouth: (index == 0).then_some(channels[1]).flatten(),
        variant,
        costume: (index == 0).then_some(channels[4]).flatten(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::sections;
    use std::fs;

    #[test]
    fn field_texture_sources_keep_local_ids_and_share_recovered_palettes() -> Result<()> {
        let palette = [0x0020af30_u32, 0, 12]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        let decoded = Arc::new(crate::texture::decode_source(&palette)?);
        let mut recovered = crate::scene::recovered::RecoveredModels::default();
        recovered.remember_textures(Arc::clone(&decoded));
        let mut sources = Sources::default();
        sources.insert(330, palette.clone(), Some(&recovered))?;
        sources.insert(331, palette, Some(&recovered))?;
        assert!(Arc::ptr_eq(&sources.textures[&330], &decoded));
        assert!(Arc::ptr_eq(&sources.textures[&331], &decoded));
        Ok(())
    }

    #[test]
    fn field_model_bank_uses_its_id_table_and_checks_aliases_and_ranges() {
        // ID order differs from ID value; offsets can alias the same package.
        let mut data: Vec<_> = [4u32, 20, 28, 36, 28, 0x014a0030, 0x00350000, 31, 7, 31, 9]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        let models = field_models(&data).unwrap();
        assert_eq!(
            models.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [330, 48, 53]
        );
        assert_eq!(models[0].1, models[2].1);
        assert_eq!(word(models[1].1, 4).unwrap(), 9);
        data[16..20].copy_from_slice(&0xfffcu32.to_be_bytes());
        assert!(field_models(&data).is_err());
        data[16..20].copy_from_slice(&28u32.to_be_bytes());
        data[24..26].copy_from_slice(&330u16.to_be_bytes());
        assert!(field_models(&data).is_err());
        assert!(field_models(&[0, 0, 0, 1, 0, 0, 0, 0]).unwrap().is_empty());

        // The file extent, rather than a roster-sized limit, bounds the table.
        let count = 513;
        let ids = 4 + count * 4;
        let model = ids + (count - 1) * 2;
        let mut data = vec![0; model + 4];
        data[..4].copy_from_slice(&(count as u32).to_be_bytes());
        for index in 0..count {
            let offset = if index == 0 { ids } else { model };
            data[4 + index * 4..8 + index * 4].copy_from_slice(&(offset as u32).to_be_bytes());
        }
        for index in 0..count - 1 {
            data[ids + index * 2..ids + index * 2 + 2]
                .copy_from_slice(&(index as u16).to_be_bytes());
        }
        assert_eq!(field_model_entries(&data).unwrap().len(), count - 1);
        data[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(field_model_count(&data).is_err());
    }
    #[test]
    fn archive_fallback_checks_counts_and_payload_ranges() {
        let data: Vec<u8> = [2u32, 20, 4, 0, 0, 0x12345678]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        assert_eq!(
            archive_entry(&data, 1).unwrap(),
            &0x12345678u32.to_be_bytes()
        );
        assert!(archive_entry(&data, 2).is_err());
        assert!(archive_entry(&data[..23], 0).is_err());
    }

    #[test]
    #[ignore = "requires both original extracted discs; no mesh or texture conversion"]
    fn original_shared_clips_bind_to_all_party_defaults_and_npc_models() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        for disc in ["disc1", "disc2"] {
            let root = local.join("extracted").join(disc);
            let files = root.join("files");
            let catalogue = crate::resource::read(&fs::read(root.join("sys/main.dol"))?)?;
            let declarations = [0x10004, 0x10012, 0x10802, 0x10a20, 0x2000d, 0x30030].into();
            let mut resources = Resources::open(&root, &catalogue)?;
            let sources = Sources::read(&mut resources, &declarations)?;
            assert_eq!(
                sources.packages.keys().copied().collect::<Vec<_>>(),
                [0x2000d]
            );
            assert_eq!(sources.animations.len(), 5);
            assert!(sources.textures.is_empty());
            assert_eq!(catalogue.party_bodies.len(), 9);
            let read = |name| {
                fs::read(files.join(crate::field_resources::resolve_path(&files, name)?))
                    .map_err(anyhow::Error::from)
            };
            let motions = sources
                .animations
                .iter()
                .map(|(id, authored)| {
                    let id = *id;
                    let archive = read(catalogue.source(id)?)?;
                    let bytes = decode_clip(archive_entry(&archive, (id & 0xffff) as usize)?)?;
                    let json = crate::animation::unbound_indexed(&bytes)?;
                    assert_eq!(serde_json::to_value(authored.as_ref())?, json);
                    let duration = json["duration_frames"]
                        .as_f64()
                        .context("motion duration")? as f32;
                    Ok((id, duration, authored, bytes))
                })
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(
                motions.iter().map(|(id, ..)| *id).collect::<BTreeSet<_>>(),
                sources.animations.iter().map(|(id, _)| *id).collect()
            );
            let mut packages: BTreeMap<_, _> = sources
                .packages
                .keys()
                .map(|&id| {
                    let archive = read(catalogue.source(id)?)?;
                    Ok((
                        id,
                        archive_entry(&archive, (id & 0xffff) as usize)?.to_vec(),
                    ))
                })
                .collect::<Result<_>>()?;
            for id in 1..=catalogue.party_bodies.len() as u32 {
                packages.insert(
                    id,
                    read(catalogue.party(crate::resource::PartyResource::Body, id as u8, 0)?)?,
                );
                let field_motion = read(catalogue.field_motion(id as u8)?)?;
                section(&field_motion, &sections(&field_motion)?, 2)?;
                let service = read(catalogue.field_service(id as u8)?)?;
                let ranges = sections(&service)?;
                let doors = [20, 24]
                    .into_iter()
                    .map(|slot| {
                        Ok((
                            slot,
                            serde_json::from_value(crate::animation::unbound_indexed(
                                &decode_clip(section(&service, &ranges, (slot - 4) / 4)?)?,
                            )?)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let service_id = resonance_content::field::DOOR_MOTION_RESOURCE_BASE + id;
                let extra = doors.iter().chain(&doors).map(|(slot, animation)| Clip {
                    slot: *slot as u16,
                    resource: Some(service_id),
                    animation,
                });
                let clips = unique_clips(extra)?;
                assert_eq!(clips.len(), 2);
                assert_eq!(
                    clips
                        .iter()
                        .filter(|c| c.resource == Some(service_id))
                        .map(|c| c.slot)
                        .collect::<Vec<_>>(),
                    [20, 24]
                );
            }
            assert_eq!(packages.len(), 10);
            for (id, package) in packages {
                let layers = sections(&package)?;
                for range in layers.into_iter().take(2).flatten() {
                    let (_, model) = crate::geometry::model_resource(&package[range])?;
                    let range = crate::geometry::skeleton_range(model)?;
                    let bindings = crate::animation::ModelBindings::read(&model[range])?;
                    let node_count = bindings.node_ids.len();
                    for (resource, duration, authored, bytes) in &motions {
                        let motion = authored.motion(&bindings).with_context(|| {
                            format!("{disc}: animation {resource:#x} on model {id:#x}")
                        })?;
                        assert_eq!(
                            serde_json::to_value(&motion)?,
                            serde_json::to_value(bindings.motion(bytes)?)?
                        );
                        assert_eq!(motion.duration_frames, *duration);
                        assert!(
                            motion
                                .tracks
                                .iter()
                                .all(|t| usize::from(t.bone) < node_count)
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original disc; prepares actors without physical intermediates or launching the game"]
    fn original_field_actors_publish_shared_clips_for_every_model() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let extracted = local.join("extracted/disc1");
        let staging = tempfile::tempdir()?;
        let output = staging.path();
        fs::write(
            output.join("sources.json"),
            b"not an intermediate resource catalogue",
        )?;
        for map_id in [330, 340] {
            let source = crate::field::source_for_id(&extracted, map_id)?;
            let map = crate::field::MapArchive::open(&source)?;
            let physical = binding::Map::open(output, &source)?;
            let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
            let declared = crate::field_resources::declarations(map.section(6)?)?;
            let mut resources =
                crate::field_resources::binding::Resources::open(&extracted, &catalogue)?;
            let mut sources = Sources::read(&mut resources, &declared.resources)?;
            sources.include_field(&physical, None)?;
            let shared = &sources.animations;
            let shared_ids: BTreeSet<_> = shared.iter().map(|(id, _)| *id).collect();
            let actors = cook_field(
                output,
                map_id,
                &physical,
                &sources,
                shared,
                &mut resources,
                &declared,
            )?
            .actors;
            assert!((1..=9).all(|id| actors.iter().any(|actor| actor.resource == id)));
            assert!(actors.iter().any(|actor| actor.resource > 9));
            let mut durations = BTreeSet::new();
            for actor in actors {
                for part in actor.parts {
                    let glb = crate::scene::glb::Glb::read(&output.join(&part.mesh))?;
                    assert!(glb.json.get("animations").is_none());
                    for clip in &part.clips {
                        let motion = resonance_content::animation::Motion::decode(&fs::read(
                            output.join(&clip.motion),
                        )?)?;
                        assert_eq!(
                            clip.secondary_pose_nodes,
                            motion
                                .tracks
                                .iter()
                                .filter(|track| track.times.len() > 2)
                                .map(|track| track.bone)
                                .collect::<Vec<_>>()
                        );
                        assert_eq!(
                            motion.duration_frames / resonance_content::animation::FRAME_HZ,
                            clip.duration_seconds
                        );
                    }
                    let keys: BTreeSet<_> = part
                        .clips
                        .iter()
                        .map(|clip| {
                            (
                                clip.animation_resource.unwrap_or(actor.resource),
                                clip.resource_slot,
                            )
                        })
                        .collect();
                    assert_eq!(keys.len(), part.clips.len());
                    assert_eq!(
                        part.clips
                            .iter()
                            .filter_map(|clip| clip.animation_resource)
                            .filter(|id| shared_ids.contains(id))
                            .collect::<BTreeSet<_>>(),
                        shared_ids
                    );
                    if map_id == 330 {
                        assert!(
                            !part
                                .clips
                                .iter()
                                .any(|clip| clip.animation_resource == Some(0x30030))
                        );
                        continue;
                    }
                    let clip = part
                        .clips
                        .iter()
                        .find(|clip| {
                            clip.animation_resource == Some(0x30030) && clip.resource_slot == 12
                        })
                        .context("shared motion missing from actor layer")?;
                    durations.insert(clip.duration_ticks());
                    assert!(
                        clip.secondary_pose_nodes
                            .iter()
                            .all(|&node| usize::from(node) < part.bone_names.len())
                    );
                }
            }
            assert_eq!(durations.len(), usize::from(map_id == 340));
        }
        Ok(())
    }
}
