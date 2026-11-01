#!/usr/bin/env python3
"""Observe committed combo transitions without replacing actions or timers."""
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
from strategy_combos import arrange, write
from technique_content import native_learning


def run(args):
    if digest(args.disc) != DISC_SHA256:
        raise ValueError("transition observer requires the pinned GQSEAF Disc 1")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = State(args.state)
    battle = inspect(source, battle=True)["battle"]
    actors = {a["slot"]: a for a in battle["actors"]}
    secondary = args.phase == "secondary"
    character, technique = (5, 126) if secondary else (1, 10)
    if set(actors) != {0, 1, 4} or actors[0]["kind"] != character or any(a["action"][0] != 3 for a in actors.values()):
        raise ValueError("expected a settled manual party with the requested character")
    catalogue = args.assets / "game/techniques.json"
    learning = native_learning(json.loads(catalogue.read_text()))[character - 1]
    case = dict(name=args.phase, character=character, technique=technique,
                position=1, skill=5 if secondary else 1,
                random_state=2, level=30)
    base, mask = arrange(source, battle, case, learning)
    if secondary:
        # The stock autonomous continuations do not reach Finisher. A table-only
        # mod enables that route so its second checkpoint can be exercised.
        selectors = base.read(battle["sections"][5]["address"] + 0x3a80)
        for normal in (0, 2, 3):
            base.write(selectors + normal * 4 + 1, "B", 1 << 4)
            base.write(selectors + normal * 4 + 2, "B", 4)
        case["modified_continuation"] = True
    marker_address = source.read(0x8035a578) + 0x581c + 31 * 0x360 + 0x300
    marker = int.from_bytes(sha256(args.phase.encode()).digest()[:4])
    base.write(marker_address, "I", marker)
    priming = out / "priming.s01"
    base.save(priming)
    movie = out / "neutral.dtm"
    neutral_movie(source.path, movie, struct.unpack_from("<Q", source.raw, 33)[0], polls=24000)
    shutil.copyfile(movie, str(priming) + ".dtm")
    write(out / "priming.json", dict(case=case, changes=base.changes, game_code_modified=False))
    pointer, at = "8035a86c 10 30 540", 0x1cea0
    locations = watch_locations(dict(pool=1, bss_index=6, actors=[dict(slot=i) for i in range(8)]))
    for offset, name in ((0x1174, "native"), (0x104, "selected"), (0x1170, "checkpoints"),
                         (0x117c, "combo_word"), (0x1180, "style_word"), (0x199c, "contact")):
        locations[f"{pointer} {at + offset:x}"] = "owner_" + name
    locations[f"{pointer} {at + 8:x} 0"] = "owner_strategy"
    if secondary:
        for normal in (0, 2, 3):
            locations[f"8035a86c 10 28 3a80 {normal * 4:x}"] = f"continuation_{normal}"
    for offset, name in ((0x70, "learned"), (0x78, "enabled")):
        for word in (0, 4):
            locations[f"{pointer} {at:x} {offset + word:x}"] = f"owner_{name}_{word}"
    report = dict(complete=False, phase=args.phase, cases=[], source_sha256=source.sha256,
                  scope="Committed combo transitions, effective contact and payment; no exact timing, RNG or audiovisual parity.")
    worker = None
    try:
        worker = BattleFixture(args.disc, movie, priming, out / "worker", marker_address, locations,
                               disc_sha256=DISC_SHA256, capture_images=False)
        def reached(rows):
            r = rows[-1]
            if secondary:
                age, primary, retry = r["battle_actor_0_cast_clock_word"] & 65535, r["owner_checkpoints"] >> 16, r["owner_checkpoints"] & 65535
                return (r["battle_actor_0_action"] >> 24 == 5 and not r["owner_native"] & 65535
                        and r["owner_style_word"] >> 16 & 255 == 4 and age <= 1)
            return r["battle_actor_0_action"] >> 24 == 5 and r["owner_native"] & 65535 == 10
        prelude = worker.observe_until(priming, out / "prelude", marker, 900, reached, stepped=True)
        worker.snapshot(out / "committed.s01")
        if not reached(prelude):
            raise ValueError("natural combo did not reach the requested transition")
        if secondary:
            aligned = State(out / "committed.s01")
            actor, target = actors[0]["address"], actors[4]["address"]
            heading = math.radians(aligned.read(actor + 0x18d0, "f"))
            position = [aligned.read(actor + 0x18c0, "f") + math.sin(heading) * 60., 0.,
                        aligned.read(actor + 0x18c8, "f") + math.cos(heading) * 60.]
            for axis, value in enumerate(position):
                aligned.write(target + 0x18c0 + axis * 4, "f", value)
            aligned.save(out / "aligned.s01")
            shutil.copyfile(str(out / "committed.s01") + ".dtm", str(out / "aligned.s01") + ".dtm")
            write(out / "alignment.json", dict(changes=aligned.changes, game_code_modified=False))
            def after_primary(rows):
                r = rows[-1]
                return (r["battle_actor_0_action"] >> 24 == 5 and r["owner_contact"]
                        and r["owner_checkpoints"] >> 16 < r["battle_actor_0_cast_clock_word"] & 65535 < r["owner_checkpoints"] & 65535)
            effective = worker.observe_until(out / "aligned.s01", out / "effective-prelude", marker, 60, after_primary, stepped=True)
            if not after_primary(effective):
                raise ValueError("committed Finisher did not make effective contact before its retry")
        worker.snapshot(out / "ready.s01")
        state = State(out / "ready.s01")
        actor = actors[0]["address"]
        initial = dict(age=state.read(actor + 0x1be, "H"), primary=state.read(actor + 0x1170, "H"),
                       retry=state.read(actor + 0x1172, "H"), selected=state.read(actor + 0x104),
                       native=state.read(actor + 0x1174) & 65535, normal=state.read(actor + 0x1181, "B"),
                       stage=state.read(actor + 0x117d, "B"), contact=state.read(actor + 0x199c),
                       tp=state.read(actors[0]["vitals_address"] + 0x2a, "H"))
        report["initial"] = initial
        enabled = [126] if secondary else [1, 2, 10, 24]
        mask = sum(1 << learning.index(key) for key in enabled)
        state.write(state.read(actor) + 0x78, "Q", mask)
        for offset, value in enumerate((1, 1, 6 if secondary else 3)):
            state.write(actors[0]["vitals_address"] + offset, "B", value)
        marker ^= 0xa5a5a5a5
        state.write(marker_address, "I", marker)
        path = out / "input.s01"
        state.save(path)
        shutil.copyfile(str(out / "ready.s01") + ".dtm", str(path) + ".dtm")
        write(out / "fixture.json", dict(changes=state.changes, case_marker=marker,
                                         state_sha256=digest(path), game_code_modified=False))
        rows = worker.observe(path, out / "observations", marker, 180)
        worker.snapshot(out / "terminal.s01")
        admissions, payments = [], []
        prior = initial["native"]
        for i, r in enumerate(rows):
            native = r["owner_native"] & 65535
            if r["battle_actor_0_action"] >> 24 == 5 and native and native != prior:
                admissions.append(dict(sample=i, native=native, prior_age=(rows[i-1]["battle_actor_0_cast_clock_word"] & 65535) if i else initial["age"],
                                       prior_contact=rows[i-1]["owner_contact"] if i else initial["contact"]))
            if r["battle_actor_0_action"] >> 24 == 5 or not native:
                prior = native
            before = rows[i-1]["battle_actor_0_tp_word"] & 65535 if i else initial["tp"]
            cost = before - (r["battle_actor_0_tp_word"] & 65535)
            if cost > 0:
                payments.append(dict(sample=i, native=native, cost=cost))
        policy = all(r["owner_strategy"] >> 8 == (0x010106 if secondary else 0x010103)
                     and r["battle_actor_0_flags"] >> 27 & 7 == 2
                     and (r["owner_enabled_0"] << 32 | r["owner_enabled_4"]) == mask for r in rows)
        if secondary:
            policy &= all(r[f"continuation_{normal}"] == (normal << 24 | 0x100400)
                          for r in rows for normal in (0, 2, 3))
        learned = all((r["owner_learned_0"], r["owner_learned_4"]) ==
                      (rows[0]["owner_learned_0"], rows[0]["owner_learned_4"]) for r in rows)
        sole = all(r[f"battle_actor_{slot}_action"] >> 24 != 5 for r in rows for slot in (1, 4))
        if secondary:
            matched = bool(admissions) and admissions[0]["native"] == 63 and admissions[0]["prior_age"] >= initial["retry"]
            paid = any(p["native"] == 63 and p["cost"] == 5 for p in payments)
        else:
            sequence = [entry["native"] for entry in admissions]
            matched = bool(sequence) and sequence[0] in (2, 24) and sequence in ([2], [24], [2, 24])
            paid = all(any(p["native"] == entry["native"] and p["cost"] == {2: 16, 24: 30}.get(entry["native"]) for p in payments) for entry in admissions)
        effective = bool(admissions) and all(entry["prior_contact"] for entry in admissions)
        result = dict(case=dict(case, enabled=enabled, transition=args.phase), passed=bool(policy and learned and sole and matched and paid and effective),
                      admissions=admissions, payments=payments, live_policy_verified=policy,
                      learned_repertoire_unchanged=learned, sole_actor=sole, effective_prior_contact=effective,
                      observations_sha256=digest(out / "observations/observations.json"))
        report["cases"].append(result)
        report["complete"] = result["passed"]
        print(json.dumps(result), flush=True)
    finally:
        if worker:
            worker.close(complete=report["complete"])
        write(out / "report.json", report)
    if not report["complete"]:
        raise RuntimeError("transition differs; inspect retained report and observations")
    write(out / "outcome-acceptance.json", dict(report, input_report_sha256=digest(out / "report.json")))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--disc", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--assets", type=Path, default=Path("local/all-assets"))
    parser.add_argument("--phase", choices=("secondary", "successors"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    run(parser.parse_args())
