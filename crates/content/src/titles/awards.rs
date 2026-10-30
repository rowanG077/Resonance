use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Awards {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<Threshold>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub combos: Vec<Threshold>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<Equipment>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Threshold {
    pub character: PartyCharacter,
    pub title: String,
    pub minimum: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equipment {
    pub character: PartyCharacter,
    pub title: String,
    pub requirements: Vec<Requirement>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub slots: Vec<Slot>,
    pub items: Vec<String>,
}
/// The saved party equipment order, also used by award evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Weapon,
    Body,
    Head,
    Accessory1,
    Accessory2,
    Arm,
}
impl Slot {
    pub const ALL: [Self; 6] = [
        Self::Weapon,
        Self::Body,
        Self::Head,
        Self::Accessory1,
        Self::Accessory2,
        Self::Arm,
    ];
}

impl Awards {
    pub fn from_parts(
        rules: &menu_data::TitleAwardRules,
        titles: &BTreeMap<PartyCharacter, Mappings>,
        items: &Mappings,
    ) -> Result<Self> {
        let title = |character: u8, id: u8| -> Result<String> {
            Ok(titles
                .get(&PartyCharacter::try_from(character)?)
                .context("missing character titles")?
                .target(u32::from(id))?
                .into())
        };
        Ok(Self {
            levels: rules
                .levels
                .iter()
                .map(|r| {
                    Ok(Threshold {
                        character: r.character.try_into()?,
                        title: title(r.character, r.title)?,
                        minimum: u16::from(r.minimum),
                    })
                })
                .collect::<Result<_>>()?,
            combos: rules
                .combos
                .iter()
                .map(|r| {
                    Ok(Threshold {
                        character: r.character.try_into()?,
                        title: title(r.character, r.title)?,
                        minimum: r.minimum,
                    })
                })
                .collect::<Result<_>>()?,
            equipment: rules
                .equipment
                .iter()
                .map(|r| {
                    Ok(Equipment {
                        character: r.character.try_into()?,
                        title: title(r.character, r.title)?,
                        requirements: r
                            .requirements
                            .iter()
                            .map(|r| {
                                Ok(Requirement {
                                    slots: r
                                        .slots
                                        .iter()
                                        .map(|&slot| {
                                            Slot::ALL
                                                .get(usize::from(slot))
                                                .copied()
                                                .context("invalid equipment slot")
                                        })
                                        .collect::<Result<_>>()?,
                                    items: r
                                        .items
                                        .iter()
                                        .map(|&id| Ok(items.target(u32::from(id))?.into()))
                                        .collect::<Result<_>>()?,
                                })
                            })
                            .collect::<Result<_>>()?,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }

    pub(super) fn prepare(
        &self,
        titles: &BTreeMap<PartyCharacter, Mappings>,
        items: &Mappings,
    ) -> Result<menu_data::TitleAwardRules> {
        let title = |character: PartyCharacter, name: &str| -> Result<u8> {
            let id = titles
                .get(&character)
                .context("missing character titles")?
                .iter()
                .find(|(_, target)| *target == name)
                .with_context(|| format!("unknown award title {character:?}/{name}"))?
                .0;
            Ok(u8::try_from(id)?)
        };
        let items: BTreeMap<_, _> = items.iter().map(|(id, name)| (name, id)).collect();
        Ok(menu_data::TitleAwardRules {
            levels: self
                .levels
                .iter()
                .map(|r| {
                    Ok(menu_data::LevelTitleAward {
                        character: r.character as u8,
                        title: title(r.character, &r.title)?,
                        minimum: u8::try_from(r.minimum)?,
                    })
                })
                .collect::<Result<_>>()?,
            combos: self
                .combos
                .iter()
                .map(|r| {
                    Ok(menu_data::ComboTitleAward {
                        character: r.character as u8,
                        title: title(r.character, &r.title)?,
                        minimum: r.minimum,
                    })
                })
                .collect::<Result<_>>()?,
            equipment: self
                .equipment
                .iter()
                .map(|r| {
                    Ok(menu_data::EquipmentTitleAward {
                        character: r.character as u8,
                        title: title(r.character, &r.title)?,
                        requirements: r
                            .requirements
                            .iter()
                            .map(|r| {
                                Ok(menu_data::EquipmentRequirement {
                                    slots: r.slots.iter().map(|slot| *slot as u8).collect(),
                                    items: r
                                        .items
                                        .iter()
                                        .map(|name| {
                                            Ok(u16::try_from(
                                                *items.get(name.as_str()).with_context(|| {
                                                    format!("unknown title equipment {name}")
                                                })?,
                                            )?)
                                        })
                                        .collect::<Result<_>>()?,
                                })
                            })
                            .collect::<Result<_>>()?,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }
}
