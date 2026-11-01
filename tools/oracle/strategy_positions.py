#!/usr/bin/env python3
"""Observe companion position policies from a recorded idle battle checkpoint.

Fixtures edit declared controls, repertoire, placement, vitals and command data.
Instructions, action states, poses and timers are preserved.
"""
import argparse
from hashlib import sha256
import json
import math
from pathlib import Path
import shutil
import struct

from battle_fixture import BattleFixture, neutral_movie
from battle_state import watch_locations
from state import State, digest, inspect
from strategy_buffs import DISC_SHA256
from technique_content import native_learning

CASES = tuple(dict(name=f"position-{position}-no-techniques", position=position,
                   skill=5, expected="idle" if position == 3 else "normal")
              for position in range(1, 7)) + (
    dict(name="hold-healer", position=3, skill=7, heal=True, wounded=True, expected="idle"),
    dict(name="hold-command", position=3, skill=5, heal=True, wounded=True,
         command=True, expected="heal"),
    dict(name="hold-away", position=3, skill=5, away=100, expected="idle"),
    dict(name="hold-nine", position=3, skill=5, away=9, expected="idle"),
    dict(name="hold-eleven", position=3, skill=5, away=11, expected="idle"),
    dict(name="hold-clamped", position=3, skill=5, outside=True, expected="move"),
    dict(name="hold-clamped-nine", position=3, skill=5, outside=True, away=798.5, expected="idle"),
    dict(name="hold-clamped-eleven", position=3, skill=5, outside=True, away=796.5, expected="move"),
    dict(name="hold-switch", position=3, skill=5, switch=True, expected="brake"),
    dict(name="magic-healer-idle", position=5, skill=7, heal=True, expected="idle"),
    dict(name="magic-healer-low-tp", position=5, skill=7, heal=True, tp=25, expected="normal"),
    dict(name="magic-healer-above-tp", position=5, skill=7, heal=True, tp=26, expected="idle"),
    dict(name="magic-healer-wounded", position=5, skill=7, heal=True, wounded=True, expected="heal"),
)


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def float_word(word):
    return struct.unpack(">f", struct.pack(">I", word))[0]


def review(rows, case):
    choices = [1, case["skill"], case["position"]]
    valid = bool(rows) and all([(r["owner_strategy"] >> shift) & 255 for shift in (24, 16, 8)]
                               == choices and (r["battle_actor_1_flags"] >> 27) & 7 == 2
                               for r in rows)
    # Native resource zero is a normal only when an actual action table is selected.
    normals = [i for i, r in enumerate(rows) if r["battle_actor_1_action"] >> 24 == 5
               and r["owner_selected"] and not r["owner_native"] & 65535]
    techniques = [(i, r["owner_native"] & 65535) for i, r in enumerate(rows)
                  if r["battle_actor_1_action"] >> 24 == 5 and r["owner_native"] & 65535]
    xs = [float_word(r["battle_actor_1_x_bits"]) for r in rows]
    start = xs[0] if case.get("switch") and xs else float(case.get("away", 0))
    movement = max((abs(x - start) for x in xs), default=0)
    idle = not normals and not techniques and movement < 0.01
    moved = not normals and not techniques and movement > 0.1
    braked = bool(rows) and not normals and not techniques and movement < 100 and rows[-1]["battle_actor_1_action"] >> 24 in (3, 10)
    expected = case["expected"]
    passed = valid and {"idle": idle, "move": moved, "brake": braked, "normal": bool(normals) and not techniques,
                        "heal": bool(techniques) and techniques[0][1] == 236}[expected]
    return dict(passed=passed, live_policy_verified=valid, first_normal=normals[0] if normals else None,
                first_technique=techniques[0] if techniques else None,
                start_x=start, end_x=xs[-1] if xs else None, max_displacement=movement)


def arrange(base, battle, case, bit):
    state = base.fork()
    actors = {a["slot"]: a for a in battle["actors"]}
    positions = {0: [-600., 0., -300.], 1: [float(case.get("away", 0)), 0., 0.],
                 4: [600., 0., 0.]}
    if case.get("outside"):
        positions[0] = [300., 0., -300.]
    homes = {0: [-600., 0., -300.], 1: [0., 0., 0.], 4: [600., 0., 0.]}
    homes[0] = positions[0]
    offsets = {0: [0., 0., 0.], 1: [600., 0., 300.], 4: [0., 0., 0.]}
    for slot, actor in actors.items():
        at, vitals = actor["address"], actor["vitals_address"]
        state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38
                    | (0x10 if slot == 1 else 0))
        for offset, vector in ((0x18c0, positions[slot]), (0x192c, homes[slot]),
                               (0x1938, offsets[slot])):
            for axis, value in enumerate(vector):
                state.write(at + offset + axis * 4, "f", value)
        maximum = 2000 if actor["side"] == 0 else 30000
        state.write(vitals + 0x20, "I", maximum)
        state.write(vitals + 0x24, "I", 400 if slot == 0 and case.get("wounded") else maximum)
        for other_slot, other in actors.items():
            if other["side"] == actor["side"]:
                continue
            distance = math.hypot(positions[slot][0] - positions[other_slot][0],
                                  positions[slot][2] - positions[other_slot][2])
            rank = state.read(other["address"] + 0x107c, "B") & 7
            state.write(at + 0x1084 + rank * 4, "f", distance)
    at, vitals = actors[1]["address"], actors[1]["vitals_address"]
    for offset, value in enumerate((1, case["skill"], case["position"])):
        state.write(vitals + offset, "B", value)
    state.write(vitals + 0x28, "H", 100)
    state.write(vitals + 0x2a, "H", case.get("tp", 100))
    member = state.read(at)
    for offset in (0x70, 0x78):
        state.write(member + offset, "Q", bit if case.get("heal") else 0)
    if case.get("command"):
        state.write(at + 0x189e, "H", 98)
        state.write(at + 0x18a8, "I", actors[0]["address"])
    return state



def position_inputs(state, battle):
    return [dict(slot=actor["slot"],
                 position=[state.read(actor["address"] + 0x18c0 + axis * 4, "f") for axis in range(3)],
                 home=[state.read(actor["address"] + 0x192c + axis * 4, "f") for axis in range(3)],
                 offset=[state.read(actor["address"] + 0x1938 + axis * 4, "f") for axis in range(3)],
                 hp=state.read(actor["vitals_address"] + 0x24),
                 max_hp=state.read(actor["vitals_address"] + 0x20))
            for actor in battle["actors"]]

def run(args):
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("position observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    base = State(args.state)
    battle = inspect(base, battle=True)["battle"]
    actors = {a["slot"]: a for a in battle["actors"]}
    if (battle["mode"] != 9 or set(actors) != {0, 1, 4}
            or actors[1]["kind"] != 4 or actors[1]["action"][0] != 3):
        raise ValueError("expected idle Lloyd/Raine checkpoint with one opponent")
    catalogue = args.assets / "game/techniques.json"
    bit = 1 << native_learning(json.loads(catalogue.read_text()))[3].index(98)
    marker = base.read(0x8035a578) + 0x581c + 31 * 0x360 + 0x300
    movie = out / "neutral.dtm"
    cursor = struct.unpack_from("<Q", base.raw, 33)[0]
    neutral_movie(base.path, movie, cursor, polls=24000)
    initial = out / "ready.s01"
    shutil.copyfile(base.path, initial)
    shutil.copyfile(movie, str(initial) + ".dtm")
    base = State(initial)
    pointer = "8035a86c 10 30 540"
    at = 0x1cea0 + 0x1ba0
    locations = watch_locations(dict(pool=1, bss_index=6, actors=[dict(slot=i) for i in range(8)]))
    for offset, name in ((0x1174, "native"), (0x104, "selected"), (0x198c, "support_target"),
                         (0x189c, "command_word"), (0x18a8, "command_target"),
                         (0x192c, "home_x"), (0x1934, "home_z")):
        locations[f"{pointer} {at + offset:x}"] = "owner_" + name
    locations[f"{pointer} {at + 8:x} 0"] = "owner_strategy"
    cases = [case for case in CASES if not args.cases or case["name"] in args.cases]
    if not cases or args.cases and set(args.cases) != {case["name"] for case in cases}:
        raise ValueError("unknown or empty position case selection")
    report = dict(complete=False, cases=[], source_sha256=base.sha256,
                  techniques_sha256=digest(catalogue),
                  scope="Position policy admission and idle movement; no timing or outcome parity claim.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, initial, out / "worker", marker,
                               locations, disc_sha256=DISC_SHA256, capture_images=False)
        for case in cases:
            target = out / case["name"]
            target.mkdir()
            state = arrange(base, battle, {**case, "position": 2} if case.get("switch") else case, bit)
            case_marker = int.from_bytes(sha256(json.dumps(case, sort_keys=True).encode()).digest()[:4])
            state.write(marker, "I", case_marker)
            state.save(target / "input.s01")
            shutil.copyfile(movie, target / "input.s01.dtm")
            write(target / "fixture.json", dict(case=case, changes=state.changes,
                                                case_marker=case_marker, game_code_modified=False,
                                                position_inputs=position_inputs(state, battle),
                                                state_sha256=digest(target / "input.s01")))
            if case.get("switch"):
                worker.observe(target / "input.s01", target / "approach", case_marker, 24)
                worker.snapshot(target / "approaching.s01")
                state = State(target / "approaching.s01")
                if state.read(actors[1]["address"] + 0x1ac, "B") != 4:
                    raise ValueError("position switch fixture did not reach an approach")
                state.write(actors[1]["vitals_address"] + 2, "B", 3)
                case_marker ^= 0xaaaaaaaa
                state.write(marker, "I", case_marker)
                state.save(target / "switch.s01")
                shutil.copyfile(str(target / "approaching.s01") + ".dtm", target / "switch.s01.dtm")
                write(target / "switch.json", dict(changes=state.changes, case_marker=case_marker,
                                                   position_inputs=position_inputs(state, battle),
                                                   game_code_modified=False))
                input_state = target / "switch.s01"
            else:
                input_state = target / "input.s01"
            rows = worker.observe(input_state, target / "observations", case_marker, args.vis)
            worker.snapshot(target / "terminal.s01")
            result = dict(case, **review(rows, case))
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = all(case["passed"] for case in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("position observations differ; inspect report.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True, help="Idle battle checkpoint with .dtm companion")
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cases", nargs="+", help="Optional subset of named cases")
    parser.add_argument("--vis", type=int, default=120)
    run(parser.parse_args())
