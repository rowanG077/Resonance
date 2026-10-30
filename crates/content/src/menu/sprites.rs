use super::*;
use crate::{diagnostics::Diagnostics, library::Mappings, ui_art};
use serde_json::Value;
#[cfg(test)]
mod tests;

pub(super) const COLORS: &[&str] = &[
    "black",
    "blue",
    "red",
    "magenta",
    "green",
    "cyan",
    "yellow",
    "disabled",
    "gold",
    "white",
    "pale_green",
];
const NUMBER_COLORS: &[&str] = &[
    "black",
    "blue",
    "red",
    "magenta",
    "green",
    "cyan",
    "yellow",
    "disabled",
    "gold",
    "white",
    "pale_green",
    "separator",
    "normal",
    "unassigned_13",
    "low",
    "zero",
    "unassigned_16",
    "shadow",
];
const BARS: &[&str] = &["hp", "tp", "shadow"];
const BUTTONS: &[&str] = &[
    "stick_neutral",
    "stick_up",
    "stick_down",
    "stick_right",
    "stick_left",
    "a_raised",
    "a_pressed",
    "b_raised",
    "b_pressed",
    "x_side",
    "x_front",
    "y_side",
    "y_front",
    "l_raised",
    "l_pressed",
    "r_raised",
    "r_pressed",
    "round_button_raised",
    "round_button_pressed",
    "window_left",
    "window_middle",
    "window_right",
    "z_raised",
    "z_pressed",
    "c_stick_neutral",
    "c_stick_up",
    "c_stick_down",
    "d_pad_neutral",
    "d_pad_up",
    "d_pad_down",
    "d_pad_right",
    "d_pad_left",
];

fn roles(group: Sprite) -> Vec<String> {
    let names = match group {
        Sprite::Buttons => BUTTONS,
        Sprite::Portraits | Sprite::PetrifiedPortraits | Sprite::StrategyCharacters => {
            ui_art::characters::NAMES
        }
        Sprite::Elements => &ui_art::scan::NAMES[1..9],
        Sprite::ItemTabs => &[
            "all",
            "consumables",
            "weapons",
            "body",
            "head",
            "arms",
            "accessories",
            "ingredients",
            "key_items",
        ],
        Sprite::TechRanks => &["intermediate", "advanced"],
        Sprite::CookingStars => &["earned", "available"],
        Sprite::ConditionIcons => &[
            "poison_1",
            "poison_2",
            "poison_3",
            "poison_4",
            "unassigned_4",
            "unassigned_5",
            "unassigned_6",
            "unassigned_7",
            "paralysis_left_1",
            "paralysis_right_1",
            "paralysis_left_2",
            "paralysis_right_2",
            "curse",
            "curse_glow",
        ],
        Sprite::EquipmentMarkers => &[
            "improvement_1",
            "improvement_2",
            "improvement_3",
            "reduction_1",
            "reduction_2",
            "reduction_3",
            "unchanged",
            "equipped",
        ],
        Sprite::Technique => &[
            "center",
            "right_active",
            "left_inactive",
            "right_inactive",
            "left_active",
            "right_low",
            "right_medium",
            "right_high",
            "right_empty",
            "left_low",
            "left_medium",
            "left_high",
            "left_empty",
        ],
        Sprite::WorldHud => &[
            "map_frame",
            "compass_center",
            "compass_direction",
            "north",
            "east",
            "west",
            "south",
        ],
        Sprite::Items => {
            return (1..=49)
                .map(|id| {
                    if id == 49 {
                        return "greens".into();
                    }
                    crate::items::Category::from_index(id)
                        .map(String::from)
                        .unwrap_or_else(|_| format!("unassigned_{id}"))
                })
                .collect();
        }
        Sprite::Leader | Sprite::Numbers | Sprite::ItemImages | Sprite::Recipes => &[],
    };
    names.iter().map(|name| (*name).into()).collect()
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Gradient {
    Solid([u8; 4]),
    Vertical([[u8; 4]; 2]),
}
impl Gradient {
    fn from(value: [[u8; 4]; 2]) -> Self {
        if value[0] == value[1] {
            Self::Solid(value[0])
        } else {
            Self::Vertical(value)
        }
    }
    fn colors(self) -> [[u8; 4]; 2] {
        match self {
            Self::Solid(value) => [value; 2],
            Self::Vertical(value) => value,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Source {
    rects: Value,
    number_colors: Value,
    bar_colors: Value,
    names: Value,
}

impl MenuSprites {
    pub fn name_catalogue(&mut self, group: Sprite, mappings: &Mappings) -> Result<()> {
        let rects = self.group(group)?;
        let names = rects
            .iter()
            .enumerate()
            .map(|(i, rect)| Ok((mappings.target(i as u32)?.to_owned(), *rect)))
            .collect::<Result<_>>()?;
        self.named_rects.insert(group, names);
        Ok(())
    }
    pub fn use_character_defaults(&mut self, defaults: &[String]) -> Result<()> {
        anyhow::ensure!(
            self.names.len() == 9 && defaults.len() == 9,
            "incomplete menu character names"
        );
        self.name_overrides = ui_art::characters::NAMES
            .iter()
            .zip(self.names.iter().zip(defaults))
            .filter(|(_, (name, default))| name != default)
            .map(|(role, (name, _))| ((*role).into(), name.clone()))
            .collect();
        Ok(())
    }
    pub(super) fn decode(
        value: Value,
        palette: &[[u8; 4]],
        diagnostics: &Diagnostics,
    ) -> Result<Self> {
        let source: Source = decode_art_group(value, diagnostics, "menu sprite groups")?;
        let mut result = Self::default();
        for (group, value) in
            decode_art_rows::<Sprite, Value>(source.rects, diagnostics, "menu sprites")?
        {
            let decoded = (|| -> Result<_> {
                if matches!(group, Sprite::Leader | Sprite::Numbers) {
                    return Ok(vec![serde_json::from_value(value)?]);
                }
                let values: BTreeMap<String, [u32; 4]> = serde_json::from_value(value)?;
                if matches!(group, Sprite::ItemImages | Sprite::Recipes) {
                    result.named_rects.insert(group, values);
                    return Ok(Vec::new());
                }
                let names = roles(group);
                // Sparse mod groups retain their slots, so a missing icon only
                // affects the page selecting it rather than shifting later icons.
                anyhow::ensure!(
                    values.keys().all(|key| names.contains(key)),
                    "unknown menu sprite role"
                );
                Ok(names
                    .iter()
                    .map(|name| values.get(name).copied().unwrap_or_default())
                    .collect())
            })();
            if let Some(rects) = diagnostics.attempt("menu sprite group", decoded)? {
                result.rects.insert(group, rects);
            }
        }
        let mut number_colors = palette.iter().map(|color| [*color; 2]).collect::<Vec<_>>();
        number_colors.resize(NUMBER_COLORS.len(), [[0; 4]; 2]);
        for (name, value) in decode_art_rows::<String, Gradient>(
            source.number_colors,
            diagnostics,
            "menu number colors",
        )? {
            if let Some(index) = diagnostics.attempt(
                "menu number color role",
                NUMBER_COLORS
                    .iter()
                    .position(|role| *role == name)
                    .context("unknown menu number color"),
            )? {
                number_colors[index] = value.colors();
            }
        }
        result.number_colors = number_colors;
        let bars: BTreeMap<String, [[u8; 4]; 4]> =
            decode_art_group(source.bar_colors, diagnostics, "menu bar colors")?;
        result.bar_colors = if bars.is_empty() {
            Vec::new()
        } else {
            diagnostics
                .attempt("menu bar colors", ui_art::ordered(bars, BARS))?
                .unwrap_or_default()
        };
        result.name_overrides = decode_art_group(source.names, diagnostics, "menu party names")?;
        anyhow::ensure!(
            result
                .name_overrides
                .keys()
                .all(|name| ui_art::characters::NAMES.contains(&name.as_str())),
            "unknown menu character name"
        );
        Ok(result)
    }

    pub(super) fn prepare_catalogues<B: AsRef<[u8]>>(
        &mut self,
        read: &mut impl FnMut(&str) -> Result<B>,
    ) {
        #[derive(Deserialize)]
        struct Catalogue {
            mappings: Mappings,
        }
        for (group, path) in [
            (Sprite::ItemImages, crate::items::PATH),
            (Sprite::Recipes, crate::cooking::PATH),
        ] {
            let Some(named) = self.named_rects.get(&group) else {
                continue;
            };
            let result = (|| -> Result<Vec<[u32; 4]>> {
                let catalogue: Catalogue = serde_json::from_slice(read(path)?.as_ref())?;
                let size = catalogue
                    .mappings
                    .iter()
                    .map(|(id, _)| id)
                    .max()
                    .unwrap_or(0);
                anyhow::ensure!(size <= u32::from(u16::MAX), "menu sprite ID out of range");
                let mut rects = vec![[0; 4]; size as usize + 1];
                for (id, name) in catalogue.mappings.iter() {
                    if let Some(rect) = named.get(name) {
                        rects[id as usize] = *rect;
                    }
                }
                Ok(rects)
            })();
            if let Ok(rects) = result {
                self.rects.insert(group, rects);
            } else {
                self.rects.remove(&group);
            }
        }
        #[derive(Deserialize)]
        struct Character {
            name: String,
        }
        #[derive(Deserialize)]
        struct Characters {
            mappings: Mappings,
            characters: BTreeMap<String, Character>,
        }
        self.names = (|| -> Result<_> {
            let defaults = read("game/characters.json")
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Characters>(bytes.as_ref()).ok());
            ui_art::characters::NAMES
                .iter()
                .enumerate()
                .map(|(i, role)| {
                    self.name_overrides
                        .get(*role)
                        .cloned()
                        .or_else(|| {
                            let defaults = defaults.as_ref()?;
                            defaults
                                .characters
                                .get(defaults.mappings.target(i as u32 + 1).ok()?)
                                .map(|v| v.name.clone())
                        })
                        .context("missing menu character name")
                })
                .collect()
        })()
        .unwrap_or_default();
    }

    pub(super) fn encode(&self, palette: &[[u8; 4]]) -> Result<Value> {
        let mut rects = BTreeMap::new();
        for (group, values) in &self.rects {
            let value = if matches!(group, Sprite::ItemImages | Sprite::Recipes) {
                serde_json::to_value(
                    self.named_rects
                        .get(group)
                        .context("menu catalogue artwork is unnamed")?,
                )?
            } else if matches!(group, Sprite::Leader | Sprite::Numbers) {
                serde_json::to_value(values.first().context("missing menu sprite")?)?
            } else {
                let names = roles(*group);
                anyhow::ensure!(
                    values.len() == names.len(),
                    "incomplete menu sprite group {group:?}"
                );
                serde_json::to_value(names.into_iter().zip(values).collect::<BTreeMap<_, _>>())?
            };
            rects.insert(*group, value);
        }
        // A decoded catalogue has not necessarily prepared its machine slots.
        for (group, named) in &self.named_rects {
            rects.insert(*group, serde_json::to_value(named)?);
        }
        let number_colors = self
            .number_colors
            .iter()
            .enumerate()
            .filter(|(i, value)| palette.get(*i).is_none_or(|color| **value != [*color; 2]))
            .map(|(i, value)| {
                Ok((
                    *NUMBER_COLORS.get(i).context("unknown number color slot")?,
                    Gradient::from(*value),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut value = serde_json::json!({"rects":rects,"number_colors":number_colors});
        if !self.bar_colors.is_empty() {
            value["bar_colors"] = serde_json::to_value(ui_art::named(&self.bar_colors, BARS)?)?;
        }
        if !self.name_overrides.is_empty() {
            value["names"] = serde_json::to_value(&self.name_overrides)?;
        }
        Ok(value)
    }
}
