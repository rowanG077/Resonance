use super::*;
use std::collections::BTreeSet;

pub const EX_LABELS: [&str; 18] = [
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
    "gem_empty",
    "gem_max",
    "gem_level",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExTendency {
    Technical,
    Strike,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExActivation {
    Constant,
    Chance,
    BattleEnd,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExStat {
    Strength,
    Defense,
    Accuracy,
    Evasion,
    MaxHp,
    MaxTp,
    Luck,
    Intelligence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExStatBonus {
    pub stat: ExStat,
    /// Each bonus is rounded independently from the character's base statistic.
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExSkill {
    pub name: String,
    pub description: MenuText,
    pub stat_bonuses: Vec<ExStatBonus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_point_tp_cost: Option<u8>,
    pub tendency: Option<ExTendency>,
    pub activation: ExActivation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompoundExSkill {
    pub skill: u8,
    /// All these base skills must be equipped. Learning occurs through battle.
    pub required: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterExSkills {
    /// Four choices per gem level, in displayed order. Max gems offer all levels.
    pub levels: [[u8; 4]; 4],
    /// Indices are stable character-local identities used by saved knowledge.
    pub compounds: Vec<CompoundExSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExSkillData {
    pub skills: BTreeMap<u8, ExSkill>,
    pub characters: Vec<CharacterExSkills>,
    /// Inventory IDs for Lv1 through Lv4, followed by the Max gem.
    pub gem_items: [u16; 5],
    pub activation_labels: BTreeMap<ExActivation, String>,
    pub labels: BTreeMap<String, String>,
}

impl ExSkillData {
    pub fn validate(&self, items: usize) -> Result<()> {
        ensure!(
            !self.skills.is_empty()
                && !self.skills.contains_key(&0)
                && self.characters.len() == 9
                && self
                    .gem_items
                    .iter()
                    .all(|&id| id > 0 && usize::from(id) < items)
                && self.gem_items.into_iter().collect::<BTreeSet<_>>().len() == 5,
            "invalid EX skill catalog"
        );
        for (&id, skill) in &self.skills {
            skill.description.validate()?;
            ensure!(
                !skill.name.is_empty()
                    && skill.stat_bonuses.len() <= 2
                    && skill.stat_bonuses.iter().all(|b| b.percent > 0)
                    && skill.save_point_tp_cost.is_none_or(|cost| cost > 0),
                "invalid EX skill {id}"
            );
        }
        for (character, data) in self.characters.iter().enumerate() {
            let allowed: BTreeSet<_> = data.levels.iter().flatten().copied().collect();
            ensure!(
                allowed.len() == 16
                    && allowed
                        .iter()
                        .all(|id| self.skills.get(id).is_some_and(|s| s.tendency.is_some()))
                    && data.compounds.len() == 24,
                "invalid EX skill choices for character {character}"
            );
            for compound in &data.compounds {
                ensure!(
                    self.skills
                        .get(&compound.skill)
                        .is_some_and(|s| s.tendency.is_none())
                        && (2..=4).contains(&compound.required.len())
                        && compound.required.iter().all(|id| allowed.contains(id))
                        && compound.required.iter().collect::<BTreeSet<_>>().len()
                            == compound.required.len(),
                    "invalid compound EX skill {} for character {character}",
                    compound.skill
                );
            }
        }
        for activation in [
            ExActivation::Constant,
            ExActivation::Chance,
            ExActivation::BattleEnd,
            ExActivation::Other,
        ] {
            ensure!(
                self.activation_labels
                    .get(&activation)
                    .is_some_and(|s| !s.is_empty()),
                "missing EX activation label"
            );
        }
        for key in EX_LABELS {
            ensure!(
                self.labels.get(key).is_some_and(|s| !s.is_empty()),
                "missing EX skill label {key}"
            );
        }
        Ok(())
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.skills
            .values()
            .flat_map(|s| std::iter::once(s.name.as_str()).chain(s.description.texts()))
            .chain(self.activation_labels.values().map(String::as_str))
            .chain(self.labels.values().map(String::as_str))
    }
}
