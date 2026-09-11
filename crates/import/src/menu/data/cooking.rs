use super::*;
use resonance_content::menu_data::{
    CookingData, FoodPreferences, Ingredient, IngredientGroup, MealEffect, RECIPE_COUNT, Recipe,
    RecipeCook, RecipeGrade,
};

pub(super) fn cook(
    executable: &[u8],
    text: &impl Fn(&[u8], usize) -> Result<String>,
) -> Result<CookingData> {
    let half = |bytes: &[u8], offset: usize| {
        u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
    };
    let word = |bytes: &[u8], offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let list = |pointer: u32| -> Result<Vec<u16>> {
        let count = usize::from(half(dol::slice(executable, pointer, 2)?, 0));
        ensure!(count <= 16, "invalid cooking ingredient list");
        Ok(dol::slice(executable, pointer + 2, count * 2)?
            .chunks_exact(2)
            .map(|r| half(r, 0))
            .collect())
    };
    let ingredients = |row: &[u8], offset: usize, maximum: usize| -> Result<Vec<Ingredient>> {
        let count = usize::from(half(row, offset));
        ensure!(count <= maximum, "invalid recipe ingredient count");
        row[offset + 2..offset + 2 + count * 2]
            .chunks_exact(2)
            .map(|v| {
                let id = half(v, 0);
                Ok(if id == 0 {
                    Ingredient::None
                } else if id < 30000 {
                    Ingredient::Item(id)
                } else {
                    ensure!(id < 30032, "unknown recipe ingredient group");
                    Ingredient::Any((id - 30000) as u8)
                })
            })
            .collect()
    };
    let recipes = dol::slice(executable, 0x80215b0c, RECIPE_COUNT * 0x2bc)?
        .chunks_exact(0x2bc)
        .map(|row| {
            Ok(Recipe {
                name: text(row, 0)?,
                description: text(row, 4)?,
                required: ingredients(row, 8, 3)?,
                cooks: (0..9)
                    .map(|character| {
                        let row = &row[0x10 + character * 0x4c..];
                        Ok(RecipeCook {
                            base_stars: half(row, 0).try_into()?,
                            grades: (0..3)
                                .map(|grade| {
                                    let row = &row[4 + grade * 0x18..];
                                    let mask = word(row, 0);
                                    ensure!(mask & !0xfff == 0, "unknown cooking effect");
                                    Ok(RecipeGrade {
                                        effects: MealEffect::ALL
                                            .into_iter()
                                            .enumerate()
                                            .filter_map(|(i, effect)| {
                                                (mask & (1 << i) != 0).then_some(effect)
                                            })
                                            .collect(),
                                        recovery: half(row, 4).try_into()?,
                                        extras: ingredients(row, 6, 8)?,
                                    })
                                })
                                .collect::<Result<Vec<_>>>()?
                                .try_into()
                                .unwrap(),
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()?;
    let groups = (0..32)
        .map(|i| {
            Ok(IngredientGroup {
                name: text(dol::slice(executable, 0x802159fc + i * 4, 4)?, 0)?,
                category: dol::slice(executable, 0x80199e68 + i, 1)?[0],
                items: list(word(dol::slice(executable, 0x8021597c + i * 4, 4)?, 0))?,
            })
        })
        .collect::<Result<_>>()?;
    let preferences = (0..9)
        .map(|i| {
            Ok(FoodPreferences {
                likes: list(word(dol::slice(executable, 0x80215ac4 + i * 4, 4)?, 0))?,
                dislikes: list(word(dol::slice(executable, 0x80215ae8 + i * 4, 4)?, 0))?,
            })
        })
        .collect::<Result<_>>()?;
    let labels = [
        ("cook", 0x78),
        ("required", 0x7c),
        ("additional", 0x80),
        ("success", 0x84),
        ("failure", 0x88),
        ("no_effect", 0x8c),
        ("missing", 0x90),
        ("full", 0x94),
        ("unknown", 0x98),
        ("result_join", 0x9c),
    ]
    .into_iter()
    .map(|(name, at)| {
        Ok((
            name.into(),
            text(dol::slice(executable, 0x80199dc8 + at, 4)?, 0)?,
        ))
    })
    .chain(std::iter::once(Ok((
        "locked".into(),
        text(&0x80199e88u32.to_be_bytes(), 0)?,
    ))))
    .collect::<Result<_>>()?;
    Ok(CookingData {
        recipes,
        groups,
        preferences,
        labels,
        effects: dol::slice(executable, 0x80215a7c, 12 * 4)?
            .chunks_exact(4)
            .map(|r| text(r, 0))
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        bonus_skill: half(dol::slice(executable, 0x8018d1f4, 2)?, 0).try_into()?,
    })
}
