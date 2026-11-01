<p align="center"><img src="resources/logo.svg" alt="Resonance" width="640"></p>

Resonance is a Rust reimplementation of the GameCube version of Tales of
Symphonia, built with Bevy. Its goals are:

- Faithful gameplay, visuals and audio on modern platforms.
- Human-readable, editable assets for modding.
- Support for creating new fields, events, skits and other content.

## Clone and build

With Git and Nix installed:

```sh
git clone https://github.com/rowanG077/Resonance.git resonance
cd resonance
nix develop
cargo build --release -p resonance -p resonance-import-cli
```

## Import assets and play

You need both North American GameCube disc images (GQSEAF revision 0):

```sh
target/release/resonance-import /path/to/Disc1.rvz /path/to/Disc2.rvz
target/release/resonance
```

The importer writes to `local/all-assets`, which must be empty before importing.
The game loads that directory by default.

Move with WASD or the arrow keys, confirm with Enter, and open the field menu
with Tab. Gamepads are also supported. Movement runs by default; hold Walk
(Shift or L2 by default), or tilt the movement stick halfway or less, to walk.

See the [cooking guide](docs/cooking.md) for import options and
[editing guide](docs/editable-content.md) for modding.

Customize → Display Mode selects Windowed, Borderless or Fullscreen. Resolution
sets the render size and windowed size; borderless fills the desktop, and
fullscreen uses a matching monitor mode when available. Resolution choices include
the current monitor's physical desktop size (marked Native), its advertised video
modes and common presets. Enter opens the Resolution dropdown; Up/Down highlights
a size and Enter applies it. Left/Right changes Display Mode. Choose Keep within
15 seconds after a display change to accept;
Revert, Escape or the timeout restores the previous display settings. Confirmed
choices save in `config.toml` when you exit Customize.
Cancel at the top restores the settings from when you opened the menu; Default
restores the library's defaults. `--resolution WIDTHxHEIGHT` overrides the saved
resolution at startup.
Fullscreen waits until the window's monitor is known. On Wayland it uses desktop
fullscreen, since exclusive video-mode switching is unavailable.

Customize → Graphics Backend lists the native backends detected on the current
GPU: DirectX 12 and Vulkan on Windows, Metal on macOS, and Vulkan on Linux where
available. Automatic prefers DirectX 12 on Windows. Enter opens the list;
Up/Down highlights a backend and Enter selects it. Changes save when you exit
Customize and take effect after restarting the game. The menu shows the backend
currently running.
`WGPU_BACKEND` takes precedence over the saved setting for troubleshooting.

Customize → Anti-Aliasing offers one selector: Off, SMAA Low/Medium/High/Ultra,
supported 2×/4×/8× MSAA, and SMAA S2x (requires 2× MSAA support).
The default is 4× MSAA. S2x reconstructs the two spatial samples separately,
then averages them within the same frame. SMAA processes the 3D scene before
menus, dialogue and text are composited. Unsupported GPU choices are hidden.
All modes are spatial: no temporal accumulation, reprojection or camera jitter.
Enter opens the Anti-Aliasing dropdown; Up/Down highlights a mode and Enter
applies it. Cancel closes either dropdown without changing the setting.
Resolution and AA ignore Left/Right. Accepted AA choices apply immediately;
settings save when you exit Customize.

Customize → Keymap assigns keyboard keys, controller buttons, D-pad and stick
motions to standard actions. Select an action, confirm, release the opener,
then press and release the new input. The editor uses default menu controls so
remapping navigation cannot strand its cursor.

The Artes page lists each character's artes with one combo per arte. Confirm
records inputs held together and then released; recording again replaces that
arte's combo. Stick directions include moderate tilts and can be combined with
standard keymap inputs. If a combo belongs to another arte or character, a popup
names its owner and asks whether to transfer it. Cancel or No preserves both
assignments. Details clears a combo, Guard toggles AI use, and Menu executes an
arte. Auto characters use controller one's assignments. Single inputs used by
normal combat (Attack, Guard, movement, targeting, orders, etc.) are protected.
Cancel and field/menu-only inputs can be assigned on their own. A rejected
assignment opens a popup explaining the conflict or unsupported combo. Combos
can include combat inputs: the complete combo consumes its action buttons until
release while preserving movement modifiers and unrelated controls. More specific
overlapping combos take priority, including across characters. Menus, target
selection, and Unison confirmation retain their contextual controls. Existing
singles that conflict after a keymap edit are marked and inactive until corrected.

Configured prompts show the keyboard or controller assignment for the last
configured device input used. Unassigned inputs, held buttons, stick jitter and
stick release do not change the displayed device. Before the first valid keyboard
input, controller icons are preferred; actions assigned on only one device use
that assignment. If both devices register input in the same frame, the current
icons are retained. Cooking exports the high-resolution input atlas and layout;
the viewer requires these editable assets. Defaults are bundled only with the
importer; see [input icons](docs/cooking.md#input-icons).

The Unison menu selects one learned, Unison-eligible arte for each character.
This loadout is saved separately from regular arte combos and key mappings.
Timed character confirmation uses that character's selected Unison arte;
stick directions do not change it.

Status has separate title and costume rows. Owned costume titles unlock outfits
in the costume selector; changing a title keeps the selected outfit. Default
restores the character's story outfit. The title selector compares the current
and highlighted titles' bonuses per level as numbers, including fractional stat
points, with colors indicating better, worse or equal growth.

The importer automatically includes [Ghost Wolf](mods/ghost-wolf/README.md), an
explicit modding example that adds a new Lloyd arte through editable action,
animation, effect, learning and menu assets. Its Blender source is included for
animation editing.
