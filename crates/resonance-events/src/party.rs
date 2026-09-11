//! Session-owned party state. Rendering and event bytecode do not own inventory.
use resonance_content::session::SessionData;
use std::collections::{BTreeMap, BTreeSet};
mod bestiary;
mod cooking;
mod ex_skills;
pub use bestiary::MonsterKnowledge;
mod items;
mod stats;
mod strategy;
mod techniques;
mod travel;
pub use cooking::{Cooking, CookingError, Meal};
pub use items::EncounterModifier;
pub use stats::{EquipmentTraits, Stats};
pub use travel::Travel;

fn initial_title() -> u8 {
    1
}
fn initial_titles() -> BTreeSet<u8> {
    [1].into()
}
fn initial_leader() -> u8 {
    1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Member {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Rules and owner identity are rebound on load, not duplicated in saves.
    #[serde(skip)]
    ex_rules: Option<ex_skills::Rules>,
    #[serde(default = "initial_title")]
    pub title: u8,
    #[serde(default = "initial_titles")]
    pub titles: BTreeSet<u8>,
    /// Negative favors technical arts; positive favors strike arts.
    #[serde(default)]
    pub technique_balance: i8,
    pub affinity: i32,
    pub level: u8,
    pub experience: u32,
    pub base_stats: [u16; 7],
    pub hp: u16,
    pub tp: u16,
    pub conditions: u32,
    pub luck: u8,
    pub overlimit: u8,
    pub equipment: [u16; 6],
    pub techniques: BTreeSet<u16>,
    pub shortcuts: [u16; 4],
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub disabled_techniques: BTreeSet<u16>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub technique_uses: BTreeMap<u16, u16>,
    #[serde(default)]
    pub assist_shortcuts: [Option<TechniqueShortcut>; 2],
    /// Action, skill/magic policy, and starting position.
    #[serde(default)]
    pub strategy: [u8; 3],
    #[serde(default)]
    pub cooking: [u8; resonance_content::menu_data::RECIPE_COUNT],
    #[serde(default)]
    pub ex_skills: [u8; 4],
    #[serde(default)]
    pub ex_gems: [u8; 4],
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub compound_ex_skills: BTreeSet<u8>,
    /// Highlights for recently learned compound skills.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub recent_compound_ex_skills: BTreeSet<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TechniqueShortcut {
    pub character: usize,
    pub technique: u16,
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::session::{CharacterDefinition, ItemDefinition, StatGrowth};

    fn data() -> SessionData {
        SessionData {
            ex_skills: None,
            version: 1,
            executable_sha256: "0".repeat(64),
            experience: vec![0, 0, 10, 30, 60],
            items: (0..4)
                .map(|id| ItemDefinition {
                    equipment_kind: (id != 0).then_some(0),
                    allowed_characters: 511,
                    stack_limit: if id == 3 { 1 } else { 20 },
                })
                .collect(),
            characters: (0..9)
                .map(|_| CharacterDefinition {
                    cooking: [0; resonance_content::menu_data::RECIPE_COUNT],
                    ex_skills: [0; 4],
                    ex_gems: [0; 4],
                    compound_ex_skills: Vec::new(),
                    recent_compound_ex_skills: Vec::new(),
                    technique_balance: 0,
                    affinity: 0,
                    level: 1,
                    experience: 0,
                    base_stats: [100, 20, 30, 40, 50, 60, 70],
                    luck: 10,
                    overlimit: 50,
                    equipment: [0; 6],
                    techniques: vec![],
                    allowed_techniques: vec![10],
                    shortcuts: [0; 4],
                    growth: std::array::from_fn(|_| StatGrowth {
                        base: 1,
                        random: 1,
                        title_bonus: 0,
                    }),
                    level_techniques: [(2, vec![10])].into(),
                })
                .collect(),
        }
    }

    #[test]
    fn equipment_transfers_items_and_tracks_stack_limits_and_discoveries() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.change_item(&data, 1, 3).unwrap();
        party.change_item(&data, 2, 1).unwrap();
        party.equip(&data, 0, 1).unwrap();
        assert_eq!(party.items[&1], 2);
        party.equip(&data, 0, 2).unwrap();
        assert_eq!(party.items[&1], 3);
        assert!(!party.items.contains_key(&2));
        assert_eq!(party.members[0].equipment[0], 2);
        party.unequip(&data, 0, 0).unwrap();
        assert_eq!(party.items[&2], 1);
        assert_eq!(party.members[0].equipment[0], 0);
        assert!(party.change_item(&data, 3, 10).unwrap());
        assert_eq!(party.items[&3], 1);
        assert!(!party.change_item(&data, 3, 1).unwrap());
        party.change_item(&data, 3, -8).unwrap();
        assert!(!party.items.contains_key(&3));
        assert!(party.found_items.contains(&3));
        assert_eq!(party.recent_items[0], 3);
    }

    #[test]
    fn an_equipment_swap_with_no_room_leaves_inventory_and_equipment_intact() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.change_item(&data, 3, 1).unwrap();
        party.equip(&data, 0, 3).unwrap();
        party.change_item(&data, 3, 1).unwrap();
        party.change_item(&data, 1, 2).unwrap();
        let before = party.items.clone();
        assert!(party.equip(&data, 0, 1).is_err());
        assert_eq!(party.items, before);
        assert_eq!(party.members[0].equipment[0], 3);
        assert!(party.unequip(&data, 0, 0).is_err());
        assert_eq!(party.items, before);
        assert_eq!(party.members[0].equipment[0], 3);
    }

    #[test]
    fn level_recovery_and_currency_update_persistent_state() {
        let data = data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        let mut draws = 0;
        party
            .raise_level(&data, 0, 3, None, || {
                draws += 1;
                3
            })
            .unwrap();
        assert_eq!(draws, 14);
        assert_eq!(party.members[0].experience, 30);
        assert_eq!(party.members[0].hp, 104);
        assert_eq!(party.members[0].tp, 24);
        assert_eq!(party.members[0].shortcuts, [10, 0, 0, 0]);
        party
            .raise_level(&data, 0, 2, None, || {
                panic!("lower level must not draw random numbers")
            })
            .unwrap();
        assert_eq!(party.members[0].level, 3);
        party.members[0].conditions = 0xff;
        party.members[0].hp = 1;
        party.heal(|| 207);
        assert_eq!(party.members[0].hp, 104);
        assert_eq!(party.members[0].conditions, 0);
        assert_eq!(party.members[0].luck, 7);
        assert_eq!(party.members[0].overlimit, 40);
        assert_eq!(party.add_gald(500), 500);
        assert_eq!(party.add_gald(-600), 0);
        assert_eq!(party.spent_gald, 500);
        assert_eq!(party.add_gald(i32::MAX), 99_999_999);
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    #[serde(flatten)]
    pub preferences: resonance_content::menu_data::CustomizeSettings,
    /// Manual 0, semi-auto 1, auto 2; one entry per battle controller.
    pub battle_controls: [u8; 4],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            preferences: Default::default(),
            battle_controls: [1, 2, 2, 2],
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Party {
    #[serde(default)]
    pub figurines: BTreeSet<u16>,
    #[serde(default)]
    pub monsters: BTreeMap<u8, MonsterKnowledge>,
    #[serde(default)]
    pub travel: Travel,
    #[serde(default)]
    pub cooking: Cooking,
    /// Unedited saves use the cooked defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_presets: Option<[resonance_content::menu_data::StrategyPreset; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encounter_modifier: Option<EncounterModifier>,
    #[serde(default)]
    pub viewed_skits: BTreeSet<u16>,
    pub members: Vec<Member>,
    pub formation: Vec<u8>,
    /// One-based character ID, independent of the battle formation.
    #[serde(default = "initial_leader")]
    pub field_leader: u8,
    #[serde(default)]
    pub leader_locked: bool,
    pub items: BTreeMap<u16, u8>,
    pub found_items: BTreeSet<u16>,
    pub recent_items: Vec<u16>,
    pub gald: u32,
    pub spent_gald: u32,
    pub settings: Settings,
}
impl Party {
    /// Keep the selected leader unless absent, knocked out or petrified.
    /// The manual selection lock does not prevent this automatic fallback.
    pub fn restore_field_leader(&mut self) -> u8 {
        let available = |id: u8| self.members[usize::from(id - 1)].can_lead_field();
        if (!self.formation.contains(&self.field_leader) || !available(self.field_leader))
            && let Some(id) = self.formation.iter().copied().find(|&id| available(id))
        {
            self.field_leader = id;
        }
        self.field_leader
    }

    pub fn validate(&self, data: &SessionData) -> anyhow::Result<()> {
        use anyhow::ensure;
        ensure!(
            self.members.iter().all(|m| m
                .name
                .as_ref()
                .is_none_or(|name| (1..=12).contains(&name.len())
                    && name.bytes().all(|b| (32..127).contains(&b)))),
            "invalid saved character name"
        );
        self.settings.preferences.validate()?;
        self.travel.validate()?;
        ensure!(
            self.monsters.iter().all(|(&id, knowledge)| usize::from(id)
                < resonance_content::monster::MONSTER_COUNT
                && knowledge.variant < 16),
            "invalid saved monster knowledge"
        );
        ensure!(
            usize::from(self.cooking.recipe) < resonance_content::menu_data::RECIPE_COUNT
                && usize::from(self.cooking.chef) < self.members.len()
                && self
                    .members
                    .iter()
                    .all(|m| m.cooking.iter().all(|&v| v <= 8)),
            "invalid saved cooking state"
        );
        ensure!(
            self.strategy_presets
                .as_ref()
                .is_none_or(|presets| presets.iter().all(|p| p.validate().is_ok())),
            "invalid saved strategy presets"
        );
        ensure!(
            self.members.len() == data.characters.len()
                && (1..=8).contains(&self.formation.len())
                && self.formation.contains(&self.field_leader)
                && self
                    .formation
                    .iter()
                    .all(|id| *id > 0 && usize::from(*id) <= self.members.len())
                && self.formation.iter().collect::<BTreeSet<_>>().len() == self.formation.len()
                && self.settings.battle_controls.iter().all(|v| *v <= 2)
                && self.gald <= 99_999_999
                && self.viewed_skits.iter().all(|&id| (1..=860).contains(&id)),
            "invalid saved party"
        );
        ensure!(
            self.figurines
                .iter()
                .all(|&id| usize::from(id) < resonance_content::figurine::FIGURINE_COUNT),
            "invalid saved figurine"
        );
        ensure!(
            self.encounter_modifier
                .as_ref()
                .is_none_or(|m| (1..=2).contains(&m.rate)
                    && (1..=EncounterModifier::DURATION).contains(&m.remaining)),
            "invalid saved encounter modifier"
        );
        ensure!(
            self.items.iter().all(|(id, count)| data
                .items
                .get(usize::from(*id))
                .is_some_and(|item| *count > 0 && *count <= item.stack_limit))
                && self
                    .found_items
                    .iter()
                    .chain(&self.recent_items)
                    .all(|id| usize::from(*id) < data.items.len())
                && self.recent_items.len() <= 32,
            "invalid saved inventory"
        );
        for (index, (member, definition)) in self.members.iter().zip(&data.characters).enumerate() {
            member.validate_ex(data.ex_skills.as_deref(), index)?;
            let [max_hp, max_tp] = member.vitals_with_ex(data.ex_skills.as_deref(), index);
            ensure!(
                member.level > 0
                    && member
                        .strategy
                        .iter()
                        .zip(resonance_content::menu_data::STRATEGY_COUNTS)
                        .all(|(v, count)| usize::from(*v) < count)
                    && member.titles.contains(&member.title)
                    && member.titles.iter().all(|id| (1..32).contains(id))
                    && (-100..=100).contains(&member.technique_balance)
                    && usize::from(member.level) < data.experience.len()
                    && member.hp <= max_hp
                    && member.tp <= max_tp
                    && member
                        .base_stats
                        .iter()
                        .zip([9999, 999, 32767, 32767, 32767, 32767, 32767])
                        .all(|(stat, limit)| *stat <= limit)
                    && member
                        .equipment
                        .iter()
                        .all(|id| usize::from(*id) < data.items.len())
                    && member
                        .techniques
                        .iter()
                        .all(|id| definition.allowed_techniques.contains(id))
                    && member
                        .shortcuts
                        .iter()
                        .all(|id| *id == 0 || member.techniques.contains(id))
                    && member.disabled_techniques.is_subset(&member.techniques)
                    && member
                        .technique_uses
                        .iter()
                        .all(|(id, count)| definition.allowed_techniques.contains(id)
                            && *count <= 9999)
                    && member.assist_shortcuts.iter().flatten().all(|shortcut| self
                        .members
                        .get(shortcut.character)
                        .is_some_and(|m| m.techniques.contains(&shortcut.technique))),
                "invalid saved party member"
            );
        }
        Ok(())
    }

    pub fn new(data: &SessionData, settings: Settings) -> anyhow::Result<Self> {
        data.validate()?;
        Ok(Self {
            cooking: Cooking::default(),
            encounter_modifier: None,
            members: data
                .characters
                .iter()
                .enumerate()
                .map(|(index, character)| Member {
                    name: None,
                    ex_rules: data.ex_skills.clone().map(|data| ex_skills::Rules {
                        data,
                        character: index,
                    }),
                    title: initial_title(),
                    titles: initial_titles(),
                    technique_balance: character.technique_balance,
                    affinity: character.affinity,
                    level: character.level,
                    experience: character.experience,
                    base_stats: character.base_stats,
                    hp: character.base_stats[0],
                    tp: character.base_stats[1],
                    conditions: 0,
                    luck: character.luck,
                    overlimit: character.overlimit,
                    equipment: character.equipment,
                    techniques: character.techniques.iter().copied().collect(),
                    shortcuts: character.shortcuts,
                    disabled_techniques: BTreeSet::new(),
                    technique_uses: BTreeMap::new(),
                    assist_shortcuts: [None; 2],
                    strategy: [0; 3],
                    cooking: character.cooking,
                    ex_skills: character.ex_skills,
                    ex_gems: character.ex_gems,
                    compound_ex_skills: character.compound_ex_skills.iter().copied().collect(),
                    recent_compound_ex_skills: character
                        .recent_compound_ex_skills
                        .iter()
                        .copied()
                        .collect(),
                })
                .collect(),
            formation: vec![1],
            monsters: BTreeMap::new(),
            figurines: BTreeSet::new(),
            travel: Travel::default(),
            field_leader: 1,
            leader_locked: false,
            strategy_presets: None,
            viewed_skits: BTreeSet::new(),
            items: BTreeMap::new(),
            found_items: BTreeSet::new(),
            recent_items: Vec::new(),
            gald: 0,
            spent_gald: 0,
            settings,
        })
    }
    pub fn change_item(&mut self, data: &SessionData, id: u16, delta: i8) -> Result<bool, String> {
        let item = data.items.get(usize::from(id)).ok_or("unknown item")?;
        let previous = self.items.get(&id).copied().unwrap_or(0);
        if delta > 0 && previous == item.stack_limit || delta <= 0 && previous == 0 {
            return Ok(false);
        }
        let count =
            (i16::from(previous) + i16::from(delta)).clamp(0, i16::from(item.stack_limit)) as u8;
        if count == 0 {
            self.items.remove(&id);
        } else {
            self.items.insert(id, count);
        }
        if delta > 0 {
            self.found_items.insert(id);
            self.recent_items.retain(|old| *old != id);
            self.recent_items.insert(0, id);
            self.recent_items.truncate(32);
        }
        Ok(true)
    }
    pub fn unequip(
        &mut self,
        data: &SessionData,
        member: usize,
        slot: usize,
    ) -> Result<(), String> {
        self.equip_slot(data, member, slot, 0).map(|_| ())
    }
    pub fn equip(&mut self, data: &SessionData, member: usize, id: u16) -> Result<(), String> {
        let item = data.items.get(usize::from(id)).ok_or("unknown item")?;
        let character = self.members.get(member).ok_or("unknown party member")?;
        if self.items.get(&id).copied().unwrap_or(0) == 0
            || item.allowed_characters & (1 << member) == 0
        {
            return Ok(());
        }
        let Some(slot) = item
            .equipment_kind
            .and_then(|kind| character.preferred_equipment_slot(kind))
        else {
            return Ok(());
        };
        self.equip_slot(data, member, slot, id).map(|_| ())
    }
    pub fn add_gald(&mut self, amount: i32) -> u32 {
        let previous = self.gald;
        self.gald = (i64::from(previous) + i64::from(amount)).clamp(0, 99_999_999) as u32;
        self.spent_gald = self
            .spent_gald
            .saturating_add(previous.saturating_sub(self.gald));
        self.gald
    }
    pub fn heal(&mut self, mut random: impl FnMut() -> u32) {
        for member in &mut self.members {
            [member.hp, member.tp] = member.maximum_vitals();
            member.conditions = 0;
            member.luck = (random() % 100) as u8;
            member.overlimit = member.overlimit.saturating_sub(10);
        }
    }
    pub fn raise_level(
        &mut self,
        data: &SessionData,
        index: usize,
        level: u8,
        title_growth: Option<[u8; 7]>,
        mut random: impl FnMut() -> u32,
    ) -> Result<(), String> {
        if level == 0 || usize::from(level) >= data.experience.len() {
            return Err("invalid target level".into());
        }
        let definition = data.characters.get(index).ok_or("unknown party member")?;
        let member = self.members.get_mut(index).ok_or("unknown party member")?;
        while member.level < level {
            member.level += 1;
            member.experience = data.experience[usize::from(member.level)];
            for (index, growth) in definition.growth.iter().enumerate() {
                let gain = u32::from(growth.base)
                    + random() % (u32::from(growth.random) + 1)
                    + u32::from(title_growth.map_or(growth.title_bonus, |title| title[index]));
                member.base_stats[index] =
                    (u32::from(member.base_stats[index]) + gain).min(match index {
                        0 => 9999,
                        1 => 999,
                        _ => 32767,
                    }) as u16;
            }
        }
        member.hp = member.base_stats[0];
        member.tp = member.base_stats[1];
        for (_, techniques) in definition.level_techniques.range(..=member.level) {
            for &technique in techniques {
                if member.techniques.insert(technique)
                    && let Some(slot) = member.shortcuts.iter_mut().find(|slot| **slot == 0)
                {
                    *slot = technique;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod settings_tests {
    use super::Settings;

    #[test]
    fn legacy_settings_default_skit_notifications_to_enabled() {
        let settings: Settings =
            serde_json::from_str(r#"{"rumble":true,"stereo":true,"battle_controls":[1,2,2,2]}"#)
                .unwrap();
        assert!(settings.preferences.skit_notifications);
    }
}
