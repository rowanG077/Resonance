//! Optional menu presentation sections, separate from numeric gameplay records.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedText {
    pub name: String,
    pub description: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemText {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub details: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemCaptions {
    #[serde(default)]
    pub items: Vec<Option<ItemText>>,
    pub item_categories: Vec<String>,
    pub inventory_categories: Vec<String>,
    pub item_group_prompt: Option<MenuText>,
    pub item_bottle_count: Option<MenuText>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyText {
    #[serde(default)]
    pub groups: [Vec<Option<ItemText>>; 3],
    pub presets: Option<[String; 3]>,
    pub keyboard: Option<String>,
    pub keys: Option<[String; 9]>,
    pub labels: Option<[String; 3]>,
}
fn decode_caption<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    scope: &str,
    diagnostics: &crate::diagnostics::Diagnostics,
) -> Result<Option<T>> {
    diagnostics.attempt(
        scope,
        serde_json::from_value(value).map_err(anyhow::Error::from),
    )
}
fn take(value: &mut serde_json::Value, key: &str) -> serde_json::Value {
    value
        .as_object_mut()
        .and_then(|object| object.remove(key))
        .unwrap_or_default()
}
fn decode_rows<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    scope: &str,
    diagnostics: &crate::diagnostics::Diagnostics,
) -> Result<Vec<Option<T>>> {
    let rows =
        decode_caption::<Vec<serde_json::Value>>(value, scope, diagnostics)?.unwrap_or_default();
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| decode_caption(row, &format!("{scope} {index}"), diagnostics))
        .collect()
}
impl ItemCaptions {
    fn decode(
        mut value: serde_json::Value,
        diagnostics: &crate::diagnostics::Diagnostics,
    ) -> Result<Option<Self>> {
        let rows = take(&mut value, "items");
        let rows = if rows.is_null() {
            Vec::new()
        } else {
            decode_rows::<ItemText>(rows, "item caption", diagnostics)?
        };
        let [prompt, count] = ["item_group_prompt", "item_bottle_count"].map(|key| {
            value
                .as_object_mut()
                .and_then(|object| object.remove(key))
                .unwrap_or_default()
        });
        let Some(mut text) = decode_caption::<Self>(value, "menu items captions", diagnostics)?
        else {
            return Ok(None);
        };
        text.items = rows;
        text.item_group_prompt = decode_caption(prompt, "item group prompt", diagnostics)?;
        text.item_bottle_count = decode_caption(count, "item bottle count", diagnostics)?;
        Ok(Some(text))
    }
}
impl StrategyText {
    pub fn keyboard(&self) -> Result<&str> {
        self.keyboard
            .as_deref()
            .context("Strategy keyboard was not prepared")
    }
    pub fn keys(&self) -> Result<&[String; 9]> {
        self.keys
            .as_ref()
            .context("Strategy keyboard labels were not prepared")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookingText {
    #[serde(default)]
    pub recipes: Vec<Option<NamedText>>,
    pub groups: Vec<String>,
    pub effects: Option<[String; 12]>,
    pub labels: BTreeMap<String, String>,
}
impl CookingText {
    pub fn recipe(&self, id: usize) -> Result<&NamedText> {
        self.recipes
            .get(id)
            .and_then(Option::as_ref)
            .context("recipe caption was not prepared")
    }
    pub fn effect(&self, effect: MealEffect) -> Result<&str> {
        self.effects
            .as_ref()
            .map(|effects| effects[effect as usize].as_str())
            .context("cooking effect caption was not prepared")
    }
    pub fn label(&self, key: &str) -> Result<&str> {
        required_label(&self.labels, key)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusText {
    pub conditions: Option<ConditionText>,
    #[serde(default)]
    pub equipment_effects: BTreeMap<String, String>,
    pub technical_type: String,
    pub strike_type: String,
}
impl StatusText {
    fn decode(
        mut value: serde_json::Value,
        diagnostics: &crate::diagnostics::Diagnostics,
    ) -> Result<Option<Self>> {
        let conditions = take(&mut value, "conditions");
        let Some(mut text) = decode_caption::<Self>(value, "Status captions", diagnostics)? else {
            return Ok(None);
        };
        text.conditions = decode_caption(conditions, "Status condition captions", diagnostics)?;
        Ok(Some(text))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionText {
    pub poison: String,
    pub severe_poison: String,
    pub paralysis: String,
    pub petrified: String,
    pub curse: String,
    pub attack_up: String,
    pub attack_down: String,
    pub defense_up: String,
    pub defense_down: String,
    pub accuracy_up: String,
    pub accuracy_down: String,
    pub magic_attack_up: String,
    pub magic_attack_down: String,
    pub magic_defense_up: String,
    pub knockout: String,
}
impl ConditionText {
    fn texts(&self) -> [&str; 15] {
        [
            &self.poison,
            &self.severe_poison,
            &self.paralysis,
            &self.petrified,
            &self.curse,
            &self.attack_up,
            &self.attack_down,
            &self.defense_up,
            &self.defense_down,
            &self.accuracy_up,
            &self.accuracy_down,
            &self.magic_attack_up,
            &self.magic_attack_down,
            &self.magic_defense_up,
            &self.knockout,
        ]
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldMapText {
    #[serde(with = "crate::ui_art::worlds")]
    pub names: [String; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExCaption {
    pub name: String,
    pub description: MenuText,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExSkillText {
    pub skills: BTreeMap<u8, ExCaption>,
    pub activation_labels: BTreeMap<ExActivation, String>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MenuPresentation {
    pub labels: BTreeMap<String, String>,
    pub items: Option<ItemCaptions>,
    pub titles: Option<Vec<Vec<Option<NamedText>>>>,
    pub techniques: Option<Vec<Option<NamedText>>>,
    pub names: Option<Vec<String>>,
    pub strategy: Option<StrategyText>,
    pub cooking: Option<CookingText>,
    pub status: Option<StatusText>,
    pub world_map: Option<WorldMapText>,
    pub shops: Option<Vec<String>>,
    pub ex_skills: Option<ExSkillText>,
    pub monsters: Option<crate::monster::MonsterBook>,
}
impl MenuPresentation {
    pub(super) fn decode(value: serde_json::Value, files: &crate::prepared::Files) -> Result<Self> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Source {
            labels: serde_json::Value,
            items: serde_json::Value,
            status: serde_json::Value,
            world_map: serde_json::Value,
            shops: serde_json::Value,
            monsters: serde_json::Value,
        }
        let diagnostics = files.diagnostics();
        let source: Source = diagnostics
            .attempt(
                "menu presentation",
                serde_json::from_value(value).map_err(anyhow::Error::from),
            )?
            .unwrap_or_default();
        Ok(Self {
            labels: crate::menu::decode_labels(source.labels, diagnostics, "menu label")?,
            items: ItemCaptions::decode(source.items, diagnostics)?,
            status: StatusText::decode(source.status, diagnostics)?,
            world_map: decode_caption(source.world_map, "world map captions", diagnostics)?,
            shops: decode_caption(source.shops, "shop captions", diagnostics)?,
            monsters: diagnostics.attempt(
                "monster directory",
                crate::monster::MonsterBook::decode_named(source.monsters, files),
            )?,
            ..Self::default()
        })
    }
    pub(super) fn validate(&self, data: &MenuData) -> Result<()> {
        for page in MenuPage::ALL {
            data.validate_page(page)?;
        }
        data.item_group_prompt()?;
        data.item_bottle_count()?;
        ensure!(
            self.shops
                .as_ref()
                .context("shop captions are unavailable")?
                .len()
                == data.world_map.shops.len(),
            "invalid shop caption bindings"
        );
        for caption in data.ex_skill_text()?.skills.values() {
            caption.description.validate()?;
            ensure!(!caption.name.is_empty(), "missing EX skill name");
        }
        Ok(())
    }
    pub(super) fn texts(&self) -> impl Iterator<Item = &str> {
        let mut texts: Vec<&str> = self.labels.values().map(String::as_str).collect();
        if let Some(data) = &self.items {
            for row in data.items.iter().flatten() {
                texts.extend([row.name.as_str(), &row.description, &row.details]);
            }
            texts.extend(
                data.item_categories
                    .iter()
                    .chain(&data.inventory_categories)
                    .map(String::as_str),
            );
            texts.extend(data.item_group_prompt.iter().flat_map(MenuText::texts));
            texts.extend(data.item_bottle_count.iter().flat_map(MenuText::texts));
        }
        for row in self
            .titles
            .iter()
            .flatten()
            .flatten()
            .chain(self.techniques.iter().flatten())
            .flatten()
        {
            texts.extend([row.name.as_str(), &row.description]);
        }
        texts.extend(self.names.iter().flatten().map(String::as_str));
        if let Some(data) = &self.strategy {
            for row in data.groups.iter().flatten().flatten() {
                texts.extend([row.name.as_str(), &row.description, &row.details]);
            }
            texts.extend(
                data.presets
                    .iter()
                    .flatten()
                    .chain(data.keys.iter().flatten())
                    .chain(data.labels.iter().flatten())
                    .chain(data.keyboard.iter())
                    .map(String::as_str),
            );
        }
        if let Some(data) = &self.cooking {
            for row in data.recipes.iter().flatten() {
                texts.extend([row.name.as_str(), &row.description]);
            }
            texts.extend(
                data.groups
                    .iter()
                    .chain(data.effects.iter().flatten())
                    .chain(data.labels.values())
                    .map(String::as_str),
            );
        }
        if let Some(data) = &self.status {
            texts.extend(
                data.conditions
                    .iter()
                    .flat_map(ConditionText::texts)
                    .chain(data.equipment_effects.values().map(String::as_str))
                    .chain([data.technical_type.as_str(), &data.strike_type]),
            );
        }
        if let Some(data) = &self.world_map {
            texts.extend(data.names.iter().map(String::as_str));
        }
        texts.extend(self.shops.iter().flatten().map(String::as_str));
        if let Some(data) = &self.ex_skills {
            for row in data.skills.values() {
                texts.push(&row.name);
                texts.extend(row.description.texts());
            }
            texts.extend(
                data.labels
                    .values()
                    .chain(data.activation_labels.values())
                    .map(String::as_str),
            );
        }
        if let Some(data) = &self.monsters {
            texts.extend(data.texts());
        }
        texts.into_iter()
    }
}
pub(super) fn required_label<'a>(
    labels: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str> {
    let text = labels
        .get(key)
        .with_context(|| format!("missing menu label {key}"))?;
    ensure!(
        !text.is_empty()
            && text.len() <= 4096
            && text.chars().all(|c| !c.is_control() || c == '\n'),
        "invalid menu label {key}"
    );
    Ok(text)
}

/// Complete caption groups required when publishing menu data.
#[derive(Debug, Clone, Copy)]
enum MenuPage {
    Items,
    Equipment,
    Techniques,
    Titles,
    Status,
    Cooking,
    ExSkills,
    Strategy,
    WorldMap,
    Monsters,
    Collection,
    Unison,
    Party,
}
impl MenuPage {
    const ALL: [Self; 13] = [
        Self::Items,
        Self::Equipment,
        Self::Techniques,
        Self::Titles,
        Self::Status,
        Self::Cooking,
        Self::ExSkills,
        Self::Strategy,
        Self::WorldMap,
        Self::Monsters,
        Self::Collection,
        Self::Unison,
        Self::Party,
    ];

    fn required_labels(self) -> &'static [&'static str] {
        match self {
            Self::Items => &[
                "item_slash",
                "item_thrust",
                "item_defense",
                "item_accuracy",
                "item_evasion",
                "item_intelligence",
                "item_luck",
                "item_attack",
                "strength",
                "defense",
                "luck",
                "accuracy",
                "evasion",
                "intelligence",
                "discard",
                "transformed",
                "discarded",
                "confirm_discard",
                "select_item",
                "select_target",
                "equip_target",
                "transform_full",
                "transform_empty",
                "holy_aura",
                "dark_aura",
            ],
            Self::Equipment => &[
                "weapon",
                "body",
                "head",
                "arm",
                "accessory_1",
                "accessory_2",
                "slash",
                "attack",
                "thrust",
                "defense",
                "accuracy",
                "evasion",
                "intelligence",
                "luck",
                "stat_arrow",
                "optimal",
                "remove",
                "change_order",
                "optimal_selection",
                "optimal_slash",
                "optimal_thrust",
                "alphabetical",
                "parameter",
            ],
            Self::Techniques => &[
                "tech_usage",
                "tech_remove",
                "tech_auto",
                "tech_execute",
                "tech_forget",
                "tech_unison",
                "tech_control",
                "tech_manual",
                "tech_semi_auto",
                "tech_auto_mode",
                "tech_select",
                "tech_shortcut",
                "tech_unison_title",
                "tech_strength",
                "tech_slash",
                "tech_thrust",
                "tech_defense",
                "tech_luck",
                "tech_accuracy",
                "tech_evasion",
                "tech_intelligence",
                "tech_attack",
                "tech_target",
                "tech_target_all",
                "tech_cannot_forget",
                "tech_related",
                "tech_forget_warning",
                "tech_forget_confirm",
            ],
            Self::Titles => &[
                "growth",
                "growth_hp",
                "growth_tp",
                "growth_strength",
                "growth_defense",
                "growth_intelligence",
                "growth_evasion",
                "growth_accuracy",
            ],
            Self::Status => &[
                "status",
                "next",
                "strength",
                "defense",
                "slash",
                "accuracy",
                "attack",
                "thrust",
                "evasion",
                "intelligence",
                "luck",
                "weapon",
                "body",
                "head",
                "arm",
                "accessory_1",
                "accessory_2",
                "element_attack",
                "element_defense",
                "weak",
                "absorb",
                "invalid",
                "reduce",
            ],
            Self::Strategy => &[
                "strategy_title",
                "strategy_orders",
                "strategy_rename",
                "strategy_default",
            ],
            Self::Unison => &["unison_title", "unison_player", "tech_usage"],
            Self::Party => &[
                "party_slash",
                "party_thrust",
                "party_attack",
                "party_defense",
                "party_luck",
                "party_accuracy",
                "party_evasion",
                "party_swap_target",
                "party_leader",
                "party_swap",
            ],
            Self::Collection => &["collectors_book"],
            Self::Monsters => &["preview_loading"],
            Self::ExSkills => &["stat_arrow"],
            Self::Cooking | Self::WorldMap => &[],
        }
    }
}

impl MenuData {
    /// Publishing requires complete presentation catalogues; runtime reads selected entries.
    fn validate_page(&self, page: MenuPage) -> Result<()> {
        match page {
            MenuPage::Items | MenuPage::Equipment | MenuPage::Collection => {
                let text = self.items_text()?;
                ensure!(
                    text.items.len() == self.items.len()
                        && text.items.iter().all(Option::is_some)
                        && text.item_categories.len() == 48
                        && text.inventory_categories.len() == 9,
                    "invalid item caption bindings"
                );
            }
            MenuPage::Techniques | MenuPage::Unison => {
                let text = self.technique_texts()?;
                ensure!(
                    text.len() == self.techniques.len() && text.iter().skip(1).all(Option::is_some),
                    "invalid technique caption bindings"
                );
            }
            MenuPage::Titles => {
                self.validate_page(MenuPage::Status)?;
            }
            MenuPage::Status => {
                self.items_text()?;
                let text = self.title_texts()?;
                ensure!(
                    text.len() == self.titles.len()
                        && text
                            .iter()
                            .zip(&self.titles)
                            .all(|(text, rules)| text.len() == rules.len()
                                && text.iter().all(Option::is_some)),
                    "invalid title caption bindings"
                );
                let text = self.full_names()?;
                ensure!(
                    text.len() == self.initial_names.len(),
                    "invalid character name caption bindings"
                );
                self.status.validate(&self.items)?;
                let text = self.status_text()?;
                ensure!(text.conditions.is_some(), "missing Status conditions");
                ensure!(
                    self.status
                        .equipment_effects
                        .keys()
                        .all(|key| text.equipment_effects.contains_key(key)),
                    "invalid Status caption bindings"
                );
            }
            MenuPage::Cooking => {
                self.items_text()?;
                let text = self.cooking_text()?;
                ensure!(
                    text.recipes.len() == self.cooking.recipes.len()
                        && text.recipes.iter().all(Option::is_some)
                        && text.effects.is_some()
                        && text.groups.len() == self.cooking.groups.len(),
                    "invalid Cooking caption bindings"
                );
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
                    "result_join",
                    "locked",
                ] {
                    required_label(&text.labels, key).context("Cooking captions")?;
                }
            }
            MenuPage::ExSkills => {
                let text = self.ex_skill_text()?;
                ensure!(
                    self.ex_skills
                        .skills
                        .keys()
                        .all(|key| text.skills.contains_key(key)),
                    "invalid EX caption bindings"
                );
                for key in [
                    "title",
                    "set_gem",
                    "replace_gem",
                    "yes",
                    "no",
                    "hp",
                    "tp",
                    "slash",
                    "thrust",
                    "defense",
                    "accuracy",
                    "evasion",
                    "intelligence",
                    "luck",
                    "attack",
                    "gem_max",
                    "gem_level",
                    "gem_empty",
                ] {
                    required_label(&text.labels, key).context("EX captions")?;
                }
                for skill in self.ex_skills.skills.values() {
                    ensure!(
                        text.activation_labels
                            .get(&skill.activation)
                            .is_some_and(|text| !text.is_empty()),
                        "missing EX activation caption"
                    );
                }
            }
            MenuPage::Strategy => {
                self.strategy.validate_rules()?;
                let text = self.strategy_text()?;
                ensure!(
                    text.groups.iter().flatten().all(Option::is_some),
                    "missing Strategy option captions"
                );
                ensure!(
                    text.groups
                        .iter()
                        .zip(STRATEGY_COUNTS)
                        .all(|(group, count)| group.len() == count)
                        && text.keyboard()?.len() == 90
                        && text.keyboard()?.is_ascii(),
                    "invalid Strategy captions"
                );
                text.keys()?;
                text.labels
                    .as_ref()
                    .context("Strategy group labels were not prepared")?;
                self.strategy_presets()?;
            }
            MenuPage::WorldMap => {
                self.world_map.validate(self.items.len())?;
                let text = self.world_map_text()?;
                ensure!(
                    text.names.iter().all(|name| !name.is_empty()),
                    "invalid world map names"
                );
            }
            MenuPage::Monsters => {
                self.monsters()?.validate(self.items.len())?;
            }
            MenuPage::Party => {}
        }
        for &key in page.required_labels() {
            self.label(key)?;
        }
        Ok(())
    }

    pub fn items_text(&self) -> Result<&ItemCaptions> {
        let text = self
            .presentation
            .items
            .as_ref()
            .context("item captions are unavailable")?;
        Ok(text)
    }
    pub fn title_texts(&self) -> Result<&[Vec<Option<NamedText>>]> {
        let text = self
            .presentation
            .titles
            .as_ref()
            .context("title captions are unavailable")?;
        Ok(text)
    }
    pub fn technique_texts(&self) -> Result<&[Option<NamedText>]> {
        let text = self
            .presentation
            .techniques
            .as_ref()
            .context("technique captions are unavailable")?;
        Ok(text)
    }
    pub fn full_names(&self) -> Result<&[String]> {
        let text = self
            .presentation
            .names
            .as_ref()
            .context("character name captions are unavailable")?;
        Ok(text)
    }
    pub fn item_text(&self, id: u16) -> Result<&ItemText> {
        self.presentation
            .items
            .as_ref()
            .context("item captions are unavailable")?
            .items
            .get(usize::from(id))
            .and_then(Option::as_ref)
            .context("unknown item caption")
    }
    pub fn technique_text(&self, id: u16) -> Result<&NamedText> {
        self.presentation
            .techniques
            .as_ref()
            .context("technique captions are unavailable")?
            .get(usize::from(id))
            .and_then(Option::as_ref)
            .context("unknown technique caption")
    }
    pub fn title_text(&self, character: u8, title: u8) -> Result<&NamedText> {
        self.presentation
            .titles
            .as_ref()
            .context("title captions are unavailable")?
            .get(usize::from(
                character
                    .checked_sub(1)
                    .context("invalid title character")?,
            ))
            .and_then(|titles| {
                title
                    .checked_sub(1)
                    .and_then(|id| titles.get(usize::from(id)))
            })
            .and_then(Option::as_ref)
            .context("unknown title caption")
    }
    pub fn shop_text(&self, id: u8) -> Result<&str> {
        self.presentation
            .shops
            .as_ref()
            .context("shop captions are unavailable")?
            .get(usize::from(id))
            .map(String::as_str)
            .context("unknown shop caption")
    }
    pub fn item_group_prompt(&self) -> Result<&[MenuSpan]> {
        self.items_text()?
            .item_group_prompt
            .as_ref()
            .context("item group prompt was not prepared")?
            .single_line()
    }
    pub fn item_bottle_count(&self) -> Result<&[MenuSpan]> {
        let spans = self
            .items_text()?
            .item_bottle_count
            .as_ref()
            .context("item bottle count was not prepared")?
            .single_line()?;
        ensure!(
            spans
                .iter()
                .all(|span| matches!(span, MenuSpan::Text { .. })),
            "button in bottle count"
        );
        Ok(spans)
    }
    pub fn strategy_text(&self) -> Result<&StrategyText> {
        let text = self
            .presentation
            .strategy
            .as_ref()
            .context("Strategy captions are unavailable")?;
        Ok(text)
    }
    pub fn strategy_option(&self, group: usize, option: usize) -> Result<&ItemText> {
        self.strategy_text()?
            .groups
            .get(group)
            .and_then(|group| group.get(option))
            .and_then(Option::as_ref)
            .context("Strategy option caption was not prepared")
    }
    pub fn strategy_presets(&self) -> Result<[StrategyPreset; 3]> {
        let text = self.strategy_text()?;
        let names = text
            .presets
            .as_ref()
            .context("Strategy presets were not prepared")?;
        let presets = std::array::from_fn(|index| StrategyPreset {
            name: names[index].clone(),
            members: self.strategy.presets[index],
        });
        for preset in &presets {
            preset.validate()?;
        }
        Ok(presets)
    }
    pub fn cooking_text(&self) -> Result<&CookingText> {
        let text = self
            .presentation
            .cooking
            .as_ref()
            .context("Cooking captions are unavailable")?;
        Ok(text)
    }
    pub fn status_text(&self) -> Result<&StatusText> {
        let text = self
            .presentation
            .status
            .as_ref()
            .context("Status captions are unavailable")?;
        Ok(text)
    }
    pub fn world_map_text(&self) -> Result<&WorldMapText> {
        let text = self
            .presentation
            .world_map
            .as_ref()
            .context("world map captions are unavailable")?;
        Ok(text)
    }
    pub fn ex_skill_text(&self) -> Result<&ExSkillText> {
        let text = self
            .presentation
            .ex_skills
            .as_ref()
            .context("EX captions are unavailable")?;
        Ok(text)
    }
    pub fn monsters(&self) -> Result<&crate::monster::MonsterBook> {
        self.presentation
            .monsters
            .as_ref()
            .context("monster directory is unavailable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostics;

    #[test]
    fn malformed_caption_entries_and_controls_are_local_and_paranoid_admission_rejects_them()
    -> Result<()> {
        let row =
            serde_json::json!({"name":"name", "description":"description", "details":"details"});
        let mut healthy = serde_json::json!({
            "labels": {"healthy": "text"},
            "items": {"items":[row], "item_categories":[], "inventory_categories":[], "item_group_prompt":"Use on whom?", "item_bottle_count":"Remaining: %d"},
            "status": {"conditions":{
                "poison":"condition", "severe_poison":"condition", "paralysis":"condition", "petrified":"condition", "curse":"condition",
                "attack_up":"condition", "attack_down":"condition", "defense_up":"condition", "defense_down":"condition", "accuracy_up":"condition", "accuracy_down":"condition",
                "magic_attack_up":"condition", "magic_attack_down":"condition", "magic_defense_up":"condition", "knockout":"condition"
            }, "equipment_effects":{}, "technical_type":"T", "strike_type":"S"},
            "world_map": {"names":{"sylvarant":"one","tethe_alla":"two"}}, "shops":["shop"],
            "monsters": {"records":[], "labels":{}}
        });
        // Preserve catalogue IDs after a failed entry; the next record stays at index 1.
        let rows = healthy["items"]["items"].as_array_mut().unwrap();
        rows.push(rows[0].clone());
        let decode = |value, diagnostics: &Diagnostics| {
            MenuPresentation::decode(value, &crate::prepared::Files::new(diagnostics.clone()))
        };
        let mut expected_healthy = healthy.clone();
        for key in [
            "titles",
            "techniques",
            "names",
            "strategy",
            "cooking",
            "ex_skills",
        ] {
            expected_healthy[key] = serde_json::Value::Null;
        }
        assert_eq!(
            serde_json::to_value(decode(healthy.clone(), &Diagnostics::new(true))?)?,
            expected_healthy
        );
        for (section, pointer) in [
            ("/items/items/0", "/items/items/0/description"),
            ("/status/conditions", "/status/conditions/poison"),
            ("/world_map", "/world_map/names"),
            ("/shops", "/shops/0"),
            ("/monsters", "/monsters/records"),
        ] {
            let mut malformed = healthy.clone();
            *malformed.pointer_mut(pointer).unwrap() = false.into();
            let diagnostics = Diagnostics::new(false);
            let admitted = serde_json::to_value(decode(malformed.clone(), &diagnostics)?)?;
            let mut expected = expected_healthy.clone();
            *expected.pointer_mut(section).unwrap() = serde_json::Value::Null;
            assert_eq!(admitted, expected, "{section}");
            assert_eq!(diagnostics.entries().len(), 1, "{section}");
            assert!(
                decode(malformed, &Diagnostics::new(true)).is_err(),
                "{section}"
            );
        }
        for pointer in [
            "/items/item_group_prompt",
            "/items/item_bottle_count",
            "/status/conditions",
        ] {
            for malformed in [None, Some(serde_json::Value::Bool(false))] {
                let mut document = healthy.clone();
                let (parent, key) = pointer.rsplit_once('/').unwrap();
                document
                    .pointer_mut(parent)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                if let Some(value) = malformed {
                    document.pointer_mut(parent).unwrap()[key] = value;
                }
                let admitted =
                    serde_json::to_value(decode(document.clone(), &Diagnostics::new(false))?)?;
                let mut expected = expected_healthy.clone();
                *expected.pointer_mut(pointer).unwrap() = serde_json::Value::Null;
                assert_eq!(admitted, expected, "{pointer}");
                assert!(
                    decode(document, &Diagnostics::new(true)).is_err(),
                    "{pointer}"
                );
            }
        }
        Ok(())
    }
}
