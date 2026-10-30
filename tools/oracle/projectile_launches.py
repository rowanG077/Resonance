"""Bind authored randomized launches to retained Dolphin observations.

The first initialized sample has already completed one position/velocity update.
Its launch origin is reconstructed from that source sample, never a native run.
All observations and events are retained unchanged for the ballistic comparator.
"""
import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess

import arte_equivalence as capture
from battle_fixture import enemy_package
from named_content import Snapshot as ContentSnapshot, same_evidence


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
                and not response.get("ricochet_on_deflect")
                and response["restitution"] == 0.75
                and (definition.get("contact") or {}).get("survives_contact"))
    deflect_only = (response and not response["bounce"] and not response["ricochet"]
                    and response.get("ricochet_on_deflect")
                    and not response.get("ground_ricochet")
                    and response["restitution"] == 0.75)
    return (not definition.get("unsupported_reason")
            and (motion["origin"] == "actor" or isinstance(motion["origin"], dict)
                 and set(motion["origin"]) == {"bone"})
            and any(abs(value) >= 1 for value in motion["velocity_jitter"])
            and not any(motion.get(key) for key in (
                "bone_direction", "departure", "velocity_reset",
                "aim_at_target", "stop_on_ground", "aqua_edge", "contact_path",
                "speed", "steering"))
            and (ricochet or deflect_only or not response))


def derive(reference, assets, usual, packages=None):
    trace = (json.loads(subprocess.check_output(["zstd", "-dcq", str(reference)]))
             if reference.suffix == ".zst" else copy.deepcopy(capture.read(reference)))
    setup = trace["setup"]
    require(not trace["incomplete_observations"], "source trace is incomplete")
    require("projectile_launches" not in setup, "source already has launch descriptors")
    retained = setup["ready_state"].get("retained") is False
    for key in (("capture",) if retained else ("ready_state", "ready_movie", "capture")):
        require(capture.digest(setup[key]["path"]) == setup[key]["sha256"],
                f"{key} provenance changed")
    require(not retained or trace["case"].startswith("enemy:") and packages is not None,
            "discarded admission state requires an authored enemy action binding")
    observations = Path(setup["capture"]["path"]).with_name("observations.json")
    raw_available = observations.exists() or observations.with_suffix(".json.zst").exists()
    require(raw_available or retained, "live capture lacks raw projectile observations")
    raw_rows = capture.read_capture_observations(setup["capture"]["path"]) if raw_available else []
    state = None if retained else capture.State(setup["ready_state"]["path"])
    if state is not None:
        require(recorded_fields(capture.inspect(state, battle=True)["battle"], setup["initial_battle"])
                == setup["initial_battle"],
                "ready state differs from trace admission")
    memory = None if state is None else bytes(state.ram)
    evidence_directory = reference.parent / "content-evidence"
    original = usual.read_bytes()
    archive = list(capture.sections(original))
    templates = archive[7]
    metadata_path = assets / "battle/projectiles.json"
    content = ContentSnapshot(assets, evidence_directory)
    metadata = content.document("battle/projectiles.json",
        ("records", "resident_spells", "support_durations", "fire_ball"))
    template_inputs = {metadata_path: dict(kind="projectiles",
        content=content.evidence("battle/projectiles.json", original))}

    def effect_input(raw, path):
        bank = capture.effect_bank(raw, path, assets, evidence_directory)
        observed = setup.get("effect_inputs", {}).get(bank["entry"])
        require(observed is not None and same_evidence(observed, bank["evidence"]),
                f"effect input differs from the observed current content: {bank['entry']}")
        template_inputs[path] = dict(kind="effects", content=bank["evidence"])
        return bank
    address = None if memory is None else memory.find(templates)
    require(memory is None or address >= 0 and memory.find(templates, address + 1) == -1,
            "authored projectile table is not uniquely loaded")
    require(len(templates) == len(metadata["records"]) * 400,
            "projectile table length differs")
    banks, effect_metadata = {}, {}
    for bank, section, name in ((0, 2, "common"), (1, 3, "techniques")):
        raw = archive[section]
        path = assets / f"battle/effects/{name}/effects.json"
        signature = hashlib.sha256(raw).hexdigest()
        require(memory is None or memory.find(raw) >= 0,
                f"{name} effect table is not loaded from the authored bytes")
        evidence = effect_input(raw, path)
        banks[bank] = signature
        effect_metadata[signature] = (path, evidence["metadata"])
    tables = [(templates, metadata_path, metadata["records"],
               None if address is None else address + 0x80000000, banks)]
    sources = [capture.pin(usual)]
    action_projectiles = []
    action_template = None
    if trace["case"].startswith("enemy:") and packages is not None:
        enemy = int(trace["case"].split(":")[1])
        package_path = packages / f"{enemy:03}.bin"
        package = package_path.read_bytes()
        base = None if state is None else enemy_package(state, package)[0]
        content = ContentSnapshot(assets, evidence_directory)
        entry, definition, _ = content.enemy(enemy)
        path = assets / entry
        package_proofs = setup.get("detached_contacts", {}).get("enemy_packages", [])
        require(any(proof["enemy"] == enemy
                    and proof["package"]["sha256"] == hashlib.sha256(package).hexdigest()
                    for proof in package_proofs),
                "enemy package differs from independently captured evidence")
        evidence = content.evidence(entry, package)
        template_inputs[path] = dict(kind={"enemy": enemy}, content=evidence)
        action = definition["actions"]["rows"][int(trace["case"].split(":")[2])]
        action_template = dict(enemy=enemy, content=evidence, index=int(trace["case"].split(":")[2]))
        action_projectiles = [(clock, event["projectile"])
                             for clock, event in (action.get("execution") or {}).get("attack", {}).get("events", [])
                             if "projectile" in event]
        pointers = struct.unpack_from(">116I", package, 0x18)
        member = lambda start: package[start:min([p for p in pointers if p > start] + [len(package)])]
        start = struct.unpack_from(">I", package, 0x1c8)[0]
        if start:
            raw = member(start)
            require(memory is None or memory[(base & 0x1ffffff) + start:(base & 0x1ffffff) + start + len(raw)] == raw,
                    "enemy projectile table differs from loaded bytes")
            require(len(raw) // 400 == len(definition["projectiles"]), "enemy projectile table length differs")
            effect_path = assets / content.document(entry, ("rules", "artwork", "effects"))["effects"]
            effect_bytes = member(struct.unpack_from(">I", package, 0x1cc)[0])
            signature = hashlib.sha256(effect_bytes).hexdigest()
            require(memory is None or memory.find(effect_bytes) >= 0,
                    "enemy effect table differs from loaded bytes")
            effects = effect_input(effect_bytes, effect_path)
            effect_metadata[signature] = (effect_path, effects["metadata"])
            tables.append((raw, path, [record["definition"] for record in definition["projectiles"]],
                           None if base is None else base + start, {**banks, 2: signature}))
            sources.append(capture.pin(package_path))

    rows_by_clock = {}
    for row in raw_rows:
        rows_by_clock.setdefault(row["battle_combat_tick_word"], []).append(row)
    launches, witnesses, seen = [], [], set()
    for frame_index, frame in enumerate(trace["frames"]):
        for shot in frame["projectiles"]:
            if shot["id"] in seen:
                continue
            seen.add(shot["id"])
            clock = frame["combat_tick"] - trace["frames"][0]["combat_tick"]
            emissions = [emission for at, emission in action_projectiles if at == clock]
            authored = (emissions[0]["definition"] if shot["owner"] == setup["owner"]
                        and len(emissions) == 1 else None)
            attached = [program for program in frame["programs"]
                        if program["attachment"] == dict(kind="projectile", projectile=shot["id"])
                        and program["owner"] == shot["owner"]]
            if len(attached) > 1 or not attached and not action_projectiles:
                continue
            program = attached[0] if attached else None
            clock = frame["combat_tick"] - trace["frames"][0]["combat_tick"]
            births = [event for event in trace["events"]
                      if event["kind"] == "projectile_started" and event.get("projectile") == shot["id"]]
            require(len(births) == 1 and births[0]["clock"] == clock
                    and births[0]["owner"] == shot["owner"] and births[0]["target"] == shot["target"],
                    "projectile first sample lacks its unique attributed birth")
            require(sum(event["kind"] == "started" and event.get("owner") == shot["owner"]
                        and event.get("action") == births[0]["action"] and event["clock"] <= clock
                        for event in trace["events"]) == 1,
                    "projectile birth lacks a unique owned action")
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
            require(not raw_available or len(matches) == 1,
                    "initialized projectile sample is not independently unique")
            node, row, prefix = None, None, None
            if raw_available:
                node, (row, prefix) = next(iter(matches.items()))
            choices = []
            for data, path, definitions, base, bank_map in tables:
                for index, definition in enumerate(definitions):
                    raw = data[index * 400:(index + 1) * 400]
                    effect = (authored or definition)["birth_effect"]
                    if ((row is None or struct.unpack_from(">I", raw, 8)[0] == row[prefix + "30"])
                            and (effect["member"] == 0 if program is None else
                                 bank_map.get(effect["bank"]) == program["bank"]
                                 and effect["member"] == program["program"])):
                        choices.append((index, definition, raw, path, base))
            action_binding = None
            contact = None
            emitted = None
            if shot["owner"] == setup["owner"] and action_projectiles:
                clock = frame["combat_tick"] - trace["frames"][0]["combat_tick"]
                emissions = [emission for at, emission in action_projectiles if at == clock]
                if len(emissions) == 1:
                    emission = emissions[0]
                    emitted = emission["definition"]
                    if not supported(emitted):
                        continue
                    bound = []
                    for choice in choices:
                        declaration = copy.deepcopy(choice[1])
                        declaration["motion"]["origin"] = emitted["motion"]["origin"]
                        # These fields are the authored modifier operations
                        # prepared by battle_projectile::with_modifier. Bind
                        # their final values to the pinned action declaration.
                        for field in ("velocity", "acceleration", "spawn_offset"):
                            declaration[field] = emitted[field]
                        declaration["motion"]["velocity_jitter"] = emitted["motion"]["velocity_jitter"]
                        declaration["birth_effect"]["member"] = emitted["birth_effect"]["member"]
                        if declaration == emitted:
                            bound.append(choice)
                    # Prefer a declaration needing only the emission's origin
                    # binding. Otherwise modifier fields can make two distinct
                    # base rows appear identical after replacing their values.
                    direct = []
                    for choice in bound:
                        declaration = copy.deepcopy(choice[1])
                        declaration["motion"]["origin"] = emitted["motion"]["origin"]
                        if declaration == emitted:
                            direct.append(choice)
                    bound = direct or bound
                    require(len(bound) == 1, "owned action emission does not uniquely bind a projectile template")
                    choices = bound
                    action_binding = dict(clock=clock, origin=emitted["motion"]["origin"])
                    contact = dict(cooldown=emission["hit"]["cooldown"],
                                   repeat_limit=emitted["contact"]["repeat_limit"],
                                   active=emitted.get("active"))
            if retained:
                require(action_binding is not None,
                        "retained launch lacks a unique owned action emission")
            eligible = supported(emitted) if emitted is not None else any(supported(choice[1]) for choice in choices)
            if not eligible:
                continue
            require(len(choices) == 1, "projectile template identity is ambiguous")
            index, definition, raw, metadata_path, base = choices[0]
            if emitted is not None:
                definition = emitted
            require(supported(definition), "unsupported randomized projectile motion")
            if (definition["motion"].get("response") or {}).get("ricochet"):
                flags = struct.unpack_from(">I", raw, 8)[0]
                # Contact ricochets survive contact; facing is an independent
                # authored bit. Initial bounce/deflection is a different path.
                require(flags & 0x1008 == 0x1008 and not flags & 0x100020
                        and bool(flags & 0x200) == bool(definition["motion"].get("face_velocity"))
                        and struct.unpack_from(">f", raw, 0x40)[0] == 0.75
                        and (None if struct.unpack_from(">HH", raw, 0x84)[1] == 0 else
                             [struct.unpack_from(">HH", raw, 0x84)[0],
                              sum(struct.unpack_from(">HH", raw, 0x84))]) == definition.get("active"),
                        "ricochet response or active window differs from loaded template bytes")
                clock = frame["combat_tick"] - trace["frames"][0]["combat_tick"]
                require(action_binding is not None,
                        "ricochet launch lacks a unique owned action emission")
            table_definition = choices[0][1]
            for field, offset in (("velocity", 0x24), ("acceleration", 0x30),
                                  ("spawn_offset", 0x60)):
                # Cooked JSON uses shortest float32 decimals; authenticate the
                # binary values rather than Python's float64 decimal parsing.
                require(struct.pack(">3f", *table_definition[field]) == raw[offset:offset + 12],
                        f"{field} differs from authored template")
            require(struct.pack(">3f", *table_definition["motion"]["velocity_jitter"]) == raw[0x6c:0x78]
                    and definition["lifetime"] == struct.unpack_from(">h", raw, 0xc)[0]
                    and [table_definition["birth_effect"][key] for key in ("bank", "member")] == list(raw[0x5c:0x5e]),
                    "launch parameters differ from authored template")
            require(frame_index > 0 and shot["age"] == 1
                    and (row is None or row[prefix + "278"] >> 16 == 1),
                    "launch is not first observed immediately after one update")
            require(not definition["clamp_ground"] or shot["position"][1] >= 0.1,
                    "ground-clamped birth needs an unclamped source origin witness")
            actor = next(actor for actor in frame["actors"] if actor["actor"] == shot["owner"])
            prior = next(actor for actor in trace["frames"][frame_index - 1]["actors"]
                         if actor["actor"] == shot["owner"])
            require(actor["heading"] == prior["heading"], "owner changed heading during launch")
            source_slot = next(binding["source_slot"] for binding in setup["source_actor_map"]
                               if binding["actor"] == shot["owner"])
            actor_address = next(actor["address"] for actor in setup["initial_battle"]["actors"]
                                 if actor["slot"] == source_slot)
            require(row is None or row[prefix + "28"] == actor_address,
                    "projectile owner differs from raw observation")
            heading = 0 if definition["motion"].get("world_direction") else actor["heading"]
            acceleration = rotate(definition["acceleration"], heading)
            initial_velocity = [value - change for value, change in zip(shot["velocity"], acceleration)]
            origin = [position - velocity for position, velocity in zip(shot["position"], initial_velocity)]
            launches.append(dict(projectile=shot["id"],
                template=dict(**template_inputs[metadata_path], index=index),
                source_initial_age=shot["age"], origin=origin, heading=actor["heading"],
                survives_contact=bool((definition.get("contact") or {}).get("survives_contact")),
                contact=contact,
                authored_action=None if action_binding is None else dict(**action_template, clock=clock),
                **{field: definition[field] for field in (
                    "velocity", "acceleration", "motion", "clamp_ground", "lifetime")}))
            if definition["trail_effect"]["member"] and definition["trail_interval"]:
                bank_map = next(table[4] for table in tables if table[1] == metadata_path)
                bank = bank_map[definition["trail_effect"]["bank"]]
                path, _ = effect_metadata[bank]
                launches[-1]["trail"] = dict(bank=bank,
                    template=dict(**template_inputs[path], index=definition["trail_effect"]["member"]),
                    interval=definition["trail_interval"])
            if definition["ground_effect"]["member"]:
                bank_map = next(table[4] for table in tables if table[1] == metadata_path)
                bank = bank_map[definition["ground_effect"]["bank"]]
                path, _ = effect_metadata[bank]
                launches[-1]["ground"] = dict(bank=bank,
                    template=dict(**template_inputs[path], index=definition["ground_effect"]["member"]))
            witnesses.append(dict(projectile=shot["id"], frame=frame_index,
                combat_tick=frame["combat_tick"], vi_sample=None if row is None else row["vi_sample"], node=node,
                birth_program=None if program is None else program["id"],
                birth_bank=None if program is None else program["bank"],
                flags=None if row is None else row[prefix + "30"], source_position=shot["position"],
                source_velocity=shot["velocity"], source_initial_age=shot["age"],
                action_emission=action_binding,
                origin_method="source age-one position minus (source velocity minus authored acceleration)",
                template=dict(index=index, address=None if base is None else base + index * 400,
                              bytes=400, sha256=hashlib.sha256(raw).hexdigest())))
    if launches:
        setup["projectile_launches"] = launches
        admitted = {launch["projectile"] for launch in launches}
        following = {}
        for frame in trace["frames"]:
            for model in frame["effect_models"]:
                if (model.get("attachment") or {}).get("projectile") not in admitted:
                    continue
                path, bank = effect_metadata[model["bank"]]
                member = model["member"]
                definition = bank["actors"][member]
                template = definition["template"]
                require(definition["kind"] == "model_particle" and template["follow_origin"],
                        "randomized projectile model has unsupported authored attachment")
                following[(model["bank"], member)] = dict(bank=model["bank"], member=member,
                    template=dict(**template_inputs[path], index=member),
                    follow_origin=True, follow_orientation=template["follow_orientation"])
                launch = next(item for item in launches
                              if item["projectile"] == model["attachment"]["projectile"])
                if (launch["motion"].get("response") or {}).get("ricochet"):
                    state = template["state"]
                    require(not launch["motion"].get("face_velocity") or ((state["angles"] == [-90, 0, 0]
                             or state["angles"] == [0, 0, 0] and template["follow_orientation"])
                            and all(state[field] == [0, 0, 0] for field in (
                                "offset", "velocity", "acceleration", "angular_velocity", "orbit"))
                            and template["jerk"] == [0, 0, 0]
                            and template["orbit_velocity"] == [0, 0, 0]
                            and state["geometry"] == {"Size": dict(value=[1, 1, 1],
                                                                 velocity=[0, 0, 0], acceleration=[0, 0, 0])}),
                            "ricochet model has unsupported authored motion or scale")
                    binding = dict(bank=model["bank"], member=member,
                                   template=dict(**template_inputs[path], index=member))
                    require(launch.get("model", binding) == binding,
                            "ricochet projectile has multiple model bindings")
                    launch["model"] = binding
        require(all(not (launch["motion"].get("response") or {}).get("ricochet")
                    or "model" in launch for launch in launches),
                "ricochet projectile lacks its owned model")
        if following:
            setup["projectile_follow_models"] = list(following.values())
    provenance = dict(native_outputs_used=False,
        admission_state_retained=not retained,
        raw_observations_retained=raw_available,
        binding=("raw_node_and_authored_template" if raw_available else
                 "normalized_owned_birth_and_unique_authored_emission"),
        semantic_change="Register source-reconstructed launch origins and authored ballistic templates; retain every frame and event.",
        source=capture.pin(reference),
        observations=capture.observation_pin(observations) if raw_available else None,
        capture=setup["capture"], ready_state=setup["ready_state"], ready_movie=setup["ready_movie"],
        sources=sources, metadata=[template_inputs[table[1]] for table in tables],
        effects=[template_inputs[path] for path, _ in effect_metadata.values()],
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
