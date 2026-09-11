use super::*;

pub const RECIPE_COUNT: usize = 24;
pub const RECIPE_ROWS: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MealEffect {
    HpRecovery,
    TpRecovery,
    CurePoison,
    CureParalysis,
    CurePetrify,
    CureCurse,
    Revive,
    AttackBoost,
    DefenseBoost,
    AccuracyBoost,
    MagicAttackBoost,
    MagicDefenseBoost,
}
impl MealEffect {
    pub const ALL: [Self; 12] = [
        Self::HpRecovery,
        Self::TpRecovery,
        Self::CurePoison,
        Self::CureParalysis,
        Self::CurePetrify,
        Self::CureCurse,
        Self::Revive,
        Self::AttackBoost,
        Self::DefenseBoost,
        Self::AccuracyBoost,
        Self::MagicAttackBoost,
        Self::MagicDefenseBoost,
    ];
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ingredient {
    None,
    Item(u16),
    Any(u8),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngredientGroup {
    pub name: String,
    pub category: u8,
    pub items: Vec<u16>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeGrade {
    pub effects: Vec<MealEffect>,
    pub recovery: u8,
    pub extras: Vec<Ingredient>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeCook {
    pub base_stars: u8,
    pub grades: [RecipeGrade; 3],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    pub description: String,
    pub required: Vec<Ingredient>,
    pub cooks: Vec<RecipeCook>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoodPreferences {
    pub likes: Vec<u16>,
    pub dislikes: Vec<u16>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookingData {
    pub recipes: Vec<Recipe>,
    pub groups: Vec<IngredientGroup>,
    pub preferences: Vec<FoodPreferences>,
    pub effects: [String; 12],
    pub labels: BTreeMap<String, String>,
    /// Genis's personal EX skill increases the meal's recovery.
    pub bonus_skill: u8,
}
impl CookingData {
    pub fn validate(&self, items: usize) -> Result<()> {
        let valid_item = |id: &u16| *id > 0 && usize::from(*id) < items;
        let valid_ingredient = |ingredient: &Ingredient| match ingredient {
            Ingredient::None => true,
            Ingredient::Item(id) => valid_item(id),
            Ingredient::Any(group) => usize::from(*group) < self.groups.len(),
        };
        ensure!(
            self.recipes.len() == RECIPE_COUNT
                && self.groups.len() == 32
                && self.preferences.len() == 9
                && self.bonus_skill != 0,
            "invalid cooking catalog"
        );
        ensure!(
            self.groups.iter().all(|group| group.category <= 49
                && !group.items.is_empty()
                && group.items.len() <= 16
                && group.items.iter().all(valid_item)),
            "invalid ingredient group"
        );
        ensure!(
            self.preferences
                .iter()
                .all(|p| p.likes.iter().chain(&p.dislikes).all(valid_item)),
            "invalid food preference"
        );
        for (id, recipe) in self.recipes.iter().enumerate() {
            ensure!(
                !recipe.required.is_empty()
                    && recipe.required.len() <= 3
                    && recipe.required.iter().all(valid_ingredient)
                    && recipe.cooks.len() == 9
                    && recipe
                        .cooks
                        .iter()
                        .all(|cook| (1..=6).contains(&cook.base_stars)
                            && cook.grades.iter().all(|grade| grade.effects.len()
                                <= MealEffect::ALL.len()
                                && grade.recovery <= 100
                                && grade.extras.len() <= 9 - recipe.required.len()
                                && grade.extras.iter().all(valid_ingredient))),
                "invalid recipe {id}: {recipe:?}"
            );
        }
        for key in [
            "cook",
            "required",
            "additional",
            "success",
            "failure",
            "no_effect",
            "missing",
            "full",
            "unknown",
            "locked",
            "result_join",
        ] {
            ensure!(
                self.labels.get(key).is_some_and(|s| !s.is_empty()),
                "missing cooking label {key}"
            );
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.recipes
            .iter()
            .flat_map(|r| [&r.name, &r.description])
            .chain(self.groups.iter().map(|g| &g.name))
            .chain(&self.effects)
            .chain(self.labels.values())
            .map(String::as_str)
    }
}
