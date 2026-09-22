# Musical synthesis

`resonance-audio` renders typed scores and instrument operations into stereo PCM.
The importer, player and offline recorder share this core. It owns no device,
thread, original bank reader or window.

`package` validates versioned JSON and mono sample WAVs, including hashes, loop
ranges and references. Voices borrow shared samples. The resumable sequencer
emits 160-frame blocks at 32028 Hz, retaining fractional clocks, held notes,
controller state, voice allocation, modulation and reverb across loops.

`sequence::shared::Synthesizer` supplies a single RNG and ordered control clock
for supported sound cues, including exclusive groups across cues and their
release callbacks. Create streams with `Stream::in_synthesizer`,
set their controls, advance the synthesizer once per mixer frame, then read their
separate buses. Cues enter the next unrendered DSP block. Cooked scores distinguish
sequences from sound effects: SFX allocate in request order, then sequences run
newest first, including timed events and loops. Isolated preview streams require
deterministic programs; random programs, child macros, messages and global variables
need the shared synthesizer. Each voice starts with 16 zeroed local variables;
16 global variables persist across cues. Registers preserve 32-bit handles and
messages. Arithmetic reads and saturates signed 16-bit operands; branches compare
the full signed register values. Controller operands use canonical 14-bit values;
sequence voices share MIDI channel controls, while sound-effect voices have
independent controls. Package version 2 preserves these values and rejects older
packages; regenerate prepared audio after updating the cooker.

The shared pool has 64 slots with sequence/SFX limits of 42/22. Typed instrument
and sound identities enforce source limits across cues. Stealing respects priority,
fractional age and allocation-list order; slot generations isolate replaced voices.
Child sound macros copy volume, pan, reverb, pitch and surround controls plus
source limits, begin on the next control
pass, and can outlive their parent. Allocation protects the running parent.
Self/last-child handles support cross-cue messaging, four-entry mailboxes and
one-shot message traps. External stream reservations, host message callbacks and
traps that resume a freed voice slot remain unsupported. Standalone diagnostic
previews still reject slot exhaustion.

`cue::Studio` applies live gain to envelope PCM and typed controls, then accumulates
voice buses through shared effects before saturation. Baking gain or quantizing
each wet tail separately changes fades and overlapping cues. Unsupported
instruments/controllers fail explicitly; current coverage is not full-game coverage.

See [audio compilation](../audio-cook/README.md),
[playback ownership](../../docs/audio-video-architecture.md), and
[offline comparisons](../../tools/oracle/README.md).
