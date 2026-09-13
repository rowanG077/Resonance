//! Static native bindings. Signatures and handlers are registered together.
use crate::{ARGUMENT_STACK_LIMIT, Memory};
use symphonia_script::authored::NativeDeclaration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSignature {
    pub arguments: usize,
    pub returns_value: bool,
}
#[derive(Debug, Clone)]
pub enum NativeResult {
    Continue(Option<i32>),
    /// Immediate fixed-layout authored result, flattened in declaration order.
    Values(Vec<i32>),
    Suspend,
}
pub type NativeHandler<H> = fn(&mut H, &[i32], &mut Memory) -> Result<NativeResult, String>;

pub struct NativeBinding<H> {
    pub signature: NativeSignature,
    pub declaration: Option<NativeDeclaration>,
    pub(crate) handler: NativeHandler<H>,
}
/// One slot per bytecode ID; unregistered calls fail before consuming arguments.
/// Build in a host's associated constant: no allocation or runtime registration.
pub struct NativeBindings<H> {
    entries: [Option<NativeBinding<H>>; 256],
}
impl<H> NativeBindings<H> {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; 256],
        }
    }
    /// Duplicate IDs are errors, including when this builder is evaluated at compile time.
    pub const fn register(
        mut self,
        opcode: u8,
        arguments: usize,
        returns_value: bool,
        handler: NativeHandler<H>,
    ) -> Self {
        assert!(
            self.entries[opcode as usize].is_none(),
            "duplicate native binding"
        );
        assert!(
            arguments <= ARGUMENT_STACK_LIMIT,
            "native arguments exceed the VM stack"
        );
        self.entries[opcode as usize] = Some(NativeBinding {
            signature: NativeSignature {
                arguments,
                returns_value,
            },
            declaration: None,
            handler,
        });
        self
    }
    /// Use the same declaration when checking source and registering execution.
    pub const fn register_typed(
        self,
        declaration: NativeDeclaration,
        handler: NativeHandler<H>,
    ) -> Self {
        assert!(
            match declaration.result {
                Some(ty) =>
                    ty.slots() > 0
                        && ty.slots() <= symphonia_script::authored::VALUE_SLOT_LIMIT
                        && !ty.contains_message()
                        && !(ty.aggregate() && declaration.suspends),
                None => true,
            },
            "native result must have a bounded layout; aggregate results cannot suspend or contain messages"
        );
        let mut bindings = self.register(
            declaration.opcode,
            declaration.argument_slots(),
            declaration.result.is_some(),
            handler,
        );
        bindings.entries[declaration.opcode as usize]
            .as_mut()
            .unwrap()
            .declaration = Some(declaration);
        bindings
    }
    pub fn declarations(&self) -> impl Iterator<Item = NativeDeclaration> + '_ {
        self.entries
            .iter()
            .filter_map(|entry| entry.as_ref()?.declaration)
    }
    pub fn get(&self, opcode: u8) -> Option<&NativeBinding<H>> {
        self.entries[usize::from(opcode)].as_ref()
    }
}
impl<H> Default for NativeBindings<H> {
    fn default() -> Self {
        Self::new()
    }
}
/// The host owns the ABI and state; the interpreter only invokes registered handlers.
/// An empty table is useful for pure expressions with no native capabilities.
pub trait Host: Sized {
    const NATIVES: NativeBindings<Self> = NativeBindings::new();
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::new();
    /// Admit a child of the currently running task. The host owns scheduling.
    fn spawn(&mut self, _function: u16, _arguments: &[i32]) -> Result<i32, String> {
        Err("this host does not support child tasks".into())
    }
    /// Consume a completed owned child, or report a pending join. Completion
    /// later reaches the same VM through `complete_task`.
    fn join(&mut self, _handle: i32) -> Result<Option<Vec<i32>>, String> {
        Err("this host does not support child task joins".into())
    }
}
