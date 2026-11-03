//! The installed port retains a few original actor and motion payloads.
use super::*;
use resonance_content::{
    SceneClip,
    field::document::{Actor, Actors, AnimationBinding, Scene},
};

pub(super) fn retains_source(source: &Source, name: &str) -> Result<bool> {
    if !name.to_ascii_lowercase().ends_with(".bin") || !source.contains(name)? {
        return Ok(false);
    }
    let bytes = source.require(name)?;
    if bytes.get(..4) == Some(&31u32.to_be_bytes()) {
        return Ok(true);
    }
    if bytes.len() < 4 || !(2..=65536).contains(&pe::word(&bytes, 0)?) {
        return Ok(false);
    }
    let package = Package::parse(&bytes)?;
    for slot in 1..package.len() {
        if package
            .member(slot)?
            .is_some_and(|motion| motion.get(..4) == Some(&0x007b7960u32.to_le_bytes()))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn refs(source: &Source, bank: &str) -> Result<Vec<(u32, String)>> {
    let bytes = source.require(bank)?;
    let package = Package::parse(&bytes)?;
    let ids = source.require(
        package
            .reference(0)?
            .context("field bank has no ID table")?,
    )?;
    (1..package.len())
        .filter_map(|slot| match package.reference(slot) {
            Ok(Some(name)) => {
                Some(pe::half(&ids, (slot - 1) * 2).map(|id| (u32::from(id), name.to_owned())))
            }
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

pub(super) fn cook(source: &Source, library: &Path, mappings: &mut ModelMappings) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let fields: resonance_content::library::Mappings =
        serde_json::from_slice(&fs::read(library.join(resonance_content::library::FIELDS))?)?;
    let mut actors_added = 0;
    let mut banks_added = 0;
    for (field, name) in fields.iter() {
        let filename = image.text(pe::word(image.slice(0x8cb908 + field * 24, 4)?, 0)?)?;
        let bytes = source.require(filename)?;
        let package = Package::parse(&bytes)?;
        let Some(bank_name) = package.reference(7)? else {
            continue;
        };
        let root = format!("fields/{name}");
        let actors_path = library.join(&root).join("actors.json");
        let mut actors: Actors = serde_json::from_slice(&fs::read(&actors_path)?)?;
        if bank_name.to_ascii_lowercase().ends_with(".pac") {
            let mut changed = false;
            for (resource, filename) in refs(source, bank_name)? {
                if !filename.to_ascii_lowercase().ends_with(".bin") {
                    continue;
                }
                let bytes = source.require(&filename)?;
                if bytes.get(..4) != Some(&31u32.to_be_bytes()) {
                    continue;
                }
                let owner = format!("field-models/{name}/model-{resource:08x}");
                resonance_importer::cook_field_model_package(library, resource, &owner, &bytes)?;
                mappings
                    .0
                    .insert(resource, ModelTarget::Model(owner.clone()));
                actors.models.retain(|model| !matches!(model, ModelBinding::Local(actor) if actor.resource == resource));
                actors.models.push(ModelBinding::Local(Actor {
                    resource,
                    model: format!("meshes/{owner}/model.json"),
                    hidden_geometry: None,
                }));
                if resource < 53 {
                    actors.local_resources.insert(resource);
                }
                actors_added += 1;
                changed = true;
            }
            if changed {
                output::json(&actors_path, &actors)?;
            }
            continue;
        }
        if !bank_name.to_ascii_lowercase().ends_with(".bin") {
            continue;
        }
        let bytes = source.require(bank_name)?;
        let bank = Package::parse(&bytes)?;
        if bank.len() <= 1 {
            continue;
        }
        let ids = bank.member(0)?.context("retained field bank has no IDs")?;
        let mut scene: Scene =
            serde_json::from_slice(&fs::read(library.join(&root).join("scene.json"))?)?;
        for slot in 1..bank.len() {
            let Some(motion) = bank.member(slot)? else {
                continue;
            };
            if motion.get(..4) != Some(&0x007b7960u32.to_le_bytes()) {
                continue;
            }
            let resource = u32::from(pe::half(ids, (slot - 1) * 2)?);
            let mut bindings = Vec::new();
            let mut targets = Vec::<(u32, String)>::new();
            for binding in &actors.models {
                match binding {
                    ModelBinding::Local(actor) => {
                        targets.push((actor.resource, actor.model.clone()))
                    }
                    ModelBinding::Shared(name) => {
                        let (&id, target) = mappings
                            .0
                            .iter()
                            .find(|(_, target)| target.name() == name)
                            .context("shared actor is not mapped")?;
                        targets.push((id, format!("meshes/{}/model.json", target.model())));
                    }
                    ModelBinding::Group { group } => {
                        let groups: BTreeMap<String, Vec<ModelBinding>> = serde_json::from_slice(
                            &fs::read(library.join(document::MODEL_GROUPS))?,
                        )?;
                        for binding in groups.get(group).context("missing model group")? {
                            let ModelBinding::Shared(name) = binding else {
                                anyhow::bail!("nested field model group")
                            };
                            let (&id, target) = mappings
                                .0
                                .iter()
                                .find(|(_, target)| target.name() == name)
                                .context("group actor is not mapped")?;
                            targets.push((id, format!("meshes/{}/model.json", target.model())));
                        }
                    }
                }
            }
            let mut next = 0;
            for (actor, path) in targets {
                let model: Model = serde_json::from_slice(&fs::read(library.join(path))?)?;
                for (part, spec) in model.parts.iter().enumerate() {
                    let mesh = fs::read(library.join(&spec.mesh))?;
                    let skeleton = Skeleton::from_glb(&mesh)?;
                    if !matches(motion, skeleton.bones.len())? {
                        continue;
                    }
                    let names = skeleton
                        .bones
                        .iter()
                        .map(|bone| bone.name.clone())
                        .collect::<Vec<_>>();
                    let bytes = crate::legacy_motion::convert(motion, &mesh, &names)?;
                    let path =
                        format!("{root}/animations/retained-{resource:08x}/actor-{next}.glb");
                    next += 1;
                    output::write(&library.join(&path), &bytes)?;
                    bindings.push(AnimationBinding {
                        actors: vec![mappings.0[&actor].name().to_owned()],
                        parts: vec![part as u16],
                        motion: path,
                        joints: Default::default(),
                        resource_slot: 12,
                        pose_chains: Vec::new(),
                    });
                }
            }
            for part in &mut scene.parts {
                let mesh = fs::read(library.join(&part.mesh))?;
                let skeleton = Skeleton::from_glb(&mesh)?;
                if !matches(motion, skeleton.bones.len())? {
                    continue;
                }
                let names = skeleton
                    .bones
                    .iter()
                    .map(|bone| bone.name.clone())
                    .collect::<Vec<_>>();
                let bytes = crate::legacy_motion::convert(motion, &mesh, &names)?;
                let path = format!(
                    "{root}/animations/retained-{resource:08x}/scenery-{}.glb",
                    part.resource
                );
                output::write(&library.join(&path), &bytes)?;
                part.clips
                    .retain(|clip| clip.animation_resource != Some(resource));
                part.clips.push(SceneClip {
                    motion: path,
                    joints: Default::default(),
                    resource_slot: 12,
                    animation_resource: Some(resource),
                    duration_seconds: 0.,
                    pose_chains: Vec::new(),
                });
            }
            actors.local_animations.insert(resource, bindings);
            banks_added += 1;
        }
        output::json(&actors_path, &actors)?;
        output::json(&library.join(&root).join("scene.json"), &scene)?;
    }
    eprintln!(
        "  Retained field resources: {actors_added} actor packages and {banks_added} motion banks"
    );
    Ok(())
}

fn matches(bytes: &[u8], bones: usize) -> Result<bool> {
    let count = usize::from(pe::half(bytes, 10)?);
    let start = pe::word(bytes, 28)? as usize;
    for index in 0..count {
        if usize::from(pe::half(bytes, start + index * 16 + 10)?) < bones {
            return Ok(true);
        }
    }
    Ok(false)
}
