//! Finisher choreography owns its clock, target and camera until the recovery handoff.
use super::*;
use crate::{ActionId, Activity, AttackPose, BattleClock, CameraPose};
use std::sync::Arc;

/// Performance identity is shared by choreography, camera and world effects.
/// In particular a caster never inherits a weapon user's approach or ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalStyle {
    TwinBlades,
    Chakram,
    Arcane,
    Sacred,
    Seals,
    Spellblade,
    HeavyAxe,
    Martial,
    Seraph,
}

impl FatalStyle {
    pub fn caster(self) -> bool {
        matches!(self, Self::Arcane | Self::Sacred)
    }

    pub fn ranged(self) -> bool {
        self.caster() || self == Self::Chakram
    }
}

#[derive(Debug, Clone)]
pub struct FatalPerformance {
    pub style: FatalStyle,
    /// Prepared body joint from which the cast releases; never a HUD anchor.
    pub emitter: Option<u16>,
    pub poses: Vec<(u16, AttackPose)>,
    /// Per-update retiming keeps anticipation, strike and follow-through in one clip.
    pub rates: Vec<(u16, f32)>,
    pub impact_at: u16,
    pub duration: u16,
    pub reach: f32,
    /// Authored contact in model space. Admission aligns the approach with the
    /// victim's body while preserving its ground/air plane.
    pub contact: Option<[f32; 3]>,
    /// Highest authored body/equipment point over the full performance.
    pub visual_height: Option<f32>,
    pub voice: Option<crate::Sound>,
    pub charge_sound: Option<crate::Sound>,
    pub impact_kind: crate::DamageKind,
    pub impact_sound: Option<crate::Sound>,
    pub impact_effect: Option<crate::EffectAppearance>,
}

impl FatalPerformance {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.impact_at >= 24 && self.duration > self.impact_at + 12 && self.duration <= 240,
            "invalid Fatal Strike performance timing"
        );
        ensure!(
            self.reach.is_finite() && (20. ..=300.).contains(&self.reach),
            "invalid Fatal Strike reach"
        );
        ensure!(
            self.contact
                .is_none_or(|point| point.iter().all(|v| v.is_finite())
                    && point.iter().all(|v| v.abs() <= 1000.)),
            "invalid Fatal Strike contact"
        );
        ensure!(
            self.visual_height
                .is_none_or(|height| height.is_finite() && (1. ..=2000.).contains(&height)),
            "invalid Fatal Strike visual bounds"
        );
        ensure!(
            self.poses.windows(2).all(|pair| pair[0].0 < pair[1].0)
                && self.poses.iter().all(|(at, pose)| *at < self.duration
                    && pose.start.is_finite()
                    && pose.start >= 0.
                    && pose.rate.is_finite()
                    && (pose.rate == 0. || (0.01..=4.).contains(&pose.rate))
                    && pose
                        .end
                        .is_none_or(|end| end.is_finite() && end > 0. && end >= pose.start)),
            "invalid Fatal Strike pose track"
        );
        ensure!(
            self.rates.windows(2).all(|pair| pair[0].0 < pair[1].0)
                && self.rates.iter().all(|(at, rate)| *at < self.duration
                    && rate.is_finite()
                    && (*rate == 0. || (0.01..=4.).contains(rate))),
            "invalid Fatal Strike retiming track"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FatalSceneFrame {
    pub performer: ActorId,
    pub target: ActorId,
    pub color: FatalColor,
    pub style: FatalStyle,
    pub emitter: Option<u16>,
    pub contact: Option<[f32; 3]>,
    pub chain: u8,
    pub age: u16,
    pub impact_at: u16,
    pub duration: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatalSuccess {
    pub performer: ActorId,
    pub target: ActorId,
    pub color: FatalColor,
    pub chain: u8,
    pub defeated: bool,
    pub damage: i32,
}

#[derive(Debug)]
pub(crate) struct Scene {
    pub(super) action: ActionId,
    pub(crate) performer: ActorId,
    target: ActorId,
    opening: FatalOpening,
    performance: Arc<FatalPerformance>,
    age: u16,
    origin: [f32; 3],
    destination: [f32; 3],
    direction: [f32; 3],
    original_camera: CameraPose,
    pub(super) camera: CameraPose,
    pub(super) fov: f32,
    impacted: bool,
    equipment: Vec<(crate::WeaponSlot, bool)>,
}

impl Scene {
    pub(super) fn frame(&self) -> FatalSceneFrame {
        FatalSceneFrame {
            performer: self.performer,
            target: self.target,
            color: self.opening.color,
            style: self.performance.style,
            emitter: self.performance.emitter,
            contact: self.performance.contact,
            chain: self.opening.chain,
            age: self.age.saturating_sub(1),
            impact_at: self.performance.impact_at,
            duration: self.performance.duration,
        }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (t * (t * 6. - 15.) + 10.)
}
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn spline(points: &[(f32, [f32; 3])], time: f32) -> [f32; 3] {
    let index = points
        .windows(2)
        .position(|pair| time <= pair[1].0)
        .unwrap_or(points.len() - 2);
    let (ta, a) = points[index];
    let (tb, b) = points[index + 1];
    let t = ((time - ta) / (tb - ta)).clamp(0., 1.);
    std::array::from_fn(|axis| {
        // Limit tangents using both adjacent secants. The previous Catmull
        // curve could fly through the face after a long establishing move.
        // One tangent per key preserves C1 continuity, including uneven timing.
        let tangent = |key: usize| {
            if key == 0 || key + 1 == points.len() {
                return 0.;
            }
            let (before, here, after) = (points[key - 1], points[key], points[key + 1]);
            let incoming = (here.1[axis] - before.1[axis]) / (here.0 - before.0);
            let outgoing = (after.1[axis] - here.1[axis]) / (after.0 - here.0);
            if incoming * outgoing <= 0. {
                0.
            } else {
                incoming.signum() * incoming.abs().min(outgoing.abs())
            }
        };
        let ma = tangent(index) * (tb - ta);
        let mb = tangent(index + 1) * (tb - ta);
        (2. * t.powi(3) - 3. * t * t + 1.) * a[axis]
            + (t.powi(3) - 2. * t * t + t) * ma
            + (-2. * t.powi(3) + 3. * t * t) * b[axis]
            + (t.powi(3) - t * t) * mb
    })
}

fn camera(eye: [f32; 3], focus: [f32; 3]) -> CameraPose {
    let delta: [f32; 3] = std::array::from_fn(|i| eye[i] - focus[i]);
    let horizontal = delta[0].hypot(delta[2]);
    CameraPose {
        eye,
        focus,
        radius: horizontal.hypot(delta[1]),
        yaw: delta[2].atan2(delta[0]).to_degrees(),
        pitch: delta[1].atan2(horizontal).to_degrees(),
    }
}

fn cinematic_time(age: u16, impact: u16, duration: u16) -> f32 {
    if age <= impact {
        f32::from(age) * 56. / f32::from(impact)
    } else {
        56. + f32::from(age - impact) * 56. / f32::from(duration - impact)
    }
}

/// Continuous shots retain the same side of the action axis. Bounds include
/// both bodies; a large target opens the lens rather than cropping its head.
fn cinematic_camera(
    scene: &Scene,
    hero: &crate::Actor,
    target: &crate::Actor,
) -> (CameraPose, f32) {
    let t = cinematic_time(
        scene.age,
        scene.performance.impact_at,
        scene.performance.duration,
    );
    let height = |actor: &crate::Actor| (actor.body_top() - actor.position[1]).max(90.);
    let center = |actor: &crate::Actor| {
        [
            actor.position[0],
            actor.position[1] + height(actor) * 0.53,
            actor.position[2],
        ]
    };
    let hero_center = center(hero);
    let victim_center = center(target);
    let pair = mix(hero_center, victim_center, 0.5);
    let gap = (hero_center[0] - victim_center[0]).hypot(hero_center[2] - victim_center[2]);
    let cotangent = 1. / 14_f32.to_radians().tan();
    let hero_extent = scene
        .performance
        .visual_height
        .map_or(height(hero) * 0.65, |top| {
            ((top * hero.body.scale - height(hero) * 0.53) * 1.55).max(height(hero) * 0.65)
        });
    let radius = ((hero_extent.max(height(target) * 0.65)
        + (hero_center[1] - victim_center[1]).abs() * 0.5)
        * cotangent)
        .max(gap * 0.58 * cotangent)
        .clamp(340., 3000.);
    let side = [-scene.direction[2], 0., scene.direction[0]];
    let orbit = if scene.opening.color == FatalColor::Red {
        -1.
    } else {
        1.
    };
    let shot = |focus: [f32; 3], distance: f32, angle: f32, elevation: f32| {
        let (sin, cos) = angle.sin_cos();
        let mut eye = std::array::from_fn(|i| {
            focus[i] + (side[i] * cos * orbit + scene.direction[i] * sin) * distance
        });
        eye[1] += elevation;
        eye[1] = eye[1].max(18.);
        camera(eye, focus)
    };
    let close_focus = [
        hero_center[0],
        hero.position[1] + height(hero) * 0.72,
        hero_center[2],
    ];
    let elevation = match scene.opening.color {
        FatalColor::Blue => 12.,
        FatalColor::Red => radius * 0.32,
        FatalColor::Green => radius * 0.1,
    };
    let shots = [
        (0., scene.original_camera),
        (12., shot(close_focus, height(hero) * 3.8, 0.38, 10.)),
        (22., shot(close_focus, height(hero) * 3.65, 0.32, 8.)),
        (38., shot(pair, radius * 1.08, 0.12, elevation)),
        (50., shot(pair, radius, 0.02, elevation * 0.7)),
        (56., shot(pair, radius * 0.86, -0.02, elevation * 0.55)),
        (62., shot(pair, radius * 0.88, -0.025, elevation * 0.55)),
        (92., shot(pair, radius * 1.22, -0.2, elevation * 0.8)),
        (112., scene.original_camera),
    ];
    let eye = spline(&shots.map(|(time, pose)| (time, pose.eye)), t);
    let focus = spline(&shots.map(|(time, pose)| (time, pose.focus)), t);
    let pose = camera([eye[0], eye[1].max(18.), eye[2]], focus);
    let lens = smooth(t / 12.) * (1. - smooth((t - 92.) / 20.));
    let punch = smooth((t - 52.) / 4.) * (1. - smooth((t - 56.) / 9.));
    (
        pose,
        crate::camera::VERTICAL_FOV_DEGREES + (28. - crate::camera::VERTICAL_FOV_DEGREES) * lens
            - punch * 1.8,
    )
}

impl Battle {
    /// Inspect the prepared performance without mutating the cinematic clock.
    pub fn fatal_performance(
        &self,
        actor: ActorId,
        color: FatalColor,
    ) -> Option<&FatalPerformance> {
        self.fatal_strikes
            .performances
            .get(&actor)
            .map(|performances| performances[color.index()].as_ref())
    }

    /// Install original choreography against the prepared character's own rig.
    pub fn configure_fatal_performances(
        &mut self,
        actor: ActorId,
        performances: [FatalPerformance; 3],
    ) -> Result<()> {
        ensure!(
            self.actor(actor)?.side == Side::Party,
            "Fatal Strike performer must be a party member"
        );
        for performance in &performances {
            performance.validate()?;
            if let Some(model) = self
                .models
                .as_ref()
                .and_then(|models| models.definition(actor))
            {
                for (_, pose) in &performance.poses {
                    ensure!(
                        pose.motion.model == model.resource
                            && model.motions.contains_key(&pose.motion.clip),
                        "Fatal Strike pose is not prepared for its actor"
                    );
                }
                ensure!(
                    (!performance.style.caster() || performance.emitter.is_some())
                        && performance
                            .emitter
                            .is_none_or(|bone| usize::from(bone) < model.skeleton.bones.len()),
                    "Fatal Strike emitter is not on its actor rig"
                );
            }
        }
        self.fatal_strikes
            .performances
            .insert(actor, performances.map(Arc::new));
        Ok(())
    }

    /// Validate and exclusively claim the currently selected opening.
    pub fn request_fatal_strike(&mut self, performer: ActorId) -> Result<bool> {
        self.actor(performer)?;
        if self.is_paused()
            || !self.command_admission_allowed()
            || !self.fatal_strikes.enabled
            || self.fatal_strikes.scene.is_some()
            || self.actors[performer.index()].side != Side::Party
            || !self.actors[performer.index()].available()
            || self.actors[performer.index()].time_stop != 0
            || !matches!(
                self.activity(performer),
                Activity::Idle
                    | Activity::Approaching
                    | Activity::Action
                    | Activity::Recovering
                    | Activity::Guarding
                    | Activity::Taunting
            )
            || self.is_captured(performer)
        {
            return Ok(false);
        }
        let Some(target) = self.target(performer) else {
            return Ok(false);
        };
        if !self.actors[target.index()].available()
            || self.actors[target.index()].time_stop != 0
            || self.prepared.actor_setup[target.index()].target_excluded
            || self.is_captured(target)
        {
            return Ok(false);
        }
        let Some(opening) = self
            .fatal_strikes
            .gauges
            .get(target.index())
            .and_then(Option::as_ref)
            .and_then(|gauge| gauge.opening)
        else {
            return Ok(false);
        };
        let Some(performance) = self
            .fatal_strikes
            .performances
            .get(&performer)
            .map(|performances| Arc::clone(&performances[opening.color.index()]))
        else {
            return Ok(false);
        };
        let origin = self.actors[performer.index()].position;
        let victim = &self.actors[target.index()];
        let maximum = if performance.style.ranged() {
            2200.
        } else {
            900.
        };
        if (origin[0] - victim.position[0]).hypot(origin[2] - victim.position[2]) > maximum {
            return Ok(false);
        }
        let direction = crate::distance::planar_direction(victim.position, origin, [1., 0., 0.]);
        let radius = victim
            .body
            .collider
            .map_or(40., |body| body.radius * victim.body.scale);
        let destination = if performance.style.ranged() {
            [
                origin[0],
                if self.actors[performer.index()].movement.flying {
                    self.actors[performer.index()].movement.hover_height
                } else {
                    0.
                },
                origin[2],
            ]
        } else if let Some([x, y, _]) = performance.contact {
            let scale = self.actors[performer.index()].body.scale;
            let center = victim.target_center();
            [
                center[0] - (direction[2] * x - direction[0] * y) * scale,
                victim.position[1].max(0.),
                center[2] - (-direction[0] * x - direction[2] * y) * scale,
            ]
        } else {
            [
                victim.position[0] - direction[0] * (performance.reach + radius),
                victim.position[1].max(0.),
                victim.position[2] - direction[2] * (performance.reach + radius),
            ]
        };
        let focus = victim.effect_origin();
        let original_camera = self
            .camera
            .as_ref()
            .map(|camera| camera.frame())
            .unwrap_or_else(|| camera([focus[0], focus[1] + 200., focus[2] + 1800.], focus));
        let action = ActionId(self.next_action);
        self.next_action += 1;
        let mut cues = Vec::new();
        for actor in [performer, target] {
            self.interrupt_actor(actor, &mut cues);
            self.enter_idle(actor);
            self.actors[actor.index()].hit_stop = 0;
            self.actors[actor.index()].guard.active = false;
            self.actors[actor.index()].movement.forward = 0.;
            self.actors[actor.index()].movement.vertical = 0.;
        }
        // Catch only this performer's detached weapons. Other released attacks
        // retain their owners and resume after the cinematic.
        for slot in crate::WeaponSlot::PRIMARY {
            self.retire_weapon_flight(performer, slot);
        }
        self.actors[performer.index()].facing_direction = direction;
        self.actors[performer.index()].heading = direction[0].atan2(direction[2]).to_degrees();
        let mut equipment = if performance.style == FatalStyle::Arcane {
            self.models
                .as_ref()
                .map(|models| models.primary_weapon_visibility(performer))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        // Cancellation may already have queued restoration of an attack's
        // temporary weapon state. Save that resolved state, not the old draw.
        for request in &self.model_requests {
            match request {
                crate::ModelRequest::PrimaryWeapons { actor, visible } if *actor == performer => {
                    for (_, saved) in &mut equipment {
                        *saved = *visible;
                    }
                }
                crate::ModelRequest::Weapon {
                    actor,
                    slot: Some(slot),
                    visible,
                } if *actor == performer => {
                    if let Some((_, saved)) = equipment.iter_mut().find(|(saved, _)| saved == slot)
                    {
                        *saved = *visible;
                    }
                }
                _ => {}
            }
        }
        self.fatal_strikes.gauges[target.index()]
            .as_mut()
            .unwrap()
            .opening = None;
        self.fatal_strikes.scene = Some(Scene {
            action,
            performer,
            target,
            opening,
            performance,
            age: 0,
            origin,
            destination,
            direction,
            original_camera,
            camera: original_camera,
            fov: crate::camera::VERTICAL_FOV_DEGREES,
            impacted: false,
            equipment,
        });
        cues.push(Cue::Started {
            definition: None,
            action,
            actor: performer,
        });
        self.pending_cues.extend(cues);
        Ok(true)
    }

    pub(crate) fn advance_fatal_scene(&mut self, cues: &mut Vec<Cue>) -> Result<()> {
        let Some(mut scene) = self.fatal_strikes.scene.take() else {
            return Ok(());
        };
        let actor = scene.performer;
        let target = scene.target;
        let performance = &scene.performance;
        self.clock = BattleClock::FatalStrike(actor);
        if !self.actors[actor.index()].available()
            || (!scene.impacted && !self.actors[target.index()].available())
        {
            self.retire_fatal_scene(scene, false, cues);
            return Ok(());
        }
        let age = scene.age;
        if age == 0 {
            self.model_requests
                .push(crate::ModelRequest::ActiveExpression { actor });
            for &(slot, _) in &scene.equipment {
                self.model_requests.push(crate::ModelRequest::Weapon {
                    actor,
                    slot: Some(slot),
                    visible: false,
                });
            }
            cues.push(Cue::StageTint(crate::StageTint {
                color: [30, 35, 52],
                duration: performance.duration,
            }));
        }
        if age == 12
            && let Some(sound) = performance.charge_sound
        {
            cues.push(Cue::Sound {
                actor,
                sound,
                position: scene.origin,
                priority: 1,
            });
        }
        if age == 32 {
            cues.push(Cue::ActorPulse {
                actor,
                color: scene.opening.color.rgb().map(|value| value / 2),
            });
            if let Some(voice) = performance.voice {
                self.request_voice(actor, voice, crate::VoicePriority::Announcement);
            }
        }
        for (_, pose) in performance.poses.iter().filter(|(at, _)| *at == age) {
            self.request_pose(actor, Some(pose.motion), pose.playback());
        }
        for (_, rate) in performance.rates.iter().filter(|(at, _)| *at == age) {
            self.model_requests
                .push(crate::ModelRequest::AnimationRate { actor, rate: *rate });
        }
        let time = cinematic_time(age, performance.impact_at, performance.duration);
        let approach = smooth((time - 36.) / 18.);
        let mut position = mix(scene.origin, scene.destination, approach);
        let ground = if self.actors[actor.index()].movement.flying {
            self.actors[actor.index()].movement.hover_height
        } else {
            0.
        };
        position[1] = ground
            + (scene.origin[1] - ground) * (1. - smooth(time / 18.))
            + (scene.destination[1] - ground) * approach * (1. - smooth((time - 78.) / 30.));
        let leap = ((time - 28.) / 28.).clamp(0., 1.);
        position[1] += if performance.style.ranged() {
            0.
        } else {
            let height = match (performance.style, scene.opening.color) {
                (_, FatalColor::Green) => 0.,
                (FatalStyle::Martial, FatalColor::Red) => 90.,
                (FatalStyle::Martial, FatalColor::Blue) => 28.,
                _ => 0.,
            };
            (leap * std::f32::consts::PI).sin().powi(2) * height
        };
        self.actors[actor.index()].position = position;
        self.constrain_actor(actor.index(), None);
        if age == performance.impact_at {
            scene.impacted = true;
            let gauge = self.fatal_strikes.gauges[target.index()].as_ref().unwrap();
            let amount =
                if gauge.rules.outcome == FatalOutcome::Defeat && self.event_boss != Some(target) {
                    self.actors[target.index()].hp
                } else {
                    let base = i64::from(self.actors[target.index()].equipment.max_hp) / 10
                        + i64::from(self.actors[actor.index()].equipment.stats.slash.max(1)) * 3;
                    (base * (100 + i64::from(scene.opening.chain.saturating_sub(1)) * 15) / 100)
                        .clamp(1, i64::from(i32::MAX)) as i32
                };
            let before = self.actors[target.index()].hp;
            crate::contact::fatal_impact(
                self,
                actor,
                target,
                scene.action,
                amount,
                performance.impact_kind,
                cues,
            )?;
            self.restore_event_boss_hp(target);
            let damage = (before - self.actors[target.index()].hp).max(0);
            let success = FatalSuccess {
                performer: actor,
                target,
                color: scene.opening.color,
                chain: scene.opening.chain,
                defeated: self.actors[target.index()].hp == 0,
                damage,
            };
            self.ledger.fatal_strikes.push(success);
            if damage > 0 {
                self.ledger.adjust(20 + i64::from(scene.opening.chain) * 5);
            }
            cues.push(Cue::FatalImpact(success));
            cues.push(Cue::Shake {
                duration: if performance.style == FatalStyle::HeavyAxe {
                    14
                } else {
                    9
                },
                amplitude: match performance.style {
                    FatalStyle::HeavyAxe => 14,
                    FatalStyle::Martial => 10,
                    FatalStyle::Chakram | FatalStyle::Seals => 6,
                    _ => 9,
                },
            });
            if let Some(appearance) = performance.impact_effect {
                cues.push(Cue::Effect(crate::EffectRequest {
                    owner: actor,
                    target,
                    appearance,
                    origin: self.actors[target.index()].effect_origin(),
                    heading: self.actors[actor.index()].heading,
                    follow: None,
                    scale: 1.7,
                    tint: Default::default(),
                }));
            }
            if let Some(gauge) = &mut self.fatal_strikes.gauges[target.index()] {
                *gauge = Gauge::new(gauge.rules);
            }
        }
        if !performance.style.caster() && age == performance.impact_at.saturating_sub(12) {
            let slots: Vec<_> = self
                .models
                .as_ref()
                .and_then(|models| models.definition(actor))
                .map(|model| {
                    model
                        .weapons
                        .iter()
                        .filter_map(|weapon| weapon.slot)
                        .filter(|slot| slot.is_primary())
                        .collect()
                })
                .unwrap_or_else(|| vec![crate::WeaponSlot(0)]);
            for slot in slots {
                cues.push(Cue::WeaponTrail {
                    actor,
                    slot,
                    duration: 24,
                    highlighted: true,
                });
            }
        }
        if age
            == performance
                .impact_at
                .saturating_sub(if performance.style.caster() {
                    0
                } else if performance.style == FatalStyle::Chakram {
                    12
                } else {
                    8
                })
            && let Some(sound) = performance.impact_sound
        {
            cues.push(Cue::Sound {
                actor,
                sound,
                position: if performance.style.ranged() {
                    self.actors[target.index()].effect_origin()
                } else {
                    position
                },
                priority: 1,
            });
        }
        self.paused_actors = self
            .actor_ids()
            .filter(|id| *id != actor && (*id != target || age < performance.impact_at + 4))
            .collect();
        (scene.camera, scene.fov) = cinematic_camera(
            &scene,
            &self.actors[actor.index()],
            &self.actors[target.index()],
        );
        scene.age += 1;
        if scene.age > performance.duration {
            self.retire_fatal_scene(scene, true, cues);
        } else {
            self.fatal_strikes.scene = Some(scene);
        }
        Ok(())
    }

    pub(super) fn retire_fatal_scene(
        &mut self,
        scene: Scene,
        completed: bool,
        cues: &mut Vec<Cue>,
    ) {
        for id in [scene.performer, scene.target] {
            if self.actors[id.index()].available() {
                self.enter_idle(id);
            }
        }
        let actor = &mut self.actors[scene.performer.index()];
        actor.position[1] = if actor.movement.flying {
            actor.movement.hover_height
        } else {
            0.
        };
        actor.movement.vertical = 0.;
        self.paused_actors.clear();
        for (slot, visible) in scene.equipment {
            self.model_requests.push(crate::ModelRequest::Weapon {
                actor: scene.performer,
                slot: Some(slot),
                visible,
            });
        }
        cues.push(if completed {
            Cue::Completed {
                action: scene.action,
            }
        } else {
            Cue::Interrupted {
                action: scene.action,
            }
        });
        cues.push(Cue::StageTint(crate::StageTint {
            color: [255; 3],
            duration: 0,
        }));
    }
}

#[cfg(test)]
mod camera_tests {
    use super::*;

    #[test]
    fn close_up_spline_stays_between_shots_and_keeps_continuous_velocity() {
        let points = [
            (0., [0., 450., 2000.]),
            (12., [20., 85., 280.]),
            (22., [22., 83., 265.]),
            (38., [100., 120., 500.]),
            (56., [110., 125., 450.]),
        ];
        for pair in points.windows(2) {
            for step in 0..=100 {
                let time = pair[0].0 + (pair[1].0 - pair[0].0) * step as f32 / 100.;
                let point = spline(&points, time);
                for axis in 0..3 {
                    assert!(point[axis] >= pair[0].1[axis].min(pair[1].1[axis]) - 0.001);
                    assert!(point[axis] <= pair[0].1[axis].max(pair[1].1[axis]) + 0.001);
                }
            }
        }
        for (time, _) in points.iter().skip(1).take(points.len() - 2) {
            let h = 0.001;
            let left = glam::Vec3::from_array(spline(&points, *time - h));
            let at = glam::Vec3::from_array(spline(&points, *time));
            let right = glam::Vec3::from_array(spline(&points, *time + h));
            assert!(((at - left) / h - (right - at) / h).length() < 0.25);
        }
    }
}

impl Battle {
    /// Companions leave the human players' selected openings available to them.
    pub(crate) fn admit_companion_fatal_strike(&mut self) -> Result<()> {
        if !self.fatal_strikes.enabled || !self.command_admission_allowed() {
            return Ok(());
        }
        let candidates: Vec<_> = self
            .actors
            .iter()
            .enumerate()
            .filter_map(|(index, actor)| {
                if actor.side != Side::Party || actor.control != crate::Control::Auto {
                    return None;
                }
                let id = ActorId(index as u8);
                let target = self.target(id)?;
                let opening = self
                    .fatal_strikes
                    .gauges
                    .get(target.index())?
                    .as_ref()?
                    .opening?;
                if opening.remaining > 45 {
                    return None;
                }
                let human_target = self.actors.iter().enumerate().any(|(other_index, other)| {
                    other.side == Side::Party
                        && other.available()
                        && matches!(
                            other.control,
                            crate::Control::Manual | crate::Control::SemiAuto
                        )
                        && self.target(ActorId(other_index as u8)) == Some(target)
                });
                (!human_target).then_some(id)
            })
            .collect();
        for actor in candidates {
            if self.request_fatal_strike(actor)? {
                break;
            }
        }
        Ok(())
    }
}
