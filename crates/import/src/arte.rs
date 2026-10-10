//! Decode and publish technique data.
mod definition;
mod tables;

use anyhow::{Context, Result, ensure};
use std::path::Path;

pub(crate) use resonance_content::arte::{Catalogue, Definition};

pub(crate) struct Imported {
    pub catalogue: Catalogue,
    pub menu: Vec<MenuDefinition>,
}

/// Fields used to build menu data, omitted from the battle catalogue.
pub(crate) struct MenuDefinition {
    pub text: resonance_content::menu_data::NamedText,
    pub tp_percent: bool,
    pub rank: u8,
    pub route: u8,
    pub alternatives: [u16; 4],
}

pub(crate) fn read(executable: &[u8]) -> Result<Imported> {
    let (definitions, menu): (Vec<_>, Vec<_>) = definition::definitions(executable)?
        .into_iter()
        .map(|row| (row.gameplay, row.menu))
        .unzip();
    let mut catalogue = Catalogue {
        definitions,
        learning: tables::learning(executable)?,
    };
    catalogue.validate()?;
    ensure!(
        menu.iter().all(|row| row.route <= 3
            && row
                .alternatives
                .iter()
                .all(|&id| usize::from(id) < catalogue.definitions.len())),
        "invalid technique menu metadata"
    );
    learning_conditions(&mut catalogue)?;
    Ok(Imported { catalogue, menu })
}

fn learning_conditions(catalogue: &mut Catalogue) -> Result<()> {
    const COLETTE: u8 = 2;
    const RING_WHIRLWIND: usize = 45;
    const HEALING_TRAINING: [(usize, u16); 2] = [(122, 120), (123, 119)];
    let successors: Vec<_> = catalogue
        .learned_by(COLETTE)?
        .iter()
        .flat_map(|&id| {
            let rule = &catalogue.definitions[usize::from(id)].learning;
            [rule.technical_successor, rule.strike_successor]
                .into_iter()
                .flatten()
        })
        .collect();
    for id in successors {
        catalogue.definitions[usize::from(id)]
            .learning
            .requires_story_unlock = true;
    }
    catalogue.definitions[RING_WHIRLWIND]
        .learning
        .requires_story_unlock = true;
    for (id, prerequisite) in HEALING_TRAINING {
        let group = catalogue.definitions[id]
            .learning
            .prerequisites
            .iter_mut()
            .find(|group| group.any_of == [prerequisite])
            .context("missing healing learning prerequisite")?;
        group.minimum_uses = 50;
    }
    Ok(())
}

pub fn publish(catalogue: &Catalogue, output: &Path) -> Result<String> {
    let path = resonance_content::arte::PATH;
    crate::write_atomic(&output.join(path), &serde_json::to_vec(catalogue)?)?;
    Ok(path.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs};

    #[test]
    #[ignore = "requires both original extracted discs; only publishes JSON tables"]
    fn both_discs_publish_the_same_catalogue_and_menu_cost_policy() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = tempfile::tempdir()?;
        let mut payloads = BTreeSet::new();
        for disc in [1, 2] {
            let source = extracted.join(format!("disc{disc}/sys/main.dol"));
            let executable = fs::read(&source)?;
            let imported = read(&executable)?;
            let destination = output.path().join(format!("disc{disc}"));
            let path = publish(&imported.catalogue, &destination)?;
            let bytes = fs::read(destination.join(path))?;
            let restored: Catalogue = serde_json::from_slice(&bytes)?;
            restored.validate()?;
            assert_eq!(
                serde_json::to_value(&restored)?,
                serde_json::to_value(&imported.catalogue)?
            );
            assert_eq!(
                imported
                    .menu
                    .iter()
                    .enumerate()
                    .filter_map(|(id, row)| row.tp_percent.then_some(id))
                    .collect::<Vec<_>>(),
                [34, 202, 203, 204, 205, 206]
            );
            assert_eq!(restored.learned_by(10)?, [66, 98]);
            assert!(restored.learned_by(11)?.is_empty());
            assert!(restored.learned_by(0).is_err() && restored.learned_by(12).is_err());
            assert!(restored.definition(253).is_err());
            for id in [36, 45, 224] {
                assert!(restored.definitions[id].learning.requires_story_unlock);
            }
            assert!(!restored.definitions[35].learning.requires_story_unlock);
            for (id, prerequisite) in [(122, 120), (123, 119)] {
                let group = &restored.definitions[id].learning.prerequisites[0];
                assert_eq!(
                    (&group.any_of, group.minimum_uses),
                    (&vec![prerequisite], 50)
                );
            }
            payloads.insert(crate::digest(&bytes));
        }
        assert_eq!(payloads.len(), 1);
        Ok(())
    }
}
