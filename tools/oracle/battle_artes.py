"""Prepare, capture and review one catalogue-bound ordinary battle arte.

Fixtures use persistent party data and ordinary controller input. Special entry
conditions remain explicit manifest gaps until a measured fixture supports them.
"""
import argparse
import gc
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import struct

from battle_fixture import BattleFixture, DTM_BUTTONS, neutral_movie
from battle_state import projectile_locations, watch_locations
from effect_fixture import prepare
from state import State, digest, inspect
import technique_content

MANIFEST = Path(__file__).with_name("cases") / "battle-artes.json"
SETTINGS = 0x8035A768
MEMBERS, MEMBER_BYTES = 0x2B8, 0x118
PARTY_ORDER, PARTY_CONTROLS = 0xE9D, 0xEA5
LEVEL, HP, TP, GAUGE, SHORTCUT, BALANCE = 0x10, 0x12, 0x14, 0x56, 0xD8, 0x10C
JOIN_CHARACTER, LEARN_TECHNIQUE, START_BATTLE = 0x24, 0x2E, 0xCD
SET_CHARACTER_COSTUME = 0x5A
BATTLE_POINTER = "8035a86c 10 30 540"
ACTORS, ACTOR_BYTES = 0x1CEA0, 0x1BA0
ATTACK_ACTION, RECOVERY_ACTION, HIT_REACTION_ACTION = 5, 8, 9


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def selected(manifest, key):
    character, arte = map(int, key.split(":"))
    return next(row for row in manifest["cases"]
                if (row["character"], row["arte"]) == (character, arte))


def member(state, character):
    return state.read(SETTINGS) + MEMBERS + (character - 1) * MEMBER_BYTES


def locations():
    result = watch_locations({"pool": 1, "bss_index": 6,
                              "actors": [{"slot": i} for i in range(8)]})
    result[f"{BATTLE_POINTER} {ACTORS + 0x1174:x}"] = "requested_native_word"
    for slot in range(4):
        actor = ACTORS + slot * ACTOR_BYTES
        for offset in (0x10, 0x14):
            result[f"{BATTLE_POINTER} {actor + 4:x} {offset:x}"] = f"condition_{slot}_{offset:x}"
    result.update(projectile_locations(BATTLE_POINTER))
    return result


def prepare_case(args, manifest):
    case = selected(manifest, args.case)
    if reason := case.get("pending_setup"):
        raise ValueError(manifest["pending_setups"][reason])
    inputs = manifest["inputs"]
    source_path = args.state or Path(inputs["state"]["path"])
    techniques_path = args.techniques or Path(inputs["techniques"]["path"])
    for path, key in [(source_path, "state"), (Path(str(source_path) + ".dtm"), "movie"),
                      (techniques_path, "techniques")]:
        if digest(path) != inputs[key]["sha256"]:
            raise ValueError(f"{key} does not match the pinned fixture input: {path}")
    output = args.output.resolve()
    if not output.is_relative_to(Path("local/battle-capture-work").resolve()):
        raise ValueError("attempt output must be under local/battle-capture-work")
    output.mkdir(parents=True, exist_ok=False)
    defaults = dict(manifest["fixture_defaults"])
    if "formation" in case:
        defaults["formation"] = case["formation"]
    defaults["capture_frames"] = manifest["capture_frames"][case["fixture"]]
    character, arte = case["character"], case["arte"]
    input_arte = case.get("input_arte", arte)
    document = read(techniques_path)
    catalogue = technique_content.prepare(document)
    catalogue["learning"] = technique_content.native_learning(document)
    definition = catalogue["definitions"][arte]
    if definition["tp_cost"] != case["tp_cost"]:
        raise ValueError("manifest TP cost differs from the authored catalogue")
    route = definition["learning"]["route"] or "technical"
    grants = [value for value in catalogue["learning"][character - 1]
              if 0 < catalogue["definitions"][value]["required_level"] <= defaults["level"]
              and catalogue["definitions"][value]["learning"]["route"] in (None, route)]
    grants = sorted(set(grants + [input_arte]
                        + [int(technique) for technique in case.get("technique_uses", {})]))
    companions = [value for value in (1, 2, 3, 4) if value != character]
    party = [character] if case["fixture"] == "martial" else [character] + companions[:3]
    commands = [(JOIN_CHARACTER, [value]) for value in party]
    commands += [(LEARN_TECHNIQUE, [character, value]) for value in grants]
    if "colette_state" in case:
        commands.append((SET_CHARACTER_COSTUME, [character, 3]))
    commands += [(START_BATTLE, [defaults["formation"], defaults["arena"], 0, -1,
                                 0, 0, 0, 0, 0, 0, 0, 0])]
    source = State(source_path)
    observation = inspect(source)
    entry = output / "entry.s01"
    fixture = prepare(source, entry, observation, [], commands, seed=1)
    state = State(entry)
    settings = state.read(SETTINGS)
    for slot in range(8):
        state.write(settings + PARTY_ORDER + slot, "B", party[slot] if slot < len(party) else 0)
    for slot in range(4):
        state.write(settings + PARTY_CONTROLS + slot, "B",
                    int(slot == 0 and case["fixture"] == "martial" and not case.get("input")))
    for index, value in enumerate(party):
        at = member(state, value)
        state.write(at + LEVEL, "B", defaults["level"])
        for offset in (0x26, 0x36):
            state.write(at + offset, "H", defaults["hp"])
        hp = 4000 if case["fixture"] == "recovery" else defaults["hp"]
        state.write(at + HP, "H", 0 if case.get("target_dead") and index == 1 else hp)
        for offset in (TP, 0x28, 0x38):
            state.write(at + offset, "H", defaults["tp"])
        # Saved OverLimit protects the measured martial sequence from interruption.
        state.write(at + GAUGE, "B", 100 if index == 0 and case["fixture"] in ("martial", "summon")
                    and case.get("input") != "guard_down" else 0)
    at = member(state, character)
    state.write(at + SHORTCUT, "H", case.get("input_arte", arte))
    for technique, count in case.get("technique_uses", {}).items():
        index = catalogue["learning"][character - 1].index(int(technique))
        state.write(at + 0x88 + index * 2, "H", count)
    state.write(at + BALANCE, "b", -100 if route == "technical" else 100)
    if "colette_state" in case:
        state.write(state.read(0x8035A578) + 0x4C, "i", case["colette_state"])
    state.save(entry)
    cursor = observation["movie"]["input_count"]
    press = cursor + defaults["press_after_polls"]
    movie_path = output / "input.dtm"
    neutral_movie(source_path, movie_path, cursor, polls=12000)
    movie = bytearray(movie_path.read_bytes())
    if case.get("input") == "aerial":
        # Measured Eagle/Triple Rage Kick input: jump32 polls, neutral8, then B.
        for poll in range(press - 40, press - 8):
            movie[256 + poll * 8 + 5] = 255
    if case.get("input") == "guard_down":
        for poll in range(press - 8, press + defaults["press_polls"]):
            struct.pack_into("<H", movie, 256 + poll * 8, 0x4000 | DTM_BUTTONS["x"])
        for poll in range(press, press + defaults["press_polls"]):
            movie[256 + poll * 8 + 5] = 0
    for poll in range(press, press + defaults["press_polls"]):
        if case.get("input") != "guard_down":
            struct.pack_into("<H", movie, 256 + poll * 8, 0x4000 | DTM_BUTTONS["b"])
    movie_path.write_bytes(movie)
    entry_movie = Path(str(entry) + ".dtm")
    entry_movie.unlink()
    entry_movie.hardlink_to(movie_path)
    record = {"case": case, "defaults": defaults, "party": party, "grants": grants,
              "source": str(source_path), "source_sha256": digest(source_path),
              "movie_sha256": digest(movie_path), "entry_sha256": digest(entry),
              "manifest_sha256": digest(args.manifest), "commands": commands,
              "fixture": fixture, "changes": state.changes, "press_poll": press,
              "learned_index": catalogue["learning"][character - 1].index(input_arte),
              "disc_sha256": inputs["disc_sha256"],
              "assistance": "saved OverLimit" if case["fixture"] in ("martial", "summon")
              and case.get("input") != "guard_down" else None,
              "scope": "controlled ordinary input; no natural learning claim"}
    write(output / "fixture.json", record)
    write(output / "locations.json", locations())
    return output


def ready_state(worker, output, record):
    if expected := record.get("warm_sha256"):
        if digest(output / "warm.s01") != expected:
            raise ValueError("retained warm checkpoint changed")
    else:
        fixture = record["fixture"]
        worker.warmup(output / "entry.s01", output / "warm", fixture["case_marker"], 300)
        worker.snapshot(output / "warm.s01")
    return prepare_ready(output, record)


def prepare_ready(output, record):
    """Validate and arrange an initialized checkpoint without advancing it."""
    state = State(output / "warm.s01")
    before = inspect(state, battle=True)
    movie = inspect(state)["movie"]
    expected_party = (record["party"] + [0] * 4)[:4]
    if before["battle"]["party_order"] != expected_party:
        raise ValueError("initialized party does not match the requested character fixture")
    if movie["input_count"] >= record["press_poll"] - 8:
        raise ValueError("warmup consumed the future input")
    case = record["case"]
    at = member(state, case["character"])
    if state.read(at + SHORTCUT, "H") != case.get("input_arte", case["arte"]):
        raise ValueError("requested shortcut changed during battle entry")
    bit = 1 << record["learned_index"]
    if not state.read(at + 0x70, "Q") & bit:
        raise ValueError("requested input arte was not learned")
    if case["character"] == 2 and not state.read(at + 0x80, "Q") & bit:
        raise ValueError("requested Colette arte is unavailable in the prepared story state")
    actors = before["battle"]["actors"]
    caster = next(actor for actor in actors if actor["slot"] == 0)
    for actor in actors:
        if actor["side"] == 1:
            for offset in (0x20, 0x24):
                state.write(actor["vitals_address"] + offset, "I", 30000)
        x = None
        if case["fixture"] == "recovery" or case.get("input") == "guard_down":
            if actor["side"] == 1:
                x = 5000.0
            elif case.get("input") == "guard_down":
                x = 0.0
            else:
                x = [-1600.0, -1500.0, -1100.0, 0.0][actor["slot"]]
        elif case["fixture"] != "martial" and actor["slot"] == 0:
            # Keep the ordinary target position, using the measured safe-caster setup.
            x = -1250.0
        if x is not None:
            state.write(actor["address"] + 0x18C0, "f", x)
            state.write(actor["address"] + 0x195C, "f", actor["center"][0] + x - actor["position"][0])
        if case["fixture"] == "recovery" and actor["side"] == 0:
            if condition := case.get("target_condition"):
                profile = state.read(actor["address"] + 4)
                mask = {"poison": 1, "attack_down": 1 << 4}[condition]
                state.write(profile + 0x10, "Q", state.read(profile + 0x10, "Q") | mask)
            if actor["slot"] != 0:
                state.write(actor["vitals_address"] + 0x2A, "H", 100)
    if case["fixture"] == "recovery":
        target = next(actor for actor in actors if actor["slot"] == 1)
        state.write(caster["address"] + 0x1990, "I", target["address"])
    ready = output / "ready.s01"
    state.save(ready)
    movie_path = Path(str(ready) + ".dtm")
    movie_path.unlink(missing_ok=True)
    movie_path.hardlink_to(output / "input.dtm")
    write(output / "ready.json", {"battle": inspect(state, battle=True)["battle"],
                                  "movie": movie, "changes": state.changes})
    return ready


def review(output):
    record = read(output / "fixture.json")
    case = record["case"]
    with gzip.open(output / "observations.json.gz", "rb") as stream:
        raw = stream.read()
    rows = json.loads(raw)
    capture = read(output / "capture.json")
    sample = capture["samples"][0]
    registered = capture["complete"] and sample["observations_sha256"] == hashlib.sha256(raw).hexdigest()
    caster = next(actor["address"] for actor in read(output / "ready.json")["battle"]["actors"] if actor["slot"] == 0)
    losses = [{"frame": i, "amount": (rows[i - 1]["battle_actor_0_tp_word"] & 65535) -
               (rows[i]["battle_actor_0_tp_word"] & 65535)} for i in range(1, len(rows))
              if (rows[i - 1]["battle_actor_0_tp_word"] & 65535) >
              (rows[i]["battle_actor_0_tp_word"] & 65535)]
    expected_tp = record["defaults"]["tp"] // 10 if case.get("input") == "guard_down" else case["tp_cost"]
    payments = [loss["frame"] for loss in losses if loss["amount"] == expected_tp]
    paid = payments[0] if payments else None
    identity = paid is not None and any(row["requested_native_word"] & 65535 == case["native"]
                                       for row in rows[paid:paid + 12])
    phases = [row["battle_actor_0_primary_phase_age_word"] >> 16 for row in rows]
    active = next((i for i in range(paid or 0, len(rows)) if phases[i] == 0x140), None)
    interrupted = False
    if case["fixture"] == "martial":
        ended = next((i for i in range((paid or 0) + 1, len(rows))
                      if rows[i]["battle_actor_0_action"] >> 24 != ATTACK_ACTION), None)
        next_action = rows[ended]["battle_actor_0_action"] >> 24 if ended is not None else None
        recovered = ended if next_action == RECOVERY_ACTION else None
        interrupted = next_action == HIT_REACTION_ACTION
    else:
        # Basic casts clear the presentation flag before cleanup; advanced
        # scenes retain it. Both observed endings release the cast normally.
        recovered = next((i for i in range(active or len(rows), len(rows))
                          if phases[i] in (0x200, 0x240)), None)
    owned = [sorted({row[f"projectile_{slot}_address"] for slot in range(16)
                     if row.get(f"projectile_{slot}_address") and row.get(f"projectile_{slot}_28") == caster})
             for row in rows]
    projectiles = any(owned)
    cleaned = not any(owned[-10:])
    births = [i for i in range(1, len(owned)) if set(owned[i]) - set(owned[i - 1])]
    hp_changes = [i for i in range(1, len(rows)) if any(
        rows[i].get(f"battle_actor_{slot}_hp") != rows[i - 1].get(f"battle_actor_{slot}_hp")
        for slot in range(8))]
    mechanical = registered and identity and len(payments) == len(losses) == 1 and recovered is not None and cleaned and not interrupted
    result = {"character": case["character"], "arte": case["arte"], "native": case["native"],
              "requested_identity_observed": identity, "payment_frames": payments,
              "tp_losses": losses,
              "active_frame": active, "recovery_frame": recovered,
              "owned_projectiles_observed": projectiles, "owned_projectiles_retired": cleaned,
              "projectile_birth_frames": births, "hp_change_frames_unattributed": hp_changes,
              "interrupted_before_recovery": interrupted,
              "registered_observations_match": registered,
              "mechanical_pass": mechanical, "semantic_review": "required",
              "native_comparison": "not_performed", "observations_sha256": digest(output / "observations.json.gz"),
              "limits": ["Review the named caption and actual effect/contact/support outcome.",
                         "HP changes alone are not attributed to this arte.",
                         "Mechanical success does not establish visual parity or alternate entry paths."]}
    write(output / "review.json", result)
    selected_frames = {0, len(rows) - 1}
    for start in (paid, active, recovered):
        if start is not None:
            selected_frames.update(min(start + offset, len(rows) - 1) for offset in (0, 24, 60))
    for events in (births, hp_changes):
        selected_frames.update([frame for frame in events if paid is not None and frame >= paid][:3])
    return result, sorted(selected_frames)


def capture_case(worker, output, record):
    ready = ready_state(worker, output, record)
    gc.collect()
    frames = output / "frames"
    worker.sequence(ready, frames, record["fixture"]["case_marker"], record["defaults"]["capture_frames"])
    with gzip.open(output / "observations.json.gz", "wb") as stream:
        stream.write((frames / "observations.json").read_bytes())
    shutil.copyfile(frames / "capture.json", output / "capture.json")
    result, samples = review(output)
    for frame in samples:
        shutil.copyfile(frames / f"frame-{frame:04}.png", output / f"frame-{frame:04}.png")
    shutil.rmtree(frames)
    if result["mechanical_pass"]:
        # Entry and ready checkpoints retain both reproducible inputs. The
        # intermediate warm snapshot is useful only when preparation fails.
        for name in ("warm.s01", "warm.s01.dtm"):
            (output / name).unlink(missing_ok=True)
    return {"complete": result["mechanical_pass"], "capture_complete": True, "selected_frames": samples}


def run(output, disc):
    if not output.is_relative_to(Path("local/battle-capture-work").resolve()):
        raise ValueError("attempt output must be under local/battle-capture-work")
    record = read(output / "fixture.json")
    for name, expected in [("entry.s01", record["entry_sha256"]), ("input.dtm", record["movie_sha256"])]:
        if digest(output / name) != expected:
            raise ValueError(f"prepared {name} changed")
    if (output / "outcome.json").exists():
        raise ValueError("this attempt was already run; preserve it and prepare a new output")
    worker = None
    outcome = {"complete": False, "worker_closed": False}
    try:
        worker = BattleFixture(str(disc), output / "input.dtm", output / "entry.s01",
                               output / "worker", record["fixture"]["marker_address"],
                               read(output / "locations.json"), disc_sha256=record["disc_sha256"])
        outcome.update(capture_case(worker, output, record))
    except Exception as error:
        outcome["error"] = str(error)
    finally:
        if worker is not None:
            worker.close(complete=outcome["complete"])
            outcome["worker_closed"] = True
        write(output / "outcome.json", outcome)
    return outcome


def batch(args, manifest):
    """Reuse one paused worker; every case still has its own input and receipt."""
    output = args.output.resolve()
    if not output.is_relative_to(Path("local/battle-capture-work").resolve()):
        raise ValueError("batch output must be under local/battle-capture-work")
    if len(args.case) != len(set(args.case)):
        raise ValueError("a resident batch cannot repeat a case")
    for key in args.case:
        case = selected(manifest, key)
        if reason := case.get("pending_setup"):
            raise ValueError(f"{key}: {manifest['pending_setups'][reason]}")
    output.mkdir(parents=True, exist_ok=False)
    worker = None
    results = []
    report = {"complete": False, "worker_closed": False, "cases": results}
    try:
        for key in args.case:
            case_output = output / key.replace(":", "-")
            prepare_args = argparse.Namespace(**vars(args) | {"case": key, "output": case_output})
            previous = args.resume_from / case_output.name if args.resume_from else None
            if previous and (previous / "fixture.json").exists():
                reuse_prepared(previous, case_output, selected(manifest, key))
            else:
                prepare_case(prepare_args, manifest)
            record = read(case_output / "fixture.json")
            if worker is None:
                marker = record["fixture"]["marker_address"]
                worker = BattleFixture(str(args.disc), case_output / "input.dtm", case_output / "entry.s01",
                                       output / "worker", marker, read(case_output / "locations.json"),
                                       disc_sha256=record["disc_sha256"])
            elif record["fixture"]["marker_address"] != marker:
                raise ValueError("prepared case changed the shared marker address")
            outcome = {"case": key, "complete": False, "worker_closed": False,
                       "resident_worker": str(output / "worker")}
            try:
                outcome.update(capture_case(worker, case_output, record))
            except Exception as error:
                outcome["error"] = str(error)
            write(case_output / "outcome.json", outcome)
            results.append(outcome)
            print(json.dumps(outcome), flush=True)
            # A semantic failure is a retained result. A broken capture cannot
            # safely supply the next case's registered state, so stop the worker.
            if not outcome.get("capture_complete"):
                break
        report["complete"] = len(results) == len(args.case) and all(case["complete"] for case in results)
    except Exception as error:
        report["error"] = str(error)
    finally:
        if worker is not None:
            worker.close(complete=report["complete"])
            report["worker_closed"] = True
            for result in results:
                result["worker_closed"] = True
                write(output / result["case"].replace(":", "-") / "outcome.json", result)
        write(output / "outcome.json", report)
    return report


def reuse_prepared(previous, output, case):
    """Reuse immutable inputs from an interrupted batch, preserving its evidence."""
    if (previous / "capture.json").exists():
        raise ValueError("a captured case needs review, not automatic repetition")
    record = read(previous / "fixture.json")
    if record["case"] != case:
        raise ValueError("retained fixture differs from the requested manifest case")
    for name, expected in (("entry.s01", record["entry_sha256"]),
                           ("input.dtm", record["movie_sha256"])):
        if digest(previous / name) != expected:
            raise ValueError(f"retained {name} changed")
    output.mkdir(exist_ok=False)
    for name in ("entry.s01", "entry.s01.dtm", "input.dtm", "locations.json"):
        (output / name).hardlink_to(previous / name)
    record["resume_from"] = str(previous)
    if (previous / "warm.s01").exists():
        (output / "warm.s01").hardlink_to(previous / "warm.s01")
        record["warm_sha256"] = digest(output / "warm.s01")
    write(output / "fixture.json", record)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    commands = parser.add_subparsers(dest="command", required=True)
    listing = commands.add_parser("list")
    listing.add_argument("--character", type=int)
    preparing = commands.add_parser("prepare")
    preparing.add_argument("--case", required=True, help="character:catalogue, e.g. 3:68")
    preparing.add_argument("--output", type=Path, required=True)
    preparing.add_argument("--state", type=Path)
    preparing.add_argument("--techniques", type=Path)
    running = commands.add_parser("run")
    running.add_argument("--prepared", type=Path, required=True)
    running.add_argument("--disc", type=Path, required=True)
    reviewing = commands.add_parser("review")
    reviewing.add_argument("--output", type=Path, required=True)
    resident = commands.add_parser("batch")
    resident.add_argument("--case", action="append", required=True, help="repeat character:catalogue for each case")
    resident.add_argument("--output", type=Path, required=True)
    resident.add_argument("--disc", type=Path, required=True)
    resident.add_argument("--state", type=Path)
    resident.add_argument("--techniques", type=Path)
    resident.add_argument("--resume-from", type=Path,
                          help="reuse uncaptured prepared cases from a stopped batch")
    args = parser.parse_args()
    manifest = read(args.manifest)
    if args.command == "list":
        for case in manifest["cases"]:
            if args.character is None or case["character"] == args.character:
                print(json.dumps(case | {"runnable": "pending_setup" not in case}))
    elif args.command == "prepare":
        print(prepare_case(args, manifest))
    elif args.command == "run":
        outcome = run(args.prepared.resolve(), args.disc)
        print(json.dumps(outcome))
        return 0 if outcome["complete"] else 1
    elif args.command == "batch":
        outcome = batch(args, manifest)
        print(json.dumps(outcome))
        return 0 if outcome["complete"] else 1
    else:
        result, _ = review(args.output)
        print(json.dumps(result))
        return 0 if result["mechanical_pass"] else 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
