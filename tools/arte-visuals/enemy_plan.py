#!/usr/bin/env python3
"""Plan bounded enemy visual captures; shared evidence never implies a pass."""
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path

from catalogue import CURRENT_VERSION, enumerate_library, fingerprint, read, visual_structure


def fixture(formations, enemy_key, enemy, row, revive, behavior):
    candidates = []
    for identifier, key in formations["mappings"].items():
        formation = formations["formations"][key]
        actors = formation["actors"]
        count = sum(actor.get("count", 1) for actor in actors)
        variants = [actor.get("statistics_variant", 0) for actor in actors
                    if actor["enemy"] == enemy_key]
        minimum = 2 if enemy == 191 and row >= 2 else 0
        if any(variant >= minimum for variant in variants) and (not revive or count >= 2):
            candidates.append((count, int(identifier), key, formation))
    if not candidates:
        return None
    _, identifier, key, definition = min(candidates, key=lambda candidate: candidate[:2])
    hp, stage = 100, "initial"
    for kind, controller in behavior.items():
        final = controller.get("final_actions", {})
        if final.get("start", 1 << 16) <= row < final.get("end", 0):
            stage = kind + ":final"
            hp = controller.get("airborne_hp_percent", controller.get("hp_percent", 100))
    return {"formation": identifier, "formation_key": key, "definition": definition,
            "arena": 13, "arena_basis": "declared existing component arena",
            "owner_hp_percent": hp, "controller_stage": stage}


def cohort_signature(edition, enemy_key, visual_signature, row, rules, setup, target):
    """Conservative reuse within one body owner, retaining every selection field.

    The existing catalogue signature resolves execution/contact/projectile/effect
    references. All remaining row data, actual fixture actors and controller
    settings also participate. Only its existing audio/hit-power filter applies.
    """
    if visual_signature is None or setup is None:
        return None
    selection = {key: value for key, value in row.items() if key != "execution"}
    return fingerprint({"edition": edition, "enemy_owner": enemy_key,
                        "visual": visual_signature,
                        "selection": visual_structure(selection, rules),
                        "fixture": setup, "target": target,
                        "policy": visual_structure(rules["actions"].get("policy", {}), rules)})


def associate(cases):
    groups = defaultdict(list)
    for case in cases:
        # Unknown execution/context never borrows another row's evidence.
        groups[case.get("cohort_signature") or case["id"]].append(case)
    cohorts = []
    for signature, members in groups.items():
        members.sort(key=lambda member: (member["binding"]["enemy"], member["binding"]["row"]))
        representative = members[0]
        identifier = f"{representative['edition']}:enemy-cohort:{signature[:20]}"
        for member in members:
            member["cohort"] = identifier
            member["shared_evidence"] = {"status": "pending", "cohort": identifier,
                                         "representative": representative["id"],
                                         "relationship": "representative" if member is representative
                                         else "same_visual_and_fixture_signature"}
        cohorts.append({"id": identifier, "edition": representative["edition"],
                        "context": representative["context"],
                        "representative": representative["id"],
                        "members": [member["id"] for member in members],
                        "signature": representative.get("cohort_signature"),
                        "capture": {"status": "pending"}, "review": {"status": "pending"}})
    return cohorts


def build(catalogue, batch_size=24):
    if catalogue["cooked_layout_version"] != CURRENT_VERSION:
        raise ValueError("enemy planner requires the current cooked layout")
    if not 1 <= batch_size <= 64:
        raise ValueError("enemy capture batches must contain 1..64 representatives")
    cases, summary = [], {}
    for edition, root in catalogue["libraries"].items():
        root = Path(root)
        # Read the same resolved full visual signatures used for counterparts.
        resolved = {case["id"]: case for case in enumerate_library(root, edition)
                    if case["category"] == "enemy"}
        formations = read(root / "battle/formations.json")
        mappings = read(root / "battle/enemies/mappings.json")
        definitions = {int(number): read(root / "battle/enemies" / key / "enemy.json")["rules"]
                       for number, key in mappings.items()}
        counts = defaultdict(Counter)
        for source in catalogue["cases"]:
            if source["edition"] != edition or source["category"] != "enemy":
                continue
            enemy, index = source["binding"]["enemy"], source["binding"]["row"]
            rules = definitions[enemy]
            row = rules["actions"]["rows"][index]
            execution = row.get("execution")
            kind = next(iter(execution)) if isinstance(execution, dict) else execution or "missing"
            policy = row.get("target_policy", rules.get("target_strategy", "nearest"))
            allied = row.get("target_allies", False)
            revive = allied and policy == "nearest_incapacitated"
            target = {"kind": "dead_ally" if revive else "self" if allied and policy == "self_target"
                      else "living_ally" if allied else "living_opponent", "policy": policy}
            behavior = rules["actions"].get("policy", {}).get("behavior", {})
            setup = fixture(formations, source["asset_key"], enemy, index, revive, behavior)
            category = "revival" if revive else "self" if target["kind"] == "self" else "allied" if allied else "leave" if kind == "leave_arena" else kind
            stage = setup["controller_stage"] if setup else "missing_fixture"
            visual = resolved[source["id"]]["_source"]
            signature = cohort_signature(edition, source["asset_key"], visual[1] if visual else None,
                                         row, rules, setup, target)
            natural = {key: value for key, value in row["requirements"].items()
                       if key != "difficulty" or value != {"start": 0, "end": 2}}
            counts["execution"][kind] += 1
            counts["setup_category"][category] += 1
            counts["form_context"][stage] += 1
            counts["natural_selection_requirements"].update(natural.keys())
            cases.append({"id": source["id"], "edition": edition, "binding": source["binding"],
                          "display_name": source["display_name"], "enemy_owner": source["asset_key"],
                          "native_technique_binding": source.get("native_technique_binding"),
                          "context": f"{category}:{stage}", "fixture": setup,
                          "target_context": target, "event_flags": [row["required_flag"]]
                          if row.get("required_flag") is not None else [],
                          "natural_selection_requirements": natural,
                          "planning_status": "planned" if setup and visual else "pending",
                          "cohort_signature": signature, "capture": {"status": "pending"},
                          "review": {"status": "pending"}})
        summary[edition] = {"rows": sum(counts["execution"].values()), "categories": dict(counts)}
    cohorts = associate(cases)
    groups, batches = defaultdict(list), []
    for cohort in cohorts:
        groups[cohort["edition"], cohort["context"]].append(cohort["representative"])
    for (edition, context), representatives in sorted(groups.items()):
        for index in range(0, len(representatives), batch_size):
            batches.append({"id": f"{edition}:{context}:{index // batch_size:03}",
                            "edition": edition, "context": context,
                            "cases": representatives[index:index + batch_size], "status": "pending"})
    for edition, counts in summary.items():
        counts["cohorts"] = sum(cohort["edition"] == edition for cohort in cohorts)
        counts["shared_member_rows"] = counts["rows"] - counts["cohorts"]
    return {"schema_version": 1, "cooked_layout_version": CURRENT_VERSION,
            "catalogue_signature": fingerprint(catalogue), "libraries": catalogue["libraries"],
            "scope": "Controlled execution captures; every row remains listed and pending. Shared evidence associations do not imply capture, natural AI eligibility, or visual acceptance.",
            "summary": summary, "batches": batches, "cohorts": cohorts, "cases": cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalogue", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--batch-size", type=int, default=24)
    args = parser.parse_args()
    plan = build(read(args.catalogue), args.batch_size)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(plan, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(plan["summary"], indent=2))


if __name__ == "__main__":
    main()
