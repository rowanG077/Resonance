//! Bind authored battle scenes to shared body, animation and dialogue assets.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    ScenePart,
    animation::Skeleton,
    battle_event::{Model, Scene},
};
use std::{collections::BTreeMap, fs, path::Path};

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

pub struct Prepared {
    pub scenes: BTreeMap<u16, Scene>,
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

/// Bind native event scripts and actor poses to the prepared party bodies.
pub fn prepare(
    extracted: &Path,
    bodies: &BTreeMap<(u8, u8), resonance_content::battle_model::Party>,
    output: &Path,
) -> Result<Prepared> {
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
        let body = bodies
            .get(&(character, 0))
            .with_context(|| format!("event requires default party body {character}"))?;
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
                skeleton: body.body.skeleton.clone(),
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
    let declarations = crate::source_assets::Sources::read(extracted)?;
    let usual = fs::read(source.path(&declarations.usual)?)?;
    let enemy = source.path(&declarations.enemy)?;
    let bytes = crate::source_assets::enemy_package(&enemy, &usual, 219)?;
    let boss = crate::monsters::event_model(&bytes, 219, output)?;
    let blink = crate::field_effects::blink(&executable)?;
    let mut scenes = BTreeMap::new();
    for id in [2_u16, 3] {
        let script = fs::read(source.path(&format!("btl_ev{id:03}.so"))?)?;
        let script_dir = format!("assets/{}", crate::digest(&script));
        ensure!(
            crate::all_assets::cook_script(&script, &script_dir, output)?,
            "invalid battle event script {id}"
        );
        let mut actors = party.clone();
        if id == 2 {
            actors.remove(&9);
            actors.insert(0x2005d, soldier.clone());
        }
        actors.insert(17, boss.clone());
        let scene = Scene {
            blink: Some(blink.clone()),
            script: format!("{script_dir}/script.ssb"),
            messages: format!("{script_dir}/messages.json"),
            audio: crate::media::field_audio::event_requirements(extracted, &executable, &script)?,
            actors,
        };
        scenes.insert(id, scene);
    }
    Ok(Prepared {
        scenes,
        inputs: source.inputs,
    })
}
