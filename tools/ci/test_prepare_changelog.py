"""Exercise release changelog commits against a disposable local Git remote."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from prepare_changelog import git, prepare


QLOG_STUB = '''#!/usr/bin/env python3
from pathlib import Path
import sys

entries = sorted(Path("changelog/entries").glob("*.md"))
if sys.argv[1] == "check":
    sys.exit(int(any("INVALID" in entry.read_text() for entry in entries)))
if sys.argv[1] == "collect":
    version = sys.argv[sys.argv.index("--version") + 1]
    output = Path(sys.argv[sys.argv.index("--out") + 1])
    output.write_text("## " + version + "\\n" + "\\n".join(entry.read_text() for entry in entries))
else:
    sys.exit("unexpected qlog command")
'''


class ChangelogPreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / "work"
        self.remote = self.base / "origin.git"
        self.branch = "release/v1.2.3"
        subprocess.run(["git", "init", "--bare", str(self.remote)],
                       check=True, capture_output=True)
        subprocess.run(["git", "init", "-b", self.branch, str(self.root)],
                       check=True, capture_output=True)
        git(self.root, "config", "user.name", "Test")
        git(self.root, "config", "user.email", "test@example.com")
        git(self.root, "remote", "add", "origin", str(self.remote))
        (self.root / "changelog/entries").mkdir(parents=True)
        (self.root / "changelog/config.yaml").write_text("project: example/resonance\n")
        (self.root / "changelog/entries/.gitkeep").touch()
        self.entry("first", "First change")
        self.initial = self.commit("Initial change")
        git(self.root, "push", "origin", f"HEAD:refs/heads/{self.branch}")
        tools = self.base / "bin"
        tools.mkdir()
        executable = tools / "qlog"
        executable.write_text(QLOG_STUB)
        executable.chmod(0o755)
        environment = patch.dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"])
        environment.start()
        self.addCleanup(environment.stop)

    def entry(self, name, message):
        (self.root / f"changelog/entries/{name}.md").write_text(message + "\n")

    def commit(self, message):
        git(self.root, "add", ".")
        git(self.root, "commit", "-m", message)
        return git(self.root, "rev-parse", "HEAD")

    def remote_head(self):
        return git(self.root, "ls-remote", "origin", f"refs/heads/{self.branch}").split()[0]

    def test_consolidation_commits_archives_and_is_idempotent(self):
        commit, changed = prepare(self.root, "v1.2.3", self.branch, self.initial)
        self.assertTrue(changed)
        self.assertEqual(git(self.root, "rev-parse", "HEAD^"), self.initial)
        self.assertEqual(self.remote_head(), commit)
        self.assertIn("github-actions[bot]", git(self.root, "show", "-s", "--format=%an"))
        self.assertIn("[skip ci]", git(self.root, "show", "-s", "--format=%s"))
        self.assertEqual(list((self.root / "changelog/entries").iterdir()),
                         [self.root / "changelog/entries/.gitkeep"])
        self.assertTrue((self.root / "changelog/releases/v1.2.3/entries/first.md").is_file())
        self.assertIn("First change", (self.root / "changelog/releases/v1.2.3.md").read_text())
        self.assertEqual(prepare(self.root, "v1.2.3", self.branch, commit), (commit, False))

    def test_later_pushes_include_earlier_entries_but_next_versions_do_not(self):
        prepare(self.root, "v1.2.3", self.branch, self.initial)
        self.entry("second", "Second change")
        head = self.commit("Another change")
        git(self.root, "push", "origin", self.branch)
        prepare(self.root, "v1.2.3", self.branch, head)
        notes = (self.root / "changelog/releases/v1.2.3.md").read_text()
        self.assertEqual(notes.count("First change"), 1)
        self.assertEqual(notes.count("Second change"), 1)
        git(self.root, "switch", "-c", "release/v1.2.4")
        self.entry("third", "Next release")
        head = self.commit("Next change")
        prepare(self.root, "v1.2.4", "release/v1.2.4", head)
        notes = (self.root / "changelog/releases/v1.2.4.md").read_text()
        self.assertIn("Next release", notes)
        self.assertNotIn("First change", notes)
        self.assertNotIn("Second change", notes)

    def test_invalid_entries_leave_the_branch_and_remote_unchanged(self):
        self.entry("invalid", "INVALID")
        head = self.commit("Invalid entry")
        git(self.root, "push", "origin", self.branch)
        with self.assertRaises(subprocess.CalledProcessError):
            prepare(self.root, "v1.2.3", self.branch, head)
        self.assertEqual(git(self.root, "rev-parse", "HEAD"), head)
        self.assertEqual(self.remote_head(), head)
        self.assertEqual(git(self.root, "status", "--porcelain"), "")
        self.assertFalse((self.root / "changelog/releases").exists())

    def test_a_concurrent_remote_update_is_never_overwritten(self):
        other = self.base / "other"
        subprocess.run(["git", "clone", "--branch", self.branch, str(self.remote), str(other)],
                       check=True, capture_output=True)
        (other / "another-change").write_text("Concurrent change")
        git(other, "add", ".")
        git(other, "-c", "user.name=Test", "-c", "user.email=test@example.com",
            "commit", "-m", "Concurrent change")
        git(other, "push", "origin", self.branch)
        concurrent = git(other, "rev-parse", "HEAD")
        with self.assertRaisesRegex(ValueError, "release branch changed"):
            prepare(self.root, "v1.2.3", self.branch, self.initial)
        self.assertEqual(self.remote_head(), concurrent)

    def test_retry_after_a_successful_push_reuses_the_changelog_commit(self):
        commit, _ = prepare(self.root, "v1.2.3", self.branch, self.initial)
        git(self.root, "switch", "--detach", self.initial)
        self.assertEqual(prepare(self.root, "v1.2.3", self.branch, self.initial), (commit, True))
        self.assertEqual(self.remote_head(), commit)
        self.assertEqual(git(self.root, "rev-parse", "HEAD"), commit)

    def test_only_the_matching_release_branch_and_commit_can_be_changed(self):
        for version, branch, head in [("../escape", self.branch, self.initial),
                                      ("v1.2.3", "main", self.initial),
                                      ("v1.2.3", self.branch, "0" * 40)]:
            with self.subTest(version=version, branch=branch, head=head), self.assertRaises(ValueError):
                prepare(self.root, version, branch, head)
        self.assertEqual(self.remote_head(), self.initial)


if __name__ == "__main__":
    unittest.main()
