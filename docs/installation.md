# Install and use Resonance

Resonance is a Rust reimplementation of the GameCube version of Tales of
Symphonia. Download a prebuilt archive from
[GitHub Releases](https://github.com/rowanG077/Resonance/releases).
Each archive includes the game, the asset importer, this README and the license.
Rust, Nix and external media conversion tools are not required.

## Choose a download

| Platform | Archive ending | Requirements |
| --- | --- | --- |
| Windows | `windows-x64.zip` | 64-bit Windows 10/11; DirectX 12 or Vulkan graphics drivers |
| macOS | `macos-arm64.tar.gz` | Apple Silicon, macOS 14 or newer; Metal |
| Linux | `linux-x64.tar.gz` | x86-64, Ubuntu 24.04 or compatible; Vulkan graphics drivers |
| Linux | `linux-arm64.tar.gz` | ARM64, Ubuntu 24.04 or compatible; Vulkan graphics drivers |

The macOS build runs natively on Apple Silicon. Intel Macs are not included.
Linux binaries use system audio, windowing and Vulkan libraries. On Ubuntu 24.04:

```sh
sudo apt-get update
sudo apt-get install libasound2t64 libdbus-1-3 libudev1 libvulkan1 \
  libx11-6 libxcursor1 libxi6 libxrandr2 libxkbcommon0 libwayland-client0 libwayland-cursor0 libwayland-egl1
```

Install the Vulkan driver appropriate for your GPU as well (for example,
`mesa-vulkan-drivers` for supported Intel/AMD GPUs).

## Extract and run

Extract the whole archive into a writable folder. Keep both executables together.
Open a terminal in the extracted `resonance-vX.X.X-PLATFORM` folder and run the
commands below there. Relative paths use the current directory.

You need your own images of **both North American GameCube discs, GQSEAF
revision 0**. RVZ, ISO and GCM are supported. Game assets and disc images are
not included in these downloads.

### Windows (PowerShell)

Use **Extract All** on the ZIP, open PowerShell in the extracted folder, then:

```powershell
.\resonance-import.exe "C:\Games\Symphonia\Disc1.rvz" "C:\Games\Symphonia\Disc2.rvz"
.\resonance.exe
```

### macOS and Linux

Extract your downloaded archive, then enter its folder:

```sh
tar -xzf resonance-vX.X.X-PLATFORM.tar.gz
cd resonance-vX.X.X-PLATFORM
./resonance-import "/path/to/Disc1.rvz" "/path/to/Disc2.rvz"
./resonance
```

Replace the example version, platform and disc paths with your actual filenames.
The importer reads both discs and creates `local/all-assets`. Importing can take
some time and requires free disk space for temporary extraction and cooked assets.
The game reads `local/all-assets` by default. Import once before starting it.
Cooking writes directly to its output and overwrites existing files; use a fresh
output directory to avoid mixing asset versions or overwriting your edits.

These builds are unsigned. On macOS, if Gatekeeper blocks a binary you trust,
allow it in **System Settings → Privacy & Security → Open Anyway**. You may need
to allow both executables separately.

For assets in another directory:

```sh
./resonance-import "/path/to/Disc1.rvz" "/path/to/Disc2.rvz" --output "/path/to/assets"
./resonance --assets "/path/to/assets"
```

In PowerShell use `.\resonance-import.exe` and `.\resonance.exe` instead.
Use `--help` with either executable to see all command-line options.

## Play

Move with WASD or the arrow keys, confirm with Enter, and open the field menu
with Tab. Gamepads are supported. Movement runs by default; hold Shift or L2,
or tilt the movement stick halfway or less, to walk.

Use **Customize** in the game to change display mode, resolution, graphics
backend, anti-aliasing and controls. Saves and `config.toml` normally live in:

- Windows: `%LOCALAPPDATA%\Resonance`
- macOS: `~/Library/Application Support/Resonance`
- Linux: `$XDG_DATA_HOME/resonance`, or `~/.local/share/resonance` by default

For a specific save location, launch with `--save-directory "/path/to/saves"`.
Keep your cooked assets and saves when replacing the executables with a newer
release. If a newer release reports an unsupported cooked layout, import the
discs again into a fresh directory using its importer.

For more details, see the
[cooking guide](https://github.com/rowanG077/Resonance/blob/main/docs/cooking.md),
[editing guide](https://github.com/rowanG077/Resonance/blob/main/docs/editable-content.md)
and [project README](https://github.com/rowanG077/Resonance#readme).

## Verify downloads

The release includes `SHA256SUMS` with SHA-256 hashes for all four archives.
On Linux run `sha256sum --ignore-missing -c SHA256SUMS`. On macOS run
`shasum -a 256 /path/to/archive.tar.gz` and compare its hash with the listed one.
On Windows run `Get-FileHash .\resonance-vX.X.X-windows-x64.zip -Algorithm SHA256`
in PowerShell and compare its hash with `SHA256SUMS`.
