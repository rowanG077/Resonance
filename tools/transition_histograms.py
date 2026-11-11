"""Plot residency_probe.py benchmark JSONL files (requires matplotlib and numpy)."""
import argparse
import csv
import json
from pathlib import Path
import re

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np


def stats(values):
    return {
        "n": len(values), "median_ms": float(np.median(values)),
        "p95_ms": float(np.percentile(values, 95)), "max_ms": float(max(values)),
        "min_ms": float(min(values)), "mean_ms": float(np.mean(values)),
    }


def histogram(ax, rows, title, color="#3476ad"):
    values = [row["total_ms"] for row in rows]
    summary = stats(values)
    ax.hist(values, bins="auto", color=color, edgecolor="white", alpha=0.85)
    ax.axvline(summary["median_ms"], color="#263442", lw=1.8, label="Median")
    ax.axvline(summary["p95_ms"], color="#c86e23", ls="--", lw=1.5, label="p95")
    ax.set_title(title, loc="left", fontweight="bold")
    ax.set_xlabel("Admission to GPU-ready opening frame (ms)")
    ax.set_ylabel("Samples")
    ax.text(0.97, 0.95,
            f'n={len(values)}\nmedian {summary["median_ms"]:.1f} ms\n'
            f'p95 {summary["p95_ms"]:.1f} ms\nmax {summary["max_ms"]:.1f} ms',
            transform=ax.transAxes, ha="right", va="top", fontsize=10,
            bbox=dict(facecolor="white", alpha=0.9, edgecolor="none"))
    ax.spines[["top", "right"]].set_visible(False)
    ax.grid(axis="y", alpha=0.15)
    ax.set_axisbelow(True)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    metadata, rows = [], []
    for run, path in enumerate(args.inputs, 1):
        records = [json.loads(line) for line in path.read_text().splitlines()]
        meta = records.pop(0)
        assert meta["kind"] == "metadata"
        assert len(records) == 1 + 12 * meta["rounds"], "incomplete benchmark"
        metadata.append(meta)
        for row in records:
            assert row["transition"]["outcome"] == "ready"
            rows.append(dict(run=run, round=row["round"], kind=row["kind"],
                             first_pass=row["round"] == 0, **row["transition"]))
    for key in ("resolution", "adapter", "anti_aliasing", "simulation", "preparation_trace"):
        assert all(meta[key] == metadata[0][key] for meta in metadata), f"mixed {key}"
    repeat = [row for row in rows if not row["first_pass"]]
    if not repeat:
        parser.error("histograms require at least two rounds per launch")
    groups = {
        "field_transition": "Field → field · six destinations",
        "field_battle": "Field → battle · formation 1, arena 13",
        "world_battle": "Overworld → battle · formation 1, arena 13",
        "field_to_world": "Field → overworld",
        "world_to_field": "Overworld → field",
        "battle_return": "Battle return",
    }
    summary = {"metadata": metadata, "first_pass": {}, "repeat_visits": {}}
    for phase, samples in (("first_pass", [r for r in rows if r["first_pass"]]),
                           ("repeat_visits", repeat)):
        for kind in ["initial_field", *groups]:
            values = [r["total_ms"] for r in samples if r["kind"] == kind]
            if values:
                summary[phase][kind] = stats(values)
    summary["boot"] = stats([meta["boot_ms"] for meta in metadata])
    plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 10})
    fig, axes = plt.subplots(2, 2, figsize=(14, 9.5))
    for ax, kind, color in zip(axes.flat,
                               ["field_transition", "field_battle", "world_battle", "field_to_world"],
                               ["#3476ad", "#4b9473", "#8d6da8", "#a78346"]):
        histogram(ax, [r for r in repeat if r["kind"] == kind], groups[kind], color)
    resolution = metadata[0]["resolution"]
    adapter = re.search(r'name: "([^"]+)"', metadata[0]["adapter"])
    title = f"{adapter.group(1)} · transition benchmark" if adapter else "Transition benchmark"
    fig.suptitle(title, fontsize=20, fontweight="bold", y=0.98)
    fig.text(0.5, 0.925,
             f'{resolution["width"]}×{resolution["height"]} · AA {metadata[0]["anti_aliasing"]} · '
             f'release-fast · {len(metadata)} independent launches · first route pass excluded',
             ha="center", color="#48505a")
    shatter = ("Battle shatter minimum duration disabled. "
               if metadata[0].get("shatter_minimum_duration_disabled")
               else "Battle entry includes the real-time 90-tick shatter. ")
    fig.text(0.5, 0.017,
             shatter + "No screenshot readbacks. "
             "Repeat visits may include cache eviction; filesystem cache was not flushed.",
             ha="center", fontsize=9, color="#48505a")
    fig.tight_layout(rect=(0, 0.04, 1, 0.90), h_pad=3, w_pad=3)
    fig.savefig(args.output / "histograms.png", dpi=160)
    fig.savefig(args.output / "histograms.svg")
    plt.close(fig)
    fig, axes = plt.subplots(2, 3, figsize=(16, 8.5))
    summary["fields"] = {}
    for ax, map_id in zip(axes.flat, [340, 341, 376, 520, 374, 332]):
        field_rows = [r for r in repeat if r["kind"] == "field_transition"
                      and r["destination"] == f"field:{map_id}"]
        summary["fields"][str(map_id)] = histogram(ax, field_rows, f"Field {map_id}")
    fig.suptitle("Field → field · repeat visits by destination", fontsize=19, fontweight="bold")
    fig.tight_layout(rect=(0, 0, 1, 0.94), h_pad=3, w_pad=3)
    fig.savefig(args.output / "field-histograms.png", dpi=160)
    fig.savefig(args.output / "field-histograms.svg")
    plt.close(fig)
    fig, axes = plt.subplots(2, 2, figsize=(14, 9.5))
    first_pass = [row for row in rows if row["first_pass"]]
    for ax, kind in zip(axes.flat,
                        ["field_transition", "field_battle", "world_battle", "initial_field"]):
        histogram(ax, [r for r in first_pass if r["kind"] == kind],
                  groups.get(kind, "Title → initial field 332"), "#b17e4d")
    fig.suptitle("First route pass after startup", fontsize=20, fontweight="bold")
    fig.text(0.5, 0.925,
             f'{len(metadata)} independent launches · small sample counts · '
             'filesystem and driver shader caches were not cleared', ha="center", color="#48505a")
    fig.text(0.5, 0.017, shatter + "Field timings exclude entry-fade playback.",
             ha="center", fontsize=9, color="#48505a")
    fig.tight_layout(rect=(0, 0.04, 1, 0.90), h_pad=3, w_pad=3)
    fig.savefig(args.output / "first-pass-histograms.png", dpi=160)
    plt.close(fig)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    with (args.output / "samples.csv").open("w", newline="") as output:
        writer = csv.DictWriter(output, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
