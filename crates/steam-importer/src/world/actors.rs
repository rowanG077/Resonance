//! Travel actors reuse the character owner and ordinary model/motion writers.
use super::*;
use crate::{animation::Animation, model};
use resonance_content::{
    SceneClip,
    animation::Skeleton,
    appearance::costume_resource,
    field::document::{Model as FieldModel, ModelMappings},
};
use symphonia_constants::WorldActorId;

fn actor_name(id: u16) -> Result<String> {
    Ok(WorldActorId::try_from(id)?
        .resource_name()
        .context("unnamed world actor")?
        .into())
}
fn clip(motion: String, slot: u16) -> SceneClip {
    SceneClip {
        motion,
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: None,
        joints: Default::default(),
        pose_chains: Vec::new(),
    }
}

pub(super) fn cook(source: &Source, library: &Path, image: &pe::Image<'_>) -> Result<()> {
    let bytes = source.require(image.text(0x86ff64)?)?;
    let common = Package::parse(&bytes)?;
    ensure!(common.len() == 21, "unexpected world actor directory");
    let bytes = source.require(common.reference(20)?.context("missing mount motion bank")?)?;
    let mounts = Package::parse(&bytes)?;
    ensure!(mounts.len() == 18, "unexpected mount motion directory");
    let indices = [0x4ae5c8, 0x4ae5ca, 0x4ae5d0, 0x4ae5d2]
        .map(|address| {
            pe::half(image.slice(address, 2)?, 0)?
                .checked_sub(1)
                .map(usize::from)
                .context("invalid native world motion slot")
        })
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let mut mappings = BTreeMap::new();
    let models: ModelMappings = serde_json::from_slice(&fs::read(
        library.join(resonance_content::field::document::MODELS),
    )?)?;
    for id in 1..=9u16 {
        let name = actor_name(id)?;
        let owner = format!("world/actors/{name}");
        let path = models.model_path(costume_resource(u32::from(id), 0))?;
        let mut model: FieldModel = serde_json::from_slice(&fs::read(library.join(&path))?)?;
        let native = source.require(image.text(pe::word(
            image.slice(0x8cfe80 + u32::from(id - 1) * 7 * 4, 4)?,
            0,
        )?)?)?;
        let native = Package::parse(&native)?;
        let native = model::Rig::read(
            &source.require(
                native
                    .reference(0)?
                    .context("party world actor has no body")?,
            )?,
        )?;
        let skeleton = Skeleton::from_glb(&fs::read(library.join(&model.parts[0].mesh))?)?;
        let bytes = source.require(image.text(0x870990 + u32::from(id - 1) * 12)?)?;
        let bank = Package::parse(&bytes)?;
        let mut selected = Vec::new();
        for (slot, &index) in indices.iter().enumerate() {
            if index < bank.len() && bank.reference(index)?.is_some() {
                selected.push((slot as u16, u16::try_from(4 + index * 4)?));
            }
        }
        let mut mount_clips = Vec::new();
        for slot in 0..2 {
            let name = mounts
                .reference((usize::from(id) - 1) * 2 + slot)?
                .context("missing party mount motion")?;
            let animation = Animation::read(&source.require(name)?)?
                .bind(&native, &skeleton)?
                .encode(&skeleton)?;
            let path = format!("{owner}/animations/motion-{}.glb", slot + 4);
            output::write(&library.join(&path), &animation)?;
            mount_clips.push(clip(path, (slot + 4) as u16));
        }
        for part in &mut model.parts {
            let shared = std::mem::take(&mut part.clips);
            for &(slot, shared_slot) in &selected {
                let mut clip = shared
                    .iter()
                    .find(|clip| {
                        clip.resource_slot == shared_slot && clip.animation_resource.is_none()
                    })
                    .with_context(|| {
                        format!("party world actor {id} lacks shared motion {shared_slot}")
                    })?
                    .clone();
                clip.resource_slot = slot;
                part.clips.push(clip);
            }
            part.clips.extend(mount_clips.iter().cloned());
            part.autoplay = false;
        }
        output::json(
            &library.join(&owner).join("model.json"),
            &document::Model { parts: model.parts },
        )?;
        mappings.insert(u32::from(id), name);
    }
    for (slot, id) in [
        (0, 100),
        (1, 101),
        (2, 200),
        (3, 202),
        (4, 201),
        (5, 99),
        (6, 203),
        (7, 204),
        (8, 205),
        (9, 206),
        (10, 207),
        (11, 208),
        (12, 209),
        (13, 210),
        (14, 211),
        (15, 212),
        (16, 213),
        (17, 214),
        (18, 215),
        (19, 216),
    ] {
        let name = actor_name(id)?;
        let owner = format!("world/actors/{name}");
        let resource = common
            .reference(slot)?
            .context("missing world actor package")?;
        let bytes = source.require(resource)?;
        let package = Package::parse(&bytes)?;
        let motions = indices
            .iter()
            .enumerate()
            .filter_map(|(slot, &index)| {
                if index >= package.len() {
                    return None;
                }
                match package.reference(index) {
                    Ok(Some(name)) => Some(Ok((slot as u16, name.to_owned()))),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let body = package.reference(0)?.context("world actor has no body")?;
        let mut parts = if matches!(slot, 0 | 1 | 3) {
            preview_model::cook(
                source,
                library,
                [body, package.reference(1)?.unwrap_or("")],
                &owner,
                &motions,
            )?
            .0
            .parts
            .into_iter()
            .map(|part| part.scene)
            .collect::<Vec<_>>()
        } else {
            vec![preview_model::part(source, library, body, &owner, "body", &motions)?.0]
        };
        for part in &mut parts {
            part.autoplay = !part.clips.is_empty();
        }
        output::json(
            &library.join(&owner).join("model.json"),
            &document::Model { parts },
        )?;
        mappings.insert(u32::from(id), name);
    }
    output::json(&library.join(document::ACTORS), &Mappings::new(mappings)?)?;
    eprintln!("  World actors: nine party bodies and twenty travel/symbol owners");
    Ok(())
}
