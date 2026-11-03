//! Reuse exact model-bound field curves in the shared event-bank catalogue.
use super::*;
use resonance_content::field::document::{
    Actors, AnimationMappings, AnimationTarget, SharedAnimationBinding, SharedBankBindings,
};

pub(super) fn cook(source: &Source, library: &Path, models: &ModelMappings) -> Result<()> {
    let resources = Resources::read(source)?;
    let mut bindings = SharedBankBindings::default();
    let mut mappings = AnimationMappings(BTreeMap::new());
    // A local numeric ID can name different models in different fields. Only
    // globally owned actors are safe to promote into reusable bank bindings.
    let global_names = models
        .0
        .iter()
        .filter(|&(&id, _)| {
            (1..=9).contains(&id)
                || (1..=2).contains(&(id >> 16))
                || id >> 24 == 0x23
                || id == COLETTE_WINGS_RESOURCE
        })
        .map(|(_, model)| model.name())
        .collect::<BTreeSet<_>>();
    let fields: resonance_content::library::Mappings =
        serde_json::from_slice(&fs::read(library.join(resonance_content::library::FIELDS))?)?;
    let mut seen = BTreeSet::new();
    for (_, name) in fields.iter() {
        let actors: Actors = serde_json::from_slice(&fs::read(
            library.join(format!("fields/{name}/actors.json")),
        )?)?;
        for (resource, rows) in actors.local_animations {
            let Ok(source_name) = resources.loaded(resource) else {
                continue;
            };
            if !source_name.to_ascii_lowercase().ends_with(".toanmb") {
                continue;
            }
            for row in rows {
                if !row
                    .actors
                    .iter()
                    .all(|actor| global_names.contains(actor.as_str()))
                {
                    continue;
                }
                // The original actor pass already caches by native descriptor
                // and motion. Repeated fields therefore share identical paths.
                let key = serde_json::to_vec(&(
                    resource,
                    &row.actors,
                    &row.parts,
                    &row.motion,
                    &row.joints,
                    row.resource_slot,
                    &row.pose_chains,
                ))?;
                if !seen.insert(key) {
                    continue;
                }
                let joint_map = if row.joints.is_empty() {
                    None
                } else {
                    let name = format!("map-{}", bindings.joint_maps.len());
                    bindings.joint_maps.insert(name.clone(), row.joints);
                    Some(name)
                };
                mappings
                    .0
                    .entry(resource)
                    .or_insert_with(|| AnimationTarget::Shared {
                        name: format!("event-{resource:08x}"),
                        motion: row.motion.clone(),
                    });
                bindings
                    .banks
                    .entry(resource)
                    .or_default()
                    .push(SharedAnimationBinding {
                        actors: row.actors,
                        parts: row.parts,
                        motion: row.motion,
                        joint_map,
                        resource_slot: row.resource_slot,
                        pose_chains: row.pose_chains,
                    });
            }
        }
    }
    let available = resources
        .entries
        .iter()
        .filter(|(_, name)| {
            name.as_ref()
                .is_some_and(|name| name.to_ascii_lowercase().ends_with(".toanmb"))
        })
        .count();
    eprintln!(
        "  Shared field animations: {} banks and {} exact actor/part bindings ({available} source banks)",
        mappings.0.len(),
        bindings.banks.values().map(Vec::len).sum::<usize>()
    );
    if mappings.0.len() < available {
        eprintln!(
            "  Global banks without a known concrete field target remain unbound; no neutral or synthetic motion is substituted"
        );
    }
    output::json(&library.join(document::ANIMATIONS), &mappings)?;
    output::json(&library.join("animations/bindings.json"), &bindings)?;
    cameras(source, library, &resources)
}

fn cameras(source: &Source, library: &Path, resources: &Resources) -> Result<()> {
    let mut mappings = BTreeMap::new();
    let mut owners = BTreeMap::<String, String>::new();
    for (&resource, entry) in &resources.entries {
        let Some(name) = entry else { continue };
        if !name.to_ascii_lowercase().ends_with(".cam") {
            continue;
        }
        if !source.contains(name)? {
            eprintln!("  Shared camera source is absent: {resource:#x} ({name})");
            continue;
        }
        let path = if let Some(path) = owners.get(&name.to_ascii_lowercase()) {
            path.clone()
        } else {
            let path = format!("cameras/event-{resource:08x}.glb");
            let bytes = crate::field_scene::camera(&source.require(name)?)?;
            output::write(&library.join(&path), &bytes)?;
            owners.insert(name.to_ascii_lowercase(), path.clone());
            path
        };
        mappings.insert(resource, path);
    }
    eprintln!(
        "  Shared field cameras: {} resource bindings",
        mappings.len()
    );
    output::json(&library.join("cameras/mappings.json"), &mappings)
}
