//! Prepared attacks run resolved events on the actor's gameplay clock.
#[cfg(test)]
use crate::ResourceId;
use crate::TechniqueId;
use crate::WeaponSlot;
use crate::{
    ActionExecution, ActionId, Battle, Cue, MeleeDefinition, MotionBinding, Sound,
    WeaponFlightDefinition,
};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

mod hammer;
mod shot;
pub use hammer::{HammerProjectile, HammerRing};
pub use shot::{ProjectileScatter, ProjectileShot};

const ATTACK_BRAKING: f32 = 0.4125;
const RECOVERY_PER_LINK: u32 = 4;

#[derive(Debug, Clone, Copy)]
pub struct AttackPose {
    pub motion: MotionBinding,
    pub start: f32,
    pub end: Option<f32>,
    pub repeat: bool,
    pub rate: f32,
    pub blend: u8,
}

impl AttackPose {
    pub(crate) fn playback(self) -> crate::Pose {
        crate::Pose {
            frame: self.start,
            end: self.end,
            repeat: self.repeat,
            rate: self.rate,
            blend: self.blend,
            ..Default::default()
        }
    }

    pub(crate) fn play(self, battle: &mut Battle, index: usize) {
        battle.request_pose(
            crate::ActorId(index as u8),
            Some(self.motion),
            self.playback(),
        );
    }
}

#[derive(Debug, Clone)]
pub enum AttackEvent {
    /// Emission has its own clock, but interruption cancels its pending births.
    ActionEffect {
        appearance: crate::EffectAppearance,
        origin: AttackOrigin,
    },
    /// Action-owned emission initialized before this update's particle visit.
    InitializedActionEffect {
        appearance: crate::EffectAppearance,
        origin: AttackOrigin,
    },
    /// Hold ordinary scene effects while the action schedule continues.
    HoldEffects(u16),
    /// Initialize at creation before the ordinary effect update.
    InitializedEffect {
        appearance: crate::EffectAppearance,
        origin: AttackOrigin,
    },
    UnisonFeedback {
        duration: u16,
        limit: u8,
    },
    StageTint(crate::StageTint),
    SceneCamera(crate::SpellCamera),
    CameraDistance {
        distance: f32,
        duration: u16,
    },
    Cutin {
        catalogue: TechniqueId,
        duration: u16,
    },
    TemporaryWeapons(Vec<Arc<crate::WeaponDefinition>>),
    RestoreWeapons,
    Contact {
        definition: Arc<MeleeDefinition>,
        /// Number of active gameplay updates, including this event's update.
        duration: u16,
    },
    /// Change an active contact's definition and expiry while retaining its victims.
    ContinueContact {
        definition: Arc<MeleeDefinition>,
        duration: u16,
    },
    Throw(Arc<WeaponFlightDefinition>),
    Projectile(Arc<crate::ProjectileDefinition>),
    GroundProjectiles {
        definitions: [Arc<crate::ProjectileDefinition>; 2],
        retreat: f32,
    },
    HammerProjectile(Arc<HammerProjectile>),
    ProjectileShot(Arc<ProjectileShot>),
    /// Release independently of this attack; an occupied slot consumes the event silently.
    ReleaseSpell {
        definition: Arc<crate::PreparedSpell>,
        slot: crate::SpellSlot,
        effect: Option<crate::EffectAppearance>,
        tint: crate::effect::EffectTint,
        sound: Option<Sound>,
    },
    ProjectileAt {
        definition: Arc<crate::ProjectileDefinition>,
        origin: AttackOrigin,
        heading: Option<f32>,
        height: Option<f32>,
    },
    /// Fire inward from a randomly selected point on a ring around the live target.
    ProjectileRing {
        definition: Arc<crate::ProjectileDefinition>,
        radius: f32,
        directions: u16,
    },
    /// Choose one appearance and release on a world-space grid around the actor.
    ScatteredProjectile {
        choices: Arc<[Arc<crate::ProjectileDefinition>]>,
        offset: [f32; 3],
        step: [f32; 3],
        range: u16,
    },
    /// Burst once at the last melee victim's current ground position; misses do nothing.
    ContactBurst {
        definition: Arc<crate::ProjectileDefinition>,
        appearance: Option<crate::EffectAppearance>,
    },
    EffectAt {
        appearance: crate::EffectAppearance,
        origin: AttackOrigin,
        heading: Option<f32>,
        /// None adopts the actor's effect scale.
        scale: Option<f32>,
        follow: bool,
    },
    /// Select the single hammer once on paid entry: plain, ice, or poison.
    ChooseHammer([Arc<crate::ActionDefinition>; 3]),
    /// Select Grave Blade once on paid entry with Earth and at least 200 uses.
    ChooseGraveBlade([Arc<crate::ActionDefinition>; 2]),
    /// Select ground or airborne entry once after payment.
    ChooseAirborne([Arc<crate::ActionDefinition>; 2]),
    StealBonus(Arc<crate::ProjectileDefinition>),
    UseItem(u16),
    Steal(Arc<StealDefinition>),
    Seal(crate::SealDefinition),
    RecoveryDown {
        color: [u8; 3],
    },
    MarkCycloneTarget(crate::EffectAppearance),
    CycloneContact {
        definition: Arc<crate::ProjectileDefinition>,
        appearance: crate::EffectAppearance,
        opening: bool,
    },
    Drain {
        kind: crate::RecoveryKind,
        percent: u8,
    },
    Notice {
        duration: u16,
    },
    Trail {
        slot: WeaponSlot,
        duration: u16,
    },
    Voice {
        sound: Sound,
        priority: crate::VoicePriority,
    },
    Sound(Option<Sound>),
    Move {
        forward: Option<f32>,
        vertical: Option<f32>,
    },
    Acceleration(f32),
    Gravity(f32),
    Braking(f32),
    Turn(f32),
    TurnMotion(f32),
    FaceTarget,
    TraverseTarget {
        duration: u16,
        clearance: f32,
    },
    SetPosition([f32; 3]),
    PositionFromTarget {
        height: f32,
        retreat: f32,
    },
    OffsetPosition {
        height: f32,
        retreat: f32,
    },
    SecondaryWind {
        enabled: bool,
        duration: u16,
    },
    PrimaryWeapons(bool),
    Tint([u8; 3]),
    /// Defeat the owner without damage; released projectiles remain independent.
    Defeat {
        rewards: bool,
    },
    PassThrough {
        enabled: bool,
        /// Zero lasts until the action ends; otherwise shared actor updates count down.
        duration: u8,
    },
    SpecialGuard,
    Armor(u8),
    ReactionArmor(u16),
    StatUp {
        condition: crate::conditions::Condition,
        amount: i16,
    },
    AfflictOpponents {
        groups: Vec<Vec<crate::conditions::Condition>>,
        dispel: bool,
    },
    Stun,
    /// Pause event timing until melee contact, without extending a missed action.
    WaitForHit {
        deadline: u16,
        recovery: Option<AttackPose>,
    },
    WaitForEffectiveHit {
        deadline: u16,
    },
    ReleaseCaptured {
        hitstun: u16,
    },
    CapturedVisibility(bool),
    CapturedForward(f32),
    CapturedVertical(f32),
    /// Wait for the ground before consuming this event or those after it.
    Land {
        pose: Option<AttackPose>,
        effect: Option<crate::EffectAppearance>,
        /// Terminal landing tail; its deadline uses the existing action age.
        settle: Option<u16>,
        /// Pose selected at the terminal settling deadline.
        recovery_pose: Option<AttackPose>,
    },
    Pose(AttackPose),
    PartialPose {
        pose: AttackPose,
        bones: Vec<u16>,
    },
    PoseKeepingParts(AttackPose),
    AnimationRate(f32),
    CarriedPose {
        slot: WeaponSlot,
        clip: u16,
        rate: f32,
        repeat: bool,
    },
    WeaponVisibility(Vec<(Option<WeaponSlot>, bool)>),
    Effect {
        appearance: crate::EffectAppearance,
        tints: [crate::effect::EffectTint; 10],
        /// Attach to the body center instead of the actor root.
        centered: bool,
    },
}

/// Resolve the live participant or target transform at the event's gameplay age.
#[derive(Debug, Clone, Copy)]
pub enum AttackOrigin {
    World([f32; 3]),
    ActorRoot,
    ActorAim,
    TargetRoot,
    TargetAim,
    Bone(crate::MeleeAnchor),
}

impl AttackOrigin {
    fn resolve(
        self,
        battle: &Battle,
        actor: crate::ActorId,
        target: crate::ActorId,
    ) -> Result<([f32; 3], Option<crate::EffectFollow>)> {
        Ok(match self {
            Self::World(position) => (position, None),
            Self::ActorRoot => (
                battle.actor(actor)?.position,
                Some(crate::EffectFollow::Actor(actor)),
            ),
            Self::ActorAim => (
                battle.actor(actor)?.effect_origin(),
                Some(crate::EffectFollow::Center(actor)),
            ),
            Self::TargetRoot => (
                battle.actor(target)?.position,
                Some(crate::EffectFollow::Actor(target)),
            ),
            Self::TargetAim => (
                battle.actor(target)?.effect_origin(),
                Some(crate::EffectFollow::Center(target)),
            ),
            Self::Bone(anchor) => (
                battle
                    .models
                    .as_ref()
                    .context("action effect has no model")?
                    .anchor(actor, anchor)?,
                None,
            ),
        })
    }
}

#[derive(Debug, Clone)]
pub struct StealDefinition {
    pub gald: bool,
    pub success: Arc<crate::ActionDefinition>,
    pub success_voice: Option<Sound>,
    pub failure_voice: Option<Sound>,
}

#[derive(Debug, Clone)]
pub struct PreparedAttack {
    pub contact_recovery: Option<crate::ContactRecovery>,
    /// Technique chaining opens at this age; ordinary combos come from the controller.
    pub chain_at: Option<u16>,
    /// Gameplay age at which recovery may begin, after the last event.
    pub end_at: u16,
    /// None keeps the actor's current pose.
    pub opening: Option<AttackPose>,
    /// Ordered by gameplay age. Hit-stop pauses the clock; animation does not.
    pub events: Vec<(u16, AttackEvent)>,
    pub recovery: u16,
}

impl PreparedAttack {
    /// An attack with no events or recovery delay; callers supply its gameplay duration.
    pub fn new(end_at: u16) -> Self {
        Self {
            contact_recovery: None,
            chain_at: None,
            end_at,
            opening: None,
            events: Vec::new(),
            recovery: 0,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(recovery) = self.contact_recovery {
            recovery.validate()?;
        }
        ensure!(
            self.events.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "unordered attack events"
        );
        ensure!(
            self.chain_at.is_none_or(|at| at <= self.end_at),
            "attack chain opens after recovery"
        );
        let mut contact_end = None;
        for (index, (at, event)) in self.events.iter().enumerate() {
            match event {
                AttackEvent::Contact {
                    definition,
                    duration,
                }
                | AttackEvent::ContinueContact {
                    definition,
                    duration,
                } => {
                    definition.validate()?;
                    ensure!(*duration > 0, "empty melee window");
                    if matches!(event, AttackEvent::ContinueContact { .. }) {
                        ensure!(
                            contact_end.is_some_and(|end| u32::from(*at) < end),
                            "contact continuation has no active window"
                        );
                    } else {
                        ensure!(
                            contact_end.is_none_or(|end| u32::from(*at) >= end),
                            "overlapping attack contacts"
                        );
                    }
                    contact_end = Some(u32::from(*at) + u32::from(*duration));
                }
                AttackEvent::WaitForHit { deadline, .. }
                | AttackEvent::WaitForEffectiveHit { deadline } => ensure!(
                    *at <= *deadline && *deadline <= self.end_at,
                    "contact wait deadline outside attack"
                ),
                AttackEvent::Land {
                    settle: Some(_), ..
                } => {
                    ensure!(
                        self.events[index + 1..].iter().all(|(_, event)| matches!(
                            event,
                            AttackEvent::PassThrough { enabled: false, .. }
                        )),
                        "settling landing only permits collision cleanup afterward"
                    );
                }
                AttackEvent::Move {
                    forward, vertical, ..
                } => ensure!(
                    forward.is_none_or(f32::is_finite) && vertical.is_none_or(f32::is_finite),
                    "invalid attack movement"
                ),
                AttackEvent::ChooseHammer(_)
                | AttackEvent::ChooseGraveBlade(_)
                | AttackEvent::ChooseAirborne(_) => {
                    let variants: &[Arc<crate::ActionDefinition>] = match event {
                        AttackEvent::ChooseHammer(variants) => variants,
                        AttackEvent::ChooseGraveBlade(variants) => variants,
                        AttackEvent::ChooseAirborne(variants) => variants,
                        _ => unreachable!(),
                    };
                    ensure!(
                        *at == 0 && self.events.len() == 1,
                        "attack choice must be the entry event"
                    );
                    for variant in variants {
                        let ActionExecution::Attack(attack) = &variant.execution else {
                            anyhow::bail!("attack choice requires attacks");
                        };
                        ensure!(
                            attack.events.iter().all(|(_, event)| !matches!(
                                event,
                                AttackEvent::ChooseHammer(_)
                                    | AttackEvent::ChooseGraveBlade(_)
                                    | AttackEvent::ChooseAirborne(_)
                            )),
                            "nested attack choice"
                        );
                        attack.validate()?;
                    }
                }
                AttackEvent::Steal(recipe) => {
                    let ActionExecution::Attack(success) = &recipe.success.execution else {
                        anyhow::bail!("steal success requires an attack");
                    };
                    ensure!(
                        success.events.iter().all(|(age, event)| *age >= *at
                            && !matches!(
                                event,
                                AttackEvent::Steal(_)
                                    | AttackEvent::StealBonus(_)
                                    | AttackEvent::ChooseHammer(_)
                                    | AttackEvent::ChooseGraveBlade(_)
                                    | AttackEvent::ChooseAirborne(_)
                            )),
                        "invalid steal success continuation"
                    );
                    success.validate()?;
                }
                AttackEvent::Projectile(projectile)
                | AttackEvent::StealBonus(projectile)
                | AttackEvent::ContactBurst {
                    definition: projectile,
                    ..
                }
                | AttackEvent::CycloneContact {
                    definition: projectile,
                    ..
                } => projectile.validate()?,
                AttackEvent::GroundProjectiles {
                    definitions,
                    retreat,
                } => {
                    ensure!(
                        retreat.is_finite() && *retreat >= 0.,
                        "invalid ground projectile retreat"
                    );
                    for definition in definitions {
                        definition.validate()?;
                    }
                }
                AttackEvent::ReleaseSpell { definition, .. } => definition.validate()?,
                AttackEvent::HammerProjectile(definition) => definition.validate()?,
                AttackEvent::ProjectileShot(definition) => definition.validate()?,
                AttackEvent::Seal(definition) => definition.validate()?,
                AttackEvent::Drain { percent, .. } => ensure!(
                    (1..=100).contains(percent),
                    "invalid contact drain percentage"
                ),
                AttackEvent::StatUp { condition, amount } => ensure!(
                    *amount >= 0
                        && matches!(
                            condition,
                            crate::conditions::Condition::AttackUp
                                | crate::conditions::Condition::DefenseUp
                        ),
                    "unsupported action stat increase"
                ),
                AttackEvent::AfflictOpponents { groups, .. } => ensure!(
                    !groups.is_empty() && groups.iter().all(|group| !group.is_empty()),
                    "empty opponent affliction choices"
                ),
                AttackEvent::ProjectileAt {
                    definition,
                    heading,
                    height,
                    ..
                } => {
                    definition.validate()?;
                    ensure!(
                        heading.is_none_or(f32::is_finite) && height.is_none_or(f32::is_finite),
                        "invalid targeted projectile transform"
                    );
                }
                AttackEvent::ProjectileRing {
                    definition,
                    radius,
                    directions,
                } => {
                    definition.validate()?;
                    ensure!(
                        radius.is_finite() && *radius > 0. && *directions > 0,
                        "invalid projectile ring"
                    );
                }
                AttackEvent::ScatteredProjectile {
                    choices,
                    offset,
                    step,
                    range,
                } => {
                    ensure!(
                        !choices.is_empty()
                            && *range > 0
                            && offset.iter().chain(step).all(|value| value.is_finite()),
                        "invalid projectile scatter"
                    );
                    for definition in choices.iter() {
                        definition.validate()?;
                    }
                }
                AttackEvent::EffectAt { heading, scale, .. } => {
                    ensure!(
                        heading.is_none_or(f32::is_finite)
                            && scale.is_none_or(|v| v.is_finite() && v > 0.),
                        "invalid targeted effect transform"
                    );
                }
                AttackEvent::Acceleration(value)
                | AttackEvent::Gravity(value)
                | AttackEvent::Turn(value)
                | AttackEvent::TurnMotion(value)
                | AttackEvent::CapturedForward(value)
                | AttackEvent::CapturedVertical(value) => {
                    ensure!(value.is_finite(), "invalid attack acceleration")
                }
                AttackEvent::SetPosition(value) => {
                    ensure!(
                        value.iter().all(|v| v.is_finite()),
                        "invalid action position"
                    )
                }
                AttackEvent::TraverseTarget {
                    duration,
                    clearance,
                } => ensure!(
                    *duration > 0 && clearance.is_finite() && *clearance > 0.,
                    "invalid target traversal"
                ),
                AttackEvent::PositionFromTarget { height, retreat }
                | AttackEvent::OffsetPosition { height, retreat } => ensure!(
                    height.is_finite() && retreat.is_finite(),
                    "invalid action position offset"
                ),
                AttackEvent::Braking(value) => {
                    ensure!(value.is_finite() && *value >= 0., "invalid attack braking")
                }
                AttackEvent::Throw(definition) => definition.validate()?,
                AttackEvent::CarriedPose { rate, .. } => ensure!(
                    rate.is_finite() && (0.01..=4.).contains(rate),
                    "invalid carried animation rate"
                ),
                AttackEvent::UseItem(item) => ensure!(*item != 0, "empty enemy item identity"),
                AttackEvent::Voice { .. }
                | AttackEvent::FaceTarget
                | AttackEvent::Notice { .. }
                | AttackEvent::Trail { .. }
                | AttackEvent::Pose(_)
                | AttackEvent::PartialPose { .. }
                | AttackEvent::PoseKeepingParts(_)
                | AttackEvent::AnimationRate(_)
                | AttackEvent::WeaponVisibility(_)
                | AttackEvent::PrimaryWeapons(_)
                | AttackEvent::Tint(_)
                | AttackEvent::RecoveryDown { .. }
                | AttackEvent::MarkCycloneTarget(_)
                | AttackEvent::Defeat { .. }
                | AttackEvent::Stun
                | AttackEvent::PassThrough { .. }
                | AttackEvent::SpecialGuard
                | AttackEvent::Armor(_)
                | AttackEvent::ReactionArmor(_)
                | AttackEvent::ReleaseCaptured { .. }
                | AttackEvent::CapturedVisibility(_)
                | AttackEvent::SecondaryWind { .. }
                | AttackEvent::Land { .. }
                | AttackEvent::Sound(_)
                | AttackEvent::Effect { .. }
                | AttackEvent::ActionEffect { .. }
                | AttackEvent::InitializedActionEffect { .. }
                | AttackEvent::InitializedEffect { .. } => {}
                AttackEvent::HoldEffects(duration) => ensure!(*duration != 0, "empty effect hold"),
                AttackEvent::UnisonFeedback { .. } => {}
                AttackEvent::CameraDistance { distance, duration } => ensure!(
                    distance.is_finite() && *distance > 0. && *duration != 0,
                    "invalid attack camera distance"
                ),
                AttackEvent::StageTint(_)
                | AttackEvent::SceneCamera(_)
                | AttackEvent::Cutin { .. }
                | AttackEvent::TemporaryWeapons(_)
                | AttackEvent::RestoreWeapons => {}
            }
        }
        Ok(())
    }
}

fn steal_roll(luck: u16, random: &mut crate::Random) -> u16 {
    (random.next_u16() % 100).saturating_sub(luck / 10)
}

fn steal_bonus_count(uses: u16, luck: u16, random: &mut crate::Random) -> u16 {
    const EXPERIENCED_USES: u16 = 200;
    // Paid attempts consume this draw before the use threshold is checked.
    let chance = steal_roll(luck, random);
    if uses >= EXPERIENCED_USES && chance < 10 {
        random.next_u16() % 3 + 1
    } else {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HammerVariant {
    Ordinary,
    Ice,
    Toss,
}

impl HammerVariant {
    pub(crate) fn notice(self) -> Option<&'static str> {
        match self {
            Self::Ordinary => None,
            Self::Ice => Some("Ice Hammer"),
            Self::Toss => Some("Toss Hammer"),
        }
    }
}

pub(crate) fn hammer_choice(
    uses: u16,
    luck: u16,
    element: Option<crate::Element>,
    random: &mut crate::Random,
) -> HammerVariant {
    const ALTERNATE_USES: u16 = 200;
    if uses < ALTERNATE_USES {
        HammerVariant::Ordinary
    } else if random.next_u16() % 100 < luck / 8 {
        HammerVariant::Toss
    } else if element == Some(crate::Element::Ice) {
        HammerVariant::Ice
    } else {
        HammerVariant::Ordinary
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttackRun {
    Starting,
    Active { next_event: usize, delay: u32 },
    Settling { recover_at: u32 },
}

fn advance_contact(
    id: ActionId,
    sequence: &mut crate::action::Sequence,
    contacts: &mut crate::contact::Contacts,
    cues: &mut Vec<Cue>,
    highlighted_trail: bool,
    waiting: bool,
) -> Result<()> {
    let actor = sequence.actor;
    if let Some(window) = &mut sequence.melee {
        if let Some(slot) = window.definition.trail {
            cues.push(Cue::WeaponTrail {
                actor,
                slot,
                duration: 1,
                highlighted: highlighted_trail,
            });
        }
        if waiting {
            contacts.melee_at(
                actor,
                id,
                &window.definition,
                &window.struck,
                Some(&window.positions),
            )?;
        } else {
            window.positions.clear();
            contacts.melee(actor, id, &window.definition, &window.struck)?;
        }
        if !waiting {
            window.remaining -= 1;
            if window.remaining == 0 {
                sequence.melee = None;
            }
        }
    }
    Ok(())
}

/// A captured endpoint keeps the pass readable even while the victim recoils.
/// Integration still belongs to ordinary actor movement, collision and hit-stop.
pub(crate) struct Traversal {
    started_at: u32,
    duration: u16,
    start: [f32; 3],
    end: [f32; 3],
}

impl Traversal {
    fn finished(&self, age: u32) -> bool {
        age.saturating_sub(self.started_at) >= u32::from(self.duration)
    }

    fn advance(&self, age: u32, actor: &mut crate::Actor) {
        let elapsed = age.saturating_sub(self.started_at);
        actor.movement.acceleration = 0.;
        actor.movement.braking = 0.;
        let t = (elapsed + 1) as f32 / f32::from(self.duration);
        let weight = t * t * (3. - 2. * t);
        let desired = [
            self.start[0] + (self.end[0] - self.start[0]) * weight,
            actor.position[1],
            self.start[2] + (self.end[2] - self.start[2]) * weight,
        ];
        let delta = [
            desired[0] - actor.position[0],
            0.,
            desired[2] - actor.position[2],
        ];
        let distance = (delta[0] * delta[0] + delta[2] * delta[2]).sqrt();
        if distance > 0.001 {
            actor.movement.direction = [delta[0] / distance, 0., delta[2] / distance];
        }
        actor.movement.forward = distance;
    }
}

pub(crate) fn step(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut crate::action::Sequence,
    run: AttackRun,
    contacts: &mut crate::contact::Contacts,
    items: &mut dyn crate::item::Provider,
    cues: &mut Vec<Cue>,
) -> Result<Option<u32>> {
    let definition = Arc::clone(&sequence.definition);
    let ActionExecution::Attack(attack) = &definition.execution else {
        unreachable!("attack lifecycle needs a prepared attack")
    };
    let actor = sequence.actor;
    let index = actor.index();
    if run == AttackRun::Starting
        && let Some((0, AttackEvent::ChooseAirborne(variants))) = attack.events.first()
    {
        sequence.definition = Arc::clone(&variants[usize::from(battle.actors[index].airborne())]);
        return step(battle, id, sequence, run, contacts, items, cues);
    }
    if run == AttackRun::Starting
        && let Some((0, AttackEvent::ChooseGraveBlade(variants))) = attack.events.first()
    {
        let uses = battle
            .technique_catalogue_for_action(actor, sequence.action)
            .and_then(|catalogue| battle.technique_uses(actor, catalogue))
            .unwrap_or(0);
        let element =
            crate::damage::resolve_element(crate::HitElement::Inherited, &battle.actors[index]);
        let grave = uses >= 200 && element == Some(crate::Element::Earth);
        if grave {
            cues.push(Cue::EffectNotice {
                actor,
                text: "Grave Blade".into(),
                duration: 90,
            });
        }
        sequence.definition = Arc::clone(&variants[usize::from(grave)]);
        return step(battle, id, sequence, run, contacts, items, cues);
    }
    if run == AttackRun::Starting
        && let Some((0, AttackEvent::ChooseHammer(variants))) = attack.events.first()
    {
        let uses = battle
            .technique_catalogue_for_action(actor, sequence.action)
            .and_then(|catalogue| battle.technique_uses(actor, catalogue))
            .unwrap_or(0);
        let owner = &battle.actors[index];
        let element = crate::damage::resolve_element(crate::HitElement::Inherited, owner);
        let choice = hammer_choice(uses, owner.equipment.luck, element, &mut battle.random);
        if let Some(text) = choice.notice() {
            cues.push(Cue::EffectNotice {
                actor,
                text: text.into(),
                duration: 90,
            });
        }
        sequence.definition = Arc::clone(&variants[choice as usize]);
        return step(battle, id, sequence, run, contacts, items, cues);
    }
    let highlighted_trail = battle
        .prepared
        .technique(actor, sequence.action)
        .is_some_and(|row| row.capabilities.spell || row.capabilities.family.is_some());
    let combo_recovery = if sequence.normal.is_some() && !battle.actors[index].airborne() {
        u32::from(battle.runtime[index].combo.normal_links) * RECOVERY_PER_LINK
    } else {
        0
    };
    let recovery = u32::from(attack.recovery) + combo_recovery;
    if sequence
        .traversal
        .as_ref()
        .is_some_and(|traversal| traversal.finished(sequence.age))
    {
        // Release the pass before this tick's authored movement takes ownership.
        sequence.traversal = None;
        battle.actors[index].movement.forward = 0.;
    }
    let (mut next_event, mut delay) = match run {
        AttackRun::Starting => {
            let movement = &mut battle.actors[index].movement;
            movement.braking = ATTACK_BRAKING;
            movement.gravity = if movement.flying {
                0.
            } else {
                crate::movement::GRAVITY
            };

            if let Some(pose) = attack.opening {
                pose.play(battle, index);
            }
            (0, 0)
        }
        AttackRun::Active { next_event, delay } => (next_event, delay),
        AttackRun::Settling { recover_at } => {
            sequence.execution = crate::action::Execution::Attack(run);
            let landing = attack
                .events
                .iter()
                .position(|(_, event)| {
                    matches!(
                        event,
                        AttackEvent::Land {
                            settle: Some(_),
                            ..
                        }
                    )
                })
                .expect("settling attack has a landing event");
            for (at, _) in &attack.events[landing + 1..] {
                if sequence.age >= u32::from(*at) {
                    battle.runtime[index].body_bypass = false;
                    battle.runtime[index].body_bypass_ticks = 0;
                }
            }
            if sequence.age < recover_at {
                advance_contact(id, sequence, contacts, cues, highlighted_trail, false)?;
                return Ok(None);
            }
            if let AttackEvent::Land {
                recovery_pose: Some(pose),
                ..
            } = &attack.events[landing].1
            {
                pose.play(battle, index);
            }
            return Ok(Some(recovery));
        }
    };
    if sequence
        .contact_blend_ends
        .is_some_and(|end| sequence.age >= end)
    {
        sequence.contact_blend_ends = None;
        if sequence.melee.is_some()
            || attack.events[next_event..].iter().any(|(_, event)| {
                matches!(
                    event,
                    AttackEvent::Contact { .. } | AttackEvent::ContinueContact { .. }
                )
            })
        {
            sequence.contact_target = None;
            if let Some(window) = &mut sequence.melee {
                window.struck.clear();
            }
        }
    }
    // A delayed continuation must not push pending movement past the action deadline.
    // An already-entered landing settlement keeps its own outer-age deadline above.
    if delay != 0 && sequence.age >= u32::from(attack.end_at) {
        if let Some(pose) = attack.events.iter().find_map(|(_, event)| match event {
            AttackEvent::WaitForHit { recovery, .. } => *recovery,
            _ => None,
        }) {
            pose.play(battle, index);
        }
        sequence.melee = None;
        return Ok(Some(recovery));
    }
    let event_age = sequence.age - delay;
    let mut waiting = false;
    let mut defeat = None;
    while let Some((at, event)) = attack.events.get(next_event) {
        if u32::from(*at) > event_age || defeat.is_some_and(|(end, _)| *at > end) {
            break;
        }
        match event {
            AttackEvent::UseItem(item) => {
                battle.release_enemy_item(actor, sequence.target, *item, cues)?;
            }
            AttackEvent::WaitForHit { deadline, .. }
            | AttackEvent::WaitForEffectiveHit { deadline } => {
                let (hit, pose) = match event {
                    AttackEvent::WaitForHit { recovery, .. } => {
                        (sequence.contact_target, *recovery)
                    }
                    _ => (sequence.struck_target, None),
                };
                if hit.is_none() {
                    if sequence.age >= u32::from(*deadline) {
                        if let Some(pose) = pose {
                            pose.play(battle, index);
                        }
                        sequence.melee = None;
                        return Ok(Some(recovery));
                    }
                    delay += 1;
                    waiting = true;
                    break;
                }
            }
            AttackEvent::ReleaseCaptured { hitstun } => {
                battle.request_capture_release(actor, *hitstun);
            }
            AttackEvent::CapturedVisibility(visible) => {
                battle.captured_visibility(actor, *visible);
            }
            AttackEvent::CapturedForward(value) => {
                battle.captured_velocity(actor, Some(*value), None);
            }
            AttackEvent::CapturedVertical(value) => {
                battle.captured_velocity(actor, None, Some(*value));
            }
            AttackEvent::ActionEffect { appearance, origin }
            | AttackEvent::InitializedActionEffect { appearance, origin }
            | AttackEvent::InitializedEffect { appearance, origin } => {
                let (position, follow) = origin.resolve(battle, actor, sequence.target)?;
                let owner = &battle.actors[index];
                let request = crate::EffectRequest {
                    owner: actor,
                    target: sequence.target,
                    appearance: *appearance,
                    origin: position,
                    heading: owner.heading,
                    follow,
                    scale: 1.,
                    tint: Default::default(),
                };
                cues.push(if matches!(event, AttackEvent::InitializedEffect { .. }) {
                    if let AttackOrigin::Bone(anchor) = origin {
                        Cue::InitializedBoneEffect {
                            request,
                            anchor: *anchor,
                        }
                    } else {
                        Cue::InitializedEffect(request)
                    }
                } else if matches!(event, AttackEvent::InitializedActionEffect { .. }) {
                    Cue::InitializedActionEffect {
                        action: id,
                        request,
                    }
                } else {
                    Cue::ActionEffect {
                        action: id,
                        request,
                    }
                });
            }
            AttackEvent::HoldEffects(duration) => cues.push(Cue::HoldEffects(*duration)),
            AttackEvent::UnisonFeedback { duration, limit } => {
                battle.unison_feedback(*duration, *limit)
            }
            AttackEvent::StageTint(tint) => cues.push(Cue::StageTint(*tint)),
            AttackEvent::SceneCamera(request) => {
                if let Some(camera) = &mut battle.camera {
                    camera.request_scene(*request);
                }
            }
            AttackEvent::CameraDistance { distance, duration } => {
                if let Some(camera) = &mut battle.camera {
                    camera.request_distance(*distance, *duration);
                }
            }
            AttackEvent::Cutin {
                catalogue,
                duration,
            } => cues.push(Cue::Cutin {
                action: id,
                catalogue: *catalogue,
                duration: *duration,
            }),
            AttackEvent::TemporaryWeapons(definitions) => battle
                .models
                .as_mut()
                .context("temporary weapon action has no models")?
                .override_action_weapons(actor, id, definitions)?,
            AttackEvent::RestoreWeapons => battle
                .models
                .as_mut()
                .context("temporary weapon action has no models")?
                .restore_action_weapons(actor, id),
            AttackEvent::Contact {
                definition: contact,
                duration,
            } => {
                sequence.melee = Some(crate::melee::Window {
                    definition: Arc::clone(contact),
                    remaining: *duration,
                    positions: Vec::new(),
                    struck: Vec::new(),
                });
            }
            AttackEvent::ContinueContact {
                definition,
                duration,
            } => {
                let window = sequence
                    .melee
                    .as_mut()
                    .context("contact continuation has no active window")?;
                window.definition = Arc::clone(definition);
                window.remaining = *duration;
            }
            AttackEvent::ChooseHammer(_)
            | AttackEvent::ChooseGraveBlade(_)
            | AttackEvent::ChooseAirborne(_) => {
                unreachable!("attack variant is selected before its event loop")
            }
            AttackEvent::SecondaryWind { enabled, duration } => {
                battle.actors[index].secondary_wind.set(*enabled, *duration);
            }
            AttackEvent::StealBonus(projectile) => {
                let uses = battle
                    .technique_catalogue_for_action(actor, sequence.action)
                    .and_then(|catalogue| battle.technique_uses(actor, catalogue))
                    .unwrap_or(0);
                let count = steal_bonus_count(
                    uses,
                    battle.actors[index].equipment.luck,
                    &mut battle.random,
                );
                for _ in 0..count {
                    battle.emit(
                        Arc::clone(projectile),
                        id,
                        actor,
                        sequence.target,
                        battle.actors[index].position,
                    )?;
                }
            }
            AttackEvent::Steal(recipe) => {
                let owner = &battle.actors[index];
                let luck = owner.equipment.luck;
                let guaranteed = owner.equipment.guaranteed_steal;
                let item = match sequence.struck_target {
                    Some(target) => items.steal(crate::item::StealRequest {
                        user: actor,
                        target,
                        guaranteed,
                        gald: recipe.gald,
                        adjusted_roll: steal_roll(luck, &mut battle.random),
                    })?,
                    None => None,
                };
                if let Some(name) = item {
                    battle.actors[index].reaction.protection.armor(60);
                    if let Some(voice) = recipe.success_voice {
                        battle.request_voice(actor, voice, crate::VoicePriority::Action);
                    }
                    cues.push(Cue::EffectNotice {
                        actor,
                        text: format!("Stole {name}"),
                        duration: 90,
                    });
                    sequence.definition = Arc::clone(&recipe.success);
                    sequence.combo = None;
                    sequence.melee = None;
                    return step(
                        battle,
                        id,
                        sequence,
                        AttackRun::Active {
                            next_event: 0,
                            delay: 0,
                        },
                        contacts,
                        items,
                        cues,
                    );
                }
                if battle.random.next_u16() & 1 != 0
                    && let Some(voice) = recipe.failure_voice
                {
                    battle.request_voice(actor, voice, crate::VoicePriority::Action);
                }
            }
            AttackEvent::Seal(definition) => {
                battle.attempt_seal(
                    actor,
                    sequence.struck_target,
                    *definition,
                    battle.actors[index].equipment.contact.prolong,
                    crate::ContactSource::Melee { actor, action: id },
                    cues,
                )?;
            }
            AttackEvent::Drain { kind, percent } => {
                battle.drain_contact(actor, sequence.struck_damage, *kind, *percent, cues)?;
            }
            AttackEvent::RecoveryDown { color } => {
                battle.apply_recovery_down(sequence.target, *color, cues)?;
            }
            AttackEvent::MarkCycloneTarget(appearance) => {
                let [x, _, z] = battle.actors[sequence.target.index()].position;
                let origin = [x, 0., z];
                sequence.cyclone_target = Some((sequence.target, origin));
                cues.push(Cue::InitializedEffect(crate::EffectRequest {
                    owner: actor,
                    target: sequence.target,
                    appearance: *appearance,
                    origin,
                    heading: 0.,
                    scale: 1.,
                    follow: None,
                    tint: Default::default(),
                }));
            }
            AttackEvent::CycloneContact {
                definition,
                appearance,
                opening,
            } => {
                if let Some((target, marked)) = sequence.cyclone_target {
                    let recipient = &battle.actors[target.index()];
                    const CAPTURE_RADIUS: f32 = 200.;
                    let dx = recipient.position[0] - marked[0];
                    let dz = recipient.position[2] - marked[2];
                    if *opening
                        && (battle.prepared.actor_setup[target.index()].resists_cyclone
                            || recipient.airborne()
                            || dx * dx + dz * dz >= CAPTURE_RADIUS * CAPTURE_RADIUS)
                    {
                        sequence.cyclone_target = None;
                    } else {
                        let origin = battle.actor_emission_centers[target.index()];
                        battle.emit_at(
                            Arc::clone(definition),
                            id,
                            actor,
                            target,
                            origin,
                            battle.actors[index].heading,
                        )?;
                        cues.push(Cue::InitializedEffect(crate::EffectRequest {
                            owner: actor,
                            target,
                            appearance: *appearance,
                            origin,
                            heading: 0.,
                            scale: 1.,
                            follow: Some(crate::EffectFollow::Center(target)),
                            tint: Default::default(),
                        }));
                    }
                }
            }
            AttackEvent::Throw(flight) => battle.throw_weapon(actor, id, Arc::clone(flight)),
            AttackEvent::Projectile(projectile) => {
                battle.emit(
                    Arc::clone(projectile),
                    id,
                    actor,
                    sequence.target,
                    battle.actors[index].position,
                )?;
            }
            AttackEvent::HammerProjectile(definition) => {
                definition.emit(battle, id, actor, sequence.target)?;
            }
            AttackEvent::GroundProjectiles {
                definitions,
                retreat,
            } => {
                let owner = &battle.actors[index];
                let heading = owner.heading;
                let mut targets: Vec<_> = battle
                    .actors
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| {
                        candidate.side != owner.side && candidate.available() && candidate.hp > 0
                    })
                    .map(|(index, _)| crate::ActorId(index as u8))
                    .collect();
                let offset = crate::geometry::rotate([0., 0., -*retreat], heading);
                for definition in definitions {
                    if targets.is_empty() {
                        break;
                    }
                    #[cfg(any(test, feature = "test-support"))]
                    let choice = battle
                        .random_inputs
                        .as_mut()
                        .map(|inputs| {
                            inputs.target(
                                battle.ledger.elapsed_ticks,
                                actor,
                                id,
                                battle.next_projectile,
                                &targets,
                            )
                        })
                        .transpose()?
                        .flatten()
                        .unwrap_or_else(|| usize::from(battle.random.next_u16()) % targets.len());
                    #[cfg(not(any(test, feature = "test-support")))]
                    let choice = usize::from(battle.random.next_u16()) % targets.len();
                    let target = targets.swap_remove(choice);
                    let [x, _, z] = battle.actors[target.index()].position;
                    battle.emit_at(
                        Arc::clone(definition),
                        id,
                        actor,
                        target,
                        [x + offset[0], 0., z + offset[2]],
                        heading,
                    )?;
                }
            }
            AttackEvent::ProjectileShot(definition) => {
                definition.emit(battle, id, actor, sequence.target)?;
            }
            AttackEvent::ReleaseSpell {
                definition,
                slot,
                effect,
                tint,
                sound,
            } => {
                if let Some(resident) = battle.release_volley(
                    Arc::clone(definition),
                    actor,
                    sequence.target,
                    *slot,
                    Some(id),
                    cues,
                )? {
                    battle.actors[index].casting_state.power_percent = 100;
                    battle.volleys.get_mut(&resident).unwrap().cast_pose =
                        Some(crate::release::CastPoseTransition {
                            parent: id,
                            action: sequence.action,
                            opening: None,
                            release: None,
                            release_loop_start: 0.,
                            at: 0,
                        });
                    let owner = &battle.actors[index];
                    if let Some(appearance) = effect {
                        let request = crate::EffectRequest {
                            owner: actor,
                            target: sequence.target,
                            appearance: *appearance,
                            origin: owner.effect_origin(),
                            heading: owner.heading,
                            follow: Some(crate::EffectFollow::Center(actor)),
                            scale: owner.effect_scale,
                            tint: *tint,
                        };
                        cues.push(if definition.transition_ticks > 0 {
                            Cue::InitializedEffect(request)
                        } else {
                            Cue::Effect(request)
                        });
                    }
                    if let Some(sound) = sound {
                        cues.push(Cue::Sound {
                            actor,
                            sound: *sound,
                            position: owner.position,
                            priority: 1,
                        });
                    }
                }
            }
            AttackEvent::ProjectileAt {
                definition,
                origin,
                heading,
                height,
            } => {
                let (mut position, _) = origin.resolve(battle, actor, sequence.target)?;
                if let Some(height) = height {
                    position[1] = *height;
                }
                battle.emit_at(
                    Arc::clone(definition),
                    id,
                    actor,
                    sequence.target,
                    position,
                    heading.unwrap_or(battle.actors[index].heading),
                )?;
            }
            AttackEvent::ProjectileRing {
                definition,
                radius,
                directions,
            } => {
                let heading = f32::from(battle.random.next_u16() % directions)
                    * (360. / f32::from(*directions));
                let (sin, cos) = heading.to_radians().sin_cos();
                let [x, y, z] = battle.actor(sequence.target)?.position;
                battle.emit_at(
                    Arc::clone(definition),
                    id,
                    actor,
                    sequence.target,
                    [x - sin * radius, y, z - cos * radius],
                    heading,
                )?;
            }
            AttackEvent::ScatteredProjectile {
                choices,
                offset,
                step,
                range,
            } => {
                let owner = battle.actor(actor)?;
                let origin = owner.position;
                let heading = owner.heading;
                let position = std::array::from_fn(|axis| {
                    let sample = i32::from(battle.random.next_u16() as i16) % i32::from(*range);
                    origin[axis] + offset[axis] + step[axis] * sample as f32
                });
                let choice = usize::from(battle.random.next_u16()) % choices.len();
                battle.emit_at(
                    Arc::clone(&choices[choice]),
                    id,
                    actor,
                    sequence.target,
                    position,
                    heading,
                )?;
            }
            AttackEvent::ContactBurst {
                definition,
                appearance,
            } => {
                if let Some(victim) = sequence.contact_target {
                    let [x, _, z] = battle.actor(victim)?.position;
                    let origin = [x, 0., z];
                    let heading = battle.actors[index].heading;
                    battle.emit_at(
                        Arc::clone(definition),
                        id,
                        actor,
                        sequence.target,
                        origin,
                        heading,
                    )?;
                    if let Some(appearance) = appearance {
                        cues.push(Cue::InitializedEffect(crate::EffectRequest {
                            owner: actor,
                            target: victim,
                            appearance: *appearance,
                            origin,
                            heading,
                            scale: 1.,
                            follow: None,
                            tint: Default::default(),
                        }));
                    }
                }
            }
            AttackEvent::EffectAt {
                appearance,
                origin,
                heading,
                scale,
                follow,
            } => {
                let (position, attachment) = origin.resolve(battle, actor, sequence.target)?;
                cues.push(Cue::Effect(crate::EffectRequest {
                    owner: actor,
                    target: sequence.target,
                    appearance: *appearance,
                    origin: position,
                    heading: heading.unwrap_or(battle.actors[index].heading),
                    scale: scale.unwrap_or(battle.actors[index].effect_scale),
                    follow: if *follow { attachment } else { None },
                    tint: Default::default(),
                }));
            }
            AttackEvent::Notice { duration } => cues.push(Cue::Notice {
                actor,
                action: sequence.action,
                duration: *duration,
            }),
            AttackEvent::Trail { slot, duration } => {
                cues.push(Cue::WeaponTrail {
                    actor,
                    slot: *slot,
                    duration: *duration,
                    highlighted: highlighted_trail,
                });
            }
            AttackEvent::Voice { sound, priority } => {
                battle.request_voice(actor, *sound, *priority);
            }
            AttackEvent::Sound(sound) => {
                if let Some(sound) = sound {
                    cues.push(Cue::Sound {
                        actor,
                        sound: *sound,
                        position: battle.actors[index].position,
                        priority: 1,
                    });
                }
            }
            AttackEvent::Move { forward, vertical } => {
                let movement = &mut battle.actors[index].movement;
                if let Some(forward) = forward {
                    movement.forward = *forward;
                }
                if let Some(vertical) = vertical {
                    movement.vertical = *vertical;
                }
            }
            AttackEvent::Acceleration(value) => battle.actors[index].movement.acceleration = *value,
            AttackEvent::Gravity(value) => battle.actors[index].movement.gravity = *value,
            AttackEvent::Braking(value) => battle.actors[index].movement.braking = *value,
            AttackEvent::Turn(degrees) => {
                let owner = &mut battle.actors[index];
                let [x, y, z] = owner.facing_direction;
                let (sin, cos) = degrees.to_radians().sin_cos();
                owner.heading += *degrees;
                owner.facing_direction = [x * cos + z * sin, y, z * cos - x * sin];
                owner.movement.direction = owner.facing_direction;
            }
            AttackEvent::TurnMotion(degrees) => {
                let owner = &mut battle.actors[index];
                owner.heading += *degrees;
                let [x, y, z] = owner.movement.target_direction;
                let (sin, cos) = degrees.to_radians().sin_cos();
                owner.movement.direction = [x * cos + z * sin, y, z * cos - x * sin];
            }
            AttackEvent::FaceTarget => {
                let owner = &mut battle.actors[index];
                let direction = owner.movement.target_direction;
                crate::control::snap_heading(owner, direction);
                owner.movement.direction = direction;
            }
            AttackEvent::TraverseTarget {
                duration,
                clearance,
            } => {
                let start = battle.actors[index].position;
                let target = battle.actors[sequence.target.index()].position;
                let direction = crate::distance::planar_direction(
                    target,
                    start,
                    battle.actors[index].facing_direction,
                );
                sequence.traversal = Some(Traversal {
                    started_at: sequence.age,
                    duration: *duration,
                    start,
                    end: [
                        target[0] + direction[0] * clearance,
                        start[1],
                        target[2] + direction[2] * clearance,
                    ],
                });
            }
            AttackEvent::SetPosition(position) => battle.actors[index].position = *position,
            AttackEvent::PositionFromTarget { height, retreat }
            | AttackEvent::OffsetPosition { height, retreat } => {
                let origin = if matches!(event, AttackEvent::PositionFromTarget { .. }) {
                    battle.actors[sequence.target.index()].position
                } else {
                    battle.actors[index].position
                };
                let owner = &mut battle.actors[index];
                let direction = owner.movement.target_direction;
                owner.position = [
                    origin[0] - direction[0] * retreat,
                    *height,
                    origin[2] - direction[2] * retreat,
                ];
            }
            AttackEvent::PrimaryWeapons(visible) => {
                battle
                    .model_requests
                    .push(crate::ModelRequest::PrimaryWeapons {
                        actor,
                        visible: *visible,
                    })
            }
            AttackEvent::PassThrough { enabled, duration } => {
                battle.runtime[index].body_bypass = *enabled;
                battle.runtime[index].body_bypass_ticks = if *enabled { *duration } else { 0 };
            }
            AttackEvent::Tint(color) => battle.model_requests.push(crate::ModelRequest::Tint {
                actor,
                color: *color,
            }),
            AttackEvent::Defeat { rewards } => {
                defeat = Some((*at, *rewards));
            }
            AttackEvent::Stun => {
                battle.enter_action_stun(actor, cues);
                sequence.melee = None;
                sequence.execution = crate::action::Execution::Finished;
                return Ok(None);
            }
            AttackEvent::Land {
                pose,
                effect,
                settle,
                recovery_pose,
            } => {
                if battle.actors[index].airborne() {
                    break;
                }
                if let Some(pose) = pose {
                    pose.play(battle, index);
                }
                if let Some(appearance) = effect {
                    let owner = &battle.actors[index];
                    cues.push(Cue::InitializedEffect(crate::EffectRequest {
                        owner: actor,
                        target: actor,
                        appearance: *appearance,
                        origin: owner.position,
                        heading: owner.heading,
                        scale: owner.effect_scale,
                        follow: None,

                        tint: Default::default(),
                    }));
                }
                if let Some(settle) = settle {
                    // Landing changes the pose, not the lifetime of an active strike.
                    advance_contact(id, sequence, contacts, cues, highlighted_trail, false)?;
                    if battle.runtime[index].body_bypass_ticks == 0 {
                        battle.runtime[index].body_bypass &= attack.events[next_event + 1..]
                            .iter()
                            .any(|(at, _)| sequence.age < u32::from(*at));
                    }
                    sequence.execution = crate::action::Execution::Attack(AttackRun::Settling {
                        recover_at: sequence.age + u32::from(*settle),
                    });
                    if *settle == 0
                        && let Some(pose) = recovery_pose
                    {
                        pose.play(battle, index);
                    }
                    return Ok((*settle == 0).then_some(recovery));
                }
            }
            AttackEvent::SpecialGuard => {
                battle.begin_special_guard(actor, sequence.action)?;
            }
            AttackEvent::ReactionArmor(duration) => {
                let protection = &mut battle.actors[index].reaction.protection;
                if *duration == 0 {
                    if protection.mode == crate::ProtectionMode::Armor {
                        *protection = Default::default();
                    }
                } else {
                    protection.armor(u32::from(*duration));
                }
            }
            AttackEvent::StatUp { condition, amount } => {
                let owner = &mut battle.actors[index];
                owner
                    .conditions
                    .apply_stat_condition(*condition, *amount, 0, false);
                cues.push(Cue::ConditionLabel {
                    actor,
                    kind: crate::conditions::ConditionLabel::Applied,
                    position: owner.condition_label_origin(),
                });
                cues.push(Cue::DeferredFlash {
                    actor,
                    color: [112, 40, 40],
                    fade_first: true,
                });
            }
            AttackEvent::AfflictOpponents { groups, dispel } => {
                let side = battle.actors[index].side;
                for target in 0..battle.actors.len() {
                    if battle.actors[target].side == side
                        || battle.actors[target].hp == 0
                        || battle.actors[target].availability == crate::ActorAvailability::Absent
                    {
                        continue;
                    }
                    let recipient = &mut battle.actors[target];
                    if *dispel
                        && recipient
                            .conditions
                            .cure(crate::conditions::Cure::AntiMagic)
                    {
                        recipient.elements.enchantment = None;
                    }
                    let mut label = None;
                    for group in groups {
                        let choice = usize::from(battle.random.next_u16()) % group.len();
                        if let Some(applied) = recipient.conditions.apply_hit(crate::HitCondition {
                            condition: group[choice],
                            chance: 100,
                            value: 0,
                        }) {
                            label = Some(applied);
                        }
                    }
                    if let Some(kind) = label {
                        cues.push(Cue::ConditionLabel {
                            actor: crate::ActorId(target as u8),
                            kind,
                            position: recipient.condition_label_origin(),
                        });
                    }
                }
            }
            AttackEvent::Armor(threshold) => {
                let armor = &mut battle.actors[index].reaction.armor;
                armor.threshold = *threshold;
                armor.received = 0;
            }
            AttackEvent::Pose(pose) => {
                sequence.contact_blend_ends =
                    (pose.blend != 0).then_some(sequence.age + u32::from(pose.blend));
                pose.play(battle, index);
            }
            AttackEvent::PartialPose { pose, bones } => {
                battle
                    .model_requests
                    .push(crate::ModelRequest::PartialPose {
                        actor,
                        motion: pose.motion,
                        pose: pose.playback(),
                        bones: bones.clone(),
                    });
            }
            AttackEvent::PoseKeepingParts(pose) => {
                sequence.contact_blend_ends =
                    (pose.blend != 0).then_some(sequence.age + u32::from(pose.blend));
                battle
                    .model_requests
                    .push(crate::ModelRequest::PlayKeepingParts {
                        actor,
                        motion: pose.motion,
                        pose: pose.playback(),
                    });
            }
            AttackEvent::AnimationRate(rate) => {
                battle
                    .model_requests
                    .push(crate::ModelRequest::AnimationRate { actor, rate: *rate });
            }
            AttackEvent::CarriedPose {
                slot,
                clip,
                rate,
                repeat,
            } => battle
                .model_requests
                .push(crate::ModelRequest::CarriedPose {
                    actor,
                    slot: *slot,
                    clip: *clip,
                    rate: *rate,
                    repeat: *repeat,
                }),
            AttackEvent::WeaponVisibility(slots) => {
                for &(slot, visible) in slots {
                    battle.model_requests.push(crate::ModelRequest::Weapon {
                        actor,
                        slot,
                        visible,
                    });
                }
            }
            AttackEvent::Effect {
                appearance,
                tints,
                centered,
            } => {
                let element = crate::damage::resolve_element(
                    crate::HitElement::Inherited,
                    &battle.actors[index],
                )
                .map_or(0, |element| element as usize + 1);
                cues.push(Cue::Effect(crate::EffectRequest {
                    owner: actor,
                    target: actor,
                    appearance: *appearance,
                    origin: if *centered {
                        battle.actors[index].effect_origin()
                    } else {
                        battle.actors[index].position
                    },
                    heading: battle.actors[index].heading,
                    follow: Some(if *centered {
                        crate::EffectFollow::Center(actor)
                    } else {
                        crate::EffectFollow::Actor(actor)
                    }),
                    scale: 1.,
                    tint: tints[element],
                }));
            }
        }
        next_event += 1;
    }
    if let Some((_, rewards)) = defeat {
        if !rewards {
            battle.ledger.exclude_reward(actor);
        }
        battle.defeat_actor(actor, cues);
        battle.retarget_after_defeat(actor)?;
        return Ok(None);
    }
    sequence.execution = crate::action::Execution::Attack(AttackRun::Active { next_event, delay });
    if let Some(traversal) = &sequence.traversal {
        traversal.advance(sequence.age, &mut battle.actors[index]);
    }
    if waiting {
        cues.push(Cue::PauseActionEffects(id));
    }
    advance_contact(id, sequence, contacts, cues, highlighted_trail, waiting)?;
    // Feedback and redundant cleanup cannot extend gameplay; contacts and landing can.
    if sequence.age >= u32::from(attack.end_at)
        && sequence.melee.is_none()
        && attack.events[next_event..].iter().all(|(_, event)| {
            matches!(
                event,
                AttackEvent::Notice { .. }
                    | AttackEvent::Trail { .. }
                    | AttackEvent::Voice { .. }
                    | AttackEvent::Sound(_)
                    | AttackEvent::Pose(_)
                    | AttackEvent::PartialPose { .. }
                    | AttackEvent::PoseKeepingParts(_)
                    | AttackEvent::AnimationRate(_)
                    | AttackEvent::WeaponVisibility(_)
                    | AttackEvent::PrimaryWeapons(_)
                    | AttackEvent::Tint(_)
                    | AttackEvent::PassThrough { enabled: false, .. }
                    | AttackEvent::Effect { .. }
                    | AttackEvent::EffectAt { .. }
                    | AttackEvent::ActionEffect { .. }
                    | AttackEvent::InitializedActionEffect { .. }
                    | AttackEvent::HoldEffects(_)
                    | AttackEvent::InitializedEffect { .. }
                    | AttackEvent::UnisonFeedback { .. }
            )
        })
    {
        return Ok(Some(recovery));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorId, ActorSetup, BattleInput, HitCondition, Side, conditions::Condition};

    #[test]
    fn target_traversal_eases_across_the_ground_to_its_endpoint() {
        for direction in [[1., 0., 0.], [-1., 0., 0.], [0.6, 0., 0.8]] {
            let mut actor = crate::tests::actor(Side::Party);
            actor.position = [10., 12., 20.];
            let start = actor.position;
            let end = [
                start[0] + direction[0] * 240.,
                start[1],
                start[2] + direction[2] * 240.,
            ];
            let traversal = Traversal {
                started_at: 10,
                duration: 14,
                start,
                end,
            };
            let mut steps = Vec::new();
            for age in 10..24 {
                assert!(!traversal.finished(age));
                traversal.advance(age, &mut actor);
                steps.push(actor.movement.forward);
                actor.movement.integrate(&mut actor.position);
            }
            assert!(steps.iter().all(|step| *step > 0. && *step < 26.));
            assert!(steps[0] < steps[6] && steps[13] < steps[6]);
            for (actual, expected) in actor.position.into_iter().zip(end) {
                assert!((actual - expected).abs() < 0.001);
            }
            assert!(traversal.finished(24));
        }
    }

    fn movement_battle(events: Vec<(u16, AttackEvent)>) -> Result<Battle> {
        let mut action = crate::tests::action(40);
        crate::tests::attack_mut(&mut action).events = events;
        let mut owner = crate::tests::actor(Side::Party);
        owner.heading = 90.;
        owner.facing_direction = [1., 0., 0.];
        owner.movement.direction = owner.facing_direction;
        let mut target = crate::tests::actor(Side::Enemy);
        target.position[0] = 100.;
        let mut prepared = crate::PreparedBattle::new(
            vec![
                (owner, ActorSetup::default()),
                (target, ActorSetup::default()),
            ],
            vec![action].into(),
            1,
        )?;
        crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
        prepared.finish()
    }

    fn movement_request() -> BattleInput {
        crate::ActionRequest {
            actor: ActorId(0),
            target: ActorId(1),
            action: crate::ActionKey(0),
        }
        .into()
    }

    #[test]
    fn target_traversal_integrates_every_tick_including_admission() -> Result<()> {
        for (at, duration) in [(0, 1), (1, 1), (0, 9), (16, 9)] {
            let mut battle = movement_battle(vec![(
                at,
                AttackEvent::TraverseTarget {
                    duration,
                    clearance: 70.,
                },
            )])?;
            for tick in 0..=at + duration {
                battle.step(if tick == 0 {
                    movement_request()
                } else {
                    BattleInput::default()
                })?;
                let elapsed = (tick + 1).saturating_sub(at).min(duration);
                let t = f32::from(elapsed) / f32::from(duration);
                let expected = 170. * t * t * (3. - 2. * t);
                assert!(
                    (battle.actors[0].position[0] - expected).abs() < 0.001,
                    "traversal at {at} for {duration} ticks missed movement at tick {tick}"
                );
            }
            assert_eq!(battle.actors[0].movement.forward, 0.);
        }
        Ok(())
    }

    #[test]
    fn target_traversal_expiry_preserves_following_motion_after_hit_stop() -> Result<()> {
        let mut battle = movement_battle(vec![
            (
                1,
                AttackEvent::TraverseTarget {
                    duration: 2,
                    clearance: 70.,
                },
            ),
            (
                3,
                AttackEvent::Move {
                    forward: Some(5.),
                    vertical: None,
                },
            ),
            (3, AttackEvent::Acceleration(2.)),
            (3, AttackEvent::Braking(0.25)),
        ])?;
        battle.step(movement_request())?;
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].position[0], 85.);
        battle.actors[0].hit_stop = 3;
        crate::tests::step_idle(&mut battle, 3)?;
        assert_eq!(battle.actors[0].position[0], 85.);
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].position[0], 170.);
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].position[0], 175.);
        let movement = &battle.actors[0].movement;
        assert_eq!(movement.forward, 6.75);
        assert_eq!(movement.acceleration, 2.);
        assert_eq!(movement.braking, 0.25);
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].position[0], 181.75);
        Ok(())
    }

    #[test]
    fn ordinary_move_keeps_its_admission_hold() -> Result<()> {
        let mut battle = movement_battle(vec![
            (
                0,
                AttackEvent::Move {
                    forward: Some(5.),
                    vertical: None,
                },
            ),
            (0, AttackEvent::Braking(0.)),
        ])?;
        battle.step(movement_request())?;
        assert_eq!(battle.actors[0].position[0], 0.);
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].position[0], 5.);
        Ok(())
    }

    #[test]
    fn opponent_affliction_dispels_buffs_but_respects_immunity_and_dead_allies() -> Result<()> {
        use crate::conditions::{ConditionSet, Conditions, Layers};

        let mut action = crate::tests::action(3);
        crate::tests::attack_mut(&mut action).events = vec![(
            1,
            AttackEvent::AfflictOpponents {
                groups: vec![vec![Condition::PoisonSevere], vec![Condition::Weak]],
                dispel: true,
            },
        )];
        let mut protected = crate::tests::actor(Side::Party);
        protected.conditions = Conditions::new(Layers {
            base: ConditionSet::of(&[Condition::PhysicalProtection, Condition::DefenseUp]),
            immunity: Condition::Weak.into(),
            ..Default::default()
        });
        let mut dead = crate::tests::actor(Side::Party);
        dead.hp = 0;
        dead.availability = crate::ActorAvailability::Dead;
        let mut battle = crate::PreparedBattle::new(
            vec![
                (crate::tests::actor(Side::Enemy), ActorSetup::default()),
                (protected, ActorSetup::default()),
                (crate::tests::actor(Side::Party), ActorSetup::default()),
                (dead, ActorSetup::default()),
            ],
            vec![action].into(),
            1,
        )?
        .finish()?;
        battle.start_admitted_action(
            crate::ActionRequest {
                actor: ActorId(0),
                action: crate::ActionKey(0),
                target: ActorId(1),
            },
            &mut Vec::new(),
        )?;
        battle.step(BattleInput::default())?;
        assert!(
            !battle.actors[1]
                .conditions
                .base()
                .contains(Condition::PoisonSevere)
        );
        battle.step(BattleInput::default())?;
        assert_eq!(battle.actors[0].conditions.base(), ConditionSet::EMPTY);
        assert_eq!(
            battle.actors[1].conditions.base(),
            Condition::PoisonSevere.into()
        );
        assert_eq!(
            battle.actors[2].conditions.base(),
            ConditionSet::of(&[Condition::PoisonSevere, Condition::Weak])
        );
        assert_eq!(battle.actors[3].conditions.base(), ConditionSet::EMPTY);
        Ok(())
    }

    #[test]
    fn self_stun_replaces_the_action_and_counts_its_entry_visit() -> Result<()> {
        let mut action = crate::tests::action(30);
        crate::tests::attack_mut(&mut action).events = vec![
            (
                0,
                AttackEvent::Voice {
                    sound: Sound::Cue(809),
                    priority: crate::VoicePriority::Announcement,
                },
            ),
            (0, AttackEvent::Stun),
            (0, AttackEvent::Sound(Some(Sound::Cue(1)))),
        ];
        let mut battle = crate::PreparedBattle::new(
            vec![
                (crate::tests::actor(Side::Enemy), ActorSetup::default()),
                (crate::tests::actor(Side::Party), ActorSetup::default()),
            ],
            vec![action].into(),
            1,
        )?
        .finish()?;
        let actor = ActorId(0);
        battle.start_admitted_action(
            crate::ActionRequest {
                actor,
                action: crate::ActionKey(0),
                target: ActorId(1),
            },
            &mut Vec::new(),
        )?;
        let frame = battle.step(BattleInput::default())?;
        assert_eq!(battle.activity(actor), crate::Activity::Stunned);
        assert!(frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Voice {
                priority: crate::VoicePriority::Announcement,
                ..
            }
        )));
        assert!(!frame.cues.iter().any(|cue| matches!(
            cue,
            Cue::Sound {
                sound: Sound::Cue(1),
                ..
            }
        )));
        for _ in 0..180 {
            battle.step(BattleInput::default())?;
            assert_eq!(battle.activity(actor), crate::Activity::Stunned);
        }
        battle.step(BattleInput::default())?;
        assert_eq!(battle.activity(actor), crate::Activity::Recovering);
        assert!(!battle.actor_command_ready(actor));
        battle.step(BattleInput::default())?;
        assert_eq!(battle.activity(actor), crate::Activity::Idle);
        assert!(battle.actor_command_ready(actor));
        Ok(())
    }

    #[test]
    fn direct_spell_slots_release_once_without_chanting_or_repaying_and_outlive_the_attack()
    -> Result<()> {
        let release = |slot| AttackEvent::ReleaseSpell {
            definition: Arc::new(crate::tests::volley()),
            slot,
            effect: Some(crate::EffectAppearance {
                resource: ResourceId(1),
                member: 7,
            }),
            tint: Default::default(),
            sound: Some(Sound::Cue(123)),
        };
        let mut action = crate::tests::action(2);
        action.tp_cost = 7;
        crate::tests::attack_mut(&mut action).events = vec![
            (0, release(crate::SpellSlot::Primary)),
            (0, release(crate::SpellSlot::Secondary)),
            (1, release(crate::SpellSlot::Primary)),
            (1, release(crate::SpellSlot::Secondary)),
        ];
        let mut owner = crate::tests::actor(Side::Enemy);
        owner.equipment.max_tp = 50;
        owner.tp = 50;
        let mut battle = crate::PreparedBattle::new(
            vec![
                (owner, ActorSetup::default()),
                (crate::tests::actor(Side::Party), ActorSetup::default()),
            ],
            vec![action].into(),
            1,
        )?
        .finish()?;
        battle.start_admitted_action(
            crate::ActionRequest {
                actor: ActorId(0),
                action: crate::ActionKey(0),
                target: ActorId(1),
            },
            &mut Vec::new(),
        )?;
        let mut released = 0;
        let mut projectiles = 0;
        let mut sounds = 0;
        let mut effects = 0;
        for tick in 0..40 {
            let frame = battle.step(BattleInput::default())?;
            for cue in &frame.cues {
                match cue {
                    Cue::Released { actor, slot, .. } => {
                        assert_eq!(*actor, ActorId(0));
                        assert!(matches!(
                            slot,
                            crate::SpellSlot::Primary | crate::SpellSlot::Secondary
                        ));
                        released += 1;
                    }
                    Cue::ProjectileStarted { .. } => projectiles += 1,
                    Cue::Sound {
                        sound: Sound::Cue(123),
                        ..
                    } => sounds += 1,
                    Cue::Effect(_) => effects += 1,
                    Cue::Casting { .. } => panic!("direct release entered casting"),
                    _ => {}
                }
            }
            assert_eq!(battle.actors[0].tp, 43);
            if tick == 5 {
                assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
                assert!(battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
            }
        }
        assert_eq!((released, projectiles, sounds, effects), (2, 6, 2, 2));
        assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Primary));
        assert!(!battle.spell_active(ActorId(0), crate::SpellSlot::Secondary));
        Ok(())
    }

    #[test]
    fn airborne_entry_is_selected_once_without_repaying() -> Result<()> {
        for airborne in [false, true] {
            let variant = |sound| {
                let mut action = crate::tests::action(3);
                action.tp_cost = 7;
                crate::tests::attack_mut(&mut action).events = vec![
                    (0, AttackEvent::Sound(Some(Sound::Cue(sound)))),
                    (1, AttackEvent::Sound(Some(Sound::Cue(sound + 10)))),
                ];
                Arc::new(action)
            };
            let mut entry = crate::tests::action(3);
            entry.tp_cost = 7;
            crate::tests::attack_mut(&mut entry).events =
                vec![(0, AttackEvent::ChooseAirborne([variant(1), variant(2)]))];
            let mut owner = crate::tests::actor(Side::Party);
            owner.position[1] = if airborne { 100. } else { 0. };
            let mut battle = crate::PreparedBattle::new(
                vec![
                    (owner, ActorSetup::default()),
                    (crate::tests::actor(Side::Enemy), ActorSetup::default()),
                ],
                vec![entry].into(),
                1,
            )?
            .finish()?;
            battle.start_admitted_action(
                crate::ActionRequest {
                    actor: ActorId(0),
                    action: crate::ActionKey(0),
                    target: ActorId(1),
                },
                &mut Vec::new(),
            )?;
            let chosen = if airborne { 2 } else { 1 };
            for sound in [chosen, chosen + 10] {
                let frame = battle.step(BattleInput::default())?;
                let sounds: Vec<_> = frame
                    .cues
                    .iter()
                    .filter_map(|cue| match cue {
                        Cue::Sound {
                            sound: Sound::Cue(value),
                            ..
                        } => Some(*value),
                        _ => None,
                    })
                    .collect();
                assert_eq!(sounds, [sound]);
                assert_eq!(battle.actors[0].tp, 33);
                // A changed height cannot replace the already selected action.
                battle.actors[0].position[1] = if airborne { 0. } else { 100. };
            }
        }
        Ok(())
    }

    #[test]
    fn grave_blade_selects_once_after_payment_and_wind_uses_the_common_clock() {
        for (uses, element, grave) in [
            (198, Some(crate::Element::Earth), false),
            (199, Some(crate::Element::Earth), true),
            (200, None, false),
        ] {
            let variant = |sound| {
                let mut definition = crate::tests::action(12);
                definition.tp_cost = 16;
                crate::tests::attack_mut(&mut definition).events = vec![
                    (1, AttackEvent::Sound(Some(Sound::Cue(sound)))),
                    (
                        1,
                        AttackEvent::SecondaryWind {
                            enabled: true,
                            duration: 2,
                        },
                    ),
                ];
                Arc::new(definition)
            };
            let mut entry = (*variant(1)).clone();
            crate::tests::attack_mut(&mut entry).events =
                vec![(0, AttackEvent::ChooseGraveBlade([variant(1), variant(2)]))];
            let mut owner = crate::tests::actor(Side::Party);
            owner.equipment.base_element = element;
            owner.tp = 15;
            let mut battle = crate::PreparedBattle::new(
                vec![
                    (
                        owner,
                        ActorSetup {
                            techniques: vec![crate::tests::technique(crate::ActionKey(0), 4)],
                            ..Default::default()
                        },
                    ),
                    (crate::tests::actor(Side::Enemy), ActorSetup::default()),
                ],
                vec![entry].into(),
                1,
            )
            .unwrap()
            .with_technique_learning_members(vec![crate::tests::counted_techniques(
                ActorId(0),
                &[4],
                &[(4, uses)],
            )])
            .unwrap()
            .finish()
            .unwrap();
            let request = crate::ActionRequest {
                actor: ActorId(0),
                action: crate::ActionKey(0),
                target: ActorId(1),
            };
            battle.step(BattleInput::from(request)).unwrap();
            assert_eq!(
                battle.technique_uses(ActorId(0), TechniqueId(4)),
                Some(uses)
            );
            assert_eq!(battle.actors[0].tp, 15);
            battle.actors[0].tp = 40;
            let frame = battle.step(BattleInput::from(request)).unwrap();
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue, Cue::EffectNotice { .. }))
                    .count(),
                usize::from(grave)
            );
            // Changing equipment after admission cannot replace the selected variant.
            battle.actors[0].equipment.base_element = if grave {
                None
            } else {
                Some(crate::Element::Earth)
            };
            let frame = battle.step(BattleInput::default()).unwrap();
            let sounds: Vec<_> = frame
                .cues
                .iter()
                .filter_map(|cue| match cue {
                    Cue::Sound {
                        sound: Sound::Cue(id),
                        ..
                    } => Some(*id),
                    _ => None,
                })
                .collect();
            assert_eq!(sounds, vec![if grave { 2 } else { 1 }]);
            assert_eq!(battle.actors[0].secondary_wind.force[1], 0.2);
            let wind = battle.actors[0].secondary_wind;
            battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(battle.actors[0].secondary_wind, wind);
            battle.actors[0].hit_stop = 3;
            for tick in 0..3 {
                let frame = battle.step(BattleInput::default()).unwrap();
                assert!(
                    !frame
                        .cues
                        .iter()
                        .any(|cue| matches!(cue, Cue::Sound { .. } | Cue::EffectNotice { .. }))
                );
                assert_eq!(
                    battle.actors[0].secondary_wind.force[1],
                    if tick < 2 { 0.2 } else { 0. }
                );
            }
            assert_eq!(
                battle.technique_uses(ActorId(0), TechniqueId(4)),
                Some(uses + 1)
            );
            assert_eq!(battle.actors[0].tp, 24);
        }
    }

    #[test]
    fn hit_wait_preserves_event_spacing_pause_and_miss_deadline() {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Contact {
            Miss,
            Ordinary,
            PreviousPose,
            Survives,
            Armored,
            Guarded,
            GuardBreak,
        }
        for (effective, contact) in [
            (false, Contact::Miss),
            (false, Contact::Ordinary),
            (false, Contact::PreviousPose),
            (true, Contact::Miss),
            (true, Contact::Ordinary),
            (true, Contact::PreviousPose),
            (true, Contact::Survives),
            (true, Contact::Armored),
            (true, Contact::Guarded),
            (true, Contact::GuardBreak),
        ] {
            let hit = contact != Contact::Miss;
            let continues = hit
                && if effective {
                    matches!(
                        contact,
                        Contact::Ordinary | Contact::Survives | Contact::PreviousPose
                    )
                } else {
                    contact != Contact::PreviousPose
                };
            let mut target = crate::tests::actor(Side::Enemy);
            if contact == Contact::Survives {
                target.hp = 1;
            }
            target.position[0] = if contact == Contact::PreviousPose {
                0.
            } else {
                1000.
            };
            target.body.collider = Some(crate::Collider::sphere(1.));
            let mut rule = crate::tests::contact_projectile(false, true)
                .contact
                .unwrap()
                .hit;
            rule.power = crate::Power::Fixed(1);
            rule.prevents_defeat = contact == Contact::Survives;
            rule.guard.breaks = contact == Contact::GuardBreak;
            let mut definition = crate::tests::action(8);
            let attack = crate::tests::attack_mut(&mut definition);
            attack.recovery = 2;
            attack.events = vec![
                (
                    0,
                    AttackEvent::Contact {
                        definition: Arc::new(MeleeDefinition {
                            hit: rule,
                            trail: None,
                            volume: crate::MeleeVolume::Fixed {
                                offset: [0.; 3],
                                radius: 5.,
                                half_height: 5.,
                            },
                        }),
                        duration: 6,
                    },
                ),
                (
                    1,
                    if effective {
                        AttackEvent::WaitForEffectiveHit { deadline: 6 }
                    } else {
                        AttackEvent::WaitForHit {
                            deadline: 6,
                            recovery: None,
                        }
                    },
                ),
                (1, AttackEvent::Sound(Some(Sound::Cue(1)))),
                (3, AttackEvent::Sound(Some(Sound::Cue(2)))),
                (
                    7,
                    AttackEvent::Move {
                        forward: Some(10.),
                        vertical: None,
                    },
                ),
            ];
            if contact == Contact::PreviousPose {
                attack.events.pop();
                attack.events[1].0 = 4;
                attack.events[2].0 = 4;
                attack.events[3].0 = 6;
                attack.events.insert(
                    1,
                    (
                        1,
                        AttackEvent::Pose(AttackPose {
                            motion: MotionBinding {
                                model: ResourceId(1),
                                clip: 0,
                            },
                            start: 0.,
                            end: None,
                            repeat: true,
                            rate: 0.5,
                            blend: 3,
                        }),
                    ),
                );
            }
            let mut prepared = crate::PreparedBattle::new(
                vec![
                    (crate::tests::actor(Side::Party), Default::default()),
                    (target, Default::default()),
                ],
                vec![definition].into(),
                1,
            )
            .unwrap();
            crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
            let mut battle = prepared.finish().unwrap();
            let first = battle
                .step(BattleInput::from(crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: crate::ActionKey(0),
                }))
                .unwrap();
            let action = first.actions[0].0;
            if contact == Contact::PreviousPose {
                battle.actors[1].position[0] = 1000.;
            }
            for _ in 0..3 {
                let frame = battle.step(BattleInput::default()).unwrap();
                assert!(
                    !frame
                        .cues
                        .iter()
                        .any(|cue| matches!(cue, Cue::Sound { .. }))
                );
            }
            assert_eq!(battle.action_age(action), Some(4));
            for _ in 0..3 {
                battle
                    .step(BattleInput {
                        paused: true,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(battle.action_age(action), Some(4));
            }
            let last_contact_position = battle.actors[0].position;
            battle.actors[0].position[0] += 100.;
            if hit && contact != Contact::PreviousPose {
                // A waiting window keeps its last geometry while the owner moves.
                battle.actors[1].position = last_contact_position;
            }
            let target = &mut battle.actors[1];
            if contact == Contact::Armored {
                target.reaction.armor.threshold = 1;
            }
            if matches!(contact, Contact::Guarded | Contact::GuardBreak) {
                target.guard.active = true;
                target.guard.break_pressure = 10;
                target.facing_direction = [0., 0., -1.];
            }
            let mut sounds = Vec::new();
            for _ in 0..30 {
                let frame = battle.step(BattleInput::default()).unwrap();
                for cue in frame.cues {
                    if let Cue::Sound {
                        sound: Sound::Cue(id @ (1 | 2)),
                        ..
                    } = cue
                    {
                        sounds.push((id, battle.action_age(action).unwrap()));
                    }
                }
                if battle.activity(ActorId(0)) == crate::Activity::Recovering {
                    break;
                }
            }
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
            assert_eq!(
                battle.action_age(action),
                Some(if continues { 8 } else { 6 })
            );
            assert_eq!(
                sounds,
                if continues && contact == Contact::PreviousPose {
                    vec![(1, 5), (2, 7)]
                } else if continues {
                    vec![(1, 6), (2, 8)]
                } else {
                    vec![]
                }
            );
            assert_eq!(
                battle.actors[1].hp,
                if contact == Contact::Survives {
                    1
                } else if hit {
                    49
                } else {
                    50
                }
            );
            assert_eq!(battle.actors[0].movement.forward, 0.);
        }
    }

    #[test]
    fn ground_projectiles_choose_distinct_living_targets_at_release() {
        for living in [1, 3] {
            let mut owner = crate::tests::actor(Side::Party);
            owner.position = [1000., 0., 0.];
            owner.heading = -90.;
            let mut actors = vec![(owner, Default::default())];
            for index in 0..4 {
                let mut target = crate::tests::actor(Side::Enemy);
                target.control = crate::Control::Manual;
                target.position = [-300. - 200. * index as f32, 0., 0.];
                if index >= living {
                    target.hp = 0;
                    target.availability = crate::ActorAvailability::Dead;
                }
                actors.push((target, Default::default()));
            }
            let mut projectile = crate::tests::contact_projectile(false, true);
            projectile.contact = None;
            projectile.lifetime = Some(2);
            let projectile = Arc::new(projectile);
            let mut definition = crate::tests::action(4);
            crate::tests::attack_mut(&mut definition).events = vec![(
                2,
                AttackEvent::GroundProjectiles {
                    definitions: [Arc::clone(&projectile), projectile],
                    retreat: 225.,
                },
            )];
            let mut prepared =
                crate::PreparedBattle::new(actors, vec![definition].into(), 1).unwrap();
            crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
            let mut battle = prepared.finish().unwrap();
            let opening = battle
                .step(BattleInput::from(crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: crate::ActionKey(0),
                }))
                .unwrap();
            assert!(opening.projectiles.is_empty());
            for (index, target) in battle.actors.iter_mut().enumerate().skip(1) {
                target.position = [-500. * index as f32, 40., 100. * index as f32];
            }
            let mut births = std::collections::BTreeMap::new();
            for _ in 0..10 {
                for projectile in battle.step(BattleInput::default()).unwrap().projectiles {
                    births.entry(projectile.id).or_insert(projectile);
                }
            }
            assert_eq!(births.len(), living.min(2));
            let mut targets = std::collections::BTreeSet::new();
            for projectile in births.values() {
                let target = projectile.target.index();
                assert!((1..=living).contains(&target));
                assert!(targets.insert(target));
                assert_eq!(projectile.heading, -90.);
                assert_eq!(projectile.position[0], -500. * target as f32 + 225.);
                assert_eq!(projectile.position[1], 0.);
                assert!((projectile.position[2] - 100. * target as f32).abs() < 0.0001);
            }
            assert!(battle.projectiles.is_empty());
        }
    }

    #[test]
    fn cyclone_contacts_use_the_prior_center_in_both_roster_orders() {
        for (owner, target) in [(ActorId(0), ActorId(1)), (ActorId(1), ActorId(0))] {
            let appearance = crate::EffectAppearance {
                resource: ResourceId(1),
                member: 7,
            };
            let mut projectile = crate::tests::contact_projectile(false, true);
            projectile.contact = None;
            projectile.clamp_ground = false;
            projectile.lifetime = Some(1);
            let projectile = Arc::new(projectile);
            let mut action = crate::tests::action(8);
            crate::tests::attack_mut(&mut action).events = vec![
                (0, AttackEvent::MarkCycloneTarget(appearance)),
                (
                    2,
                    AttackEvent::CycloneContact {
                        definition: Arc::clone(&projectile),
                        appearance,
                        opening: false,
                    },
                ),
                (
                    4,
                    AttackEvent::CycloneContact {
                        definition: projectile,
                        appearance,
                        opening: false,
                    },
                ),
            ];
            let mut actors = vec![
                (crate::tests::actor(Side::Party), ActorSetup::default()),
                (crate::tests::actor(Side::Party), ActorSetup::default()),
            ];
            actors[target.index()].0.side = Side::Enemy;
            actors[target.index()].0.control = crate::Control::Manual;
            actors[target.index()].0.position = [100., 40., 300.];
            actors[target.index()].0.body.center_offset = [0., 80., 0.];
            let mut battle = crate::PreparedBattle::new(actors, vec![action].into(), 1)
                .unwrap()
                .finish()
                .unwrap();
            let mut started = vec![];
            battle
                .start_admitted_action(
                    crate::ActionRequest {
                        actor: owner,
                        target,
                        action: crate::ActionKey(0),
                    },
                    &mut started,
                )
                .unwrap();
            let action = started
                .iter()
                .find_map(|cue| match cue {
                    Cue::Started { action, .. } => Some(*action),
                    _ => None,
                })
                .unwrap();
            let mut contacts = 0;
            let mut contact_effects = 0;
            for tick in 0..6 {
                // Publish distinct centers on consecutive updates, so using the
                // live recipient or a cache refreshed earlier in roster order fails.
                battle.actors[target.index()].position =
                    [200. + 100. * tick as f32, 50. + 10. * tick as f32, 300.];
                if tick == 2 {
                    let age = battle.action_age(action);
                    let centers = battle.actor_emission_centers.clone();
                    let paused = battle
                        .step(BattleInput {
                            paused: true,
                            ..Default::default()
                        })
                        .unwrap();
                    assert_eq!(battle.action_age(action), age);
                    assert_eq!(battle.actor_emission_centers, centers);
                    assert!(
                        !paused
                            .cues
                            .iter()
                            .any(|cue| matches!(cue, Cue::ProjectileStarted { .. }))
                    );
                }
                let expected = [100. + 100. * tick as f32, 120. + 10. * tick as f32, 300.];
                let frame = battle.step(BattleInput::default()).unwrap();
                for cue in &frame.cues {
                    if let Cue::ProjectileStarted { projectile, .. } = cue {
                        assert_eq!(battle.projectiles[projectile].frame.position, expected);
                        contacts += 1;
                    }
                    if let Cue::InitializedEffect(effect) = cue
                        && effect.follow == Some(crate::EffectFollow::Center(target))
                    {
                        assert_eq!(effect.origin, expected);
                        contact_effects += 1;
                    }
                }
            }
            assert_eq!(contacts, 2);
            assert_eq!(contact_effects, 2);
        }
    }

    #[test]
    fn contact_burst_requires_hit_and_samples_moved_victim_once() {
        for (connects, with_impact) in [(false, false), (false, true), (true, false), (true, true)]
        {
            let mut victim = crate::tests::actor(Side::Enemy);
            victim.control = crate::Control::Manual;
            victim.body.collider = Some(crate::Collider::sphere(1.));
            victim.position[0] = if connects { 0. } else { 1000. };
            let mut selected = crate::tests::actor(Side::Enemy);
            selected.control = crate::Control::Manual;
            selected.position[0] = 500.;
            let mut projectile = crate::tests::contact_projectile(false, true);
            let mut hit = projectile.contact.take().unwrap().hit;
            hit.power = crate::Power::Fixed(1);
            projectile.lifetime = Some(2);
            projectile.velocity = [0.; 3];
            let appearance = crate::EffectAppearance {
                resource: ResourceId(1),
                member: 7,
            };
            let mut definition = crate::tests::action(4);
            crate::tests::attack_mut(&mut definition).events = vec![
                (
                    0,
                    AttackEvent::Contact {
                        definition: Arc::new(MeleeDefinition {
                            hit,
                            trail: None,
                            volume: crate::MeleeVolume::Fixed {
                                offset: [0.; 3],
                                radius: 5.,
                                half_height: 5.,
                            },
                        }),
                        duration: 1,
                    },
                ),
                (
                    2,
                    AttackEvent::ContactBurst {
                        definition: Arc::new(projectile),
                        appearance: with_impact.then_some(appearance),
                    },
                ),
            ];
            let mut prepared = crate::PreparedBattle::new(
                vec![
                    (crate::tests::actor(Side::Party), Default::default()),
                    (victim, Default::default()),
                    (selected, Default::default()),
                ],
                vec![definition].into(),
                1,
            )
            .unwrap();
            crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
            let mut battle = prepared.finish().unwrap();
            let first = battle
                .step(BattleInput::from(crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(2),
                    action: crate::ActionKey(0),
                }))
                .unwrap();
            assert_eq!(
                battle.sequence(&first.actions[0].0).unwrap().contact_target,
                connects.then_some(ActorId(1))
            );
            // Use the moved victim, not the opening impact or selected target.
            battle.actors[1].position = [120., 40., 25.];
            let mut effects = Vec::new();
            let mut births = 0;
            for _ in 0..20 {
                let frame = battle.step(BattleInput::default()).unwrap();
                for cue in frame.cues {
                    match cue {
                        Cue::InitializedEffect(effect) => effects.push(effect),
                        Cue::ProjectileStarted { .. } => births += 1,
                        _ => {}
                    }
                }
                for projectile in frame.projectiles {
                    assert_eq!(projectile.position, [120., 0., 25.]);
                }
            }
            assert_eq!(births, usize::from(connects));
            assert_eq!(effects.len(), usize::from(connects && with_impact));
            if let Some(effect) = effects.first() {
                assert_eq!(effect.origin, [120., 0., 25.]);
                assert_eq!(effect.target, ActorId(1));
                assert_eq!(effect.appearance, appearance);
                assert_eq!(effect.follow, None);
                assert_eq!(effect.scale, 1.);
            }
        }
    }

    #[test]
    fn direct_defeat_preserves_hp_and_released_projectile_contacts() {
        for (rewards, defeat_first) in [(true, false), (false, false), (true, true), (false, true)]
        {
            let owner = crate::tests::actor(Side::Enemy);
            let owner_hp = owner.hp;
            let mut target = crate::tests::actor(Side::Party);
            target.body.collider = Some(crate::Collider::sphere(1.));
            let target_hp = target.hp;
            let mut survivor = crate::tests::actor(Side::Enemy);
            survivor.position[0] = 1000.;
            let mut projectile = crate::tests::contact_projectile(false, true);
            projectile.lifetime = Some(6);
            projectile.active = Some([2, 3]);
            let contact = projectile.contact.as_mut().unwrap();
            contact.radius = 100.;
            contact.hit.power = crate::Power::Fixed(1);
            let mut events = vec![
                (1, AttackEvent::Projectile(Arc::new(projectile))),
                (1, AttackEvent::Defeat { rewards }),
                (2, AttackEvent::Sound(Some(Sound::Cue(1)))),
            ];
            if defeat_first {
                events.swap(0, 1);
            }
            let mut prepared = crate::PreparedBattle::new(
                vec![
                    (owner, Default::default()),
                    (target, Default::default()),
                    (survivor, Default::default()),
                ],
                vec![crate::ActionDefinition {
                    normal: None,
                    tp_cost: 0,
                    execution: ActionExecution::Attack(PreparedAttack {
                        recovery: 2,
                        events,
                        ..PreparedAttack::new(4)
                    }),
                }]
                .into(),
                1,
            )
            .unwrap();
            crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
            let mut battle = prepared.finish().unwrap();
            battle.set_decision_target(ActorId(1), ActorId(0)).unwrap();
            battle
                .step(BattleInput::from(crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(1),
                    action: crate::ActionKey(0),
                }))
                .unwrap();
            let mut defeats = 0;
            for _ in 0..8 {
                let was_available = battle.actors[0].available();
                let frame = battle.step(BattleInput::default()).unwrap();
                let defeated = was_available && !battle.actors[0].available();
                if defeated {
                    defeats += 1;
                    assert_eq!(frame.projectiles.len(), 1);
                    assert!(frame.actions.is_empty());
                    assert_eq!(battle.actors[1].hp, target_hp);
                    assert_eq!(battle.target(ActorId(1)), Some(ActorId(2)));
                }
                assert!(
                    !frame
                        .cues
                        .iter()
                        .any(|cue| matches!(cue, Cue::Sound { .. } | Cue::Defeated { .. }))
                );
            }
            assert_eq!(defeats, 1);
            assert_eq!(battle.actors[0].hp, owner_hp);
            assert!(!battle.actors[0].available());
            assert_eq!(battle.actors[1].hp, target_hp - 1);
            assert_eq!(battle.ledger.reward_eligible(ActorId(0)), rewards);
            assert!(battle.ledger.reward_eligible(ActorId(1)));
        }
    }

    fn landing_contact() -> Arc<MeleeDefinition> {
        Arc::new(MeleeDefinition {
            hit: crate::HitRule {
                arte: true,
                power: crate::Power::Fixed(1),
                ..crate::tests::hit()
            },
            trail: None,
            volume: crate::MeleeVolume::Fixed {
                offset: [0.; 3],
                radius: 5.,
                half_height: 5.,
            },
        })
    }

    #[test]
    fn landing_and_contacts_delay_recovery_but_trailing_feedback_does_not() {
        let mut owner = crate::tests::actor(Side::Party);
        owner.position[1] = 2000.;

        let mut target = crate::tests::actor(Side::Enemy);
        target.position[0] = 1000.;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let contact = landing_contact();
        let mut prepared = crate::PreparedBattle::new(
            vec![(owner, Default::default()), (target, Default::default())],
            (vec![crate::ActionDefinition {
                normal: None,
                tp_cost: 8,
                execution: ActionExecution::Attack(PreparedAttack {
                    recovery: 3,
                    events: vec![
                        (
                            0,
                            AttackEvent::Land {
                                pose: None,
                                effect: None,
                                settle: None,
                                recovery_pose: None,
                            },
                        ),
                        (
                            0,
                            AttackEvent::Contact {
                                definition: contact,
                                duration: 3,
                            },
                        ),
                        (u16::MAX, AttackEvent::Sound(Some(Sound::Cue(1)))),
                        (
                            u16::MAX,
                            AttackEvent::PassThrough {
                                enabled: false,
                                duration: 0,
                            },
                        ),
                    ],
                    ..PreparedAttack::new(0)
                }),
            }])
            .into(),
            1,
        )
        .unwrap();
        crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
        prepared.resources.actor_setup[0].techniques[0]
            .capabilities
            .aerial = true;
        prepared.actors[0].equipment.combo_traits.aerial_arte = true;
        let mut battle = prepared.finish().unwrap();
        let first = battle
            .step(BattleInput::from(crate::ActionRequest {
                actor: ActorId(0),
                target: ActorId(0),
                action: crate::ActionKey(0),
            }))
            .unwrap();
        let action = first.actions[0].0;
        for _ in 0..10 {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
            assert_eq!(battle.actors[1].hp, 50);
        }
        battle.actors[0].position[1] = 0.;
        battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
        for _ in 0..2 {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
            assert_eq!(battle.actors[1].hp, 50);
        }
        // Landing may happen after end_at, but the full strike window still runs.
        battle.actors[1].position = [0.; 3];
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[1].hp, 49);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
        let mut completions = 0;
        for _ in 0..10 {
            let frame = battle.step(BattleInput::default()).unwrap();
            completions += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Completed { action: id } if *id == action))
                .count();
        }
        assert_eq!(completions, 1);
        assert_eq!(battle.actors[1].hp, 49);
        assert_eq!(battle.actors[0].tp, 32);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
    }

    #[test]
    fn terminal_landing_keeps_action_age_and_settles_before_recovery() {
        let mut owner = crate::tests::actor(Side::Party);
        owner.position[1] = 2000.;
        let mut target = crate::tests::actor(Side::Enemy);
        target.position[0] = 1000.;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let landing_effect = crate::EffectAppearance {
            resource: ResourceId(7),
            member: 3,
        };
        let mut prepared = crate::PreparedBattle::new(
            vec![(owner, Default::default()), (target, Default::default())],
            vec![crate::ActionDefinition {
                normal: None,
                tp_cost: 8,
                execution: ActionExecution::Attack(PreparedAttack {
                    recovery: 3,
                    events: vec![
                        (
                            0,
                            AttackEvent::PassThrough {
                                enabled: true,
                                duration: 0,
                            },
                        ),
                        (
                            0,
                            AttackEvent::Contact {
                                definition: landing_contact(),
                                duration: 14,
                            },
                        ),
                        (
                            2,
                            AttackEvent::Land {
                                pose: None,
                                effect: Some(landing_effect),
                                settle: Some(4),
                                recovery_pose: Some(AttackPose {
                                    motion: MotionBinding {
                                        model: ResourceId(1),
                                        clip: 0,
                                    },
                                    start: 0.,
                                    end: None,
                                    repeat: true,
                                    rate: 0.5,
                                    blend: 3,
                                }),
                            },
                        ),
                        (
                            13,
                            AttackEvent::PassThrough {
                                enabled: false,
                                duration: 0,
                            },
                        ),
                    ],
                    ..PreparedAttack::new(100)
                }),
            }]
            .into(),
            1,
        )
        .unwrap();
        crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
        prepared.resources.actor_setup[0].techniques[0]
            .capabilities
            .aerial = true;
        prepared.actors[0].equipment.combo_traits.aerial_arte = true;
        let mut battle = prepared.finish().unwrap();
        let first = battle
            .step(BattleInput::from(crate::ActionRequest {
                actor: ActorId(0),
                target: ActorId(1),
                action: crate::ActionKey(0),
            }))
            .unwrap();
        let action = first.actions[0].0;
        for _ in 0..10 {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
        }
        let landed_age = battle.action_age(action).unwrap();
        assert!(battle.sequence(&action).unwrap().melee.is_some());
        assert!(battle.bypasses_body_collision(ActorId(0)));
        battle.actors[0].position[1] = 0.;
        battle.actors[0].movement.vertical = 0.;
        for elapsed in 0..4 {
            if elapsed == 1 {
                battle.actors[1].position = battle.actors[0].position;
            }
            assert_eq!(battle.action_age(action), Some(landed_age + elapsed));
            battle
                .step(BattleInput {
                    paused: true,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(battle.action_age(action), Some(landed_age + elapsed));
            let frame = battle.step(BattleInput::default()).unwrap();
            assert_eq!(
                frame
                    .cues
                    .iter()
                    .filter(|cue| matches!(cue, Cue::InitializedEffect(request) if request.appearance == landing_effect))
                    .count(),
                usize::from(elapsed == 0)
            );
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
            assert_eq!(
                battle.sequence(&action).unwrap().melee.is_some(),
                elapsed < 2
            );
            assert_eq!(battle.actors[1].hp, if elapsed == 0 { 50 } else { 49 });
            assert_eq!(
                battle.bypasses_body_collision(ActorId(0)),
                landed_age + elapsed < 13
            );
        }
        let frame = battle.step(BattleInput::default()).unwrap();
        assert!(frame.model_requests.iter().any(|request| matches!(request,
            crate::ModelRequest::Play { actor: ActorId(0), motion: MotionBinding { model: ResourceId(1), clip: 0 }, pose } if pose.repeat && pose.blend == 3
        )));
        assert_eq!(battle.action_age(action), Some(landed_age + 4));
        assert_eq!(battle.action_recovery_remaining(action), Some(3));
        assert_eq!(battle.actors[0].tp, 32);
        for remaining in (0..3).rev() {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.action_recovery_remaining(action), Some(remaining));
            assert!(!battle.actor_command_ready(ActorId(0)));
        }
        let completed = battle.step(BattleInput::default()).unwrap();
        assert!(
            completed
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Completed { action: id } if *id == action))
        );
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
        assert!(!battle.actor_command_ready(ActorId(0)));
        battle.step(BattleInput::default()).unwrap();
        assert!(battle.actor_command_ready(ActorId(0)));
    }

    #[test]
    fn late_contact_applies_the_condition_to_the_hit_target_and_recovers_once() {
        let owner = crate::tests::actor(Side::Party);

        let mut enemy = crate::tests::actor(Side::Enemy);
        enemy.position[0] = 1000.;
        enemy.body.collider = Some(crate::Collider::sphere(1.));
        enemy.equipment.luck = 0;
        let hit = crate::HitRule {
            arte: true,
            power: crate::Power::Fixed(1),
            condition: Some(HitCondition {
                condition: resonance_content::battle_action::Condition::DefenseDown,
                chance: 100,
                value: -10,
            }),
            ..crate::tests::hit()
        };
        let attack = PreparedAttack {
            recovery: 2,
            events: vec![
                (
                    8,
                    AttackEvent::Contact {
                        definition: Arc::new(MeleeDefinition {
                            hit,
                            trail: None,
                            volume: crate::MeleeVolume::Fixed {
                                offset: [0.; 3],
                                radius: 5.,
                                half_height: 5.,
                            },
                        }),
                        duration: 13,
                    },
                ),
                (
                    20,
                    AttackEvent::ContinueContact {
                        definition: Arc::new(MeleeDefinition {
                            hit: crate::HitRule {
                                power: crate::Power::Fixed(2),
                                ..hit
                            },
                            trail: None,
                            volume: crate::MeleeVolume::Fixed {
                                offset: [0.; 3],
                                radius: 10.,
                                half_height: 5.,
                            },
                        }),
                        duration: 4,
                    },
                ),
            ],
            ..PreparedAttack::new(24)
        };
        let mut battle = crate::PreparedBattle::new(
            vec![
                (
                    owner,
                    ActorSetup {
                        techniques: vec![crate::tests::technique(crate::ActionKey(0), 2)],
                        ..Default::default()
                    },
                ),
                (enemy.clone(), ActorSetup::default()),
                (enemy, ActorSetup::default()),
            ],
            (vec![crate::ActionDefinition {
                normal: None,
                tp_cost: 4,
                execution: ActionExecution::Attack(attack),
            }])
            .into(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        battle
            .step(BattleInput::from(crate::ActionRequest {
                actor: ActorId(0),
                target: ActorId(2),
                action: crate::ActionKey(0),
            }))
            .unwrap();
        for _ in 0..17 {
            battle.step(BattleInput::default()).unwrap();
        }
        assert!(battle.actors.iter().all(|actor| {
            !actor
                .conditions
                .effective()
                .contains(Condition::DefenseDown)
        }));
        // A different enemy enters the active hit volume after the windup.
        battle.actors[1].position = [0.; 3];

        let contact = battle.step(BattleInput::default()).unwrap();
        assert!(contact.cues.iter().any(|cue| matches!(
            cue,
            Cue::Hit {
                actor: ActorId(1),
                ..
            }
        )));
        assert_eq!(
            battle.actors[1]
                .conditions
                .magnitude(Condition::DefenseDown),
            -10
        );
        assert!(
            !battle.actors[2]
                .conditions
                .effective()
                .contains(Condition::DefenseDown)
        );
        let duration = battle.actors[1]
            .conditions
            .remaining(Condition::DefenseDown)
            .unwrap();
        let action = contact.actions[0].0;
        let mut completions = 0;
        let mut continued_hits = 0;
        for _ in 0..40 {
            // The first victim stays in reach. A second enters only after the
            // original window would expire, inside the enlarged volume.
            battle.actors[1].position = [0.; 3];
            if battle
                .sequence(&action)
                .is_some_and(|sequence| sequence.age >= 22)
            {
                battle.actors[2].position = [8., 0., 0.];
            }
            let frame = battle.step(BattleInput::default()).unwrap();
            for cue in &frame.cues {
                if let Cue::Hit { actor, result, .. } = cue {
                    assert_eq!(*actor, ActorId(2));
                    assert_eq!(result.hp_change, -2);
                    continued_hits += 1;
                }
            }
            completions += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Completed { .. }))
                .count();
        }
        assert_eq!(continued_hits, 1);
        assert_eq!(completions, 1);
        assert_eq!(battle.actors[0].tp, 36);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
        assert!(
            battle.actors[1]
                .conditions
                .remaining(Condition::DefenseDown)
                .unwrap()
                < duration
        );
    }
}
