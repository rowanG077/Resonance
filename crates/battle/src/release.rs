//! Released volleys retain their origin; projectiles own their flight.
use crate::{
    ActionId, ActorId, Battle, Cue, EffectAppearance, ProjectileDefinition, Sound, SpellSlot,
};
use anyhow::{Result, ensure};
use std::sync::Arc;

const REVEALED_NOTICE_TICKS: u16 = 90;

#[derive(Debug, Clone)]
pub struct PreparedSpell {
    pub summon: Option<Arc<crate::PreparedSummon>>,
    pub policy: SpellPolicy,
    /// Projectile births, measured in scene ticks from release.
    pub projectiles: Vec<SpellProjectile>,
    pub effects: Vec<SpellEffect>,
    pub recovery: Vec<SpellRecovery>,
    pub origin: SpellOrigin,
    /// Opening scene transition holds gameplay while its framing and feedback advance.
    pub transition_ticks: u16,
    /// Starts when the stored scene initializes after its opening transition.
    pub stage_tint: Option<StageTint>,
    pub camera: Option<SpellCamera>,
    /// Retain the spell slot until this tick, including artwork after its last hit.
    pub duration: u16,
    pub startup: Option<(u16, SpellStartup)>,
    pub sound: Option<Sound>,
}

/// A contact's placement is relative to the retained scene origin and heading.
#[derive(Debug, Clone)]
pub struct SpellProjectile {
    pub at: u16,
    pub definition: Arc<ProjectileDefinition>,
    pub origin_offset: [f32; 3],
    pub launch: SpellLaunch,
}

impl SpellProjectile {
    pub fn new(at: u16, definition: Arc<ProjectileDefinition>) -> Self {
        Self {
            at,
            definition,
            origin_offset: [0.; 3],
            launch: SpellLaunch::Live,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum SpellLaunch {
    #[default]
    Live,
    Captured(f32),
    /// Aim at the retained origin's XZ and this absolute height.
    AtHeight(f32),
    /// Sample the caster's root and body height independently for each shot.
    CasterAbove {
        center_height_scale: f32,
        height_offset: f32,
    },
    /// Retain the allocation visit's aim for initialization on the following visit.
    ForwardCone {
        minimum_dot: f32,
        vertical_limit: f32,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub enum SpellPolicy {
    #[default]
    Static,
    /// A scene starts a caster gesture on its independent release clock.
    CasterPose {
        at: u16,
        pose: Option<crate::AttackPose>,
        lift: Option<SpellLift>,
    },
    /// Carry a benefit upward from the caster, then down onto the moving recipient.
    TransferRecovery {
        appearance: EffectAppearance,
        apex: f32,
        speed: f32,
        benefit: SpellBenefit,
    },
    /// Move the retained anchor and its contacts toward the target during this interval.
    TrackGround { start: u16, end: u16, speed: f32 },
    /// Select once by roster order; later contacts keep that same recipient.
    CaptureRecipient { at: u16, radius: f32 },
    /// Party portrait, optional technique reveal, and their presentation hold.
    Cutin {
        at: u16,
        catalogue: u16,
        duration: u16,
        hold_ticks: u16,
        /// Replace a concealed technique caption on the spell's existing clock.
        reveal: Option<(u16, &'static str)>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SpellLift {
    pub height: f32,
    pub rise_until: u16,
    pub fall_at: u16,
}

fn cone_direction(
    origin: [f32; 3],
    target: [f32; 3],
    heading: f32,
    minimum_dot: f32,
    vertical_limit: f32,
) -> [f32; 3] {
    let (sin, cos) = heading.to_radians().sin_cos();
    let ahead = [sin, 0., cos];
    let delta = std::array::from_fn::<_, 3, _>(|axis| target[axis] - origin[axis]);
    let horizontal = delta[0].hypot(delta[2]);
    if horizontal < 0.5 {
        return ahead;
    }
    let x = delta[0] / horizontal;
    let z = delta[2] / horizontal;
    if sin * x + cos * z <= minimum_dot {
        return ahead;
    }
    [
        x,
        (delta[1] / horizontal.hypot(delta[1])).clamp(-vertical_limit, vertical_limit),
        z,
    ]
}

#[derive(Debug, Clone, Copy)]
pub struct SpellEffect {
    pub at: u16,
    pub appearance: EffectAppearance,
    pub placement: SpellEffectPlacement,
    /// Start the program now and initialize its particles on the next update.
    pub deferred: bool,
    /// Scene placement: None uses world heading zero; Some adds to the retained heading.
    pub captured_heading_offset: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub enum SpellEffectPlacement {
    /// Offset rotated by the retained scene heading.
    Scene([f32; 3]),
    /// Capture the caster's ground position and heading when the effect begins.
    CasterGround,
    CasterCenter,
    EnemyCenters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageTint {
    pub color: [u8; 3],
    pub duration: u16,
}

/// A scene's framing outlives its opening close-up and has its own camera clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpellCamera {
    pub bounds: resonance_content::battle_scene::CameraBounds,
    pub duration: u16,
}

#[derive(Debug, Clone, Copy)]
pub enum SpellStartup {
    Effect(EffectAppearance),
    /// Create particles after this update's particle visit.
    DeferredEffect(EffectAppearance),
    CasterGroundEffect(EffectAppearance),
    AllyRing {
        effects: [EffectAppearance; 4],
        first_heading: f32,
        heading_step: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecoveryTargets {
    Target,
    Caster,
    LivingAllies,
    LivingOpponents,
    Area { radius: f32, inclusive: bool },
}

#[derive(Debug, Clone, Copy, Default)]
pub enum RecoveryEffectPlacement {
    /// Recipient-owned center effect, scaled to the recipient. Group pulses use its heading.
    #[default]
    RecipientCenter,
    /// Recipient-owned root effect, with the recipient's heading and unit scale.
    RecipientRoot,
    /// Caster-owned recipient-center effect, with the caster's heading and unit scale.
    CasterFacingCenter,
    /// Caster-owned recipient-center effect, with the recipient's heading and scale.
    RecipientFacingCenter,
    /// Caster-owned recipient-center effect, with the recipient's heading and unit scale.
    RecipientFacingCenterUnscaled,
}

#[derive(Debug, Clone, Copy)]
pub struct SpellRecovery {
    pub at: u16,
    /// Number of consecutive updates during which recipients are reconsidered.
    pub duration: u16,
    pub benefit: SpellBenefit,
    pub targets: RecoveryTargets,
    pub effect: Option<(EffectAppearance, crate::effect::EffectTint)>,
    pub effect_placement: RecoveryEffectPlacement,
    pub flash: Option<[u8; 3]>,
}

/// A timed benefit uses the same recipient and release clock as healing spells.
#[derive(Debug, Clone, Copy)]
pub enum SpellBenefit {
    Hp(u8),
    /// Each full horizontal distance step reduces the recovery percentage.
    HpByDistance {
        percent: u8,
        distance_step: u16,
        penalty_per_step: u8,
        maximum_penalty: u8,
    },
    Tp(u16),
    Cure(crate::conditions::Cure),
    Buff(crate::conditions::Buff),
    Resurrect(u8),
    Sacrifice(EffectAppearance),
    Stat {
        condition: crate::conditions::Condition,
        percent: u8,
        duration: u16,
    },
    DefenseDown {
        /// Retain an existing stronger reduction; positive input cannot raise defense.
        amount: i16,
        /// Zero selects the shared condition duration.
        duration: u16,
        tint: [u8; 3],
    },
}

impl SpellBenefit {
    fn valid(self) -> bool {
        match self {
            Self::Hp(percent) => percent > 0,
            Self::HpByDistance {
                percent,
                distance_step,
                penalty_per_step,
                maximum_penalty,
            } => percent > 0 && distance_step > 0 && penalty_per_step > 0 && maximum_penalty > 0,
            Self::Tp(amount) => amount > 0,
            Self::Cure(_) | Self::Buff(_) | Self::Sacrifice(_) | Self::DefenseDown { .. } => true,
            Self::Resurrect(percent) => percent <= 100,
            Self::Stat {
                condition,
                percent,
                duration,
            } => {
                matches!(
                    condition,
                    crate::conditions::Condition::AttackUp
                        | crate::conditions::Condition::DefenseUp
                ) && percent > 0
                    && duration > 0
            }
        }
    }

    pub(crate) fn accepts(self, actor: &crate::Actor) -> bool {
        if matches!(self, Self::Resurrect(_)) {
            return actor.hp <= 0 && actor.availability == crate::ActorAvailability::Dead;
        }
        actor.available()
            || (actor.hp > 0
                && actor.is_petrified()
                && matches!(self, Self::Cure(cure) if cure.thaws_petrify()))
    }

    fn apply(
        self,
        battle: &mut Battle,
        caster: ActorId,
        target: ActorId,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        match self {
            Self::Hp(percent) => battle.recover(target, i16::from(percent), cues)?,
            Self::HpByDistance {
                percent,
                distance_step,
                penalty_per_step,
                maximum_penalty,
            } => {
                let origin = battle.actors[caster.index()].position;
                let point = battle.actors[target.index()].position;
                let distance = (point[0] - origin[0]).hypot(point[2] - origin[2]);
                let steps = (distance / f32::from(distance_step)) as u32;
                let penalty = steps
                    .saturating_mul(u32::from(penalty_per_step))
                    .min(u32::from(maximum_penalty)) as u8;
                battle.recover(target, i16::from(percent.saturating_sub(penalty)), cues)?;
            }
            Self::Tp(amount) => {
                let actor = &mut battle.actors[target.index()];
                let previous = actor.tp;
                actor.recover_flat_tp(i32::from(amount));
                cues.push(Cue::Recovered {
                    floating: true,
                    actor: target,
                    kind: crate::RecoveryKind::Tp,
                    nominal: i32::from(amount),
                    applied: i32::from(actor.tp) - i32::from(previous),
                });
            }
            Self::Cure(cure) => battle.apply_cure(target, cure, cues),
            Self::Buff(buff) => apply_buff(battle, target, buff, cues),
            Self::Resurrect(percent) => {
                battle.enter_revival(target, cues);
                if percent != 0 {
                    battle.recover_vitals(target, i16::from(percent), true, true, cues)?;
                }
            }
            Self::Sacrifice(effect) => sacrifice(battle, caster, effect, cues)?,
            Self::Stat {
                condition,
                percent,
                duration,
            } => {
                let actor = &mut battle.actors[target.index()];
                if !actor.conditions.rejects(condition) {
                    actor.conditions.apply_stat_condition(
                        condition,
                        i16::from(percent),
                        u32::from(duration),
                        false,
                    );
                    cues.push(Cue::ConditionLabel {
                        actor: target,
                        kind: crate::conditions::ConditionLabel::StatUp(condition),
                        position: actor.condition_label_origin(),
                    });
                }
            }
            Self::DefenseDown {
                amount,
                duration,
                tint,
            } => {
                use crate::conditions::{Condition, ConditionLabel};
                let actor = &mut battle.actors[target.index()];
                let cancels_buff = !actor.conditions.rejects(Condition::DefenseDown)
                    && actor.conditions.effective().contains(Condition::DefenseUp);
                let amount = actor
                    .conditions
                    .magnitude(Condition::DefenseDown)
                    .min(amount);
                let applied = actor.conditions.apply_stat_condition(
                    Condition::DefenseDown,
                    amount,
                    u32::from(duration),
                    false,
                );
                if applied {
                    cues.push(Cue::ConditionLabel {
                        actor: target,
                        kind: ConditionLabel::StatusDown,
                        position: actor.condition_label_origin(),
                    });
                } else if cancels_buff {
                    cues.push(Cue::CustomLabel {
                        actor: target,
                        position: actor.condition_label_origin(),
                        left: "STATUS".into(),
                        right: "CANCEL".into(),
                        duration: 45,
                    });
                }
                battle.model_requests.push(crate::ModelRequest::Tint {
                    actor: target,
                    color: tint,
                });
            }
        }
        Ok(())
    }
}

fn sacrifice(
    battle: &mut Battle,
    caster: ActorId,
    appearance: EffectAppearance,
    cues: &mut Vec<Cue>,
) -> Result<()> {
    let actor = &mut battle.actors[caster.index()];
    let (origin, side, amount) = (actor.position, actor.side, actor.hp);
    actor.hp = 0;
    cues.push(Cue::IncidentalDamage {
        actor: caster,
        amount,
    });
    battle.enter_death(caster, cues);
    for index in 0..battle.actors.len() {
        let actor = &battle.actors[index];
        if index == caster.index()
            || actor.side != side
            || !(actor.available() || actor.availability == crate::ActorAvailability::Dead)
        {
            continue;
        }
        let distance = (actor.position[0] - origin[0]).hypot(actor.position[2] - origin[2]);
        let percent = sacrifice_percent(distance);
        let target = ActorId::from_index(index)?;
        if actor.availability == crate::ActorAvailability::Dead {
            battle.enter_revival(target, cues);
        }
        battle.recover_vitals(target, percent, true, true, cues)?;
        let actor = &battle.actors[index];
        cues.push(Cue::DeferredEffect(crate::EffectRequest {
            owner: caster,
            target,
            appearance,
            origin: actor.effect_origin(),
            heading: actor.heading,
            follow: Some(crate::EffectFollow::Center(target)),
            scale: 1.,
            tint: Default::default(),
        }));
    }
    Ok(())
}

fn sacrifice_percent(distance: f32) -> i16 {
    const NEAR_PERCENT: i16 = 50;
    const DISTANCE_PER_PERCENT: f32 = 36.;
    const FAR_DISTANCE: f32 = 936.;
    const FAR_PERCENT: i16 = 30;
    if distance >= FAR_DISTANCE {
        FAR_PERCENT
    } else {
        NEAR_PERCENT - (distance / DISTANCE_PER_PERCENT) as i16
    }
}

fn apply_buff(
    battle: &mut Battle,
    target: ActorId,
    buff: crate::conditions::Buff,
    cues: &mut Vec<Cue>,
) {
    let actor = &mut battle.actors[target.index()];
    let position = actor.condition_label_origin();
    if let Some(kind) = actor.conditions.prepare_buff(buff, false).commit(actor) {
        cues.push(Cue::ConditionLabel {
            actor: target,
            kind,
            position,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpellOrigin {
    Caster,
    CasterRoot,
    /// Capture the ground point immediately, offset toward the caster.
    GroundAtRelease {
        toward_caster: f32,
    },
    /// Capture after the opening transition, before the scene's startup effect.
    SceneGround {
        toward_caster: f32,
    },
    SceneTarget {
        minimum_height: f32,
    },
    /// Target root X/Z, with twice its world-space center height added to root Y.
    SceneAboveTarget,
    SceneCasterAhead {
        distance: f32,
        height: f32,
    },
    SceneCasterRing {
        distance: f32,
        minimum_height: f32,
    },
    SceneFixed {
        position: [f32; 3],
    },
    SceneTargetPlane {
        height: f32,
        toward_caster: f32,
        body_center: bool,
    },
    /// Bind the scene to its target; each contact samples the current body center.
    FollowingTarget,
    /// Body center selected on the update after release, then retained.
    Target,
    /// Ground point selected on the update after release, then retained.
    TargetGround,
}

impl SpellOrigin {
    fn waits_for_target(self) -> bool {
        matches!(self, Self::Target | Self::TargetGround)
    }

    fn waits_for_scene(self) -> bool {
        matches!(
            self,
            Self::SceneGround { .. }
                | Self::SceneTarget { .. }
                | Self::SceneAboveTarget
                | Self::SceneCasterAhead { .. }
                | Self::SceneCasterRing { .. }
                | Self::SceneFixed { .. }
                | Self::SceneTargetPlane { .. }
                | Self::FollowingTarget
        )
    }
}

impl PreparedSpell {
    pub(crate) fn uses_support_target(&self) -> bool {
        !self.recovery.is_empty() || matches!(self.policy, SpellPolicy::TransferRecovery { .. })
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            match self.policy {
                SpellPolicy::Static => true,
                SpellPolicy::CasterPose { at, lift, .. } =>
                    at > self.transition_ticks
                        && at < self.duration
                        && lift.is_none_or(|lift| lift.height.is_finite()
                            && lift.height > 0.
                            && lift.rise_until > at
                            && lift.fall_at >= lift.rise_until
                            && u32::from(lift.fall_at) + u32::from(lift.rise_until - at)
                                < u32::from(self.duration)),
                SpellPolicy::TransferRecovery {
                    apex,
                    speed,
                    benefit,
                    ..
                } =>
                    apex.is_finite()
                        && speed.is_finite()
                        && speed > 0.
                        && benefit.valid()
                        && self.origin == SpellOrigin::Caster
                        && self.transition_ticks == 0,
                SpellPolicy::TrackGround { start, end, speed } =>
                    start > self.transition_ticks
                        && start < end
                        && end <= self.duration
                        && speed.is_finite()
                        && speed > 0.,
                SpellPolicy::CaptureRecipient { at, radius } =>
                    at > self.transition_ticks
                        && at < self.duration
                        && radius.is_finite()
                        && radius > 0.,
                SpellPolicy::Cutin {
                    at,
                    duration,
                    hold_ticks,
                    reveal,
                    ..
                } =>
                    at > self.transition_ticks
                        && at < self.duration
                        && duration > 0
                        && hold_ticks > 0
                        && reveal.is_none_or(|(tick, text)| {
                            tick > self.transition_ticks && tick < self.duration && !text.is_empty()
                        }),
            },
            "invalid spell policy"
        );
        if let Some(summon) = &self.summon {
            ensure!(
                self.projectiles.is_empty()
                    && self.effects.is_empty()
                    && self.recovery.is_empty()
                    && self.transition_ticks == 0
                    && self.startup.is_none(),
                "summon duplicates ordinary spell release"
            );
            summon.validate(self.duration)?;
        }
        ensure!(
            (!self.projectiles.is_empty()
                || !self.recovery.is_empty()
                || !self.effects.is_empty()
                || self.startup.is_some()
                || self.summon.is_some()
                || matches!(self.policy, SpellPolicy::TransferRecovery { .. }))
                && self
                    .projectiles
                    .windows(2)
                    .all(|rows| rows[0].at <= rows[1].at)
                && self
                    .projectiles
                    .last()
                    .is_none_or(|projectile| projectile.at <= self.duration)
                && self.effects.windows(2).all(|rows| rows[0].at <= rows[1].at)
                && self
                    .effects
                    .last()
                    .is_none_or(|effect| effect.at <= self.duration)
                && self
                    .recovery
                    .windows(2)
                    .all(|pulses| pulses[0].at <= pulses[1].at)
                && self.recovery.iter().all(|recovery| {
                    recovery.duration > 0
                        && u32::from(recovery.at) + u32::from(recovery.duration) - 1
                            <= u32::from(self.duration)
                        && recovery.benefit.valid()
                        && (!matches!(recovery.benefit, SpellBenefit::Sacrifice(_))
                            || recovery.targets == RecoveryTargets::Caster)
                        && match recovery.targets {
                            RecoveryTargets::Area { radius, .. } => {
                                radius.is_finite() && radius > 0.
                            }
                            _ => true,
                        }
                })
                && self.transition_ticks <= self.duration
                && self.stage_tint.is_none_or(|tint| {
                    tint.duration > 0
                        && (self.transition_ticks > 0 || self.summon.is_some())
                        && self.transition_ticks < self.duration
                })
                && self.startup.is_none_or(|(at, _)| at <= self.duration)
                && (!self.origin.waits_for_target()
                    || (self
                        .projectiles
                        .first()
                        .is_none_or(|projectile| projectile.at > 0)
                        && self.effects.first().is_none_or(|effect| effect.at > 0)
                        && self.startup.is_none_or(|(at, _)| at > 0)
                        && self.recovery.first().is_none_or(|pulse| pulse.at > 0))),
            "empty or invalid spell schedule"
        );
        if let Some(camera) = self.camera {
            camera.bounds.validate()?;
            ensure!(
                camera.duration > 0
                    && (self.transition_ticks > 0 || self.summon.is_some())
                    && self.transition_ticks < self.duration,
                "invalid stored scene camera duration"
            );
        }
        if let Some((
            _,
            SpellStartup::AllyRing {
                first_heading,
                heading_step,
                ..
            },
        )) = self.startup
        {
            ensure!(
                first_heading.is_finite() && heading_step.is_finite(),
                "invalid spell ring heading"
            );
        }
        if self.origin.waits_for_scene() {
            ensure!(
                self.projectiles
                    .first()
                    .is_none_or(|projectile| projectile.at > self.transition_ticks)
                    && self
                        .effects
                        .first()
                        .is_none_or(|effect| effect.at > self.transition_ticks)
                    && self
                        .startup
                        .is_none_or(|(at, _)| at > self.transition_ticks)
                    && self
                        .recovery
                        .first()
                        .is_none_or(|pulse| pulse.at > self.transition_ticks),
                "stored origin used before scene initialization"
            );
        }
        ensure!(
            match self.origin {
                SpellOrigin::GroundAtRelease { toward_caster }
                | SpellOrigin::SceneGround { toward_caster } =>
                    toward_caster.is_finite() && toward_caster >= 0.,
                SpellOrigin::SceneTarget { minimum_height } => minimum_height.is_finite(),
                SpellOrigin::SceneCasterAhead { distance, height } =>
                    distance.is_finite() && distance >= 0. && height.is_finite(),
                SpellOrigin::SceneCasterRing {
                    distance,
                    minimum_height,
                } => distance.is_finite() && distance >= 0. && minimum_height.is_finite(),
                SpellOrigin::SceneFixed { position } => position.iter().all(|v| v.is_finite()),
                SpellOrigin::SceneTargetPlane {
                    height,
                    toward_caster,
                    ..
                } => height.is_finite() && toward_caster.is_finite() && toward_caster >= 0.,
                _ => true,
            },
            "invalid spell origin"
        );
        for projectile in &self.projectiles {
            ensure!(
                projectile
                    .origin_offset
                    .iter()
                    .all(|value| value.is_finite())
                    && match projectile.launch {
                        SpellLaunch::Live => true,
                        SpellLaunch::Captured(offset) | SpellLaunch::AtHeight(offset) =>
                            offset.is_finite(),
                        SpellLaunch::CasterAbove {
                            center_height_scale,
                            height_offset,
                        } => center_height_scale.is_finite() && height_offset.is_finite(),
                        SpellLaunch::ForwardCone {
                            minimum_dot,
                            vertical_limit,
                        } =>
                            (0. ..1.).contains(&minimum_dot) && (0. ..1.).contains(&vertical_limit),
                    },
                "invalid spell projectile placement"
            );
            projectile.definition.validate()?;
        }
        ensure!(
            self.effects.iter().all(|effect| {
                let placement_valid = match effect.placement {
                    SpellEffectPlacement::Scene(offset) => {
                        offset.iter().all(|value| value.is_finite())
                    }
                    _ => true,
                };
                placement_valid && effect.captured_heading_offset.is_none_or(f32::is_finite)
            }),
            "invalid spell effect placement"
        );
        Ok(())
    }
}

pub(crate) struct CastPoseTransition {
    pub parent: ActionId,
    pub action: crate::ActionKey,
    pub opening: Option<crate::MotionBinding>,
    pub release: Option<crate::AttackPose>,
    pub release_loop_start: f32,
    pub at: u16,
}

impl CastPoseTransition {
    fn owns(&self, battle: &Battle, actor: ActorId) -> bool {
        battle.actors[actor.index()].available()
            && battle.sequence(&self.parent).is_some_and(|sequence| {
                sequence.actor == actor
                    && sequence.action == self.action
                    && matches!(
                        &sequence.execution,
                        crate::action::Execution::Attack(_)
                            | crate::action::Execution::Casting(_)
                            | crate::action::Execution::Recovering { .. }
                    )
            })
    }
}

#[derive(Default)]
pub(crate) enum TransferPhase {
    #[default]
    Rising,
    Falling,
    Complete,
}

/// A released spell owns its targeting and clock independently of the caster.
pub(crate) struct Volley {
    pub definition: Arc<PreparedSpell>,
    pub actor: ActorId,
    pub target: ActorId,
    /// Aim captured during the caster's visit, before later actors move.
    pub projectile_target: [f32; 3],
    pub slot: SpellSlot,
    pub age: u32,
    pub origin: Option<[f32; 3]>,
    pub effect_origin: Option<[f32; 3]>,
    pub heading: f32,
    pub emitted: usize,
    pub summon: Option<crate::summon::ActiveSummon>,
    pub recipient: Option<ActorId>,
    pub cast_pose: Option<CastPoseTransition>,
    /// Release feedback waits until the stored scene's opening ends.
    pub scene_action: Option<crate::ActionKey>,
    pub transfer_phase: TransferPhase,
}

impl Volley {
    pub(crate) fn advancing_cast_pose(&self, battle: &Battle) -> Option<ActorId> {
        let pose = self.cast_pose.as_ref()?;
        (self.age > 0 && pose.owns(battle, self.actor)).then_some(self.actor)
    }

    pub(crate) fn transitioning(&self) -> bool {
        self.definition.transition_ticks != 0
            && self.age <= u32::from(self.definition.transition_ticks)
    }

    pub(crate) fn step(
        &mut self,
        battle: &mut Battle,
        id: ActionId,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        // The release owns this clock, but only the original cast owns its pose.
        // Age zero precedes consumption of the opening's model request.
        if self.age > 0
            && let Some(pose) = &self.cast_pose
        {
            let owned = pose.owns(battle, self.actor)
                && battle.models.as_ref().is_none_or(|models| {
                    models.definition(self.actor).is_some_and(|model| {
                        let expected = if self.age > u32::from(pose.at) || pose.at == 0 {
                            pose.release.map(|release| release.motion)
                        } else {
                            pose.opening
                        };
                        expected.is_none_or(|expected| {
                            model.resource == expected.model
                                && models
                                    .main_motion_observation(self.actor)
                                    .is_some_and(|motion| motion.clip == expected.clip)
                        })
                    })
                });
            if owned
                && self.age == u32::from(pose.at)
                && let Some(release) = pose.release
            {
                battle.request_pose(
                    self.actor,
                    Some(release.motion),
                    crate::Pose {
                        loop_start: pose.release_loop_start,
                        ..release.playback()
                    },
                );
            }
            if !owned || self.age > u32::from(self.definition.transition_ticks) {
                self.cast_pose = None;
            }
        }
        if let Some(definition) = &self.definition.summon {
            if self.summon.is_none() {
                self.summon = Some(crate::summon::ActiveSummon::begin(
                    definition,
                    battle,
                    self.actor,
                    &mut self.target,
                ));
                cues.push(Cue::EffectNotice {
                    actor: self.actor,
                    text: definition.title.clone(),
                    duration: self.definition.duration,
                });
                if let Some(tint) = self.definition.stage_tint {
                    cues.push(Cue::StageTint(tint));
                }
                if let (Some(request), Some(camera)) = (self.definition.camera, &mut battle.camera)
                {
                    camera.request_scene(request);
                }
            }
            self.summon.as_mut().unwrap().step(
                definition,
                battle,
                id,
                self.actor,
                self.target,
                self.age,
                cues,
            )?;
            if self.age >= u32::from(self.definition.duration) {
                let resources: std::collections::BTreeSet<_> = definition
                    .effects
                    .iter()
                    .map(|effect| effect.appearance.resource)
                    .collect();
                for resource in resources {
                    cues.push(Cue::ClearEffectBank {
                        actor: self.actor,
                        resource,
                    });
                }
                battle.discard_projectiles(id, cues);
                return Ok(true);
            }
            self.age += 1;
            return Ok(false);
        }
        if self.age == u32::from(self.definition.transition_ticks) + 1
            && let Some(action) = self.scene_action.take()
        {
            cues.push(Cue::Casting {
                actor: self.actor,
                action,
                phase: crate::CastPhase::Released,
            });
        }
        if self.definition.origin.waits_for_scene()
            && self.age <= u32::from(self.definition.transition_ticks)
        {
            self.age += 1;
            return Ok(false);
        }
        if self.age == 0 && self.definition.origin.waits_for_target() {
            self.age = 1;
            return Ok(false);
        }
        let following = self.definition.origin == SpellOrigin::FollowingTarget;
        let mut origin = if following && self.origin.is_some() {
            battle.actors[self.target.index()].effect_origin()
        } else if let Some(origin) = self.origin {
            origin
        } else {
            let target = &battle.actors[self.target.index()];
            let available = |actor: &crate::Actor| {
                self.definition
                    .recovery
                    .first()
                    .map_or_else(|| actor.available(), |pulse| pulse.benefit.accepts(actor))
            };
            if !available(target)
                && let Some(index) = battle
                    .actors
                    .iter()
                    .position(|a| a.side == target.side && available(a))
            {
                self.target = ActorId::from_index(index)?;
                self.projectile_target = battle.actor_start_centers[index];
            }
            let actor = &battle.actors[self.actor.index()];
            let target = &battle.actors[self.target.index()];
            match self.definition.origin {
                SpellOrigin::Caster => actor.effect_origin(),
                SpellOrigin::CasterRoot => actor.position,
                SpellOrigin::GroundAtRelease { toward_caster }
                | SpellOrigin::SceneGround { toward_caster } => {
                    let mut point = [target.position[0], 0., target.position[2]];
                    let direction = [
                        actor.position[0] - point[0],
                        0.,
                        actor.position[2] - point[2],
                    ];
                    let distance = crate::distance::length(direction);
                    if distance >= 0.5 {
                        for axis in [0, 2] {
                            point[axis] += direction[axis] / distance * toward_caster;
                        }
                    }
                    point
                }
                SpellOrigin::SceneTarget { minimum_height } => {
                    let mut point = target.effect_origin();
                    point[1] = point[1].max(minimum_height);
                    point
                }
                SpellOrigin::SceneAboveTarget => {
                    let mut point = target.position;
                    point[1] += 2. * target.effect_origin()[1];
                    point
                }
                SpellOrigin::SceneCasterAhead { distance, height } => {
                    let point = actor.effect_origin();
                    let (sin, cos) = actor.heading.to_radians().sin_cos();
                    [point[0] + sin * distance, height, point[2] + cos * distance]
                }
                SpellOrigin::SceneCasterRing {
                    distance,
                    minimum_height,
                } => {
                    let point = actor.effect_origin();
                    let (sin, cos) = actor.heading.to_radians().sin_cos();
                    [
                        point[0] + sin * distance,
                        point[1].max(minimum_height),
                        point[2] + cos * distance,
                    ]
                }
                SpellOrigin::SceneFixed { position } => position,
                SpellOrigin::SceneTargetPlane {
                    height,
                    toward_caster,
                    body_center,
                } => {
                    let point = if body_center {
                        target.effect_origin()
                    } else {
                        target.position
                    };
                    let delta = [actor.position[0] - point[0], actor.position[2] - point[2]];
                    let distance = delta[0].hypot(delta[1]);
                    let nudge = if distance >= 0.5 {
                        toward_caster / distance
                    } else {
                        0.
                    };
                    [
                        point[0] + delta[0] * nudge,
                        height,
                        point[2] + delta[1] * nudge,
                    ]
                }
                SpellOrigin::Target | SpellOrigin::FollowingTarget => target.effect_origin(),
                SpellOrigin::TargetGround => [target.position[0], 0., target.position[2]],
            }
        };
        if self.origin.is_none() {
            self.heading = battle.actors[self.actor.index()].heading;
        }
        if let SpellPolicy::Cutin {
            reveal: Some((at, text)),
            ..
        } = self.definition.policy
            && self.age == u32::from(at)
            && battle.actors[self.actor.index()].side == crate::Side::Party
        {
            cues.push(Cue::EffectNotice {
                actor: self.actor,
                text: text.into(),
                duration: REVEALED_NOTICE_TICKS,
            });
        }
        // Visual emitters visit the captured anchor before scene movement.
        self.effect_origin = Some(origin);
        match self.definition.policy {
            SpellPolicy::CasterPose { at, pose, lift } => {
                if self.age == u32::from(at)
                    && let Some(pose) = pose
                {
                    battle.request_pose(self.actor, Some(pose.motion), pose.playback());
                }
                if let Some(lift) = lift {
                    let rise_ticks = lift.rise_until - at;
                    let landing = u32::from(lift.fall_at) + u32::from(rise_ticks);
                    if (u32::from(at)..=landing).contains(&self.age) {
                        let movement = &mut battle.actors[self.actor.index()].movement;
                        let speed = lift.height / f32::from(rise_ticks);
                        movement.gravity = if self.age == landing && !movement.flying {
                            crate::movement::GRAVITY
                        } else {
                            0.
                        };
                        movement.vertical = if self.age < u32::from(lift.rise_until) {
                            speed
                        } else if self.age >= u32::from(lift.fall_at) && self.age < landing {
                            -speed
                        } else {
                            0.
                        };
                        if self.age < landing
                            && let Some(models) = &mut battle.models
                        {
                            // The next actor visit precedes the scene's next update.
                            models.preserve_pose_through(self.actor, battle.update + 2);
                        }
                    }
                }
            }
            SpellPolicy::TransferRecovery {
                appearance,
                apex,
                speed,
                benefit,
            } if self.age > 0 && !matches!(self.transfer_phase, TransferPhase::Complete) => {
                let target = &battle.actors[self.target.index()];
                let arrived = match self.transfer_phase {
                    TransferPhase::Rising => {
                        origin[1] += speed;
                        if origin[1] >= apex {
                            self.transfer_phase = TransferPhase::Falling;
                        }
                        false
                    }
                    TransferPhase::Falling => {
                        origin = [target.position[0], origin[1] - speed, target.position[2]];
                        (origin[1] - target.position[1]).abs() <= speed
                    }
                    TransferPhase::Complete => unreachable!(),
                };
                cues.push(Cue::DeferredEffect(crate::EffectRequest {
                    owner: self.actor,
                    target: self.target,
                    appearance,
                    origin,
                    heading: battle.actors[self.actor.index()].heading,
                    follow: None,
                    scale: 1.,
                    tint: Default::default(),
                }));
                if arrived && benefit.accepts(target) {
                    self.transfer_phase = TransferPhase::Complete;
                    benefit.apply(battle, self.actor, self.target, cues)?;
                }
            }
            SpellPolicy::Cutin {
                at,
                catalogue,
                duration,
                hold_ticks,
                ..
            } if self.age == u32::from(at)
                && battle.actors[self.actor.index()].side == crate::Side::Party =>
            {
                battle.request_timed_hold(hold_ticks, None);
                cues.push(Cue::Cutin {
                    action: id,
                    catalogue,
                    duration,
                });
            }
            SpellPolicy::TrackGround { start, end, speed }
                if (u32::from(start)..u32::from(end)).contains(&self.age) =>
            {
                let target = battle.actors[self.target.index()].position;
                let delta = [target[0] - origin[0], target[2] - origin[2]];
                let distance = delta[0].hypot(delta[1]);
                if distance >= 0.5 {
                    let delta = [delta[0] / distance * speed, 0., delta[1] / distance * speed];
                    for (coordinate, delta) in origin.iter_mut().zip(delta) {
                        *coordinate += delta;
                    }
                    for projectile in battle.projectiles.values_mut().filter(|p| p.action == id) {
                        for (coordinate, delta) in projectile.frame.position.iter_mut().zip(delta) {
                            *coordinate += delta;
                        }
                    }
                }
            }
            SpellPolicy::CaptureRecipient { at, radius } if self.age == u32::from(at) => {
                let side = battle.actors[self.actor.index()].side;
                self.recipient = battle
                    .actors
                    .iter()
                    .position(|actor| {
                        actor.side != side
                            && (actor.position[0] - origin[0]).hypot(actor.position[2] - origin[2])
                                < radius
                    })
                    .map(|index| ActorId(index as u8));
                if let Some(target) = self.recipient {
                    self.target = target;
                }
            }
            _ => {}
        }
        self.origin = Some(origin);
        let has_recipient = !matches!(self.definition.policy,
            SpellPolicy::CaptureRecipient { at, .. } if self.age >= u32::from(at) && self.recipient.is_none());
        if self.age == u32::from(self.definition.transition_ticks) + 1 {
            if let Some(tint) = self.definition.stage_tint {
                cues.push(Cue::StageTint(tint));
            }
            if let (Some(request), Some(camera)) = (self.definition.camera, &mut battle.camera) {
                camera.request_scene(request);
            }
        }
        if let Some((at, startup)) = self.definition.startup
            && self.age == u32::from(at)
        {
            let actor = &battle.actors[self.actor.index()];
            let request = crate::EffectRequest {
                owner: self.actor,
                target: self.target,
                appearance: match startup {
                    SpellStartup::Effect(effect)
                    | SpellStartup::DeferredEffect(effect)
                    | SpellStartup::CasterGroundEffect(effect) => effect,
                    SpellStartup::AllyRing { effects, .. } => effects[0],
                },
                origin,
                heading: actor.heading,
                follow: if matches!(self.definition.policy, SpellPolicy::TrackGround { .. }) {
                    Some(crate::EffectFollow::Spell(id))
                } else {
                    following.then_some(crate::EffectFollow::Center(self.target))
                },
                scale: if self.definition.transition_ticks > 0 {
                    1.
                } else {
                    actor.effect_scale
                },
                tint: Default::default(),
            };
            match startup {
                SpellStartup::Effect(_) => cues.push(Cue::Effect(request)),
                SpellStartup::DeferredEffect(_) => cues.push(Cue::DeferredEffect(request)),
                SpellStartup::CasterGroundEffect(_) => {
                    cues.push(Cue::Effect(crate::EffectRequest {
                        origin: [actor.position[0], 0., actor.position[2]],
                        follow: None,
                        ..request
                    }))
                }
                SpellStartup::AllyRing {
                    effects,
                    first_heading,
                    heading_step,
                } => {
                    let count = battle
                        .actors
                        .iter()
                        .filter(|other| {
                            other.side == actor.side
                                && other.availability != crate::ActorAvailability::Absent
                        })
                        .count();
                    ensure!(
                        count <= effects.len(),
                        "spell ring exceeds its model roster"
                    );
                    for (slot, appearance) in effects.into_iter().take(count).enumerate() {
                        cues.push(Cue::DeferredEffect(crate::EffectRequest {
                            appearance,
                            origin: [0.; 3],
                            heading: actor.heading + first_heading + slot as f32 * heading_step,
                            scale: 1.,
                            ..request.clone()
                        }));
                    }
                }
            }
        }
        let placed = |offset| {
            let offset = crate::geometry::rotate(offset, self.heading);
            std::array::from_fn(|axis| origin[axis] + offset[axis])
        };
        for effect in &self.definition.effects {
            if self.age == u32::from(effect.at) && has_recipient {
                let mut emit = |target, position, heading, follow| {
                    let request = crate::EffectRequest {
                        owner: self.actor,
                        target,
                        appearance: effect.appearance,
                        origin: position,
                        heading,
                        follow,
                        scale: 1.,
                        tint: Default::default(),
                    };
                    cues.push(if effect.deferred {
                        Cue::DeferredEffect(request)
                    } else {
                        Cue::Effect(request)
                    })
                };
                match effect.placement {
                    SpellEffectPlacement::Scene(offset) => {
                        let heading = if matches!(
                            self.definition.policy,
                            SpellPolicy::CaptureRecipient { .. }
                        ) {
                            battle.actors[self.target.index()].heading
                        } else {
                            effect
                                .captured_heading_offset
                                .map_or(0., |offset| self.heading + offset)
                        };
                        emit(self.target, placed(offset), heading, None);
                    }
                    SpellEffectPlacement::CasterGround => {
                        let actor = &battle.actors[self.actor.index()];
                        emit(
                            self.actor,
                            [actor.position[0], 0., actor.position[2]],
                            actor.heading,
                            None,
                        );
                    }
                    SpellEffectPlacement::CasterCenter => {
                        let actor = &battle.actors[self.actor.index()];
                        emit(
                            self.actor,
                            actor.effect_origin(),
                            actor.heading,
                            Some(crate::EffectFollow::Center(self.actor)),
                        );
                    }
                    SpellEffectPlacement::EnemyCenters => {
                        let side = battle.actors[self.actor.index()].side;
                        for (index, actor) in battle.actors.iter().enumerate() {
                            if actor.side != side && actor.available() {
                                let target = ActorId::from_index(index)?;
                                emit(
                                    target,
                                    actor.effect_origin(),
                                    actor.heading,
                                    Some(crate::EffectFollow::Center(target)),
                                );
                            }
                        }
                    }
                }
            }
        }
        while let Some(projectile) = self.definition.projectiles.get(self.emitted)
            && self.age >= u32::from(projectile.at)
        {
            self.emitted += 1;
            if !has_recipient {
                continue;
            }
            let position = match projectile.launch {
                SpellLaunch::CasterAbove {
                    center_height_scale,
                    height_offset,
                } => {
                    let actor = &battle.actors[self.actor.index()];
                    let mut point = actor.position;
                    point[1] += actor.body.center_offset[1] * center_height_scale + height_offset;
                    point
                }
                _ => placed(projectile.origin_offset),
            };
            let live_heading = battle.actors[self.actor.index()].heading;
            let heading = match projectile.launch {
                SpellLaunch::Captured(offset) => self.heading + offset,
                _ => live_heading,
            };
            let emitted = battle.emit_aimed(
                Arc::clone(&projectile.definition),
                id,
                self.actor,
                self.target,
                position,
                heading,
                match projectile.launch {
                    SpellLaunch::AtHeight(height) => Some([origin[0], height, origin[2]]),
                    _ => Some(self.projectile_target),
                },
            )?;
            if let SpellLaunch::ForwardCone {
                minimum_dot,
                vertical_limit,
            } = projectile.launch
            {
                let projectile = battle.projectiles.get_mut(&emitted).unwrap();
                projectile.velocity = cone_direction(
                    position,
                    self.projectile_target,
                    live_heading,
                    minimum_dot,
                    vertical_limit,
                );
                projectile.frame.velocity = projectile.velocity;
                for cue in &mut battle.pending_cues {
                    if let Cue::ProjectileStarted {
                        projectile: id,
                        velocity,
                        ..
                    } = cue
                        && *id == emitted
                    {
                        *velocity = projectile.velocity;
                    }
                }
            }
            if let Some(sound) = self.definition.sound {
                cues.push(Cue::Sound {
                    actor: self.actor,
                    sound,
                    position: origin,
                    priority: 2,
                });
            }
        }
        for recovery in self.definition.recovery.iter().filter(|pulse| {
            (u32::from(pulse.at)..u32::from(pulse.at) + u32::from(pulse.duration))
                .contains(&self.age)
        }) {
            let side = battle.actors[self.actor.index()].side;
            let eligible =
                |actor: &crate::Actor| actor.side == side && recovery.benefit.accepts(actor);
            let targets: Vec<_> = match recovery.targets {
                RecoveryTargets::Caster => eligible(&battle.actors[self.actor.index()])
                    .then_some(self.actor)
                    .into_iter()
                    .collect(),
                RecoveryTargets::Target => {
                    let target = if eligible(&battle.actors[self.target.index()]) {
                        Some(self.target)
                    } else {
                        battle
                            .actors
                            .iter()
                            .position(eligible)
                            .map(|index| ActorId(index as u8))
                    };
                    if let Some(target) = target {
                        self.target = target;
                    }
                    target.into_iter().collect()
                }
                RecoveryTargets::LivingAllies => battle
                    .actors
                    .iter()
                    .enumerate()
                    .filter_map(|(index, actor)| eligible(actor).then_some(ActorId(index as u8)))
                    .collect(),
                RecoveryTargets::LivingOpponents => battle
                    .actors
                    .iter()
                    .enumerate()
                    .filter_map(|(index, actor)| {
                        (actor.side != side && recovery.benefit.accepts(actor))
                            .then_some(ActorId(index as u8))
                    })
                    .collect(),
                RecoveryTargets::Area { radius, inclusive } => battle
                    .actors
                    .iter()
                    .enumerate()
                    .filter_map(|(index, actor)| {
                        let distance =
                            (actor.position[0] - origin[0]).hypot(actor.position[2] - origin[2]);
                        (eligible(actor)
                            && match recovery.benefit {
                                SpellBenefit::Cure(cure) => {
                                    cure.eligible_base(actor.conditions.effective())
                                }
                                _ => true,
                            }
                            && if inclusive {
                                distance <= radius
                            } else {
                                distance < radius
                            })
                        .then_some(ActorId(index as u8))
                    })
                    .collect(),
            };
            for target in targets {
                let flash = recovery.flash.filter(|_| match recovery.benefit {
                    SpellBenefit::Stat { condition, .. } => {
                        !battle.actors[target.index()].conditions.rejects(condition)
                    }
                    _ => true,
                });
                recovery.benefit.apply(battle, self.actor, target, cues)?;
                if let Some(color) = flash {
                    cues.push(Cue::DeferredFlash {
                        actor: target,
                        color,
                        fade_first: false,
                    });
                }
                let Some((appearance, tint)) = recovery.effect else {
                    continue;
                };
                let actor = &battle.actors[target.index()];
                let (owner, follow, origin, heading, scale) = match recovery.effect_placement {
                    RecoveryEffectPlacement::RecipientCenter => (
                        target,
                        crate::EffectFollow::Center(target),
                        actor.effect_origin(),
                        if recovery.targets == RecoveryTargets::LivingAllies {
                            actor.heading
                        } else {
                            0.
                        },
                        actor.effect_scale,
                    ),
                    RecoveryEffectPlacement::RecipientRoot => (
                        target,
                        crate::EffectFollow::Actor(target),
                        actor.position,
                        actor.heading,
                        1.,
                    ),
                    RecoveryEffectPlacement::CasterFacingCenter => (
                        self.actor,
                        crate::EffectFollow::Center(target),
                        actor.effect_origin(),
                        battle.actors[self.actor.index()].heading,
                        1.,
                    ),
                    RecoveryEffectPlacement::RecipientFacingCenter => (
                        self.actor,
                        crate::EffectFollow::Center(target),
                        actor.effect_origin(),
                        actor.heading,
                        actor.effect_scale,
                    ),
                    RecoveryEffectPlacement::RecipientFacingCenterUnscaled => (
                        self.actor,
                        crate::EffectFollow::Center(target),
                        actor.effect_origin(),
                        actor.heading,
                        1.,
                    ),
                };
                let request = crate::EffectRequest {
                    owner,
                    target,
                    appearance,
                    origin,
                    heading,
                    follow: Some(follow),
                    scale,
                    tint,
                };
                cues.push(if matches!(recovery.benefit, SpellBenefit::Resurrect(_)) {
                    Cue::AfterParticlesEffect(request)
                } else {
                    Cue::DeferredEffect(request)
                });
            }
        }
        // Water blades finish contacts before their independent scene retires them.
        let retired: Vec<_> = battle
            .projectiles
            .iter()
            .filter_map(|(&flight, projectile)| {
                (projectile.action == id
                    && projectile.definition.motion.aqua_edge
                    && projectile.retiring)
                    .then_some(flight)
            })
            .collect();
        for flight in retired {
            battle.expire_projectile(flight, cues);
        }
        if self.age >= u32::from(self.definition.duration) {
            return Ok(true);
        }
        self.age += 1;
        Ok(false)
    }
}

impl Battle {
    pub(crate) fn spell_active(&self, actor: ActorId, slot: SpellSlot) -> bool {
        self.volleys
            .values()
            .any(|volley| volley.actor == actor && volley.slot == slot)
    }

    /// Release work is independent of the caster's current actor action.
    pub(crate) fn release_volley(
        &mut self,
        definition: Arc<PreparedSpell>,
        actor: ActorId,
        target: ActorId,
        slot: SpellSlot,
        parent: Option<ActionId>,
        cues: &mut Vec<Cue>,
    ) -> Result<Option<ActionId>> {
        if self.spell_active(actor, slot) {
            return Ok(None);
        }
        let volley = Volley {
            definition,
            actor,
            target,
            projectile_target: self.actor_start_centers[target.index()],
            slot,
            age: 0,
            origin: None,
            effect_origin: None,
            heading: 0.,
            emitted: 0,
            summon: None,
            recipient: None,
            cast_pose: None,
            scene_action: None,
            transfer_phase: TransferPhase::default(),
        };
        let id = self.allocate_action_id()?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(inputs) = &mut self.random_inputs {
            inputs.released(actor, parent, id);
        }
        self.volleys.insert(id, volley);
        cues.push(Cue::Released {
            action: id,
            parent,
            actor,
            slot,
        });
        Ok(Some(id))
    }
}

#[cfg(test)]
mod advanced_tests {
    use super::*;

    #[test]
    fn opposing_defense_pulse_preserves_signed_strength_and_recipient_rules() {
        use crate::conditions::{Condition::*, Conditions, Layers};
        for retained in [0, -25] {
            let (_, mut volley) = fixture(SpellPolicy::Static);
            let actors = (0..6)
                .map(|index| {
                    crate::tests::actor(if index < 2 {
                        crate::Side::Enemy
                    } else {
                        crate::Side::Party
                    })
                })
                .collect();
            let mut battle = crate::tests::prepared(actors, 100).finish().unwrap();
            battle.actors[3]
                .conditions
                .apply_stat_condition(DefenseUp, 20, 900, false);
            battle.actors[4].conditions = Conditions::new(Layers {
                immunity: DefenseDown.into(),
                ..Default::default()
            });
            battle.actors[5].availability = crate::ActorAvailability::Petrified;
            if retained != 0 {
                battle.actors[2]
                    .conditions
                    .apply_stat_condition(DefenseDown, retained, 900, false);
            }
            let conditions: Vec<_> = battle
                .actors
                .iter()
                .map(|actor| actor.conditions.clone())
                .collect();
            let vitals: Vec<_> = battle
                .actors
                .iter()
                .map(|actor| (actor.hp, actor.tp))
                .collect();
            let random = battle.random.state();
            let definition = Arc::make_mut(&mut volley.definition);
            definition.projectiles.clear();
            definition.recovery = vec![SpellRecovery {
                duration: 1,
                effect_placement: Default::default(),
                flash: None,
                at: 3,
                benefit: SpellBenefit::DefenseDown {
                    amount: 15,
                    duration: 0,
                    tint: [32; 3],
                },
                targets: RecoveryTargets::LivingOpponents,
                effect: None,
            }];
            definition.validate().unwrap();
            let mut cues = Vec::new();
            for _ in 0..3 {
                volley.step(&mut battle, ActionId(402), &mut cues).unwrap();
            }
            assert!(
                battle
                    .actors
                    .iter()
                    .zip(&conditions)
                    .all(|(actor, before)| &actor.conditions == before)
            );
            assert!(battle.model_requests.is_empty());
            for _ in 0..3 {
                volley.step(&mut battle, ActionId(402), &mut cues).unwrap();
            }
            assert_eq!(battle.actors[2].conditions.magnitude(DefenseDown), retained);
            assert_eq!(
                battle.actors[2].conditions.remaining(DefenseDown),
                Some(1800)
            );
            assert!(!battle.actors[3].conditions.effective().contains(DefenseUp));
            assert!(
                !battle.actors[3]
                    .conditions
                    .effective()
                    .contains(DefenseDown)
            );
            for index in [0, 1, 4, 5] {
                assert_eq!(battle.actors[index].conditions, conditions[index]);
            }
            let tinted: Vec<_> = battle
                .model_requests
                .iter()
                .filter_map(|request| match request {
                    crate::ModelRequest::Tint { actor, color } => {
                        assert_eq!(*color, [32; 3]);
                        Some(actor.index())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(tinted, [2, 3, 4]);
            assert_eq!(
                cues.iter()
                    .filter(|cue| matches!(cue, Cue::ConditionLabel { .. }))
                    .count(),
                1
            );
            assert_eq!(
                cues.iter()
                    .filter(
                        |cue| matches!(cue, Cue::CustomLabel { right, .. } if right == "CANCEL")
                    )
                    .count(),
                1
            );
            assert_eq!(
                battle
                    .actors
                    .iter()
                    .map(|actor| (actor.hp, actor.tp))
                    .collect::<Vec<_>>(),
                vitals
            );
            assert_eq!(battle.random.state(), random);
            assert!(battle.projectiles.is_empty());
        }
    }

    #[test]
    fn sacrifice_revives_allies_and_keeps_the_later_contact() {
        let (_, mut volley) = fixture(SpellPolicy::Static);
        let mut actors = vec![
            crate::tests::actor(crate::Side::Party),
            crate::tests::actor(crate::Side::Party),
            crate::tests::actor(crate::Side::Party),
            crate::tests::actor(crate::Side::Enemy),
        ];
        actors[0].position = [0., 120., 0.];
        for (index, distance) in [(1, 935.), (2, 936.)] {
            actors[index].equipment.max_hp = 1000;
            actors[index].position = [distance, 0., 0.];
        }
        actors[1].hp = 100;
        actors[2].hp = 0;
        actors[2].availability = crate::ActorAvailability::Dead;
        let mut battle = crate::tests::prepared(actors, 100).finish().unwrap();
        let tp = battle
            .actors
            .iter()
            .map(|actor| actor.tp)
            .collect::<Vec<_>>();
        volley.target = ActorId(3);
        let definition = Arc::make_mut(&mut volley.definition);
        definition.startup = Some((
            2,
            SpellStartup::CasterGroundEffect(EffectAppearance {
                resource: 2,
                member: 1,
            }),
        ));
        definition.effects = [
            SpellEffectPlacement::CasterCenter,
            SpellEffectPlacement::EnemyCenters,
        ]
        .into_iter()
        .map(|placement| SpellEffect {
            at: 2,
            appearance: EffectAppearance {
                resource: 2,
                member: 3,
            },
            placement,
            deferred: false,
            captured_heading_offset: None,
        })
        .collect();
        definition.recovery = vec![SpellRecovery {
            duration: 1,
            effect_placement: Default::default(),
            flash: None,
            at: 3,
            benefit: SpellBenefit::Sacrifice(EffectAppearance {
                resource: 2,
                member: 2,
            }),
            targets: RecoveryTargets::Caster,
            effect: None,
        }];
        let mut cues = Vec::new();
        for _ in 0..=5 {
            assert!(!volley.step(&mut battle, ActionId(1), &mut cues).unwrap());
        }
        assert_eq!(battle.actors[0].hp, 0);
        assert_eq!(
            battle.actors[0].availability,
            crate::ActorAvailability::Dead
        );
        assert_eq!(battle.actors[1].hp, 350);
        assert_eq!(battle.actors[2].hp, 300);
        assert_eq!(
            battle.actors[2].availability,
            crate::ActorAvailability::Active
        );
        assert_eq!(
            battle
                .actors
                .iter()
                .map(|actor| actor.tp)
                .collect::<Vec<_>>(),
            tp
        );
        assert_eq!(battle.projectiles.len(), 2);
        assert!(cues.iter().any(|cue| matches!(cue, Cue::Effect(effect)
            if effect.appearance.member == 1 && effect.origin == [0.; 3] && effect.follow.is_none())));
        for actor in [ActorId(0), ActorId(3)] {
            assert!(cues.iter().any(|cue| matches!(cue, Cue::Effect(effect)
                if effect.follow == Some(crate::EffectFollow::Center(actor)))));
        }
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::IncidentalDamage {
                actor: ActorId(0),
                ..
            }
        )));

        // Losing HP during a lift must not replace the scene's pose before landing.
        let mut battle = crate::tests::prepared(
            vec![
                crate::tests::actor(crate::Side::Party),
                crate::tests::actor(crate::Side::Party),
                crate::tests::actor(crate::Side::Enemy),
            ],
            100,
        )
        .finish()
        .unwrap();
        let mut model = crate::tests::model_definition(7, [(0, 1.), (3, 10.), (7, 10.), (80, 60.)]);
        model.idle_motions = [Some(0); 2];
        model.reactions.hurt = [Some(3); 2];
        model.reactions.defeated = Some(7);
        battle.set_models(
            crate::Models::new(
                battle.actors(),
                vec![Some(Arc::new(model)), None, None],
                Default::default(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )
            .unwrap(),
        );
        let (_, mut volley) = fixture(SpellPolicy::CasterPose {
            at: 2,
            pose: Some(crate::AttackPose {
                motion: crate::MotionBinding { model: 7, clip: 80 },
                start: 0.,
                end: None,
                rate: 0.5,
                blend: 0,
                repeat: true,
            }),
            lift: Some(SpellLift {
                height: 2.,
                rise_until: 4,
                fall_at: 5,
            }),
        });
        let definition = Arc::make_mut(&mut volley.definition);
        definition.projectiles.clear();
        definition.recovery = vec![SpellRecovery {
            duration: 1,
            effect_placement: Default::default(),
            flash: None,
            at: 4,
            benefit: SpellBenefit::Sacrifice(EffectAppearance {
                resource: 2,
                member: 2,
            }),
            targets: RecoveryTargets::Caster,
            effect: None,
        }];
        battle
            .release_volley(
                volley.definition,
                ActorId(0),
                ActorId(0),
                SpellSlot::Primary,
                None,
                &mut vec![],
            )
            .unwrap();
        for (age, height) in [0., 0., 0., 1., 2., 2., 1., 0., 0.].into_iter().enumerate() {
            let mut frame = battle.step(crate::BattleInput::default()).unwrap();
            let clock = frame.clock;
            battle.advance_models(&mut frame, clock, true).unwrap();
            assert_eq!(frame.actors[0].position[1], height, "lift age {age}");
            if age >= 4 {
                assert_eq!(frame.actors[0].availability, crate::ActorAvailability::Dead);
            }
            if age >= 2 {
                assert_eq!(
                    frame.models[0].clip,
                    if age < 8 { 80 } else { 7 },
                    "pose age {age}"
                );
            }
        }
    }

    fn fixture(policy: SpellPolicy) -> (Battle, Volley) {
        let battle = crate::tests::prepared(
            vec![
                crate::tests::actor(crate::Side::Party),
                crate::tests::actor(crate::Side::Enemy),
                crate::tests::actor(crate::Side::Enemy),
            ],
            100,
        )
        .finish()
        .unwrap();
        let mut definition = crate::tests::volley();
        definition.policy = policy;
        definition.origin = SpellOrigin::SceneFixed { position: [0.; 3] };
        definition.transition_ticks = 1;
        definition.duration = 10;
        let mut projectile = (*definition.projectiles[0].definition).clone();
        projectile.velocity = [0.; 3];
        projectile.offset = [0.; 3];
        definition.projectiles = [3, 5]
            .into_iter()
            .map(|at| SpellProjectile::new(at, Arc::new(projectile.clone())))
            .collect();
        definition.validate().unwrap();
        let projectile_target = battle.actor_start_centers[2];
        (
            battle,
            Volley {
                definition: Arc::new(definition),
                actor: ActorId(0),
                target: ActorId(2),
                projectile_target,
                slot: SpellSlot::Primary,
                age: 0,
                origin: None,
                effect_origin: None,
                heading: 0.,
                emitted: 0,
                summon: None,
                recipient: None,
                cast_pose: None,
                scene_action: None,
                transfer_phase: TransferPhase::default(),
            },
        )
    }

    #[test]
    fn tracking_moves_the_anchor_and_existing_contacts_only_inside_its_window() {
        let (mut battle, mut volley) = fixture(SpellPolicy::TrackGround {
            start: 4,
            end: 6,
            speed: 1.25,
        });
        battle.actors[2].position = [100., 900., 0.];
        let action = ActionId(400);
        for age in 0..7 {
            volley.step(&mut battle, action, &mut Vec::new()).unwrap();
            if age == 4 {
                assert_eq!(volley.effect_origin, Some([0.; 3]));
                assert_eq!(volley.origin, Some([1.25, 0., 0.]));
            }
            if age == 5 {
                assert_eq!(volley.effect_origin, Some([1.25, 0., 0.]));
                assert_eq!(volley.origin, Some([2.5, 0., 0.]));
            }
        }
        assert_eq!(volley.origin, Some([2.5, 0., 0.]));
        assert_eq!(battle.projectiles.len(), 2);
        assert!(
            battle
                .projectiles
                .values()
                .all(|p| p.frame.position == [2.5, 0., 0.])
        );
        let mut frame = battle.frame(Vec::new(), None);
        frame
            .spell_origins
            .push((action, volley.effect_origin.unwrap()));
        assert_eq!(
            crate::EffectFollow::Spell(action).position(&frame),
            volley.origin
        );
    }

    #[test]
    fn capture_keeps_roster_order_and_cone_aim_keeps_its_vertical_limit() {
        let (mut battle, mut volley) =
            fixture(SpellPolicy::CaptureRecipient { at: 3, radius: 10. });
        battle.actors[1].position = [9., 1000., 0.];
        battle.actors[1].hp = 0;
        battle.actors[2].position = [1., 0., 0.];
        for _ in 0..4 {
            volley
                .step(&mut battle, ActionId(401), &mut Vec::new())
                .unwrap();
        }
        battle.actors[1].position = [1000.; 3];
        for _ in 0..2 {
            volley
                .step(&mut battle, ActionId(401), &mut Vec::new())
                .unwrap();
        }
        assert_eq!(volley.recipient, Some(ActorId(1)));
        assert!(
            battle
                .projectiles
                .values()
                .all(|p| p.frame.target == ActorId(1))
        );
        assert_eq!(
            cone_direction([0.; 3], [100., 10., 0.], 0., 0.5, 0.35),
            [0., 0., 1.]
        );

        let (mut battle, mut volley) = fixture(SpellPolicy::Static);
        battle.actors[0].heading = 0.;
        volley.projectile_target = [0., 1000., 100.];
        Arc::make_mut(&mut volley.definition).projectiles[0].launch = SpellLaunch::ForwardCone {
            minimum_dot: 0.5,
            vertical_limit: 0.35,
        };
        for _ in 0..4 {
            volley
                .step(&mut battle, ActionId(403), &mut Vec::new())
                .unwrap();
        }
        assert!(battle.pending_cues.iter().any(|cue| matches!(
            cue,
            Cue::ProjectileStarted { velocity, .. } if *velocity == [0., 0.35, 1.]
        )));
        let projectile = battle.projectiles.values_mut().next().unwrap();
        projectile.step().unwrap();
        assert_eq!(projectile.frame.position, [0., 0.35, 1.]);

        let (mut battle, mut volley) =
            fixture(SpellPolicy::CaptureRecipient { at: 3, radius: 10. });
        battle.actors[1].position = [10., 0., 0.];
        battle.actors[2].position = [11., 0., 0.];
        for _ in 0..6 {
            volley
                .step(&mut battle, ActionId(402), &mut Vec::new())
                .unwrap();
        }
        assert!(volley.recipient.is_none() && battle.projectiles.is_empty());
    }

    #[test]
    fn recovery_pulses_follow_allies_entering_and_leaving_the_retained_area() {
        let (mut battle, mut volley) = fixture(SpellPolicy::Static);
        let definition = Arc::make_mut(&mut volley.definition);
        definition.projectiles.clear();
        definition.recovery = [3, 5]
            .into_iter()
            .map(|at| SpellRecovery {
                duration: 1,
                effect_placement: Default::default(),
                flash: None,
                at,
                benefit: SpellBenefit::Hp(15),
                targets: RecoveryTargets::Area {
                    radius: 10.,
                    inclusive: false,
                },
                effect: None,
            })
            .collect();
        definition.validate().unwrap();
        battle.actors[0].hp = 10;
        battle.actors[1].side = crate::Side::Party;
        battle.actors[1].hp = 10;
        battle.actors[1].position = [20., 0., 0.];
        let mut cues = Vec::new();
        for _ in 0..4 {
            volley.step(&mut battle, ActionId(403), &mut cues).unwrap();
        }
        assert_eq!([battle.actors[0].hp, battle.actors[1].hp], [25, 10]);
        battle.actors[0].position = [20., 0., 0.];
        battle.actors[1].position = [0.; 3];
        for _ in 0..2 {
            volley.step(&mut battle, ActionId(403), &mut cues).unwrap();
        }
        assert_eq!([battle.actors[0].hp, battle.actors[1].hp], [25, 25]);
        assert_eq!(battle.actors[2].hp, 50);
    }

    #[test]
    fn cure_area_ignores_healthy_allies_and_cures_later_arrivals_until_it_ends() {
        use crate::conditions::{Condition, Conditions, Cure, Layers};
        let (mut battle, mut volley) = fixture(SpellPolicy::Static);
        let definition = Arc::make_mut(&mut volley.definition);
        definition.projectiles.clear();
        definition.recovery = vec![SpellRecovery {
            at: 3,
            duration: 3,
            benefit: SpellBenefit::Cure(Cure::Physical),
            targets: RecoveryTargets::Area {
                radius: 10.,
                inclusive: true,
            },
            effect: Some((
                EffectAppearance {
                    resource: 0,
                    member: 1,
                },
                Default::default(),
            )),
            effect_placement: Default::default(),
            flash: None,
        }];
        definition.validate().unwrap();
        let cursed = || {
            Conditions::new(Layers {
                base: crate::conditions::ConditionSet::of(&[Condition::Curse]),
                ..Default::default()
            })
        };
        battle.actors[1].side = crate::Side::Party;
        battle.actors[1].conditions = cursed();
        battle.actors[1].position = [20., 0., 0.];
        let mut cues = Vec::new();
        for _ in 0..5 {
            volley.step(&mut battle, ActionId(403), &mut cues).unwrap();
        }
        assert!(!cues.iter().any(|cue| matches!(cue, Cue::DeferredEffect(_))));
        battle.actors[1].position = [10., 0., 0.];
        volley.step(&mut battle, ActionId(403), &mut cues).unwrap();
        assert!(
            !battle.actors[1]
                .conditions
                .effective()
                .contains(Condition::Curse)
        );
        assert_eq!(
            cues.iter()
                .filter(
                    |cue| matches!(cue, Cue::DeferredEffect(effect) if effect.target == ActorId(1))
                )
                .count(),
            1
        );
        battle.actors[1].conditions = cursed();
        volley.step(&mut battle, ActionId(403), &mut cues).unwrap();
        assert!(
            battle.actors[1]
                .conditions
                .effective()
                .contains(Condition::Curse)
        );
    }

    #[test]
    fn distance_healing_uses_horizontal_steps_and_keeps_minimum_recovery() {
        let (mut battle, _) = fixture(SpellPolicy::Static);
        battle.actors[0].position = [100., 800., 30.];
        battle.actors[1].side = crate::Side::Party;
        for (percent, offset, recovered) in [
            (25, [199., 800., 0.], 25),
            (25, [200., 0., 0.], 20),
            (25, [399., 0., 399.], 15),
            (25, [1000., 0., 0.], 1),
            (25, [2400., 0., 0.], 1),
            (45, [1000., 0., 0.], 20),
        ] {
            battle.actors[1].position =
                std::array::from_fn(|axis| battle.actors[0].position[axis] + offset[axis]);
            battle.actors[1].hp = 1;
            let benefit = SpellBenefit::HpByDistance {
                percent,
                distance_step: 200,
                penalty_per_step: 5,
                maximum_penalty: 25,
            };
            assert!(benefit.valid());
            benefit
                .apply(&mut battle, ActorId(0), ActorId(1), &mut Vec::new())
                .unwrap();
            assert_eq!(battle.actors[1].hp, 1 + recovered);
        }
    }

    #[test]
    fn zero_hp_revival_uses_prepared_pose_and_allows_followup_healing() {
        let (mut battle, _) = fixture(SpellPolicy::Static);
        let target = ActorId(1);
        battle.actors[1].side = crate::Side::Party;
        battle.actors[1].hp = 0;
        battle.enter_death(target, &mut Vec::new());
        let ticks = 12;
        battle.prepared.actor_setup[1].revival = Some((
            crate::AttackPose {
                motion: crate::MotionBinding { model: 0, clip: 9 },
                start: 0.,
                end: None,
                repeat: false,
                rate: 0.5,
                blend: 4,
            },
            ticks,
        ));
        let mut cues = Vec::new();
        assert!(SpellBenefit::Resurrect(0).accepts(&battle.actors[1]));
        SpellBenefit::Resurrect(0)
            .apply(&mut battle, ActorId(0), target, &mut cues)
            .unwrap();
        assert!(battle.actors[1].available());
        assert_eq!(battle.actors[1].hp, 0);
        assert!(!cues.iter().any(|cue| matches!(cue, Cue::Recovered { .. })));
        assert!(SpellBenefit::Hp(15).accepts(&battle.actors[1]));
        SpellBenefit::Hp(15)
            .apply(&mut battle, ActorId(0), target, &mut cues)
            .unwrap();
        assert_eq!(battle.actors[1].hp, 15);
        for _ in 0..ticks - 1 {
            battle.step(crate::BattleInput::default()).unwrap();
        }
        assert_eq!(battle.activity(target), crate::Activity::Recovering);
        battle.step(crate::BattleInput::default()).unwrap();
        assert_eq!(battle.activity(target), crate::Activity::Idle);
    }
}
