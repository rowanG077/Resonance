//! Fierce Demon Fang keeps its character track and selects its shockwave at entry.
use super::{ActionDefinition, Character, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{ActionExecution, AttackEvent, ProjectileContact, ProjectileDefinition};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_fierce_fang<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
        grave_blade: bool,
    ) -> Result<ActionDefinition> {
        let base = if self.character == Character::Lloyd {
            4
        } else {
            209
        };
        let mut timeline = resources
            .voices
            .party_table()
            .martial_attack(base, self.character as u8)?
            .clone();
        let recipe = timeline
            .fierce_fang
            .take()
            .context("missing Fierce Demon Fang recipe")?;
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let knight_delay = if self.character == Character::Lloyd {
            0
        } else {
            5
        };
        let mut earth = timeline.clone();
        earth.effect = Some(11);
        for (_, event) in &mut earth.events {
            if let resonance_content::battle_normal::Event::Contact(contact) = event {
                contact.hit.element = resonance_content::battle_action::HitElement::Element(
                    resonance_content::menu_data::Element::Earth,
                );
            }
        }
        let mut earth = self.prepare_authored_attack(resources, model, tp_cost, earth)?;
        let row = resources
            .projectiles
            .records
            .get(9)
            .context("missing Grave Blade projectile")?
            .clone();
        let mut projectile = resources.prepared.projectile(
            resources.files,
            &row,
            crate::battle::hit::authored(&recipe.hits[1], &recoil)?,
            &[(1, &resources.techniques)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("Grave Blade has no contact")?
            .cooldown = recipe.hits[1].cooldown;
        let ActionExecution::Attack(attack) = &mut earth.execution else {
            unreachable!()
        };
        if !grave_blade {
            attack
                .events
                .retain(|(_, event)| !matches!(event, AttackEvent::Notice { .. }));
        }
        for index in 0..5 {
            attack.events.push((
                22 + knight_delay + index * 3,
                AttackEvent::Projectile(Arc::clone(&projectile)),
            ));
        }
        attack.events.sort_by_key(|(at, _)| *at);
        if grave_blade {
            return Ok(earth);
        }
        let mut ordinary = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut ordinary.execution else {
            unreachable!()
        };
        attack
            .events
            .extend(resources.fierce_fang_shockwaves(&recipe.hits[0], 24 + knight_delay)?);
        attack.events.sort_by_key(|(at, _)| *at);
        let mut entry = ordinary.clone();
        let ActionExecution::Attack(attack) = &mut entry.execution else {
            unreachable!()
        };
        attack.events = vec![(
            0,
            AttackEvent::ChooseGraveBlade([Arc::new(ordinary), Arc::new(earth)]),
        )];
        Ok(entry)
    }
}

impl<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>
    crate::battle::encounter::resources::Resources<'_, S>
{
    pub(crate) fn fierce_fang_shockwaves(
        &mut self,
        hit: &resonance_content::battle_action::ActionHit,
        first_at: u16,
    ) -> Result<Vec<(u16, AttackEvent)>> {
        let recoil = crate::battle::recoil::Parameters::load(self.files)?;
        [90., 120., 150.]
            .into_iter()
            .enumerate()
            .map(|(index, forward)| {
                let mut hit = *hit;
                hit.recoil = if index == 2 { 7 } else { 1 };
                Ok((
                    first_at + index as u16 * 4,
                    AttackEvent::Projectile(Arc::new(ProjectileDefinition {
                        lifetime: Some(2),
                        velocity: [0.; 3],
                        acceleration: [0.; 3],
                        offset: [0., 50., forward],
                        clamp_ground: false,
                        active: None,
                        birth: None,
                        motion: Default::default(),
                        effects: Default::default(),
                        contact: Some(ProjectileContact {
                            hit: crate::battle::hit::authored(&hit, &recoil)?,
                            cooldown: hit.cooldown,
                            repeat_limit: 0,
                            radius: 90.,
                            height: 90.,
                            shape: resonance_battle::HitShape::Box,
                            offset: [0.; 3],
                            radius_growth: 0.,
                            height_growth: 0.,
                            survives_contact: true,
                            clashes: false,
                        }),
                    })),
                ))
            })
            .collect()
    }
}
