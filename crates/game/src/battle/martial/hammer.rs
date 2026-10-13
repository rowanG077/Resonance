//! Pow Hammer shares the authored pose/voice timeline and one ballistic recipe.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{ActionExecution, AttackEvent, EffectAppearance, ProjectileContactFeedback};
use resonance_content::battle_martial::HammerPattern;
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_hammer<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let mut source = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let hammer = source
            .hammer
            .take()
            .context("missing Pow Hammer variants")?;
        let HammerPattern::Single { alternate_events } = &hammer.pattern else {
            anyhow::bail!("Pow Hammer has a different release pattern");
        };
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let row = resources
            .projectiles
            .records
            .get(7)
            .context("missing Pow Hammer projectile")?
            .clone();
        resources.effect(true, &[hammer.impact_effect])?;
        let feedback = ProjectileContactFeedback {
            appearance: EffectAppearance {
                resource: resources.techniques.resource,
                member: hammer.impact_effect,
            },
            on_guard: true,
            sound: (resources.sound)(Sound::Cue(hammer.impact_sound))?,
        };
        let mut variants = Vec::new();
        for (index, hit) in hammer.hits.iter().enumerate() {
            let mut timeline = source.clone();
            if index != 0 {
                timeline.events.clone_from(alternate_events);
            }
            let mut definition =
                self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
            let ActionExecution::Attack(attack) = &mut definition.execution else {
                unreachable!()
            };
            if index != 0 {
                attack
                    .events
                    .retain(|(_, event)| !matches!(event, AttackEvent::Notice { .. }));
            }
            let mut row = row.clone();
            row.birth_effect.member = [15, 16, 17][index];
            let mut projectile = resources.prepared.projectile(
                resources.files,
                &row,
                crate::battle::hit::authored(hit, &recoil)?,
                &[(1, &resources.techniques)],
            )?;
            let projectile_data = Arc::make_mut(&mut projectile);
            projectile_data
                .contact
                .as_mut()
                .context("hammer has no contact")?
                .cooldown = hit.cooldown;
            projectile_data.effects.contact = Some(feedback);
            projectile_data.effects.spin_on_rebound = true;
            // Measured paid-entry to release interval, including the initial blend.
            const RELEASE_TICK: u16 = 25;
            attack
                .events
                .push((RELEASE_TICK, AttackEvent::Projectile(projectile)));
            attack.events.sort_by_key(|(at, _)| *at);
            variants.push(Arc::new(definition));
        }
        let variants: [Arc<ActionDefinition>; 3] = variants
            .try_into()
            .map_err(|_| anyhow::anyhow!("missing hammer variant"))?;
        let mut definition = (*variants[0]).clone();
        let ActionExecution::Attack(attack) = &mut definition.execution else {
            unreachable!()
        };
        attack.events = vec![(0, AttackEvent::ChooseHammer(variants))];
        Ok(definition)
    }
}
