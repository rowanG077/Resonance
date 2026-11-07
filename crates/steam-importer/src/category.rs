//! Focused development cooks write the same owners as the full importer.
use super::*;

#[derive(Debug, Clone, Copy)]
pub enum Category {
    Tables,
    FieldScripts,
    PartyModels,
    MartialAttacks,
    EnemyModels,
    EnemyEffects,
    EnemyRules,
    ResidentEffects,
}

impl std::str::FromStr for Category {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(match value {
            "tables" => Self::Tables,
            "field-scripts" => Self::FieldScripts,
            "party-models" => Self::PartyModels,
            "martial-attacks" => Self::MartialAttacks,
            "enemy-models" => Self::EnemyModels,
            "enemy-effects" => Self::EnemyEffects,
            "enemy-rules" => Self::EnemyRules,
            "resident-effects" => Self::ResidentEffects,
            _ => anyhow::bail!(
                "unknown Steam category {value}; expected tables, field-scripts, party-models, martial-attacks, enemy-models, enemy-effects, enemy-rules or resident-effects; field scenery, field actors and audio require a full cook"
            ),
        })
    }
}

/// Recook selected owners; dependencies must already exist in the output library.
pub fn cook_category(
    installation: &Path,
    output: &Path,
    language: Language,
    category: Category,
    ids: &[u16],
) -> Result<()> {
    validate_output(installation, output)?;
    // Partial publication cannot upgrade an older library's format descriptor.
    resonance_content::library::Library::open(output, &[])
        .context("category cooking needs a current library")?;
    match category {
        Category::EnemyModels | Category::EnemyEffects | Category::EnemyRules => (),
        Category::FieldScripts => {
            ensure!(ids.len() <= 1, "field categories accept at most one --id")
        }
        _ => ensure!(ids.is_empty(), "this category does not accept --id"),
    }
    let source = source::Source::open(installation, language)?;
    let selected = (!ids.is_empty()).then_some(ids);
    let field = ids.first().copied().map(u32::from);
    match category {
        Category::Tables => cook_tables(&source, output),
        Category::FieldScripts => field_core::cook(&source, output, field),
        Category::PartyModels => {
            battle_party_mappings::cook(&source, output)?;
            battle_party_model::cook(&source, output)
        }
        Category::MartialAttacks => {
            battle_martial::cook(&source, output)?;
            techniques::cook(&source, output)
        }
        Category::EnemyModels => monsters::models_selected(&source, output, selected),
        Category::EnemyEffects => battle_effects::cook_enemies(&source, output, selected),
        Category::EnemyRules => battle_enemies::cook(&source, output, selected),
        Category::ResidentEffects => battle_effects::cook_resident(&source, output),
    }
    .with_context(|| format!("Steam {category:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_cooking_rejects_incomplete_owners_and_obsolete_libraries() -> Result<()> {
        for name in ["field-actors", "field-scenery", "audio"] {
            assert!(name.parse::<Category>().is_err());
        }
        let source = tempfile::tempdir()?;
        let output = tempfile::tempdir()?;
        let path = output.path().join(resonance_content::library::DESCRIPTOR);
        let bytes = serde_json::to_vec(&resonance_content::library::Descriptor {
            version: resonance_content::library::VERSION - 1,
            game: resonance_content::library::GAME.into(),
        })?;
        std::fs::write(&path, &bytes)?;
        let error = cook_category(
            source.path(),
            output.path(),
            Language::English,
            Category::Tables,
            &[],
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("unsupported cooked layout"));
        assert_eq!(std::fs::read(path)?, bytes);
        Ok(())
    }
}
