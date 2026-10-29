//! Authored behaviors and definitions generated for cooking and development tools.
//! Runtime reads the user's cooked scripts; this crate is not a runtime fallback.
use std::{collections::BTreeMap, fmt::Write};
use symphonia_constants::*;

include!(concat!(env!("OUT_DIR"), "/sources.rs"));
pub const PREVIEW_BINDINGS: &str = include_str!("../../../scripts/preview/bindings.json");

fn enumeration<T: KnownId>(source: &mut String) {
    writeln!(source, "pub enum {} {{", T::TYPE_NAME).unwrap();
    for &id in T::ALL {
        writeln!(source, "    {} = {},", id.name(), id.value()).unwrap();
    }
    source.push_str("}\n");
}

/// Definitions are generated from the same declarations used by Rust consumers.
pub fn definitions() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    macro_rules! module {
        ($path:literal, $($id:ty),+ $(,)?) => {{
            let mut source = String::from("script library;\n");
            $(enumeration::<$id>(&mut source);)+
            files.insert(concat!("std/", $path, ".sym").into(), source);
        }};
    }
    module!("characters", CharacterId, PartyCharacterId);
    module!("items", ItemId);
    module!("monsters", MonsterId);
    module!("figurines", FigurineId);
    module!("fields", FieldId, FieldSystemCommand);
    module!("music", MusicId);
    module!("battle", BattleEventId);
    module!("locations", WorldLocationId);
    module!("effects", EffectId);
    module!("treasure", TreasureKind);
    module!("story", StoryFlag, GlobalId, StoryStage, QuestStep);
    module!(
        "world",
        WorldId,
        WorldActorId,
        MountId,
        MusicBankId,
        WorldInteraction,
        SylvarantMarkerId,
        TetheAllaMarkerId,
        CaravanStopId
    );
    let mut nodes = String::from("script library;\n");
    for &(symbol, name) in MODEL_NODES {
        writeln!(nodes, "pub const {symbol}: string = {name:?};").unwrap();
    }
    files.insert("std/model_nodes.sym".into(), nodes);
    files
}

/// Paths relative to the cooked `scripts/` directory.
pub fn files() -> BTreeMap<String, String> {
    let mut files = definitions();
    for &(path, source) in AUTHORED_FILES {
        assert!(
            files.insert(path.into(), source.into()).is_none(),
            "authored source shadows generated definition: {path}"
        );
    }
    files
}

pub fn modules() -> BTreeMap<String, String> {
    files()
        .into_iter()
        .filter_map(|(path, source)| {
            path.strip_suffix(".sym")
                .map(|path| (path.replace('/', "::"), source))
        })
        .collect()
}

/// Names come from the final model. Repeated labels use the model service's
/// occurrence selectors; distinct labels that sanitize alike keep distinct symbols.
pub fn node_definitions(names: &[&str]) -> String {
    let mut counts = BTreeMap::new();
    for &name in names {
        *counts.entry(name).or_insert(0) += 1;
    }
    let mut occurrences = BTreeMap::new();
    let mut symbols = std::collections::BTreeSet::new();
    let mut source = String::from("script library;\n");
    for (index, &name) in names.iter().enumerate() {
        let occurrence = occurrences.entry(name).or_insert(0);
        let mut selector = name.replace('#', "##");
        if counts[name] > 1 {
            write!(selector, "#{occurrence}").unwrap();
        }
        *occurrence += 1;
        let mut symbol = String::new();
        for part in name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
        {
            let mut chars = part.chars();
            symbol.extend(chars.next().unwrap().to_uppercase());
            symbol.extend(chars);
        }
        if symbol.is_empty() || symbol.starts_with(|c: char| c.is_ascii_digit()) {
            symbol.insert_str(0, "Node");
        }
        if counts[name] > 1 {
            write!(symbol, "{}", *occurrence - 1).unwrap();
        }
        while !symbols.insert(symbol.clone()) {
            write!(symbol, "_{index}").unwrap();
        }
        writeln!(source, "pub const {symbol}: string = {selector:?};").unwrap();
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_definitions_are_compilable_and_keep_unknown_ids_usable() {
        let mut sources = modules();
        for (path, _) in definitions() {
            let module = path.strip_suffix(".sym").unwrap().replace('/', "::");
            symphonia_script_compiler::check(&module, &sources, &[]).unwrap();
        }
        sources.insert(
            "example".into(),
            r#"script library;
            use std::items::ItemId;
            use std::fields::FieldId;
            use std::locations::WorldLocationId;
            pub fn item() -> i32 { return i32(ItemId::MemoryGem); }
            pub fn field() -> i32 { return i32(FieldId::Iselia0332); }
            pub fn landmark() -> i32 { return i32(WorldLocationId::Iselia); }
            pub fn mod_added() -> i32 { return 60000; }
        "#
            .into(),
        );
        symphonia_script_compiler::check("example", &sources, &[]).unwrap();
    }

    #[test]
    fn model_symbols_handle_duplicates_punctuation_and_escaped_selectors() {
        let source = node_definitions(&[
            "neck",
            "hand",
            "hand",
            "a#b",
            "a-b",
            "a_b",
            "123",
            "quote\"\\",
        ]);
        assert!(source.contains("Neck: string = \"neck\""));
        assert!(source.contains("Hand0: string = \"hand#0\""));
        assert!(source.contains("Hand1: string = \"hand#1\""));
        assert!(source.contains("\"a##b\""));
        symphonia_script_compiler::check("nodes", &BTreeMap::from([("nodes".into(), source)]), &[])
            .unwrap();
    }
}
