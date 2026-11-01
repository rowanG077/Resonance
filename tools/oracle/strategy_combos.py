#!/usr/bin/env python3
"""Observe original companion combo admission from a naturally settled battle.

Component fixtures declare controls, placement, vitals and learned/enable masks.
No instructions, action states, poses or timers are replaced.
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
from strategy_positions import position_inputs
from technique_content import native_learning

CASES = (
    dict(name="default-demon-fang", position=0, skill=1, technique=1, expected_native=1),
    dict(name="frontlines-demon-fang", position=1, skill=1, technique=1, expected_native=1),
    dict(name="frontlines-sonic-thrust", position=1, skill=1, technique=10, expected_native=10),
    dict(name="no-use-sonic-thrust", position=1, skill=5, technique=10, expected_native=None),
    dict(name="no-use-unrestricted", position=6, skill=5, technique=10, expected_native=10),
    dict(name="hold-committed-combo", position=3, skill=1, technique=10, staged=True, random_state=2, expected_native=10),
    dict(name="ranged-committed-combo", position=4, skill=1, technique=10, staged=True, random_state=2, expected_native=10),
    dict(name="magic-committed-combo", position=5, skill=1, technique=10, staged=True, random_state=2, expected_native=10),
    dict(name="level-one-early", position=1, skill=1, technique=10, level=1, expected_native=10),
    dict(name="save-at-reserve", position=1, skill=4, technique=10, tp=750, max_tp=1000, expected_native=None),
    dict(name="save-above-reserve", position=1, skill=4, technique=10, tp=760, max_tp=1000, expected_native=10),
    dict(name="edited-no-use-reserve", position=1, skill=5, technique=10, tp_reserve=0, expected_native=10),
)


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def review(rows, case, mask):
    valid = bool(rows) and all([(r["owner_strategy"] >> shift) & 255 for shift in (24, 16, 8)]
                               == [1, case["skill"], case["position"]]
                               and r["battle_actor_0_flags"] >> 27 & 7 == 2
                               and (r["owner_enabled_0"] << 32 | r["owner_enabled_4"]) == mask
                               for r in rows)
    level = case.get("level", 30)
    difference = max(-8, min(8, (level + 20) // 2 - 20)) & 255
    levels_valid = all(r["owner_vitals_level_word"] >> 16 & 255 == level
                       and r["owner_saved_level_word"] >> 24 == level
                       and r["level_difference_word"] & 255 == difference for r in rows)
    learned = bool(rows) and all((r["owner_learned_0"], r["owner_learned_4"])
                               == (rows[0]["owner_learned_0"], rows[0]["owner_learned_4"]) for r in rows)
    reserve_valid = "tp_reserve" not in case or bool(rows) and all(
        r["reserve_tail_word"] >> 16 & 255 == case["tp_reserve"] for r in rows)
    normals = [i for i, r in enumerate(rows) if r["battle_actor_0_action"] >> 24 == 5
               and r["owner_selected"] and not r["owner_native"] & 65535]
    techniques = [(i, r["owner_native"] & 65535) for i, r in enumerate(rows)
                  if r["battle_actor_0_action"] >> 24 == 5 and r["owner_native"] & 65535]
    first = techniques[0] if techniques else None
    before = first[0] if first else len(rows)
    normal_tables = {rows[i]["owner_selected"] for i in normals if i < before}
    contact = any(r["battle_actor_4_hp"] < 30000 for r in rows[:before])
    sole = all(r[f"battle_actor_{slot}_action"] >> 24 != 5 for r in rows for slot in (1, 4))
    expected = case["expected_native"]
    admitted = (first is not None and first[1] == expected and normals and normals[0] < first[0]) if expected is not None else not techniques and len(normal_tables) >= 2
    payment = None
    if first:
        for i in range(first[0], len(rows)):
            if i and rows[i]["owner_native"] & 65535 == first[1]:
                delta = (rows[i - 1]["battle_actor_0_tp_word"] & 65535) - (rows[i]["battle_actor_0_tp_word"] & 65535)
                if delta > 0:
                    payment = dict(sample=i, amount=delta)
                    break
    paid = expected is None or payment is not None and payment["amount"] == {1: 4, 10: 5}[expected]
    before_word = rows[first[0] - 1]["owner_combo_word"] if first and first[0] else None
    stage = before_word >> 16 & 255 if before_word is not None else None
    limit = before_word >> 5 & 7 if before_word is not None else None
    stage_valid = (first is None or before_word is not None and (case.get("level", 30) < 30 or case["position"] == 6 or stage >= limit - 1))
    return dict(passed=bool(valid and levels_valid and learned and reserve_valid and admitted and contact and sole and paid and stage_valid),
                edited_reserve_verified=reserve_valid,
                stage_before_technique=stage, normal_limit=limit, live_levels_verified=levels_valid, learned_repertoire_unchanged=learned, live_policy_and_enable_verified=valid,
                first_normal=normals[0] if normals else None, first_technique=first,
                distinct_normal_tables=len(normal_tables), normal_contact=contact, sole_actor=sole,
                first_normal_direction=rows[normals[0]]["owner_style_word"] >> 16 & 255 if normals else None,
                combo_word_at_technique=rows[first[0]]["owner_combo_word"] if first else None,
                combo_word_before_technique=rows[first[0] - 1]["owner_combo_word"] if first and first[0] else None,
                payment=payment, first_tp=rows[0]["battle_actor_0_tp_word"] & 65535 if rows else None,
                last_tp=rows[-1]["battle_actor_0_tp_word"] & 65535 if rows else None)


def arrange(base, battle, case, learning):
    state = base.fork()
    actors = {a["slot"]: a for a in battle["actors"]}
    positions = {0: [-100., 0., 0.], 1: [-500., 0., -250.], 4: [float(case.get("target_x", 180)), 0., 0.]}
    for slot, actor in actors.items():
        at, vitals = actor["address"], actor["vitals_address"]
        state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38 | (0x10 if slot == 0 else 0))
        for axis, value in enumerate(positions[slot]):
            state.write(at + 0x18c0 + axis * 4, "f", value)
            state.write(at + 0x192c + axis * 4, "f", value)
            state.write(at + 0x1938 + axis * 4, "f", value - positions[0][axis] if slot < 4 else 0.)
        state.write(vitals + 0x1d, "B", 20)
        if slot < 4:
            state.write(state.read(at) + 0x10, "B", 20)
        for offset in (0x20, 0x24):
            state.write(vitals + offset, "I", 2000 if slot < 4 else 30000)
        for other_slot, other in actors.items():
            if other["side"] == actor["side"]:
                continue
            rank = state.read(other["address"] + 0x107c, "B") & 7
            distance = math.hypot(positions[slot][0] - positions[other_slot][0], positions[slot][2] - positions[other_slot][2])
            state.write(at + 0x1084 + rank * 4, "f", distance)
    if "random_state" in case:
        state.write(battle["pool"] + 0x7a670, "I", case["random_state"])
    if "tp_reserve" in case:
        state.write(battle["sections"][4]["address"] + 0x2860 + case["skill"], "B", case["tp_reserve"])
    owner = actors[0]
    level = case.get("level", 30)
    state.write(owner["vitals_address"] + 0x1d, "B", level)
    state.write(state.read(owner["address"]) + 0x10, "B", level)
    state.write(battle["pool"] + 0x159c3, "b", max(-8, min(8, (level + 20) // 2 - 20)))
    for offset, value in enumerate((1, case["skill"], case["position"])):
        state.write(owner["vitals_address"] + offset, "B", value)
    state.write(owner["vitals_address"] + 0x28, "H", case.get("max_tp", 100))
    state.write(owner["vitals_address"] + 0x2a, "H", case.get("tp", 100))
    mask = 1 << learning.index(case["technique"])
    member = state.read(owner["address"])
    state.write(member + 0x70, "Q", (1 << len(learning)) - 1)
    state.write(member + 0x78, "Q", mask)
    return state, mask


def run(args):
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("combo observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = State(args.state)
    battle = inspect(source, battle=True)["battle"]
    actors = {a["slot"]: a for a in battle["actors"]}
    if set(actors) != {0, 1, 4} or actors[0]["kind"] != 1 or actors[1]["kind"] != 4 or battle["phase"] != 3:
        raise ValueError("expected the initialized Lloyd/Raine combat checkpoint")
    catalogue = args.assets / "game/techniques.json"
    learning = native_learning(json.loads(catalogue.read_text()))[0]
    marker_address = source.read(0x8035a578) + 0x581c + 31 * 0x360 + 0x300
    movie = out / "neutral.dtm"
    neutral_movie(source.path, movie, struct.unpack_from("<Q", source.raw, 33)[0], polls=24000)
    settling = out / "settling.s01"
    state = source.fork()
    for actor in actors.values():
        at = actor["address"]
        state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38)
    settle_marker = int.from_bytes(sha256(b"combo-settle").digest()[:4])
    state.write(marker_address, "I", settle_marker)
    state.save(settling)
    shutil.copyfile(movie, str(settling) + ".dtm")
    write(out / "settling.json", dict(changes=state.changes, case_marker=settle_marker, game_code_modified=False))
    pointer = "8035a86c 10 30 540"
    at = 0x1cea0
    locations = watch_locations(dict(pool=1, bss_index=6, actors=[dict(slot=i) for i in range(8)]))
    for offset, name in ((0x1174, "native"), (0x104, "selected"), (0x117c, "combo_word"), (0x1180, "style_word")):
        locations[f"{pointer} {at + offset:x}"] = "owner_" + name
    locations[f"{pointer} {at + 8:x} 0"] = "owner_strategy"
    locations[f"{pointer} {at + 8:x} 1c"] = "owner_vitals_level_word"
    locations[f"{pointer} {at:x} 10"] = "owner_saved_level_word"
    locations[f"{pointer} 159c0"] = "level_difference_word"
    locations["8035a86c 10 20 2864"] = "reserve_tail_word"
    for offset, name in ((0x70, "learned"), (0x78, "enabled")):
        for word in (0, 4):
            locations[f"{pointer} {at:x} {offset + word:x}"] = f"owner_{name}_{word}"
    report = dict(complete=False, cases=[], source_sha256=source.sha256,
                  techniques_sha256=digest(catalogue),
                  scope="Declared repertoire and neutral opponent; first effective normal-to-arte admission, not timing or audiovisual parity.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, settling, out / "worker", marker_address, locations,
                               disc_sha256=DISC_SHA256, capture_images=False)
        worker.observe_until(settling, out / "settling", settle_marker, 900,
                             lambda rows: all(rows[-1][f"battle_actor_{slot}_action"] >> 24 == 3 for slot in actors))
        ready = out / "ready.s01"
        worker.snapshot(ready)
        base = State(ready)
        battle = inspect(base, battle=True)["battle"]
        if any(a["action"][0] != 3 for a in battle["actors"]):
            raise ValueError("manual opponents did not naturally finish their committed actions")
        cases = [case for case in CASES if (case["name"] in args.cases if args.cases else "tp_reserve" not in case)]
        if not cases or args.cases and set(args.cases) != {case["name"] for case in cases}:
            raise ValueError("unknown or empty combo case selection")
        for case in cases:
            target = out / case["name"]
            target.mkdir()
            state, mask = arrange(base, battle, {**case, "position": 1, "skill": 5} if case.get("staged") else case, learning)
            marker = int.from_bytes(sha256(json.dumps(case, sort_keys=True).encode()).digest()[:4])
            state.write(marker_address, "I", marker)
            state.save(target / "input.s01")
            shutil.copyfile(str(ready) + ".dtm", target / "input.s01.dtm")
            write(target / "fixture.json", dict(case=case, changes=state.changes, case_marker=marker,
                position_inputs=position_inputs(state, battle), enabled_mask=mask,
                repertoire="Declared full learned repertoire, one autonomous technique enabled",
                game_code_modified=False, state_sha256=digest(target / "input.s01")))
            input_state = target / "input.s01"
            if case.get("staged"):
                worker.observe_until(input_state, target / "normal-prelude", marker, 180,
                    lambda rows: rows[-1]["battle_actor_0_action"] >> 24 == 5 and rows[-1]["owner_selected"] != 0
                    and not rows[-1]["owner_native"] & 65535, stepped=True)
                worker.snapshot(target / "normal.s01")
                state = State(target / "normal.s01")
                if state.read(actors[0]["address"] + 0x1ac, "B") != 5:
                    raise ValueError("staged combo did not reach a committed normal")
                for offset, value in enumerate((1, case["skill"], case["position"])):
                    state.write(actors[0]["vitals_address"] + offset, "B", value)
                marker ^= 0xa5a5a5a5
                state.write(marker_address, "I", marker)
                input_state = target / "switch.s01"
                state.save(input_state)
                shutil.copyfile(str(target / "normal.s01") + ".dtm", str(input_state) + ".dtm")
                write(target / "switch.json", dict(changes=state.changes, case_marker=marker, game_code_modified=False))
            rows = worker.observe(input_state, target / "observations", marker, args.vis)
            worker.snapshot(target / "terminal.s01")
            result = dict(case=case, **review(rows, case, mask))
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = all(case["passed"] for case in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("combo observations differ; inspect report.json")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True, help="Initialized Lloyd/Raine battle checkpoint with .dtm companion")
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--vis", type=int, default=420)
    parser.add_argument("--cases", nargs="*", help="Optional named subset; edited-no-use-reserve is an opt-in table-edit case")
    run(parser.parse_args())
