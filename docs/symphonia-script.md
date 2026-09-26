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
    await field::wait_ticks(ticks(20));
}
```

Module names follow logical paths: `field::start` is `field/start.sym` under the
source root. `use` imports modules or public declarations. Imports are checked
transitively; cycles, unknown names and access to private declarations fail.

Every file starts with `script field;`, `script model;`, `script battle;`, or `script library;`
(comments may precede it). The checker selects the native API from this declaration,
and preparation rejects an entry for the wrong host. Field, model and battle scripts may
import their own kind or libraries. Libraries may import other libraries and use
the importing entry's native API. Check a library requiring host services through
its field/model/battle entry. Headers are required; there is no command-line mode override.

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
- Explicit `i32(...)`, `f32(...)` and `ticks(...)` conversions. Construct a strongly
  typed duration with `ticks(20)` or `ticks(count)`, including in constants.
  APIs taking `Ticks` require this type; integers are not implicitly converted.
  Negative durations, integer overflow and invalid arithmetic are rejected.
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
imported by any execution host.
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

## Battle sequences

`script battle;` selects the battle native API in `resonance-script check`.
`resonance-script api battle` prints its registered operations. Battle
modules can import other battle modules and libraries; field and model hosts reject
them. See [battle status](battle-status.md) for the implemented sequence operations,
load-time preparation and remaining combat work. `battle::lightning` captures a ground
point and emits a prepared projectile at age 20. Its projectile clock survives task
completion and interruption after emission. `battle::nurse` implements model
emissions, actor visibility, recipient effects before age-120 recovery and retained
scene lifetime, actor/stage tint, camera bounds and recipient voice requests.
The complete Raine entry loads in headless preparation; scene rendering and audio
playback remain presentation work.

`battle::retarget_unavailable()` keeps the sequence's living, non-petrified target,
otherwise selects the first available actor on the same side in roster order.
It updates and returns the sequence target, retaining the original if no candidate
exists. Lightning calls this once before capturing its ground origin.

Melee streams can use `await battle::hit_window(contact, ticks(start), ticks(duration))`
with a prepared `battle::Melee` asset. The interval includes its final tick. Its
clock is separate from action age and holds on each completed window; the next
window starts on the next eligible actor update. One task owns the pending window,
and cancellation removes it. Actor-phase tasks run before projectile updates and
contacts; resident spell tasks run afterward. Bindings choose that source-derived
phase during loading, not from render timing. The current Lloyd source maintains
motion bindings, hit streams, forward-speed commands, sound/voice requests and
recovery timing. Playback, combos, idle-pose transitions and full admission remain pending.

`await battle::animate(motion, ticks(blend), start, rate, repeat)` binds a verified
`battle::Motion` asset to the sequence owner. Start/rate are native clip frames,
not simulation ticks. A cross-fade holds its first sample while its weight runs
from `1 / (blend + 1)` through `blend / (blend + 1)`; actor commands and their
clocks wait until the blend clears. `await battle::animation_end()` waits for the
current body motion's completion flag. Cancellation removes these VM waits.
The battle owns sampling, hurt points, rigid weapon anchors and immutable model
frames; presentation does not advance the animation or calculate contacts.
Independent animated weapons, bone masks and full actor pause policies remain
tracked battle work.

`battle::forward_speed(value, minimum)` sets the current forward velocity; when
`minimum` is true it only raises that velocity. `vertical_speed(value)`,
`acceleration(value)` and `gravity(value)` change the corresponding motion values.
These operations require an actor sequence. Continuous movement follows the
actor's commands and continues while an animation blend holds those commands.
Local hit-stop holds attack commands and movement while common timers advance.

`await battle::recover(ticks(duration))` enters action recovery, cancels the caller's
children and hit window, and holds action age. The separate countdown advances on
subsequent actor updates, including local hit-stop and animation blends. At zero it
waits for landing unless the actor has a hover height or fixed height. The task then
resumes and can call `battle::finish()`. Menu pause holds both clocks. This operation
does not choose an idle animation or an AI follow-up.

`battle::sound(sound, priority)` requests a prepared `battle::Sound` asset at the
owner's current position. Priority must fit in a byte. Requests retain script
order and consume no gameplay randomness. They follow their task's pause rules;
interrupting the task cancels future requests, while presentation owns playback
and the tails of sounds already started. A source sound index of zero is silent.

`battle::voice(actor, line, priority)` queues a prepared `battle::Voice` line and
returns whether the request was accepted. Bindings contain one optional line per
actor; unavailable lines are silent. Priorities range from 0 to 15. A higher
playing priority rejects the request; the last accepted pending request wins.
The common actor callback dispatches it, so a resident recovery request first
plays on the next actor visit. Cancellation does not remove a queued or playing
voice. Presentation acknowledges completed `VoiceId`s through
`BattleInput::voices_finished`; late completion of a replaced voice cannot stop its
replacement. Menu pause holds dispatch while playback acknowledgments still apply.
Nurse requests its non-caster recovery lines from maintained source.
`battle::voice_duration(actor, line)` returns the verified original duration as
`Ticks`; `battle::voice_idle(actor)` observes actual playback independently of the
pending request. The shared casting source compares its pre-decrement countdown
with that duration plus `ticks(20)`, remembers attempted counts across holds, and
only considers the fallback while audio is idle. Release voices use the same
operation. Nurse selects its original alternate chant when targeting herself.

`battle::armor(threshold)` sets the actor's action-armor threshold (0–255) and
clears its received-hit counter. It requires an actor sequence. Armored contacts
still deal damage; the contact that reaches the threshold remains armored, and
the next contact can interrupt the action. Enemy action recovery clears this
temporary armor; ordinary recovery, completion and explicit cancellation restore
the actor's base threshold. Released spell and effect tasks cannot change armor.

In `normal_lloyd`, `spawn neutral_hits()` lets the hit-window child run alongside
the parent's movement and recovery timing. `await neutral_hits()` would instead
wait for the whole hit sequence before continuing. To start a child and join it
later, use `let hits = spawn neutral_hits();` followed by `await hits;`.
`at_age(ticks(4))` waits for the shared action age to reach four;
`wait_ticks(ticks(4))` waits four additional action updates.

`battle::play_motion(motion, ticks(blend), start, rate, repeat, loop_start)` binds
a prepared motion without suspending the caller. Start and loop origin are
independent clip-frame values. `motion_is(motion)` queries the bound motion,
including a pending blend; `motion_duration(motion)` reads its verified duration,
and `animation_finished()` reads the sticky completion flag. `animate` retains
its blend wait. During casting that wait suspends its own task, while siblings and
the casting counter can continue. Normal attacks retain their command-wide hold.

Prepared casting entries use `ActionPhase::Casting`. They run with actor callbacks
through blends and local hit-stop; menu pause still holds them. They end explicitly
in source, without the attack/resident lifetime limit. `cast_remaining()` and
`set_cast_remaining(ticks(value))` access the actor's single casting counter, which
guard resolution also observes. During release this counter measures elapsed
waiting time. These operations require a casting actor and accept the original
nonnegative signed-clock range. `automatic_control()` distinguishes auto/enemy
control from manual/semi-auto control.

`casting::ordinary` maintains the implemented ordinary countdown, early release
pose, TP debit, motion completion, spell release, recovery and periodic pulses.
Its `Effects` parameter binds the pulse/release members, release sound, actor scale
and element tint. Release emits the burst, requests sound 123, then activates the
spell in that order. Pulses
begin on the first countdown callback and repeat every eight callbacks, including
occupied-slot holds; entering release stops new pulses.

A `battle::Casting` asset resolves the selected party profile, technique and body
model during preparation. `battle::cast_parameters(asset)` returns typed clock,
release-motion, pulse and element-tint parameters; `battle::chant_step(asset, index)` returns a
typed motion row. Motion references belong to the same prepared action as ordinary
`battle::Motion` assets. The loader checks admission cost against the technique and
verifies every referenced clip before activation. `casting::prepared` consumes
these records and runs `casting::chant` beside the countdown. Genis uses his three
original chant rows; ordinary party profiles use their single chant pose.
`genis_lightning.sym` supplies the Lightning bindings and initial sound request.
`raine_nurse.sym` supplies the verified stored-scene entry. Both entries bind chant,
fallback and release voices before activation. Modifier decisions, held release
and complete pause rules remain shared-mechanics work; complete rendered arte
acceptance remains pending.

`casting::stored` maintains the ordinary stored-spell countdown, entry motion and
sound, transition effect, TP debit, early release pose, activation and recovery.
`scene_available()` requires a free slot and no pending transition. A casting task
claims one of two slots with `begin_scene(spell, ticks(duration))`; the native call
does not suspend. Its continuation runs at the end of each update, after resident
dispatch, while actor callbacks, contacts, ordinary effects and particles hold.
The owner's model and late effects/particles continue. The first `next_update()`
after entry resumes at that end-of-update visit in the **same** simulation update.
Action age holds; `scene_remaining()` reads the separate transition countdown,
which decrements after each visit. At zero, `activate_scene()` releases the resident
and binds the slot's lifetime to it. Initialization waits until the next resident
pass. Recovery requires activation first.

Interrupting the pending casting action frees its scene slot. After activation,
the resident owns that slot independently of the caster. `hide_actors()` requires
an active stored resident and changes drawing visibility without holding combat
clocks. Active scene cleanup restores visibility, including interruption. The
Nurse regression uses the maintained scripts and original observed clocks; full
stored casting preparation still rejects missing scene controllers and resources.

`battle::Effect` assets resolve to prepared banks of effect programs. `show` starts
the selected member immediately through the same VM, with its own action age and
tasks. Later visits run in the effect group, independently of caster recovery or
interruption. Original `ef1` command records and modifier words are decoded into
this shared execution representation during preparation; authored programs remain
`.sym` source, and no executable is published by cooking.

`show_at(effect, member, point, heading)` emits at an explicit world origin and
heading, with unit scale and the calling actor as owner/target. `heading(actor)`
reads an actor's live heading in degrees. Nurse uses these operations for its
source-defined model arrangement. Prepared scene banks have independent model
instances in each active scene, while sharing immutable rig and clip resources.
Original modifier 22 translates to `play_effect_model(battle::EffectModel)` and
the model selector to `set_particle_model(particle, slot)`. Bindings are verified
before activation; playback and selectors require the owning scene and effect.
Models sample after the transition callback, once per initialized model-particle
visit. Transition holds retain the displayed pose even if a new clip is bound;
their world placement can still change. Scene cleanup releases its effects and
model particles without touching another scene or common-bank particle tails.

`show_following(effect, member, actor, scale, late, tint)` instead refreshes the effect's
origin from that actor on every timeline visit. Particles normally keep their birth
origin and heading. A declaration with `follow_origin` retains the effect's actor or
projectile attachment and refreshes its origin on each particle visit, independently
of effect completion or cancellation; its heading stays fixed. Emitting such a
particle requires an attachment. An expired projectile handle leaves its last
origin and cannot attach to a new projectile.
`show_centered(effect, member, actor, scale, late, tint)` follows the actor's sampled
profile center instead. The core scales and rotates the verified profile offset,
then adds the actor's root position before its callback and movement. That sample
holds throughout a stored transition, including the activation update. Petrified
actors still refresh it before their callbacks and timers are held. Nurse's stored
casting source uses this attachment for the transition effect.
`show_on(effect, member, actor)` also follows the sampled center, using that actor
as both effect owner and target, with its verified profile effect scale. It uses
the ordinary effect group and no element tint. Nurse calls it separately for each
eligible recipient before healing; its particle declarations still determine
whether individual particles retain a fixed birth origin or follow the center.
The actor's effect scale is independent of its model scale.
`tint_actor(actor, table, index)` copies a color from a verified `battle::ActorTints`
asset. Nurse selects entry zero after healing. During ordinary living actor
updates, RGB returns toward the prepared stage ambient color by one per channel.
Model presentation samples tint before actor callbacks, preserving the original
one-update delay. Menu pauses hold both; stored transitions hold color recovery
while models can still sample the held color. This operation does not draw RNG.
`tint_stage(channel, color, duration, step)` controls stage model colors. `color`
is a `battle::Color` record with byte-valued `red`, `green`, `blue` and `alpha`.
Channel zero is the ordinary scene color; channel one takes priority during entry.
Only the highest active timer advances. RGB approaches the selected target by
`step` per update; after expiry it approaches the stage's prepared default by two.
Color requests outlive the requesting task. A zero duration sets present model
colors immediately, including alpha. The core samples stage colors after object
updates and before resident callbacks, independently of actor tint sampling.
Update order is ordinary effects, ordinary particles, late
effects, then late particles. Newly emitted particles initialize when their group
is next visited, including late particles born during that update's late effect
pass. Both choices execute the constructor immediately and
set its target to the supplied actor, retaining the emitting sequence's owner.
Casting selects the late group and passes the prepared `EffectTint` in source.
The tint contains `enabled`, `palette`, `red`, `green` and `blue`; disabled tint
preserves the declaration. The verified element tables provide these values.

Effect programs can bind `battle::ParticleTemplate` assets and call
`spawn_particle(template)` to obtain a `battle::Particle` handle. Check
`particle_alive(handle)` before applying modifiers: exhausted object capacity
returns an inactive handle without consuming RNG. Handles belong to the emitting
effect; stale or foreign handles fault when used to change particles.
`particle_angles`, `particle_angular_velocity`, `particle_velocity` and `particle_size` read `battle::Point`
values; their `set_particle_*` counterparts change those vectors. Size operations
require size geometry. `effect_value(index)` and `set_effect_value(index, value)`
access four float values shared by that effect's tasks. `random_signed()` consumes
the original battle LCG and returns its signed upper half.
`apply_effect_appearance(particle)` applies the emitting effect's scale to the original
geometry-dependent fields, then its element tint to particles marked for tinting.
Tint replaces the first palette and second colour's RGB, preserving alpha. Original command translation calls it after modifiers,
before initialization; linear particle velocity remains unscaled.
Billboard trails keep their temporal radius speed separate from the size, position
and angle steps between drawn segments. `particle_segment_angle_step` and
`set_particle_segment_angle_step` read/write that angle as a float; they require
billboard-trail geometry. Original Nurse modifiers use these operations through
the shared VM to reverse the trail's spin and segment direction.
`set_particle_cull_back(particle, enabled)` changes the particle's back-face
culling request. Original declarations and Nurse's flag modifier supply this
presentation value; unsupported original flag writes still fail preparation.

Particles initialize in their declared ordinary or late update group. Their motion, colors,
geometry and inclusive lifetime run in Rust; they survive their emitting effect.
The prepared subset covers common controllers, UV rows, float modifiers and the
integer writes used by release bursts. Particles marked to draw after their target
retain that actor in presentation state; drawing order is independent of their
update group.
Bone attachments, retained-particle commands and complete
rendering bindings remain pending. Loading rejects unsupported commands/controllers.

A `battle::Spell` asset names a resident definition prepared in the same generation.
`release(spell, secondary)` returns false when the selected slot is occupied;
otherwise its independent sequence starts in the resident phase. Primary precedes
secondary and party precedes enemy, regardless of creation order. The initializer
keeps age zero, then the first active callback runs next update. `spell_active`
queries a slot. Ordinary released spells survive caster interruption and death;
`retain_resident()` may be called during resident initialization to retain slot
occupancy after the inclusive final callback. The next resident dispatch completes
it without resuming its tasks. Explicit interruption still releases it immediately.
Nurse uses this lifecycle; its scene slot and visibility are released with the
resident, independently of task or caster completion.

Action admission checks `tp_cost()` without paying it. Source calls
`pay_tp(amount)` once at the original commitment point. It returns false without
changing TP when live TP is insufficient; a second commitment faults. Released
definitions have no admission cost. Their resources and source must all prepare
successfully before activating the generation.

`battle::camera_bounds(ticks(duration), minimum_radius, minimum_pitch)` requests
shared camera bounds; radius uses world units and pitch uses degrees. Active
requests merge by taking the largest duration, radius and pitch. The camera owns
the request after the action completes or is interrupted. Its timer holds during
stored-scene entry and return, then resumes after the eye reaches ordinary framing.
It also holds with the battle menu. A camera must be prepared before invoking this
operation. `PreparedBattle::with_camera` currently prepares manual/semi-auto
framing; `BattleFrame::camera` supplies the resulting eye, focus, angles and radius.
