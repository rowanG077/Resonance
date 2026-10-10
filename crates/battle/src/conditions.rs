//! Live condition layers, stat modifiers, and expiry clocks.
//! Save-format conversion belongs to the game. Availability and enchantment remain actor state.
use crate::{Actor, Element};

use Condition::*;
pub use resonance_content::battle_conditions::{Condition, ConditionSet};

pub const POISON: ConditionSet = ConditionSet::of(&[PoisonMild, PoisonSevere]);
pub const STAT_CONDITIONS: ConditionSet = ConditionSet::of(&[
    AttackUp,
    DefenseUp,
    AccuracyUp,
    MagicAttackUp,
    MagicDefenseUp,
    AttackDown,
    DefenseDown,
    AccuracyDown,
    MagicAttackDown,
    MagicDefenseDown,
    EvasionDown,
]);
pub const PHYSICAL_AILMENTS: ConditionSet = ConditionSet::of(&[
    PoisonMild,
    PoisonSevere,
    Paralysis,
    Petrified,
    Curse,
    PhysicalAffliction,
]);
pub const MAGICAL_AILMENTS: ConditionSet = ConditionSet::of(&[
    Weak,
    ReduceItemEffect,
    Heavy,
    AttackDown,
    DefenseDown,
    AccuracyDown,
    MagicAttackDown,
    MagicDefenseDown,
    EvasionDown,
]);
pub(crate) const PROTECTION: ConditionSet =
    ConditionSet::of(&[PhysicalProtection, MagicalProtection]);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Layers {
    pub base: ConditionSet,
    pub intrinsic: ConditionSet,
    pub equipment_overlay: ConditionSet,
    pub immunity: ConditionSet,
}

/// Percentages rebuilt from equipped properties, independently of condition layers and
/// clocks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GearRegeneration {
    pub hp_percent: u8,
    pub tp_percent: u8,
}

/// Active EX recipes, independent of mutable condition layers and clocks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Traits {
    /// EX58/94: the contact owner rejects physical ailments before its draw.
    pub physical_ailment_guard: bool,
    /// EX99 rejects contact and magical ailments.
    pub magical_ailment_guard: bool,
    /// Extend condition duration by 25% on the actual recipient.
    pub extended_duration: bool,
}

/// Mutable conditions own their complete lifetime and magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveEffect {
    pub condition: Condition,
    pub remaining: Option<u32>,
    pub magnitude: i16,
}

/// A periodic effect exists only while its condition is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodicEffect {
    pub condition: Condition,
    pub remaining: u32,
    pub period: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conditions {
    effects: Vec<ActiveEffect>,
    periodic: Vec<PeriodicEffect>,
    intrinsic: ConditionSet,
    equipment_overlay: ConditionSet,
    immunity: ConditionSet,
    profile_intrinsic: ConditionSet,
    profile_immunity: ConditionSet,
    traits: Traits,
    gear_regeneration: GearRegeneration,
}

#[derive(Default)]
struct ConditionTick {
    poison_percent: u8,
    regenerate_hp: bool,
    regenerate_tp: bool,
}

const POISON_INTERVAL: u32 = 30;
const REGENERATION_INTERVAL: u32 = 360;
const DEFAULT_STAT_CONDITION_DURATION: u32 = 1800;
/// Check paralysis before admitting grounded controls.
///
/// Grounded paralysis uses a percentage chance reduced by luck.
/// Airborne actors and actors without paralysis keep control.
pub(crate) fn paralysis_controller_roll(actor: &Actor, random: &mut crate::state::Random) -> bool {
    if !actor.conditions.effective().contains(Paralysis) || actor.airborne() {
        return false;
    }
    let chance = 100 - (i32::from(actor.equipment.luck) >> 2);
    let roll = i32::from(random.next_u16() % 100);
    roll < chance
}

/// Condition labels last 45 updates. Weak uses a distinct presentation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionLabel {
    Applied,
    StatusDown,
    /// Native category9, shared by EX13 resistance and other EX activations.
    ExSkillEffect,
    /// Native category8, used by equipment13 without single EX13.
    EquipmentEffect,
    /// Native category14 MAGIC/EFFECT, selected by the Revive rescue owner.
    Revive,
}

/// Supported condition requests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Buff {
    Flare,
    Guard,
    Acuity,
    PhysicalAilmentGuard { persistent: bool },
    MagicalAilmentGuard { persistent: bool },
    Quartz(Element),
}

/// Cure requests inspect mutable ailments independently of equipment layers and availability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cure {
    /// Panacea Bottle.
    Physical,
    /// Anti-Magic (Remedy) Bottle.
    AntiMagic,
    /// Miracle Bottle.
    All,
}

impl Cure {
    /// Panacea and Miracle thaw petrified actors. Anti-Magic changes ailments while retaining
    /// petrification.
    pub(crate) fn thaws_petrify(self) -> bool {
        matches!(self, Self::Physical | Self::All)
    }

    pub(crate) fn eligible_base(self, base: ConditionSet) -> bool {
        base.intersects(match self {
            Self::Physical => PHYSICAL_AILMENTS,
            Self::AntiMagic => MAGICAL_AILMENTS,
            Self::All => PHYSICAL_AILMENTS.union(MAGICAL_AILMENTS),
        })
    }
}

pub(crate) struct PreparedBuff {
    effect: Option<ActiveEffect>,
    element: Option<Element>,
}

fn stat_sign(condition: Condition) -> Option<i16> {
    match condition {
        AttackUp | DefenseUp | AccuracyUp | MagicAttackUp | MagicDefenseUp => Some(1),
        AttackDown | DefenseDown | AccuracyDown | MagicAttackDown | MagicDefenseDown
        | EvasionDown => Some(-1),
        _ => None,
    }
}

fn opposing_stat_condition(condition: Condition) -> Option<Condition> {
    Some(match condition {
        AttackUp => AttackDown,
        AttackDown => AttackUp,
        DefenseUp => DefenseDown,
        DefenseDown => DefenseUp,
        AccuracyUp => AccuracyDown,
        AccuracyDown => AccuracyUp,
        MagicAttackUp => MagicAttackDown,
        MagicAttackDown => MagicAttackUp,
        MagicDefenseUp => MagicDefenseDown,
        MagicDefenseDown => MagicDefenseUp,
        _ => return None,
    })
}

impl Conditions {
    fn initial_effect(&self, condition: Condition) -> ActiveEffect {
        let remaining = match condition {
            Paralysis | Heavy => Some(600),
            Flare | Guard | Acuity | Quartz => Some(1200),
            PhysicalProtection | MagicalProtection => Some(900),
            condition if STAT_CONDITIONS.contains(condition) => {
                Some(DEFAULT_STAT_CONDITION_DURATION)
            }
            _ => None,
        };
        ActiveEffect {
            condition,
            remaining: remaining.map(|ticks| self.duration(ticks, false)),
            magnitude: stat_sign(condition).unwrap_or(0) * 10,
        }
    }
    pub fn new(layers: Layers) -> Self {
        let mut conditions = Self::default();
        conditions.reload(layers);
        conditions
    }

    pub fn traits(&self) -> Traits {
        self.traits
    }
    pub fn with_traits(mut self, traits: Traits) -> Self {
        self.traits = traits;
        self
    }
    pub fn active_effects(&self) -> &[ActiveEffect] {
        &self.effects
    }
    pub fn periodic_effects(&self) -> &[PeriodicEffect] {
        &self.periodic
    }
    pub fn remaining(&self, condition: Condition) -> Option<u32> {
        self.effects
            .iter()
            .find(|effect| effect.condition == condition)
            .and_then(|effect| effect.remaining)
    }
    pub fn magnitude(&self, condition: Condition) -> i16 {
        self.effects
            .iter()
            .find(|effect| effect.condition == condition)
            .map_or(0, |effect| effect.magnitude)
    }
    pub fn base(&self) -> ConditionSet {
        self.effects.iter().map(|effect| effect.condition).collect()
    }
    /// Item hints inspect removable ailments, never equipment or forced layers.
    pub fn needs_cure(&self, cure: Cure) -> bool {
        cure.eligible_base(self.base())
    }
    pub fn effective(&self) -> ConditionSet {
        self.base()
            .union(self.profile_intrinsic)
            .union(self.intrinsic)
            .union(self.equipment_overlay)
    }
    pub fn immunity(&self) -> ConditionSet {
        self.profile_immunity.union(self.immunity)
    }
    /// Mutable ailments and gear-owned layers. Profile traits survive their replacement.
    pub fn layers(&self) -> Layers {
        Layers {
            base: self.base(),
            intrinsic: self.intrinsic,
            equipment_overlay: self.equipment_overlay,
            immunity: self.immunity,
        }
    }
    pub fn arte_queue_allowed(&self) -> bool {
        !self.effective().contains(Curse)
    }
    pub fn gear_regeneration(&self) -> GearRegeneration {
        self.gear_regeneration
    }
    pub fn reload_gear_regeneration(&mut self, value: GearRegeneration) {
        self.gear_regeneration = value;
    }
    /// Install the actor's profile once during preparation. Initial ailments are mutable;
    /// intrinsic traits and immunities remain independent of equipment changes.
    pub fn initialize_profile(
        &mut self,
        initial: ConditionSet,
        intrinsic: ConditionSet,
        immunity: ConditionSet,
    ) {
        let layers = Layers {
            base: self.base().union(initial),
            ..self.layers()
        };
        self.profile_intrinsic = intrinsic;
        self.profile_immunity = immunity;
        self.reload(layers);
    }

    /// Keep retained mutable effects intact; equipment never becomes mutable state.
    pub(crate) fn reload(&mut self, layers: Layers) {
        self.effects
            .retain(|effect| layers.base.contains(effect.condition));
        for condition in layers.base.without(self.base()).iter() {
            self.effects.push(self.initial_effect(condition));
        }
        self.effects.sort_by_key(|effect| effect.condition);
        self.intrinsic = layers.intrinsic;
        self.equipment_overlay = layers.equipment_overlay;
        self.immunity = layers.immunity;
        self.sync_periodic();
    }
    pub fn reload_layers(&mut self, layers: Layers) {
        self.reload(layers);
    }

    fn set_effect(&mut self, effect: ActiveEffect) {
        if let Some(current) = self
            .effects
            .iter_mut()
            .find(|current| current.condition == effect.condition)
        {
            *current = effect;
        } else {
            self.effects.push(effect);
            self.effects.sort_by_key(|effect| effect.condition);
        }
        self.sync_periodic();
    }
    fn remove(&mut self, removed_conditions: ConditionSet) -> bool {
        let removed = self.base().intersection(removed_conditions);
        self.effects
            .retain(|effect| !removed_conditions.contains(effect.condition));
        self.sync_periodic();
        removed.intersects(ConditionSet::of(&[Quartz, Enchanted]))
            && !self
                .effective()
                .intersects(ConditionSet::of(&[Quartz, Enchanted]))
    }
    pub(crate) fn clear_base(&mut self) -> bool {
        self.remove(self.base())
    }
    pub(crate) fn consume_revive(&mut self) {
        self.remove(Revive.into());
        self.intrinsic = self.intrinsic.without(Revive.into());
        self.profile_intrinsic = self.profile_intrinsic.without(Revive.into());
    }

    pub(crate) fn contact_ailment_guard(&self, condition: Condition) -> bool {
        (self.traits.physical_ailment_guard && PHYSICAL_AILMENTS.contains(condition))
            || (self.traits.magical_ailment_guard && MAGICAL_AILMENTS.contains(condition))
    }
    pub(crate) fn rejects(&self, condition: Condition) -> bool {
        let active = self.effective();
        self.immunity().contains(condition)
            || (active.contains(PhysicalProtection) && PHYSICAL_AILMENTS.contains(condition))
            || ((active.contains(MagicalProtection) || self.traits.magical_ailment_guard)
                && MAGICAL_AILMENTS.contains(condition))
    }
    fn duration(&self, ticks: u32, extended: bool) -> u32 {
        if extended || self.traits.extended_duration {
            ticks.saturating_add(ticks / 4)
        } else {
            ticks
        }
    }
    pub fn apply_hit(&mut self, hit: crate::HitCondition) -> Option<ConditionLabel> {
        let condition = hit.condition.into();
        if STAT_CONDITIONS.contains(condition) {
            return self
                .apply_stat_condition(
                    condition,
                    i16::from(hit.value),
                    DEFAULT_STAT_CONDITION_DURATION,
                    false,
                )
                .then_some(ConditionLabel::StatusDown);
        }
        if self.rejects(condition) {
            return None;
        }
        self.set_effect(ActiveEffect {
            condition,
            remaining: (condition == Paralysis).then(|| self.duration(600, false)),
            magnitude: 0,
        });
        Some(if condition == Weak {
            ConditionLabel::StatusDown
        } else {
            ConditionLabel::Applied
        })
    }

    pub fn apply_stat_condition(
        &mut self,
        condition: Condition,
        value: i16,
        duration: u32,
        strength_boost: bool,
    ) -> bool {
        let Some(sign) = stat_sign(condition) else {
            return false;
        };
        if self.rejects(condition) {
            return false;
        }
        if let Some(opposite) = opposing_stat_condition(condition)
            && self.effective().contains(opposite)
        {
            self.remove(opposite.into());
            return false;
        }
        let strength = i32::from(value).abs();
        let strength = if strength_boost {
            strength * 120 / 100
        } else {
            strength
        };
        let magnitude = (strength.min(i32::from(i16::MAX)) as i16) * sign;
        let retained = self.magnitude(condition);
        self.set_effect(ActiveEffect {
            condition,
            remaining: Some(self.duration(
                if duration == 0 {
                    DEFAULT_STAT_CONDITION_DURATION
                } else {
                    duration
                },
                false,
            )),
            magnitude: if retained.unsigned_abs() > magnitude.unsigned_abs() {
                retained
            } else {
                magnitude
            },
        });
        true
    }

    fn stat_with_conditions(
        &self,
        base: i32,
        positive: Option<Condition>,
        negative: Condition,
    ) -> i32 {
        let percent = i32::from(positive.map_or(0, |condition| self.magnitude(condition)))
            + i32::from(self.magnitude(negative));
        (i64::from(base) + i64::from(base) * i64::from(percent) / 100).clamp(0, i64::from(i32::MAX))
            as i32
    }
    pub(crate) fn attack_with_conditions(&self, base: i32) -> i32 {
        self.stat_with_conditions(base, Some(AttackUp), AttackDown)
    }
    pub(crate) fn defense_with_conditions(&self, base: i32) -> i32 {
        let base = if self.effective().contains(DefenseHalved) {
            base >> 1
        } else {
            base
        };
        self.stat_with_conditions(base, Some(DefenseUp), DefenseDown)
    }
    pub(crate) fn accuracy_with_conditions(&self, base: i32) -> i32 {
        self.stat_with_conditions(base, Some(AccuracyUp), AccuracyDown)
    }
    pub(crate) fn evasion_with_conditions(&self, base: i32) -> i32 {
        self.stat_with_conditions(base, None, EvasionDown)
    }
    pub(crate) fn prepare_buff(&self, buff: Buff, extended: bool) -> PreparedBuff {
        let (condition, duration, element) = match buff {
            Buff::Flare => (Flare, Some(1200), None),
            Buff::Guard => (Guard, Some(1200), None),
            Buff::Acuity => (Acuity, Some(1200), None),
            Buff::PhysicalAilmentGuard { persistent } => {
                (PhysicalProtection, (!persistent).then_some(900), None)
            }
            Buff::MagicalAilmentGuard { persistent } => {
                (MagicalProtection, (!persistent).then_some(900), None)
            }
            Buff::Quartz(element) => (Quartz, Some(1200), Some(element)),
        };
        PreparedBuff {
            effect: (!self.immunity().contains(condition)).then(|| ActiveEffect {
                condition,
                remaining: duration.map(|ticks| self.duration(ticks, extended)),
                magnitude: 0,
            }),
            element,
        }
    }
    /// Remove mutable effects completely. Return whether their enchantment also needs clearing.
    pub(crate) fn cure(&mut self, cure: Cure) -> bool {
        self.remove(match cure {
            Cure::Physical => PHYSICAL_AILMENTS,
            Cure::AntiMagic => STAT_CONDITIONS
                .union(MAGICAL_AILMENTS)
                .union(PROTECTION)
                .union(ConditionSet::of(&[
                    Flare,
                    Guard,
                    Acuity,
                    Revive,
                    DefenseHalved,
                ])),
            Cure::All => self.base(),
        })
    }

    fn sync_periodic(&mut self) {
        let active = self.effective();
        self.periodic
            .retain(|effect| active.contains(effect.condition));
        for (condition, period) in [
            (PoisonMild, POISON_INTERVAL),
            (PoisonSevere, POISON_INTERVAL),
            (RegenerateHp, REGENERATION_INTERVAL),
            (RegenerateTp, REGENERATION_INTERVAL),
        ] {
            if active.contains(condition)
                && !self
                    .periodic
                    .iter()
                    .any(|effect| effect.condition == condition)
            {
                self.periodic.push(PeriodicEffect {
                    condition,
                    remaining: period,
                    period,
                });
            }
        }
        self.periodic.sort_by_key(|effect| effect.condition);
    }
    fn advance(&mut self, enchantment: &mut Option<Element>) -> ConditionTick {
        let mut expired = ConditionSet::EMPTY;
        for effect in &mut self.effects {
            if let Some(remaining) = &mut effect.remaining {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    expired = expired.union(effect.condition.into());
                }
            }
        }
        if expired.contains(Quartz) {
            expired = expired.union(Enchanted.into());
        }
        if self.remove(expired) {
            *enchantment = None;
        }
        let active = self.effective();
        let mut tick = ConditionTick::default();
        for effect in &mut self.periodic {
            effect.remaining -= 1;
            if effect.remaining != 0 {
                continue;
            }
            effect.remaining = effect.period;
            match effect.condition {
                PoisonMild if !active.contains(PoisonSevere) => tick.poison_percent = 1,
                PoisonSevere => tick.poison_percent = 2,
                RegenerateHp => tick.regenerate_hp = true,
                RegenerateTp => tick.regenerate_tp = true,
                _ => {}
            }
        }
        tick
    }
}

impl PreparedBuff {
    pub(crate) fn commit(self, actor: &mut Actor) -> Option<ConditionLabel> {
        let effect = self.effect?;
        actor.conditions.set_effect(effect);
        if let Some(element) = self.element {
            actor.elements.enchantment = Some(element);
            actor.conditions.set_effect(ActiveEffect {
                condition: Enchanted,
                remaining: None,
                magnitude: 0,
            });
        }
        Some(ConditionLabel::Applied)
    }
}

impl Actor {
    fn tick_conditions(&mut self, combat: bool) -> ConditionTick {
        if !combat || !self.available() {
            return ConditionTick::default();
        }
        let tick = self.conditions.advance(&mut self.elements.enchantment);
        if tick.poison_percent != 0 {
            let damage =
                (i64::from(self.equipment.max_hp) * i64::from(tick.poison_percent) / 100).max(1);
            self.hp = (i64::from(self.hp) - damage).max(1) as i32;
        }
        tick
    }

    #[cfg(test)]
    pub(crate) fn advance_conditions(&mut self, combat: bool) -> bool {
        self.tick_conditions(combat).poison_percent != 0
    }
}

#[cfg(test)]
mod poison_tests;
mod regeneration;
#[cfg(test)]
mod regeneration_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod petrify_tests;
