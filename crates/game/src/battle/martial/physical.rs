//! Physical arte callbacks add only emissions absent from the authored action track.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result, bail};
use resonance_battle::{
    ActionExecution, AttackEvent, AttackOrigin, AttackPose, EffectAppearance, ModelDefinition,
};

impl Technique {
    pub(super) fn prepare_eagle<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let timeline = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let landing_ring = if self.catalogue == 187 {
            let hit = timeline
                .callback_hits
                .get(&1)
                .context("missing Eagle Fall landing hit")?;
            let row = resources
                .projectiles
                .records
                .get(16)
                .context("missing Eagle Fall landing projectile")?
                .clone();
            let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
            Some(super::sword::projectile(
                resources, &row, hit, None, &recoil,
            )?)
        } else {
            None
        };
        resources.effect(true, &[92])?;
        let landing_effect = EffectAppearance {
            resource: resources.techniques.resource,
            member: 92,
        };
        let mut action = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut action.execution else {
            unreachable!("martial preparation produces an attack")
        };
        let recovery_pose = attack
            .events
            .iter()
            .find_map(|(at, event)| match event {
                AttackEvent::Pose(pose) if *at == attack.end_at => Some(*pose),
                _ => None,
            })
            .context("missing Eagle recovery pose")?;
        let landing_at = attack
            .events
            .iter()
            .filter_map(|(at, event)| matches!(event, AttackEvent::Contact { .. }).then_some(*at))
            .max()
            .context("missing Eagle dive contact")?
            + 1;
        attack.events.retain(|(at, _)| *at < landing_at);
        attack.chain_at = None;
        let landing_pose = AttackPose {
            motion: crate::battle::model::motion(model, 52)
                .context("missing Eagle landing pose")?,
            start: 0.,
            end: None,
            rate: recovery_pose.rate,
            blend: 2,
            repeat: false,
        };
        const LANDING_TICKS: u16 = 24;
        // The ring is released only after the grounded pose and before recovery.
        attack.events.push((
            landing_at,
            AttackEvent::Land {
                pose: Some(landing_pose),
                effect: Some(landing_effect),
                settle: landing_ring.is_none().then_some(LANDING_TICKS),
                recovery_pose: Some(recovery_pose),
            },
        ));
        if let Some(projectile) = landing_ring {
            attack
                .events
                .push((landing_at, AttackEvent::Projectile(projectile)));
            attack.events.push((
                landing_at,
                AttackEvent::Land {
                    pose: None,
                    effect: None,
                    settle: Some(LANDING_TICKS),
                    recovery_pose: Some(recovery_pose),
                },
            ));
        }
        Ok(action)
    }

    pub(super) fn prepare_destruction<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let timeline = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let hit = timeline
            .callback_hits
            .get(&0)
            .context("missing Destruction rock hit")?;
        let mut rock = resources
            .projectiles
            .records
            .get(9)
            .context("missing Destruction rock projectile")?
            .clone();
        // Each sweep releases its rocks ahead of the ordinary projectile origin.
        const AXE_RELEASE_FORWARD: f32 = 25.;
        const RELEASE_INTERVAL: u16 = 3;
        let volleys: &[(u16, u16)] = match self.catalogue {
            154 => &[(36, 5)],
            155 => &[(36, 5), (59, 4)],
            156 => &[(36, 5), (62, 4), (104, 4)],
            157 => &[(36, 3)],
            170 => &[(36, 2), (45, 2)],
            171 => &[(36, 2), (45, 3)],
            _ => bail!("missing Destruction volley schedule"),
        };
        rock.spawn_offset[2] += AXE_RELEASE_FORWARD;
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let projectile = super::sword::projectile(resources, &rock, hit, None, &recoil)?;
        let mut action = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut action.execution else {
            unreachable!("martial preparation produces an attack")
        };
        for &(start, count) in volleys {
            attack.events.extend((0..count).map(|index| {
                (
                    start + index * RELEASE_INTERVAL,
                    AttackEvent::ProjectileAt {
                        definition: projectile.clone(),
                        origin: AttackOrigin::ActorRoot,
                        heading: None,
                        // Later sweeps can lift Presea; the rocks still begin at ground level.
                        height: Some(0.),
                    },
                )
            }));
        }
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(action)
    }
}
