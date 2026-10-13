//! Bind authored battle scenes to shared body, animation and dialogue assets.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    ScenePart,
    animation::Skeleton,
    battle_event::{Model, Scene},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, fs, path::Path};

fn read<T: serde::de::DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(root.join(path))?)?)
}

fn parts(
    output: &Path,
    bytes: &[u8],
    clips: &[crate::character::Clip<'_>],
) -> Result<Vec<ScenePart>> {
    let layers = crate::field::sections(bytes)?;
    let primary = &bytes[layers
        .first()
        .and_then(Option::as_ref)
        .context("missing actor body")?
        .clone()];
    let mut decoded = crate::scene::decoded::Package::default();
    for layer in layers.iter().take(2).flatten() {
        decoded.decode_model(&bytes[layer.clone()], primary, output)?;
    }
    crate::character::cook_source_parts(output, "battle event", bytes, clips, &decoded)
}

fn inventory(scene: &Scene, base: &Path, output: &Path) -> Result<BTreeMap<String, File>> {
    let mut paths = BTreeMap::<String, Role>::new();
    paths.insert(scene.script.clone(), Role::Script);
    paths.insert(scene.messages.clone(), Role::Data);
    paths.insert("effects/field.json".into(), Role::Data);
    paths.insert("data/embedded/field-effects.json".into(), Role::Data);
    for part in scene.actors.values().flat_map(|model| &model.parts) {
        paths.insert(part.mesh.clone(), Role::Mesh);
        paths.extend(
            part.textures
                .iter()
                .map(|path| (path.clone(), Role::Texture)),
        );
        paths.extend(
            part.clips
                .iter()
                .map(|clip| (clip.motion.clone(), Role::Data)),
        );
    }
    let dialogue: resonance_content::font::DialogueArt = read(base, "ui/dialogue.json")?;
    let font: resonance_content::font::BitmapFont = read(base, &dialogue.font)?;
    paths.insert("ui/dialogue.json".into(), Role::Data);
    paths.insert(dialogue.font, Role::Data);
    paths.insert(font.texture, Role::Texture);
    paths.extend(
        dialogue
            .textures
            .iter()
            .chain([&dialogue.cursor])
            .map(|texture| (texture.path.clone(), Role::Texture)),
    );
    paths
        .into_iter()
        .map(|(path, role)| {
            let fresh = output.join(&path);
            let actual = if fresh.is_file() {
                fresh
            } else {
                base.join(&path)
            };
            Ok((
                path,
                File {
                    sha256: crate::media::hash_file(&actual)?,
                    bytes: fs::metadata(actual)?.len(),
                    roles: [role].into(),
                },
            ))
        })
        .collect()
}

pub struct Publication {
    pub paths: std::collections::BTreeSet<String>,
    pub inputs: std::collections::BTreeSet<String>,
}

struct Source<'a> {
    extracted: &'a Path,
    catalogue: crate::resource::Catalogue,
    inputs: std::collections::BTreeSet<String>,
}

impl Source<'_> {
    fn path(&mut self, declared: &str) -> Result<std::path::PathBuf> {
        let name = crate::field_resources::resolve_path(&self.extracted.join("files"), declared)?;
        self.inputs.insert(format!("files/{name}"));
        Ok(self.extracted.join("files").join(name))
    }

    fn resource(&mut self, id: u32) -> Result<Vec<u8>> {
        let declared = self.catalogue.source(id)?.to_owned();
        let path = self.path(&declared)?;
        let file = fs::File::open(path)?;
        crate::field_resources::binding::read_member(&file, file.metadata()?.len(), id)
    }
}

/// The finishing pass runs after ordinary shared body, script and UI publication.
/// Only these scene-specific bindings are made here; art and curves share their
/// existing content-addressed publications.
pub fn publish(extracted: &Path, library: &Path, output: &Path) -> Result<Publication> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let catalogue = crate::resource::read(&executable)?;
    let mut source = Source {
        extracted,
        catalogue,
        inputs: ["sys/main.dol".to_owned()].into(),
    };
    let mut party = BTreeMap::new();
    for character in [1_u8, 2, 3, 9] {
        let declared = source
            .catalogue
            .party(crate::resource::PartyResource::Body, character, 0)?
            .to_owned();
        let bytes = crate::compression::payload(fs::read(source.path(&declared)?)?)?;
        let body: resonance_content::battle_model::Party = read(
            library,
            &resonance_content::battle_model::party_path(character),
        )?;
        let resources: &[u32] = match character {
            1 => &[0x3008e, 0x300a8],
            2 => &[0x4008e, 0x400a8],
            3 => &[0x5008e, 0x500a8],
            9 => &[0xb008e],
            _ => unreachable!(),
        };
        let motions = resources
            .iter()
            .map(|&resource| {
                Ok((
                    resource,
                    crate::animation::read_member(&source.resource(resource)?)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let clips = motions
            .iter()
            .map(|(resource, animation)| crate::character::Clip {
                slot: 0,
                resource: Some(*resource),
                animation,
            })
            .collect::<Vec<_>>();
        let mut new_parts = parts(output, &bytes, &clips)?;
        ensure!(
            new_parts.len() == body.parts.len(),
            "event party layers changed"
        );
        for (new, existing) in new_parts.iter_mut().zip(&body.parts) {
            ensure!(
                new.mesh == existing.mesh && new.bone_names == existing.bone_names,
                "event party mesh differs"
            );
            new.clips.extend(existing.clips.clone());
        }
        party.insert(
            u32::from(character),
            Model {
                skeleton: body.body.skeleton,
                parts: new_parts,
            },
        );
    }
    let soldier_bytes = source.resource(0x2005d)?;
    let external = crate::animation::read_member(&source.resource(0x10aa0)?)?;
    let sections = crate::field::sections(&soldier_bytes)?;
    let own = sections
        .iter()
        .enumerate()
        .skip(2)
        .filter_map(|(slot, range)| range.as_ref().map(|range| (slot, range)))
        .filter(|(slot, range)| {
            !crate::character::collision_slot(*slot, &soldier_bytes[(*range).clone()])
        })
        .map(|(slot, range)| {
            Ok((
                (slot - 2) as u16,
                crate::animation::read_member(&soldier_bytes[range.clone()])?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut clips = own
        .iter()
        .map(|(slot, animation)| crate::character::Clip {
            slot: *slot,
            resource: None,
            animation,
        })
        .collect::<Vec<_>>();
    clips.push(crate::character::Clip {
        slot: 0,
        resource: Some(0x10aa0),
        animation: &external,
    });
    let soldier_parts = parts(output, &soldier_bytes, &clips)?;
    let soldier = Model {
        skeleton: Skeleton::from_glb(&fs::read(output.join(&soldier_parts[0].mesh))?)?,
        parts: soldier_parts,
    };
    let enemy: resonance_content::battle_model::Enemy = read(library, "battle/enemies/219.json")?;
    let monster: resonance_content::monster::Monster = read(library, "monsters/219.json")?;
    let boss = Model {
        skeleton: enemy.body.skeleton,
        parts: monster
            .preview
            .parts
            .into_iter()
            .filter(|part| part.attached_to.is_none())
            .map(|part| part.scene)
            .collect(),
    };
    let mut paths = std::collections::BTreeSet::new();
    for id in [2_u16, 3] {
        let script = fs::read(source.path(&format!("btl_ev{id:03}.so"))?)?;
        let script_dir = format!("assets/{}", crate::digest(&script));
        let mut actors = party.clone();
        if id == 2 {
            actors.remove(&9);
            actors.insert(0x2005d, soldier.clone());
        }
        actors.insert(17, boss.clone());
        let mut scene = Scene {
            script: format!("{script_dir}/script.ssb"),
            messages: format!("{script_dir}/messages.json"),
            audio: crate::media::field_audio::event_requirements(extracted, &executable, &script)?,
            actors,
            files: BTreeMap::new(),
        };
        scene.files = inventory(&scene, library, output)?;
        let path = resonance_content::battle_event::path(id);
        crate::write_atomic(&output.join(&path), &serde_json::to_vec(&scene)?)?;
        paths.insert(path);
    }
    Ok(Publication {
        paths,
        inputs: source.inputs,
    })
}
