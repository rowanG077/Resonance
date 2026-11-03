#!/usr/bin/env python3
"""Enumerate visual work from cooked libraries; never infer visual acceptance."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import unicodedata


CURRENT_VERSION = int(re.search(r"pub const VERSION: u32 = (\d+);", (
    Path(__file__).resolve().parents[2] / "crates/content/src/library.rs").read_text())[1])


def read(path):
    return json.loads(Path(path).read_text())


def normalized(name):
    return " ".join(unicodedata.normalize("NFKC", name).casefold().split())


def named(name):
    return bool(name.strip()) and any(c.isalpha() for c in name)


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"))
                          .encode()).hexdigest()


def pending():
    return {"status": "pending"}


def enemy_technique_bindings(techniques, unison):
    """Mirror Unison::prepare then enemy::catalogue_id's ordered lookup.

    Enemy spell/physical IDs are native matching groups. A placement may map a
    differently named technique to that group; production chooses the first
    prepared placement in runtime catalogue-ID order, not the same numeric ID.
    """
    runtime_ids = {key: int(number) for number, key in techniques["mappings"].items()}
    native_ids = {key: int(number) for number, key in unison["mappings"].items()}
    placements = []
    for group in unison["placements"]:
        for member in group["techniques"]:
            key, matches = ((member, member) if isinstance(member, str) else
                            (member["technique"], member["matches"]))
            if key not in runtime_ids or matches not in native_ids:
                raise ValueError(f"unknown enemy Unison placement {member}")
            placements.append((runtime_ids[key], native_ids[matches], key))
    bindings = {}
    for runtime, native, key in sorted(placements):
        bindings.setdefault(native, {"native_id": native, "catalogue_id": runtime,
                                     "asset_key": key})
    return bindings


def case(edition, category, binding, label, key):
    return {"id": f"{edition}:{key}", "edition": edition, "category": category,
            "binding": binding, "display_name": label, "capture": pending(),
            "review": pending(), "original_gc_reference": {"status": "missing"}}


def visual_structure(value, rules, context=""):
    """Resolve local action references and omit audio and hit power only.

    Timing, poses, effect identities, projectile motion, contact shape, reaction,
    and support commands remain part of the fingerprint. This proposes a visual
    comparison candidate; it cannot establish matching original-game pixels.
    """
    if isinstance(value, list):
        if context == "events" and all(isinstance(v, list) and len(v) == 2
                                       and isinstance(v[1], dict) for v in value):
            events = [(at, visual_structure(command, rules)) for at, command in value]
            return [[at, command] for at, command in events if command]
        return [visual_structure(v, rules, context) for v in value]
    if not isinstance(value, dict):
        return value
    result = {}
    for key, member in value.items():
        if key in {"sound", "voice", "speech", "music", "audio"} or (context == "hit" and key == "power"):
            continue
        if isinstance(member, str) and key in {"projectile", "contact", "continue_contact"}:
            group = "projectiles" if key == "projectile" else "contacts"
            member = rules[group][member]
        elif key == "projectiles" and isinstance(member, list) and all(isinstance(v, str) for v in member):
            member = [rules["projectiles"][v] for v in member]
        result[key] = visual_structure(member, rules, key)
    return result


def enumerate_library(root, edition):
    root = Path(root)
    descriptor = read(root / "content.json")
    if descriptor != {"game": "tales-of-symphonia", "version": CURRENT_VERSION}:
        raise ValueError(f"{root}: expected Tales of Symphonia cooked layout {CURRENT_VERSION}, got {descriptor}")
    techniques = read(root / "game/techniques.json")
    ids = {key: int(number) for number, key in techniques["mappings"].items()}
    characters = {key: int(number) for number, key in techniques["characters"].items()}
    result = []
    for character, entries in techniques["learning"].items():
        for key in entries:
            arte = ids[key]
            definition = techniques["techniques"][key]
            name = definition.get("text", {}).get("name", "")
            rules = definition.get("rules", {})
            row = case(edition, "party", {"character": characters[character], "arte": arte},
                       name, f"party:{characters[character]}:{arte}")
            row.update(character_name=character, asset_key=key,
                       capabilities=rules.get("capabilities", {}),
                       action=rules.get("action"),
                       placeholder_name=not named(name))
            row["_semantic"] = (character, normalized(name)) if named(name) else None
            # Cooked keys can retain obsolete names when an ID is repurposed.
            # Only unchanged rules make a blank-name fallback conservative.
            row["_source"] = (character, key, fingerprint(rules))
            result.append(row)

    unison = read(root / "battle/unison.json")
    enemy_bindings = enemy_technique_bindings(techniques, unison)
    group_ids = {key: int(number) for number, key in unison["mappings"].items()}
    for number, combination in enumerate(unison["combinations"], 1):
        if "id" in combination or "native_id" in combination or "scene" not in combination:
            raise ValueError(f"{root}: expected current named Unison combination {number}")
        name = combination["name"]
        recipes = combination["recipes"]
        participants = combination.get("participants", len(recipes[0]) if recipes else None)
        if participants is None:
            raise ValueError(f"{root}: dormant Unison combination {number} has no participants")
        row = case(edition, "unison", {"combination": number}, name, f"unison:{number}")
        row.update(scene=combination["scene"], participants=participants,
                   recipe_variants=[{"id": f"{edition}:unison:{number}:recipe:{index}",
                                     "binding": {"combination": number, "recipe": index},
                                     "index": index, "arte_keys": recipe,
                                     "artes": [group_ids[key] for key in recipe],
                                     "capture": pending(), "review": pending()}
                                    for index, recipe in enumerate(recipes)])
        for variant in row["recipe_variants"]:
            identities = []
            for key in variant["arte_keys"]:
                definition = techniques["techniques"].get(key, {})
                caption = definition.get("text", {}).get("name", "")
                identities.append(("unresolved", edition, key) if not definition else
                                  ("name", normalized(caption)) if named(caption) else
                                  ("source", key, fingerprint(definition.get("rules", {}))))
            variant["_semantic"] = identities
        row["_semantic"] = (normalized(name), participants) if named(name) else None
        row["_source"] = None
        result.append(row)

    mappings = read(root / "battle/enemies/mappings.json")
    for number, key in sorted(mappings.items(), key=lambda item: int(item[0])):
        enemy = read(root / "battle/enemies" / key / "enemy.json")
        rules = enemy.get("rules", {})
        owner = re.sub(r"-\d+$", "", key)
        bank = read(root / enemy["effects"]) if enemy.get("effects") else {}
        effect_graph = visual_structure({k: bank[k] for k in ["programs", "templates"] if k in bank}, rules)
        for index, action in enumerate(rules.get("actions", {}).get("rows", [])):
            execution = action.get("execution")
            resolved = execution
            spell_name = ""
            technique_binding = None
            if isinstance(execution, dict) and "attack" in execution:
                resolved = {"attack": rules["attacks"][execution["attack"]]}
            elif isinstance(execution, dict) and "physical" in execution:
                physical = execution["physical"]
                technique_binding = enemy_bindings.get(physical["technique"])
                definition = techniques["techniques"].get(
                    technique_binding["asset_key"], {}) if technique_binding else {}
                resolved = {"physical": rules["attacks"][physical["attack"]],
                            "technique": definition} if definition else None
            elif isinstance(execution, dict) and "spell" in execution:
                technique_binding = enemy_bindings.get(execution["spell"])
                if technique_binding is not None:
                    spell_key = technique_binding["asset_key"]
                    spell = techniques["techniques"][spell_key]
                    spell_name = spell.get("text", {}).get("name", "")
                    resolved = {"spell": spell_key, "definition": spell}
                else:
                    # A missing native placement cannot borrow a technique at
                    # the same runtime number to pair unrelated enemy spells.
                    spell_name = f"spell {execution['spell']}"
                    resolved = None
            row = case(edition, "enemy", {"enemy": int(number), "row": index},
                       f"{owner}: {spell_name or 'row ' + str(index)}",
                       f"enemy:{number}:{index}")
            row.update(enemy_name=owner, asset_key=key, execution=execution,
                       native_technique_binding=technique_binding,
                       display_name_source="cooked_owner_key_and_native_technique_caption",
                       execution_identity_resolved=resolved is not None)
            # A row ordinal or 'attack-1' is not a cross-edition action identity.
            row["_semantic"] = None
            row["_source"] = (owner, fingerprint({"execution": visual_structure(resolved, rules),
                                                  "effect_graph": effect_graph})) if resolved is not None else None
            result.append(row)
    return result


def pair(rows):
    indices = {}
    for row in rows:
        for member in ["_semantic", "_source"]:
            identity = row[member]
            if identity is not None:
                indices.setdefault((row["edition"], row["category"], member, identity), []).append(row)
    for row in rows:
        other = "gamecube" if row["edition"] == "steam" else "steam"
        candidates = []
        basis = None
        for member, label in [("_semantic", "semantic_name"), ("_source", "visual_action_structure"
                               if row["category"] == "enemy" else "unchanged_source_identity")]:
            identity = row[member]
            if identity is None:
                continue
            found = indices.get((other, row["category"], member, identity), [])
            if member == "_source" and row["category"] == "party":
                found = [r for r in found if not named(row["display_name"])
                         or not named(r["display_name"])
                         or normalized(row["display_name"]) == normalized(r["display_name"])]
            if found:
                candidates = [r["id"] for r in found]
                basis = label
                break
        row["counterpart"] = {"status": "candidate" if len(candidates) == 1 else
                              "ambiguous" if candidates else "unmatched",
                              "candidates": candidates, "basis": basis}
        flags = []
        if not candidates:
            flags.append("no_semantic_counterpart")
            if row["edition"] == "steam":
                flags.append("steam_only_candidate")
        same_number = [r for r in rows if r["edition"] == other
                       and r["category"] == row["category"] and r["binding"] == row["binding"]]
        if row["category"] in ("party", "unison") and any(
                named(r["display_name"]) and named(row["display_name"])
                and normalized(r["display_name"]) != normalized(row["display_name"])
                for r in same_number):
            flags.append("numeric_binding_name_changed")
        row["flags"] = flags

    by_id = {row["id"]: row for row in rows}
    for row in rows:
        for variant in row.get("recipe_variants", []):
            candidates = [other["id"] for candidate in row["counterpart"]["candidates"]
                          for other in by_id[candidate]["recipe_variants"]
                          if variant["_semantic"] == other["_semantic"]]
            variant["counterpart"] = {
                "status": "candidate" if len(candidates) == 1 else "ambiguous" if candidates else "unmatched",
                "candidates": candidates, "basis": "semantic_recipe" if candidates else None}
            variant["flags"] = [] if candidates else ["no_semantic_counterpart"]
            if not candidates and row["edition"] == "steam":
                variant["flags"].append("steam_only_candidate")

    for row in rows:
        for variant in row.get("recipe_variants", []):
            del variant["_semantic"]


def annotate_scope(rows):
    """Attach individually proven scope facts without changing execution evidence."""
    evidence = read(Path(__file__).with_name("source_slots.json"))
    unison_path = Path(__file__).with_name("unison_source_exceptions.json")
    unison_evidence = read(unison_path)
    unison_digest = hashlib.sha256(unison_path.read_bytes()).hexdigest()

    def unison_exception(row, variant=None):
        for entry in unison_evidence["entries"]:
            selector = entry["selector"]
            if (row["edition"], row.get("display_name"), row.get("scene")) != (
                    selector["edition"], selector["name"], selector["scene"]):
                continue
            if selector.get("empty_recipes"):
                if variant is not None or row.get("recipe_variants"):
                    continue
            elif (variant is None or fingerprint({"keys": variant["arte_keys"],
                                                 "native_ids": variant["artes"]}) !=
                  fingerprint({"keys": selector["recipe_keys"],
                               "native_ids": selector["native_recipe_ids"]})):
                continue
            return {"document": "tools/arte-visuals/unison_source_exceptions.json",
                    "sha256": unison_digest, "entry": entry["id"]}
        return None

    for row in rows:
        row["scope_status"] = "original_game_required"
        for entry in evidence["entries"]:
            if all(row.get(key) == value for key, value in entry["selector"].items()):
                row["scope_status"] = entry["scope_status"]
                row["scope_evidence"] = {"document": "tools/arte-visuals/source_slots.json",
                                         "entry": entry["id"]}
                row["flags"].append("not_original_game_arte")
                break
        if row.get("category") == "unison":
            if proof := unison_exception(row):
                row.update(scope_status="source_unselectable", scope_evidence=proof)
                row["flags"].append("source_unselectable")
            for variant in row["recipe_variants"]:
                variant["scope_status"] = "original_game_required"
                if proof := unison_exception(row, variant):
                    variant.update(scope_status="source_unselectable", scope_evidence=proof)
                    variant["flags"].append("source_unselectable")
                variant["requires_visual_comparison"] = variant["scope_status"] != "source_unselectable"
        row["requires_visual_comparison"] = row["scope_status"] not in {
            "source_unimplemented_slot", "repository_authored", "source_unselectable"}


def build(gamecube, steam, legacy=None):
    rows = enumerate_library(gamecube, "gamecube") + enumerate_library(steam, "steam")
    pair(rows)
    annotate_scope(rows)
    legacy = legacy or {}
    old_cases = {f"party:{c['character']}:{c['arte']}": c for c in legacy.get("cases", [])}
    exclusions = set(legacy.get("excluded_comparisons", []))
    for row in rows:
        binding = row["id"].split(":", 1)[1]
        old = old_cases.get(binding, {})
        row["legacy_gc_manifest"] = {
            "same_numeric_binding_listed": binding in old_cases,
            "same_numeric_binding_excluded": binding in exclusions,
            "listed_name": old.get("name"),
            "listed_name_matches_current": named(old.get("name", "")) and
                normalized(old["name"]) == normalized(row["display_name"]),
            "pending_setup": old.get("pending_setup"),
            "applies_to_current_case": False,
        }
        del row["_semantic"], row["_source"]
    summary = {}
    for edition in ["gamecube", "steam"]:
        selected = [r for r in rows if r["edition"] == edition]
        summary[edition] = dict(Counter(r["category"] for r in selected))
        summary[edition]["unison_recipe_variants"] = sum(
            len(r.get("recipe_variants", [])) for r in selected)
        summary[edition]["counterparts"] = dict(Counter(r["counterpart"]["status"] for r in selected))
        summary[edition]["scope_status"] = dict(Counter(r["scope_status"] for r in selected))
        summary[edition]["party_scope_status"] = dict(Counter(
            r["scope_status"] for r in selected if r["category"] == "party"))
        summary[edition]["unison_recipe_scope_status"] = dict(Counter(
            variant["scope_status"] for row in selected for variant in row.get("recipe_variants", [])))
    return {"schema_version": 1, "cooked_layout_version": CURRENT_VERSION,
            "libraries": {"gamecube": str(Path(gamecube).resolve()), "steam": str(Path(steam).resolve())},
            "scope": "Catalogue candidates only; captures and visual review remain independent pending work.",
            "summary": summary, "cases": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gamecube", required=True, type=Path)
    parser.add_argument("--steam", required=True, type=Path)
    parser.add_argument("--legacy-manifest", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = build(args.gamecube, args.steam,
                   read(args.legacy_manifest) if args.legacy_manifest else None)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
