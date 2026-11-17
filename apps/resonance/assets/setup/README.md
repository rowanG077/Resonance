The first-run screen embeds these files; no cooked game assets are required.

- `window-frame.png` and `menu-pattern.png` are rasterizations of the matching
  SVGs in `apps/website/static`, at their original dimensions.
- `logo.png` is `resources/logo.svg` rasterized to fit 320 × 240.
- `kalam.ttf` is the website's `kalam-regular.woff2` decompressed without changing
  its outlines. Its SIL Open Font License is included in `kalam-OFL.txt`.
- DejaVu Sans and Sans Mono provide readable body text and diagnostic output,
  including punctuation absent from Bevy's subset font. Their redistribution
  license is included in `DejaVu-LICENSE.txt`.
- `import-profile.json` contains measured full-cook stage durations by actual CPU
  budget. Regenerate it with `tools/profile-import.py`; see `docs/cooking.md`.

The window and menu colors follow the website's existing Tales-inspired styling.

Manual GPU/layout and spoiler-gate verification:

```sh
cargo test -p resonance first_run::tests::setup_screen_spoiler_gate_and_layout -- --ignored
```

This writes review PNGs to `target/setup-review`, including the compact 800 × 640
layout, without reading or changing an installed library.
