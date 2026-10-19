use super::*;
use resonance_content::menu_data::{MealEffect, MenuData, RECIPE_COUNT, RECIPE_ROWS};
use resonance_events::party::{CookingError, Meal};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Focus {
    #[default]
    Header,
    Cooks,
    Recipes,
}
#[derive(Debug, serde::Serialize)]
pub enum Content {
    Meal(Meal),
    Notice(Notice),
}
impl Content {
    pub fn text(&self, data: &MenuData, recipe: u8) -> anyhow::Result<(String, Vec<String>)> {
        let captions = data.cooking_text()?;
        match self {
            Self::Notice(notice) => Ok((captions.label(notice.label())?.into(), vec![])),
            Self::Meal(meal) => {
                let name = &captions.recipe(usize::from(recipe))?.name;
                let title = format!(
                    "{}{}{}",
                    captions.label("result_join")?,
                    name,
                    captions.label(if meal.success { "success" } else { "failure" })?
                );
                let mut effects: Vec<_> = meal
                    .effects
                    .iter()
                    .map(|(&effect, amount)| {
                        let label = captions.effect(effect)?;
                        if matches!(effect, MealEffect::HpRecovery | MealEffect::TpRecovery) {
                            Ok(format!("{label} {amount}%"))
                        } else {
                            Ok(label.to_owned())
                        }
                    })
                    .collect::<anyhow::Result<_>>()?;
                if effects.is_empty() {
                    effects.push(captions.label("no_effect")?.into());
                }
                Ok((title, effects))
            }
        }
    }
}
#[derive(Debug, serde::Serialize)]
pub enum Notice {
    #[serde(rename = "full")]
    Full,
    #[serde(rename = "missing")]
    MissingIngredients,
    #[serde(rename = "unknown")]
    UnknownRecipe,
}
impl Notice {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::MissingIngredients => "missing",
            Self::UnknownRecipe => "unknown",
        }
    }
}
#[derive(Debug, serde::Serialize)]
pub struct Popup {
    pub content: Content,
    pub active: bool,
    pub opacity: u8,
}
#[derive(Debug, Default, serde::Serialize)]
pub struct Cooking {
    #[serde(flatten)]
    pub transition: super::Transition,
    pub focus: Focus,
    pub choose_recipe: bool,
    pub chef_slot: usize,
    pub recipe: usize,
    pub first: usize,
    pub scroll: i8,
    pub description_previous: usize,
    pub description_fade: u8,
    /// The outgoing recipe is drawn before the fade counter advances.
    pub description_opacity: u8,
    pub popup: Option<Popup>,
}
impl Cooking {
    pub fn header_cursor(&self) -> [f32; 2] {
        [
            28. - (u32::from(self.transition.page_fade) * 402 / 256) as f32,
            if self.choose_recipe { 92. } else { 68. },
        ]
    }
    fn fade(&mut self) {
        if let Some(popup) = &mut self.popup {
            popup.opacity = if popup.active {
                popup.opacity.saturating_add(32)
            } else {
                popup.opacity.saturating_sub(32)
            };
            if popup.opacity == 0 {
                self.popup = None;
            }
        }
    }
    fn show(&mut self, content: Content) {
        let opacity = self.popup.as_ref().map_or(0, |p| {
            // A departing meal fades once before transferring to a notice.
            if matches!(p.content, Content::Meal(_)) && matches!(content, Content::Notice(_)) {
                p.opacity.saturating_sub(32)
            } else {
                p.opacity
            }
        });
        self.popup = Some(Popup {
            content,
            active: true,
            opacity,
        });
    }
}
impl Menu {
    pub(super) fn open_cooking(&mut self) {
        let party = &mut self.checkpoint.as_mut().unwrap().progress_mut().party;
        if !party.formation.contains(&(party.cooking.chef + 1)) {
            party.cooking.chef = party.formation[0] - 1;
            self.party_changed = true;
        }
        self.cooking = Cooking {
            transition: super::Transition::opening(),
            choose_recipe: self.cooking.choose_recipe,
            recipe: usize::from(party.cooking.recipe),
            description_fade: DESCRIPTION_FADE_START,
            ..Default::default()
        };
        self.entering = Some(Page::Cooking);
    }
    pub fn cooking_selection(&self) -> (usize, usize) {
        let party = self.party();
        (
            if self.cooking.focus == Focus::Cooks {
                usize::from(party.formation[self.cooking.chef_slot] - 1)
            } else {
                usize::from(party.cooking.chef)
            },
            if self.cooking.focus == Focus::Recipes {
                self.cooking.recipe
            } else {
                usize::from(party.cooking.recipe)
            },
        )
    }
    pub(super) fn remember_cooking_description(&mut self) {
        if self.cooking.description_fade == 0 {
            self.cooking.description_previous = self.cooking_selection().1;
        }
    }
    pub(super) fn fade_cooking_description(&mut self) {
        let changed = self.cooking.description_fade == 0
            && self.cooking.description_previous != self.cooking_selection().1;
        self.cooking.description_opacity =
            255 - fade_description(&mut self.cooking.description_fade, changed);
    }
    pub(super) fn step_cooking(&mut self, input: Input) -> Option<i16> {
        let state = &mut self.cooking;
        state.scroll = (state.scroll + state.scroll.signum()) % 5;
        if state.transition.animating() || state.scroll != 0 {
            return None;
        }
        let cue = self.cooking_input(input);
        self.cooking.fade();
        cue
    }
    fn cooking_input(&mut self, input: Input) -> Option<i16> {
        use MenuAction::*;
        let [left, right, up, down] = [Left, Right, Up, Down].map(|action| input == Some(action));
        if let Some(popup) = &mut self.cooking.popup
            && popup.active
        {
            if matches!(input, Some(Confirm | Cancel)) {
                popup.active = false;
                return Some(2);
            }
            return None;
        }
        if input == Some(Alternate) && self.cooking.focus == Focus::Header {
            let progress = self.checkpoint.as_mut().unwrap().progress_mut();
            let data = &self.resources.as_ref().unwrap().data;
            let (content, cue) = match progress.cook(data) {
                Ok(meal) => {
                    self.party_changed = true;
                    (Content::Meal(meal), 2)
                }
                Err(CookingError::UnavailableCook) => return Some(4),
                Err(error) => (
                    Content::Notice(match error {
                        CookingError::Full => Notice::Full,
                        CookingError::MissingIngredients => Notice::MissingIngredients,
                        CookingError::UnknownRecipe => Notice::UnknownRecipe,
                        CookingError::UnavailableCook => unreachable!(),
                    }),
                    4,
                ),
            };
            self.cooking.show(content);
            return Some(cue);
        }
        let state = &mut self.cooking;
        let progress = self.checkpoint.as_mut().unwrap().progress_mut();
        let party = &mut progress.party;
        if input == Some(Cancel) {
            if state.focus == Focus::Header {
                state.transition.page_closing = true;
                self.select_main(Page::Cooking);
            } else {
                state.focus = Focus::Header;
            }
            return Some(3);
        }
        match state.focus {
            Focus::Header => {
                if input == Some(Confirm) {
                    state.focus = if state.choose_recipe {
                        Focus::Recipes
                    } else {
                        Focus::Cooks
                    };
                    state.recipe = usize::from(party.cooking.recipe);
                    state.first = state
                        .first
                        .min(state.recipe / 2 * 2)
                        .max((state.recipe / 2).saturating_sub(RECIPE_ROWS / 2 - 1) * 2);
                    state.chef_slot = party
                        .formation
                        .iter()
                        .position(|&id| id == party.cooking.chef + 1)
                        .unwrap();
                    return Some(2);
                }
                if up || down {
                    state.choose_recipe = !state.choose_recipe;
                    return Some(1);
                }
            }
            Focus::Cooks => {
                if input == Some(Confirm) {
                    party.cooking.chef = party.formation[state.chef_slot] - 1;
                    self.party_changed = true;
                    state.focus = Focus::Header;
                    return Some(2);
                }
                let old = state.chef_slot;
                let step = if up || down { 4 } else { 1 };
                if left || up {
                    state.chef_slot = old.checked_sub(step).unwrap_or(old);
                } else if (right || down) && old + step < party.formation.len() {
                    state.chef_slot = old + step;
                }
                return (state.chef_slot != old).then_some(1);
            }
            Focus::Recipes => {
                if input == Some(Confirm) {
                    if !party.cooking.knows(state.recipe as u8) {
                        state.show(Content::Notice(Notice::UnknownRecipe));
                        return Some(4);
                    }
                    party.cooking.recipe = state.recipe as u8;
                    self.party_changed = true;
                    state.focus = Focus::Header;
                    return Some(2);
                }
                let old = state.recipe;
                if matches!(input, Some(PreviousTab | PageUp)) {
                    let step = state.first.min(RECIPE_ROWS);
                    state.first -= step;
                    state.recipe -= step;
                } else if matches!(input, Some(NextTab | PageDown)) {
                    if state.first + RECIPE_ROWS < RECIPE_COUNT {
                        state.first += RECIPE_ROWS;
                        state.recipe = (old + RECIPE_ROWS).min(RECIPE_COUNT - 1);
                    }
                } else {
                    let first = state.first;
                    let step = if up || down { 2 } else { 1 };
                    if left || up {
                        state.recipe = old.checked_sub(step).unwrap_or(old);
                    } else if (right || down) && old + step < RECIPE_COUNT {
                        state.recipe = old + step;
                    }
                    state.first = state
                        .first
                        .min(state.recipe / 2 * 2)
                        .max((state.recipe / 2).saturating_sub(RECIPE_ROWS / 2 - 1) * 2);
                    state.scroll = (state.first as i32 - first as i32).signum() as i8;
                }
                return (state.recipe != old).then_some(
                    if matches!(input, Some(PreviousTab | NextTab | PageUp | PageDown)) {
                        38
                    } else {
                        1
                    },
                );
            }
        }
        None
    }
}
