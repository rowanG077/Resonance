"""Stage pinned Microsoft WARP and DXC DLLs beside the Windows CI example."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parents[2]
# Match the versions used by gfx-rs/wgpu's Windows CI. These DLLs are test-only.
PACKAGES = (
    (
        "warp-1.0.20.zip",
        "https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.warp/1.0.20/microsoft.direct3d.warp.1.0.20.nupkg",
        "e5fe5de661ce98b58ef9cfb736e73c0a7a2623d3bbf5f14839b2d55566d87e40",
        {"build/native/bin/x64/d3d10warp.dll": "d3d10warp.dll"},
    ),
    (
        "dxc-2026-05-27.zip",
        "https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.9.2602.24/dxc_2026_05_27.zip",
        "cf658aacf070d3045e31b8f1f8a696c2945f37c1095019481ef7c513368db3b4",
        {"bin/x64/dxcompiler.dll": "dxcompiler.dll", "bin/x64/dxil.dll": "dxil.dll"},
    ),
)
WARP_DRIVER_VERSION = "1.0.20.0"


def install_archive(archive, expected_hash, members, destination):
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != expected_hash:
        raise ValueError(f"SHA-256 mismatch for {archive.name}: {digest}")
    with zipfile.ZipFile(archive) as bundle:
        for member in members:
            bundle.getinfo(member)
        destination.mkdir(parents=True, exist_ok=True)
        for member, filename in members.items():
            with bundle.open(member) as source, (destination / filename).open("wb") as target:
                shutil.copyfileobj(source, target)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=ROOT / "target/ci-windows-renderer")
    parser.add_argument("--output", type=Path, default=ROOT / "target/release-fast/examples")
    parser.add_argument("--github-env", type=Path, default=os.environ.get("GITHUB_ENV"))
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    for filename, url, digest, members in PACKAGES:
        archive = args.cache / filename
        if not archive.exists():
            with urllib.request.urlopen(url, timeout=60) as source, archive.open("wb") as target:
                shutil.copyfileobj(source, target)
        install_archive(archive, digest, members, args.output)
        print(f"Verified and staged {filename} in {args.output}")
    if args.github_env:
        with args.github_env.open("a", encoding="utf-8") as environment:
            environment.write(f"WGPU_DX12_COMPILER=dxc\nRESONANCE_CI_WARP_VERSION={WARP_DRIVER_VERSION}\n")


if __name__ == "__main__":
    main()
