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

/// Authored EX identities used by gameplay projections; these are not saved traits.
pub mod ex_effect {
    pub const TAUNT: u8 = 3;
    pub const DASH: u8 = 6;
    pub const ADDITIONAL_COMBO: u8 = 11;
    pub const GUARD_REDUCTION: u8 = 12;
    pub const AILMENT_RESISTANCE: u8 = 13;
    pub const SKY_COMBO: u8 = 14;
    pub const ABILITY_PLUS: u8 = 15;
    pub const FOLLOW_UP: u8 = 16;
    pub const AERIAL_ARTE: u8 = 17;
    pub const RESURRECT: u8 = 20;
    pub const BOOST: u8 = 21;
    pub const ANGEL_SONG: u8 = 22;
    pub const RHYTHM: u8 = 25;
    pub const DAMAGE_REDUCTION: u8 = 26;
    pub const SPEED_CAST: u8 = 27;
    pub const SPELL_SAVE: u8 = 28;
    pub const SPELL_CHARGE: u8 = 29;
    pub const NULLIFY_DAMAGE: u8 = 30;
    pub const QUICK_ITEM: u8 = 32;
    pub const HAPPINESS: u8 = 33;
    pub const SIX_HIT_COMBO: u8 = 41;
    pub const SUPER_CHAIN: u8 = 42;
    pub const REAR_GUARD: u8 = 43;
    pub const ENDURE: u8 = 45;
    pub const CHARGE: u8 = 46;
    pub const STUN_BONUS: u8 = 47;
    pub const ALONE: u8 = 48;
    pub const LOW_HP_RECOVERY: u8 = 49;
    pub const GUILT: u8 = 51;
    pub const FLASH: u8 = 52;
    pub const GUARD_DAMAGE_BOOST: u8 = 54;
    pub const PHYSICAL_STABILITY: u8 = 55;
    pub const COUNTER: u8 = 57;
    pub const PHYSICAL_AILMENT_GUARD: u8 = 58;
    pub const EXTENDED_OVERLIMIT: u8 = 59;
    pub const REBOUND: u8 = 60;
    pub const TAUNT_GUARD: u8 = 61;
    pub const HP_GROWTH: u8 = 62;
    pub const TP_GROWTH: u8 = 63;
    pub const TAUNT_CANCEL: u8 = 64;
    pub const QUICK_TURN: u8 = 65;
    pub const BACKSTEP_GUARD: u8 = 66;
    pub const IDLE_TP: u8 = 68;
    pub const CRITICAL_BONUS: u8 = 69;
    pub const AERIAL_ARTE_COMPOUND: u8 = 70;
    pub const QUICK_ESCAPE: u8 = 71;
    pub const INCREASE_EXPERIENCE: u8 = 72;
    pub const JUMP_COMBO: u8 = 73;
    pub const AERIAL_GUARD: u8 = 74;
    pub const TP_COST_REDUCTION: u8 = 75;
    pub const COUNTER_COMBO: u8 = 76;
    pub const SELF_CURE: u8 = 77;
    pub const IDLE_HP_TP: u8 = 79;
    pub const ITEM_FINDER: u8 = 81;
    pub const GALD_FINDER: u8 = 82;
    pub const LOW_HP_SPECIAL_GUARD: u8 = 83;
    pub const REFLECT_DAMAGE: u8 = 84;
    pub const NULLIFY_DAMAGE_COMPOUND: u8 = 85;
    pub const COMBO_HP: u8 = 87;
    pub const COMBO_TP: u8 = 88;
    pub const HAMMER_REVENGE: u8 = 89;
    pub const PHYSICAL_COUNTER: u8 = 90;
    pub const BATTLE_CRY: u8 = 91;
    pub const EXPERIENCE_PLUS: u8 = 92;
    pub const ANGEL_TEAR: u8 = 93;
    pub const PHYSICAL_AILMENT_GUARD_COMPOUND: u8 = 94;
    pub const CASTING_STABILITY: u8 = 95;
    pub const LUCKY_RECOVERY: u8 = 97;
    pub const ELEMENTAL_STABILITY: u8 = 98;
    pub const MAGICAL_AILMENT_GUARD: u8 = 99;
    pub const HARD_HIT: u8 = 100;
    pub const DOWN_TP: u8 = 101;
    pub const AIR_BRAKE: u8 = 102;
    pub const REDUCER: u8 = 103;
    pub const RANDOM_CAST: u8 = 104;
    pub const NIMBLE: u8 = 106;
    pub const ROLL: u8 = 107;
    pub const STORED_SPELL_STABILITY: u8 = 108;
    pub const DAMAGE_TP: u8 = 109;
    pub const SPELL_REVENGE: u8 = 110;
    pub const CASTING_STABILITY_COMPOUND: u8 = 111;
    pub const AID_REVENGE: u8 = 113;
    pub const ELEMENTAL_DAMAGE_REDUCTION: u8 = 114;
    pub const LUCKY_MAGIC: u8 = 116;
    pub const QUICK_CAST: u8 = 117;
    pub const EXTENDED_CONDITIONS: u8 = 118;
    pub const SPECIAL_GUARD_REDUCTION: u8 = 120;
    pub const SPIRIT_HEALER: u8 = 122;
    pub const ELEMENTAL_PHYSICAL_BOOST: u8 = 129;
    pub const PHYSICAL_ARTE_BOOST: u8 = 134;
    pub const TIMED_GUARD: u8 = 135;
    pub const REPRISE: u8 = 136;
    pub const LANDING: u8 = 137;
    pub const SUPER_BLAST: u8 = 138;
    pub const CHIVALRY: u8 = 139;
    pub const IDLE_HP: u8 = 140;
    pub const COMBO_FORCE: u8 = 142;
    pub const RUN_MAGIC_STABILITY: u8 = 143;
    pub const LUCKY_CHARGE: u8 = 146;
    pub const CHARGED_NEUTRAL_STABILITY: u8 = 147;
    pub const TAUNT_HP: u8 = 148;
    pub const VARIABLE_ATTACK: u8 = 149;
    pub const SINGLE_CHARGE_GUARD: u8 = 150;
    pub const SUPPRESS_SMALL_HITS: u8 = 151;
    pub const TOUGH_EXPERIENCE: u8 = 152;
    pub const CHARGED_RUN_STABILITY: u8 = 153;
    pub const STABILITY: u8 = 154;
    pub const SPECIAL_GUARD_SURVIVAL: u8 = 155;
    pub const NORMAL_GUARD: u8 = 156;
    pub const LAST_HIT_RECOVERY: u8 = 157;
    pub const TAUNT_VITALS: u8 = 159;
    pub const DOUBLE_JUMP: u8 = 160;
}

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

impl CharacterExSkills {
    /// Equipped recipes are active even before their discovery notice is recorded.
    pub fn equipped_compounds<'a>(
        &'a self,
        skills: &'a [u8; 4],
    ) -> impl Iterator<Item = (usize, &'a CompoundExSkill)> + 'a {
        self.compounds
            .iter()
            .enumerate()
            .filter(move |(_, recipe)| recipe.required.iter().all(|id| skills.contains(id)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExSkillData {
    pub skills: BTreeMap<u8, ExSkill>,
    pub characters: Vec<CharacterExSkills>,
    /// Inventory IDs for Lv1 through Lv4, followed by the Max gem.
    pub gem_items: [u16; 5],
}

impl ExSkillData {
    pub fn validate_rules(&self, items: usize) -> Result<()> {
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
            ensure!(
                skill.stat_bonuses.iter().all(|b| b.percent > 0)
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
                    && data.compounds.len() <= usize::from(u8::MAX) + 1,
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
        Ok(())
    }
}
