//! Native actor-to-motion bindings contributed by a field's selected banks.
use crate::{
    character_attachments, character_models, dol, field_actors, field_core, field_npc_models,
    mesh_mappings, shared_animations, text, world_gpl,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::document::{AnimationBinding, AnimationJoint, Model, ModelMappings, ModelTarget},
    secondary_motion::Definition,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Clone)]
struct Rig {
    name: String,
    joints: Vec<(u16, String, String)>,
    parts: Vec<u16>,
    dynamics: Definition,
}

#[derive(Default)]
pub(crate) struct Bindings {
    rigs: BTreeMap<u32, Vec<Rig>>,
    labels: BTreeMap<u32, BTreeMap<u16, String>>,
}

fn source_labels(
    extracted: &Path,
    library: &Path,
    id: u32,
    name: &str,
    native: &[u8],
    cache: &mut Bindings,
) -> Result<BTreeMap<u16, String>> {
    if let Some(labels) = cache.labels.get(&id) {
        return Ok(labels.clone());
    }
    let labels = shared_animations::track_names(extracted, library, native, name)
        .with_context(|| format!("animation {id} ({name}) source labels"))?;
    cache.labels.insert(id, labels.clone());
    Ok(labels)
}

fn selected_rigs(
    extracted: &Path,
    field_id: u32,
    library: &Path,
    banks: &BTreeMap<u32, String>,
    cache: &mut Bindings,
) -> Result<Vec<Rig>> {
    let models = mesh_mappings::mesh_mappings()?.0;
    let mut selected = field_actors::declared_static_resources(extracted, field_id)?
        .into_iter()
        .filter(|id| models.contains_key(id) && !banks.contains_key(id))
        .collect::<BTreeSet<_>>();
    // The field loader offers every party costume, and Colette's equipped wings
    // are a service model even when the script does not request them explicitly.
    selected.extend(
        models
            .keys()
            .copied()
            .filter(|id| (1..=9).contains(id) || id >> 24 == 0x23),
    );
    selected.insert(0x2200_0000);
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let mut rigs = Vec::new();
    for id in selected {
        if let Some(rig) = cache.rigs.get(&id) {
            rigs.extend(rig.clone());
            continue;
        }
        let model: &ModelTarget = models
            .get(&id)
            .with_context(|| format!("unmapped model {id}"))?;
        let mut outline_joints = None;
        let (joints, parts) = if id == 0x2200_0000 {
            let joints = character_attachments::wing_animation_bindings(extracted, &exe)?;
            let names = world_gpl::animation_target_bindings(joints.clone());
            let named = joints
                .into_iter()
                .zip(names)
                .map(|((id, source), (_, name))| (id, name, source))
                .collect();
            (named, vec![0])
        } else if (1..=9).contains(&id) || id >> 24 == 0x23 {
            let character = if id <= 9 { id as u8 } else { (id & 0xff) as u8 };
            let variant = if id <= 9 { 0 } else { ((id >> 8) & 0xff) as u8 };
            let body = character_models::party_body(extracted, character, variant)?;
            (world_gpl::named_animation_bindings(&body)?, vec![0, 1])
        } else {
            let package = field_actors::read_shared_resource(extracted, id)?;
            let (body, parts) = if world_gpl::is_model(&package) {
                (&package[..], vec![0])
            } else {
                let body = field_core::optional_section(&package, 0)
                    .with_context(|| format!("field {field_id} model {id} body section"))?
                    .with_context(|| format!("model {id} has no native body"))?;
                let parts = if let Some(outline) = field_core::optional_section(&package, 1)
                    .with_context(|| format!("field {field_id} model {id} outline section"))?
                {
                    outline_joints = Some(world_gpl::named_animation_bindings(outline)?);
                    vec![0, 1]
                } else {
                    vec![0]
                };
                (body, parts)
            };
            (world_gpl::named_animation_bindings(body)?, parts)
        };
        let dynamics =
            if model.name().starts_with("characters/") && !model.name().contains("/wings") {
                let path = format!("meshes/{}/model.json", model.model());
                let model: Model = serde_json::from_slice(
                    &fs::read(library.join(&path))
                        .with_context(|| format!("read actor dynamics from {path}"))?,
                )?;
                model
                    .parts
                    .into_iter()
                    .next()
                    .context("actor model has no body")?
                    .secondary_motion
            } else {
                Definition::default()
            };
        let mut rig = Rig {
            name: model.name().to_owned(),
            joints,
            parts,
            dynamics,
        };
        let mut variants = Vec::new();
        // Some native outlines have distinct joint labels; a body map cannot
        // target both parts even when their binding IDs are identical.
        if let Some(joints) = outline_joints.filter(|joints| joints != &rig.joints) {
            rig.parts = vec![0];
            let mut outline = rig.clone();
            outline.joints = joints;
            outline.parts = vec![1];
            variants.push(outline);
        }
        variants.push(rig);
        cache.rigs.insert(id, variants.clone());
        rigs.extend(variants);
    }
    Ok(rigs)
}

pub(crate) fn global_rigs(
    extracted: &Path,
    library: &Path,
) -> Result<BTreeMap<String, Vec<AnimationJoint>>> {
    let mut sources = Vec::new();
    for (path, standalone) in [
        ("meshes/mappings.json", false),
        ("meshes/standalone/mappings.json", true),
    ] {
        let mappings: ModelMappings = serde_json::from_slice(&fs::read(library.join(path))?)?;
        sources.extend(
            mappings
                .0
                .into_iter()
                .map(|(id, target)| (id, target, standalone)),
        );
    }
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let effects = field_npc_models::grouped_bank(extracted, &exe, 1)?;
    let npcs = field_npc_models::native_bank(extracted, &exe)?;
    let wanted = sources
        .iter()
        .filter(|(id, _, standalone)| !standalone && *id > 9 && *id < 65536)
        .map(|(id, _, _)| *id)
        .collect();
    let mut embedded = field_npc_models::field_sources(extracted, &exe, &wanted)?;
    let mut output = BTreeMap::new();
    for (id, target, standalone) in sources {
        let model: Model = serde_json::from_slice(&fs::read(
            library.join(format!("meshes/{}/model.json", target.model())),
        )?)?;
        if model
            .parts
            .iter()
            .all(|part| output.contains_key(&part.mesh))
        {
            continue;
        }
        let package = if !standalone && ((1..=9).contains(&id) || id >> 24 == 0x23) {
            character_models::party_package(
                extracted,
                id as u8,
                if id <= 9 { 0 } else { (id >> 8) as u8 },
            )?
        } else if !standalone && field_npc_models::service_source(id).is_some() {
            field_npc_models::service_package(extracted, &exe, id)?
        } else if let Some(package) = embedded.remove(&id).filter(|_| !standalone) {
            package
        } else if id == 0x2200_0000 {
            let joints = character_attachments::wing_animation_bindings(extracted, &exe)?;
            let names = world_gpl::animation_target_bindings(joints.clone());
            output.insert(
                model.parts[0].mesh.clone(),
                joints
                    .into_iter()
                    .zip(names)
                    .map(|((id, source), (_, name))| AnimationJoint {
                        id,
                        name,
                        source: Some(source),
                        animated: false,
                    })
                    .collect(),
            );
            continue;
        } else if id >> 16 == 1 {
            field_npc_models::expanded_member(field_npc_models::grouped_member(&effects, id)?)?
        } else if id >> 16 == 2 {
            field_npc_models::member(&npcs, id)?.to_vec()
        } else {
            field_actors::read_shared_resource(extracted, id)?
        };
        for part in model.parts {
            if output.contains_key(&part.mesh) {
                continue;
            }
            let body = if world_gpl::is_model(&package) {
                &package[..]
            } else {
                field_core::optional_section(&package, usize::from(part.resource))?
                    .context("animation rig has no native part")?
            };
            output.insert(
                part.mesh,
                world_gpl::named_animation_bindings(body)?
                    .into_iter()
                    .map(|(id, name, source)| AnimationJoint {
                        id,
                        name,
                        source: Some(source),
                        animated: false,
                    })
                    .collect(),
            );
        }
    }
    Ok(output)
}

/// Field-owned models exist after the scene pass; merge their native joint
/// selections once so dynamically requested banks work on these actors too.
pub fn cook_local_animation_rigs(extracted: &Path, library: &Path) -> Result<()> {
    use resonance_content::field::document::{Actors, SharedBankBindings};
    let path = library.join("animations/bindings.json");
    let mut bindings: SharedBankBindings = serde_json::from_slice(&fs::read(&path)?)?;
    let fields: BTreeMap<u32, String> =
        serde_json::from_slice(&fs::read(library.join("fields/mappings.json"))?)?;
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let fields = fields.into_iter().collect::<Vec<_>>();
    let workers = crate::parallel::workers();
    for (batch_index, batch) in fields.chunks(workers).enumerate() {
        let prepared = crate::parallel::map(batch, |index, (id, name)| {
            let id = *id;
            eprintln!(
                "  [{}/{} jobs] Local rigs: {name} ({id})",
                batch_index * workers + index + 1,
                fields.len()
            );
            let mut field_rigs = Vec::new();
            let actors: Actors = serde_json::from_slice(
                &fs::read(library.join(format!("fields/{name}/actors.json")))
                    .with_context(|| format!("read field {id} ({name}) actors"))?,
            )?;
            let locals = actors
                .models
                .into_iter()
                .filter_map(|model| match model {
                    resonance_content::field::document::ModelBinding::Local(actor)
                        if resonance_content::field::LOCAL_MODEL_RESOURCES
                            .contains(&actor.resource) =>
                    {
                        Some(actor)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if locals.is_empty() {
                return Ok(Vec::new());
            }
            let row = dol::slice(&exe, field_core::PHASES + id * 24, 24)?;
            let archive = field_core::native_archive(extracted, &exe, row, id as usize)?
                .with_context(|| format!("field {id} ({name}) local rig archive missing"))?;
            for (index, actor) in locals.iter().enumerate() {
                eprintln!(
                    "    [{}/{} models] {} ({:#x})",
                    index + 1,
                    locals.len(),
                    actor.model,
                    actor.resource
                );
                let section = 16
                    + (actor.resource - resonance_content::field::LOCAL_MODEL_RESOURCES.start)
                        as usize;
                let package = field_core::optional_section(&archive, section)?
                .with_context(|| format!("field {id} ({name}) local resource {:#x}, section {section}, model {} has no package", actor.resource, actor.model))?;
                let model: Model = serde_json::from_slice(&fs::read(library.join(&actor.model))?)?;
                for part in model.parts {
                    let body = if world_gpl::is_model(package) {
                        package
                    } else {
                        field_core::optional_section(package, usize::from(part.resource))?
                            .context("local rig part missing")?
                    };
                    let names = resonance_content::animation::joint_names_from_glb(&fs::read(
                        library.join(&part.mesh),
                    )?)?;
                    let native = world_gpl::named_animation_bindings(body)?;
                    ensure!(
                        native.len() == names.len(),
                        "local native rig differs from {}",
                        part.mesh
                    );
                    field_rigs.push((
                        part.mesh,
                        native
                            .into_iter()
                            .zip(names)
                            .map(|((id, _, source), name)| AnimationJoint {
                                id,
                                name,
                                source: Some(source),
                                animated: false,
                            })
                            .collect(),
                    ));
                }
            }
            Ok(field_rigs)
        })?;
        for (mesh, joints) in prepared.into_iter().flatten() {
            bindings.rigs.insert(mesh, joints);
        }
    }
    fs::write(path, resonance_asset_writer::json::encode(&bindings)?)?;
    Ok(())
}

pub(crate) struct Track {
    pub(crate) id: u16,
    pub(crate) name: Option<String>,
}

pub(crate) fn source_tracks(native: &[u8]) -> Result<Vec<Track>> {
    let count = usize::from(dol::u16(native, 12)?);
    let names_present = dol::u32(native, 16)? != 0 && dol::u32(native, 20)? != 0;
    let names = (|| -> Result<Vec<String>> {
        ensure!(names_present, "motion has no name table");
        let mut cursor = dol::u32(native, 20)? as usize;
        (0..count)
            .map(|_| {
                let tail = native
                    .get(cursor..)
                    .context("animation names exceed source")?;
                let end = tail
                    .iter()
                    .position(|&byte| byte == 0)
                    .context("unterminated animation joint name")?;
                let (label, invalid) =
                    encoding_rs::SHIFT_JIS.decode_without_bom_handling(&tail[..end]);
                ensure!(!invalid, "invalid native animation joint name");
                cursor += end + 1;
                Ok(label.into_owned())
            })
            .collect()
    })()
    .ok();
    (0..count)
        .map(|index| {
            Ok(Track {
                id: dol::u16(native, 36 + index * 16 + 10)?,
                name: names.as_ref().map(|names| names[index].clone()),
            })
        })
        .collect()
}

/// The native clip selects by exact original names when the two rigs share any;
/// otherwise its track IDs address the model's original binding IDs.
pub(crate) fn selected_tracks(
    joints: &[(u16, String, String)],
    tracks: &[Track],
) -> Vec<Option<usize>> {
    let by_name = tracks
        .iter()
        .filter_map(|track| track.name.as_deref())
        .any(|name| joints.iter().any(|(_, _, source)| source == name));
    let targets = tracks
        .iter()
        .map(|track| {
            if by_name {
                track
                    .name
                    .as_ref()
                    .and_then(|name| joints.iter().find(|(_, _, source)| source == name))
                    .map(|(id, _, _)| *id)
            } else {
                Some(track.id)
            }
        })
        .collect::<Vec<_>>();
    joints
        .iter()
        .map(|(id, _, _)| targets.iter().position(|target| *target == Some(*id)))
        .collect()
}

/// Read one field's reusable animation selections and their native actor rigs.
/// The caller merges these rows across fields into the single library document.
pub(crate) fn native_bank_bindings(
    extracted: &Path,
    library: &Path,
    field_id: u32,
    cache: &mut Bindings,
) -> Result<BTreeMap<u32, Vec<AnimationBinding>>> {
    ensure!(
        (field_id as usize) < field_core::FIELD_COUNT,
        "field ID {field_id} is outside native catalogue"
    );
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let rows = dol::slice(&exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let row = &rows[field_id as usize * 24..][..24];
    let pointer = dol::u32(row, 0)?;
    if pointer == 0 {
        return Ok(BTreeMap::new());
    }
    let source = text::string(&exe, pointer)?;
    ensure!(
        !source.is_empty() && !source.contains(['/', '\\']) && source != "." && source != "..",
        "invalid native field source {source:?}"
    );
    if !extracted
        .join("disc1/files/MAP")
        .join(source.to_ascii_lowercase())
        .is_file()
    {
        return Ok(BTreeMap::new());
    }
    let banks = field_actors::declared_animation_banks(extracted, field_id)?;
    if banks.is_empty() {
        return Ok(BTreeMap::new());
    }
    let rigs = selected_rigs(extracted, field_id, library, &banks, cache)?;
    let mut result = BTreeMap::new();
    for (id, name) in banks {
        let native = shared_animations::read_bank(extracted, Some(field_id), id)?;
        let tracks = source_tracks(&native)?;
        let labels = source_labels(extracted, library, id, &name, &native, cache)?;
        let target = tracks
            .iter()
            .map(|track| labels.get(&track.id).cloned())
            .collect::<Vec<_>>();
        let automatic = target.iter().flatten().collect::<BTreeSet<_>>();
        let path = shared_animations::mapped_motion_path(library, id)?;
        let mut groups = BTreeMap::<
            (Vec<u16>, BTreeMap<String, Option<String>>, Vec<String>),
            Vec<String>,
        >::new();
        for rig in &rigs {
            let selections = selected_tracks(&rig.joints, &tracks);
            let joints = rig
                .joints
                .iter()
                .zip(&selections)
                .filter_map(|((_, actor, _), track)| {
                    let desired = track.and_then(|index| target[index].clone());
                    let natural = automatic.contains(actor);
                    if desired.as_deref() == natural.then_some(actor.as_str()) {
                        None
                    } else {
                        Some((actor.clone(), desired))
                    }
                })
                .collect();
            let pose = character_models::pose_chains(
                &native,
                &rig.joints
                    .iter()
                    .map(|(id, _, source)| (*id, source.clone()))
                    .collect::<Vec<_>>(),
                &rig.joints
                    .iter()
                    .map(|(_, friendly, _)| friendly.clone())
                    .collect::<Vec<_>>(),
                &rig.dynamics,
            )?;
            groups
                .entry((rig.parts.clone(), joints, pose))
                .or_default()
                .push(rig.name.clone());
        }
        let rows = groups
            .into_iter()
            .map(|((parts, joints, pose_chains), mut actors)| {
                actors.sort();
                AnimationBinding {
                    actors,
                    parts,
                    motion: path.clone(),
                    joints,
                    resource_slot: 12,
                    pose_chains,
                }
            })
            .collect();
        result.insert(id, rows);
    }
    Ok(result)
}
