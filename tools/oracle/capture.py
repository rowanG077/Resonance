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


def field_locations(observation):
    """Default field watches used by paired image/party/inventory/cooking gates."""
    locations = {}
    # Skits replace script storage. Follow its live pointer instead of retaining
    # the checkpoint's addresses and reporting stale map/story words.
    for section, pointer, offset, name in [
            ("field", "8035a768", 0x10d0, "map_id"),
            ("progress", "8035a578", 0x40, "story")]:
        if section in observation:
            locations[pointer] = f"{section}_address"
            locations[f"{pointer} {offset:x}"] = name
    if "field" in observation:
        for offset in [0, 0x10, 0x14, 0x18]:
            locations[f"{0x80230724 + offset:08x}"] = f"inventory_menu_{offset:02x}_word"
        locations["8022e74c"] = "figurine_model_load_word"
        locations["8022f048"] = "figurine_opacity_word"
        for offset, name in [(0xe9d, "party_formation_first_word"),
                             (0xea1, "party_formation_last_word"),
                             (0x1e20, "party_leaders_word"),
                             (0xea8, "party_restrictions_word"),
                             (0x1e18, "cooking_known_word"),
                             (0x1e1c, "cooking_settings_word")]:
            locations[f"8035a768 {offset:x}"] = name
        for index in range(132):
            name = {10: "ex_gem_inventory_word", 124: "ex_max_inventory_word"}.get(
                index, f"inventory_{index}_word")
            locations[f"8035a768 {0xead + index * 4:x}"] = name
        for character in range(9):
            base = 0x2b8 + character * 0x118
            for offset, name in [(0x12, "vitals"), (0x1c, "conditions")]:
                locations[f"8035a768 {base + offset:x}"] = f"tech_character_{character}_{name}_word"
            for index in range(6):
                locations[f"8035a768 {base + 0xf4 + index * 4:x}"] = f"cooking_character_{character}_training_{index}_word"
    return locations


def explicit_locations(defaults, paths):
    """Apply explicit names after defaults; conflicting explicit requests fail."""
    requested = {}
    for path in paths:
        locations = json.loads(path.read_text())
        if not isinstance(locations, dict) or not all(
                isinstance(address, str) and isinstance(name, str)
                for address, name in locations.items()):
            raise ValueError("watch-locations must contain a JSON path-to-name object")
        for address, name in locations.items():
            address = " ".join(address.lower().split())
            if address in requested and requested[address] != name:
                raise ValueError(f"conflicting watcher location: {address}")
            requested[address] = name
    return defaults | requested


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
    parser.add_argument("--video", action="store_true",
                        help="With --watch-vis, also record lossless RGB video with emulated timestamps")
    parser.add_argument("--dolphin", default="dolphin-emu")
    parser.add_argument("--backend", default="OGL")
    parser.add_argument("--timeout", type=float, default=240)
    parser.add_argument("--xvfb", action="store_true", help="Linux isolated virtual display")
    parser.add_argument("--save-state", action="store_true", help="Save a nearby paused checkpoint; requires --xvfb")
    parser.add_argument("--keep-frames", action="store_true", help="Retain every intermediate frame (large)")
    parser.add_argument("--keep-duplicate-frames", action="store_true",
                        help="Dump repeated XFB presentations too, preserving their VI timing")
    parser.add_argument("--cpu-clock", type=float, default=1.0,
                        help="Diagnostic emulated CPU multiplier; non-default runs are separate evidence")
    parser.add_argument("--fast-disc", action="store_true",
                        help="Diagnostic unlimited disc speed; never changes the baseline configuration")
    parser.add_argument("--field-origin", action="store_true",
                        help="Prepare field watches without unrelated checkpoint diagnostics")
    parser.add_argument("--watch-state", action="store_true",
                        help="Record named game words every VI without enabling the debugger")
    parser.add_argument("--watch-locations", type=Path, action="append", default=[],
                        help="Add read-only MemoryWatcher pointer paths from a JSON path-to-name object")
    parser.add_argument("--watch-synopsis", action="store_true",
                        help="Observe Synopsis navigation and all saved scenario records")
    parser.add_argument("--watch-actor", type=int, action="append", default=[],
                        help="Observe an actor's turn in the initial state's field; repeat up to eight times")
    parser.add_argument("--watch-particle", type=int, action="append", default=[],
                        help="Observe a particle slot in the initial state's field; repeat up to four times")
    parser.add_argument("--watch-volume-group", type=int, action="append", default=[],
                        help="Observe a music/effect volume envelope (0..31); repeat up to eight times")
    args = parser.parse_args()
    if (args.frame or args.watch_vis or 0) < 1 or args.timeout <= 0:
        parser.error("frame and timeout must be positive")
    if args.video and args.watch_vis is None:
        parser.error("--video requires --watch-vis")
    if args.watch_vis is not None:
        args.watch_state = True
        if args.save_state:
            parser.error("--save-state requires a --frame capture")
    if args.watch_actor:
        if not args.initial_state or len(args.watch_actor) > 8:
            parser.error("--watch-actor requires an initial state and at most eight actors")
        args.watch_state = True
    if args.watch_particle:
        if (not args.initial_state or len(args.watch_particle) > 4
                or any(not 0 <= slot < 2048 for slot in args.watch_particle)):
            parser.error("--watch-particle requires an initial state and up to four slots in 0..2047")
        args.watch_state = True
    if args.watch_volume_group:
        if len(args.watch_volume_group) > 8 or any(not 0 <= g < 32 for g in args.watch_volume_group):
            parser.error("--watch-volume-group requires at most eight group IDs in 0..31")
        args.watch_state = True
    if not math.isfinite(args.cpu_clock) or not 0.1 <= args.cpu_clock <= 16:
        parser.error("cpu-clock must be between 0.1 and 16")
    if args.save_state and not args.xvfb:
        parser.error("--save-state requires the isolated virtual display")
    disc, movie = args.disc.resolve(strict=True), args.movie.resolve(strict=True)
    if movie.read_bytes()[:10] != b"DTM\x1aGQSEAF":
        parser.error("capture supports only GQSEAF DTMs")
    output = args.output.resolve()
    if output.exists():
        parser.error("output already exists; use a fresh directory for each run")
    output.mkdir(parents=True)
    config = output / "user" / "Config"
    shutil.copytree(Path(__file__).parent / "config", config)
    if args.xvfb:
        # Pin the virtual-display keyboard instead of relying on device defaults
        # chosen before Dolphin creates its render window.
        (config / "Hotkeys.ini").write_text(
            "[Hotkeys]\nDevice = XInput2/0/Virtual core pointer\n"
            "General/Toggle Pause = F10\nSave State/Save State Slot 1 = F8\n")
    shutil.copyfile(movie, output / "input.dtm")
    initial_state = None
    actor_locations = {}
    if args.watch_locations:
        args.watch_state = True
    # Read the envelope itself so voice overlap
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
        observation = inspect(initial_state, field_origin=args.field_origin,
                              actors=bool(args.watch_actor), particles=bool(args.watch_particle),
                              battle=args.watch_state)
        if args.watch_state and "battle" in observation:
            from battle_state import watch_locations
            actor_locations.update(watch_locations(observation["battle"]))
        if "battle" in observation and (args.watch_actor or args.watch_particle):
            parser.error("field actor/particle watches require a field checkpoint")
        if args.watch_state:
            actor_locations.update(field_locations(observation))
            if args.watch_synopsis:
                actor_locations["800000f8"] = "synopsis_bus_clock"
                for offset, name in [(4, "row_first"), (8, "scroll"), (12, "reading_count"),
                                     (0x1a0, "text_start"), (0x1a4, "text_end"), (0x1ac, "fade")]:
                    actor_locations[f"{0x802a2ef0 + offset:x}"] = f"synopsis_{name}_word"
                for index in range(100):
                    actor_locations[f"{0x802a2f00 + index * 4:x}"] = f"synopsis_ids_{index}_word"
                if "field" in observation:
                    for index in range(200):
                        for offset, name in [(0, "record"), (8, "time_hi"), (12, "time_lo")]:
                            actor_locations[f"8035a768 {0x1168 + index * 16 + offset:x}"] = f"synopsis_{index}_{name}_word"
        if args.watch_particle:
            pool = int(observation["particle_pool_address"], 16)
            if not 0x80000000 <= pool <= 0x81800000 - 2048 * 0x6c:
                parser.error("initial state has no valid particle pool")
            for slot in set(args.watch_particle):
                for offset, name in [(0, "timer_flags"), (4, "x_bits"), (8, "y_bits"),
                                     (12, "z_bits"), (0x10, "rx_bits"), (0x14, "ry_bits"),
                                     (0x18, "rz_bits"), (0x20, "rgba"), (0x30, "recipe"),
                                     (0x28, "size_x_bits"), (0x2c, "size_y_bits"),
                                     (0x40, "fall_bits"), (0x48, "heading_bits"),
                                     (0x54, "turn_bits")]:
                    actor_locations[f"{pool + slot * 0x6c + offset:08x}"] = f"particle_{slot}_{name}"
        prefix_end = 256 + observation["movie"]["input_byte"]
        recorded = companion.read_bytes()
        replay = movie.read_bytes()
        if len(recorded) < prefix_end or len(replay) < prefix_end or recorded[256:prefix_end] != replay[256:prefix_end]:
            parser.error("DTM input before the initial checkpoint differs from its recorded history")
        # Addresses are discovered from this recorded state. The id word makes
        # a reused actor slot detectable; these observations span one field.
        actors = observation.get("actors", [])[:]
        if "controlled_actor" in observation:
            actors.append(observation["controlled_actor"])
        for actor_id in set(args.watch_actor):
            matches = [a for a in actors if a["id"] == actor_id]
            if len(matches) != 1:
                parser.error(f"initial state does not uniquely identify actor {actor_id}")
            for offset, name in [(0x04, "x_bits"), (0x08, "y_bits"), (0x0c, "z_bits"),
                                 (0x40, "heading_bits"), (0x74, "target_bits"),
                                 (0x78, "step_bits"), (0x80, "speed_bits"),
                                 (0x94, "behavior_word"), (0xb8, "id"),
                                 (0x98, "autonomy_word"), (0xb0, "decision_timer"),
                                 (0xa8, "floor_attributes"), (0x790, "movement_speed_bits"),
                                 (0xc4, "eye_mode_word"),
                                 (0xc8, "eye_mouth_mode_word"),
                                 (0xcc, "mouth_texture_timer_word"),
                                 (0x794, "destination_x_bits"),
                                 (0x798, "destination_y_bits"),
                                 (0x79c, "destination_z_bits")]:
                actor_locations[f'{matches[0]["address"] + offset:08x}'] = f"actor_{actor_id}_{name}"
            # MemoryWatcher follows space-separated pointer offsets, keeping
            # observations attached to the actor when animation slots change.
            controller = matches[0]["address"] + 0x6fc
            actor_locations[f"{controller:08x}"] = f"actor_{actor_id}_animation_address"
            for offset, name in [(8, "time"), (16, "end"), (24, "speed"),
                                 (36, "blend_duration"), (40, "blend_tick")]:
                actor_locations[f"{controller:08x} {offset:x}"] = f"actor_{actor_id}_animation_{name}_bits"
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
            # Preserve signed Euler targets and the tween's double precision;
            # a rendered view alone hides a long rotation through equivalent angles.
            for offset, name in [(0x08, "position"), (0x20, "position_target"),
                                 (0xa0, "angles"), (0xb8, "angles_target")]:
                for axis, label in enumerate("xyz"):
                    for half, part in enumerate(["hi", "lo"]):
                        address = 0x802c8ee8 + offset + axis * 8 + half * 4
                        actor_locations[f'{address:08x}'] = f"camera_motion_{name}_{label}_{part}"
            for offset, name in [(0x48, "fov_hi"), (0x4c, "fov_lo"),
                                 (0xd8, "angle_frame"), (0xdc, "angle_end")]:
                actor_locations[f'{0x802c8ee8 + offset:08x}'] = f"camera_motion_{name}"
        # Movie.cpp loads MOVIE.sav and State.cpp checks its .dtm companion.
        shutil.copyfile(initial_state, output / "input.dtm.sav")
        shutil.copyfile(companion, output / "input.dtm.sav.dtm")
    try:
        actor_locations = explicit_locations(actor_locations, args.watch_locations)
    except ValueError as error:
        parser.error(str(error))
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
        "presentation": {"profile": "unmodified-game-v1", "disable_copy_filter": True},
        "backend": args.backend, "requested_frame": args.frame,
        "requested_vi_samples": args.watch_vis,
        "audio": {"backend": "No Audio Output", "muted": True, "dump": True},
        "timing": {"cpu_clock": args.cpu_clock, "fast_disc": args.fast_disc,
                   "diagnostic_override": bool(args.cpu_clock != 1.0 or args.fast_disc)},
        "complete": False,
    }
    # A Nix launcher is a wrapper; retain the actual executable's identity too.
    wrapped = Path(executable).resolve().with_name("." + Path(executable).name + "-wrapped")
    if wrapped.is_file():
        metadata["dolphin_wrapped_binary_sha256"] = sha256(wrapped)
    manifest = output / "capture.json"
    manifest.write_text(json.dumps(metadata, indent=2) + "\n")
    display = process = watch_directory = watcher = None
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
                   "-C", "Dolphin.Core.EnableCheats=False",
                   "-C", "Graphics.Enhancements.DisableCopyFilter=True"]
        if initial_state:
            command += ["-s", str(output / "input.dtm.sav")]
        if args.video:
            command += ["-C", "Dolphin.Movie.DumpFrames=True",
                        "-C", "Graphics.Settings.DumpFramesAsImages=False",
                        "-C", "Graphics.Settings.DumpFormat=matroska",
                        "-C", "Graphics.Settings.DumpCodec=ffv1",
                        "-C", "Graphics.Settings.DumpPixelFormat=bgr0",
                        # Dolphin's UseLossless toggle forces Ut Video instead.
                        "-C", "Graphics.Settings.UseLossless=False"]
        elif args.watch_vis is not None:
            command += ["-C", "Dolphin.Movie.DumpFrames=False"]
        if args.keep_duplicate_frames:
            command += ["-C", "Graphics.Hacks.SkipDuplicateXFBs=False"]
        metadata["presentation"]["keep_duplicate_frames"] = args.keep_duplicate_frames
        if args.cpu_clock != 1.0:
            command += ["-C", "Dolphin.Core.OverclockEnable=True",
                        "-C", f"Dolphin.Core.Overclock={args.cpu_clock}"]
        if args.fast_disc:
            command += ["-C", "Dolphin.Core.FastDiscSpeed=True"]
        if args.xvfb:
            command += ["-C", "Dolphin.General.HotkeysRequireFocus=False"]
        metadata["command"] = command
        with (output / "dolphin.log").open("w") as log:
            process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
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
            metadata["memory_watch"]["locations"] = watcher.locations
            if not metadata["memory_watch"]["complete"]:
                metadata["complete"] = False
        if watch_directory is not None:
            watch_directory.cleanup()
        metadata["audio"]["recordings"] = audio_evidence(output)
        if args.video:
            metadata["video"] = [{"path": str(path.relative_to(output)), "sha256": sha256(path)}
                                 for path in sorted((output / "user" / "Dump" / "Frames").glob("*.matroska"))]
            if not metadata["video"]:
                metadata["complete"] = False
                metadata["video_error"] = "no timestamped video recording was produced"
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
