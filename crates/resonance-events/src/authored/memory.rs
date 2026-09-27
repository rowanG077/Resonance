//! Cooked memory-circle text and an atomic inventory/unlock operation.
use super::*;

const POINT: Type = Type::Handle("game::memory::Circle");
const MESSAGE: Type = Type::Enum {
    name: "game::memory::Message",
    variants: &[
        variant("Tutorial", 0, &[]),
        variant("Unlock", 1, &[]),
        variant("NoGem", 2, &[]),
    ],
};

fn message(h: &FieldHost<'_>, id: i32) -> Result<ResolvedMessage, String> {
    let text = &h.resources.memory_circle_text;
    let spans = match id {
        0 => &text.tutorial,
        1 => &text.unlock,
        2 => &text.no_gem,
        _ => return Err("invalid memory-circle message".into()),
    };
    if spans.is_empty() {
        return Err("memory-circle message is not cooked".into());
    }
    Ok(ResolvedMessage::from_spans(spans))
}
pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .function(
            "game::memory::notice",
            &[MESSAGE, Type::I32],
            None,
            true,
            |h, a, _| {
                let text = message(h, a[0])?;
                let flags = u16::try_from(a[1]).map_err(|_| "invalid notice flags")?;
                let notice = h.world.show_notice(text, flags)?;
                h.operations.track(&notice)?;
                *h.wait = Some(Wait::Complete(notice));
                Ok(NativeResult::Suspend)
            },
        )
        .function(
            "game::memory::choose",
            &[MESSAGE],
            Some(Type::I32),
            true,
            |h, a, _| {
                let text = message(h, a[0])?;
                let (notice, choice) = h.world.show_choice_notice(text, 1, 2)?;
                let wait = Wait::Choice {
                    result: choice,
                    window: Box::new(Wait::Complete(notice)),
                };
                wait.track(h.operations)?;
                *h.wait = Some(wait);
                Ok(NativeResult::Suspend)
            },
        )
        .function(
            "game::memory::unlock",
            &[POINT, Type::I32],
            Some(Type::Bool),
            false,
            |h, a, _| {
                let point = h
                    .world
                    .save_points
                    .get(usize::try_from(a[0]).map_err(|_| "invalid memory circle")?)
                    .ok_or("memory circle is missing")?;
                let flag = point.unlock_flag.ok_or("memory circle has no seal")?;
                if h.world.event_flags.contains(&flag) {
                    return Ok(NativeResult::Continue(Some(1)));
                }
                let item = u16::try_from(a[1]).map_err(|_| "invalid memory gem")?;
                let party = h
                    .world
                    .party
                    .as_mut()
                    .ok_or("memory-circle party is missing")?;
                let Some(count) = party.items.get_mut(&item).filter(|count| **count > 0) else {
                    return Ok(NativeResult::Continue(Some(0)));
                };
                *count -= 1;
                if *count == 0 {
                    party.items.remove(&item);
                }
                h.world.event_flags.insert(flag);
                Ok(NativeResult::Continue(Some(1)))
            },
        )
}
