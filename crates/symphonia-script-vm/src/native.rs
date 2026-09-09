//! Static native bindings. Signatures and handlers are registered together.
use crate::{ARGUMENT_STACK_LIMIT, Memory};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSignature {
    pub arguments: usize,
    pub returns_value: bool,
}
#[derive(Debug, Clone, Copy)]
pub enum NativeResult {
    Continue(Option<i32>),
    Suspend,
}
pub type NativeHandler<H> = fn(&mut H, &[i32], &mut Memory) -> Result<NativeResult, String>;

pub struct NativeBinding<H> {
    pub signature: NativeSignature,
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
            handler,
        });
        self
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
}
