//! A summon restores the interrupted formation before its released spell begins.
use crate::{
    ActorId, Battle, CameraPose, Cue, EffectAppearance, MotionBinding, PreparedSpell, Side, Sound,
    SpellSlot,
};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

const RESUME_AGE: u16 = 110;
const FLASH_AGE: u16 = 120;
const HANDOFF_AGE: u16 = 151;
const CASTER_RECOVERY: u32 = 210;
const ENEMY_RECOVERY: u32 = 120;
const RESUME_CLIP: u16 = 12;

#[derive(Debug, Clone, Copy)]
pub struct SummonEntryDefinition {
    pub effect: EffectAppearance,
    pub sound: Option<Sound>,
}

/// Drawing retains the final zero-opacity flash request before the scene disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonFrame {
    pub owner: Option<ActorId>,
    pub age: Option<u16>,
    pub hide_hud: bool,
    pub generation: u64,
    pub dissolve: u8,
    pub flash: u8,
    pub flash_active: bool,
}

#[derive(Clone, Copy)]
struct Placement {
    position: [f32; 3],
    heading: f32,
    resume: Option<MotionBinding>,
}

struct Entry {
    actor: ActorId,
    target: ActorId,
    parent: Option<crate::ActionId>,
    spell: Arc<PreparedSpell>,
    placements: Vec<Placement>,
    camera: CameraPose,
    resume: MotionBinding,
    age: u16,
    dissolve: u8,
}

#[derive(Clone, Copy)]
enum FlashPhase {
    Rising,
    Hold(u8),
    Falling,
}

#[derive(Clone, Copy)]
struct Flash {
    opacity: u8,
    phase: FlashPhase,
}

impl Flash {
    fn tick(&mut self, entering: bool) -> bool {
        match self.phase {
            FlashPhase::Rising if self.opacity == u8::MAX => {
                if !entering {
                    self.phase = FlashPhase::Hold(16);
                }
            }
            FlashPhase::Rising => self.opacity = self.opacity.saturating_add(8),
            FlashPhase::Hold(1) => self.phase = FlashPhase::Falling,
            FlashPhase::Hold(ticks) => self.phase = FlashPhase::Hold(ticks - 1),
            FlashPhase::Falling if self.opacity == 0 => return false,
            FlashPhase::Falling => self.opacity = self.opacity.saturating_sub(8),
        }
        true
    }
}

#[derive(Default)]
pub(crate) struct State {
    entry: Option<Entry>,
    generation: u64,
    flash: Option<Flash>,
}

impl State {
    pub(crate) fn active(&self) -> bool {
        self.entry.is_some()
    }

    pub(crate) fn owner(&self) -> Option<ActorId> {
        self.entry.as_ref().map(|entry| entry.actor)
    }

    pub(crate) fn camera(&self) -> Option<CameraPose> {
        self.entry.as_ref().map(|entry| entry.camera)
    }

    pub(crate) fn frame(&self) -> Option<SummonFrame> {
        (self.entry.is_some() || self.flash.is_some()).then(|| SummonFrame {
            owner: self.owner(),
            age: self
                .entry
                .as_ref()
                .and_then(|entry| entry.age.checked_sub(1)),
            hide_hud: self.entry.as_ref().is_some_and(|entry| entry.age != 0),
            generation: self.generation,
            dissolve: self.entry.as_ref().map_or(0, |entry| entry.dissolve),
            flash: self.flash.map_or(0, |flash| flash.opacity),
            flash_active: self.flash.is_some(),
        })
    }

    pub(crate) fn tick_flash(&mut self) {
        if let Some(flash) = &mut self.flash
            && !flash.tick(self.entry.is_some())
        {
            self.flash = None;
        }
    }
}

impl Battle {
    pub(crate) fn begin_summon_entry(
        &mut self,
        definition: SummonEntryDefinition,
        spell: Arc<PreparedSpell>,
        actor: ActorId,
        target: ActorId,
        parent: Option<crate::ActionId>,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        if self.summon_entry.active() || self.unison.active() {
            return Ok(false);
        }
        ensure!(
            spell.transition_ticks == 0,
            "summon callback has an ordinary prelude"
        );
        let caster = self.actor(actor)?;
        ensure!(
            caster.side == Side::Party && caster.available(),
            "summon caster is unavailable"
        );
        self.actor(target)?;
        let model = self
            .models
            .as_ref()
            .and_then(|models| models.definition(actor))
            .context("summon entry has no caster model")?;
        ensure!(
            model.motions.contains_key(&RESUME_CLIP),
            "summon concluding motion is not prepared"
        );
        let resume = MotionBinding {
            model: model.resource,
            clip: RESUME_CLIP,
        };
        let [x, y, z] = caster.position;
        let vertical: f32 = 875. - 75.;
        let distance: f32 = 2250.;
        let camera = CameraPose {
            eye: [x, y + 875., z + distance],
            focus: [x, y + 75., z],
            radius: distance.hypot(vertical),
            pitch: vertical.atan2(distance).to_degrees(),
            yaw: 90.,
        };
        let placements = self
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                let resume = if actor.side == Side::Enemy && actor.available() {
                    let model = self
                        .models
                        .as_ref()
                        .and_then(|models| models.definition(ActorId(index as u8)))
                        .context("summon restore has no enemy model")?;
                    ensure!(
                        model.motions.contains_key(&3),
                        "summon enemy restore motion is not prepared"
                    );
                    Some(MotionBinding {
                        model: model.resource,
                        clip: 3,
                    })
                } else {
                    None
                };
                Ok(Placement {
                    position: [
                        actor.position[0],
                        if actor.movement.flying {
                            actor.movement.hover_height
                        } else {
                            crate::movement::GROUND_TOLERANCE
                        },
                        actor.position[2],
                    ],
                    heading: actor.heading,
                    resume,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let generation = self
            .summon_entry
            .generation
            .checked_add(1)
            .context("summon scene generation overflow")?;
        self.items.pending = None;
        self.target_selector = None;
        self.command_target = None;
        self.timed_hold = None;
        if let Some(camera) = &mut self.camera {
            camera.clear_scene();
        }
        for index in 0..self.actors.len() {
            let id = ActorId(index as u8);
            if id != actor {
                self.interrupt_actor(id, cues);
            }
            let owner = &mut self.actors[index];
            owner.input = Default::default();
            owner.hit_stop = 0;
            if owner.overlimit.is_active() {
                owner.overlimit = Default::default();
            }
            owner.guard.active = false;
            owner.guard.recovery = 0;
        }
        for id in std::mem::take(&mut self.volleys).keys() {
            cues.push(Cue::Interrupted { action: *id });
        }
        for projectile in self.projectiles.keys().copied().collect::<Vec<_>>() {
            self.expire_projectile(projectile, cues);
        }
        self.retire_weapon_flights();
        cues.push(Cue::ClearEffects);
        let caster = &self.actors[actor.index()];
        cues.push(Cue::Effect(crate::EffectRequest {
            owner: actor,
            target: actor,
            appearance: definition.effect,
            origin: caster.position,
            heading: caster.heading,
            follow: Some(crate::EffectFollow::Actor(actor)),
            scale: 1.,
            tint: Default::default(),
        }));
        if let Some(sound) = definition.sound {
            cues.push(Cue::Sound {
                actor,
                sound,
                position: caster.position,
                priority: 1,
            });
        }
        self.summon_entry = State {
            entry: Some(Entry {
                actor,
                target,
                parent,
                spell,
                placements,
                camera,
                resume,
                age: 0,
                dissolve: u8::MAX,
            }),
            generation,
            flash: None,
        };
        Ok(true)
    }

    /// The restore update initializes the active callback without revisiting ordinary casting.
    pub(crate) fn advance_summon_entry(&mut self, cues: &mut Vec<Cue>) -> Result<()> {
        let Some(mut entry) = self.summon_entry.entry.take() else {
            return Ok(());
        };
        entry.dissolve = entry.dissolve.saturating_sub(10);
        match entry.age {
            0 => cues.push(Cue::StageTint(crate::StageTint {
                color: [12; 3],
                duration: 150,
            })),
            RESUME_AGE => self.request_pose(
                entry.actor,
                Some(entry.resume),
                crate::Pose {
                    rate: 0.4,
                    ..Default::default()
                },
            ),
            FLASH_AGE => {
                self.summon_entry.flash = Some(Flash {
                    opacity: 0,
                    phase: FlashPhase::Rising,
                })
            }
            _ => {}
        }
        if entry.age == HANDOFF_AGE {
            self.restore_summon_actors(entry.actor, &entry.placements);
            if let Some(action) = entry.parent {
                cues.push(Cue::Completed { action });
            }
            self.release_volley(
                entry.spell,
                entry.actor,
                entry.target,
                SpellSlot::Primary,
                entry.parent,
                cues,
            )?
            .context("summon callback slot is occupied")?;
            self.advance_volleys(cues, false)?;
        } else {
            entry.age += 1;
            self.summon_entry.entry = Some(entry);
        }
        Ok(())
    }

    fn restore_summon_actors(&mut self, caster: ActorId, placements: &[Placement]) {
        for (index, saved) in placements.iter().enumerate() {
            let id = ActorId(index as u8);
            let actor = &mut self.actors[index];
            actor.position = saved.position;
            actor.heading = saved.heading;
            actor.facing_direction = crate::direction_from_heading(saved.heading);
            actor.movement.forward = 0.;
            actor.movement.vertical = 0.;
            actor.movement.acceleration = 0.;
            actor.movement.gravity = crate::movement::GRAVITY;
            actor.movement.airborne_action = false;
            actor.input = Default::default();
            actor.reaction.protection = Default::default();
            if !actor.available() {
                continue;
            }
            if id == caster || actor.side == Side::Enemy {
                if id != caster {
                    self.request_pose(id, saved.resume, crate::Pose::default());
                }
                self.set_task(
                    index,
                    crate::state::ActorTask::Recovering {
                        remaining: if id == caster {
                            CASTER_RECOVERY
                        } else {
                            ENEMY_RECOVERY
                        },
                        after_hit: false,
                    },
                );
            } else {
                // Restoration is not a new idle decision: retain its random state and
                // restart the prepared idle pose without rolling a controller delay.
                actor.movement.locomotion = crate::Locomotion::Idle;
                let injured = i64::from(actor.hp) * 4 < i64::from(actor.equipment.max_hp);
                let motion = self
                    .models
                    .as_ref()
                    .and_then(|models| models.definition(id))
                    .and_then(|model| {
                        model.idle_motions[usize::from(injured)]
                            .or(model.idle_motions[0])
                            .map(|clip| MotionBinding {
                                model: model.resource,
                                clip,
                            })
                    });
                self.request_pose(
                    id,
                    motion,
                    crate::Pose {
                        repeat: true,
                        ..Default::default()
                    },
                );
                self.runtime[index].idle_timer = 0;
                self.set_task(index, crate::state::ActorTask::None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BattleClock, BattleInput, ModelDefinition, Models, Playback,
        tests::{actor, prepared},
    };
    use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
    use std::collections::BTreeMap;

    #[test]
    fn entry_holds_combat_restores_formation_and_releases_once_across_flash_tail() -> Result<()> {
        let mut battle = prepared(
            vec![actor(Side::Party), actor(Side::Enemy), actor(Side::Party)],
            0,
        )
        .finish()?;
        battle.actors[0].position = [-100., 90., 20.];
        battle.actors[1].position = [300., 400., -20.];
        battle.actors[1].movement.flying = true;
        battle.actors[1].movement.hover_height = 180.;
        battle.actors[2].position = [-300., 50., 40.];
        battle.actors[0].overlimit = crate::OverLimit::active(200)?;
        battle.actors[2].overlimit = crate::OverLimit::new(400)?;
        let vitality: Vec<_> = battle
            .actors
            .iter()
            .map(|actor| (actor.hp, actor.tp))
            .collect();
        let model = Arc::new(ModelDefinition {
            tint: [64, 64, 64, 255],
            body_spheres: vec![],
            reception_spheres: None,
            fade_on_defeat: false,
            resource: 7,
            skeleton: Skeleton {
                bones: vec![Bone {
                    name: "root".into(),
                    parent: None,
                    bind_channels: TransformChannels(8),
                    bind: Transform::default(),
                }],
            },
            motions: [0, 3, 12]
                .into_iter()
                .map(|clip| {
                    (
                        clip,
                        Motion {
                            duration_frames: 500.,
                            tracks: vec![],
                        },
                    )
                })
                .collect(),
            secondary_motion: vec![],
            initial: Playback {
                clip: 0,
                frame: 0.,
                rate: 0.5,
                repeat: true,
            },
            reactions: Default::default(),
            idle_motions: [Some(0); 2],
            idle_expression: [0; 4],
            blink: None,
            weapons: vec![],
            shadow: None,
            suppress_root_translation: [false; 3],
        });
        battle.set_models(Models::new(
            &battle.actors,
            vec![Some(model); 3],
            BTreeMap::new(),
            battle.diagnostics.clone(),
        )?);
        let mut spell = crate::tests::volley();
        spell.duration = 500;
        spell.projectiles.truncate(1);
        spell.projectiles[0].at = 400;
        let mut cues = Vec::new();
        assert!(battle.begin_summon_entry(
            SummonEntryDefinition {
                effect: EffectAppearance {
                    resource: 1,
                    member: 36
                },
                sound: None,
            },
            Arc::new(spell),
            ActorId(0),
            ActorId(1),
            None,
            &mut cues
        )?);
        assert!(matches!(cues.first(), Some(Cue::ClearEffects)));
        assert_eq!(battle.actors[0].overlimit.remaining(), 0);
        assert_eq!(battle.actors[2].overlimit.charge(), 400);
        assert!(!battle.command_admission_allowed());
        let birth = battle.snapshot().summon.unwrap();
        assert_eq!(
            (birth.age, birth.hide_hud, birth.dissolve),
            (None, false, 255)
        );
        let mut releases = 0;
        for age in 0..=HANDOFF_AGE {
            let mut frame = battle.step(BattleInput::default())?;
            let clock = frame.clock;
            battle.advance_models(&mut frame, clock, false)?;
            releases += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Released { .. }))
                .count();
            assert_eq!(frame.clock, BattleClock::SummonTransition);
            if age < HANDOFF_AGE {
                let scene = frame.summon.unwrap();
                assert_eq!(scene.age, Some(age));
                assert!(scene.hide_hud);
                assert_eq!(frame.models.iter().filter(|model| model.visible).count(), 1);
                assert_eq!(battle.actors[0].position, [-100., 90., 20.]);
                assert!(battle.volleys.is_empty());
            }
            if age == RESUME_AGE {
                assert_eq!(
                    battle
                        .models()
                        .unwrap()
                        .main_motion_observation(ActorId(0))
                        .unwrap()
                        .clip,
                    12
                );
            }
            if age == FLASH_AGE {
                let before = frame.summon;
                let paused = battle.step(BattleInput {
                    paused: true,
                    ..Default::default()
                })?;
                assert_eq!(paused.summon, before);
            }
        }
        assert_eq!(releases, 1);
        assert_eq!(
            battle
                .actors
                .iter()
                .map(|actor| (actor.hp, actor.tp))
                .collect::<Vec<_>>(),
            vitality
        );
        assert_eq!(battle.actors[0].position, [-100., 0.1, 20.]);
        assert_eq!(battle.actors[1].position, [300., 180., -20.]);
        assert_eq!(battle.actors[2].position, [-300., 0.1, 40.]);
        assert!(matches!(
            battle.runtime[0].task(),
            crate::state::ActorTask::Recovering {
                remaining: CASTER_RECOVERY,
                after_hit: false,
            }
        ));
        assert!(matches!(
            battle.runtime[1].task(),
            crate::state::ActorTask::Recovering {
                remaining: ENEMY_RECOVERY,
                after_hit: false,
            }
        ));
        assert_eq!(battle.activity(ActorId(2)), crate::Activity::Idle);
        let handed_off = battle.snapshot().summon.unwrap();
        assert_eq!(
            (handed_off.owner, handed_off.hide_hud, handed_off.flash),
            (None, false, 248)
        );
        for age in 152..=202 {
            let frame = battle.step(BattleInput::default())?;
            assert!(
                !frame
                    .cues
                    .iter()
                    .any(|cue| matches!(cue, Cue::Released { .. }))
            );
            if age == 152 {
                assert_eq!(frame.summon.unwrap().flash, 255);
            }
            if age == 201 {
                assert_eq!(frame.summon.unwrap().flash, 0);
            }
            if age == 202 {
                assert!(frame.summon.is_none());
            }
        }
        battle.invalidate();
        assert!(battle.snapshot().summon.is_none());
        Ok(())
    }
}
