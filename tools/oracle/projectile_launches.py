"""Bind authored randomized launches to retained Dolphin observations.

The first initialized sample has already completed one position/velocity update.
Its launch origin is reconstructed from that source sample, never a native run.
All observations and events are retained unchanged for the ballistic comparator.
"""
import argparse
import copy
import hashlib
import math
from pathlib import Path
import struct

import arte_equivalence as capture
from battle_fixture import enemy_package


def require(condition, message):
    if not condition:
        raise ValueError(message)


def rotate(vector, heading):
    x, y, z = vector
    sine, cosine = math.sin(math.radians(heading)), math.cos(math.radians(heading))
    return [cosine * x + sine * z, y, cosine * z - sine * x]


def recorded_fields(actual, expected):
    """Compare retained fields when a newer inspector exposes extra fields."""
    if isinstance(expected, dict):
        return {key: recorded_fields(actual[key], value) for key, value in expected.items()}
    if isinstance(expected, list):
        require(len(actual) == len(expected), "inspected list length differs")
        return [recorded_fields(item, value) for item, value in zip(actual, expected)]
    return actual


def supported(definition):
    motion = definition["motion"]
    response = motion.get("response")
    ricochet = (response and not response["bounce"] and response["ricochet"]
                and response.get("ground_ricochet") and not response.get("ricochet_on_deflect")
                and response["restitution"] == 0.75 and motion.get("face_velocity")
                and definition["acceleration"] == [0, 0, 0]
                and definition.get("active") is None
                and (definition.get("contact") or {}).get("survives_contact"))
    return (not definition.get("unsupported_reason")
            and not definition["clamp_ground"]
            and (motion["origin"] == "actor" or ricochet and isinstance(motion["origin"], dict)
                 and set(motion["origin"]) == {"bone"})
            and any(abs(value) >= 1 for value in motion["velocity_jitter"])
            and not any(motion.get(key) for key in (
                "bone_direction", "departure", "velocity_reset",
                "aim_at_target", "stop_on_ground", "aqua_edge", "contact_path",
                "speed", "steering"))
            and (ricochet or not response and not motion.get("face_velocity")))


def derive(reference, assets, usual, packages=None):
    trace = copy.deepcopy(capture.read(reference))
    setup = trace["setup"]
    require(not trace["incomplete_observations"], "source trace is incomplete")
    require("projectile_launches" not in setup, "source already has launch descriptors")
    for key in ("ready_state", "ready_movie", "capture"):
        require(capture.digest(setup[key]["path"]) == setup[key]["sha256"],
                f"{key} provenance changed")
    receipt = capture.read(setup["capture"]["path"])
    observations = Path(setup["capture"]["path"]).with_name("observations.json")
    require(receipt["complete"] and len(receipt["samples"]) == 1
            and capture.digest(observations) == receipt["samples"][0]["observations_sha256"],
            "raw observation receipt differs")
    state = capture.State(setup["ready_state"]["path"])
    require(recorded_fields(capture.inspect(state, battle=True)["battle"], setup["initial_battle"])
            == setup["initial_battle"],
            "ready state differs from trace admission")
    memory = bytes(state.ram)
    archive = list(capture.sections(usual.read_bytes()))
    templates = archive[7]
    metadata_path = assets / "battle/projectiles.json"
    metadata = capture.read(metadata_path)
    require(metadata["source_sha256"] == capture.digest(usual),
            "projectile metadata belongs to another archive")
    address = memory.find(templates)
    require(address >= 0 and memory.find(templates, address + 1) == -1,
            "authored projectile table is not uniquely loaded")
    require(len(templates) == len(metadata["records"]) * 400,
            "projectile table length differs")
    banks, effect_metadata = {}, {}
    for bank, section, name in ((0, 2, "common"), (1, 3, "techniques")):
        raw = archive[section]
        path = assets / f"battle/effects/{name}.json"
        signature = hashlib.sha256(raw).hexdigest()
        require(capture.read(path)["source_sha256"] == signature and memory.find(raw) >= 0,
                f"{name} effect table is not loaded from the authored bytes")
        banks[bank] = signature
        effect_metadata[signature] = (path, capture.read(path))
    tables = [(templates, metadata_path, metadata["records"], address + 0x80000000, banks)]
    sources = [capture.pin(usual)]
    action_projectiles = []
    if trace["case"].startswith("enemy:") and packages is not None:
        enemy = int(trace["case"].split(":")[1])
        package_path = packages / f"{enemy:03}.bin"
        package = package_path.read_bytes()
        base, _ = enemy_package(state, package)
        path = assets / f"battle/enemies/{enemy:03}/definition.json"
        definition = capture.read(path)
        require(definition["source_sha256"] == capture.digest(package_path),
                "enemy metadata belongs to another package")
        action = definition["actions"]["rows"][int(trace["case"].split(":")[2])]
        action_projectiles = [(clock, event["projectile"]["definition"])
                             for clock, event in (action.get("execution") or {}).get("attack", {}).get("events", [])
                             if "projectile" in event]
        pointers = struct.unpack_from(">116I", package, 0x18)
        member = lambda start: package[start:min([p for p in pointers if p > start] + [len(package)])]
        start = struct.unpack_from(">I", package, 0x1c8)[0]
        if start:
            raw = member(start)
            require(memory[(base & 0x1ffffff) + start:(base & 0x1ffffff) + start + len(raw)] == raw,
                    "enemy projectile table differs from loaded bytes")
            require(len(raw) // 400 == len(definition["projectiles"]), "enemy projectile table length differs")
            effect_path = assets / f"battle/enemies/{enemy:03}/effects.json"
            effects = capture.read(effect_path)
            effect_bytes = member(struct.unpack_from(">I", package, 0x1cc)[0])
            signature = hashlib.sha256(effect_bytes).hexdigest()
            require(effects["source_sha256"] == signature and memory.find(effect_bytes) >= 0,
                    "enemy effect table differs from loaded bytes")
            effect_metadata[signature] = (effect_path, effects)
            tables.append((raw, path, [record["definition"] for record in definition["projectiles"]],
                           base + start, {**banks, 2: signature}))
            sources.append(capture.pin(package_path))

    rows_by_clock = {}
    for row in capture.read(observations):
        rows_by_clock.setdefault(row["battle_combat_tick_word"], []).append(row)
    launches, witnesses, seen = [], [], set()
    for frame_index, frame in enumerate(trace["frames"]):
        for shot in frame["projectiles"]:
            if shot["id"] in seen:
                continue
            seen.add(shot["id"])
            attached = [program for program in frame["programs"]
                        if program["attachment"] == dict(kind="projectile", projectile=shot["id"])
                        and program["owner"] == shot["owner"] and program["age"] == 0]
            if len(attached) != 1:
                continue
            program = attached[0]
            matches = {}
            for row in rows_by_clock.get(frame["combat_tick"], []):
                nodes, complete = capture.tracked_list(
                    row, "projectile", capture.observation_counts(row)["projectile"])
                require(complete, "raw projectile observation is incomplete")
                for index, node in nodes:
                    prefix = f"projectile_{index}_"
                    if (row[prefix + "14"] >> 24 == 1
                            and [capture.f32(row[prefix + f"{at:x}"]) for at in (0x40, 0x44, 0x48)] == shot["position"]
                            and [capture.f32(row[prefix + f"{at:x}"]) for at in (0x4c, 0x50, 0x54)] == shot["velocity"]):
                        matches[node] = (row, prefix)
            require(len(matches) == 1, "initialized projectile sample is not independently unique")
            node, (row, prefix) = next(iter(matches.items()))
            choices = []
            for data, path, definitions, base, bank_map in tables:
                for index, definition in enumerate(definitions):
                    raw = data[index * 400:(index + 1) * 400]
                    effect = definition["birth_effect"]
                    if (struct.unpack_from(">I", raw, 8)[0] == row[prefix + "30"]
                            and bank_map.get(effect["bank"]) == program["bank"]
                            and effect["member"] == program["program"]):
                        choices.append((index, definition, raw, path, base))
            if len(choices) > 1 and shot["owner"] == setup["owner"]:
                clock = frame["combat_tick"] - trace["frames"][0]["combat_tick"]
                emissions = [definition for at, definition in action_projectiles if at == clock]
                if len(emissions) == 1:
                    emitted = emissions[0]
                    bound = []
                    for choice in choices:
                        declaration = copy.deepcopy(choice[1])
                        declaration["motion"]["origin"] = emitted["motion"]["origin"]
                        if declaration == emitted:
                            bound.append(choice)
                    require(len(bound) == 1, "owned action emission does not uniquely bind a projectile template")
                    choices = bound
            if not any(supported(choice[1]) for choice in choices):
                continue
            require(len(choices) == 1, "projectile template identity is ambiguous")
            index, definition, raw, metadata_path, base = choices[0]
            require(supported(definition), "unsupported randomized projectile motion")
            action_binding = None
            if definition["motion"].get("response"):
                flags = struct.unpack_from(">I", raw, 8)[0]
                # Contact and ground ricochets face their velocity and survive
                # contact. Initial bounce/deflection response is a different path.
                require(flags & 0x9208 == 0x9208 and not flags & 0x100020
                        and struct.unpack_from(">f", raw, 0x40)[0] == 0.75
                        and struct.unpack_from(">HH", raw, 0x84) == (0, 0),
                        "ricochet response or active window differs from loaded template bytes")
                clock = frame["combat_tick"] - trace["frames"][0]["combat_tick"]
                candidates = [(at, event) for at, event in action_projectiles if at == clock]
                require(shot["owner"] == setup["owner"] and len(candidates) == 1,
                        "ricochet launch lacks a unique owned action emission")
                _, emitted = candidates[0]
                raw_definition = copy.deepcopy(emitted)
                raw_definition["motion"]["origin"] = definition["motion"]["origin"]
                require(raw_definition == definition and supported(emitted),
                        "action emission differs from the authenticated projectile template")
                definition = emitted
                action_binding = dict(clock=clock, origin=definition["motion"]["origin"])
            for field, offset in (("velocity", 0x24), ("acceleration", 0x30),
                                  ("spawn_offset", 0x60)):
                require(definition[field] == list(struct.unpack_from(">3f", raw, offset)),
                        f"{field} differs from authored template")
            require(definition["motion"]["velocity_jitter"] == list(struct.unpack_from(">3f", raw, 0x6c))
                    and definition["lifetime"] == struct.unpack_from(">h", raw, 0xc)[0]
                    and [definition["birth_effect"][key] for key in ("bank", "member")] == list(raw[0x5c:0x5e]),
                    "launch parameters differ from authored template")
            require(frame_index > 0 and shot["age"] == 1
                    and row[prefix + "278"] >> 16 == 1,
                    "launch is not first observed immediately after one update")
            actor = next(actor for actor in frame["actors"] if actor["actor"] == shot["owner"])
            prior = next(actor for actor in trace["frames"][frame_index - 1]["actors"]
                         if actor["actor"] == shot["owner"])
            require(actor["heading"] == prior["heading"], "owner changed heading during launch")
            source_slot = next(binding["source_slot"] for binding in setup["source_actor_map"]
                               if binding["actor"] == shot["owner"])
            actor_address = next(actor["address"] for actor in setup["initial_battle"]["actors"]
                                 if actor["slot"] == source_slot)
            require(row[prefix + "28"] == actor_address, "projectile owner differs from raw observation")
            heading = 0 if definition["motion"].get("world_direction") else actor["heading"]
            acceleration = rotate(definition["acceleration"], heading)
            initial_velocity = [value - change for value, change in zip(shot["velocity"], acceleration)]
            origin = [position - velocity for position, velocity in zip(shot["position"], initial_velocity)]
            launches.append(dict(projectile=shot["id"],
                template=dict(**capture.pin(metadata_path), index=index),
                source_initial_age=shot["age"], origin=origin, heading=actor["heading"],
                survives_contact=bool((definition.get("contact") or {}).get("survives_contact")),
                **{field: definition[field] for field in (
                    "velocity", "acceleration", "motion", "clamp_ground", "lifetime")}))
            witnesses.append(dict(projectile=shot["id"], frame=frame_index,
                combat_tick=frame["combat_tick"], vi_sample=row["vi_sample"], node=node,
                birth_program=program["id"], birth_bank=program["bank"],
                flags=row[prefix + "30"], source_position=shot["position"],
                source_velocity=shot["velocity"], source_initial_age=shot["age"],
                action_emission=action_binding,
                origin_method="source age-one position minus (source velocity minus authored acceleration)",
                template=dict(index=index, address=base + index * 400,
                              bytes=400, sha256=hashlib.sha256(raw).hexdigest())))
    if launches:
        setup["projectile_launches"] = launches
        admitted = {launch["projectile"] for launch in launches}
        following = {}
        for frame in trace["frames"]:
            for model in frame["effect_models"]:
                if model["attachment"].get("projectile") not in admitted:
                    continue
                path, bank = effect_metadata[model["bank"]]
                member = model["member"]
                definition = bank["actors"][member]
                template = definition["template"]
                require(definition["kind"] == "model_particle" and template["follow_origin"]
                        and definition["binding"]["animation"] is None,
                        "randomized projectile model has unsupported authored attachment")
                following[(model["bank"], member)] = dict(bank=model["bank"], member=member,
                    template=dict(**capture.pin(path), index=member),
                    follow_origin=True, follow_orientation=template["follow_orientation"])
                launch = next(item for item in launches
                              if item["projectile"] == model["attachment"]["projectile"])
                if launch["motion"].get("response"):
                    state = template["state"]
                    require(state["angles"] == [-90, 0, 0]
                            and all(state[field] == [0, 0, 0] for field in (
                                "offset", "velocity", "acceleration", "angular_velocity", "orbit"))
                            and template["jerk"] == [0, 0, 0]
                            and template["orbit_velocity"] == [0, 0, 0]
                            and state["geometry"] == {"Size": dict(value=[1, 1, 1],
                                                                 velocity=[0, 0, 0], acceleration=[0, 0, 0])},
                            "ricochet model has unsupported authored motion or scale")
                    binding = dict(bank=model["bank"], member=member,
                                   template=dict(**capture.pin(path), index=member))
                    require(launch.get("model", binding) == binding,
                            "ricochet projectile has multiple model bindings")
                    launch["model"] = binding
        require(all(not launch["motion"].get("response") or "model" in launch for launch in launches),
                "ricochet projectile lacks its owned model")
        if following:
            setup["projectile_follow_models"] = list(following.values())
    provenance = dict(native_outputs_used=False,
        semantic_change="Register source-reconstructed launch origins and authored ballistic templates; retain every frame and event.",
        source=capture.pin(reference), observations=capture.pin(observations),
        capture=setup["capture"], ready_state=setup["ready_state"], ready_movie=setup["ready_movie"],
        sources=sources, metadata=[capture.pin(table[1]) for table in tables],
        effects=[capture.pin(path) for path, _ in effect_metadata.values()],
        script=capture.pin(__file__), adapter=capture.pin(capture.__file__), witnesses=witnesses)
    return trace, provenance


def register(reference, assets, usual, packages):
    trace = capture.read(reference)
    if trace["incomplete_observations"] or not any(frame["projectiles"] for frame in trace["frames"]):
        return trace
    try:
        trace, provenance = derive(reference, assets, usual, packages)
    except (OSError, ValueError, KeyError, struct.error) as error:
        trace["incomplete_observations"].append(f"projectile launch registration unavailable: {error}")
        capture.write(reference, trace)
        return trace
    if trace["setup"].get("projectile_launches"):
        provenance["source_trace_sha256"] = provenance.pop("source")["sha256"]
        destination = reference.with_name("projectile-launches.json")
        capture.write(destination, provenance)
        trace["setup"]["projectile_launch_provenance"] = capture.pin(destination)
        capture.write(reference, trace)
    return trace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--assets", type=Path, default=capture.ROOT / "local/all-assets")
    parser.add_argument("--usual", type=Path,
                        default=capture.ROOT / "local/extracted/disc1/files/BTL/BTLusual.dat")
    parser.add_argument("--packages", type=Path,
                        default=capture.ROOT / "local/battle-actions-recovery-20261011/input")
    args = parser.parse_args()
    trace, provenance = derive(args.reference.resolve(), args.assets.resolve(), args.usual.resolve(),
                               args.packages.resolve())
    require(trace["setup"].get("projectile_launches"), "no uniquely bound supported randomized launches")
    args.output.mkdir(parents=True, exist_ok=False)
    destination = args.output / trace["case"].replace(":", "-") / "trace.json"
    destination.parent.mkdir()
    capture.write(destination, trace)
    provenance["output"] = capture.pin(destination)
    capture.write(args.output / "derivation.json", provenance)
    print(dict(output=str(destination), launches=len(trace["setup"]["projectile_launches"])))


if __name__ == "__main__":
    main()
