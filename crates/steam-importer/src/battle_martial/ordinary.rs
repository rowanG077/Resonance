//! Proven ordinary native actions use the existing authored-action document.
use super::*;
use resonance_content::{battle_martial::AuthoredAction, battle_unison::Placement};

const SWORD_RAIN_TEMPEST: &str = "battle/artes/sword-rain-tempest/action.json";

pub(crate) fn action_path(image: &pe::Image<'_>, id: usize, row: &[u8]) -> Result<Option<String>> {
    if id != 30 {
        return Ok(None);
    }
    ensure!(
        pe::half(row, 0)? == 149,
        "Sword Rain: Tempest no longer selects ordinary martial149"
    );
    let descriptor = pe::word(image.slice(0x8e98d0 + 149 * 4, 4)?, 0)?;
    let handlers = image.slice(descriptor, 8)?;
    ensure!(
        pe::word(handlers, 0)? == 0x427dc0 && pe::word(handlers, 4)? == 0x427f20,
        "Sword Rain: Tempest has an unconverted native callback"
    );
    Ok(Some(SWORD_RAIN_TEMPEST.to_owned()))
}

pub(super) fn cook(source: &Source, library: &Path, catalogue: &Catalogue) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let row = image.slice(0x8d84b0 + 30 * 96, 96)?;
    let path =
        action_path(&image, 30, row)?.context("missing Sword Rain: Tempest authored path")?;
    let binding = catalogue
        .bindings
        .get(&30)
        .and_then(|bindings| bindings.get(&1))
        .context("Sword Rain: Tempest has no Lloyd source binding")?;
    ensure!(
        (binding.member, binding.variant) == (149, 0),
        "Sword Rain: Tempest source binding changed"
    );
    let attack = catalogue.variant(*binding)?.clone();
    ensure!(
        attack
            .events
            .iter()
            .any(|(_, event)| matches!(event, Event::Contact(_))),
        "ordinary martial149 has no contact timeline"
    );
    output::json(
        &library.join(path),
        &AuthoredAction {
            attack,
            motions: BTreeMap::new(),
            effects: None,
            unison: Placement {
                native_id: pe::half(row, 0)?,
                altitude: pe::word(row, 0x50)?,
                distance: pe::half(row, 0x54)?,
                duration: pe::half(row, 0x56)?,
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires native Steam; publishes only martial and technique rule owners"]
    fn installed_sword_rain_tempest_publishes_existing_authored_action() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let temporary = tempfile::tempdir()?;
        let library = std::env::var_os("RESONANCE_TEST_LIBRARY")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| temporary.path().to_owned());
        crate::battle_martial::cook(&source, &library)?;
        crate::techniques::cook(&source, &library)?;
        let document: resonance_content::arte::document::Document =
            serde_json::from_slice(&std::fs::read(library.join(resonance_content::arte::PATH))?)?;
        let entry = &document.techniques[document.mappings.target(30)?];
        assert_eq!(entry.rules.action.as_deref(), Some(SWORD_RAIN_TEMPEST));
        for id in [31, 32] {
            assert!(
                document.techniques[document.mappings.target(id)?]
                    .rules
                    .action
                    .is_none()
            );
        }
        let action: AuthoredAction =
            serde_json::from_slice(&std::fs::read(library.join(SWORD_RAIN_TEMPEST))?)?;
        let catalogue = crate::battle_martial::read(&source)?;
        assert_eq!(
            serde_json::to_value(&action.attack)?,
            serde_json::to_value(catalogue.attack(30, 1)?)?
        );
        assert_eq!(action.unison.native_id, 149);
        assert!(
            action
                .attack
                .events
                .iter()
                .filter(|(_, event)| matches!(event, Event::Contact(_)))
                .count()
                >= 8
        );
        Ok(())
    }
}
