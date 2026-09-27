use super::effects::{CONTEXT, origin};
use super::*;
use crate::projectile::{Contact, Motion, OwnedPose, Projectile};
use symphonia_script::authored::{NativeField, NativeVariant};

const ACTOR: Type = Type::Handle("game::actors::Actor");
const POSE: Type = Type::Handle("game::actors::Pose");
pub(super) const PROJECTILE: Type = Type::Handle("game::projectiles::Projectile");
pub(super) const VECTOR: Type = Type::Record {
    name: "game::geometry::Vector",
    fields: &[
        NativeField {
            name: "x",
            ty: Type::F32,
        },
        NativeField {
            name: "y",
            ty: Type::F32,
        },
        NativeField {
            name: "z",
            ty: Type::F32,
        },
    ],
};
pub(super) const COLOR: Type = Type::Record {
    name: "game::effects::Color",
    fields: &[
        NativeField {
            name: "red",
            ty: Type::I32,
        },
        NativeField {
            name: "green",
            ty: Type::I32,
        },
        NativeField {
            name: "blue",
            ty: Type::I32,
        },
    ],
};
const SPARK: Type = Type::Record {
    name: "game::effects::Spark",
    fields: &[
        NativeField {
            name: "lifetime",
            ty: Type::Ticks,
        },
        NativeField {
            name: "size",
            ty: Type::F32,
        },
        NativeField {
            name: "growth",
            ty: Type::F32,
        },
        NativeField {
            name: "rotation",
            ty: Type::F32,
        },
        NativeField {
            name: "spin",
            ty: Type::F32,
        },
        NativeField {
            name: "velocity",
            ty: VECTOR,
        },
        NativeField {
            name: "inherit_velocity",
            ty: Type::F32,
        },
        NativeField {
            name: "color",
            ty: COLOR,
        },
        NativeField {
            name: "offset",
            ty: VECTOR,
        },
        NativeField {
            name: "fade",
            ty: Type::F32,
        },
        NativeField {
            name: "alpha",
            ty: Type::I32,
        },
        NativeField {
            name: "blend",
            ty: super::visual::BLEND,
        },
    ],
};
#[repr(i32)]
enum ContactTag {
    Flying,
    Actor,
    Barrier,
}
const CONTACT: Type = Type::Enum {
    name: "game::projectiles::Contact",
    variants: &[
        NativeVariant {
            name: "Flying",
            tag: ContactTag::Flying as i32,
            payload: &[],
        },
        NativeVariant {
            name: "Actor",
            tag: ContactTag::Actor as i32,
            payload: &[ACTOR],
        },
        NativeVariant {
            name: "Barrier",
            tag: ContactTag::Barrier as i32,
            payload: &[],
        },
    ],
};
const ACTOR_SCAN: Type = Type::Enum {
    name: "game::projectiles::ActorScan",
    variants: &[
        NativeVariant {
            name: "Start",
            tag: 0,
            payload: &[],
        },
        NativeVariant {
            name: "After",
            tag: 1,
            payload: &[ACTOR],
        },
        NativeVariant {
            name: "End",
            tag: 2,
            payload: &[],
        },
    ],
};

const TARGETS: Type = Type::Enum {
    name: "game::projectiles::Targets",
    variants: &[
        NativeVariant {
            name: "All",
            tag: 0,
            payload: &[],
        },
        NativeVariant {
            name: "Interactions",
            tag: 1,
            payload: &[],
        },
    ],
};

const STUN_EFFECT: Type = Type::Enum {
    name: "game::actors::StunEffect",
    variants: &[
        NativeVariant {
            name: "None",
            tag: 0,
            payload: &[],
        },
        NativeVariant {
            name: "Electric",
            tag: 1,
            payload: &[],
        },
        NativeVariant {
            name: "Lightning",
            tag: 2,
            payload: &[],
        },
        NativeVariant {
            name: "Ice",
            tag: 3,
            payload: &[],
        },
        NativeVariant {
            name: "Darkness",
            tag: 4,
            payload: &[],
        },
    ],
};

const MOTION: Type = Type::Enum {
    name: "game::projectiles::Motion",
    variants: &[
        NativeVariant {
            name: "Swept",
            tag: 0,
            payload: &[],
        },
        NativeVariant {
            name: "Scripted",
            tag: 1,
            payload: &[],
        },
    ],
};

pub(super) fn projectile<'a>(h: &'a FieldHost<'_>, id: i32) -> Result<&'a Projectile, String> {
    h.world
        .projectiles
        .get(&id)
        .filter(|p| p.operation.is_pending())
        .ok_or_else(|| "projectile handle is stale".into())
}
pub(super) fn float(word: i32) -> Result<f32, String> {
    let value = f32::from_bits(word as u32);
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| "nonfinite effect parameter".into())
}

pub(super) fn floats<const N: usize>(a: &[i32]) -> Result<[f32; N], String> {
    let words: [i32; N] = a.try_into().map_err(|_| "invalid effect argument count")?;
    let values = words.map(|v| f32::from_bits(v as u32));
    if values.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite effect parameter".into());
    }
    Ok(values)
}

pub(super) fn color<const N: usize>(words: &[i32; N]) -> Result<[u8; N], String> {
    let mut color = [0; N];
    for (out, word) in color.iter_mut().zip(words) {
        *out = u8::try_from(*word).map_err(|_| "invalid effect color")?;
    }
    Ok(color)
}

pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .function(
            "game::actors::controlled",
            &[],
            Some(ACTOR),
            false,
            |h, _, _| {
                Ok(NativeResult::Continue(Some(
                    h.world.authored_actor(h.world.controlled_actor)?,
                )))
            },
        )
        .function(
            "game::actors::play_clip",
            &[ACTOR, Type::I32],
            Some(POSE),
            false,
            |h, a, _| play_clip(h, a, false),
        )
        .function(
            "game::actors::play_service_clip",
            &[ACTOR, Type::I32],
            Some(POSE),
            false,
            |h, a, _| play_clip(h, a, true),
        )
        .function(
            "game::actors::release_pose",
            &[POSE],
            None,
            false,
            |h, a, _| {
                let pose = h
                    .world
                    .owned_poses
                    .get(&a[0])
                    .ok_or("pose handle is stale")?;
                pose.operation.complete(None)?;
                h.world.reap_authored_resources();
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::projectiles::launch",
            &[ACTOR, Type::F32, Type::F32, Type::F32],
            Some(PROJECTILE),
            false,
            |h, a, _| {
                let id = h.actor_id(a[0])?;
                let v = floats::<3>(&a[1..])?;
                if v[2] <= 0. {
                    return Err("projectile radius must be positive".into());
                }
                let actor = h
                    .world
                    .actors
                    .get(&id)
                    .ok_or("projectile source is missing")?;
                let (sin, cos) = actor.heading.to_radians().sin_cos();
                let shot = Projectile {
                    task: h.tasks.root(h.handle),
                    source: id,
                    source_instance: actor.instance,
                    position: [
                        actor.position[0],
                        actor.position[1],
                        actor.position[2] + v[0],
                    ],
                    velocity: [sin * v[1], -cos * v[1], 0.],
                    radius: v[2],
                    contact: Contact::Flying,
                    motion: Motion::Swept,
                    shadow: None,
                    paused: false,
                    blocks_menu: false,
                    operation: h.operations.begin()?,
                };
                let handle = h.world.allocate_effect()?;
                h.world.projectiles.insert(handle, shot);
                Ok(NativeResult::Continue(Some(handle)))
            },
        )
        .function(
            "game::projectiles::contact",
            &[PROJECTILE],
            Some(CONTACT),
            false,
            |h, a, _| {
                let shot = projectile(h, a[0])?;
                let mut contact = shot.contact;
                if contact == Contact::Flying && shot.motion == Motion::Scripted {
                    let (position, radius) = (shot.position, shot.radius);
                    for index in 0..h.world.triggers.len() {
                        let trigger = &mut h.world.triggers[index];
                        if !trigger.ring_barrier {
                            continue;
                        }
                        if !trigger.touches(position, radius) {
                            trigger.activations = 0;
                            continue;
                        }
                        let Some(context) = trigger.activation_context() else {
                            continue;
                        };
                        let key = trigger.key;
                        let started = h.start_trigger(key, context)?;
                        h.world.triggers[index].record_activation(started);
                        if started {
                            contact = Contact::Barrier;
                        }
                    }
                }
                Ok(NativeResult::Values(match contact {
                    Contact::Flying => vec![ContactTag::Flying as i32, 0],
                    Contact::Actor(actor) => {
                        vec![ContactTag::Actor as i32, h.world.authored_actor(actor)?]
                    }
                    Contact::Barrier => vec![ContactTag::Barrier as i32, 0],
                }))
            },
        )
        .function(
            "game::projectiles::finish",
            &[PROJECTILE],
            None,
            false,
            |h, a, _| {
                projectile(h, a[0])?.operation.complete(None)?;
                h.world.reap_authored_resources();
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::projectiles::hold",
            &[PROJECTILE],
            None,
            false,
            |h, a, _| {
                projectile(h, a[0])?;
                h.world.projectiles.get_mut(&a[0]).unwrap().paused = true;
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::projectiles::motion",
            &[PROJECTILE, MOTION],
            None,
            false,
            |h, a, _| {
                projectile(h, a[0])?;
                h.world.projectiles.get_mut(&a[0]).unwrap().motion = match a[1] {
                    0 => Motion::Swept,
                    1 => Motion::Scripted,
                    _ => return Err("invalid projectile motion".into()),
                };
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::projectiles::advance",
            &[PROJECTILE],
            None,
            false,
            |h, a, _| {
                if projectile(h, a[0])?.motion != Motion::Scripted {
                    return Err("only scripted projectiles may advance explicitly".into());
                }
                let shot = h.world.projectiles.get_mut(&a[0]).unwrap();
                shot.position = std::array::from_fn(|i| shot.position[i] + shot.velocity[i]);
                shot.contact = Contact::Flying;
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::projectiles::touches_actor",
            &[PROJECTILE, ACTOR, Type::F32],
            Some(Type::Bool),
            false,
            |h, a, _| {
                let shot = projectile(h, a[0])?;
                let radius = float(a[2])?;
                if radius <= 0. {
                    return Err("invalid contact radius".into());
                }
                let id = h.actor_id(a[1])?;
                let touches = h
                    .world
                    .actors
                    .get(&id)
                    .is_some_and(|actor| shot.touches_actor(actor, radius));
                Ok(NativeResult::Continue(Some(i32::from(touches))))
            },
        )
        .function(
            "game::projectiles::overlapping",
            &[PROJECTILE, Type::F32, TARGETS, ACTOR_SCAN],
            Some(ACTOR_SCAN),
            false,
            |h, a, _| {
                let shot = projectile(h, a[0])?;
                let radius = float(a[1])?;
                if radius <= 0. {
                    return Err("invalid contact radius".into());
                }
                let interaction_only = match a[2] {
                    0 => false,
                    1 => true,
                    _ => return Err("invalid contact target filter".into()),
                };
                let start = match a[3] {
                    0 => 0,
                    1 => {
                        let id = h.actor_id(a[4])?;
                        h.world
                            .actor_order
                            .iter()
                            .position(|entry| *entry == id)
                            .ok_or("contact cursor actor is missing")?
                            + 1
                    }
                    2 => return Ok(NativeResult::Values(vec![2, 0])),
                    _ => return Err("invalid contact cursor".into()),
                };
                let actor = h.world.actor_order[start..].iter().find(|id| {
                    **id != shot.source
                        && h.world.actors.get(id).is_some_and(|actor| {
                            (!interaction_only || actor.role == crate::ActorRole::Interaction)
                                && actor.projectile_target()
                                && shot.touches_actor(actor, radius)
                        })
                });
                let actor = actor.copied();
                Ok(NativeResult::Values(match actor {
                    Some(id) => vec![1, h.world.authored_actor(id)?],
                    None => vec![2, 0],
                }))
            },
        )
        .function(
            "game::field::random",
            &[Type::I32],
            Some(Type::I32),
            false,
            |h, a, _| {
                if a[0] <= 0 {
                    return Err("random bound must be positive".into());
                }
                Ok(NativeResult::Continue(Some(
                    (h.world.random() % a[0] as u32) as i32,
                )))
            },
        )
        .function(
            "game::field::effect_tick",
            &[],
            Some(Type::I32),
            false,
            |h, _, _| Ok(NativeResult::Continue(Some(h.world.effect_tick as i32))),
        )
        .function(
            "game::actors::stun",
            &[ACTOR, Type::Ticks, STUN_EFFECT],
            None,
            false,
            |h, a, _| {
                let id = h.actor_id(a[0])?;
                let duration = u16::try_from(a[1]).map_err(|_| "invalid stun duration")?;
                let effect = match a[2] {
                    0 => crate::effect::StunEffect::None,
                    1 => crate::effect::StunEffect::Electric,
                    2 => crate::effect::StunEffect::Lightning,
                    3 => crate::effect::StunEffect::Ice,
                    4 => crate::effect::StunEffect::Darkness,
                    _ => return Err("invalid stun effect".into()),
                };
                if let Some(actor) = h.world.actors.get_mut(&id)
                    && let Some(enemy) = &mut actor.enemy
                {
                    enemy.stun = std::num::NonZeroU16::new(duration)
                        .map(|remaining| crate::effect::Stun { remaining, effect });
                    actor.motion = None;
                }
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::actors::bone_height",
            &[ACTOR, Type::String, VECTOR, Type::F32],
            Some(Type::F32),
            false,
            |h, a, _| {
                let id = h.actor_id(a[0])?;
                let name = h
                    .program
                    .authored()
                    .and_then(|module| {
                        usize::try_from(a[1])
                            .ok()
                            .and_then(|i| module.strings.get(i))
                    })
                    .ok_or("invalid bone name")?;
                let values = floats::<4>(&a[2..])?;
                let offset = values[..3].try_into().unwrap();
                let fallback = values[3];
                let actor = h
                    .world
                    .actors
                    .get(&id)
                    .ok_or("attachment actor is missing")?;
                let model = h
                    .resources
                    .model(actor.resource)
                    .ok_or("attachment model is missing")?;
                let height = if model.names.contains(name) {
                    let animation = actor
                        .animation
                        .as_ref()
                        .ok_or("attachment animation is missing")?;
                    let clip = h
                        .resources
                        .animation(animation)
                        .ok_or("attachment clip is missing")?;
                    let pose = h
                        .resources
                        .attachment_pose(actor)
                        .ok_or("attachment pose is not prepared")?;
                    let sample = animation.sample(
                        h.world.tick,
                        model.attachment_pose_delay,
                        clip.duration_ticks as f32,
                    );
                    let point = pose
                        .sample_offset(name, sample, offset)
                        .map_err(|e| e.to_string())?;
                    point[2] * actor.properties.get(&32).copied().unwrap_or(100) as f32 / 100.
                } else {
                    fallback
                };
                Ok(NativeResult::Continue(Some(height.to_bits() as i32)))
            },
        )
        .function(
            "game::effects::spark",
            &[CONTEXT, SPARK],
            None,
            false,
            |h, a, _| {
                let &[
                    context,
                    life,
                    size,
                    growth,
                    rotation,
                    spin,
                    vx,
                    vy,
                    vz,
                    inherit,
                    red,
                    green,
                    blue,
                    ox,
                    oy,
                    oz,
                    fade,
                    alpha,
                    blend,
                ] = a
                else {
                    return Err("invalid spark arguments".into());
                };
                let shot = origin(h, context)?;
                let [size, growth, rotation, spin, inherit, fade] =
                    floats(&[size, growth, rotation, spin, inherit, fade])?;
                let offset = floats::<3>(&[ox, oy, oz])?;
                let velocity = floats::<3>(&[vx, vy, vz])?;
                let effect = crate::effect::BillboardEffect {
                    operation: Some(shot.operation.clone()),
                    owner: None,
                    field_lighting: false,
                    field_fog: true,
                    recipe: crate::effect::GLOW_SPRITE,
                    orientation: crate::effect::SpriteOrientation::Camera,
                    anchor: resonance_content::effect::VerticalAnchor::Center,
                    palette: None,
                    born: h.world.tick,
                    lifetime: u32::try_from(life).map_err(|_| "invalid particle lifetime")?,
                    position: std::array::from_fn(|i| shot.position[i] + offset[i]),
                    velocity: std::array::from_fn(|i| velocity[i] + shot.velocity[i] * inherit),
                    controller: None,
                    acceleration: None,
                    gravity: 0.,
                    rotation: [0., 0., rotation],
                    angular_velocity: [0., 0., spin],
                    size: [size; 2],
                    size_delta: growth,
                    rgba: color(&[red, green, blue, alpha])?,
                    fade: crate::effect::Fade::Linear(fade),
                    blend_mode: Some(super::visual::blend(blend)?),
                };
                h.world.emit_billboard(effect)?;
                Ok(NativeResult::Continue(None))
            },
        )
}

// Native field services use a separate per-character bank (lbl_8035A524).
fn play_clip(h: &mut FieldHost<'_>, a: &[i32], service: bool) -> Result<NativeResult, String> {
    let id = h.actor_id(a[0])?;
    let slot = u16::try_from(a[1]).map_err(|_| "invalid animation slot")?;
    let actor = h
        .world
        .actors
        .get(&id)
        .ok_or("animation actor is missing")?;
    use crate::animation::AnimationSource;
    let (resource, source, clip) = if service {
        let resource =
            resonance_content::field::FIELD_SERVICE_MOTION_RESOURCE_BASE + actor.resource;
        (
            resource,
            AnimationSource::Resource,
            h.resources
                .animations
                .get(&resource)
                .and_then(|clips| clips.get(&slot)),
        )
    } else {
        (
            actor.resource,
            AnimationSource::Model,
            h.resources
                .model(actor.resource)
                .and_then(|m| m.clips.get(&slot)),
        )
    };
    let clip = clip.ok_or("actor clip is not prepared")?;
    let mut animation = crate::Animation::new(resource, slot, clip.duration_ticks, h.world.tick);
    animation.source = source;
    if service {
        animation.blend_ticks = 4;
    } // lbl_8035AFD4 in fn_8001DFF0.
    animation.repeat = false;
    let pose = OwnedPose {
        actor: id,
        instance: actor.instance,
        previous: actor.animation.clone(),
        scripted: actor.scripted_animation,
        slot,
        started: h.world.tick,
        tint: None,
        operation: h.operations.begin()?,
    };
    let handle = h.world.allocate_effect()?;
    let actor = h.world.actors.get_mut(&id).unwrap();
    actor.animation = Some(animation);
    actor.scripted_animation = true;
    let size = if id == h.world.controlled_actor {
        h.world.player_size.model_scale()
    } else {
        1.
    };
    actor.record_animation_binding(h.world.tick, size);
    h.world.pending_animation_bindings.insert(id);
    h.world.owned_poses.insert(handle, pose);
    Ok(NativeResult::Continue(Some(handle)))
}
