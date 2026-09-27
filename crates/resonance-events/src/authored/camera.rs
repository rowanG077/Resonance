//! Scoped camera effects. The authored task owns their timing and cancellation.
use super::*;
use crate::camera::{Fog, FogEffect};

const EFFECT: Type = Type::Handle("game::camera::FogEffect");
const FOG: Type = Type::Record {
    name: "game::camera::Fog",
    fields: &[
        field("start", Type::F32),
        field("end", Type::F32),
        field("color", super::projectile::COLOR),
    ],
};

fn fog(a: &[i32]) -> Result<Fog, String> {
    let &[start, end, red, green, blue] = a else {
        return Err("invalid fog arguments".into());
    };
    let [start, end] = super::projectile::floats(&[start, end])?;
    Ok(Fog {
        start,
        end,
        color: super::projectile::color(&[red, green, blue])?,
    })
}

pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .function(
            "game::field::event_paused",
            &[],
            Some(Type::Bool),
            false,
            |h, _, _| {
                Ok(NativeResult::Continue(Some(i32::from(
                    h.world.mapped_input_disabled,
                ))))
            },
        )
        .function(
            "game::camera::fog",
            &[FOG],
            Some(EFFECT),
            false,
            |h, a, _| {
                let effect = FogEffect {
                    task: h.tasks.root(h.handle),
                    fog: fog(a)?,
                    operation: h.operations.begin()?,
                };
                let handle = h.world.allocate_effect()?;
                h.world.fog_effects.insert(handle, effect);
                Ok(NativeResult::Continue(Some(handle)))
            },
        )
        .function(
            "game::camera::set_fog",
            &[EFFECT, FOG],
            None,
            false,
            |h, a, _| {
                h.world
                    .fog_effects
                    .get_mut(&a[0])
                    .ok_or("fog handle is stale")?
                    .fog = fog(&a[1..])?;
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::camera::finish_fog",
            &[EFFECT],
            None,
            false,
            |h, a, _| {
                h.world
                    .fog_effects
                    .get(&a[0])
                    .ok_or("fog handle is stale")?
                    .operation
                    .complete(None)?;
                h.world.reap_authored_resources();
                Ok(NativeResult::Continue(None))
            },
        )
}
