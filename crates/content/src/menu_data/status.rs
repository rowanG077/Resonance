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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EquipmentProperties {
    pub attack_element: Option<Element>,
    /// Original resistance modifier for attacks without an element.
    pub neutral_resistance: i8,
    pub resistance: BTreeMap<Element, i8>,
    pub critical_chance_bonus: u8,
    /// Signed T/S contribution recomputed with the six equipped item rows.
    pub technique_drift: i8,
    pub effects: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentEffect {
    pub description: String,
    pub suppresses: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusData {
    pub conditions: [String; 32],
    pub equipment_effects: BTreeMap<u8, EquipmentEffect>,
    pub technical_type: String,
    pub strike_type: String,
}
impl StatusData {
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.conditions
            .iter()
            .chain([&self.technical_type, &self.strike_type])
            .chain(self.equipment_effects.values().map(|e| &e.description))
            .map(String::as_str)
    }
    pub fn validate(&self, items: &[Item]) -> Result<()> {
        ensure!(
            !self.technical_type.is_empty() && !self.strike_type.is_empty(),
            "missing technique type labels"
        );
        ensure!(
            self.equipment_effects
                .iter()
                .all(|(&id, e)| !e.suppresses.contains(&id)
                    && e.suppresses
                        .iter()
                        .all(|i| self.equipment_effects.contains_key(i)))
                && items.iter().all(|i| i.properties.effects.len() <= 5
                    && i.properties
                        .effects
                        .iter()
                        .all(|id| self.equipment_effects.contains_key(id))),
            "invalid equipment effects"
        );
        Ok(())
    }
}
