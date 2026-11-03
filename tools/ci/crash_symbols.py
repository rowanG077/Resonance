"""Analyze crash sessions, generate release symbols, and verify crash debugging."""

import argparse
from bisect import bisect_left
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import subprocess
import shutil
import sys
import tempfile
import tarfile
import time
import zipfile

from package_release import strip_executable

ROOT = Path(__file__).resolve().parents[2]


def generate(binary, output):
    """Keep native debug data available; emit portable symbols with inline frames."""
    binary = binary.resolve()
    sources = [binary]
    if sys.platform == "darwin" and binary.suffix.lower() != ".exe":
        # Rebuild from this exact executable, never reuse a previous dSYM.
        subprocess.run(["dsymutil", str(binary), "-o", str(binary) + ".dSYM"], check=True)
        # Merge the executable's unwind tables with the dSYM's line tables.
        sources.append(Path(str(binary) + ".dSYM/Contents/Resources/DWARF") / binary.name)
    # dump_syms follows the PE's embedded PDB reference on Windows.
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("wb") as symbols:
        subprocess.run(["dump_syms", "--inlines", "--check-cfi", *map(str, sources)], stdout=symbols, check=True)
    with output.open(encoding="utf-8") as symbols:
        module = symbols.readline().startswith("MODULE ")
        found = set()
        for line in symbols:
            for record in ("FILE ", "FUNC ", "STACK "):
                if line.startswith(record): found.add(record)
            if len(found) == 3: break
        if not module or len(found) != 3:
            raise RuntimeError(f"missing module identity, source lines, functions or unwind tables in {output}")
    return output


def source_range(function):
    source = (ROOT / "apps/resonance/src/crash_dumps.rs").read_text()
    fixture = re.search(rf"\bfn {function}\([^)]*\)[^{{]*\{{.*?^    \}}", source, re.S | re.M)
    if fixture is None:
        raise RuntimeError(f"cannot locate {function} in source")
    return source.count("\n", 0, fixture.start()) + 1, source.count("\n", 0, fixture.end()) + 1


def check_crash_frame(report):
    """Require both the fault location and a reliably unwound application caller."""
    frames = report["threads"][report["crash_info"]["crashing_thread"]]["frames"]
    frame = frames[0] if frames else {}
    first, last = source_range("native_crash")
    if not (frame.get("trust") == "context"
            and "native_crash" in (frame.get("function") or "")
            and (frame.get("file") or "").replace("\\", "/").endswith("apps/resonance/src/crash_dumps.rs")
            and first <= (frame.get("line") or 0) <= last):
        raise RuntimeError(f"exception-context frame did not resolve to the application's source: {frame}")
    first, last = source_range("self_test")
    if not any(frame.get("trust") in ("cfi", "frame_pointer")
               and "native::self_test" in (frame.get("function") or "")
               and (frame.get("file") or "").replace("\\", "/").endswith("apps/resonance/src/crash_dumps.rs")
               and first <= (frame.get("line") or 0) <= last for frame in frames[1:]):
        raise RuntimeError(f"application caller did not unwind reliably: {frames[:8]}")


def check_debug_id(text, symbols):
    """Text-only reports must identify the same executable as portable symbols."""
    with symbols.open(encoding="utf-8") as stream:
        module = stream.readline().split()
    identity = re.search(r"^Executable debug ID: ([0-9A-F]+)$", text, re.M)
    expected = module[3] if len(module) >= 5 and module[0] == "MODULE" else None
    observed = identity[1] if identity else None
    if expected is None or observed != expected:
        raise RuntimeError(f"crash report executable debug ID {observed!r} does not match symbols {expected!r}")
    return module


def symbolize_text(text, symbols):
    """Resolve executable frames using portable symbols on any host, including PDB-derived symbols."""
    module = check_debug_id(text, symbols)
    instruction_module = re.search(r"^Instruction module: (.+)$", text, re.M)
    text = text.split("\nRecent log:\n", 1)[0]
    base = re.search(r"^Executable image base: (0x[0-9a-fA-F]+)$", text, re.M)
    if base is None:
        raise RuntimeError("report has no executable load address")
    addresses = [(int(address, 16), 1) for address in
                 re.findall(r"^\s*\d+:\s*(0x[0-9a-fA-F]+)", text, re.M)]
    pc = re.search(r"^PC: (0x[0-9a-fA-F]+)$", text, re.M)
    if pc:
        if instruction_module and Path(instruction_module[1]).name != module[4]:
            raise RuntimeError(f"fault is in {instruction_module[1]}; its symbols are required")
        addresses = [(int(pc[1], 16), 0)]  # Fault PC is not a return address.
    if not addresses:
        raise RuntimeError("report has no frame addresses")
    offsets = sorted({address - adjust - int(base[1], 16) for address, adjust in addresses})
    files, resolved, active, callsites = {}, {}, [], {}
    with symbols.open(encoding="utf-8") as stream:
        for line in stream:
            if line.startswith("FILE "):
                _, index, path = line.rstrip().split(" ", 2)
                files[int(index)] = path
            elif line.startswith("FUNC "):
                fields = line[5:].removeprefix("m ").rstrip().split(maxsplit=3)
                start, size = int(fields[0], 16), int(fields[1], 16)
                active = offsets[bisect_left(offsets, start):bisect_left(offsets, start + size)]
                callsites = {}
                for offset in active:
                    resolved[offset] = (fields[3], None, None)
            elif active and line.startswith("INLINE 0 "):
                # Report the physical function's callsite, rather than pairing
                # its name with a line from an inlined standard-library helper.
                fields = line.split()
                for start, size in zip(fields[5::2], fields[6::2]):
                    for offset in active:
                        if int(start, 16) <= offset < int(start, 16) + int(size, 16):
                            callsites[offset] = (int(fields[3]), int(fields[2]))
                            resolved[offset] = (resolved[offset][0], *callsites[offset])
            elif active and line[0] in "0123456789abcdefABCDEF":
                fields = line.split()
                if len(fields) == 4:
                    start, size = int(fields[0], 16), int(fields[1], 16)
                    for offset in active:
                        if start <= offset < start + size and offset not in callsites:
                            resolved[offset] = (resolved[offset][0], int(fields[3]), int(fields[2]))
    result = []
    for address, adjust in addresses:
        frame = resolved.get(address - adjust - int(base[1], 16))
        if frame:
            function, file, line = frame
            result.append(f"{address:#x} {function} ({files.get(file, '?')}:{line or '?'})")
    if not result:
        raise RuntimeError("no executable frames resolved with these symbols")
    return "\n".join(result) + "\n"


def analyze(session, archive):
    """Analyze a received session without executing any archived program."""
    reports = sorted(session.glob("crash.*.txt"))
    dump = session / "crash.dmp"
    if not reports and not dump.is_file():
        raise RuntimeError(f"no crash reports or minidump in {session}")
    with tempfile.TemporaryDirectory() as temporary:
        symbols = Path(temporary) / "resonance.sym"
        unpack_file(archive, symbols.name, symbols)
        texts = []
        for report in reports:
            try:
                text = report.read_text(encoding="utf-8")
            except (OSError, UnicodeError) as error:
                print(f"Could not read {report.name}: {error}; continuing with other artifacts.")
                continue
            # An incomplete report must not hide a usable dump. Still reject
            # explicit mismatches before producing any symbolicated output.
            if re.search(r"^Executable debug ID: [0-9A-F]+$", text, re.M):
                check_debug_id(text, symbols)
            texts.append((report, text))
        for report, text in texts:
            print(f"--- {report.name} ---\n{text}")
            try:
                print(symbolize_text(text, symbols))
            except RuntimeError as error:
                print(f"Source lookup unavailable: {error}")
        if dump.is_file():
            sys.stdout.flush()
            result = stackwalk(dump, symbols)
            if result.returncode:
                print("Minidump stackwalk failed; use the text report and any resolved fault PC above.")


def stackwalk(dump, symbols, output=None):
    """Use the same portable symbol reader for every target OS and CPU."""
    command = ["minidump-stackwalk", str(dump), str(symbols)]
    if output is None:
        return subprocess.run(command, check=False)
    with output.open("wb") as report:
        subprocess.run([command[0], "--json", *command[1:]], stdout=report, check=True)
    return json.loads(output.read_text(encoding="utf-8"))


def crash(binary, root, mode, stderr, *, runner=None):
    environment = dict(os.environ, RUST_LOG="off")
    with stderr.open("wb") as stream:
        command = ([str(runner)] if runner else []) + [str(binary), "--crash-self-test", str(root), mode]
        result = subprocess.run(command,
                                env=environment, stdout=subprocess.DEVNULL, stderr=stream, timeout=45)
    if result.returncode == 0: raise RuntimeError("crash self-test unexpectedly returned successfully")
    sessions = list(root.glob("session-*"))
    report = "crash.0.txt" if mode == "panic" else "crash.native.txt"
    if len(sessions) != 1 or not (sessions[0] / report).is_file():
        raise RuntimeError(f"missing {mode} crash session; inspect {stderr}")
    return sessions[0]


def verify_analysis(output):
    """Resolve collected fixtures on any host using only the shipped .sym file."""
    symbols = output / "resonance.sym"
    with symbols.open(encoding="utf-8") as stream:
        module = stream.readline().split()
    platform = module[1].lower()
    text = (output / "session/crash.native.txt").read_text(encoding="utf-8")
    check_debug_id(text, symbols)
    report = stackwalk(output / "session/crash.dmp", symbols, output / "stackwalk.json")
    check_crash_frame(report)

    text = (output / "panic-session/crash.0.txt").read_text(encoding="utf-8")
    resolved = symbolize_text(text, symbols)
    first, last = source_range("panic_crash")
    lines = re.findall(r"panic_crash[^\n]*crash_dumps\.rs:(\d+)", resolved)
    if not any(first <= int(line) <= last for line in lines):
        raise RuntimeError(f"text panic did not resolve to the application's source: {resolved}")
    if (output / "panic-session/crash.dmp").exists():
        raise RuntimeError("Rust panic unexpectedly produced a native dump")
    (output / "panic-source.txt").write_text(resolved, encoding="utf-8")

    if platform == "windows":
        report = stackwalk(output / "overflow-session/crash.dmp", symbols, output / "overflow-stackwalk.json")
        frames = report["threads"][report["crash_info"]["crashing_thread"]]["frames"][:32]
        if not any(frame.get("trust") in ("context", "cfi", "frame_pointer")
                   and "crash_dumps::native::stack_overflow" in (frame.get("function") or "") for frame in frames):
            raise RuntimeError(f"stack overflow did not resolve to the application: {frames}")
    elif platform == "linux":
        if int(report["crash_info"]["address"], 16) != 1:
            raise RuntimeError("minidump lost the fault address (expected 0x1)")
        text = (output / "overflow-session/crash.native.txt").read_text(encoding="utf-8")
        # Also check this fallback when stack dumping succeeds: it must be
        # usable independently of a dump or an ELF-aware debugger.
        if not (output / "overflow-session/memory-maps.txt").is_file():
            raise RuntimeError("missing independent CPU/module report")
        resolved = symbolize_text(text, symbols)
        if "stack_overflow" not in resolved:
            raise RuntimeError(f"saved fallback PC did not identify the overflowing function: {resolved}")
        (output / "overflow-source.txt").write_text(resolved, encoding="utf-8")
    print(f"{module[1]} {module[2]} crashes resolved with portable symbols on {sys.platform}; evidence: {output}")


@contextmanager
def temporary_installation(binary, output):
    """Copy an executable and allow its Windows crash monitor to exit before cleanup."""
    temporary = tempfile.TemporaryDirectory(dir=output)
    try:
        installed = Path(temporary.name) / binary.name
        shutil.copy2(binary, installed)
        yield installed
    finally:
        deadline = time.monotonic() + 5
        while True:
            try:
                temporary.cleanup()
                break
            except PermissionError:
                # Windows keeps the executable locked until the monitor exits.
                if sys.platform != "win32" or time.monotonic() >= deadline:
                    raise
                time.sleep(0.05)


def verify(output, binary=None, symbols=None, runner=None):
    """Collect real application crashes, then verify the portable analysis path."""
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    build_distribution = binary is None
    if build_distribution:
        build = subprocess.run(["cargo", "build", "--locked", "--release", "-p", "resonance", "--message-format=json"],
                               cwd=ROOT, capture_output=True, text=True)
        if build.returncode:
            print(build.stdout)
            print(build.stderr, file=sys.stderr)
            build.check_returncode()
        binary = next(Path(message["executable"]) for message in map(json.loads, build.stdout.splitlines())
                      if message.get("reason") == "compiler-artifact" and message.get("executable")
                      and message.get("target", {}).get("name") == "resonance")
    binary = binary.resolve()
    portable = output / "resonance.sym"
    if symbols is None:
        generate(binary, portable)
    elif symbols.resolve() != portable:
        shutil.copyfile(symbols, portable)
    if build_distribution:
        # Keep native debug data outside the disposable user installation.
        # PR smoke checks use the same stripping/signing as release packaging.
        with temporary_installation(binary, output) as installed:
            host = "windows-x64" if binary.suffix.lower() == ".exe" else (
                "macos-arm64" if sys.platform == "darwin" else "linux")
            strip_executable(installed, host)
            return verify(output, installed, portable, runner=runner)
    modes = [("access-violation", "session", "crash.stderr"),
             ("panic", "panic-session", "panic.stderr")]
    if binary.suffix.lower() == ".exe" or sys.platform.startswith("linux"):
        modes.append(("stack-overflow", "overflow-session", "overflow.stderr"))
    for mode, destination, stderr in modes:
        with temporary_installation(binary, output) as installed:
            # No adjacent PDB, dSYM or developer executable. This is also the
            # self-contained fixture consumed by the Linux analysis CI job.
            root = installed.parent
            session = crash(installed, root / "reports", mode, output / stderr, runner=runner)
            destination = output / destination
            if destination.exists():
                shutil.rmtree(destination)
            shutil.copytree(session, destination)
    verify_analysis(output)


def unpack_file(archive, filename, destination):
    # Extract only a requested member. A suffix containing directories can
    # distinguish the dSYM DWARF file from the same-named game executable.
    def matches(name):
        return name.endswith("/" + filename) if "/" in filename else Path(name).name == filename
    if zipfile.is_zipfile(archive):
        with zipfile.ZipFile(archive) as bundle:
            names = [name for name in bundle.namelist() if matches(name)]
            if len(names) != 1: raise RuntimeError(f"expected exactly one {filename} in {archive}")
            with bundle.open(names[0]) as source, destination.open("wb") as output:
                shutil.copyfileobj(source, output)
    else:
        with tarfile.open(archive) as bundle:
            files = [member for member in bundle.getmembers() if member.isfile() and matches(member.name)]
            if len(files) != 1: raise RuntimeError(f"expected exactly one {filename} in {archive}")
            with bundle.extractfile(files[0]) as source, destination.open("wb") as output:
                shutil.copyfileobj(source, output)


def verify_archives(game, symbols, output, runner=None):
    """Verify the executable and symbols extracted from the exact release assets."""
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        name = "resonance.exe" if zipfile.is_zipfile(game) else "resonance"
        binary, portable = root / name, root / "resonance.sym"
        unpack_file(game, name, binary)
        binary.chmod(0o755)
        importer = root / ("resonance-import.exe" if name.endswith(".exe") else "resonance-import")
        unpack_file(game, importer.name, importer)
        importer.chmod(0o755)
        command = ([str(runner)] if runner else []) + [str(importer), "--help"]
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=30)
        unpack_file(symbols, "resonance.sym", portable)
        verify(output, binary, portable, runner=runner)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", type=Path, metavar="DIRECTORY")
    parser.add_argument("--verify-analysis", type=Path, metavar="DIRECTORY", help="verify collected crash fixtures on any host; never runs the target executable")
    parser.add_argument("--analyze", type=Path, metavar="SESSION", help="analyze a received session with --symbols-archive")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--symbols", type=Path, help="use an existing matching symbol file")
    parser.add_argument("--runner", type=Path, help="executable wrapper for cross-platform verification (no shell parsing)")
    parser.add_argument("--game-archive", type=Path)
    parser.add_argument("--symbols-archive", type=Path)
    args = parser.parse_args()
    if args.verify_analysis:
        verify_analysis(args.verify_analysis)
    elif args.analyze and args.symbols_archive:
        analyze(args.analyze, args.symbols_archive)
    elif args.verify and args.game_archive and args.symbols_archive:
        verify_archives(args.game_archive, args.symbols_archive, args.verify, runner=args.runner)
    elif args.verify:
        verify(args.verify, args.binary, args.symbols, runner=args.runner)
    elif args.binary and args.output:
        generate(args.binary, args.output)
    else:
        parser.error("provide --analyze SESSION --symbols-archive FILE, --verify DIRECTORY, --verify-analysis DIRECTORY, or --binary FILE --output FILE")


if __name__ == "__main__":
    main()
