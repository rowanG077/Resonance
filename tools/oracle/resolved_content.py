"""Use the production startup assembler for granular documents and oracle pins.

There is deliberately no second implementation of collection/ordering policies.
Ordinary recordings, observer output and native evidence remain direct files.

A granular document has no file of its own. Its pin names the logical location
in the library with the hash of the assembled document, so reading or checking
the pin assembles the library again instead of trusting a retained copy.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess


class ResolvedContent:
    def __init__(self, root):
        executable = os.environ.get("RESONANCE_IMPORT")
        if executable:
            command = [executable]
        else:
            command = ["cargo", "run", "--quiet", "--offline", "-p", "resonance-import-cli", "--"]
        command += ["resolve", "--assets", str(root)]
        completed = subprocess.run(command, cwd=Path(__file__).resolve().parents[2], check=True,
                                   stdout=subprocess.PIPE, text=True)
        self.documents = json.loads(completed.stdout)["documents"]
        self.encoded = {}

    def read(self, path):
        if path not in self.encoded:
            self.encoded[path] = json.dumps(self.documents[path], ensure_ascii=False, sort_keys=True,
                                            separators=(",", ":"), allow_nan=False).encode()
        return self.encoded[path]


_libraries = {}


def library(root):
    """Assemble each library once per process; every reader shares the result."""
    root = Path(root).resolve(strict=True)
    if root not in _libraries:
        _libraries[root] = ResolvedContent(root)
    return _libraries[root]


def read_bytes(path):
    """Read a physical file, or the assembled form of a granular document."""
    path = Path(path).resolve()
    if path.is_file():
        return path.read_bytes()
    for parent in path.parents:
        if (parent / "content.json").is_file():
            logical = path.relative_to(parent).as_posix()
            if logical in library(parent).documents:
                return library(parent).read(logical)
            break
    raise FileNotFoundError(path)


def read_json(path):
    return json.loads(read_bytes(path))


def digest(path):
    return hashlib.sha256(read_bytes(path)).hexdigest()


def pin(path):
    return dict(path=str(Path(path).resolve()), sha256=digest(path))
