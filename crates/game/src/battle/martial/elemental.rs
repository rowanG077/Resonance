//! Prepare elemental variants as complete, independently selectable actions.
use super::{PreparedAttack, Sequence, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result, bail};
use resonance_battle::{AttackEvent, AttackOrigin};
use resonance_content::{
    arte::elemental,
    battle_action::{HitElement, Power},
    battle_martial::Binding,
    battle_normal::Event,
    menu_data::Element,
};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_super_swallow_dance(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let mut attack = resources
            .voices
            .party_table()
            .martial_attacks
            .variant(Binding {
                member: 119,
                variant: 1,
            })?
            .clone();
        for (_, event) in &mut attack.events {
            if let Event::Contact(contact) = event {
                contact.hit.element = HitElement::Element(Element::Wind);
            }
        }
        self.prepare_authored_attack(resources, model, attack)
    }

    pub(super) fn prepare_elemental(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<PreparedAttack> {
        match self.catalogue {
            elemental::RISING_PHOENIX => self.prepare_rising_falcon(resources, model, tp_cost),
            elemental::GRAVE_BLADE | elemental::KNIGHT_GRAVE_BLADE => Self {
                sequence: Sequence::FierceDemonFang,
                ..self
            }
            .prepare_fierce_fang(resources, model, tp_cost, true),
            elemental::ICE_HAMMER => self.prepare_hammer(resources, model, tp_cost, true),
            elemental::SUPER_SWALLOW_DANCE => self.prepare_super_swallow_dance(resources, model),
            elemental::LIGHTNING_THRUST | elemental::LIGHTNING_TIGER => {
                self.prepare_lightning_variant(resources, model)
            }
            _ => bail!("unknown elemental arte"),
        }
    }

    fn prepare_lightning_variant(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let tiger = self.catalogue == elemental::LIGHTNING_TIGER;
        let binding = if tiger {
            Binding {
                member: 6,
                variant: 1,
            }
        } else {
            Binding {
                member: 10,
                variant: 3,
            }
        };
        let mut timeline = resources
            .voices
            .party_table()
            .martial_attacks
            .variant(binding)?
            .clone();
        for (_, event) in &mut timeline.events {
            if let Event::Contact(contact) = event {
                contact.hit.element = HitElement::Element(Element::Lightning);
            }
        }
        let mut hit = timeline
            .events
            .iter()
            .rev()
            .find_map(|(_, event)| match event {
                Event::Contact(contact) => Some(contact.hit),
                _ => None,
            })
            .context("elemental sword arte has no contact")?;
        hit.kind = resonance_content::battle_projectile::DamageKind::Slash;
        hit.recoil = 1;
        if !tiger {
            hit.power = Power::Percent(50);
            hit.guard_pressure = 2;
            hit.stun_chance = 10;
            hit.stagger = 0;
        }
        let recoil = resources.recoil;
        let mut projectile = resources
            .projectiles
            .records
            .get(4)
            .context("missing lightning projectile")?
            .clone();
        const LIGHTNING_STRIKE_EFFECT: u8 = 74;
        projectile.birth_effect.member = LIGHTNING_STRIKE_EFFECT;
        if tiger {
            projectile.spawn_offset[2] = 120.;
        }
        let mut projectile = resources.prepared.projectile(
            resources.files,
            &projectile,
            crate::battle::hit::authored(&hit, recoil)?,
            &[(1, &resources.techniques)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("lightning projectile has no contact")?
            .cooldown = hit.cooldown;
        let mut definition = self.prepare_authored_attack(resources, model, timeline)?;
        if tiger {
            definition = super::tiger_blade::finish_on_landing(definition)?;
        }
        const LIGHTNING_RELEASE_TICK: u16 = 29;
        definition.events.push((
            LIGHTNING_RELEASE_TICK,
            if tiger {
                AttackEvent::ProjectileAt {
                    definition: projectile,
                    origin: AttackOrigin::ActorRoot,
                    heading: None,
                    height: Some(0.),
                }
            } else {
                AttackEvent::ContactBurst {
                    definition: projectile,
                    appearance: None,
                }
            },
        ));
        definition.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
