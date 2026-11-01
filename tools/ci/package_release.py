"""Package the two release executables without depending on platform archivers."""

import argparse
from pathlib import Path
import re
import tarfile
import tempfile
import shutil
import zipfile


PLATFORMS = ("windows-x64", "macos-arm64", "linux-x64", "linux-arm64")
ROOT = Path(__file__).resolve().parents[2]


def package(version, platform, binaries, output, root=ROOT):
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)|dev-[0-9a-f]{7}", version):
        raise ValueError("expected vX.X.X or dev-<seven-character commit>")
    if platform not in PLATFORMS:
        raise ValueError(f"unsupported platform: {platform}")
    name = f"resonance-{version}-{platform}"
    suffix = ".exe" if platform == "windows-x64" else ""
    files = {
        f"resonance{suffix}": binaries / f"resonance{suffix}",
        f"resonance-import{suffix}": binaries / f"resonance-import{suffix}",
        "README.md": root / "docs/installation.md",
        "LICENSE": root / "LICENSE",
    }
    # Check every input before creating an archive, including both executables.
    for source in files.values():
        if not source.is_file():
            raise FileNotFoundError(source)
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (name + (".zip" if suffix else ".tar.gz"))
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary) / name
        directory.mkdir()
        for filename, source in files.items():
            destination = directory / filename
            shutil.copyfile(source, destination)
            destination.chmod(0o755 if filename.startswith("resonance") else 0o644)
        if suffix:
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                for source in sorted(directory.iterdir()):
                    bundle.write(source, f"{name}/{source.name}")
        else:
            with tarfile.open(archive, "w:gz") as bundle:
                bundle.add(directory, arcname=name)
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--platform", required=True, choices=PLATFORMS)
    parser.add_argument("--binaries", type=Path, default=ROOT / "target/release")
    parser.add_argument("--output", type=Path, default=ROOT / "target/dist")
    args = parser.parse_args()
    print(package(args.version, args.platform, args.binaries, args.output))


if __name__ == "__main__":
    main()
