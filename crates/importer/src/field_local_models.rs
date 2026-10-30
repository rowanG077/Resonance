//! Field-owned actor packages, directly from native MAP sections.
use crate::{
    character_models, compression, dol, field_actors, field_core, field_npc_models, mesh_mappings,
    shared_animations, world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, CullFace, SceneClip, SceneImage,
    field::{
        ModelCollision, collision_mesh,
        document::{Actor, Model},
    },
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

const LOCAL_BASE: u32 = 0xffee_0000;
const HEIMDALL_OWNER: &str = "meshes/props/heimdall-unidentified-1";
const HEIMDALL_IDLE: &str = "meshes/figurines/bush-baby-0287/animations/idle.glb";
const TETHEALLA_WALKER: &str = "meshes/props/tethealla-base-unidentified-2";
const GAORACCHIA_CONE: &str = "meshes/props/gaoracchia-forest-unidentified-1";
const GAORACCHIA_SOURCE: u32 = 511;
const LATHEON_CAPTURE: &str = "meshes/props/latheon-gorge-unidentified-1";
const LATHEON_SOURCE: u32 = 492;
const SYLVARANT_BASE_PROP: &str = "meshes/props/sylvarant-base-unidentified-4";
const THODA_IDLE: &str = "fields/thoda-geyser-0007/animations/unidentified-1-idle.glb";
const SYLVARANT_IDLE: &str = "meshes/props/sylvarant-base-unidentified-4/animations/idle.glb";
const ISELIA_RANCH_IDLE: &str = "fields/iselia-human-ranch-0193/animations/unidentified-1-idle.glb";
const PALMACOSTA_SHARED_IDLE: &str =
    "meshes/props/field-unidentified-1-variant-2/animations/idle.glb";

fn reused_field_motion(root: &str, section: usize, slot: u16) -> Option<&'static str> {
    if root == "fields/triet-ruins-0220"
        && section == 18
        && (12..=116).contains(&slot)
        && (slot - 12).is_multiple_of(4)
    {
        return Some("meshes/props/wooden-crate/animations/idle.glb");
    }
    match (root, section, slot) {
        ("fields/asgard-0031", 24, 12 | 36) => {
            Some("battle/enemies/windmaster-0179/attachment-0-layer-0-animation-0.glb")
        }
        ("fields/tower-of-salvation-0179", 16, 12 | 36)
        | ("fields/tower-of-salvation-0181", 18, 12 | 36) => {
            Some("meshes/props/tower-of-salvation-unidentified-2/animations/idle.glb")
        }
        (
            "fields/tower-of-salvation-0179",
            23,
            12 | 24 | 36 | 40 | 44 | 48 | 52 | 60 | 64 | 68 | 72 | 76,
        ) => Some("battle/enemies/volt-0198/attachment-0-layer-0-animation-0.glb"),
        (
            "fields/tower-of-salvation-0179",
            24,
            12 | 24 | 36 | 40 | 44 | 48 | 52 | 60 | 64 | 68 | 72 | 76,
        ) => Some("fields/tower-of-salvation-0152/animations/unidentified-13-idle.glb"),
        ("fields/triet-ruins-0220", 19, 12) => {
            Some("meshes/monsters/slime-0157/animations/motion-0.glb")
        }
        ("fields/triet-ruins-0220", 19, 36 | 40) => {
            Some("meshes/monsters/slime-0157/animations/motion-1.glb")
        }
        ("fields/thoda-geyser-0008", 16, 12 | 36) => Some(THODA_IDLE),
        ("fields/tethealla-base-0252", 16, 12 | 36) => Some(SYLVARANT_IDLE),
        ("fields/tethealla-base-0254", 17, 12) => {
            Some("fields/tethealla-base-0253/animations/unidentified-2-idle.glb")
        }
        ("fields/tethealla-base-0254", 17, 36) => {
            Some("fields/tethealla-base-0253/animations/unidentified-2-walk.glb")
        }
        ("fields/tethealla-base-0254", 17, 40 | 60) => {
            Some("fields/tethealla-base-0253/animations/unidentified-2-run.glb")
        }
        ("fields/tethealla-base-0254", 17, 64) => {
            Some("fields/tethealla-base-0253/animations/unidentified-2-motion-64.glb")
        }
        ("fields/tethealla-base-0254", 19, 12 | 36) => {
            Some("fields/sylvarant-base-0268/animations/unidentified-4-idle.glb")
        }
        ("fields/tower-of-mana-0363", 20, 12 | 36) => Some(ISELIA_RANCH_IDLE),
        ("fields/iselia-human-ranch-0196", 16, 12 | 36) => {
            Some("fields/iselia-human-ranch-0194/animations/unidentified-1-idle.glb")
        }
        ("fields/iselia-human-ranch-0193", 25, 12 | 36) => {
            Some("fields/iselia-forest-0192/animations/unidentified-1-idle.glb")
        }
        ("fields/iselia-human-ranch-0193", 26, 12 | 36) => {
            Some("fields/iselia-forest-0192/animations/unidentified-2-idle.glb")
        }
        ("fields/iselia-human-ranch-0193", 27, 12 | 36) => {
            Some("fields/iselia-forest-0192/animations/unidentified-3-idle.glb")
        }
        ("fields/island-ranch-0232", 18, 12 | 36) => {
            Some("fields/island-ranch-0225/animations/unidentified-1-idle.glb")
        }
        ("fields/palmacosta-ranch-0205", 16 | 18 | 20 | 22, 12 | 36) => {
            Some(PALMACOSTA_SHARED_IDLE)
        }
        ("fields/palmacosta-ranch-0205", 17, 12 | 36) => {
            Some("fields/palmacosta-ranch-0201/animations/unidentified-2-idle.glb")
        }
        ("fields/palmacosta-ranch-0205", 19, 12 | 36) => {
            Some("fields/palmacosta-ranch-0201/animations/unidentified-4-idle.glb")
        }
        ("fields/palmacosta-ranch-0205", 21, 12 | 36) => {
            Some("fields/palmacosta-ranch-0201/animations/unidentified-6-idle.glb")
        }
        ("fields/palmacosta-ranch-0205", 23, 12 | 36) => {
            Some("fields/palmacosta-ranch-0201/animations/unidentified-8-idle.glb")
        }
        ("fields/temple-of-ice-0324", 23, 12 | 36) => {
            Some("fields/temple-of-ice-0323/animations/unidentified-2-idle.glb")
        }
        ("fields/temple-of-ice-0324", 25, 12) => {
            Some("fields/temple-of-ice-0323/animations/unidentified-5-idle.glb")
        }
        ("fields/temple-of-ice-0324", 25, 36) => {
            Some("fields/temple-of-ice-0323/animations/unidentified-5-walk.glb")
        }
        ("fields/meltokio-0466", 21, 12) => {
            Some("fields/meltokio-0466/animations/unidentified-5-idle.glb")
        }
        ("fields/meltokio-0466", 21, 80) => {
            Some("fields/meltokio-0466/animations/unidentified-5-motion-80.glb")
        }
        ("fields/meltokio-0467", 16, 12 | 36) => {
            Some("fields/meltokio-0466/animations/unidentified-1-idle.glb")
        }
        ("fields/meltokio-0467", 16, 80) => {
            Some("fields/meltokio-0466/animations/unidentified-1-motion-80.glb")
        }
        ("fields/meltokio-0467", 17, 12) => {
            Some("fields/meltokio-0466/animations/unidentified-2-idle.glb")
        }
        ("fields/meltokio-0467", 17, 36) => {
            Some("fields/meltokio-0466/animations/unidentified-2-walk.glb")
        }
        ("fields/meltokio-0467", 18, 12 | 36) => {
            Some("fields/meltokio-0466/animations/unidentified-3-idle.glb")
        }
        ("fields/meltokio-0467", 19, 12 | 36) => {
            Some("fields/meltokio-0466/animations/unidentified-4-idle.glb")
        }
        ("fields/meltokio-0467", 20, 12) => {
            Some("fields/meltokio-0466/animations/unidentified-5-idle.glb")
        }
        ("fields/meltokio-0467", 20, 36 | 40) => {
            Some("fields/meltokio-0466/animations/unidentified-5-walk.glb")
        }
        ("fields/meltokio-0467", 20, 80) => {
            Some("fields/meltokio-0466/animations/unidentified-5-motion-80.glb")
        }
        ("fields/field-0536", 30, 12 | 36) => {
            Some("fields/linkite-tree-0523/animations/unidentified-2-idle.glb")
        }
        ("fields/luin-0448", 16, 12) => Some("meshes/monsters/mantis-0132/animations/motion-0.glb"),
        ("fields/luin-0448", 16, 36) => Some("meshes/monsters/mantis-0132/animations/motion-1.glb"),
        ("fields/luin-0448", 16, 40) => Some("meshes/monsters/mantis-0132/animations/motion-2.glb"),
        ("fields/luin-0448", 16, 80) => {
            Some("meshes/monsters/mantis-0132/animations/motion-21.glb")
        }
        ("fields/luin-0448", 16, 84) => {
            Some("meshes/monsters/mantis-0132/animations/motion-30.glb")
        }
        ("fields/luin-0448", 16, 88) => {
            Some("meshes/monsters/mantis-0132/animations/motion-31.glb")
        }
        ("fields/luin-0448", 16, 92) => {
            Some("meshes/monsters/mantis-0132/animations/motion-32.glb")
        }
        _ => None,
    }
}

fn gaoracchia_motion(owner: &str, slot: u16) -> Option<String> {
    let number = owner
        .strip_prefix("meshes/props/gaoracchia-forest-unidentified-")?
        .parse::<u8>()
        .ok()?;
    let label = match (number, slot) {
        (3..=6, 12 | 36) => "idle",
        (5 | 6, 80) => "motion-80",
        _ => return None,
    };
    Some(format!(
        "fields/gaoracchia-forest-0511/animations/unidentified-{number}-{label}.glb"
    ))
}

fn reused_motion(owner: &str, slot: u16) -> Option<&'static str> {
    match (owner, slot) {
        (HEIMDALL_OWNER, 12) => Some(HEIMDALL_IDLE),
        ("meshes/props/tethealla-base-unidentified-1", 12 | 36) => {
            Some("fields/tethealla-base-0253/animations/unidentified-1-idle.glb")
        }
        (TETHEALLA_WALKER, 12 | 36) => {
            Some("fields/tethealla-base-0253/animations/unidentified-2-walk.glb")
        }
        (
            "meshes/props/tower-of-mana-unidentified-1"
            | "meshes/props/tower-of-mana-unidentified-2",
            12 | 36 | 40,
        ) => Some("fields/tower-of-mana-0362/animations/unidentified-1-idle.glb"),
        ("meshes/props/tower-of-mana-unidentified-3", 12 | 36 | 40) => {
            Some("fields/tower-of-mana-0362/animations/unidentified-3-idle.glb")
        }
        ("meshes/props/tower-of-mana-unidentified-4", 12) => {
            Some("fields/tower-of-mana-0362/animations/unidentified-4-idle.glb")
        }
        ("meshes/props/tethealla-base-unidentified-3", 12 | 36) => {
            Some("fields/tethealla-base-0253/animations/unidentified-3-idle.glb")
        }
        ("meshes/props/meltokio-unidentified-6", 12 | 36) => {
            Some("fields/meltokio-0466/animations/unidentified-8-idle.glb")
        }
        _ => None,
    }
}

/// Ordinary MAP-local models can be written in place when no authored owner
/// redirects their physical resource into a shared mesh. The authored table
/// is also needed by the remaining, more complex field classes.
pub(crate) fn owner_bindings(id: u32) -> Result<BTreeMap<u32, String>> {
    let mut owners: BTreeMap<String, BTreeMap<u32, String>> =
        serde_json::from_str(include_str!("field-local-model-owners.json"))?;
    Ok(owners
        .remove(&field_core::field_name(id as usize))
        .unwrap_or_default())
}

#[derive(Clone, Default, Deserialize)]
struct PackageBorrow {
    media: Option<String>,
    source: Option<[u32; 2]>,
    source_model: Option<String>,
    #[serde(default)]
    publish: bool,
    #[serde(default)]
    clips: BTreeMap<u16, String>,
    animated_banks: Option<Vec<String>>,
    document: Option<String>,
}

#[derive(Deserialize)]
struct PackageFamily {
    source: u32,
    members: Vec<u32>,
    media: String,
    outlined_animation_owner: Option<String>,
    #[serde(default)]
    source_documents: bool,
    #[serde(default)]
    include_source: bool,
}

fn package_family(id: u32) -> Result<Option<PackageFamily>> {
    let families: Vec<PackageFamily> =
        serde_json::from_str(include_str!("field-package-families.json"))?;
    Ok(families.into_iter().find(|family| {
        id == family.source && (!family.source_documents || family.include_source)
            || family.members.contains(&id)
    }))
}

fn family_borrows(
    extracted: &Path,
    library: &Path,
    exe: &[u8],
    id: u32,
    archive: &[u8],
) -> Result<BTreeMap<usize, PackageBorrow>> {
    let Some(family) = package_family(id)? else {
        return Ok(BTreeMap::new());
    };
    if id == family.source && family.source_documents {
        return Ok(BTreeMap::new());
    }
    let rows = dol::slice(exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let source_row = &rows[family.source as usize * 24..][..24];
    let source_archive =
        field_core::native_archive(extracted, exe, source_row, family.source as usize)?
            .context("package family has no source field")?;
    let source_root = format!("fields/{}", field_core::field_name(family.source as usize));
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let source_names = owner_bindings(family.source)?;
    let names = owner_bindings(id)?;
    let mut result = BTreeMap::new();
    for section in 16..dol::u32(archive, 0)? as usize {
        let package =
            field_core::optional_section(archive, section)?.context("family package missing")?;
        if dol::u32(package, 0)? != 31 {
            continue;
        }
        let local = LOCAL_BASE + (section - 16) as u32;
        let document = names
            .get(&local)
            .filter(|path| path.starts_with(&root))
            .cloned();
        let source_section = if id == family.source {
            Some(section)
        } else {
            (16..dol::u32(&source_archive, 0)? as usize).find(|&source_section| {
                field_core::optional_section(&source_archive, source_section)
                    .ok()
                    .flatten()
                    == Some(package)
            })
        };
        let Some(source_section) = source_section else {
            if document.is_some() {
                result.insert(
                    section,
                    PackageBorrow {
                        media: None,
                        source: None,
                        source_model: None,
                        publish: false,
                        clips: BTreeMap::new(),
                        animated_banks: None,
                        document,
                    },
                );
            }
            continue;
        };
        let source_local = LOCAL_BASE + (source_section - 16) as u32;
        let label = source_names
            .get(&source_local)
            .and_then(|name| name.rsplit('/').next())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("unidentified-{}", source_section - 15));
        let media = format!("{}/{label}", family.media);
        let source_model = family.source_documents.then(|| {
            let source = source_names.get(&source_local).cloned().unwrap_or_else(|| {
                format!("{source_root}/models/unidentified-{}", source_section - 15)
            });
            if source.starts_with("meshes/") {
                format!("{source}/model.json")
            } else {
                format!("{source}.json")
            }
        });
        let mut clips = BTreeMap::new();
        if id != family.source && !family.source_documents {
            // Source-first publication supplies the exact names chosen after
            // native motion deduplication; the package equality above guards reuse.
            let source_name = source_names.get(&source_local).cloned().unwrap_or_else(|| {
                format!("{source_root}/models/unidentified-{}", source_section - 15)
            });
            let path = library.join(format!("{source_name}.json"));
            let model: Model = serde_json::from_slice(&fs::read(&path).with_context(|| {
                format!("cook package source {} before field {id}", family.source)
            })?)?;
            for clip in model
                .parts
                .first()
                .context("source model has no body")?
                .clips
                .iter()
            {
                if clip.animation_resource.is_none() {
                    clips.insert(clip.resource_slot, clip.motion.clone());
                }
            }
        }
        let outlined = field_core::optional_section(package, 1)?.is_some();
        result.insert(
            section,
            PackageBorrow {
                media: (!family.source_documents).then_some(media),
                source: (id != family.source).then_some([family.source, source_section as u32]),
                source_model,
                publish: id == family.source,
                clips,
                animated_banks: outlined
                    .then(|| family.outlined_animation_owner.clone())
                    .flatten()
                    .map(|owner| vec![owner]),
                document,
            },
        );
    }
    Ok(result)
}

fn check_package_source(
    extracted: &Path,
    exe: &[u8],
    package: &[u8],
    borrow: &PackageBorrow,
) -> Result<()> {
    let Some([field, section]) = borrow.source else {
        return Ok(());
    };
    let rows = dol::slice(exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let row = &rows[field as usize * 24..][..24];
    let archive = field_core::native_archive(extracted, exe, row, field as usize)?
        .with_context(|| format!("borrowed field {field} has no native archive"))?;
    let source = field_core::optional_section(&archive, section as usize)?
        .context("borrowed field package is missing")?;
    if borrow.media.is_none() && borrow.source_model.is_none() && !borrow.clips.is_empty() {
        // A reduced package may omit source motions while sharing every
        // retained native member. Container offsets need not be identical.
        for member in 0..dol::u32(package, 0)? as usize {
            if let Some(bytes) = field_core::optional_section(package, member)? {
                ensure!(
                    field_core::optional_section(source, member)? == Some(bytes),
                    "borrowed field member {member} differs from its physical owner"
                );
            }
        }
    } else {
        ensure!(
            package == source,
            "borrowed field package differs from its physical owner"
        );
    }
    Ok(())
}

fn package_borrows(id: u32) -> Result<BTreeMap<usize, PackageBorrow>> {
    let all: BTreeMap<String, BTreeMap<String, PackageBorrow>> =
        serde_json::from_str(include_str!("field-package-borrows.json"))?;
    all.get(&field_core::field_name(id as usize))
        .into_iter()
        .flat_map(|rows| rows.iter())
        .map(|(section, borrow)| {
            let mut borrow = borrow.clone();
            if let Some([field, member]) = borrow.source {
                if let Some(source) = all
                    .get(&field_core::field_name(field as usize))
                    .and_then(|rows| rows.get(&member.to_string()))
                {
                    borrow.media = source.media.clone();
                    if borrow.clips.is_empty() {
                        borrow.clips = source.clips.clone();
                    }
                } else {
                    ensure!(
                        borrow.media.is_none() && !borrow.clips.is_empty(),
                        "borrowed field package has no physical owner"
                    );
                }
            }
            Ok((section.parse::<usize>()?, borrow))
        })
        .collect()
}

pub(crate) fn local_motion_sections(archive: &[u8]) -> Result<Vec<usize>> {
    let count = dol::u32(archive, 0)? as usize;
    let mut sections = Vec::new();
    for section in 16..count {
        let Some(bytes) = field_core::optional_section(archive, section)? else {
            continue;
        };
        if bytes.len() >= 36 && dol::u32(bytes, 0)? == 0x007b_7960 {
            sections.push(section - 16);
        }
    }
    Ok(sections)
}

/// Native actor packages select their final model path directly. A bare GPL in
/// the first extra slot needs an explicit actor owner because that slot can also
/// hold a caller-textured scenery surface.
pub(crate) fn local_models(id: u32, archive: &[u8]) -> Result<Vec<(usize, Actor)>> {
    let owners = owner_bindings(id)?;
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let mut models = Vec::new();
    for section in 16..dol::u32(archive, 0)? as usize {
        let Some(package) = field_core::optional_section(archive, section)? else {
            continue;
        };
        if id == 125 && section == 16 {
            continue;
        }
        let resource = LOCAL_BASE + (section - 16) as u32;
        let owner = owners.get(&resource);
        if dol::u32(package, 0)? != 31
            && !(section > 16 && bare_local_model(package)?)
            && !(owner.is_some() && world_gpl::is_model(package))
        {
            continue;
        }
        let model = match owner {
            Some(owner) if owner.starts_with("meshes/") => format!("{owner}/model.json"),
            Some(owner) => {
                ensure!(
                    owner.starts_with(&format!("{root}/")),
                    "field {id} model belongs to another field"
                );
                format!("{owner}.json")
            }
            None => format!("{root}/models/unidentified-{}.json", section - 15),
        };
        models.push((
            section,
            Actor {
                resource,
                model,
                hidden_geometry: None,
            },
        ));
    }
    Ok(models)
}

fn clip(motion: String, slot: u16, bank: Option<u32>) -> SceneClip {
    SceneClip {
        motion,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: bank,
        pose_chains: Vec::new(),
    }
}

fn shared_clips(
    extracted: &Path,
    library: &Path,
    field_id: u32,
    banks: &BTreeMap<u32, String>,
) -> Result<
    Vec<(
        u32,
        String,
        Vec<crate::field_bank_bindings::Track>,
        BTreeMap<u16, String>,
    )>,
> {
    if banks.is_empty() {
        return Ok(Vec::new());
    }
    let party = [
        "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
    ];
    let mappings = mesh_mappings::mesh_mappings()?;
    let mut npc_ids = BTreeMap::new();
    for name in banks.values() {
        if !name.starts_with("characters/") {
            continue;
        }
        let owner = name
            .split('/')
            .nth(1)
            .context("field animation has no owner")?;
        if !party.contains(&owner) {
            let target = format!("characters/{owner}");
            let id = mappings
                .0
                .iter()
                .find(|(_, model)| model.name() == target)
                .map(|(&id, _)| id)
                .with_context(|| format!("field animation owner {owner} has no model ID"))?;
            npc_ids.insert(owner.to_owned(), id);
        }
    }
    let npc = field_npc_models::npc_native_bodies(
        extracted,
        &npc_ids
            .values()
            .copied()
            .filter(|id| id >> 16 == 2)
            .collect::<Vec<_>>(),
    )?;
    let embedded = if npc_ids.values().any(|id| *id <= u16::MAX as u32) {
        let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
        let rows = dol::slice(&exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
        let row = &rows[field_id as usize * 24..field_id as usize * 24 + 24];
        let archive = field_core::native_archive(extracted, &exe, row, field_id as usize)?
            .context("field animation owner has no native archive")?;
        field_core::optional_section(&archive, 7)?
            .map(field_npc_models::field_model_entries)
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .filter(|(id, _)| {
                npc_ids
                    .values()
                    .any(|requested| *requested == u32::from(*id))
            })
            .map(|(id, package)| {
                let body = if package.len() >= 36 && dol::u32(package, 32)? == 0x005b_bc61 {
                    package
                } else {
                    field_core::optional_section(package, 0)?
                        .context("embedded field model has no body")?
                };
                Ok((u32::from(id), body.to_vec()))
            })
            .collect::<Result<BTreeMap<_, _>>>()?
    } else {
        BTreeMap::new()
    };
    let mut rigs = BTreeMap::new();
    for name in banks.values() {
        if !name.starts_with("characters/") {
            continue;
        }
        let owner = name
            .split('/')
            .nth(1)
            .context("field animation has no owner")?;
        if rigs.contains_key(owner) {
            continue;
        }
        let joints = if let Some(index) = party.iter().position(|name| *name == owner) {
            let body = character_models::party_body(extracted, index as u8 + 1, 0)?;
            world_gpl::named_animation_bindings(&body)?
        } else if let Some(body) = npc.get(&npc_ids[owner]) {
            world_gpl::named_animation_bindings(body)?
        } else if let Some(body) = embedded.get(&npc_ids[owner]) {
            world_gpl::named_animation_bindings(body)?
        } else {
            let body = field_npc_models::standalone_model_body(extracted, npc_ids[owner])?;
            world_gpl::named_animation_bindings(&body)?
        };
        rigs.insert(owner.to_owned(), joints);
    }
    banks
        .iter()
        .map(|(id, name)| {
            let owner = name
                .split('/')
                .nth(1)
                .context("field animation has no owner")?;
            let native = shared_animations::read_bank(extracted, Some(field_id), *id)?;
            let joints = crate::field_bank_bindings::source_tracks(&native)?;
            let empty = Vec::new();
            let rig = rigs.get(owner).unwrap_or(&empty);
            let labels = if name.starts_with("unassigned/") {
                shared_animations::unassigned_track_names(&native)?
            } else {
                world_motion::shared_track_names(&native, rig)
                    .with_context(|| format!("field animation {id} {name}"))?
            };
            Ok((
                *id,
                shared_animations::mapped_motion_path(library, *id)?,
                joints,
                labels,
            ))
        })
        .collect()
}

fn actor_motion(bytes: &[u8]) -> bool {
    if dol::u32(bytes, 0).ok() == Some(0x007b_7960) {
        return true;
    }
    if !matches!(bytes.first(), Some(0 | 1 | 3)) {
        return false;
    }
    compression::decode(bytes)
        .ok()
        .is_some_and(|motion| world_motion::indexed_joints(&motion).is_ok())
}

pub(crate) fn actor_collision(package: &[u8]) -> Result<ModelCollision> {
    let mut collision = ModelCollision::default();
    for (slot, groups) in [(29, &mut collision.floors), (30, &mut collision.solids)] {
        let Some(native) = field_core::optional_section(package, slot)? else {
            continue;
        };
        if actor_motion(native) {
            continue;
        }
        *groups = field_core::collision_groups(native, true)?;
    }
    Ok(collision)
}

fn check_model_media(library: &Path, model: &Model) -> Result<()> {
    for part in &model.parts {
        for path in std::iter::once(part.mesh.as_str())
            .chain(part.textures.iter().map(String::as_str))
            .chain(part.clips.iter().map(|clip| clip.motion.as_str()))
        {
            ensure!(library.join(path).is_file(), "source model requires {path}");
        }
    }
    if let Some(path) = &model.collision {
        ensure!(library.join(path).is_file(), "source model requires {path}");
    }
    Ok(())
}

// Match the native encoder: names select the whole bank when any name
// overlaps; otherwise native IDs select it. Block accidental GLB identity
// matches for joints deliberately left at rest by that selection.
fn shared_joint_map(
    bindings: &[(u16, String)],
    target: &BTreeMap<u16, String>,
    source: &[crate::field_bank_bindings::Track],
    labels: &BTreeMap<u16, String>,
) -> BTreeMap<String, Option<String>> {
    let joints = bindings
        .iter()
        .filter_map(|(id, raw)| Some((*id, target.get(id)?.clone(), raw.clone())))
        .collect::<Vec<_>>();
    let selected = crate::field_bank_bindings::selected_tracks(&joints, source);
    joints
        .iter()
        .zip(selected)
        .filter_map(|((_, target, _), track)| {
            let selected = track
                .and_then(|index| labels.get(&source[index].id))
                .cloned();
            (selected.is_some() || labels.values().any(|name| name == target))
                .then(|| (target.clone(), selected))
        })
        .collect()
}

fn convert_source_document(
    library: &Path,
    archive: &[u8],
    root: &str,
    body: &[u8],
    base: &str,
    source: &str,
    shared: &[(
        u32,
        String,
        Vec<crate::field_bank_bindings::Track>,
        BTreeMap<u16, String>,
    )],
    local_motions: &[usize],
    replace: bool,
) -> Result<()> {
    let mut model: Model = serde_json::from_slice(
        &fs::read(library.join(source))
            .with_context(|| format!("cook source model {source} first"))?,
    )?;
    ensure!(
        model.parts.len() == 1,
        "source model reuse requires one native part"
    );
    let bindings = world_gpl::animation_bindings(body)?;
    let target = world_gpl::animation_target_bindings(bindings.clone())
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let part = &mut model.parts[0];
    part.clips.retain(|clip| clip.animation_resource.is_none());
    for (id, path, joints, rig) in shared {
        let mut binding = clip(path.clone(), 12, Some(*id));
        binding.joints = shared_joint_map(&bindings, &target, joints, rig);
        part.clips.push(binding);
    }
    for &index in local_motions {
        let native = field_core::optional_section(archive, 16 + index)?
            .context("field-local motion is missing")?;
        let joints = world_motion::indexed_joints(native)?;
        let source_names = world_motion::indexed_track_names(native)?;
        let path = format!("{root}/animations/local-motion-{index}.glb");
        ensure!(
            library.join(&path).is_file(),
            "field-local motion requires {path}"
        );
        let mut binding = clip(path, 12, Some(LOCAL_BASE + index as u32));
        binding.joints = bindings
            .iter()
            .filter(|(id, _)| joints.binary_search(id).is_ok())
            .filter_map(|(id, _)| {
                Some((
                    target.get(id)?.clone(),
                    Some(
                        source_names
                            .get(id)
                            .cloned()
                            .unwrap_or_else(|| format!("joint-{id}")),
                    ),
                ))
            })
            .collect();
        part.clips.push(binding);
    }
    check_model_media(library, &model)?;
    let path = library.join(format!("{base}.json"));
    let bytes = resonance_asset_writer::json::encode(&model)?;
    if replace {
        fs::write(path, bytes)?;
        Ok(())
    } else {
        crate::output::write(&path, &bytes)
    }
}

fn convert(
    library: &Path,
    archive: &[u8],
    root: &str,
    section: usize,
    shared: &[(
        u32,
        String,
        Vec<crate::field_bank_bindings::Track>,
        BTreeMap<u16, String>,
    )],
    local_motions: &[usize],
    owner: Option<&str>,
    borrow: Option<&PackageBorrow>,
    motions: &mut BTreeMap<Vec<u8>, String>,
    textures: &mut BTreeMap<Vec<u8>, String>,
) -> Result<()> {
    let native = field_core::optional_section(archive, section)?.context("local actor missing")?;
    let package = native;
    let bare = bare_local_model(native)?;
    let body = if bare {
        native
    } else {
        field_core::optional_section(package, 0)?.context("local actor has no body")?
    };
    let name = format!("unidentified-{}", section - 15);
    let grasshopper = root == "fields/luin-0448" && section == 16;
    let base = owner
        .map(str::to_owned)
        .or_else(|| borrow.and_then(|borrow| borrow.document.clone()))
        .unwrap_or_else(|| format!("{root}/models/{name}"));
    if let Some(source) = borrow.and_then(|borrow| borrow.source_model.as_deref()) {
        return convert_source_document(
            library,
            archive,
            root,
            body,
            &base,
            source,
            shared,
            local_motions,
            false,
        );
    }
    let media = borrow.and_then(|borrow| borrow.media.as_deref());
    let publish_media = borrow.is_none_or(|borrow| borrow.media.is_none() || borrow.publish);
    let start = dol::u32(body, 0)? as usize;
    let end = dol::u32(body, 4)? as usize;
    let mut normalized = body.to_vec();
    if start == 0 {
        // An actor without a TPL puts its model immediately after the GPL.
        // The GPL decoder still needs that boundary in the first header word.
        ensure!(
            end >= 0x20 && end <= body.len(),
            "invalid palette-free actor body"
        );
        normalized[..4].copy_from_slice(&(end as u32).to_be_bytes());
    }
    let body = normalized.as_slice();
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    if start != 0 {
        let tpl = body.get(start..end).context("local palette exceeds body")?;
        ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "local body lacks TPL");
        for index in 0..dol::u32(tpl, 4)? as usize {
            let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
            let path = if let Some(path) = textures
                .get(&texture)
                .filter(|path| media.is_none_or(|media| path.starts_with(media)))
            {
                path.clone()
            } else {
                let path = if let Some(media) = media {
                    format!("{media}/textures/body-texture-{index}.ktx2")
                } else if owner.is_some() {
                    format!("{base}/textures/body-texture-{index}.ktx2")
                } else {
                    format!("{base}-texture-{}.ktx2", index + 1)
                };
                if publish_media {
                    crate::output::write(&library.join(&path), &texture)?;
                } else {
                    ensure!(
                        library.join(&path).is_file(),
                        "borrowed field media requires {path}"
                    );
                }
                textures.insert(texture.clone(), path.clone());
                path
            };
            paths.push(path);
            alpha.push(transparent);
        }
    }
    let (mesh, mut part) = world_gpl::static_model(body, paths.clone(), alpha)
        .with_context(|| format!("local actor section {section}"))?;
    if start == 0 {
        for material in &mut part.materials {
            if material.color.is_none() && material.multiply.is_none() {
                material.blend = false;
            }
        }
    }
    part.mesh = if let Some(media) = media {
        format!("{media}/meshes/body.glb")
    } else if grasshopper {
        "meshes/monsters/grasshopper-0138/meshes/body.glb".into()
    } else if owner.is_some() {
        format!("{base}/meshes/body.glb")
    } else {
        format!("{base}.glb")
    };
    if grasshopper || !publish_media {
        ensure!(
            library.join(&part.mesh).is_file(),
            "field model requires body mesh {}",
            part.mesh
        );
    } else {
        crate::output::write(&library.join(&part.mesh), &mesh)?;
    }
    let bindings = world_gpl::animation_bindings(body)?;
    let target_bindings = world_gpl::animation_target_bindings(bindings.clone());
    let target = target_bindings.into_iter().collect::<BTreeMap<_, _>>();
    let count = if bare {
        0
    } else {
        dol::u32(package, 0)? as usize
    };
    for index in 2..count {
        let Some(native) = field_core::optional_section(package, index)? else {
            continue;
        };
        // Actor packages reserve their final two members for optional collision.
        if index >= 29 && !actor_motion(native) {
            continue;
        }
        let slot = u16::try_from(4 + index * 4)?;
        if let Some(path) = borrow.and_then(|borrow| borrow.clips.get(&slot)) {
            ensure!(
                library.join(path).is_file(),
                "borrowed field clip requires {path}"
            );
            part.clips.push(clip(path.clone(), slot, None));
            continue;
        }
        if let Some(path) = owner
            .is_none()
            .then(|| reused_field_motion(root, section, slot))
            .flatten()
        {
            ensure!(
                library.join(path).is_file(),
                "field-local motion requires {path}"
            );
            part.clips.push(clip(path.into(), slot, None));
            continue;
        }
        if let Some(path) = owner.and_then(|owner| reused_motion(owner, slot)) {
            // Reused field motions keep one physical copy in their first owner.
            part.clips.push(clip(path.into(), slot, None));
            continue;
        }
        // Tower of Mana repeats the Ranch idle verbatim; write one canonical clip.
        let source_path = if owner.is_none()
            && root == "fields/tower-of-mana-0362"
            && section == 20
            && matches!(slot, 12 | 36)
        {
            Some(ISELIA_RANCH_IDLE.to_owned())
        } else {
            owner.and_then(|owner| gaoracchia_motion(owner, slot))
        };
        if let Some(path) = &source_path {
            if library.join(path).is_file() {
                part.clips.push(clip(path.clone(), slot, None));
                continue;
            }
        }
        let expanded = if dol::u32(native, 0)? == 0x007b_7960 {
            native.to_vec()
        } else {
            compression::decode(native)?
        };
        let motion = world_motion::encode_bound(&expanded, &mesh, &bindings)
            .with_context(|| format!("local actor {section} motion {slot}"))?;
        let path = if let Some(path) = motions.get(&motion) {
            path.clone()
        } else {
            let label = match (owner, slot) {
                (_, 12) => "idle".to_owned(),
                (None, 36) if root == "fields/tethealla-base-0253" && section == 17 => {
                    "walk".to_owned()
                }
                (None, 40) if root == "fields/tethealla-base-0253" && section == 17 => {
                    "run".to_owned()
                }
                (None, 36) if root == "fields/temple-of-ice-0323" && section == 20 => {
                    "walk".to_owned()
                }
                (None, 36) if root == "fields/meltokio-0466" && matches!(section, 17 | 20 | 21) => {
                    "walk".to_owned()
                }
                (None, 40) if root == "fields/meltokio-0466" && section == 21 => "run".to_owned(),
                (Some(HEIMDALL_OWNER), 36) => "walk".to_owned(),
                (Some(HEIMDALL_OWNER), 40) => "run".to_owned(),
                (Some(TETHEALLA_WALKER), 40) => "run".to_owned(),
                (None, 36) if root == "fields/tower-of-mana-0365" && section == 21 => {
                    "walk".to_owned()
                }
                _ => format!("motion-{slot}"),
            };
            let path = source_path.unwrap_or_else(|| {
                if let Some(media) = media {
                    format!("{media}/animations/{label}.glb")
                } else if owner.is_some() {
                    format!("{base}/animations/{label}.glb")
                } else {
                    format!("{root}/animations/{name}-{label}.glb")
                }
            });
            crate::output::write(&library.join(&path), &motion)?;
            motions.insert(motion, path.clone());
            path
        };
        part.clips.push(clip(path, slot, None));
    }
    for (id, path, joints, rig) in shared {
        let mut binding = clip(path.clone(), 12, Some(*id));
        if borrow
            .and_then(|borrow| borrow.animated_banks.as_ref())
            .is_none_or(|owners| {
                owners
                    .iter()
                    .any(|owner| path.starts_with(&format!("animations/{owner}/")))
            })
        {
            binding.joints = shared_joint_map(&bindings, &target, joints, rig);
        }
        part.clips.push(binding);
    }
    for &index in local_motions {
        let motion = field_core::optional_section(archive, 16 + index)?
            .context("Tower local motion is missing")?;
        let joints = world_motion::indexed_joints(motion)?;
        let source_names = world_motion::indexed_track_names(motion)?;
        let mut binding = clip(
            format!("{root}/animations/local-motion-{index}.glb"),
            12,
            Some(LOCAL_BASE + index as u32),
        );
        binding.joints = bindings
            .iter()
            .filter(|(id, _)| joints.binary_search(id).is_ok())
            .filter_map(|(id, _)| {
                Some((
                    target.get(id)?.clone(),
                    Some(
                        source_names
                            .get(id)
                            .cloned()
                            .unwrap_or_else(|| format!("joint-{id}")),
                    ),
                ))
            })
            .collect();
        part.clips.push(binding);
    }
    let collision = if bare {
        ModelCollision::default()
    } else {
        actor_collision(package)?
    };
    let collision = if collision
        .floors
        .iter()
        .chain(&collision.solids)
        .any(|group| !group.triangles.is_empty())
    {
        let path = if let Some(media) = media {
            format!("{media}/collision.glb")
        } else if owner.is_some() {
            format!("{base}/collision.glb")
        } else {
            format!("{base}-collision.glb")
        };
        if publish_media {
            crate::output::write(
                &library.join(&path),
                &collision_mesh::encode_model(&collision)?,
            )?;
        } else {
            ensure!(
                library.join(&path).is_file(),
                "borrowed field collision requires {path}"
            );
        }
        Some(path)
    } else {
        None
    };
    let outline = if bare {
        None
    } else {
        field_core::optional_section(package, 1)?
    };
    let inherited_outline = outline.map(|bytes| dol::u32(bytes, 0)).transpose()? == Some(0);
    ensure!(
        outline.is_none()
            || owner.is_some()
            || media.is_some()
            || inherited_outline
            || matches!(
                (root, section),
                ("fields/temple-of-ice-0323", 20)
                    | ("fields/temple-of-ice-0324", 25)
                    | ("fields/meltokio-0466", 20 | 21)
                    | ("fields/meltokio-0467", 20)
                    | ("fields/triet-ruins-0220", 19)
                    | ("fields/luin-0448", 16)
            ),
        "field-owned outline needs a supported native shape"
    );
    let hidden_geometry = {
        part.prepare_bones(&mesh)?;
        // The Ice Temple bird uses its second TPL image as a two-row eye atlas.
        if matches!(
            (root, section),
            ("fields/temple-of-ice-0323", 20) | ("fields/temple-of-ice-0324", 25)
        ) {
            ensure!(paths.len() == 4, "Ice Temple bird palette changed");
            part.appearance = Some(AppearanceTextures {
                eyes: Some(1),
                mouth: None,
                variant: None,
                costume: None,
            });
            for material in &mut part.materials {
                if material.color.is_none() && material.multiply.is_none() {
                    material.blend = false;
                }
            }
        }
        part.bone_names
            .iter()
            .filter(|name| name.starts_with("kk"))
            .cloned()
            .collect()
    };
    let mut parts = vec![part];
    if let Some(outline) = outline {
        let own_palette = dol::u32(outline, 0)? != 0;
        ensure!(
            !own_palette
                || owner.is_none()
                    && (media.is_some() || root == "fields/triet-ruins-0220" && section == 19),
            "unexpected field outline palette"
        );
        let normalized = if own_palette {
            outline.to_vec()
        } else {
            world_gpl::with_primary_palette(body, outline)?
        };
        let outline_paths = if own_palette {
            let start = dol::u32(&normalized, 0)? as usize;
            let end = dol::u32(&normalized, 4)? as usize;
            let tpl = normalized
                .get(start..end)
                .context("field outline palette exceeds body")?;
            ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "field outline lacks TPL");
            let mut outline_paths = Vec::new();
            for index in 0..dol::u32(tpl, 4)? as usize {
                let (texture, _) = world_texture::tpl_texture(tpl, index)?;
                if let Some(path) = textures
                    .get(&texture)
                    .filter(|path| media.is_none_or(|media| path.starts_with(media)))
                {
                    outline_paths.push(path.clone());
                } else {
                    let path = if let Some(media) = media {
                        format!("{media}/textures/outline-texture-{index}.ktx2")
                    } else {
                        format!("{base}-part-1-texture-{}.ktx2", index + 1)
                    };
                    if publish_media {
                        crate::output::write(&library.join(&path), &texture)?;
                    } else {
                        ensure!(
                            library.join(&path).is_file(),
                            "borrowed field outline texture requires {path}"
                        );
                    }
                    textures.insert(texture, path.clone());
                    outline_paths.push(path);
                }
            }
            outline_paths
        } else {
            paths.clone()
        };
        let (outline_mesh, mut part) = world_gpl::static_model(
            &normalized,
            outline_paths.clone(),
            vec![false; outline_paths.len()],
        )?;
        part.mesh = if let Some(media) = media {
            format!("{media}/meshes/outline.glb")
        } else if grasshopper {
            "meshes/monsters/grasshopper-0138/meshes/outline.glb".into()
        } else {
            owner
                .map(|owner| format!("{owner}/meshes/outline.glb"))
                .unwrap_or_else(|| format!("{base}-part-1.glb"))
        };
        part.resource = 1;
        part.outline_color = Some([0, 0, 0, 127]);
        part.clips = parts[0].clips.clone();
        for material in &mut part.materials {
            material.cull = CullFace::Front;
            material.blend = true;
            material.draw_order += 65536;
        }
        if grasshopper || !publish_media {
            ensure!(
                library.join(&part.mesh).is_file(),
                "field model requires outline mesh {}",
                part.mesh
            );
        } else {
            crate::output::write(&library.join(&part.mesh), &outline_mesh)?;
        }
        parts.push(part);
    }
    let model = Model {
        parts,
        collision,
        hidden_geometry,
    };
    crate::output::json(
        &library.join(if owner.is_some() {
            format!("{base}/model.json")
        } else {
            format!("{base}.json")
        }),
        &model,
    )
}

/// A standalone GPL with an embedded TPL is a local model even without an
/// actor-package header (Gaoracchia's cone is one native example).
pub(crate) fn bare_local_model(bytes: &[u8]) -> Result<bool> {
    if bytes.len() < 36 || dol::u32(bytes, 32)? != 0x005b_bc61 {
        return Ok(false);
    }
    let start = dol::u32(bytes, 0)? as usize;
    let end = dol::u32(bytes, 4)? as usize;
    Ok(start >= 36
        && start + 8 <= end
        && end <= bytes.len()
        && dol::u32(bytes, start)? == 0x0020_af30)
}

fn convert_volt(
    library: &Path,
    archive: &[u8],
    root: &str,
    shared: &[(
        u32,
        String,
        Vec<crate::field_bank_bindings::Track>,
        BTreeMap<u16, String>,
    )],
    publish: bool,
) -> Result<()> {
    let package = field_core::optional_section(archive, 16)?.context("Volt package is missing")?;
    let body = field_core::optional_section(package, 0)?.context("Volt has no body")?;
    let base = format!("{root}/models/volt");
    let media = "meshes/props/volt";
    let start = dol::u32(body, 0)? as usize;
    let end = dol::u32(body, 4)? as usize;
    let tpl = body.get(start..end).context("Volt palette exceeds body")?;
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "Volt lacks a TPL palette");
    let mut textures = Vec::new();
    let mut alpha = Vec::new();
    for index in 0..dol::u32(tpl, 4)? as usize {
        let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
        let path = format!("{media}/textures/body-texture-{index}.ktx2");
        volt_media(library, &path, &texture, publish)?;
        textures.push(path);
        alpha.push(transparent);
    }
    let (mesh, mut part) = world_gpl::static_model(body, textures.clone(), alpha)?;
    part.mesh = format!("{media}/meshes/body.glb");
    volt_media(library, &part.mesh, &mesh, publish)?;
    part.prepare_bones(&mesh)?;
    let bindings = world_gpl::animation_bindings(body)?;
    let target = world_gpl::animation_target_bindings(bindings.clone())
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mut unique = BTreeMap::<Vec<u8>, String>::new();
    for index in 2..31 {
        let Some(native) = field_core::optional_section(package, index)? else {
            continue;
        };
        let expanded = if dol::u32(native, 0)? == 0x007b_7960 {
            native.to_vec()
        } else {
            compression::decode(native)?
        };
        let slot = u16::try_from(4 + index * 4)?;
        let motion = world_motion::encode_bound(&expanded, &mesh, &bindings)
            .with_context(|| format!("Volt motion slot {slot}"))?;
        let path = if let Some(path) = unique.get(&motion) {
            path.clone()
        } else {
            let label = match slot {
                12 => "idle".to_owned(),
                36 => "walk".to_owned(),
                40 => "run".to_owned(),
                _ => format!("motion-{slot}"),
            };
            let path = format!("{media}/animations/{label}.glb");
            volt_media(library, &path, &motion, publish)?;
            unique.insert(motion, path.clone());
            path
        };
        part.clips.push(clip(path, slot, None));
    }
    for (id, path, joints, labels) in shared {
        let mut binding = clip(path.clone(), 12, Some(*id));
        binding.joints = shared_joint_map(&bindings, &target, joints, labels);
        part.clips.push(binding);
    }
    let hidden_geometry = part
        .bone_names
        .iter()
        .filter(|name| name.starts_with("kk"))
        .cloned()
        .collect();
    let mut parts = vec![part];
    if let Some(outline) = field_core::optional_section(package, 1)? {
        let normalized = world_gpl::with_primary_palette(body, outline)?;
        let (outline_mesh, mut part) = world_gpl::static_model(
            &normalized,
            textures,
            vec![false; dol::u32(tpl, 4)? as usize],
        )?;
        part.mesh = format!("{media}/meshes/outline.glb");
        part.resource = 1;
        part.outline_color = Some([0, 0, 0, 127]);
        part.clips = parts[0].clips.clone();
        for material in &mut part.materials {
            material.cull = CullFace::Front;
            material.blend = true;
            material.draw_order += 65536;
        }
        volt_media(library, &part.mesh, &outline_mesh, publish)?;
        parts.push(part);
    }
    let model = Model {
        parts,
        collision: None,
        hidden_geometry,
    };
    crate::output::json(&library.join(format!("{base}.json")), &model)
}

fn volt_media(library: &Path, path: &str, bytes: &[u8], publish: bool) -> Result<()> {
    let path = library.join(path);
    if publish {
        crate::output::write(&path, bytes)
    } else {
        ensure!(
            fs::read(&path)? == bytes,
            "shared Volt media differs at {}",
            path.display()
        );
        Ok(())
    }
}

/// Convert a field's native local actor packages.
pub fn cook_field_local_models(extracted: &Path, library: &Path, id: u32) -> Result<()> {
    let (exe, archive) = field_core::read(extracted, id)?;
    let banks = field_actors::animation_banks(extracted, &exe, &archive)?;
    cook_local_models(extracted, library, id, &exe, &archive, &banks)
}

pub(crate) fn cook_local_models(
    extracted: &Path,
    library: &Path,
    id: u32,
    exe: &[u8],
    archive: &[u8],
    banks: &BTreeMap<u32, String>,
) -> Result<()> {
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let shared = shared_clips(extracted, library, id, banks)?;
    let mut borrows = package_borrows(id)?;
    for (section, borrow) in family_borrows(extracted, library, exe, id, archive)? {
        ensure!(
            borrows.insert(section, borrow).is_none(),
            "overlapping package borrow"
        );
    }
    if matches!(id, 80 | 475) {
        if id == 475 {
            let package =
                field_core::optional_section(archive, 16)?.context("Volt package is missing")?;
            let borrow = borrows.get(&16).context("Volt source mapping is missing")?;
            ensure!(borrow.source.is_some(), "Volt source mapping is incomplete");
            check_package_source(extracted, exe, package, borrow)?;
        }
        convert_volt(library, archive, &root, &shared, id == 80)?;
    }
    let local_motions = if id == 184 {
        vec![0, 1, 13]
    } else {
        local_motion_sections(archive)?
    };
    let mut motions = BTreeMap::new();
    let mut textures = BTreeMap::new();
    for (section, actor) in local_models(id, archive)? {
        if actor.model.starts_with("meshes/") || section == 16 && matches!(id, 80 | 475) {
            continue;
        }
        let package =
            field_core::optional_section(archive, section)?.context("local actor missing")?;
        let mut borrow = borrows.remove(&section).unwrap_or_default();
        check_package_source(extracted, exe, package, &borrow)?;
        borrow.document = Some(
            actor
                .model
                .strip_suffix(".json")
                .context("local model lacks .json")?
                .to_owned(),
        );
        convert(
            library,
            archive,
            &root,
            section,
            &shared,
            &local_motions,
            None,
            Some(&borrow),
            &mut motions,
            &mut textures,
        )?;
    }
    Ok(())
}

fn cook_gaoracchia_cone(library: &Path, archive: &[u8]) -> Result<()> {
    let body = field_core::optional_section(archive, 16)?.context("Gaoracchia cone missing")?;
    let tpl = field_core::optional_section(archive, 17)?.context("Gaoracchia palette missing")?;
    ensure!(
        dol::u32(body, 0)? == 0 && dol::u32(tpl, 0)? == 0x0020_af30 && dol::u32(tpl, 4)? == 1,
        "Gaoracchia cone has an unexpected GPL/TPL shape"
    );
    // The bare GPL leaves its geometry end unset and has no model hierarchy.
    // Its separate TPL supplies texture pixels, not a trailing model section.
    let mut body = body.to_vec();
    let end = u32::try_from(body.len())?;
    body[..4].copy_from_slice(&end.to_be_bytes());
    let (texture, transparent) = world_texture::tpl_texture(tpl, 0)?;
    let texture_path = format!("{GAORACCHIA_CONE}/textures/body-texture-0.ktx2");
    crate::output::write(&library.join(&texture_path), &texture)?;
    let (mesh, mut part) = world_gpl::static_model(&body, vec![texture_path], vec![transparent])?;
    part.mesh = format!("{GAORACCHIA_CONE}/meshes/body.glb");
    crate::output::write(&library.join(&part.mesh), &mesh)?;
    crate::output::json(
        &library.join(format!("{GAORACCHIA_CONE}/model.json")),
        &Model {
            parts: vec![part],
            collision: None,
            hidden_geometry: Vec::new(),
        },
    )
}

fn cook_latheon_capture(library: &Path, archive: &[u8]) -> Result<()> {
    let body = field_core::optional_section(archive, 16)?.context("Latheon capture GPL missing")?;
    ensure!(
        dol::u32(body, 0)? == 0 && dol::u32(body, 4)? == 0 && dol::u32(body, 32)? == 0x005b_bc61,
        "Latheon capture has an unexpected bare GPL"
    );
    let mut body = body.to_vec();
    let end = u32::try_from(body.len())?;
    body[..4].copy_from_slice(&end.to_be_bytes());
    // The native draw samples the current scene capture as texture stage 0.
    // Supply that stage for GPL decoding, then express it as the editable
    // capture role rather than a nonexistent image file.
    let (mesh, mut part) = world_gpl::static_model(&body, vec!["capture".into()], vec![false])?;
    ensure!(
        part.materials.len() == 1,
        "Latheon capture has multiple draws"
    );
    part.textures.clear();
    let material = &mut part.materials[0];
    material.image = SceneImage::Capture;
    material.color = None;
    material.multiply = None;
    material.vertex_color = true;
    material.blend = false;
    part.mesh = format!("{LATHEON_CAPTURE}/meshes/body.glb");
    crate::output::write(&library.join(&part.mesh), &mesh)?;
    crate::output::json(
        &library.join(format!("{LATHEON_CAPTURE}/model.json")),
        &Model {
            parts: vec![part],
            collision: None,
            hidden_geometry: Vec::new(),
        },
    )
}

/// Publish one canonical, single-part field model from its native MAP package.
/// Another field using the same owner only needs the finished model document.
pub(crate) fn cook_shared_owner(
    extracted: &Path,
    library: &Path,
    id: u32,
    resource: u32,
    model_path: &str,
    banks: &BTreeMap<u32, String>,
) -> Result<()> {
    let owner = model_path
        .strip_suffix("/model.json")
        .context("canonical model path lacks model.json")?;
    ensure!(
        owner.starts_with("meshes/"),
        "canonical local model is not under meshes"
    );
    if library.join(model_path).is_file() {
        for slot in [12, 36, 80] {
            let path = reused_motion(owner, slot)
                .map(str::to_owned)
                .or_else(|| gaoracchia_motion(owner, slot));
            if let Some(path) = path {
                ensure!(
                    library.join(&path).is_file(),
                    "canonical model {owner} requires shared motion {path}"
                );
            }
        }
    }
    if id == LATHEON_SOURCE && owner == SYLVARANT_BASE_PROP {
        ensure!(
            library.join(model_path).is_file()
                && library
                    .join(format!("{owner}/animations/idle.glb"))
                    .is_file(),
            "Latheon requires the canonical Sylvarant Base prop from field 279"
        );
        return Ok(());
    }
    let mut section = 16
        + usize::try_from(
            resource
                .checked_sub(LOCAL_BASE)
                .context("invalid local resource")?,
        )?;
    let source_id = if owner == "meshes/props/thoda-geyser-unidentified-2" {
        section = 17;
        8
    } else if matches!(
        owner,
        "meshes/props/tower-of-salvation-unidentified-1-variant-2"
            | "meshes/props/tower-of-salvation-unidentified-4"
    ) {
        168
    } else if owner == "meshes/props/field-unidentified-1-variant-3" {
        section = 16;
        151
    } else if owner == TETHEALLA_WALKER {
        256
    } else if owner.starts_with("meshes/props/tethealla-base-unidentified-") {
        253
    } else if owner == "meshes/props/palmacosta-ranch-unidentified-9" {
        section = 24;
        205
    } else if owner.starts_with("meshes/props/tower-of-mana-unidentified-") {
        363
    } else if owner == "meshes/props/temple-of-ice-unidentified-9" {
        section = 24;
        324
    } else if owner == "meshes/props/island-ranch-unidentified-1-variant-2" {
        section = 16;
        229
    } else if owner == "meshes/props/island-ranch-unidentified-1" {
        section = 16;
        230
    } else if owner == GAORACCHIA_CONE || gaoracchia_motion(owner, 12).is_some() {
        GAORACCHIA_SOURCE
    } else if owner.starts_with("meshes/props/latheon-gorge-unidentified-") {
        LATHEON_SOURCE
    } else {
        id
    };
    if library.join(model_path).is_file() && id != source_id {
        let model: Model = serde_json::from_slice(&fs::read(library.join(model_path))?)?;
        check_model_media(library, &model)?;
        return Ok(());
    }
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let rows = dol::slice(&exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let row = &rows[source_id as usize * 24..source_id as usize * 24 + 24];
    let archive = field_core::native_archive(extracted, &exe, row, source_id as usize)?
        .with_context(|| format!("field {source_id} has no native archive"))?;
    if library.join(model_path).is_file() && local_motion_sections(&archive)?.is_empty() {
        let model: Model = serde_json::from_slice(&fs::read(library.join(model_path))?)?;
        check_model_media(library, &model)?;
        return Ok(());
    }
    if owner == GAORACCHIA_CONE {
        ensure!(section == 16, "Gaoracchia cone has an unexpected resource");
        return cook_gaoracchia_cone(library, &archive);
    }
    if owner == LATHEON_CAPTURE {
        ensure!(section == 16, "Latheon capture has an unexpected resource");
        return cook_latheon_capture(library, &archive);
    }
    let package = field_core::optional_section(&archive, section)?
        .context("canonical local model has no native package")?;
    ensure!(
        dol::u32(package, 0)? == 31,
        "canonical local model {owner} has no package"
    );
    let root = format!("fields/{}", field_core::field_name(source_id as usize));
    let local_motions = local_motion_sections(&archive)?
        .into_iter()
        .filter(|index| {
            library
                .join(format!("{root}/animations/local-motion-{index}.glb"))
                .is_file()
        })
        .collect::<Vec<_>>();
    if library.join(model_path).is_file() {
        let model: Model = serde_json::from_slice(&fs::read(library.join(model_path))?)?;
        check_model_media(library, &model)?;
        if !local_motions.iter().all(|index| {
            model.parts.iter().any(|part| {
                part.clips
                    .iter()
                    .any(|clip| clip.animation_resource == Some(LOCAL_BASE + *index as u32))
            })
        }) {
            let body = field_core::optional_section(package, 0)?
                .context("canonical local model has no body")?;
            convert_source_document(
                library,
                &archive,
                &root,
                body,
                model_path
                    .strip_suffix(".json")
                    .context("canonical model path lacks .json")?,
                model_path,
                &shared_clips(extracted, library, id, banks)?,
                &local_motions,
                true,
            )?;
        }
        return Ok(());
    }
    for slot in [12, 36] {
        if let Some(path) = reused_motion(owner, slot) {
            ensure!(
                library.join(path).is_file(),
                "canonical model {owner} requires shared motion {path}"
            );
        }
    }
    if owner == HEIMDALL_OWNER {
        ensure!(
            field_core::optional_section(package, 1)?.is_some(),
            "Heimdall model has no outline"
        );
    } else {
        ensure!(
            field_core::optional_section(package, 1)?.is_none(),
            "canonical local model {owner} needs an outlined owner"
        );
    }
    convert(
        library,
        &archive,
        &root,
        section,
        &shared_clips(extracted, library, id, banks)?,
        &local_motions,
        Some(owner),
        None,
        &mut BTreeMap::new(),
        &mut BTreeMap::new(),
    )
}
