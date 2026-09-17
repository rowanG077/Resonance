# Offline media tools

The Rust importer owns parsing, cooking, validation and caching. `nix develop`
supplies these helpers; they are separate from player deployment:

| Tool | Use |
|---|---|
| vgmstream | Decode original movie audio and speech |
| KTX tools | Produce cooked KTX2 textures |

The player uses the shared Rust synthesizer, CPAL output, Rust FFV1/FLAC
decoders and Rubato device-rate conversion. It does not launch these converter programs. See [build/cook commands](../../README.md)
and [audio/video ownership](../../docs/audio-video-architecture.md).

Music cooking requires Dolphin's `Sys/GC/dsp_coef.bin` explicitly through
`--coefficients`; the importer verifies its expected hash. No recording is used
as a cooking input. Scores/instruments compile to typed operations and decoded
samples; effects retain gain controls and persistent shared reverb.

Movie recipes decode video with the published [`h4m` 0.3.0](https://crates.io/crates/h4m/0.3.0), reorder by presentation index,
apply the executable's exact color tables, and encode RGB FFV1 + FLAC in Matroska
using `codec_ffv1` and `flacenc`. The Matroska writer emits the fixed cooked
profile in timestamp order. `h4m` skips audio; vgmstream still decodes movie audio
and speech. The recipe records codec versions and invalidates older cooks.
Playback and oracle extraction use the pinned `rust-av/ffv1` decoder and
`matroska-demuxer`; FLAC playback uses Symphonia. Existing cooked movies remain
readable, including entropy state carried between video keyframes.
`cook-intro --audio-stream 1` selects stereo; stream 2 is mono. Manifests retain
source/recipe identity and intermediate hashes for reuse. The process runner
bounds logs/timeouts and reaps failed converters. Media cooks share
`local/cooked/.cook-media.lock`; remove a stale lock only after its process stops.

Use `target/debug/resonance-import --help` for offline inspection and rendering commands.
They write files without an output device. [Oracle instructions](../oracle/README.md)
cover PCM comparisons. Music and effect compilation also use Rust.

The H4M dependency is LGPL-2.0-or-later and is linked into the offline importer;
see its [license](https://github.com/rowanG077/h4m-rs/blob/v0.3.0/LICENSE).
