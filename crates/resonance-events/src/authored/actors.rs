//! Opaque actor handles survive suspension without referring to replacement actors.
use super::*;

impl GameWorld {
    pub fn authored_actor(&mut self, id: i32) -> Result<i32, String> {
        let actor = self.actors.get(&id).ok_or("actor is missing")?;
        if let Some(handle) = actor.authored_handle {
            return Ok(handle);
        }
        let handle = self.allocate_effect()?;
        self.actors.get_mut(&id).unwrap().authored_handle = Some(handle);
        self.authored_actors.insert(handle, id);
        Ok(handle)
    }

    pub(crate) fn actor_id(&self, handle: i32) -> Result<i32, String> {
        self.authored_actors
            .get(&handle)
            .copied()
            .filter(|id| {
                self.actors
                    .get(id)
                    .is_some_and(|actor| actor.authored_handle == Some(handle))
            })
            .ok_or_else(|| "actor handle is stale".into())
    }
}

impl FieldHost<'_> {
    pub(super) fn actor_id(&self, handle: i32) -> Result<i32, String> {
        self.world.actor_id(handle)
    }
}

const ACTOR: Type = Type::Handle("game::actors::Actor");

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
            "game::effects::station_transfer",
            &[ACTOR, ACTOR],
            None,
            true,
            |h, a, _| {
                // The interaction can retire either actor before the lights start.
                if h.actor_id(a[0]).is_err() || h.actor_id(a[1]).is_err() {
                    return Ok(NativeResult::Continue(None));
                }
                let operation = h.operations.begin()?;
                h.world
                    .station_transfers
                    .push(crate::effect::station::Transfer::new(
                        a[0],
                        a[1],
                        operation.clone(),
                        h.world.tick,
                    ));
                *h.wait = Some(Wait::Complete(operation));
                Ok(NativeResult::Suspend)
            },
        )
        .function("game::actors::interact", &[ACTOR], None, true, |h, a, _| {
            let id = h.actor_id(a[0])?;
            h.call_event(0, id as u32, id as i16)
        })
}
