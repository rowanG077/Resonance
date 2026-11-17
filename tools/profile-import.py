#!/usr/bin/env python3
"""Measure full cooks at explicit CPU budgets; retain per-stage timings.

Uses Linux affinity when available, otherwise the importer's worker limit. Never
labels an oversubscribed run as a measurement on more CPUs than the host has.
The output directory must be dedicated to this benchmark: each cook replaces it.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--importer", type=Path, default=Path("target/release-fast/resonance-import"))
    parser.add_argument("--extracted", type=Path, default=Path("local/extracted-discs"))
    parser.add_argument("--work", type=Path, default=Path("target/import-profiles"))
    parser.add_argument("--cores", type=int, nargs="+", default=[1, 2, 4, 8, 16])
    parser.add_argument("--profile", type=Path, default=Path("apps/resonance/assets/setup/import-profile.json"))
    parser.add_argument("--fresh", action="store_true", help="replace existing timing samples instead of resuming")
    args = parser.parse_args()
    if any(n < 1 for n in args.cores):
        parser.error("core counts must be positive")
    if any(path.is_symlink() for path in [args.work, args.work / "assets", args.profile]):
        parser.error("profiling paths must not be symbolic links")
    cpus = sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else list(range(os.cpu_count() or 1))
    # On heterogeneous Linux CPUs, keep the faster cores in the smaller budgets.
    def capacity(cpu):
        path = Path(f"/sys/devices/system/cpu/cpu{cpu}/cpu_capacity")
        return int(path.read_text()) if path.exists() else 1024
    cpus.sort(key=lambda cpu: (capacity(cpu), cpu), reverse=True)
    budgets = sorted({min(n, len(cpus)) for n in args.cores if n > 0})
    if any(n > len(cpus) for n in args.cores):
        print(f"Host exposes {len(cpus)} CPUs; larger counts are extrapolations, not measured runs.", flush=True)
    args.work.mkdir(parents=True, exist_ok=True)
    marker = args.work / ".profile-workspace"
    output = args.work / "assets"
    if output.exists() and not marker.exists():
        parser.error("refusing to replace an output not created by this profiler")
    marker.touch()
    profile = {"schema": 1, "host_cpus": len(cpus), "cpu_affinity": hasattr(os, "sched_getaffinity"), "runs": [], "stages": []}
    # Resume completed runs, but never infer completion from partial log files.
    if args.profile.exists() and not args.fresh:
        profile = json.loads(args.profile.read_text())
    for budget in budgets:
        if any(run.get("requested_cores", run["cores"]) == budget for run in profile["runs"]):
            print(f"Already measured {budget} CPUs; keeping the recorded run.", flush=True)
            continue
        log_path = args.work / f"cores-{budget}.log"
        env = dict(os.environ, RESONANCE_IMPORT_WORKERS=str(budget))
        command = [str(args.importer.resolve()), "--mods-directory", str(args.work / "mods"),
                   "cook-all", "--extracted", str(args.extracted), "--output", str(output)]
        # taskset starts the importer inside its budget, before it creates threads.
        if hasattr(os, "sched_getaffinity"):
            command = ["taskset", "--cpu-list", ",".join(map(str, cpus[:budget])), *command]
        print(f"Measuring {budget} CPUs; output: {log_path}", flush=True)
        started = time.monotonic()
        with log_path.open("w") as log:
            subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        elapsed = time.monotonic() - started
        log = log_path.read_text()
        # available_parallelism may be smaller than the affinity mask under a
        # container quota. Record what the importer actually used.
        effective = max(map(int, re.findall(r"Using (\d+) worker threads", log)), default=1)
        matches = re.findall(r"\[asset type (\d+)/(\d+)\] (.+) took ([\d.]+) seconds", log)
        assert matches and len(matches) == int(matches[-1][1]), "incomplete pass timings"
        if not profile["stages"]:
            profile["stages"] = [{"name": name, "seconds": {}} for _, _, name, _ in matches]
        assert [s["name"] for s in profile["stages"]] == [m[2] for m in matches], "importer stage order changed"
        for stage, (_, _, _, seconds) in zip(profile["stages"], matches):
            stage["seconds"][str(effective)] = float(seconds)
        profile["runs"].append({"cores": effective, "requested_cores": budget, "elapsed_seconds": round(elapsed, 3), "affinity": cpus[:budget]})
        args.profile.parent.mkdir(parents=True, exist_ok=True)
        temporary = args.profile.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(profile, indent=2) + "\n")
        temporary.replace(args.profile)
        print(f"Recorded {effective} effective workers (requested {budget} CPUs): {elapsed:.1f}s", flush=True)


if __name__ == "__main__":
    main()
