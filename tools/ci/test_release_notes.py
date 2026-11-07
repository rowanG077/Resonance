"""Check that publication reads only the locally prepared release notes."""

from pathlib import Path
import tempfile
import unittest

from release_notes import prepared_release_notes, release_notes


class ReleaseNotesTests(unittest.TestCase):
    def test_selects_exact_version_between_other_releases(self):
        notes = "## 1.2.3 - *07 Nov, 2026*\n\n### Fixed:\n- A fix. [P12](https://example.com/12)\n"
        changelog = ("# Changelog\n\n## 1.2.30\n\n- Future release.\n\n" + notes
                     + "\n## 1.2.2\n\n- Older release.\n")
        self.assertEqual(release_notes(changelog, "v1.2.3"), notes)

    def test_selects_last_release(self):
        self.assertEqual(release_notes("# Changelog\n\n## 1.2.3\n\n- A fix.\n", "v1.2.3"),
                         "## 1.2.3\n\n- A fix.\n")

    def test_missing_duplicate_and_empty_sections_fail(self):
        for changelog in ["## 1.2.30\n\n- Wrong release.\n",
                          "## 1.2.3\n\n- First.\n\n## 1.2.3\n\n- Second.\n",
                          "## 1.2.3\n\n## 1.2.2\n\n- Older release.\n"]:
            with self.subTest(changelog=changelog), self.assertRaises(ValueError):
                release_notes(changelog, "v1.2.3")

    def test_pending_fragments_must_be_consolidated(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            notes = "## 1.2.3\n\n- A fix.\n"
            (root / "CHANGELOG.md").write_text(notes)
            entries = root / "changelog/entries"
            entries.mkdir(parents=True)
            (entries / ".gitkeep").touch()
            self.assertEqual(prepared_release_notes(root, "v1.2.3"), notes)
            (entries / "pending.md").write_text("A pending change")
            with self.assertRaisesRegex(ValueError, "locally"):
                prepared_release_notes(root, "v1.2.3")


if __name__ == "__main__":
    unittest.main()
