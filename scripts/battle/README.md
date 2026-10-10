# Battle scripts

No authored battle scripts remain. Ordinary encounters
prepare native records for normal attacks, maintained martial techniques, Special
Guard, supported enemy attacks, casting, Fire Ball, and result notices. Lightning
and Nurse also have native releases used by integration scenarios. Battle
preparation consumes native definitions without a script compiler cache or named
resource declarations. The battle VM and compiled battle fixtures are gone;
SymphoniaScript supports field and model hosts only.

Rust owns admission, companion policy, conditions, spell lifetimes, and encounter
progression. Each encounter prepares the assets needed by its party and enemies.

Actions should express gameplay intent directly. Prefer named operations and
shared parameters over numeric command sequences. Imported resource IDs belong
at the preparation boundary. Internal task layout and update ordering may change
when supported gameplay remains correct.

Casting commits TP when the action releases its spell. Released spells, effects,
and particles can outlive the actor task that created them. Cleanup must respect
those owners when an actor is interrupted or a battle ends.

`audio-requirements.json` lists the sound and voice resources required by the
currently prepared actions. It is an asset inventory, not a guarantee that every
technique or encounter is implemented.

Test observable outcomes and a few representative action sequences. Avoid tests
that merely restate internal command order or require obsolete implementation
quirks. See [battle support and validation](../../docs/battle-status.md) for current
limits and the behavior checks being rebaselined.
