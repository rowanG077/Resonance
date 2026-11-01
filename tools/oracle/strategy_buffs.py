#!/usr/bin/env python3
"""Trace autonomous buff admission in the original GQSEAF battle controller.

Uses copied field data, ordinary scenario commands and read-only observations.
The cases establish admission only, not spell release, outcomes or timing parity.
"""
import argparse
import json
from pathlib import Path
import shutil

from battle_fixture import BattleFixture, neutral_movie
from battle_state import watch_locations
from effect_fixture import prepare
from state import State, digest, inspect
from technique_content import native_learning

DISC_SHA256 = "e9328d7b825269d1d834c3c53fa71cef6b8e4b973b516113d5a53f32bff1bce3"
CASES = ((1, 0, 100, True), (1, 1, 100, False),
         (8, 8, 12, True), (5, 0, 100, False))


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def review(rows, expected):
    """A negative case must actually choose a normal, not simply remain idle."""
    natives = [r["actor_1_native"] & 65535 for r in rows
               if r["battle_actor_1_action"] >> 24 == 5 and r["actor_1_native"] & 65535]
    normal = any(r["battle_actor_1_action"] >> 24 == 5
                 and r["actor_1_selected"] != 0 for r in rows)
    return dict(first_native=natives[0] if natives else None,
                normal_admitted=normal, buff_expected=expected,
                passed=bool(natives and natives[0] == 245) if expected else not natives and normal)


def run(args):
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("strategy observer requires the pinned GQSEAF Disc 1")
    source = State(args.state)
    observed = inspect(source)
    catalogue_path = args.assets / "game/techniques.json"
    catalogue = json.loads(catalogue_path.read_text())
    bit = 1 << native_learning(catalogue)[3].index(107)
    entry = out / "entry.s01"
    # Lloyd and Raine, with Sharpness learned through the ordinary scenario API.
    fixture = prepare(source, entry, observed, [], [
        (0x24, [1]), (0x24, [4]), (0x2e, [4, 107]),
        (0xcd, [0, 0, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0])], seed=1)
    state = State(entry)
    settings = state.read(0x8035a768)
    for slot, character in enumerate((1, 4, 0, 0, 0, 0, 0, 0)):
        state.write(settings + 0xe9d + slot, "B", character)
    for slot in range(4):
        state.write(settings + 0xea5 + slot, "B", 0)  # Manual during entry.
    for character in (1, 4):
        member = settings + 0x2b8 + (character - 1) * 0x118
        state.write(member + 0x10, "B", 20)
        for offset in (0x12, 0x26, 0x36):
            state.write(member + offset, "H", 2000)
        for offset in (0x14, 0x28, 0x38):
            state.write(member + offset, "H", 100)
    fixture["party_changes"] = state.changes
    state.save(entry)
    movie = out / "neutral.dtm"
    neutral_movie(source.path, movie, observed["movie"]["input_count"], polls=24000)
    shutil.copyfile(movie, str(entry) + ".dtm")
    locations = watch_locations(dict(pool=1, bss_index=6,
                                    actors=[dict(slot=i) for i in range(8)]))
    pointer = "8035a86c 10 30 540"
    locations[pointer + " 159c0"] = "level_advantage_word"
    for slot in (0, 1, 4):
        base = 0x1cea0 + slot * 0x1ba0
        for offset, name in ((0x1174, "native"), (0x104, "selected"),
                             (0x198c, "support_target"), (0x107c, "control")):
            locations[f"{pointer} {base + offset:x}"] = f"actor_{slot}_{name}"
    report = dict(complete=False, cases=[], fixture=fixture,
                  source_sha256=source.sha256, source_movie_sha256=digest(str(source.path) + ".dtm"),
                  techniques_sha256=digest(catalogue_path),
                  scope="Original-game AI buff admission only; no release, outcome or timing parity claim.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, entry, out / "worker",
                               fixture["marker_address"], locations,
                               disc_sha256=DISC_SHA256, capture_images=False)
        worker.warmup(entry, out / "warmup", fixture["case_marker"], 240)
        ready = out / "ready.s01"
        worker.snapshot(ready)
        base = State(ready)
        battle = inspect(base, battle=True)["battle"]
        write(out / "ready.json", battle)
        actor = next(a for a in battle["actors"] if a["slot"] == 1)
        if (battle["mode"] != 9 or battle["formation"] != 0 or actor["kind"] != 4
                or actor["action"][0] != 3):
            raise ValueError("Raine is not idle in the requested encounter")
        for skill, difference, tp, expected in CASES:
            case = out / f"skill{skill}-adv{difference}-tp{tp}"
            case.mkdir()
            state = base.fork()
            # Component fixtures declare matching roster levels and the
            # derived entry-level difference consumed by buff selection.
            state.write(battle["pool"] + 0x159c3, "b", difference)
            for other in battle["actors"]:
                at, vitals = other["address"], other["vitals_address"]
                state.write(vitals + 0x1d, "B", 20 + difference if other["side"] == 0 else 20)
                state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38
                            | (0x10 if other["slot"] == 1 else 0))
                if other["side"] == 1:
                    for offset in (0x20, 0x24):
                        state.write(vitals + offset, "I", 30000)
            vitals = actor["vitals_address"]
            for offset, value in enumerate((1, skill, 2)):
                state.write(vitals + offset, "B", value)
            state.write(vitals + 0x28, "H", 100)
            state.write(vitals + 0x2a, "H", tp)
            member = state.read(0x8035a768) + 0x2b8 + 3 * 0x118
            if state.read(actor["address"]) != member:
                raise ValueError("Raine's character record moved unexpectedly")
            for offset in (0x70, 0x78):
                state.write(member + offset, "Q", bit)
            state.save(case / "input.s01")
            shutil.copyfile(str(ready) + ".dtm", case / "input.s01.dtm")
            write(case / "fixture.json", dict(changes=state.changes, game_code_modified=False,
                                               state_sha256=digest(case / "input.s01")))
            rows = worker.observe(case / "input.s01", case / "observations",
                                  fixture["case_marker"], 150)
            worker.snapshot(case / "terminal.s01")
            result = dict(skill=skill, difference=difference, tp=tp, **review(rows, expected))
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = all(case["passed"] for case in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("original strategy admission differs; inspect report.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True, help="Loaded field checkpoint with .dtm companion")
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
