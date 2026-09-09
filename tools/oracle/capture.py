#!/usr/bin/env python3
"""Run a bounded Dolphin replay in a fresh user directory and preserve evidence.

Uses the regular launcher: dolphin-emu-nogui parses --movie but ignores it.
No retail bytes, recordings, or captures belong in the source repository.
Audio is recorded to WAV files with speaker output disabled for unattended runs.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import wave


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def stop(process):
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def audio_evidence(root):
    """Inspect finalized PCM files without ever playing or opening an audio device."""
    recordings = []
    for path in sorted((root / "user" / "Dump" / "Audio").glob("*.wav")):
        info = {"path": str(path.relative_to(root)), "sha256": sha256(path)}
        try:
            with wave.open(str(path), "rb") as recording:
                frames = 0
                nonzero = False
                stride = recording.getnchannels() * recording.getsampwidth()
                while chunk := recording.readframes(65536):
                    frames += len(chunk) // stride
                    nonzero |= any(chunk)
                info.update(sample_rate=recording.getframerate(), channels=recording.getnchannels(),
                            frames=frames, nonzero_pcm=nonzero,
                            finalized=frames == recording.getnframes())
        except (wave.Error, EOFError, OSError) as error:
            info.update(finalized=False, error=str(error))
        recordings.append(info)
    return recordings


def press_key(env, *keys):
    subprocess.run(["xdotool", "keydown", *keys], env=env, check=True)
    try:
        # A complete emulation input poll must observe the key-down state.
        time.sleep(0.15)
    finally:
        subprocess.run(["xdotool", "keyup", *reversed(keys)], env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--movie", type=Path, required=True)
    parser.add_argument("--initial-state", type=Path,
                        help="Resume a recorded checkpoint; the DTM must retain its input prefix")
    parser.add_argument("--output", type=Path, required=True)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--frame", type=int, help="Stop after this dumped PNG index")
    target.add_argument("--watch-vis", type=int,
                        help="Record this many VI observations and audio, without PNG dumping")
    parser.add_argument("--dolphin", default="dolphin-emu")
    parser.add_argument("--backend", default="OGL")
    parser.add_argument("--timeout", type=float, default=240)
    parser.add_argument("--xvfb", action="store_true", help="Linux isolated virtual display")
    parser.add_argument("--save-state", action="store_true", help="Save a nearby paused checkpoint; requires --xvfb")
    parser.add_argument("--keep-frames", action="store_true", help="Retain every intermediate frame (large)")
    parser.add_argument("--cpu-clock", type=float, default=1.0,
                        help="Diagnostic emulated CPU multiplier; non-default runs are separate evidence")
    parser.add_argument("--fast-disc", action="store_true",
                        help="Diagnostic unlimited disc speed; never changes the baseline configuration")
    parser.add_argument("--trace-startup", action="store_true",
                        help="Diagnostic read-only GDB trace through the first 12 movie frames")
    parser.add_argument("--watch-state", action="store_true",
                        help="Record named game words every VI without enabling the debugger")
    parser.add_argument("--watch-actor", type=int, action="append", default=[],
                        help="Observe an actor's turn in the initial state's field; repeat up to eight times")
    parser.add_argument("--watch-volume-group", type=int, action="append", default=[],
                        help="Observe a music/effect volume envelope (0..31); repeat up to eight times")
    args = parser.parse_args()
    if (args.frame or args.watch_vis or 0) < 1 or args.timeout <= 0:
        parser.error("frame and timeout must be positive")
    if args.watch_vis is not None:
        args.watch_state = True
        if args.save_state:
            parser.error("--save-state requires a --frame capture")
    if args.watch_actor:
        if not args.initial_state or len(args.watch_actor) > 8:
            parser.error("--watch-actor requires an initial state and at most eight actors")
        args.watch_state = True
    if args.watch_volume_group:
        if len(args.watch_volume_group) > 8 or any(not 0 <= g < 32 for g in args.watch_volume_group):
            parser.error("--watch-volume-group requires at most eight group IDs in 0..31")
        args.watch_state = True
    if not math.isfinite(args.cpu_clock) or not 0.1 <= args.cpu_clock <= 16:
        parser.error("cpu-clock must be between 0.1 and 16")
    if args.save_state and not args.xvfb:
        parser.error("--save-state requires the isolated virtual display")
    if args.trace_startup and args.watch_state:
        parser.error("keep debugger traces separate from ordinary MemoryWatcher evidence")
    disc, movie = args.disc.resolve(strict=True), args.movie.resolve(strict=True)
    if movie.read_bytes()[:10] != b"DTM\x1aGQSEAF":
        parser.error("the no-blur Gecko profile supports only GQSEAF DTMs")
    output = args.output.resolve()
    if output.exists():
        parser.error("output already exists; use a fresh directory for each run")
    output.mkdir(parents=True)
    config = output / "user" / "Config"
    shutil.copytree(Path(__file__).parent / "config", config)
    game_settings = output / "user" / "GameSettings"
    shutil.copytree(Path(__file__).parent / "game-settings", game_settings)
    if args.xvfb:
        # Pin the virtual-display keyboard instead of relying on device defaults
        # chosen before Dolphin creates its render window.
        (config / "Hotkeys.ini").write_text(
            "[Hotkeys]\nDevice = XInput2/0/Virtual core pointer\n"
            "General/Toggle Pause = F10\nSave State/Save State Slot 1 = F8\n")
    shutil.copyfile(movie, output / "input.dtm")
    initial_state = None
    # Observe that Gecko actually installed the two return instructions. This
    # remains read-only; the enabled game profile performs the requested patch.
    actor_locations = {"80023c10": "focus_patch_word", "8003efa4": "secondary_blur_patch_word"}
    # fn_80137FD0 / fn_8013769C: read the envelope itself so voice overlap
    # in a mixed PCM recording cannot conceal an incorrect music fade.
    for group in set(args.watch_volume_group):
        for index, name in enumerate(["value", "target", "previous", "progress", "step"]):
            address = 0x8030817c + group * 0x30 + index * 4
            actor_locations[f"{address:08x}"] = f"volume_{group}_{name}_bits"
    if args.watch_volume_group:
        actor_locations["8035ac70"] = "synth_clock_hi"
        actor_locations["8035ac74"] = "synth_clock_lo"
    if args.initial_state:
        initial_state = args.initial_state.resolve(strict=True)
        if movie.read_bytes()[12] != 1:
            parser.error("initial-state requires a DTM fixture with from_save_state=true")
        companion = Path(str(initial_state) + ".dtm")
        if not companion.is_file():
            parser.error("initial-state requires its recorded .dtm companion for prefix validation")
        from state import inspect
        observation = inspect(initial_state)
        prefix_end = 256 + observation["movie"]["input_byte"]
        recorded = companion.read_bytes()
        replay = movie.read_bytes()
        if len(recorded) < prefix_end or len(replay) < prefix_end or recorded[256:prefix_end] != replay[256:prefix_end]:
            parser.error("DTM input before the initial checkpoint differs from its recorded history")
        # Addresses are discovered from this recorded state. The id word makes
        # a reused actor slot detectable; these observations span one field.
        actors = [observation["controlled_actor"], *observation.get("actors", [])]
        for actor_id in set(args.watch_actor):
            matches = [a for a in actors if a["id"] == actor_id]
            if len(matches) != 1:
                parser.error(f"initial state does not uniquely identify actor {actor_id}")
            for offset, name in [(0x04, "x_bits"), (0x08, "y_bits"), (0x0c, "z_bits"),
                                 (0x40, "heading_bits"), (0x74, "target_bits"),
                                 (0x78, "step_bits"), (0x80, "speed_bits"),
                                 (0x94, "behavior_word"), (0xb8, "id"),
                                 (0xc8, "eye_mouth_mode_word"),
                                 (0xcc, "mouth_texture_timer_word"),
                                 (0x794, "destination_x_bits"),
                                 (0x798, "destination_y_bits"),
                                 (0x79c, "destination_z_bits")]:
                actor_locations[f'{matches[0]["address"] + offset:08x}'] = f"actor_{actor_id}_{name}"
        if args.watch_actor:
            for window in observation.get("dialogue_windows", []):
                actor_locations[f'{window["address"] + 0x13540:08x}'] = f'dialogue_{window["slot"]}_status_word'
            for address, name in [(0x8035a40c, "fade_alpha_bits"),
                                  (0x802c8ee8, "camera_motion_flags_word"),
                                  (0x802c8f20, "camera_position_speed_hi"),
                                  (0x802c8f24, "camera_position_speed_lo"),
                                  (0x802c8fb8, "camera_angle_speed_hi"),
                                  (0x802c8fbc, "camera_angle_speed_lo")]:
                actor_locations[f'{address:08x}'] = name
        # Movie.cpp loads MOVIE.sav and State.cpp checks its .dtm companion.
        shutil.copyfile(initial_state, output / "input.dtm.sav")
        shutil.copyfile(companion, output / "input.dtm.sav.dtm")
    executable = shutil.which(args.dolphin)
    if executable is None:
        parser.error("Dolphin not found; enter nix develop")
    # Qt constructs the application even for --version. This must also work
    # before the isolated X display exists and in unattended shells.
    version_env = dict(os.environ, QT_QPA_PLATFORM="offscreen")
    version = subprocess.check_output([executable, "--version"], env=version_env,
                                      stderr=subprocess.PIPE, text=True).strip()
    metadata = {
        "dolphin_version": version,
        "dolphin_binary_sha256": sha256(Path(executable).resolve()),
        "disc_sha256": sha256(disc), "movie_sha256": sha256(movie),
        "initial_state_sha256": sha256(initial_state) if initial_state else None,
        "configs": {p.name: sha256(p) for p in config.iterdir()},
        "game_settings": {p.name: sha256(p) for p in game_settings.iterdir()},
        "presentation": {"profile": "no-blur-v1", "disable_copy_filter": True,
                         "gecko": "Remove Blur", "gecko_verified": None},
        "backend": args.backend, "requested_frame": args.frame,
        "requested_vi_samples": args.watch_vis,
        "audio": {"backend": "No Audio Output", "muted": True, "dump": True},
        "timing": {"cpu_clock": args.cpu_clock, "fast_disc": args.fast_disc,
                   "diagnostic_override": args.cpu_clock != 1.0 or args.fast_disc or args.trace_startup},
        "complete": False,
    }
    # A Nix launcher is a wrapper; retain the actual executable's identity too.
    wrapped = Path(executable).resolve().with_name("." + Path(executable).name + "-wrapped")
    if wrapped.is_file():
        metadata["dolphin_wrapped_binary_sha256"] = sha256(wrapped)
    manifest = output / "capture.json"
    manifest.write_text(json.dumps(metadata, indent=2) + "\n")
    display = process = trace_directory = watch_directory = watcher = None
    env = os.environ.copy()
    try:
        if args.xvfb:
            read_fd, write_fd = os.pipe()
            with (output / "display.log").open("w") as log:
                display = subprocess.Popen(
                    ["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "800x600x24", "-nolisten", "tcp"],
                    pass_fds=(write_fd,), stdout=log, stderr=subprocess.STDOUT)
            os.close(write_fd)
            # Xvfb returns an available display number once it is listening.
            import select
            if not select.select([read_fd], [], [], 10)[0]:
                raise RuntimeError("Xvfb did not start; see display.log")
            with os.fdopen(read_fd) as pipe:
                number = pipe.readline().strip()
            if not number.isdecimal():
                raise RuntimeError("Xvfb could not allocate a display")
            env.update(DISPLAY=":" + number, QT_QPA_PLATFORM="xcb")
            metadata["isolated_display"] = env["DISPLAY"]
        user_path = output / "user"
        if args.watch_state:
            from memory_watch import Watcher
            # Dolphin derives the Unix socket path from its user directory.
            # A short alias avoids the AF_UNIX pathname limit without moving
            # the evidence/profile outside the capture directory.
            watch_directory = tempfile.TemporaryDirectory(prefix="resonance-watch-")
            user_path = Path(watch_directory.name) / "user"
            user_path.symlink_to(output / "user", target_is_directory=True)
            watcher = Watcher(user_path, output / "memory.jsonl", actor_locations)
        command = [executable, "-b", "-u", str(user_path), "-e", str(disc),
                   "-m", str(output / "input.dtm"), "-v", args.backend,
                   "-C", "Dolphin.DSP.Backend=No Audio Output",
                   "-C", "Dolphin.DSP.Muted=True",
                   "-C", "Dolphin.DSP.DumpAudio=True",
                   "-C", "Dolphin.Core.EnableCheats=True",
                   "-C", "GFX.Enhancements.DisableCopyFilter=True"]
        if initial_state:
            command += ["-s", str(output / "input.dtm.sav")]
        if args.watch_vis is not None:
            command += ["-C", "Dolphin.Movie.DumpFrames=False"]
        if args.cpu_clock != 1.0:
            command += ["-C", "Dolphin.Core.OverclockEnable=True",
                        "-C", f"Dolphin.Core.Overclock={args.cpu_clock}"]
        if args.fast_disc:
            command += ["-C", "Dolphin.Core.FastDiscSpeed=True"]
        if args.trace_startup:
            trace_directory = tempfile.TemporaryDirectory(prefix="resonance-trace-")
            trace_socket = Path(trace_directory.name) / "gdb.sock"
            command += ["-d", "-C", f"Dolphin.General.GDBSocket={trace_socket}"]
        if args.xvfb:
            command += ["-C", "Dolphin.General.HotkeysRequireFocus=False"]
        metadata["command"] = command
        with (output / "dolphin.log").open("w") as log:
            process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
        if args.trace_startup:
            from startup_trace import trace
            trace_output = output / "startup-trace.jsonl"
            metadata["startup_trace"] = trace(trace_socket, trace_output, process, args.timeout)
            metadata["startup_trace"].update(path=trace_output.name, sha256=sha256(trace_output))
        frame = (output / "user" / "Dump" / "Frames" / f"framedump_{args.frame}.png"
                 if args.frame is not None else None)
        deadline = time.monotonic() + args.timeout
        while not (frame.is_file() if frame is not None else watcher.rows >= args.watch_vis):
            if process.poll() is not None:
                raise RuntimeError(f"Dolphin exited ({process.returncode}); see dolphin.log")
            if watcher is not None and watcher.error is not None:
                raise RuntimeError(f"MemoryWatcher failed: {watcher.error}")
            if time.monotonic() > deadline:
                raise TimeoutError("Dolphin did not reach the capture target before its deadline")
            time.sleep(0.1)
        # PNG writes are asynchronous. Wait for the next frame before copying.
        if frame is not None:
            next_frame = frame.with_name(f"framedump_{args.frame+1}.png")
            while not next_frame.is_file() and time.monotonic() < deadline:
                time.sleep(0.1)
            if not next_frame.is_file():
                raise TimeoutError("Replay ended before the requested frame was confirmed complete")
            shutil.copyfile(frame, output / "reference.png")
            metadata["reference_sha256"] = sha256(output / "reference.png")
        if args.save_state:
            # These events are restricted to this script's own virtual display.
            windows = subprocess.check_output(
                ["xdotool", "search", "--onlyvisible", "--pid", str(process.pid)],
                env=env, text=True).splitlines()
            if not windows:
                raise RuntimeError("Dolphin has no visible render window for checkpoint hotkeys")
            subprocess.run(["xdotool", "windowfocus", "--sync", windows[-1]], env=env, check=True)
            press_key(env, "F10")
            time.sleep(0.2)
            press_key(env, "F8")
            state = output / "user" / "StateSaves" / "GQSEAF.s01"
            state_deadline = time.monotonic() + 10
            while not state.is_file() and time.monotonic() < state_deadline:
                time.sleep(0.1)
            if not state.is_file():
                raise RuntimeError("Dolphin did not save the requested checkpoint")
            # The checkpoint follows the requested frame; it is not claimed to
            # be the exact same tick as reference.png.
            metadata["nearby_checkpoint"] = str(state.relative_to(output))
            # Retain the last complete dumped image as a nearby visual guide.
            # It can trail the saved CPU/actor pose by a render update. Do not
            # call it an exact pose match; inspect the first resumed scene
            # frame and validate authored/secondary joints when registering.
            paused = max(frame.parent.glob("framedump_*.png"),
                         key=lambda p: int(p.stem.removeprefix("framedump_")))
            shutil.copyfile(paused, output / "checkpoint-reference.png")
            metadata["checkpoint_dump_frame"] = int(paused.stem.removeprefix("framedump_"))
            metadata["checkpoint_reference_sha256"] = sha256(output / "checkpoint-reference.png")
            metadata["checkpoint_image_alignment"] = "last pre-pause dump; may precede saved actor pose"
        if args.xvfb:
            subprocess.run(["xdotool", "search", "--pid", str(process.pid), "windowclose", "%@"],
                           env=env, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
        metadata["complete"] = True
    finally:
        stop(process)
        stop(display)
        if watcher is not None:
            metadata["memory_watch"] = watcher.finish()
            metadata["memory_watch"].update(path="memory.jsonl", sha256=sha256(output / "memory.jsonl"))
            if not metadata["memory_watch"]["complete"]:
                metadata["complete"] = False
            last = None
            with (output / "memory.jsonl").open() as samples:
                for line in samples:
                    last = json.loads(line)
            # A requested watch capture must prove that both patches are live.
            words = last or {}
            metadata["presentation"]["gecko_verified"] = all(
                words.get(name) == 0x4e800020
                for name in ("focus_patch_word", "secondary_blur_patch_word"))
            if not metadata["presentation"]["gecko_verified"]:
                metadata["complete"] = False
                metadata["presentation"]["error"] = "no-blur Gecko instructions were not observed"
        if watch_directory is not None:
            watch_directory.cleanup()
        if trace_directory is not None:
            trace_directory.cleanup()
        metadata["audio"]["recordings"] = audio_evidence(output)
        metadata["effective_configs"] = {p.name: sha256(p) for p in config.iterdir() if p.is_file()}
        if checkpoint := metadata.get("nearby_checkpoint"):
            metadata["checkpoint_sha256"] = sha256(output / checkpoint)
        if any(not recording["finalized"] for recording in metadata["audio"]["recordings"]):
            metadata["complete"] = False
            metadata["audio"]["error"] = "an audio recording was not finalized"
        if not args.keep_frames and (output / "reference.png").is_file():
            for intermediate in (output / "user" / "Dump" / "Frames").glob("framedump_*.png"):
                intermediate.unlink()
        manifest.write_text(json.dumps(metadata, indent=2) + "\n")
    if not metadata["complete"]:
        raise RuntimeError("capture evidence is incomplete; see capture.json")
    print(output / ("reference.png" if args.frame is not None else "memory.jsonl"))


if __name__ == "__main__":
    main()
