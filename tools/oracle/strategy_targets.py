#!/usr/bin/env python3
"""Observe the original game's eight action-target strategies from idle AI.

The component fixtures edit vitals, placement, target references and the observed
spell-history latch. They do not replace instructions, actions, poses or timers.
"""
import argparse
from hashlib import sha256
import json
import math
from pathlib import Path
import shutil

from battle_fixture import BattleFixture, neutral_movie, prepare_entry
from battle_state import watch_locations
from state import State, digest, inspect
from strategy_buffs import DISC_SHA256

CASES = (
    dict(name="nearest", policy=1, expected=[6]),
    dict(name="farthest", policy=2, expected=[5]),
    dict(name="leader", policy=3, expected=[5]),
    dict(name="spread", policy=4, leader_target=6, expected=[4, 5]),
    dict(name="spread-exhausted", policy=4, leader_target=6, hide_others=True, expected=[6]),
    dict(name="flying", policy=5, expected=[4]),
    dict(name="magic-before-casting", policy=6, leader_target=6, expected=[4, 5]),
    dict(name="magic-after-casting", policy=6, leader_target=6, spell_history=True, expected=[6]),
    dict(name="lowest-hp", policy=7, expected=[5]),
    dict(name="protect-wounded", policy=8, wounded=True, expected=[4]),
    dict(name="protect-tie", policy=8, expected=[6]),
    dict(name="protect-opponent-tie", policy=8, wounded=True, opponent_tie=True, expected=[6]),
    dict(name="protect-fallen", policy=8, fallen=True, expected=[4]),
    dict(name="leader-fallen", policy=3, fallen=True, expected=[5]),
)


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def review(rows, addresses, expected, policy):
    for index, row in enumerate(rows):
        if row["battle_actor_1_action"] >> 24 in (4, 5):
            target = addresses.get(row["battle_actor_1_target"])
            choices = [(row["actor_1_strategy"] >> shift) & 255 for shift in (24, 16, 8)]
            return dict(first_decision=index, target=target, observed_choices=choices,
                        passed=target in expected and choices == [policy, 5, 2])
    return dict(first_decision=None, target=None, passed=False)


def arrange(base, battle, case):
    state = base.fork()
    actors = {a["slot"]: a for a in battle["actors"]}
    positions = {0: [550., 0., 0.], 1: [0., 0., 0.], 4: [300., 0., 0.],
                 5: [-600., 0., 0.], 6: [50., 0., 50.]}
    if case.get("opponent_tie"):
        positions[6] = [800., 0., 0.]
    for slot, actor in actors.items():
        at, vitals = actor["address"], actor["vitals_address"]
        state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38
                    | (0x10 if slot == 1 else 0))
        if case.get("hide_others") and slot in (4, 5):
            state.write(at + 0x281, "B", state.read(at + 0x281, "B") | 8)
        for axis, value in enumerate(positions[slot]):
            state.write(at + 0x18c0 + axis * 4, "f", value)
        maximum = 2000 if actor["side"] == 0 else 30000
        hp = maximum if actor["side"] == 0 else {4: 5000, 5: 300, 6: 2000}[slot]
        if slot == 0:
            hp = 0 if case.get("fallen") else 500 if case.get("wounded") else maximum
        state.write(vitals + 0x20, "I", maximum)
        state.write(vitals + 0x24, "I", hp)
        state.write(at + 0x107d, "B", state.read(at + 0x107d, "B") & ~7
                    | (2 if hp == 0 else 3))
        # These are the game's maintained pairwise distances consumed by the
        # selector. Make them agree with the declared positions at restore.
        for other_slot, other in actors.items():
            if other["side"] == actor["side"]:
                continue
            distance = math.hypot(positions[slot][0] - positions[other_slot][0],
                                  positions[slot][2] - positions[other_slot][2])
            rank = state.read(other["address"] + 0x107c, "B") & 7
            state.write(at + 0x1084 + rank * 4, "f", distance)
    leader, owner = actors[0]["address"], actors[1]["address"]
    state.write(leader + 0x1990, "I", actors[case.get("leader_target", 5)]["address"])
    state.write(owner + 0x1990, "I", actors[4]["address"])
    state.write(owner + 0x1c9, "B", state.read(owner + 0x1c9, "B") & ~0x40
                | (0x40 if case.get("spell_history") else 0))
    vitals = actors[1]["vitals_address"]
    for offset, value in enumerate((case["policy"], 5, 2)):
        state.write(vitals + offset, "B", value)
    return state


def target_inputs(state, battle):
    slots = {a["address"]: a["slot"] for a in battle["actors"]}
    actors = []
    for actor in battle["actors"]:
        at, vitals = actor["address"], actor["vitals_address"]
        maximum, hp = state.read(vitals + 0x20), state.read(vitals + 0x24)
        availability = state.read(at + 0x107d, "B") & 7
        actors.append(dict(slot=actor["slot"], side=actor["side"],
            position=[state.read(at + 0x18c0 + axis * 4, "f") for axis in range(3)],
            hp=hp, hp_percent=hp * 100 // maximum if maximum else 0,
            flying=bool(state.read(state.read(at + 4) + 0x5c) & 1),
            has_cast_spell=bool(state.read(at + 0x1c9, "B") & 0x40),
            available=availability > 2, hidden=bool(state.read(at + 0x281, "B") & 8),
            dying=availability == 2, target=slots.get(state.read(at + 0x1990))))
    return dict(owner=1, actors=actors)


def run(args):
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("target observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = State(args.state)
    observed = inspect(source)
    entry = out / "entry.s01"
    fixture = prepare_entry(source, entry, observed, 238, 0, party=(1, 4))
    movie = out / "neutral.dtm"
    neutral_movie(source.path, movie, observed["movie"]["input_count"], polls=24000)
    shutil.copyfile(movie, str(entry) + ".dtm")
    locations = watch_locations(dict(pool=1, bss_index=6,
                                    actors=[dict(slot=i) for i in range(8)]))
    pointer = "8035a86c 10 30 540"
    for slot in (0, 1, 4, 5, 6):
        base = 0x1cea0 + slot * 0x1ba0
        locations[f"{pointer} {base + 0x1c8:x}"] = f"actor_{slot}_casting_flags"
        locations[f"{pointer} {base + 0x1988:x}"] = f"actor_{slot}_leader"
        locations[f"{pointer} {base + 4:x} 5c"] = f"actor_{slot}_traits"
    locations[f"{pointer} {0x1cea0 + 0x1ba0 + 8:x} 0"] = "actor_1_strategy"
    report = dict(complete=False, cases=[], fixture=fixture, source_sha256=source.sha256,
                  scope="First autonomous target decisions, with declared component data; not action outcome equivalence.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, entry, out / "worker", fixture["marker_address"],
                               locations, disc_sha256=DISC_SHA256, capture_images=False)
        worker.warmup(entry, out / "warmup", fixture["case_marker"], 240)
        ready = out / "ready.s01"
        worker.snapshot(ready)
        base = State(ready)
        battle = inspect(base, battle=True)["battle"]
        write(out / "ready.json", battle)
        actors = {a["slot"]: a for a in battle["actors"]}
        if (battle["mode"] != 9 or battle["formation"] != 238
                or set(actors) != {0, 1, 4, 5, 6} or actors[1]["action"][0] != 3):
            raise ValueError("requested idle party and three-opponent encounter was not initialized")
        traits = {slot: base.read(base.read(actors[slot]["address"] + 4) + 0x5c)
                  for slot in (4, 5, 6)}
        if not traits[4] & 1 or not traits[5] & 1 or traits[6] & 1:
            raise ValueError("authored encounter does not contain the requested flying/ground targets")
        report["enemy_traits"] = traits
        addresses = {actor["address"]: slot for slot, actor in actors.items()}
        for case in CASES:
            target = out / case["name"]
            target.mkdir()
            state = arrange(base, battle, case)
            case_marker = int.from_bytes(sha256(json.dumps(case, sort_keys=True).encode()).digest()[:4])
            state.write(fixture["marker_address"], "I", case_marker)
            state.save(target / "input.s01")
            shutil.copyfile(str(ready) + ".dtm", target / "input.s01.dtm")
            write(target / "fixture.json", dict(case=case, changes=state.changes,
                                                target_inputs=target_inputs(state, battle),
                                                case_marker=case_marker,
                                                game_code_modified=False,
                                                state_sha256=digest(target / "input.s01")))
            rows = worker.observe(target / "input.s01", target / "observations",
                                  case_marker, 24)
            result = dict(case, **review(rows, addresses, case["expected"], case["policy"]))
            if result["first_decision"] is not None:
                random = state.read(battle["pool"] + 0x7a670)
                final = rows[result["first_decision"]]["battle_random_state_word"]
                for draws in range(1000):
                    if random == final:
                        result["random_draws_to_decision"] = draws
                        break
                    random = (random * 0x41c64e6d + 1234567) & 0xffffffff
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = all(case["passed"] for case in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("original target observations differ; inspect report.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
