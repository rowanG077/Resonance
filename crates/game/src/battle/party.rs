//! Prepare the first four formation members as independent battle actors.
mod conditional_stats;
#[cfg(test)]
pub(super) mod projection_tests;
use super::model::ModelSetup;
use anyhow::{Context, Result, ensure};
use resonance_battle::{Actor, Affinity, CombatStats, Control, Side};
use resonance_content::menu_data::{MenuData, ex_effect as ex};
use resonance_events::party::{Member, Party};
#[cfg(test)]
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Character {
    Lloyd = 1,
    Colette,
    Genis,
    Raine,
    Sheena,
    Zelos,
    Presea,
    Regal,
    Kratos,
}

impl TryFrom<u8> for Character {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use Character::*;
        match value {
            1 => Ok(Lloyd),
            2 => Ok(Colette),
            3 => Ok(Genis),
            4 => Ok(Raine),
            5 => Ok(Sheena),
            6 => Ok(Zelos),
            7 => Ok(Presea),
            8 => Ok(Regal),
            9 => Ok(Kratos),
            _ => anyhow::bail!("unknown party character {value}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Setup {
    pub position: [f32; 3],
    pub heading: f32,
    pub model: Option<ModelSetup>,
}

/// Construct one roster member's combat state independently of artwork.
#[expect(
    clippy::too_many_arguments,
    reason = "Preparation borrows independently owned content and runtime resources."
)]
pub(super) fn prepare_actor(
    recoil: &super::recoil::Parameters,
    menus: &MenuData,
    party: &Party,
    slot: usize,
    loadout: &Loadout,
    position: [f32; 3],
    heading: f32,
    devils_arms_unlocked: bool,
    profile: &resonance_content::battle_profile::Profile,
) -> Result<Actor> {
    let character = *party
        .formation
        .get(slot)
        .context("missing battle character")?;
    ensure!(
        !party.formation[..slot].contains(&character),
        "duplicate battle character {character}"
    );
    let index = usize::from(
        character
            .checked_sub(1)
            .context("invalid party character")?,
    );
    let member = party.members.get(index).context("missing party member")?;
    let control = match party.settings.battle_controls[slot] {
        0 => Control::Manual,
        1 => Control::SemiAuto,
        2 => Control::Auto,
        _ => anyhow::bail!("invalid battle control for slot {slot}"),
    };
    let mut actor = actor(loadout, member, control, position, heading)?;
    actor.body.collider = Some(match Character::try_from(character)? {
        Character::Genis | Character::Presea => resonance_battle::Collider::standing(24., 110.),
        _ => resonance_battle::Collider::standing(30., 150.),
    });
    // Project the battle-entry technique balance without changing the suspended party.
    actor.equipment.contact.technique_balance = member.activated_technique_balance(menus)?;
    actor.equipment.stats = loadout.battle_stats(
        &party.formation,
        devils_arms_unlocked,
        party.battles.kills[index],
    );
    super::profile::apply(recoil, profile, &mut actor)?;
    actor.control_slot = slot as u8;
    // Guard recovery depends on position strategy and character identity, not formation slot.
    actor.guard.recovery_bonus = recoil.guard_recovery_bonus(member.strategy[2], character)?;
    actor.guard.reduction = actor
        .guard
        .reduction
        .saturating_add(loadout.guard_reduction_bonus)
        .min(100);
    Ok(actor)
}

fn validate_member(menus: &MenuData, member: &Member, character: usize) -> Result<()> {
    ensure!(
        member.overlimit <= 100,
        "saved Over Limit percentage exceeds 100"
    );
    let title = member
        .title
        .checked_sub(1)
        .and_then(|index| menus.titles.get(character)?.get(usize::from(index)))
        .context("missing party title")?;
    ensure!(
        title.costume.is_none(),
        "battle title costume is not prepared"
    );
    let rules = menus
        .ex_skills
        .characters
        .get(character)
        .context("missing character EX rules")?;
    for &id in member.ex_skills.iter().filter(|&&id| id != 0) {
        ensure!(
            menus.ex_skills.skills.contains_key(&id),
            "missing EX skill {id}"
        );
        ensure!(
            id != ex::SPELL_CHARGE || character + 1 == Character::Genis as usize,
            "Spell Charge resources are not prepared for character {}",
            character + 1
        );
    }
    for &index in &member.compound_ex_skills {
        ensure!(
            usize::from(index) < rules.compounds.len(),
            "invalid compound EX skill"
        );
    }
    for &id in &member.equipment {
        let item = menus
            .items
            .get(usize::from(id))
            .with_context(|| format!("missing equipped item {id}"))?;
        ensure!(
            !item.properties.unsupported_modifier,
            "equipment item {id} has an unavailable modifier"
        );
    }
    ensure!(member.level != 0, "invalid party level");
    Ok(())
}

/// Runtime behavior derived from the equipped skills and gear. Discovery history is irrelevant.
pub struct Loadout {
    pub attributes: resonance_battle::EquipmentAttributes,
    pub(super) gear: super::conditions::GearEffects,
    conditions: resonance_battle::conditions::Traits,
    guard_reduction_bonus: u8,
    pub spell_charge: bool,
    pub quick_item: bool,
    pub extended_overlimit: bool,
    pub(super) happiness: bool,
    pub(super) spirit_healer: bool,
    pub(super) maximum_vital_growth: [bool; 2],
    pub(super) increase_experience: bool,
    pub(super) tough_experience: bool,
    pub(super) experience_plus: bool,
    pub(super) item_finder: bool,
    pub(super) gald_finder: bool,
    battle_cry: bool,
    chivalry: bool,
    guilt: bool,
    kills_damage: bool,
}

fn active_compounds(
    menus: &MenuData,
    member: &Member,
    character: usize,
) -> std::collections::BTreeSet<u8> {
    menus.ex_skills.characters[character]
        .equipped_compounds(&member.ex_skills)
        .map(|(_, recipe)| recipe.skill)
        .collect()
}

/// Reward eligibility also queries inactive members, whose battle resources are not prepared.
pub(super) fn happiness(member: &Member) -> bool {
    member.ex_skills.contains(&ex::HAPPINESS)
}

pub fn loadout(menus: &MenuData, member: &Member, character: usize) -> Result<Loadout> {
    validate_member(menus, member, character)?;
    let active = active_compounds(menus, member, character);
    ensure!(
        active
            .iter()
            .all(|id| menus.ex_skills.skills.contains_key(id)),
        "missing compound EX skill"
    );
    let compound = |id| active.contains(&id);
    let traits = member.equipment_traits(menus);
    let gear = super::conditions::GearEffects::equipped(menus, member);
    let weapon_species = menus.items[usize::from(member.equipment[0])]
        .properties
        .species_bonus;
    let stats = member.stats_for(menus, character);
    let mut affinities = [Affinity::Normal; 9];
    affinities[0] = affinity(traits.neutral_resistance);
    for (target, value) in affinities[1..].iter_mut().zip(traits.resistance) {
        *target = affinity(value);
    }
    Ok(Loadout {
        conditions: resonance_battle::conditions::Traits {
            physical_ailment_guard: compound(ex::PHYSICAL_AILMENT_GUARD)
                || compound(ex::PHYSICAL_AILMENT_GUARD_COMPOUND),
            magical_ailment_guard: compound(ex::MAGICAL_AILMENT_GUARD),
            extended_duration: compound(ex::EXTENDED_CONDITIONS),
        },
        attributes: resonance_battle::EquipmentAttributes {
            max_hp: i32::from(stats.hp),
            max_tp: stats.tp,
            luck: stats.luck,
            stats: CombatStats {
                slash: i32::from(stats.slash),
                thrust: i32::from(stats.thrust),
                defense: i32::from(stats.defense),
                intelligence: i32::from(stats.intelligence),
                accuracy: i32::from(stats.accuracy),
                evasion: i32::from(stats.evasion),
                level: member.level,
            },
            base_element: traits.attack_element,
            affinities,
            dagger_reach: matches!(character, 5 | 8)
                && menus.items[usize::from(member.equipment[0])].category == 14,
            casting: resonance_battle::CastingTraits {
                speed_cast: member.ex_skills.contains(&ex::SPEED_CAST),
                angel_song: member.ex_skills.contains(&ex::ANGEL_SONG),
                rhythm: member.ex_skills.contains(&ex::RHYTHM),
                spell_save: member.ex_skills.contains(&ex::SPELL_SAVE),
                reprise: compound(ex::REPRISE),
                nimble: compound(ex::NIMBLE),
                reducer: compound(ex::REDUCER),
                random: compound(ex::RANDOM_CAST),
                lucky_magic: compound(ex::LUCKY_MAGIC),
                quick: compound(ex::QUICK_CAST),
            },
            reaction_ex: resonance_battle::ContactEx {
                follow_up: member.ex_skills.contains(&ex::FOLLOW_UP),
                combo_hp: compound(ex::COMBO_HP),
                combo_tp: compound(ex::COMBO_TP),
                down_tp: compound(ex::DOWN_TP),
                damage_tp: compound(ex::DAMAGE_TP),
                reflect_damage: compound(ex::REFLECT_DAMAGE),
                hammer_revenge: compound(ex::HAMMER_REVENGE),
                aid_revenge: compound(ex::AID_REVENGE),
            },
            damage: resonance_battle::DamageTraits {
                weapon_species,
                alone: member.ex_skills.contains(&ex::ALONE),
                rear_guard: member.ex_skills.contains(&ex::REAR_GUARD),
                single_charge_guard: member.ex_skills.contains(&ex::SINGLE_CHARGE_GUARD),
                guard_damage_boost: compound(ex::GUARD_DAMAGE_BOOST),
                special_guard_reduction: compound(ex::SPECIAL_GUARD_REDUCTION),
                low_hp_special_guard: compound(ex::LOW_HP_SPECIAL_GUARD),
                special_guard_survival: compound(ex::SPECIAL_GUARD_SURVIVAL),
                physical_stability: compound(ex::PHYSICAL_STABILITY),
                elemental_stability: compound(ex::ELEMENTAL_STABILITY),
                casting_stability: compound(ex::CASTING_STABILITY)
                    || compound(ex::CASTING_STABILITY_COMPOUND),
                stored_spell_stability: compound(ex::STORED_SPELL_STABILITY),
                elemental_damage_reduction: compound(ex::ELEMENTAL_DAMAGE_REDUCTION),
                run_magic_stability: compound(ex::RUN_MAGIC_STABILITY),
                charged_neutral_stability: compound(ex::CHARGED_NEUTRAL_STABILITY),
                charged_run_stability: compound(ex::CHARGED_RUN_STABILITY),
                stability: compound(ex::STABILITY),
                // Critical Up adds five after the equipment bonuses.
                critical_chance_bonus: traits
                    .critical_chance_bonus
                    .saturating_add(if compound(ex::CRITICAL_BONUS) { 5 } else { 0 })
                    .min(100),
                physical_arte_boost: compound(ex::PHYSICAL_ARTE_BOOST),
                // Duplicate damage-boosting rings do not stack.
                physical_damage_boost: gear.physical_damage_boost,
                physical_damage_reduction: gear.physical_damage_reduction,
                magic_damage_boost: gear.magic_damage_boost,
                damage_reduction: member.ex_skills.contains(&ex::DAMAGE_REDUCTION),
                ailment_resistance: member.ex_skills.contains(&ex::AILMENT_RESISTANCE),
                physical_counter: compound(ex::PHYSICAL_COUNTER),
                elemental_physical_boost: compound(ex::ELEMENTAL_PHYSICAL_BOOST),
                variable_attack: compound(ex::VARIABLE_ATTACK),
                suppress_small_hits: compound(ex::SUPPRESS_SMALL_HITS),
                nullify_damage: member.ex_skills.contains(&ex::NULLIFY_DAMAGE)
                    || compound(ex::NULLIFY_DAMAGE_COMPOUND),
            },
            combo_traits: resonance_battle::ComboTraits {
                sky_combo: member.ex_skills.contains(&ex::SKY_COMBO),
                aerial_arte: member.ex_skills.contains(&ex::AERIAL_ARTE)
                    || compound(ex::AERIAL_ARTE_COMPOUND),
                ability_plus: member.ex_skills.contains(&ex::ABILITY_PLUS),
                super_chain: member.ex_skills.contains(&ex::SUPER_CHAIN),
                flash: member.ex_skills.contains(&ex::FLASH),
                counter_combo: compound(ex::COUNTER_COMBO),
                jump_combo: compound(ex::JUMP_COMBO),
                landing: compound(ex::LANDING),
                super_blast: compound(ex::SUPER_BLAST),
                combo_force: compound(ex::COMBO_FORCE),
            },
            recovery: resonance_battle::RecoveryTraits {
                boost: member.ex_skills.contains(&ex::BOOST),
                lucky: compound(ex::LUCKY_RECOVERY),
                lethal: resonance_battle::LethalRescueTraits {
                    resurrect: member.ex_skills.contains(&ex::RESURRECT),
                    angel_tear: compound(ex::ANGEL_TEAR),
                    equipment: [5, 3, 4].map(|slot| {
                        use resonance_battle::RescueEquipment;
                        use resonance_content::menu_data::EquipmentRescue;
                        if member.equipment[slot] == 0 {
                            return None;
                        }
                        menus.items[usize::from(member.equipment[slot])]
                            .properties
                            .rescue
                            .map(|rescue| match rescue {
                                EquipmentRescue::Chance => RescueEquipment::Chance,
                                EquipmentRescue::Consumable => {
                                    RescueEquipment::Consumable(slot as u8)
                                }
                            })
                    }),
                },
                common: resonance_battle::CommonRecoveryTraits {
                    low_hp: member.ex_skills.contains(&ex::LOW_HP_RECOVERY),
                    last_hit: compound(ex::LAST_HIT_RECOVERY),
                    self_cure: compound(ex::SELF_CURE),
                    idle_hp_tp: compound(ex::IDLE_HP_TP),
                    idle_hp: compound(ex::IDLE_HP),
                    idle_tp: compound(ex::IDLE_TP),
                },
            },
            contact: resonance_battle::ContactTraits {
                technique_balance: member.technique_balance,
                endure: member.ex_skills.contains(&ex::ENDURE),
                hard_hit: compound(ex::HARD_HIT),
                air_brake: compound(ex::AIR_BRAKE),
            },
            control_ex: resonance_battle::ControlExTraits {
                counter: compound(ex::COUNTER),
                rebound: compound(ex::REBOUND),
                roll: compound(ex::ROLL),
                aerial_guard: compound(ex::AERIAL_GUARD),
                timed_guard: compound(ex::TIMED_GUARD),
                double_jump: compound(ex::DOUBLE_JUMP),
                charge: member.ex_skills.contains(&ex::CHARGE),
                lucky_charge: compound(ex::LUCKY_CHARGE),
                taunt_hp: compound(ex::TAUNT_HP),
                taunt_vitals: compound(ex::TAUNT_VITALS),
            },
            spell_revenge: compound(ex::SPELL_REVENGE),
            quick_escape: compound(ex::QUICK_ESCAPE),
            taunt_guard: compound(ex::TAUNT_GUARD),
            taunt_cancel: compound(ex::TAUNT_CANCEL),
            quick_turn: compound(ex::QUICK_TURN),
            backstep_guard: compound(ex::BACKSTEP_GUARD),
            normal_guard: compound(ex::NORMAL_GUARD),
            tp_cost_reduction: compound(ex::TP_COST_REDUCTION),
            taunt_enabled: member.ex_skills.contains(&ex::TAUNT),
            speed_multiplier: 1.0
                + (gear.movement_bonus + i32::from(member.ex_skills.contains(&ex::DASH))) as f32
                    * 0.1,
            normal_combo_limit: normal_combo_limit(member.ex_skills, character),
            stun_ex_bonus: member.ex_skills.contains(&ex::STUN_BONUS),
        },
        guard_reduction_bonus: if member.ex_skills.contains(&ex::GUARD_REDUCTION) {
            5
        } else {
            0
        },

        spell_charge: member.ex_skills.contains(&ex::SPELL_CHARGE),
        quick_item: member.ex_skills.contains(&ex::QUICK_ITEM),
        extended_overlimit: compound(ex::EXTENDED_OVERLIMIT),
        happiness: happiness(member),
        spirit_healer: compound(ex::SPIRIT_HEALER),
        maximum_vital_growth: [compound(ex::HP_GROWTH), compound(ex::TP_GROWTH)],
        increase_experience: compound(ex::INCREASE_EXPERIENCE),
        tough_experience: compound(ex::TOUGH_EXPERIENCE),
        experience_plus: compound(ex::EXPERIENCE_PLUS),
        item_finder: compound(ex::ITEM_FINDER),
        gald_finder: compound(ex::GALD_FINDER),
        battle_cry: compound(ex::BATTLE_CRY),
        chivalry: compound(ex::CHIVALRY),
        guilt: member.ex_skills.contains(&ex::GUILT),
        kills_damage: menus.items[usize::from(member.equipment[0])]
            .properties
            .kills_damage,
        gear,
    })
}

impl Loadout {
    pub(super) fn battle_stats(
        &self,
        formation: &[u8],
        devils_arms_unlocked: bool,
        kills: u16,
    ) -> CombatStats {
        conditional_stats::project(
            self.attributes.stats,
            self.kills_damage,
            kills,
            devils_arms_unlocked,
            self.battle_cry,
            self.chivalry,
            self.guilt,
            formation,
        )
    }

    /// Refresh gear layers without replacing live condition clocks or profile layers.
    pub(super) fn equipment_attributes(
        &self,
        member: &Member,
        live_conditions: &resonance_battle::conditions::Conditions,
        entering: bool,
    ) -> Result<(
        resonance_battle::EquipmentAttributes,
        resonance_battle::conditions::Conditions,
    )> {
        let current = live_conditions.clone().with_traits(self.conditions);
        let conditions = if entering {
            super::conditions::prepare_reload(
                &current,
                member.ailments,
                &member.queued_buffs,
                &self.gear,
                self.attributes.recovery.boost,
            )?
        } else {
            self.gear.refresh(&current)?
        };
        Ok((self.attributes.clone(), conditions))
    }
}

pub(super) fn actor(
    loadout: &Loadout,
    member: &Member,
    control: Control,
    position: [f32; 3],
    heading: f32,
) -> Result<Actor> {
    let (attributes, conditions) =
        loadout.equipment_attributes(member, &Default::default(), true)?;
    ensure!(
        attributes.max_hp > 0
            && i32::from(member.hp) <= attributes.max_hp
            && member.tp <= attributes.max_tp,
        "invalid party battle vitals"
    );
    ensure!(
        position.iter().all(|v| v.is_finite()) && heading.is_finite(),
        "invalid battle placement"
    );
    Ok(Actor {
        side: Side::Party,
        species: 0,
        equipment: attributes,
        control,
        availability: if member.hp == 0 {
            resonance_battle::ActorAvailability::Dead
        } else if member.ailments.petrified {
            resonance_battle::ActorAvailability::Petrified
        } else {
            resonance_battle::ActorAvailability::Active
        },
        overlimit: resonance_battle::OverLimit::new(u16::from(member.overlimit) * 10)?,
        proficiency: 0,
        input: Default::default(),
        guard: Default::default(),
        hp: i32::from(member.hp),
        tp: member.tp,
        control_ex_state: Default::default(),
        casting_state: Default::default(),
        stored_spell: None,
        control_slot: 0,
        elements: Default::default(),
        attack_power: 100,
        conditions,
        position,
        heading,
        facing_direction: resonance_battle::direction_from_heading(heading),
        effect_scale: 1.,
        body: Default::default(),
        movement: Default::default(),
        reaction: Default::default(),
        hit_stop: 0,
        time_stop: 0,
    })
}

fn affinity(value: i16) -> Affinity {
    // Combine resistance values before choosing the damage affinity.
    match value {
        ..=-2 => Affinity::Weak,
        -1..=1 => Affinity::Normal,
        2..=4 => Affinity::Resistant,
        5..=6 => Affinity::Immune,
        _ => Affinity::Absorb,
    }
}

/// Normal combo capacity starts with the character baseline, then applies equipped skills.
pub(super) fn normal_combo_limit(skills: [u8; 4], character: usize) -> u8 {
    let base = if skills.contains(&ex::SIX_HIT_COMBO) {
        6
    } else if matches!(character, 1 | 2 | 3 | 6 | 7) {
        2
    } else {
        3
    };
    base + u8::from(skills.contains(&ex::ADDITIONAL_COMBO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires current cooked encounter and equipment assets; CPU only"]
    fn prepared_profile_conditions_survive_live_equipment_changes() -> Result<()> {
        use resonance_battle::conditions::{
            Condition::{
                AilmentResistance, AttackUp, CastingSpeed, Curse, Paralysis, PoisonMild,
                RegenerateHp,
            },
            ConditionSet,
        };
        use resonance_battle::{
            ActorId, BattleInput, EquipmentReplacement, Playback, PreparedBattle,
        };
        use resonance_content::{
            battle_profile,
            prepared::{Cache, Files},
            session::SessionData,
        };
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
            });
        let mut cache = Cache::default();
        let mut files = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
            false
        })?;
        let mut menus: MenuData = files.json("game/menu-data.json")?;
        let mut data: SessionData = files.json("game/session-data.json")?;
        data.rules = Some(Arc::new(menus.clone()));
        let mut party = Party::new(&data, Default::default())?;
        party.formation = vec![1];
        party.members[0].equipment = [135, 0, 0, 0, 0, 367];
        party.members[0].ailments.poison = resonance_events::party::Poison::Mild;
        party.members[0].ex_skills = [0; 4];
        menus.items[367].properties = resonance_content::menu_data::EquipmentProperties {
            ailment_resistance: true,
            immunities: [resonance_content::menu_data::EquipmentAilment::Paralysis].into(),
            hp_regeneration: 1,
            ..Default::default()
        };
        let mut profiles: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
        profiles.records[0].initial_conditions = AttackUp.into();
        profiles.records[0].intrinsic_conditions =
            ConditionSet::of(&[CastingSpeed, AilmentResistance]);
        profiles.records[0].immunities = ConditionSet::of(&[Paralysis, Curse]);
        files.insert(
            battle_profile::PARTY_PATH.into(),
            serde_json::to_vec(&profiles)?.into(),
        );
        let inputs = crate::battle::encounter::Inputs::load(
            &root,
            &files,
            &menus,
            &data,
            &party,
            resonance_events::battle::Setup {
                route: [0; 5],
                encounter: resonance_events::battle::Encounter::Formation(1),
                arena: 13,
                music: None,
                defeat: resonance_events::battle::DefeatPolicy::ResumeEvent,
            },
            &mut cache,
            || false,
        )?;
        let member = inputs.prepare_party_member(
            &menus,
            0,
            loadout(&menus, &party.members[0], 0)?,
            Setup {
                position: [0.; 3],
                heading: 0.,
                model: Some(ModelSetup {
                    resource: 1,
                    initial: Playback {
                        clip: 0,
                        frame: 0.,
                        rate: 0.5,
                        repeat: true,
                    },
                    suppress_root_translation: [true; 3],
                }),
            },
            false,
            100,
            None,
        )?;
        let mut battle = PreparedBattle::new(
            vec![(member.actor, Default::default())],
            Default::default(),
            1,
        )?
        .finish()?;
        let profile = ConditionSet::of(&[CastingSpeed, AilmentResistance]);
        assert_eq!(
            battle.actors()[0].conditions.effective(),
            profile.union(ConditionSet::of(&[AttackUp, PoisonMild, RegenerateHp]))
        );
        let initial_duration = battle.actors()[0].conditions.remaining(AttackUp).unwrap();
        for _ in 0..3 {
            battle.step(BattleInput::default())?;
        }
        let before = battle.actors()[0].conditions.clone();
        assert!(
            before
                .remaining(AttackUp)
                .is_some_and(|remaining| remaining < initial_duration)
        );
        party.members[0].equipment[5] = 0;
        let (attributes, conditions) = loadout(&menus, &party.members[0], 0)?
            .equipment_attributes(&party.members[0], &before, false)?;
        battle.replace_equipment_batch(vec![EquipmentReplacement {
            actor: ActorId::from_index(0)?,
            attributes,
            conditions,
            equipment: None,
        }])?;
        let after = &battle.actors()[0].conditions;
        assert_eq!(
            after.effective(),
            profile.union(ConditionSet::of(&[AttackUp, PoisonMild]))
        );
        assert_eq!(after.immunity(), ConditionSet::of(&[Paralysis, Curse]));
        assert_eq!(after.active_effects(), before.active_effects());
        assert_eq!(after.periodic_effects().len(), 1);
        assert_eq!(after.periodic_effects()[0], before.periodic_effects()[0]);
        Ok(())
    }

    #[test]
    fn resistance_maps_to_damage_affinity() {
        for (value, expected) in [
            (-2, Affinity::Weak),
            (-1, Affinity::Normal),
            (1, Affinity::Normal),
            (2, Affinity::Resistant),
            (4, Affinity::Resistant),
            (5, Affinity::Immune),
            (6, Affinity::Immune),
            (7, Affinity::Absorb),
            (127, Affinity::Absorb),
            (128, Affinity::Absorb),
            (255, Affinity::Absorb),
            (i16::MAX, Affinity::Absorb),
            (i16::MIN, Affinity::Weak),
        ] {
            assert_eq!(affinity(value), expected, "resistance {value}");
        }
    }
}
