//! Script-selected field actors and animation banks.
use crate::{
    character_models, compression, dol, field_core, field_npc_models, mesh_mappings,
    native_archive, shared_animations, text, world_gpl, world_motion,
};
use anyhow::{Context, Result, ensure};
use resonance_content::field::document::{Actor, Actors, AnimationBinding, ModelBinding};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_script::{NativeCall, Op, Program, scenario};

const GROUPS: u32 = 0x801f_86b8;
const STANDALONE: u32 = 0x801f_85e4;
const LOCAL_BASE: u32 = 0xffee_0000;

fn source_name(exe: &[u8], id: u32) -> Result<String> {
    let address = if id >> 16 == 0 {
        STANDALONE + id * 4
    } else {
        GROUPS + ((id >> 16) - 1) * 12
    };
    let name = text::pointer(exe, address)?;
    ensure!(
        !name.is_empty() && !name.contains(['/', '\\']),
        "invalid native resource name {name:?}"
    );
    Ok(name)
}

fn resource(extracted: &Path, exe: &[u8], id: u32) -> Result<Vec<u8>> {
    let name = source_name(exe, id)?;
    let bytes = native_archive::read_named(&extracted.join("disc1/files"), &name)?;
    let bytes = if id >> 16 == 0 {
        bytes
    } else {
        let count = dol::u32(&bytes, 0)? as usize;
        let index = (id & 0xffff) as usize;
        ensure!(
            index < count && count <= bytes.len().saturating_sub(4) / 8,
            "resource {id} exceeds native archive"
        );
        let mut at = 4 + index * 8;
        if dol::u32(&bytes, at + 4)? == 0 {
            at = 4;
        }
        let start = dol::u32(&bytes, at)? as usize;
        let size = dol::u32(&bytes, at + 4)? as usize;
        ensure!(
            start >= 4 + count * 8 && size > 0,
            "resource {id} has no archive member"
        );
        bytes
            .get(start..start.checked_add(size).context("resource size overflow")?)
            .with_context(|| format!("resource {id} exceeds native archive"))?
            .to_vec()
    };
    if bytes.starts_with(b"MSCF") {
        compression::cabinet(&bytes)
    } else if bytes
        .first()
        .is_some_and(|value| matches!(value, 0 | 1 | 3))
        && bytes.len() >= 9
        && u32::from_le_bytes(bytes[1..5].try_into()?) as usize + 9 == bytes.len()
    {
        compression::decode(&bytes)
    } else {
        Ok(bytes)
    }
}

/// Resolve a native script resource without exposing archive structure to callers.
pub(crate) fn read_shared_resource(extracted: &Path, id: u32) -> Result<Vec<u8>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    resource(extracted, &exe, id)
}

fn declarations_with_unresolved(script: &[u8], allow_unresolved: bool) -> Result<BTreeSet<u32>> {
    let program = Program::decode(script)?;
    let analysis = scenario::analyze(script)?;
    let call = NativeCall::ResolveScriptResource;
    let args = scenario::calls::analyzed_arguments(&program, &analysis, call, 1)?;
    let finite = if args.values().any(|values| values[0].is_none()) {
        scenario::calls::finite_arguments(&program, &analysis, call)
    } else {
        BTreeMap::new()
    };
    let mut ids = BTreeSet::new();
    for (pc, value) in args {
        if let Some(id) = value[0] {
            ids.insert(u32::try_from(id)?);
        } else if let Some(possible) = finite.get(&pc) {
            for &id in possible {
                ids.insert(u32::try_from(id)?);
            }
        } else if !allow_unresolved {
            anyhow::bail!("dynamic field resource selector at script PC {pc:#x}");
        }
    }
    Ok(ids)
}

fn declarations(script: &[u8]) -> Result<BTreeSet<u32>> {
    declarations_with_unresolved(script, false)
}

fn service_models(script: &[u8]) -> Result<Vec<u32>> {
    let program = Program::decode(script)?;
    let analysis = scenario::analyze(script)?;
    let mut save_point = false;
    let mut treasures = false;
    for &pc in analysis.instructions.keys() {
        let Some((op, _)) = program.instruction(pc) else {
            continue;
        };
        save_point |= [
            NativeCall::CreateSavePoint,
            NativeCall::CreateSealedSavePoint,
        ]
        .into_iter()
        .any(|call| op == Op::Native(call as u8));
        treasures |= op == Op::Native(NativeCall::CreateTreasureChest as u8);
    }
    let mut resources = Vec::new();
    if save_point {
        resources.push(resonance_content::field::SAVE_POINT_RESOURCE);
    }
    if treasures {
        resources
            .extend((0..3).map(|kind| resonance_content::field::TREASURE_RESOURCE_BASE + kind));
    }
    Ok(resources)
}

pub(crate) fn animation_name(id: u32, names: &BTreeMap<u32, String>) -> String {
    names.get(&id).cloned().unwrap_or_else(|| {
        let group = id >> 16;
        if (3..=11).contains(&group) {
            let party = [
                "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal",
                "kratos",
            ];
            format!(
                "characters/{}/unassigned-motion-{}",
                party[group as usize - 3],
                id as u16
            )
        } else {
            format!("unassigned/motion-{id}")
        }
    })
}

fn embedded_animation_banks(
    archive: &[u8],
    names: &BTreeMap<u32, String>,
) -> Result<BTreeMap<u32, String>> {
    let Some(bank) = field_core::optional_section(archive, 7)? else {
        return Ok(BTreeMap::new());
    };
    field_npc_models::field_model_entries(bank)?
        .into_iter()
        .filter_map(|(id, bytes)| {
            (world_motion::indexed_joints(bytes).is_ok())
                .then(|| (u32::from(id), animation_name(u32::from(id), names)))
        })
        .map(Ok)
        .collect()
}

/// Names of the native shared animation banks selected by one field script.
pub(crate) fn declared_animation_banks(extracted: &Path, id: u32) -> Result<BTreeMap<u32, String>> {
    let (exe, archive) = field_core::read(extracted, id)?;
    animation_banks(extracted, &exe, &archive)
}

pub(crate) fn animation_banks(
    extracted: &Path,
    exe: &[u8],
    archive: &[u8],
) -> Result<BTreeMap<u32, String>> {
    let script = field_core::optional_section(&archive, 6)?.context("field has no script")?;
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("field-animation-names.json"))?;
    let local = field_core::optional_section(&archive, 7)?
        .map(field_npc_models::field_model_entries)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|(id, _)| u32::from(id))
        .collect::<BTreeSet<_>>();
    // A dynamic selector has no finite bank row to publish. Preserve all
    // statically known selections; actor cooking still requires complete IDs.
    let mut selected = declarations_with_unresolved(script, true)?
        .into_iter()
        .filter(|resource| !local.contains(resource))
        .map(|resource_id| {
            let bytes = resource(extracted, &exe, resource_id)?;
            Ok((dol::u32(&bytes, 0)? == 0x007b_7960)
                .then(|| (resource_id, animation_name(resource_id, &names))))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeMap<_, _>>();
    for (id, name) in embedded_animation_banks(&archive, &names)? {
        if let Some(previous) = selected.insert(id, name.clone()) {
            ensure!(previous == name, "conflicting embedded animation {id}");
        }
    }
    Ok(selected)
}

/// Resource selectors from every static script branch, before type filtering.
pub(crate) fn declared_resources(extracted: &Path, id: u32) -> Result<BTreeSet<u32>> {
    let (_, archive) = field_core::read(extracted, id)?;
    let script = field_core::optional_section(&archive, 6)?.context("field has no script")?;
    declarations(script)
}

/// Static and finite resource selections for a precomputed bank table. Runtime
/// actor publication uses `declared_resources` and rejects unresolved selectors.
pub(crate) fn declared_static_resources(extracted: &Path, id: u32) -> Result<BTreeSet<u32>> {
    let (_, archive) = field_core::read(extracted, id)?;
    let script = field_core::optional_section(&archive, 6)?.context("field has no script")?;
    declarations_with_unresolved(script, true)
}

fn local_bindings(
    extracted: &Path,
    exe: &[u8],
    archive: &[u8],
    root: &str,
    models: &BTreeMap<u32, resonance_content::field::document::ModelTarget>,
    actors: &mut Actors,
    indices: &[usize],
) -> Result<()> {
    let mut selected = BTreeMap::<String, (u32, String)>::new();
    for (&id, model) in models {
        if (1..=9).contains(&id)
            || id >> 24 == 0x23
            || actors.models.iter().any(
                |binding| matches!(binding, ModelBinding::Shared(name) if name == model.name()),
            )
        {
            selected.insert(model.name().to_owned(), (id, model.model().to_owned()));
        }
    }
    let npc_ids = selected
        .values()
        .map(|(id, _)| *id)
        .filter(|id| id >> 16 == 2)
        .collect::<Vec<_>>();
    let npc_bodies = field_npc_models::npc_native_bodies(extracted, &npc_ids)?;
    let field_bank = field_core::optional_section(archive, 7)?
        .map(field_npc_models::field_model_entries)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|(id, package)| (u32::from(id), package))
        .collect::<BTreeMap<_, _>>();
    let mut rigs = BTreeMap::new();
    for (name, (id, _owner)) in &selected {
        if name == "characters/colette/wings/equipped" {
            continue;
        }
        let (body, parts) = if (1..=9).contains(id) || id >> 24 == 0x23 {
            let character = if *id <= 9 {
                *id as u8
            } else {
                (*id & 0xff) as u8
            };
            let variant = if *id <= 9 {
                0
            } else {
                ((*id >> 8) & 0xff) as u8
            };
            (
                character_models::party_body(extracted, character, variant)?,
                vec![0, 1],
            )
        } else {
            let package = if let Some(package) = field_bank.get(id) {
                package.to_vec()
            } else if field_npc_models::service_source(*id).is_some() {
                field_npc_models::service_package(extracted, exe, *id)?
            } else if *id >> 16 == 0 {
                field_npc_models::standalone_package(extracted, exe, *id)?
            } else {
                resource(extracted, exe, *id)?
            };
            let bare = world_gpl::is_model(&package);
            let body = if *id >> 16 == 2 {
                npc_bodies
                    .get(id)
                    .with_context(|| format!("shared actor {id} has no native body"))?
                    .clone()
            } else if bare {
                package.clone()
            } else {
                field_core::optional_section(&package, 0)?
                    .with_context(|| format!("shared actor {id} has no native body"))?
                    .to_vec()
            };
            let parts = if !bare && field_core::optional_section(&package, 1)?.is_some() {
                vec![0, 1]
            } else {
                vec![0]
            };
            (body, parts)
        };
        let bindings = world_gpl::named_animation_bindings(&body)?;
        let raw = bindings
            .iter()
            .map(|(_, _, source)| source.clone())
            .collect::<Vec<_>>();
        let chains = character_models::character_dynamics::chain_roots(&raw);
        rigs.insert(name.clone(), (bindings, parts, chains));
    }
    if selected.contains_key("characters/colette/wings/equipped") {
        let source = crate::character_attachments::wing_animation_bindings(extracted, exe)?;
        let targets = world_gpl::animation_target_bindings(source.clone());
        let bindings = source
            .into_iter()
            .zip(targets)
            .map(|((id, raw), (_, target))| (id, target, raw))
            .collect();
        rigs.insert(
            "characters/colette/wings/equipped".into(),
            (bindings, vec![0], Vec::new()),
        );
    }
    for &index in indices {
        let Some(motion) = field_core::optional_section(archive, 16 + index)? else {
            continue;
        };
        if dol::u32(motion, 0)? != 0x007b_7960 {
            continue;
        }
        let joints = world_motion::indexed_joints(motion)?;
        let animated = world_motion::indexed_animated_joints(motion)?;
        let source_names = world_motion::indexed_track_names(motion)?;
        let mut groups =
            BTreeMap::<(Vec<(String, String)>, Vec<String>, Vec<u16>), Vec<String>>::new();
        for (name, (bindings, parts, chains)) in &rigs {
            let mut map = Vec::new();
            for (index, target, _) in bindings {
                if joints.binary_search(index).is_ok() {
                    map.push((
                        target.clone(),
                        source_names
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| format!("joint-{index}")),
                    ));
                }
            }
            if map.is_empty() {
                continue;
            }
            map.sort();
            let mut pose = chains
                .iter()
                .filter(|(raw, _)| {
                    bindings
                        .iter()
                        .any(|(id, _, source)| source == raw && animated.contains(id))
                })
                .map(|(_, name)| name.clone())
                .collect::<Vec<_>>();
            pose.sort();
            groups
                .entry((map, pose, parts.clone()))
                .or_default()
                .push(name.clone());
        }
        let target = actors
            .local_animations
            .entry(LOCAL_BASE + index as u32)
            .or_default();
        for ((joints, pose_chains, parts), mut names) in groups {
            names.sort();
            target.push(AnimationBinding {
                actors: names,
                parts,
                motion: format!("{root}/animations/local-motion-{index}.glb"),
                joints: joints
                    .into_iter()
                    .map(|(target, source)| (target, Some(source)))
                    .collect(),
                resource_slot: 12,
                pose_chains,
            });
        }
    }
    Ok(())
}

/// Convert one field's script IDs into its editable actor selections.
///
/// Shared models, mesh mappings/groups, and shared animation banks must be
/// cooked first. Ordinary fields with only shared actors need no special case.
/// Native field-owned actor packages are written before their selections.
pub fn cook_field_actors(extracted: &Path, library: &Path, id: u32) -> Result<()> {
    let (exe, archive) = field_core::read(extracted, id)?;
    cook_from_source(extracted, library, id, &exe, &archive)
}

pub(crate) fn cook_from_source(
    extracted: &Path,
    library: &Path,
    id: u32,
    exe: &[u8],
    archive: &[u8],
) -> Result<()> {
    let script = field_core::optional_section(&archive, 6)?.context("field has no script")?;
    let dynamic_resources = matches!(id, 544 | 545);
    let declared = declarations_with_unresolved(script, dynamic_resources)?;
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("field-animation-names.json"))?;
    let models = mesh_mappings::mesh_mappings()?;
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let mut actors = Actors::default();
    actors.dynamic_resources = dynamic_resources;
    if let Some(bank) = field_core::optional_section(&archive, 7)? {
        actors.local_resources.extend(
            field_npc_models::field_model_entries(bank)?
                .into_iter()
                .map(|(resource, _)| u32::from(resource)),
        );
    }
    let mut selected_banks = BTreeMap::new();
    actors.models.push(ModelBinding::Group {
        group: "party".into(),
    });
    // Native field model entries are loaded before script-selected resources.
    if id != 184 {
        if let Some(bank) = field_core::optional_section(&archive, 7)? {
            let mut resource_ids = field_npc_models::field_model_entries(bank)?
                .into_iter()
                .filter(|(_, bytes)| world_motion::indexed_joints(bytes).is_err())
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            resource_ids.sort_unstable();
            for resource_id in resource_ids {
                let model = models
                    .0
                    .get(&u32::from(resource_id))
                    .with_context(|| format!("field {id} has unnamed model {resource_id}"))?;
                if !actors.models.iter().any(
                    |binding| matches!(binding, ModelBinding::Shared(name) if name == model.name()),
                ) {
                    actors
                        .models
                        .push(ModelBinding::Shared(model.name().into()));
                }
            }
        }
    }
    for resource_id in declared {
        // MAP entries above already supplied the contextual model selection;
        // embedded motions are collected below from that same field bank.
        if actors.local_resources.contains(&resource_id) {
            continue;
        }
        if resource_id < 53 && !actors.local_resources.contains(&resource_id) {
            let standalone: resonance_content::field::document::ModelMappings =
                serde_json::from_slice(&fs::read(
                    library.join("meshes/standalone/mappings.json"),
                )?)?;
            if standalone.0.contains_key(&resource_id) {
                actors.models.push(ModelBinding::Local(Actor {
                    resource: resonance_content::field::STANDALONE_MODEL_RESOURCE_BASE
                        | resource_id,
                    model: standalone.model_path(resource_id)?,
                    hidden_geometry: None,
                }));
                continue;
            }
        }
        let bytes = resource(extracted, &exe, resource_id)?;
        if dol::u32(&bytes, 0)? == 0x007b_7960 {
            let name = animation_name(resource_id, &names);
            actors.animations.insert(name.clone());
            selected_banks.insert(resource_id, name);
        } else if let Some(model) = models.0.get(&resource_id) {
            if !actors.models.iter().any(
                |binding| matches!(binding, ModelBinding::Shared(name) if name == model.name()),
            ) {
                actors
                    .models
                    .push(ModelBinding::Shared(model.name().into()));
            }
        } else if dol::u32(&bytes, 0)? == 0x0020_af30 {
            // Script-selected TPL images belong to their visual owner,
            // not the actor catalogue (for example Iselia title art).
        } else {
            ensure!(
                id == 184,
                "field {id} uses unconverted script resource {resource_id}"
            );
        }
    }
    for (id, name) in embedded_animation_banks(&archive, &names)? {
        actors.animations.insert(name.clone());
        if let Some(previous) = selected_banks.insert(id, name.clone()) {
            ensure!(previous == name, "conflicting embedded animation {id}");
        }
    }
    let local_models = crate::field_local_models::local_models(id, archive)?;
    let has_local = !local_models.is_empty();
    let mut shared_owners = BTreeMap::new();
    for (_, actor) in &local_models {
        if actor.model.starts_with("meshes/") {
            shared_owners.insert(actor.resource, actor.model.clone());
        }
        actors.models.push(ModelBinding::Local(actor.clone()));
    }
    let local_motion_indices = crate::field_local_models::local_motion_sections(archive)?;
    let bind_local_motion = has_local && !local_motion_indices.is_empty();
    // The field actor service loads Colette's independent wing attachment in
    // every field, regardless of whether the script explicitly requests it.
    actors.models.push(ModelBinding::Shared(
        "characters/colette/wings/equipped".into(),
    ));
    for resource in service_models(script)? {
        let model = models
            .0
            .get(&resource)
            .with_context(|| format!("field service model {resource} has no mapping"))?;
        actors
            .models
            .push(ModelBinding::Shared(model.name().into()));
    }
    if id == 184 || bind_local_motion {
        let indices = if id == 184 {
            &[0usize, 1, 13][..]
        } else {
            &local_motion_indices
        };
        local_bindings(
            extracted,
            &exe,
            &archive,
            &root,
            &models.0,
            &mut actors,
            indices,
        )?;
    }
    if bind_local_motion {
        for index in &local_motion_indices {
            let path = format!("{root}/animations/local-motion-{index}.glb");
            ensure!(
                library.join(&path).is_file(),
                "field {id} actors require {path}"
            );
        }
    }
    for path in [
        "meshes/mappings.json",
        "meshes/groups.json",
        "meshes/joint-mappings.json",
    ] {
        ensure!(
            library.join(path).is_file(),
            "field {id} actors require {path}"
        );
    }
    for (&resource, path) in &shared_owners {
        crate::field_local_models::cook_shared_owner(
            extracted,
            library,
            id,
            resource,
            path,
            &selected_banks,
        )?;
        ensure!(
            library.join(path).is_file(),
            "field {id} actors require canonical local model {path}"
        );
    }
    for (&resource, target) in &models.0 {
        if !((1..=9).contains(&resource)
            || resource >> 24 == 0x23
            || actors.models.iter().any(
                |binding| matches!(binding, ModelBinding::Shared(name) if name == target.name()),
            ))
        {
            continue;
        }
        let path = format!("meshes/{}/model.json", target.model());
        ensure!(
            library.join(&path).is_file(),
            "field {id} actors require {path}"
        );
    }
    if !selected_banks.is_empty() {
        for path in ["animations/mappings.json", "animations/bindings.json"] {
            ensure!(
                library.join(path).is_file(),
                "field {id} actors require {path}"
            );
        }
        for &bank in selected_banks.keys() {
            let path = shared_animations::mapped_motion_path(library, bank)?;
            ensure!(
                library.join(&path).is_file(),
                "field {id} actors require {path}"
            );
        }
    }
    if has_local {
        crate::field_local_models::cook_local_models(
            extracted,
            library,
            id,
            exe,
            archive,
            &selected_banks,
        )?;
    }
    // Only low IDs can collide with the independent standalone catalogue.
    actors.local_resources.retain(|id| *id < 53);
    crate::output::json(&library.join(format!("{root}/actors.json")), &actors)
}
