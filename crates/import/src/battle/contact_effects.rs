//! Preserve physical contact tables and bind the supported combat events.
use super::embedded::Layout;
use super::*;
use resonance_content::battle::contact_effects::ContactEffects;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContactEffectTables {
    /// Ten four-byte colors; the native flash setter consumes only RGB.
    flash: [[u8; 4]; 10],
    elemental: [u8; 10],
    /// Unexplained bytes between the effects and the next sound table.
    trailing_storage: Vec<u8>,
}

impl ContactEffectTables {
    pub(super) fn bind(&self) -> ContactEffects {
        ContactEffects {
            elemental: std::array::from_fn(|i| self.elemental[i]),
            flash: std::array::from_fn(|i| {
                let [r, g, b, _] = self.flash[i];
                [r, g, b]
            }),
        }
    }
}

fn parse(colors: &[u8], elemental: &[u8]) -> Result<ContactEffectTables> {
    let (colors, remainder) = colors.as_chunks::<4>();
    ensure!(remainder.is_empty(), "incomplete contact color");
    let (elemental, trailing_storage) = elemental
        .split_at_checked(10)
        .context("incomplete contact effects")?;
    Ok(ContactEffectTables {
        flash: colors.try_into().context("expected ten contact colors")?,
        elemental: elemental.try_into()?,
        trailing_storage: trailing_storage.to_vec(),
    })
}

fn read(rel: &actions::Rel, layout: &Layout) -> Result<ContactEffectTables> {
    let colors = layout
        .contact_effects
        .checked_sub(layout.contact_colors)
        .context("reversed contact color table")?;
    let effects = layout
        .contact_sounds
        .checked_sub(layout.contact_effects)
        .context("reversed contact effect table")?;
    parse(
        rel.at((5, layout.contact_colors))?
            .get(..colors)
            .context("truncated contact colors")?,
        rel.at((5, layout.contact_effects))?
            .get(..effects)
            .context("truncated contact effects")?,
    )
}

pub(super) fn cook(rel: &actions::Rel) -> Result<ContactEffects> {
    Ok(read(rel, &Layout::RETAIL)?.bind())
}

pub(crate) fn cook_all(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((_, layout)) = Layout::identify(file) else {
        return Ok(None);
    };
    embedded::write(
        file,
        output,
        "battle-contact-effects",
        &read(&actions::Rel::read(file)?, &layout)?,
        serde_json::json!({
            "section": 5,
            "colors": { "offset": layout.contact_colors, "stride": 4, "count": 10 },
            "elemental": { "offset": layout.contact_effects, "end": layout.contact_sounds },
        }),
    )
    .map(Some)
}

pub(super) fn require_inert_guard_effect(effect: u8) -> Result<()> {
    // 3B370 also dispatches metadata9e from an actor-relative effect bank.
    // Every currently selected original actor has zero; a nonzero consumer
    // must be recovered explicitly before its actor can enter a cooked battle.
    ensure!(
        effect == 0,
        "unsupported actor-specific guard impact effect"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_records_survive_binding_and_incomplete_tables_fail() -> Result<()> {
        let colors: Vec<u8> = (0..40).collect();
        let effects: Vec<u8> = (0..12).collect();
        let tables = parse(&colors, &effects)?;
        assert_eq!(tables.flash[9], [36, 37, 38, 39]);
        assert_eq!(tables.elemental, effects[..10]);
        assert_eq!(tables.trailing_storage, [10, 11]);
        let binding = tables.bind();
        assert_eq!(binding.elemental, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(binding.flash[8], [32, 33, 34]);
        let json = serde_json::to_vec(&tables)?;
        assert_eq!(
            serde_json::from_slice::<ContactEffectTables>(&json)?,
            tables
        );
        assert_eq!(parse(&colors, &effects[..10])?.bind(), binding);
        for length in [0, 36, 39] {
            assert!(parse(&colors[..length], &effects).is_err());
        }
        for length in [0, 9] {
            assert!(parse(&colors, &effects[..length]).is_err());
        }
        let mut layout = Layout::RETAIL;
        layout.contact_colors = 0;
        layout.contact_effects = 40;
        layout.contact_sounds = 52;
        let mut rel = actions::Rel {
            bytes: [vec![0], colors, effects].concat(),
            sections: vec![(1, 52); 6],
            pointers: Default::default(),
            local_targets: Default::default(),
        };
        assert_eq!(read(&rel, &layout)?, tables);
        rel.sections[5].1 = 51;
        assert!(read(&rel, &layout).is_err());
        rel.sections[5].1 = 52;
        layout.contact_effects = 53;
        assert!(read(&rel, &layout).is_err());
        layout.contact_colors = 54;
        assert!(read(&rel, &layout).is_err());
        Ok(())
    }

    #[test]
    fn actor_guard_effect_absence_is_explicit_and_nonzero_stays_strict() {
        require_inert_guard_effect(0).unwrap();
        assert!(require_inert_guard_effect(1).is_err());
    }

    #[test]
    #[ignore = "requires both original extracted discs; no media conversion"]
    fn original_contact_tables_preserve_every_module_and_runtime_binding() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("contact-effects"));
        let expected = ContactEffects {
            elemental: [0, 30, 31, 29, 32, 33, 34, 49, 0],
            flash: [
                [192, 128, 128],
                [64, 64, 192],
                [64, 192, 64],
                [192, 64, 64],
                [192, 192, 128],
                [160, 160, 192],
                [160, 160, 192],
                [192, 192, 192],
                [48, 48, 48],
            ],
        };
        let mut publications = BTreeSet::new();
        for disc in [1, 2] {
            let files = local.join(format!("disc{disc}/files"));
            let mut modules = 0;
            for file in fs::read_dir(files)? {
                let file = file?.path();
                let Some((module, layout)) = Layout::identify(&file) else {
                    continue;
                };
                let rel = actions::Rel::read(&file)?;
                let tables = read(&rel, &layout)?;
                assert_eq!(tables.bind(), expected, "{module}");
                assert_eq!(
                    tables.flash.as_flattened(),
                    &rel.at((5, layout.contact_colors))?[..40]
                );
                assert_eq!(tables.flash[9], [192, 192, 192, 255]);
                assert_eq!(
                    tables.trailing_storage.len(),
                    if module == "Top2BtlD.rel" { 0 } else { 2 }
                );
                assert_eq!(tables.elemental, rel.at((5, layout.contact_effects))?[..10]);
                assert_eq!(
                    tables.trailing_storage,
                    rel.at((5, layout.contact_effects))?
                        [10..layout.contact_sounds - layout.contact_effects]
                );
                for root in [
                    layout.contact_colors,
                    layout.contact_effects,
                    layout.contact_sounds,
                ] {
                    assert!(rel.local_targets().contains(&(5, root)));
                }
                let paths = cook_all(&file, &output)?.unwrap();
                let published: ContactEffectTables =
                    crate::embedded::read(&output, "battle-contact-effects", module)?;
                assert_eq!(published, tables);
                publications.insert(paths[0].clone());
                if module == "US_r_Top2Btl.rel" {
                    assert_eq!(cook(&rel)?, expected);
                }
                modules += 1;
            }
            assert_eq!(modules, 7);
        }
        assert_eq!(publications.len(), 2);
        fs::remove_dir_all(output)?;
        Ok(())
    }

    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_selected_actors_have_no_additional_guard_effect() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = actions::Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        for character in 0..9 {
            let settings = super::super::all::ActorSettings::read(
                rel.at((5, 0x3d30 + character * 0x1f0)).unwrap(),
            )
            .unwrap();
            require_inert_guard_effect(settings.model.guard_effect).unwrap();
        }
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let enemies = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
        let directory = word(&usual, 0x2c).unwrap() as usize;
        for id in [
            36, 39, 49, 51, 52, 72, 73, 100, 101, 104, 107, 172, 195, 205, 206, 207, 247, 250,
        ] {
            let start = word(&usual, directory + id * 4).unwrap() as usize;
            let end = word(&usual, directory + (id + 1) * 4).unwrap() as usize;
            let package = crate::compression::decode(&enemies[start..end]).unwrap();
            let metadata = &package[half(&package, 4).unwrap() as usize..];
            require_inert_guard_effect(
                super::super::all::ActorSettings::read(metadata)
                    .unwrap()
                    .model
                    .guard_effect,
            )
            .unwrap();
        }
    }
}
