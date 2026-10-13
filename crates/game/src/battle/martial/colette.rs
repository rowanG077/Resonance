//! Colette's projectile families share authored tracks and prepared launches.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActionExecution, AttackEvent, HammerProjectile, HammerRing, ProjectileScatter, ProjectileShot,
};
use resonance_content::battle_martial::{HammerPattern, ProjectileFeedback};
use resonance_content::{battle_martial::PoseStart, battle_normal::Event};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_colette_projectiles<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
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
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let mut projectile = |member: usize, rule: u8| {
            let row = resources
                .projectiles
                .records
                .get(member)
                .context("missing Colette projectile")?
                .clone();
            let hit = source
                .callback_hits
                .get(&rule)
                .context("missing Colette projectile hit")?;
            super::sword::projectile(resources, &row, hit, None, &recoil)
        };
        let shot = |definition, offset, heading_offset, scatter| {
            AttackEvent::ProjectileShot(Arc::new(ProjectileShot {
                definition,
                offset,
                heading_offset,
                scatter,
            }))
        };
        let mut events = Vec::new();
        match self.catalogue {
            37 | 39 => {
                let (member, spread) = if self.catalogue == 37 {
                    (6, 20.)
                } else {
                    (11, 45.)
                };
                let ray = projectile(member, 0)?;
                const RELEASES: [u16; 3] = [24, 30, 36];
                for (at, angle) in RELEASES.into_iter().zip([spread, 0., -spread]) {
                    events.push((at, shot(Arc::clone(&ray), [0.; 3], angle, None)));
                }
            }
            43 | 44 => {
                let child = projectile(4, 1)?;
                let mut ball = projectile(10, 0)?;
                Arc::make_mut(&mut ball).effects.rebound_projectile = Some(child);
                let count = if self.catalogue == 43 { 2 } else { 4 };
                for index in 0..count {
                    let side = if index % 2 == 0 { 35. } else { -35. };
                    events.push((
                        15 + index * 20,
                        shot(Arc::clone(&ball), [side, 110., 40.], 0., None),
                    ));
                }
                // These captures establish the attack clock with absent default
                // motions. Display the real alternate-bank clips without adding
                // unmeasured blend delays to the contacts or recovery.
                for step in source
                    .pose_sequence
                    .take()
                    .context("missing Para Ball pose track")?
                {
                    let PoseStart::At(at) = step.start else {
                        anyhow::bail!("unexpected Para Ball continuation pose");
                    };
                    let mut pose = step.pose;
                    pose.blend = 0;
                    source.events.push((at, Event::Pose(pose)));
                }
                source.events.sort_by_key(|(at, _)| *at);
            }
            50 => {
                let ray = projectile(25, 0)?;
                const CROSSING_ORIGIN: [f32; 3] = [0., 0., 200.];
                for angle in [45., -45.] {
                    events.push((28, shot(Arc::clone(&ray), CROSSING_ORIGIN, angle, None)));
                }
                events.push((40, shot(projectile(4, 1)?, CROSSING_ORIGIN, 0., None)));
            }
            52 => {
                let mut opening = projectile(13, 0)?;
                Arc::make_mut(&mut opening).lifetime = Some(30);
                events.push((25, AttackEvent::Projectile(opening)));

                let mut rain = projectile(24, 1)?;
                let definition = Arc::make_mut(&mut rain);
                // This callback places each hammer on a world grid instead of
                // using the template's ordinary actor-local launch offset.
                definition.offset = [0.; 3];
                definition.motion.velocity_jitter = [0.; 3];
                let rain = Arc::new(ProjectileShot {
                    definition: rain,
                    offset: [0., 380., 220.],
                    heading_offset: 0.,
                    scatter: Some(ProjectileScatter {
                        position_range: 4,
                        position_step: 35.,
                        lateral_range: 40,
                        lateral_step: 0.1,
                    }),
                });
                const RAIN_RELEASES: std::ops::RangeInclusive<u16> = 53..=65;
                events.extend(
                    RAIN_RELEASES.map(|at| (at, AttackEvent::ProjectileShot(Arc::clone(&rain)))),
                );
            }
            _ => anyhow::bail!("unexpected Colette projectile family"),
        }
        let mut definition = self.prepare_authored_attack(resources, model, tp_cost, source)?;
        let ActionExecution::Attack(attack) = &mut definition.execution else {
            unreachable!("martial preparation produces an attack")
        };
        attack.events.extend(events);
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }

    pub(super) fn prepare_hammer_volley<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
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
            .context("missing hammer family recipe")?;
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let mut row = resources
            .projectiles
            .records
            .get(7)
            .context("missing hammer projectile")?
            .clone();
        let (ring, opening) = match &hammer.pattern {
            HammerPattern::Volley {
                offset,
                forward,
                jitter,
            } => {
                ensure!(self.catalogue == 41, "unexpected hammer volley catalogue");
                row.spawn_offset = *offset;
                row.velocity[2] = *forward;
                row.motion.velocity_jitter = [*jitter; 3];
                (None, None)
            }
            HammerPattern::Ring {
                offset,
                step,
                radians,
                speed,
                speed_jitter,
                lift,
                lift_jitter,
                opening_hit,
            } => {
                ensure!(self.catalogue == 42, "unexpected hammer rain catalogue");
                row.spawn_offset = *offset;
                let opening = resources
                    .projectiles
                    .records
                    .get(13)
                    .context("missing hammer rain opening projectile")?
                    .clone();
                let opening =
                    super::sword::projectile(resources, &opening, opening_hit, None, &recoil)?;
                // The birth count and signed jitter range come from the observed burst.
                // Magnitudes and spacing are supplied by the authored parameters.
                (
                    Some(HammerRing {
                        count: 10,
                        angle_step: step * radians,
                        speed: *speed,
                        speed_jitter: *speed_jitter,
                        lift: *lift,
                        lift_jitter: *lift_jitter,
                        jitter_range: 50,
                    }),
                    Some(opening),
                )
            }
            HammerPattern::Single { .. } => anyhow::bail!("expected a repeated hammer pattern"),
        };
        let feedback = Some(ProjectileFeedback {
            bank: 1,
            effect: hammer.impact_effect,
            sound: hammer.impact_sound,
            on_guard: true,
        });
        let mut choices = Vec::with_capacity(3);
        for (hit, birth) in hammer.hits.iter().zip([15, 16, 17]) {
            row.birth_effect.member = birth;
            let mut projectile = super::sword::projectile(resources, &row, hit, feedback, &recoil)?;
            Arc::make_mut(&mut projectile).effects.spin_on_rebound = true;
            choices.push(projectile);
        }
        let shots = Arc::new(HammerProjectile {
            choices: choices
                .try_into()
                .map_err(|_| anyhow::anyhow!("missing hammer variant"))?,
            uses_catalogue: 41,
            ring,
        });
        let mut definition = self.prepare_authored_attack(resources, model, tp_cost, source)?;
        let ActionExecution::Attack(attack) = &mut definition.execution else {
            unreachable!()
        };
        // Paid-entry ages from the ordinary activation; these include pose blends.
        if let Some(opening) = opening {
            const OPENING: u16 = 25;
            const RAIN: u16 = 66;
            attack.events.extend([
                (OPENING, AttackEvent::Projectile(opening)),
                (RAIN, AttackEvent::HammerProjectile(shots)),
            ]);
        } else {
            const RELEASES: [u16; 4] = [24, 26, 28, 30];
            attack
                .events
                .extend(RELEASES.map(|at| (at, AttackEvent::HammerProjectile(Arc::clone(&shots)))));
        }
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
