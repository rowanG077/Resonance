"""Read a prepared release's notes from the top-level changelog."""

import argparse
from pathlib import Path
import re


def release_notes(changelog, version):
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("expected vX.X.X")
    headings = list(re.finditer(r"^## .+$", changelog, re.MULTILINE))
    matching = [index for index, heading in enumerate(headings)
                if re.match(rf"^## {re.escape(version[1:])}(?:\s|$)", heading.group())]
    if len(matching) != 1:
        raise ValueError(f"CHANGELOG.md must contain exactly one section for {version}")
    index = matching[0]
    heading = headings[index]
    end = headings[index + 1].start() if index + 1 < len(headings) else len(changelog)
    if not changelog[heading.end():end].strip():
        raise ValueError(f"CHANGELOG.md has no release notes for {version}")
    return changelog[heading.start():end].strip() + "\n"


def prepared_release_notes(root, version):
    entries = root / "changelog/entries"
    if entries.exists() and any(not path.name.startswith(".") for path in entries.iterdir()):
        raise ValueError("Consolidate pending changelog entries locally before releasing")
    return release_notes((root / "CHANGELOG.md").read_text(), version)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    args = parser.parse_args()
    print(prepared_release_notes(Path.cwd(), args.version), end="")


if __name__ == "__main__":
    main()
