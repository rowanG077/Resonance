//! Read-only menu statistics. Combat retains its separate derived base.
use resonance_content::menu_data::{Item, MenuData};
use resonance_events::party::{Member, Stats};

#[derive(Clone, Copy, Default)]
pub struct Projection {
    devils_arms_unlocked: bool,
    kills: [u16; 9],
}

impl Projection {
    pub fn new(devils_arms_unlocked: bool, kills: [u16; 9]) -> Self {
        Self {
            devils_arms_unlocked,
            kills,
        }
    }

    fn weapon_bonus(self, item: &Item, character: usize) -> u16 {
        if self.devils_arms_unlocked && item.properties.kills_damage {
            self.kills[character]
        } else {
            0
        }
    }

    pub fn member(self, data: &MenuData, member: &Member, character: usize) -> Stats {
        let mut stats = member.stats_for(data, character);
        let bonus = self.weapon_bonus(&data.items[usize::from(member.equipment[0])], character);
        // Combat adds kills after the ordinary 3000-point equipment cap.
        stats.slash = stats.slash.saturating_add(bonus);
        stats.thrust = stats.thrust.saturating_add(bonus);
        stats
    }

    pub fn preview_equipment(
        self,
        data: &MenuData,
        member: &Member,
        character: usize,
        slot: usize,
        item: u16,
    ) -> Stats {
        let mut preview = member.clone();
        preview.equipment[slot] = item;
        self.member(data, &preview, character)
    }

    pub(crate) fn battle_member(
        self,
        data: &MenuData,
        member: &Member,
        character: usize,
        formation: &[u8],
    ) -> anyhow::Result<Stats> {
        let loadout = crate::battle::party::loadout(
            data,
            member,
            resonance_battle::Character::from_index(character)?,
        )?;
        let combat =
            loadout.battle_stats(formation, self.devils_arms_unlocked, self.kills[character]);
        Ok(with_combat(member.stats_for(data, character), combat))
    }

    /// With no selected wielder, report the static item values.
    pub fn equipment(self, item: &Item, character: Option<usize>) -> [i32; 7] {
        let mut stats = item.equipment_stats.map(i32::from);
        let bonus = i32::from(character.map_or(0, |character| self.weapon_bonus(item, character)));
        stats[0] += bonus;
        stats[1] += bonus;
        stats
    }
}

pub(crate) fn live_member(
    data: &MenuData,
    committed: &Member,
    character: usize,
    actor: &resonance_battle::Actor,
) -> Stats {
    let mut stats = with_combat(committed.stats_for(data, character), actor.equipment.stats);
    stats.hp = actor.equipment.max_hp.clamp(0, i32::from(u16::MAX)) as u16;
    stats.tp = actor.equipment.max_tp;
    stats.luck = actor.equipment.luck;
    stats
}

fn with_combat(mut stats: Stats, combat: resonance_battle::CombatStats) -> Stats {
    let display = |value: i32| value.clamp(0, i32::from(u16::MAX)) as u16;
    stats.slash = display(combat.slash);
    stats.thrust = display(combat.thrust);
    stats.defense = display(combat.defense);
    stats.intelligence = display(combat.intelligence);
    stats.accuracy = display(combat.accuracy);
    stats.evasion = display(combat.evasion);
    stats
}
