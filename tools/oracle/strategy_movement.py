#!/usr/bin/env python3
"""Observe blocked formation choices, post-action return and pursuit timeout.

Uses a naturally settled Lloyd/Raine battle checkpoint. Component fixtures edit
controls, placement, homes and profiles; action states, poses and timers remain
owned by the original game.
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
from strategy_positions import arrange, float_word, position_inputs, write

CASES = (
    dict(name="idle-clamped-free", mode="idle", position=3, skill=5, outside=True, expected="move"),
    dict(name="idle-clamped-occupied", mode="idle", position=3, skill=5, outside=True, enemy_x=807.5, expected="stay"),
    dict(name="idle-occupancy-equality", mode="idle", position=3, skill=5, outside=True, enemy_x=607.5, expected="move"),
    dict(name="idle-occupancy-below", mode="idle", position=3, skill=5, outside=True, enemy_x=608.5, expected="stay"),
    dict(name="idle-occupancy-wide", mode="idle", position=3, skill=5, outside=True, enemy_x=657.5, expected="stay"),
    dict(name="idle-occupancy-edited", mode="idle", position=3, skill=5, outside=True, enemy_x=657.5, occupancy_unit=50., expected="move"),
    dict(name="idle-moving-obstacle", mode="idle", position=3, skill=5, outside=True, enemy_x=807.5, moving_obstacle=True, expected="move"),
    dict(name="return-formation-free", mode="return", position=2, skill=5, expected_home=[0., 0.]),
    dict(name="return-formation-occupied", mode="return", position=2, skill=5, away=500, enemy_x=0., staged_obstacle=True, expected_home=[0., 0.]),
    dict(name="approach-timeout", mode="timeout", position=1, skill=5, run_speed=0.01, walk_speed=0.01),
)


def movement_inputs(state, battle):
    inputs = position_inputs(state, battle)
    for row, actor in zip(inputs, battle["actors"]):
        profile = state.read(actor["address"] + 4)
        row.update(positioning_radius=state.read(profile + 0xe7, "B"),
            run_speed=state.read(profile + 0x24, "f"), walk_speed=state.read(profile + 0x20, "f"))
    return inputs


def review(rows, case):
    valid = bool(rows) and all([(r["owner_strategy"] >> shift) & 255 for shift in (24, 16, 8)]
        == [1, case["skill"], case["position"]] and r["battle_actor_1_flags"] >> 27 & 7 == 2 for r in rows)
    actions = [r["battle_actor_1_action"] >> 24 for r in rows]
    xs = [float_word(r["battle_actor_1_x_bits"]) for r in rows]
    zs = [float_word(r["battle_actor_1_z_bits"]) for r in rows]
    displacement = max((math.hypot(x - xs[0], z - zs[0]) for x, z in zip(xs, zs)), default=0.)
    first_return = next((i for i, action in enumerate(actions) if action == 6), None)
    first_normal = next((i for i, action in enumerate(actions) if action == 5
        and rows[i]["owner_selected"] and not rows[i]["owner_native"] & 65535), None)
    home = [float_word(rows[first_return]["owner_home_" + axis]) for axis in ("x", "z")] if first_return is not None else None
    ended = next((i for i in range((first_return or 0) + 1, len(rows)) if actions[i] in (2, 3)), None) if first_return is not None else None
    first_stop = next((i for i, action in enumerate(actions) if action == 10), None)
    first_approach = next((i for i, action in enumerate(actions) if action == 4), None)
    approach_end = next((i for i in range(first_approach + 1, len(rows)) if actions[i] != 4), None) if first_approach is not None else None
    ages = [r["owner_age_word"] & 65535 for r in rows[first_approach:approach_end]] if first_approach is not None else []
    contact = any(r["battle_actor_4_hp"] < 30000 for r in (rows[:first_return + 1] if first_return is not None else rows))
    sole = all(r[f"battle_actor_{slot}_action"] >> 24 != 5 for r in rows for slot in (0, 4))
    moving_valid = not case.get("moving_obstacle") or bool(rows) and all(r["battle_actor_4_action"] >> 24 == 4 for r in rows)
    unit_valid = "occupancy_unit" not in case or bool(rows) and all(abs(float_word(r["occupancy_unit_word"]) - case["occupancy_unit"]) < 0.0001 for r in rows)
    speed_valid = "run_speed" not in case or bool(rows) and all(abs(float_word(r["owner_run_speed_word"]) - case["run_speed"]) < 0.0001 for r in rows)
    if case["mode"] == "idle":
        expected = not any(action in (4, 5, 6) for action in actions) and (
            displacement > 10 if case["expected"] == "move" else displacement < 0.01)
    elif case["mode"] == "return":
        expected = first_normal is not None and first_return is not None and first_normal < first_return and contact and ended is not None
        expected = expected and all(abs(a - b) < 0.01 for a, b in zip(home, case["expected_home"]))
        expected = expected and 8 in actions[first_normal:first_return]
        if expected and case.get("staged_obstacle"):
            expected = math.hypot(float_word(rows[first_return]["battle_actor_4_x_bits"]) - home[0],
                float_word(rows[first_return]["battle_actor_4_z_bits"]) - home[1]) < 0.01
        if expected:
            start_gap = math.hypot(xs[first_return] - home[0], zs[first_return] - home[1])
            end_gap = math.hypot(xs[ended] - home[0], zs[ended] - home[1])
            expected = end_gap < start_gap
    else:
        expected = bool(ages) and ages[0] == 0 and max(ages) == 300 and approach_end is not None and actions[approach_end] == 10
        expected = expected and rows[approach_end]["owner_age_word"] & 65535 == 301 and not any(action == 5 for action in actions)
    return dict(passed=bool(valid and sole and moving_valid and unit_valid and speed_valid and expected), live_policy_verified=valid,
        live_moving_obstacle_verified=moving_valid, live_occupancy_unit_verified=unit_valid, live_run_speed_verified=speed_valid,
        idle_home=[float_word(rows[-1]["owner_home_" + axis]) for axis in ("x", "z")] if rows else None,
        sole_actor=sole, max_displacement=displacement, first_normal=first_normal,
        first_return=first_return, return_home=home, first_return_end=ended,
        normal_contact=contact, first_stop=first_stop, max_approach_age=max(ages, default=None))


def run(args):
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("movement observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    base = State(args.state)
    battle = inspect(base, battle=True)["battle"]
    actors = {a["slot"]: a for a in battle["actors"]}
    if set(actors) != {0, 1, 4} or (actors[0]["kind"], actors[1]["kind"]) != (1, 4) or any(a["action"][0] != 3 for a in actors.values()):
        raise ValueError("expected the naturally settled manual Lloyd/Raine checkpoint")
    marker_address = base.read(0x8035a578) + 0x581c + 31 * 0x360 + 0x300
    movie = out / "neutral.dtm"
    neutral_movie(base.path, movie, struct.unpack_from("<Q", base.raw, 33)[0], polls=24000)
    ready = out / "ready.s01"
    shutil.copyfile(base.path, ready)
    shutil.copyfile(movie, str(ready) + ".dtm")
    pointer, at = "8035a86c 10 30 540", 0x1cea0 + 0x1ba0
    locations = watch_locations(dict(pool=1, bss_index=6, actors=[dict(slot=i) for i in range(8)]))
    for offset, name in ((0x104, "selected"), (0x1174, "native"), (0x1bc, "age_word"),
        (0x192c, "home_x"), (0x1934, "home_z"), (0x1910, "forward_word")):
        locations[f"{pointer} {at + offset:x}"] = "owner_" + name
    locations[f"{pointer} {at + 8:x} 0"] = "owner_strategy"
    locations[f"{pointer} {at + 4:x} 24"] = "owner_run_speed_word"
    locations[f"{pointer} {0x1cea0 + 4 * 0x1ba0 + 0x1910:x}"] = "enemy_forward_word"
    locations["8035a86c 10 20 15f4"] = "occupancy_unit_word"
    report = dict(complete=False, cases=[], source_sha256=base.sha256,
        scope="Formation decisions, completed post-action return and natural approach timeout; no exact random sequence or path parity.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, ready, out / "worker", marker_address, locations,
            disc_sha256=DISC_SHA256, capture_images=False)
        for case in CASES:
            target = out / case["name"]
            target.mkdir()
            state = arrange(base, battle, case, 0)
            enemy = actors[4]
            if "enemy_x" in case:
                for offset in (0x18c0, 0x192c):
                    state.write(enemy["address"] + offset, "f", case["enemy_x"])
                for slot in (0, 1):
                    actor = actors[slot]
                    distance = math.hypot(state.read(actor["address"] + 0x18c0, "f") - case["enemy_x"],
                        state.read(actor["address"] + 0x18c8, "f"))
                    rank = state.read(enemy["address"] + 0x107c, "B") & 7
                    state.write(actor["address"] + 0x1084 + rank * 4, "f", distance)
            if "run_speed" in case:
                state.write(state.read(actors[1]["address"] + 4) + 0x24, "f", case["run_speed"])
                state.write(state.read(actors[1]["address"] + 4) + 0x20, "f", case["walk_speed"])
            if "occupancy_unit" in case:
                state.write(battle["sections"][4]["address"] + 0x15f4, "f", case["occupancy_unit"])
            if case.get("moving_obstacle"):
                state.write(actors[1]["address"] + 0x107c, "B", state.read(actors[1]["address"] + 0x107c, "B") & ~0x38)
                state.write(enemy["address"] + 0x107c, "B", state.read(enemy["address"] + 0x107c, "B") & ~0x38 | 0x18)
                state.write(state.read(enemy["address"] + 4) + 0x24, "f", 0.01)
            marker = int.from_bytes(sha256(json.dumps(case, sort_keys=True).encode()).digest()[:4])
            state.write(marker_address, "I", marker)
            initial = target / "input.s01"
            state.save(initial)
            shutil.copyfile(movie, str(initial) + ".dtm")
            write(target / "fixture.json", dict(case=case, changes=state.changes,
                position_inputs=movement_inputs(state, battle), case_marker=marker, game_code_modified=False))
            prefix = []
            if case.get("moving_obstacle") or case.get("staged_obstacle"):
                moving = case.get("moving_obstacle")
                prefix = worker.observe_until(initial, target / "prelude", marker, 350,
                    lambda rows: rows[-1]["battle_actor_4_action"] >> 24 == 4 if moving else
                        rows[-1]["battle_actor_1_action"] >> 24 == 8 and float_word(rows[-1]["enemy_forward_word"]) == 0., stepped=True)
                checkpoint = target / "prelude.s01"
                worker.snapshot(checkpoint)
                state = State(checkpoint)
                if moving:
                    state.write(actors[1]["address"] + 0x107c, "B", state.read(actors[1]["address"] + 0x107c, "B") & ~0x38 | 0x10)
                else:
                    for axis in range(3):
                        state.write(enemy["address"] + 0x18c0 + axis * 4, "f", 0.)
                initial = target / "switch.s01"
                state.save(initial)
                shutil.copyfile(str(checkpoint) + ".dtm", str(initial) + ".dtm")
                write(target / "switch.json", dict(changes=state.changes, position_inputs=movement_inputs(state, battle), game_code_modified=False))
            rows = worker.observe(initial, target / "observations", marker, 90 if case["mode"] == "idle" else 700)
            worker.snapshot(target / "terminal.s01")
            result = dict(case=case, **review(prefix + rows if case.get("staged_obstacle") else rows, case))
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = all(c["passed"] for c in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("movement observations differ; inspect report.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
