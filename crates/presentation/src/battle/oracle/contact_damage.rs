//! Serialize native contact-time inputs for the shared independent Python envelope.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Activity, Affinity, DamageKind, GuardResult, HitElement, HitResult, Power, ProtectionMode,
    Side, conditions::Condition, contact_observation::ContactObservation,
};
use serde_json::{Value, json};

fn forced_reaction_keeps_damage(
    forced: bool,
    kind: DamageKind,
    guarding: bool,
    protection: ProtectionMode,
    hp_change: i32,
    guard: GuardResult,
) -> bool {
    // Magic cannot attempt automatic guard. With no pre-existing guard and an
    // actual HP loss, force only changes the subsequent reaction. Avoided
    // zero-loss contacts and hidden physical guard outcomes need other proof.
    !forced
        || kind == DamageKind::Magic
            && !guarding
            && matches!(protection, ProtectionMode::None | ProtectionMode::Armor)
            && hp_change < 0
            && guard == GuardResult::None
}

pub(super) fn observe(input: &ContactObservation, result: HitResult) -> Result<Value> {
    use Condition::*;
    let owner = &input.owner;
    let target = &input.target;
    let physical = input.rule.kind != DamageKind::Magic;
    let attack = &owner.equipment.damage;
    let defense = &target.equipment.damage;
    ensure!(
        input.ordinary_mode && !input.all_divide && !target.overlimit.is_active(),
        "unsupported contact mode or global modifier"
    );
    ensure!(
        forced_reaction_keeps_damage(
            input.rule.reaction.force,
            input.rule.kind,
            target.guard.active,
            target.reaction.protection.mode,
            result.hp_change,
            result.guard
        ),
        "forced contact has unobserved guard or avoided-damage semantics"
    );
    ensure!(
        !attack.variable_attack
            && !attack.alone
            && !defense.alone
            && !defense.damage_reduction
            && !defense.elemental_damage_reduction
            && !defense.suppress_small_hits
            && !defense.nullify_damage
            && !defense.low_hp_special_guard
            && !defense.special_guard_survival
            && (!physical
                || !attack.physical_damage_boost
                    && !attack.physical_arte_boost
                    && !attack.elemental_physical_boost
                    && !defense.physical_damage_reduction
                    && !owner.control_ex_state.charge.charged()
                    && !(owner.side == Side::Party
                        && owner.equipment.contact.technique_balance >= 100))
            && (physical || !attack.magic_damage_boost && !owner.equipment.recovery.lucky),
        "unsupported live equipment damage modifier"
    );
    let mut adjustments = [0i16; 4];
    let mut magic_modifiers = Vec::new();
    for (attacking, actor) in [(true, owner), (false, target)] {
        for condition in actor.conditions.effective().iter() {
            let admitted = match condition {
                PoisonMild | PoisonSevere | Paralysis | MovementBoost | Heavy
                | ReduceItemEffect => true,
                PhysicalBarrier | MagicalBarrier => attacking,
                AttackUp | AttackDown | AccuracyUp | AccuracyDown => !physical || !attacking,
                DefenseUp | DefenseDown | EvasionDown => !physical || attacking,
                MagicAttackUp | MagicAttackDown => physical || !attacking,
                MagicDefenseUp | MagicDefenseDown => physical || attacking,
                HolySong => !physical,
                _ => false,
            };
            if admitted {
                continue;
            }
            if physical {
                let index = match (attacking, condition) {
                    (true, AttackUp | AttackDown) => 0,
                    (false, DefenseUp | DefenseDown) => 1,
                    (true, AccuracyUp | AccuracyDown) => 2,
                    (false, EvasionDown) => 3,
                    _ => anyhow::bail!("unsupported live physical condition {condition:?}"),
                };
                adjustments[index] = adjustments[index]
                    .checked_add(actor.conditions.magnitude(condition))
                    .context("stat adjustment overflow")?;
            } else {
                let modifier = match (attacking, condition) {
                    (true, MagicAttackUp) => "magic_attack_up",
                    (true, MagicAttackDown) => "magic_attack_down",
                    (false, MagicDefenseUp) => "magic_defense_up",
                    (false, MagicDefenseDown) => "magic_defense_down",
                    (true, PhysicalProtection | MagicalProtection) => "physical_protection",
                    _ => anyhow::bail!("unsupported live magical condition {condition:?}"),
                };
                if !magic_modifiers.iter().any(|value| value == modifier) {
                    magic_modifiers.push(modifier.to_owned());
                }
            }
        }
    }
    let element = match input.rule.element {
        HitElement::Inherited => owner
            .elements
            .action
            .or(owner.elements.enchantment)
            .or(owner.equipment.base_element),
        HitElement::Neutral => None,
        HitElement::Element(element) => Some(element),
    };
    let affinity =
        match target.equipment.affinities[element.map_or(0, |element| element as usize + 1)] {
            Affinity::Normal => "normal",
            Affinity::Weak => "weak",
            Affinity::Resistant => "resistant",
            _ => anyhow::bail!("unsupported live affinity"),
        };
    let protection = target.reaction.protection.mode;
    let window = target.reaction.stagger.window;
    let (recovery_avoided, protection_shift) = match protection {
        ProtectionMode::None | ProtectionMode::Armor => (false, 0),
        ProtectionMode::Down if window != 0 && input.rule.reaction.hits_down => (false, 0),
        ProtectionMode::Down if window != 0 => (false, 3),
        ProtectionMode::Down if target.side == Side::Enemy => (false, 2),
        ProtectionMode::Recovery | ProtectionMode::Escape if target.side == Side::Enemy => {
            (false, 3)
        }
        ProtectionMode::Down | ProtectionMode::Recovery => (true, 0),
        _ => anyhow::bail!("unsupported avoided contact protection"),
    };
    let guard_percent = match result.guard {
        GuardResult::None | GuardResult::Broken => 100,
        GuardResult::Blocked { special: true, .. } => {
            ensure!(
                physical && !defense.special_guard_reduction,
                "unsupported special guard modifier"
            );
            20
        }
        GuardResult::Blocked { special: false, .. } => {
            ensure!(
                physical && !attack.guard_damage_boost,
                "unsupported normal guard modifier"
            );
            100u8
                .checked_sub(target.guard.reduction)
                .context("invalid guard reduction")?
        }
    };
    ensure!(
        guard_percent == 100 || protection_shift == 0,
        "combined protection and guard is unsupported"
    );
    let power_percent = match input.rule.power {
        Power::Normal | Power::AdjustAfterContact(_) => 100,
        Power::Percent(value) => value,
        _ => anyhow::bail!("unsupported contact power rule"),
    };
    let stats = &owner.equipment.stats;
    let defending = &target.equipment.stats;
    let stats_json = |stats: &resonance_battle::CombatStats| {
        json!({
        "conditions":[], "stats":{"slash":stats.slash,"thrust":stats.thrust,
        "defense":stats.defense,"intelligence":stats.intelligence,"accuracy":stats.accuracy,
        "evasion":stats.evasion,"level":stats.level}})
    };
    magic_modifiers.sort_by_key(|name| match name.as_str() {
        "magic_attack_up" => 0,
        "magic_attack_down" => 1,
        "magic_defense_down" => 2,
        "magic_defense_up" => 3,
        _ => 4,
    });
    Ok(json!({
        "rule":{"kind":match input.rule.kind { DamageKind::Slash => "slash", DamageKind::Thrust => "thrust", DamageKind::Magic => "magic" },
            "power":{"percent":power_percent}},
        "attacker":stats_json(stats), "defender":stats_json(defending),
        "context":{
            "strength_percent":if physical { input.attack_power } else { u16::from(owner.casting_state.power_percent) },
            "proficiency":[owner.proficiency,owner.proficiency,owner.proficiency], "familiarity_percent":100,
            "critical":null, "target_accuracy_override":input.activity == Activity::Stunned,
            "affinity":affinity, "modifiers":[], "modifiers_verified":true,
            "weapon_species_match":physical && attack.weapon_species == Some(target.species),
            "guard":match result.guard { GuardResult::Blocked { special:true,.. } => Some("special"), GuardResult::Blocked { special:false,.. } => Some("normal"), _ => None },
            "guard_reduction":if matches!(result.guard,GuardResult::Blocked { special:false,.. }) { Some(100-guard_percent) } else { None },
            "protection_shift":protection_shift, "stat_adjustments":adjustments,
            "magic_modifiers":magic_modifiers},
        "avoided_protection":if recovery_avoided { Some(match protection { ProtectionMode::Down => "down_window_closed", _ => "recovery" }) } else { None },
        "hp_before":target.hp, "prevents_defeat":input.rule.prevents_defeat
    }))
}

#[test]
fn forced_magic_retains_damage_but_does_not_admit_avoided_or_guarded_contacts() {
    assert!(forced_reaction_keeps_damage(
        true,
        DamageKind::Magic,
        false,
        ProtectionMode::None,
        -994,
        GuardResult::None
    ));
    for (kind, guarding, protection, hp_change, guard) in [
        (
            DamageKind::Magic,
            false,
            ProtectionMode::None,
            0,
            GuardResult::None,
        ),
        (
            DamageKind::Magic,
            true,
            ProtectionMode::None,
            -994,
            GuardResult::None,
        ),
        (
            DamageKind::Slash,
            false,
            ProtectionMode::None,
            -994,
            GuardResult::None,
        ),
        (
            DamageKind::Magic,
            false,
            ProtectionMode::None,
            -994,
            GuardResult::Broken,
        ),
        (
            DamageKind::Magic,
            false,
            ProtectionMode::Down,
            -994,
            GuardResult::None,
        ),
        (
            DamageKind::Magic,
            false,
            ProtectionMode::Recovery,
            0,
            GuardResult::None,
        ),
    ] {
        assert!(!forced_reaction_keeps_damage(
            true, kind, guarding, protection, hp_change, guard
        ));
    }
}
