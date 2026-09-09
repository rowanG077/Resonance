# SymphoniaScript

SymphoniaScript runs the original event bytecode against ordinary Rust game
services. Title and field events share the same bindings. Scene setup supplies
asset mappings; rendering and audio consume the resulting game state.

## Crate boundaries

| Crate | Owns |
|---|---|
| `symphonia-script` | Decoding, assembly/disassembly, immutable `Program`, `NativeCall` IDs, inspection metadata |
| `symphonia-script-vm` | Checked stacks and memory, expressions, control flow, native registration, suspension |
| `resonance-events` | Native service implementations, verified signatures, event scheduling, actor/camera/dialogue/effect state |

The VM does not depend on game objects, scene IDs, Bevy or the native metadata
catalog. Shims operate on high-level state, without CPU, native-pointer or GX
emulation. The presentation adapter applies that state to meshes, animations,
cameras, dialogue, audio and effects.

## Native registration

`NativeCall` is a `#[repr(u8)]` enum covering the 233 catalogued native calls.
Discriminants are their bytecode IDs. `TryFrom<u8>` checks conversion;
`NativeCall::ALL` lists them. Interpreter control flow has separate `Op` variants.
Calls with unresolved semantics retain `UnknownXX` names instead of guessed
behavior. An enum variant does not grant a script access to a handler.

The supported ABI is declared once in
[`native/bindings.rs`](../crates/resonance-events/src/native/bindings.rs):

```rust
SetActorPosition(4) -> () = dispatch;
ChangeItemCount(2) -> i32 = party;
ChangeField(5) -> () = field;
```

Each row supplies the typed ID, argument count, return type and service handler.
The table currently registers 76 calls. Service handlers match named variants;
there is no fallback chain of numeric signature lookups.

The declaration builds `Host::NATIVES`, a `NativeBindings<Self>` constant. The
VM's generic `register` API accepts a bytecode ID, argument count, return flag
and function pointer. The game-specific declaration converts enum IDs to bytes
at this boundary. Registration needs no allocation or runtime initialization;
duplicate IDs and signatures larger than the argument stack fail at compile
time. Host state can borrow the active world, resources and instance registers.
An empty host table is used to evaluate dialogue expressions without native
capabilities.

When a native instruction executes, the VM looks up its binding before touching
the argument stack. It passes only that signature's argument tail to the handler,
preserving outer arguments during nested calls. Unregistered calls fail with the
instruction address and bytecode ID. Handler errors additionally include the
semantic call name and arguments.

To implement another service:

1. Use its existing `NativeCall` variant, giving an unresolved ID a semantic name
   once its behavior is understood. Keep the discriminant unchanged.
2. Implement the shim using game state and add its verified signature/handler to
   `native/bindings.rs`. Inspection metadata (`NativeProcedure`) does not supply
   default runtime implementations or return values.
3. Test the resulting state and completion behavior. Validate original events
   with the local assets and oracle fixtures that exercise the service.

## Execution and completion

The VM checks the 64-value, 64-argument and 16-call limits, memory ranges, access
widths and instruction targets. Arithmetic preserves the bytecode's 32-bit
integer and shift semantics. Invalid operations, unsupported calls and exhausted
instruction budgets are errors. Halt and failure are terminal.

Handlers return `NativeResult::Continue(value)` or `NativeResult::Suspend`.
Immediate and deferred return values must match the registered signature.
A suspended call must be completed exactly once before execution resumes.
The scheduler supplies the corresponding wait condition: time, movement,
animation, dialogue, choice, movie, voice or field handoff.

The scheduler uses stable slot order, a 32-instance limit, 8,192 instructions
per resume and 32,768 per update. Instances share script memory, while coordinate
registers and VM stacks remain local to each instance. Resource/event handles are
Rust IDs. Scene readiness gates script execution while dependencies are prepared.

## Verification

```sh
cargo test -p symphonia-script -p symphonia-script-vm -p resonance-events
cargo test -p resonance-game --test classroom_script --test new_game_setup -- --ignored
```

The second command needs locally cooked assets; `RESONANCE_TEST_ASSETS` selects
another cooked directory. These tests open no audio or graphics devices. Tests
cover enum/catalog agreement, registered signatures, duplicate registration,
borrowed host state, nested arguments, immediate/deferred results, unsupported
calls, stack/memory limits and the original classroom/setup events. Bytecode
`.ssb` files retain their original encoding and are hash-checked before execution.
