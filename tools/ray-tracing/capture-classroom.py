#!/usr/bin/env python3
"""Render the classroom event offline to PNGs and a 60 fps MP4, up to 1080p."""
import argparse
import datetime
from decimal import Decimal, InvalidOperation
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FPS = 60
QUALITY = {
    "low": (640, 360, 1),
    "medium": (1280, 720, 32),
    "high": (1920, 1080, 256),
}


def nonnegative_seconds(value):
    try:
        seconds = Decimal(value)
    except InvalidOperation as error:
        raise argparse.ArgumentTypeError("time must be a number of seconds") from error
    if not seconds.is_finite() or not 0 <= seconds <= 600:
        raise argparse.ArgumentTypeError("time must be between 0 and 600 seconds")
    return seconds


def positive_seconds(value):
    seconds = nonnegative_seconds(value)
    if seconds == 0:
        raise argparse.ArgumentTypeError("duration must be greater than 0 and at most 600 seconds")
    return seconds


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="new output directory (relative paths use the repository root)")
    parser.add_argument("--max-duration", type=positive_seconds,
                        help="maximum VIDEO seconds from the capture start; omit to finish when Lloyd gains control")
    parser.add_argument("--quality", choices=QUALITY, default="high",
                        help="low: 360p/1 sample; medium: 720p/32 samples; high: 1080p/256 samples (default)")
    parser.add_argument("--samples", type=int,
                        help="override the quality preset's lighting samples per frozen frame (1–4096)")
    parser.add_argument("--renderer", choices=("lavapipe", "gpu"), default="lavapipe")
    parser.add_argument("--threads", type=int, default=8, help="Lavapipe worker threads (default: 8)")
    start = parser.add_mutually_exclusive_group()
    start.add_argument("--from", dest="from_seconds", type=nonnegative_seconds, default=Decimal(0), metavar="SECONDS",
                       help="start this many video seconds into the event; skip earlier frames without rendering")
    start.add_argument("--start-tick", type=int, help="diagnostic override: start at a later absolute script tick")
    parser.add_argument("--plan-only", action="store_true", help="check event timing/handoff without rendering")
    parser.add_argument("--no-video", action="store_true", help="keep PNGs only; skip FFmpeg")
    parser.add_argument("--no-build", action="store_true", help="use the existing development capture binary")
    args = parser.parse_args()
    width, height, preset_samples = QUALITY[args.quality]
    samples = args.samples if args.samples is not None else preset_samples
    if not 1 <= samples <= 4096:
        parser.error("samples must be between 1 and 4096")
    if not 1 <= args.threads <= 256:
        parser.error("threads must be between 1 and 256")
    if args.start_tick is not None and not 0 <= args.start_tick <= 20000:
        parser.error("start tick must be between 0 and 20000")
    output = args.output or Path("local") / ("classroom-event-" + datetime.datetime.now().strftime("%Y%m%d-%H%M%S"))
    output = (ROOT / output).resolve()
    if output.exists():
        parser.error(f"output already exists: {output}")
    ffmpeg = shutil.which("ffmpeg")
    if not args.plan_only and not args.no_video and ffmpeg is None:
        parser.error("ffmpeg is needed for MP4 output; install it or use --no-video")
    if not args.no_build:
        subprocess.run(["cargo", "build", "-p", "resonance-presentation", "--features", "solari",
                        "--example", "classroom_showcase"], cwd=ROOT, check=True)
    binary = ROOT / "target/debug/examples/classroom_showcase"
    if not binary.is_file():
        parser.error(f"capture binary missing: {binary}; omit --no-build")
    max_frames = math.ceil(args.max_duration * FPS) if args.max_duration is not None else None
    settings = {"samples": samples, "resolution": [width, height], "max_frames": max_frames,
                "from_frame": math.ceil(args.from_seconds * FPS),
                "start_tick": args.start_tick, "plan_only": args.plan_only}
    with tempfile.TemporaryDirectory(prefix="resonance-event-") as temporary:
        config = Path(temporary) / "settings.json"
        config.write_text(json.dumps(settings))
        command = [str(binary), str(output), str(config)]
        if not args.plan_only and args.renderer == "lavapipe":
            command.insert(0, str(ROOT / "tools/ray-tracing/lavapipe.sh"))
        env = os.environ.copy()
        env["LP_NUM_THREADS"] = str(args.threads)
        print(f"Output: {output}", flush=True)
        if settings["from_frame"]:
            print(f"Start: {settings['from_frame'] / FPS:.6f}s into the event (frame {settings['from_frame']})", flush=True)
        if not args.plan_only:
            print(f"Quality: {args.quality}; {width}x{height}, 60 fps, {samples} lighting samples/frame. "
                  "The duration limit measures video time, not render time.", flush=True)
        subprocess.run(command, cwd=ROOT, env=env, check=True)
    recording = json.loads((output / "recording.json").read_text())
    if not args.plan_only:
        frames = sorted((output / "frames").glob("frame-*.png"))
        expected = [f"frame-{i:06}.png" for i in range(recording["frames"])]
        if [frame.name for frame in frames] != expected:
            raise RuntimeError("rendered sequence contains missing or unexpected frames")
        if not args.no_video:
            subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "warning", "-n",
                            "-framerate", str(FPS), "-start_number", "0",
                            "-i", str(output / "frames/frame-%06d.png"),
                            "-frames:v", str(recording["frames"]), "-an", "-c:v", "libx264",
                            "-preset", "slow", "-crf", "16", "-pix_fmt", "yuv420p",
                            "-movflags", "+faststart", str(output / "classroom.mp4")], check=True)
            print(f"Video: {output / 'classroom.mp4'}")
        print(f"Frames: {output / 'frames'}")
    print(f"{recording['frames']} frames, {recording['duration_seconds']:.3f}s; stopped: {recording['stop_reason']}")


if __name__ == "__main__":
    try:
        main()
    except (subprocess.CalledProcessError, RuntimeError) as error:
        raise SystemExit(str(error)) from error
