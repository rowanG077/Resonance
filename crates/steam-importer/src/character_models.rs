//! Party bodies and field/service motions use the ordinary shared model documents.
use crate::{actor_model, animation::Animation, output, package::Package, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip,
    animation::Skeleton,
    appearance::costume_resource,
    field::{
        FIELD_SERVICE_MOTION_RESOURCE_BASE, ServiceMotion,
        document::{self, ModelAlias, ModelMappings, ModelTarget},
    },
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::PartyCharacterId;

pub(crate) fn cook(source: &Source, library: &Path, selected: Option<u8>) -> Result<()> {
    if let Some(id) = selected {
        ensure!((1..=9).contains(&id), "invalid party character {id}");
    }
    let image = pe::Image::parse(&source.executable)?;
    let common = resonance_importer::party_model_mappings()?;
    let mut mappings = BTreeMap::new();
    let mut owners = BTreeMap::<String, String>::new();
    for id in 1..=9u8 {
        if selected.is_some_and(|only| only != id) {
            continue;
        }
        let name = PartyCharacterId::try_from(id)?
            .resource_name()
            .context("unnamed party character")?;
        let motions = bank(source, image.text(0x870990 + u32::from(id - 1) * 12)?)?;
        let services = bank(source, image.text(0x870df8 + u32::from(id - 1) * 16)?)?;
        for variant in 0..7u8 {
            let resource = costume_resource(u32::from(id), variant);
            let target = common
                .get(&resource)
                .map(|t| t.name().to_owned())
                .unwrap_or_else(|| format!("characters/{name}/costume-{variant}"));
            let at = 0x8cfe80 + (u32::from(id - 1) * 7 + u32::from(variant)) * 4;
            let package_name = image.text(pe::word(image.slice(at, 4)?, 0)?)?;
            if let Some(owner) = owners.get(&package_name.to_ascii_lowercase()) {
                mappings.insert(
                    resource,
                    ModelTarget::Alias(ModelAlias {
                        name: target,
                        model: owner.clone(),
                    }),
                );
                continue;
            }
            eprintln!("  Party model: {target}");
            let base = format!("meshes/{target}");
            let (mut document, native) = actor_model::cook(source, library, package_name, &base)?;
            let body = &mut document.parts[0];
            let skeleton = Skeleton::from_glb(&fs::read(library.join(&body.mesh))?)?;
            for (&slot, animation) in motions.range(12..) {
                let path = format!("{base}/animations/{}.glb", motion_name(slot, false));
                output::write(
                    &library.join(&path),
                    &animation.bind(&native.rig, &skeleton)?.encode(&skeleton)?,
                )?;
                body.clips.push(clip(path, slot, None));
            }
            for service in ServiceMotion::ALL {
                let slot = service as u16;
                let Some(animation) = services.get(&slot) else {
                    continue;
                };
                let path = format!("{base}/animations/{}.glb", motion_name(slot, true));
                output::write(
                    &library.join(&path),
                    &animation.bind(&native.rig, &skeleton)?.encode(&skeleton)?,
                )?;
                body.clips.push(clip(
                    path,
                    slot,
                    Some(FIELD_SERVICE_MOTION_RESOURCE_BASE + u32::from(id)),
                ));
            }
            let clips = document.parts[0].clips.clone();
            for part in document.parts.iter_mut().skip(1) {
                part.clips.extend(
                    clips
                        .iter()
                        .filter(|clip| {
                            !part.clips.iter().any(|c| {
                                c.resource_slot == clip.resource_slot
                                    && c.animation_resource == clip.animation_resource
                            })
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                );
            }
            output::json(&library.join(&base).join("model.json"), &document)?;
            owners.insert(package_name.to_ascii_lowercase(), target.clone());
            mappings.insert(resource, ModelTarget::Model(target));
        }
    }
    let mappings = ModelMappings(mappings);
    mappings.by_name()?;
    output::json(&library.join(document::MODELS), &mappings)
}

fn bank(source: &Source, name: &str) -> Result<BTreeMap<u16, Animation>> {
    let bytes = source.require(name)?;
    let package = Package::parse(&bytes)?;
    let mut motions = BTreeMap::new();
    for slot in 2..package.len() {
        if let Some(name) = package.reference(slot)? {
            motions.insert(
                (4 + slot * 4).try_into()?,
                Animation::read(&source.require(name)?)
                    .with_context(|| format!("party motion {name}"))?,
            );
        }
    }
    Ok(motions)
}

fn clip(motion: String, resource_slot: u16, animation_resource: Option<u32>) -> SceneClip {
    SceneClip {
        motion,
        resource_slot,
        animation_resource,
        joints: Default::default(),
        duration_seconds: 0.,
        pose_chains: Vec::new(),
    }
}
fn motion_name(slot: u16, service: bool) -> String {
    match (slot, service) {
        (12, false) => "idle".into(),
        (36, false) => "walk".into(),
        (40, false) => "run".into(),
        (20, true) => "field-service-open-door".into(),
        (24, true) => "field-service-pull-door".into(),
        (32, true) => "field-service-hold-block".into(),
        (36, true) => "field-service-push-block".into(),
        (40, true) => "field-service-pull-block".into(),
        (52, true) => "field-service-cast-ring".into(),
        _ => format!(
            "{}motion-{slot}",
            if service { "field-service-" } else { "" }
        ),
    }
}
