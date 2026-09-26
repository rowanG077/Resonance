//! Project the suspended session's formation into owned battle candidates.
//! Original 5878/40C8 select the first four slots; 1CAA8/CEA8 prepare each actor.
use super::model::{self, ModelSetup};
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Actor, Affinity, AttackElements, CombatStats, Control, ModelDefinition, Side,
};
use resonance_content::{battle_model, menu_data::MenuData, prepared::Files};
use resonance_events::party::{Member, Party};
use std::sync::Arc;

pub struct Setup {
    pub position: [f32; 3],
    pub heading: f32,
    pub model: ModelSetup,
}

pub struct Prepared {
    /// One-based persistent character identity, independent of the actor slot.
    pub character: u8,
    pub actor: Actor,
    pub model: Arc<ModelDefinition>,
    pub weapons: Vec<Weapon>,
}

/// Resource selection and body attachment for the presentation/contact loader.
/// This does not select rigid, owner-linked or independent weapon playback.
pub struct Weapon {
    pub item: u16,
    pub part: u8,
    pub slot: u8,
    pub bone: u16,
}

impl Prepared {
    /// Presentation allocates resource IDs; the battle candidate owns every
    /// carried pose and contact. Install the complete set atomically.
    pub fn attach_weapons(
        &mut self,
        files: &Files,
        resources: &std::collections::BTreeMap<u8, u32>,
    ) -> Result<Vec<Vec<u16>>> {
        ensure!(
            self.model.weapons.is_empty()
                && resources.len() == self.weapons.len()
                && self
                    .weapons
                    .iter()
                    .all(|weapon| resources.contains_key(&weapon.slot)),
            "weapon presentation slots differ from prepared equipment"
        );
        let mut candidate = (*self.model).clone();
        let mut groups = Vec::new();
        for weapon in &self.weapons {
            let source = model::weapon(files, weapon.item)?;
            let part = source
                .parts
                .get(&weapon.part)
                .context("missing weapon part")?;
            let playback = if self.character == 3 {
                super::weapon::owner_linked()
            } else {
                resonance_battle::WeaponPlayback::Rigid
            };
            groups.extend(super::weapon::attach(
                files,
                &mut candidate,
                part,
                weapon.slot,
                weapon.bone,
                resources[&weapon.slot],
                playback,
            )?);
        }
        self.model = Arc::new(candidate);
        Ok(groups)
    }
}

/// Placement, playback and resource IDs are encounter inputs. This function
/// borrows persistent state throughout; failed preparation publishes nothing.
pub fn prepare(
    files: &Files,
    menus: &MenuData,
    party: &Party,
    setups: Vec<Setup>,
) -> Result<Vec<Prepared>> {
    ensure!(
        !party.formation.is_empty() && setups.len() == party.formation.len().min(4),
        "battle placement must match the first four formation slots"
    );
    let recoil = super::recoil::Parameters::load(files)?;
    let mut prepared = Vec::with_capacity(setups.len());
    for (slot, (&character, setup)) in party.formation.iter().zip(setups).enumerate() {
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
        validate_member(menus, member, index)?;
        let control = match party.settings.battle_controls[slot] {
            0 => Control::Manual,
            1 => Control::SemiAuto,
            2 => Control::Auto,
            _ => anyhow::bail!("invalid battle control for slot {slot}"),
        };
        let actor = actor(menus, member, index, control, setup.position, setup.heading)?;
        let (mut actor, model) = model::party(files, character, actor, setup.model)?;
        actor.hud.control_slot = slot as u8;
        // 1A9AC reads the saved position strategy, with one-based character
        // defaults. Formation/control slots do not select this contribution.
        actor.guard.recovery_bonus = recoil.guard_recovery_bonus(member.strategy[2], character)?;
        if member.ex_skills.contains(&12) {
            actor.guard.reduction = actor.guard.reduction.wrapping_add(5);
        }
        let body: battle_model::Party = files.json(&battle_model::party_path(character))?;
        let mut weapons = Vec::new();
        // 1ABBC/153BC select each weapon by its instance slot.
        // Shields are separate resources attached at slot one (14EF8/8628).
        for (item, first) in [(member.equipment[0], 0), (member.equipment[5], 1)] {
            if item == 0 || first == 1 && !(356..=366).contains(&item) {
                continue;
            }
            let weapon = model::weapon(files, item)?;
            for part in weapon.parts.into_keys() {
                let slot = part.checked_add(first).context("invalid weapon slot")?;
                let attachment = body
                    .body
                    .attachments
                    .get(&slot)
                    .context("missing weapon attachment")?;
                ensure!(
                    usize::from(*attachment) < model.skeleton.bones.len(),
                    "invalid weapon attachment"
                );
                weapons.push(Weapon {
                    item,
                    part,
                    slot,
                    bone: *attachment,
                });
            }
        }
        prepared.push(Prepared {
            character,
            actor,
            model,
            weapons,
        });
    }
    Ok(prepared)
}

fn validate_member(menus: &MenuData, member: &Member, character: usize) -> Result<()> {
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
            matches!(id, 1 | 2 | 4 | 5 | 7 | 10 | 12 | 19 | 21 | 23),
            "battle EX skill {id} is not prepared"
        );
    }
    for &index in &member.compound_ex_skills {
        ensure!(
            usize::from(index) < rules.compounds.len(),
            "invalid compound EX skill"
        );
    }
    for compound in &rules.compounds {
        if compound
            .required
            .iter()
            .all(|id| member.ex_skills.contains(id))
        {
            ensure!(
                menus.ex_skills.skills.contains_key(&compound.skill),
                "missing compound EX skill"
            );
            ensure!(
                matches!(compound.skill, 97 | 134),
                "battle compound EX skill {} is not prepared",
                compound.skill
            );
        }
    }
    for &item in &member.equipment {
        let item = menus
            .items
            .get(usize::from(item))
            .context("missing equipped item")?;
        ensure!(
            item.properties
                .effects
                .iter()
                .all(|id| menus.status.equipment_effects.contains_key(id)),
            "missing equipment effect"
        );
    }
    ensure!(
        !matches!(
            member.equipment[0],
            158 | 174 | 189 | 204 | 218 | 227 | 241 | 257 | 273
        ),
        "Devil's Arms battle power is not prepared"
    );
    ensure!(
        member.conditions & !0x8000_0100 == 0,
        "battle conditions {:#x} are not prepared",
        member.conditions & !0x8000_0100
    );
    ensure!(
        member.conditions & 0x8000_0000 == 0 || member.hp == 0,
        "knocked-out party member has HP"
    );
    ensure!(member.overlimit <= 100, "invalid party Over Limit gauge");
    ensure!(member.level != 0, "invalid party level");
    Ok(())
}

fn actor(
    menus: &MenuData,
    member: &Member,
    character: usize,
    control: Control,
    position: [f32; 3],
    heading: f32,
) -> Result<Actor> {
    let stats = member.stats_for(menus, character);
    let traits = member.equipment_traits(menus);
    ensure!(
        traits.effects.is_empty(),
        "battle equipment effects {:?} are not prepared",
        traits.effects
    );
    ensure!(
        traits.critical_chance_bonus == 0,
        "battle equipment critical bonus is not prepared"
    );
    ensure!(
        stats.hp > 0 && member.hp <= stats.hp && member.tp <= stats.tp,
        "invalid party battle vitals"
    );
    ensure!(
        position.iter().all(|v| v.is_finite()) && heading.is_finite(),
        "invalid battle placement"
    );
    let compound = |id| {
        menus.ex_skills.characters[character]
            .compounds
            .iter()
            .any(|c| c.skill == id && c.required.iter().all(|id| member.ex_skills.contains(id)))
    };
    let mut affinities = [Affinity::Normal; 9];
    affinities[0] = affinity(traits.neutral_resistance);
    for (target, value) in affinities[1..].iter_mut().zip(traits.resistance) {
        *target = affinity(value);
    }
    Ok(Actor {
        side: Side::Party,
        control,
        activity: Default::default(),
        availability: Default::default(),
        overlimit: u16::from(member.overlimit) * 10,
        overlimit_active: false,
        guard: Default::default(),
        hp: i32::from(member.hp),
        max_hp: i32::from(stats.hp),
        tp: member.tp,
        max_tp: stats.tp,
        hud: Default::default(),
        // CEA8 stores the derived signed halfword in the actor's luck byte.
        luck: stats.luck as u8,
        stats: CombatStats {
            slash: stats.slash as i16,
            thrust: stats.thrust as i16,
            defense: stats.defense as i16,
            intelligence: stats.intelligence as i16,
            accuracy: stats.accuracy as i16,
            evasion: stats.evasion as i16,
            level: member.level,
        },
        elements: AttackElements {
            base: traits.attack_element,
            ..Default::default()
        },
        affinities,
        attack_power: 100,
        physical_arte_boost: compound(134),
        recovery: resonance_battle::RecoveryTraits {
            boost: member.ex_skills.contains(&21),
            lucky: compound(97),
            weak: false,
        },
        petrified: member.conditions & 0x100 != 0,
        position,
        heading,
        facing_direction: resonance_battle::direction_from_heading(heading),
        effect_scale: 1.,
        framing: Default::default(),
        body: Default::default(),
        movement: Default::default(),
        reaction: Default::default(),
        hit_stop: 0,
    })
}

fn affinity(value: i16) -> Affinity {
    // CEA8 accumulates nine resistance bytes with wrapping, then compares signed.
    match value as i8 {
        ..=-2 => Affinity::Weak,
        -1..=1 => Affinity::Normal,
        2..=4 => Affinity::Resistant,
        5..=6 => Affinity::Immune,
        _ => Affinity::Absorb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resistance_uses_signed_wrapping_before_original_thresholds() {
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
            (128, Affinity::Weak),
            (255, Affinity::Normal),
            (258, Affinity::Resistant),
        ] {
            assert_eq!(affinity(value), expected, "resistance {value}");
        }
    }
}
