//! Element-dependent martial variants shared by battle, menus and saved parties.
use super::{Catalogue, LearningRules};
use crate::menu_data::{Element, NamedText, Technique};
use anyhow::{Context, Result};

pub const ACTIVATION_USES: u16 = 200;
pub const LEARNING_USES: u16 = 50;

pub const LIGHTNING_THRUST: u16 = 253;
pub const LIGHTNING_TIGER: u16 = 254;
pub const GRAVE_BLADE: u16 = 255;
pub const KNIGHT_GRAVE_BLADE: u16 = 256;
pub const ICE_HAMMER: u16 = 257;
pub const SUPER_SWALLOW_DANCE: u16 = 258;
pub const RISING_PHOENIX: u16 = 259;

#[derive(Debug, Clone, Copy)]
pub struct Variant {
    pub id: u16,
    pub base: u16,
    pub name: &'static str,
    pub element: Element,
}

pub const VARIANTS: &[Variant] = &[
    Variant {
        id: LIGHTNING_THRUST,
        base: 10,
        name: "Lightning Thrust",
        element: Element::Lightning,
    },
    Variant {
        id: LIGHTNING_TIGER,
        base: 6,
        name: "Lightning Tiger",
        element: Element::Lightning,
    },
    Variant {
        id: GRAVE_BLADE,
        base: 4,
        name: "Grave Blade",
        element: Element::Earth,
    },
    Variant {
        id: KNIGHT_GRAVE_BLADE,
        base: 209,
        name: "Grave Blade",
        element: Element::Earth,
    },
    Variant {
        id: ICE_HAMMER,
        base: 40,
        name: "Ice Hammer",
        element: Element::Ice,
    },
    Variant {
        id: SUPER_SWALLOW_DANCE,
        base: 183,
        name: "Super Swallow Dance",
        element: Element::Wind,
    },
    Variant {
        id: RISING_PHOENIX,
        base: 29,
        name: "Rising Phoenix",
        element: Element::Fire,
    },
];

pub fn find(id: u16) -> Option<&'static Variant> {
    VARIANTS.iter().find(|variant| variant.id == id)
}

pub fn extend_learning(allowed: &mut Vec<u16>) {
    for variant in VARIANTS {
        if allowed.contains(&variant.base) && !allowed.contains(&variant.id) {
            allowed.push(variant.id);
        }
    }
}

/// Materialize the original elemental variants during import. Current library
/// readers never call this or overwrite a modder's authored variant values.
pub fn cook_variants(
    data: &mut Catalogue,
    menu: &mut Vec<Technique>,
    text: &mut Vec<serde_json::Value>,
) -> Result<()> {
    for variant in VARIANTS {
        let mut definition = data.definition(usize::from(variant.base))?.clone();
        definition.element = variant.element as u8 + 1;
        definition.required_level = 0;
        definition.learn_on_level_up = false;
        definition.learning = LearningRules::default();
        let id = usize::from(variant.id);
        data.definitions
            .resize_with(data.definitions.len().max(id + 1), Default::default);
        data.definitions[id] = definition;
    }
    for allowed in &mut data.learning {
        extend_learning(allowed);
    }
    for variant in VARIANTS {
        let mut definition = menu
            .get(usize::from(variant.base))
            .context("elemental arte has no base menu entry")?
            .clone();
        definition.element = variant.element as u8 + 1;
        definition.level = 0;
        // Remain hidden until learned, independently of the character's route.
        definition.route = 3;
        definition.prerequisite = 0;
        definition.alternatives = [variant.id, 0, 0, 0];
        let id = usize::from(variant.id);
        menu.resize(menu.len().max(id + 1), definition.clone());
        menu[id] = definition;
        let description = text
            .get(usize::from(variant.base))
            .and_then(|text| text["description"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| "An elemental martial arte.".into());
        text.resize(text.len().max(id + 1), serde_json::Value::Null);
        text[id] = serde_json::to_value(NamedText {
            name: variant.name.into(),
            description,
        })?;
    }
    Ok(())
}
