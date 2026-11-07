//! Encounter-wide owners not contained in actor profiles: victory clips and groups.
//! Item-use rules already live in the ordinary item catalogue and party profiles.
use crate::{
    animation::Animation, battle_party_mappings, battle_voices, model, output, package::Package,
    pe, source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::Skeleton,
    battle_model::document as models,
    battle_victory::{
        self, Condition,
        document::{self, Definition, NamedGroup, NamedSelection, Performance, Timeline},
    },
    library::Mappings,
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::PartyCharacterId;

mod events;
pub(crate) use events::cook as cook_events;

/// The victory command stream has two numeric fields in each twelve-byte row.
/// Expression bytes and pose flags keep their original byte order.
fn timeline(bytes: &[u8], duration: f32) -> Result<Vec<(u16, battle_victory::Event)>> {
    let mut canonical = Vec::new();
    let mut at = 0;
    loop {
        let tick = pe::half(bytes, at)?;
        if tick == (-2i16) as u16 {
            canonical.extend(tick.to_be_bytes());
            break;
        }
        let row = bytes
            .get(at..at + 12)
            .context("truncated Steam victory command")?;
        canonical.extend(tick.to_be_bytes());
        canonical.extend_from_slice(&row[2..8]);
        canonical.extend(pe::word(row, 8)?.to_be_bytes());
        at += 12;
    }
    resonance_importer::decode_victory_timeline(&canonical, duration)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    condition: Condition,
    #[serde(default)]
    required_leader: Option<String>,
    selection: NamedSelection,
}

fn groups(image: &pe::Image<'_>, library: &Path) -> Result<()> {
    ensure!(
        image.slice(0x482229, 7)? == [0x8d, 0x0c, 0xcd, 0x60, 0x3b, 0x8e, 0],
        "victory speech table consumer changed"
    );
    let recipes: BTreeMap<u8, Recipe> = serde_json::from_str(include_str!(
        "../../importer/src/battle-victory-groups.json"
    ))?;
    ensure!(
        recipes.keys().copied().eq(1..=battle_victory::GROUP_COUNT),
        "incomplete victory group recipes"
    );
    let names: Mappings = serde_json::from_slice(&fs::read(library.join(models::PARTY))?)?;
    let mut groups = Vec::new();
    let mut prepared = Vec::<battle_victory::Group>::new();
    for (id, recipe) in recipes {
        let row = image.slice(0x8e3b60 + u32::from(id) * 8, 8)?;
        let count = usize::from(row[3]);
        ensure!(
            (1..=4).contains(&count),
            "invalid victory group participant count"
        );
        let group = NamedGroup {
            id,
            character: names.target(u32::from(row[4]))?.into(),
            voice: battle_voices::sound(u32::from(pe::half(row, 0)?))?,
            participants: row[4..4 + count]
                .iter()
                .map(|&id| Ok(names.target(u32::from(id))?.into()))
                .collect::<Result<_>>()?,
            required_leader: recipe.required_leader,
            condition: recipe.condition,
            selection: recipe.selection,
        };
        let decoded = group.clone().prepare(&names)?;
        ensure!(
            prepared
                .iter()
                .all(|prior| !decoded.selection.conflicts_with(&prior.selection)),
            "ambiguous victory group rules"
        );
        prepared.push(decoded);
        groups.push(group);
    }
    output::json(&library.join(document::GROUPS), &groups)
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    // ((character - 1) * 5 + selector) << 7 selects one inline package.
    for (address, bytes) in [
        (0x46030e, &[0x02, 0xd2, 0x02, 0xd2, 0x02, 0xc2][..]),
        (0x46034e, &[0x68, 0x88, 0xec, 0x86, 0]),
        (0x460367, &[0xc1, 0xe2, 7]),
    ] {
        ensure!(
            image.slice(address, bytes.len())? == bytes,
            "victory package consumer changed at {address:#x}"
        );
    }
    let bytes = source.require(image.text(pe::word(image.slice(0x46034f, 4)?, 0)?)?)?;
    ensure!(
        bytes.len() == 9 * 5 * 128,
        "invalid victory package directory length"
    );
    let mut costumes = 0;
    let mut performances = 0;
    for character in 1..=9u8 {
        let name = PartyCharacterId::try_from(character)?
            .resource_name()
            .context("unnamed victory character")?;
        let mut clips = Vec::new();
        for selector in 0..5u8 {
            let index = usize::from((character - 1) * 5 + selector);
            let package = Package::parse(&bytes[index * 128..(index + 1) * 128])?;
            ensure!(package.len() == 2, "invalid victory package");
            let animation = Animation::read(
                &source.require(package.reference(1)?.context("missing victory motion")?)?,
            )?;
            clips.push((
                animation,
                package
                    .member(0)?
                    .context("missing victory timeline")?
                    .to_vec(),
            ));
        }
        let mut definition: Definition = Definition::default();
        let mut shared = BTreeMap::<String, BTreeMap<String, Performance>>::new();
        for costume in 0..7u8 {
            let package_name = battle_party_mappings::source_name(&image, character, costume)?;
            let variant = battle_party_mappings::costume_name(&image, character, costume)?;
            let key = package_name.to_ascii_lowercase();
            let selections = if let Some(selections) = shared.get(&key) {
                selections.clone()
            } else {
                let bytes = source.require(package_name)?;
                let package = Package::parse(&bytes)?;
                let descriptor = package
                    .reference(0)?
                    .context("missing victory body descriptor")?;
                let rig = model::Rig::read(&source.require(descriptor)?)?;
                let path = format!("battle/party/{name}/{variant}/party.json");
                let party: models::PartyDefinition =
                    serde_json::from_slice(&fs::read(library.join(&path))?)?;
                let skeleton = Skeleton::from_glb(&fs::read(library.join(&party.parts[0].mesh))?)?;
                let mut selections = BTreeMap::new();
                for (selector, (animation, commands)) in clips.iter().enumerate() {
                    let motion = animation.bind(&rig, &skeleton)?;
                    let label = document::selector_name(selector as u8)?;
                    let motion_path = format!("battle/party/{name}/{variant}/victory/{label}.glb");
                    output::write(&library.join(&motion_path), &motion.encode(&skeleton)?)?;
                    let key = format!("celebration-{}", selector + 1);
                    let timeline = Timeline {
                        events: Some(timeline(commands, motion.duration_frames)?),
                        unsupported_reason: None,
                    };
                    if let Some(prior) = definition.timelines.insert(key.clone(), timeline.clone())
                    {
                        ensure!(
                            prior == timeline,
                            "victory timeline changes between costumes"
                        );
                    }
                    selections.insert(
                        label.into(),
                        Performance {
                            motion: motion_path,
                            timeline: key,
                        },
                    );
                }
                shared.insert(key, selections.clone());
                selections
            };
            performances += selections.len();
            costumes += 1;
            if let Some(prior) = definition.costumes.insert(variant, selections.clone()) {
                ensure!(prior == selections, "aliased victory costumes disagree");
            }
        }
        output::json(
            &library.join(format!("battle/party/{name}/victory.json")),
            &definition,
        )?;
    }
    groups(&image, library)?;
    eprintln!(
        "  Battle victories: {costumes} costume selections, {performances} performances and {} groups",
        battle_victory::GROUP_COUNT
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn victory_completion_uses_shared_clip_frames_and_preserves_expression_bytes() -> Result<()> {
        let mut row = vec![0, 0, 23, 2, 0, 0, 8, 255, 0, 0, 0, 0];
        row[8..12].copy_from_slice(&0.5f32.to_le_bytes());
        row.extend([0, 0, 255, 4, 0, 0, 0, 0, 0, 0, 0, 0]);
        row.extend([253, 255, 0, 3, 0, 0, 72, 255, 0, 0, 0, 63]);
        row.extend([254, 255]);
        let events = timeline(&row, 10.)?;
        ensure!(
            matches!(
                events[1],
                (0, battle_victory::Event::Expression([4, 0, 0, 0]))
            ),
            "victory expression changed"
        );
        ensure!(
            matches!(events[2], (23, battle_victory::Event::Idle { blend: 3 })),
            "victory completion clock changed"
        );
        Ok(())
    }
}
