# Audio compilation

This Rust crate reads original `.snd`/`.song` resources, decodes ADPCM, and
compiles instruments, layered notes and controls into
[`resonance-audio`](../audio/README.md) packages. It also cooks effect envelope PCM
and gain automation. Original readers are importer-only; the player loads the
versioned cooked packages.

| Modules | Responsibility |
|---|---|
| `bank`, `dsp`, `parameters` | Checked resource tables, samples and audio parameters |
| `song` | Arrangements, patterns, controller streams and event ordering |
| `instrument`, `compile` | Layer selection, musical macro validation and typed operations |
| `render` | Effect envelope PCM, gain controls and diagnostic rendering |

Supported music includes source modes 0/1/2, ADPCM loop restoration, table-based
pitch, ordinary/DLS envelopes, note-off handling, vibrato/tremolo and shared
reverb. Relative beat waits retain fractional deadlines; two independent pitch
sweeps add before pitch quantization. Loops preserve held-note clocks, controls
and effect tails. Random/absolute beat waits, surround macros,
sustain-pedal sequencing and unsupported macros/controllers fail explicitly.

Relative millisecond random waits use one shared synthesizer RNG and ordered
1ms control passes. Cooked scores retain sequence versus sound-effect origin so
timed and looping music, layered cues, callback wakes and exclusive groups share
the native admission order. Isolated preview rendering still rejects random
programs and child macros; those require the shared synthesizer. Child macro
commands retain their entry point, signed key offset, priority and voice limit.
Cooking includes present child dependencies and validates their entry points;
a missing optional child remains a failed spawn.
Self/last-child handle reads, targeted/broadcast messages, mailbox reads and
message traps compile to typed operations. Present trap dependencies are included;
missing optional targets preserve the existing trap.
Variable assignments, saturated arithmetic and conditional branches retain typed
local/global/controller operands. Paired controllers preserve 14-bit values,
including fractional writes; LFO reads are read-only. RPN data entry, the second
oscillator and unsupported switch/source-selection operations still fail explicitly.
Physical sample commands retain their offsets; standard ADPCM playback
ignores those offsets, matching the original loader and DSP setup.
Music setups retain their bank group, and note events retain typed program/drum
or sound identities. The shared synthesizer uses them for source limits and voice
stealing; diagnostic previews still reject slot exhaustion.

DSP interpolation coefficients are explicit cooking inputs, with hashes recorded
in the recipes.
Sample WAVs preserve independent initial and loop traversals. Preview rendering
uses the same synthesis core as runtime; it cannot establish physical device
latency or full-route audiovisual timing.

See [offline media tools](../../tools/media/README.md) for codec dependencies.
