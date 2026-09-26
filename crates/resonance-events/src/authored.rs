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

pub(crate) struct Spawn {
    pub handle: i32,
    pub entry: u32,
    pub arguments: Vec<i32>,
}

#[repr(u8)]
enum FieldCall {
    WaitTicks,
    NextUpdate,
    Flag,
    SetFlag,
    Notice,
    CharacterText,
    ItemText,
}

const WAIT_TICKS: NativeDeclaration = NativeDeclaration {
    name: "game::field::wait_ticks",
    opcode: FieldCall::WaitTicks as u8,
    parameters: &[Type::Ticks],
    result: None,
    suspends: true,
};
const NEXT_UPDATE: NativeDeclaration = NativeDeclaration {
    name: "game::field::next_update",
    opcode: FieldCall::NextUpdate as u8,
    parameters: &[],
    result: None,
    suspends: true,
};
const FLAG: NativeDeclaration = NativeDeclaration {
    name: "game::story::flag",
    opcode: FieldCall::Flag as u8,
    parameters: &[Type::I32],
    result: Some(Type::Bool),
    suspends: false,
};
const SET_FLAG: NativeDeclaration = NativeDeclaration {
    name: "game::story::set_flag",
    opcode: FieldCall::SetFlag as u8,
    parameters: &[Type::I32, Type::Bool],
    result: None,
    suspends: false,
};
const NOTICE: NativeDeclaration = NativeDeclaration {
    name: "game::field::notice",
    opcode: FieldCall::Notice as u8,
    parameters: &[Type::Message],
    result: None,
    suspends: true,
};
const CHARACTER_TEXT: NativeDeclaration = NativeDeclaration {
    name: "game::text::character",
    opcode: FieldCall::CharacterText as u8,
    parameters: &[Type::I32],
    result: Some(Type::TextReference {
        name: "game::text::Character",
        kind: TextReferenceKind::Character,
    }),
    suspends: false,
};
const ITEM_TEXT: NativeDeclaration = NativeDeclaration {
    name: "game::text::item",
    opcode: FieldCall::ItemText as u8,
    parameters: &[Type::I32],
    result: Some(Type::TextReference {
        name: "game::text::Item",
        kind: TextReferenceKind::Item,
    }),
    suspends: false,
};

pub fn native_declarations() -> Vec<NativeDeclaration> {
    FieldHost::AUTHORED_NATIVES.declarations().collect()
}

pub(crate) struct FieldHost<'a> {
    pub world: &'a mut GameWorld,
    pub resources: &'a ResourceLibrary,
    pub program: &'a Program,
    pub wait: &'a mut Option<Wait>,
    pub operations: &'a mut OperationScope,
    pub handle: i32,
    pub tasks: &'a mut Tasks,
    pub spawns: &'a mut Vec<Spawn>,
    pub next_handle: &'a mut i32,
    pub free_slots: usize,
}

impl Host for FieldHost<'_> {
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register_typed(WAIT_TICKS, |host, args, _| host.wait_ticks(args[0] as u32))
        .register_typed(NEXT_UPDATE, |host, _, _| host.wait_ticks(1))
        .register_typed(FLAG, |host, args, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.world.event_flags.contains(&flag(args[0])?),
            ))))
        })
        .register_typed(SET_FLAG, |host, args, _| {
            let flag = flag(args[0])?;
            if args[1] != 0 {
                host.world.event_flags.insert(flag);
            } else {
                host.world.event_flags.remove(&flag);
            }
            Ok(NativeResult::Continue(None))
        })
        .register_typed(NOTICE, |host, args, _| {
            let text = host.message(args)?;
            let operation = host.world.show_notice(
                ResolvedMessage {
                    tokens: vec![TextToken::Text { text }],
                },
                0,
            )?;
            host.operations.track(&operation)?;
            *host.wait = Some(Wait::Complete(operation));
            Ok(NativeResult::Suspend)
        })
        .register_typed(CHARACTER_TEXT, |host, args, _| {
            let id = if args[0] == crate::CONTROLLED_ACTOR {
                host.world.controlled_actor
            } else {
                args[0]
            };
            host.text_reference(TextReferenceKind::Character, id)?;
            Ok(NativeResult::Continue(Some(id)))
        })
        .register_typed(ITEM_TEXT, |host, args, _| {
            host.text_reference(TextReferenceKind::Item, args[0])?;
            Ok(NativeResult::Continue(Some(args[0])))
        });

    fn spawn(&mut self, function: u16, arguments: &[i32]) -> Result<i32, String> {
        if self.free_slots == 0 {
            return Err("event pool exhausted (32 instances)".into());
        }
        let function = self
            .program
            .authored()
            .and_then(|module| module.functions.get(usize::from(function)))
            .filter(|function| function.is_task)
            .ok_or("spawn target is not a task")?;
        let handle = *self.next_handle;
        let next = handle.checked_add(1).ok_or("event handle overflow")?;
        self.tasks.register(self.handle, handle)?;
        *self.next_handle = next;
        self.free_slots -= 1;
        self.spawns.push(Spawn {
            handle,
            entry: function.entry,
            arguments: arguments.to_vec(),
        });
        Ok(handle)
    }

    fn join(&mut self, handle: i32) -> Result<Option<Vec<i32>>, String> {
        self.tasks.join(self.handle, handle)
    }
}

impl FieldHost<'_> {
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
