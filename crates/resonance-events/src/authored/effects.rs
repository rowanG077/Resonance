//! Task-owned effect origins. Projectiles expose their moving origin through the same API.
use super::projectile::{PROJECTILE, VECTOR, floats, projectile};
use super::*;
use crate::effect::EffectContext;

pub(super) const CONTEXT: Type = Type::Handle("game::effects::Context");

pub(super) struct Origin<'a> {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub operation: &'a crate::Operation,
}

pub(super) fn origin<'a>(h: &'a FieldHost<'_>, handle: i32) -> Result<Origin<'a>, String> {
    let origin = if let Some(shot) = h.world.projectiles.get(&handle) {
        Origin {
            position: shot.position,
            velocity: shot.velocity,
            operation: &shot.operation,
        }
    } else {
        let context = h
            .world
            .effect_contexts
            .get(&handle)
            .ok_or("effect context is stale")?;
        Origin {
            position: context.position,
            velocity: [0.; 3],
            operation: &context.operation,
        }
    };
    if !origin.operation.is_pending() {
        return Err("effect context has finished".into());
    }
    Ok(origin)
}

pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .function(
            "game::effects::begin",
            &[VECTOR],
            Some(CONTEXT),
            false,
            |h, a, _| {
                let context = EffectContext {
                    task: h.tasks.root(h.handle),
                    position: floats(a)?,
                    operation: h.operations.begin()?,
                };
                let handle = h.world.allocate_effect()?;
                h.world.effect_contexts.insert(handle, context);
                Ok(NativeResult::Continue(Some(handle)))
            },
        )
        .function(
            "game::effects::finish",
            &[CONTEXT],
            None,
            false,
            |h, a, _| {
                origin(h, a[0])?.operation.complete(None)?;
                h.world.reap_authored_resources();
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::projectiles::effect",
            &[PROJECTILE],
            Some(CONTEXT),
            false,
            |h, a, _| {
                projectile(h, a[0])?;
                Ok(NativeResult::Continue(Some(a[0])))
            },
        )
}
