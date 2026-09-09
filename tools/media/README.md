# Offline media tools

The Rust importer owns parsing, cooking, validation and caching. `nix develop`
supplies these helpers; they are separate from player deployment:

| Tool | Use |
|---|---|
| HVQM4 | Decode original movie video |
| vgmstream | Decode original movie audio and speech |
| FFmpeg | Encode lossless cooked movies and inspect/compare media |
| KTX tools | Produce cooked KTX2 textures |

The player uses the shared Rust synthesizer, CPAL output and FFmpeg movie
libraries. It does not launch these converter programs. See [build/cook commands](../../README.md)
and [audio/video ownership](../../docs/audio-video-architecture.md).

Music cooking requires Dolphin's `Sys/GC/dsp_coef.bin` explicitly through
`--coefficients`; the importer verifies its expected hash. No recording is used
as a cooking input. Scores/instruments compile to typed operations and decoded
samples; effects retain gain controls and persistent shared reverb.

Movie recipes convert color/channel ordering, then encode FFV1 + FLAC in Matroska.
`cook-intro --audio-stream 1` selects stereo; stream 2 is mono. Manifests retain
source/recipe identity and intermediate hashes for reuse. The process runner
bounds logs/timeouts and reaps failed converters. Media cooks share
`local/cooked/.cook-media.lock`; remove a stale lock only after its process stops.

Use `target/debug/resonance-import --help` for offline inspection and rendering commands.
They write files without an output device. [Oracle instructions](../oracle/README.md)
cover PCM comparisons. The HVQM4 package and raw-output patch here provide planar
video frames to the Rust movie cooker. Music and effect compilation use Rust.
