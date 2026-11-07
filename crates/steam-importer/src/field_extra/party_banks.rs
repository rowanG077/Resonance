//! Native per-character event directories also own script-computed party banks.
use super::*;
use crate::{animation::Animation, model};
use resonance_content::{
    appearance::costume_resource,
    field::document::{
        AnimationMappings, AnimationTarget, SharedAnimationBinding, SharedBankBindings,
    },
};

pub(super) fn cook(
    source: &Source,
    library: &Path,
    models: &ModelMappings,
    resources: &Resources,
    mappings: &mut AnimationMappings,
    bindings: &mut SharedBankBindings,
) -> Result<()> {
    const OWNERS: [&str; 9] = [
        "LLO", "COL", "GEN", "REF", "SHI", "ZEL", "PRE", "REG", "KRA",
    ];
    let image = pe::Image::parse(&source.executable)?;
    let mut rigs = BTreeMap::<String, (model::Rig, Skeleton)>::new();
    let mut added = 0;
    let mut aliases = 0;
    for (index, owner) in OWNERS.into_iter().enumerate() {
        let character = index as u32 + 1;
        let bank = character + 2;
        // The executable registers these dedicated directories independently
        // of the generic NPC bank. Ownership is native, not guessed by rig IDs.
        let registration = 0x5788b6 + (bank - 1) * 28;
        let args = image.slice(registration, 10)?;
        ensure!(
            args[0] == 0xba && args[5] == 0xb9,
            "party event registration changed"
        );
        ensure!(
            image.text(pe::word(args, 6)?)? == format!("{owner}_ALL_NPCPACK"),
            "party event directory owner changed"
        );
        let mut targets = Vec::new();
        for variant in 0..7u8 {
            let resource = costume_resource(character, variant);
            let Some(target) = models.0.get(&resource) else {
                continue;
            };
            let package_name = image.text(pe::word(
                image.slice(0x8cfe80 + ((character - 1) * 7 + u32::from(variant)) * 4, 4)?,
                0,
            )?)?;
            let bytes = source.require(package_name)?;
            let package = Package::parse(&bytes)?;
            let document: document::Model =
                serde_json::from_slice(&fs::read(library.join(models.model_path(resource)?))?)?;
            for (part, scene) in document.parts.iter().enumerate() {
                let descriptor = package
                    .reference(part)?
                    .or(package.reference(0)?)
                    .context("party event target lacks body")?;
                if !rigs.contains_key(descriptor) {
                    rigs.insert(
                        descriptor.to_owned(),
                        (
                            model::Rig::read(&source.require(descriptor)?)?,
                            Skeleton::from_glb(&fs::read(library.join(&scene.mesh))?)?,
                        ),
                    );
                }
                targets.push((
                    target.name().to_owned(),
                    part as u16,
                    descriptor.to_owned(),
                    resource,
                ));
            }
        }
        if targets.is_empty() {
            continue;
        }
        for (&resource, entry) in resources.entries.range(bank << 16..(bank + 1) << 16) {
            if mappings.0.contains_key(&resource) {
                continue;
            }
            let Some(name) = entry else { continue };
            if !name.to_ascii_lowercase().ends_with(".toanmb") || !source.contains(name)? {
                continue;
            }
            let animation = Animation::read(&source.require(name)?)?;
            let mut paths = BTreeMap::<String, String>::new();
            let mut rows = Vec::new();
            for (actor, part, descriptor, model_resource) in &targets {
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
                let path = if let Some(path) = paths.get(descriptor) {
                    path.clone()
                } else {
                    let path = format!(
                        "animations/party/event-{resource:08x}-model-{model_resource:08x}-part-{part}.glb"
                    );
                    output::write(
                        &library.join(&path),
                        &animation.bind(rig, skeleton)?.encode(skeleton)?,
                    )?;
                    paths.insert(descriptor.clone(), path.clone());
                    path
                };
                rows.push(SharedAnimationBinding {
                    actors: vec![actor.clone()],
                    parts: vec![*part],
                    motion: path,
                    joint_map: None,
                    resource_slot: 12,
                    pose_chains: Vec::new(),
                });
            }
            let Some(first) = rows.first() else { continue };
            mappings.0.insert(
                resource,
                AnimationTarget::Shared {
                    name: format!("event-{resource:08x}"),
                    motion: first.motion.clone(),
                },
            );
            bindings.banks.insert(resource, rows);
            added += 1;
        }
        // Native null slots select member zero while retaining the requested
        // gameplay identity. Reuse those exact cooked curves without a bake.
        let fallback = bank << 16;
        let owners = targets
            .iter()
            .map(|(name, _, _, _)| name.as_str())
            .collect::<BTreeSet<_>>();
        let fallback_rows = bindings
            .banks
            .get(&fallback)
            .into_iter()
            .flatten()
            .filter(|row| {
                row.actors
                    .iter()
                    .all(|actor| owners.contains(actor.as_str()))
            })
            .cloned()
            .collect::<Vec<_>>();
        if !fallback_rows.is_empty() {
            for (&resource, entry) in resources.entries.range(bank << 16..(bank + 1) << 16) {
                if entry.is_none() && !mappings.0.contains_key(&resource) {
                    mappings.0.insert(
                        resource,
                        AnimationTarget::Alias {
                            name: format!("event-{resource:08x}"),
                            resource: fallback,
                        },
                    );
                    bindings.banks.insert(resource, fallback_rows.clone());
                    aliases += 1;
                }
            }
        }
    }
    eprintln!(
        "  Dedicated party event directories: {added} additional concrete banks and {aliases} native fallback slots"
    );
    Ok(())
}
