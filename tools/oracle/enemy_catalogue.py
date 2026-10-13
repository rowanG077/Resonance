#!/usr/bin/env python3
"""Build enemy arte cases from authored packages and cooked metadata; no runtime."""
import argparse
from collections import Counter, defaultdict
from hashlib import sha256
import json
from pathlib import Path
import struct

ACTION_BYTES = 68
PACKAGE_HEADER_BYTES = 488
COMMON_SPELL_START = 200
DEFAULT_PROFILE = Path(__file__).with_name("cases") / "battle-formations.json"
DEFAULT_OUTPUT = Path(__file__).with_name("cases") / "enemy-artes.json"


def read(path):
    return json.loads(Path(path).read_text())


def pinned(path):
    path = Path(path)
    return {"path": str(path), "sha256": sha256(path.read_bytes()).hexdigest()}


def section(raw, field):
    start = struct.unpack_from(">H", raw, field)[0]
    offsets = [struct.unpack_from(">H", raw, i)[0] for i in range(4, 20, 2)]
    offsets += [struct.unpack_from(">I", raw, i)[0] for i in range(24, PACKAGE_HEADER_BYTES, 4)]
    if not PACKAGE_HEADER_BYTES <= start < len(raw):
        raise ValueError(f"invalid authored enemy section {field}")
    end = min((offset for offset in offsets if start < offset < len(raw)), default=len(raw))
    return start, raw[start:end]


def formation_fixtures(profile, formations, worlds):
    caller_path = Path(profile["authored_arenas"]["path"])
    if pinned(caller_path) != profile["authored_arenas"]:
        raise ValueError("authored formation caller manifest changed")
    arenas = {}
    for call in read(caller_path)["calls"]:
        if call["command"] not in ("StartBattle", "SpawnEnemyActor", "CreateEnemySource", "Unknown37"):
            continue
        basis = f"field {call['map_id']}, {call['command']} at scenario word {call['command_word']}"
        for identity in call["formations"]:
            arenas.setdefault(identity, (call["arena"], basis))
    for world_index, world in enumerate(worlds):
        terrain_arenas = worlds[min(world_index, 1)]["arena_by_terrain"]
        for area_index, area in enumerate(world["areas"]):
            for terrain, pools in enumerate(area["encounter_groups"] if area else []):
                for pool in pools:
                    for row in formations["pools"][pool]:
                        if row["weight"]:
                            arenas.setdefault(row["formation"], (terrain_arenas[terrain],
                                f"world {world_index}, area {area_index}, terrain {terrain}, pool {pool}"))
    by_enemy = defaultdict(list)
    for identity, record in enumerate(formations["records"]):
        arena, basis = arenas.get(identity, (profile["component_arena"], "declared component arena"))
        actor_enemies = [record["resources"][actor["resource"]]["enemy"] for actor in record["actors"]]
        for enemy in set(actor_enemies):
            by_enemy[enemy].append({"formation": identity, "arena": arena, "arena_basis": basis,
                                   "actor_count": len(actor_enemies),
                                   "target_count": actor_enemies.count(enemy)})
    return by_enemy


def build_catalogue(packages: Path, assets: Path, formations_manifest: Path) -> dict:
    """Preserve every enemy row; recipe grouping never substitutes for a binding test."""
    packages, assets, formations_manifest = map(Path, (packages, assets, formations_manifest))
    profile = read(formations_manifest)
    formation_path = assets / "battle/formations.json"
    world_path = assets / "worlds/world.json"
    formations = read(formation_path)
    fixtures = formation_fixtures(profile, formations, read(world_path)["encounters"]["worlds"])
    definitions = {int(path.parent.name): path for path in
                   sorted((assets / "battle/enemies").glob("[0-9][0-9][0-9]/definition.json"))}
    raw_paths = {int(path.stem): path for path in sorted(packages.glob("[0-9][0-9][0-9].bin"))}
    if not definitions or definitions.keys() != raw_paths.keys():
        raise ValueError("raw enemy packages and cooked definitions must have the same complete IDs")
    if set(fixtures) != definitions.keys():
        raise ValueError("enemy definitions and actual formation actors differ")
    enemies, cases, groups = [], [], {}
    for enemy, definition_path in definitions.items():
        raw_path = raw_paths[enemy]
        raw = raw_path.read_bytes()
        package_sha = sha256(raw).hexdigest()
        definition = read(definition_path)
        monster_path = assets / f"monsters/{enemy:03}.json"
        monster = read(monster_path)
        if len(raw) < PACKAGE_HEADER_BYTES or raw[:4] != b"em8\0":
            raise ValueError(f"invalid authored enemy package: {raw_path}")
        if package_sha != definition["source_sha256"] or monster["id"] != enemy:
            raise ValueError(f"enemy {enemy} metadata does not match its authored package")
        row_start, authored_rows = section(raw, 10)
        policy_offset, policy = section(raw, 12)
        rows = definition["actions"]["rows"]
        if len(authored_rows) != len(rows) * ACTION_BYTES or len(policy) < 26:
            raise ValueError(f"enemy {enemy} action or policy table is truncated")
        candidates = [{**fixture, "arena_cooked":
                       (assets / f"battle/stages/{fixture['arena']:03}.json").exists()}
                      for fixture in fixtures[enemy]]
        authored = [fixture for fixture in candidates if fixture["arena_cooked"] and
                    fixture["arena_basis"] != "declared component arena"]
        fixture = min(authored or candidates, key=lambda f: (f["actor_count"], f["target_count"], f["formation"]))
        if not authored:
            fixture = {**fixture, "arena": profile["component_arena"],
                       "arena_basis": "declared component arena; no available authored caller arena"}
        if not (assets / f"battle/stages/{fixture['arena']:03}.json").exists():
            raise ValueError(f"enemy {enemy} fixture arena is not cooked")
        enemies.append({"enemy": enemy, "name": monster["name"],
                        "package": {"path": str(raw_path), "sha256": package_sha},
                        "definition": pinned(definition_path), "monster": pinned(monster_path),
                        "policy_selector": policy[8], "policy_offset": policy_offset,
                        "policy": definition["actions"]["policy"],
                        "formations": [f["formation"] for f in fixtures[enemy]],
                        "formation_candidates": candidates,
                        "action_count": len(rows)})
        for index, row in enumerate(rows):
            data = authored_rows[index * ACTION_BYTES:(index + 1) * ACTION_BYTES]
            identity = f"enemy{enemy:03}:row{index:02}"
            native = struct.unpack_from(">H", data, 64)[0] or None
            kind = "common_spell" if native and native >= COMMON_SPELL_START else "enemy_sequence"
            # The complete package pins commands, animations, contacts and body
            # context. Include the complete row, even selection-only fields, so
            # grouping cannot erase an authored execution difference.
            signature = sha256(bytes.fromhex(package_sha) + data).hexdigest()
            group = f"spell{native:03}" if kind == "common_spell" else f"enemy{enemy:03}:{signature[:16]}"
            groups.setdefault(group, {"id": group, "kind": kind, "native": native,
                                      "enemy": None if kind == "common_spell" else enemy,
                                      "signature_sha256": None if kind == "common_spell" else signature,
                                      "bindings": []})["bindings"].append(identity)
            requirements = row["requirements"]
            execution = row.get("execution")
            cases.append({"id": identity, "enemy": enemy, "action_row": index,
                          "native": native, "kind": kind, "semantic_group": group,
                          **{key: fixture[key] for key in ("formation", "arena", "arena_basis")},
                          "row_offset": row_start + index * ACTION_BYTES,
                          "row_sha256": sha256(data).hexdigest(),
                          "animation_index": struct.unpack_from(">H", data, 24)[0],
                          "command_index": struct.unpack_from(">H", data, 26)[0],
                          "contact_index": struct.unpack_from(">H", data, 28)[0],
                          "duration": struct.unpack_from(">H", data, 14)[0],
                          "flags": struct.unpack_from(">I", data, 8)[0],
                          "target_selector": data[1], "target_state": struct.unpack_from(">H", data, 12)[0],
                          "weight": struct.unpack_from(">b", data)[0],
                          "requirements": requirements, "priority": requirements["priority"],
                          "followup": {"chain_group": row["chain_group"], "chance": row["followup_chance"],
                                       "only": requirements["followup_only"],
                                       "requires_hit": requirements["followup_requires_hit"]},
                          "cooked_execution": next(iter(execution)) if execution else None,
                          "unsupported_reason": row.get("unsupported_reason")})
    return {"version": 1, "game_id": "GQSEAF",
            "description": "Every authored enemy action binding. Generated metadata is not a runtime or fidelity receipt.",
            "inputs": {"state": profile["source"], "movie": profile["source_movie"],
                       "disc_sha256": profile["disc_sha256"], "packages": str(packages), "assets": str(assets),
                       "formations_manifest": pinned(formations_manifest), "formations": pinned(formation_path),
                       "worlds": pinned(world_path), "authored_arenas": profile["authored_arenas"]},
            "fixture_defaults": {"party_hp": profile["party_hp"], "warmup_vis": profile["warmup_vis"],
                                 "entry_gate_vis": 30, "entry_gate_attempts": 8, "capture_vis": 900,
                                 "selection": "controlled_authored_row"},
            "coverage": {"bindings": "Each case requires its own enemy and row identity proof, outcome and cleanup receipt.",
                         "selection": "Controlled row selection does not establish natural admission; requirements, priority and followup remain separate tests.",
                         "groups": "Shared spell groups cover core recipes only. Physical and noncommon groups retain enemy identity and full authored context.",
                         "metadata": "Requirements and unsupported reasons describe the pinned cooked files, not current runtime acceptance.",
                         "arena": "Prefer available authored caller arenas, then smallest actual-actor formation and fewer duplicate targets. Use an explicit component arena only when none is available; retain all candidates."},
            "summary": {"enemies": len(enemies), "cases": len(cases), "groups": len(groups),
                        "formations": len(formations["records"]), "kinds": dict(Counter(c["kind"] for c in cases)),
                        "common_spell_recipes": sum(g["kind"] == "common_spell" for g in groups.values())},
            "enemies": enemies, "groups": list(groups.values()), "cases": cases}


def write_catalogue(path, catalogue):
    # Keep each binding on one line, like the party arte manifest.
    header = {key: value for key, value in catalogue.items() if key not in ("enemies", "groups", "cases")}
    text = json.dumps(header, indent=2, ensure_ascii=False)[:-2]
    for key in ("enemies", "groups", "cases"):
        rows = ",\n".join("    " + json.dumps(row, ensure_ascii=False) for row in catalogue[key])
        text += f',\n  "{key}": [\n{rows}\n  ]'
    Path(path).write_text(text + "\n}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packages", type=Path, required=True)
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--formations-manifest", type=Path, default=DEFAULT_PROFILE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    catalogue = build_catalogue(args.packages, args.assets, args.formations_manifest)
    write_catalogue(args.output, catalogue)
    print(json.dumps({"output": str(args.output), **catalogue["summary"]}))


if __name__ == "__main__":
    main()
