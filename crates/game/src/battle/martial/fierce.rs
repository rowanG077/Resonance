//! Fierce Demon Fang keeps its character track and selects its shockwave at entry.
use super::{Character, PreparedAttack, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result};
use resonance_battle::{AttackEvent, HitShape, ProjectileDefinition};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_fierce_fang(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
        grave_blade: bool,
    ) -> Result<PreparedAttack> {
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
        let recoil = resources.recoil;
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
        let mut earth = self.prepare_authored_attack(resources, model, earth)?;
        let row = resources
            .projectiles
            .records
            .get(9)
            .context("missing Grave Blade projectile")?
            .clone();
        let mut projectile = resources.prepared.projectile(
            resources.files,
            &row,
            crate::battle::hit::authored(&recipe.hits[1], recoil)?,
            &[(1, &resources.techniques)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("Grave Blade has no contact")?
            .cooldown = recipe.hits[1].cooldown;
        if !grave_blade {
            earth
                .events
                .retain(|(_, event)| !matches!(event, AttackEvent::Notice { .. }));
        }
        for index in 0..5 {
            earth.events.push((
                22 + knight_delay + index * 3,
                AttackEvent::Projectile(Arc::clone(&projectile)),
            ));
        }
        earth.events.sort_by_key(|(at, _)| *at);
        if grave_blade {
            return Ok(earth);
        }
        let mut ordinary = self.prepare_authored_attack(resources, model, timeline)?;
        ordinary
            .events
            .extend(resources.fierce_fang_shockwaves(&recipe.hits[0], 24 + knight_delay)?);
        ordinary.events.sort_by_key(|(at, _)| *at);
        let mut entry = ordinary.clone();
        entry.events = vec![(
            0,
            AttackEvent::ChooseGraveBlade(
                [ordinary, earth].map(|attack| Arc::new(super::attack_definition(attack, tp_cost))),
            ),
        )];
        Ok(entry)
    }
}

impl Resources<'_> {
    pub(crate) fn fierce_fang_shockwaves(
        &mut self,
        hit: &resonance_content::battle_action::ActionHit,
        first_at: u16,
    ) -> Result<Vec<(u16, AttackEvent)>> {
        let recoil = self.recoil;
        [90., 120., 150.]
            .into_iter()
            .enumerate()
            .map(|(index, forward)| {
                let mut hit = *hit;
                hit.recoil = if index == 2 { 7 } else { 1 };
                Ok((
                    first_at + index as u16 * 4,
                    AttackEvent::Projectile(Arc::new(ProjectileDefinition {
                        offset: [0., 50., forward],
                        ..super::stationary_contact(&hit, recoil, 2, HitShape::Box, [90.; 2])?
                    })),
                ))
            })
            .collect()
    }
}
