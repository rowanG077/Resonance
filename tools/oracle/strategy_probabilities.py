#!/usr/bin/env python3
"""Observe one autonomous combo checkpoint with controlled random residues.

Prime attacks through ordinary Auto decisions with techniques suppressed. Replay
the resulting checkpoint with declared strategy, TP and random state only.
"""
import argparse
from hashlib import sha256
import json
from pathlib import Path
import shutil
import struct

from battle_fixture import BattleFixture, neutral_movie
from battle_state import watch_locations
from state import State, digest, inspect
from strategy_buffs import DISC_SHA256
from strategy_combos import arrange, write
from technique_content import native_learning


def advance(seed):
    return (seed * 0x41c64e6d + 1234567) & 0xffffffff


def draws(seed, count=2):
    result = []
    for _ in range(count):
        seed = advance(seed)
        result.append(seed >> 16)
    return result


def cases(phase):
    if phase == "level":
        residues, modulus = [(i,) for i in range(8)], 8
    elif phase == "reserve":
        residues, modulus = [(i,) for i in (0, 24, 25, 99)], 100
    elif phase == "throw-level":
        residues, modulus = [(49, 7, 99), (50, 0, 99), (50, 1, 49), (50, 1, 50), (99, 7, 99)], 100
    else:
        residues, modulus = [(0, 99), (49, 99), (50, 49), (50, 50), (99, 0), (99, 99)], 100
    result = []
    for wanted in residues:
        moduli = (100, 8, 100) if phase == "throw-level" else (modulus,) * len(wanted)
        seed = next(seed for seed in range(1000000)
                    if tuple(value % limit for value, limit in zip(draws(seed, len(wanted)), moduli)) == wanted)
        admitted = wanted[0] == 0 if phase == "level" else (
            wanted[0] < 25 if phase == "reserve" else (
                wanted[0] < 50 or wanted[1] == 0 or wanted[2] < 50
                if phase == "throw-level" else min(wanted) < 50))
        result.append(dict(name=phase + "-" + "-".join(map(str, wanted)),
                           phase=phase, seed=seed, residues=wanted, modulus=modulus,
                           moduli=moduli,
                           admitted=admitted))
    return result


def review(rows, case, initial, mask):
    native = 35 if case["phase"].startswith("throw") else 10
    level = 15 if case["phase"] in ("level", "throw-level") else 30
    tp = 100 if case["phase"] == "level" else 30
    skill = 1 if case["phase"] == "level" else 6
    position = 2 if case["phase"].startswith("throw") else 1
    policy = bool(rows) and all(
        [(r["owner_strategy"] >> shift) & 255 for shift in (24, 16, 8)] == [1, skill, position]
        and r["battle_actor_0_flags"] >> 27 & 7 == 2
        and r["battle_actor_0_flags"] >> 16 & 7 == 3
        and r["battle_actor_0_tp_word"] >> 16 == 100
        and (r["owner_enabled_0"] << 32 | r["owner_enabled_4"]) == mask
        and r["owner_level_word"] >> 16 & 255 == level
        and r["owner_saved_level_word"] >> 24 == level
        for r in rows)
    policy &= bool(rows) and rows[0]["battle_actor_0_tp_word"] & 65535 == tp
    learned = bool(rows) and all((r["owner_learned_0"], r["owner_learned_4"]) ==
        (rows[0]["owner_learned_0"], rows[0]["owner_learned_4"]) for r in rows)
    sole = all(r[f"battle_actor_{slot}_action"] >> 24 != 5 for r in rows for slot in (1, 4))
    selected = [r["owner_native"] & 65535 for r in rows if r["owner_native"] & 65535]
    paid = tp - (rows[-1]["battle_actor_0_tp_word"] & 65535) if rows else None
    crossed = any(r["battle_actor_0_cast_clock_word"] & 65535 > initial["checkpoint"]
                  or r["owner_selected"] != initial["selected"] or r["owner_native"] & 65535
                  for r in rows)
    reachable = [case["seed"]]
    for _ in range(2):
        reachable.append(advance(reachable[-1]))
    # A state saved directly on the checkpoint executes the gate before its
    # first observed VI. Earlier states must expose the unchanged seed first.
    expected_first = reachable[2 if selected else 1] if initial["age"] == initial["checkpoint"] else case["seed"]
    random_start = bool(rows) and rows[0]["battle_random_state_word"] == expected_first
    random_used = any(r["battle_random_state_word"] != case["seed"] for r in rows)
    outcome = bool(selected) == case["admitted"] and (not selected or set(selected) == {native})
    cost = 5 if case["admitted"] else 0
    return dict(passed=bool(policy and learned and sole and crossed and random_start and random_used and outcome and paid == cost),
                live_inputs_verified=policy, learned_repertoire_unchanged=learned, sole_actor=sole,
                crossed_checkpoint=crossed, initial_random_state_verified=random_start, random_used=random_used,
                selected_natives=sorted(set(selected)), paid=paid,
                observed_random_states=[r["battle_random_state_word"] for r in rows],
                observed_ages=[r["battle_actor_0_cast_clock_word"] & 65535 for r in rows])


def run(args):
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("probability observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = State(args.state)
    battle = inspect(source, battle=True)["battle"]
    actors = {a["slot"]: a for a in battle["actors"]}
    character = 2 if args.phase.startswith("throw") else 1
    if set(actors) != {0, 1, 4} or actors[0]["kind"] != character or any(a["action"][0] != 3 for a in actors.values()):
        raise ValueError("expected a naturally settled manual party with the requested owner")
    catalogue = args.assets / "game/techniques.json"
    learning = native_learning(json.loads(catalogue.read_text()))[character - 1]
    level = 15 if args.phase in ("level", "throw-level") else 30
    stage = 2 if args.phase == "reserve" else 0
    fixture = dict(position=2 if character == 2 else 1, skill=5,
                   technique=35 if character == 2 else 10, level=level,
                   random_state=2, target_x=500 if character == 2 else 180)
    base, mask = arrange(source, battle, fixture, learning)
    if character == 2:
        profile = base.read(actors[4]["address"] + 4)
        base.write(profile + 0x5c, "I", base.read(profile + 0x5c) | 1)
    marker_address = source.read(0x8035a578) + 0x581c + 31 * 0x360 + 0x300
    marker = int.from_bytes(sha256(args.phase.encode()).digest()[:4])
    base.write(marker_address, "I", marker)
    priming = out / "priming.s01"
    base.save(priming)
    movie = out / "neutral.dtm"
    neutral_movie(source.path, movie, struct.unpack_from("<Q", source.raw, 33)[0], polls=24000)
    shutil.copyfile(movie, str(priming) + ".dtm")
    write(out / "priming.json", dict(changes=base.changes, case_marker=marker, game_code_modified=False))
    pointer, at = "8035a86c 10 30 540", 0x1cea0
    locations = watch_locations(dict(pool=1, bss_index=6, actors=[dict(slot=i) for i in range(8)]))
    for offset, name in ((0x1174, "native"), (0x104, "selected"), (0x1170, "checkpoints"),
                         (0x117c, "combo_word"), (0x1180, "style_word"), (0x199c, "contact")):
        locations[f"{pointer} {at + offset:x}"] = "owner_" + name
    locations[f"{pointer} {at + 8:x} 0"] = "owner_strategy"
    locations[f"{pointer} {at + 8:x} 1c"] = "owner_level_word"
    locations[f"{pointer} {at:x} 10"] = "owner_saved_level_word"
    for offset, name in ((0x70, "learned"), (0x78, "enabled")):
        for word in (0, 4):
            locations[f"{pointer} {at:x} {offset + word:x}"] = f"owner_{name}_{word}"
    report = dict(complete=False, phase=args.phase, cases=[], source_sha256=source.sha256,
                  scope="One naturally reached combo checkpoint with controlled random residues; no full-sequence RNG, timing or audiovisual equivalence.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, priming, out / "worker", marker_address, locations,
                               disc_sha256=DISC_SHA256, capture_images=False)
        def reached(rows):
            r = rows[-1]
            return (r["battle_actor_0_action"] >> 24 == 5 and r["owner_selected"]
                    and not r["owner_native"] & 65535 and r["owner_contact"]
                    and r["owner_combo_word"] >> 16 & 255 == stage
                    and r["owner_checkpoints"] >> 16 > 0
                    and r["battle_actor_0_cast_clock_word"] & 65535 == (r["owner_checkpoints"] >> 16) - (args.phase != "reserve"))
        rows = worker.observe_until(priming, out / "priming", marker, 240, reached, stepped=True)
        if not reached(rows):
            raise ValueError("natural attack did not reach the requested effective checkpoint")
        ready = out / "ready.s01"
        worker.snapshot(ready)
        base = State(ready)
        actor = actors[0]["address"]
        initial = dict(checkpoint=base.read(actor + 0x1170, "H"),
                       age=base.read(actor + 0x1be, "H"), selected=base.read(actor + 0x104),
                       stage=base.read(actor + 0x117d, "B"), normal=base.read(actor + 0x1181, "B"),
                       contact=base.read(actor + 0x199c), hp=base.read(actors[4]["vitals_address"] + 0x24))
        if initial["age"] != initial["checkpoint"] - (args.phase != "reserve") or initial["stage"] != stage or not initial["contact"]:
            raise ValueError("paused checkpoint moved past the declared natural boundary")
        report["initial"] = initial
        write(out / "ready.json", initial)
        for case in cases(args.phase):
            target = out / case["name"]
            target.mkdir()
            state = base.fork()
            state.write(actors[0]["vitals_address"] + 1, "B", 1 if args.phase == "level" else 6)
            state.write(actors[0]["vitals_address"] + 0x2a, "H", 100 if args.phase == "level" else 30)
            state.write(battle["pool"] + 0x7a670, "I", case["seed"])
            case_marker = int.from_bytes(sha256(case["name"].encode()).digest()[:4])
            state.write(marker_address, "I", case_marker)
            path = target / "input.s01"
            state.save(path)
            shutil.copyfile(str(ready) + ".dtm", str(path) + ".dtm")
            write(target / "fixture.json", dict(case=case, changes=state.changes, case_marker=case_marker,
                                                 input_random_state=state.read(battle["pool"] + 0x7a670),
                                                 state_sha256=digest(path), game_code_modified=False))
            rows = worker.observe(path, target / "observations", case_marker, 8)
            result = dict(case=case, observations_sha256=digest(target / "observations/observations.json"),
                          **review(rows, case, initial, mask))
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = all(case["passed"] for case in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("probability observations differ; inspect report.json")
    write(out / "outcome-acceptance.json", dict(report, input_report_sha256=digest(out / "report.json")))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--phase", choices=("level", "reserve", "throw", "throw-level"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
