"""Exercise archive contents and publication recovery without contacting GitHub."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

from package_release import PLATFORMS, ROOT, package


class ArchiveTests(unittest.TestCase):
    def test_every_platform_contains_only_executables_readme_and_license(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "binaries"
            binaries.mkdir()
            for name in ("resonance", "resonance-import", "resonance.exe", "resonance-import.exe"):
                (binaries / name).write_bytes(b"synthetic executable")
            (binaries / "private-disc.iso").write_bytes(b"must not ship")
            (binaries / "d3d10warp.dll").write_bytes(b"test-only renderer")
            (binaries / "dxcompiler.dll").write_bytes(b"test-only shader compiler")
            (binaries / "dxil.dll").write_bytes(b"test-only shader validator")
            for platform in PLATFORMS:
                with self.subTest(platform=platform):
                    archive = package("v1.2.3", platform, binaries, root / "dist")
                    prefix = f"resonance-v1.2.3-{platform}"
                    suffix = ".exe" if platform.startswith("windows") else ""
                    expected = {"README.md", "LICENSE", f"resonance{suffix}", f"resonance-import{suffix}"}
                    if suffix:
                        with zipfile.ZipFile(archive) as bundle:
                            names = set(bundle.namelist())
                            readme = bundle.read(f"{prefix}/README.md")
                    else:
                        with tarfile.open(archive) as bundle:
                            names = {member.name for member in bundle.getmembers() if member.isfile()}
                            readme = bundle.extractfile(f"{prefix}/README.md").read()
                            for name in ("resonance", "resonance-import"):
                                self.assertEqual(bundle.getmember(f"{prefix}/{name}").mode, 0o755)
                    self.assertEqual(names, {f"{prefix}/{name}" for name in expected})
                    self.assertEqual(readme, (ROOT / "docs/installation.md").read_bytes())

    def test_missing_importer_and_unsafe_version_do_not_create_archives(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "resonance").write_bytes(b"game")
            with self.assertRaises(FileNotFoundError):
                package("v1.2.3", "linux-x64", root, root / "dist")
            for version in ("../escape", "v1.2", "v01.2.3", "v1.2.3-beta", "v1.2.3\n"):
                with self.subTest(version=version), self.assertRaises(ValueError):
                    package(version, "linux-x64", root, root / "dist")
            self.assertFalse((root / "dist").exists())


# A stateful GitHub CLI stand-in lets us verify exact commits, retries and the
# final publication boundary, including failures between upload and publication.
GH_STUB = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

path = Path(os.environ["FAKE_GH_STATE"])
state = json.loads(path.read_text())
args = sys.argv[1:]
state["calls"].append(args)
path.write_text(json.dumps(state))
if state.get("api_failure") and args[0] == "api":
    sys.exit(1)
if args[0] == "api":
    if "--method" in args:
        state["tag"] = args[args.index("-f", args.index("-f") + 1) + 1].split("=", 1)[1]
    elif any("matching-refs" in arg for arg in args):
        if state.get("tag"):
            print("refs/tags/v1.2.3")
    elif any("/commits/" in arg for arg in args):
        print(state["tag"])
    elif any("/releases?" in arg for arg in args):
        if "draft" in state:
            print(str(state["draft"]).lower())
    else:
        sys.exit("unexpected API call")
elif args[:2] == ["release", "create"]:
    state["draft"] = True
elif args[:2] == ["release", "upload"]:
    if state.get("upload_failure"):
        sys.exit(1)
    state["uploaded"] = args[3:-1]
elif args[:2] == ["release", "edit"]:
    state["draft"] = False
else:
    sys.exit("unexpected gh call")
path.write_text(json.dumps(state))
'''


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "bin").mkdir()
        (self.root / "docs").mkdir()
        (self.root / "target/dist").mkdir(parents=True)
        (self.root / "docs/installation.md").write_text("Installation instructions")
        for platform in PLATFORMS:
            suffix = ".zip" if platform.startswith("windows") else ".tar.gz"
            (self.root / f"target/dist/resonance-v1.2.3-{platform}{suffix}").write_bytes(platform.encode())
        stub = self.root / "bin/gh"
        stub.write_text(GH_STUB)
        stub.chmod(0o755)
        self.state = self.root / "state.json"
        self.state.write_text(json.dumps({"calls": []}))
        self.environment = dict(os.environ, GH_REPO="example/resonance", RELEASE_COMMIT="a" * 40,
                                GITHUB_SHA="b" * 40,
                                VERSION="v1.2.3", FAKE_GH_STATE=str(self.state),
                                PATH=str(self.root / "bin") + os.pathsep + os.environ["PATH"])

    def run_publication(self, **state):
        if state:
            self.state.write_text(json.dumps(dict(calls=[], **state)))
        result = subprocess.run(["bash", str(ROOT / "tools/ci/publish_release.sh")],
                                cwd=self.root, env=self.environment, capture_output=True, text=True)
        return result, json.loads(self.state.read_text())

    def test_creates_exact_commit_tag_uploads_all_inputs_then_publishes(self):
        result, state = self.run_publication()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(state["tag"], self.environment["RELEASE_COMMIT"])
        self.assertFalse(state["draft"])
        self.assertEqual(len(state["uploaded"]), 6)
        self.assertEqual(state["calls"][-1], ["release", "edit", "v1.2.3", "--draft=false"])
        for line in (self.root / "target/dist/SHA256SUMS").read_text().splitlines():
            digest, filename = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((self.root / "target/dist" / filename).read_bytes()).hexdigest())

    def test_different_commit_cannot_move_existing_tag(self):
        result, state = self.run_publication(tag="b" * 40)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to move", result.stderr)
        self.assertFalse(any(call[0] == "release" or "POST" in call for call in state["calls"]))

    def test_failed_upload_stays_draft_and_rerun_resumes_it(self):
        result, state = self.run_publication(upload_failure=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(state["draft"])
        result, state = self.run_publication(tag=state["tag"], draft=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(state["draft"])
        self.assertFalse(any(call[:2] == ["release", "create"] for call in state["calls"]))

    def test_published_release_is_unchanged_on_rerun(self):
        result, state = self.run_publication(tag=self.environment["RELEASE_COMMIT"], draft=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(call[0] == "release" for call in state["calls"]))

    def test_api_failure_cannot_create_a_tag(self):
        result, state = self.run_publication(api_failure=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(state["calls"]), 1)
        self.assertNotIn("tag", state)

    def test_missing_platform_cannot_create_a_tag(self):
        (self.root / "target/dist/resonance-v1.2.3-windows-x64.zip").unlink()
        result, state = self.run_publication()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(state["calls"], [])


if __name__ == "__main__":
    unittest.main()
