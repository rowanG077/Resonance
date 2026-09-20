//! Recover every battle-enabled consumable and reject unknown handler families.
use super::actor_tables::ActorTables;
use anyhow::{Context, Result, bail, ensure};
use resonance_content::{battle::items::*, menu_data::Element};

pub(super) fn cook(
    definitions: &[crate::item::Definition],
    tables: &ActorTables,
) -> Result<BattleItems> {
    tables.validate()?;
    let mut recipes = Vec::new();
    for (id, row) in definitions.iter().enumerate() {
        // Row zero is the empty inventory sentinel even though its flags are set.
        if id != 0 && row.usage_flags & 2 != 0 {
            recipes.push(ItemRecipe {
                item: id as u16,
                action: action(id as u16)?,
            });
        }
    }
    let items = BattleItems {
        recipes,
        throw_origins: tables
            .item_throw_origins
            .get(..9)
            .context("missing party item origins")?
            .try_into()?,
        recovery_color: tables.tint_palette[0],
        enhancement_color: tables.tint_palette[2],
        scan_color: tables.tint_palette[7],
    };
    items.validate()?;
    ensure!(
        items.recipes.len() == 32,
        "battle item inventory changed; audit each enabled source handler"
    );
    Ok(items)
}

fn action(id: u16) -> Result<ItemAction> {
    use ItemAction::*;
    Ok(match id {
        1..=9 => {
            let (hp, tp, party) = [
                (30, 0, false),
                (60, 0, false),
                (0, 30, false),
                (0, 60, false),
                (30, 30, false),
                (60, 60, false),
                (100, 100, false),
                (30, 0, true),
                (0, 30, true),
            ][usize::from(id - 1)];
            Recover {
                hp,
                tp,
                party,
                enhanced: id != 7,
            }
        }
        10 => Cure {
            ailments: Ailments::Physical,
        },
        11 => Revive,
        12 => Cure {
            ailments: Ailments::All,
        },
        13 => Cure {
            ailments: Ailments::Magical,
        },
        14 | 15 => Enhance {
            enhancement: Enhancement::Attack,
        },
        16 => Enhance {
            enhancement: Enhancement::Defense,
        },
        17 => Enhance {
            enhancement: Enhancement::Accuracy,
        },
        18 | 19 => Enhance {
            enhancement: Enhancement::PhysicalProtection { retained: id == 18 },
        },
        20 | 21 => Enhance {
            enhancement: Enhancement::MagicalProtection { retained: id == 20 },
        },
        37 => Scan,
        38 => HalveDamage,
        39 => StopEnemies,
        44..=51 => Enhance {
            enhancement: Enhancement::Element {
                element: [
                    Element::Water,
                    Element::Wind,
                    Element::Fire,
                    Element::Earth,
                    Element::Ice,
                    Element::Lightning,
                    Element::Darkness,
                    Element::Light,
                ][usize::from(id - 44)],
            },
        },
        _ => bail!("unimplemented battle item handler {id}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_enabled_family_has_a_recipe_and_field_only_items_do_not() {
        for id in (1..=21).chain(37..=39).chain(44..=51) {
            assert!(action(id).is_ok(), "item {id}");
        }
        for id in (22..=36).chain(40..=43).chain([0, 52, 527]) {
            assert!(action(id).is_err(), "item {id}");
        }
    }
}
