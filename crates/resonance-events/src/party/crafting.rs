//! Atomic inventory exchanges, independent of menu selection and focus.
use super::{Party, SessionData};
use resonance_content::menu_data::crafting::Recipe;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftError {
    MissingMaterials,
    InventoryFull,
}

impl Party {
    pub fn check_recipe(&self, data: &SessionData, recipe: &Recipe) -> Result<(), CraftError> {
        let required = &recipe.ingredients;
        if required
            .iter()
            .any(|(id, &count)| u16::from(self.items.get(id).copied().unwrap_or(0)) < count)
        {
            return Err(CraftError::MissingMaterials);
        }
        let result_count = u16::from(self.items.get(&recipe.result).copied().unwrap_or(0))
            - required.get(&recipe.result).copied().unwrap_or(0);
        if result_count >= u16::from(data.items[usize::from(recipe.result)].stack_limit) {
            return Err(CraftError::InventoryFull);
        }
        Ok(())
    }
    pub fn craft(&mut self, data: &SessionData, recipe: &Recipe) -> Result<(), CraftError> {
        self.check_recipe(data, recipe)?;
        for (&id, &count) in &recipe.ingredients {
            let remaining = u16::from(self.items[&id]) - count;
            if remaining == 0 {
                self.items.remove(&id);
            } else {
                self.items.insert(id, remaining as u8);
            }
        }
        self.change_item(data, recipe.result, 1)
            .expect("validated crafting result");
        Ok(())
    }
}
