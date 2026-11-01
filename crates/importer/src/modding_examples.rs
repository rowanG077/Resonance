//! Add repository-authored examples while generating native game documents.
use anyhow::{Context, Result, ensure};
use resonance_content::{arte, characters, library::Mappings};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Example {
    id: u16,
    name: String,
    character: String,
    initially_known: bool,
    technique: arte::document::Entry,
}

fn example() -> Result<Example> {
    serde_json::from_str(include_str!("../../../mods/ghost-wolf/example.json"))
        .context("Ghost Wolf modding example")
}

pub(crate) fn add_techniques(document: &mut arte::document::Document) -> Result<()> {
    let example = example()?;
    document.characters.id(&example.character)?;
    let mut mappings: BTreeMap<_, _> = document
        .mappings
        .iter()
        .map(|(id, name)| (id, name.to_owned()))
        .collect();
    ensure!(
        !mappings.contains_key(&u32::from(example.id)),
        "modding example catalogue ID {} is already used",
        example.id
    );
    ensure!(
        !mappings.values().any(|name| name == &example.name)
            && !document.techniques.contains_key(&example.name),
        "modding example {} already exists in the imported catalogue",
        example.name
    );
    let learning = document
        .learning
        .get_mut(&example.character)
        .context("modding example character has no learning list")?;
    mappings.insert(u32::from(example.id), example.name.clone());
    let mappings = Mappings::new(mappings)?;
    learning.push(example.name.clone());
    document.mappings = mappings;
    document.techniques.insert(example.name, example.technique);
    Ok(())
}

pub(crate) fn add_characters(document: &mut characters::Document) -> Result<()> {
    let example = example()?;
    if !example.initially_known {
        return Ok(());
    }
    let initial = document
        .characters
        .get_mut(&example.character)
        .context("modding example character is missing")?
        .initial
        .as_mut()
        .context("modding example character has no initial state")?;
    initial.techniques.push(example.name);
    Ok(())
}

pub(crate) fn cook_assets(library: &Path) -> Result<()> {
    for (path, bytes) in ASSETS {
        crate::output::write(&library.join(path), bytes)?;
    }
    Ok(())
}

const ASSETS: &[(&str, &[u8])] = &[
    (
        "battle/artes/ghost-wolf/action.json",
        include_bytes!("../../../mods/ghost-wolf/battle/artes/ghost-wolf/action.json"),
    ),
    (
        "battle/artes/ghost-wolf/effects.json",
        include_bytes!("../../../mods/ghost-wolf/battle/artes/ghost-wolf/effects.json"),
    ),
    (
        "battle/artes/ghost-wolf/ghost-wolf.glb",
        include_bytes!("../../../mods/ghost-wolf/battle/artes/ghost-wolf/ghost-wolf.glb"),
    ),
    (
        "battle/artes/ghost-wolf/symphonia-wind.ktx2",
        include_bytes!("../../../mods/ghost-wolf/battle/artes/ghost-wolf/symphonia-wind.ktx2"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn native_techniques() -> arte::document::Document {
        serde_json::from_value(json!({
            "mappings": {"1": "native-arte"},
            "characters": {"1": "lloyd", "2": "colette"},
            "techniques": {
                "native-arte": {"rules": {"tp_cost": 7}}
            },
            "learning": {"lloyd": ["native-arte"], "colette": ["native-arte"]}
        }))
        .unwrap()
    }

    fn native_characters() -> characters::Document {
        serde_json::from_value(json!({
            "mappings": {"1": "lloyd", "2": "colette"},
            "characters": {
                "lloyd": {"name": "Lloyd", "initial": {
                    "stats": {"hp": 200}, "techniques": ["native-arte"]}},
                "colette": {"name": "Colette", "initial": {
                    "stats": {"hp": 172}, "techniques": ["native-arte"]}}
            },
            "experience": {"1": 0, "2": 25}
        }))
        .unwrap()
    }

    #[test]
    fn adds_example_to_generated_native_documents() -> Result<()> {
        let mut techniques = native_techniques();
        let mut characters = native_characters();
        let before_techniques = serde_json::to_value(&techniques)?;
        let before_characters = serde_json::to_value(&characters)?;
        add_techniques(&mut techniques)?;
        add_characters(&mut characters)?;
        let mut techniques = serde_json::to_value(techniques)?;
        assert_eq!(techniques["mappings"]["260"], "ghost-wolf-lloyd");
        assert_eq!(
            techniques["techniques"]["ghost-wolf-lloyd"]["rules"]["action"],
            "battle/artes/ghost-wolf/action.json"
        );
        assert_eq!(
            techniques["techniques"]["ghost-wolf-lloyd"]["rules"]["required_level"],
            6
        );
        assert_eq!(
            techniques["techniques"]["ghost-wolf-lloyd"]["rules"]["learn_on_level_up"],
            true
        );
        techniques["mappings"]
            .as_object_mut()
            .unwrap()
            .remove("260");
        techniques["techniques"]
            .as_object_mut()
            .unwrap()
            .remove("ghost-wolf-lloyd");
        assert_eq!(
            techniques["learning"]["lloyd"]
                .as_array_mut()
                .unwrap()
                .pop(),
            Some(json!("ghost-wolf-lloyd"))
        );
        assert_eq!(techniques, before_techniques);
        assert_eq!(serde_json::to_value(characters)?, before_characters);
        Ok(())
    }

    #[test]
    fn rejects_collisions_with_imported_techniques() -> Result<()> {
        for mappings in [
            json!({"1": "native-arte", "260": "future-native-arte"}),
            json!({"1": "native-arte", "261": "ghost-wolf-lloyd"}),
            json!({"1": "native-arte", "260": "ghost-wolf-lloyd"}),
        ] {
            let mut techniques = serde_json::to_value(native_techniques())?;
            techniques["mappings"] = mappings;
            let mut techniques: arte::document::Document = serde_json::from_value(techniques)?;
            let before = serde_json::to_value(&techniques)?;
            assert!(add_techniques(&mut techniques).is_err());
            assert_eq!(serde_json::to_value(&techniques)?, before);
        }
        Ok(())
    }
}
