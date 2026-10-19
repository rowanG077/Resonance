"""Register source-bound random initial model angles without changing frames."""
import argparse
import copy
import hashlib
import math
from pathlib import Path
import struct

import arte_equivalence as capture
from projectile_launches import require, recorded_fields


def derive(reference, assets, usual):
    trace = copy.deepcopy(capture.read(reference))
    setup = trace["setup"]
    require(not trace["incomplete_observations"], "source observations are incomplete")
    require("effect_random_pitch" not in setup, "random pitch is already registered")
    for key in ("ready_state", "ready_movie", "capture"):
        require(capture.digest(setup[key]["path"]) == setup[key]["sha256"], f"changed {key}")
    observations = Path(setup["capture"]["path"]).with_name("observations.json")
    receipt = capture.read(setup["capture"]["path"])
    require(receipt["complete"] and len(receipt["samples"]) == 1
            and capture.digest(observations) == receipt["samples"][0]["observations_sha256"],
            "raw observation receipt differs")
    state = capture.State(setup["ready_state"]["path"])
    require(recorded_fields(capture.inspect(state, battle=True)["battle"], setup["initial_battle"])
            == setup["initial_battle"], "ready state differs")
    banks = capture.effect_banks(state, assets, None, [], usual)
    archive = list(capture.sections(usual.read_bytes()))
    raw_banks = {hashlib.sha256(archive[index]).hexdigest(): archive[index] for index in (2, 3)}
    rows = {row["battle_combat_tick_word"]: row for row in capture.read(observations)}
    seen, descriptors, witnesses = set(), [], []
    for frame_index, frame in enumerate(trace["frames"]):
        for model in frame["effect_models"]:
            identity = tuple(model[key] for key in ("owner", "bank", "member", "ordinal"))
            if identity in seen:
                continue
            seen.add(identity)
            if model["bank"] not in raw_banks:
                continue
            bank_matches = [(address, bank) for address, bank in banks.items()
                            if bank["source_sha256"] == model["bank"]]
            require(len(bank_matches) == 1, "model bank is not uniquely loaded")
            address, bank = bank_matches[0]
            programs = [program for program in frame["programs"]
                        if program["owner"] == model["owner"] and program["bank"] == model["bank"]
                        and program["attachment"] == model["attachment"] and program["age"] == 0]
            if len(programs) != 1:
                continue
            program = programs[0]["program"]
            raw = raw_banks[model["bank"]]
            cursor = bank["roots"][program]
            modifiers = []
            while True:
                age, member, _, operand = struct.unpack_from(">hBBH", raw, cursor)
                if member == 254:
                    break
                if age == 0 and member == model["member"] and operand:
                    words = list(struct.unpack_from(">5H", raw, operand))
                    if words[:2] == [11, 0x58] and words[3:] == [0, 65535]:
                        modifiers.append((operand, words))
                cursor += 6
            if not modifiers:
                continue
            require(len(modifiers) == 1, "model birth has ambiguous random pitch modifiers")
            offset, words = modifiers[0]
            divisor = struct.unpack(">h", struct.pack(">H", words[2]))[0]
            require(1 < abs(divisor) <= 1800, "pitch spans multiple matrix turns")
            declaration = bank["metadata"]["actors"][model["member"]]
            template = declaration["template"]
            initial = template["state"]
            geometry = initial["geometry"].get("Size", {})
            scale = geometry.get("value", [])
            supported = (declaration["kind"] == "model_particle"
                    and declaration["binding"]["animation"] is None
                    and not template.get("follow_orientation") and not template.get("fixed_orientation")
                    and initial["angles"] == [0, 0, 0] and initial["angular_velocity"][1:] == [0, 0]
                    and len(scale) == 3 and scale[0] > 0 and scale == [scale[0]] * 3
                    and geometry["velocity"] == [0, 0, 0] and geometry["acceleration"] == [0, 0, 0])
            if not supported:
                continue
            emissions = [event["operation"] for event in bank["metadata"]["programs"][program]
                         if event["at"] == 0 and event["operation"].get("particle") == model["member"]]
            require(len(emissions) == 1 and set(emissions[0]["birth"]) == {"random_draws", "angles"}
                    and emissions[0]["birth"]["random_draws"] == 1
                    and emissions[0]["birth"]["angles"][1:] == [None, None],
                    "model birth has additional edits")
            row = rows[frame["combat_tick"]]
            samples = []
            for label in ("particle", "alternate"):
                count = capture.observation_counts(row)[label]
                if not count:
                    continue
                nodes, complete = capture.tracked_list(row, label, count)
                require(complete, "particle birth observation is incomplete")
                for index, node in nodes:
                    prefix = f"{label}_{index}_"
                    get = lambda at: row[prefix + f"{at:x}"]
                    copied = b"".join(struct.pack(">I", get(at)) for at in range(0x28, 0x40, 4))
                    if (row[prefix + "parent_bank"] != address
                            or row[prefix + "parent_program"] != address + bank["roots"][program]
                            or capture.declaration_matches(bank, copied, bank["roots"][program]) != [model["member"]]
                            or not capture.particle_readiness(get(0x10), copied)):
                        continue
                    samples.append((node, get(0x278) >> 16,
                                    [capture.f32(get(at)) for at in (0x80, 0x84, 0x88)]))
            require(len(samples) == 1 and samples[0][1] == 1, "random model was not observed at age one")
            node, age, angles = samples[0]
            actor = next(actor for actor in frame["actors"] if actor["actor"] == model["owner"])
            require(angles[1:] == [actor["heading"], 0.], "model yaw or roll differs from authored placement")
            descriptor = dict(zip(("owner", "bank", "member", "ordinal"), identity),
                birth_clock=frame["combat_tick"] - trace["frames"][0]["combat_tick"],
                heading=actor["heading"], first_step=initial["angular_velocity"][0],
                template=dict(**capture.pin(usual), member=model["member"], program=program,
                              modifier_words=words))
            descriptors.append(descriptor)
            witnesses.append(dict(frame=frame_index, node=node, age=age, angles=angles,
                bank_address=address, bank_sha256=model["bank"], modifier_offset=offset,
                modifier_sha256=hashlib.sha256(raw[offset:offset + 10]).hexdigest(),
                metadata=capture.pin(bank["path"])))
    if descriptors:
        setup["effect_random_pitch"] = descriptors
    proof = dict(native_outputs_used=False, source=capture.pin(reference),
        observations=capture.pin(observations), capture=setup["capture"],
        ready_state=setup["ready_state"], ready_movie=setup["ready_movie"], archive=capture.pin(usual),
        script=capture.pin(__file__), adapter=capture.pin(capture.__file__), witnesses=witnesses)
    return trace, proof


def register(reference, assets, usual):
    trace = capture.read(reference)
    if trace["incomplete_observations"] or not any(frame["effect_models"] for frame in trace["frames"]):
        return trace
    try:
        trace, proof = derive(reference, assets, usual)
    except (OSError, ValueError, KeyError, struct.error) as error:
        trace["incomplete_observations"].append(f"random pitch registration unavailable: {error}")
        capture.write(reference, trace)
        return trace
    if trace["setup"].get("effect_random_pitch"):
        proof["source_trace_sha256"] = proof.pop("source")["sha256"]
        destination = reference.with_name("effect-random-pitch.json")
        capture.write(destination, proof)
        trace["setup"]["effect_random_pitch_provenance"] = capture.pin(destination)
        capture.write(reference, trace)
    return trace


def yaw_records(rows, setup, banks):
    """Replay the adapter's node generations, including pending model allocations."""
    previous_nodes, previous_age, identities, allocations = set(), {}, {}, {}
    previous_programs, particle_serial, program_serial, records = {}, 0, 0, []
    for row_index, row in enumerate(rows):
        counts = capture.observation_counts(row)
        nodes = {}
        for label in ("particle", "program", "alternate"):
            nodes[label], complete = capture.tracked_list(row, label, counts[label],
                setup.get("observer", {}).get("particle_addresses")) if counts[label] else ([], True)
            require(complete, f"incomplete {label} list at source row {row_index}")
        program_nodes = [(f"program_{i}", node) for i, node in nodes["program"]]
        program_nodes += [(f"alternate_{i}", node) for i, node in nodes["alternate"]
                         if row[f"alternate_{i}_10"] >> 24 == 0]
        programs = {}
        for prefix, node in program_nodes:
            if row[prefix + "_10"] >> 24 != 0:
                continue
            base, root = row[prefix + "_60"], row[prefix + "_6c"]
            bank = banks.get(base)
            if bank is None or root - base not in bank["roots"]:
                continue
            signature = (base, root, row[prefix + "_2c"])
            age = capture.signed16(row[prefix + "_5c"] & 65535)
            origin = tuple(row[prefix + f"_{at:x}"] for at in (0x48, 0x4c, 0x50))
            attachment = row.get(prefix + "_44")
            prior = previous_programs.get(node)
            relocated = (prior is not None and age == prior["age"] == 0
                         and attachment == prior["attachment"] == 0 and origin != prior["origin"])
            if prior is None or prior["signature"] != signature or age < prior["age"] or relocated:
                program_serial += 1
                pid, first_row = program_serial, row_index
            else:
                pid, first_row = prior["id"], prior["first_row"]
            programs[node] = dict(id=pid, first_row=first_row, signature=signature,
                                  age=age, origin=origin, attachment=attachment)
        previous_programs = programs
        particle_nodes = [(f"particle_{i}", node) for i, node in nodes["particle"]]
        particle_nodes += [(f"alternate_{i}", node) for i, node in nodes["alternate"]
                          if row[f"alternate_{i}_10"] >> 24 == 1]
        present, particles = set(), {}
        for prefix, node in particle_nodes:
            if row[prefix + "_10"] >> 24 != 1:
                continue
            copied = b"".join(struct.pack(">I", row[prefix + f"_{at:x}"])
                              for at in range(0x28, 0x40, 4))
            initialized = capture.particle_readiness(row[prefix + "_10"], copied)
            age = capture.signed16(row[prefix + "_278"] >> 16) if initialized else 0
            if node not in previous_nodes or age < previous_age.get(node, age):
                particle_serial += 1
                identities[node], allocations[node] = particle_serial, row_index
            previous_age[node] = age
            present.add(node)
            particles[identities[node]] = dict(node=node, prefix=prefix, age=age,
                initialized=initialized, first_row=allocations[node], copied=copied)
        previous_nodes = present
        records.append((particles, programs))
    return records


def yaw_template(declaration):
    """The comparator only admits world yaw over this fixed, static model basis."""
    template = declaration.get("template", {})
    initial = template.get("state", {})
    geometry = initial.get("geometry", {}).get("Size", {})
    uniform = lambda values: (len(values) == 3 and all(math.isfinite(v) for v in values)
                              and values == [values[0]] * 3)
    scale = geometry.get("value", [])
    velocity, acceleration = geometry.get("velocity", []), geometry.get("acceleration", [])
    lifetime = template.get("lifetime", 0)
    return (declaration.get("kind") == "model_particle"
            and declaration["binding"]["animation"] is None
            and not template.get("follow_origin") and not template.get("follow_orientation")
            and not template.get("fixed_orientation") and isinstance(lifetime, int) and lifetime > 0
            and initial.get("angles") == [-90, 0, 0]
            and len(initial.get("angular_velocity", [])) == 3
            and initial["angular_velocity"][::2] == [0, 0]
            and math.isfinite(initial["angular_velocity"][1])
            and uniform(scale) and uniform(velocity) and uniform(acceleration)
            and template.get("geometry_acceleration_until") is None
            and all(scale[0] + age * velocity[0] + age * (age - 1) / 2 * acceleration[0] > 0
                    for age in range(lifetime + 1)))


def derive_yaw(reference, assets, usual):
    trace = copy.deepcopy(capture.read(reference))
    setup = trace["setup"]
    require(not trace["incomplete_observations"], "source observations are incomplete")
    require("effect_random_yaw" not in setup, "random yaw is already registered")
    for key in ("ready_state", "ready_movie", "capture"):
        require(capture.digest(setup[key]["path"]) == setup[key]["sha256"], f"changed {key}")
    observations = Path(setup["capture"]["path"]).with_name("observations.json")
    receipt = capture.read(setup["capture"]["path"])
    require(receipt["complete"] and len(receipt["samples"]) == 1
            and capture.digest(observations) == receipt["samples"][0]["observations_sha256"],
            "raw observation receipt differs")
    state = capture.State(setup["ready_state"]["path"])
    require(recorded_fields(capture.inspect(state, battle=True)["battle"], setup["initial_battle"])
            == setup["initial_battle"], "ready state differs")
    rows = capture.read(observations)
    banks = capture.effect_banks(state, assets, None, [], usual)
    archive = list(capture.sections(usual.read_bytes()))
    raw_banks = {hashlib.sha256(archive[index]).hexdigest(): (archive[index], usual)
                 for index in (2, 3)}
    stored = setup.get("stored_scenes")
    verified_stored = None
    if stored:
        for key in ("module", "archive", "terminal"):
            require(capture.digest(stored[key]["path"]) == stored[key]["sha256"],
                    f"changed stored scene {key}")
        natives = {entry["native"] for entry in stored["registrations"]}
        require(len(natives) == 1, "stored scene has ambiguous physical identity")
        verified_setup = copy.deepcopy(setup)
        banks.update(capture.stored_effect_banks(dict(native=next(iter(natives))), rows,
            verified_setup, stored["terminal"]["path"], assets, usual))
        verified_stored = verified_setup["stored_scenes"]
        # Metadata may have been decoded again since capture. Rebind it to the
        # same original package and retained RAM; never alter the source setup.
        stable = lambda records: [{k: v for k, v in entry.items() if k not in ("scene", "effects")}
                                  for entry in records]
        require(stable(stored["registrations"]) == stable(verified_stored["registrations"]),
                "stored scene raw-byte registration differs")
        for entry in verified_stored["registrations"]:
            path = Path(verified_stored["archive"]["path"])
            with path.open("rb") as stream:
                start, end = entry["package_range"]
                stream.seek(start)
                package = stream.read(end - start)
            pointers = struct.unpack_from(">68I", package, 4)
            member = pointers[0]
            raw = package[member:min([v for v in pointers if v > member] + [len(package)])]
            require(hashlib.sha256(raw).hexdigest() == entry["bank_sha256"], "stored bank changed")
            raw_banks[entry["bank_sha256"]] = (raw, path)
    clocks = {}
    for index, row in enumerate(rows):
        key = (row["battle_combat_tick_word"], row["battle_presentation_tick_word"])
        require(key not in clocks, "raw source clocks are ambiguous")
        clocks[key] = index
    records = yaw_records(rows, setup, banks)
    owners = {actor["address"]: index for index, actor in enumerate(setup["initial_battle"]["actors"])}
    seen, bound_particles, descriptors, witnesses = set(), set(), [], []
    start = trace["frames"][0]["combat_tick"]
    for frame_index, frame in enumerate(trace["frames"]):
        row_index = clocks[(frame["combat_tick"], frame["presentation_tick"])]
        row = rows[row_index]
        for model in frame["effect_models"]:
            identity = tuple(model[key] for key in ("owner", "bank", "member", "ordinal"))
            if identity in seen:
                continue
            seen.add(identity)
            matches = [(address, bank) for address, bank in banks.items()
                       if bank["source_sha256"] == model["bank"]]
            if not matches or model["bank"] not in raw_banks:
                continue
            require(len(matches) == 1, "yaw model bank is not uniquely loaded")
            address, bank = matches[0]
            declaration = bank["metadata"]["actors"][model["member"]]
            if not yaw_template(declaration):
                continue
            declaration_bytes = bank["declarations"][model["member"]]
            template = declaration["template"]
            initial = template["state"]
            geometry = initial["geometry"]["Size"]
            flags = struct.unpack_from(">I", declaration_bytes, 20)[0]
            require(declaration_bytes[0] == 3 and not flags & (0x600204a4)
                    and struct.unpack_from(">h", declaration_bytes, 16)[0] + 1 == template["lifetime"]
                    and struct.unpack_from(">h", declaration_bytes, 136)[0] == 0
                    and all(struct.pack(">3f", *values) == declaration_bytes[at:at + 12]
                            for at, values in ((88, initial["angles"]), (100, initial["angular_velocity"]),
                                (176, geometry["value"]), (188, geometry["velocity"]),
                                (200, geometry["acceleration"]))),
                    "yaw template differs from its original static model declaration bytes")
            particles = [particle for particle in frame["particles"]
                         if all(particle[key] == model[key] for key in ("owner", "bank", "member", "model"))]
            require(len(particles) == 1, "model ordinal lacks a unique exact source particle")
            particle = particles[0]
            pid = particle["id"]
            sample = records[row_index][0][pid]
            prefix = sample["prefix"]
            get = lambda at: row[prefix + f"_{at:x}"]
            require(sample["initialized"] and sample["age"] == 1 and particle["initialized"],
                    "yaw model was not observed at age one")
            root = row[prefix + "_parent_program"]
            signature = (row[prefix + "_parent_bank"], root, row[prefix + "_parent_owner"])
            require(signature[0] == address and owners.get(signature[2]) == model["owner"]
                    and capture.declaration_matches(bank, sample["copied"], root - address,
                        model_binding=row.get(prefix + "_fc"), initialized=True, age=1) == [model["member"]],
                    "yaw model raw declaration or owner differs")
            require(model["attachment"] is None and not get(0x3c) & 0x600204a4
                    and model["model"]["clip"] is None and model["model"]["frame"] is None,
                    "yaw model has unsupported attachment or orientation")
            origin = [capture.f32(get(at)) for at in (0x170, 0x174, 0x178)]
            offset = [capture.f32(get(at)) for at in (0x5c, 0x60, 0x64)]
            angles = [capture.f32(get(at)) for at in (0x80, 0x84, 0x88)]
            scale = [capture.f32(get(at)) for at in (0xd8, 0xdc, 0xe0)]
            require(origin == particle["origin"] and offset == particle["offset"]
                    and capture.static_model_world(origin, offset, angles, scale,
                        get(0x3c), capture.f32(get(0x16c))) == model["model"]["world"],
                    "yaw source node does not reproduce its exact normalized model")
            allocation_index = sample["first_row"]
            allocation = records[allocation_index][0][pid]
            allocation_row = rows[allocation_index]
            allocation_prefix = allocation["prefix"]
            parent_node = allocation_row[allocation_prefix + "_184"] - 0x28
            parent = records[allocation_index][1].get(parent_node)
            require(parent is not None and parent["signature"] == signature,
                    "yaw allocation lacks its exact live parent generation")
            require(row_index - allocation_index in (0, 1)
                    and (row_index == allocation_index or not allocation["initialized"]),
                    "yaw allocation initialization was delayed or incompletely observed")
            program = bank["roots"].index(root - address)
            raw, archive_path = raw_banks[model["bank"]]
            emissions, cursor, command_age = [], bank["roots"][program], 0
            while True:
                age, member, flags, operand = struct.unpack_from(">hBBH", raw, cursor)
                command_age = max(command_age, age, 0)
                if member == 254:
                    break
                repeat_offset, count, interval = None, 1, 0
                if member == 255:
                    repeat_offset, count, interval = cursor, max(flags, 1), max(capture.signed16(operand), 0)
                    cursor += 6
                    _, member, flags, operand = struct.unpack_from(">hBBH", raw, cursor)
                    require(member < 254, "unsupported nested or terminal repeat")
                if member == model["member"]:
                    for repeat in range(count):
                        emission_age = command_age + repeat * interval
                        if emission_age <= parent["age"] <= emission_age + 1:
                            emissions.append((cursor, emission_age, flags, operand, repeat_offset, repeat))
                cursor += 6
            require(len(emissions) == 1, "yaw birth lacks a unique observed authored emission")
            command_offset, emission_age, flags, modifier_offset, repeat_offset, repeat = emissions[0]
            require(modifier_offset != 0, "yaw birth lacks its scalar modifier")
            words = list(struct.unpack_from(">5H", raw, modifier_offset))
            require(words == [11, 92, 1800, 0, 65535], "yaw birth has an unsupported modifier chain")
            authored = [event["operation"] for event in bank["metadata"]["programs"][program]
                        if event["at"] == emission_age and event["operation"].get("particle") == model["member"]]
            require(len(authored) == 1 and set(authored[0]["birth"]) == {"random_draws", "angles"}
                    and authored[0]["birth"]["random_draws"] == 1
                    and authored[0]["birth"]["angles"][::2] == [None, None],
                    "yaw birth metadata contains additional edits")
            first_step = template["state"]["angular_velocity"][1]
            initial_yaw = (angles[1] - first_step + 180) % 360 - 180
            require(angles[::2] == [-90, 0] and scale[0] > 0 and scale == [scale[0]] * 3
                    and abs(initial_yaw) <= 179.901
                    and abs(initial_yaw * 10 - round(initial_yaw * 10)) <= 0.01,
                    "yaw source birth differs from its authored tenth-degree choices")
            program_clock = rows[parent["first_row"]]["battle_combat_tick_word"] - start
            require(program_clock >= 0 and any(event["kind"] == "program_started"
                    and event["owner"] == model["owner"] and event["bank"] == model["bank"]
                    and event["program"] == program and event["clock"] == program_clock
                    for event in trace["events"]), "yaw parent start lacks its compared source event")
            require(pid not in bound_particles, "source particle has duplicate yaw descriptors")
            bound_particles.add(pid)
            descriptors.append(dict(zip(("owner", "bank", "member", "ordinal"), identity),
                birth_clock=frame["combat_tick"] - start, program_clock=program_clock,
                first_step=first_step, template=dict(**capture.pin(archive_path),
                    member=model["member"], program=program, modifier_words=words)))
            witnesses.append(dict(identity=list(identity), particle=pid, node=sample["node"],
                frame=frame_index, row=row_index, age=sample["age"], allocation_row=allocation_index,
                parent_node=parent_node, parent_generation=parent["id"], parent_age=parent["age"],
                program_clock=program_clock, command_offset=command_offset, emission_age=emission_age,
                command_flags=flags, repeat_offset=repeat_offset, repeat_index=repeat,
                modifier_offset=modifier_offset,
                modifier_sha256=hashlib.sha256(raw[modifier_offset:modifier_offset + 10]).hexdigest(),
                declaration_sha256=hashlib.sha256(declaration_bytes).hexdigest(),
                bank_address=address, bank_sha256=model["bank"], angles=angles, scale=scale,
                template=template, metadata=capture.pin(bank["path"])))
    if descriptors:
        families = {tuple(descriptor[key] for key in ("owner", "bank", "member"))
                    for descriptor in descriptors}
        admitted = {tuple(descriptor[key] for key in ("owner", "bank", "member", "ordinal"))
                    for descriptor in descriptors}
        observed, previous, retired = set(), set(), set()
        for frame in trace["frames"]:
            identities = [tuple(model[key] for key in ("owner", "bank", "member", "ordinal"))
                          for model in frame["effect_models"]
                          if tuple(model[key] for key in ("owner", "bank", "member")) in families]
            present = set(identities)
            require(len(present) == len(identities) and not present & retired,
                    "admitted yaw family contains duplicate or reappearing model identities")
            observed.update(present)
            retired.update(previous - present)
            previous = present
        require(observed == admitted and not previous,
                "yaw descriptors do not cover every observed family instance through cleanup")
        setup["effect_random_yaw"] = descriptors
    proof = dict(native_outputs_used=False, source=capture.pin(reference),
        observations=capture.pin(observations), capture=setup["capture"],
        ready_state=setup["ready_state"], ready_movie=setup["ready_movie"], archive=capture.pin(usual),
        stored_scenes=verified_stored, script=capture.pin(__file__),
        adapter=capture.pin(capture.__file__), witnesses=witnesses)
    return trace, proof


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--assets", type=Path, default=capture.ROOT / "local/all-assets")
    parser.add_argument("--usual", type=Path,
                        default=capture.ROOT / "local/extracted/disc1/files/BTL/BTLusual.dat")
    parser.add_argument("--axis", choices=("pitch", "yaw"), default="pitch")
    args = parser.parse_args()
    derive_angle = derive if args.axis == "pitch" else derive_yaw
    trace, proof = derive_angle(args.reference.resolve(), args.assets.resolve(), args.usual.resolve())
    key = "effect_random_" + args.axis
    require(trace["setup"].get(key), f"no source-bound random {args.axis} models")
    destination = args.output / trace["case"].replace(":", "-") / "trace.json"
    destination.parent.mkdir(parents=True, exist_ok=False)
    if args.axis == "yaw":
        provenance = destination.with_name("effect-random-yaw.json")
        capture.write(provenance, proof)
        trace["setup"][key + "_provenance"] = capture.pin(provenance)
    capture.write(destination, trace)
    proof["output"] = capture.pin(destination)
    capture.write(destination.with_name("derivation.json") if args.axis == "yaw"
                  else args.output / "derivation.json", proof)
    print(dict(output=str(destination), models=len(trace["setup"][key])))
