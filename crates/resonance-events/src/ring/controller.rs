//! One owner for ring timing, contacts, recovery and visual lifetimes.
use super::{BubblePhase, CallColor, ElectricOrbKind, Hit, SorcerersRing as Ability};
use crate::animation::{AnimationSource, slot};
use crate::effect::{StunEffect, ring::Visuals};
use crate::projectile::Shot;
use crate::{Actor, Animation, EventRuntime, GameWorld, Operation, PlayerSize, ResourceLibrary};
use anyhow::{Context, Result};
use resonance_content::field::{FIELD_SERVICE_MOTION_RESOURCE_BASE, ServiceMotion};

const WINDUP_TICKS: u32 = 8;
const CAST_RECOVERY_TICKS: u32 = 30;
const MUZZLE_HEIGHT: f32 = 100.;
const CONTACT_RADIUS: f32 = 20.;
const ORB_FLIGHT_TICKS: u32 = 20;
const ORB_RADIUS: f32 = 30.;
const SUNLIGHT_EXPOSURE_TICKS: u32 = 240;
const BUBBLE_RISE_TICKS: u32 = 60;
const BUBBLE_OPACITY: f32 = 96.;
const BUBBLE_FADE_IN_STEP: f32 = 5.;
const BUBBLE_FADE_OUT_STEP: f32 = 2.;
const BUBBLE_CENTER_HEIGHT: f32 = 90.;
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

#[derive(Default)]
pub(crate) struct Controller {
    casts: Vec<Cast>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Windup,
    Active,
    Hold,
    Release,
    Done,
}

struct Cast {
    ability: Ability,
    source: (i32, u64),
    phase: Phase,
    age: u32,
    recovery: u32,
    pose: Option<Pose>,
    shot: Shot,
    models: Vec<i32>,
    fog: Option<i32>,
    operation: Operation,
    callbacks: Vec<Operation>,
    bomb: Option<i32>,
    exposure: Option<(i32, u32)>,
}

struct Pose {
    previous: Option<Animation>,
    scripted: bool,
    started: u32,
    slot: u16,
    flash: bool,
}

struct Projectile {
    lifetime_ticks: u32,
    range_units: f32,
    recovery_ticks: u32,
    sound: i16,
    volume: u8,
    stun: Option<StunEffect>,
}
fn projectile(ability: Ability) -> Option<Projectile> {
    use Ability::*;
    let standard = Projectile {
        lifetime_ticks: 30,
        range_units: 300.,
        recovery_ticks: CAST_RECOVERY_TICKS,
        sound: 32,
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
            sound: 110,
            volume: 40,
            ..standard
        },
        Wind => Projectile {
            sound: 87,
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
            recovery_ticks: 60,
            sound: 270,
            volume: 64,
            ..standard
        },
        Lightning(_) => Projectile {
            range_units: 400.,
            sound: 42,
            stun: Some(StunEffect::Lightning),
            ..standard
        },
        Ice => Projectile {
            sound: 43,
            stun: Some(StunEffect::Ice),
            ..standard
        },
        Darkness => Projectile {
            lifetime_ticks: 60,
            recovery_ticks: 60,
            sound: 236,
            stun: Some(StunEffect::Darkness),
            ..standard
        },
        _ => return None,
    })
}

impl GameWorld {
    pub fn ring_shadows(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.ring
            .casts
            .iter()
            .filter(|cast| {
                matches!(cast.ability, Ability::ElectricOrb(_))
                    && !matches!(cast.phase, Phase::Windup | Phase::Done)
            })
            .map(|cast| cast.shot.position)
    }
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
        if ability == Ability::Bomb
            && !self
                .world
                .current_field
                .is_some_and(|id| (412..=415).contains(&id))
        {
            return Ok(());
        }
        if ability == Ability::Sunlight && !sunlight_ready(&self.world) {
            return Ok(());
        }
        let cast = Cast::start(self, ability)?;
        self.world.ring.casts.push(cast);
        Ok(())
    }
    pub(crate) fn step_ring(&mut self) -> Result<()> {
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
            .find_map(|c| c.bomb)
    }
    pub(crate) fn blocks_control(&self) -> bool {
        self.casts.iter().any(|cast| {
            cast.pose.is_some()
                && cast.ability != Ability::Sunlight
                && !(matches!(cast.ability, Ability::Bubble(_))
                    && cast.phase != Phase::Windup
                    && cast.phase != Phase::Active)
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
        for cast in &mut self.casts {
            let exists = events
                .world
                .actors
                .get(&cast.source.0)
                .is_some_and(|a| a.instance == cast.source.1);
            if !exists {
                // A hit may start a scene that replaces its caster. That scene
                // owns its remaining work even after the projectile is gone.
                if cast.callbacks.iter().any(Operation::is_pending) {
                    cast.finish(&mut events.world);
                    cast.pose = None;
                } else {
                    cast.cancel(&mut events.world);
                }
                continue;
            }
            // Floating and release motion continue while a scene owns player input.
            if !events.world.mapped_input_disabled
                || cast.phase == Phase::Release
                || (matches!(cast.ability, Ability::Bubble(_)) && cast.phase == Phase::Hold)
            {
                if cast.ability != Ability::Sunlight {
                    cast.recovery = cast.recovery.saturating_sub(1);
                }
                if cast.recovery == 0 {
                    cast.restore_pose(&mut events.world);
                }
                cast.step(events)?;
            }
            if !events.world.mapped_input_disabled
                || matches!(
                    cast.ability,
                    Ability::ElectricOrb(_)
                        | Ability::Sunlight
                        | Ability::Bubble(_)
                        | Ability::Radar
                )
                || cast.phase == Phase::Release
            {
                cast.draw(&mut events.world, &events.resources)?;
            }
        }
        for cast in &self.casts {
            if cast.operation.is_pending()
                && cast.phase == Phase::Done
                && cast.pose.is_none()
                && !cast.callbacks.iter().any(Operation::is_pending)
                && !cast.has_particles(&events.world)
            {
                cast.operation.complete(None).map_err(anyhow::Error::msg)?;
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
    fn start(events: &mut EventRuntime, ability: Ability) -> Result<Self> {
        let world = &mut events.world;
        let actor = world
            .actors
            .get(&world.controlled_actor)
            .context("ring actor is missing")?;
        let source = (world.controlled_actor, actor.instance);
        let mut cast = Self {
            ability,
            source,
            phase: Phase::Windup,
            age: 0,
            recovery: projectile(ability).map_or(
                match ability {
                    Ability::Earthquake => 140,
                    Ability::AnimalCall(_) => 40,
                    Ability::Bubble(_) => 180,
                    _ => CAST_RECOVERY_TICKS,
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
            fog: None,
            operation: world.operations.begin().map_err(anyhow::Error::msg)?,
            callbacks: Vec::new(),
            bomb: None,
            exposure: None,
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
        cast.play_pose(&mut events.world, &events.resources, true)?;
        sound(&mut events.world, 31, 127);
        if ability == Ability::Shrink {
            events.world.player_size = match events.world.player_size {
                PlayerSize::Normal => {
                    sound(&mut events.world, 287, 127);
                    PlayerSize::Small
                }
                PlayerSize::Small => PlayerSize::Normal,
            };
            cast.callback(events, Hit::Pulse, false)?;
            cast.finish(&mut events.world);
        }
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
    fn step(&mut self, events: &mut EventRuntime) -> Result<()> {
        if self.phase == Phase::Done {
            return Ok(());
        }
        self.age += 1;
        if self.phase == Phase::Windup {
            let windup = if self.ability == Ability::Radar {
                20
            } else {
                WINDUP_TICKS
            };
            if self.age >= windup {
                self.launch(events)?;
            }
            return Ok(());
        }
        if let Some(profile) = projectile(self.ability) {
            events.world.event_flags.remove(&RING_LAUNCH_FLAG);
            let world = &mut events.world;
            let mut visuals = Visuals::new(&self.shot, &self.operation, world.tick);
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
            if contact.is_none() {
                self.shot.advance();
            }
            if contact.is_some() || self.age >= profile.lifetime_ticks {
                let mut visuals = Visuals::new(&self.shot, &self.operation, events.world.tick);
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
                    sound(&mut events.world, 414, 127);
                }
                self.finish(&mut events.world);
                if let Some(Some(actor)) = contact {
                    self.callback(events, Hit::Actor(actor as i16), false)?;
                }
            }
            return Ok(());
        }
        match self.ability {
            Ability::ElectricOrb(kind) => {
                let (hold, duration, effect) = match kind {
                    ElectricOrbKind::Sylvarant => (300, 420, StunEffect::Electric),
                    ElectricOrbKind::Tethealla => (120, 300, StunEffect::TetheallaElectric),
                };
                self.stun(&mut events.world, ORB_RADIUS, duration, effect);
                if self.phase == Phase::Active {
                    if self.age < ORB_FLIGHT_TICKS {
                        let mut visuals =
                            Visuals::new(&self.shot, &self.operation, events.world.tick);
                        visuals.electric(true, self.age - 1, &mut events.world.random_state);
                        visuals
                            .publish(&mut events.world)
                            .map_err(anyhow::Error::msg)?;
                    }
                    let hit = contact(events, &mut self.shot)?;
                    if hit.is_none() {
                        self.shot.advance();
                    }
                    if hit.is_some() || self.age >= ORB_FLIGHT_TICKS {
                        self.transition(Phase::Hold);
                        self.shot.velocity = [0.; 3];
                        if let Some(Some(actor)) = hit {
                            self.callback(events, Hit::Actor(actor as i16), false)?;
                        }
                    }
                } else if self.age >= hold
                    || self
                        .shot
                        .touches_actor(&events.world.actors[&self.source.0], CONTACT_RADIUS)
                {
                    self.finish(&mut events.world);
                }
            }
            Ability::Bomb => {
                if self.phase == Phase::Active && self.age >= BOMB_FUSE_TICKS {
                    events
                        .world
                        .actors
                        .get_mut(&self.bomb.unwrap())
                        .unwrap()
                        .visible = false;
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
                    sound(&mut events.world, 194, 127);
                    sound(&mut events.world, 330, 127);
                    events.world.rumble = Some(
                        crate::rumble::Rumble::new(0, 8, true, events.world.tick)
                            .map_err(anyhow::Error::msg)?,
                    );
                    self.transition(Phase::Release);
                    self.callback(events, Hit::Pulse, false)?;
                } else if self.phase == Phase::Release {
                    shake(
                        &mut events.world,
                        (BOMB_BLAST_TICKS.saturating_sub(self.age) / 4) as f32,
                    );
                    if self.age >= BOMB_BLAST_TICKS {
                        self.finish(&mut events.world);
                    }
                }
            }
            Ability::Radar => {
                if self.phase == Phase::Active && self.age >= RADAR_TICKS - FADE_TICKS {
                    self.callback(events, Hit::Pulse, true)?;
                    self.transition(Phase::Release);
                } else if self.phase == Phase::Release && self.age >= FADE_TICKS {
                    self.finish(&mut events.world);
                }
            }
            Ability::Sunlight => {
                if self.phase == Phase::Active && !sunlight_ready(&events.world) {
                    self.transition(Phase::Release);
                }
                if self.phase == Phase::Release {
                    if self.age >= FADE_TICKS {
                        self.finish(&mut events.world);
                        self.restore_pose(&mut events.world);
                    }
                } else {
                    let height = muzzle_height(&events.world, &events.resources, self.source.0)?;
                    self.follow(&events.world, height);
                    let target = target_ahead(&events.world, &self.shot, 200., 100.);
                    self.exposure = target.map(|id| {
                        (
                            id,
                            self.exposure
                                .filter(|(old, _)| *old == id)
                                .map_or(1, |(_, age)| age + 1),
                        )
                    });
                    if let Some((id, age)) = self.exposure {
                        for (index, duration) in
                            [1, SUNLIGHT_EXPOSURE_TICKS].into_iter().enumerate()
                        {
                            if age == duration {
                                self.callback(events, Hit::Actor(id as i16), index == 1)?;
                            }
                        }
                    }
                }
            }
            Ability::Bubble(_) => {
                if self.phase == Phase::Active && self.age >= BUBBLE_RISE_TICKS {
                    self.transition(Phase::Hold);
                    self.callback(events, Hit::Pulse, false)?;
                } else if self.phase == Phase::Hold
                    && events.world.party.as_ref().unwrap().travel.sorcerers_ring
                        != Ability::Bubble(BubblePhase::Float)
                {
                    self.transition(Phase::Release);
                }
                if self.phase == Phase::Release && self.age >= BUBBLE_RISE_TICKS {
                    self.finish(&mut events.world);
                }
            }
            Ability::Sound | Ability::AnimalCall(_) => {
                if self.age >= 30 {
                    self.finish(&mut events.world);
                    self.callback(events, Hit::Pulse, false)?;
                }
            }
            Ability::Earthquake => {
                const COLUMN_TICKS: u32 = 21;
                const QUAKE_TICKS: u32 = 120;
                if self.phase == Phase::Active && self.age > COLUMN_TICKS {
                    self.transition(Phase::Hold);
                    events.world.rumble = Some(
                        crate::rumble::Rumble::new(0, 50, true, events.world.tick)
                            .map_err(anyhow::Error::msg)?,
                    );
                } else if self.phase == Phase::Hold {
                    shake(&mut events.world, ((QUAKE_TICKS - self.age) / 10) as f32);
                    if self.age == QUAKE_TICKS - 40 {
                        self.callback(events, Hit::Pulse, false)?;
                        self.transition(Phase::Release);
                    }
                } else if self.phase == Phase::Release {
                    shake(
                        &mut events.world,
                        (40u32.saturating_sub(self.age) / 10) as f32,
                    );
                    if self.age >= 40 {
                        self.finish(&mut events.world);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn launch(&mut self, events: &mut EventRuntime) -> Result<()> {
        let world = &mut events.world;
        let actor = &world.actors[&self.source.0];
        let speed = projectile(self.ability).map_or(
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
            muzzle_height(world, &events.resources, self.source.0)?
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
        self.transition(Phase::Active);
        if let Some(profile) = projectile(self.ability) {
            sound(world, profile.sound, profile.volume);
            if self.ability == Ability::Fire {
                world.event_flags.insert(RING_LAUNCH_FLAG);
            }
        }
        match self.ability {
            Ability::ElectricOrb(_) => sound(world, 41, 127),
            Ability::Bomb => {
                let resource = resonance_content::field::RING_BOMB_RESOURCE;
                let model = events
                    .resources
                    .model(resource)
                    .context("bomb model is not prepared")?;
                let mut bomb = Actor::new(resource, self.shot.position);
                bomb.operation = Some(self.operation.clone());
                bomb.face(world.actors[&self.source.0].heading);
                bomb.collidable = false;
                bomb.contact = crate::ActorContact::None;
                bomb.animation = model.clips.get(&slot::IDLE).map(|clip| {
                    Animation::new(resource, slot::IDLE, clip.duration_ticks, world.tick)
                });
                let id = world
                    .unaddressable_actor_key()
                    .map_err(anyhow::Error::msg)?;
                world.insert_actor(id, bomb);
                self.bomb = Some(id);
                sound(world, 107, 127);
            }
            Ability::Bubble(_) => sound(world, 289, 127),
            Ability::Earthquake => sound(world, 272, 127),
            Ability::Sound => sound(world, 129, 127),
            Ability::Radar => {
                sound(world, 263, 64);
                let handle = world.allocate_effect().map_err(anyhow::Error::msg)?;
                self.fog = Some(handle);
                world.fog_effects.insert(
                    handle,
                    crate::camera::FogEffect {
                        fog: radar_fog(0.),
                        operation: self.operation.clone(),
                    },
                );
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
    fn draw(&mut self, world: &mut GameWorld, resources: &ResourceLibrary) -> Result<()> {
        if matches!(self.phase, Phase::Windup | Phase::Done) {
            return Ok(());
        }
        let mut visuals = Visuals::new(&self.shot, &self.operation, world.tick);
        match self.ability {
            Ability::ElectricOrb(_) if self.phase != Phase::Active => {
                visuals.electric(false, self.age, &mut world.random_state)
            }
            Ability::Bomb if self.phase == Phase::Release && self.age <= 20 => {
                visuals.bomb(self.age == 0, &mut world.random_state)
            }
            Ability::Radar => {
                let amount = if self.phase == Phase::Release {
                    1. - fraction(self.age, FADE_TICKS)
                } else {
                    fraction(self.age, FADE_TICKS)
                };
                world.fog_effects.get_mut(&self.fog.unwrap()).unwrap().fog = radar_fog(amount);
            }
            Ability::Sunlight => {
                let amount = if self.phase == Phase::Release {
                    1. - fraction(self.age, FADE_TICKS)
                } else {
                    fraction(self.age, 12)
                };
                self.follow(world, muzzle_height(world, resources, self.source.0)?);
                let position = self.shot.position;
                let heading = world.actors[&self.source.0].heading;
                for (layer, width, length, twist, alpha, blue) in
                    [(0, 1., 1., -1., 180., 64), (1, 1.2, 1.1, 1., 108., 16)]
                {
                    let model = self.model(world, layer)?;
                    model.position = position;
                    model.rotation = [-90., twist * self.age as f32, heading];
                    model.scale = [
                        width * (0.1 + 1.9 * amount),
                        width * (0.1 + 1.9 * amount),
                        length * (1. + amount),
                    ];
                    model.rgba = [64, 64, blue, (alpha * amount) as u8];
                    model.blend = crate::effect::Blend::Additive;
                }
            }
            Ability::Bubble(_) => {
                let angle = (world.effect_tick as f32 * BUBBLE_TURN_PER_TICK).to_radians();
                let previous_lift = world.actors[&self.source.0]
                    .visual_lift
                    .as_ref()
                    .map_or(0., |lift| lift.height);
                let lift = match self.phase {
                    Phase::Active => 40. * fraction(self.age, BUBBLE_RISE_TICKS),
                    Phase::Hold if self.age == 0 => 40.,
                    Phase::Hold if self.age > 0 => {
                        previous_lift + (angle - BUBBLE_TURN_PER_TICK.to_radians()).sin()
                    }
                    Phase::Release if self.age > 0 => {
                        let remaining = BUBBLE_RISE_TICKS.saturating_sub(self.age);
                        previous_lift * fraction(remaining, remaining + 1)
                    }
                    _ => previous_lift,
                };
                world.actors.get_mut(&self.source.0).unwrap().visual_lift =
                    Some(crate::projectile::VisualLift {
                        height: lift,
                        operation: self.operation.clone(),
                    });
                self.follow(world, BUBBLE_CENTER_HEIGHT + lift);
                let position = self.shot.position;
                let model = self.model(world, 0)?;
                model.position = position;
                model.scale = [1.1 + angle.sin() * 0.2, 1., 1.1 + angle.cos() * 0.2];
                let opacity = match self.phase {
                    Phase::Active => (self.age as f32 * BUBBLE_FADE_IN_STEP).min(BUBBLE_OPACITY),
                    Phase::Release => {
                        (BUBBLE_OPACITY - self.age as f32 * BUBBLE_FADE_OUT_STEP).max(0.)
                    }
                    _ => BUBBLE_OPACITY,
                };
                model.rgba = [128, 192, 192, opacity as u8];
                model.orientation = crate::effect::SpriteOrientation::Camera;
            }
            Ability::Sound | Ability::AnimalCall(_) if self.age == 1 => {
                let color = match self.ability {
                    Ability::AnimalCall(CallColor::Pink) => Some([248, 96, 184]),
                    Ability::AnimalCall(CallColor::White) => Some([255; 3]),
                    Ability::AnimalCall(CallColor::Blue) => Some([32, 32, 255]),
                    _ => None,
                };
                visuals.pulse(color);
            }
            Ability::Earthquake => {
                if self.phase == Phase::Active && self.age > 0 {
                    visuals.ground_ring((21 - self.age) as f32 * 6.);
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
            actor.animation = pose.previous;
            actor.scripted_animation = pose.scripted;
        }
    }
    fn finish(&mut self, world: &mut GameWorld) {
        self.transition(Phase::Done);
        for handle in self.models.drain(..) {
            world.model_particles.remove(&handle);
        }
        if let Some(handle) = self.fog.take() {
            world.fog_effects.remove(&handle);
        }
        if let Some(id) = self.bomb.take() {
            world.remove_actor(id);
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
fn sound(world: &mut GameWorld, id: i16, volume: u8) {
    if volume != 0 {
        world.audio_commands.push(crate::AudioCommand::Sound {
            id,
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
    world
        .current_field
        .is_some_and(|id| (511..=518).contains(&id))
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
fn muzzle_height(world: &GameWorld, resources: &ResourceLibrary, id: i32) -> Result<f32> {
    const BONE: &str = "Bone_sebone03";
    let actor = &world.actors[&id];
    let model = resources
        .model(actor.resource)
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
    Ok(pose.sample_offset(BONE, sample, [0.; 3])?[2] * actor.scale_percent[2] as f32 / 100.)
}
