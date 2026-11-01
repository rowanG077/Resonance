#!/usr/bin/env python3
"""Observe autonomous support decisions, payment and attributable benefits in Dolphin.

Starts a fresh recorded encounter with ordinary party/learning commands. Component
cases edit repertoire, strategy, vitals, placement and lasting conditions only.
"""
import argparse
from hashlib import sha256
import json
from pathlib import Path
import shutil

from battle_fixture import BattleFixture, neutral_movie
from battle_state import watch_locations
from effect_fixture import prepare
from state import State, digest, inspect
from strategy_buffs import DISC_SHA256
from technique_content import native_learning

CASES = (
    dict(name="heal-tie", techniques=[98], hp=[400, 400], native=236, targets=[1], benefit="hp", cost=8),
    dict(name="heal-lowest", techniques=[98], hp=[300, 400], native=236, targets=[0], benefit="hp", cost=8),
    dict(name="heal-last-tp", techniques=[98], hp=[400, 2000], tp=8, native=236, targets=[0], benefit="hp", cost=8),
    dict(name="heal-group", techniques=[98, 99], hp=[400, 400], native=237, targets=[1], benefit="hp", cost=28),
    dict(name="buff-single", techniques=[107], hp=[2000, 2000], tp=12, native=245, targets=[0, 1], benefit="attack_up", cost=12),
    dict(name="buff-group-partial", techniques=[107, 108], hp=[2000, 2000], buffed=[0], tp=32, native=246, targets=[0, 1], benefit="attack_up", cost=32),
    dict(name="cure-curse", techniques=[101], hp=[2000, 2000], cursed=[0], tp=12, native=239, targets=[0], benefit="cure", cost=12),
    dict(name="revival", techniques=[121], hp=[0, 2000], tp=48, native=259, targets=[0], benefit="revival", cost=48),
)

COMPETITION_CASES = (
    dict(name="revival-before-healing", techniques=[121, 98], hp=[0, 400], native=259, targets=[0], benefit="revival", cost=48),
    dict(name="heal-before-cure", techniques=[98, 101], hp=[400, 2000], cursed=[0], native=236, targets=[0], benefit="hp", cost=8),
    dict(name="cure-before-buff", techniques=[101, 107], hp=[2000, 2000], cursed=[0], native=239, targets=[0], benefit="cure", cost=12),
    dict(name="heal-before-buff", techniques=[98, 107], hp=[400, 2000], native=236, targets=[0], benefit="hp", cost=8),
    dict(name="unaffordable-revival", techniques=[121, 98], hp=[0, 400], tp=8, native=236, targets=[1], benefit="hp", cost=8),
    dict(name="physical-cure-before-magical", techniques=[101, 104], hp=[2000, 2000], cursed=[0], weak=[0], native=239, targets=[0], benefit="cure", cost=12),
    dict(name="heal-at-threshold", techniques=[98, 107], hp=[1400, 2000], native=236, targets=[0], benefit="hp", cost=8),
    dict(name="buff-above-threshold", techniques=[98, 107], hp=[1420, 2000], native=245, targets=[0, 1], benefit="attack_up", cost=12),
    dict(name="default-healing-threshold", techniques=[98, 107], hp=[1200, 2000], native=236, targets=[0], benefit="hp", cost=8),
    dict(name="edited-healing-threshold", techniques=[98, 107], hp=[1200, 2000], healing_threshold=50, native=245, targets=[0, 1], benefit="attack_up", cost=12),
    dict(name="healing-circle-four-pulses", techniques=[120], hp=[400, 400], tp=56, native=258, targets=[1], benefit="hp", cost=56,
         positions=[[-250., 0., -50.], [-200., 0., 0.]], pulses=4),
)

EXTENDED_CASES = (
    dict(name="dispel-weak", techniques=[104], hp=[2000, 2000], weak=[0], native=242, targets=[0], benefit="cure_weak", cost=16),
    dict(name="nullify-weak-group", techniques=[105], hp=[2000, 2000], weak=[0, 1], native=243, targets=[0, 1], benefit="cure_weak", cost=28),
    dict(name="barrier-single", techniques=[110], hp=[2000, 2000], native=248, targets=[0, 1], benefit="defense_up", cost=8),
    dict(name="barrier-group", techniques=[111], hp=[2000, 2000], native=249, targets=[0, 1], benefit="defense_up", cost=48),
)


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + "\n")


def review(rows, case, addresses, expected_mask):
    first = next((i for i, r in enumerate(rows) if r["battle_actor_1_action"] >> 24 == 5
                  and r["actor_1_native"] & 65535), None)
    if first is None:
        return dict(passed=False, first_decision=None)
    row = rows[first]
    native = row["actor_1_native"] & 65535
    target = addresses.get(row["actor_1_support_target"])
    payment = next((i for i, r in enumerate(rows[first:], first)
                    if r["battle_actor_1_tp_word"] & 65535 < case.get("tp", 100)), None)
    events = []
    for i in range(first + 1, len(rows)):
        before, after = rows[i - 1], rows[i]
        changed = []
        for slot in (0, 1):
            prefix = f"actor_{slot}_"
            body = f"battle_actor_{slot}_"
            kind = case["benefit"]
            if kind in ("hp", "revival"):
                improved = after[body + "hp"] > before[body + "hp"]
                if kind == "revival":
                    improved &= after[body + "flags"] >> 16 & 7 == 3
            elif kind in ("attack_up", "defense_up"):
                bit = 0x1000 if kind == "attack_up" else 0x2000
                improved = not before[prefix + "conditions_low"] & bit and bool(after[prefix + "conditions_low"] & bit)
            else:
                bit = 0x10 if kind == "cure_weak" else 0x80
                improved = bool(before[prefix + "conditions_low"] & bit) and not after[prefix + "conditions_low"] & bit
            if improved:
                changed.append(slot)
        if changed:
            events.append(dict(sample=i, changes=[dict(
                slot=slot, hp_before=before[f"battle_actor_{slot}_hp"],
                hp_after=after[f"battle_actor_{slot}_hp"],
                conditions_before=before[f"actor_{slot}_conditions_low"],
                conditions_after=after[f"actor_{slot}_conditions_low"])
                for slot in changed]))
            if len(events) == case.get("pulses", 1):
                break
    benefit = events[0]["sample"] if events else None
    beneficiaries = [change["slot"] for change in events[0]["changes"]] if events else []
    paid = case.get("tp", 100) - (rows[payment]["battle_actor_1_tp_word"] & 65535) if payment is not None else None
    complete_benefit = len(events) == case.get("pulses", 1)
    end = events[-1]["sample"] if complete_benefit else len(rows) - 1
    policy = all([(r["owner_strategy"] >> shift) & 255 for shift in (24, 16, 8)] == [1, 8, 2]
                 and r["battle_actor_1_flags"] >> 27 & 7 == 2 for r in rows[:end + 1])
    sole_action = all(r[f"battle_actor_{slot}_action"] >> 24 != 5
                      for r in rows[first:end + 1] for slot in (0, 4))
    # Released recovery objects can outlive the actor's casting pose. Require
    # the paid spell identity, and exclude any intervening different technique.
    same_spell = (payment is not None and rows[payment]["actor_1_native"] & 65535 == native
                  and all(r["actor_1_native"] & 65535 in (0, native) for r in rows[first:end + 1]))
    initial_conditions = [rows[0][f"actor_{slot}_conditions_low"] for slot in (0, 1)]
    admission_conditions = [row[f"actor_{slot}_conditions_low"] for slot in (0, 1)]
    primed = (not case.get("buffed") or any(value & 0x1000 for value in admission_conditions)
              and any(not value & 0x1000 for value in admission_conditions))
    repertoire = all((r["owner_enabled_0"] << 32 | r["owner_enabled_4"]) == expected_mask
                     for r in rows[:end + 1])
    ailments = all(bool(admission_conditions[slot] & bit) == (slot in case.get(key, []))
                   for key, bit in (("cursed", 0x80), ("weak", 0x10)) for slot in (0, 1))
    retained_weak = (not case.get("weak") or benefit is not None and all(
        bool(rows[benefit][f"actor_{slot}_conditions_low"] & 0x10) == (case["benefit"] != "cure_weak")
        for slot in case["weak"]))
    threshold = all(r["healing_tail_word"] >> 24 == case.get("healing_threshold", 70)
                    for r in rows[:end + 1]) if "healing_tail_word" in row else "healing_threshold" not in case
    pulse_outcomes = (case.get("pulses", 1) == 1 or all(
        [change["slot"] for change in event["changes"]] == [0, 1]
        and all(change["hp_before"] == 400 + index * 300
                and change["hp_after"] == 700 + index * 300 for change in event["changes"])
        for index, event in enumerate(events)))
    admissions = sum(r["battle_actor_1_action"] >> 24 == 5
                     and (i == 0 or rows[i - 1]["battle_actor_1_action"] >> 24 != 5)
                     for i, r in enumerate(rows[:end + 1]))
    passed = (admissions == 1 and repertoire and primed and ailments and retained_weak and threshold
              and complete_benefit and pulse_outcomes and native == case["native"] and target in case["targets"] and policy
              and payment is not None and paid == case["cost"] and benefit is not None and payment <= benefit
              and sole_action and same_spell and (case["benefit"] in ("attack_up", "defense_up") or target in beneficiaries))
    changes = events[0]["changes"] if events else []
    return dict(passed=passed, first_decision=first, native=native, target=target, benefit_changes=changes,
                payment_sample=payment, paid=paid, admissions=admissions, benefit_sample=benefit, beneficiaries=beneficiaries,
                live_policy_verified=policy, live_repertoire_verified=repertoire, sole_action=sole_action, same_spell=same_spell,
                initial_conditions=initial_conditions, admission_conditions=admission_conditions,
                benefit_events=events, live_ailments_verified=ailments, weak_retained=retained_weak,
                live_healing_threshold_verified=threshold, complete_benefit=complete_benefit)


def arrange(base, battle, case, learning):
    state = base.fork()
    actors = {a["slot"]: a for a in battle["actors"]}
    party_positions = case.get("positions", [[-300., 0., -150.], [-200., 0., 0.]])
    positions = {0: party_positions[0], 1: party_positions[1], 4: [750., 0., 300.]}
    for slot, actor in actors.items():
        at, vitals = actor["address"], actor["vitals_address"]
        state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38 | (0x10 if slot == 1 else 0))
        for axis, value in enumerate(positions[slot]):
            state.write(at + 0x18c0 + axis * 4, "f", value)
        hp = case["hp"][slot] if slot < 4 else 30000
        for offset, value in ((0x20, 2000 if slot < 4 else 30000), (0x24, hp)):
            state.write(vitals + offset, "I", value)
        state.write(vitals + 0x1d, "B", 20)
        state.write(at + 0x107d, "B", state.read(at + 0x107d, "B") & ~7 | (2 if hp == 0 else 3))
        profile = state.read(at + 4)
        low = state.read(profile + 0x14)
        if not case.get("buffed"):
            low = (low & ~(0x1000 | 0x80 | 0x10)
                   | (0x80 if slot in case.get("cursed", []) else 0)
                   | (0x10 if slot in case.get("weak", []) else 0))
            state.write(profile + 0x14, "I", low)
    owner = actors[1]
    for offset, value in enumerate((1, 8, 2)):
        state.write(owner["vitals_address"] + offset, "B", value)
    state.write(owner["vitals_address"] + 0x28, "H", 100)
    state.write(owner["vitals_address"] + 0x2a, "H", case.get("tp", 100))
    state.write(battle["pool"] + 0x159c3, "b", 0)
    if "healing_threshold" in case:
        state.write(battle["sections"][4]["address"] + 0x286c + 8, "B", case["healing_threshold"])
    mask = sum(1 << learning.index(technique) for technique in case["techniques"])
    member = state.read(owner["address"])
    # Keep the ordinarily taught repertoire. Removing learned spells permits
    # use-triggered learning to replace a cast before it pays or releases.
    state.write(member + 0x78, "Q", mask)
    return state


def run(args):
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("support observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = State(args.state)
    observed = inspect(source)
    catalogue = args.assets / "game/techniques.json"
    learning = native_learning(json.loads(catalogue.read_text()))[3]
    techniques = sorted(set(learning) - {0})
    entry = out / "entry.s01"
    fixture = prepare(source, entry, observed, [], [(0x24, [1]), (0x24, [4])]
                      + [(0x2e, [4, technique]) for technique in techniques]
                      + [(0xcd, [0, 0, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0])], seed=1)
    state = State(entry)
    settings = state.read(0x8035a768)
    for slot, character in enumerate((1, 4, 0, 0, 0, 0, 0, 0)):
        state.write(settings + 0xe9d + slot, "B", character)
    for slot in range(4):
        state.write(settings + 0xea5 + slot, "B", 0)
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
    neutral_movie(source.path, movie, observed["movie"]["input_count"], polls=60000)
    shutil.copyfile(movie, str(entry) + ".dtm")
    pointer = "8035a86c 10 30 540"
    locations = watch_locations(dict(pool=1, bss_index=6, actors=[dict(slot=i) for i in range(8)]))
    for slot in (0, 1, 4):
        at = 0x1cea0 + slot * 0x1ba0
        for offset, name in ((0x1174, "native"), (0x198c, "support_target")):
            locations[f"{pointer} {at + offset:x}"] = f"actor_{slot}_{name}"
        locations[f"{pointer} {at + 4:x} 14"] = f"actor_{slot}_conditions_low"
    at = 0x1cea0 + 0x1ba0
    locations[f"{pointer} {at + 8:x} 0"] = "owner_strategy"
    locations["8035a86c 10 20 2874"] = "healing_tail_word"
    for offset, name in ((0x70, "learned"), (0x78, "enabled")):
        for word in (0, 4):
            locations[f"{pointer} {at:x} {offset + word:x}"] = f"owner_{name}_{word}"
    report = dict(complete=False, cases=[], fixture=fixture, source_sha256=source.sha256,
                  techniques_sha256=digest(catalogue), scope="First autonomous support decision, TP payment and declared attributable benefits; no timing or audiovisual equivalence.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, entry, out / "worker", fixture["marker_address"],
                               locations, disc_sha256=DISC_SHA256, capture_images=False)
        worker.warmup(entry, out / "warmup", fixture["case_marker"], 240)
        worker.snapshot(out / "ready.s01")
        base = State(out / "ready.s01")
        battle = inspect(base, battle=True)["battle"]
        actors = {a["slot"]: a for a in battle["actors"]}
        if set(actors) != {0, 1, 4} or actors[1]["action"][0] != 3:
            raise ValueError("requested idle support party was not initialized")
        addresses = {a["address"]: a["slot"] for a in actors.values()}
        suite = {"first": CASES, "competition": COMPETITION_CASES, "extended": EXTENDED_CASES}[args.suite]
        cases = [case for case in suite if not args.cases or case["name"] in args.cases]
        if not cases or args.cases and set(args.cases) != {case["name"] for case in cases}:
            raise ValueError("unknown or empty support case selection")
        for case in cases:
            target = out / case["name"]
            target.mkdir()
            case_base = base
            if case.get("buffed"):
                prime = out / "buff-single" / "terminal.s01"
                if not prime.is_file():
                    raise ValueError("partial-group case requires the preceding natural buff case")
                settling = target / "settling.s01"
                primed = State(prime)
                at = actors[1]["address"]
                primed.write(at + 0x107c, "B", primed.read(at + 0x107c, "B") & ~0x38)
                prime_marker = int.from_bytes(sha256((case["name"] + "-settle").encode()).digest()[:4])
                primed.write(fixture["marker_address"], "I", prime_marker)
                primed.save(settling)
                shutil.copyfile(str(prime) + ".dtm", str(settling) + ".dtm")
                write(target / "settling.json", dict(changes=primed.changes, case_marker=prime_marker,
                                                     game_code_modified=False, source_sha256=digest(prime)))
                worker.observe_until(settling, target / "settling", prime_marker, 600,
                                     lambda rows: rows[-1]["battle_actor_1_action"] >> 24 == 3)
                worker.snapshot(target / "buffed-ready.s01")
                case_base = State(target / "buffed-ready.s01")
                if case_base.read(at + 0x1ac, "B") != 3:
                    raise ValueError("natural buff fixture did not settle to idle")
            state = arrange(case_base, battle, case, learning)
            marker = int.from_bytes(sha256(json.dumps(case, sort_keys=True).encode()).digest()[:4])
            state.write(fixture["marker_address"], "I", marker)
            state.save(target / "input.s01")
            shutil.copyfile(str(case_base.path) + ".dtm", target / "input.s01.dtm")
            write(target / "fixture.json", dict(case=case, changes=state.changes, case_marker=marker,
                game_code_modified=False, state_sha256=digest(target / "input.s01")))
            mask = sum(1 << learning.index(technique) for technique in case["techniques"])
            rows = worker.observe_until(target / "input.s01", target / "observations", marker,
                                        args.vis, lambda rows: review(rows, case, addresses, mask)["passed"])
            worker.snapshot(target / "terminal.s01")
            result = dict(case=case, observations_sha256=digest(target / "observations/observations.json"), **review(rows, case, addresses, mask))
            report["cases"].append(result)
            print(json.dumps(result), flush=True)
        report["complete"] = bool(report["cases"]) and all(case["passed"] for case in report["cases"])
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("support observations differ; inspect report.json")
    write(out / "outcome-acceptance.json", dict(complete=True,
        input_report_sha256=digest(out / "report.json"), cases=report["cases"], scope=report["scope"]))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True, help="Loaded field checkpoint with .dtm companion")
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cases", nargs="+")
    parser.add_argument("--suite", choices=("first", "competition", "extended"), default="first")
    parser.add_argument("--vis", type=int, default=900)
    run(parser.parse_args())
