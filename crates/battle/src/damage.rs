//! Damage, guard resolution, and HP application.
//! Equipment and EX parameters are prepared separately from live conditions.
#[cfg(test)]
use crate::Random;
use crate::conditions::Condition;
use crate::{Actor, GuardKind, GuardResult, GuardRule, guard};
pub use resonance_content::menu_data::Element;

pub use resonance_content::battle_action::{HitCondition, HitElement, Power};

pub(crate) const MIN_ATTACK_POWER: u16 = 10;
pub(crate) const CHAIN_POWER_REDUCTION: u16 = 15;

/// Independent decisions made while resolving a hit. Ineligible decisions do not draw.
#[derive(Clone, Copy)]
pub(crate) enum Draw {
    Variance,
    MagicResistance,
    CriticalChance,
    Critical,
    AutoGuard,
    ElementalReduction,
    Stability,
    Avoidance,
    Ailment,
    AilmentResistance,
}

#[cfg(test)]
fn neutral_roll(draw: Draw) -> u16 {
    match draw {
        Draw::Variance | Draw::MagicResistance | Draw::CriticalChance => 0,
        _ => 99,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttackElements {
    pub action: Option<Element>,
    pub enchantment: Option<Element>,
}

pub(crate) fn resolve_element(element: HitElement, actor: &Actor) -> Option<Element> {
    match element {
        HitElement::Inherited => actor
            .elements
            .action
            .or(actor.elements.enchantment)
            .or(actor.equipment.base_element),
        HitElement::Neutral => None,
        HitElement::Element(element) => Some(element),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CombatStats {
    pub slash: i32,
    pub thrust: i32,
    pub defense: i32,
    pub intelligence: i32,
    pub accuracy: i32,
    pub evasion: i32,
    pub level: u8,
}

/// Encounter percentages applied after live stat conditions, independently of equipment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CombatAdjustments {
    pub attack: i8,
    pub defense: i8,
    pub intelligence: i8,
    /// Floor bonuses, expressed as signed fractions of the conditioned stat.
    pub attack_quarters: i8,
    pub defense_eighths: i8,
    pub accuracy_quarters: i8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DamageTraits {
    /// Normal guard also blocks attacks from behind.
    pub rear_guard: bool,
    /// Charged physical attacks break normal guard.
    pub charge_guard_break: bool,
    /// Deal 15 percentage points more damage through normal physical guard.
    pub guard_damage_boost: bool,
    /// Special Guard takes 15% damage instead of 20%.
    pub special_guard_reduction: bool,
    /// Special Guard avoids damage at or below one tenth of maximum HP.
    pub low_hp_special_guard: bool,
    /// Retain one HP while actively using Special Guard.
    pub special_guard_survival: bool,
    /// Physical and elemental stability share one 10% chance.
    pub physical_stability: bool,
    pub elemental_stability: bool,
    /// Prevent hit reactions while chanting.
    pub casting_stability: bool,
    /// Prevent elemental hit reactions while holding a stored spell.
    pub stored_spell_stability: bool,
    /// A 5% chance to halve elemental damage.
    pub elemental_damage_reduction: bool,
    /// Prevent magic hit reactions while running.
    pub run_magic_stability: bool,
    /// Charged stability requires a live physical charge.
    pub charged_neutral_stability: bool,
    pub charged_run_stability: bool,
    /// Prevent ordinary hit reactions.
    pub stability: bool,
    pub alone: bool,
    /// Only the equipped weapon's primary effect selects a species bonus.
    pub weapon_species: Option<u16>,
    /// Combined equipment critical chance bonus, bounded to 100%.
    pub critical_chance_bonus: u16,
    /// Increase physical arte damage.
    pub physical_arte_boost: bool,
    /// Attack Ring increases physical damage.
    pub physical_damage_boost: bool,
    /// Defense Ring reduces physical damage.
    pub physical_damage_reduction: bool,
    /// Magic Ring increases magical damage.
    pub magic_damage_boost: bool,
    /// Reduce incoming damage by 10%.
    pub damage_reduction: bool,
    /// Halve incoming ailment chance and label resisted applications.
    pub ailment_resistance: bool,
    /// Counter guarantees a physical critical against an attacking opponent.
    pub physical_counter: bool,
    /// E. Plus increases elemental physical damage.
    pub elemental_physical_boost: bool,
    /// Variable increases physical attack as HP falls.
    pub variable_attack: bool,
    /// Suppress hits below 1% of maximum HP, before guard and protection.
    pub suppress_small_hits: bool,
    /// A 5% chance to avoid damage.
    pub nullify_damage: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Affinity {
    #[default]
    Normal,
    Weak,
    Resistant,
    Absorb,
    Immune,
}

pub use resonance_content::battle_projectile::DamageKind;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRule {
    pub kind: DamageKind,
    /// Additional artwork emitted at the accepted contact point.
    pub effect: Option<crate::EffectAppearance>,
    /// Permit the contact artwork when the accepted hit is guarded.
    pub effect_on_guard: bool,
    pub sound: Option<crate::Sound>,
    /// Arte contacts do not restore the attacker's TP. The physical damage
    /// branch also uses this classification for the prepared EX bonus.
    pub arte: bool,
    /// Permit attacker pause against the active target, independently of arte cost
    /// classification.
    pub overlimit_pause: bool,
    pub power: Power,
    /// Inherited elements are resolved from the live owner at each contact.
    pub element: HitElement,
    pub prevents_defeat: bool,
    /// Award grade only after this contact confirms an enemy defeat.
    pub finisher_grade: bool,
    /// Restore nominal damage to the attacker on an unguarded, unsuppressed hit.
    pub hp_drain: bool,
    pub guard: GuardRule,
    pub reaction: crate::ReactionRule,
    pub condition: Option<HitCondition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitResult {
    /// Computed damage or absorption amount before the target HP limit.
    pub amount: i32,
    /// Actual HP change: negative for damage, positive for healing.
    pub hp_change: i32,
    pub critical: bool,
    /// Bonus-hit marker, also used by damage-number presentation.
    pub boosted: bool,
    pub affinity: Affinity,
    pub guard: GuardResult,
    pub protection: crate::HitProtection,
    pub reaction_suppressed: bool,
    /// A forced reaction may retain nominal damage while protection prevents HP loss.
    pub reaction_forced: bool,
}

impl HitResult {
    pub fn is_damage(self) -> bool {
        (self.hp_change < 0 || self.reaction_forced)
            && !matches!(self.affinity, Affinity::Absorb | Affinity::Immune)
            && self.protection != crate::HitProtection::Avoided
    }

    /// Damage applied through guard, including a guard break.
    pub fn is_unblocked_damage(self) -> bool {
        self.is_damage() && !matches!(self.guard, GuardResult::Blocked { .. })
    }

    pub fn suppresses_reaction(self) -> bool {
        !self.is_damage()
            || self.protection != crate::HitProtection::None
            || self.reaction_suppressed
    }
}

const BASE_CRITICAL_CHANCE: i32 = 1;
const LUCK_PER_CRITICAL_POINT: i32 = 20;
const CRITICAL_CHANCE_SPREAD: u16 = 4;
const VARIANCE_SPREAD: i16 = 5;

fn percentage(amount: i64, percent: i64) -> i64 {
    amount.saturating_mul(percent) / 100
}

fn adjusted(amount: i64, adjustment: i8) -> i64 {
    amount + percentage(amount, i64::from(adjustment))
}

fn vary(amount: i64, roll: &mut dyn FnMut(Draw) -> u16) -> i64 {
    let variation = (roll(Draw::Variance) as i16) % VARIANCE_SPREAD;
    percentage(amount, 100 + i64::from(variation))
}

fn proficient(stat: i32, bonus: i64) -> i32 {
    (i64::from(stat) * (100 + bonus) / 100).clamp(0, i64::from(i32::MAX)) as i32
}

fn physical_attack(owner: &Actor, kind: DamageKind) -> i32 {
    let stat = match kind {
        DamageKind::Slash => owner.equipment.stats.slash,
        _ => owner.equipment.stats.thrust,
    };
    // Each HP quarter below 75% adds 8%; proficiency applies afterward.
    let variable = if owner.equipment.damage.variable_attack {
        i64::from((3 - owner.hp_percent() / 25).max(0)) * 8
    } else {
        0
    };
    proficient(proficient(stat, variable), i64::from(owner.proficiency))
}

fn physical_bonuses(
    owner: &Actor,
    species: u16,
    element: Option<Element>,
    mut amount: i64,
) -> (i64, bool) {
    if owner.side == crate::Side::Party && owner.equipment.contact.technique_balance >= 100 {
        amount += percentage(amount, 5);
    }
    let charge = match owner.control_ex_state.charge {
        crate::ChargeLevel::None => 0,
        crate::ChargeLevel::Normal => 20,
        crate::ChargeLevel::Strong => 40,
    };
    amount += percentage(amount, charge);
    if owner.equipment.damage.elemental_physical_boost && element.is_some() {
        amount += percentage(amount, 10);
    }
    let species_match = owner.equipment.damage.weapon_species == Some(species);
    if species_match {
        amount += amount >> 3;
    }
    (
        amount,
        owner.control_ex_state.charge.charged() || species_match,
    )
}

/// Calculate magnitude and consume ordinary magic guarding. Damage truncates each integer
/// stage; physical hit power follows accuracy and criticals, while magic hit power
/// precedes resistance. Fixed power skips statistics, criticals and variance, but
/// physical hits still receive progression, charge, elemental and species bonuses.
fn calculate_amount(
    owner: &Actor,
    target: &mut Actor,
    activity: crate::Activity,
    rule: HitRule,
    attack_power: u16,
    available_counts: [u8; 2],
    roll: &mut dyn FnMut(Draw) -> u16,
) -> (i64, bool, bool) {
    let element = resolve_element(rule.element, owner);
    let physical = rule.kind != DamageKind::Magic;
    let attack_traits = &owner.equipment.damage;
    let defense_traits = &target.equipment.damage;
    let attacking = owner.conditions.effective();
    let defending = target.conditions.effective();
    let mut mitigation = 100;
    if !physical {
        if defending.contains(Condition::MagicDefenseUp) {
            mitigation -= 20;
        }
        if defending.contains(Condition::MagicDefenseDown) {
            mitigation += 20;
        }
    }

    let mut critical = false;
    let amount = if let Power::Fixed(amount) = rule.power {
        i64::from(amount)
    } else {
        let mut bonus = 100;
        let mut base = if physical {
            let attack = owner
                .conditions
                .attack_with_conditions(physical_attack(owner, rule.kind));
            let defense = target
                .conditions
                .defense_with_conditions(target.equipment.stats.defense);
            let attack = i64::from(attack)
                + i64::from(attack / 4) * i64::from(owner.combat_adjustments.attack_quarters);
            let defense = i64::from(defense)
                + i64::from(defense / 8) * i64::from(target.combat_adjustments.defense_eighths);
            let attack = adjusted(attack, owner.combat_adjustments.attack);
            let defense = adjusted(defense, target.combat_adjustments.defense);
            let mut accuracy = owner.conditions.accuracy_with_conditions(proficient(
                owner.equipment.stats.accuracy,
                i64::from(owner.proficiency),
            ));
            if attacking.contains(Condition::Acuity) {
                accuracy = proficient(accuracy, 20);
            }
            let accuracy = i64::from(accuracy)
                + i64::from(accuracy / 4) * i64::from(owner.combat_adjustments.accuracy_quarters);
            let evasion = target
                .conditions
                .evasion_with_conditions(target.equipment.stats.evasion);
            let accuracy_percent = if activity == crate::Activity::Stunned {
                100
            } else {
                90 + ((accuracy - i64::from(evasion) - 5) >> 1).clamp(-10, 10)
            };
            let base = ((attack >> 1) - defense).max(1);
            let base = percentage(base, i64::from(attack_power));
            let mut base = vary(base, roll);
            if attacking.contains(Condition::Flare) {
                base = base * 12 / 10;
            }
            if defending.contains(Condition::Guard) {
                base = base * 10 / 12;
            }
            if attacking.intersects(crate::conditions::PROTECTION) {
                base = percentage(base, 80);
            }
            if attack_traits.physical_damage_boost {
                base = percentage(base, 110);
            }
            if defense_traits.physical_damage_reduction {
                base = percentage(base, 90);
            }
            if rule.arte && attack_traits.physical_arte_boost {
                base += percentage(base, 20);
            }
            if attack_traits.alone && available_counts[owner.side as usize] == 1 {
                base = base * 12 / 10;
            }
            if defense_traits.alone && available_counts[target.side as usize] == 1 {
                base = base * 10 / 12;
            }
            percentage(base, accuracy_percent)
        } else {
            if attacking.intersects(crate::conditions::PROTECTION) {
                bonus -= 20;
            }
            if attack_traits.magic_damage_boost {
                bonus += 10;
            }
            if attacking.contains(Condition::MagicAttackUp) {
                bonus += 20;
            }
            if attacking.contains(Condition::MagicAttackDown) {
                bonus -= 20;
            }
            let intelligence = proficient(
                owner.equipment.stats.intelligence,
                i64::from(owner.proficiency),
            );
            let intelligence = adjusted(
                i64::from(intelligence),
                owner.combat_adjustments.intelligence,
            );
            let intelligence = match rule.power {
                Power::Percent(percent) => percentage(intelligence, i64::from(percent)),
                _ => intelligence,
            };
            let level = u16::from(target.equipment.stats.level);
            let resistance = (i64::from(target.equipment.stats.intelligence) >> 3)
                + if level == 0 {
                    0
                } else {
                    i64::from(roll(Draw::MagicResistance) % level)
                };
            let resistance = adjusted(resistance, target.combat_adjustments.intelligence);
            percentage(
                intelligence - resistance,
                i64::from(owner.casting_state.power_percent),
            )
        };
        if physical || owner.equipment.recovery.lucky {
            let chance = (BASE_CRITICAL_CHANCE
                + (i32::from(owner.equipment.luck) - i32::from(target.equipment.luck)).max(0)
                    / LUCK_PER_CRITICAL_POINT
                + i32::from(attack_traits.critical_chance_bonus)
                + i32::from(roll(Draw::CriticalChance) % CRITICAL_CHANCE_SPREAD))
            .clamp(0, 100);
            critical =
                (physical && attack_traits.physical_counter && activity == crate::Activity::Action)
                    || i32::from(roll(Draw::Critical) % 100) < chance;
            if critical {
                base += base >> 1;
            }
        }
        if physical {
            if let Power::Percent(percent) = rule.power {
                base = percentage(base, i64::from(percent));
            }
        } else {
            base = vary(guard::soften_magic(target, base), roll);
        }
        percentage(base, bonus)
    };
    let (amount, boosted) = if physical {
        physical_bonuses(owner, target.species, element, amount)
    } else {
        (amount, false)
    };
    let mut amount = if matches!(rule.power, Power::Fixed(_)) {
        let amount = (amount * mitigation + 50) / 100;
        if physical {
            amount
        } else {
            guard::soften_magic(target, amount)
        }
    } else {
        percentage(amount, mitigation)
    };
    if target.equipment.damage.damage_reduction {
        amount -= percentage(amount, 10);
    }
    (amount.clamp(0, i64::from(i32::MAX)), critical, boosted)
}

#[cfg(test)]
pub(crate) fn resolve(
    owner: &Actor,
    target: &mut Actor,
    rule: HitRule,
    attack_power: u16,
    incoming: [f32; 3],
    roll: &mut dyn FnMut(Draw) -> u16,
    all_divide: bool,
) -> HitResult {
    resolve_with_condition(
        owner,
        target,
        crate::Activity::Idle,
        rule,
        attack_power,
        incoming,
        roll,
        all_divide,
        [2; 2],
    )
    .0
}

/// Resolve a contact in order: affinity, avoidance, guard/protection, HP, then ailments.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_with_condition(
    owner: &Actor,
    target: &mut Actor,
    activity: crate::Activity,
    rule: HitRule,
    attack_power: u16,
    incoming: [f32; 3],
    roll: &mut dyn FnMut(Draw) -> u16,
    all_divide: bool,
    available_counts: [u8; 2],
) -> (HitResult, Option<crate::conditions::ConditionLabel>) {
    resolve_contact(
        owner,
        target,
        activity,
        rule,
        attack_power,
        incoming,
        roll,
        all_divide,
        available_counts,
        ContactMode::Ordinary,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactMode {
    Ordinary,
    Captured,
    Unison { held: bool, bonus: bool },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_contact(
    owner: &Actor,
    target: &mut Actor,
    activity: crate::Activity,
    rule: HitRule,
    attack_power: u16,
    incoming: [f32; 3],
    roll: &mut dyn FnMut(Draw) -> u16,
    all_divide: bool,
    available_counts: [u8; 2],
    mode: ContactMode,
) -> (HitResult, Option<crate::conditions::ConditionLabel>) {
    use crate::{HitProtection, conditions::ConditionLabel};
    let unison = matches!(mode, ContactMode::Unison { .. });
    let held = matches!(
        mode,
        ContactMode::Captured | ContactMode::Unison { held: true, .. }
    );
    let hp_before = target.hp;
    let element = resolve_element(rule.element, owner);
    let mut affinity = target.equipment.affinities[element.map_or(0, |e| e as usize + 1)];
    if unison && affinity == Affinity::Absorb {
        affinity = Affinity::Normal;
    }
    let (mut amount, critical, boosted) = calculate_amount(
        owner,
        target,
        activity,
        rule,
        attack_power,
        available_counts,
        roll,
    );
    amount = match affinity {
        Affinity::Weak => amount * 3 / 2,
        Affinity::Resistant => amount / 2,
        _ => amount,
    };
    if all_divide && !unison {
        amount /= 2;
    }
    let mut hit = HitResult {
        amount: 0,
        hp_change: 0,
        critical: false,
        boosted: false,
        affinity,
        guard: GuardResult::None,
        protection: HitProtection::None,
        reaction_suppressed: rule.reaction.suppress,
        reaction_forced: false,
    };
    if affinity == Affinity::Immune {
        return (hit, None);
    }
    if affinity == Affinity::Absorb {
        hit.amount = amount.clamp(1, i64::from(i32::MAX)) as i32;
        if hp_before > 0 {
            target.recover_flat_hp(hit.amount);
        }
        hit.hp_change = target.hp - hp_before;
        return (hit, None);
    }

    let overlimit = target.overlimit.is_active();
    let (protection, shift) = target.reaction.protection.hit(
        target.side,
        target.reaction.stagger.window,
        rule.reaction.hits_down,
    );
    let special_guard =
        rule.guard.enabled && target.guard.active && target.guard.kind == GuardKind::Special;
    let small_hit = target.equipment.damage.suppress_small_hits
        && amount < i64::from(target.equipment.max_hp) / 100;
    let force_reaction = rule.reaction.force && !overlimit && !special_guard && !small_hit;
    let low_hp_guard = target.equipment.damage.low_hp_special_guard
        && special_guard
        && i64::from(target.hp) * 10 <= i64::from(target.equipment.max_hp);
    let avoids = (!unison && protection == HitProtection::Avoided) || low_hp_guard || small_hit;
    let nullified =
        !avoids && target.equipment.damage.nullify_damage && roll(Draw::Avoidance) % 100 < 5;
    if avoids || (nullified && !unison) {
        hit.protection = HitProtection::Avoided;
        if low_hp_guard && !unison {
            hit.guard = GuardResult::Blocked {
                first: false,
                special: true,
            };
        }
        if force_reaction {
            hit.amount = amount.clamp(1, i64::from(i32::MAX)) as i32;
            hit.critical = critical;
            hit.boosted = boosted;
            hit.protection = HitProtection::None;
            hit.reaction_suppressed = false;
            hit.reaction_forced = true;
            target.guard.active = false;
        }
        return (hit, nullified.then_some(ConditionLabel::ExSkillEffect));
    }

    let mut label = nullified.then_some(ConditionLabel::ExSkillEffect);
    if element.is_some()
        && target.equipment.damage.elemental_damage_reduction
        && roll(Draw::ElementalReduction) % 100 < 5
    {
        amount /= 2;
        label = Some(ConditionLabel::ExSkillEffect);
    }
    hit.protection = protection;
    let mut protected_amount = amount >> shift;
    if overlimit {
        protected_amount = protected_amount.min(amount / 2);
        if hit.protection == HitProtection::None {
            hit.protection = HitProtection::Armored;
        }
    }
    if !rule.guard.enabled {
        target.guard.active = false;
    } else if !target.guard.active
        && hit.protection == HitProtection::None
        && rule.kind != DamageKind::Magic
        && target.time_stop == 0
        && (target.control != crate::Control::Enemy
            || target.reaction.armor.received >= target.reaction.armor.threshold)
    {
        guard::attempt(target, activity, affinity, || roll(Draw::AutoGuard));
    }
    if hit.protection == HitProtection::None && !target.guard.active {
        let (stable, stability_label) =
            conditional_stability(target, activity, rule.kind, element, roll);
        label = stability_label.or(label);
        if stable {
            hit.protection = HitProtection::Armored;
        }
    }
    // Armor takes the contact before guarding; the next hit can interrupt or block.
    let armor = &mut target.reaction.armor;
    if target.time_stop == 0 && armor.received < armor.threshold {
        armor.received = armor
            .received
            .saturating_add(rule.reaction.armor_damage)
            .min(armor.threshold);
        if hit.protection == HitProtection::None {
            hit.protection = HitProtection::Armored;
        }
    }
    if !hit.reaction_suppressed && (hit.protection == HitProtection::None || overlimit) {
        hit.guard = guard::resolve_from(
            owner,
            target,
            rule.kind,
            rule.guard,
            affinity,
            incoming,
            &mut amount,
        );
    }
    amount = amount.min(protected_amount);
    if matches!(mode, ContactMode::Unison { bonus: true, .. }) {
        amount += amount / 10;
    }
    if all_divide && unison {
        amount /= 2;
    }
    hit.amount = amount.clamp(1, i64::from(i32::MAX)) as i32;
    let survives = rule.prevents_defeat
        || (target.equipment.damage.special_guard_survival
            && target.guard.active
            && target.guard.kind == GuardKind::Special);
    target.hp = target
        .hp
        .saturating_sub(hit.amount)
        .max(i32::from(hp_before > 0 && survives));
    hit.hp_change = target.hp - hp_before;
    hit.critical = critical && !matches!(hit.guard, GuardResult::Blocked { .. });
    hit.boosted = boosted;
    if target.hp == 0 {
        target.guard.active = false;
        hit.guard = GuardResult::None;
        hit.protection = HitProtection::None;
        hit.reaction_suppressed = false;
    }
    if unison && affinity != Affinity::Immune {
        hit.guard = GuardResult::None;
        hit.protection = HitProtection::None;
        hit.reaction_suppressed = false;
        hit.reaction_forced = true;
        target.guard.active = false;
    }
    if !held && hit.is_unblocked_damage() && target.hp > 0 {
        if !hit.reaction_suppressed || !target.guard.active {
            label = rule
                .condition
                .and_then(|condition| apply_contact_condition(target, condition, roll))
                .or(label);
        }
        if !hit.suppresses_reaction() && target.time_stop == 0 {
            target.reaction.stagger.received = target
                .reaction
                .stagger
                .received
                .saturating_add(rule.reaction.stagger);
        }
    }
    // Keep HP mitigation, condition eligibility and stagger decisions already made.
    if force_reaction {
        hit.guard = GuardResult::None;
        hit.protection = HitProtection::None;
        hit.reaction_suppressed = false;
        hit.reaction_forced = true;
        target.guard.active = false;
    }
    (hit, label)
}

/// Luck lowers the incoming chance; equipped resistance gets a separate saving throw.
fn apply_contact_condition(
    target: &mut Actor,
    condition: HitCondition,
    roll: &mut dyn FnMut(Draw) -> u16,
) -> Option<crate::conditions::ConditionLabel> {
    use crate::conditions::ConditionLabel;
    if condition.chance == 0 {
        return None;
    }
    if target.conditions.contact_ailment_guard(condition.condition) {
        return Some(ConditionLabel::ExSkillEffect);
    }
    if target.conditions.rejects(condition.condition) {
        return None;
    }
    let luck = i32::from(target.equipment.luck);
    let chance = i32::from(condition.chance) * (100 - luck / 20) / 100;
    if i32::from(roll(Draw::Ailment) % 100) >= chance {
        return None;
    }
    let resistance = if target.equipment.damage.ailment_resistance {
        Some(ConditionLabel::ExSkillEffect)
    } else if target
        .conditions
        .effective()
        .contains(Condition::AilmentResistance)
    {
        Some(ConditionLabel::EquipmentEffect)
    } else {
        None
    };
    if let Some(label) = resistance {
        let resist_chance = 20 + luck / 10;
        if i32::from(roll(Draw::AilmentResistance) % 100) < resist_chance {
            return Some(label);
        }
    }
    target.conditions.apply_hit(condition)
}

/// Deterministic stability wins before the optional physical/elemental skill roll.
fn conditional_stability(
    target: &Actor,
    activity: crate::Activity,
    kind: DamageKind,
    element: Option<Element>,
    roll: &mut dyn FnMut(Draw) -> u16,
) -> (bool, Option<crate::conditions::ConditionLabel>) {
    let traits = &target.equipment.damage;
    let running = target.movement.locomotion == crate::Locomotion::Run;
    let charged = target.control_ex_state.charge.charged();
    if traits.stability
        || (traits.run_magic_stability && kind == DamageKind::Magic && running)
        || (traits.charged_run_stability && charged && running)
        || (traits.charged_neutral_stability && charged && element.is_none())
        || (traits.casting_stability
            && matches!(activity, crate::Activity::Casting { held: false }))
        || (traits.stored_spell_stability && element.is_some() && target.stored_spell.is_some())
    {
        return (true, None);
    }
    let stable = ((traits.physical_stability && kind != DamageKind::Magic)
        || (traits.elemental_stability && element.is_some()))
        && roll(Draw::Stability) % 100 < 10;
    (
        stable,
        stable.then_some(crate::conditions::ConditionLabel::ExSkillEffect),
    )
}

#[cfg(test)]
mod ailment_tests;
#[cfg(test)]
mod conditions_tests;
#[cfg(test)]
mod contact_ex_tests;
#[cfg(test)]
mod item_armor_tests;
#[cfg(test)]
mod overlimit_contract_tests;
#[cfg(test)]
mod protection_ex_tests;
#[cfg(test)]
mod technique_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Side, tests::actor};

    pub(super) fn rule(kind: DamageKind, power: Power) -> HitRule {
        HitRule {
            sound: None,
            effect_on_guard: false,
            effect: None,
            overlimit_pause: true,
            condition: None,
            arte: false,
            reaction: Default::default(),
            kind,
            power,
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            finisher_grade: false,
            hp_drain: false,
            guard: GuardRule::default(),
        }
    }

    #[test]
    fn element_selection_preserves_explicit_neutral_and_override_priority() {
        for side in [Side::Party, Side::Enemy] {
            let mut owner = actor(side);
            assert_eq!(resolve_element(HitElement::Inherited, &owner), None);
            owner.equipment.base_element = Some(Element::Fire);
            assert_eq!(
                resolve_element(HitElement::Inherited, &owner),
                Some(Element::Fire)
            );
            owner.elements.enchantment = Some(Element::Water);
            assert_eq!(
                resolve_element(HitElement::Inherited, &owner),
                Some(Element::Water)
            );
            owner.elements.action = Some(Element::Wind);
            assert_eq!(
                resolve_element(HitElement::Inherited, &owner),
                Some(Element::Wind)
            );
            assert_eq!(resolve_element(HitElement::Neutral, &owner), None);
            assert_eq!(
                resolve_element(HitElement::Element(Element::Ice), &owner),
                Some(Element::Ice)
            );
            owner.elements.action = None;
            owner.elements.enchantment = None;
            assert_eq!(
                resolve_element(HitElement::Inherited, &owner),
                Some(Element::Fire)
            );
        }
    }

    #[test]
    fn armor_takes_hits_before_guarding_and_exposes_the_following_contact() {
        let owner = actor(Side::Party);
        let mut target = actor(Side::Enemy);
        target.guard.reduction = 75;
        target.guard.break_pressure = 10;
        target.reaction.armor.threshold = 2;
        let mut rule = rule(DamageKind::Slash, Power::Fixed(8));
        rule.reaction.armor_damage = 1;
        for (guarding, spent, amount, armored) in [
            (true, 1, 8, true),
            (false, 2, 8, true),
            (true, 2, 2, false),
            (false, 2, 8, false),
        ] {
            target.guard.active = guarding;
            let hit = resolve(
                &owner,
                &mut target,
                rule,
                100,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert_eq!(hit.hp_change, -amount);
            assert_eq!(target.reaction.armor.received, spent);
            assert_eq!(hit.protection == crate::HitProtection::Armored, armored);
            assert_eq!(
                matches!(hit.guard, GuardResult::Blocked { .. }),
                guarding && !armored
            );
        }
        target.control = crate::Control::Enemy;
        target.guard.enemy_chance = 100;
        target.reaction.armor.received = 0;
        for armored in [true, true, false] {
            target.guard.active = false;
            let hit = resolve(
                &owner,
                &mut target,
                rule,
                100,
                [0.; 3],
                &mut |draw| {
                    assert!(!armored || !matches!(draw, Draw::AutoGuard));
                    0
                },
                false,
            );
            assert_eq!(hit.protection == crate::HitProtection::Armored, armored);
            assert_eq!(matches!(hit.guard, GuardResult::Blocked { .. }), !armored);
        }
    }

    #[test]
    fn armor_budget_saturates_while_time_stopped_and_lethal_hits_keep_their_own_gates() {
        let owner = actor(Side::Party);
        let mut rule = rule(DamageKind::Slash, Power::Fixed(8));
        rule.reaction.armor_damage = 10;
        for affinity in [Affinity::Immune, Affinity::Absorb, Affinity::Normal] {
            let mut target = actor(Side::Enemy);
            target.equipment.affinities[0] = affinity;
            target.reaction.armor.threshold = 255;
            target.reaction.armor.received = 250;
            let hit = resolve(
                &owner,
                &mut target,
                rule,
                100,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert_eq!(
                hit.protection == crate::HitProtection::Armored,
                affinity == Affinity::Normal
            );
            let expected = if affinity == Affinity::Normal {
                255
            } else {
                250
            };
            assert_eq!(target.reaction.armor.received, expected);
            target.time_stop = u16::MAX;
            let hit = resolve(
                &owner,
                &mut target,
                rule,
                100,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert!(hit.protection != crate::HitProtection::Armored);
            assert_eq!(target.reaction.armor.received, expected);
        }
        let mut target = actor(Side::Enemy);
        target.hp = 8;
        target.reaction.armor.threshold = 20;
        let hit = resolve(
            &owner,
            &mut target,
            rule,
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(target.hp, 0);
        assert!(hit.protection != crate::HitProtection::Armored);
        assert_eq!(target.reaction.armor.received, 10);
    }

    #[test]
    fn hourglass_stop_keeps_damage_but_prevents_guard_and_reaction() {
        let owner = actor(Side::Party);
        let mut target = actor(Side::Enemy);
        target.control = crate::Control::Enemy;
        target.guard.enemy_chance = 100;
        target.guard.break_pressure = 10;
        target.time_stop = u16::MAX;
        let rule = rule(DamageKind::Slash, Power::Fixed(8));
        let mut random = Random::new(1);
        let hit = resolve(
            &owner,
            &mut target,
            rule,
            100,
            [0.; 3],
            &mut |_| random.next_u16(),
            false,
        );
        assert_eq!(hit.hp_change, -8);
        assert!(!target.guard.active);
        assert!(
            crate::reaction::respond(
                &crate::tests::actor(Side::Enemy),
                &mut target,
                rule.reaction,
                hit,
                [0.; 3],
                false
            )
            .is_none()
        );
    }

    #[test]
    fn consecutive_hits_apply_feedback_and_replay_deterministically() {
        let replay = |seed| {
            let mut owner = actor(Side::Party);
            owner.equipment.stats.slash = 160;
            let mut target = actor(Side::Enemy);
            target.equipment.stats.defense = 20;
            target.equipment.max_hp = 300;
            target.hp = target.equipment.max_hp;
            target.equipment.affinities[Element::Fire as usize + 1] = Affinity::Resistant;
            let mut hit = rule(DamageKind::Slash, Power::Normal);
            hit.element = HitElement::Element(Element::Fire);
            let mut random = Random::new(seed);
            let mut hits = Vec::new();
            for _ in 0..32 {
                let before = target.hp;
                let result = resolve(
                    &owner,
                    &mut target,
                    hit,
                    100,
                    [0.; 3],
                    &mut |_| random.next_u16(),
                    false,
                );
                assert!(result.amount > 0 && target.hp < before);
                assert_eq!(result.hp_change, target.hp - before);
                assert!((0..=target.equipment.max_hp).contains(&target.hp));
                assert_eq!(result.affinity, Affinity::Resistant);
                assert!(result.is_unblocked_damage());
                assert!(!result.suppresses_reaction());
                hits.push((result, target.hp));
                if target.hp == 0 {
                    break;
                }
            }
            assert!(hits.len() > 1, "exercise a continuous sequence of hits");
            assert_eq!(target.hp, 0, "hits must eventually defeat the target");
            hits
        };
        assert_eq!(replay(7), replay(7));
    }

    #[test]
    fn critical_hits_increase_stat_based_damage_but_fixed_power_stays_fixed() {
        let mut owner = actor(Side::Party);
        owner.casting_state.power_percent = 100;
        owner.equipment.stats.slash = 200;
        let mut target = actor(Side::Enemy);
        target.hp = 1000;
        target.equipment.max_hp = 1000;
        owner.equipment.stats.intelligence = 200;
        for (kind, lucky) in [
            (DamageKind::Slash, false),
            (DamageKind::Magic, true),
            (DamageKind::Magic, false),
        ] {
            owner.equipment.recovery.lucky = lucky;
            for power in [Power::Normal, Power::Percent(150), Power::Fixed(40)] {
                let hit = rule(kind, power);
                let ordinary = resolve(
                    &owner,
                    &mut target.clone(),
                    hit,
                    100,
                    [0.; 3],
                    &mut neutral_roll,
                    false,
                );
                let critical = resolve(
                    &owner,
                    &mut target.clone(),
                    hit,
                    100,
                    [0.; 3],
                    &mut |draw| match draw {
                        Draw::Critical => 0,
                        _ => neutral_roll(draw),
                    },
                    false,
                );
                if matches!(power, Power::Fixed(_)) {
                    assert_eq!(critical.amount, 40);
                    assert!(!critical.critical);
                } else if kind == DamageKind::Magic && !lucky {
                    assert_eq!(critical, ordinary);
                } else {
                    assert!(critical.critical && critical.amount > ordinary.amount);
                }
                assert_eq!(critical.hp_change, -critical.amount);
            }
        }
    }

    #[test]
    fn native_rolls_have_explicit_variance_and_critical_boundaries() {
        let mut owner = actor(Side::Party);
        owner.casting_state.power_percent = 100;
        owner.equipment.stats.slash = 200;
        owner.equipment.stats.accuracy = 25;
        let mut target = actor(Side::Enemy);
        target.hp = 1000;
        for (bonus, chance_roll, roll, critical) in [
            (0, 0, 0, true),
            (0, 0, 1, false),
            (0, 3, 3, true),
            (0, 3, 4, false),
            (25, 0, 25, true),
            (25, 0, 26, false),
            (100, 0, 99, true),
        ] {
            owner.equipment.damage.critical_chance_bonus = bonus;
            let result = resolve(
                &owner,
                &mut target.clone(),
                rule(DamageKind::Slash, Power::Normal),
                100,
                [0.; 3],
                &mut |draw| match draw {
                    Draw::Critical => roll,
                    Draw::CriticalChance => chance_roll,
                    _ => neutral_roll(draw),
                },
                false,
            );
            assert_eq!(result.critical, critical);
        }
        owner.equipment.damage.critical_chance_bonus = 0;
        owner.equipment.stats.intelligence = 100;
        for kind in [DamageKind::Slash, DamageKind::Magic] {
            for (variance, expected) in [((-4i16) as u16, 96), (0, 100), (4, 104)] {
                let result = resolve(
                    &owner,
                    &mut target.clone(),
                    rule(kind, Power::Normal),
                    100,
                    [0.; 3],
                    &mut |draw| match draw {
                        Draw::Variance => variance,
                        _ => neutral_roll(draw),
                    },
                    false,
                );
                assert_eq!(result.amount, expected);
            }
        }
        // Odd attack, chain strength, accuracy and hit power each truncate separately.
        owner.equipment.stats.slash = 301;
        owner.equipment.stats.accuracy = 100;
        target.equipment.stats.defense = 50;
        target.equipment.stats.evasion = 85;
        for (variance, critical, ordinary, stunned) in
            [((-4i16) as u16, 99, 98, 105), (4, 0, 161, 171)]
        {
            for (activity, expected) in [
                (crate::Activity::Idle, ordinary),
                (crate::Activity::Stunned, stunned),
            ] {
                let (hit, _) = resolve_contact(
                    &owner,
                    &mut target.clone(),
                    activity,
                    rule(DamageKind::Slash, Power::Percent(130)),
                    85,
                    [0.; 3],
                    &mut |draw| match draw {
                        Draw::Variance => variance,
                        Draw::Critical => critical,
                        _ => neutral_roll(draw),
                    },
                    false,
                    [1, 1],
                    ContactMode::Ordinary,
                );
                assert_eq!(hit.amount, expected);
            }
        }
    }

    #[test]
    fn weapon_species_bonus_affects_only_matching_physical_targets() {
        let mut owner = actor(Side::Party);
        owner.casting_state.power_percent = 100;
        owner.equipment.stats.accuracy = 25;
        let mut target = actor(Side::Enemy);
        target.hp = 1000;
        target.species = 2;
        for (base, power, ordinary, matching) in [
            (7, Power::Normal, 7, 7),
            (8, Power::Normal, 8, 9),
            (9, Power::Normal, 9, 10),
            (9, Power::Percent(150), 13, 14),
            (9, Power::Fixed(9), 9, 10),
        ] {
            owner.equipment.stats.slash = base * 2;
            owner.equipment.stats.thrust = base * 2;
            owner.equipment.stats.intelligence = base;
            for kind in [DamageKind::Slash, DamageKind::Thrust, DamageKind::Magic] {
                for species in [None, Some(2), Some(3)] {
                    owner.equipment.damage.weapon_species = species;
                    let result = resolve(
                        &owner,
                        &mut target.clone(),
                        rule(kind, power),
                        100,
                        [0.; 3],
                        &mut neutral_roll,
                        false,
                    );
                    let boosted = kind != DamageKind::Magic && species == Some(target.species);
                    assert_eq!(result.boosted, boosted);
                    assert_eq!(result.amount, if boosted { matching } else { ordinary });
                }
            }
        }
    }

    #[test]
    fn physical_guard_reduces_damage_and_stagger_until_it_breaks() {
        let owner = actor(Side::Party);
        for (active, enabled, breaks, expected, amount, stagger) in [
            (false, true, false, GuardResult::None, 80, 5),
            (
                true,
                true,
                false,
                GuardResult::Blocked {
                    first: true,
                    special: false,
                },
                20,
                0,
            ),
            (true, true, true, GuardResult::Broken, 80, 5),
            (true, false, false, GuardResult::None, 80, 5),
        ] {
            let mut target = actor(Side::Enemy);
            target.hp = 500;
            target.equipment.max_hp = 500;
            target.guard.active = active;
            target.guard.reduction = 75;
            target.guard.break_pressure = 10;
            let mut hit = rule(DamageKind::Slash, Power::Fixed(80));
            hit.guard = GuardRule {
                enabled,
                suppress_sound: false,
                pressure: 1,
                breaks,
                unbreakable: false,
            };
            hit.reaction.stagger = 5;
            let result = resolve(
                &owner,
                &mut target,
                hit,
                100,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert_eq!(result.guard, expected);
            assert_eq!(result.amount, amount);
            assert_eq!(result.hp_change, -amount);
            assert_eq!(target.hp, 500 - amount);
            assert_eq!(target.reaction.stagger.received, stagger);
            assert_eq!(
                target.guard.active,
                matches!(expected, GuardResult::Blocked { .. })
            );
            assert!(!result.critical);
        }
    }

    #[test]
    fn magic_consumes_normal_guard_but_special_guard_still_blocks() {
        let mut owner = actor(Side::Party);
        owner.casting_state.power_percent = 100;
        owner.equipment.stats.intelligence = 200;
        for (power, kind, amount) in [
            (Power::Normal, GuardKind::Normal, 100),
            (Power::Normal, GuardKind::Special, 40),
            (Power::Fixed(200), GuardKind::Normal, 100),
            (Power::Fixed(200), GuardKind::Special, 40),
        ] {
            let mut target = actor(Side::Enemy);
            target.hp = 1000;
            target.guard.active = true;
            target.guard.break_pressure = 10;
            target.guard.kind = kind;
            let result = resolve(
                &owner,
                &mut target,
                rule(DamageKind::Magic, power),
                100,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert_eq!(result.amount, amount);
            assert_eq!(target.guard.active, kind == GuardKind::Special);
            assert_eq!(result.is_unblocked_damage(), kind == GuardKind::Normal);
            assert_eq!(
                matches!(result.guard, GuardResult::Blocked { special: true, .. }),
                kind == GuardKind::Special
            );
        }
    }

    #[test]
    fn unguardable_physical_hits_clear_guard_without_selecting_automatic_guard() {
        let owner = actor(Side::Enemy);
        let mut target = actor(Side::Party);
        target.control = crate::Control::Auto;
        target.guard.active = true;
        target.guard.auto_chance = 100;
        let mut rule = rule(DamageKind::Slash, Power::Fixed(20));
        rule.guard.enabled = false;
        let mut random = Random::new(1);
        let hit = resolve(
            &owner,
            &mut target,
            rule,
            100,
            [0.; 3],
            &mut |_| random.next_u16(),
            false,
        );
        assert_eq!((hit.amount, hit.guard), (20, GuardResult::None));
        assert!(!target.guard.active);
    }

    #[test]
    fn magic_uses_live_cast_strength_and_random_current_resistance() {
        let mut owner = actor(Side::Party);
        owner.equipment.stats.intelligence = 200;
        let mut target = actor(Side::Enemy);
        target.hp = 2000;
        target.equipment.stats.intelligence = 80;
        target.equipment.stats.level = 10;
        let hit = rule(DamageKind::Magic, Power::Percent(150));
        let mut roll = |draw| match draw {
            Draw::MagicResistance => 29,
            _ => neutral_roll(draw),
        };
        let fresh = resolve(
            &owner,
            &mut target.clone(),
            hit,
            100,
            [0.; 3],
            &mut roll,
            false,
        );
        assert_eq!(fresh.amount, 1);
        owner.casting_state.power_percent = 100;
        let baseline = resolve(
            &owner,
            &mut target.clone(),
            hit,
            100,
            [0.; 3],
            &mut roll,
            false,
        );
        let mut resistant_target = target.clone();
        resistant_target.equipment.stats.level = 30;
        let resisted = resolve(
            &owner,
            &mut resistant_target,
            hit,
            100,
            [0.; 3],
            &mut roll,
            false,
        );
        assert_eq!((baseline.amount, resisted.amount), (281, 261));
        owner.attack_power = 10;
        owner.casting_state.power_percent = 200;
        let charged = resolve(
            &owner,
            &mut target.clone(),
            hit,
            10,
            [0.; 3],
            &mut roll,
            false,
        );
        assert_eq!(charged.amount, baseline.amount * 2);
        owner.equipment.damage.magic_damage_boost = true;
        let boosted = resolve(
            &owner,
            &mut target.clone(),
            hit,
            200,
            [0.; 3],
            &mut roll,
            false,
        );
        assert!(boosted.amount > charged.amount);
        let divided = resolve(&owner, &mut target, hit, 200, [0.; 3], &mut roll, true);
        assert_eq!(divided.amount, boosted.amount / 2);
    }

    #[test]
    fn affinity_and_defeat_prevention_bound_large_damage_and_absorption() {
        let owner = actor(Side::Party);
        for (affinity, expected, change) in [
            (Affinity::Normal, 21, -21),
            (Affinity::Weak, 31, -31),
            (Affinity::Resistant, 10, -10),
            (Affinity::Absorb, 21, 21),
            (Affinity::Immune, 0, 0),
        ] {
            let mut target = actor(Side::Enemy);
            target.equipment.affinities[0] = affinity;
            let hit = resolve(
                &owner,
                &mut target,
                rule(DamageKind::Slash, Power::Fixed(21)),
                100,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert_eq!((hit.amount, hit.hp_change), (expected, change));
        }
        let mut target = actor(Side::Enemy);
        target.hp = 100_000;
        target.equipment.max_hp = 200_000;
        let mut hit = rule(DamageKind::Slash, Power::Fixed(u16::MAX));
        let result = resolve(
            &owner,
            &mut target,
            hit,
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(
            (result.amount, result.hp_change, target.hp),
            (65_535, -65_535, 34_465)
        );
        target.equipment.affinities[0] = Affinity::Absorb;
        let result = resolve(
            &owner,
            &mut target,
            hit,
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!((result.hp_change, target.hp), (65_535, 100_000));
        target.conditions = crate::conditions::Conditions::new(crate::conditions::Layers {
            base: Condition::Weak.into(),
            ..Default::default()
        });
        assert_eq!(
            resolve(
                &owner,
                &mut target,
                hit,
                100,
                [0.; 3],
                &mut neutral_roll,
                false
            )
            .hp_change,
            0
        );
        target.hp = 90_000;
        assert_eq!(
            resolve(
                &owner,
                &mut target,
                hit,
                100,
                [0.; 3],
                &mut neutral_roll,
                false
            )
            .hp_change,
            10_000
        );
        target.hp = 100;
        target.equipment.affinities[0] = Affinity::Normal;
        hit.power = Power::Fixed(1000);
        hit.prevents_defeat = true;
        resolve(
            &owner,
            &mut target,
            hit,
            100,
            [0.; 3],
            &mut neutral_roll,
            false,
        );
        assert_eq!(target.hp, 1);
        hit.power = Power::Fixed(0);
        hit.prevents_defeat = false;
        assert_eq!(
            resolve(
                &owner,
                &mut target,
                hit,
                100,
                [0.; 3],
                &mut neutral_roll,
                false
            )
            .amount,
            1
        );
        assert_eq!(target.hp, 0);
    }

    #[test]
    fn extreme_power_never_reverses_damage_or_overflows() {
        let mut owner = actor(Side::Party);
        owner.casting_state.power_percent = 100;
        owner.equipment.stats.slash = i32::MAX;
        owner.equipment.stats.intelligence = i32::MAX;
        owner.proficiency = u8::MAX;
        owner.attack_power = u16::MAX;
        for kind in [DamageKind::Slash, DamageKind::Magic] {
            let mut target = actor(Side::Enemy);
            target.equipment.max_hp = i32::MAX;
            target.hp = i32::MAX;
            let hit = resolve(
                &owner,
                &mut target,
                rule(kind, Power::Percent(u16::MAX)),
                u16::MAX,
                [0.; 3],
                &mut neutral_roll,
                false,
            );
            assert!(hit.amount > 65_535);
            assert!(hit.hp_change < 0);
            assert_eq!(target.hp, i32::MAX - hit.amount);
            assert!(target.hp >= 0);
        }
    }

    #[test]
    fn recovery_uses_full_vital_ranges_without_signed_narrowing() {
        let mut target = actor(Side::Party);
        target.equipment.max_hp = 200_000;
        target.hp = 10;
        target.equipment.max_tp = u16::MAX;
        target.tp = 40_000;
        assert_eq!(target.recovered_hp(50), (100_010, 100_000));
        assert_eq!(target.recovered_tp(50), (u16::MAX, 32_767));
        target.equipment.recovery.boost = true;
        assert_eq!(target.recovered_hp(50), (120_010, 120_000));
        target.equipment.max_hp = i32::MAX;
        assert_eq!(target.recovered_hp(i32::MAX), (i32::MAX, i32::MAX));
        target.recover_flat_hp(i32::MAX);
        target.recover_flat_tp(i32::MAX);
        assert_eq!((target.hp, target.tp), (i32::MAX, u16::MAX));
        target.recover_flat_hp(-1);
        target.recover_flat_tp(-1);
        assert_eq!((target.hp, target.tp), (i32::MAX, u16::MAX));
    }
}
