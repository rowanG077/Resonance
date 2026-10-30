# Input prompts

Initial artwork: [Kenney Input Prompts 1.5A](https://kenney.nl/assets/input-prompts),
CC0. The creator's license is in `License.txt`. The selected SVG originals and
`sources.json` record the source of every included icon.

`atlas.ktx2` contains 117 distinct prompts: keyboard letters, digits, function and
special keys, A/B/X/Y, L1–L5 and R1–R5, START/BACK, individual D-pad directions,
and left/right stick directions. Mouse artwork, grouped arrow-key islands,
unused generic shapes and duplicate aliases are omitted. Keyboard and controller
buttons are distinct controls; physical-input aliases resolve in code instead
of occupying extra atlas tiles.

Original Kenney frame, lettering and directional paths retain their shapes.
Keys and L/R use blue faces, gold rims and cream labels; pressed faces turn gold
with dark labels. A/B/X/Y use green, red, blue and yellow faces and brighten when
pressed. Multipart controls accent the actuated component while preserving the
remaining artwork. L3/R3 combine the pack's existing L/R frame/letter paths with
its original digit 3; no substitute font or replacement button geometry is used.
`sources.json` records the upstream variants and label adaptations.

Each tile is 256 × 256. `atlas.json` maps normal and pressed rectangles. The
4096 × 3840 KTX2 atlas, layout and complete `atlas.svg` are stored in the repo.
`atlas.svg` is under 10 MB and uses normal Git storage. Its nested SVGs have
unique IDs so each tile retains its own gradients. Normal builds and cooking
use the stored KTX2 and layout; no SVG tools or rasterizer are required.

Cooking also exports the atlas, manifest and license to `ui/controls/` in the
asset library. Those files can be replaced through the normal asset/mod layers.
Only the importer bundles the default texture, layout and license. The viewer
requires the cooked files and reports an error if either is missing or invalid.

To compose just the final SVG using Python's standard library, run
`python3 tools/build_input_prompts.py --svg-only`.
This leaves the runtime texture and layout unchanged.
To regenerate all three files after editing the SVG sources, run
`python3 tools/build_input_prompts.py` with ImageMagick (librsvg) and Rust installed.
It rasterizes the composed SVG once and publishes the outputs only after
conversion succeeds. Pillow is not needed. Ordinary cooking copies the stored
files; the viewer uses its normal KTX2 image loader.
