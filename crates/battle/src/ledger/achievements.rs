//! Per-encounter facts used by title awards. Counts belong to actor slots.
use crate::TechniqueId;
use crate::{ActionId, ActorId, Battle};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActorAchievements {
    pub spell_elements: u8,
    pub resisted_spells: u8,
    pub repeated_technique: u16,
    last_technique: Option<TechniqueId>,
    technique_streak: u16,
    pub weapon_changes: u8,
    pub guard_breaks: u8,
    pub knockdowns: u8,
    pub potions: u8,
    pub voices: u8,
    pub unison_starts: u8,
    pub acted: bool,
    pub used_arte: bool,
    pub ran: bool,
    pub stayed_poisoned: bool,
    pub commanded_alone: bool,
    pub strategy_orders: u8,
    pub assist_commands: u8,
    // One resisted spell counts once, regardless of hits or targets.
    resisted_actions: BTreeSet<ActionId>,
}
impl ActorAchievements {
    pub(crate) fn technique(
        &mut self,
        catalogue: TechniqueId,
        element: Option<crate::Element>,
        casting: bool,
    ) {
        self.technique_streak = if self.last_technique == Some(catalogue) {
            self.technique_streak.saturating_add(1)
        } else {
            1
        };
        self.last_technique = Some(catalogue);
        self.repeated_technique = self.repeated_technique.max(self.technique_streak);
        if casting
            && let Some(element) = element
            && !matches!(element, crate::Element::Light | crate::Element::Darkness)
        {
            self.spell_elements |= 1 << element as u8;
        }
    }
    pub(crate) fn resisted_spell(&mut self, action: ActionId) {
        if self.resisted_spells < 15 && self.resisted_actions.insert(action) {
            self.resisted_spells += 1;
        }
    }
}
impl Battle {
    /// Called only after the equipment transaction is accepted.
    pub fn record_weapon_change(&mut self, actor: ActorId) {
        let count = &mut self.ledger.achievements[actor.index()].weapon_changes;
        *count = count.saturating_add(1);
    }
    /// A quick order counts after its notice completes, not while browsing Strategy.
    pub fn record_strategy_order(&mut self, actor: ActorId) {
        let count = &mut self.ledger.achievements[actor.index()].strategy_orders;
        *count = count.saturating_add(1);
        self.record_command(actor);
    }

    pub(crate) fn record_command(&mut self, actor: ActorId) {
        let owner = &self.actors[actor.index()];
        if owner.side == crate::Side::Party
            && owner.available()
            && self
                .actors
                .iter()
                .filter(|a| {
                    a.side == crate::Side::Party
                        && a.availability != crate::ActorAvailability::Absent
                })
                .count()
                >= 2
            && self
                .actors
                .iter()
                .filter(|a| a.side == crate::Side::Party && a.available())
                .count()
                == 1
        {
            self.ledger.achievements[actor.index()].commanded_alone = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_credit_the_issuer_and_require_a_sole_survivor() {
        use crate::{ActorAvailability, Control, PreparedBattle, Side};
        let mut battle = PreparedBattle::new(
            [Side::Party, Side::Party, Side::Party, Side::Enemy]
                .map(|side| (crate::tests::actor(side), Default::default()))
                .into(),
            Default::default(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        let issuer = ActorId(0);
        battle.actors[1].availability = ActorAvailability::Dead;
        battle.record_command(issuer);
        assert!(!battle.ledger.achievements[0].commanded_alone);
        battle.record_strategy_order(issuer);
        battle.actors[0].control = Control::Auto;
        battle.actors[2].control = Control::Manual;
        assert_eq!(battle.ledger.achievements[0].strategy_orders, 1);
        assert_eq!(battle.ledger.achievements[2].strategy_orders, 0);
        battle.actors[0].availability = ActorAvailability::Dead;
        battle.record_strategy_order(ActorId(2));
        assert!(battle.ledger.achievements[2].commanded_alone);
        assert!(!battle.ledger.achievements[0].commanded_alone);
    }

    #[test]
    fn technique_streak_and_element_cycle_track_distinct_requirements() {
        let mut facts = ActorAchievements::default();
        for _ in 0..9 {
            facts.technique(TechniqueId(223), Some(crate::Element::Water), false);
        }
        assert_eq!(facts.repeated_technique, 9);
        assert_eq!(facts.spell_elements, 0, "martial elements are not spells");
        facts.technique(TechniqueId(224), Some(crate::Element::Water), false);
        for _ in 0..10 {
            facts.technique(TechniqueId(223), Some(crate::Element::Water), false);
        }
        assert_eq!(facts.repeated_technique, 10);
        for element in crate::Element::ALL {
            facts.technique(TechniqueId(61 + element as u16), Some(element), true);
        }
        assert_eq!(facts.spell_elements, 0b11_1111);
        assert_eq!(
            facts.repeated_technique, 10,
            "later artes preserve the achievement"
        );
    }

    #[test]
    fn resisted_spell_counts_once_across_hits_and_targets_and_caps_storage() {
        let mut facts = ActorAchievements::default();
        for _ in 0..30 {
            facts.resisted_spell(ActionId(1));
        }
        assert_eq!(facts.resisted_spells, 1);
        for action in 2..=30 {
            facts.resisted_spell(ActionId(action));
        }
        assert_eq!(facts.resisted_spells, 15);
        assert_eq!(facts.resisted_actions.len(), 15);
    }
}
