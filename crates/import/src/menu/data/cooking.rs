use super::*;
use crate::all_assets::cooking_ui::{Catalogue, Label};
use resonance_content::menu_data::{
    CookingData, FoodPreferences, Ingredient, IngredientGroup, Recipe, RecipeCook, RecipeGrade,
};

pub(super) fn cook(source: &Catalogue) -> Result<CookingData> {
    let text = |reference| -> Result<String> { Ok(source.required_text(reference)?.to_owned()) };
    let ingredients = |source: &[u16]| -> Result<Vec<Ingredient>> {
        source
            .iter()
            .map(|&item| {
                Ok(match item {
                    0 => Ingredient::None,
                    1..30000 => Ingredient::Item(item),
                    30000..30032 => Ingredient::Any((item - 30000) as u8),
                    _ => anyhow::bail!("unsupported cooking ingredient {item}"),
                })
            })
            .collect()
    };
    Ok(CookingData {
        recipes: source
            .recipes
            .iter()
            .map(|recipe| {
                Ok(Recipe {
                    name: text(recipe.name)?,
                    description: text(recipe.description)?,
                    required: ingredients(&recipe.required)?,
                    cooks: recipe
                        .cooks
                        .iter()
                        .map(|cook| {
                            Ok(RecipeCook {
                                base_stars: cook.base_stars.try_into()?,
                                grades: cook
                                    .grades
                                    .iter()
                                    .map(|grade| {
                                        ensure!(
                                            grade.unknown_effect_bits == 0,
                                            "unsupported cooking effect mask {:#x}",
                                            grade.unknown_effect_bits
                                        );
                                        Ok(RecipeGrade {
                                            effects: grade.effects.clone(),
                                            recovery: grade.recovery.try_into()?,
                                            extras: ingredients(&grade.extras)?,
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
            .collect::<Result<_>>()?,
        groups: source
            .groups
            .iter()
            .map(|group| {
                Ok(IngredientGroup {
                    name: text(group.name)?,
                    category: group.category,
                    items: group.items.clone().context("null cooking group")?,
                })
            })
            .collect::<Result<_>>()?,
        preferences: source
            .preferences
            .iter()
            .map(|p| {
                Ok(FoodPreferences {
                    likes: p.likes.clone().context("null cooking likes")?,
                    dislikes: p.dislikes.clone().context("null cooking dislikes")?,
                })
            })
            .collect::<Result<_>>()?,
        labels: Label::RUNTIME
            .into_iter()
            .map(|(label, name)| Ok((name.into(), source.label(label)?.into())))
            .chain(std::iter::once(Ok((
                "locked".into(),
                source.text(source.locked).into(),
            ))))
            .collect::<Result<_>>()?,
        effects: source
            .effects
            .map(text)
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        bonus_skill: source.bonus_skill.try_into()?,
    })
}
