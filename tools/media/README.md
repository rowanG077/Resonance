# Offline media tools

The Rust importer owns parsing, cooking, validation and caching. Audio and video
codecs run as Rust libraries. `nix develop` supplies KTX tools for producing cooked
KTX2 textures; they are separate from player deployment.

See [build/cook commands](../../README.md)
and [audio/video ownership](../../docs/audio-video-architecture.md).

Music cooking requires Dolphin's `Sys/GC/dsp_coef.bin` explicitly through
`--coefficients`; the importer verifies its expected hash. No recording is used
as a cooking input. Scores/instruments compile to typed operations and decoded
samples; effects retain gain controls and persistent shared reverb.

Movies are cooked into RGB FFV1 video and FLAC audio in Matroska files, with
channel order and playback rate normalized for the player. Cooking reuses
cached assets when their source data and conversion recipe are unchanged.
`cook-intro --audio-stream 1` selects stereo; stream 2 is mono. Manifests retain
source/recipe identity and intermediate hashes for reuse. Media cooks share
`local/cooked/.cook-media.lock`; remove a stale lock only after its process stops.

Use `target/debug/resonance-import --help` for offline inspection and rendering commands.
They write files without an output device. [Oracle instructions](../oracle/README.md)
cover PCM comparisons. Music and effect compilation also use Rust.
