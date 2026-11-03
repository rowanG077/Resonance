"""Package the two release executables without depending on platform archivers."""

import argparse
import os
import subprocess
from pathlib import Path
import re
import tarfile
import tempfile
import shutil
import zipfile


PLATFORMS = ("windows-x64", "macos-arm64", "linux-x64", "linux-arm64")
ROOT = Path(__file__).resolve().parents[2]
EXECUTABLES = ("resonance", "resonance-import")


def strip_executable(path, platform):
    """Strip only a distribution copy, preserving code addresses and debug IDs."""
    if platform == "windows-x64":
        # MSVC keeps debugging data in the separate PDB. Preserve the PE's
        # CodeView record so dumps can still identify that exact PDB.
        return
    if platform == "macos-arm64":
        subprocess.run(["strip", "-u", "-r", str(path)], check=True)
        # Modifying a Mach-O invalidates its linker-generated ad-hoc signature.
        subprocess.run(["codesign", "--force", "--sign", "-", str(path)], check=True)
    else:
        subprocess.run(["strip", "--strip-all", str(path)], check=True)


def validate(version, platform):
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)|dev-[0-9a-f]{7}", version):
        raise ValueError("expected vX.X.X or dev-<seven-character commit>")
    if platform not in PLATFORMS:
        raise ValueError(f"unsupported platform: {platform}")


def package(version, platform, binaries, output, root=ROOT):
    validate(version, platform)
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
            if filename.startswith("resonance"):
                strip_executable(destination, platform)
        if suffix:
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                for source in sorted(directory.iterdir()):
                    bundle.write(source, f"{name}/{source.name}")
        else:
            with tarfile.open(archive, "w:gz") as bundle:
                bundle.add(directory, arcname=name)
    return archive


def package_symbols(version, platform, binaries, output):
    """Keep the exact linked executable and platform debug data together."""
    validate(version, platform)
    name = f"resonance-{version}-{platform}-symbols"
    suffix = ".exe" if platform == "windows-x64" else ""
    sources = []
    for executable in EXECUTABLES:
        sources.extend([binaries / f"{executable}{suffix}", binaries / f"{executable}.sym"])
        if suffix:
            sources.append(binaries / f"{executable.replace('-', '_')}.pdb")
        elif platform == "macos-arm64":
            dsym = binaries / f"{executable}.dSYM"
            if not (dsym / f"Contents/Resources/DWARF/{executable}").is_file():
                raise FileNotFoundError(dsym)
            sources.append(dsym)
    for source in sources:
        if not source.exists():
            raise FileNotFoundError(source)
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (name + (".zip" if suffix else ".tar.gz"))
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary) / name
        directory.mkdir()
        for source in sources:
            if source.is_dir():
                shutil.copytree(source, directory / source.name)
            else:
                shutil.copyfile(source, directory / source.name)
        revision = os.environ.get("RELEASE_COMMIT") or os.environ.get("GITHUB_SHA")
        if not revision:
            result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                    capture_output=True, text=True, check=False)
            revision = result.stdout.strip() or "unknown"
        (directory / "BUILD.txt").write_text(
            f"Resonance {version}\nPlatform: {platform}\nRevision: {revision}\n"
            "Keep these files with crash reports from this exact release.\n"
            "Run: minidump-stackwalk CRASH.dmp resonance.sym\n"
            "See docs/crash-debugging.md in the source repository.\n", encoding="utf-8")
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
    parser.add_argument("--symbols", action="store_true", help="also archive original executables and matching debugging artifacts")
    args = parser.parse_args()
    print(package(args.version, args.platform, args.binaries, args.output))
    if args.symbols:
        print(package_symbols(args.version, args.platform, args.binaries, args.output))


if __name__ == "__main__":
    main()
