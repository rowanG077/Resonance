#!/usr/bin/env python3
"""Observe character-specific companion admission with declared component data.

Entry uses ordinary scenario commands. Over Limit begins naturally from a full
inactive charge; no action, pose, availability state or active timer is replaced.
"""
import argparse
from hashlib import sha256
import json
from pathlib import Path
import shutil
import struct

from battle_fixture import BattleFixture, neutral_movie
from battle_state import watch_locations
from effect_fixture import prepare
from state import State, digest, inspect
from strategy_buffs import DISC_SHA256
from strategy_combos import arrange, write
from technique_content import native_learning


def cases(character, suite="characters"):
    if suite == "elemental":
        result = []
        for name, flags, level, fire, seeds, pool in (
            ("weak-unmarked", 0, 20, 1, (0, 1), [62, 66]),
            ("weak-marked", 0x20, 20, 1, (0, 1), [66]),
            ("weak-level-two", 0x20, 24, 1, (0,), [66]),
            ("weak-level-three", 0x20, 26, 1, (0, 1), [62, 66]),
            ("immune-unmarked", 0, 20, 4, (0, 1), [62, 66]),
            ("immune-marked", 0x10000, 20, 4, (0,), [62]),
        ):
            for seed in seeds:
                result.append(dict(name=f"elemental-{name}-{seed}", character=3,
                    technique=62, techniques=[62, 66], costs={62: 8, 66: 7},
                    native_costs={200: 8, 204: 7}, position=5, skill=1, level=level,
                    target_strategy_flags=flags, target_fire_affinity=fire,
                    random_state=seed, catalogue_pool=pool,
                    native_pool=[200 if key == 62 else 204 for key in pool], observation_vis=420))
        return result
    if suite == "mixed":
        techniques = [1, 2, 10, 24] if character == 1 else [62, 66]
        costs = {1: 4, 2: 16, 10: 5, 24: 30} if character == 1 else {62: 8, 66: 7}
        native = dict(zip(techniques, techniques if character == 1 else [200, 204]))
        result = []
        for position in (0, 1, 2, 4, 5, 6):
            case = dict(name=f"mixed-{character}-position-{position}", character=character,
                        technique=techniques[0], techniques=techniques, costs=costs,
                        native_costs={native[key]: cost for key, cost in costs.items()},
                        position=position, skill=1, observation_vis=420 if character == 3 else 180)
            if (character == 1 and position in (0, 1)) or (character == 3 and position == 1):
                case["expected_first"] = 0
            elif (character == 1 and position == 5) or (character == 3 and position == 4):
                case["expected_idle"] = True
            else:
                pool = [1, 2] if character == 1 and position in (2, 4) else techniques
                case.update(catalogue_pool=pool, native_pool=[native[key] for key in pool])
                if character == 1 and position == 2:
                    case["allow_normal_opening"] = True
            result.append(case)
        if character == 1:
            result.append(dict(result[-1], name="mixed-1-committed-hold", position=3,
                               staged=True, initial_position=1, random_state=2, expected_stage=2))
        return result
    if suite == "openings":
        technique, native, cost = (1, 1, 4) if character == 1 else (66, 204, 7)
        expected = {1: (0, 0, 1, 1, None, 1), 3: (204, 0, 204, None, 204, 204)}[character]
        result = [dict(name=f"opening-{character}-position-{position}", character=character,
                       position=position, skill=1, technique=technique,
                       **({"expected_first": first} if first is not None else {"expected_idle": True}), cost=cost)
                  for position, first in zip((0, 1, 2, 4, 5, 6), expected)]
        if character == 1:
            result += [dict(name=f"opening-edited-default-{position}", character=1,
                            position=position, skill=1, technique=1,
                            default_position=5, **({"expected_first": 1} if position == 0 else {"expected_idle": True}), cost=4)
                       for position in (0, 5)]
        return result
    if character == 2:
        result = [dict(name="colette-" + name, character=2, position=position, skill=1,
                     technique=35, expected_native=expected, cost=5, staged=True,
                     random_state=seed, expected_opening=opening, expected_stage=stage,
                     flying=flying, target_x=500)
                for name, position, seed, opening, expected, stage, flying in (
                    ("throw-thrust", 2, 0, 2, 35, 0, False),
                    ("throw-low", 2, 2, 3, 35, 0, True),
                    ("neutral", 1, 1, 0, 35, 1, False))]
        for name, hp, gap, target_x, opening, stage in (
                ("close-75", 1500, None, 180, 0, 1),
                ("close-74", 1480, None, 180, 2, 0),
                ("edited-melee-gap", 2000, 1000., 500, 0, 1)):
            result.append(dict(name="colette-" + name, character=2, position=2, skill=1,
                technique=35, expected_native=35, cost=5, staged=True, random_state=0,
                expected_opening=opening, expected_stage=stage, hp=hp, target_x=target_x,
                **({"melee_gap": gap} if gap is not None else {})))
        return result
    if character == 5:
        return [dict(name="sheena-seal", character=5, position=6, skill=1,
                     technique=126, expected_native=63, cost=5),
                dict(name="sheena-overlimit-seal", character=5, position=6, skill=1,
                     technique=126, expected_native=None, overlimit=True),
                dict(name="sheena-overlimit-summon", character=5, position=6, skill=1,
                     technique=237, expected_native=286, cost=100, overlimit=True)]
    return [dict(name=f"character-{character}-hp-{hp}", character=character,
                 position=1, skill=1, technique=210, hp=hp,
                 expected_first=10 if hp < 600 else 0, cost=5)
            for hp in (600, 580)]


def admitted(rows):
    return [(i, r["owner_native"] & 65535) for i, r in enumerate(rows)
            if r["battle_actor_0_action"] >> 24 == 5 and (r["owner_selected"] or r["owner_native"] & 65535)]


def review(rows, case, mask):
    choices = [1, case["skill"], case["position"]]
    valid = bool(rows) and all([(r["owner_strategy"] >> shift) & 255 for shift in (24, 16, 8)] == choices
        and r["battle_actor_0_flags"] >> 27 & 7 == 2
        and (r["owner_enabled_0"] << 32 | r["owner_enabled_4"]) == mask for r in rows)
    learned = bool(rows) and all((r["owner_learned_0"], r["owner_learned_4"])
        == (rows[0]["owner_learned_0"], rows[0]["owner_learned_4"]) for r in rows)
    sole = all(r[f"battle_actor_{slot}_action"] >> 24 != 5 for r in rows for slot in (1, 4))
    gap_valid = "melee_gap" not in case or bool(rows) and all(r["colette_melee_gap_word"] ==
        int.from_bytes(struct.pack(">f", case["melee_gap"])) for r in rows)
    hp_valid = "hp" not in case or bool(rows) and all(r["battle_actor_0_hp"] == case["hp"] for r in rows)
    flight_valid = "flying" not in case or bool(rows) and all(bool(r["target_traits_word"] & 1) == case["flying"] for r in rows)
    default_valid = "default_position" not in case or bool(rows) and all(
        r["default_position_word"] >> 24 == case["default_position"] for r in rows)
    decision_end = next((i + 1 for i, r in enumerate(rows) if r["battle_actor_0_action"] >> 24 == 5
                         and r["owner_native"] & 65535), len(rows))
    elemental_valid = "target_strategy_flags" not in case or bool(rows) and all(
        r["target_traits_word"] & 0x10020 == case["target_strategy_flags"]
        and r["target_affinities_0"] >> 8 & 255 == 0
        and r["target_affinities_4"] >> 24 == case["target_fire_affinity"]
        and r["owner_level_word"] >> 16 & 255 == case["level"]
        and r["owner_saved_level_word"] >> 24 == case["level"]
        and r["level_difference_word"] & 255 == ((case["level"] + 20) // 2 - 20) & 255
        for r in rows[:decision_end])
    starts = admitted(rows)
    normals = [(i, r["owner_style_word"] >> 16 & 255) for i, native in starts
               if native == 0 for r in [rows[i]]]
    techniques = [(i, native) for i, native in starts if native]
    first = techniques[0] if techniques else None
    admission_rows = rows[:first[0] + 1] if first else rows
    limit = bool(admission_rows) and all(r["battle_actor_0_flags"] >> 16 & 7 ==
        (4 if case.get("overlimit") else 3) for r in admission_rows)
    payment = None
    if first:
        for i in range(first[0], len(rows)):
            if i and rows[i]["owner_native"] & 65535 == first[1]:
                delta = (rows[i - 1]["battle_actor_0_tp_word"] & 65535) - (rows[i]["battle_actor_0_tp_word"] & 65535)
                if delta > 0:
                    payment = delta
                    break
    if case.get("expected_idle"):
        expected = (len(rows) >= 150 and not starts
                    and all(r["battle_actor_0_action"] >> 24 == 3 for r in rows)
                    and all(r[f"battle_actor_0_{axis}_bits"] == rows[0][f"battle_actor_0_{axis}_bits"]
                            for r in rows for axis in ("x", "y", "z"))
                    and all(r["battle_actor_0_tp_word"] & 65535 == 100 for r in rows))
        paid = True
    elif "native_pool" in case:
        if case.get("allow_normal_opening") and starts and starts[0][1] == 0:
            expected, paid = True, True
        else:
            expected = bool(first) and first[1] in case["native_pool"] and bool(starts) and (
                starts[0][1] == 0 if case.get("staged") else starts[0][1] == first[1])
            costs = {int(key): value for key, value in case["native_costs"].items()}
            paid = bool(first) and payment == costs.get(first[1])
    elif "expected_first" in case:
        expected = bool(starts) and starts[0][1] == case["expected_first"]
        paid = case["expected_first"] == 0 or payment == case["cost"]
    elif case["expected_native"] is None:
        expected, paid = bool(normals) and not techniques, True
    else:
        expected = first is not None and first[1] == case["expected_native"]
        paid = payment == case["cost"]
    stage = rows[first[0]]["owner_combo_word"] >> 16 & 255 if first else None
    contact = bool(first) and any(r["battle_actor_4_hp"] < 30000 for r in rows[:first[0]])
    staged_contact = contact if first else any(r["battle_actor_4_hp"] < 30000 for r in rows)
    staged = not case.get("staged") or (bool(normals) and
        ("expected_opening" not in case or normals[0][1] == case["expected_opening"])
        and staged_contact and stage == case["expected_stage"])
    consumed = any(r["battle_actor_0_flags"] >> 16 & 7 == 3
        and r["battle_actor_0_tp_word"] & 65535 == 0 for r in rows)
    consumption_valid = case.get("technique") != 237 or consumed
    return dict(passed=bool(valid and learned and limit and sole and gap_valid and hp_valid and flight_valid and default_valid and elemental_valid and expected and paid and staged and consumption_valid),
        live_elemental_inputs_verified=elemental_valid,
        live_default_position_verified=default_valid,
        overlimit_consumed=consumed,
        live_gap_verified=gap_valid, live_hp_verified=hp_valid, live_flight_verified=flight_valid,
        live_policy_verified=valid, repertoire_unchanged=learned, overlimit_verified=limit,
        sole_actor=sole, first_action=starts[0] if starts else None,
        first_normal=normals[0] if normals else None, first_technique=first,
        normal_contact_before_technique=contact, stage_before_technique=stage, payment=payment)


def run(args):
    if args.suite in ("openings", "mixed") and args.character not in (1, 3):
        raise ValueError("opening and mixed suites require Lloyd or Genis")
    if args.suite == "elemental" and args.character != 3:
        raise ValueError("elemental suite requires Genis")
    if args.suite == "characters" and args.character not in (2, 5, 6, 9):
        raise ValueError("character suite requires Colette, Sheena, Zelos or Kratos")
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("character observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = State(args.state)
    observed = inspect(source)
    catalogue = args.assets / "game/techniques.json"
    learning = native_learning(json.loads(catalogue.read_text()))[args.character - 1]
    entry = out / "entry.s01"
    commands = [(0x24, [args.character]), (0x24, [4])]
    if args.character == 2:
        commands.append((0x5a, [2, 3]))
    commands.append((0xcd, [0, 0, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0]))
    fixture = prepare(source, entry, observed, [], commands, seed=1)
    state = State(entry)
    settings = state.read(0x8035a768)
    for slot, character in enumerate((args.character, 4, 0, 0, 0, 0, 0, 0)):
        state.write(settings + 0xe9d + slot, "B", character)
    for slot in range(4):
        state.write(settings + 0xea5 + slot, "B", 0)
    for character in (args.character, 4):
        member = settings + 0x2b8 + (character - 1) * 0x118
        state.write(member + 0x10, "B", 30 if character == args.character else 20)
        state.write(member + 0x56, "B", 0)
        for offset in (0x12, 0x26, 0x36):
            state.write(member + offset, "H", 2000)
        for offset in (0x14, 0x28, 0x38):
            state.write(member + offset, "H", 100)
    if args.character == 2:
        state.write(state.read(0x8035a578) + 0x4c, "i", 1)
    state.save(entry)
    movie = out / "neutral.dtm"
    neutral_movie(source.path, movie, observed["movie"]["input_count"], polls=24000)
    shutil.copyfile(movie, str(entry) + ".dtm")
    write(out / "entry.json", dict(fixture=fixture, changes=state.changes, game_code_modified=False))
    pointer, at = "8035a86c 10 30 540", 0x1cea0
    locations = watch_locations(dict(pool=1, bss_index=6, actors=[dict(slot=i) for i in range(8)]))
    for offset, name in ((0x1174, "native"), (0x104, "selected"), (0x117c, "combo_word"), (0x1180, "style_word"), (0x10a8, "gauge_word")):
        locations[f"{pointer} {at + offset:x}"] = "owner_" + name
    locations[f"{pointer} {at + 8:x} 0"] = "owner_strategy"
    locations[f"{pointer} {at + 8:x} 1c"] = "owner_level_word"
    locations[f"{pointer} {at:x} 10"] = "owner_saved_level_word"
    locations[f"{pointer} 159c0"] = "level_difference_word"
    locations[f"{pointer} {at + 4 * 0x1ba0 + 4:x} 5c"] = "target_traits_word"
    for offset in (0, 4):
        locations[f"{pointer} {at + 4 * 0x1ba0 + 4:x} {offset:x}"] = f"target_affinities_{offset}"
    locations["8035a86c 10 20 1b80"] = "colette_melee_gap_word"
    locations[f"8035a86c 10 20 {0x10dc + 20 + args.character - 1:x}"] = "default_position_word"
    for offset, name in ((0x70, "learned"), (0x78, "enabled")):
        for word in (0, 4):
            locations[f"{pointer} {at:x} {offset + word:x}"] = f"owner_{name}_{word}"
    report = dict(complete=False, cases=[], character=args.character, source_sha256=source.sha256,
        techniques_sha256=digest(catalogue), scope="Character-specific first autonomous admission and combo stage; not exact RNG, timing or audiovisual parity.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, entry, out / "worker", fixture["marker_address"], locations,
                               disc_sha256=DISC_SHA256, capture_images=False)
        worker.warmup(entry, out / "warmup", fixture["case_marker"], 240)
        initialized = out / "initialized.s01"
        worker.snapshot(initialized)
        base = State(initialized)
        battle = inspect(base, battle=True)["battle"]
        actors = {a["slot"]: a for a in battle["actors"]}
        if set(actors) != {0, 1, 4} or actors[0]["kind"] != args.character:
            raise ValueError("requested manual party did not initialize")
        for actor in actors.values():
            at = actor["address"]
            base.write(at + 0x107c, "B", base.read(at + 0x107c, "B") & ~0x38)
        settling = out / "settling.s01"
        base.save(settling)
        shutil.copyfile(str(initialized) + ".dtm", str(settling) + ".dtm")
        write(out / "settling.json", dict(changes=base.changes, game_code_modified=False))
        worker.observe_until(settling, out / "settling", fixture["case_marker"], 900,
            lambda rows: all(rows[-1][f"battle_actor_{slot}_action"] >> 24 == 3 for slot in actors))
        ready = out / "ready.s01"
        worker.snapshot(ready)
        base = State(ready)
        battle = inspect(base, battle=True)["battle"]
        if any(a["action"][0] != 3 for a in battle["actors"]):
            raise ValueError("requested manual party did not initialize and settle")
        for case in cases(args.character, args.suite):
            target = out / case["name"]
            target.mkdir()
            initial = {**case, "skill": 5, "position": case.get("initial_position", case["position"])} if case.get("staged") else case
            state, mask = arrange(base, battle, initial, learning)
            if "techniques" in case:
                mask = sum(1 << learning.index(technique) for technique in case["techniques"])
                state.write(state.read(actors[0]["address"]) + 0x78, "Q", mask)
            state.write(actors[0]["vitals_address"] + 0x24, "I", case.get("hp", 2000))
            if case.get("flying"):
                profile = state.read(actors[4]["address"] + 4)
                state.write(profile + 0x5c, "I", state.read(profile + 0x5c) | 1)
            if "target_strategy_flags" in case:
                profile = state.read(actors[4]["address"] + 4)
                state.write(profile + 0x5c, "I", state.read(profile + 0x5c) & ~0x10020 | case["target_strategy_flags"])
                state.write(profile + 2, "B", 0)
                state.write(profile + 4, "B", case["target_fire_affinity"])
            if "melee_gap" in case:
                state.write(battle["sections"][4]["address"] + 0x1b80, "f", case["melee_gap"])
            if "default_position" in case:
                state.write(battle["sections"][4]["address"] + 0x10dc + 20 + args.character - 1,
                            "B", case["default_position"])
            marker = int.from_bytes(sha256(json.dumps(case, sort_keys=True).encode()).digest()[:4])
            state.write(fixture["marker_address"], "I", marker)
            if case.get("overlimit"):
                state.write(actors[0]["address"] + 0x107c, "B", state.read(actors[0]["address"] + 0x107c, "B") & ~0x38)
                state.write(actors[0]["address"] + 0x10a8, "H", 1000)
            initial_state = target / "input.s01"
            state.save(initial_state)
            shutil.copyfile(str(ready) + ".dtm", str(initial_state) + ".dtm")
            write(target / "fixture.json", dict(case=case, changes=state.changes, game_code_modified=False))
            if case.get("overlimit"):
                worker.observe_until(initial_state, target / "charge", marker, 120,
                    lambda rows: len(rows) >= 60 and rows[-1]["battle_actor_0_flags"] >> 16 & 7 == 4)
                worker.snapshot(target / "charged.s01")
                state = State(target / "charged.s01")
                state.write(actors[0]["address"] + 0x107c, "B", state.read(actors[0]["address"] + 0x107c, "B") & ~0x38 | 0x10)
                initial_state = target / "auto.s01"
                state.save(initial_state)
                shutil.copyfile(str(target / "charged.s01") + ".dtm", str(initial_state) + ".dtm")
                write(target / "auto.json", dict(changes=state.changes, game_code_modified=False))
            if case.get("staged"):
                worker.observe_until(initial_state, target / "normal-prelude", marker, 180,
                    lambda rows: bool(admitted(rows)), stepped=True)
                worker.snapshot(target / "normal.s01")
                state = State(target / "normal.s01")
                for offset, value in enumerate((1, case["skill"], case["position"])):
                    state.write(actors[0]["vitals_address"] + offset, "B", value)
                marker ^= 0xa5a5a5a5
                state.write(fixture["marker_address"], "I", marker)
                initial_state = target / "switch.s01"
                state.save(initial_state)
                shutil.copyfile(str(target / "normal.s01") + ".dtm", str(initial_state) + ".dtm")
                write(target / "switch.json", dict(changes=state.changes, game_code_modified=False))
            rows = worker.observe(initial_state, target / "observations", marker,
                                  case.get("observation_vis", 420 if case.get("technique") in (237, 66) else 180))
            worker.snapshot(target / "terminal.s01")
            result = dict(case=case, **review(rows, case, mask))
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = bool(report["cases"]) and all(c["passed"] for c in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("character observations differ; inspect report.json")
    write(out / "outcome-acceptance.json", dict(complete=True,
        input_report_sha256=digest(out / "report.json"), cases=report["cases"], scope=report["scope"]))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True, help="Loaded field checkpoint and .dtm companion")
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--character", type=int, required=True, choices=(1, 2, 3, 5, 6, 9))
    parser.add_argument("--suite", choices=("characters", "openings", "mixed", "elemental"), default="characters")
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
