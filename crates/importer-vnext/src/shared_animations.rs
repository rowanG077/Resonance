//! Script-selected motion banks become shared editable GLBs.
//!
//! The mapping is the only ID catalogue. Shared actor-to-joint selections live
//! in one binding table; a motion is cooked once even when several scenes use it.
use crate::{
    character_models, compression, dol, field_actors, field_bank_bindings, field_core,
    field_npc_models, mesh_mappings, native_archive, text, world_gpl, world_motion,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::Skeleton,
    field::document::{
        AnimationJoint, AnimationMappings, AnimationTarget, SharedAnimationBinding,
        SharedBankBindings,
    },
    library::Mappings,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

// These native members are byte-identical to the unsuffixed bank clip. Their
// bank identities stay distinct, but they need only one editable motion GLB.
const SHARED_MOTIONS: [u32; 22] = [
    67842, 68786, 196616, 196697, 196750, 196765, 196766, 196800, 262178, 262179, 327727, 327750,
    327753, 327822, 458838, 458839, 458894, 458909, 524296, 655368, 721038, 721068,
];

// The executable's standalone resource table has 53 entries. Higher plain
// IDs are local MAP-section motion members, selected by a field rather than
// by the global resource table.
const STANDALONE_COUNT: u32 = (0x801f_86b8 - 0x801f_85e4) / 4;

fn embedded_member(extracted: &Path, exe: &[u8], field: u32, id: u32) -> Result<Option<Vec<u8>>> {
    let rows = dol::slice(exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let at = field as usize * 24;
    let row = rows
        .get(at..at + 24)
        .context("field ID exceeds native catalogue")?;
    let Some(archive) = field_core::native_archive(extracted, exe, row, field as usize)? else {
        return Ok(None);
    };
    let Some(bank) = field_core::optional_section(&archive, 7)? else {
        return Ok(None);
    };
    Ok(field_npc_models::field_model_entries(bank)?
        .into_iter()
        .find(|(member, _)| u32::from(*member) == id)
        .map(|(_, bytes)| bytes.to_vec()))
}

fn embedded_bank(extracted: &Path, exe: &[u8], field: u32, id: u32) -> Result<Option<Vec<u8>>> {
    Ok(embedded_member(extracted, exe, field, id)?
        .filter(|bytes| world_motion::indexed_joints(bytes).is_ok()))
}

fn embedded_model(extracted: &Path, id: u32) -> Result<Vec<u8>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    for field in 0..field_core::FIELD_COUNT as u32 {
        if let Some(bytes) = embedded_member(extracted, &exe, field, id)? {
            return Ok(bytes);
        }
    }
    anyhow::bail!("no native field contains model {id}")
}

fn embedded_sources(extracted: &Path, ids: &BTreeSet<u32>) -> Result<BTreeMap<u32, Vec<u8>>> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let rows = dol::slice(&exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let mut found = BTreeMap::new();
    for field in 0..field_core::FIELD_COUNT as u32 {
        let row = &rows[field as usize * 24..field as usize * 24 + 24];
        let Some(archive) = field_core::native_archive(extracted, &exe, row, field as usize)?
        else {
            continue;
        };
        let Some(bank) = field_core::optional_section(&archive, 7)? else {
            continue;
        };
        // A source field is chosen by native field order, once per motion ID.
        for (member, bytes) in field_npc_models::field_model_entries(bank)? {
            let id = u32::from(member);
            if ids.contains(&id)
                && !found.contains_key(&id)
                && world_motion::indexed_joints(bytes).is_ok()
            {
                found.insert(id, bytes.to_vec());
            }
        }
        if found.len() == ids.len() {
            break;
        }
    }
    for id in ids {
        ensure!(
            found.contains_key(id),
            "no native field contains animation {id}"
        );
    }
    Ok(found)
}

/// Read a bank from the field that selected it, or from the executable's
/// global resource catalogue. A lone embedded ID is located from native MAP
/// archives, so there is no maintained source-field table.
pub(crate) fn read_bank(extracted: &Path, field: Option<u32>, id: u32) -> Result<Vec<u8>> {
    if id >> 16 != 0 || id < STANDALONE_COUNT {
        return field_actors::read_shared_resource(extracted, id);
    }
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    if let Some(field) = field {
        if let Some(bytes) = embedded_bank(extracted, &exe, field, id)? {
            return Ok(bytes);
        }
    }
    embedded_sources(extracted, &BTreeSet::from([id]))?
        .remove(&id)
        .with_context(|| format!("no native field contains animation {id}"))
}

/// Enumerate actual motion payloads, including members absent from the naming
/// catalogue. Empty native slots are fallback aliases, handled separately.
pub(crate) fn native_motion_banks(
    extracted: &Path,
) -> Result<(BTreeMap<u32, Vec<u8>>, BTreeMap<u32, u32>)> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let mut motions = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    let mut keep = |id, bytes: &[u8]| -> Result<()> {
        let expanded = if bytes.starts_with(b"MSCF") {
            compression::cabinet(bytes)?
        } else if bytes.len() >= 9
            && matches!(bytes[0], 0 | 1 | 3)
            && u32::from_le_bytes(bytes[1..5].try_into()?) as usize + 9 == bytes.len()
        {
            compression::decode(bytes)?
        } else {
            bytes.to_vec()
        };
        if expanded.starts_with(&0x007b_7960u32.to_be_bytes()) {
            world_motion::indexed_joints(&expanded)
                .with_context(|| format!("native animation {id}"))?;
            motions.insert(id, expanded);
        }
        Ok(())
    };
    for id in 0..STANDALONE_COUNT {
        match field_actors::read_shared_resource(extracted, id) {
            Ok(bytes) => keep(id, &bytes)?,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    for group in [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
        let filename = text::pointer(&exe, 0x801f_86b8 + (group - 1) * 12)?;
        let bank = native_archive::read_named(&extracted.join("disc1/files"), &filename)?;
        let bank = if bank.starts_with(b"MSCF") {
            compression::cabinet(&bank)?
        } else {
            bank
        };
        let count = dol::u32(&bank, 0)? as usize;
        ensure!(
            count <= bank.len().saturating_sub(4) / 8,
            "invalid motion bank {group}"
        );
        for index in 0..count {
            let at = 4 + index * 8;
            let start = dol::u32(&bank, at)? as usize;
            let size = dol::u32(&bank, at + 4)? as usize;
            if size != 0 {
                keep(
                    group << 16 | index as u32,
                    bank.get(start..start + size)
                        .context("motion member exceeds bank")?,
                )?;
            } else {
                aliases.insert(group << 16 | index as u32, group << 16);
            }
        }
    }
    Ok((motions, aliases))
}

fn names(ids: impl IntoIterator<Item = u32>) -> Result<BTreeMap<u32, String>> {
    let mut names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("field-animation-names.json"))?;
    for id in ids {
        if !names.contains_key(&id) {
            names.insert(id, field_actors::animation_name(id, &names));
        }
    }
    names.insert(131328, "characters/colette/package-131328".into());
    Mappings::new(names.clone())?;
    ensure!(
        names.values().all(|name| {
            !name.contains(['\\', '.']) && !name.starts_with('/') && !name.ends_with('/')
        }),
        "invalid shared animation name"
    );
    Ok(names)
}

/// An ID resolves to one GLB, independent of the field that requested it.
pub(crate) fn motion_path(id: u32, name: &str) -> Result<String> {
    if let Some(canonical) = match id {
        68976 => Some("unassigned/motion-68099"),
        68978 => Some("unassigned/motion-68096"),
        _ => None,
    } {
        return Ok(format!("animations/{canonical}.glb"));
    }
    let canonical = if SHARED_MOTIONS.contains(&id) {
        let suffix = format!("-{id}");
        name.strip_suffix(&suffix)
            .with_context(|| format!("shared motion alias {id} has no ID suffix"))?
    } else {
        name
    };
    Ok(format!("animations/{canonical}.glb"))
}

pub(crate) fn mapped_motion_path(library: &Path, id: u32) -> Result<String> {
    let mappings: AnimationMappings =
        serde_json::from_slice(&fs::read(library.join("animations/mappings.json"))?)?;
    mappings.motion(id)
}

fn reference(library: &Path, name: &str) -> Result<Option<Vec<u8>>> {
    let Some(owner) = name.rsplit_once('/') else {
        return Ok(None);
    };
    if owner.0 == "unassigned" {
        return Ok(None);
    }
    let body = library.join(format!("meshes/{}/meshes/body.glb", owner.0));
    Ok(Some(fs::read(&body).with_context(|| {
        format!("shared animation needs body {}", body.display())
    })?))
}

fn bindings(extracted: &Path, owner: &str) -> Result<Vec<(u16, String)>> {
    let party = [
        "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
    ];
    if let Some(character) = owner.strip_prefix("characters/")
        && let Some(index) = party.iter().position(|&candidate| candidate == character)
    {
        return character_models::party_joint_bindings(extracted, index as u8 + 1, 0);
    }
    let model_id = mesh_mappings::mesh_mappings()?
        .0
        .into_iter()
        .filter(|(_, target)| target.model() == owner)
        // A standalone model alias can share its numeric ID with an unrelated
        // script resource. Prefer the native NPC bank that owns the rig.
        .min_by_key(|(id, _)| (*id >> 16 != 2, *id))
        .map(|(id, _)| id)
        .with_context(|| format!("no native model for shared animation owner {owner}"))?;
    let package = if model_id >> 16 == 0 && model_id >= STANDALONE_COUNT {
        embedded_model(extracted, model_id)
    } else {
        field_actors::read_shared_resource(extracted, model_id)
    }
    .with_context(|| format!("read native model {model_id} for {owner}"))?;
    let body = field_core::optional_section(&package, 0)?
        .with_context(|| format!("native model {model_id} has no body"))?;
    world_gpl::animation_bindings(body)
}

pub(crate) fn unassigned_track_names(native: &[u8]) -> Result<BTreeMap<u16, String>> {
    let indices = world_motion::indexed_joints(native)?;
    let named = (|| -> Result<BTreeMap<u16, String>> {
        ensure!(
            dol::u32(native, 16)? != 0 && dol::u32(native, 20)? != 0,
            "motion has no name table"
        );
        let count = usize::from(dol::u16(native, 12)?);
        let mut cursor = dol::u32(native, 20)? as usize;
        let mut raw = Vec::with_capacity(count);
        for index in 0..count {
            let tail = native.get(cursor..).context("motion names exceed source")?;
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .context("unterminated motion joint name")?;
            let (label, invalid) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&tail[..end]);
            ensure!(!invalid, "invalid native motion joint name");
            raw.push((dol::u16(native, 36 + index * 16 + 10)?, label.into_owned()));
            cursor += end + 1;
        }
        Ok(world_gpl::animation_target_bindings(raw)
            .into_iter()
            .collect())
    })();
    // Some unassigned motions carry an unusable nominal name table. Their
    // joint IDs are still authoritative and match the generic cooked GLB.
    Ok(named.unwrap_or_else(|_| {
        indices
            .into_iter()
            .map(|id| (id, format!("joint-{id}")))
            .collect()
    }))
}

fn unassigned(native: &[u8]) -> Result<Vec<u8>> {
    let names = unassigned_track_names(native)?;
    world_motion::encode_named_indexed(native, &names)
}

/// Native track ID to the exact editable node name in the cooked shared GLB.
/// Scenery and actor binding writers use this instead of guessing GLB labels.
pub(crate) fn track_names(
    extracted: &Path,
    library: &Path,
    native: &[u8],
    name: &str,
) -> Result<BTreeMap<u16, String>> {
    let Some(glb) = reference(library, name)? else {
        return unassigned_track_names(native);
    };
    let owner = name.rsplit_once('/').context("invalid animation owner")?.0;
    let bindings = bindings(extracted, owner)?;
    let skeleton = Skeleton::from_glb(&glb)?;
    ensure!(
        bindings.len() == skeleton.bones.len(),
        "shared animation rig differs from native model"
    );
    let owner = bindings
        .iter()
        .zip(&skeleton.bones)
        .map(|((id, raw), bone)| (*id, bone.name.clone(), raw.clone()))
        .collect::<Vec<_>>();
    world_motion::shared_track_names(native, &owner)
}

fn encode(extracted: &Path, library: &Path, native: &[u8], id: u32, name: &str) -> Result<Vec<u8>> {
    let Some(glb) =
        reference(library, name).with_context(|| format!("read vnext owner rig for {name}"))?
    else {
        return unassigned(native);
    };
    let owner = name.rsplit_once('/').context("invalid animation owner")?.0;
    let bindings = bindings(extracted, owner)
        .with_context(|| format!("read native rig bindings for {owner}"))?;
    world_motion::encode_bound(native, &glb, &bindings)
        .with_context(|| format!("convert shared animation {id} ({name})"))
}

fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("animation output has no parent")?)?;
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .context("animation output has no extension")?
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
        .with_context(|| format!("publish animation output {}", path.display()))
}

fn existing_motions(library: &Path) -> Result<BTreeMap<Vec<u8>, String>> {
    let mut directories = vec![library.to_owned()];
    let mut motions = BTreeMap::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                directories.push(entry.path());
            } else {
                let path = entry.path();
                let relative = path
                    .strip_prefix(library)?
                    .to_str()
                    .context("non-UTF8 motion path")?;
                if relative.ends_with(".glb")
                    && (relative.starts_with("animations/") || relative.contains("/animations/"))
                {
                    motions
                        .entry(fs::read(&path)?)
                        .and_modify(|existing: &mut String| {
                            if relative < existing.as_str() {
                                *existing = relative.to_owned();
                            }
                        })
                        .or_insert_with(|| relative.to_owned());
                }
            }
        }
    }
    Ok(motions)
}

fn cook_selected(
    extracted: &Path,
    library: &Path,
    selected: Option<u32>,
    selected_field: Option<u32>,
) -> Result<()> {
    let (native, aliases) = native_motion_banks(extracted)?;
    let known = names(native.keys().chain(aliases.keys()).copied())?;
    let mut selection = if let Some(field) = selected_field {
        let selected = field_actors::declared_animation_banks(extracted, field)?;
        for (&id, name) in &selected {
            ensure!(
                known.get(&id) == Some(name),
                "field {field} animation {id} disagrees with global names"
            );
        }
        selected
    } else {
        known
            .iter()
            .filter(|(id, _)| selected.is_none_or(|wanted| wanted == **id))
            .map(|(&id, name)| (id, name.clone()))
            .collect()
    };
    ensure!(!selection.is_empty(), "no shared animations selected");
    let canonical = selection
        .keys()
        .filter_map(|id| aliases.get(id))
        .copied()
        .collect::<BTreeSet<_>>();
    if canonical.iter().any(|id| !selection.contains_key(id)) {
        for id in canonical {
            selection.entry(id).or_insert_with(|| known[&id].clone());
        }
    }
    let embedded = if selected_field.is_none() {
        let mut missing = BTreeSet::new();
        for (&id, name) in &selection {
            if id >> 16 == 0
                && id >= STANDALONE_COUNT
                && !library.join(motion_path(id, name)?).is_file()
            {
                missing.insert(id);
            }
        }
        embedded_sources(extracted, &missing)?
    } else {
        BTreeMap::new()
    };
    let path = library.join("animations/mappings.json");
    let mut targets = if (selected.is_some() || selected_field.is_some()) && path.is_file() {
        serde_json::from_slice::<AnimationMappings>(&fs::read(&path)?)?
    } else {
        AnimationMappings(BTreeMap::new())
    };
    let mut available = None;
    for (&id, name) in &selection {
        if targets
            .motion(id)
            .is_ok_and(|path| library.join(path).is_file())
        {
            continue;
        }
        if id == 131328 {
            targets.0.insert(
                id,
                field_npc_models::cook_motion_only_npc(extracted, library)?.0,
            );
            continue;
        }
        if let Some(&resource) = aliases.get(&id) {
            targets.0.insert(
                id,
                AnimationTarget::Alias {
                    name: name.clone(),
                    resource,
                },
            );
            continue;
        }
        let mut motion = motion_path(id, name)?;
        let path = library.join(&motion);
        if !path.is_file() {
            if available.is_none() {
                available = Some(existing_motions(library)?);
            }
            let available = available.as_mut().unwrap();
            let bytes = if let Some(bytes) = native.get(&id).or_else(|| embedded.get(&id)) {
                encode(extracted, library, bytes, id, name)
            } else {
                encode(
                    extracted,
                    library,
                    &read_bank(extracted, selected_field, id)?,
                    id,
                    name,
                )
            };
            let bytes = bytes.with_context(|| format!("shared animation {id} ({name})"))?;
            if let Some(existing) = available.get(&bytes) {
                motion = existing.clone();
            } else {
                publish(&path, &bytes)?;
                available.insert(bytes, motion.clone());
            }
        }
        let target = if motion == format!("animations/{name}.glb") {
            AnimationTarget::Named(name.clone())
        } else {
            AnimationTarget::Shared {
                name: name.clone(),
                motion,
            }
        };
        targets.0.insert(id, target);
    }
    let mut used_names = BTreeSet::new();
    for (_, name) in targets.iter() {
        ensure!(
            used_names.insert(name),
            "duplicate animation bank name {name}"
        );
    }
    publish(&path, &resonance_asset_writer::json::encode(&targets)?)
}

/// Convert a selected native resource ID, or all known shared animations.
/// Models must already exist so exported clips carry their editable rest rigs.
pub fn cook_shared_animations(
    extracted: &Path,
    library: &Path,
    selected: Option<u32>,
) -> Result<()> {
    cook_selected(extracted, library, selected, None)
}

/// Cook only banks selected by one field, for a focused integration pass.
pub fn cook_shared_animations_for_field(
    extracted: &Path,
    library: &Path,
    field: u32,
) -> Result<()> {
    cook_selected(extracted, library, None, Some(field))
}

/// Preserve loose motions without inventing script IDs or unproven owners.
/// Mods can reference these ordinary GLBs directly through SceneClip.motion.
pub fn cook_original_animations(extracted: &Path, library: &Path) -> Result<()> {
    for source in [
        "mdl_ask_i00_00.anm",
        "mdl_ask_i00_00_l.anm",
        "mdl_ask_t02_ivt00.anm",
        "mdl_iwa_d00_01.anm",
        "mdl_spd_I00_00.anm",
        "mdl_spd_I00_00_L.anm",
        "mdl_spd_t00.anm",
    ] {
        let name = source.strip_suffix(".anm").unwrap().to_ascii_lowercase();
        let path = library.join(format!("animations/unidentified/{name}.glb"));
        if !path.is_file() {
            let native = native_archive::read_named(&extracted.join("disc1/files"), source)?;
            crate::output::write(
                &path,
                &unassigned(&native).with_context(|| format!("original motion {source}"))?,
            )?;
        }
    }
    Ok(())
}

type JointMap = BTreeMap<String, Option<String>>;
type Target = (u32, String, u16, u16);
type Binding = (String, JointMap, Vec<String>);

/// Collect native actor targets once per bank, retaining one definition for an
/// identical target seen in several fields.
pub fn cook_shared_animation_bindings(
    extracted: &Path,
    library: &Path,
    selected_field: Option<u32>,
) -> Result<()> {
    let (native, aliases) = native_motion_banks(extracted)?;
    let names = names(native.keys().chain(aliases.keys()).copied())?;
    let fields: Vec<u32> = match selected_field {
        Some(id) => vec![id],
        None => (0..field_core::FIELD_COUNT as u32).collect(),
    };
    let mut targets = BTreeMap::<Target, Binding>::new();
    let mut cache = field_bank_bindings::Bindings::default();
    for field in fields {
        for (id, rows) in
            field_bank_bindings::native_bank_bindings(extracted, library, field, &mut cache)
                .with_context(|| format!("derive shared animation bindings in field {field}"))?
        {
            ensure!(
                names.contains_key(&id),
                "field {field} selected unnamed animation {id}"
            );
            let path = mapped_motion_path(library, id)?;
            for row in rows {
                ensure!(
                    !row.actors.is_empty() && !row.parts.is_empty() && row.motion == path,
                    "invalid native animation binding {id} in field {field}"
                );
                for actor in &row.actors {
                    for &part in &row.parts {
                        let key = (id, actor.clone(), part, row.resource_slot);
                        let value = (
                            row.motion.clone(),
                            row.joints.clone(),
                            row.pose_chains.clone(),
                        );
                        if let Some(previous) = targets.insert(key, value.clone()) {
                            ensure!(
                                previous == value,
                                "conflicting animation {id} for actor {actor} part {part} in field {field}"
                            );
                        }
                    }
                }
            }
        }
    }
    ensure!(!targets.is_empty(), "no shared animation bindings found");
    let mut actor_parts = BTreeMap::<(u32, String, u16, Binding), BTreeSet<u16>>::new();
    for ((id, actor, part, slot), binding) in targets {
        actor_parts
            .entry((id, actor, slot, binding))
            .or_default()
            .insert(part);
    }
    let mut grouped = BTreeMap::<(u32, Vec<u16>, u16, Binding), Vec<String>>::new();
    for ((id, actor, slot, binding), parts) in actor_parts {
        grouped
            .entry((id, parts.into_iter().collect(), slot, binding))
            .or_default()
            .push(actor);
    }
    let mut definitions = SharedBankBindings::default();
    definitions.rigs = field_bank_bindings::global_rigs(extracted, library)?;
    for (id, bytes) in native {
        let name = names.get(&id).context("native animation lacks a name")?;
        let labels = track_names(extracted, library, &bytes, name)?;
        definitions.tracks.insert(
            id,
            field_bank_bindings::source_tracks(&bytes)?
                .into_iter()
                .enumerate()
                .map(|(index, track)| -> Result<AnimationJoint> {
                    Ok(AnimationJoint {
                        id: track.id,
                        name: labels
                            .get(&track.id)
                            .context("motion track has no editable node")?
                            .clone(),
                        source: track.name,
                        animated: dol::u16(&bytes, 36 + index * 16 + 8)? > 2,
                    })
                })
                .collect::<Result<_>>()?,
        );
    }
    definitions.banks.insert(
        131328,
        field_npc_models::cook_motion_only_npc(extracted, library)?.1,
    );
    let mut map_names = BTreeMap::<JointMap, String>::new();
    for ((id, parts, resource_slot, (motion, joints, pose_chains)), actors) in grouped {
        let joint_map = if joints.is_empty() {
            None
        } else if let Some(existing) = map_names.get(&joints) {
            Some(existing.clone())
        } else {
            let (owner, clip) = names[&id]
                .rsplit_once('/')
                .context("shared motion has no readable owner")?;
            let actor = actors.first().context("shared motion has no actor")?;
            let base = format!("{owner}/to/{actor}");
            let mut label = base.clone();
            if definitions.joint_maps.contains_key(&label) {
                label = format!("{base}/{clip}");
            }
            let mut number = 2;
            while definitions.joint_maps.contains_key(&label) {
                label = format!("{base}/{clip}-{number}");
                number += 1;
            }
            definitions.joint_maps.insert(label.clone(), joints.clone());
            map_names.insert(joints, label.clone());
            Some(label)
        };
        definitions
            .banks
            .entry(id)
            .or_default()
            .push(SharedAnimationBinding {
                actors,
                parts,
                motion,
                joint_map,
                resource_slot,
                pose_chains,
            });
    }
    let path = library.join("animations/bindings.json");
    publish(&path, &resonance_asset_writer::json::encode(&definitions)?)
}
