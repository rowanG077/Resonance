//! Observable support benefits. Timed magnitudes and damage mitigation need separate evidence.
use anyhow::{Result, ensure};
use resonance_battle::{
    Actor, Element, GuardKind,
    conditions::{Condition, ConditionSet, MAGICAL_AILMENTS, PHYSICAL_AILMENTS},
};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Terminal {
    #[default]
    Returned,
    SelfDefeated,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(super) struct Observation {
    pub terminal: Terminal,
    #[serde(default)]
    pub time_stop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct State {
    #[serde(serialize_with = "sorted_conditions")]
    pub conditions: ConditionSet,
    pub tp: u16,
    pub enchantment: Option<Element>,
    pub special_guard: bool,
}

fn sorted_conditions<S: Serializer>(
    conditions: &ConditionSet,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut names = serde_json::to_value(conditions).map_err(serde::ser::Error::custom)?;
    names
        .as_array_mut()
        .unwrap()
        .sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    names.serialize(serializer)
}

impl State {
    pub fn observe(actor: &Actor) -> Self {
        Self {
            conditions: actor.conditions.base(),
            tp: actor.tp,
            enchantment: actor.elements.enchantment,
            special_guard: actor.guard.active && actor.guard.kind == GuardKind::Special,
        }
    }

    pub fn validate_initial(self, actor: &Actor) -> Result<()> {
        ensure!(
            self == Self::observe(actor),
            "support initial state differs from the prepared actor"
        );
        ensure!(
            self.enchantment.is_none() && !self.special_guard,
            "initial enchantment or special guard needs its observed lifetime/input state"
        );
        Ok(())
    }

    pub fn changes(self, before: Self) -> Vec<Value> {
        let mut events = Vec::new();
        if self.conditions != before.conditions {
            events.push(json!({"kind":"condition_changed",
                "before":json!(before)["conditions"],"after":json!(self)["conditions"]}));
        }
        if self.tp > before.tp {
            events.push(json!({"kind":"tp_recovered","amount":self.tp-before.tp}));
        }
        if self.enchantment != before.enchantment {
            events.push(json!({"kind":"enchantment_changed","before":before.enchantment,"after":self.enchantment}));
        }
        if self.special_guard && !before.special_guard {
            events.push(json!({"kind":"special_guard_started"}));
        }
        events
    }
}

pub(super) fn attributable(actor: &Actor) -> bool {
    let regeneration = actor.conditions.gear_regeneration();
    actor.conditions.periodic_effects().is_empty()
        && actor
            .conditions
            .active_effects()
            .iter()
            .all(|effect| effect.remaining.is_none())
        && regeneration.hp_percent == 0
        && regeneration.tp_percent == 0
        && actor.equipment.recovery.common == Default::default()
}

pub(super) fn beneficial(event: &Value) -> Result<bool> {
    Ok(match event["kind"].as_str() {
        Some("time_stop_started") => event["ticks"].as_u64().is_some_and(|ticks| ticks > 0),
        Some("recovered" | "tp_recovered") => {
            event["amount"].as_u64().is_some_and(|amount| amount > 0)
        }
        Some("condition_changed") => {
            let before: ConditionSet = serde_json::from_value(event["before"].clone())?;
            let after: ConditionSet = serde_json::from_value(event["after"].clone())?;
            let buffs = ConditionSet::of(&[
                Condition::AttackUp,
                Condition::DefenseUp,
                Condition::AccuracyUp,
                Condition::MagicAttackUp,
                Condition::MagicDefenseUp,
                Condition::PhysicalBarrier,
                Condition::MagicalBarrier,
                Condition::HolySong,
                Condition::Revive,
                Condition::Flare,
            ]);
            before
                .without(after)
                .intersects(PHYSICAL_AILMENTS.union(MAGICAL_AILMENTS))
                || after.without(before).intersects(buffs)
        }
        Some("enchantment_changed") => {
            let before: Option<Element> = serde_json::from_value(event["before"].clone())?;
            let after: Option<Element> = serde_json::from_value(event["after"].clone())?;
            after.is_some() && before != after
        }
        Some("special_guard_started" | "revived") => true,
        _ => false,
    })
}
