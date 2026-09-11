use super::*;
use super::{
    items::{INCAPACITATED, REVIVAL_CLEARS},
    stats::recover,
};
use resonance_content::menu_data::{Ingredient, MealEffect, MenuData};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cooking {
    pub known: u32,
    pub recipe: u8,
    /// Index into members, independent of formation order.
    pub chef: u8,
    pub full: bool,
}
impl Default for Cooking {
    fn default() -> Self {
        Self {
            known: 1,
            recipe: 0,
            chef: 0,
            full: false,
        }
    }
}
impl Cooking {
    pub fn knows(&self, recipe: u8) -> bool {
        recipe < 32 && self.known & (1 << recipe) != 0
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Meal {
    pub success: bool,
    pub ingredients: Vec<u16>,
    /// Each active effect carries its recovery percentage, where applicable.
    pub effects: BTreeMap<MealEffect, u16>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookingError {
    UnavailableCook,
    UnknownRecipe,
    MissingIngredients,
    Full,
}

impl Party {
    pub fn ingredient_count(&self, data: &MenuData, ingredient: Ingredient) -> u16 {
        match ingredient {
            Ingredient::None => 0,
            Ingredient::Item(id) => u16::from(self.items.get(&id).copied().unwrap_or(0)),
            Ingredient::Any(group) => data.cooking.groups[usize::from(group)]
                .items
                .iter()
                .map(|id| u16::from(self.items.get(id).copied().unwrap_or(0)))
                .sum(),
        }
    }
    pub fn has_ingredients(&self, data: &MenuData, recipe: u8) -> bool {
        data.cooking
            .recipes
            .get(usize::from(recipe))
            .is_some_and(|r| {
                r.required
                    .iter()
                    .all(|&ingredient| self.ingredient_count(data, ingredient) > 0)
            })
    }
    /// Rejected meals consume neither ingredients nor random draws. Successful
    /// attempts, including failed dishes, consume food and train the cook.
    pub fn cook(
        &mut self,
        data: &MenuData,
        mut random: impl FnMut() -> u32,
    ) -> Result<Meal, CookingError> {
        use CookingError::*;
        let chef = usize::from(self.cooking.chef);
        let recipe_id = usize::from(self.cooking.recipe);
        let Some(member) = self.members.get(chef) else {
            return Err(UnavailableCook);
        };
        if !self.formation.contains(&(self.cooking.chef + 1))
            || member.conditions & INCAPACITATED != 0
        {
            return Err(UnavailableCook);
        }
        let Some(recipe) = data.cooking.recipes.get(recipe_id) else {
            return Err(UnknownRecipe);
        };
        if !self.cooking.knows(self.cooking.recipe) {
            return Err(UnknownRecipe);
        }
        if self.cooking.full {
            return Err(Full);
        }
        if !self.has_ingredients(data, self.cooking.recipe) {
            return Err(MissingIngredients);
        }
        let grade = usize::from(member.cooking[recipe_id] / 3);
        let extra = &recipe.cooks[chef].grades[grade];
        let available_extras = extra
            .extras
            .iter()
            .filter(|&&v| self.ingredient_count(data, v) > 0)
            .count();
        let chance = 100 + i32::from(member.stats(data).luck) / 2
            - [15, 10, 5][grade]
            - recipe.required.len() as i32
            - available_extras as i32;
        let mut meal = Meal {
            success: (random() % 100) as i32 <= chance,
            ingredients: Vec::new(),
            effects: extra
                .effects
                .iter()
                .copied()
                .map(|effect| {
                    (
                        effect,
                        if effect <= MealEffect::AttackBoost {
                            u16::from(extra.recovery)
                        } else {
                            0
                        },
                    )
                })
                .collect(),
        };
        for &ingredient in recipe.required.iter().chain(&extra.extras) {
            let id = match ingredient {
                Ingredient::None => continue,
                Ingredient::Item(id) => id,
                Ingredient::Any(group) => {
                    let owned: Vec<_> = data.cooking.groups[usize::from(group)]
                        .items
                        .iter()
                        .copied()
                        .filter(|id| self.items.contains_key(id))
                        .collect();
                    if owned.is_empty() {
                        continue;
                    }
                    owned[random() as usize % owned.len()]
                }
            };
            let Some(count) = self.items.get_mut(&id) else {
                continue;
            };
            *count -= 1;
            if *count == 0 {
                self.items.remove(&id);
            }
            meal.ingredients.push(id);
            let (mask, amount) = match data.items[usize::from(id)].category {
                7 => (1, 5),
                9 => (2, 5),
                10 => (3, 3),
                11 => (1, 3),
                12 if !(123..=126).contains(&id) => (3, 3),
                _ => (0, 0),
            };
            for (bit, effect) in [(1, MealEffect::HpRecovery), (2, MealEffect::TpRecovery)] {
                if mask & bit != 0 {
                    *meal.effects.entry(effect).or_default() += amount;
                }
            }
        }
        let roll = random() % 100;
        let percent = if meal.success {
            if roll < 50 {
                100
            } else if roll < 85 {
                105
            } else {
                110
            }
        } else if roll < 15 {
            0
        } else if roll < 85 {
            20
        } else {
            50
        };
        let skill = &mut self.members[chef].cooking[recipe_id];
        *skill = (*skill + if meal.success { 1 } else { 2 }).min(8);
        if percent == 0 {
            meal.effects.clear();
        } else {
            let bonus = u16::from(
                self.formation.contains(&3)
                    && self.members[2]
                        .ex_skills
                        .contains(&data.cooking.bonus_skill),
            ) * 5;
            for amount in meal.effects.values_mut() {
                *amount = ((*amount + bonus) * percent + 50) / 100;
            }
        }
        for &id in &self.formation {
            let index = usize::from(id - 1);
            let member = &mut self.members[index];
            let preferences = &data.cooking.preferences[index];
            for (foods, change) in [(&preferences.likes, 5), (&preferences.dislikes, -5)] {
                if foods.iter().any(|food| meal.ingredients.contains(food)) {
                    member.overlimit = (i16::from(member.overlimit) + change).clamp(0, 100) as u8;
                    if chef == 0 {
                        member.affinity =
                            (member.affinity + i32::from(change / 5)).clamp(-10000, 10000);
                    }
                }
            }
            if index == chef {
                member.overlimit = (i16::from(member.overlimit) + if meal.success { 5 } else { -5 })
                    .clamp(0, 100) as u8;
            }
            let [hp, tp] = member.maximum_vitals();
            for (&effect, &amount) in &meal.effects {
                use MealEffect::*;
                match effect {
                    HpRecovery if !member.knocked_out() => {
                        recover(&mut member.hp, hp, amount);
                    }
                    TpRecovery if !member.knocked_out() => {
                        recover(&mut member.tp, tp, amount);
                    }
                    CurePoison => member.conditions &= !0x60,
                    CureParalysis => member.conditions &= !0x80,
                    CurePetrify => member.conditions &= !0x100,
                    CureCurse => member.conditions &= !0x200,
                    Revive if member.knocked_out() => {
                        member.conditions &= !REVIVAL_CLEARS;
                        recover(&mut member.hp, hp, 35);
                    }
                    AttackBoost => member.conditions |= 0x1000,
                    DefenseBoost => member.conditions |= 0x4000,
                    AccuracyBoost => member.conditions |= 0x10000,
                    MagicAttackBoost => member.conditions |= 0x40000,
                    MagicDefenseBoost => member.conditions |= 0x100000,
                    _ => {}
                }
            }
        }
        self.cooking.full = true;
        Ok(meal)
    }
}
