//! One owner for ring timing, contacts, recovery and visual lifetimes.
use super::{BubblePhase, CallColor, ElectricOrbKind, Hit, SorcerersRing as Ability};
use crate::animation::{AnimationSource, slot};
use crate::effect::{StunEffect, ring::Visuals};
use crate::projectile::Shot;
use crate::{Actor, Animation, EventRuntime, GameWorld, Operation, PlayerSize, ResourceLibrary};
use anyhow::{Context, Result};
use resonance_content::{
    field::{FIELD_SERVICE_MOTION_RESOURCE_BASE, RingScenery, ServiceMotion},
    field_audio::ServiceCue,
};

const WINDUP_TICKS: u32 = 8;
const BOMB_PLACEMENT_TICKS: u32 = 9;
const SHRINK_TRANSFORM_TICKS: u32 = 3;
const RECOVERY_AFTER_LAUNCH_TICKS: u32 = 24;
const MIST_RECOVERY_TICKS: u32 = 62;
const MUZZLE_HEIGHT: f32 = 100.;
const CONTACT_RADIUS: f32 = 20.;
const ORB_FLIGHT_TICKS: u32 = 20;
const ORB_RADIUS: f32 = 30.;
const SUNLIGHT_EXPOSURE_TICKS: u32 = 240;
const SUNLIGHT_MUZZLE_OFFSET: [f32; 3] = [0., -20., 0.];
const BUBBLE_RISE_TICKS: u32 = 60;
const BUBBLE_OPACITY: f32 = 96.;
const BUBBLE_FADE_IN_STEP: f32 = 4.;
const BUBBLE_FADE_OUT_STEP: f32 = 2.;
const BUBBLE_CENTER_HEIGHT: f32 = 90.;
const BUBBLE_FLOAT_HEIGHT: f32 = 40.;
const BUBBLE_LANDING_MARGIN: f32 = 5.;
const BUBBLE_TURN_PER_TICK: f32 = 4.;
const BOMB_FUSE_TICKS: u32 = 180;
const BOMB_BLAST_TICKS: u32 = 60;
const RADAR_TICKS: u32 = 600;
const FADE_TICKS: u32 = 30;
const RING_LAUNCH_FLAG: u16 = 21;
const PROJECTILE_STUN_TICKS: i16 = 240;
const BOMB_RADIUS: f32 = 130.;
const BOMB_STAGGER_RADIUS: f32 = 120.;
const BOMB_STAGGER_TICKS: u32 = 62;
const EARTHQUAKE_COLUMN_TICKS: u32 = 20;
const QUAKE_TICKS: u32 = 120;
const QUAKE_FADE_TICKS: u32 = 40;

#[derive(Default)]
pub(crate) struct Controller {
    casts: Vec<Cast>,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Windup,
    Projectile,
    OrbFlying(ElectricOrbKind),
    OrbHeld(ElectricOrbKind),
    BubbleRising,
    BubbleFloating,
    BubbleFalling,
    Call,
    EarthquakeColumn,
    EarthquakeShake,
    EarthquakeFade,
    Done,
    BombFuse(i32),
    BombBlast(i32),
    RadarScan(i32),
    RadarFade(i32),
    SunlightBeam(Option<(i32, u32)>),
    SunlightFade(u32),
}

#[derive(Clone, Copy, PartialEq)]
enum Pause {
    Freeze,
    Animate,
    Continue,
}
impl Phase {
    fn pause(self) -> Pause {
        match self {
            Self::BubbleFloating
            | Self::BubbleFalling
            | Self::EarthquakeFade
            | Self::BombBlast(_)
            | Self::RadarFade(_)
            | Self::SunlightFade(_) => Pause::Continue,
            Self::OrbFlying(_)
            | Self::OrbHeld(_)
            | Self::SunlightBeam(_)
            | Self::BubbleRising
            | Self::RadarScan(_) => Pause::Animate,
            _ => Pause::Freeze,
        }
    }
}

struct Cast {
    ability: Ability,
    actor: i32,
    source: (i32, u64),
    phase: Phase,
    age: u32,
    recovery: u32,
    pose: Option<Pose>,
    shot: Shot,
    models: Vec<i32>,
    operation: Operation,
    callbacks: Vec<Operation>,
}

struct Pose {
    previous: Option<Animation>,
    scripted: bool,
    started: u32,
    slot: u16,
    flash: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct Projectile {
    lifetime_ticks: u32,
    range_units: f32,
    recovery_ticks: u32,
    sound: ServiceCue,
    volume: u8,
    stun: Option<StunEffect>,
}
fn projectile(ability: Ability) -> Option<Projectile> {
    use Ability::*;
    let standard = Projectile {
        lifetime_ticks: 30,
        range_units: 300.,
        recovery_ticks: WINDUP_TICKS + RECOVERY_AFTER_LAUNCH_TICKS,
        sound: ServiceCue::RingFire,
        volume: 127,
        stun: None,
    };
    Some(match ability {
        Fire => Projectile {
            lifetime_ticks: 20,
            stun: Some(StunEffect::None),
            ..standard
        },
        Water => Projectile {
            lifetime_ticks: 20,
            sound: ServiceCue::RingWater,
            volume: 40,
            ..standard
        },
        Wind => Projectile {
            sound: ServiceCue::RingWind,
            ..standard
        },
        LongRangeFire => Projectile {
            range_units: 500.,
            volume: 0,
            stun: Some(StunEffect::None),
            ..standard
        },
        Mana => Projectile {
            lifetime_ticks: 60,
            range_units: 150.,
            recovery_ticks: MIST_RECOVERY_TICKS,
            sound: ServiceCue::RingMana,
            volume: 64,
            ..standard
        },
        Lightning(_) => Projectile {
            range_units: 400.,
            sound: ServiceCue::RingLightning,
            stun: Some(StunEffect::Lightning),
            ..standard
        },
        Ice => Projectile {
            sound: ServiceCue::RingIce,
            stun: Some(StunEffect::Ice),
            ..standard
        },
        Darkness => Projectile {
            lifetime_ticks: 60,
            recovery_ticks: MIST_RECOVERY_TICKS,
            sound: ServiceCue::RingDarkness,
            stun: Some(StunEffect::Darkness),
            ..standard
        },
        _ => return None,
    })
}

impl EventRuntime {
    /// Activate from field input; saved party state is the only ability selection.
    pub fn activate_ring(&mut self, pressed: bool) -> Result<()> {
        if !self.player_has_control() {
            return Ok(());
        }
        let held = self.world.input.held.contains(crate::input::Button::Ring);
        let Some(party) = self
            .world
            .party
            .as_mut()
            .filter(|p| p.items.get(&super::ITEM).is_some_and(|n| *n > 0))
        else {
            return Ok(());
        };
        let mut ability = party.travel.sorcerers_ring;
        if !pressed && !(held && ability == Ability::Sunlight) {
            return Ok(());
        }
        if ability == Ability::Disabled {
            ability = Ability::Fire;
            party.travel.sorcerers_ring = ability;
        }
        let capacity = if matches!(ability, Ability::ElectricOrb(_)) {
            2
        } else {
            1
        };
        if self
            .world
            .ring
            .casts
            .iter()
            .filter(|c| c.phase != Phase::Done)
            .count()
            >= capacity
        {
            return Ok(());
        }
        if ability == Ability::Bomb && self.world.ring_scenery != RingScenery::Bomb {
            return Ok(());
        }
        if ability == Ability::Sunlight && !sunlight_ready(&self.world) {
            return Ok(());
        }
        let cast = Cast::start(self, ability)?;
        self.world.ring.casts.push(cast);
        Ok(())
    }
    pub(crate) fn step_effects(&mut self) -> Result<()> {
        let mut ring = std::mem::take(&mut self.world.ring);
        let result = ring.step(self);
        if result.is_err() {
            ring.cancel(&mut self.world);
        }
        self.world.ring = ring;
        result
    }
}

impl Controller {
    pub(crate) fn bomb(&self) -> Option<i32> {
        self.casts
            .iter()
            .filter(|c| c.operation.is_pending())
            .find_map(Cast::bomb)
    }
    pub(crate) fn blocks_control(&self) -> bool {
        self.casts.iter().any(|cast| match cast.ability {
            Ability::Sunlight => false,
            Ability::Bubble(_) => matches!(cast.phase, Phase::Windup | Phase::BubbleRising),
            _ => cast.pose.is_some() || cast.phase == Phase::Windup,
        })
    }
    pub(crate) fn blocks_menu(&self) -> bool {
        self.casts.iter().any(|c| {
            c.phase != Phase::Done && matches!(c.ability, Ability::Sunlight | Ability::Bubble(_))
        })
    }
    pub(crate) fn pose_tint(&self, actor: i32, tick: u32) -> Option<[u8; 3]> {
        self.casts
            .iter()
            .find(|c| c.source.0 == actor && c.pose.as_ref().is_some_and(|p| p.flash))
            .map(|_| {
                if (tick / 4).is_multiple_of(2) {
                    [66, 66, 255]
                } else {
                    [191; 3]
                }
            })
    }
    fn step(&mut self, events: &mut EventRuntime) -> Result<()> {
        // Recovery can outlive the cast's scene actor.
        for cast in self.casts.iter_mut().filter(|c| c.phase == Phase::Done) {
            cast.update(events)?;
        }
        // Casts and scripted emitters share the scene's stable update order.
        for id in events.world.actor_order.clone() {
            if let Some(cast) = self
                .casts
                .iter_mut()
                .find(|c| c.actor == id && c.phase != Phase::Done)
            {
                cast.update(events)?;
            } else {
                events
                    .world
                    .step_emitter(id, &events.resources)
                    .map_err(anyhow::Error::msg)?;
            }
        }
        self.casts.retain(|cast| cast.operation.is_pending());
        events.world.reap_authored_resources();
        Ok(())
    }
    pub(crate) fn cancel(&mut self, world: &mut GameWorld) {
        for cast in &mut self.casts {
            cast.cancel(world);
        }
        self.casts.clear();
    }
}

impl Cast {
    fn bomb(&self) -> Option<i32> {
        match self.phase {
            Phase::BombFuse(id) | Phase::BombBlast(id) => Some(id),
            _ => None,
        }
    }
    fn start(events: &mut EventRuntime, ability: Ability) -> Result<Self> {
        let world = &mut events.world;
        let actor = world
            .actors
            .get(&world.controlled_actor)
            .context("ring actor is missing")?;
        let source = (world.controlled_actor, actor.instance);
        let profile = projectile(ability);
        let mut cast = Self {
            ability,
            actor: world
                .unaddressable_actor_key()
                .map_err(anyhow::Error::msg)?,
            source,
            phase: Phase::Windup,
            age: 0,
            recovery: profile.map_or(
                match ability {
                    Ability::Earthquake => 140,
                    Ability::AnimalCall(_) => 40,
                    Ability::Bubble(_) => 180,
                    Ability::Bomb => BOMB_PLACEMENT_TICKS + RECOVERY_AFTER_LAUNCH_TICKS,
                    _ => WINDUP_TICKS + RECOVERY_AFTER_LAUNCH_TICKS,
                },
                |p| p.recovery_ticks,
            ),
            pose: None,
            shot: Shot {
                source: source.0,
                position: actor.position,
                velocity: [0.; 3],
                radius: CONTACT_RADIUS,
            },
            models: Vec::new(),
            operation: world.operations.begin().map_err(anyhow::Error::msg)?,
            callbacks: Vec::new(),
        };
        if ability == Ability::Mana {
            const TP_COST: u16 = 10;
            let member =
                &mut world.party.as_mut().unwrap().members[actor.resource.wrapping_sub(1) as usize];
            let Some(tp) = member.tp.checked_sub(TP_COST) else {
                cast.callback(events, Hit::Pulse, true)?;
                cast.finish(&mut events.world);
                return Ok(cast);
            };
            member.tp = tp;
        }
        if ability != Ability::Shrink {
            cast.play_pose(&mut events.world, &events.resources, true)?;
        }
        sound(&mut events.world, ServiceCue::RingPrepare, 127);
        let mut actor = Actor::new(0, cast.shot.position);
        actor.operation = Some(cast.operation.clone());
        actor.visible = false;
        actor.grounded = false;
        actor.collidable = false;
        actor.contact = crate::ActorContact::None;
        actor.casts_shadow = false;
        events.world.insert_actor(cast.actor, actor);
        Ok(cast)
    }
    fn transition(&mut self, phase: Phase) {
        self.phase = phase;
        self.age = 0;
    }
    fn callback(&mut self, events: &mut EventRuntime, hit: Hit, secondary: bool) -> Result<()> {
        if let Some(operation) = events.queue_ring_callback(hit, secondary)? {
            self.callbacks.push(operation);
        }
        Ok(())
    }
    fn update(&mut self, events: &mut EventRuntime) -> Result<()> {
        let exists = events
            .world
            .actors
            .get(&self.source.0)
            .is_some_and(|a| a.instance == self.source.1);
        if exists {
            let pause = self.phase.pause();
            let advancing = !events.world.mapped_input_disabled || pause == Pause::Continue;
            if advancing {
                if self.ability != Ability::Sunlight {
                    self.recovery = self.recovery.saturating_sub(1);
                }
                if self.recovery == 0 {
                    self.restore_pose(&mut events.world);
                }
                self.advance(events)?;
            }
            if advancing || pause == Pause::Animate {
                self.update_position(&mut events.world, &events.resources)?;
                self.update_appearance(&mut events.world)?;
            }
        } else if self.callbacks.iter().any(Operation::is_pending) {
            // A hit callback owns its remaining work after replacing its caster.
            self.finish(&mut events.world);
            self.pose = None;
        } else {
            self.cancel(&mut events.world);
        }
        if self.operation.is_pending()
            && self.phase == Phase::Done
            && self.pose.is_none()
            && !self.callbacks.iter().any(Operation::is_pending)
            && !self.has_particles(&events.world)
        {
            self.operation.complete(None).map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
    fn advance(&mut self, events: &mut EventRuntime) -> Result<()> {
        if self.phase == Phase::Done {
            return Ok(());
        }
        self.age += 1;
        if self.phase == Phase::Windup {
            if self.ability == Ability::Shrink {
                if self.age == 1 {
                    self.play_pose(&mut events.world, &events.resources, true)?;
                } else if self.age == SHRINK_TRANSFORM_TICKS {
                    events.world.player_size = match events.world.player_size {
                        PlayerSize::Normal => {
                            sound(&mut events.world, ServiceCue::RingShrink, 127);
                            PlayerSize::Small
                        }
                        PlayerSize::Small => PlayerSize::Normal,
                    };
                    self.callback(events, Hit::Pulse, false)?;
                    self.finish(&mut events.world);
                }
                return Ok(());
            }
            let windup = match self.ability {
                Ability::Radar => 20,
                Ability::Bomb => BOMB_PLACEMENT_TICKS,
                _ => WINDUP_TICKS,
            };
            if self.age >= windup {
                self.launch(events)?;
            }
            return Ok(());
        }
        if self.phase == Phase::Projectile {
            let profile = projectile(self.ability).expect("projectile ability");
            events.world.event_flags.remove(&RING_LAUNCH_FLAG);
            let world = &mut events.world;
            let mut visuals = Visuals::new(&self.shot, &self.operation, world);
            visuals.shot(
                self.ability,
                self.age as i32,
                false,
                &mut world.random_state,
            );
            visuals.publish(world).map_err(anyhow::Error::msg)?;
            if self.ability == Ability::Wind {
                let shot = &self.shot;
                let mut acceleration = shot.velocity.map(|v| v * 0.02);
                acceleration[2] -= 0.1;
                world
                    .actors
                    .get_mut(&self.source.0)
                    .unwrap()
                    .chain_impulses
                    .insert(
                        world.tick + 1,
                        crate::projectile::ChainImpulse {
                            acceleration,
                            operation: self.operation.clone(),
                        },
                    );
            }
            if let Some(effect) = profile.stun {
                self.stun(world, CONTACT_RADIUS, PROJECTILE_STUN_TICKS, effect);
            }
            let contact = contact(events, &mut self.shot)?;
            if contact.is_none() && self.age < profile.lifetime_ticks {
                self.shot.advance();
            }
            if contact.is_some() || self.age >= profile.lifetime_ticks {
                let mut visuals = Visuals::new(&self.shot, &self.operation, &events.world);
                visuals.shot(
                    self.ability,
                    self.age as i32,
                    true,
                    &mut events.world.random_state,
                );
                visuals
                    .publish(&mut events.world)
                    .map_err(anyhow::Error::msg)?;
                if self.ability == Ability::Water {
                    sound(&mut events.world, ServiceCue::RingSplash, 127);
                }
                self.finish(&mut events.world);
                if let Some(Some(actor)) = contact {
                    self.callback(events, Hit::Actor(actor as i16), false)?;
                }
            }
            return Ok(());
        }
        match self.phase {
            Phase::OrbFlying(kind) | Phase::OrbHeld(kind) => {
                let (hold, duration, effect) = match kind {
                    ElectricOrbKind::Sylvarant => (300, 420, StunEffect::Electric),
                    ElectricOrbKind::Tethealla => (120, 300, StunEffect::TetheallaElectric),
                };
                self.stun(&mut events.world, ORB_RADIUS, duration, effect);
                if matches!(self.phase, Phase::OrbFlying(_)) {
                    if self.age < ORB_FLIGHT_TICKS {
                        let mut visuals = Visuals::new(&self.shot, &self.operation, &events.world);
                        visuals.electric(true, self.age - 1, &mut events.world.random_state);
                        visuals
                            .publish(&mut events.world)
                            .map_err(anyhow::Error::msg)?;
                    }
                    let hit = contact(events, &mut self.shot)?;
                    let stopped = hit.is_some() || self.age >= ORB_FLIGHT_TICKS;
                    if stopped {
                        let mut visuals = Visuals::new(&self.shot, &self.operation, &events.world);
                        visuals.electric(false, 0, &mut events.world.random_state);
                        visuals
                            .publish(&mut events.world)
                            .map_err(anyhow::Error::msg)?;
                    }
                    if hit.is_none() {
                        self.shot.advance();
                    }
                    if stopped {
                        self.transition(Phase::OrbHeld(kind));
                        self.shot.velocity = [0.; 3];
                        if let Some(Some(actor)) = hit {
                            self.callback(events, Hit::Actor(actor as i16), false)?;
                        }
                    }
                } else if self.age > hold
                    || self
                        .shot
                        .touches_actor(&events.world.actors[&self.source.0], CONTACT_RADIUS)
                {
                    self.finish(&mut events.world);
                }
            }
            Phase::BombFuse(bomb) if self.age >= BOMB_FUSE_TICKS => {
                events.world.actors.get_mut(&bomb).unwrap().visible = false;
                let victims: Vec<_> = self
                    .shot
                    .targets(&events.world, BOMB_RADIUS)
                    .filter(|(id, _)| events.world.actors[id].enemy.is_some())
                    .map(|(id, _)| id)
                    .collect();
                for id in victims {
                    events.world.remove_actor(id);
                }
                if self
                    .shot
                    .touches_actor(&events.world.actors[&self.source.0], BOMB_STAGGER_RADIUS)
                {
                    self.play_pose(&mut events.world, &events.resources, false)?;
                    self.recovery = BOMB_STAGGER_TICKS;
                }
                sound(&mut events.world, ServiceCue::RingBombBlast, 127);
                sound(&mut events.world, ServiceCue::RingBombDebris, 127);
                events.world.rumble = Some(
                    crate::rumble::Rumble::new(0, 8, true, events.world.tick)
                        .map_err(anyhow::Error::msg)?,
                );
                self.transition(Phase::BombBlast(bomb));
                self.callback(events, Hit::Pulse, false)?;
            }
            Phase::BombBlast(_) => {
                shake(
                    &mut events.world,
                    ((BOMB_BLAST_TICKS + 1).saturating_sub(self.age) / 4) as f32,
                );
                if self.age >= BOMB_BLAST_TICKS {
                    self.finish(&mut events.world);
                }
            }
            Phase::RadarScan(handle) if self.age >= RADAR_TICKS - FADE_TICKS => {
                self.callback(events, Hit::Pulse, true)?;
                self.transition(Phase::RadarFade(handle));
            }
            Phase::RadarFade(_) if self.age >= FADE_TICKS => self.finish(&mut events.world),
            Phase::SunlightBeam(_) if !sunlight_ready(&events.world) => {
                self.transition(Phase::SunlightFade(self.age));
            }
            Phase::SunlightFade(_) if self.age >= FADE_TICKS => {
                self.finish(&mut events.world);
                self.restore_pose(&mut events.world);
            }
            Phase::SunlightBeam(previous) => {
                let height = muzzle_height(
                    &events.world,
                    &events.resources,
                    self.source.0,
                    SUNLIGHT_MUZZLE_OFFSET,
                )?;
                self.follow(&events.world, height);
                let target = target_ahead(&events.world, &self.shot, 200., 100.);
                let exposure = target.map(|id| {
                    (
                        id,
                        previous
                            .filter(|(old, _)| *old == id)
                            .map_or(1, |(_, age)| age + 1),
                    )
                });
                self.phase = Phase::SunlightBeam(exposure);
                if let Some((id, age)) = exposure {
                    for (index, duration) in [1, SUNLIGHT_EXPOSURE_TICKS].into_iter().enumerate() {
                        if age == duration {
                            self.callback(events, Hit::Actor(id as i16), index == 1)?;
                        }
                    }
                }
            }
            Phase::BubbleRising if self.age >= BUBBLE_RISE_TICKS => {
                self.transition(Phase::BubbleFloating);
                self.callback(events, Hit::Pulse, false)?;
            }
            Phase::BubbleFloating
                if events.world.party.as_ref().unwrap().travel.sorcerers_ring
                    != Ability::Bubble(BubblePhase::Float) =>
            {
                self.transition(Phase::BubbleFalling);
            }
            Phase::BubbleFalling if self.age >= BUBBLE_RISE_TICKS => self.finish(&mut events.world),
            Phase::Call if self.age >= 30 => {
                self.finish(&mut events.world);
                self.callback(events, Hit::Pulse, false)?;
            }
            Phase::EarthquakeColumn if self.age == EARTHQUAKE_COLUMN_TICKS => {
                shake(&mut events.world, (QUAKE_TICKS / 10) as f32);
            }
            Phase::EarthquakeColumn if self.age > EARTHQUAKE_COLUMN_TICKS => {
                self.transition(Phase::EarthquakeShake);
                self.age = 1;
                shake(&mut events.world, ((QUAKE_TICKS - self.age) / 10) as f32);
                events.world.rumble = Some(
                    crate::rumble::Rumble::new(0, 50, true, events.world.tick)
                        .map_err(anyhow::Error::msg)?,
                );
            }
            Phase::EarthquakeShake => {
                shake(&mut events.world, ((QUAKE_TICKS - self.age) / 10) as f32);
                if self.age == QUAKE_TICKS - QUAKE_FADE_TICKS {
                    self.callback(events, Hit::Pulse, false)?;
                    self.transition(Phase::EarthquakeFade);
                }
            }
            Phase::EarthquakeFade => {
                shake(
                    &mut events.world,
                    (QUAKE_FADE_TICKS.saturating_sub(self.age) / 10) as f32,
                );
                if self.age >= QUAKE_FADE_TICKS {
                    self.finish(&mut events.world);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn launch(&mut self, events: &mut EventRuntime) -> Result<()> {
        let profile = projectile(self.ability);
        let world = &mut events.world;
        let actor = &world.actors[&self.source.0];
        let speed = profile.map_or(
            match self.ability {
                Ability::ElectricOrb(ElectricOrbKind::Sylvarant) => 22.5,
                Ability::ElectricOrb(ElectricOrbKind::Tethealla) => 15.,
                _ => 0.,
            },
            |p| p.range_units / p.lifetime_ticks as f32,
        );
        let height = if matches!(
            self.ability,
            Ability::Bomb | Ability::Earthquake | Ability::Radar
        ) {
            0.
        } else {
            muzzle_height(world, &events.resources, self.source.0, [0.; 3])?
        };
        let (sin, cos) = actor.heading.to_radians().sin_cos();
        self.shot.position = actor.position;
        self.shot.position[2] += height;
        self.shot.velocity = [sin * speed, -cos * speed, 0.];
        self.shot.radius = if matches!(self.ability, Ability::ElectricOrb(_)) {
            ORB_RADIUS
        } else {
            CONTACT_RADIUS
        };
        self.age = 0;
        if let Some(profile) = profile {
            self.phase = Phase::Projectile;
            sound(world, profile.sound, profile.volume);
            if self.ability == Ability::Fire {
                world.event_flags.insert(RING_LAUNCH_FLAG);
            }
        }
        match self.ability {
            Ability::ElectricOrb(kind) => {
                self.phase = Phase::OrbFlying(kind);
                sound(world, ServiceCue::RingElectric, 127);
            }
            Ability::Bomb => {
                let resource = resonance_content::field::RING_BOMB_RESOURCE;
                let model = events
                    .resources
                    .model(resource)
                    .context("bomb model is not prepared")?;
                let mut bomb = Actor::new(resource, self.shot.position);
                bomb.operation = Some(self.operation.clone());
                bomb.collidable = false;
                bomb.contact = crate::ActorContact::None;
                bomb.animation = model.clips.get(&slot::IDLE).map(|clip| {
                    Animation::new(resource, slot::IDLE, clip.duration_ticks, world.tick)
                });
                let id = world
                    .unaddressable_actor_key()
                    .map_err(anyhow::Error::msg)?;
                world.insert_actor(id, bomb);
                self.phase = Phase::BombFuse(id);
                sound(world, ServiceCue::RingBombPlace, 127);
            }
            Ability::Sunlight => self.phase = Phase::SunlightBeam(None),
            Ability::Bubble(_) => {
                self.phase = Phase::BubbleRising;
                sound(world, ServiceCue::RingBubble, 127);
            }
            Ability::Earthquake => {
                self.phase = Phase::EarthquakeColumn;
                sound(world, ServiceCue::RingEarthquake, 127);
            }
            Ability::Sound | Ability::AnimalCall(_) => {
                self.phase = Phase::Call;
                if self.ability == Ability::Sound {
                    sound(world, ServiceCue::RingSound, 127);
                }
            }
            Ability::Radar => {
                sound(world, ServiceCue::RingRadar, 64);
                let handle = world.allocate_effect().map_err(anyhow::Error::msg)?;
                self.phase = Phase::RadarScan(handle);
                self.callback(events, Hit::Pulse, false)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn stun(&self, world: &mut GameWorld, radius: f32, ticks: i16, effect: StunEffect) {
        let targets: Vec<_> = self.shot.targets(world, radius).map(|(id, _)| id).collect();
        for id in targets {
            let actor = world.actors.get_mut(&id).unwrap();
            if let Some(enemy) = &mut actor.enemy {
                enemy.pause_ticks = ticks;
                enemy.reaction = effect;
                actor.motion = None;
            }
        }
    }
    fn follow(&mut self, world: &GameWorld, height: f32) {
        let mut position = world.actors[&self.source.0].position;
        position[2] += height;
        self.shot.position = position;
    }
    fn update_position(
        &mut self,
        world: &mut GameWorld,
        resources: &ResourceLibrary,
    ) -> Result<()> {
        match self.phase {
            Phase::SunlightBeam(_) | Phase::SunlightFade(_) => {
                let offset = if matches!(self.phase, Phase::SunlightBeam(_)) && self.age == 0 {
                    [0.; 3]
                } else {
                    SUNLIGHT_MUZZLE_OFFSET
                };
                self.follow(
                    world,
                    muzzle_height(world, resources, self.source.0, offset)?,
                );
            }
            Phase::BubbleRising | Phase::BubbleFloating | Phase::BubbleFalling => {
                let angle = (world.effect_tick as f32 * BUBBLE_TURN_PER_TICK).to_radians();
                let previous_lift = world.actors[&self.source.0]
                    .visual_lift
                    .as_ref()
                    .map_or(0., |lift| lift.height);
                let lift = match self.phase {
                    Phase::BubbleRising => {
                        BUBBLE_FLOAT_HEIGHT * fraction(self.age, BUBBLE_RISE_TICKS)
                    }
                    Phase::BubbleFloating if self.age == 0 => BUBBLE_FLOAT_HEIGHT,
                    Phase::BubbleFloating if self.age > 0 => previous_lift + angle.sin(),
                    Phase::BubbleFalling if self.age > 0 => {
                        let remaining = BUBBLE_RISE_TICKS.saturating_sub(self.age);
                        (previous_lift - (previous_lift + BUBBLE_LANDING_MARGIN) / remaining as f32)
                            .max(0.)
                    }
                    _ => previous_lift,
                };
                world.actors.get_mut(&self.source.0).unwrap().visual_lift =
                    Some(crate::projectile::VisualLift {
                        height: lift,
                        operation: self.operation.clone(),
                    });
                self.follow(world, BUBBLE_CENTER_HEIGHT + previous_lift);
            }
            _ => {}
        }
        Ok(())
    }
    fn update_appearance(&mut self, world: &mut GameWorld) -> Result<()> {
        if matches!(self.phase, Phase::Windup | Phase::Done) {
            return Ok(());
        }
        let mut visuals = Visuals::new(&self.shot, &self.operation, world);
        match self.phase {
            Phase::OrbHeld(_) if self.age > 0 => {
                visuals.electric(false, self.age, &mut world.random_state)
            }
            Phase::BombBlast(_) if (1..=21).contains(&self.age) => {
                visuals.bomb(self.age == 1, &mut world.random_state)
            }
            Phase::RadarScan(handle) | Phase::RadarFade(handle) => {
                let amount = if matches!(self.phase, Phase::RadarFade(_)) {
                    1. - fraction(self.age.saturating_sub(1), FADE_TICKS + 1)
                } else {
                    fraction(self.age, FADE_TICKS + 1)
                };
                if self.age > 0 {
                    world.fog_effects.insert(
                        handle,
                        crate::camera::FogEffect {
                            fog: radar_fog(amount),
                            operation: self.operation.clone(),
                        },
                    );
                }
            }
            Phase::SunlightBeam(_) | Phase::SunlightFade(_) => {
                let (radius, reach, opacity, rotation_tick) =
                    if let Phase::SunlightFade(beam_ticks) = self.phase {
                        let remaining = FADE_TICKS.saturating_sub(self.age + 1);
                        (
                            (2. * remaining as f32 / FADE_TICKS as f32).max(0.01),
                            2.,
                            180 * remaining / FADE_TICKS,
                            beam_ticks + self.age,
                        )
                    } else {
                        let growth = fraction(self.age, 10);
                        let opacity = if self.age == 0 {
                            0
                        } else {
                            (16 + 15 * self.age).min(180)
                        };
                        ((0.1 + 2. * growth).min(2.), 1. + growth, opacity, self.age)
                    };
                let position = self.shot.position;
                let heading = world.actors[&self.source.0].heading;
                for (layer, width, length, twist, alpha, blue) in
                    [(0, 1., 1., -1., 180, 64), (1, 1.2, 1.1, 1., 108, 16)]
                {
                    let model = self.model(world, layer)?;
                    model.position = position;
                    model.rotation = [-90., twist * rotation_tick as f32, heading];
                    model.scale = [width * radius, width * radius, length * reach];
                    model.rgba = [64, 64, blue, (alpha * opacity / 180) as u8];
                    model.blend = crate::effect::Blend::Additive;
                }
            }
            Phase::BubbleRising | Phase::BubbleFloating | Phase::BubbleFalling => {
                let angle = (world.effect_tick as f32 * BUBBLE_TURN_PER_TICK).to_radians();
                let position = self.shot.position;
                let model = self.model(world, 0)?;
                model.position = position;
                model.scale = [1.1 + angle.sin() * 0.2, 1., 1.1 + angle.cos() * 0.2];
                let opacity = match self.phase {
                    Phase::BubbleRising => {
                        (self.age as f32 * BUBBLE_FADE_IN_STEP).min(BUBBLE_OPACITY)
                    }
                    Phase::BubbleFalling => {
                        (BUBBLE_OPACITY - self.age as f32 * BUBBLE_FADE_OUT_STEP).max(0.)
                    }
                    _ => BUBBLE_OPACITY,
                };
                model.rgba = [128, 192, 192, opacity as u8];
                model.orientation = crate::effect::SpriteOrientation::Camera;
            }
            Phase::Call if self.age == 1 => {
                let color = match self.ability {
                    Ability::AnimalCall(CallColor::Pink) => Some([248, 96, 184]),
                    Ability::AnimalCall(CallColor::White) => Some([255; 3]),
                    Ability::AnimalCall(CallColor::Blue) => Some([32, 32, 255]),
                    _ => None,
                };
                visuals.pulse(color);
            }
            Phase::EarthquakeColumn if self.age > 0 => {
                visuals.ground_ring((EARTHQUAKE_COLUMN_TICKS + 1 - self.age) as f32 * 6.);
                if self.age == EARTHQUAKE_COLUMN_TICKS {
                    visuals.ground_ripple();
                }
            }
            _ => {}
        }
        visuals.publish(world).map_err(anyhow::Error::msg)
    }
    fn play_pose(
        &mut self,
        world: &mut GameWorld,
        resources: &ResourceLibrary,
        cast: bool,
    ) -> Result<()> {
        let actor = world
            .actors
            .get_mut(&self.source.0)
            .context("ring actor is missing")?;
        let (resource, slot, source) = if cast {
            (
                FIELD_SERVICE_MOTION_RESOURCE_BASE + actor.resource,
                ServiceMotion::CastRing as u16,
                AnimationSource::Resource,
            )
        } else {
            let model = resources
                .model(actor.resource)
                .context("ring actor model is missing")?;
            (
                actor.resource,
                if model.clips.contains_key(&slot::STAGGER) {
                    slot::STAGGER
                } else {
                    slot::IDLE
                },
                AnimationSource::Model,
            )
        };
        let mut animation = Animation::new(resource, slot, 0, world.tick);
        animation.source = source;
        animation.duration_ticks = resources
            .animation(&animation)
            .context("ring animation is not prepared")?
            .duration_ticks;
        animation.repeat = false;
        animation.blend_ticks = if cast { 4 } else { 0 };
        self.pose = Some(Pose {
            previous: actor.animation.take(),
            scripted: actor.scripted_animation,
            started: world.tick,
            slot,
            flash: !cast,
        });
        actor.animation = Some(animation);
        actor.scripted_animation = true;
        actor.motion = None;
        Ok(())
    }
    fn restore_pose(&mut self, world: &mut GameWorld) {
        if let Some(pose) = self.pose.take()
            && let Some(actor) = world
                .actors
                .get_mut(&self.source.0)
                .filter(|a| a.instance == self.source.1)
            && actor
                .animation
                .as_ref()
                .is_some_and(|a| a.start_tick == pose.started && a.slot == pose.slot)
        {
            if !pose.scripted {
                actor.animation = pose.previous;
            }
            actor.scripted_animation = pose.scripted;
            if let Some(ai) = &mut actor.autonomy {
                ai.activity = crate::Activity::Idle;
                ai.initialized = false;
            }
        }
    }
    fn finish(&mut self, world: &mut GameWorld) {
        let phase = self.phase;
        self.transition(Phase::Done);
        world.remove_actor(self.actor);
        for handle in self.models.drain(..) {
            world.model_particles.remove(&handle);
        }
        match phase {
            Phase::RadarScan(handle) | Phase::RadarFade(handle) => {
                world.fog_effects.remove(&handle);
            }
            Phase::BombFuse(id) | Phase::BombBlast(id) => {
                world.remove_actor(id);
            }
            _ => {}
        }
        if let Some(actor) = world.actors.get_mut(&self.source.0)
            && actor
                .visual_lift
                .as_ref()
                .is_some_and(|lift| lift.operation.id() == self.operation.id())
        {
            actor.visual_lift = None;
        }
    }
    fn has_particles(&self, world: &GameWorld) -> bool {
        world
            .billboards
            .values()
            .filter_map(|p| p.operation.as_ref())
            .chain(
                world
                    .refractions
                    .values()
                    .filter_map(|p| p.operation.as_ref()),
            )
            .any(|op| op.id() == self.operation.id())
    }
    fn model<'a>(
        &mut self,
        world: &'a mut GameWorld,
        layer: usize,
    ) -> Result<&'a mut crate::model_particle::ModelParticle> {
        if self.models.len() <= layer {
            let particle = crate::model_particle::ModelParticle::scoped(
                resonance_content::field::RING_BEAM_RESOURCE,
                self.operation.clone(),
            );
            let handle = world
                .emit_model_particle(particle)
                .map_err(anyhow::Error::msg)?;
            anyhow::ensure!(handle != 0, "ring model effect pool is full");
            self.models.push(handle);
        }
        world
            .model_particles
            .get_mut(&self.models[layer])
            .context("ring model is missing")
    }
    fn cancel(&mut self, world: &mut GameWorld) {
        self.operation.cancel();
        for callback in &self.callbacks {
            callback.cancel();
        }
        self.restore_pose(world);
        self.phase = Phase::Done;
        world.reap_authored_resources();
    }
}

fn fraction(age: u32, duration: u32) -> f32 {
    (age as f32 / duration as f32).min(1.)
}
fn sound(world: &mut GameWorld, cue: ServiceCue, volume: u8) {
    if volume != 0 {
        world.audio_commands.push(crate::AudioCommand::Sound {
            id: cue as i16,
            pan: 64,
            volume,
            slot: None,
        });
    }
}
fn shake(world: &mut GameWorld, amount: f32) {
    world
        .field_camera
        .get_or_insert_default()
        .shake
        .configure(amount, 8, 8);
}
fn radar_fog(amount: f32) -> crate::camera::Fog {
    const CLEAR_START: f32 = 9900.;
    const SCAN_START: f32 = -226.667;
    crate::camera::Fog {
        start: CLEAR_START + (SCAN_START - CLEAR_START) * amount,
        end: 10000.,
        color: [10, 255, 10],
    }
}
fn sunlight_ready(world: &GameWorld) -> bool {
    world.ring_scenery == RingScenery::Sunlight
        && world.party.as_ref().is_some_and(|p| {
            p.travel.sorcerers_ring == Ability::Sunlight && p.travel.ring_timer > 0
        })
        && world.input.held.contains(crate::input::Button::Ring)
}
fn contact(events: &mut EventRuntime, shot: &mut Shot) -> Result<Option<Option<i32>>> {
    let target = shot.nearest_target(&events.world, shot.radius);
    let impact = target
        .map(|(_, time)| time)
        .or_else(|| shot.barrier(&events.world));
    if let Some(time) = impact {
        shot.position = std::array::from_fn(|i| shot.position[i] + shot.velocity[i] * time);
        return Ok(Some(target.map(|(actor, _)| actor)));
    }
    let (position, radius) = (shot.position, shot.radius);
    for index in 0..events.world.triggers.len() {
        let trigger = &mut events.world.triggers[index];
        if !trigger.ring_barrier {
            continue;
        }
        if !trigger.touches(position, radius) {
            trigger.activations = 0;
            continue;
        }
        if events.contact_trigger(index)? {
            return Ok(Some(None));
        }
    }
    Ok(None)
}
fn target_ahead(world: &GameWorld, shot: &Shot, distance: f32, radius: f32) -> Option<i32> {
    let actor = &world.actors[&shot.source];
    let (sin, cos) = actor.heading.to_radians().sin_cos();
    let position = [
        shot.position[0] + sin * distance,
        shot.position[1] - cos * distance,
        shot.position[2],
    ];
    world
        .actors
        .iter()
        .filter(|(_, a)| a.role == crate::ActorRole::Interaction && a.projectile_target())
        .filter(|(_, a)| a.projectile_contact(position, [0.; 3], radius).is_some())
        .min_by(|(_, a), (_, b)| {
            let distance = |a: &Actor| {
                a.position
                    .iter()
                    .zip(position)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(&id, _)| id)
}
fn muzzle_height(
    world: &GameWorld,
    resources: &ResourceLibrary,
    id: i32,
    offset: [f32; 3],
) -> Result<f32> {
    const BONE: &str = "Bone_sebone03";
    let actor = &world.actors[&id];
    let model = resources
        .model(actor.model_resource())
        .context("ring actor model is missing")?;
    if !model.names.iter().any(|name| name == BONE) {
        return Ok(MUZZLE_HEIGHT);
    }
    let animation = actor
        .animation
        .as_ref()
        .context("ring animation is missing")?;
    let clip = resources
        .animation(animation)
        .context("ring clip is missing")?;
    let pose = resources
        .attachment_pose(actor)
        .context("ring attachment pose is missing")?;
    let sample = animation.sample(
        world.tick,
        model.attachment_pose_delay,
        clip.duration_ticks as f32,
    );
    Ok(pose.sample_offset(BONE, sample, offset)?[2] * actor.scale_percent[2] as f32 / 100.)
}
