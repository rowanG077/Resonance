# Battle source

These maintained sources use `script battle;` and the shared SymphoniaScript VM.
They are embedded and published unchanged. Executables are compiled only from the
verified cooked snapshot during battle preparation and remain in memory.

`nurse::recover` translates only the age-120 recovery portion of `fn_1_60510`.
Returning this task leaves the enclosing resident action alive until age 250.
Stored-spell entry, models, recipient tint/voices and ordinary Raine/Meredy entry
are pending. No encounter currently binds this callback for playable admission.
See [battle status](../../docs/battle-status.md) for scope and source identity.

Resident initialization leaves age zero; its first active callback follows on the
next update. Primary and secondary spell slots are independent of caster tasks.
Caster admission checks TP; the authored caster pays at its commitment point.

`casting::ordinary` implements the ordinary casting countdown, automatic early
release pose, manual fallback, TP commitment, animation completion and recovery.
Its `Effects` parameter supplies pulse/release members, scale, tint and release
sound. Source emits the ordinary eight-callback pulse in the late effect group,
follows the caster for subsequent emissions and stops new pulses when entering
release. Once the release motion finishes, it requests burst 7/8, sound 123, then
the spell. Existing effects and particles survive release or caster interruption.

`casting::prepared` consumes verified technique/profile parameters and runs
`casting::chant` beside the countdown. Genis uses the three original motion rows,
retaining row-clock holds; other ordinary party profiles use one repeating pose.
`genis_lightning::run` binds this preparation, the resident spell and sounds.
The loader verifies cost, clips and element tint before activation. Casting
modifiers, held/stored casting, voices and full rendering/audio remain pending.
No live encounter binds these helpers yet.

`lightning::release` translates ground-origin capture and the age-20 projectile
emission. The projectile owns its clock after emission, independently of the task.
The real encounter resource binding, caster route, effects and damage remain pending.
The declared projectile path is a preparation requirement, not an embedded fallback.

`normal_lloyd` maintains all seven original Lloyd selectors as readable tasks:
`neutral`, `rising`, `thrust`, `low`, `finisher`, `aerial_slash`, and `aerial_thrust`.
Each spawns its own hit-window task. `battle::hit_window` takes a prepared
`battle::Melee` asset, start and inclusive duration, using the actor's separate
hit-stream clock. The loader resolves body/weapon attachments to evaluated pose
anchors. Finisher windows retain the original end-update clock hold. Each parent
selects the original motion blend/start/rate, movement and ordered voice/sword
requests, then enters recovery at the descriptor duration. Rising sets vertical
speed 19 at age two and dispatches clip 16 at age forty. The first neutral,
thrust and low attacks retain a faster approach speed; chained attacks and the
finisher replace that speed, as `3DA00` passes to command dispatcher `2C8B0`.
`battle::normal::lloyd_bindings` and `lloyd_melee` prepare all seven selectors in
one generation with distinct contact definitions. Neutral and finisher's first
contact share identical parameters; their hit windows remain separate source.
Recovery cancels remaining children and holds action age while its countdown runs.
Grounded normal recovery adds four updates per completed chain link in the source
helper `recovery_ticks`; airborne recovery retains the descriptor duration.
`battle::sound` takes a prepared `battle::Sound` asset and a byte priority. It
emits an ordered presentation cue at the actor's current position; presentation
owns playback and already-started sound tails. Control admission, chaining,
landing and idle handoff belong to the shared normal controller. Sequence tests
without that controller prove the authored streams only; they do not establish
playable aerial or directional-input fidelity.

Check these with:

```sh
cargo run -p resonance-script -- check scripts battle::nurse
cargo run -p resonance-script -- check scripts battle::lightning
cargo run -p resonance-script -- check scripts battle::normal_lloyd
cargo run -p resonance-script -- check scripts battle::casting
cargo run -p resonance-script -- check scripts battle::genis_lightning
```

`demon_fang::attack` and `ray_thrust::attack` maintain the opening martial
sequences for Lloyd and Colette. Their admission costs and projectile hit rules
come from the verified technique/action tables. The source tasks commit TP,
select motion and braking, issue the original ordered commands, emit the
projectile and enter ten-update recovery. Martial completion compares after the
clock increment, so these tasks enter recovery at ages 43 and 94 respectively.
Demon Fang's startup timeline uses `attach_effect`: the actor owns its pending
emissions, starts visiting it on the next callback and holds it during hit-stop.
Its command sets the second blade timer to 90; the shared trail object retains
and fades its sampled history after that timer expires. Ray Thrust's projectile
birth effect uses an independent model instance of Colette's equipped ring.
