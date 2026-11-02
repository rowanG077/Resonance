//! Native battle package selections in the same costume mappings as GC.
use crate::{output, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::library::Mappings;
use std::{collections::BTreeMap, path::Path};
use symphonia_constants::PartyCharacterId;

pub(crate) fn costume_name(image: &pe::Image<'_>, character: u8, costume: u8) -> Result<String> {
    let package = source_name(image, character, costume)?;
    for earlier in 0..costume {
        if source_name(image, character, earlier)?.eq_ignore_ascii_case(package) {
            return costume_name(image, character, earlier);
        }
    }
    let names: BTreeMap<u8, BTreeMap<u8, serde_json::Value>> =
        serde_json::from_str(include_str!("../../importer/src/costume-names.json"))?;
    Ok(names
        .get(&character)
        .and_then(|row| row.get(&costume))
        .and_then(|row| row["name"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("costume-{costume}")))
}

pub(crate) fn source_name<'a>(
    image: &pe::Image<'a>,
    character: u8,
    costume: u8,
) -> Result<&'a str> {
    ensure!(
        (1..=9).contains(&character) && costume < 7,
        "invalid battle costume"
    );
    let index = u32::from(character - 1) * 7 + u32::from(costume);
    image.text(pe::word(image.slice(0x9191c0 + index * 4, 4)?, 0)?)
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let mut characters = BTreeMap::new();
    for character in 1..=9u8 {
        let name = PartyCharacterId::try_from(character)?
            .resource_name()
            .context("unnamed battle character")?;
        let mut costumes = BTreeMap::new();
        for costume in 0..7u8 {
            let package = source_name(&image, character, costume)?;
            ensure!(
                package.to_ascii_lowercase().ends_with(".pac") && source.contains(package)?,
                "missing battle costume package {character}:{costume}"
            );
            let target = costume_name(&image, character, costume)?;
            costumes.insert(u32::from(costume), target);
        }
        output::json(
            &library.join(format!("battle/party/{name}/mappings.json")),
            &Mappings::new(costumes)?,
        )?;
        characters.insert(u32::from(character), name.to_owned());
    }
    output::json(
        &library.join("battle/party/mappings.json"),
        &Mappings::new(characters)?,
    )
}
