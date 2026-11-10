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
- Fixed-layout structs, enums with optional integer discriminants and tuple payloads,
  fixed arrays and `Option<T>`.
- Exhaustive statement `match`, including payload bindings and wildcard arms.
- `i32`, `f32`, `bool`, `string`, `Ticks`, `Message`, and native-declared handles/assets.
- Explicit `i32(...)`, `f32(...)` and `ticks(...)` conversions. `20ticks` is a
  duration literal. Integer overflow and invalid arithmetic are runtime faults.
- Literal/array constants, named messages and typed logical asset declarations.
- Typed message templates with named number, character-name and item-name substitutions.

Enums declare numeric identities directly: `enum Model { Portal = 1, Caravan = 11 }`.
Omitted discriminants start at zero or increment the previous value. Discriminants
must be unique signed 32-bit integers. A fieldless enum converts explicitly with
`i32(value)`; integers cannot be cast into enums. Different enum types remain
incompatible even when their discriminants coincide.

Records and arrays have value semantics and compile to fixed local slots. They can
be passed to and returned from functions. Fields and array elements can be mutated
only through mutable locals. General dynamic lists, string manipulation,
closures and recursion are not supported.
Nested dynamic indexing currently requires an intermediate local.

Persistent variables are declared in source with an explicit type and default:

```rust
enum Stop { Triet = 54, Hima = 55 }
state stop: Stop = Stop::Triet;

pub fn move_caravan() {
    stop = Stop::Hima;
}
```

`state` supports `i32`, `f32`, `bool`, `Ticks` and fieldless enums. Reads and
assignments use the declared type, including across imported library functions.
Defaults apply to missing values; reads and writes validate saved scalar domains
and enum discriminants. A state's qualified declaration name is its save key,
independent of the caller's compiled program or VM instance. Names and enum
discriminants define the saved representation; save compatibility is not guaranteed
during pre-alpha.
Strings, runtime handles, assets and aggregate values cannot be persisted this way.
State is private unless exported with `pub`; it cannot initialize a constant.

Game hosts preserve these values through field/world transitions and save/load.
The VM delegates storage through `Host::load_state` and `Host::store_state`, without
knowing game-specific variables. Hosts without persistent storage reject state
access; the overworld's presentation refresh exposes it read-only. World entry
hooks commit their state changes only after the complete hook succeeds.

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
`Type::Array { element, len }` declares a fixed array. `Type::Enum { name, variants }`
uses `NativeVariant { name, tag, payload }` to expose an ADT directly. These types
may nest. Enum values contain the declared tag, the selected payload, then zero
padding to the widest payload. Invalid tags, payload types and padding fail at
both host and script boundaries.
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

The game command selects the native API from the script's mode. Use
`check --host world scripts world::rules` for overworld rules, which have a
separate runtime API. `api field`, `api world`, and `api model` print each host's declarations.
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

Field preparation rereads bindings and sources, including field revisits and
save restores. Unchanged programs reuse the compiler cache; active tasks retain
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
published verbatim under `<assets>/scripts`. Model documents select named resources
and entry functions; they supply no function arguments. Preparation resolves named
nodes and validates entry signatures without executing behavior. It captures the
current script bytes, including mod overrides, and the renderer compiles that
snapshot before activating a preview. Replace a file at its existing path and
restart to load the edit; no manifest or author-maintained checksum is required.

The numeric standard definitions are generated from the shared
[`symphonia-constants`](../crates/symphonia-constants/README.md) Rust enums during
cooking. They cover story flags, characters, fields, music, items, monsters,
figurines, world landmarks and travel. The generated `.sym` enums retain the
explicit discriminants; there is no separately maintained script catalogue.
For example, `i32(std::story::StoryFlag::SwordDancerTailVisible)` is 147,
`i32(std::characters::CharacterId::Lloyd)` is 1, and
`i32(std::items::ItemId::MemoryGem)` is 491. Script services still accept raw
numbers, so mods can introduce identities without changing the Rust enums.

Model nodes are string labels, not numeric IDs. `resonance-script nodes MODEL.glb`
generates an optional reference module from a published GLB skeleton. Cooking
generates only the small set of semantic aliases used by shipped behaviors in
`std::model_nodes`. No checked-in skeleton catalogue or hash registry is needed.
`model::node(name: string)` accepts
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
use std::story::StoryFlag;
use std::model_nodes;
pub fn apply() {
    if !story::flag(i32(StoryFlag::SwordDancerTailVisible)) {
        model::set_scale(model::node(model_nodes::SwordDancerTail), 0.0, 0.0, 0.0);
    }
}
```

Operations affect only the current
instance. Preview evaluation produces fresh overrides once per menu tick, with
a bounded instruction budget; it cannot suspend or mutate game progress.

Use `resonance-script check scripts preview::sword_dancer` to check against the
standard library generated from Rust, and `resonance-script api model` to inspect the native API.
Adding `--assets ASSETS` checks against cooked `std` exclusively, ignoring local
standard-library files. Both field and model runtime preparation resolve `std`
from the current captured content. Library modules declare `script library;` and
can be imported by either script kind.
Edit the installed `scripts/` files directly or provide those paths in a mod
directory, then restart the application. Preparation checks and compiles the
effective sources; no cooker-recorded checksum or recook is required. Missing or
invalid required modules fail preparation without an embedded runtime fallback.
The development `--scripts` option selects a field-entry project; use ordinary
content mods to override shared or model script files.

`configure_battle_rules(rule, value)` names the original E7 service. Its
`scenario::BattleRule` selector distinguishes `Modifiers`, `DisabledCommands`,
`Coliseum`, `AttackPercent`, `DefensePercent` and `IntelligencePercent`. Niflheim
uses the percentage adjustments; Meltokio 0392 brackets coliseum encounters with
the coliseum flag. Each call returns the previous value. Modifiers store the low
16 bits; disabled commands store the low byte; percentage adjustments store a
signed byte. Coliseum treats every nonzero input as enabled. Unknown selectors
return zero without changing settings. These settings persist independently of
story flags, and the value operand remains a scalar or mask appropriate to its
selector rather than another rule ID.

Equipment queries and removals use `items::EquipmentSlot::{Weapon, Body,
Head, Accessory1, Accessory2, Arm}`. The script order differs from physical save
order: selector5 is the shield/gauntlet/bracelet slot, while selectors3/4 are the
two accessory slots. Query results remain item IDs, so Exire's gemstone checks
name both the accessory slot and the gemstone. Invalid character/slot queries
return zero; removals outside the six slots do nothing.

Script equipment updates retain their original inventory sequences. An occupied
weapon, body, head or arm replacement adds the requested item, removes one of
that same item, then overwrites the equipment; the displaced item is lost. At
capacity, the failed addition is followed by the removal. Accessories instead
consume first and use the first empty accessory slot, replacing the second when
both are occupied. Scripted removal clears the slot even when its old item
cannot be returned to a full inventory. The equipment menu uses its separate
atomic exchange. Executable and runtime audits cover these distinctions.

`configure_figurine` uses `figurines::FigurineOperation::{SetOwned,
ClearOwned, QueryOwned}` alongside the figurine's own ID. It always returns the
previous ownership bit; operations other than0/1 also act as queries. The native
bank accepts IDs0..327 although the catalogue has288 records. Bits288..327
persist separately as `figurine_reserved_flags` and carry with the Figurines
New Game Plus benefit; they do not become catalogue entries. IDs328 and above
return zero. Negative IDs can alias unrelated original memory and remain an
explicit runtime error.

`snapshot_party` uses `scenario::PartySnapshotOperation::{Capture, Clear}`.
Capture copies up to eight formation entries into independent saved state;
Clear discards that snapshot. Neither operation reorders/restores the current
party. Other operations do nothing, and this native has no return value. Both
operation domains are separate from figurine IDs, character IDs and story flags.

`configure_rendering(command, value)` uses `scenario::RenderingCommand`.
`Setting0` through `Setting7` write the eight field-dependent render settings;
these values can be actor keys, texture indices, timing or other scene scalars,
so they do not receive a guessed global identity domain. `Enabled` (128) accepts
`scenario::RenderingToggle::{Disabled, Enabled}`; every other nonzero value also
enables the field render effects. `Reinitialize` (129) clears the native initialization bit and
ignores its value. Signed commands below zero alias setting `command & 7`;
unsupported positive commands do nothing. Ignored value expressions still execute.

`configure_screen_copy(command, depth_hundredths)` uses
`scenario::ScreenCopyCommand::{SetPass0Depth, SetPass1Depth, ReadPass0Depth,
ReadPass1Depth, ResetPass0Depth, ResetPass1Depth}`. Both planes are independent;
all six commands return their previous depth in hundredths. Reset restores174
or190, and reads/resets ignore the evaluated second argument. Returned values
follow Gekko's truncating/saturating integer conversion, including NaN to the
minimum signed integer, as implemented by
[Dolphin's floating-point interpreter](https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/Core/PowerPC/Interpreter/Interpreter_FloatingPoint.cpp).
Unknown screen-copy commands remain an error because the original has no defined
result assignment for them.

`preload_voice_bank(voice_reference, bank_slot)` names the packed archive
reference and destination separately. `resources::VoiceArchiveReference` lists
all17 original event, skit and battle archives with their source paths. A
reference such as `voice_archive(resources::VoiceArchiveReference::EventScenes001Through009,
86)` preserves both words of the original integer. This native consumes only
the archive's high word; the retained member word does not select a line to play.
`scenario::VoiceBankSlot::{Primary, Secondary}` names the two destination buffers;
every nonzero slot encoding selects the secondary buffer. A zero high word skips
loading. Physical archive handles remain separate from packed reference IDs.

`voice_archive(base, member)` is a compile-time integer constructor: it requires
an aligned unsigned32-bit base and member0..65535, and emits one original-width
literal. Its result uses the signed32-bit interpretation of the packed bits.
It also works inside `encoded` so calculator trees remain unchanged. Unknown
archive references remain numeric, and an equal-valued scalar outside this
native keeps its original role. Directory and filename-byte audits bind these
names to the original archives.

`play_camera_track(resource_handle, start_mode, repeat_mode)` names its options
with `scenario::CameraTrackStart::{Playing, Paused}` and
`scenario::CameraTrackRepeat::{Once, Loop}`. Start mode zero plays; every other
integer pauses. Initial looping requires the full repeat operand to equal1.
Resource handles keep their scene-local identities.

`configure_camera_track(operation, value)` uses `scenario::CameraTrackCommand`
for playback, frame/rate queries, and actor targets. Only `SetRepeat` interprets
its value as a repeat mode; `SetTargetActor` receives actor identities. Frames,
rates, offsets and ignored arguments retain scalar values. `SetRatePercent`
divides by200, so100 restores the normal half-frame step. `ReadFrameStep`
truncates the raw step rather than returning a percentage. `SetRepeat` stores
the low byte and `ReadRepeat` returns that byte; any nonzero stored byte loops.
Offset setters return their floating-point storage converted back to an integer.
Unknown command encodings return zero. Noncanonical encodings stay numeric,
and explicit widths and calculator trees still recompile exactly.

Field camera tracks retain their current frame as single-precision state.
`SeekFrame` stores the requested integer converted to float without clamping;
`ReadFrame` reads that stored state. Rendering samples the current pose before
adding the frame step once per active camera update. Negative frames add one
duration; an upper-end crossing sets the completion byte and either subtracts
one duration when looping or stops at the endpoint. These are single-wrap
operations, including oversized steps. Reaching the endpoint exactly does not
set completion. Pause, rewind and seek preserve the completion byte; starting
a new track clears it. Repeated additions preserve native float rounding.

`SetTargetActor` stores and returns the original actor key. The controlled-actor
selector resolves on each camera update, so a later leader change retargets the
view. Zero removes the override and restores the track's target. The original
pose/advance instruction order and native floating-point snapshots are covered
by executable audits and runtime regressions. Sampling malformed or out-of-range
track data remains separate from these command and advancement semantics.

`map_camera_track_position` retains all eight evaluated operands. The original
uses only position, range start and range end: it divides track duration by the
range in double precision, multiplies the position offset, bounds that result,
and rounds the final frame once to single precision. This differs from rounding
an interpolation fraction before scaling. Reversed ranges are supported. An
equal start/end maps positions above or below it to the corresponding endpoint;
the equal-position case stores NaN, whose frame query returns the native minimum
signed integer. Mapping preserves pause and completion state. Nonfinite frame
sampling is still rejected by the asset sampler.

`select_camera` addresses eight independent field camera templates, named
`scenario::CameraSelector::Template0` through `Template7`. The original field
initializer builds all eight records with the ordinary player-follow preset;
the retained debug menu's selector 6 therefore uses `Template6`. The ordinary
selector stores its low byte, so numeric 262 also selects template 6 while
retaining its original literal encoding. The runtime accepts byte-selected
indices 0–7. Exact -2/-1 keep their separate script-path/entry-camera meanings
and leave the template index unchanged; every selection returns the old index.
Editing the entry camera still targets that camera after selecting a template.

The initial follow preset and next-field entry camera use bounds of -10000 to
10000 on each axis. `reset_camera_bounds` separately expands them to -100000
through 100000. Original initializer/dispatch bodies and the three selector-6
sites are pinned by the executable audit. Scalar-control regressions check all
eight presets, independent FOV/bounds edits, selector aliases and entry editing
against original-code captures; this does not prove complete render-frame math
parity.

`configure_battle_camera(command, ...)` replaces the former `stage_command`
name at opcode0x93. In battle scripts, `battle::BattleCameraCommand` names
`SetPose`, `MoveTo`, `MoveBy`, `PitchTo`, `PitchBy`, `HeadingTo`, `HeadingBy`,
`MoveInHeading` and `SetFovTenths`. Pose values are XYZ, pitch and heading;
movement uses XYZ followed by speed or high-bit frame count. Pitch/heading
commands use an angle followed by that timing. `MoveInHeading` uses distance,
an offset from the current heading, and timing. `SetFovTenths` consumes only its
first value; the original following timing expression is retained and evaluated
even though it is ignored. Commands0/1 and unrecognized encodings stay numeric.
The original native does nothing outside battle mode and always returns zero;
camera command identities are therefore restricted to battle decompilation.
The runtime accepts full `SetPose` XYZ/pitch/heading values (its final operand
is ignored), and pose replacement cancels active movement. Absolute and relative
pitch/heading commands fix a single-precision increment at command time, allow
retargeting during motion, and step ordinary unwrapped angles. A high-bit count
divides the initial angular distance; an ordinary timing value is speed in
hundredths of a degree per update. Zero ordinary speed leaves the angle in place.
FOV changes immediately using the original single-precision multiply by 0.1;
all trailing operands are ignored by the camera service. These angular states
are checked against snapshots executed through the original DOL dispatch, REL
initializer/service and per-update consumer/helper. Rendered trigonometric
output and relative position movement do not yet have that complete parity
proof; position commands still admit their prepared subset.

`move_actor_relative(actor_id, heading_degrees, distance, speed_or_duration,
facing)` names field actors and `scenario::MovementFacing::{FollowMovement,
KeepHeading}`. A nonzero facing operand locks heading; zero allows the actor to
turn with its movement. The battle branch forwards a separate request record,
so its actor/facing operands do not receive these field identities.

`configure_battle_control(party_slot, mode)` uses
`battle::BattleControlSlot::{Slot1, Slot2, Slot3, Slot4}` and
`battle::BattleControlMode::{Query, Manual, SemiAuto, Auto}`. Slots address the first four formation positions, independently of character IDs. It returns the previous mode;
mode-1 only reads, while other values store their low three bits. An invalid slot
queries slot1 without writing. The original new-game choice text and branches
establish Manual0, SemiAuto1 and Auto2; other stored encodings keep their numbers.

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
Pure expressions can use an empty host table; event dialogue expressions use
the registered event services.

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

## Editing original scenarios

The [verified item and story-flag scope](#verified-item-and-story-flag-scope)
records coverage, the accepted numeric exceptions and the checks used to close
out that work.

Cooking publishes all original field, world, battle and skit programs as `.sym`
source. The selected source is authoritative. Preparation compiles it against the
current cooked `scripts/std/*.sym` definitions before activation; a source error
fails preparation. Active VMs retain their immutable compiled generation.
Within a preparation, dependency discovery and playback share each path's
compiled bytes and validated program, including aliases of the same skit.
New snapshots observe edits; retained snapshots keep their existing programs.
Layout 103 requires a fresh cook to switch existing resource paths to `.sym`.

Matching means reproducing the scenario bytes supplied to the decompiler. Field
cooking first applies the existing gameplay corrections in
[`field_gameplay.rs`](../crates/importer/src/field_gameplay.rs) to fields 0126,
0127, 0139, 0377, 0413 and 0524. Their published sources reproduce those prepared
scripts. Decompiling an unmodified scenario directly preserves its unmodified
bytes; archive compression and packaging are outside the scenario compiler.

The `script scenario;` profile preserves the original VM's memory, integer
arithmetic, evaluation order and native ABI. For example:

```rust
script scenario;
use std::story;
use std::characters;
use std::items;

events {
    entry entry;
    auxiliary auxiliary;
}

pub task entry() {
    if (native::test_event_bit(bit: story::StoryFlag::ManualMovementLearned)) {
        eval native::change_item_count(item_id: items::ItemId::MemoryGem, amount: 1);
    }
    eval native::remove_party_member(character_id: characters::CharacterId::Lloyd);
    end;
}

auxiliary:
data { hex("0000"); }
```

Generated source recovers expressions, assignments, named native arguments,
`if`/`while` regions and named event/subroutine entries. Remaining control flow
uses labels, `goto`, `call`, `return` and `end`. `task` boundaries name physical
entries; they add no instructions, local variables or automatic return. Labels
remain available across entries for shared tails and unusual control flow.
Original names and comments cannot be recovered from bytecode.

The `events` declaration describes entry points without exposing the binary
header, record widths, counts or code offsets. Each `on` line registers an event
kind and key with a task:

```rust
events {
    entry entry;
    auxiliary auxiliary;
    on scenario::EventKind::Spawned(2100) => spawned_2100;
    on scenario::EventKind::Region(2000) => region_2000;
    on scenario::EventKind::Region(2000) => region_2000_r2;
}
```

Import `std::scenario` for the named event kinds. Keys can also use imported
constants, for example `characters::CharacterId::Lloyd` for an actor interaction.
`entry` is required; omit `auxiliary` when the resource has no auxiliary table.
Record order and duplicate keys are preserved: lookup selects the first matching
record. Handler labels use the event kind and decimal key; repeated keys with
different handlers receive a record suffix. The compiler calculates the header
and registry sizes and relocates all declared references when events are added
or removed. An `events` block must precede resource data and replaces `code_base`.

`memory::i8[0x0810]`, `memory::i16[...]` and `memory::i32[...]` retain the exact
shared memory width and address. Indexing one of these references again performs
the original width-scaled indexed load. Known persistent globals use
`globals[story::GlobalId::Story]`; comparisons and assignments use that word's
reviewed value domain, such as `StoryStage`, `ColetteAngelStage` or a scoped
quest-stage enum. Native arguments use the generated enums
for their known domains, including story flags, characters, monsters, items,
fields, music, figurines, techniques, recipes, packed titles, EX skills, skits,
portraits, shared models and motion resources. The native ABI metadata supplies
argument/result domains. Field, world, battle and skit contexts distinguish
overlapping namespaces; scene-local actors and loaded handles keep their local
values. Dialogue grid cells and screen coordinates are not character IDs.

`GlobalId::ManaLeafHerbRoute` names word `0x12c`, which the original debug menu
labels `s_leaf` / “Valley of the medicinal herb.” Its `ManaLeafHerbStage` values
span Ymir's seeds and fruit delivery, Heimdall admission and the elder's staff,
then Latheon's flower explanation, cave approach, herb pickup and Storyteller
history. This is one shared route word, separate from the main story and item
ownership. Ymir's failed fish deliveries can reset the route to
`YmirFruitRouteReady` while keeping the seed flags. Reader-only alternate inputs
and inclusive dialogue limits have explicit input/boundary names; they do not
invent events at those numbers. Earlier Palmacosta scripts reuse the same word
as warp-room state: the card-door entrances write 7, room 0206 copies it to
halfword `0x0818`, and its portal handlers update the layout input 0..8. This
is a room-state selector, not a card-bit mask. Reviewed original loads use
`PalmacostaGlobalId::WarpRoomState`, while the layout inputs stay numeric.
The original word references and all reviewed stage operands are checked
independently of the names.

The remaining zero comparisons retain plain numeric zero. Flanoir 0310's
merchant dispatcher uses the range 0..100, Latheon 0499's Storyteller dispatcher
uses a nonnegative lower bound, and Ice 0324's zero input writes the existing
entry marker before its setup helper. These tests do not identify additional
quest milestones; coverage gaps at those operands should not prompt invented
event names.

`GlobalId::ShadowPactRoute` names word `0xf0`, displayed as `t_shadow` in the
original debug menu. Its `ShadowPactStage` values include the laboratory's Blue
Candle request, Kate's rescue prerequisite and Ozette discussion, the temple's
five-fragment puzzle, the battle retry and pact rewards. Kate's other route
word and the individual fragment flags remain separate. The 3000 laboratory
revisit input has only a retained debug writer; 33001 is an accepted alternate
lower-floor input with no recovered literal writer. Stage 32400 is written
before the second block moves, whereas 32300 follows the first block's motion.
These distinctions preserve the original timing and avoid treating every phase
as a completed puzzle step.

`CelsiusPactRoute` (`0xe8`, original `t_ice`) and `GnomePactRoute` (`0xec`,
`t_earth`) use `CelsiusPactStage` and `GnomePactStage`. Celsius's preparation
phase 1000 can follow either glove-making information or the failed lake-freeze
discussion; it does not establish glove possession. The Ice word is also an
earlier Palmacosta 0206 battle counter: `PalmacostaGlobalId::WarpBattleCount`
names its reviewed resets, increment and 20-count hint check. Counter values
remain numeric. Its increment follows the battle call outside the victory-only
despawn branch, so the name does not assert a victory count. The retained Ice seal-test choice 3000 is named
as a test input. Both pact routes distinguish the first-battle dispatch limit
40000, defeat retry 41000 and completed pact rewards 50000.

Gnome's route includes the gate, spicy-food request, curry, escort interruptions
and joint earthquake attempts. Phase 23300 has a potion available for the choice;
23400 has none. Neither means a potion was already given. Re-entry can reset an
active escort to its ready phase, and the failed joint earthquake has its own
retry state. Curry, potion, Gnomelette and earthquake flags retain their separate
roles. Later route phases record the exit earthquake and five-Gnomelette gate.

`ColetteWingsRequested` (`0x44`) requests Colette's automatic wing accessory.
The original native reader checks actor 2 and a nonzero word before creating
actor 90021. The accessory's update retires it when the word clears or its
Colette owner disappears. Availability, actor capacity and update timing still
affect visibility. Angel progression, flight animation, collision and shadow
settings remain separate. The scripts' zero/nonzero values stay numeric.

`KateRescueRoute` (`0x128`, original `t_kate`) uses `KateRescueStage` for the
execution report, registration, bout, prison escape, escort and Ozette family
discussion. Registration input 2000 is also written by the earlier Seles gossip
scene; it does not prove the rescue plan was heard. The separate Shadow word
still tracks the Blue Candle prerequisite, and the retained Kate 10000 choice is
identified as a debug preset.

`SideEvent` (`0x13c`, original `sub_no`) uses `SideEventState` for the shared
side-scene latch. Its states include noble-party conversations, Kuchinawa's
duel, arena entry modes, Kratos's lessons, searches, study and costume scenes.
`Idle` is a script-state value; writing it does not cancel tasks or clear their
saved quest flags. Encoded counters keep their arithmetic: the banquet starts
at 10102, Palmacosta study adds preparation points and then 100 for the examination,
and Sybak's lost-child inquiries count above 130000. Record 301's first denial
assigns 130000+1 rather than incrementing an existing count; that original reset
is preserved. Reader-only limits and retained inputs have explicit names rather
than invented completion events.

`SeveredManaLinkCount` (`0x100`, original `t_spike`) counts severed mana links.
Volt's aftermath writes 1; completing the Sylph/Gnome and Efreet/Celsius pairs
increments it. The count selects earthquake scenes and dialogue, while Linkite
Tree checks 3 together with the independent Shadow pact flag. Its numeric
counts and increments remain ordinary arithmetic.

`TowerOfManaLightPuzzle0X` through `TowerOfManaLightPuzzle3Y` name the saved
X/Y coordinates of actors 480 through 483 in room 0366. The original debug
labels are `s_lig_0x` through `s_lig_3y`. Spawning, position sampling, beam
geometry and puzzle checks share these words; coordinates retain their values.
The debug menu reuses the first five cells for a saved return field, X, Y, Z
and heading. Their conventional puzzle names describe shared physical storage.

`PalmacostaOrienteeringCheckpointMask` names the word at `0x138` used to reject
previously selected random checkpoints. Its count and current checkpoint remain
separate halfwords at `0x134` and `0x136`. The selector's `count >= 4` and
`checkpoint == 5` exits are preserved, including the retained restart code.

`KatzExpeditionStatusBase` names the indexed status array starting at `0x54`.
Receptionists distinguish idle (0), returned (1), and still away (above 1).
Starting an expedition writes 36000 to its selected cell; collecting a result
or cancelling clears that cell. The array index and separate result bytes at
`0xa4` remain explicit, with no quest-stage interpretation of these numbers.

`SavedCostume0` through `SavedCostume7` (`0x15c` through `0x178`) hold the
eight formation members' previous costume indices during the Altamira beach
and Meltokio banquet scenes. `SavedCostume0` also holds a single character's
previous costume in other scenes. The original costume function returns the
previous byte index and treats -1 as a query. `SavedLloydEquippedTitle` (`0x17c`)
holds the title restored alongside these costume changes. Character selection,
formation bytes, costume indices and title values retain their separate roles.

`set_equipped_title` returns the previous packed `characters::TitleId`, including
when it writes a new title. The original native handler reads the unsigned
16-bit equipped-title field at save-state address
`character_id * 0x118 + 0x1ae` and pushes that value before checking the new argument.
Any negative title argument queries
without writing; the standard `scenario::TitleSelection::Query` names -1.
Nonnegative writes retain the original low 16 bits. The native fixture checks
the complete handler and its return-before-write order.
The character argument directly selects a save record. In particular, 999999
is not resolved as the controlled actor by this handler, so it stays numeric
unless the selected character definitions explicitly name that record index.

Minigame records also have explicit storage names. `SeaKatzHighestClearedLevel`
and `SeaKatzBestTimeBase` hold Red Light/Green Light's cleared-level maximum
and three per-level minimum play times. A zero time accepts the first result.
`UncleGameBestClearedStage` keeps a maximum, including the original high-speed
mode's +5 encoding; `UncleGameCorrectSelections` counts correct selections
separately. `EmotionalBalloonBestRoundsBase` holds maximum round counts for
Non-stop and Endless modes, indexed by mode minus four. The test skit reuses
the Uncle Game stage word for a portrait handle and writes 1 to the Emotional
Balloon base; these words keep their scalar values without a quest-stage domain.
`PalmacostaCafeteriaBestGaldReward` keeps the largest paid Gald reward, while
`PalmacostaOrienteeringBestTimeTicks` keeps the smallest completed field-tick
time, independently of the checkpoint mask and in-progress flag. Names for
retained game code describe its operations without implying normal reachability.

Narrow shared cells use optional `story::ByteOffset` and
`story::HalfwordOffset` definitions. For example,
`memory::i8[story::ByteOffset::SavedFormation0]` reads the saved signed character
byte, while `memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCheckpointCount]`
reads the separate signed checkpoint count. Saved formation and leader bytes,
the Katz result-byte array, a second formation backup, and Heimdall's selected
companion actor have reviewed names. The debug menu also writes a truncated
return-heading copy to `SavedFormation4`; its return warp uses a separate
full-word heading. That original signed-byte store remains unchanged.

These enums name byte addresses, with no implied type or identity for the
values stored there. Width, indexing, arithmetic and overlapping accesses stay
explicit. Only the enum for the actual load width supplies an address name.
The decompiler uses the selected editable definitions; removing an optional
enum or member leaves a numeric address. Arbitrary other enums with the same
numbers do not supply aliases, and a 32-bit access still uses `GlobalId` or its
numeric byte address.

`BattleCameraEyeX`, `BattleCameraEyeY` and `BattleCameraEyeZ` name the common
battle event's initial eye-coordinate words (`0x308`, `0x30c`, `0x310`).
`SetPose` passes their signed coordinate values to the original initializer,
followed by pitch and heading; the coordinate values remain ordinary scalars.

`load_portrait` takes a `skits::PortraitId` and returns an opaque resource-slot
handle. Successful original allocations pack `0xfffe0000 | slot` using one of
64 resource slots. The decompiler names the input portrait ID and keeps handle
comparisons numeric. `TestSkitLloydNeutralHandle` (`0x19c`) holds one such result
for a resource wait and portrait creation; it does not give the stored handle
a portrait-ID value domain.

`find_party_member(character_or_slot)` has two encodings. A nonzero high word
selects one of eight formation cells using the low word; the result is that
cell's signed character byte, or zero for an empty or out-of-range cell. The
original reports an out-of-range slot but still returns zero and continues.
`scenario::FormationSlotSelector` names the observed slot encodings.

With a zero high word, the native returns a one-based formation position. An
exact zero argument returns zero immediately. Other inputs narrow to a byte
before searching all eight cells, including empty cells. Thus 257 searches for
Lloyd, while 256 returns the first empty position, or zero when all cells are
occupied. The retained Salvation 0502 query 320 searches for byte 64; it is not
an existence test for scene actor 320. Its original numeric operand and branch
remain intact. Runtime regressions compare these cases and encoded slot queries
against original native-code results without changing the formation.

`configure_actor_animation` uses `scenario::AnimationSlot` only in a field call
whose resource selector is the constant `AnimationSource::CurrentModel` (-1).
The native handler then reads the actor's model table at that byte offset,
falling back to offset 12 for small offsets or an empty entry. For a loaded
animation resource, it resolves the animation pointer directly; the automatic
selector (0) restores ambient playback. Neither of those branches uses this
operand as a model-table slot. Dynamic selectors therefore retain a numeric
operand until their branch is proven. Battle and skit selectors keep their own
interpretation; in the battle handler zero is a resource selector, not the
automatic-animation control. This distinction prevents a loaded animation's
ignored offset from acquiring a misleading `Idle` or `Default` name.
Slot names describe conventional model-table offsets rather than a universal
motion. For example, the Fire barrier uses offsets 80/84/88 to rise, hold and
lower, while Altamira's transport uses its own motions at 80/84. Model-specific
offsets remain numeric until the relevant model's clip meaning is established;
they are separate from globally named shared animation resources.
The non-battle automatic branch also ignores the flags operand. Its literals
and expressions keep their exact encodings but receive no playback-flag names;
for example, Flanoir 0313's automatic request retains raw 60 rather than naming
its unused `Once` bit. With no execution context, selector zero remains numeric
and flag consumption is unproven. A known battle context still names its
consumed flags, since resource zero takes the battle resource-resolution branch.


`scenario::AnimationFlag::Default` (1) names the original track initializer's
normal playback value. It starts unpaused and looping; bit 0 itself is not a
loop switch. `Paused` (2) stops time advancement, and `Once` (8) clamps at the
end and pauses instead of wrapping. Configuring an explicit field animation ORs its flags
into the initializer's value, whereas the battle bridge writes its signed-byte
flags value directly. `set_actor_animation_flags(..., 0)` instead uses
`scenario::AnimationControl::Resume`: the original field and battle handlers
clear only the pause bit, preserving `Once`. Updating with `Default` replaces
the low flags and restores normal playback. Field updates with `Paused` preserve
the other bits; the battle update replaces the low flags for every nonzero
operand. The native fixture checks both bridges and the shared playback reader.
Other flag bits retain their numeric remainder.

Battle request operands use `battle::FormationId`, named from all 1000 original
`BTLusual.dat` formation rows. For example, `Vidarr` is 4, and
`WhipMasterAndBowmanVariant0007` preserves the Iselia Forest request at 7.
Names describe the enemy groups and multiplicities. Reused compositions receive
`VariantNNNN` suffixes so different original request IDs remain distinct,
including rows with different positions or battle settings. These names do not
assert quest progress or identify weighted encounter pools. Unknown requests
stay numeric. The original-table audit reconstructs every name from the enemy
resource slots and actor entries, and checks both battle-call handlers.

All three battle-entry calls use `battle::BattleDefeatPolicy`: `GameOver` (0)
selects the normal defeat transition, and `ResumeEvent` (1) permits the event to
continue. The original helper only stores bit 0. `ResumeEventEncoded123` retains
the distinct original operand 123 in Gaoracchia Forest; it has the same policy
as 1. Other encodings stay numeric. The long call also names its arena and music
arguments. Positive signed-halfword music overrides use `music::MusicId`;
`battle::BattleMusicSelection::Automatic` (-1) and `AutomaticEncodedZero` (0)
select the loader's automatic battle theme. This -1 is separate from the stop
control accepted by other music calls. Values outside that override range stay
numeric. The original-executable audit checks the argument forwarding, low-bit
defeat handling, zero normalization and signed-halfword music reader.

`battle::BattleOutcome` names the native results `Escaped` (1), `Victory` (2)
and `Defeat` (3). The original wait handler writes a completed battle result to
shared word `0x24`, which dialogue and menus can also reuse. The decompiler only
names comparisons when every incoming control-flow path proves a battle origin.
Copies retain their captured result after the shared word changes. Aliasing
writes, arithmetic, unrelated native/script calls and external task entries
discard the proof; conflicting joins and loops must also pass that check. The
word itself never gains a permanent battle type. All three battle-entry APIs
(`start_enemy_battle`, `start_battle_simple` and `start_battle`) establish this
origin. Their native result expressions use the same domain. Unknown outcomes
remain numeric, and no memory read is folded.
The native fixture audit checks the DOL result handoff and the battle REL's
outcome writers. The runtime's outcome codes reference the same constants.

`trigger_exists` uses `scenario::TriggerKind`: `Touch` (1), `Automatic` (2)
and `Confirmed` (3). The native query reads the type and key of a 52-byte trigger
record, and the trigger constructors write those values. This is a separate
namespace from `EventKind`, whose values select event registries. In particular,
confirmed triggers use record type 3 but dispatch through registry 2. The shared
Rust enum also supplies the runtime's record kinds. The native fixture audit
checks the original opcode entries, record query and constructor stores.

`create_automatic_event_trigger` and `create_script_record` name their line
geometry as `(record_key, x0, y0, z0, x1, y1, z1, height)`.
`create_script_record_variant` keeps its three record metadata operands before
the same geometry. `create_triangle_trigger` names three XYZ vertices followed
by height. The original contact code reads these coordinates as signed
halfwords; height extends the region vertically rather than defining a
horizontal radius. Full source operands and their widths remain intact, so
matching recompilation preserves the original narrowing.
Removal calls name their full-width `record_key` and clear all matching records
of their own trigger kind. Keys, coordinates and heights remain scalar values,
even when their numbers overlap known story flags, items or actors. The native
audit checks constructor stack slots, record stores, contact geometry and each
remover's kind/key comparison.

`heal_party(mode)` uses `scenario::PartyRecoveryMode`, including percentage HP
and TP restoration, HP damage, TP drain, full `Rest`, and
`ReviveIncapacitated`. Percentages apply to each member's maximum value. HP
damage leaves living members at one HP and does not revive knocked-out members.
The minimal revival clears incapacitation/petrification and sets affected HP to
one; it does not perform a full rest. Mode 1 and values outside 0..23 have no
action and remain numeric in source.
`add_gald(amount)` keeps its signed scalar amount and returns the new balance.
The original handler wraps 32-bit addition before clamping to 0..99999999. Only
negative requests increment the spent-gald counter, and that counter also wraps.
The runtime and native fixture audit preserve these arithmetic details.

`configure_session(command, value)` names the observed `SessionCommand` values:
rumble, skit notifications, stereo, leader locking, menu disabling, clear-count
query/recording, title/game-over/credits transitions, and New Game Plus setup.
For toggle commands, `SessionToggle::Query` (-1) reads without writing; other
values write their low bit and return the previous state. Known 0/1 values use
`Disabled`/`Enabled`; other encodings stay numeric.
Clear-count and transition commands ignore the value operand, which remains
numeric or retains its original expression and any evaluation effects.
`RecordGameClear` returns the previous count, increments it up to 100, and
refunds previous Grade Shop benefits on every invocation. The original handler
has no repeat-call guard. These behaviors are checked against its dispatch table,
saved-state fields and return path, alongside the original settings dialogue.

`configure_actor_bone_translation(actor_id, controller_slot, bone_index, x, y,
z, duration)` distinguishes the model's controller slot from its model-local
bone index. Observed slots use `BoneControllerSlot::Slot0` through `Slot7`;
bone indices remain numeric or retain their lookup expression. A bone index of
-1 skips the whole adjustment, leaving its operands numeric and preserving
expression evaluation. The native helper stores the XYZ target and divides its
change from the current translation by the supplied interpolation step count;
zero selects one step. Source retains the original zero.

Field actor lookups in this command and `wait_actor_animation_frame(actor_id,
frame)` name controlled-player and scenery aliases through `ActorSelector`.
Battle requests keep those keys literal. Frame waits compare integer authored
frames exactly, including paused frames. The controlled alias is resolved on
each field poll, so it follows a player change while waiting. A missing field
actor leaves the wait pending; a missing battle actor releases it. The native
fixture audit checks both branches, the slot/bone stores and the wait consumer.

`configure_actor_bone_rotation` names its six operands as actor, controller slot,
bone index, `rotation_x`, `rotation_z`, and `rotation_y`. The helper receives
`[rotation_x, rotation_y, rotation_z]` and uses a fixed 30-step interpolation.
`configure_actor_attachment` sets the same rotation controller
with an explicit `duration` operand; its former `scale` label described that
operand incorrectly. Zero duration selects one step.
`configure_actor_bone_scale` uses the same controller slots and model-local bone
indices, with XYZ percentages and an explicit interpolation count. Its scale
components are divided by 100. All three commands skip a bone index of -1,
resolve field actor aliases, and preserve literal battle request keys. The
native audit checks their axis order, constants, helpers and host branches.

`DialogueFlag::Yellow` selects the original fixed yellow frame base color
`E8C800FF`; the Green and Red choices take precedence, and theme shading still
applies. `DialogueFlag::TimedClose` initializes a 300-count close timer. At
message end, the original parser consumes that timer only after its persistent,
automatic-page and active-choice branches; reaching zero requests closing.
These names come from the original initializer, parser and window renderer,
whose instructions and color data are checked by the native fixture audit.

`effects::EffectId` is the field-service sound-cue namespace. Its
`AcquisitionFanfare` (36) accompanies item and technique awards, with original
reuses for exchanges and the Eternal Swordsman title. `LearningFanfare` (37)
accompanies title awards, learned recipes and the Tethe'alla Base pass-code
notices. These describe the sounds; playing one does not grant a reward or
assert quest progress. The original calls retain their exact chosen cue even
when a notice reuses the less usual fanfare. Cue36 maps to macro459/sample196,
and cue37 to macro711/sample328 in the resident SND bank; these program and
sample IDs are separate namespaces. The native fixture checks the original
sound handlers and bank bindings. The editable audio targets remain `cue-0036`
and `cue-0037`, independent of the names used in script code.

`DoorKnock` (156) accompanies the visitor introductions in Flanoir 0317 and
Heimdall 0285. All 28 original requests belong to these overnight conversations;
the script retains the spacing between repeated knocks and handles the visitor
separately. `ElectricalDischarge` (187) accompanies Lightning Temple 0475..0478's
rod activation, shock scene and alternating screen flashes. `StoneFloorCollapse`
(210) accompanies Earth Temple 0236's earthquake-triggered collapses and room
0237's Gnomelette-3 collapse. Playing the cue does not move a floor carrier or
perform the fall. The 18 discharge and 26 collapse requests retain their original
public cue IDs. The resident bank selects program137/sample89,
program772/sample367 and program76/sample45 respectively; the original bank
records and complete programs are checked independently of the script names.
Their editable audio targets remain `cue-0156`, `cue-0187` and `cue-0210`.

The same sound namespace names `ElectronicBeep` (189), `MechanicalSlide` (239)
and `ElevatorDepartureBeep` (306). The first is reused around consoles and
machinery; it uses macro351/sample130. `RingRadar` (263) shares that sample but
uses macro574 with different processing, so the two cues remain distinct.
`MechanicalSlide` accompanies doors and moving panels with macro33/sample16;
its name does not assert that a movement has finished. Altamira 0024–0027 play
`ElevatorDepartureBeep` three times, with waits of 15 ticks between calls, before
transferring to the selected floor. Its macro105 uses sample263, which is a separate namespace
from sound cue263. The bank audit verifies all these bindings, and their existing
`cue-0189`, `cue-0239` and `cue-0306` audio targets remain editable.

`TeleportPadActivation` (366) accompanies nearby teleport pads creating their
`RisingOrbs` emitter. Its 47 uses describe activating the pad's effect, before
the departure sequence. `TeleportDissolve` (367) accompanies the departure
fade/stretch in those helpers and related teleports; one of its 51 uses also
accompanies Derris-Kharlan 0135's puzzle blocks rising and shrinking away.
These cues do not themselves move an actor or confirm a field handoff.
The resident bank binds cue366 to layer `0x8021` with two voices and cue367 to
layer `0x8022` with six voices. The latter retains both entries for program275,
which have different note/velocity controls, followed by programs276..279.
Program/sample/layer numbers remain separate from public cue IDs. The native
bank audit checks every voice and complete program; the editable targets stay
`cue-0366` and `cue-0367`.

`JumpTakeoff` (282) and `LandingImpact` (283) name the sounds commonly paired
with scripted jumps and their landing points. The 47 and 51 original requests
are independent: the decompiler does not insert a missing counterpart or move
either cue to a different animation frame. Some animations reuse these sounds;
Ozette 0425 plays cue282 during Colette's stumble, and cinematic impacts in
Flying Dragon 0137 and Gaoracchia 0517 also request cue283. These remain the
original chosen sound, without implying a jump, collision query or physics update.
The resident bank binds cue282 to two voices in layer `0x8084` and cue283 to
four voices in layer `0x8085`. Cue283 retains both program462 entries with their
different velocities. Programs460/461 in the takeoff layer are separate from
public animal-call cues460/461; the bank audit pins complete programs and every
layer voice. Editable audio targets stay `cue-0282` and `cue-0283`.

`YmirFishMovement` (175) is requested by the fish-route helpers in Ymir Forest
0519..0521. The 21 original calls include small-fish actors and movement carriers
3601/3900, when the fish follows or is attached to them. The local model bindings
and geometry identify the small fish separately from the larger territorial
fish. The cue selects resident program825/sample406 with pitch offset -2, and
keeps editable target `cue-0175`. Requesting this sound does not spawn a fish,
plant a seed or complete a fruit-delivery step; an absent actor also keeps the
native ambient-sound no-op. Original bank bindings and the reviewed call operands
are checked independently of the descriptive name.

Ymir's animal-call cues also have separate names: `YmirBoarCall` (456),
`YmirFishCall` (457), `YmirBirdCall` (460) and `YmirDefaultAnimalCall` (461).
The first three accompany the pink-, blue- and white-flower branches; the
default cue is also used in the introduction and before the fish/bird progress
thresholds. `YmirBoarMovement` (458) attaches ambient sound to the charging boar,
and `YmirBoarImpact` (459) accompanies its scripted impact, alongside cue 220.
These are sound requests; the following helpers still perform their own seed,
progress and route checks. All six cues belong to original event bank
`se_ev03.snd`, with distinct programs 663, 666, 662, 661, 668 and 667 respectively.
Cues 456/458 share sample 308 and cues 457/461 share sample 309, but their
programs differ. The public cue IDs and editable `cue-0456`..`cue-0461` targets
remain separate. The original bank records, complete programs and all 67 script
operands are audited.

`BodyImpact` (71) accompanies physical hits, falls and staged collisions,
including comic reprimands and combat cinematics. All 57 requests retain their
animation, attachment, timing and any separate damage operations. Resident
program144/sample94 keeps the existing `cue-0071` binding; sample94 is not public
cue94, which remains its separately named silent placeholder.
`WeaponReady` (195) accompanies weapon drawing/readying beside prop visibility,
attachments and combat animations. Its 50 uses also include attack/effect staging
with fireball/model-trail emitters and the Vidarr rescue cinematic. This names
the reused cue rather than asserting a literal weapon draw at every call.
Resident program759/sample356 and the complete key-off tail retain `cue-0195`.
Neither sound request equips a weapon, changes hit points or runs the separately
scripted movement, animation or effect command.

`SuccessFanfare` (80) accompanies party-join notices, rewards and recipe notices,
and is also retained in Gaoracchia's plant-clearing callbacks. The 30 original
requests do not add party members, grant inventory or teach recipes by themselves.
The forest callbacks retain numeric animation index80 independently of the sound.
Resident layer `0x803c` keeps eleven voices and all program controls; its seven
sample171 voices, three sample53 voices and sample156 voice stay in their
original order. The audio binding remains `cue-0080`.

`MachineryMotionVariant0248` (248) accompanies lift arrivals and transport
mechanisms, with retained cinematic reuse in Salvation. All 29 requests keep
their original spatial hosts, bindings, movement, fade and stop commands. Resident
program516/sample221 has pitch offset +6 and its own envelope/tail; it remains
separate from cues251/252's program518 despite the shared sample. Its target stays
`cue-0248`, and the name does not assert that every request starts or stops motion.

`AlarmSignal` (299) appears in ranch/base warning scenes, lock notices and
Salvation's gate controls. The 27 requests include timed repetitions and separate
conditional last-lock paths; they are not combined or moved. Camera flashes,
lock updates and enemy/cinematic actions remain separate commands. Native layer
`0x8053` retains six distinct programs6..11 using sample5, including their
individual pitches and timing. The audio binding remains `cue-0299`.

`MechanismSignal` (246) is requested around switches, moving mechanisms and
puzzle reactions, and is also reused in cinematic staging. All 48 calls retain
their original timing and volume, including the five spaced bridge requests,
Sylvarant's orb effects and Kate's prison escape. The name does not assert a
literal switch operation or puzzle success at every request. Resident
program597/sample263 keeps its full program and `cue-0246` target; it differs
from `ElevatorDepartureBeep`'s program105 despite the shared sample.

`WonderChefPoof` (373) accompanies the Wonder Chef's smoke-covered arrivals and
departures in 19 fields, including Meltokio's cooking scene. All 38 requests
immediately precede a separate `RisingSmoke` emitter. Visibility, actor replacement,
recipe teaching and ingredient rewards remain separate commands. Its native
layer `0x8025` retains five voices in order: programs289/291/290/292/293, with four
using sample343 and one using sample137. The complete layer controls and all five
programs are pinned; the editable audio target remains `cue-0373`.

`MachineryMotion` (251) accompanies lifts, moving platforms, conveyors and
other mechanisms; `MachineryMotionVariant0252` (252) is reused for Altamira
transport and Tethe'alla Base block-pushing sequences. The 75 and 31 original
uses retain their chosen public cue IDs. Both resident cue records select
program518/sample221 with pitch offset -5, but each has its own sound group and
editable target (`cue-0251` or `cue-0252`). Their names describe cue usage; sound
playback does not start movement or signal that it has completed. Native bank
records, the full program and every reviewed operand are checked against the
original data. Program/sample IDs and the two public cues remain distinct.

`RingModeChange` (320) accompanies the ring stations' changed-mode and
returned-to-normal notices: all 50 uses in 22 handlers across 21 fields have
the corresponding notice and later ring-mode write. Playing the cue itself
does not change the ring. Its resident-bank binding is layer `0x80a1`, with
five voices using programs 740, 360, 357, 358 and 359. They use samples 346 and
202, with their original pan, velocity and pitch processing. The cue remains
distinct from the chest-opening sample 320, and keeps audio target `cue-0320`.

`MachineryDrive` (405) accompanies Asgard Ranch 0212's animated production
machinery, Toize Mine 0412's lift route and Salvation 0503's moving mechanisms.
All 42 uses retain their spatial hosts or playback slots and separate animation,
movement and stop commands. Resident program538/sample220 differs from the
program518/sample221 used by cues251/252. Its complete program, sustained
playback and key-off tail retain the existing editable `cue-0405` target.
This cue does not move a mechanism, choose a lift route or complete a puzzle.

`LowRumble` (181) accompanies moving scenery, tremors and cinematic effects.
Its 68 original requests include 10 temporary actor-ambient bindings for Martel,
Ice Temple and Thoda mechanisms. The other 58 include retained Earth Temple
routines and Heimdall's conditional collapse ambience. Playback does not perform
the separately scripted movement, camera shake or controller rumble. Resident
program 639/sample 295 keeps its original pitch offset, wait and release tail;
the editable target remains `cue-0181`.

The ambient cues also keep separate, editable names. `FireAmbience` (384) is
attached to Balacruf's fire emitters, Thoda's torches and Niflheim's flames, and
used at hearths, burning Iselia and a Salvation effect. `SurfAmbience` (385)
accompanies shores and Izoold's boat departure, with a retained reuse during
Kuchinawa's Mizuho scene. `FlowingWaterAmbience` (386) accompanies water-side
locations, including the separate water-plane sources in Altamira and Latheon.
These names describe cue usage; they do not create the corresponding effects.

All 150 original operands are reviewed across all three native consumers:
68 actor-ambient bindings, 81 extended sound requests and one simple request.
Fire keeps program 62/sample 36 and its volume loop. Surf keeps layer `0x80A7`'s
four programs (834/831/833/832), their separate envelopes and sample 411.
Flowing water keeps layer `0x8038`'s two voices (480/481), sample 201, opposing
pan values and the second voice's delayed start. Full original program and
layer bytes are checked. The existing targets `cue-0384`..`cue-0386`, actor
positions, radii, volumes, explicit sound slots and stop requests stay intact.

`WaterfallAmbience` (394) remains distinct from flowing water and surf. Toize's
two sources occupy the same vertical cascade at different heights; Latheon's
stacked sources accompany its falls. Selected original surface geometry and
texture regions establish those placements. The cue also retains its forest,
hot-spring, boat and Mizuho uses without asserting a waterfall at every call.
All 32 operands are reviewed: 25 ambient bindings and seven extended requests.
Resident program 768/sample 364 keeps its vibrato, sustained wait and key-off
tail, with editable target `cue-0394`. Binding a cue does not spawn water or
change the field geometry.

`MagicalFade` (275) accompanies scripted appearances and disappearances. Of its
65 original requests in 16 fields, 40 adjoin spawned opacity/scale helpers and
22 adjoin inline opacity changes. The other three accompany Kratos's immediate
despawn, the Island Ranch helper's hide/relocate/restore sequence and a screen
dissolve that retains its extended sound volume of 90. The name describes
cue usage; playback itself does not perform those separate operations.
Its resident layer `0x80A4` has three voices: programs 811/812 use sample 236,
and program 813 uses sample 240. Their full controls and processing remain
distinct. Public cue 275 is separate from program 275 inside `TeleportDissolve`
(367), and its existing editable audio target stays `cue-0275`.

`resources::BuiltinFieldModelId::Sphere` names the special model key `24`. The
original resolver selects a fixed built-in descriptor, containing the `nc.gpl`
model and `Sphere01` node, before consulting any field bank. Ordinary field
spawns and scene/interaction constructors initially hide it and disable shadow,
collision and grounding. The name identifies the selected model; later property
edits remain separate. This selector has no external asset path.
The optional editable definition supplies its name; removing it keeps numeric
operands. Only key `24` takes this built-in path. Other small model keys retain
their field-bank context, and an explicit per-field model name takes precedence.
The existing model-operand guards still exclude ordinary emote operands,
ignored controlled-player spawns, battle requests and unknown contexts.

Model constructors also support explicit scene-local names in editable
`resources::FieldNNNNModelId` enums. A known `fields/...-NNNN` input path selects
that scene's declared bindings. Ymir 0519 names its `SmallFish` handle; 0520 and
0521 name both `SmallFish` and `LargeFish`. Their native handles remain
-1179641/-1179640 (`0xFFEE0007`/`0xFFEE0008`), selecting original MAP sections
23/24. The original fish packages are checked across the three fields, and their
geometry identifies the small and large fish separately.

The same scene enums name the reviewed flower, boulder, boar and bird models.
`PinkFlower` describes the flowers present before seed planting; `BlueFlower`
and the white-flower variants describe the rendered plants without asserting
seed inventory or quest completion. `WhiteFlowerPair` and `WhiteFlowerCluster`
use two and four copies of the white-flower geometry respectively. `Boulder`
selects local package 7 in 0520 and package 10 in 0521; keep those scene bindings
distinct even though both display a boulder. The model identities come from the
selected texture regions, geometry, original archive packages and every original
constructor operand; the existing mesh paths remain editable.

These names apply only to consumed model operands in the matching field. Equal
coordinates, animation resources, emote target arguments and ignored spawn
branches keep their own interpretations. Unknown and other scene contexts keep
local handles numeric. A mod can add or rename the explicit enum in its selected
`std::resources` source; decompilation uses those declarations without an embedded
name fallback. These model names do not rename the underlying mesh paths or actor
keys, and they do not assign universal actions to model-specific motion slots.

`create_ring_station(actor_id, x, y, z, unused_4, resource_id)` defines a new
scene actor key and binds a model. All 22 original calls use
`resources::FieldModelId::SorcerersRingStation` (363), whose published model path
remains `props/model-363`. Coordinates and the new actor key remain scalars;
the constructor does not resolve an existing actor or the controlled-player
alias. Field source uses the shared model catalogue for the last argument.
The fifth argument remains numeric: the original constructor reads the other
five operands but skips it, even though the original values resemble ring
modes. Its expression is still evaluated and discarded. The executable audit
checks the skipped stack slot, model resolver and installed station callback.

`spawn_actor` also has distinct field and battle branches. In battle mode9 it
forwards an actor/model request, position and heading to the battle manager;
the final movement-kind and speed operands are ignored. The 18 retained battle
calls keep their original numeric zeros for these operands. They do not acquire
field `ActorBehavior` names, and runtime battle spawns do not install field
autonomy even when a field camera is present. Negative IDs in this branch do
not select the field emote protocol. Without a known host, these mode-dependent
operands remain numeric. In the field branch, actor key 999999 skips creation;
it does not select an existing player to spawn. Its operands remain numeric,
including expressions whose evaluation still has effects.
`spawn_collision_actor` and `spawn_scenery_actor` instead define scene actor
keys for every signed value. Negative keys do not select emotes, and 999999
does not select the player. Their model and behavior operands keep their own
domains even when the new actor key is computed dynamically. They do not use
the ordinary spawn command's field no-op or battle-manager branch.

`create_scene_actor` and `spawn_interaction_actor` use the same explicit
`(actor_id, x, y, z, heading, resource, behavior, speed)` argument layout.
Both allocate scene actors, store the literal key, resolve the sixth operand
as a model, and pass their behavior byte and speed to the NPC callback. They
do not select emotes or existing party members from the key. Field source names
shared model IDs and known behaviors independently of that key; local resource
handles, small bank-dependent resources and unknown behaviors remain numeric.
The original executable audit checks both handlers and their argument storage.

`ActorBehavior::NoDecision13` and `NoDecision90` retain the two observed
encodings that fall through the original actor decision selector. With no
explicit actor-state override, the selector dispatches only byte values 0..12;
13 and 90 return without selecting a new autonomous state. They keep their
separate numeric identities and do not mean `Stationary`. An explicit actor-state
override still takes priority, and neither encoding cancels scripted movement.
The original input 300 remains numeric: its stored behavior byte is 44, and
replacing the source operand with that byte would change the compiled script.

`create_particle(sprite, lifetime, x, y, z, velocity_x, velocity_y, velocity_z,
size, alpha, fade, palette, variant)` names all thirteen original operands.
It shares `scenario::ParticleRecipe` with `create_effect_object`, whose separate
`direction_x/y/z` and `speed_percent` operands normalize direction before moving.
The directed constructor divides its signed speed by 100 as an integer before
converting to float; the direct-velocity constructor has no speed operand.
Neither constructor uses actor emitter recipes, model IDs or sound cues.
The shared factory selects the named sprite descriptors, including glow, smoke,
falling/rising leaves, refraction, rings and bound texture slots 0/7.

Lifetime stores a halfword; alpha stores a byte and also initializes a shifted
halfword fade accumulator. Size sets width/height using the descriptor's aspect
ratio sentinel when present. Fade initializes a shifted signed halfword; zero
clears the explicit fade bit so the default tail fade applies. The last two
operands select the palette and a recipe-specific parameter, respectively; the
parameter can control spin or select a bound texture's update callback. They
remain numeric rather than acquiring unrelated color or rotation identities.
The original inputs 501..505 also remain numeric: they are outside the factory's
0..96 switch and do not establish additional named recipes. Their expressions
and all narrowing, negative values and encoded widths survive recompilation.
The executable audit pins both constructors' object writes and shared dispatch.

`find_actor_node(actor_id, name_index)` reads a name from the current script's
auxiliary string table and searches the actor's active model. Its second operand
is a script-local name index, independent of animation resource slots or global
body-part IDs. `find_actor_body_part(actor_id, body_part)` instead uses the
seventeen fixed `BodyPartId` names, whose original string table is audited.
Both return a model-local bone index, or -1 when no node is selected or found.
Field actor zero selects no actor; battle zero is a literal request key.
Field controlled/scenery selectors receive names, while battle controlled keys
remain literal and query that battle actor rather than the field player.

The retained body-part selector56 in Dirk's House selects no name and returns -1;
the following timed rotation therefore skips its bone adjustment. It stays
numeric in recovered source. Runtime also returns -1 for unrecognized selectors
without searching a manufactured placeholder name. This prevents a mod's model
node named `<body part 56>` from changing that original no-op. The five original
controlled-player body-part queries in Ossa Trail and Triet Ruins now resolve the
current field player, just like the named-node query. The executable audit pins
both host branches, the field aliases and every fixed body-part name.

`read_actor_attachment(actor_id, bone_index)` writes the selected bone origin's
world coordinates to coordinate registers 0, 1 and 2, truncating toward zero.
Field calls name verified character and scenery actor selectors; `bone_index`
remains a model-local index, including when calculated by `find_actor_node`.
Bone -1 leaves the coordinate registers unchanged; a missing actor writes zero.
The original battle branch ignores `actor_id` and uses `bone_index` as both the
battle actor request key and the bone index on that actor. Battle and unspecified
contexts keep the first operand numeric. Expressions are still evaluated, even
when lookup ignores their result. The executable audit pins this host distinction,
the field aliases, the origin vector and the coordinate-register writes.

`configure_scenery_animation(layer, channel, resource_id, start_frame, rate, flags)`
names scenery layers, animation channels, shared field animation resources and
the ordinary `AnimationFlag` bits. `SceneryAnimationChannel::Base` (-1) selects
the base track; `Slot0` through `Slot3` address the separate channel arrays.
Local resource handles remain numeric. Frame and percentage-rate operands retain
their original units and encoding, including negative rates and paused playback.
The constructor replaces the initialized flags with its complete flags operand.

Scenery animation edits have a different layer selector. Starting an animation
accepts numbered layers 2/3/4 and actor-key aliases 999997/999998/999980. Clearing,
seeking and changing rate recognize only numbered layers 2/3; all other values
select the main bank. Thus an edit using 999980 targets the main bank even though
starting with that key targets the fourth bank. These edit operands stay numeric
to avoid implying the constructor's alias routing. The runtime preserves that
original difference, and the executable audit checks all four handler branches.

Actor properties `EmitterParameter0` through `EmitterParameter9` (113..122)
address the ten parameter slots passed to `create_effect_emitter`. Their effects
depend on the constructor's recipe; the same slot can control different things
in different recipes. The original handlers only access these slots for emitter
actors. Slots 0..3 are signed halfwords. Slots 4..9 are stored as floats, but
scalar property writes first truncate to unsigned 16 bits; their constructor
arguments remain signed. Naming the selector preserves these original value
conversions and does not impose a shared palette, position or size meaning.
The native audit verifies the original opcode registry, switch tables, callback
checks and constructor/property load and store instructions.

The constructor's `recipe` operand uses `scenario::EmitterRecipe`, including
`Fire`, `Rain`, `Smoke`, `Portal`, `Column` and the separate `ExpandingColumn`.
These names describe the emitter behaviors implemented by the renderer; they
are independent of cooking recipes, sound cues and the billboard sprite recipes
born by an emitter. The native constructor stores this selector at actor+0x84c,
which the original renderer dispatches through its recipe table. The fixture
audit checks that storage and dispatch, including the original table bytes.
Only observed recipe values have reviewed names. Unknown or mod-added values
stay numeric; model IDs, coordinates and parameter values keep their own roles.

For `change_field` destinations of 3000 or greater, the X operand is instead a
`locations::WorldLocationId` landmark; the same applies to `play_world_cinematic`'s
`x_or_landmark` when its following destination enters the world service. Normal
field coordinates and operands with an unknown dynamic destination stay numeric.
The two observed destination encodings use `scenario::WorldTransferId::Standard`
(3000) and `Alternate` (3001). The original handlers 800500D4 and 8004FE90 both
test `>=3000` and take the world-service branch for either encoding. These names
preserve the separate literals without claiming that they select different
worlds or that a further original distinction has been established. Ordinary
field destinations still use `fields::FieldId`; unregistered world-transfer
encodings remain numeric in their own domain.
Field resource loads use `resources::FieldOverlayId` for the shared radial-burst
and ornate-ring images, separately from models and motion banks. The same
catalogue supplies their cooked media paths, scene bindings and texture-bank
names. Equal values in animation-source operands or ordinary scalars do not
automatically gain an overlay name.

`create_overlay` uses `scenario::OverlayActorId::LocationCaption` (999989) for
the special creation key that selects the native location-caption renderer.
The original constructor resolves the image resource independently, then tests
this full-width key before installing renderer `8007E1BC`. The 32 observed
requests in 28 fields keep their separate image resources, center coordinates,
automatic sizes and caption hold expressions. This optional name applies only
to the constructor's first operand in field context; other actor references and
equal scalar values stay numeric. Selected definitions can rename the member;
omitting it or moving its value restores numeric output with matching bytes.

`play_sound_simple(sound_id, pan_offset)` uses the current field sound level.
`play_sound(sound_id, pan_offset, volume, binding_index)` exposes the extended
call's actual controls. The native handler converts pan with signed integer
division toward zero: `pan_offset / 2 + 64`. If that result falls outside 0..127,
the handler resets it to 64.
Volume 255 selects the field sound level. Binding index 255 plays without a
persistent slot; the original replay routine visits cached bindings 0..4.
These names preserve the original integer encodings and evaluation order.
Readable source uses `scenario::SoundVolume::FieldDefault` for that volume and
`scenario::SoundBinding::Unbound` for that binding. Slot-volume updates use the
same volume name only with operation `Volume`. Bound cue -1 stops a slot without
applying volume, so that ignored operand stays numeric; unknown cue/binding
combinations also stay numeric when the consuming branch is unproven. Pan,
ordinary volume levels, cached binding indices, and `configure_sound`'s binding
255 stay numeric. The two new domains are optional in selected definitions:
renamed members at 255 are respected, and missing or moved members leave the
original numbers intact. Spatial audio retains its existing
`scenario::SpatialSoundVolume::Default` spelling.
The native sound controller keeps the low seven bits of the volume byte.
Consequently the original explicit volumes `200` and `-80` reach controller 7
as 72 and 48. They remain `200` and `-80` in source so recompilation preserves
the original operands. Spatial sound attenuates the byte before this conversion;
it also selects ambient emitters using the attenuated byte. The host applies the
same conversion when emitting sound commands. Its current field-default volume
is still 127; recovering the saved field level is separate from this conversion.

Older named source remains accepted: `raw_velocity` aliases `pan_offset`,
`channel` aliases `volume`, and `note` aliases `binding_index` in these sound
signatures. The registry's optional `argument_aliases` metadata declares these
per-argument spellings. Aliases do not reorder operands or apply to other native
signatures. Decompilation and the original VM registry expose the canonical names.

`play_sound` uses `scenario::SoundControl::StopSlot` for sound ID -1 only when
its `binding_index` operand is known and differs from the default 255. The original
handler then clears that slot's stored sound parameters and stops its handle.
The same number with the default binding index, an unknown dynamic binding index, or
`play_sound_simple` stays numeric; music's -1 remains its separate stop command.
`set_actor_ambient_sound(actor_id, sound_id, volume, radius)` retains an emitter
on the actor. `play_actor_sound` uses the same argument roles for one spatial
request. Both cue operands use `effects::EffectId`, and the controlled-actor
alias resolves to the current player. The ambient call's full-width cue -1 is
`scenario::AmbientSoundControl::Disable`: it clears the emitter-enable bit
without issuing a slot stop. The one-shot call's -1 remains numeric and does
not acquire that emitter control meaning. A literal 65535 also remains numeric;
in the ambient call it takes the enable branch before narrowing the stored cue.

`scenario::SpatialSoundVolume::Default` (255) selects the original current field
sound-level byte when consumed by either spatial-audio path. Ambient disabling
does not update volume or radius, so even volume 255 stays numeric in that
branch. Radius and ordinary volume values remain scalars. Both actor lookups
return no actor in battle mode9; recovered battle source preserves their
arguments numerically, and runtime battle events ignore these field requests.
The executable audit checks the handlers, controlled-actor lookup, storage
widths, default-volume readers and common field-cue service calls.

`configure_sound` operations use `scenario::SoundSlotOperation::Volume` (100),
`Pan` (101) and `Stop` (102). Slot numbers and values remain scalars. The four
retained operation-255 calls stay numeric and take the original handler's
unrecognized-operation path; the decompiler does not repair them into stops.
Unknown identities remain numeric, allowing mods to introduce IDs. Missing
definitions for an identified domain fail decompilation instead of silently
discarding its names. The compiler resolves symbolic values from the selected `.sym`
definitions; it does not substitute an embedded standard library at runtime.

Story flags and treasure save bits are separate namespaces, even when their
numeric values coincide. `std::treasure::TreasureFlag` names the original room and
reward, including pickups implemented directly by script rather than the chest
service. Gald rewards use `TreasureRewardEncoding::GaldBase + amount`; item rewards
use `ItemId`. Quest globals recovered from the retail debug menu use the menu's
quest scopes. Counters such as Luin donations do not acquire unrelated quest-step
names merely because their values happen to coincide.
Dungeon puzzle flags identify the original room and object where useful, such as
`GaoracchiaForest0513Barrier710Cleared`. Separate mechanisms keep separate names,
including targets destroyed together by one bomb and the two sockets for a stone
block. These are recovered roles rather than original source identifiers.
Latheon's flower flags distinguish a blowing flower from temporary bubble-flight
segments. Shared original writes remain explicit: room 0496's feeding callback
uses a room 0495 flower flag, and Lightning's Battle Cloak lowering sequence also
writes the Duel Helm lowering flag. Names and evidence notes preserve these
couplings. A chest's lowering flag remains separate from its opened-treasure bit.
Names also distinguish pending scenes from completed conversations. For example,
`BalacrufEarthquakeConversationPending` is consumed by a later room, while
`TrietExitReminderSuppressed` is cleared when the oracle explains the ruins.
Escort-arrival flags are cleared at their destinations. A flag set next to an
item or Gald reward does not imply that the original script consumes an item;
the Celsius' Tear merchant grants his reward without removing the tear.
Colette's angel nights, loss/restoration of her soul and crystal-disease stages
share a dedicated enum for global word 3. The original assignment to 201 inside
a condition in field 240 remains an assignment, including its original effect.

Reviewed flag meanings and loot identities have
[original source references](../crates/symphonia-script-tools/data/scenario-identity-evidence.tsv),
including script path, native parameter and original word PC. The evidence test
checks both generated symbol values and the referenced original instructions.
Each reference's `note_id` selects its reviewed explanation in
[`scenario-identity-notes.tsv`](../crates/symphonia-script-tools/data/scenario-identity-notes.tsv),
so repeated uses share the same note. Large instruction snapshots live in
[`original-words.tsv`](../crates/symphonia-script-tools/tests/fixtures/original-words.tsv);
the tests retain the source-image hashes and semantic checks.

```sh
cargo test -p symphonia-script-tools --test identity_evidence
cargo test -p symphonia-script-tools --test identity_evidence -- --ignored
```

The second command requires `local/all-assets` with the original `.ssb` corpus.
These checks protect numeric associations; the notes record the dialogue or
control-flow evidence used to establish each meaning. Reused scene scratch flags
and unverified puzzle states must be interpreted in their own scene; they do not
receive guessed global story names.

Reviewed scene meanings use `story::FieldNNNNFlag` enums in the same editable
`std/story.sym` definitions. For example, Asgard 0033/0034 use
`StairTransferPending` for bit 150, while Martel Temple 0309 uses
`OracleConversationAffinityBonusClaimed` for that same bit. The latter gates one
shared +60 bonus across three conversations; it does not stop their dialogue
from repeating. These aliases compile to the original shared session bit and
do not create independent storage or introduce a reset on field entry.

Cooking and the decompile command obtain the field number from the standard
`fields/<name>-NNNN/` input directory. API callers can pass `Context::FieldScene`.
Only an explicitly declared variant in the matching field enum is selected;
unknown fields, missing enums and unreviewed values keep the ordinary global
flag name or numeric operand. Changing a cooked alias changes the generated
source name without changing its encoded value. A field enum cannot establish
the meaning of a dynamic offset or of an equal-valued item ID.

`story::WorldFlag` provides the same opt-in aliasing for overworld scripts. Bit
1749, for example, is `TrietEntranceArrivalPending` in the world entry event;
Triet consumes it while checking the early Raine title scene. The later Derris
fields use this same bit as `RouteVia0129Selected`: when set, room 0530's north
portal leads to Salvation 0502 and its south portal to Derris 0129. When clear,
those portals lead to Derris 0130 and Salvation 0502 respectively. Both layouts
have usable portals. These world and field aliases preserve the shared bit and
require matching declarations in the supplied definitions.

The shared building interiors also retain an exterior selector in bits 192
through 199. Entry in each of exteriors 0081/0082/0083/0084/0089/0091/0092/0093
sets its own bit and clears the other seven. Their field aliases use names such
as `ExteriorHouseOfSalvation0081Selected`; the shared Salvation and Guidance
interiors use these bits for return destinations and local variants. Nova's
shared caravan resources retain their applicable selectors for placement and
overworld return. These bits are temporary selectors, separate from visited
flags and from puzzle meanings assigned to the same IDs in other fields.

The sewer aliases distinguish `PlayerShrinkEnabled` from configuring the ring's
Shrink ability and from finishing its size animation. Five passage selectors
choose room 0469's treasure/return variants, while treasure flags retain opened
loot state. `RideArrivalFrom0467Pending` preserves the two-part actor ride into
0466, and the lower-entry layout selector preserves the original scenery choice
for 0467's different entry heights. Other transient aliases cover Latheon's wide
entry camera request, Balacruf's door-opening request and Palmacosta's Radar
introduction latch; their names do not turn these requests into permanent
completion milestones.

Lightning's current electricity flags name Blue/Yellow/Red independently of
the permanent ring-changer and machinery milestones. Its scoped block flags
record the temporary destroyed-block layouts, and rod 103/104/105 aliases record
their raised collision layouts. The four `DarkPath*PassageSelected` aliases
describe local-coordinate lower/upper, left/right passage variants in room 0478;
each passage can be entered from either end. Normal exits clear the selectors,
while a fall retains one for the return layout and cinematic in 0475. The names
do not assign those local coordinates a world-map compass direction.

The ranch aliases preserve the distinction between each movable block and each
socket in Palmacosta 0202, as well as its independently enabled paths. Room 0206
uses separate radar-target suppression bits for successful pickups and ambush
targets; an ambush target's suppression bit has no recovered literal victory
writer. The item reward and permanent treasure flag remain independent operands.

Asgard's team representation is also explicit: at `150 + character * 3`, the
first bit selects team A/B and the next two encode slots 1–3 with weights 2/1.
The original decoder skips character IDs 6, 7 and 8; overlapping bits 168–171
restore the field leader with weights 8/4/2/1. The aliases retain the ignored
Regal debug write and the original packing rather than assigning that character
to a team or rewriting the saved leader representation.
Room 0214's low-bit save branch sets 172 or clears 170 instead of updating 171;
the restore decoder still reads 171. `Field0214Flag::LeaderLowBitSaveMiswrite`
names that observed write to 172, making the misdirected operation visible in
the source. It does not make 172 a working low-bit encoding: there is no
recovered reader or matching clear in this protocol. Room 0215 still reads its
separate `RestoredFieldLeaderBit1` at 171, and the false save branch still clears
`RestoredFieldLeaderBit2` at 170.

Heimdall forest 0482's retained cleanup helper also has names for its observed
reader roles. Bit 190 enables the handoff to `BushBabyLostNoticePending` (197),
whose reader in room 0481 displays the lost-sight notice. Bit 194 enables removal
of actor 201 and touch triggers 6100 when the suppression flags are clear. Bit
195 suppresses that removal block but is consumed by later cleanup; bit 196
suppresses the whole helper. The names do not identify unrecovered setters,
actor 201's identity or normal-play reachability. Bits 191..193 remain numeric
where the helper only conditionally clears them without establishing their roles.
The generic debug display also reads these bits through embedded message
expressions. Those reads expose the current bit values but do not identify their
scene-specific set-state meanings. They are included in the separate message
identity report and in flag-context audits.
The other remaining raw session-flag operands also retain their uncertainty:
Tethe'alla Base 0254's debug “all on/off” choices write 152 and 153 without a
recovered gameplay reader; Flanoir 0317's debug refresh clears 171 without a matching
invitation-dispatch writer or reader. Salvation 0502's two exits clear 155,
and retained Altessa's House routines clear 199 (0138's Ozette Cold departure)
and 198 (0139's recovery conversation). Those contexts establish where the
clears occur, but do not establish the flags' set-state meanings. Equal values
used by other scenes have separate scoped aliases; their meanings are not
transferred to these unresolved uses.

Checking all setter instructions in the six affected original scripts, including
retained routines, finds 75 constant-resolved calls. Of the nine unnamed values,
only 152 and 153 are set within those scripts, by the debug choices above.
This finding covers direct setters in those six images; native callbacks and
cross-field handoffs require separate evidence.

For Salvation 0502's unexplained bit155 exit clears, a further check follows six
constant-destination incoming calls from rooms0501/0503, Derris0530 and retained
field0238. All 53 direct scenario setters in those four code images have resolved
constant arguments, including retained routines, and none sets155. The fixture
separately sweeps canonical retained code and requires argument results for every
setter PC, as well as checking the six incoming-transfer PCs and complete image
hashes. This rules out a direct setter in those four images as the missing writer;
it does not cover transitive predecessors, dynamic destinations elsewhere,
embedded message expressions or native callbacks. Bit155 therefore stays numeric.

A further bounded audit covers 36 reviewed constant-destination incoming calls
to Altessa 0138/0139, Tethe'alla Base 0254, Flanoir 0317 and Heimdall 0482 from
19 source images. It resolves all 196 canonical setter arguments, including
three retained setters outside the default execution graph: Altessa 0138 writes
1089/1091 and Toize 0415 writes 1063. None supplies the unresolved destination
flag values, except the overworld's two already-reviewed 199 writes in its Hima
and caravan callbacks. Those distinct callback roles do not establish a meaning
for Altessa's exit clear. This is a reviewed image set, not a complete incoming
graph; transitive paths, dynamic destinations, message expressions and native
state transfers remain separate work.

Argument audits can use `scenario::calls::decoded_arguments` with an explicitly
recovered canonical instruction view. Runtime `Program::decode` still admits
only its registered/default execution graph. The regression case places a
setter in retained code and opcode-looking words inside an operand and auxiliary
data: the audit finds only the retained setter, while runtime lookup excludes it.

The native `change_field` path in mode10 follows an optional callback at
`8035A508`. The field REL registers code-section2 offset `0x1C008` there. The
original-image fixture resolves both relocation pairs, pins the registration and
complete callback, and checks its four direct imported call targets. Its leaf
callee `800EDB24` records caption IDs 1..98 and 257..337 in separate save-data
banks at `0x1DF8` and `0x1E08`, respectively. Its complete body and three store
instructions are checked; these banks are separate from the story-flag bank
and the unresolved temporary flag bytes. Other callees and later field loading
remain outside this check. It establishes neither a blanket reset nor blanket
preservation of temporary flags across transitions;
the explicit overworld reset loops remain separate evidence.

The original GQSEAF executable also has 132 direct calls to the pure story-flag
reader. At 130 sites, an immediate `li r3, flag` supplies a value outside those
nine unresolved values, with no direct branch entering at the call itself. The
other two arguments come from the sealed save-point constructor and the battle
manual's topic table. The latter's nine chapter rows contain flags 104..133.
The constructor retains its fourth script operand at actor offset `0x834`; its
opening callback sets that same flag. Native restore loops pass flags 850..898
and 900..999 to the pure set/clear helpers. In the supported field REL, the three
table-driven readers and one setter use the existing long-range mapping table,
whose 21 distinct flags are 901..922 excluding 910.
Another inline bank reader at `800810D4` rejects negative indices, then adds
900 before its upper comparison with 940. None of the nine unnamed flag values
can be selected by a nonnegative input to that reader. This does not describe
all accepted indices: very large inputs can overflow the addition into a
negative result, and no general memory-safety claim follows from the comparison.
The original-image tests in `native_story_flags.rs` and `identity_evidence.rs`
preserve these table and call-site checks. This bounds these particular native
paths; it does not exclude indirect calls, other inlined accesses or untraced
cross-field uses, and does not establish meanings for the nine unresolved flags.

A further scan of every main-executable text section checks the full byte span
of each D-form memory operand, including byte, halfword, word, doubleword and
multiple-register loads and stores. None overlaps offsets `0xCB0`, `0xCB2`,
`0xCB4` or `0xCB5`, which hold those nine flags relative to the save-data base.
This rules out those literal displacement accesses, including wider accesses
that start before a watched byte. Indexed operations, adjusted base pointers,
bulk copies and indirect calls remain outside that result; the flags keep their
numeric operands until evidence establishes their meanings.

A separate census checks arithmetic/logical-immediate and D-form memory instructions
whose immediate is the story-bank base `0xC9D`. The original main executable has
13 such sites: the two reviewed restore loops, VM flag handlers, pure bit
helpers and the additional long-range reader. The supported field and battle
REL text sections have none. Complete image hashes and the exact instruction
set are checked. This bounds these literal bank-base references; it does not
exclude other adjusted/indexed pointers, bulk copies or indirect writes, and
does not provide names for the nine unresolved flags.

Cooked `scripts/std/story.sym` also carries the reviewed notes beside its enums
and members. They explain shared storage, reader-only bounds and early writes,
including the limits of names that do not imply event completion. The six enum
introductions above document their remaining numeric operands. These comments
come from the same declarations as the IDs and are freely editable in the cooked
library; they do not turn an unidentified bit into a supported symbolic name.

Some flag consumers run inside native actors rather than explicit script tests.
`create_sealed_save_point` retains its fourth argument as the unlock flag: the
constructor reads it, and the unlock callback consumes one `ItemId::MemoryGem`
before setting that same flag. Coordinates remain ordinary numbers. The original
executable audit binds both callbacks to the story-flag bank and checks the
inventory byte used for the gem.

Enemy action rows have a separate story-flag prerequisite at byte offset `0x3a`;
zero bypasses it. The original archive audit checks all 251 enemy packages and
1,210 action rows. Only Baby Dragon's fifth row has a nonzero prerequisite, flag
73 (`IslandRanch0222Visited`). Cooking preserves it in `required_flag`, and
encounter preparation disables the row when that flag is clear. This shared
native use does not establish a new meaning for the nine unresolved scene flags.
These native-data consumers are outside the script-operand coverage report.

The script flag handlers accept indices 0–2047. For indices 2048 and above,
`set_event_bit` and `clear_event_bit` consume their argument and leave the bank
unchanged. `test_event_bit` consumes its argument and pushes no result. A bare
terminal expression retains the original no-result behavior; a number message
prints -1 without replacing the cached expression value. Negative indices remain
unsupported because the original arithmetic can address preceding save data.
The executable audit pins the complete original handlers and their native
registrations; runtime regressions cover both task and message calls.


Iselia's later ranch split uses a four-slot reactor/rescue representation. Its
first bit is clear for reactor and set for rescue, followed by two slot bits;
the original decoder excludes Sheena. The reviewed companion aliases identify
rescue-team selections used by the affinity and scene-staging calls, retaining
the original masks and candidate order.

The Tower of Mana uses the same team/slot triplets, plus two saved-leader words
at 168..171 and 172..175. Its setup flag records whether Sheena was present when
the selectable three-person exploration team was enabled. Team selection locks,
team switching and pending entrance walks remain separate states. Room 0362's
team-A leader save has an original mismatch: for the low bit it sets 172 instead
of 171, or clears 170 instead of 171. Room 0366 saves that bit to 171 correctly.
Names follow the decoded storage roles, so the cross-team write and incorrect
clear remain visible and compile to the original instructions.

Island Ranch 0227 has two independent elevator landing selections. Their aliases
also appear in the adjoining rooms that set the landing before returning. Bit
152 suppresses lower-elevator restoration; no recovered normal setter establishes
an extra landing. The revolt plan introduction, the four prisoner-area switches,
the revolt scene's start and the Sound ring introduction have distinct names.

Thoda's torch and water-jug aliases preserve the dependency between two lit
torches and the upper jug. Extinguishing either torch reverses that jug. Its
block monitor still tests only X = 525 before locking the block into place.
Balacruf's torch flags retain their original polarity: bits 155 and 158 mean
extinguished, while the other reviewed torch bits mean lit. The Temple of Ice
keeps the statue-door solution and clue conversation separate from the two
temporary ice-block formation bits, which normal room exits clear.

Tethe'alla Base's pass-code aliases distinguish acquired phrases (170..178) from
the terminal's current selections (180..188). Each of three guards supplies a
different phrase depending on the guards already defeated; the pass code follows
defeat order. Orochi's recap and the terminal share the acquired phrase names.
Current selections are cleared on success, error or cancellation, and a separate
flag starts unlocking the door. The four lift aliases identify their selected
landing; lift 151 uses set for its lower landing. Lift 152 retains the original
460-unit entry translation versus its 450-unit raising animation. The debug
all-on/off writes to bits 152/153 alone do not establish gameplay meanings.

Sylvarant Base 0270 stores its Cycle Lock rotation as three bit contributions
of -45, -90 and -180 degrees. Rooms 0279/0280 share two block-position triplets:
one validity bit and two local row/column bits per block. The aliases preserve
their coordinate mapping between rooms and the separate lowered-platform state.
Room 0280's two powered switches retain their different timer prerequisites.

Meltokio's coliseum aliases distinguish a saved highest-affinity companion pick
from the debut entrant. The retained encouragement code maps a matching non-Lloyd
entrant to Lloyd instead; its visible caller follows an unconditional return,
so the names do not assert normal reachability. The debut entrant bits put Regal
before Presea, while the affinity bits follow character-ID order. All original
party snapshots, writes and retained code remain in place.
Room 0411 uses the same entrant selection for its character-specific Kate-rescue
dialogue, including Regal before Presea. Its independent tests and debug setter
remain intact. The upstairs-room selectors in 0399/0400 separately choose room
layouts and return doors; the left-room selector also gates a later Kate scene.

Altamira's two elevator flags retain opposite polarities: 0011 selects the lower
landing when set, while 0013 selects the upper landing. Room 0013's restore offset
15 differs from the raising animation's offset 2. Ice room 0323 likewise retains
formation at Z 1050 versus re-entry at Z 900 for its frozen droplet block. Its
formation mark does not assert that the landing monitor has finished. The radar
pickup marks in Palmacosta Ranch 0203 remain separate from permanent treasure
flags and are set only on the successful reward path with room in the stack.

Darkness's five extinguished-light bits are current selections: a new shot first
restores all five lights, then extinguishes the chosen one. Its four linked block
pairs have separate movement-enable bits and displacement bits with weights
150/300. Names follow the restore formula while preserving the original monitor
coordinates, including asymmetric first-side tests, the -1715 versus -1695 final
Y values for pair 3105/3106, and pair 3107/3108's missing fourth position writer.
The latter still has a four-state restore decoder. No coordinate or writer is
normalized by assigning a name.

Flanoir's night-visit flags distinguish rank dispatch from individual declined
invitations. Cleanup applies the recorded decline penalties and clears those
marks. The doctor's departure scene has its own character marks: its original
code stages ranks 7/6/5/4 although the dialogue asks for three escorts. The rank-5
Genis branch removes him without writing his mark; that omission remains visible.

The Salvation aliases distinguish staged barrier hits from the cleared barrier.
Room 0171's lowered-bridge mode changes how its two tilt-state bits are decoded;
those selector bits do not denote unconditional positive/negative angles. The
original collision arrangements and passage-trigger conditions remain explicit.
Room 0505's five floor bits match its 1F–5F menu and retain the entry path that
sets floor 1 without clearing other floor bits. Room 0506's entrance selector
chooses the left/right return passage; it is separate from the selected floor and
the Apocalypse Devil's Arm acquisition flag.

Derris rooms 0130/0131/0132 share lower/middle/upper layout selectors, including
their camera presets. Room 0133 has separate EXGemLv4 and BlueShield layouts;
the associated bits still control each layout's collision, enemies and passages.
Shared corridor 0134 has nine named variants that retain their selected return
landing and destination layout. These tests remain independent. The future-side
block-transfer selector is consumed inside room 0135's per-block loop; multiple
queued block marks therefore retain the original ordering-dependent placement.

Ymir's two boulder-hit marks remain independent: either first hit leaves cracks,
and both clear the fish's path. Its carrier transfers have separate pending-arrival
bits and departure-animation suppression switches. Bit 180 retains its cross-use:
the fruit-route return from 0521 marks it, room 0519 consumes it only at step 1900,
and a different 0519 helper also uses it to skip a carrier departure. The enemy
respawn marks are written outside the battle-result condition, so their names do
not claim victory. The blue-flower camera tour and switches without literal
setters retain names for their observed effects rather than inferred milestones.

Toize's lift-101 detour setting is separate from discovering the cable switch.
Its fast-mode flag restores the original rounded duration table; the compiler
does not derive new durations by dividing the slower values. The two tunnel
light controls in 0413 retain their own visibility/animation states. Gaoracchia's
six entry requests create the selected room-0514 blocker layout and are then
cleared. Its room-0518 return selectors instead remain set until the return
event. Independent tests and default transfers retain their original order.

Sybak's shared hall 0438 has three persistent layout modes, separate from its
nine room-door selectors. The room selectors restore scenery and NPC variants
and return to the appropriate local-X door; they do not replace the selected
hall mode. The early Key Crest visit selects its room directly, while two story
stages block switching between hall layouts. Altessa's attack aftermath has
three independent conversation marks; only their combined state advances the
story, and Yuan's mark separately controls his re-entry appearance.

Luin's donation request updates rebuilding on town exit, separately from the
reached-donation-target flag. Six exit variants do nothing at stage 600000;
0455/0456 instead mark the first missing statue in Sheena/Lloyd/Raine order.
The exit handlers retain their different branches and lack of an explicit clear.
Sylvarant Base's prison-door marks remain independent of its solo-battle reminder
and electric-orb passage state. Room 0268 uses bit 160 for the orb introduction,
while the Cycle Lock and its side rooms use that bit as a rotation component.
The Rheaird departure mark keeps its story-stage guard, and room 0276's three
block-transfer writes preserve the initial position decoded by 0279/0280.

Room 0238 shares camp scenes and stone-marker interactions. Its nine selectors
retain the overworld return destination; only the seven regional marker cases
also unlock Long-range Mode. Landmark 52 keeps a destination-only name, and the
Ex Skill tutorial selector remains separate. Both original event-1000 registry
entries and their independent tests remain. Applicable overworld setters use
the same reviewed meanings through `WorldFlag`.

Camp room 0250 clears its four conversation marks on entry at story 1304000,
without clearing the shared affinity-adjustment latch. Genis's second choice
can consume that latch for -60, while Raine's second choice neither adjusts
affinity nor consumes it. Bridge 0077's side-layout selectors are separate from
visiting the bridge; the approach from 0078 also selects the 0075-side layout.
Kate's bridge-rescue return selects Meltokio's left upstairs room separately
from the permanent rescue flag.

Hima's pre-Tower conversations share one +60 bonus latch; its Pietro inn reminder
has a separate switch consumed by the approach event. The Earth temple's chasm
notice is distinct from its synchronized-earthquake retry switch, which remains
set when the ring has the wrong function. Thoda's sign can suppress the first
return-trip question without playing it, and its accepted sightseeing departures
use a separate request consumed before the outbound cinematic. The extra room-8
jug/block reads preserve the states restored by room 9.

Triet's entrance blocker and retracting slab retain independent marks and the
later-story override. Its retained boundary-5051 latch still has no extra action
in the ring-contact handler. Mizuho's arrival animation request clears after five
ticks following control return, rather than after the animation finishes.
Mithos's Altessa placement selector keeps its story-13601000 guard; Heimdall's
room-0285 arrival writes retain a name for their provenance without asserting an
unrecovered reader or reward.

The banquet aliases distinguish selected terrace companions from first title
conversations; Meltokio 0399 clears both sets afterward. The Mizuho and Igaguri
aftermath scenes each allow one shared first-conversation affinity bonus, even
though several companions remain available to talk to.
Original counter quirks remain visible: Sybak 0434's first lost-child inquiry
assigns stage 130001 rather than adding one, and Sybak 0436's slums remark tests
1712 but sets 1713. The latter suppresses counting the library inquiry in 0445
without proving that conversation happened. Naming those operands preserves
the original mismatch and its cross-room effect.

Some flags have no recovered literal setter. Their names describe the observed
switch rather than an assumed preceding event: for example,
`FinalSwordDaisDescriptionSuppressed` skips that description, while
`ClaraAtNeilFollowupEnabled` enables Clara's appearance and replies. These names
do not assert that a quest completed or that a missing writer has been recovered.

The field debug menu reuses bits 150..152 within a single entry: they select
blue/yellow/red lightning and reserve actor slots 32..34. Its ID1..ID8 actor
menus use bits 150..157 for actors 32..39, showing "used" or "not present" from
the marker; both creation paths set it, and removal clears it before despawning.
`DebugLightningFlag` and `DebugActorSlotFlag` distinguish these operand meanings
while retaining their shared storage and the original collision. They do not
assert that the engine currently has an actor in the slot.
Because a whole-script namespace would misname one feature, the compiler's
`data/scenario-operand-domains.tsv` selects a domain for each reviewed native PC
only when the entire original binary's SHA-256 matches, the native, parameter,
type and value agree, and that value has an explicit name in the supplied cooked
definitions. Each profile declares its field or world host. Variant names are
read from the supplied definitions. A changed binary, missing definition or
incompatible host context uses the ordinary
identity rules instead. Once exported, `.sym` edits compile normally; no original
checksum is needed to compile or run a mod.

The same catalogue can identify a reviewed `memory::global_alias` load. It
requires the complete image hash, host, word width, exact PC and byte address.
The selected alias enum stores the ordinary global index, so a name changes
neither the physical word nor its encoding. Palmacosta 0205/0206 use fourteen
such references for their warp-room state and battle counter. Retained quest
debug routines in the same room-0206 image keep the Celsius and Mana Leaf Herb
route meanings; a field-wide replacement would misname those uses.
Proven non-quest loads also discard quest-stage provenance through copies.
Renamed alias members are honored; omitting the optional alias or moving its
index restores the ordinary shared-word label while retaining untyped numeric
values. It does not force a quest-stage name onto the room input or counter.
The original-data fixture pins both images, every profiled load, all nine room
dispatch inputs, counter increment placement, hint guards and later quest uses.

Profiles can also name a reviewed flag-ID assignment to shared scratch memory.
`memory::flag_constant32` requires the complete image hash, host and exact PC,
plus contiguous word-load, literal and assignment instructions matching the
recorded address and flag value. Incoming labels between those instructions
prevent annotation. The selected flag name replaces only that literal; the
cell does not acquire a permanent type and callee reads stay as memory reads.
Missing or moved flag names restore the number, and renamed members are honored.

Latheon rooms 0492..0498 use this for 50 flag-ID assignments in flower and plant
helpers. Forty-six select flower blowing-state flags; four select the existing
plant-introduction or record-suppression flags. Room 0496 deliberately uses
room 0495 flower 3001's bit 465. Zero sentinels remain zero, and the shared
helpers retain their Kirima/Amango branches, dynamic set/clear/query calls and
original waits. The original-data fixture pins all seven images, every assignment
and its consuming slot, the shared 465 identity and matching recompilation.

The original overworld also gives bit 199 two meanings: region event 8 enables
Hima's Pietro inn reminder, whereas event 60 selects the House of Guidance 0093
exterior before entering Nova's caravan 0243. The latter's two exits read it to
return to landmark 60. Operand profiles use the existing destination-field enum
for each write; there is no single overworld alias for 199.

The room-0247 transfer marks describe their observed writers. Twenty-two world
registry entries are indexed exploration callbacks: their shared indices use
marks `160 + index`, with only the nineteen actually written indices declared.
The two worlds share the marks at indices 07, 25 and 31; index 31 covers both
the Sylvarant Sand Worm callback and a Tethealla item callback. Their
`WorldFlag::ExplorationNNCaravan0247TransferMarked` names preserve the actual
transfer to room 0247, rather than asserting an item award or encounter victory.
World event 54 has the separate `TrietCaravan0247TransferMarked` write at 193.
Room 0247 has no recovered reader of these marks and both exits still return
to landmark 54. These are writer roles, not consumed return selectors or
permanent exploration-completion flags. Equal bits in other scenes keep their
own meanings, and undeclared exploration indices remain numeric.

Luin's 514/515 names retain the original scenery groups `ATrap` and `Btrap`,
verified through the scene's animation-bank mappings and mesh joints. Region
1101 writes `LuinATrapAnimationPlayed` after its 100-tick animation wait; region
1102 writes `LuinBTrapAnimationStarted` before animation and dust event 3000,
then changes to its retained pose later. Entry restores each group's alternate
animation bank. These flags do not describe a rebuilding donation or reward.
The Light-pact sequence similarly writes `LightPactSequenceMarked` (9) before
entering the Luna/Aska battle and again after the rewards; it cannot prove the
battle was won. Camp 0522's `AngelNightCampSequenceStarted` is written before
the selected Colette angel-night scene, with its later stage changes separate.
These last two aliases describe observed writes without claiming an additional
effect from an unrecovered reader.

`LongRangeSylvarantRegion10Unlocked` names flag 910 through its native reader.
The world travel-permission grid is separate from the named stone-discovery
table: Sylvarant uses grid code 10 in 38 cells, and the native lookup passes
`world * 11 + region` to a query of event bit `900 + index`. Thus world zero's
region 10 uses 910; Tethe'alla's same grid code uses 921. The name keeps the
native index without guessing a geographic label or treating it as another
Katz Island flag. All 16 mapping-service group-8 copies retain their original
write, and the group-15 loop still sets 900..910. The native evidence audit
checks the original grid, lookup instructions, relocation targets and executable
flag query, with exact GQSEAF fixture hashes.

The field module's native unlock path also uses a twelve-row table at data+0x8c.
Each row has a key, the original region caption and up to three flag halfwords.
The handler tests the first flag before setting each nonzero slot. Its 21 distinct
flags are901..922 excluding910; the captions support the corresponding named
long-range regions, including the separate flags for regions covering multiple
grid codes. The executable audit resolves the table and caption relocations,
checks the first-flag gate and three-slot writer, and binds every slot to its
declared name. This particular native writer supplies no meaning for the
remaining unnamed scene flags; it is not an exhaustive inventory of native writes.

The reused ranges 150..199 still need meanings scoped to the particular scene. The observed
1700..1717 and 1749 literal uses now have reviewed field/world aliases; their
numeric reuse must remain visible rather than becoming global milestones.

Main-story stage names describe phases of the shared word, rather than separate
permanent flags. The early Iselia/Martel Temple/Marble/Dirk sequence now names
its individual classroom, escort, tutorial, oracle, rescue and departure phases.
For example, `MarbleRescueGuardBattleComplete` is written after formation 6
returns, while the escape animation still runs; the separate
`MarbleRescueForestAftermathPending` is written just before returning to the
forest. `ColetteBirthdayTerraceTalkPending` covers both starting the talk
immediately and leaving Colette waiting at the bench. `BurningIseliaExplorationActive`
restores control before the Marble confrontation, not after victory. Duplicate
stage writes and the original phase order remain intact.

The base escape and Triet return stages also retain their alternate paths:
`SylvarantBaseLloydImprisoned` can be restored by recapture, the guard-driven-off
stage precedes the cell-door animation, and Triet's first morning can follow
either the optional Kratos advice scene or choosing sleep. Securing Max's ship
does not prove Lyla's letter was accepted; both negotiation choices reach that
stage, while the letter inventory and flag remain branch-specific.

Retained Dirk rooms 0370/0371 reuse 1601000/1602000 for finding Dirk alive and
agreeing to repair Genis's Key Crest, whereas the main route uses those values
for Palmacosta arrival and the potion dispute. Explicit `FieldNNNNStoryStage`
enums in the supplied standard definitions override `StoryStage` only for their
declared values in the matching field context. The coupled Dirk rooms share
their three prototype phase meanings; other stage values, angel progression,
counters and native IDs retain their own domains. Ordinary global definitions
apply when no scoped variant is supplied. Numeric storage and phase transitions
are unchanged. Their original-PC evidence checks both the raw assignment and
the selected source namespace.

The following Palmacosta/northern-route phases keep the route choices distinct
from their common progress. `PalmacostaRanchTrapResponseChosen` follows either
attacking the ranch or returning to question Dorr; their quest words identify
which choice was made. `PalmacostaCrisisAftermath` follows the two orderings of
the ranch/Dorr sequence, with Chocolat still missing. `HakonesiaNorthernRouteActive`
can instead be set by presenting the road pass and by the route-B debug presets;
it does not prove the Palmacosta crisis was completed first.

Hima's private-talk choice, completed Colette conversation, morning assassin
encounter and dragon passenger selection have separate stages. The first Tower
escape queues the return to the Renegade base, where the awakening briefing,
Yuan confrontation, individual block/ring operations and decision to seek
Tethe'alla's research remain distinct. The initial Rheaird departure stage is
written before its acquisition flag and flight handoff. Meltokio's sacred-wood
plan, Presea's agreement, gate permission, letter handover and supervision deal
also stay separate; the Sybak agreement is written before Zelos's join/setup.

Sybak's diagnosis, junk Key Crest, laboratory permission, arrest and Kate escape
phases retain the differing Academy scene versions. The shared escape stage
does not imply the Tethe'alla Map was awarded in both versions. Colette's Fooji
recovery stage precedes the Pronyma battle, while its aftermath has a separate
value. Commissioning the modified Elemental Cargo, receiving the Wing Pack,
approaching the bridge pickup and starting the EC departure remain distinct;
the departure stage precedes the acquisition flag. Gaoracchia's battle and
Mizuho retreat stages do not imply Regal has joined. Mizuho's alliance stage
also precedes Lloyd's Gentle Idealist title award, and Regal's join/setup follows
in a separate phase. The original writes and reward timing remain unchanged.

The Meltokio sewer phases distinguish the first ring change, tutorial starts,
fixed block placements, spider-web hint starts and opened passages. The ring
change stage can follow either direction of the Fire/Shrink toggle. Room 0466
also adds block-placement bits to the story word arithmetically; those operations
remain intact. Room 0467's alternate block-3005 landing at (-655,4300,350) has a
distinct stage, even though original entry restoration uses (-505,4150,350) for
both landing phases. The relocation and hint stages are written before their
animations, and the room-0468 ambush stage precedes its battle.

Ozette's alternate scenes share the follow-Presea, wood-delivery, Colette-stumble
and post-awakening rescue phases. Altessa's first-visit stage has no audience
dialogue itself; Tabatha's advice and the inhibitor-ore journey are separate.
Toize distinguishes its guard battle, bomb barrier, search introduction, ore
award and Vharley confrontation. Sheena's Volt trial, agreement to retry, temple
briefing, pact aftermath and Orochi's Rheaird-location report also remain
separate. The Tethe'alla base's orb targets 43/44 advance the main story word,
whereas target 45 uses its own flag; activation stages precede the final wait
and sound cleanup.

Colette's Flying Dragon escape and later party rejoin have separate stages,
as do the Ozette survivor escort, Altessa's confession, Raine's departure and
the search with Mithos. Altamira's Employee ID, Sky Terrace permission and
Alicia's request are distinct from Regal's later request to return there.
The latter phase is also forced by two Meltokio test-menu presets; it does not
prove that the normal Altessa conversation ran when using those menus.

Island Ranch value 13401200 spans the flooding sequence. Both flood-cutaway
rooms write it before returning to the control room, where entry at that value
starts the Rodyle battle and Botta sequence. The control-room helper writes the
same value again before the escape-room transfer. Its name therefore describes
the shared active phase, rather than claiming Botta's sacrifice already happened
on every setting path. The original repeated write remains intact.

The final Light-pact approach, Kratos warning and pact rewards are separate
from the Great Seed movie and subsequent Iselia Ranch operation. Kratos's join
retains both original Traitor-title calls. The ranch phases distinguish team
selection, freeing the captives, dispatching the rescue team, the Forcystus
battle, stopping the reactor and Colette's crystal-disease reveal. That reveal
also writes her separate angel-progression word. The following Yuan and Mithos
cutaways retain their individual phases, followed by Phaidra's briefing about
the vanished Tower of Salvation.

The next Iselia and Dirk phases separate the village's welcome, Dirk's
consultation, the night and the return-to-Altessa plan. Twenty paths write
`DirkPostTreeMorningPending`: eighteen selected-companion handlers in the
retained room variants, accepting bed, and the balcony's retained affection-event
placeholder. The stage queues the morning without identifying a particular
completed conversation. The selected-character conditions remain intact.

Meltokio's archive authorization, Rune Crest recipe and King’s Letter remain
distinct operations. The recipe scene and later letter award both write
`RuneCrestMaterialsSearchActive`; the latter does not advance to a new value.
Seles's agreement to return the crystal is separate from its acquisition, and
the Tower stair creation and entry discussion precede the next Kratos encounter.
Its two battle-result continuations still converge on the prison transfer.

Welgaia's prison discussion, Regal's cell-door break, Colette's scouting, box
discovery, obstruction removal and restored transporter have separate stages.
The disguise plan and Mana Fragment award follow before the emergency escape.
The weightlessness tutorial, two gate unlocks, Elevator Key award and key use
also retain their original order. Colette's Rune Crest treatment writes both
the main-story phase and her separate cured angel-progression value. After the
Mithos attack, each of the three aftermath conversations can be the last one to
set their shared completion stage; the bedside cutaway and Flanoir doctor plan
are later phases.

Flanoir's companion selection precedes its private conversation. Each invitation
has a rank-four fallback and an acceptance write; declining instead dispatches
another invitation. The shared completion stage follows any of the eight private
conversations. Bit 26 is now `FlanoirKratosRouteSelected`: its only recovered
setter is at the start of Kratos's conversation, and the later Tower scripts use
it to select Zelos's battle/death route and Kratos staging. It is not a generic
night-complete bit. The original early setter and all readers remain unchanged.

The Great Seed hall writes its reunion stage before the Mithos battle. Heimdall's
night has a direct Kratos-route morning transition as well as the outdoor-talk
path, and its two village exits share a departure marker. The Kratos duel stage
precedes the Origin pact, while Dirk's forging stage precedes the Eternal Ring
inventory award. These phases retain their original timing.

Ending stages distinguish the first camera sequence, failed awakening attempt,
final wish and successive epilogue transfers. The Altamira-pending value can
follow either Zelos's Meltokio epilogue or the direct Mizuho route when Zelos is
dead. It does not imply that the skipped Meltokio scene played. The final Kratos
line still waits for its media, fades and hands off to field 0532.

Dirk's farewell at Anna's grave then writes `EndingClearDataPending` before the
completion-data screen. That screen writes `GameClearData` before opening the
save menu; loading this marker selects the GRADE Shop/new-game path. The marker
therefore does not prove that a save or GRADE purchases have finished.
`NewGameStart` resets the story word before the classroom transfer. The retained
Mana route-A and Ossa Normal debug presets also write zero; this assignment alone
does not reset other flags or globals. Ossa's Holy/Dark Bottle test choices use
the field-specific `Field0351StoryStage` aliases for 200 and 100.

All 446 recovered constant assignments to the main story word now have reviewed
names, covering 339 distinct numbers with field-specific aliases where needed.
This inventory excludes arithmetic updates and interactive number input.
Comparison-only limits have separate reader names rather than invented event
names. Proven copied dispatch snapshots can use the same domains. Uncertain
origins remain numeric; named operands do not imply that every expression or
unnamed shared-memory word has been resolved.

Reviewed comparison-only names describe their observed reader role. An
`UpperBound`, `LowerBound`, `Boundary` or `Predecessor` is not a newly recovered
story event or a claim that the game assigns that exact number. Their evidence
rows point to the original comparison and preserve its operator. For example,
the Iselia ranch's Rune Helm chest requires story strictly greater than
`IseliaRanchRuneHelmSpawnBoundary`, while Dirk's pellet processing selects
figurine season nine at or above `FigurineSeasonNineLowerBound`. Chest ownership,
pellet types and other eligibility conditions remain independent. The Triet exit
input-release marker has an original reader but no recovered constant setter.
The angel-night inns, steamship introduction, Rosa/Joshua quest and shared skit
helpers likewise retain their separate bounds and flags.

Sewer 0466 starts block progress at 10801120 and adds 1, 2 and 4 for the three
placements. `Sewer0466AllBlocksSumThreshold` names their pre-lever sum; later
phases also satisfy its `>=` checks. The first lever adds 74 to the current word,
yielding 10801200 from 10801126 or 10801201 from 10801127. Its
`Sewer0466FirstLeverThreshold` comparisons and equality-only temporary-state
reset therefore remain distinct. Names do not replace those additions with an
assignment or turn the story word into an independent set of Boolean flags.

Welgaia's Chipped Dagger, Spider Figurine and Mirror Shard rescue helpers each
add 1000 to the story word. From its ordinary arrival phase, three such updates
reach `WelgaiaCompanionMemoryReturnPhase`; entry also requires the separate
pending bit before dispatching the selected companion's return scene. The
retained alternate trap phase remains separate. Thoda's dog-flag debug menu uses
`Field0006StoryStage::DogFlagsDebugMenu` for its exact-value reader, without
inventing a global story milestone for that debug selector.

Luin's reconstruction word has its own `LuinRebuildingStage` domain at 0x00f8.
The neighbouring `LuinDonationProgress` word at 0x00fc remains a numeric credited
gald counter. Pietro's Hima pledge, first fund conversation, intermediate funding
goals, city restoration, expansion and statue phase have distinct names. Funding
goals queue a local update bit; the town exit applies one transition from its
saved stage snapshot. The donation clamps, original payment deduction and
main-story gate for the 173500-gald goal remain unchanged. Partial donations can
contribute to a goal; a phase name does not count individual payment calls.

The `Ruined` marker's original setters are camp continuations. Camp 0522 tests
updated angel progress against 201 before setting it. Retained camp 0240 assigns
201 inside its condition, whose nonzero result enters the body. The original-PC
evidence checks both different operators. Earlier descriptions of these two
setters as merely debug presets have been corrected; the original assignment
quirk and its effect on Luin remain unchanged.

Stage 600000 is the expanded-city statue phase. The Sheena, Lloyd and Raine
statues use separate flags and further credited-gald goals, while this stage
persists. Pietro's final thanks also checks the Raine statue flag. Authored world
rules use the typed `quests::luin_before` helper and retain their existing 40000
model boundary as `WorldModelRestoredBoundary`. That authored threshold has no
recovered direct original operand and is separate from the field's normal
rebuilding phases. Other quest words do not gain Luin's
equal-valued stage names. Supplied definitions must include the Luin enum when
decompiling this word; modded variant names come from those definitions.

The Book of Regeneration word at 0x00e4 has its own
`BookOfRegenerationStage` names. Its early values distinguish the potion dispute,
Cacao's shop encounter, potion delivery and the Hakonesia pursuit plan. Koton's
statue request and the priests' recovery plan are later phases. Thoda writes
`ThodaStatueRetrievalPhase` after Genis's Icicle scene, before it starts the timed
retrieval events and restores control. It does not prove the retrieval succeeded.
The priests' earlier plan can also be agreed before Genis knows Icicle.

Koton's normal exchange branch writes `BookConsulted` after the party reads the
seal passages. He explicitly keeps the book; this phase does not represent an
inventory grant. The separate late conversation, angel-progress-dependent reader
and Golden Armor conditions remain intact. `ConsultationTimerUpperBound` is the
exclusive 50001 limit in retained timer helpers, including the 50000 consultation
phase in their range, without inventing a new event at 50001.

The Linkite Tree word at 0x0130 uses `LinkiteTreeStage`. Its family inquiry starts
at 10000 and records first interviews by addition:

| Interview | Contribution | Information or reward |
|---|---:|---|
| Sarah | 1000 | The bird's location cannot be reached on foot |
| May | 3000 | Linkite Nut and its sound |
| Alduin | 6000 | Ossa peak, strong wind, melody and the missing tree |

Names for the six partial combinations and the complete sum 20000 retain these
additions and both caravan copies' duplicate-interview checks. This is arithmetic
in one shared word, rather than independent Boolean flags. Completing the sum
dispatches the requirements summary; the memo, dead-tree reconfirmation, revival
plan and flute-construction plan use separate phases. A recorded plan does not
prove that the party has found the tree or obtained every revival resource.
`LinkiteOcarinaReceived` follows the item grant and also selects the restored-tree
world model, while subsequent Aska handling and the Light pact remain separate.

The authored caravan and world rules use typed book/Linkite helpers. Equal values
stay in their own domains: 40000 is the Thoda retrieval phase in the book word,
ocarina receipt in the Linkite word and an authored model boundary in Luin's
word. Unknown mod-added values stay numeric, and arithmetic deltas do not acquire
quest-stage names merely because their numbers coincide.

Temporary copies of reviewed globals can also supply comparison names. A copy
of the story word into `memory::i32[0x0000]` can later compare against
`story::StoryStage::FirstAngelNightCampPending` while retaining the memory read.
The analysis follows jumps and loops, keeping an origin at a join only when
every incoming path agrees. Script and native calls clear inferred memory facts;
overlapping narrow writes, arithmetic updates and uncertain indexed writes also
invalidate affected facts. External event/call entries start without facts.

This is a captured value's domain, not a permanent type for the temporary slot.
Assigning a new numeric value to that slot stays numeric and ends the inference.
Changing the original global alone does not invalidate an already captured
snapshot. Reversed comparisons, literal widths, pure constant encodings and
field-specific story aliases remain intact. Unproven paths retain their numbers.

The two Palmacosta ranch words use `PalmacostaRanchAssaultStage` and
`PalmacostaRanchDorrStage`. They separately record the decision to attack first
or investigate Dorr first, Neil's escort, captive-rescue plan, Chocolat's rescue,
the control-room ambush and projection, and the later escape. Shared scenes keep
their route-specific values. In particular, 6000 starts the Magnius confrontation
in the assault word but records Neil's key-infiltration agreement in the Dorr
word. The Magnius-confrontation values are written before the boss battle, and
the Chocolat-removal cutaway has its own later completion markers. Conditional
Pass/Blue Card awards and the optional promise to help Clara remain separate.

Both original consecutive aftermath assignments remain: the assault escape
scene writes 9000 then 10000 before the Dorr visit, while the Dorr-first escape
scene writes 14000 then 15000. Their final 15000 markers occur at different
points: the assault route's village farewell and the Dorr route's ranch-escape
farewell. Authored terrain and caravan helpers preserve those differing
thresholds and the caravan's inclusive `<=` comparison, including its endpoint.

`AsgardRanchStage` belongs to word 0x00e0. It distinguishes Orb entry, the map
briefing, assigned teams, the requested monster-production shutdown, the six
cleared conveyor targets, support-team arrival and guard battle, captive
evacuation, transporter denial and return, and transporter activation. Activation
is earlier than the Kvar battle. The exterior's rest-departure agreement and the
inn's later discussion of Colette's humanity are separate phases. Retained debug
presets and the original leader-bit assignment quirks keep their exact writes.

The shared `QuestStep` enum and its generic world helpers have been replaced by
quest-specific enums and typed helpers. Unknown or unreviewed values remain
numeric. Equal values in a reviewed quest never supply names for another word,
narrow memory writes or arithmetic deltas.

The Fire and Water progress words use `FireSealStage` and `WaterSealStage` at
0x00c0 and 0x00c4. Their equal numbers can have different timing. Fire's 21000
marker follows the guardian battle but precedes the altar prayer and Remiel's
blessing. Water's guardian aftermath is 20000; its 21000 marker follows the
branch-dependent seal-release conversation. The Water dialogue can describe the
second, third or fourth seal according to Colette's angel progression.

Fire's 41000 marker records Efreet's trial introduction before the battle. Its
50000 victory-aftermath marker is written before the vow, Garnet, summon
techniques and pact bit. The exterior earthquake discussion can later write
51000 under its original mana-pair-counter condition. These names preserve the
early writes and the independent pact/pending-earthquake flags.

Water's early phases distinguish the first washtub departure, Thoda arrival,
lookout sign, oracle stone and handled temple entry. Its 22000/23000 phases
record failed/successful Undine trials before the angel-night camp. The 30000
camp-pending phase transfers to 0522; the camp continuations advance it to
31000 before returning to Thoda. The 40000 revisit-entry marker includes both
an optional introduction and silent skips. Later failed trials use 41000, and
later pact completion uses 50000. Both successful pact paths write their value
after the Aquamarine, techniques and pact bit, unlike Fire's early 50000 write.
Temple-entry markers do not prove a particular conversation played; their
original story, party, consent and pact conditions remain independent.

One Triet Fire Temple debug preset writes main-story value 10002000 when marking
the Celsius pact. `Field0221StoryStage::CelsiusPactDebugPreset` names that local
debug write. It is not the normal Efreet pact-completion event or a recovered
global Tethe'alla milestone, and its unusual story assignment remains intact.

The Wind word at 0x00c8 uses `WindSealStage`. Visiting Balacruf before the Asgard
quest adds 500; Asgard's history, bomb incident, follow-up, sacrifice explanation,
dancer agreement, inn departure and deciphered-map conversation add 1000 each.
The original ranges preserve that offset. The exact 2500 and 7500 readers name
the alternate bomb aftermath and map phase. Map possession remains independently
checked, and 8000 is an exclusive timer bound rather than a recovered event.

Balacruf's altar plan, oracle-stone entrance, first interior conversation and
exterior Sheena cutaway have separate markers. The cutaway-handled marker also
covers the branch that skips it. Wind's 21000 follows Remiel's seal-release
conversation, while its guardian bit is set later. The camp uses the same
30000-to-31000 continuation pattern as Water, without fixing the ordinal seal.

Wind's 41000 records a failed Sylph trial. Its 50000 follows the vow, Opal,
techniques, pact bit and conditional title; the exterior earthquake discussion
uses 60000. These timings differ from Fire's equal-valued markers. The retained
999999 history-answer selector and inclusive 20000 guardian-entry bound describe
their original readers; neither has a recovered constant setter or claims a
normal quest milestone.

The Light word at 0x00cc uses `LightSealStage`. Its Tower of Mana introduction
does not prove possession of the key. The library's split-party plan, departure,
remote-team agreement and reentry are separate from the later regroup call and
restored roster. The silent exterior Sheena cutaway also remains separate from
the library's later battle and encounter flag.

Rooms 0365 and 0363 each distinguish activating the light source from starting
the reflected-path sequence. The path markers are written before their camera
and scenery animations. Room 0366 records mirror-coordinate initialization,
light-source activation and puzzle completion separately. Its three illuminated
targets gate completion; the countdown-zero check only selects a sound. The
original countdown, actor tests, party selection and saved roster remain intact.

The 21000 marker follows the guardian, Luna/Aska request and conditional Remiel
conversation, before the guardian bit. The camp's pending/completed phases do
not establish a Light pact; that later sequence uses main-story progression.
The retained Luin hint reader still tests exactly 1, while normal introduction
helpers write 1000. `LuinLostTowerKeyHintSelector` names that reader without
inventing a setter or correcting its value. The 1001 timer lower bound and 20000
guardian-entry upper bound likewise describe comparisons rather than events.

The two Boltzman words have different domains. `BoltzmanTowerKeyStage` at 0x00d0
names the early Sophia invitation, inn welcome, Tower search plan, key-inquiry
phase, Piccaring's pilgrimage/destination hints and Tower Key receipt. The key
phase can be written before the tower's introduction and lock dialogue, and
its original branch for an already possessed key remains intact. The separate
healing-request choice is not implied by these stages. Key receipt also does not
establish receipt of Boltzman's Book or Pietro's cure.

`PietroInfiltrationStage` at 0x00d4 records Sophia's report that Pietro died,
then the later infiltration bargain. Pietro appears alive in the grave scene;
`SophiaReportedPietroDead` therefore names her statement rather than an actual
death. The later marker follows the Desian Orb grant and the quest-explained
bit. It records agreement to exchange ranch help for a cure, without implying
that either side has fulfilled the bargain. Original route gates, item grants,
flags and shared timer comparisons retain their order and conditions.

The first Asgard ranch infiltration uses two separate words:
`AsgardRanchEarlyRouteStage` at 0x00d8 and `AsgardRanchLaterRouteStage` at 0x00dc.
The initial Luin reaction selects the early word before
`PalmacostaFirstArrivalComplete`, and the later word at or after that cutoff.
Both retain the ruined-city introduction, patrol ambush, Raine disguise, gate
bluff, interior cutaway, Botta/Kvar encounter and Sheena escape phases. The 32000
marker is written before Botta appears, while 33000 follows the room escape.
The optional private Kratos/Exsphere discussion uses 40000; its other response
can return to Luin at 35000. Both aftermath entry paths remain valid.

Their endings differ. The early word advances directly to 50000 for Luin's
escape aftermath; the later word uses 41000 there and advances to 50000 only
after the exit discussion identifies Pietro in Hima and agrees to visit him.
The early route's return-to-Hima decision uses a separate bit. Its retained
41000 NPC dialogue boundary has no recovered constant setter. Sheena's joining,
Pietro's location bit and debug presets remain independent, and neither word is
the later `AsgardRanchStage` used for the second infiltration and Kvar battle.

Colette's angel-word night names follow seal order: `SecondSealNightComplete`
is 101, `ThirdSealNightComplete` is 201, and `FourthSealNightComplete` is 301.
The older names numbered these one night too early. The guardian dialogue reads
1 for the second seal and 101 for the third; the next blessing opens the route
to the Tower of Salvation. The shared camp handles sleeplessness, lost sensation
and voice loss, adding 100 after each corresponding night. Retained camp 0240
still assigns 201 inside its condition; normal camp 0522 compares the updated
word against 201. The separate Luin ruins writes keep their original operators.

The first Fire camp has its own main-story phase,
`FirstAngelNightCampDepartureReady` at 1305000. Camp 0250 writes it after the
walk-and-cough scene, party restoration and healing, before the timer helper
and world-map transfer. It was previously named as the walk's start and its
note incorrectly claimed that Colette's angel word recorded later completion.

`VoicelessEligibilityLowerBound` names comparison value 300. Speech, dog-naming
and side-event readers usually test the inclusive 300..1000 range or its
complement; the Undine response uses `<300` alone. No recovered setter writes
300. `IseliaEntryAppealLowerBound` names 20000 in the two retained village-entry
appeal guards. Both still require `<301 && >=20000`, an unsatisfiable interval
for the same word. The decompiler preserves the eager AND and both constants;
the name does not repair the condition or invent an angel event at 20000.

The reviewed direct main-story comparison values also have reader names.
`LightPactEraBoundary` at 20000000 partitions shared timer handling and Dirk's
earlier dialogue/figurine path. The timer ranges intentionally retain both the
old range's inclusive `<=` and the following range's inclusive `>=` at that
value. `RuneCrestMaterialsTimerBoundary` at 21306000 separates the earlier
archive/material-search range from later cure-journey timer handling. Its first
later case independently requires Zircon and Mana Leaf Herb. Neither number
has a recovered constant setter or represents a completed event.

Reviewed snapshot limits also name Koton's pre-pursuit and statue-demand
dialogue, Neil's pre-card-key wait, the cleared-conveyor scenery in Asgard
ranch, and Luin's expanded-layout transfers. Tower of Mana library setup uses
an inclusive 99999 bound, while access from room 0366 to guardian room 0369
uses 999999. Their names describe those readers and preserve the different
limits; they do not add completed quest events at those values. Linkite's zero
phase distinguishes the initial caravan progress from early hospitality, while
the separate Nova-search flag selects repeat reminders. Wind's 4500 explanation
phase retains the optional earlier Balacruf visit's cumulative +500 offset.

`StoryStage::TrietWantedPosterExamined` is the computed 1001500 phase: the
wanted-poster conversation sets its separate flag and adds 500 to the initial
`TrietWantedPostersSeen` phase. The crater conversation sets its own flag without
advancing Story. Both retained town scripts use the computed phase as a dialogue
upper bound and an exact emote selector. The recovered source keeps the addition
and the captured story value used by the interaction.

Other main-story snapshot names describe the observed setup or availability
range: Iselia's entry refusal, Heimdall's early Raine/Genis separation, the
Palmacosta ranch warning and academy response, post-exit sewer scenery, Clara's
Izoold restoration, Welgaia's companion-search setup and Altamira's ending music.
These bounds do not assert completion of the associated event. Welgaia's three
rescue helpers each add 1000, while their item flags identify which rescues took
place. Salvation 0096's word-game dialogue keeps its overlapping `<=22701999`
and `>=22701999` boundaries and original branch order.

`OssaEntryDispatchUpperBound` names 999999999 in the later scenery setup in
room 0350 and an empty retained dispatch branch in room 0347. It is a local
reader limit, not a maximum valid story value. The separate 1401000 equality
in room 0347 remains numeric: its body is empty and no event meaning has been
established. Naming the other bounds does not resolve that unknown phase.

`FlyingDragonWorldVisibilityUpperBound` at 12400000 names the existing authored
world rule's inclusive visibility limit after `RheairdsRecovered`. It replaces
the former `UnknownId12400000` label. No direct original script operand at that
value was recovered, so it has no fabricated original-PC evidence row. The
world rule still shows the blocked marker within its original range and hides
it outside; the normal rescue landing debrief uses 12401000 instead.

After exporting the corpus, inspect all setting paths and nearby dialogue with:

```sh
python3 tools/scenario_identity_context.py 553
```

For reused bits, limit the result to exact field directory names with repeatable
`--field` options, for example
`python3 tools/scenario_identity_context.py 150 --field house-of-guidance-0111`.
The tool still audits all original field occurrences before filtering the result;
an unknown field directory is an error rather than a misleading empty report.

Every occurrence includes its identity domain, resolution status, original input,
and source expression. Equal numeric bits from different scene namespaces remain
distinct in the console summary. `--domain story::StoryFlag --unresolved` selects
uses whose meanings still lack a supported name; combine it with `--field` and
`--all-uses` to inspect a particular scene's readers and clears. Unknown domain
names are errors. The JSON output retains these fields, including `identity_value`
and `bank_slot` for structural bank resets, so a bank-base reference cannot be
mistaken for an individually named bit. An explicit source enum from a different
namespace fails the audit even when its integer matches.

This research tool resolves symbolic and calculated operands and checks every
recovered field flag occurrence against the word-PC report or the message byte
offset report. It fails if occurrences are missing, distinguishes executable
calls from quoted dialogue and audits message bodies independently of tasks.
Use `--all-uses` with a focused flag query to include readers and clears. It includes declined tutorial prompts,
alternative companion scenes and copied unused events; a nearby item grant alone
does not prove a flag means that item was obtained on every route.

Ordinary literals use their smallest original-VM encoding. `s8(...)`, `s16(...)`,
`u16(...)` and `s32(...)` preserve a different original literal width where needed
for byte matching. Arithmetic retains wrapping integers, eager Boolean operators
and the original division/shift behavior. These differ from the authored field
and model profiles; a scenario does not silently adopt authored semantics.

In a `script scenario`,
`bound_before(story::StoryStage::RaineSearchWithMithosAgreed)` denotes one integer
below that supplied stage's value. It emits a single literal with no runtime
subtraction. The anchor and result must fit signed 32-bit integers; the minimum
signed value has no predecessor. Width wrappers work around the constructor.
The decompiler uses it for inclusive stage upper bounds and reversed `>=`
comparisons when an exact name is absent. It preserves `<=` rather than changing
the original calculator opcode to `<`. Exact names take precedence, and this
inference does not apply to assignments, equality tests or unrelated ID domains.

This is an arithmetic relationship to a known stage, not an invented event at
the preceding number or a claim that it is the previous quest step. A mod can
rename the anchor in its supplied definitions. Changing the anchor's value
changes an explicitly authored bound; decompilation uses the constructor only
when the actual original operand is exactly one below the selected anchor.

For a named ID originally calculated from several operands,
`encoded(story::StoryFlag::ManualMovementLearned, (100 + 4))` preserves those
original pushes and arithmetic instructions. The compiler verifies that the
expression produces the named value. Calls, memory reads and writes are rejected
inside this form. Editing its arithmetic requires updating the expected ID too;
replacing the whole form with a constant is valid when byte matching is no longer
required. Bitwise flag combinations name their individual operands while keeping
the original OR instructions.

An item or flag ID first assigned to a shared memory word can also gain a name
when its later native argument proves the domain within the same straight-line
region. For example, Latheon's `memory::i32[0x0A0C] = 115` becomes an assignment
of `items::ItemId::Kirima`. The native still reads the original memory word.
Pure constant arithmetic keeps its `encoded(...)` form. Calls, native calls,
branches, labels and overlapping writes end this inference; conflicting item/flag
uses found within a region keep the source value numeric. Names describe the proven use of a constant,
and leave the shared memory model intact.

Reviewed indexed flag families use a separate `StoryFlagBase` enum. Darkness's
five small Shadows use
`story::StoryFlagBase::TempleOfDarknessGatheredShadows + memory::i32[0x094C]`;
the original loop supplies offsets 0 through 4 for individual flags 623 through
627. Addition keeps its operand order, memory reads and literal widths, including
an `encoded(...)` form if the base was calculated. A family name requires an
explicit base in the supplied definitions; an equal-valued individual flag does
not establish a family or prove bounds for a modded index.
Overworld entry also explicitly clears three reusable banks, each with offsets
0 through 49: `WorldResetSceneBank150`, `WorldResetSceneBank1700` and
`WorldResetSceneBank1800`. The names describe those original reset loops; they
do not imply that the engine clears these bits on every field transition or
give the bits a single meaning across all scenes.

Complete, uninterrupted sequences of 50 literal clears of these reviewed banks
use `flag_slot(story::StoryFlagBase::WorldResetSceneBank150, 3)` for their operands.
This constant constructor emits a single push of 153; it adds no runtime operation.
Explicit width wrappers preserve the original literal encoding. Its two arguments
must be nonnegative constants whose sum fits an i32. The constructor does not
check bank length. Recognition requires all 50 clears in ascending order, supplied
base metadata and no control-flow entry inside the sequence. All original calls
and their order remain in the source. Partial resets and individual puzzle uses
retain their existing names or numeric operands.

`eval expression;` emits an expression and its original expression-end operation.
A native expression can be nested inside another call or arithmetic. A standalone
native statement emits only that call; a returned value remains on the original
operand stack. Rare sequences with stack values crossing control-flow edges use
`push`, `argument`, `result`, `if_false` and `vm::calculate`, `vm::argument` or
`vm::native`. They are explicit source operations with the original stack effects.
`flags(Enum::First, Enum::Second)` combines constant masks into a single literal;
it does not insert a runtime OR. `stack_depth()` preserves the retail interpreter's
empty-stack depth alias in the few original attachment calls that use it.

Recognized auxiliary message/name tables use readable Shift-JIS text and
relocatable byte offsets:

```rust
auxiliary:
data {
    relative32(greeting, auxiliary);
    relative32(bone_name, auxiliary);
}
greeting:
data { sjis("Welcome, Lloyd.\n"); byte(0); }
bone_name:
data { sjis("Bone_atama"); byte(0); }
```

`sjis` encodes its quoted UTF-8 source as Shift-JIS bytes, without appending a
terminator. Text that cannot be represented is a compile error. Ordinary quote,
backslash and newline escapes work as they do elsewhere in `.sym` files.
Editing text length relocates every declared table entry. `relative16(target,
base)` and `relative32(target, base)` emit unsigned big-endian **byte** offsets;
targets may be odd byte addresses. The compiler rejects missing labels, targets
before their base and offsets exceeding the selected width.

Message substitutions and embedded calculations use the same original-VM
expression syntax inside `message_control`:

```rust
data {
    sjis("Found ");
    message_control(scenario::MessageControl::ItemName, items::ItemId::AppleGel);
    sjis("!");
    byte(0);
}
```

The constructor writes the control byte, its expression, expression-end and
terminator instructions. It supports the eleven original controls. Character,
item, packed-title and technique substitutions receive their respective ID names;
`CharacterName` also accepts the original controlled-player selector. Number,
delay, color, input and voice operands keep their own interpretations. `Evaluate`
executes its expression without adding text; `Control7` retains its unresolved
formatting role. Native calls and memory effects keep their original order.
Explicit literal widths and `encoded` constant trees preserve matching bytes.
Malformed expressions and unrecognized control bytes are compile errors.

Native dialogue waits observe the current physical window in the selected slot.
There are three windows, `Main`, `Secondary` and `Auxiliary`. The original helper
compares the full signed wait selector: every value >=3 falls back to `Main`,
without byte narrowing. Hot Spring 0089's `MainFallback8` waits for the main
window to reach status 5 or 6; Iselia Human Ranch 0193's `MainFallback20` waits
for status 0. The separate names retain their exact original encodings. They are
wait selectors: `configure_dialogue` instead rejects slots >=3.

`DialogueClosed` tests status 0, whereas `DialogueReady` tests status >=5. A
closed or missing window does not satisfy `DialogueReady`. Replacing a window
while a script is waiting makes subsequent polls observe the replacement window,
using the same slot. Original Native64, the full dispatcher, its window resolver
and original jump table were executed without patches for 154 captured cases;
the runtime regression covers the same selector/status arrangements.

`DialogueGrid::DefaultCenter200` preserves Meltokio 0409's out-of-grid operand.
Original dialogue configuration uses the fallback position (320,240) for values
outside 0..8. This differs from the ordinary `Center` cell at (318,240).

Dialogue expressions invoke the same registered native services as event code,
including flag reads, actor geometry and session queries. Arguments evaluate in
order and returned values update the original shared result cache. Original
field `get_actor_property` branches that push no value remain distinct from
branches returning zero. In the retained Ymir debug messages, an existing actor
with selector999999 leaves the terminal expression depth at -1, so the number
control displays -1; a missing actor returns zero. This compatibility behavior
does not relax authored native result signatures. Unsupported nested missing
results and suspending dialogue calls fail explicitly.

Dialogue `message_control(scenario::MessageControl::Voice, ...)` also names
archive/member references through `voice_archive`. Playback consumes the member
word, unlike preloading. Zero uses `scenario::VoiceControl::None` and does not
replace an active voice; -1 uses `VoiceControl::Stop`. Direct-stream references
whose high nibble is8 keep their original numeric encoding instead of receiving
an archive identity. Number controls containing the same integer remain numbers.
Voice identities are reported at their original expression-end byte addresses,
including odd file offsets. Archive tests check every distinct referenced member
against original AFS indices and AHX headers on both discs, with each archive
pinned by SHA-256.

`message-identity-uses.tsv` audits these expressions separately: `byte_offset` is
an absolute file byte address, including odd addresses, and `control` identifies
the enclosing message control. `message::value` refers to the expression's final
expression-end instruction; other rows refer to their embedded native call.
The per-domain `message-identity-coverage.tsv` includes unresolved values rather
than inferring a quest meaning from a generic debug read. Across code and message
expressions, all 350 observed item IDs are named; message substitutions add the
otherwise unobserved `FlareBottle` and `SyrupBottle` IDs.

The remaining unnamed message operands have two specific sources: the generic
field debug menu prints 366 flag values (341 distinct bits), and Ymir Forest
0519/0520/0521 retain twelve reversed `get_actor_property` calls whose property
operand is 999999. Those invalid selectors keep their original encoding and
no-result behavior. The debug display does not establish scene-specific flag
meanings. These message occurrences are separate from the fifteen uses in main
script code of the nine unnamed scene flags described above.

Recovered tables retain their original width, ordering and duplicate entries;
aliased entries continue to point at the same labeled body. Text extraction uses
the message parser's control-expression boundaries. Supported embedded VM
expressions use `message_control`; formatting controls, padding and terminators
remain explicit bytes. Text edits preserve substitutions and their evaluation. Invalid or noncanonical
Shift-JIS sequences retain `hex` data unless decoding and re-encoding reproduce
the same bytes. Unrecognized tables and malformed message bodies also retain
their explicit data. Label `aux_0032`, for example, identifies the target of
auxiliary slot50; a name query's numeric `name_index` still selects that slot.

`data` blocks retain auxiliary tables and unused or unknown data. Resources with
overlapping headers/code or other unusual registry layouts also retain an
explicit header and `code_base` instead of an `events` declaration.
`pc(label)` and `pc32(label)` relocate word PCs relative to `code_base`;
`offset(label)` relocates an absolute file word offset. Event registry order,
duplicate keys, padding, original encodings and overlapping layouts are retained.
The compiler relocates these declared references when edits change code size.
Opaque auxiliary bytes remain explicit data; custom pointers added inside them
must be declared as references if they need relocation.

To inspect an existing binary or export source back to the original format:

```sh
cargo run -p resonance-script -- scenario decompile scenario.ssb scenario.sym --assets local/cooked
cargo run -p resonance-script -- scenario compile scenario.sym edited.ssb --assets local/cooked
```

Without `--assets`, these commands generate the standard definitions from Rust.
With it, they use the specified library's actual `.sym` definitions. Use the same
current cooked library for both commands so edited aliases stay consistent.
Decompilation checks that compiling its source reproduces every input byte before publishing.
Compilation validates the resulting VM program and writes atomically; failures
leave an existing output intact. The lower-level `scenario disassemble` and
`scenario assemble` commands remain available for `.ssasm` inspection.

For a whole cooked binary corpus, including per-file verification and ID reports:

```sh
cargo run -p symphonia-script-tools --example decompile_corpus -- local/all-assets local/readable-scripts
```

This writes the generated standard definitions beside the recovered sources and
records a byte-matching result for every script in `verification.tsv`.
`identifiers.tsv` records each constant identity operand's domain, native parameter, value,
symbolic/unresolved status, use count and an example PC. It excludes ordinary
coordinates/counts and scene-local handles. This makes the remaining unresolved
identities inspectable rather than concealing them behind guessed names.
`identity-uses.tsv` retains every typed operand's original file and PC.
`memory::battle_outcome` entries record proven battle-result comparisons. Their
PC points to the original 32-bit load, and `0x24 from battle/0x20` records an
equality test of the shared result word. A copied result instead records its
temporary address. The original-operand audit checks the literal and calculator
opcode and recomputes the control-flow proof on the original binary.
Entries with native `memory::constant32` also audit direct typed-global
assignments and comparisons: the PC points to the 32-bit load, and the parameter
records the memory address and calculator opcode, such as `0x40/0x10` for a
main-story assignment. These require an adjacent literal/calculator pair with no
incoming label between them. Arithmetic deltas, narrow loads and plain counters
are excluded. More complex expressions and copied stage-dispatch snapshots are
not covered by that direct-global report. A symbolic status means an enum
reference or checked `bound_before(...)` formula exists; an `UnknownId...`
placeholder still needs semantic research. Derived-bound records retain the
actual emitted bound in their value column, not the anchor's value. They do not
add an enum discriminant for the preceding integer.
Entries with native `memory::snapshot32` separately audit comparisons through
proven copies of globals. Their PC points to the temporary's 32-bit load, and
`0x0 from 0x40/0x23` means a comparison with the snapshot in word zero, originating
from the story word, using calculator `>=`. An adjacent literal/calculator pair
is required for this report; more complex rendered comparisons are not included.
The original-instruction evidence audit rechecks the load, literal and operator,
and recomputes the origin proof against the original program's control flow.
Snapshot reporting can expose additional unresolved comparison values; it does
not invent missing event names or assert equality with the current global value.
Entries with native `memory::copy` additionally record an item/flag constant
traced through shared memory, its defining instruction PC and its native use.
Entries with native `memory::flag_constant32` instead record an individually
reviewed flag-ID assignment used by shared helpers. Its PC is the destination
word load; the parameter retains the byte address and assignment opcode, such
as `0x8c0/0x10`. It does not claim a general interprocedural type for that cell.
Entries with native `indexed::...` record a declared flag-family base at its
consuming native call. Their `story::StoryFlagBase` domain keeps these dynamic
expressions separate from resolved individual flag values in the coverage report.
Field-specific families use explicit `story::FieldNNNNFlagBase` declarations in
the selected definitions. They apply only to additive flag arguments in that
field; equal literals, item arguments, other arithmetic and other fields do not
inherit the family meaning. Omitting the declaration leaves the base numeric
unless the selected definitions separately declare a general `StoryFlagBase`.
Renaming a member is supported, including a mod's own field family declarations.
Nested offsets retain their original additions, for example
`((PartyAssignmentBank + character_id * 3) + 2)`. The renderer follows only
additive chains with one constant side at each step. It does not move constants,
reassociate the tree or cross multiplication, subtraction or two dynamic sides.
An explicitly declared outer base keeps precedence over a nested candidate.

Thirteen fields in the Iselia Ranch, Asgard Ranch/base and Tower of Mana party
protocols declare `PartyAssignmentBank = 150`. Their 312 computed team/slot
arguments keep the original triplet layout: at `150 + character_id * 3`, offset
zero selects the team and offsets one/two encode slot weights two/one. Iselia
uses Reactor/Rescue teams and slots 0..3; Asgard and Mana use A/B teams and slots
1..3. Iselia's decoder skips Sheena, while Asgard/Mana decoders skip IDs 6/7/8
whose triplets overlap saved-leader storage. The bank names do not change those
exclusions or repair Asgard 0214's mismatched low-bit save.
Two additional uses in Mana 0361 reset offsets 0..29 of the bank; these remain
resets rather than individual character assignments. The names describe the
field-local bank and do not assign a character meaning to every bit. Original
image hashes, all 314 bank calls, the Iselia slot encoder/decoder, Mana's reset
bounds and the retained Asgard mismatch are checked by an original-data fixture.

`StoryFlagBase::DogNamingBank` names 1214 in the thirty-flag dog-naming family
(Bob through Cammy). Thirty-two completion helpers sum offsets 0..29 and check
for a total of 30 before awarding Colette's Dog Lover title, with a separate
already-owned check. Thoda 0006 also retains a debug choice that sets all thirty
flags; this does not replay the individual naming conversations. The title,
affinity adjustment and other rewards remain separate calls.

The sixteen mapping-service copies use `LongRangeSylvarantBank` (900) and
`LongRangeTetheallaBank` (911). Their original loops set 11 and 10 flags,
respectively: 900..910 and 911..920. The second loop leaves 921 and 922 untouched;
the family name does not claim that every Tethe'alla flag was unlocked. The
native region grid and stone table retain their distinct layouts. These three
family names cover 65 computed calls in 41 scripts. An original-data fixture
pins every source image, all call counts, dog-count/title guards and map-loop
bounds, and verifies matching recompilation. Individual dog and region flags
keep their own names; omitting a base from selected definitions restores numeric
additions without changing their bytecode.

`Field0130FlagBase::BlockTransferPending` and its room-0135 counterpart name the
base 164 in the Derris Kharlan block-transfer protocol. The hub loops over block
indices 0..7 (actors 1000+index), marking a transfer at either inscription door.
Room 0135 consumes the corresponding flags into actors 3000+index and clears
each consumed mark. Side flag 172 independently chooses X -1282 or 1282; its
clear remains inside the future-side branch, so multiple queued blocks retain
the original ordering-dependent behavior. These marks are separate from the
permanent placed-block flags. They do not identify bit 171 in Flanoir or any of
the other unresolved flag meanings. The original-image fixture pins both script
hashes, loop bounds, index/actor/resource expressions, consuming calls and clear
order, and recompiles the resulting source to identical bytes.

Entries with native `bank_reset::clear_event_bit` record a complete bank reset's
base and a `bit[slot]` parameter. Adding that slot to the recorded base reconstructs
the original operand. These entries describe storage slots, so they do not count
as recovered meanings for individual quest flags.
Scene aliases appear under their `story::FieldNNNNFlag` domain. Naming bit 150
in one field does not resolve that bit's remaining numeric uses in other fields;
compare use counts and field domains rather than counting all aliases as newly
discovered global story bits.
`identity-coverage.tsv` separates unique values and use counts by domain, so
frequent native selectors do not hide gaps in story-flag coverage.
A value with any unnamed use counts as unresolved even if another comparison
can name it through `bound_before(...)`. Named-use totals count individual uses;
they do not treat every occurrence of that number as named. A partially named
mask counts separately when there are no wholly unnamed uses of that value.
Runtime mods should override the
source path selected by the owning resource, rather than install this report.

## Verified item and story-flag scope

Item-ID naming and reviewed story-flag readability are complete for the supported
GQSEAF corpus, with the numeric exceptions below. Further recovery of those
meanings is optional research, not a requirement for editing or matching
recompilation. Unknown values remain usable numbers; equal values in unrelated
fields do not justify transferring a name.

The verified deliverables are:

- **953 original scenario owners published as readable `.sym` files.** Their
  compiled bytes match the normalized originals (17,811,493 bytes), using only
  the definitions published beside them. The cooked library has no `.ssb`
  alternative roots.
- **350 observed item IDs named**, covering 7,692 main-code operands and 4,401
  message operands. Message substitutions include the otherwise unobserved
  Flare Bottle and Syrup Bottle IDs. This is observed-corpus coverage, not a
  restriction on mod-added item IDs.
- **Nine unproven story-flag values retained numerically at 15 main-code uses.**
  The table identifies every use, including retained/debug routines. PCs are
  original word offsets relative to each scenario's code base.
- **Matching before gameplay corrections:** 998 extracted scenario references
  across both discs round-trip exactly to 953 owners (17,822,127 input bytes).
  The six existing field corrections remain separate from decompilation.

| Value | Field | Original word PCs and operations | Established context; remaining uncertainty |
|---|---|---|---|
| 152 | Tethe'alla Base 0254 | clear `0x2A99`; set `0x2AC7` | Debug all-off/all-on choices; no recovered gameplay reader establishes the set-state meaning. |
| 153 | Tethe'alla Base 0254 | clear `0x2A9E`; set `0x2ACC` | Same debug choices; its gameplay role is unproven. |
| 155 | Tower of Salvation 0502 | clear `0xF47`, `0xF79` | Two exit handlers; the corresponding set-state meaning is unproven. |
| 171 | Flanoir 0317 | clear `0x44B3` | Retained debug refresh; no matching invitation-dispatch writer/reader establishes its role. |
| 191 | Heimdall 0482 | test `0x4BA`; clear `0x4C4` | Retained conditional cleanup; the condition alone does not identify its set-state meaning. |
| 192 | Heimdall 0482 | test `0x4CB`; clear `0x4D5` | Same cleanup helper; no established flag-specific role. |
| 193 | Heimdall 0482 | test `0x4DC`; clear `0x4E6` | Same cleanup helper; no established flag-specific role. |
| 198 | Altessa's House 0139 | clear `0x72CB` | Retained recovery conversation; its set-state meaning is unproven. |
| 199 | Altessa's House 0138 | clear `0x5E93` | Retained Ozette Cold departure; its set-state meaning is unproven. |

The separate message audit retains 366 generic debug reads of 341 unnamed flag
values. A generic display does not establish scene-specific meanings. Twelve
reversed Ymir Forest 0519/0520/0521 property calls also keep their original
invalid selector 999999 and no-result behavior. These are preserved original
operands, not missing item names. Coordinates, local handles, counters, explicit
VM operations and other unreviewed domains are outside the item/flag naming
claim; readable source need not invent a semantic name for every integer.

The exhaustive coverage gate reads every original normalized scenario, recompiles
it and requires exactly the item counts and numeric exceptions above. It does
not trust generated coverage reports. The published-source gate independently
compiles the cooked `.sym` files using their editable cooked definitions:

```sh
cargo test -p symphonia-script-tools --test identity_evidence --offline original_item_and_story_identity_coverage_has_only_reviewed_gaps -- --exact --ignored --nocapture
cargo run -p symphonia-script-tools --example verify_cooked_sources --offline -- local/all-assets local/cooked-sym126
cargo test -p resonance-importer --lib --offline scripts::tests::original_scenarios_roundtrip_before_gameplay_corrections -- --exact --ignored --nocapture
```

The reviewed meanings retain their original references in
[`scenario-identity-evidence.tsv`](../crates/symphonia-script-tools/data/scenario-identity-evidence.tsv).
The bounded original-image investigations for the nine unknown values live in
[`native_story_flags.rs`](../crates/symphonia-script-tools/tests/native_story_flags.rs).
Neither those bounded scans nor this closeout asserts that every native write
or transitive cross-field path has been recovered.

## Verification

```sh
cargo test -p symphonia-script -p symphonia-script-compiler -p symphonia-script-vm -p symphonia-script-tools -p resonance-events
cargo test -p resonance-game --test classroom_script --test new_game_setup -- --ignored
```

The second command needs locally cooked assets; `RESONANCE_TEST_ASSETS` selects
another cooked directory. These tests open no audio or graphics devices. Tests
cover enum/catalog agreement, registered signatures, duplicate registration,
borrowed host state, nested arguments, immediate/deferred results, unsupported
calls, stack/memory limits and the original classroom/setup events. Original scenario
`.sym` files compile to the original VM instruction encoding. Preparation captures
the selected source and constants and checks the resulting program structure; there is no comparison
against a mandatory cooker-recorded hash. Derived identities track gameplay for
save admission and internal cache reuse.

To audit item and story coverage directly from the complete normalized binary
corpus in `local/all-assets`, without trusting generated identity reports:

```sh
cargo test -p symphonia-script-tools --test identity_evidence --offline original_item_and_story_identity_coverage_has_only_reviewed_gaps -- --exact --ignored --nocapture
```

This decompiles and recompiles all 953 inputs, requires names for all 350 observed
item IDs and checks every remaining unnamed scene flag against its original file
and word PC. Unnamed message operands must stay within the two reviewed groups
above. A new unnamed operand outside those groups fails the audit.

To check the bounded native reader, manual-table and restore-loop evidence
against the original GQSEAF executable in `local/extracted`:

```sh
cargo test -p symphonia-script-tools --test native_story_flags --offline -- --ignored --nocapture
```

To verify matching directly from extracted GameCube inputs, before gameplay
corrections or any previous cooked publication:

```sh
cargo test -p resonance-importer --lib --offline scripts::tests::original_scenarios_roundtrip_before_gameplay_corrections -- --exact --ignored --nocapture
```

This reads `local/extracted` (or `RESONANCE_TEST_EXTRACTED`) and checks the original
501 field scenario members, all 487 skit packages, the world script and all battle
programs. It verifies 998 original references covering 953 published owners;
aliases share published skit and battle files. It also checks that the existing
gameplay corrections affect exactly the six fields listed above. The audit does
not write to the extracted inputs or cooked library.

For the supported GameCube input, both discs contain identical scenario inputs.
The following faster audit compares the executable, all field and skit packages,
the world script and battle programs directly across the two discs (999 files):

```sh
cargo test -p resonance-importer --lib --offline scripts::tests::original_disc_script_inputs_are_identical -- --exact --ignored --nocapture
```

It checks both file inventories and contents. Together with the original-input
round trip above, this verifies matching for the scenario inputs on both discs.
