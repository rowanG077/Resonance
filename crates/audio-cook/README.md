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
reverb. Loops preserve held-note clocks, controls and effect tails. Voice stealing,
sustain-pedal sequencing and unsupported macros/controllers fail explicitly.

DSP interpolation coefficients are explicit cooking inputs, with hashes recorded
in the recipes.
Sample WAVs preserve independent initial and loop traversals. Preview rendering
uses the same synthesis core as runtime; it cannot establish physical device
latency or full-route audiovisual timing.

See [offline media tools](../../tools/media/README.md) for codec dependencies.
