"""Consolidate release entries and append a changelog commit to its branch."""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def git(root, *arguments):
    return subprocess.check_output(["git", *arguments], cwd=root, text=True).strip()


def prepare(root, version, branch, expected):
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("expected vX.X.X")
    if branch != f"release/{version}":
        raise ValueError("changelog commits are restricted to the matching release branch")
    if git(root, "rev-parse", "HEAD") != expected:
        raise ValueError("checkout does not match the planned release commit")
    if git(root, "status", "--porcelain"):
        raise ValueError("release checkout must be clean")

    planned = expected
    message = f"Update changelog for {version} [skip ci]"
    remote = git(root, "ls-remote", "origin", f"refs/heads/{branch}")
    if remote and remote.split()[0] != expected:
        # Recover if the push succeeded but dispatching CI failed. Only reuse
        # our direct changelog-only child; unrelated branch updates must fail.
        git(root, "fetch", "origin", f"refs/heads/{branch}")
        candidate = git(root, "rev-parse", "FETCH_HEAD")
        paths = git(root, "diff", "--name-only", expected, candidate).splitlines()
        if (git(root, "show", "-s", "--format=%P", candidate) != expected
                or git(root, "show", "-s", "--format=%s", candidate) != message
                or not all(path.startswith("changelog/entries/")
                           or path == f"changelog/releases/{version}.md"
                           or path.startswith(f"changelog/releases/{version}/entries/")
                           for path in paths)):
            raise ValueError("release branch changed; retry CI for its new head")
        git(root, "switch", "--detach", candidate)
        expected = candidate

    entries = root / "changelog/entries"
    archive = root / f"changelog/releases/{version}/entries"
    notes = root / f"changelog/releases/{version}.md"
    pending = sorted(path for path in entries.iterdir() if not path.name.startswith("."))
    if not pending and notes.is_file():
        return expected, expected != planned

    # Retain versioned fragments so later pushes can regenerate complete notes
    # without repeating entries from older releases or losing earlier changes.
    with tempfile.TemporaryDirectory() as temporary:
        staging = Path(temporary)
        staged_entries = staging / "changelog/entries"
        staged_entries.mkdir(parents=True)
        shutil.copyfile(root / "changelog/config.yaml", staging / "changelog/config.yaml")
        archived = sorted(archive.iterdir()) if archive.exists() else []
        for source in archived + pending:
            target = staged_entries / source.name
            if target.exists() and target.read_bytes() != source.read_bytes():
                raise ValueError(f"conflicting changelog entries: {source.name}")
            shutil.copyfile(source, target)
        subprocess.run(["qlog", "check", "--all"], cwd=staging, check=True)
        output = staging / "release.md"
        subprocess.run(["qlog", "collect", "--version", version.removeprefix("v"),
                        "--out", str(output)], cwd=staging, check=True)
        if not output.is_file() or not output.stat().st_size:
            raise ValueError("qlog produced no release notes")
        notes.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(output, notes)

    if pending:
        archive.mkdir(parents=True, exist_ok=True)
        for source in pending:
            shutil.copyfile(source, archive / source.name)
            source.unlink()
    git(root, "add", "--", "changelog/entries", "changelog/releases")
    git(root, "-c", "user.name=github-actions[bot]",
        "-c", "user.email=41898282+github-actions[bot]@users.noreply.github.com",
        "commit", "-m", message)
    # A normal push rejects a concurrent branch update. Never overwrite it.
    git(root, "push", "origin", f"HEAD:refs/heads/{branch}")
    return git(root, "rev-parse", "HEAD"), True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--branch", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    commit, changed = prepare(Path.cwd(), args.version, args.branch, args.commit)
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"commit={commit}\nchanged={str(changed).lower()}\n")


if __name__ == "__main__":
    main()
