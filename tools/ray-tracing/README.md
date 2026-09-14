# Ray-traced classroom quick start

Run these commands from the repository root on the `raytracing-prototype` branch.
The GPU capture has been tested on Linux with an NVIDIA RTX 4080 SUPER. It needs
a working Vulkan driver with ray-query support. The interactive player can fall
back to raster lighting on unsupported devices; the capture tool requires ray tracing.

## Setup and assets

```sh
nix develop
```

The development shell supplies Rust, Python with Pillow, FFmpeg and the asset
conversion tools. Follow [Build and cook](../../README.md#build-and-cook) to
prepare `local/cooked` from your own supported game discs (North American GQSEAF
revision 0). This includes the classroom audio. Alternatively, copy an already
prepared `local/cooked` directory, including `overrides/`, from your other checkout.
Game assets, the HD archive and generated overrides are not included in Git.

For HD classroom, NPC, dialogue and emote textures, put `hd-textures.7z` in the
repository root and run this after cooking. The extra shell supplies 7-Zip and
libxxhash; keep the development shell open underneath it.

```sh
nix-shell -p p7zip xxHash
7z x hd-textures.7z -olocal/hd-textures
python3 tools/hd-textures/prepare_classroom.py \
  --cooked local/cooked --pack local/hd-textures/GQS \
  --extracted local/extracted/disc1
```

Overrides load automatically. The `--extracted` argument enables the dialogue,
font and emote replacements as well as the models. Preparation leaves the
source texture pack unchanged.

## Play

```sh
cargo run -p resonance -- --skip-intro --resolution 1920x1080
```

Choose New Game. `--skip-intro` skips to the title; the capture commands below
skip both the title and movies. Solari lighting is enabled by default in the
classroom. F6 toggles the authored lighting; Enter advances dialogue.

## Capture a short preview

```sh
# A quick one-second test in the visible classroom: 720p, 32 samples/frame.
python3 tools/ray-tracing/capture-classroom.py --renderer gpu --lighting solari \
  --quality medium --from 15 --max-duration 1 --output local/classroom-preview

# The same section at 4K, 256 samples/frame, with stronger SMAA.
python3 tools/ray-tracing/capture-classroom.py --renderer gpu --lighting solari \
  --quality ultra --samples 256 --smaa strong \
  --from 15 --max-duration 1 --output local/classroom-4k-test
```

The script builds the capture binary automatically. Each output directory must
be new. Open `local/classroom-preview/classroom.mp4` for the first command's
60 fps video with synchronized audio. Lossless PNGs are in `frames/`; timing and
settings are in `recording.json` and `settings.json` beside the video.

- Keep `--lighting solari` explicit: the script currently defaults to the reference
  full path tracer, which needs many more samples to reduce noise.
- `--from` skips video seconds; `--max-duration` limits captured video time,
  not render time. The opening contains black-screen dialogue; the room appears
  around eight seconds in.
- `--quality low` is the fastest preview; `high` gives 1080p with 256 samples.
  `--samples` overrides the preset. Offline accumulation takes much longer than
  interactive play; 60 fps is the encoded playback rate.
- Omit `--from` to start at the beginning. Omit `--max-duration` too to capture
  through the handoff to Lloyd; this can take hours. Ctrl+C stops a capture.

See [the prototype guide](../../docs/classroom-prototype.md) for lighting details,
driver workarounds and further capture options, or run the script with `--help`.
