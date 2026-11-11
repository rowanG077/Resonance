"""Capture or profile installed-content residency using the internal GPU driver."""
import argparse
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assets", type=Path, help="complete named content library")
    parser.add_argument("--captures", type=Path, help="write 1080p scene and battle captures")
    parser.add_argument("--trace", action="store_true", help="report preparation scopes and schedules")
    parser.add_argument("--msaa", choices=("2", "4", "8"), help="override MSAA for this probe")
    parser.add_argument("--audit-pipelines", action="store_true", help="compare prewarming with live pipelines")
    parser.add_argument("--resolution", help="render size, e.g. 1920x1080 (default 640x480)")
    parser.add_argument("--benchmark-rounds", type=int, help="repeat field and battle routes for timing")
    parser.add_argument("--jsonl", type=Path, help="write benchmark metadata and transition samples")
    args = parser.parse_args()
    if args.benchmark_rounds is not None:
        if args.benchmark_rounds < 1 or args.jsonl is None:
            parser.error("--benchmark-rounds requires a positive count and --jsonl")
        if args.captures is not None:
            parser.error("benchmark runs cannot include screenshot readbacks")
    elif args.jsonl is not None:
        parser.error("--jsonl requires --benchmark-rounds")
    env = os.environ.copy()
    for name in (
        "RESONANCE_RESIDENCY_CAPTURE", "RESONANCE_PREPARATION_TRACE",
        "RESONANCE_RESIDENCY_MSAA", "RESONANCE_PIPELINE_AUDIT",
        "RESONANCE_RESIDENCY_RESOLUTION", "RESONANCE_BENCHMARK_ROUNDS",
        "RESONANCE_BENCHMARK_JSONL",
    ):
        env.pop(name, None)
    env["RESONANCE_TEST_ASSETS"] = str(args.assets.resolve())
    if args.captures is not None:
        env["RESONANCE_RESIDENCY_CAPTURE"] = str(args.captures.resolve())
    if args.trace:
        env["RESONANCE_PREPARATION_TRACE"] = "1"
    if args.msaa is not None:
        env["RESONANCE_RESIDENCY_MSAA"] = args.msaa
    if args.audit_pipelines:
        env["RESONANCE_PIPELINE_AUDIT"] = "1"
    if args.resolution is not None:
        env["RESONANCE_RESIDENCY_RESOLUTION"] = args.resolution
    if args.benchmark_rounds is not None:
        env["RESONANCE_BENCHMARK_ROUNDS"] = str(args.benchmark_rounds)
        env["RESONANCE_BENCHMARK_JSONL"] = str(args.jsonl.resolve())
    # The ignored driver uses private scene/admission hooks without expanding
    # the production API or making diagnostic modes part of regression tests.
    return subprocess.run(
        ["cargo", "test", "--locked", "-p", "resonance-presentation", "--lib",
         "--profile", "release-fast", "--features", "preparation-trace",
         "field_ui::resident::probe::residency_probe", "--", "--exact",
         "--ignored", "--nocapture", "--test-threads=1"],
        cwd=Path(__file__).resolve().parents[1], env=env,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
