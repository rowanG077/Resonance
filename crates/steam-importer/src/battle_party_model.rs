//! Native battle motion banks bind to the ordinary party bodies and GC documents.
use crate::{
    animation::Animation, battle_party_mappings, model, output, package::Package, pe,
    preview_model, source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip,
    animation::Skeleton,
    appearance::costume_resource,
    battle_model::document::{PartyDefinition, Visuals, take_part_animations},
    battle_victory::document::Selection,
    field::document::{self, Model, ModelMappings},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::PartyCharacterId;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let mappings: ModelMappings =
        serde_json::from_slice(&fs::read(library.join(document::MODELS))?)?;
    let enemy = source.require(image.text(pe::word(image.slice(0x8e5b50 + 242 * 4, 4)?, 0)?)?)?;
    let duel =
        preview_model::reference(&enemy, 0x20 + 60 * 4)?.context("missing Lloyd duel motion")?;
    let wings_path = "meshes/characters/colette/wings/model.json";
    let bytes = source.require("col_wing.pac")?;
    let wings = Package::parse(&bytes)?;
    let (wing, _) = preview_model::part(
        source,
        library,
        wings.reference(0)?.context("missing wing body")?,
        "meshes/characters/colette/wings",
        "body",
        &[(
            12,
            wings
                .reference(2)?
                .context("missing wing motion")?
                .to_owned(),
        )],
    )?;
    output::json(&library.join(wings_path), &Visuals { parts: vec![wing] })?;

    let mut owners = BTreeMap::new();
    let mut rigs = BTreeMap::new();
    let mut motions = BTreeMap::<(usize, String), String>::new();
    for character in 1..=9u8 {
        let name = PartyCharacterId::try_from(character)?
            .resource_name()
            .context("unnamed party")?;
        for costume in 0..7u8 {
            let variant = battle_party_mappings::costume_name(&image, character, costume)?;
            let owner = format!("battle/party/{name}/{variant}");
            let filename = battle_party_mappings::source_name(&image, character, costume)?;
            if let Some(previous) = owners.insert(owner.clone(), filename.to_owned()) {
                ensure!(
                    previous.eq_ignore_ascii_case(filename),
                    "conflicting battle costume {owner}"
                );
                continue;
            }
            eprintln!("  Battle party model: {name}/{variant}");
            let bytes = source.require(filename)?;
            let package = Package::parse(&bytes)?;
            ensure!(
                (3..=258).contains(&package.len()),
                "invalid party battle motion count"
            );
            let descriptor = package.reference(0)?.context("missing battle body")?;
            let index = u32::from(character - 1) * 7 + u32::from(costume);
            let field =
                source.require(image.text(pe::word(image.slice(0x8cfe80 + index * 4, 4)?, 0)?)?)?;
            let field = Package::parse(&field)?;
            // The GC importer also shares field geometry with battle. Reuse only
            // when the native descriptor identifies exactly the same body.
            let (mut parts, rig) = if field
                .reference(0)?
                .is_some_and(|name| name.eq_ignore_ascii_case(descriptor))
                && package.reference(1)?.is_none()
            {
                let path = mappings.model_path(costume_resource(u32::from(character), costume))?;
                let document: Model = serde_json::from_slice(&fs::read(library.join(path))?)?;
                (
                    document.parts,
                    model::Rig::read(&source.require(descriptor)?)?,
                )
            } else {
                let (preview, native) = preview_model::cook(
                    source,
                    library,
                    [descriptor, package.reference(1)?.unwrap_or("")],
                    &owner,
                    &[],
                )?;
                (
                    preview.parts.into_iter().map(|part| part.scene).collect(),
                    native.rig,
                )
            };
            ensure!(parts.len() == 2, "battle body needs its body and outline");
            let sizes = parts[0]
                .textures
                .iter()
                .map(|path| {
                    resonance_asset_writer::texture::dimensions(&fs::read(library.join(path))?)
                })
                .collect::<Result<Vec<_>>>()?;
            parts[0].appearance = Some(model::party_appearance(source, descriptor, &sizes)?);
            let skeleton = Skeleton::from_glb(&fs::read(library.join(&parts[0].mesh))?)?;
            // Identical native rigs may share a motion file across costumes.
            // Different bind poses or channel identities always get their own.
            let key = serde_json::to_vec(&(&rig.nodes, &rig.animation_bindings, &skeleton))?;
            let next = rigs.len();
            let rig_id = *rigs.entry(key).or_insert(next);
            let attachment_nodes = skeleton
                .bones
                .iter()
                .filter_map(|bone| {
                    bone.name
                        .to_ascii_lowercase()
                        .strip_prefix("kk")
                        .map(|suffix| Ok((u8::from_str_radix(suffix, 16)?, bone.name.clone())))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            ensure!(
                !attachment_nodes.is_empty(),
                "missing battle weapon targets"
            );
            let mut selected = BTreeMap::new();
            let mut null_motions = BTreeSet::new();
            for index in 2..package.len() {
                let slot = u16::try_from(index - 2)?;
                if let Some(name) = package.reference(index)? {
                    selected.insert(slot, name.to_owned());
                } else {
                    null_motions.insert(slot);
                }
            }
            if character == 1 {
                ensure!(
                    selected.insert(100, duel.clone()).is_none(),
                    "duel motion slot collision"
                );
                null_motions.remove(&100);
            }
            let mut clips = Vec::new();
            for (slot, name) in selected {
                let key = (rig_id, name.to_ascii_lowercase());
                let path = if let Some(path) = motions.get(&key) {
                    path.clone()
                } else {
                    ensure!(
                        name.to_ascii_lowercase().ends_with(".toanmb"),
                        "unknown battle motion {name}"
                    );
                    let motion = Animation::read(&source.require(&name)?)?.bind(&rig, &skeleton)?;
                    let path = format!(
                        "{owner}/animations/{}.glb",
                        resonance_content::battle_motion::name(slot)
                    );
                    output::write(&library.join(&path), &motion.encode(&skeleton)?)?;
                    motions.insert(key, path.clone());
                    path
                };
                clips.push(SceneClip {
                    motion: path,
                    resource_slot: slot,
                    joints: Default::default(),
                    duration_seconds: 0.,
                    animation_resource: None,
                    pose_chains: Vec::new(),
                });
            }
            for part in &mut parts {
                part.autoplay = false;
                part.clips = clips.clone();
            }
            let animations = take_part_animations(&mut parts);
            output::json(
                &library.join(format!("{owner}/party.json")),
                &PartyDefinition {
                    attachment_nodes,
                    parts,
                    animations,
                    null_motions,
                    wings: (character == 2).then(|| wings_path.into()),
                    victory: Selection {
                        file: format!("battle/party/{name}/victory.json"),
                        costume: variant,
                    },
                },
            )?;
        }
    }
    Ok(())
}
