"""Exercise archive contents and publication recovery without contacting GitHub."""

import hashlib
import json
import os
import shutil
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from unittest.mock import patch

from package_release import EXECUTABLES, PLATFORMS, ROOT, package, package_symbols, strip_executable
from crash_symbols import analyze, check_crash_frame, check_debug_id, crash, source_range, symbolize_text, temporary_installation, verify_analysis, verify_archives


class ArchiveTests(unittest.TestCase):
    def test_reports_identify_the_exact_executable_including_without_a_dump(self):
        with tempfile.TemporaryDirectory() as temporary:
            symbols = Path(temporary) / "resonance.sym"
            symbols.write_text("MODULE Linux arm64 ABCD0 resonance\n")
            check_debug_id("Executable debug ID: ABCD0\n", symbols)
            for text in ["Executable debug ID: DCBA0\n", "Revision: same-revision\n",
                         "Executable debug ID: unavailable (cannot read executable)\n"]:
                with self.subTest(text=text), self.assertRaisesRegex(RuntimeError, "debug ID"):
                    check_debug_id(text, symbols)

    def test_source_resolution_requires_the_original_exception_frame(self):
        frame = dict(trust="context", function="resonance::crash_dumps::native::native_crash",
                     file="/source/apps/resonance/src/crash_dumps.rs", line=550)
        def check(frames):
            check_crash_frame(dict(crash_info=dict(crashing_thread=0), threads=[dict(frames=frames)]))
        caller = dict(frame, trust="cfi", function="resonance::crash_dumps::native::self_test")
        with patch("crash_symbols.source_range", return_value=(548, 552)):
            check([frame, caller])
            for frames in [[], [dict(frame, trust="scan")],
                           [dict(frame, function="unknown"), frame],
                           [dict(frame, line=999)], [dict(frame, file="unrelated.rs")]]:
                with self.subTest(frames=frames), self.assertRaisesRegex(RuntimeError, "exception-context"):
                    check(frames)
            for callers in [[], [dict(caller, trust="scan")], [dict(caller, trust="cfi_scan")],
                            [dict(caller, line=999)], [dict(caller, function="unrelated")]]:
                with self.subTest(callers=callers), self.assertRaisesRegex(RuntimeError, "caller did not unwind"):
                    check([frame, *callers])

    def test_portable_text_lookup_checks_identity_and_address_boundaries(self):
        with tempfile.TemporaryDirectory() as temporary:
            symbols = Path(temporary) / "resonance.sym"
            symbols.write_text("MODULE windows x86_64 ABCD0 resonance.exe\n"
                               "FILE 0 C:/source with spaces/crash_dumps.rs\n"
                               "FUNC m 100 20 0 resonance::crash_dumps::native::panic_crash\n"
                               "100 10 550 0\n110 10 551 0\n")
            text = "Executable debug ID: ABCD0\nExecutable image base: 0x1000\n"
            resolved = symbolize_text(text + "  8: 0x1110 - <unknown>\n  9: 0x2000\n", symbols)
            self.assertIn("panic_crash (C:/source with spaces/crash_dumps.rs:550)", resolved)
            # A fault PC is an instruction address, not a caller's return address.
            self.assertIn(":551)", symbolize_text(text + "PC: 0x1110\n", symbols))
            self.assertIn(":551)", symbolize_text(text + "PC: 0x1110\n\nRecent log:\n  0: 0x1101\n", symbols))
            with self.assertRaisesRegex(RuntimeError, "debug ID"):
                symbolize_text(text.replace("ABCD0", "DCBA0") + "  0: 0x1110\n", symbols)
            with self.assertRaisesRegex(RuntimeError, "no executable frames"):
                symbolize_text(text + "  0: 0x1100\n  1: 0x1121\n", symbols)
            with self.assertRaisesRegex(RuntimeError, "its symbols are required"):
                symbolize_text(text + "PC: 0x1110\nInstruction module: /lib/driver.so\n", symbols)
            symbols.write_text(symbols.read_text().replace("100 10 550 0", "INLINE 0 552 0 7 100 10\n100 10 990 0"))
            self.assertIn(":552)", symbolize_text(text + "  0: 0x1110\n", symbols))

    def test_analysis_checks_all_report_ids_before_running_stackwalker(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            session = root / "session"
            session.mkdir()
            (session / "crash.dmp").write_bytes(b"dump")
            text = "Executable debug ID: ABCD0\nExecutable image base: 0x1000\n  8: 0x1110\n"
            (session / "crash.0.txt").write_text(text)
            (session / "crash.native.txt").write_text(text)
            archive = root / "symbols.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("release/resonance.sym", "MODULE windows x86_64 ABCD0 resonance.exe\n"
                                "FILE 0 crash_dumps.rs\nFUNC 100 20 0 panic_crash\n100 20 550 0\n")
            with patch("crash_symbols.subprocess.run") as run, patch("builtins.print") as output:
                run.return_value = subprocess.CompletedProcess([], 1)
                analyze(session, archive)
                self.assertTrue(any("panic_crash (crash_dumps.rs:550)" in str(call) for call in output.call_args_list))
                command = run.call_args.args[0]
                self.assertEqual(command[:2], ["minidump-stackwalk", str(session / "crash.dmp")])
                self.assertTrue(any("stackwalk failed" in str(call) for call in output.call_args_list))
                (session / "crash.native.txt").write_text(text.replace("ABCD0", "DCBA0"))
                run.reset_mock()
                with self.assertRaisesRegex(RuntimeError, "debug ID"):
                    analyze(session, archive)
                run.assert_not_called()
                # Dump collection can succeed even when the text reporter fails.
                for report in session.glob("*.txt"):
                    report.unlink()
                analyze(session, archive)
                run.assert_called_once()

    def test_incomplete_text_reports_do_not_block_other_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            session = root / "session"
            session.mkdir()
            (session / "crash.dmp").write_bytes(b"dump")
            archive = root / "symbols.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("resonance.sym", "MODULE windows x86_64 ABCD0 resonance.exe\n")
            for mode in ("empty", "invalid-utf8", "directory", "unavailable-id"):
                with self.subTest(mode=mode):
                    report = session / "crash.native.txt"
                    if mode == "directory":
                        report.mkdir()
                    else:
                        report.write_bytes({"empty": b"", "invalid-utf8": b"\xff",
                                            "unavailable-id": b"Executable debug ID: unavailable\n"}[mode])
                    with patch("crash_symbols.subprocess.run", return_value=subprocess.CompletedProcess([], 0)) as run, \
                         patch("builtins.print"):
                        analyze(session, archive)
                        self.assertEqual(run.call_args.args[0][:2], ["minidump-stackwalk", str(session / "crash.dmp")])
                        run.assert_called_once()
                    if report.is_dir(): report.rmdir()
                    else: report.unlink()

    def test_collected_fixtures_use_portable_symbols_on_linux_for_every_target(self):
        for platform, cpu, filename in (("windows", "x86_64", "resonance.exe"),
                                        ("mac", "arm64", "resonance"),
                                        ("Linux", "x86_64", "resonance"),
                                        ("Linux", "arm64", "resonance")):
            with self.subTest(platform=platform, cpu=cpu), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                panic_line, _ = source_range("panic_crash")
                native_line, _ = source_range("native_crash")
                caller_line, _ = source_range("self_test")
                header = "Executable debug ID: ABCD0\nExecutable image base: 0x100000000\n"
                (root / "resonance.sym").write_text(
                    f"MODULE {platform} {cpu} ABCD0 {filename}\n"
                    "FILE 0 /source/apps/resonance/src/crash_dumps.rs\n"
                    "FUNC 100 20 0 resonance::crash_dumps::native::panic_crash\n"
                    f"100 20 {panic_line} 0\n"
                    "FUNC 300 20 0 resonance::crash_dumps::native::stack_overflow\n"
                    "300 20 600 0\n", encoding="utf-8")
                for directory in ("session", "panic-session", "overflow-session"):
                    (root / directory).mkdir()
                (root / "session/crash.native.txt").write_text(header, encoding="utf-8")
                (root / "session/crash.dmp").write_bytes(b"native dump")
                (root / "panic-session/crash.0.txt").write_text(header + "  8: 0x100000111\n", encoding="utf-8")
                (root / "overflow-session/crash.dmp").write_bytes(b"overflow dump")
                (root / "overflow-session/crash.native.txt").write_text(header + "PC: 0x100000310\n", encoding="utf-8")
                (root / "overflow-session/memory-maps.txt").write_text("maps", encoding="utf-8")
                frame = dict(trust="context", function="resonance::crash_dumps::native::native_crash",
                             file="/source/apps/resonance/src/crash_dumps.rs", line=native_line)
                caller = dict(frame, trust="cfi", function="resonance::crash_dumps::native::self_test", line=caller_line)
                def walk(command, *, stdout, check):
                    self.assertEqual(command[:2], ["minidump-stackwalk", "--json"])
                    self.assertEqual(command[-1], str(root / "resonance.sym"))
                    self.assertTrue(check)
                    frames = [frame, caller] if Path(command[2]).parent.name == "session" else [
                        dict(frame, function="resonance::crash_dumps::native::stack_overflow")]
                    stdout.write(json.dumps(dict(crash_info=dict(crashing_thread=0, address="0x1"),
                                                 threads=[dict(frames=frames)])).encode())
                # Fail if verification tries to launch a game, Wine, addr2line,
                # atos, dump_syms or a platform debugger on the Linux host.
                with patch("crash_symbols.sys.platform", "linux"), \
                     patch("crash_symbols.subprocess.run", side_effect=walk) as run:
                    verify_analysis(root)
                    self.assertEqual(run.call_count, 2 if platform == "windows" else 1)
                self.assertIn(f"crash_dumps.rs:{panic_line}", (root / "panic-source.txt").read_text())
                if platform == "Linux":
                    self.assertIn("stack_overflow", (root / "overflow-source.txt").read_text())

    def setUp(self):
        # Synthetic executables cannot be passed to a real platform strip tool.
        # Make stripping observable so archives must contain distribution copies
        # while the build directory and symbols archive retain the originals.
        def strip_copy(path, platform):
            if platform != "windows-x64":
                path.write_bytes(b"stripped:" + path.read_bytes())
        patcher = patch("package_release.strip_executable", side_effect=strip_copy)
        self.strip = patcher.start()
        self.addCleanup(patcher.stop)

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
                    for executable in EXECUTABLES:
                        filename = executable + suffix
                        original = b"synthetic executable"
                        self.assertEqual((binaries / filename).read_bytes(), original)
                        shipped = original if suffix else b"stripped:" + original
                        if suffix:
                            with zipfile.ZipFile(archive) as bundle:
                                self.assertEqual(bundle.read(f"{prefix}/{filename}"), shipped)
                        else:
                            with tarfile.open(archive) as bundle:
                                self.assertEqual(bundle.extractfile(f"{prefix}/{filename}").read(), shipped)

    def test_symbols_require_platform_data_and_keep_exact_executable(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "binaries"
            binaries.mkdir()
            for executable in EXECUTABLES:
                (binaries / executable).write_bytes(b"exact ELF or Mach-O")
                (binaries / f"{executable}.exe").write_bytes(b"exact PE")
            with self.assertRaises(FileNotFoundError):
                package_symbols("v1.2.3", "windows-x64", binaries, root / "dist")
            with self.assertRaises(FileNotFoundError):
                package_symbols("v1.2.3", "macos-arm64", binaries, root / "dist")
            for executable in EXECUTABLES:
                (binaries / f"{executable.replace('-', '_')}.pdb").write_bytes(b"matching PDB")
            # Portable symbols are mandatory alongside native debug data.
            with self.assertRaises(FileNotFoundError):
                package_symbols("v1.2.3", "linux-x64", binaries, root / "dist")
            symbols = b"MODULE Linux arm64 ABCD0 resonance\nFUNC 100 20 0 native_crash\n"
            for executable in EXECUTABLES:
                (binaries / f"{executable}.sym").write_bytes(symbols)
                dwarf = binaries / f"{executable}.dSYM/Contents/Resources/DWARF/{executable}"
                dwarf.parent.mkdir(parents=True)
                dwarf.write_bytes(b"matching DWARF")
            for platform in PLATFORMS:
                with patch.dict(os.environ, RELEASE_COMMIT="a" * 40, GITHUB_SHA="b" * 40):
                    archive = package_symbols("v1.2.3", platform, binaries, root / "dist")
                prefix = f"resonance-v1.2.3-{platform}-symbols"
                if platform.startswith("windows"):
                    with zipfile.ZipFile(archive) as bundle:
                        self.assertIn(b"Revision: " + b"a" * 40, bundle.read(f"{prefix}/BUILD.txt"))
                        self.assertEqual(bundle.read(f"{prefix}/resonance.sym"), symbols)
                        self.assertEqual(bundle.read(f"{prefix}/resonance.exe"), b"exact PE")
                        self.assertEqual(bundle.read(f"{prefix}/resonance.pdb"), b"matching PDB")
                        self.assertEqual(bundle.read(f"{prefix}/resonance-import.exe"), b"exact PE")
                        self.assertEqual(bundle.read(f"{prefix}/resonance_import.pdb"), b"matching PDB")
                else:
                    with tarfile.open(archive) as bundle:
                        self.assertIn(b"Revision: " + b"a" * 40, bundle.extractfile(f"{prefix}/BUILD.txt").read())
                        self.assertEqual(bundle.extractfile(f"{prefix}/resonance.sym").read(), symbols)
                        self.assertEqual(bundle.extractfile(f"{prefix}/resonance").read(), b"exact ELF or Mach-O")
                        self.assertEqual(bundle.extractfile(f"{prefix}/resonance-import").read(), b"exact ELF or Mach-O")
                        self.assertEqual(bundle.extractfile(f"{prefix}/resonance-import.sym").read(), symbols)
                        if platform.startswith("macos"):
                            self.assertEqual(bundle.extractfile(f"{prefix}/resonance.dSYM/Contents/Resources/DWARF/resonance").read(), b"matching DWARF")
                            self.assertEqual(bundle.extractfile(f"{prefix}/resonance-import.dSYM/Contents/Resources/DWARF/resonance-import").read(), b"matching DWARF")

    def test_strip_failure_does_not_publish_or_modify_build_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in EXECUTABLES:
                (root / name).write_bytes(b"original debugging artifact")
            self.strip.side_effect = subprocess.CalledProcessError(1, "strip")
            with self.assertRaises(subprocess.CalledProcessError):
                package("v1.2.3", "linux-x64", root, root / "dist")
            self.assertEqual(list((root / "dist").iterdir()), [])
            for name in EXECUTABLES:
                self.assertEqual((root / name).read_bytes(), b"original debugging artifact")

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

    def test_release_verification_uses_executable_and_symbols_from_the_archives(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = root / "binaries"
            binaries.mkdir()
            for name in ("resonance", "resonance-import", "resonance.exe", "resonance-import.exe", "resonance.pdb", "resonance_import.pdb"):
                (binaries / name).write_bytes(name.encode())
            for executable in EXECUTABLES:
                (binaries / f"{executable}.sym").write_bytes(b"exact portable symbols")
                dwarf = binaries / f"{executable}.dSYM/Contents/Resources/DWARF/{executable}"
                dwarf.parent.mkdir(parents=True)
                dwarf.write_bytes(b"exact DWARF")
            for platform in PLATFORMS:
                with self.subTest(platform=platform):
                    game = package("v1.2.3", platform, binaries, root / "dist")
                    symbols = package_symbols("v1.2.3", platform, binaries, root / "dist")
                    name = "resonance.exe" if platform.startswith("windows") else "resonance"
                    expected_runner = Path("runner with spaces") if platform.startswith("windows") else None
                    def check(output, executable, portable, runner=None):
                        self.assertEqual(runner, expected_runner)
                        shipped = name.encode() if platform.startswith("windows") else b"stripped:" + name.encode()
                        self.assertEqual(executable.read_bytes(), shipped)
                        self.assertEqual(portable.read_bytes(), b"exact portable symbols")
                        importer = "resonance-import.exe" if platform.startswith("windows") else "resonance-import"
                        shipped_importer = importer.encode() if platform.startswith("windows") else b"stripped:" + importer.encode()
                        self.assertEqual((executable.parent / importer).read_bytes(), shipped_importer)
                        # Native PDB/DWARF data is retained for deeper debugging,
                        # but routine verification needs only the portable symbols.
                        self.assertEqual({path.name for path in executable.parent.iterdir()},
                                         {name, importer, "resonance.sym"})
                    host = "darwin" if platform.startswith("macos") else "linux"
                    with patch("crash_symbols.verify", side_effect=check) as verify, \
                         patch("crash_symbols.sys.platform", host), \
                         patch("crash_symbols.subprocess.run") as help_command:
                        verify_archives(game, symbols, root / "evidence", runner=expected_runner)
                        verify.assert_called_once()
                        command = help_command.call_args.args[0]
                        self.assertEqual(command[-1], "--help")
                        self.assertEqual(Path(command[-2]).name, "resonance-import.exe" if platform.startswith("windows") else "resonance-import")
                        self.assertTrue(help_command.call_args.kwargs["check"])
                        if expected_runner:
                            self.assertEqual(command[0], str(expected_runner))

    def test_crash_runner_and_paths_are_separate_arguments(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            session = root / "session-123-4"
            session.mkdir()
            (session / "crash.native.txt").write_text("native crash")
            binary, runner = root / "game with spaces.exe", root / "runner with spaces"
            with patch("crash_symbols.subprocess.run", return_value=subprocess.CompletedProcess([], 1)) as run:
                self.assertEqual(crash(binary, root, "access-violation", root / "stderr", runner=runner), session)
                self.assertEqual(run.call_args.args[0],
                                 [str(runner), str(binary), "--crash-self-test", str(root), "access-violation"])

    def test_windows_installation_cleanup_retries_locks_but_still_times_out(self):
        remove = shutil.rmtree
        for permanent in (False, True):
            with self.subTest(permanent=permanent), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                binary = root / "resonance.exe"
                binary.write_bytes(b"executable")
                attempts = []

                def cleanup(path, *args, **kwargs):
                    attempts.append(path)
                    if permanent or len(attempts) == 1:
                        raise PermissionError("crash monitor still running")
                    remove(path, *args, **kwargs)

                with patch("crash_symbols.sys.platform", "win32"), \
                     patch("crash_symbols.shutil.rmtree", side_effect=cleanup), \
                     patch("crash_symbols.time.monotonic", side_effect=[0, 1, 6]), \
                     patch("crash_symbols.time.sleep") as sleep:
                    def install():
                        with temporary_installation(binary, root) as installed:
                            self.assertEqual(installed.read_bytes(), b"executable")
                        self.assertFalse(installed.parent.exists())

                    if permanent:
                        with self.assertRaises(PermissionError):
                            install()
                    else:
                        install()
                    self.assertEqual(len(attempts), 2)
                    sleep.assert_called_once_with(0.05)


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
    state["notes"] = Path(args[args.index("--notes-file") + 1]).read_text()
elif args[:2] == ["release", "upload"]:
    if state.get("upload_failure"):
        sys.exit(1)
    state["uploaded"] = args[3:-1]
elif args[:2] == ["release", "edit"]:
    state["draft"] = False
    state["notes"] = Path(args[args.index("--notes-file") + 1]).read_text()
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
        (self.root / "changelog/releases").mkdir(parents=True)
        self.notes = "## 1.2.3\n\n### Added\n- Session logs and crash reports.\n"
        (self.root / "changelog/releases/v1.2.3.md").write_text(self.notes)
        for platform in PLATFORMS:
            suffix = ".zip" if platform.startswith("windows") else ".tar.gz"
            (self.root / f"target/dist/resonance-v1.2.3-{platform}{suffix}").write_bytes(platform.encode())
            (self.root / f"target/dist/resonance-v1.2.3-{platform}-symbols{suffix}").write_bytes(platform.encode())
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
        self.assertEqual(len(state["uploaded"]), 10)
        self.assertEqual(state["notes"], self.notes)
        self.assertEqual(state["calls"][-1], ["release", "edit", "v1.2.3",
                                             "--notes-file", "changelog/releases/v1.2.3.md", "--draft=false"])
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
        result, state = self.run_publication(tag=state["tag"], draft=True, notes="outdated notes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(state["draft"])
        self.assertEqual(state["notes"], self.notes)
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

    def test_missing_versioned_notes_cannot_create_a_tag(self):
        (self.root / "changelog/releases/v1.2.3.md").unlink()
        result, state = self.run_publication()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(state["calls"], [])


if __name__ == "__main__":
    unittest.main()
