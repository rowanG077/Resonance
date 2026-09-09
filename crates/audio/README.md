# Musical synthesis

`resonance-audio` renders typed scores and instrument operations into stereo PCM.
The importer, player and offline recorder share this core. It owns no device,
thread, original bank reader or window.

`package` validates versioned JSON and mono sample WAVs, including hashes, loop
ranges and references. Voices borrow shared samples. The resumable sequencer
emits 160-frame blocks at 32028 Hz, retaining fractional clocks, held notes,
controller state, voice allocation, modulation and reverb across loops.

`cue::Studio` applies live gain to envelope PCM and typed controls, then accumulates
voice buses through shared effects before saturation. Baking gain or quantizing
each wet tail separately changes fades and overlapping cues. Unsupported
instruments/controllers fail explicitly; current coverage is not full-game coverage.

See [audio compilation](../audio-cook/README.md),
[playback ownership](../../docs/audio-video-architecture.md), and
[offline comparisons](../../tools/oracle/README.md).
