"""Bind current enemy/recoil documents to independently captured original bytes."""
import argparse
import hashlib
import json
from pathlib import Path

from named_content import Snapshot


def pin(path):
    path = Path(path).resolve(strict=True)
    return dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def original(proof):
    data = Path(proof["path"]).read_bytes()
    if hashlib.sha256(data).hexdigest() != proof["sha256"]:
        raise ValueError(f"original reaction input changed: {proof['path']}")
    return data


def register(trace, assets, usual, evidence_directory):
    setup = trace["setup"]
    if "reaction_recipients" in setup or "reaction_recoil" in setup:
        raise ValueError("reaction evidence is already registered")
    content = Snapshot(assets, evidence_directory)
    formations = content.document("battle/formations.json", ("records", "pools"))
    formation = formations["records"][setup["formation"]]
    enemies = [actor for actor in setup["initial_battle"]["actors"] if actor["side"] == 1]
    if len(enemies) != len(formation["actors"]):
        raise ValueError("reaction formation differs from observed enemy actors")
    packages = {entry["enemy"]: entry["package"]
                for entry in setup.get("detached_contacts", {}).get("enemy_packages", [])}
    actors = []
    for observed, spawn in zip(enemies, formation["actors"]):
        if observed["kind"] != spawn["resource"]:
            raise ValueError("reaction actor resource differs from its formation")
        enemy = formation["resources"][spawn["resource"]]["enemy"]
        mappings = [entry for entry in setup["source_actor_map"] if entry["source_slot"] == observed["slot"]]
        if len(mappings) != 1:
            raise ValueError("reaction actor has no unique source-slot binding")
        if enemy not in packages:
            # No normalization can be justified without original package evidence.
            # Leaving this recipient unqualified keeps its ordinary strict comparison.
            continue
        current = Snapshot(assets, evidence_directory)
        path, _, _ = current.enemy(enemy)
        actors.append(dict(actor=mappings[0]["actor"], enemy=enemy,
                           content=current.evidence(path, original(packages[enemy]))))
    setup["reaction_recipients"] = dict(
        formations=content.files["battle/formations.json"][1], actors=actors)
    module = Path(usual).parent.parent / "US_r_Top2Btl.rel"
    module_pin = pin(module)
    stored = setup.get("stored_scenes")
    if stored and stored["module"]["sha256"] != module_pin["sha256"]:
        raise ValueError("reaction module differs from stored scene evidence")
    recoil = Snapshot(assets, evidence_directory)
    recoil.document("battle/recoil.json", ("impulses", "suppression_distance", "guard_speed",
        "light_vertical_scale", "heavy_vertical_scale", "default_guard_recovery_bonuses"))
    setup["reaction_recoil"] = recoil.evidence("battle/recoil.json", original(module_pin))
    setup["reaction_recoil_module"] = module_pin


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--assets", required=True, type=Path)
    parser.add_argument("--usual", required=True, type=Path)
    args = parser.parse_args()
    trace = json.loads(args.reference.read_bytes())
    register(trace, args.assets, args.usual, args.output.parent / "content-evidence")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(trace, indent=2) + "\n")


if __name__ == "__main__":
    main()
