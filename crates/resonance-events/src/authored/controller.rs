//! Reusable input, following, and model drawing for authored field controllers.
use super::effects::{CONTEXT, origin};
use super::projectile::{COLOR, PROJECTILE, VECTOR, float, floats, projectile};
use super::*;
use symphonia_script::authored::NativeVariant;

const ACTOR: Type = Type::Handle("game::actors::Actor");
const MODEL_EFFECT: Type = Type::Handle("game::effects::ModelEffect");
const MODEL: Type = Type::Record {
    name: "game::effects::Model",
    fields: &[
        field("position", VECTOR),
        field("rotation", VECTOR),
        field("scale", VECTOR),
        field("color", COLOR),
        field("alpha", Type::I32),
        field("blend", super::visual::BLEND),
        field("facing", super::visual::FACING),
    ],
};
const fn button(name: &'static str, button: crate::input::Button) -> NativeVariant {
    NativeVariant {
        name,
        tag: button as i32,
        payload: &[],
    }
}
const BUTTON: Type = Type::Enum {
    name: "game::input::Button",
    variants: &[
        button("Left", crate::input::Button::Left),
        button("Right", crate::input::Button::Right),
        button("Down", crate::input::Button::Down),
        button("Up", crate::input::Button::Up),
        button("Skit", crate::input::Button::Skit),
        button("NextPage", crate::input::Button::NextPage),
        button("PreviousPage", crate::input::Button::PreviousPage),
        button("Accept", crate::input::Button::Accept),
        button("Cancel", crate::input::Button::Cancel),
        button("Ring", crate::input::Button::Ring),
        button("Menu", crate::input::Button::Menu),
        button("Start", crate::input::Button::Start),
    ],
};

fn set_model(model: &mut crate::model_particle::ModelParticle, a: &[i32]) -> Result<(), String> {
    let &[
        px,
        py,
        pz,
        rx,
        ry,
        rz,
        sx,
        sy,
        sz,
        red,
        green,
        blue,
        alpha,
        blend,
        facing,
    ] = a
    else {
        return Err("invalid model arguments".into());
    };
    let scale = floats::<3>(&[sx, sy, sz])?;
    if scale.iter().any(|v| *v < 0.) {
        return Err("negative model scale".into());
    }
    let position = floats(&[px, py, pz])?;
    let rotation = floats(&[rx, ry, rz])?;
    let rgba = super::projectile::color(&[red, green, blue, alpha])?;
    let blend = i32::from(super::visual::blend(blend)?).try_into()?;
    let orientation = super::visual::facing(facing)?;
    model.position = position;
    model.rotation = rotation;
    model.scale = scale;
    model.rgba = rgba;
    model.blend = blend;
    model.orientation = orientation;
    Ok(())
}

pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .function(
            "game::projectiles::lift",
            &[PROJECTILE, Type::F32],
            None,
            false,
            |h, a, _| {
                let shot = projectile(h, a[0])?;
                let height = float(a[1])?;
                let lift = crate::projectile::VisualLift {
                    height,
                    operation: shot.operation.clone(),
                };
                let (source, instance) = (shot.source, shot.source_instance);
                let actor = h
                    .world
                    .actors
                    .get_mut(&source)
                    .filter(|actor| actor.instance == instance)
                    .ok_or("lift source is missing")?;
                actor.visual_lift = Some(lift);
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::actors::tint",
            &[Type::Handle("game::actors::Pose"), COLOR],
            None,
            false,
            |h, a, _| {
                let tint = a[1..]
                    .iter()
                    .map(|v| u8::try_from(*v).map_err(|_| "invalid pose tint"))
                    .collect::<Result<Vec<_>, _>>()?;
                h.world
                    .owned_poses
                    .get_mut(&a[0])
                    .filter(|pose| pose.operation.is_pending())
                    .ok_or("pose handle is stale")?
                    .tint = Some(tint.try_into().unwrap());
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::actors::spawn",
            &[CONTEXT, ACTOR, Type::I32, Type::I32, Type::F32],
            Some(ACTOR),
            false,
            |h, a, _| {
                let origin = origin(h, a[0])?;
                let source = &h.world.actors[&h.actor_id(a[1])?];
                if h.world.actors.contains_key(&a[2]) {
                    return Err("scene actor ID is already in use".into());
                }
                let resource = u32::try_from(a[3]).map_err(|_| "invalid scene model")?;
                let model = h
                    .resources
                    .model(resource)
                    .ok_or("scene model is not prepared")?;
                let radius = float(a[4])?;
                if radius <= 0. {
                    return Err("invalid scene object radius".into());
                }
                let mut actor = crate::Actor::new(resource, origin.position);
                actor.operation = Some(origin.operation.clone());
                actor.face(source.heading);
                actor.collidable = false;
                actor.contact = crate::ActorContact::None;
                actor.radius = radius;
                actor.scripted_animation = true;
                let slot = crate::animation::slot::IDLE;
                actor.animation = model.clips.get(&slot).map(|clip| {
                    crate::Animation::new(resource, slot, clip.duration_ticks, h.world.tick)
                });
                h.world.insert_actor(a[2], actor);
                Ok(NativeResult::Continue(Some(h.world.authored_actor(a[2])?)))
            },
        )
        .function(
            "game::actors::show",
            &[ACTOR, Type::Bool],
            None,
            false,
            |h, a, _| {
                let id = h.actor_id(a[0])?;
                if let Some(actor) = h.world.actors.get_mut(&id) {
                    actor.visible = a[1] != 0;
                }
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::actors::is_enemy",
            &[ACTOR],
            Some(Type::Bool),
            false,
            |h, a, _| {
                let id = h.actor_id(a[0])?;
                Ok(NativeResult::Continue(Some(i32::from(
                    h.world.actors.get(&id).is_some_and(|a| a.enemy.is_some()),
                ))))
            },
        )
        .function("game::actors::despawn", &[ACTOR], None, false, |h, a, _| {
            let id = h.actor_id(a[0])?;
            h.world.actors.remove(&id);
            h.world.billboards.retain(|_, p| p.owner != Some(id));
            h.world.overlays.remove(&id);
            h.world.emotes.remove(&id);
            Ok(NativeResult::Continue(None))
        })
        .function("game::field::take_control", &[], None, false, |h, _, _| {
            h.tasks.released.remove(&h.tasks.root(h.handle));
            Ok(NativeResult::Continue(None))
        })
        .function(
            "game::camera::shake",
            &[Type::F32, Type::Ticks, Type::Ticks],
            None,
            false,
            |h, a, _| {
                let amount = float(a[0])?;
                let decay = u16::try_from(a[1]).map_err(|_| "invalid shake duration")?;
                h.world
                    .field_camera
                    .get_or_insert_default()
                    .shake
                    .configure(amount, decay, a[2]);
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::input::rumble",
            &[Type::Ticks],
            None,
            false,
            |h, a, _| {
                h.world.rumble = Some(crate::rumble::Rumble::new(0, a[0], true, h.world.tick)?);
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::actors::has_clip",
            &[ACTOR, Type::I32],
            Some(Type::Bool),
            false,
            |h, a, _| {
                let id = h.actor_id(a[0])?;
                let present = h
                    .world
                    .actors
                    .get(&id)
                    .and_then(|actor| h.resources.model(actor.resource))
                    .is_some_and(|model| {
                        u16::try_from(a[1]).is_ok_and(|slot| model.clips.contains_key(&slot))
                    });
                Ok(NativeResult::Continue(Some(i32::from(present))))
            },
        )
        .function(
            "game::input::held",
            &[BUTTON],
            Some(Type::Bool),
            false,
            |h, a, _| {
                Ok(NativeResult::Continue(Some(i32::from(
                    h.world.input.read(1, 0, h.world.mapped_input_disabled) & a[0] != 0,
                ))))
            },
        )
        .function(
            "game::ring::charge",
            &[],
            Some(Type::I32),
            false,
            |h, _, _| {
                Ok(NativeResult::Continue(Some(
                    h.world
                        .party
                        .as_ref()
                        .ok_or("ring party is missing")?
                        .travel
                        .ring_timer as i32,
                )))
            },
        )
        .function(
            "game::field::map_id",
            &[],
            Some(Type::I32),
            false,
            |h, _, _| {
                Ok(NativeResult::Continue(Some(
                    h.world.current_field.ok_or("field owner is missing")? as i32,
                )))
            },
        )
        .function(
            "game::actors::heading",
            &[ACTOR],
            Some(Type::F32),
            false,
            |h, a, _| {
                let id = h.actor_id(a[0])?;
                Ok(NativeResult::Continue(Some(
                    h.world
                        .actors
                        .get(&id)
                        .ok_or("heading actor is missing")?
                        .heading
                        .to_bits() as i32,
                )))
            },
        )
        .function(
            "game::actors::id",
            &[ACTOR],
            Some(Type::I32),
            false,
            |h, a, _| Ok(NativeResult::Continue(Some(h.actor_id(a[0])?))),
        )
        .function(
            "game::ring::secondary_hit",
            &[ACTOR],
            None,
            true,
            |h, a, _| {
                h.call_event(
                    0,
                    crate::ring::SECONDARY_CALLBACK,
                    crate::ring::Hit::Actor(h.actor_id(a[0])? as i16).event_actor(),
                )
            },
        )
        .function(
            "game::projectiles::source_exists",
            &[PROJECTILE],
            Some(Type::Bool),
            false,
            |h, a, _| {
                let shot = projectile(h, a[0])?;
                Ok(NativeResult::Continue(Some(i32::from(
                    h.world
                        .actors
                        .get(&shot.source)
                        .is_some_and(|actor| actor.instance == shot.source_instance),
                ))))
            },
        )
        .function(
            "game::projectiles::follow",
            &[PROJECTILE, Type::F32, Type::F32],
            Some(VECTOR),
            false,
            |h, a, _| {
                let shot = projectile(h, a[0])?;
                let values = floats::<2>(&a[1..])?;
                let source = h
                    .world
                    .actors
                    .get(&shot.source)
                    .filter(|actor| actor.instance == shot.source_instance)
                    .ok_or("projectile source is missing")?;
                let (sin, cos) = source.heading.to_radians().sin_cos();
                let position = [
                    source.position[0] + sin * values[1],
                    source.position[1] - cos * values[1],
                    source.position[2] + values[0],
                ];
                h.world.projectiles.get_mut(&a[0]).unwrap().position = position;
                Ok(NativeResult::Values(
                    position.map(|v| v.to_bits() as i32).to_vec(),
                ))
            },
        )
        .function(
            "game::projectiles::block_menu",
            &[PROJECTILE],
            None,
            false,
            |h, a, _| {
                projectile(h, a[0])?;
                h.world.projectiles.get_mut(&a[0]).unwrap().blocks_menu = true;
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::effects::model",
            &[CONTEXT, Type::I32, MODEL],
            Some(MODEL_EFFECT),
            false,
            |h, a, _| {
                let operation = origin(h, a[0])?.operation.clone();
                let resource = a[1] as u32;
                if h.resources.model(resource).is_none() {
                    return Err(format!("model effect {resource:#x} is not prepared"));
                }
                let mut model = crate::model_particle::ModelParticle::scoped(resource, operation);
                set_model(&mut model, &a[2..])?;
                let handle = h.world.emit_model_particle(model)?;
                if handle == 0 {
                    return Err("model effect pool is full".into());
                }
                Ok(NativeResult::Continue(Some(handle)))
            },
        )
        .function(
            "game::effects::set_model",
            &[MODEL_EFFECT, MODEL],
            None,
            false,
            |h, a, _| {
                let model = h
                    .world
                    .model_particles
                    .get_mut(&a[0])
                    .filter(|p| {
                        p.operation
                            .as_ref()
                            .is_some_and(crate::Operation::is_pending)
                    })
                    .ok_or("model effect handle is stale")?;
                set_model(model, &a[1..])?;
                Ok(NativeResult::Continue(None))
            },
        )
}
