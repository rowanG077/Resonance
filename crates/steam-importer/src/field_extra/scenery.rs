//! Script-selected scenery motions bind to the same native rig as its default clip.
use super::*;
use crate::{animation::Animation, model};
use resonance_content::{
    SceneClip,
    field::{LOCAL_MODEL_RESOURCES, document::Scene},
};

pub(super) fn cook(source: &Source, library: &Path) -> Result<()> {
    let resources = Resources::read(source)?;
    let image = pe::Image::parse(&source.executable)?;
    let fields: resonance_content::library::Mappings =
        serde_json::from_slice(&fs::read(library.join(resonance_content::library::FIELDS))?)?;
    let mut rigs = BTreeMap::<String, (model::Rig, Skeleton)>::new();
    let mut animations = BTreeMap::<String, Animation>::new();
    let mut files = BTreeMap::<(String, String), String>::new();
    let mut missing = BTreeSet::new();
    let mut added = 0;
    for (field, name) in fields.iter() {
        let filename = image.text(pe::word(image.slice(0x8cb908 + field * 24, 4)?, 0)?)?;
        let bytes = source.require(filename)?;
        let package = Package::parse(&bytes)?;
        let root = format!("fields/{name}");
        let script = fs::read(library.join(&root).join("scripts/scenario.ssb"))?;
        let (declared, _) = crate::field_actors::declarations(&script)?;
        let mut motions = BTreeMap::new();
        for resource in declared {
            if let Ok(name) = resources.loaded(resource)
                && name.to_ascii_lowercase().ends_with(".toanmb")
            {
                motions.insert(resource, name.to_owned());
            }
        }
        if let Some(bank) = package.reference(7)?
            && bank.to_ascii_lowercase().ends_with(".pac")
        {
            for (resource, name) in super::legacy::refs(source, bank)? {
                if name.to_ascii_lowercase().ends_with(".toanmb") {
                    motions.insert(resource, name);
                }
            }
        }
        for member in 16..package.len() {
            if let Some(name) = package.reference(member)?
                && name.to_ascii_lowercase().ends_with(".toanmb")
            {
                motions.insert(
                    LOCAL_MODEL_RESOURCES.start + (member - 16) as u32,
                    name.to_owned(),
                );
            }
        }
        if motions.is_empty() {
            continue;
        }
        let path = library.join(&root).join("scene.json");
        let mut scene: Scene = serde_json::from_slice(&fs::read(&path)?)?;
        let mut changed = false;
        for (resource, name) in motions {
            if !source.contains(&name)? {
                missing.insert(name);
                continue;
            }
            let motion_key = name.to_ascii_lowercase();
            if !animations.contains_key(&motion_key) {
                animations.insert(
                    motion_key.clone(),
                    Animation::read(&source.require(&name)?)?,
                );
            }
            let animation = &animations[&motion_key];
            for part in &mut scene.parts {
                let descriptor = package
                    .reference(usize::from(part.resource))?
                    .context("scenery has no source descriptor")?;
                if !rigs.contains_key(descriptor) {
                    let rig = model::Rig::read(&source.require(descriptor)?)?;
                    let skeleton = Skeleton::from_glb(&fs::read(library.join(&part.mesh))?)?;
                    rigs.insert(descriptor.into(), (rig, skeleton));
                }
                let (rig, skeleton) = &rigs[descriptor];
                if !animation.targets.is_empty()
                    && !animation.targets.iter().any(|target| {
                        matches!(target.kind, 3 | 0x10014 | 0x20003)
                            && rig
                                .animation_bindings
                                .contains(&(target.kind, target.binding))
                    })
                {
                    continue;
                }
                let key = (motion_key.clone(), descriptor.to_ascii_lowercase());
                let motion = if let Some(path) = files.get(&key) {
                    path.clone()
                } else {
                    let path = format!(
                        "{root}/animations/event-{resource:08x}-scenery-{}.glb",
                        part.resource
                    );
                    let bytes = animation.bind(rig, skeleton)?.encode(skeleton)?;
                    output::write(&library.join(&path), &bytes)?;
                    files.insert(key, path.clone());
                    path
                };
                part.clips.retain(|clip| {
                    clip.animation_resource != Some(resource) || clip.resource_slot != 12
                });
                part.clips.push(SceneClip {
                    motion,
                    joints: Default::default(),
                    resource_slot: 12,
                    animation_resource: Some(resource),
                    duration_seconds: 0.,
                    pose_chains: Vec::new(),
                });
                changed = true;
                added += 1;
            }
        }
        if changed {
            output::json(&path, &scene)?;
        }
    }
    eprintln!(
        "  Scenery event motions: {added} bindings, {} shared GLBs",
        files.len()
    );
    for name in missing {
        eprintln!("  Scenery event source is absent: {name}");
    }
    Ok(())
}
