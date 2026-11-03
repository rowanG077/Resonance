#!/usr/bin/env python3
"""Plan one reproducible distinct-owner capture per actual Unison recipe."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path

from catalogue import CURRENT_VERSION, fingerprint, read


def distinct_assignments(choices):
    """Keep ordered recipe roles; do not enumerate repeated owners."""
    completed = []

    def expand(members):
        if len(members) == len(choices):
            completed.append(list(members))
            return
        used = {member["character"] for member in members}
        for candidate in choices[len(members)]:
            if candidate["character"] not in used:
                expand([*members, candidate])

    expand([])
    return sorted(completed, key=lambda members: tuple(
        (member["character_name"], member["arte_key"]) for member in members))


def plan_recipe(identifier, binding, choices, scene_signature):
    alternatives = []
    for members in distinct_assignments(choices):
        blockers = [member["scope_evidence"] for member in members
                    if member["scope_status"] == "source_unimplemented_slot"]
        alternatives.append({
            "assignment": [[member["character"], member["arte"]] for member in members],
            "semantic_members": [{"character_name": member["character_name"],
                                  "arte_key": member["arte_key"],
                                  "action_signature": member["action_signature"]} for member in members],
            "choreography_signature": fingerprint({"scene": scene_signature,
                "roles": [(member["character_name"], member["action_signature"], member["placement"])
                          for member in members]}),
            "primary_choreography_signature": fingerprint({"scene": scene_signature,
                "ordered_bodies": [member["character_name"] for member in members]}),
            "blockers": blockers,
        })
    usable = [assignment for assignment in alternatives if not assignment["blockers"]]
    selected = usable[0] if usable else None
    return {"id": identifier, "category": "unison", "binding": binding,
            "planning_status": "planned" if selected else "pending",
            "pending_reason": None if selected else "source_null_ingredients" if alternatives
                else "no_distinct_owner_assignment",
            "assignment": selected["assignment"] if selected else None,
            "choreography_signature": selected["choreography_signature"] if selected else None,
            "primary_choreography_signature": selected["primary_choreography_signature"] if selected else None,
            "alternatives": alternatives, "capture": {"status": "pending"},
            "review": {"status": "pending"}}


def action_identity(root, martial, arte, character, key, rules):
    """Record actual named track content, not just member/variant ordinals."""
    if rules.get("action"):
        return {"kind": "authored", "path": rules["action"], "definition": read(root / rules["action"])}
    binding = martial["bindings"].get(str(arte), {}).get(str(character))
    if binding:
        member_name = martial["mappings"].get(str(binding["member"]))
        if member_name is None:
            return {"kind": "unresolved_martial", "key": key, "binding": binding}
        member = martial["techniques"][member_name]
        variant_name = member["mappings"][str(binding["variant"])]
        return {"kind": "martial", "variant_name": variant_name,
                "track": member["variants"].get(variant_name),
                "contacts": member.get("contacts", {}), "projectiles": member.get("projectiles", {})}
    # A spell's named identity is preserved when no martial track exists. Do
    # not equate unrelated spells merely because their casting rules match.
    return {"kind": "named_technique", "key": key, "rules": rules}


def candidates(root, techniques, unison, martial, catalogue_cases):
    ids = {key: int(number) for number, key in techniques["mappings"].items()}
    characters = {key: int(number) for number, key in techniques["characters"].items()}
    placement = {}
    for group in unison["placements"]:
        settings = {key: value for key, value in group.items() if key != "techniques"}
        for technique in group["techniques"]:
            key = technique if isinstance(technique, str) else technique["technique"]
            matches = technique if isinstance(technique, str) else technique["matches"]
            placement[key] = (matches, settings)
    scopes = {(case.get("character_name"), case.get("asset_key")): case
              for case in catalogue_cases if case["category"] == "party"}
    choices = defaultdict(list)
    for owner, keys in techniques["learning"].items():
        for key in keys:
            rules = techniques["techniques"][key].get("rules", {})
            # This is exactly Document::prepare's menu unison_usable rule.
            if key not in placement or not rules.get("capabilities", {}).get("offensive", False):
                continue
            matching_group, settings = placement[key]
            source = scopes[owner, key]
            action = ({"kind": "source_unimplemented_slot", "evidence": source["scope_evidence"]}
                      if source.get("scope_status") == "source_unimplemented_slot" else
                      action_identity(root, martial, ids[key], characters[owner], key, rules))
            choices[matching_group].append({
                "character": characters[owner], "character_name": owner, "arte": ids[key],
                "arte_key": key, "placement": settings,
                "scope_status": source.get("scope_status", "original_game_required"),
                "scope_evidence": source.get("scope_evidence"),
                "action_signature": fingerprint(action),
            })
    return choices


def build(catalogue, editions):
    if catalogue["cooked_layout_version"] != CURRENT_VERSION:
        raise ValueError("Unison planner requires a catalogue from the current cooked layout")
    requests, combinations = [], []
    for edition in editions:
        root = Path(catalogue["libraries"][edition])
        if read(root / "content.json") != {"game": "tales-of-symphonia", "version": CURRENT_VERSION}:
            raise ValueError(f"{root}: expected cooked layout {CURRENT_VERSION}")
        techniques = read(root / "game/techniques.json")
        unison = read(root / "battle/unison.json")
        martial = read(root / "battle/party/techniques.json")
        cases = [case for case in catalogue["cases"] if case["edition"] == edition]
        available = candidates(root, techniques, unison, martial, cases)
        for case in cases:
            if case["category"] != "unison":
                continue
            definition = unison["combinations"][case["binding"]["combination"] - 1]
            scene = read(root / "battle/scenes" / definition["scene"] / "scene.json")
            scene_content = {"combination": definition, "scene": scene, "host": unison["host"]}
            for member in ["rules", "effects"]:
                if isinstance(scene.get(member), str):
                    scene_content[member] = read(root / scene[member])
            scene_signature = fingerprint(scene_content)
            planned = []
            for variant in case["recipe_variants"]:
                row = plan_recipe(variant["id"], variant["binding"],
                    [available[key] for key in variant["arte_keys"]], scene_signature)
                row.update(edition=edition, parent_id=case["id"], display_name=case["display_name"],
                           arte_keys=variant["arte_keys"], scene=definition["scene"],
                           scene_signature=scene_signature,
                           scope_status=variant.get("scope_status", "original_game_required"),
                           scope_evidence=variant.get("scope_evidence"),
                           requires_visual_comparison=variant.get("requires_visual_comparison", True))
                planned.append(row["id"])
                requests.append(row)
            combinations.append({"id": case["id"], "edition": edition,
                "binding": case["binding"], "display_name": case["display_name"],
                "scene": definition["scene"], "recipe_case_ids": planned,
                "scope_status": case.get("scope_status", "original_game_required"),
                "scope_evidence": case.get("scope_evidence"),
                "requires_visual_comparison": case.get("requires_visual_comparison", True),
                "planning_status": "planned" if planned else "pending",
                "pending_reason": None if planned else "native_recipe_list_empty",
                "capture": {"status": "pending"}, "review": {"status": "pending"}})

    signatures, primary_signatures = defaultdict(list), defaultdict(list)
    for request in requests:
        if request["choreography_signature"]:
            signatures[request["choreography_signature"]].append(request["id"])
            primary_signatures[request["primary_choreography_signature"]].append(request["id"])
    for request in requests:
        request["evidence_association"] = {
            "signature": request["choreography_signature"],
            "candidate_recipe_cases": signatures.get(request["choreography_signature"], []),
            "candidate_primary_scene_cases": primary_signatures.get(request["primary_choreography_signature"], []),
            "status": "pending", "basis": "same ordered scene/body/action/placement signature",
            "limit": "Primary-scene associations omit ingredient actions/placement and cannot cover them. An unrun recipe is never marked captured or passed."}
    primary_groups = [{"signature": signature, "recipe_cases": members,
                       "capture": {"status": "pending"}, "review": {"status": "pending"},
                       "scope": "Primary finisher scene and ordered bodies only; ingredient execution remains separate."}
                      for signature, members in sorted(primary_signatures.items())]
    summary = {}
    for edition in editions:
        selected = [request for request in requests if request["edition"] == edition]
        summary[edition] = {"combinations": sum(row["edition"] == edition for row in combinations),
                            "recipe_variants": len(selected),
                            "planning_status": dict(Counter(row["planning_status"] for row in selected)),
                            "distinct_owner_assignment_variants": sum(len(row["alternatives"]) for row in selected),
                            "primary_scene_groups": len({row["primary_choreography_signature"] for row in selected
                                                         if row["primary_choreography_signature"]})}
    return {"schema_version": 1, "cooked_layout_version": CURRENT_VERSION,
            "libraries": catalogue["libraries"], "summary": summary,
            "combinations": combinations, "requests": requests, "primary_scene_groups": primary_groups}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalogue", required=True, type=Path)
    parser.add_argument("--edition", choices=["gamecube", "steam"])
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = build(read(args.catalogue), [args.edition] if args.edition else ["gamecube", "steam"])
    result["catalogue"] = str(args.catalogue.resolve())
    result["catalogue_sha256"] = hashlib.sha256(args.catalogue.read_bytes()).hexdigest()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
