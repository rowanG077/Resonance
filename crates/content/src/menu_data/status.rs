use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    Water,
    Wind,
    Fire,
    Earth,
    Lightning,
    Ice,
    Light,
    Darkness,
}
impl Element {
    pub const ALL: [Self; 8] = [
        Self::Water,
        Self::Wind,
        Self::Fire,
        Self::Earth,
        Self::Lightning,
        Self::Ice,
        Self::Light,
        Self::Darkness,
    ];
}

/// Ailment groups applied or prevented by equipment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentAilment {
    Poison,
    Stun,
    Paralysis,
    Weak,
    Petrify,
    Curse,
    Heavy,
    PhysicalAilments,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TpDiscount {
    #[default]
    None,
    Third,
    Half,
}
impl TpDiscount {
    pub fn apply(self, cost: u32) -> u32 {
        match self {
            Self::None => cost,
            Self::Third => (u64::from(cost) * 2 / 3) as u32,
            Self::Half => cost / 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentRescue {
    Chance,
    Consumable,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EquipmentProperties {
    pub attack_element: Option<Element>,
    pub neutral_resistance: i8,
    pub resistance: BTreeMap<Element, i8>,
    pub critical_chance_bonus: u8,
    pub technique_drift: i8,
    pub species_bonus: Option<u16>,
    pub stat_bonuses: Vec<ExStatBonus>,
    pub tp_discount: TpDiscount,
    pub rescue: Option<EquipmentRescue>,
    pub immunities: std::collections::BTreeSet<EquipmentAilment>,
    pub ailments: std::collections::BTreeSet<EquipmentAilment>,
    pub hp_regeneration: u8,
    pub tp_regeneration: u8,
    pub movement_bonus: i16,
    pub experience_percent: u8,
    pub ailment_resistance: bool,
    pub short_stun: bool,
    pub faster_casting: bool,
    pub kill_hp_recovery: bool,
    pub kill_tp_recovery: bool,
    pub physical_damage_boost: bool,
    pub physical_damage_reduction: bool,
    pub magic_damage_boost: bool,
    pub gald_one_and_a_half: bool,
    pub gald_double: bool,
    pub defense_halved: bool,
    pub quick_escape: bool,
    pub kills_damage: bool,
    /// Only presentation uses these identifiers and their suppression rules.
    pub caption_ids: Vec<u8>,
    /// A selected item whose behavior is unavailable cannot enter battle.
    pub unsupported_modifier: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentEffect {
    pub suppresses: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusData {
    pub equipment_effects: BTreeMap<u8, EquipmentEffect>,
}
impl StatusData {
    pub fn validate(&self, items: &[Item]) -> Result<()> {
        ensure!(
            self.equipment_effects
                .iter()
                .all(|(&id, e)| !e.suppresses.contains(&id)
                    && e.suppresses
                        .iter()
                        .all(|i| self.equipment_effects.contains_key(i)))
                && items.iter().all(|i| i
                    .properties
                    .caption_ids
                    .iter()
                    .all(|id| self.equipment_effects.contains_key(id))),
            "invalid equipment effects"
        );
        Ok(())
    }
}
