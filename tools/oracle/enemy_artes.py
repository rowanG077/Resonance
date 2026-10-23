#!/usr/bin/env python3
"""Observe authored enemy action bindings with one resident Dolphin per enemy.

Preparation changes selection data and fixture resources/positions. It preserves
the authored action, animation, contact and command streams. A recording is
evidence to review, never an automatic claim of gameplay or visual parity.
"""
import argparse
import gc
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import struct

from battle_fixture import (BattleFixture, DTM_BUTTONS, PARTY_SETTINGS_POINTER, enemy_package,
                            neutral_movie, prepare_entry)
from battle_identity import EnemyCatalogue
from battle_state import projectile_locations, watch_locations
from enemy_catalogue import ACTION_BYTES, section
from state import State, digest, inspect

MANIFEST = Path(__file__).with_name("cases") / "enemy-artes.json"
BATTLE_POINTER = "8035a86c 10 30 540"
ACTORS, ACTOR_BYTES = 0x1CEA0, 0x1BA0
PRIORITY = 0x80000
FOLLOWUP_ONLY, FOLLOWUP_HIT, DO_NOT_REPEAT = 0x200000, 0x400000, 0x40000000
ANIMATION_POINTER, COMMAND_POINTER, NATIVE_WORD = 0x104C, 0x1050, 0x1174
ACTION_TABLE_POINTER, SELECTED_ROW_POINTER = 0x50, 0x104
POSITION_X, POSITION_Z, CENTER_X, CENTER_Z = 0x18C0, 0x18C8, 0x195C, 0x1964
HP_OFFSET = 0x24
MAX_TP_OFFSET, TP_OFFSET, ROW_TP_COST = 0x28, 0x2A, 52
BATTLE_RANK_OFFSET = 0x19F
ENTRY_ACTION = 13
FOLLOWUP_CHANCE = 50


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def pinned(record):
    path = Path(record["path"])
    if digest(path) != record["sha256"]:
        raise ValueError(f"pinned input changed: {path}")
    return path


def output_path(path):
    path = Path(path).resolve()
    if not path.is_relative_to(Path("local/battle-capture-work").resolve()):
        raise ValueError("attempt output must be under local/battle-capture-work")
    return path


def locations():
    result = watch_locations({"pool": 1, "bss_index": 6,
                              "actors": [{"slot": slot} for slot in range(12)]})
    for slot in range(4, 12):
        base = ACTORS + slot * ACTOR_BYTES
        for offset, name in [(ANIMATION_POINTER, "animation_pointer"),
                             (COMMAND_POINTER, "command_pointer"), (NATIVE_WORD, "native_word"),
                             (ACTION_TABLE_POINTER, "action_table_pointer"),
                             (SELECTED_ROW_POINTER, "selected_row_pointer")]:
            result[f"{BATTLE_POINTER} {base + offset:x}"] = f"enemy_{slot}_{name}"
    result.update(projectile_locations(BATTLE_POINTER))
    return result


def select(manifest, cases, enemy):
    requested = set(cases or [])
    result = [case for case in manifest["cases"] if
              case["id"] in requested or (enemy is not None and case["enemy"] == enemy)]
    if requested - {case["id"] for case in result}:
        raise ValueError("unknown requested enemy case")
    if not result or len({case["enemy"] for case in result}) != 1:
        raise ValueError("prepare one enemy, using --enemy or one or more --case options")
    return result


def distinct_ground_targets(rows):
    """Supply every distinct recipient needed by an authored ground release."""
    minimum = 1
    for row in rows:
        execution = row.get("execution")
        if not isinstance(execution, dict):
            continue
        attack = execution.get("attack", execution.get("physical", {}).get("attack", {}))
        for _, event in attack.get("events", []):
            if isinstance(event, dict) and "ground_projectiles" in event:
                minimum = max(minimum, len(event["ground_projectiles"]["definitions"]))
    return minimum


def prepare(args, manifest):
    cases = select(manifest, args.case, args.enemy)
    inputs = manifest["inputs"]
    source_path = pinned(inputs["state"])
    pinned(inputs["movie"])
    if digest(Path(str(source_path) + ".dtm")) != inputs["movie"]["sha256"]:
        raise ValueError("checkpoint companion differs from the pinned movie")
    formation_path = pinned(inputs["formations"])
    enemy = next(row for row in manifest["enemies"] if row["enemy"] == cases[0]["enemy"])
    if (requested_formation := getattr(args, "formation", None)) is not None:
        candidates = [fixture for fixture in enemy["formation_candidates"]
                      if fixture["formation"] == requested_formation and fixture["arena_cooked"]]
        if len(candidates) != 1:
            raise ValueError("fixture formation must be a cooked encounter containing the selected enemy")
        encounter = {key: candidates[0][key] for key in ("formation", "arena", "arena_basis")}
        cases = [dict(case, **encounter) for case in cases]
    raw_path = pinned(enemy["package"])
    raw = raw_path.read_bytes()
    for case in cases:
        data = raw[case["row_offset"]:case["row_offset"] + ACTION_BYTES]
        if hashlib.sha256(data).hexdigest() != case["row_sha256"]:
            raise ValueError(f"authored row differs from the manifest: {case['id']}")
    formation, arena = cases[0]["formation"], cases[0]["arena"]
    if any((case["formation"], case["arena"]) != (formation, arena) for case in cases):
        raise ValueError("resident cases must share one encounter")
    intervals = [case["requirements"]["difficulty"] for case in cases]
    if any(not 0 <= interval["start"] <= interval["end"] <= 2 for interval in intervals):
        raise ValueError("authored difficulty interval is outside the supported battle ranks")
    minimum, maximum = max(interval["start"] for interval in intervals), min(
        interval["end"] for interval in intervals)
    if minimum > maximum:
        raise ValueError("resident cases require different encounter difficulties")
    opponent_bounds = [predicate["opponent_count"] for case in cases
                       for predicate in case["requirements"]["predicates"]
                       if isinstance(predicate, dict) and "opponent_count" in predicate]
    party_minimum = max([1] + [bounds["minimum"] for bounds in opponent_bounds
                             if bounds["minimum"] is not None])
    party_maximum = min([4] + [bounds["maximum"] for bounds in opponent_bounds
                             if bounds["maximum"] is not None])
    action_rows = read(pinned(enemy["definition"]))["actions"]["rows"]
    ground_target_minimum = distinct_ground_targets(action_rows[case["action_row"]] for case in cases)
    party_minimum = max(party_minimum, ground_target_minimum)
    fixture_party_minimum = getattr(args, "party_minimum", None)
    if fixture_party_minimum is None:
        fixture_party_minimum = 1
    if type(fixture_party_minimum) is not int or not 1 <= fixture_party_minimum <= 4:
        raise ValueError("fixture party minimum must be between one and four")
    party_minimum = max(party_minimum, fixture_party_minimum)
    formations = read(formation_path)["records"]
    formation_record = formations[formation]
    station_admission_minimum = 1
    if "three_stations" in (enemy["policy"].get("behavior") or {}):
        # Station movement can put the owner behind its neutral companions.
        # A larger ordinary party admits the encounter's first three enemies.
        station_admission_minimum = max(1, min(3, len(formation_record["actors"])) - 1)
        party_minimum = max(party_minimum, station_admission_minimum)
    if not 1 <= party_minimum <= party_maximum <= 4:
        raise ValueError("resident cases have no supported living party-count interval")
    party = list(range(1, party_minimum + 1))
    roster = sorted({formation_record["resources"][actor["resource"]]["enemy"]
                     for actor in formation_record["actors"]})
    packages = [row for row in manifest["enemies"] if row["enemy"] in roster]
    for record in packages:
        pinned(record["package"])
        pinned(record["monster"])
    output = output_path(args.output)
    output.mkdir(parents=True, exist_ok=False)
    source = State(source_path)
    session = source.read(PARTY_SETTINGS_POINTER)
    if not 0x80000000 <= session <= 0x81800000 - BATTLE_RANK_OFFSET - 1:
        raise ValueError("entry session pointer is outside RAM")
    previous_rank = source.read(session + BATTLE_RANK_OFFSET, "B")
    if previous_rank > 2:
        raise ValueError("entry battle rank is invalid")
    selected_rank = max(minimum, min(previous_rank, maximum))
    if (requested_rank := getattr(args, "difficulty", None)) is not None:
        if not minimum <= requested_rank <= maximum:
            raise ValueError("fixture difficulty is outside the authored action interval")
        selected_rank = requested_rank
    if selected_rank != previous_rank:
        source.write(session + BATTLE_RANK_OFFSET, "B", selected_rank)
    difficulty_fixture = {"authored_interval": {"start": minimum, "end": maximum},
                          "previous": previous_rank, "selected": selected_rank,
                          "changes": source.changes.copy(),
                          "scope": "Session difficulty selected before encounter initialization; "
                                   "ordinary initialization derives difficulty-scaled vitals and selection rules."}
    observation = inspect(source)
    defaults = manifest["fixture_defaults"]
    capture_vis = args.capture_vis or defaults["capture_vis"]
    movie_polls = 4 * (capture_vis + defaults["warmup_vis"] +
                       defaults["entry_gate_attempts"] * defaults["entry_gate_vis"]) + 1000
    fixture = prepare_entry(source, output / "entry.s01", observation, formation, arena,
                            hp=defaults["party_hp"], party=party)
    neutral_movie(source_path, output / "input.dtm", observation["movie"]["input_count"],
                  DTM_BUTTONS["x"] if args.guard else 0, polls=movie_polls)
    shutil.copyfile(output / "input.dtm", output / "entry.s01.dtm")
    record = {"cases": cases, "enemy": enemy, "packages": packages,
              "formation": formation, "arena": arena, "fixture": fixture,
              "difficulty_fixture": difficulty_fixture,
              "party_count_fixture": {"party": party, "authored_bounds": opponent_bounds,
                  "fixture_minimum": fixture_party_minimum,
                  "station_admission_minimum": station_admission_minimum,
                  "ground_target_minimum": ground_target_minimum,
                  "scope": "Party joined before encounter initialization; authored selection requirements unchanged."},
              "defaults": defaults, "inputs": inputs,
              "manifest_sha256": digest(args.manifest),
              "entry_sha256": digest(output / "entry.s01"),
              "movie_sha256": digest(output / "input.dtm"),
              "capture_vis": capture_vis, "movie_polls": movie_polls,
              "images": args.images, "retain_states": args.retain_states, "guard": args.guard,
              "enemy_x": args.enemy_x, "enemy_hp_percent": args.enemy_hp_percent,
              "expected_slots": list(range(len(party))) + list(range(4, 4 + len(formation_record["actors"]))),
              "checkpoint": "initialized entry before the first combat choice",
              "selection": "controlled row weight; competing priorities suppressed; selected follow-up/repeat gates cleared",
              "natural_admission_tested": False}
    write(output / "fixture.json", record)
    write(output / "locations.json", locations())
    return output


def ready(worker, output, record):
    marker = record["fixture"]["case_marker"]
    defaults = record["defaults"]
    path = output / "initialized.s01"
    # Body emitters start during entry. Stop at their first initialized visit,
    # so replay never needs to invent particles born before its ready snapshot.
    worker.warmup(output / "entry.s01", output / "warmup", marker,
                  max(1, defaults["warmup_vis"] // 2))
    if worker.watcher.latest.get("battle_presentation_tick_word", 0) > 1:
        worker.load(output / "entry.s01")
        worker.step()
    slots = record["expected_slots"]
    readiness = []
    limit = defaults["warmup_vis"] + defaults["entry_gate_attempts"] * defaults["entry_gate_vis"]
    for attempt in range(limit + 1):
        row = worker.watcher.latest
        actions = [row.get(f"battle_actor_{slot}_action", 0) >> 24 for slot in slots]
        entry_ready = bool(row.get("battle_camera_address")) and actions == [ENTRY_ACTION] * len(slots)
        readiness.append({"attempt": attempt, "camera": row.get("battle_camera_address"),
                          "actions": actions, "combat_tick": row.get("battle_combat_tick_word"),
                          "presentation_tick": row.get("battle_presentation_tick_word"),
                          "ready": entry_ready})
        if row.get("battle_combat_tick_word", 0) != 0:
            raise ValueError("initialized checkpoint passed the first combat choice; retained for inspection")
        if entry_ready:
            worker.snapshot(path)
            write(output / "entry-readiness.json", readiness)
            break
        if attempt == limit:
            write(output / "entry-readiness.json", readiness)
            raise ValueError("encounter did not initialize before combat within the bounded entry gate")
        worker.step()
    return initialized_state(path, output, record)


def initialized_state(path, output, record, *, idle=False):
    """Validate a newly captured or retained preselection state identically."""
    slots = record["expected_slots"]
    state = State(path)
    if difficulty := record.get("difficulty_fixture"):
        session = state.read(PARTY_SETTINGS_POINTER)
        if not 0x80000000 <= session <= 0x81800000 - BATTLE_RANK_OFFSET - 1:
            raise ValueError("initialized session pointer is outside RAM")
        if state.read(session + BATTLE_RANK_OFFSET, "B") != difficulty["selected"]:
            raise ValueError("retained encounter difficulty differs from the requested fixture; "
                             "initialize a fresh encounter at the eligible difficulty")
    observation = inspect(state, battle=True)["battle"]
    if (observation.get("mode"), observation.get("formation"), observation.get("arena")) != (
            9, record["formation"], record["arena"]):
        raise ValueError("initialized encounter does not match the requested formation/arena")
    if sorted(actor["slot"] for actor in observation["actors"]) != slots:
        raise ValueError("retained encounter roster differs from the requested fixture")
    if party_fixture := record.get("party_count_fixture"):
        party = party_fixture["party"]
        living = [actor for actor in observation["actors"] if actor["side"] == 0 and actor["hp"] > 0]
        if observation["party_order"] != party + [0] * (4 - len(party)) or len(living) != len(party):
            raise ValueError("initialized encounter differs from its declared living party count")
    if idle:
        if (observation["phase"] != 3 or observation["pause_flags"] or
                observation["menu_flags"] & 0x80000000 or
                any(actor["action"][0] != 3 for actor in observation["actors"])):
            raise ValueError("retained encounter is not ordinary idle combat")
        pool = observation["pool"]
        for offset in (0x75c40, 0x75ec8, 0x763d8, 0x76660):
            sentinel = pool + offset
            if state.read(sentinel) != sentinel or state.read(sentinel + 4) != sentinel:
                raise ValueError("retained idle encounter still has live effects")
        if (any(observation["stored_transition"]["occupied"]) or
                observation["stored_transition"]["remaining"] or
                any(scene["resource"] for scene in observation["stored_scenes"])):
            raise ValueError("retained idle encounter still has a stored action")
    elif (observation["combat_tick"] != 0 or
            any(actor["action"][0] != ENTRY_ACTION for actor in observation["actors"])):
        raise ValueError("retained entry snapshot is not the observed preselection checkpoint")
    catalogue = EnemyCatalogue(record["inputs"]["packages"], record["inputs"]["assets"])
    identities = [catalogue.identify(state, actor) for actor in observation["actors"] if actor["side"] == 1]
    matches = [item for item in identities if item["enemy"] == record["enemy"]["enemy"]]
    if not matches or (idle and len(matches) != 1):
        raise ValueError("requested actor has no unambiguous authored identity in initialized data")
    actor = next(item for item in observation["actors"] if item["slot"] == matches[0]["slot"])
    packages = {}
    for item in record["packages"]:
        raw = pinned(item["package"]).read_bytes()
        address, proof = enemy_package(state, raw)
        packages[str(item["enemy"])] = {"address": address, "proof": proof}
    record = {"battle": observation, "actor": actor, "identities": identities,
              "packages": packages, "state_sha256": digest(path),
              "checkpoint": "observed idle combat" if idle else "initialized entry before the first combat choice",
              "settled_idle": idle,
              "movie_sha256": digest(Path(str(path) + ".dtm"))}
    write(output / "ready.json", record)
    return state, record


def prepare_row(base, output, record, initialized, case):
    idle = initialized.get("settled_idle", False)
    if ((idle and any(actor["action"][0] != 3 for actor in initialized["battle"]["actors"])) or
            (not idle and (initialized["battle"]["combat_tick"] != 0 or
             any(actor["action"][0] != ENTRY_ACTION for actor in initialized["battle"]["actors"])))):
        raise ValueError("selection fixture requires a verified entry or idle checkpoint")
    state = base.fork()
    target = initialized["actor"]
    for item in record["packages"]:
        raw = pinned(item["package"]).read_bytes()
        start, rows = section(raw, 10)
        package = initialized["packages"][str(item["enemy"])]["address"]
        for index in range(len(rows) // ACTION_BYTES):
            at = package + start + index * ACTION_BYTES
            chosen = item["enemy"] == case["enemy"] and index == case["action_row"]
            state.write(at, "B", 100 if chosen else 0)
            flags = state.read(at + 8)
            cleared = (FOLLOWUP_ONLY | FOLLOWUP_HIT | DO_NOT_REPEAT) if chosen else PRIORITY
            state.write(at + 8, "I", flags & ~cleared)
            if chosen:
                # Compare one selected row; automatic chains are a separate action.
                state.write(at + FOLLOWUP_CHANCE, "b", 0)
    for actor in initialized["battle"]["actors"]:
        if idle or actor["slot"] not in (0, target["slot"]):
            continue
        at = actor["address"]
        x = 0.0 if actor["side"] == 0 else record["enemy_x"]
        state.write(at + POSITION_X, "f", x)
        state.write(at + POSITION_Z, "f", 0.0)
        state.write(at + CENTER_X, "f", actor["center"][0] + x - actor["position"][0])
        state.write(at + CENTER_Z, "f", actor["center"][2] - actor["position"][2])
        if actor["slot"] == target["slot"]:
            state.write(actor["vitals_address"] + HP_OFFSET, "I",
                        max(1, actor["max_hp"] * record["enemy_hp_percent"] // 100))
    package = initialized["packages"][str(case["enemy"])]["address"]
    raw = pinned(record["enemy"]["package"]).read_bytes()
    required_tp = raw[case["row_offset"] + ROW_TP_COST]
    tp_budget = None
    if target["tp"] < required_tp:
        # Some authored rows cost TP even though every encounter variant has
        # none. Fund one ordinary admission without changing its cost.
        state.write(target["vitals_address"] + MAX_TP_OFFSET, "H", max(target["max_tp"], required_tp))
        state.write(target["vitals_address"] + TP_OFFSET, "H", required_tp)
        tp_budget = dict(before=target["tp"], maximum_before=target["max_tp"], required=required_tp,
                         scope="Resource budget for one authored action; ordinary admission and consumption retained.")
    expected = {"animation_pointer": package + struct.unpack_from(">H", raw, 18)[0] + case["animation_index"] * 12,
                "command_pointer": package + struct.unpack_from(">H", raw, 14)[0] + case["command_index"] * 2,
                "selected_row_pointer": package + case["row_offset"]}
    start, rows = section(raw, 10)
    aliases = []
    for index in range(len(rows) // ACTION_BYTES):
        row = rows[index * ACTION_BYTES:(index + 1) * ACTION_BYTES]
        native = struct.unpack_from(">H", row, 64)[0] or None
        same_spell = case["kind"] == "common_spell" and native == case["native"]
        same_tracks = case["kind"] != "common_spell" and (native is None or native < 200) and (
            struct.unpack_from(">H", row, 24)[0] == case["animation_index"] and
            struct.unpack_from(">H", row, 26)[0] == case["command_index"])
        if same_spell or same_tracks:
            aliases.append(index)
    path = output / "ready.s01"
    state.save(path)
    neutral_movie(record["initialized_state"], Path(str(path) + ".dtm"),
                  inspect(base)["movie"]["input_count"], DTM_BUTTONS["x"] if record["guard"] else 0,
                  polls=record.get("movie_polls", 12000))
    write(output / "fixture.json", {"case": case, "actor": target,
          "expected": expected, "execution_aliases": aliases, "changes": state.changes,
          "tp_budget": tp_budget,
          "state_sha256": digest(path), "movie_sha256": digest(Path(str(path) + ".dtm")),
          "selection_scope": "one authored row; automatic followup chance disabled with recorded preimage",
          "natural_admission_tested": False})
    return path


def matching_metadata(fixture, row):
    case, slot = fixture["case"], fixture["actor"]["slot"]
    for key in ("selected_row_pointer",):
        if key in fixture["expected"] and row[f"enemy_{slot}_{key}"] != fixture["expected"][key]:
            return False
    if case["kind"] != "common_spell":
        return all(row[f"enemy_{slot}_{key}"] == value for key, value in fixture["expected"].items())
    return row[f"enemy_{slot}_native_word"] & 65535 == case["native"]


def owns_projectile(fixture, row):
    return any(row[f"projectile_{index}_address"] and
               row[f"projectile_{index}_28"] == fixture["actor"]["address"] for index in range(16))


def completion_signal(fixture):
    """Stop collecting after observed execution/recovery, without certifying it."""
    seen, executing, recovered, clear = 0, False, False, 0
    slot = fixture["actor"]["slot"]

    def done(rows):
        nonlocal seen, executing, recovered, clear
        for row in rows[seen:]:
            action = row[f"battle_actor_{slot}_action"] >> 24
            executing |= action == 5 and matching_metadata(fixture, row)
            recovered |= executing and action == 3
            clear = 0 if owns_projectile(fixture, row) else clear + 1
        seen = len(rows)
        return recovered and clear >= 10

    return done


def review(output):
    fixture = read(output / "fixture.json")
    case, actor = fixture["case"], fixture["actor"]
    with gzip.open(output / "observations.json.gz", "rt") as stream:
        rows = json.load(stream)
    slot = actor["slot"]
    matching = [matching_metadata(fixture, row) for row in rows]
    active = [match and row[f"battle_actor_{slot}_action"] >> 24 == 5 for match, row in zip(matching, rows)]
    # An inherited action at sample zero cannot prove the changed selection ran.
    selected = [index for index in range(1, len(rows)) if matching[index] and not matching[index - 1]]
    starts = [index for index in range(1, len(rows)) if active[index] and not active[index - 1]]
    first = starts[0] if starts else None
    idle = next((index for index in range((first or 0) + 1, len(rows))
                 if first is not None and rows[index][f"battle_actor_{slot}_action"] >> 24 == 3), None)
    hp_changes = [{"sample": index, "before": rows[index - 1]["battle_actor_0_hp"],
                   "after": rows[index]["battle_actor_0_hp"]} for index in range(1, len(rows))
                  if rows[index]["battle_actor_0_hp"] != rows[index - 1]["battle_actor_0_hp"]]
    owned = [owns_projectile(fixture, row) for row in rows]
    result = {"case": case["id"], "samples": len(rows), "selected_metadata_onsets": selected,
              "execution_metadata_onsets": starts,
              "identity_already_present_at_start": matching[0],
              "exact_row_pointer_recorded": "selected_row_pointer" in fixture["expected"],
              "unique_row_metadata_onset": bool(starts) and (
                  "selected_row_pointer" in fixture["expected"] or len(fixture["execution_aliases"]) == 1),
              "row_execution_verified": False,
              "execution_aliases": fixture["execution_aliases"], "idle_after_onset_sample": idle,
              "party_hp_changes_unattributed": hp_changes,
              "owned_projectiles_observed": any(owned),
              "no_owned_projectiles_in_final_ten_samples": not any(owned[-10:]) if len(rows) >= 10 else None,
              "observations_sha256": digest(output / "observations.json.gz"),
              "natural_admission_tested": False, "semantic_review": "required",
              "native_comparison": "not_performed", "fidelity_verified": False,
              "limits": ["Metadata onsets require active action/pose/contact review before they prove execution.",
                         "Shared tracks or native IDs do not independently identify an aliased action row.",
                         "HP changes need contact attribution; idle alone does not prove complete effect or model-part cleanup.",
                         "Projectile observations cover the first sixteen linked records."]}
    write(output / "review.json", result)
    return result


def run(output, disc):
    output = output_path(output)
    record = read(output / "fixture.json")
    if ("expected_slots" not in record or "entry_gate_vis" not in record["defaults"]):
        raise ValueError("prepare a fresh entry-preselection fixture; this prepared input uses the older active-combat gate")
    for filename, key in [("entry.s01", "entry_sha256"), ("input.dtm", "movie_sha256")]:
        if digest(output / filename) != record[key]:
            raise ValueError(f"prepared input changed: {filename}")
    if (output / "outcome.json").exists():
        raise ValueError("attempt already ran; prepare a fresh output")
    worker = None
    outcome = {"capture_complete": False, "cases": [], "semantic_review": "required"}
    try:
        worker = BattleFixture(str(disc), output / "input.dtm", output / "entry.s01", output / "worker",
                               record["fixture"]["marker_address"], read(output / "locations.json"),
                               disc_sha256=record["inputs"]["disc_sha256"], capture_images=record["images"])
        base, initialized = ready(worker, output, record)
        record["initialized_state"] = str(output / "initialized.s01")
        for case in record["cases"]:
            destination = output / case["id"].replace(":", "-")
            destination.mkdir()
            case_result = {"case": case["id"], "capture_complete": False}
            try:
                path = prepare_row(base, destination, record, initialized, case)
                gc.collect()
                frames = destination / "frames"
                if record["images"]:
                    worker.sequence(path, frames, record["fixture"]["case_marker"], record["capture_vis"])
                else:
                    worker.observe_until(path, frames, record["fixture"]["case_marker"],
                                         record["capture_vis"], completion_signal(read(destination / "fixture.json")))
                with gzip.open(destination / "observations.json.gz", "wb") as stream:
                    stream.write((frames / "observations.json").read_bytes())
                shutil.copyfile(frames / "capture.json", destination / "capture.json")
                result = review(destination)
                continuation_required = (not result["execution_metadata_onsets"] or
                                         result["idle_after_onset_sample"] is None or
                                         result["no_owned_projectiles_in_final_ten_samples"] is not True)
                retain_states = record.get("retain_states", False) or continuation_required
                if retain_states:
                    worker.snapshot(destination / "terminal.s01")
                images = {}
                if record["images"]:
                    samples = {0, record["capture_vis"] - 1}
                    for start in result["execution_metadata_onsets"][:2]:
                        samples.update(min(start + delta, record["capture_vis"] - 1) for delta in (0, 24, 60))
                    for sample in sorted(samples):
                        image = destination / f"frame-{sample:04}.png"
                        shutil.copyfile(frames / image.name, image)
                        images[image.name] = digest(image)
                case_result.update(capture_complete=True, review=result, retained_images=images,
                                   continuation_required=continuation_required)
                if retain_states:
                    case_result["terminal_sha256"] = digest(destination / "terminal.s01")
                    case_result["terminal_movie_sha256"] = digest(destination / "terminal.s01.dtm")
                else:
                    path.unlink()
                    Path(str(path) + ".dtm").unlink()
                shutil.rmtree(frames)
            except Exception as error:
                case_result["error"] = str(error)
                # A failed image/worker handshake cannot safely service the next case.
                outcome["cases"].append(case_result)
                write(destination / "outcome.json", case_result)
                raise
            write(destination / "outcome.json", case_result)
            outcome["cases"].append(case_result)
        outcome["capture_complete"] = True
    except Exception as error:
        outcome["error"] = str(error)
    finally:
        if worker is not None:
            try:
                worker.close(complete=outcome["capture_complete"])
                outcome["worker_closed"] = True
            except Exception as error:
                outcome["worker_closed"] = False
                outcome["close_error"] = str(error)
        write(output / "outcome.json", outcome)
    return outcome


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    commands = parser.add_subparsers(dest="command", required=True)
    listing = commands.add_parser("list")
    listing.add_argument("--enemy", type=int)
    preparing = commands.add_parser("prepare")
    preparing.add_argument("--case", action="append", help="enemyNNN:rowNN; repeat for one resident encounter")
    preparing.add_argument("--enemy", type=int)
    preparing.add_argument("--output", type=Path, required=True)
    preparing.add_argument("--capture-vis", type=int)
    preparing.add_argument("--enemy-x", type=float, default=90.0)
    preparing.add_argument("--enemy-hp-percent", type=int, choices=range(1, 101), default=25)
    preparing.add_argument("--guard", action="store_true")
    preparing.add_argument("--images", action="store_true", help="retain a small registered image selection; defaults to numeric only")
    preparing.add_argument("--retain-states", action="store_true", help="keep per-case ready and terminal snapshots for part/pose analysis")
    running = commands.add_parser("run")
    running.add_argument("--prepared", type=Path, required=True)
    running.add_argument("--disc", type=Path, required=True)
    reviewing = commands.add_parser("review")
    reviewing.add_argument("--output", type=Path, required=True, help="one recorded case directory")
    args = parser.parse_args()
    if args.command == "list":
        for case in read(args.manifest)["cases"]:
            if args.enemy is None or case["enemy"] == args.enemy:
                print(json.dumps(case))
    elif args.command == "prepare":
        if args.capture_vis is not None and not 1 <= args.capture_vis <= 12000:
            raise ValueError("capture must contain between one and 12000 VIs")
        print(prepare(args, read(args.manifest)))
    elif args.command == "run":
        result = run(args.prepared, args.disc)
        print(json.dumps(result))
        return 0 if result["capture_complete"] and result.get("worker_closed") else 1
    else:
        print(json.dumps(review(output_path(args.output))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
