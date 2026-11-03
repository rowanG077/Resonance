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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires installed Steam and cooked party fixtures; converts only unbound dedicated party banks into a temporary library"]
    fn installed_unbound_party_directories_complete_shared_catalogue() -> Result<()> {
        let installation =
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?;
        let source = Source::open(Path::new(&installation), crate::Language::English)?;
        let fixture =
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?;
        let fixture = Path::new(&fixture);
        let temporary = tempfile::tempdir()?;
        let models: ModelMappings =
            serde_json::from_slice(&fs::read(fixture.join(document::MODELS))?)?;
        let mut copied = BTreeSet::new();
        for character in 1..=9 {
            for variant in 0..7 {
                let path = models.model_path(costume_resource(character, variant))?;
                let document: document::Model =
                    serde_json::from_slice(&fs::read(fixture.join(&path))?)?;
                for path in
                    std::iter::once(path).chain(document.parts.into_iter().map(|part| part.mesh))
                {
                    if copied.insert(path.clone()) {
                        let output = temporary.path().join(&path);
                        fs::create_dir_all(output.parent().unwrap())?;
                        fs::copy(fixture.join(path), output)?;
                    }
                }
            }
        }
        let mut mappings: AnimationMappings =
            serde_json::from_slice(&fs::read(fixture.join(document::ANIMATIONS))?)?;
        let mut bindings: SharedBankBindings =
            serde_json::from_slice(&fs::read(fixture.join("animations/bindings.json"))?)?;
        let resources = Resources::read(&source)?;
        let missing = resources
            .entries
            .iter()
            .filter_map(|(&resource, name)| {
                ((3..=11).contains(&(resource >> 16))
                    && !mappings.0.contains_key(&resource)
                    && name
                        .as_ref()
                        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".toanmb")))
                .then_some(resource)
            })
            .collect::<Vec<_>>();
        cook(
            &source,
            temporary.path(),
            &models,
            &resources,
            &mut mappings,
            &mut bindings,
        )?;
        for resource in &missing {
            let character = (resource >> 16) - 2;
            let owners = (0..7)
                .filter_map(|variant| models.0.get(&costume_resource(character, variant)))
                .map(ModelTarget::name)
                .collect::<BTreeSet<_>>();
            let rows = bindings
                .banks
                .get(resource)
                .context("dedicated party bank remains unavailable")?;
            ensure!(
                !rows.is_empty()
                    && rows.iter().all(|row| row
                        .actors
                        .iter()
                        .all(|actor| owners.contains(actor.as_str()))),
                "dedicated bank bound outside its native party owner"
            );
            for row in rows {
                ensure!(
                    temporary.path().join(&row.motion).is_file(),
                    "dedicated bank has no cooked motion"
                );
            }
        }
        let bytes = serde_json::to_vec(&bindings)?;
        ensure!(
            bytes.len() <= resonance_content::preparation::JSON_LIMIT,
            "completed party catalogue exceeds shared JSON limit"
        );
        eprintln!(
            "  Verified {} additional party banks; shared bindings {} bytes",
            missing.len(),
            bytes.len()
        );
        output::json(&temporary.path().join(document::ANIMATIONS), &mappings)?;
        output::json(
            &temporary.path().join("animations/bindings.json"),
            &bindings,
        )?;
        let library = resonance_content::library::Library::open_paths(
            fixture,
            &[temporary.path().to_owned()],
        )?;
        let fields: resonance_content::library::Mappings =
            serde_json::from_slice(&fs::read(fixture.join(resonance_content::library::FIELDS))?)?;
        let definition: document::Definition = serde_json::from_slice(&fs::read(
            fixture.join(format!("fields/{}/field.json", fields.target(8)?)),
        )?)?;
        let mut assets = definition.assemble(8, |path| Ok(fs::read(fixture.join(path))?.into()))?;
        for resource in [0x30001, 0x30002] {
            let actors = document::Actors::capture_resource(
                &mut library.snapshot(),
                &mut assets,
                resource,
                &|| false,
            )?;
            ensure!(
                actors
                    .iter()
                    .filter(|actor| actor.resource == 1)
                    .flat_map(|actor| &actor.parts)
                    .flat_map(|part| &part.clips)
                    .any(|clip| clip.animation_resource == Some(resource)),
                "shared dynamic request did not bind native bank {resource:#x} to Lloyd"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam; converts a formerly unbound native Lloyd event bank in a temporary library"]
    fn installed_dedicated_party_bank_keeps_its_native_owner() -> Result<()> {
        let installation =
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?;
        let source = Source::open(Path::new(&installation), crate::Language::English)?;
        let temporary = tempfile::tempdir()?;
        let image = pe::Image::parse(&source.executable)?;
        let package = image.text(pe::word(image.slice(0x8cfe80, 4)?, 0)?)?;
        let owner = "characters/lloyd";
        let (model, _) = crate::actor_model::cook(
            &source,
            temporary.path(),
            package,
            &format!("meshes/{owner}"),
        )?;
        output::json(
            &temporary.path().join(format!("meshes/{owner}/model.json")),
            &model,
        )?;
        let models = ModelMappings(BTreeMap::from([(1, ModelTarget::Model(owner.into()))]));
        let resource = 0x30001;
        let resources = Resources::read(&source)?;
        let source_name = resources.loaded(resource)?.to_owned();
        let fallback = 0x30000;
        let alias = 0x30002;
        ensure!(
            resources.entries[&alias].is_none(),
            "native null slot changed"
        );
        let selected = Resources {
            entries: BTreeMap::from([
                (fallback, Some(resources.loaded(fallback)?.to_owned())),
                (resource, Some(source_name)),
                (alias, None),
            ]),
        };
        let mut mappings = AnimationMappings(BTreeMap::new());
        let mut bindings = SharedBankBindings::default();
        cook(
            &source,
            temporary.path(),
            &models,
            &selected,
            &mut mappings,
            &mut bindings,
        )?;
        ensure!(
            mappings.0.contains_key(&resource),
            "native event bank remained unavailable"
        );
        ensure!(
            mappings.resource(alias)? == fallback,
            "native null slot lost member-zero fallback"
        );
        ensure!(
            bindings.banks[&alias]
                .iter()
                .map(|row| &row.motion)
                .eq(bindings.banks[&fallback].iter().map(|row| &row.motion)),
            "native fallback synthesized different curves"
        );
        let rows = &bindings.banks[&resource];
        ensure!(
            rows.len() == model.parts.len(),
            "event bank lost a body or outline binding"
        );
        for row in rows {
            ensure!(
                row.actors == [owner] && row.resource_slot == 12,
                "dedicated event bank changed its native owner or slot"
            );
            let part = &model.parts[usize::from(row.parts[0])];
            let skeleton = Skeleton::from_glb(&fs::read(temporary.path().join(&part.mesh))?)?;
            let clip = resonance_content::SceneClip {
                motion: row.motion.clone(),
                joints: BTreeMap::new(),
                resource_slot: 12,
                duration_seconds: 0.,
                animation_resource: Some(resource),
                pose_chains: Vec::new(),
            };
            let motion = clip
                .decode_for_skeleton(&fs::read(temporary.path().join(&row.motion))?, &skeleton)?;
            motion.validate(&skeleton)?;
            ensure!(
                motion.duration_frames > 0.,
                "native event bank became an empty substitute"
            );
        }
        Ok(())
    }
}
