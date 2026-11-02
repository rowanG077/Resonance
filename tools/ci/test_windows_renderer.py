import hashlib
from pathlib import Path
import tempfile
import unittest
import zipfile

from install_windows_renderer import install_archive


class WindowsRendererInstallerTests(unittest.TestCase):
    def test_verifies_archive_and_stages_only_requested_dll(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "runtime.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("bin/x64/runtime.dll", b"test runtime")
                bundle.writestr("bin/arm64/runtime.dll", b"wrong architecture")
                bundle.writestr("bin/x64/tool.exe", b"unused")
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            output = root / "example"
            install_archive(archive, digest, {"bin/x64/runtime.dll": "runtime.dll"}, output)
            self.assertEqual({p.name for p in output.iterdir()}, {"runtime.dll"})
            self.assertEqual((output / "runtime.dll").read_bytes(), b"test runtime")

    def test_wrong_digest_cannot_install_a_dll(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "runtime.zip"
            archive.write_bytes(b"untrusted or incomplete download")
            output = root / "example"
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                install_archive(archive, "0" * 64, {"runtime.dll": "runtime.dll"}, output)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
