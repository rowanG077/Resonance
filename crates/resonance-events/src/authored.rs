//! Typed field services for authored source, using the ordinary event dispatcher.
use crate::{
    GameWorld, ResourceLibrary,
    dialogue::{ResolvedMessage, TextToken},
    operation::{OperationScope, Wait},
};
use symphonia_script::{
    Program,
    authored::{MessagePart, NativeDeclaration, TextReferenceKind, Type},
};
use symphonia_script_vm::{Host, NativeBindings, NativeResult, Tasks};
mod actors;
mod exploration;
mod memory;

const fn variant(
    name: &'static str,
    tag: i32,
    payload: &'static [Type],
) -> symphonia_script::authored::NativeVariant {
    symphonia_script::authored::NativeVariant { name, tag, payload }
}

pub(crate) struct Spawn {
    pub handle: i32,
    pub target: SpawnTarget,
}
pub(crate) enum SpawnTarget {
    Task {
        entry: u32,
        arguments: Vec<i32>,
    },
    Callback {
        entry: u32,
        key: u32,
        event_actor: i16,
        completion: crate::Operation,
    },
}

pub fn native_declarations() -> Vec<NativeDeclaration> {
    FieldHost::AUTHORED_NATIVES.declarations().collect()
}

pub(crate) struct FieldHost<'a> {
    pub world: &'a mut GameWorld,
    pub resources: &'a ResourceLibrary,
    pub program: &'a Program,
    pub scenario: &'a Program,
    pub wait: &'a mut Option<Wait>,
    pub operations: &'a mut OperationScope,
    pub handle: i32,
    pub tasks: &'a mut Tasks,
    pub spawns: &'a mut Vec<Spawn>,
    pub next_handle: &'a mut i32,
    pub free_slots: usize,
}

impl Host for FieldHost<'_> {
    fn load_state(&self, name: &str) -> Result<Option<i32>, String> {
        Ok(self.world.script_state.get(name).copied())
    }
    fn store_state(&mut self, name: &str, value: i32) -> Result<(), String> {
        self.world.script_state.insert(name.into(), value);
        Ok(())
    }
    const AUTHORED_NATIVES: NativeBindings<Self> = {
        let bindings = NativeBindings::<Self>::new()
            .register_authored(
                "game::field::release_control",
                &[],
                None,
                false,
                |host, _, _| {
                    host.tasks.release_control(host.handle);
                    Ok(NativeResult::Continue(None))
                },
            )
            .register_authored(
                "game::field::wait_ticks",
                &[Type::Ticks],
                None,
                true,
                |host, args, _| host.wait_ticks(args[0] as u32),
            )
            .register_authored("game::field::next_update", &[], None, true, |host, _, _| {
                host.wait_ticks(1)
            })
            .register_authored(
                "game::story::flag",
                &[Type::I32],
                Some(Type::Bool),
                false,
                |host, args, _| {
                    Ok(NativeResult::Continue(Some(i32::from(
                        host.world.event_flags.contains(&flag(args[0])?),
                    ))))
                },
            )
            .register_authored(
                "game::story::set_flag",
                &[Type::I32, Type::Bool],
                None,
                false,
                |host, args, _| {
                    let flag = flag(args[0])?;
                    if args[1] != 0 {
                        host.world.event_flags.insert(flag);
                    } else {
                        host.world.event_flags.remove(&flag);
                    }
                    Ok(NativeResult::Continue(None))
                },
            )
            .register_authored(
                "game::field::notice",
                &[Type::Message],
                None,
                true,
                |host, args, _| host.notice(args, 0),
            )
            .register_authored(
                "game::text::character",
                &[Type::I32],
                Some(Type::TextReference {
                    name: "game::text::Character",
                    kind: TextReferenceKind::Character,
                }),
                false,
                |host, args, _| {
                    let id = if args[0] == crate::CONTROLLED_ACTOR {
                        host.world.controlled_actor
                    } else {
                        args[0]
                    };
                    host.text_reference(TextReferenceKind::Character, id)?;
                    Ok(NativeResult::Continue(Some(id)))
                },
            )
            .register_authored(
                "game::text::item",
                &[Type::I32],
                Some(Type::TextReference {
                    name: "game::text::Item",
                    kind: TextReferenceKind::Item,
                }),
                false,
                |host, args, _| {
                    host.text_reference(TextReferenceKind::Item, args[0])?;
                    Ok(NativeResult::Continue(Some(args[0])))
                },
            );
        let bindings = exploration::register(bindings);
        let bindings = actors::register(bindings);
        memory::register(bindings)
    };

    fn spawn(&mut self, function: u16, arguments: &[i32]) -> Result<i32, String> {
        let function = self
            .program
            .authored()
            .and_then(|module| module.functions.get(usize::from(function)))
            .filter(|function| function.is_task)
            .ok_or("spawn target is not a task")?;
        let entry = function.entry;
        let handle = self.reserve_child()?;
        self.spawns.push(Spawn {
            handle,
            target: SpawnTarget::Task {
                entry,
                arguments: arguments.to_vec(),
            },
        });
        Ok(handle)
    }

    fn join(&mut self, handle: i32) -> Result<Option<Vec<i32>>, String> {
        self.tasks.join(self.handle, handle)
    }
}

impl FieldHost<'_> {
    fn reserve_slot(&mut self) -> Result<i32, String> {
        if self.free_slots == 0 {
            return Err("event pool exhausted (32 instances)".into());
        }
        let handle = *self.next_handle;
        let next = handle.checked_add(1).ok_or("event handle overflow")?;
        *self.next_handle = next;
        self.free_slots -= 1;
        Ok(handle)
    }

    fn reserve_child(&mut self) -> Result<i32, String> {
        let handle = self.reserve_slot()?;
        self.tasks.register(self.handle, handle)?;
        Ok(handle)
    }

    fn call_event(
        &mut self,
        kind: u32,
        key: u32,
        event_actor: i16,
    ) -> Result<NativeResult, String> {
        let Some(entry) = self.scenario.event(kind, key) else {
            return Ok(NativeResult::Continue(None));
        };
        let completion = self.operations.begin()?;
        let handle = self.reserve_child()?;
        self.spawns.push(Spawn {
            handle,
            target: SpawnTarget::Callback {
                entry,
                key,
                event_actor,
                completion: completion.clone(),
            },
        });
        *self.wait = Some(Wait::Complete(completion));
        Ok(NativeResult::Suspend)
    }

    fn notice(&mut self, arguments: &[i32], flags: u16) -> Result<NativeResult, String> {
        let text = self.message(arguments)?;
        let operation = self.world.show_notice(
            ResolvedMessage {
                tokens: vec![TextToken::Text { text }],
            },
            flags,
        )?;
        self.operations.track(&operation)?;
        *self.wait = Some(Wait::Complete(operation));
        Ok(NativeResult::Suspend)
    }

    fn text_reference(&self, kind: TextReferenceKind, value: i32) -> Result<String, String> {
        match kind {
            TextReferenceKind::Character => self
                .resources
                .names(self.world.party.as_ref())
                .remove(&value)
                .ok_or_else(|| format!("character name {value} is unavailable")),
            TextReferenceKind::Item => self
                .resources
                .text
                .items
                .get(&u16::try_from(value).map_err(|_| "invalid item text ID")?)
                .cloned()
                .ok_or_else(|| format!("item name {value} is unavailable")),
        }
    }

    fn message(&self, arguments: &[i32]) -> Result<String, String> {
        let module = self
            .program
            .authored()
            .ok_or("expected an authored program")?;
        let id = u32::try_from(arguments[0]).map_err(|_| "invalid message ID")?;
        let text = module
            .texts
            .get(id as usize)
            .ok_or("message is missing from the authored module")?;
        let Some(template) = module.templates.get(&id) else {
            return Ok(text.clone());
        };
        let values = template
            .parameters
            .iter()
            .zip(&arguments[1..])
            .map(|(parameter, value)| match parameter.ty {
                Type::I32 => Ok(value.to_string()),
                Type::TextReference { kind, .. } => self.text_reference(kind, *value),
                _ => Err("unsupported message substitution type".into()),
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut text = String::new();
        for part in &template.parts {
            match part {
                MessagePart::Text(literal) => text.push_str(literal),
                MessagePart::Argument(index) => text.push_str(
                    values
                        .get(usize::from(*index))
                        .ok_or("message substitution is missing")?,
                ),
            }
        }
        Ok(text)
    }

    fn wait_ticks(&mut self, duration: u32) -> Result<NativeResult, String> {
        if duration == 0 {
            return Ok(NativeResult::Continue(None));
        }
        let tick = self
            .world
            .tick
            .checked_add(duration)
            .ok_or("field wait clock overflow")?;
        *self.wait = Some(Wait::Tick(tick));
        Ok(NativeResult::Suspend)
    }
}

fn flag(value: i32) -> Result<u16, String> {
    value.try_into().map_err(|_| "invalid story flag ID".into())
}
