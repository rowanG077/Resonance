use crate::all_assets::title_catalogue::CostumeBindings;
use anyhow::{Context, Result, bail, ensure};
use resonance_content::menu_data::{Costume, Title};

const MEMBERS: usize = 9;

pub(super) fn cook(bindings: &CostumeBindings, titles: &mut [Vec<Title>]) -> Result<()> {
    ensure!(titles.len() == MEMBERS, "invalid costume character count");
    for (row, variant) in bindings.titles.iter().zip(bindings.variants) {
        let costume = match variant {
            1 => Costume::Variant1,
            2 => Costume::Variant2,
            4 => Costume::Variant4,
            _ => bail!("invalid title costume variant {variant}"),
        };
        for (member, &id) in row.iter().enumerate() {
            if id == -1 {
                continue;
            }
            ensure!((1..32).contains(&id), "invalid costume title {id}");
            let title = titles[member]
                .get_mut(id as usize - 1)
                .with_context(|| format!("missing costume title {id} for member {member}"))?;
            ensure!(
                title.costume.replace(costume).is_none(),
                "duplicate costume title {id} for member {member}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(count: usize) -> Vec<Vec<Title>> {
        vec![
            vec![
                Title {
                    name: String::new(),
                    description: String::new(),
                    growth: [0; 7],
                    costume: None,
                };
                count
            ];
            MEMBERS
        ]
    }

    #[test]
    fn costume_mapping_checks_absence_references_and_conflicts() {
        let mut bindings = CostumeBindings {
            titles: [[-1; MEMBERS]; 3],
            variants: [1, 2, 4],
        };
        bindings.titles[0][0] = 2;
        bindings.titles[1][6] = 6;
        bindings.titles[2][7] = 3;
        let mut data = titles(7);
        cook(&bindings, &mut data).unwrap();
        assert_eq!(data[0][1].costume, Some(Costume::Variant1));
        assert_eq!(data[6][5].costume, Some(Costume::Variant2));
        assert_eq!(data[7][2].costume, Some(Costume::Variant4));
        assert_eq!(data[6][1].costume, None);
        assert!(data[8].iter().all(|title| title.costume.is_none()));

        for invalid in [-2, 0, 8, 32] {
            let mut damaged = bindings.clone();
            damaged.titles[0][1] = invalid;
            assert!(cook(&damaged, &mut titles(7)).is_err());
        }
        let mut duplicate = bindings.clone();
        duplicate.titles[2][6] = 6;
        assert!(cook(&duplicate, &mut titles(7)).is_err());
        for invalid in [0, 3, 5, 255] {
            let mut damaged = bindings.clone();
            damaged.variants[1] = invalid;
            assert!(cook(&damaged, &mut titles(7)).is_err());
        }
        assert!(cook(&bindings, &mut titles(7)[..8]).is_err());
    }
}
