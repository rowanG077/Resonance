"""Observe one admitted enemy action with neutral input for its full effect tail."""
import json
import math
from pathlib import Path
import shutil
import struct

from state import State, digest, inspect


def normalize_fixture_targets(state, battle):
    """Repair opposing selections invalidated by fixture vitals/availability writes."""
    actors = {actor["address"]: actor for actor in battle["actors"]}

    def living(actor):
        return (state.read(actor["vitals_address"] + 0x24, "I") > 0
                and state.read(actor["address"] + 0x107d, "B") & 7 != 2)

    def position(actor):
        return [state.read(actor["address"] + offset, "f") for offset in (0x18c0, 0x18c8)]

    repairs = []
    for actor in battle["actors"]:
        previous = actors.get(state.read(actor["address"] + 0x1990, "I"))
        # Dead allies can be intentional revival targets. Only fix stale
        # opponent selections; valid selections need not be the nearest actor.
        if (not living(actor) or previous is None or previous["side"] == actor["side"]
                or living(previous)):
            continue
        candidates = [candidate for candidate in battle["actors"]
                      if candidate["side"] != actor["side"] and living(candidate)]
        if not candidates:
            raise ValueError("fixture leaves a living actor without a living opponent")
        origin = position(actor)
        replacement = min(candidates, key=lambda candidate: (
            sum((a - b) ** 2 for a, b in zip(origin, position(candidate))), candidate["slot"]))
        state.write(actor["address"] + 0x1990, "I", replacement["address"])
        repairs.append(dict(actor=actor["slot"], previous_target=previous["slot"],
                            target=replacement["slot"]))
    return dict(repairs=repairs,
        scope="After fixture health/availability changes, living actors retain valid opponent selections or select the nearest living opponent. Ordinary actor updates refresh target direction; facing, motion, actions and timers are unchanged.")


def admitted(row, owner, expected, *, departure=False):
    action = row[f"battle_actor_{owner}_action"]
    active = action >> 24 == (20 if departure else 5)
    if not departure:
        active &= bool((action >> 16) & 255)
    return active and all(row[f"actor_{owner}_{key}"] &
                         (65535 if key == "native" else 0xffffffff) == value
                         for key, value in expected.items())


def wrong_admission(row, owner, expected):
    action = row[f"battle_actor_{owner}_action"]
    return ("selected" in expected and action >> 24 == 5
            and (action >> 16) & 255 != 0
            and row[f"actor_{owner}_selected"] & 0xffffffff != expected["selected"])


def prerequisite_ready(row, owner, inputs):
    if (row["battle_presentation_tick_word"] < inputs.get("selection_after_presentation_tick", 0)
            or row[f"battle_actor_{owner}_action"] >> 24 != 3
            or row[f"actor_{owner}_raw_280"] & 0x80000):
        return False
    for slot in inputs["physical_defeats"]:
        if (row[f"battle_actor_{slot}_hp"] != 0
                or row[f"actor_{slot}_control_word"] >> 16 & 7 != 2):
            return False
    if inputs.get("target_attacking") and (row["battle_actor_0_action"] >> 24 != 5
            or row["actor_0_target_state_word"] >> 24 != 3):
        return False
    if inputs.get("origin_bounds"):
        x, z = (struct.unpack(">f", struct.pack(">I", row[f"battle_actor_{owner}_{axis}_bits"]))[0]
                for axis in "xz")
        distance = math.hypot(x, z)
        if any((bounds["minimum"] is not None and distance < bounds["minimum"])
               or (bounds["maximum"] is not None and distance > bounds["maximum"])
               for bounds in inputs["origin_bounds"]):
            return False
    minimum, maximum = inputs["selection_range"]
    distance = struct.unpack(">f", struct.pack(">I", row[f"actor_{owner}_target_distance_bits"]))[0]
    return minimum <= distance <= (maximum or float("inf"))


def combine(target, segments, rows, state_path, **metadata):
    target.mkdir()
    receipt = json.loads((segments[-1] / "capture.json").read_text())
    receipt.update(metadata, segments=[str(path / "capture.json") for path in segments],
                   combined_observations=len(rows), initial_state_sha256=digest(state_path))
    (target / "observations.json").write_text(json.dumps(rows) + "\n")
    receipt["combined_observations_sha256"] = digest(target / "observations.json")
    (target / "capture.json").write_text(json.dumps(receipt, indent=2) + "\n")


def capture(worker, state_path, target, marker, max_vis, owner, expected, done, *, departure=False,
            prerequisites=None):
    """Retain admission, switch controller input, and capture the uninterrupted tail.

    No action, pose, contact, timer, or effect is replaced. Exact frame stepping
    prevents a short action from returning before the input switch. Source
    frames on both sides must remain consecutive and are all retained.
    """
    target = Path(target)
    if prerequisites and prerequisites.get("owner_control"):
        prelude = target.with_name(target.name + "-prerequisite")
        rows = worker.observe_until(state_path, prelude, marker, max_vis,
            lambda observed: prerequisite_ready(observed[-1], owner, prerequisites), stepped=True)
        if not prerequisite_ready(rows[-1], owner, prerequisites):
            shutil.copytree(prelude, target)
            return rows
        before = target.with_name(target.name + "-released.s01")
        enabled = target.with_name(target.name + "-selectable.s01")
        worker.snapshot(before)
        state = State(before)
        battle = inspect(state, battle=True)["battle"]
        if any(battle[key] != rows[-1][f"battle_{key}_word"]
               for key in ("combat_tick", "presentation_tick")):
            raise ValueError("prerequisite snapshot differs from its final observed update")
        actor = next(actor for actor in battle["actors"] if actor["slot"] == owner)
        at = actor["address"]
        control = prerequisites["owner_control"]
        state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38 | control["mode"])
        state.write(at + 0x107d, "B", state.read(at + 0x107d, "B") & 0x3f | control["port"])
        state.save(enabled)
        shutil.copyfile(str(before) + ".dtm", str(enabled) + ".dtm")
        boundary = dict(source_owner=owner, combat_tick=battle["combat_tick"],
            presentation_tick=battle["presentation_tick"], changes=state.changes,
            released_state=dict(path=str(before), sha256=digest(before)),
            selectable_state=dict(path=str(enabled), sha256=digest(enabled)),
            scope="Restore AI input only after observed physical prerequisites and recovery; no action, pose, position or timer changes.")
        del state
        remaining = max_vis - len(rows)
        if remaining <= 0:
            shutil.copytree(prelude, target)
            return rows
        selected = target.with_name(target.name + "-selected")
        tail = capture(worker, enabled, selected, marker, remaining, owner, expected,
                       lambda observed: done(rows + observed), departure=departure)
        worker.validate_rows(rows[-1:] + tail, marker)
        rows.extend(tail)
        combine(target, [prelude, selected], rows, state_path, preselection_control=boundary)
        return rows
    admission = target.with_name(target.name + "-admission")
    rows = worker.observe_until(state_path, admission, marker, max_vis,
        lambda observed: (admitted(observed[-1], owner, expected, departure=departure)
                          or wrong_admission(observed[-1], owner, expected)), stepped=True)
    if not admitted(rows[-1], owner, expected, departure=departure):
        shutil.copytree(admission, target)
        return rows

    before = target.with_name(target.name + "-admitted.s01")
    controlled = target.with_name(target.name + "-controlled.s01")
    worker.snapshot(before)
    state = State(before)
    battle = inspect(state, battle=True)["battle"]
    observed = rows[-1]
    if any(battle[key] != observed[f"battle_{key}_word"]
           for key in ("combat_tick", "presentation_tick")):
        raise ValueError("admission snapshot does not match the final stepped observation")
    actor = next(actor for actor in battle["actors"] if actor["slot"] == owner)
    at = actor["address"]
    state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38)
    state.write(at + 0x107d, "B", (state.read(at + 0x107d, "B") & 0x3f) | 0xc0)
    state.save(controlled)
    shutil.copyfile(str(before) + ".dtm", str(controlled) + ".dtm")
    boundary = dict(source_owner=owner, combat_tick=battle["combat_tick"],
        presentation_tick=battle["presentation_tick"], changes=state.changes,
        admitted_state=dict(path=str(before), sha256=digest(before)),
        controlled_state=dict(path=str(controlled), sha256=digest(controlled)),
        scope="Neutral controller input after witnessed admission; authored execution and complete effect tail remain live.")
    del state

    remaining = max_vis - len(rows)
    if remaining <= 0 or done(rows):
        shutil.copytree(admission, target)
    else:
        tail_target = target.with_name(target.name + "-tail")
        tail = worker.observe_until(controlled, tail_target, marker, remaining,
                                    lambda observed: done(rows + observed))
        worker.validate_rows(rows[-1:] + tail, marker)
        rows.extend(tail)
        combine(target, [admission, tail_target], rows, state_path, single_action_control=boundary)
        return rows
    path = target / "capture.json"
    receipt = json.loads(path.read_text())
    receipt["single_action_control"] = boundary
    receipt["combined_observations"] = len(rows)
    receipt["combined_observations_sha256"] = digest(target / "observations.json")
    receipt["initial_state_sha256"] = digest(state_path)
    path.write_text(json.dumps(receipt, indent=2) + "\n")
    return rows
