//! Bind native persistent ailments and queued buffs to live battle condition layers.
use anyhow::{Result, ensure};
use resonance_battle::conditions::{
    Condition::*, ConditionSet, Conditions, GearRegeneration, Layers, PHYSICAL_AILMENTS, POISON,
};
use resonance_content::battle_profile::Profile;
use resonance_content::menu_data::{EquipmentAilment, EquipmentProperties, MenuData, TpDiscount};
use resonance_events::party::{Ailments, Member, Poison, StatBuff};
use std::collections::BTreeSet;

/// One catalogue projection, shared by combat conditions, movement, damage and rewards.
#[derive(Default)]
pub(super) struct GearEffects {
    layers: Layers,
    regeneration: GearRegeneration,
    unsupported_modifier: bool,
    pub movement_bonus: i32,
    pub physical_damage_boost: bool,
    pub physical_damage_reduction: bool,
    pub magic_damage_boost: bool,
    pub experience_percent: u8,
    pub gald_one_and_a_half: bool,
    pub gald_double: bool,
}

impl GearEffects {
    #[cfg(test)]
    pub fn from_properties(properties: &EquipmentProperties) -> Self {
        let mut gear = Self::default();
        gear.contribute(properties);
        gear
    }

    pub fn equipped(menus: &MenuData, member: &Member) -> Self {
        let mut gear = Self::default();
        for item in member.equipment.into_iter().filter(|&item| item != 0) {
            gear.contribute(&menus.items[usize::from(item)].properties);
        }
        gear
    }

    fn contribute(&mut self, properties: &EquipmentProperties) {
        let ailment = |value: EquipmentAilment, immunity| match value {
            EquipmentAilment::Poison => POISON,
            EquipmentAilment::Stun => Stun.into(),
            EquipmentAilment::Paralysis => Paralysis.into(),
            EquipmentAilment::Weak => Weak.into(),
            EquipmentAilment::Petrify => Petrified.into(),
            EquipmentAilment::Curse => Curse.into(),
            EquipmentAilment::Heavy => Heavy.into(),
            EquipmentAilment::PhysicalAilments if immunity => PHYSICAL_AILMENTS,
            EquipmentAilment::PhysicalAilments => PhysicalAffliction.into(),
        };
        for &value in &properties.immunities {
            self.layers.immunity = self.layers.immunity.union(ailment(value, true));
        }
        for &value in &properties.ailments {
            self.layers.equipment_overlay =
                self.layers.equipment_overlay.union(ailment(value, false));
        }
        if properties.short_stun {
            self.layers.immunity = self.layers.immunity.union(ShortStun.into());
        }
        if properties.defense_halved {
            self.layers.equipment_overlay =
                self.layers.equipment_overlay.union(DefenseHalved.into());
        }
        for (enabled, condition) in [
            (properties.ailment_resistance, AilmentResistance),
            (properties.faster_casting, CastingSpeed),
            (properties.kill_hp_recovery, KillHpRecovery),
            (properties.kill_tp_recovery, KillTpRecovery),
            (properties.hp_regeneration != 0, RegenerateHp),
            (properties.tp_regeneration != 0, RegenerateTp),
        ] {
            if enabled {
                self.layers.intrinsic = self.layers.intrinsic.union(condition.into());
            }
        }
        self.layers.intrinsic = self.layers.intrinsic.union(match properties.tp_discount {
            TpDiscount::None => ConditionSet::EMPTY,
            TpDiscount::Third => TpThird.into(),
            TpDiscount::Half => TpHalf.into(),
        });
        self.regeneration.hp_percent = self
            .regeneration
            .hp_percent
            .saturating_add(properties.hp_regeneration);
        self.regeneration.tp_percent = self
            .regeneration
            .tp_percent
            .saturating_add(properties.tp_regeneration);
        self.movement_bonus += i32::from(properties.movement_bonus);
        self.experience_percent = self.experience_percent.max(properties.experience_percent);
        self.physical_damage_boost |= properties.physical_damage_boost;
        self.physical_damage_reduction |= properties.physical_damage_reduction;
        self.magic_damage_boost |= properties.magic_damage_boost;
        self.gald_one_and_a_half |= properties.gald_one_and_a_half;
        self.gald_double |= properties.gald_double;
        self.unsupported_modifier |= properties.unsupported_modifier;
    }

    pub fn refresh(&self, current: &Conditions) -> Result<Conditions> {
        ensure!(
            !self.unsupported_modifier,
            "battle equipment modifier is not prepared"
        );
        let mut conditions = current.clone();
        conditions.reload_gear_regeneration(self.regeneration);
        conditions.reload_layers(Layers {
            base: current.base(),
            equipment_overlay: self.layers.equipment_overlay.without(self.layers.immunity),
            ..self.layers
        });
        Ok(conditions)
    }
}

/// Apply persistent ailments and queued buffs when entering battle or completing a meal.
pub(super) fn prepare_reload(
    current: &Conditions,
    ailments: Ailments,
    buffs: &BTreeSet<StatBuff>,
    gear: &GearEffects,
    strength_boost: bool,
) -> Result<Conditions> {
    const BUFF_STRENGTH: i16 = 10;
    const BUFF_TICKS: u32 = 900;
    let mut conditions = current.clone();
    let base = [
        (ailments.poison.has_mild(), PoisonMild),
        (ailments.poison.has_severe(), PoisonSevere),
        (ailments.paralysis, Paralysis),
        (ailments.petrified, Petrified),
        (ailments.curse, Curse),
    ]
    .into_iter()
    .filter_map(|(enabled, condition)| enabled.then_some(condition))
    .collect();
    conditions.reload_layers(Layers {
        base,
        intrinsic: current.layers().intrinsic,
        equipment_overlay: current.layers().equipment_overlay,
        ..Default::default()
    });
    for buff in buffs {
        let condition = match buff {
            StatBuff::AttackUp => AttackUp,
            StatBuff::DefenseUp => DefenseUp,
            StatBuff::MagicAttackUp => MagicAttackUp,
            StatBuff::MagicDefenseUp => MagicDefenseUp,
            StatBuff::AccuracyUp => AccuracyUp,
            StatBuff::AttackDown => AttackDown,
            StatBuff::DefenseDown => DefenseDown,
            StatBuff::AccuracyDown => AccuracyDown,
            StatBuff::MagicAttackDown => MagicAttackDown,
        };
        let _ =
            conditions.apply_stat_condition(condition, BUFF_STRENGTH, BUFF_TICKS, strength_boost);
    }
    gear.refresh(&conditions)
}

pub(super) fn apply_profile(profile: &Profile, conditions: &mut Conditions) -> Result<()> {
    // Petrification is an actor ailment, not a static profile trait.
    ensure!(
        !profile
            .initial_conditions
            .union(profile.intrinsic_conditions)
            .contains(Petrified),
        "battle profile petrification is not prepared"
    );
    conditions.initialize_profile(
        profile.initial_conditions,
        profile.intrinsic_conditions,
        profile.immunities,
    );
    Ok(())
}

pub(super) fn export_ailments(base: ConditionSet) -> Ailments {
    Ailments {
        poison: match (base.contains(PoisonMild), base.contains(PoisonSevere)) {
            (true, true) => Poison::Both,
            (true, false) => Poison::Mild,
            (false, true) => Poison::Severe,
            (false, false) => Poison::None,
        },
        paralysis: base.contains(Paralysis),
        petrified: base.contains(Petrified),
        curse: base.contains(Curse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::conditions::Traits;

    fn reload(ailments: Ailments, properties: EquipmentProperties) -> Conditions {
        prepare_reload(
            &Conditions::default(),
            ailments,
            &BTreeSet::new(),
            &GearEffects::from_properties(&properties),
            false,
        )
        .unwrap()
    }

    #[test]
    fn persistent_ailments_roundtrip_without_transient_or_equipment_effects() {
        for poison in [Poison::None, Poison::Mild, Poison::Severe, Poison::Both] {
            let ailments = Ailments {
                poison,
                paralysis: true,
                petrified: true,
                curse: true,
            };
            let live = reload(
                ailments,
                EquipmentProperties {
                    ailments: [EquipmentAilment::Heavy].into(),
                    tp_discount: TpDiscount::Half,
                    ..Default::default()
                },
            );
            assert_eq!(export_ailments(live.base()), ailments);
            assert_eq!(live.layers().equipment_overlay, Heavy.into());
            assert_eq!(live.layers().intrinsic, TpHalf.into());
            assert_eq!(live.remaining(Paralysis), Some(600));
            assert_eq!(live.remaining(Curse), None);
            let poison_conditions: ConditionSet = [
                (poison.has_mild(), PoisonMild),
                (poison.has_severe(), PoisonSevere),
            ]
            .into_iter()
            .filter_map(|(enabled, condition)| enabled.then_some(condition))
            .collect();
            assert_eq!(
                live.periodic_effects()
                    .iter()
                    .map(|effect| effect.condition)
                    .collect::<ConditionSet>(),
                poison_conditions
            );
            assert!(
                live.periodic_effects()
                    .iter()
                    .all(|effect| effect.remaining == 30 && effect.period == 30)
            );
        }
        assert_eq!(
            export_ailments(ConditionSet::of(&[Weak, AttackUp, RegenerateHp, TpHalf])),
            Ailments::default()
        );
    }

    #[test]
    fn queued_buffs_apply_named_signed_effects_without_becoming_persistent_ailments() {
        let requests = [
            (StatBuff::AttackUp, AttackUp, 10),
            (StatBuff::DefenseUp, DefenseUp, 10),
            (StatBuff::MagicAttackUp, MagicAttackUp, 10),
            (StatBuff::MagicDefenseUp, MagicDefenseUp, 10),
            (StatBuff::AccuracyUp, AccuracyUp, 10),
            (StatBuff::AttackDown, AttackDown, -10),
            (StatBuff::DefenseDown, DefenseDown, -10),
            (StatBuff::AccuracyDown, AccuracyDown, -10),
            (StatBuff::MagicAttackDown, MagicAttackDown, -10),
        ];
        for (request, condition, magnitude) in requests {
            let queued = [request].into();
            let live = prepare_reload(
                &Conditions::default(),
                Ailments::default(),
                &queued,
                &GearEffects::default(),
                false,
            )
            .unwrap();
            assert_eq!(live.base(), condition.into());
            assert_eq!(live.remaining(condition), Some(900));
            assert_eq!(live.magnitude(condition), magnitude);
            assert_eq!(export_ailments(live.base()), Ailments::default());
            assert_eq!(queued, [request].into());
        }
        let queued = requests
            .into_iter()
            .map(|(request, _, _)| request)
            .collect();
        let live = prepare_reload(
            &Conditions::default(),
            Ailments::default(),
            &queued,
            &GearEffects::default(),
            false,
        )
        .unwrap();
        assert_eq!(live.base(), MagicDefenseUp.into());
    }

    #[test]
    fn recipient_traits_apply_to_entry_buffs_and_ailments_without_gear_curing_them() {
        let current = Conditions::default().with_traits(Traits {
            magical_ailment_guard: true,
            extended_duration: true,
            ..Default::default()
        });
        let ailments = Ailments {
            paralysis: true,
            ..Default::default()
        };
        let live = prepare_reload(
            &current,
            ailments,
            &[StatBuff::AttackUp, StatBuff::DefenseDown].into(),
            &GearEffects::from_properties(&EquipmentProperties {
                immunities: [EquipmentAilment::Paralysis].into(),
                ..Default::default()
            }),
            true,
        )
        .unwrap();
        assert_eq!(live.base(), ConditionSet::of(&[Paralysis, AttackUp]));
        assert_eq!(live.immunity(), Paralysis.into());
        assert_eq!(
            (live.remaining(Paralysis), live.remaining(AttackUp)),
            (Some(750), Some(1125))
        );
        assert_eq!(live.magnitude(AttackUp), 12);
        assert_eq!(live.traits(), current.traits());
        let existing = live.active_effects().to_vec();
        let refreshed = GearEffects::default()
            .refresh(&live.with_traits(Default::default()))
            .unwrap();
        assert_eq!(refreshed.active_effects(), existing);
    }

    #[test]
    fn equipment_immunity_preserves_persistent_ailments_and_masks_only_forced_effects() {
        let ailments = Ailments {
            poison: Poison::Both,
            paralysis: true,
            petrified: true,
            curse: true,
        };
        for (ailment, immunity) in [
            (EquipmentAilment::Poison, POISON),
            (EquipmentAilment::Stun, Stun.into()),
            (EquipmentAilment::Paralysis, Paralysis.into()),
            (EquipmentAilment::Weak, Weak.into()),
            (EquipmentAilment::Petrify, Petrified.into()),
            (EquipmentAilment::Curse, Curse.into()),
            (EquipmentAilment::Heavy, Heavy.into()),
            (EquipmentAilment::PhysicalAilments, PHYSICAL_AILMENTS),
        ] {
            let live = reload(
                ailments,
                EquipmentProperties {
                    immunities: [ailment].into(),
                    ..Default::default()
                },
            );
            assert_eq!(export_ailments(live.base()), ailments);
            assert_eq!(live.immunity(), immunity);
        }
        let live = reload(
            ailments,
            EquipmentProperties {
                ailments: [
                    EquipmentAilment::Curse,
                    EquipmentAilment::Poison,
                    EquipmentAilment::PhysicalAilments,
                    EquipmentAilment::Heavy,
                ]
                .into(),
                immunities: [EquipmentAilment::PhysicalAilments].into(),
                short_stun: true,
                defense_halved: true,
                ailment_resistance: true,
                faster_casting: true,
                tp_discount: TpDiscount::Half,
                ..Default::default()
            },
        );
        assert_eq!(
            live.layers().equipment_overlay,
            ConditionSet::of(&[DefenseHalved, Heavy])
        );
        assert_eq!(
            live.layers().intrinsic,
            ConditionSet::of(&[AilmentResistance, CastingSpeed, TpHalf])
        );
        assert_eq!(live.immunity(), PHYSICAL_AILMENTS.union(ShortStun.into()));
        let removed = GearEffects::default().refresh(&live).unwrap();
        assert_eq!(
            removed.layers(),
            Layers {
                base: live.base(),
                ..Default::default()
            }
        );
        assert_eq!(removed.active_effects(), live.active_effects());
        assert_eq!(removed.periodic_effects(), live.periodic_effects());
    }

    #[test]
    fn equipment_refresh_retains_live_clocks_and_entry_replaces_transient_buffs() {
        let mut current = Conditions::default();
        current.initialize_profile(ConditionSet::EMPTY, CastingSpeed.into(), Paralysis.into());
        assert!(current.apply_stat_condition(DefenseDown, 10, 17, false));
        let gear = GearEffects::from_properties(&EquipmentProperties {
            ailments: [
                EquipmentAilment::Curse,
                EquipmentAilment::Poison,
                EquipmentAilment::PhysicalAilments,
            ]
            .into(),
            defense_halved: true,
            ..Default::default()
        });
        let warm = gear.refresh(&current).unwrap();
        assert_eq!(warm.base(), DefenseDown.into());
        assert_eq!(warm.active_effects(), current.active_effects());
        assert_eq!(
            warm.layers().equipment_overlay,
            ConditionSet::of(&[
                DefenseHalved,
                Curse,
                PoisonMild,
                PoisonSevere,
                PhysicalAffliction
            ])
        );
        assert_eq!(export_ailments(warm.base()), Ailments::default());
        assert!(!warm.arte_queue_allowed());
        let cold = prepare_reload(
            &warm,
            Ailments::default(),
            &[StatBuff::MagicAttackUp].into(),
            &GearEffects::default(),
            false,
        )
        .unwrap();
        assert_eq!(cold.base(), MagicAttackUp.into());
        assert_eq!(cold.magnitude(DefenseDown), 0);
        assert_eq!(cold.magnitude(MagicAttackUp), 10);
        assert!(cold.effective().contains(CastingSpeed));
        assert_eq!(cold.immunity(), Paralysis.into());
        assert!(cold.arte_queue_allowed());
    }

    #[test]
    fn equipment_regeneration_and_kill_recovery_rebuild_without_stacking() {
        let mut gear = GearEffects::from_properties(&EquipmentProperties {
            hp_regeneration: 2,
            tp_regeneration: 1,
            kill_hp_recovery: true,
            kill_tp_recovery: true,
            ..Default::default()
        });
        gear.contribute(&EquipmentProperties {
            hp_regeneration: 3,
            tp_regeneration: 3,
            ..Default::default()
        });
        let current = reload(
            Ailments {
                paralysis: true,
                ..Default::default()
            },
            EquipmentProperties::default(),
        );
        let live = gear.refresh(&current).unwrap();
        assert_eq!(
            live.gear_regeneration(),
            GearRegeneration {
                hp_percent: 5,
                tp_percent: 4
            }
        );
        assert_eq!(
            live.layers().intrinsic,
            ConditionSet::of(&[RegenerateHp, RegenerateTp, KillHpRecovery, KillTpRecovery])
        );
        assert_eq!(live.active_effects(), current.active_effects());
        assert_eq!(live.periodic_effects().len(), 2);
        assert!(
            live.periodic_effects()
                .iter()
                .all(|effect| effect.remaining == 360 && effect.period == 360)
        );
        assert_eq!(gear.refresh(&live).unwrap(), live);
        let removed = GearEffects::from_properties(&EquipmentProperties {
            kill_tp_recovery: true,
            ..Default::default()
        })
        .refresh(&live)
        .unwrap();
        assert_eq!(removed.gear_regeneration(), GearRegeneration::default());
        assert_eq!(removed.layers().intrinsic, KillTpRecovery.into());
        assert_eq!(removed.active_effects(), live.active_effects());
        assert!(removed.periodic_effects().is_empty());
    }
}
