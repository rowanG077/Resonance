//! Maintained publication names. Original IDs retain identity; display text,
//! traversal order and byte equality never assign or rename an editable resource.
use anyhow::{Result, ensure};
use resonance_content::library::Mappings;
use std::collections::{BTreeMap, BTreeSet};

/// Source-only resources have no translated catalogue label. Preserve their
/// original directory and filename so a person can trace them without a digest.
pub(crate) fn physical_source(source: &str) -> Result<String> {
    resonance_content::library::validate_path(source)?;
    ensure!(
        source
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_.-".contains(&byte)),
        "source filename needs a maintained portable name: {source}"
    );
    Ok(format!(
        "resources/physical/{}",
        source.to_ascii_lowercase()
    ))
}

/// Pinned GQSEAF revision-0 catalogue names. These are maintained importer data,
/// not names regenerated from a user's current language or edited captions.
pub fn catalogue_model(domain: &str, id: u32) -> Result<String> {
    let source = match domain {
        "monsters" => include_str!("naming/monsters.json"),
        "figurines" => include_str!("naming/figurines.json"),
        _ => anyhow::bail!("unknown model catalogue {domain}"),
    };
    let names: Mappings = serde_json::from_str(source)?;
    Ok(names
        .target(id)
        .map(str::to_owned)
        .unwrap_or_else(|_| format!("model-{id:04}")))
}

/// Maintained original item symbols name equipment; battle-only carried props
/// retain their distinct native IDs. Edited menu text never renames these owners.
pub fn equipment(id: u16) -> Result<String> {
    let names: Mappings = serde_json::from_str(include_str!("naming/equipment.json"))?;
    Ok(names
        .target(u32::from(id))
        .map(str::to_owned)
        .unwrap_or_else(|_| format!("equipment-{id:04}")))
}

/// Pinned original labels joined through native Unison placement IDs. Skill
/// catalogue IDs have a separate table; current menu captions never rename files.
pub fn battle_scene(
    kind: resonance_content::battle_scene::document::Kind,
    id: u16,
) -> Result<String> {
    use resonance_content::battle_scene::document::Kind;
    let names: Mappings = serde_json::from_str(match kind {
        Kind::Technique => include_str!("naming/battle-scenes.json"),
        Kind::Skill => include_str!("naming/battle-skills.json"),
    })?;
    Ok(names
        .target(u32::from(id))
        .map(str::to_owned)
        .unwrap_or_else(|_| format!("scene-{id:04}")))
}

/// Pinned names for the native field-service cue catalogue. Unidentified cues
/// keep their original ID, within their bank's scope.
pub fn sound(id: u32) -> String {
    let name = match id {
        1 => "navigate",
        2 => "confirm",
        3 => "cancel",
        4 => "error",
        6 => "treasure-reward",
        28 => "treasure-open",
        29 => "treasure-bag",
        30 => "door",
        31 => "ring-prepare",
        32 => "ring-fire",
        41 => "ring-electric",
        42 => "ring-lightning",
        43 => "ring-ice",
        191 => "ring-shatter",
        272 => "ring-earthquake",
        236 => "ring-darkness",
        129 => "ring-sound",
        289 => "ring-bubble",
        87 => "ring-wind",
        110 => "ring-water",
        107 => "ring-bomb-place",
        194 => "ring-bomb-blast",
        330 => "ring-bomb-debris",
        287 => "ring-shrink",
        270 => "ring-mana",
        263 => "ring-radar",
        414 => "ring-splash",
        33 => "menu-open",
        38 => "page",
        104 => "recovery",
        132 => "remedy",
        130 => "battle-entry",
        _ => return format!("cue-{id:04}"),
    };
    name.into()
}

/// Initial names are pinned from both GQSEAF revision 0 phase tables and the
/// maintained scripts/std/locations.sym names. Numeric suffixes are original field
/// IDs, not discovered positions. Unknown locations retain field-NNNN fallbacks.
/// Title day/night names follow the runtime's existing field selection.
pub fn fields(ids: impl IntoIterator<Item = u32>) -> Result<Mappings> {
    let names: Mappings = serde_json::from_str(include_str!("naming/fields.json"))?;
    let mut selected = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for id in ids {
        let name = names
            .target(id)
            .map(str::to_owned)
            .unwrap_or_else(|_| format!("field-{id:04}"));
        ensure!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "invalid field publication name {name}"
        );
        ensure!(
            selected.insert(id, name.clone()).is_none(),
            "duplicate field ID {id}"
        );
        ensure!(
            paths.insert(name.clone()),
            "conflicting field publication name {name}"
        );
    }
    Mappings::new(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_cover_native_catalogues_and_remain_portable() -> Result<()> {
        for (domain, source, count) in [
            (
                "monsters",
                include_str!("naming/monsters.json"),
                resonance_content::monster::MONSTER_COUNT,
            ),
            (
                "figurines",
                include_str!("naming/figurines.json"),
                resonance_content::figurine::FIGURINE_COUNT,
            ),
        ] {
            let names: Mappings = serde_json::from_str(source)?;
            assert_eq!(names.iter().count(), count);
            let mut unique = BTreeSet::new();
            for id in (0..count as u32).rev() {
                let name = catalogue_model(domain, id)?;
                assert_eq!(name, names.target(id)?);
                assert!(
                    name.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                );
                assert!(unique.insert(name));
            }
        }
        assert_eq!(catalogue_model("figurines", 0)?, "lloyd-irving-0000");
        Ok(())
    }

    #[test]
    fn names_are_stable_under_selection_order_and_catalogue_growth() -> Result<()> {
        let first = fields([332, 330, 531, 474])?;
        let reordered = fields([474, 531, 330, 332])?;
        assert_eq!(first, reordered);
        assert_eq!(first.target(330)?, "iselia-0330");
        assert_eq!(first.target(332)?, "iselia-0332");
        assert_eq!(first.target(474)?, "title-day");
        assert_eq!(first.target(531)?, "title-night");
        let extended = fields([9999, 330])?;
        assert_eq!(extended.target(330)?, first.target(330)?);
        assert_eq!(extended.target(9999)?, "field-9999");
        assert!(fields([330, 330]).is_err());
        Ok(())
    }

    #[test]
    fn maintained_field_names_are_unique_and_portable() -> Result<()> {
        let names: Mappings = serde_json::from_str(include_str!("naming/fields.json"))?;
        assert_eq!(fields(names.iter().map(|(id, _)| id))?, names);
        Ok(())
    }

    #[test]
    #[ignore = "requires both original GQSEAF revision 0 extracted executables"]
    fn maintained_names_cover_every_declared_field_on_both_discs() -> Result<()> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let names: Mappings = serde_json::from_str(include_str!("naming/fields.json"))?;
        let mut declared = BTreeSet::new();
        for disc in [1, 2] {
            let executable = std::fs::read(root.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = crate::field_catalogue::read(&executable)?;
            for phase in catalogue
                .records
                .iter()
                .filter(|phase| phase.resource.is_some())
            {
                let id = u32::try_from(phase.id)?;
                names.target(id)?;
                declared.insert(id);
            }
        }
        assert_eq!(
            names.iter().map(|(id, _)| id).collect::<BTreeSet<_>>(),
            declared
        );
        Ok(())
    }
}
