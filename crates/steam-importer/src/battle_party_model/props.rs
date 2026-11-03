//! The executable's four costume-owned cosmetic weapon substitutions.
use crate::pe;
use anyhow::{Result, ensure};
use resonance_content::battle_model::document::WeaponProp;
use std::{collections::BTreeMap, num::NonZeroU16};

pub(super) fn read(image: &pe::Image<'_>) -> Result<BTreeMap<(u8, u8), WeaponProp>> {
    // The same clear-count byte is incremented and capped at100 by the
    // complete-game command; the battle selector tests it before drawing.
    ensure!(
        image.slice(0x56c37d, 7)? == [0x0f, 0xb6, 0xb8, 0x5a, 0x0f, 0, 0]
            && image.slice(0x56c384, 6)? == [0x8d, 0x4f, 1, 0x83, 0xf9, 0x64]
            && image.slice(0x47cc2b, 7)? == [0x80, 0xbf, 0x5a, 0x0f, 0, 0, 1]
            && image.slice(0x47cc61, 4)? == [0xa8, 0x0f, 0x75, 0x40],
        "costume prop clear-count or random gate changed"
    );
    let mut rules = BTreeMap::new();
    for address in [0x47cc65, 0x47cc75, 0x47cc85, 0x47cc95] {
        let row = image.slice(address, 16)?;
        ensure!(
            row[..2] == [0x80, 0xfa]
                && row[3] == 0x75
                && row[5..8] == [0x80, 0x7e, 0x11]
                && row[9..12] == [0x75, 5, 0xb9],
            "costume prop selector changed"
        );
        ensure!(
            rules
                .insert(
                    (row[2], row[8]),
                    WeaponProp {
                        equipment: pe::word(row, 12)?.try_into()?,
                        minimum_game_clears: 1,
                        one_in: NonZeroU16::new(16).unwrap(),
                    }
                )
                .is_none(),
            "duplicate costume prop"
        );
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, output, source::Source};
    use anyhow::Context;
    use resonance_content::{
        battle_model::{Attachment, document},
        diagnostics::Diagnostics,
        library::Library,
    };
    use std::{fs, path::PathBuf};

    #[test]
    #[ignore = "requires installed Steam and current cooked library; updates only costume prop metadata and captures actual variants"]
    fn installed_costume_prop_policies_bind_native_variants() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let rules = read(&image)?;
        ensure!(
            rules
                .iter()
                .map(|(&(character, costume), rule)| (character, costume, rule.equipment))
                .eq([(5, 2, 534), (6, 1, 532), (7, 1, 535), (7, 5, 536)]),
            "changed native costume props"
        );
        let library = Library::open(&root, &[])?;
        for ((character, costume), rule) in rules {
            let path = document::party_path(&mut library.snapshot(), character, costume)?;
            let mut definition: document::PartyDefinition =
                serde_json::from_slice(&fs::read(root.join(&path))?)?;
            definition.weapon_prop = Some(rule);
            output::json(&root.join(path), &definition)?;
            let (_, party) = document::capture_party(
                &mut library.snapshot(),
                character,
                costume,
                &Diagnostics::new(true),
                &|| false,
            )?;
            ensure!(
                party.weapon_prop == Some(rule),
                "costume prop not retained by shared reader"
            );
            ensure!(
                matches!(
                    document::capture_equipment(
                        &mut library.snapshot(),
                        rule.equipment,
                        &Diagnostics::new(true),
                        &|| false
                    )?,
                    Attachment::Weapon(_)
                ),
                "costume prop is not a visual weapon"
            );
        }
        Ok(())
    }
}
