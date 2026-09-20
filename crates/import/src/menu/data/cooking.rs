use super::*;
use crate::all_assets::cooking_ui::{
    Catalogue, Ingredient as SourceIngredient, Ingredients, Label,
};
use resonance_content::menu_data::{
    CookingData, FoodPreferences, Ingredient, IngredientGroup, Recipe, RecipeCook, RecipeGrade,
};

pub(super) fn cook(source: &Catalogue) -> Result<CookingData> {
    let text = |reference| -> Result<String> { Ok(source.required_text(reference)?.to_owned()) };
    let ingredients = |source: &Ingredients| -> Result<Vec<Ingredient>> {
        source
            .active
            .iter()
            .map(|&item| {
                Ok(match item {
                    SourceIngredient::None => Ingredient::None,
                    SourceIngredient::Item(id) => Ingredient::Item(id),
                    SourceIngredient::Group(group) => Ingredient::Any(group),
                    SourceIngredient::Unknown(id) => {
                        anyhow::bail!("unsupported cooking ingredient {id}")
                    }
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
                    items: source.items(group.items)?.to_vec(),
                })
            })
            .collect::<Result<_>>()?,
        preferences: source
            .preferences
            .iter()
            .map(|p| {
                Ok(FoodPreferences {
                    likes: source.items(p.likes)?.to_vec(),
                    dislikes: source.items(p.dislikes)?.to_vec(),
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
