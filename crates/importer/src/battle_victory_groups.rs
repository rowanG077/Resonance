//! Native victory speakers and voices with maintained story-selection rules.
use crate::{dol, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_victory::{
        Condition, GROUP_COUNT,
        document::{NamedGroup, NamedSelection},
    },
    battle_voice::Sound,
    library::Mappings,
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::PartyCharacterId;

/// The original table has character IDs, participants and sound IDs. The
/// conditions and selection gates are interpreted gameplay rules, so they are
/// small maintained input rather than invented binary fields.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    condition: Condition,
    #[serde(default)]
    required_leader: Option<String>,
    selection: NamedSelection,
}

fn sound(encoded: u16) -> Result<Option<Sound>> {
    Ok(match encoded {
        0 => None,
        value if value & 0x8000 != 0 => Some(Sound::Stream(value & 0x7fff)),
        value => Some(Sound::Cue(
            value.checked_add(501).context("victory cue overflow")?,
        )),
    })
}

fn convert(root: &Path) -> Result<Vec<NamedGroup>> {
    let recipes: BTreeMap<u8, Recipe> =
        serde_json::from_str(include_str!("battle-victory-groups.json"))?;
    ensure!(
        recipes.keys().copied().eq(1..=GROUP_COUNT),
        "incomplete victory selection recipes"
    );
    let names = Mappings::new(
        PartyCharacterId::RESOURCE_NAMES
            .iter()
            .enumerate()
            .map(|(index, name)| ((index + 1) as u32, (*name).to_owned()))
            .collect(),
    )?;
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let mut decoded = Vec::new();
    let mut groups = Vec::new();
    for (id, recipe) in recipes {
        let row = module.slice(5, 0x5ea8 + usize::from(id - 1) * 8, 8)?;
        let count = usize::from(row[3]);
        ensure!(
            (1..=4).contains(&count),
            "invalid victory participants for {id}"
        );
        let name = |character: u8| -> Result<String> {
            Ok(names.target(u32::from(character))?.to_owned())
        };
        let group = NamedGroup {
            id,
            character: name(row[4])?,
            voice: sound(dol::u16(row, 0)?)?,
            participants: row[4..4 + count]
                .iter()
                .copied()
                .map(name)
                .collect::<Result<_>>()?,
            required_leader: recipe.required_leader,
            condition: recipe.condition,
            selection: recipe.selection,
        };
        let prepared = group.clone().prepare(&names)?;
        ensure!(
            decoded
                .iter()
                .all(|earlier: &resonance_content::battle_victory::Group| {
                    !prepared.selection.conflicts_with(&earlier.selection)
                }),
            "ambiguous victory selection for {id}"
        );
        decoded.push(prepared);
        groups.push(group);
    }
    Ok(groups)
}

pub fn cook_battle_victory_groups(root: &Path, library: &Path) -> Result<()> {
    let groups = convert(root)?;
    let output = library.join("battle/victory/groups.json");
    crate::output::json(&output, &groups)?;
    Ok(())
}
