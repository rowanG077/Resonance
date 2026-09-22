# SymphoniaScript

SymphoniaScript runs original event bytecode and readable authored scripts against
ordinary Rust game services. Authored source compiles to an immutable program;
both execution profiles use the existing VM suspension/completion mechanism.
Scene setup supplies asset mappings; presentation consumes the resulting state.

## Crate boundaries

| Crate | Owns |
|---|---|
| `symphonia-script` | Decoding, assembly/disassembly, immutable `Program`, `NativeCall` IDs, inspection metadata |
| `symphonia-script-compiler` | Readable source, imports, static types, lowering, diagnostics and formatting |
| `symphonia-script-vm` | Checked stacks and memory, expressions, control flow, native registration, suspension |
| `symphonia-script-tools` | Source discovery, immutable preparation cache and authoring commands |
| `resonance-events` | Native service implementations, verified signatures, event scheduling, actor/camera/dialogue/effect state |

The VM does not depend on game objects, scene IDs, Bevy or the native metadata
catalog. Shims operate on high-level state, without CPU, native-pointer or GX
emulation. The presentation adapter applies that state to meshes, animations,
cameras, dialogue, audio and effects.

## Readable source

Authored `.sym` files use UTF-8 text, ASCII identifiers and conventional braces and
semicolons. Comments, strings and message literals may contain Unicode; that does not imply
that every character has a glyph or that arbitrary text shaping is implemented.
Text lives in the compiled module's immutable literal/template tables. A message
value contains its resource index and bounded substitution arguments, not a
mutable string heap.

```rust
script field;
use game::field;
use game::story;

message greeting = "Welcome back, Lloyd.";

pub task start() {
    if !story::flag(500) {
        await field::notice(greeting);
        story::set_flag(500, true);
    }
    await field::wait_ticks(20ticks);
}
```

Module names follow logical paths: `field::start` is `field/start.sym` under the
source root. `use` imports modules or public declarations. Imports are checked
transitively; cycles, unknown names and access to private declarations fail.

Every file starts with `script field;`, `script model;`, or `script library;`
(comments may precede it). The checker selects the native API from this declaration,
and preparation rejects an entry for the wrong host. Field and model scripts may
import their own kind or libraries. Libraries may import other libraries and use
the importing entry's native API. Check a library requiring host services through
its field/model entry. Headers are required; there is no command-line mode override.

Implemented language constructs:

- Typed `fn` and suspendable `task`, arguments, return values and private locals.
- Owned child tasks through `spawn`, with typed `Task<T>` handles and explicit joins.
- Non-suspending `defer { ... }` blocks for orderly lexical-scope cleanup.
- `let`, `let mut`, assignment, arithmetic, bitwise and Boolean expressions.
- `if`/`else`, `while`, `for` over integer ranges, fixed arrays or host collections,
  `break`, `continue`.
- Fixed-layout structs, tuple-payload enums, fixed arrays and `Option<T>`.
- Exhaustive statement `match`, including payload bindings and wildcard arms.
- `i32`, `f32`, `bool`, `string`, `Ticks`, `Message`, and native-declared handles/assets.
- Explicit `i32(...)`, `f32(...)` and `ticks(...)` conversions. `20ticks` is a
  duration literal. Integer overflow and invalid arithmetic are runtime faults.
- Literal/array constants, named messages and typed logical asset declarations.
- Typed message templates with named number, character-name and item-name substitutions.

Records and arrays have value semantics and compile to fixed local slots. They can
be passed to and returned from functions. Fields and array elements can be mutated
only through mutable locals. General dynamic lists, string manipulation,
script-defined persistent globals, closures and recursion are not supported.
Nested dynamic indexing currently requires an intermediate local.

Resource names are ordinary constants, scoped by their module:

```rust
pub const Tail: string = "Bone_sippo01";
pub const TailVisible: i32 = 147;
pub const WingsVisible: i32 = 148;
```

These have exactly the same types as their literal values. Scripts may define and
import more constants without extending an enum or changing native declarations.
Native services validate requested resources when used; the standard library is
a collection of useful names, not a closed set of permitted values.

Quoted literals produce immutable `string` values unless the expected type is
`Message`. Strings are interned once per compiled module and do not allocate during
execution. They are separate from dialogue messages and do not require glyph
preparation. Existing payload enums keep their tagged, fixed-layout representation.

`!` performs Boolean negation or integer bitwise complement, according to its
operand type.

Duration conversions and arithmetic reject negative results immediately, before
the value can reach a comparison, return or native call. Integer overflow and
division by zero remain explicit faults.

Entry arguments supplied by Rust or `fields.json` are checked before activation,
including scalar domains inside records, arrays and selected enum/option payloads.
Booleans must be `0` or `1`, ticks cannot be negative, and float bit patterns must
be finite. Invalid tags, message resources and nonzero inactive payload padding
are rejected as well. Host services still own handle lifetime and asset identity.

`await` checks that the called task or native is suspendable and that its caller
is a task. It lowers to ordinary calls: the VM already retains its continuation
while a native waits. An authored `fn` cannot call a task without `await`, and
cannot itself use `await`. No Rust futures or generated asynchronous state machine
is involved.

`spawn` starts an authored child task through the host's existing event scheduler:

```rust
pub task start() {
    let child: Task<i32> = spawn work(7);
    let value = await child;
}

task work(value: i32) -> i32 { return value * 2; }
```

Only tasks may spawn or join. `Task` denotes a child with no return value;
`Task<T>` carries the child's fixed-layout result. Handles stay in local variables:
they cannot cross function parameters/results or enter records, arrays or options.
The scheduler checks parent ownership and permits one successful join per handle.
A completed child joins immediately; a pending child suspends through the same VM
continuation mechanism used by native waits. Returning or cancelling the parent
cancels its unfinished children and their owned operations. There is no detached
task mode or additional asynchronous executor.

`defer { ... }` runs at orderly block exit, including `return`, `break` and
`continue`. Defers run in reverse registration order, with inner scopes first.
Names bind where the defer is declared; the cleanup reads their values at exit.
A return expression is evaluated before cleanup. Cleanup cannot await, spawn,
return, or break/continue an enclosing loop; loops inside cleanup work normally.
The compiler emits ordinary synchronous instructions, without adding VM callbacks.
Cancellation and faults do not run arbitrary deferred script code: engine-owned
operation scopes remain responsible for releasing resources on those paths.

Read-only collections let host services expose rosters without creating a script
heap. A host may provide an actor view that supports `for actor in scene::actors(view)`;
collection types and operations come from its native declarations.

The host declares `Type::Collection { name, element, count, get }`, naming ordinary
non-suspending natives with signatures `count(collection) -> i32` and
`get(collection, i32) -> element`. Elements occupy one scalar slot; nested
collections and message values are not collection elements. The compiler checks
these signatures and evaluates the collection expression and count once at loop
entry. Each iteration obtains its element through `get`, preserving host order.
The host owns the view and validates lifetime, nonnegative counts and indices;
stale views and out-of-range accesses fault rather than skipping elements. A view
can remain in locals across waits when its host permits that lifetime. This adds
no VM collection allocation, indexing or mutation API. Explicit count/get calls
remain useful when a script needs original roster slot indices or a fresh roster
on each update.

### Message templates

Templates declare their arguments and use named placeholders:

```rust
use game::field;
use game::text;

message found(who: text::Character, item: text::Item, count: i32) =
    "{who} found {count} × {item}.";

pub task start() {
    let line = found(text::character(1), text::item(10), 3);
    await field::notice(line);
}
```

`text::character` and `text::item` validate the game identifier and return distinct
reference types. Passing an item where a character is expected is a compile error.
Rust dialogue services resolve character references using current party names and
item references using the item database when the notice opens. Numbers are signed
`i32` values formatted in base ten without grouping, padding or locale rules.

Within a parameterized template, `{{` and `}}` produce literal braces. Unknown
placeholders, unmatched braces, duplicate parameter names, unsupported argument
types and incorrect calls are errors. Quoted literals in a `Message` context and
declarations such as `message greeting = "Hello.";` remain literal dialogue text;
their braces are not parsed.
An explicit zero-argument template, `message greeting() = "{{Hello}}";`, uses the
template escaping rules.

`MAX_MESSAGE_ARGUMENTS` is eight. Every message occupies nine slots: its immutable
resource ID and eight argument slots, with unused slots zeroed. Messages have value
semantics, can be retained across waits, passed to functions and returned from
functions. Imported public templates use the same rules. Rust native inputs are
flattened according to these widths and checked before dispatch. Constructing a
message remains a source-language operation; native returns cannot contain messages.

No format expressions, arbitrary string concatenation or string allocation is
available in the VM. Preparation checks template literal chunks and all required
substitution glyph banks before activation, including decimal digits and the
supported character-rename alphabet.

### Native declarations and compilation

An authored `NativeDeclaration` supplies the symbolic name, opcode, argument and
result types, and whether the operation may suspend. The same declaration feeds
the compiler and `NativeBindings::register_typed`. `Host::AUTHORED_NATIVES` keeps
this ABI separate from the legacy `Host::NATIVES`. Fully qualified native types,
such as `game::text::Character`, resolve through ordinary imports.

Native settings and tables can use fixed layouts: `Type::Record { name, fields }`
declares named `NativeField { name, ty }` members, and
`Type::Array { element, len }` declares a fixed array. Records and arrays may nest.
Scripts use their ordinary value syntax, such as `settings.rows[index].age`,
and can copy, edit and pass these values to source functions or native calls.
Every boundary validates flattened slot counts and the nested scalar domains;
there is no mutable host object or script heap behind a returned record.

Handlers return these immediate values as `NativeResult::Values(Vec<i32>)` in
field/element order. `Continue(Option<i32>)` retains scalar and legacy behavior.
Aggregate native results cannot suspend. Native value layouts occupy 1–1024
slots, argument lists remain bounded to 64 slots, and malformed/recursive
layouts or mismatched returned values fail explicitly. Collection views remain
opaque host handles; they are distinct from these copied fixed arrays.

The compiler's `compile(entry_module, resolver, declarations)` returns a checked
`Program`, static asset references and the exact transitive source inputs. It has
no game-engine or filesystem dependency. Preparation owns the shared `Arc<Program>`
and invalidates cached generations when sources, imports or declarations change.
Failed preparation does not install partial or stale replacement programs.

Assets declared in source are dependencies, not files generated by compiling.
For example, a host declaring `scene::Texture` can accept:

```rust
asset portrait: scene::Texture = "characters/portrait.ktx2";
```

Event implementations remain checked-in source. Editing them does not require
recooking geometry, textures, parameters or other unchanged assets.

### Authoring tools

```sh
cargo run -p resonance-script -- check scripts preview::sword_dancer
cargo run -p resonance-script -- api model
cargo run -p resonance-script -- fmt --check scripts
```

The game command selects the native API from the script's mode. `api field` and
`api model` print the corresponding host declarations.
The engine-independent `symphonia-script-tools` binary can also check pure modules
with an empty native API. Formatting validates syntax first, preserves comments
and UTF-8 text, and is idempotent. Diagnostics include module, line and column;
authored runtime errors can resolve their script call stack back to source.

`resonance_game::authored::PreparedEvent` joins compilation, required-resource
validation and starting an event in the existing scheduler. Its explicit entry
includes the module, task and arguments.

### Running authored field entries

Start the game with `--scripts ROOT` to enable editable field bindings. The
directory contains `.sym` modules and a `fields.json` mapping field IDs to entry
tasks. For example, place the field script above in
`local/events/field/start.sym` and create `local/events/fields.json`:

```json
{
  "330": {
    "module": "field::start",
    "task": "start",
    "on": "arrival"
  }
}
```

```sh
cargo run -p resonance -- --scripts local/events --silent
```

`arrival` (the default) runs once after arriving from another field; `entry` also
runs after restoring a save. Both wait for the first player-controlled update
after presentation is ready. The event owns foreground control while it runs,
and saves are unavailable while it is queued or active.

Field preparation rereads bindings and sources, including cached field revisits
and quickloads. Unchanged programs reuse the compiler cache; active tasks retain
their original immutable generation. Compilation or missing-glyph errors block
the replacement rather than falling back to old scripts. Message literals and
typed substitutions are checked against the field's prepared font. Substitution
checks cover item/character labels, the supported rename alphabet, and signed
integer digits, including values selected on a later execution branch. Static
asset references must already belong
to its prepared inventory; authoring scripts does not load new graphics during
play. Omitting `--scripts` disables optional authored field entries. Built-in
model behaviors still load from the cooked `scripts/` directory.

## Model behavior

Maintained model scripts are embedded into the cooker with `include_str!` and
published verbatim under `<assets>/scripts`. The manifest selects named resources
and entry functions; it supplies no function arguments. Preparation resolves named
nodes and validates entry signatures without executing behavior. Sources are immutable cooked assets: field preparation
verifies their hashes and retains their bytes. The renderer compiles that verified
snapshot before activating a preview and reuses immutable programs through the
existing preparation cache.

The entire `std::` library is checked in under [scripts/std](../scripts/std/README.md).
The build embeds it alongside the model scripts; cooking copies it unchanged.
It supplies story flags, characters, items, monsters, figurines, locations and
model node names as ordinary `i32` and `string` constants. Definitions record
original IDs/names, with readable names for confirmed meanings and explicit
fallback names for unknown meanings. No symbol manifest or source generator is
involved. The standard-library index links known model node modules; new models
remain cookable without a standard-library entry.

`std::story::SwordDancerTailVisible` is integer 147;
`std::characters::Lloyd` is canonical actor ID 1, independently of localized
display text. Story flag IDs use the save format's `u16` range; scripts may define
additional constants without extending an enum.

Monsters and figurines use the same model and animation machinery, with separate
catalogue IDs. Identical ordered bone-name interfaces share a node module, even
when their meshes differ. Modules supply plain strings such as
`pub const Tail: string = "Bone_sippo01";`. `model::node(name: string)` accepts
constants or literals and returns a node handle. Preparation resolves the
program's interned names into body/outline targets, excluding attachments; calls
use those prepared indices. Requesting an unknown or ambiguous name fails with a
script location. Duplicate names use zero-based occurrence suffixes (`bone#0`,
`bone#1`); a literal `#` in a source name is escaped as `##`.
The host also provides `model::elevation() -> f32`,
`model::set_translation(f32, f32, f32)` and
`model::set_scale(model::Node, f32, f32, f32)`. Entries are parameterless synchronous
functions without a return value. For example:

```rust
script model;
use model;
use game::story;
use std::story::SwordDancerTailVisible;
use std::nodes::sword_dancer_191;
pub fn apply() {
    if !story::flag(SwordDancerTailVisible) {
        model::set_scale(model::node(sword_dancer_191::Tail), 0.0, 0.0, 0.0);
    }
}
```

Operations affect only the current
instance. Preview evaluation produces fresh overrides once per menu tick, with
a bounded instruction budget; it cannot suspend or mutate game progress.

Use `resonance-script check scripts preview::sword_dancer` to check against the
checked-in library, and `resonance-script api model` to inspect the native API.
Adding `--assets ASSETS` checks against cooked `std` exclusively, ignoring local
standard-library files. Both field and model runtime preparation resolve `std`
from verified cooked bytes. Library modules declare `script library;` and can be
imported by either script kind.
Change the checked-in source, rebuild and recook to update these programs.
Missing or modified cooked files fail integrity checks; there is no embedded
runtime fallback. The development `--scripts` field-entry tool does not override
cooked model scripts. Mod overlays are future work.

## Legacy native registration

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
Service handlers match named variants; there is no fallback chain of numeric
signature lookups.

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

## Legacy execution and shared completion

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
cargo test -p symphonia-script -p symphonia-script-compiler -p symphonia-script-vm -p symphonia-script-tools -p resonance-events
cargo test -p resonance-game --test classroom_script --test new_game_setup -- --ignored
```

The second command needs locally cooked assets; `RESONANCE_TEST_ASSETS` selects
another cooked directory. These tests open no audio or graphics devices. Tests
cover enum/catalog agreement, registered signatures, duplicate registration,
borrowed host state, nested arguments, immediate/deferred results, unsupported
calls, stack/memory limits and the original classroom/setup events. Bytecode
`.ssb` files retain their original encoding and are hash-checked before execution.
