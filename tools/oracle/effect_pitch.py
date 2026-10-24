"""Register source-bound random initial model angles without changing frames."""
import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess

import arte_equivalence as capture
from named_content import Snapshot as ContentSnapshot, same_evidence
from projectile_launches import require, recorded_fields


def derive(reference, assets, usual):
    trace = copy.deepcopy(capture.read(reference))
    setup = trace["setup"]
    require(not trace["incomplete_observations"], "source observations are incomplete")
    require("effect_random_pitch" not in setup, "random pitch is already registered")
    for key in ("ready_state", "ready_movie", "capture"):
        require(capture.digest(setup[key]["path"]) == setup[key]["sha256"], f"changed {key}")
    observations = Path(setup["capture"]["path"]).with_name("observations.json")
    raw_rows = capture.read_capture_observations(setup["capture"]["path"])
    state = capture.State(setup["ready_state"]["path"])
    require(recorded_fields(capture.inspect(state, battle=True)["battle"], setup["initial_battle"])
            == setup["initial_battle"], "ready state differs")
    banks = capture.effect_banks(state, assets, None, [], usual, reference.parent / "content-evidence")
    archive = list(capture.sections(usual.read_bytes()))
    raw_banks = {hashlib.sha256(archive[index]).hexdigest(): archive[index] for index in (2, 3)}
    rows = {(row["battle_combat_tick_word"], row["battle_presentation_tick_word"]): index
            for index, row in enumerate(raw_rows)}
    records = yaw_records(raw_rows, setup, banks)
    seen, descriptors, witnesses = set(), [], []
    for frame_index, frame in enumerate(trace["frames"]):
        model_particles = [particle for particle in frame["particles"] if particle["model"] is not None]
        require(len(model_particles) == len(frame["effect_models"]), "model particle publications differ")
        for model, particle in zip(frame["effect_models"], model_particles):
            require(all(particle[key] == model[key] for key in ("owner", "bank", "member", "model")),
                    "model publication differs from its particle")
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
            row_index = rows[(frame["combat_tick"], frame["presentation_tick"])]
            row = raw_rows[row_index]
            observed_particle = records[row_index][0].get(particle["id"])
            require(observed_particle is not None, "random model particle generation was not observed")
            samples = []
            for label in ("particle", "alternate"):
                count = capture.observation_counts(row)[label]
                if not count:
                    continue
                nodes, complete = capture.tracked_list(row, label, count)
                require(complete, "particle birth observation is incomplete")
                for index, node in nodes:
                    if node != observed_particle["node"]:
                        continue
                    prefix = f"{label}_{index}_"
                    get = lambda at: row[prefix + f"{at:x}"]
                    # Complete pool lists can also contain scheduler/overlay nodes.
                    # Only particle storage has a particle-parent binding.
                    if get(0x10) >> 24 != 1:
                        continue
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
                metadata=bank["evidence"]))
    if descriptors:
        setup["effect_random_pitch"] = descriptors
    proof = dict(native_outputs_used=False, source=capture.pin(reference),
        observations=capture.observation_pin(observations), capture=setup["capture"],
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
    previous_signature = {}
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
            signature = (copied[0], copied[2], row[prefix + "_184"])
            if (node not in previous_nodes or age < previous_age.get(node, age)
                    or previous_signature.get(node) != signature):
                particle_serial += 1
                identities[node], allocations[node] = particle_serial, row_index
            previous_age[node] = age
            previous_signature[node] = signature
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
    finite_three = lambda values: len(values) == 3 and all(math.isfinite(v) for v in values)
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
            and finite_three(scale) and finite_three(velocity) and finite_three(acceleration)
            and template.get("geometry_acceleration_until") is None
            and all(scale[axis] + age * velocity[axis]
                    + age * (age - 1) / 2 * acceleration[axis] > 0
                    for age in range(lifetime + 1) for axis in range(3)))


def yaw_modifier(words):
    if words == [11, 92, 1800, 0, 65535]:
        return {}
    if (len(words) == 13 and words[:6] == [11, 92, 1800, 0, 0, 32764]
            and words[7:] == [0, 10, 212, 32764, 0, 65535] and words[6] <= 255):
        return dict(fixed_model=words[6])
    return None


def derive_yaw(reference, assets, usual):
    trace = copy.deepcopy(capture.read(reference))
    setup = trace["setup"]
    require(not trace["incomplete_observations"], "source observations are incomplete")
    require("effect_random_yaw" not in setup, "random yaw is already registered")
    for key in ("ready_state", "ready_movie", "capture"):
        require(capture.digest(setup[key]["path"]) == setup[key]["sha256"], f"changed {key}")
    observations = Path(setup["capture"]["path"]).with_name("observations.json")
    raw_rows = capture.read_capture_observations(setup["capture"]["path"])
    state = capture.State(setup["ready_state"]["path"])
    require(recorded_fields(capture.inspect(state, battle=True)["battle"], setup["initial_battle"])
            == setup["initial_battle"], "ready state differs")
    rows = raw_rows
    banks = capture.effect_banks(state, assets, None, [], usual, reference.parent / "content-evidence")
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
            verified_setup, stored["terminal"]["path"], assets, usual, reference.parent / "content-evidence"))
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
            require(angles[::2] == [-90, 0] and all(math.isfinite(v) and v > 0 for v in scale)
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
                template=template, metadata=bank["evidence"]))
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
        observations=capture.observation_pin(observations), capture=setup["capture"],
        ready_state=setup["ready_state"], ready_movie=setup["ready_movie"], archive=capture.pin(usual),
        stored_scenes=verified_stored, script=capture.pin(__file__),
        adapter=capture.pin(capture.__file__), witnesses=witnesses)
    return trace, proof


def retained_banks(setup, assets, usual, evidence_directory, packages=None):
    """Read pinned original banks without requiring discarded emulator states."""
    banks, pins = {}, {}
    def add(raw, metadata, archive):
        bank = capture.effect_bank(raw, metadata, assets, evidence_directory)
        require(same_evidence(bank["evidence"], setup["effect_inputs"][bank["entry"]]),
                "current effect evidence differs from the observed capture")
        archive = Path(archive)
        if archive not in pins:
            pins[archive] = capture.pin(archive)
        bank.update(raw=raw, archive=archive, archive_pin=pins[archive])
        banks[bank["source_sha256"]] = bank
    archive = list(capture.sections(usual.read_bytes()))
    for index, name in ((2, "common"), (3, "techniques")):
        add(archive[index], assets / f"battle/effects/{name}/effects.json", usual)
    content = ContentSnapshot(assets, evidence_directory)
    for proof in setup.get("detached_contacts", {}).get("enemy_packages", []):
        package_path = Path(proof["package"]["path"])
        require(capture.digest(package_path) == proof["package"]["sha256"], "observed enemy package changed")
        path = content.resolve("battle/enemies/mappings.json", proof["enemy"], "enemy.json")
        require(path is not None, "observed enemy is not mapped")
        entry = content.document(path, ("rules", "artwork", "effects"))
        if entry.get("effects"):
            metadata = assets / entry["effects"]
            package = package_path.read_bytes()
            pointers = struct.unpack_from(">116I", package, 0x18)
            start = struct.unpack_from(">I", package, 0x1cc)[0]
            if start:
                raw = package[start:min([value for value in pointers if value > start] + [len(package)])]
                add(raw, metadata, package_path)
    stored = setup.get("stored_scenes")
    if stored:
        path = Path(stored["archive"]["path"])
        require(capture.digest(path) == stored["archive"]["sha256"], "stored archive changed")
        for entry in stored["registrations"]:
            with path.open("rb") as stream:
                start, end = entry["package_range"]
                stream.seek(start)
                package = stream.read(end - start)
            require(hashlib.sha256(package).hexdigest() == entry["package_sha256"],
                    "stored package changed")
            pointers = struct.unpack_from(">68I", package, 4)
            start = pointers[0]
            raw = package[start:min([value for value in pointers if value > start] + [len(package)])]
            require(hashlib.sha256(raw).hexdigest() == entry["bank_sha256"], "stored bank changed")
            add(raw, assets / entry["effects"]["entry"], path)
    return banks


def authored_emissions(bank, program):
    raw, cursor, clock = bank["raw"], bank["roots"][program], 0
    while True:
        age, member, flags, operand = struct.unpack_from(">hBBH", raw, cursor)
        clock = max(clock, age, 0)
        if member == 254:
            return
        count, interval = 1, 0
        if member == 255:
            count, interval = max(flags, 1), max(capture.signed16(operand), 0)
            cursor += 6
            _, member, flags, operand = struct.unpack_from(">hBBH", raw, cursor)
            require(member < 254, "unsupported nested emission repeat")
        if member < len(bank["declarations"]):
            words = [65535]
            if operand:
                words, at = [], operand
                while len(words) < 256:
                    word = struct.unpack_from(">H", raw, at)[0]
                    words.append(word)
                    at += 2
                    if word == 65535:
                        break
                require(words[-1] == 65535, "unterminated emission modifier")
            for repeat in range(count):
                yield dict(age=clock + repeat * interval, member=member, words=words,
                           command=cursor, modifier=operand, repeat=repeat)
        cursor += 6


def static_vertical_modifier(words):
    """Recognize only the scalar writes already admitted by the Rust contract."""
    additions, velocity, remaining = [0., 0., 0.], None, words[:]
    while remaining != [65535]:
        if (len(remaining) >= 6 and remaining[0] == 8
                and remaining[1] in (0x58, 0x5c, 0x60) and remaining[4:6] == [0, 0]):
            axis = (remaining[1] - 0x58) // 4
            require(additions[axis] == 0., "repeated constant angle write")
            additions[axis] = capture.f32(remaining[2] << 16 | remaining[3])
            remaining = remaining[6:]
        elif (len(remaining) >= 10 and remaining[:2] == [11, 0x7ff8]
                and remaining[3:10] == [0, 8, 0x44, 0, 0, 0x7ff8, 0]):
            require(velocity is None and 2 <= remaining[2] <= 1800, "unsupported velocity draw")
            extent = (remaining[2] - 1) / 10
            velocity = dict(min=-extent, max=extent, step=.1)
            remaining = remaining[10:]
        elif len(remaining) >= 4 and remaining[:2] == [11, 0x44] and remaining[3] == 0:
            require(velocity is None and 2 <= remaining[2] <= 1800, "unsupported velocity draw")
            extent = (remaining[2] - 1) / 10
            velocity = dict(min=-extent, max=extent, step=.1)
            remaining = remaining[4:]
        else:
            return None
    return additions, velocity


def observed_orientation_modifier(words, slot):
    """Independent scalar birth draws supported by the existing Rust contract."""
    offsets, angles, model, remaining = [None] * 3, [None] * 3, dict(min=slot, max=slot), words[:]
    model_draw, additions = False, [False] * 3
    while remaining != [65535]:
        if remaining[:4] == [11, 0x7ff8, 1, 0]:
            # Modulo one is exactly zero, including shared size additions.
            tail = remaining[4:]
            while (len(tail) >= 6 and tail[0] == 8
                    and tail[1] in (0x34, 0x38, 0x3c, 0x40, 0x44, 0x48,
                                    0x58, 0x5c, 0x60, 0xb0, 0xb4, 0xb8)
                    and tail[2:6] == [0, 0, 0x7ff8, 0]):
                tail = tail[6:]
            if len(tail) == len(remaining) - 4:
                return None
            remaining = tail
            continue
        length, minimum = 0, 0
        if (len(remaining) >= 12 and remaining[:2] == [2, 0x7ffc]
                and remaining[3:6] == [0, 3, 0x7ffc]
                and remaining[7:12] == [0, 10, 0xd4, 0x7ffc, 0]):
            length, minimum = 12, remaining[6]
        elif (len(remaining) >= 8 and remaining[:2] == [2, 0x7ffc] and remaining[3] == 0
                and remaining[4] in (10, 12) and remaining[5:8] == [0xd4, 0x7ffc, 0]):
            length, minimum = 8, slot if remaining[4] == 12 else 0
        if length:
            count = remaining[2]
            if model_draw or not 2 <= count <= 256 or minimum + count > 256:
                return None
            model, model_draw = dict(min=minimum, max=minimum + count - 1, step=1.), True
            remaining = remaining[length:]
            continue
        if (len(remaining) >= 10 and remaining[:2] == [11, 0x7ff8]
                and remaining[3:5] == [0, 8] and remaining[5] in (0x58, 0x5c, 0x60)
                and remaining[6:10] == [0, 0, 0x7ff8, 0]
                and 2 <= remaining[2] <= 3600):
            axis = (remaining[5] - 0x58) // 4
            if angles[axis] is not None:
                return None
            extent = (remaining[2] - 1) / 10
            angles[axis], additions[axis] = dict(min=-extent, max=extent, step=.1), True
            remaining = remaining[10:]
            continue
        if (len(remaining) < 4 or remaining[0] != 11
                or remaining[1] not in (0x34, 0x3c, 0x58, 0x5c, 0x60)
                or remaining[3] != 0 or not 2 <= remaining[2] <= 3600):
            return None
        target = offsets if remaining[1] < 0x58 else angles
        axis = (remaining[1] - (0x34 if target is offsets else 0x58)) // 4
        if target[axis] is not None or (target is offsets and remaining[2] > 1800):
            return None
        extent = (remaining[2] - 1) / 10
        target[axis] = dict(min=-extent, max=extent, step=.1)
        remaining = remaining[4:]
    return (offsets, angles, model, additions) if any(angles) else None


def retained_declaration(bank, member, projectile=False):
    declaration = bank["metadata"]["actors"][member]
    if declaration.get("kind") != "model_particle":
        return None
    template, raw = declaration["template"], bank["declarations"][member]
    initial = template["state"]
    geometry = initial["geometry"].get("Size")
    if (geometry is None or declaration["binding"]["animation"] is not None
            or bool(template.get("follow_origin")) != projectile or template.get("follow_orientation")
            or template.get("fixed_orientation") or template.get("ground_restitution") is not None
            or any(initial[key] != [0., 0., 0.] for key in ("acceleration", "orbit"))
            or any(template.get(key, [0., 0., 0.]) != [0., 0., 0.]
                   for key in ("jerk", "orbit_velocity"))):
        return None
    fields = ((52, initial["offset"]), (64, initial["velocity"]),
              (76, initial["acceleration"]), (88, initial["angles"]),
              (100, initial["angular_velocity"]), (176, geometry["value"]),
              (188, geometry["velocity"]), (200, geometry["acceleration"]))
    require(raw[0] == 3 and not struct.unpack_from(">I", raw, 20)[0] & (0x600204a4 & ~(0x400 if projectile else 0))
            and bool(struct.unpack_from(">I", raw, 20)[0] & 0x400) == projectile
            and (struct.unpack_from(">h", raw, 16)[0] == 0 if template["lifetime"] is None
                 else struct.unpack_from(">h", raw, 16)[0] + 1 == template["lifetime"])
            and struct.unpack_from(">h", raw, 136)[0] == 0
            and raw[212] == declaration["binding"]["slot"]
            and all(struct.pack(">3f", *values) == raw[at:at + 12] for at, values in fields),
            "retained declaration differs from original bytes")
    return declaration


def static_basis_matches(model, angles):
    (sx, cx), (sy, cy), (sz, cz) = [(math.sin(math.radians(a)), math.cos(math.radians(a)))
                                    for a in angles]
    basis = ((cy*cz, sx*sy*cz-cx*sz, cx*sy*cz+sx*sz),
             (cy*sz, sx*sy*sz+cx*cz, cx*sy*sz-sx*cz), (-sy, sx*cy, cx*cy))
    world = model["model"]["world"]
    for column in range(3):
        scale = sum(world[row*4+column] * basis[row][column] for row in range(3))
        if scale <= 0 or any(abs(world[row*4+column] - scale*basis[row][column]) > .001
                             for row in range(3)):
            return False
    return True


def retained_parent_heading(trace, owner, clock):
    """Detached repeats keep their parent's launch orientation after the owner turns."""
    start = trace["frames"][0]["combat_tick"]
    frames = [frame for frame in trace["frames"] if frame["combat_tick"] - start == clock]
    if len(frames) != 1:
        return None
    actors = [actor for actor in frames[0]["actors"] if actor["actor"] == owner]
    return actors[0]["heading"] if len(actors) == 1 else None


def retained_program_clocks(trace, banks, reference):
    """Bind emission age to an observed scheduler generation, not its publication tick."""
    start = trace["frames"][0]["combat_tick"]
    tracks, by_clock = {}, {}
    for index, frame in enumerate(trace["frames"]):
        by_clock.setdefault(frame["combat_tick"] - start, []).append(index)
        for program in frame["programs"]:
            tracks.setdefault(program["id"], []).append((index, program))
    raw_path = reference.with_name("prelude-observations.json.zst")
    raw_rows, raw_pin = {}, None
    if raw_path.exists():
        capture_pin = trace["setup"]["capture"]
        require(capture.digest(capture_pin["path"]) == capture_pin["sha256"], "capture changed")
        capture_info = capture.read(capture_pin["path"])
        # Some receipts pin only the action segment. Their adjacent prelude is
        # not evidence for an embedded scheduler; normalized live programs are.
        if capture_info.get("combined_observations_sha256") is not None:
            data = subprocess.check_output(["zstd", "-dcq", str(raw_path)])
            require(hashlib.sha256(data).hexdigest() == capture_info["combined_observations_sha256"],
                    "retained scheduler observations differ from capture")
            rows = json.loads(data)
            raw_rows = {(r["battle_combat_tick_word"], r["battle_presentation_tick_word"]): r for r in rows}
            require(len(raw_rows) == len(rows), "retained scheduler observations repeat a clock")
            raw_pin = capture.pin(raw_path)
    epochs = {}
    for event in trace["events"]:
        if event["kind"] != "program_started" or event["bank"] not in banks:
            continue
        key = tuple(event[k] for k in ("owner", "bank", "program", "clock"))
        candidates = [track for track in tracks.values()
                      if trace["frames"][track[0][0]]["combat_tick"] - start == event["clock"]
                      and all(track[0][1][k] == event[k] for k in ("owner", "bank", "program"))]
        if len(candidates) == 1:
            track = candidates[0]
            origin = event["clock"] + 1 - track[0][1]["age"]
            observed = set()
            for index, program in track:
                if (any(program[k] != event[k] for k in ("owner", "bank", "program"))
                        or trace["frames"][index]["combat_tick"] - start + 1 - program["age"] != origin):
                    break
                observed.add(index)
            epochs[key] = dict(clock=origin, frames=observed,
                witness=dict(kind="program", id=track[0][1]["id"], frame=track[0][0],
                             age=track[0][1]["age"], execution_origin=origin))
            continue
        if candidates:
            continue
        slots = [a["source_slot"] for a in trace["setup"].get("source_actor_map", [])
                 if a["actor"] == event["owner"]]
        first_frames = by_clock.get(event["clock"], [])
        if not raw_rows or len(slots) != 1 or len(first_frames) != 1:
            continue
        first = first_frames[0]
        prefix = f"actor_{slots[0]}_effect_"
        frame = trace["frames"][first]
        row = raw_rows.get((frame["combat_tick"], frame["presentation_tick"]), {})
        base, root, age = [row.get(prefix + k) for k in ("bank", "program", "clock_word")]
        if base is None or root is None or age is None or root - base != banks[event["bank"]]["roots"][event["program"]]:
            continue
        age = capture.signed16(age & 65535)
        origin, observed = event["clock"] + 1 - age, set()
        for index in range(first, len(trace["frames"])):
            frame = trace["frames"][index]
            row = raw_rows.get((frame["combat_tick"], frame["presentation_tick"]), {})
            if (row.get(prefix + "bank"), row.get(prefix + "program")) != (base, root):
                break
            current = capture.signed16(row[prefix + "clock_word"] & 65535)
            if frame["combat_tick"] - start + 1 - current != origin:
                break
            observed.add(index)
        epochs[key] = dict(clock=origin, frames=observed,
            witness=dict(kind="embedded", frame=first, age=age, bank_address=base,
                         root=root, execution_origin=origin, observations=raw_pin))
    return epochs


def retained_lifetime_contracts(trace, banks, excluded=()):
    """Register authored families for the dedicated full-lifetime validators."""
    owner = trace["setup"]["owner"]
    families = {}
    for frame in trace["frames"]:
        for model in frame["effect_models"]:
            if model["owner"] == owner and (model["bank"], model["member"]) not in excluded:
                families.setdefault((model["bank"], model["member"]), []).append(model)
    registered = {r["bank_sha256"]: r["effects"] for r in
                  trace["setup"].get("stored_scenes", {}).get("registrations", [])}
    result, witnesses = {}, []
    for (signature, member), models in families.items():
        bank = banks.get(signature)
        if bank is None:
            continue
        declaration = bank["metadata"]["actors"][member]
        if declaration["kind"] != "model_particle" or declaration["binding"]["animation"] is not None:
            continue
        template, slot = declaration["template"], declaration["binding"]["slot"]
        state = template["state"]
        geometry = state["geometry"].get("Size")
        if geometry is None or str(slot) not in bank["metadata"]["art"]["models"]:
            continue
        starts = [e for e in trace["events"] if e["kind"] == "program_started"
                  and e["owner"] == owner and e["bank"] == signature]
        candidates = []
        for program in sorted({e["program"] for e in starts}):
            events = bank["metadata"]["programs"][program]
            births = [e["operation"].get("birth", {}) for e in events
                      if e["operation"].get("kind") == "spawn"
                      and e["operation"].get("particle") == member]
            if not births:
                continue
            kind = None
            if (len(events) == len(births) == 1 and events[0]["at"] == 0
                    and template.get("follow_origin") and not template.get("follow_orientation")
                    and all(v > 0 and math.isfinite(v) for v in geometry["value"])
                    and geometry["velocity"] == geometry["acceleration"] == [0., 0., 0.]
                    and set(births[0]) == {"random_draws", "angles"} and births[0]["random_draws"] == 3
                    and all(r is not None and -360 <= r["min"] <= -179.9
                            and 179.9 <= r["max"] <= 360 and r.get("draw") == axis
                            for axis, r in enumerate(births[0]["angles"]))
                    and all((m["attachment"] or {}).get("kind") == "projectile" for m in models)):
                kind = "relative_rotations"
            elif (signature in registered and sum(e["program"] == program for e in starts) == 1
                    and template.get("lifetime", 0) is not None and template["lifetime"] > 1
                    and not template.get("origin_motion") and not template.get("fixed_orientation")
                    and template.get("jerk", [0., 0., 0.]) == [0., 0., 0.]
                    and state["acceleration"][::2] == [0., 0.] and state["acceleration"][1] <= 0
                    and (state["acceleration"][1] < 0 or any(any(b.get("size", [])) for b in births))
                    and state["velocity"][::2] == [0., 0.]
                    and all(m["attachment"] is None for m in models)
                    and all(set(birth) <= {"random_draws", "offset", "velocity", "angles", "size",
                                          "model", "velocity_spread"}
                            and (birth.get("velocity_spread") is None
                                 or (birth["velocity_spread"].get("angles") == [90., 0., 0.]
                                     and birth["velocity_spread"].get("angle_jitter") == 360))
                            and birth.get("velocity", [None]*3)[::2] == [None, None]
                            for birth in births)):
                kind = "ballistic_particles"
            if kind is not None:
                candidates.append((kind, program))
        if len(candidates) != 1:
            continue
        kind, program = candidates[0]
        pin = bank["evidence"]
        if kind == "ballistic_particles":
            require(same_evidence(pin, registered[signature]), "ballistic metadata differs from registered scene")
        result.setdefault("effect_" + kind, []).append(dict(metadata=pin, member=member, program=program))
        witnesses.append(dict(kind=kind, bank=signature, member=member, program=program,
            instances=len({m["ordinal"] for m in models}), model_samples=len(models), metadata=pin,
            declaration_sha256=hashlib.sha256(bank["declarations"][member]).hexdigest()))
    return result, witnesses


def derive_retained(reference, assets, usual, packages=None):
    """Bind existing observations to deterministic authored fields, never invent raw samples."""
    trace = (json.loads(subprocess.check_output(["zstd", "-dcq", str(reference)]))
             if reference.suffix == ".zst" else capture.read(reference))
    require(not trace["incomplete_observations"], "source observations are incomplete")
    setup, start = trace["setup"], trace["frames"][0]["combat_tick"]
    require(not any(setup.get(key) for key in ("effect_random_births", "effect_random_yaw",
                "effect_relative_rotations", "effect_ballistic_particles")),
            "retained birth/yaw descriptors are already registered")
    banks = retained_banks(setup, assets, usual, reference.parent / "content-evidence", packages)
    epochs = retained_program_clocks(trace, banks, reference)
    schedules, declarations = {}, {}
    for event in trace["events"]:
        if event["kind"] != "program_started" or event["bank"] not in banks:
            continue
        key = (event["owner"], event["bank"], event["program"], event["clock"])
        if key in schedules:
            continue
        bank = banks[event["bank"]]
        schedules[key] = list(authored_emissions(bank, event["program"]))
    families, candidates, seen, witnesses = {}, {}, set(), []
    for frame_index, frame in enumerate(trace["frames"]):
        for model in frame["effect_models"]:
            identity = tuple(model[key] for key in ("owner", "bank", "member", "ordinal"))
            family = identity[:3]
            if identity in seen:
                continue
            seen.add(identity)
            families.setdefault(family, set()).add(identity)
            bank = banks.get(model["bank"])
            projectile = (model["attachment"] or {}).get("kind") == "projectile"
            if bank is None or (model["attachment"] is not None and not projectile):
                continue
            dkey = (model["bank"], model["member"], projectile)
            if dkey not in declarations:
                declarations[dkey] = retained_declaration(bank, model["member"], projectile)
            declaration = declarations[dkey]
            if declaration is None:
                continue
            particles = [p for p in frame["particles"] if all(p[k] == model[k]
                         for k in ("owner", "bank", "member", "model"))]
            require(len(particles) == 1, "model birth lacks a unique retained particle")
            particle = particles[0]
            if not particle["initialized"] or particle["age"] != 1:
                continue
            clock = frame["combat_tick"] - start
            previous = trace["frames"][frame_index - 1] if frame_index else None
            pending = (previous is not None and previous["combat_tick"] + 1 == frame["combat_tick"]
                       and any(all(old[key] == particle[key]
                                   for key in ("id", "owner", "bank", "member"))
                               and not old["initialized"] and old["age"] == 0
                               and old["model"] is None for old in previous["particles"]))
            allocation_clock = clock - int(pending)
            matches = []
            for (owner, signature, program, program_clock), emissions in schedules.items():
                if (owner, signature) != identity[:2]:
                    continue
                epoch = epochs.get((owner, signature, program, program_clock))
                execution_clock = epoch["clock"] if epoch else program_clock
                if epoch and frame_index - int(pending) not in epoch["frames"]:
                    continue
                for emission in emissions:
                    # A newly published age-two parent can expose its zero-age
                    # child at age one in the same sample. Later repeats still
                    # use the observed parent clock, never a blanket delay.
                    first_publication = (epoch is not None and not pending and emission["age"] == 0
                        and epoch["witness"]["kind"] == "program"
                        and (epoch["witness"]["age"] == 2 or (projectile and epoch["witness"]["age"] == 0))
                        and epoch["witness"]["frame"] == frame_index and clock == program_clock)
                    if (emission["member"] != model["member"]
                            or (execution_clock + emission["age"] != allocation_clock
                                and not first_publication)):
                        continue
                    if projectile and not any(p["owner"] == owner and p["bank"] == signature
                            and p["program"] == program and p["attachment"] == model["attachment"]
                            for p in frame["programs"]):
                        continue
                    template = dict(**bank["archive_pin"], member=model["member"],
                                    program=program, modifier_words=emission["words"])
                    descriptor = dict(zip(("owner", "bank", "member", "ordinal"), identity),
                                      birth_clock=clock, program_clock=program_clock, template=template)
                    initial = declaration["template"]["state"]
                    heading = (next(a["heading"] for a in frame["actors"] if a["actor"] == owner)
                               if projectile else retained_parent_heading(trace, owner, program_clock))
                    if heading is None:
                        continue
                    yaw = yaw_modifier(emission["words"])
                    if yaw_template(declaration) and yaw is not None:
                        descriptor.update(yaw)
                        descriptor["first_step"] = initial["angular_velocity"][1]
                        kind = "yaw"
                    else:
                        orientation = observed_orientation_modifier(emission["words"], declaration["binding"]["slot"])
                        if (orientation is not None and "angles" in particle and "model_slot" in particle
                                and initial["velocity"][::2] == [0., 0.]):
                            offsets, angles, model_choices, additions = orientation
                            first_step = [spin + (angle + (heading if axis == 1 else 0.)
                                          if additions[axis] else angle if angles[axis] is None else 0.)
                                          for axis, (angle, spin) in enumerate(zip(
                                              initial["angles"], initial["angular_velocity"]))]
                            local = [(value if offsets[axis] is None else 0.) + velocity
                                     for axis, (value, velocity) in enumerate(zip(
                                         initial["offset"], initial["velocity"]))]
                            sine, cosine = math.sin(math.radians(heading)), math.cos(math.radians(heading))
                            first_offset = [cosine*local[0]+sine*local[2], local[1],
                                            cosine*local[2]-sine*local[0]]
                            descriptor.update(heading=heading, origin=None if projectile else particle["origin"],
                                first_step=first_step, first_offset=first_offset,
                                offsets=offsets, angles=angles, model=model_choices,
                                angular_velocity=initial["angular_velocity"],
                                lifetime=declaration["template"]["lifetime"])
                            matches.append(("births", descriptor, emission))
                            continue
                        parsed = static_vertical_modifier(emission["words"])
                        if (parsed is None or initial["angular_velocity"] != [0., 0., 0.]
                                or initial["velocity"] != [0., 0., 0.]):
                            continue
                        additions, velocity = parsed
                        first_step = [a+b for a, b in zip(initial["angles"], additions)]
                        angles = [first_step[0], heading+first_step[1], first_step[2]]
                        if not static_basis_matches(model, angles):
                            continue
                        slot = declaration["binding"]["slot"]
                        descriptor.update(heading=heading, origin=particle["origin"], first_step=first_step,
                            first_offset=initial["offset"], offsets=[None]*3, angles=[None]*3,
                            angle_additions=additions, vertical_velocity=velocity,
                            model=dict(min=slot, max=slot), lifetime=declaration["template"]["lifetime"],
                            static_binding=dict(angles=angles, model=slot))
                        kind = "births"
                    matches.append((kind, descriptor, emission))
            # Identical authored commands can emit indistinguishable siblings.
            # Their observed identities still receive one fixed lifetime mapping.
            contracts = {json.dumps((kind, descriptor), sort_keys=True) for kind, descriptor, _ in matches}
            if len(contracts) != 1:
                continue
            kind, descriptor, emission = matches[0]
            candidates[identity] = (kind, descriptor)
            scheduler = epochs.get((descriptor["owner"], descriptor["bank"],
                descriptor["template"]["program"], descriptor["program_clock"]), {}).get("witness")
            witnesses.append(dict(identity=list(identity), frame=frame_index, particle=particle["id"],
                pending_frame=frame_index - 1 if pending else None,
                scheduler=scheduler,
                initial_parent_publication=(scheduler is not None and scheduler["kind"] == "program"
                    and scheduler["age"] in (0, 2) and scheduler["frame"] == frame_index
                    and emission["age"] == 0 and clock == descriptor["program_clock"]),
                command_offset=emission["command"], modifier_offset=emission["modifier"],
                equivalent_commands=sorted({candidate[2]["command"] for candidate in matches}),
                emission_age=emission["age"], repeat=emission["repeat"],
                declaration_sha256=hashlib.sha256(bank["declarations"][model["member"]]).hexdigest(),
                metadata=bank["evidence"]))
    admitted = set()
    for family, identities in families.items():
        if not identities <= candidates.keys():
            continue
        kinds = {candidates[identity][0] for identity in identities}
        if len(kinds) != 1:
            continue
        kind = next(iter(kinds))
        if kind == "births" and not any(candidates[i][1].get("vertical_velocity")
                or any(candidates[i][1]["angles"]) for i in identities):
            continue
        admitted.update(identities)
        setup.setdefault("effect_random_" + kind, []).extend(candidates[i][1] for i in sorted(identities))
    lifetime_contracts, lifetime_witnesses = retained_lifetime_contracts(
        trace, banks, {identity[1:3] for identity in admitted})
    require(admitted or lifetime_contracts, "no complete supported retained model families")
    setup.update(lifetime_contracts)
    proof = dict(native_outputs_used=False, source=capture.pin(reference), script=capture.pin(__file__),
                 observation_changes=False, witnesses=[w for w in witnesses if tuple(w["identity"]) in admitted],
                 lifetime_contracts=lifetime_witnesses)
    return trace, proof


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--assets", type=Path, default=capture.ROOT / "local/all-assets")
    parser.add_argument("--usual", type=Path,
                        default=capture.ROOT / "local/extracted/disc1/files/BTL/BTLusual.dat")
    parser.add_argument("--axis", choices=("pitch", "yaw", "retained"), default="pitch")
    parser.add_argument("--packages", type=Path, help="original enemy packages for retained bank binding")
    args = parser.parse_args()
    derive_angle = {"pitch": derive, "yaw": derive_yaw, "retained": derive_retained}[args.axis]
    options = {"packages": args.packages.resolve()} if args.axis == "retained" and args.packages else {}
    trace, proof = derive_angle(args.reference.resolve(), args.assets.resolve(), args.usual.resolve(), **options)
    key = "effect_random_" + args.axis
    require(args.axis == "retained" or trace["setup"].get(key), f"no source-bound random {args.axis} models")
    destination = args.output / trace["case"].replace(":", "-") / "trace.json"
    destination.parent.mkdir(parents=True, exist_ok=False)
    if args.axis in ("yaw", "retained"):
        provenance = destination.with_name("effect-random-" + args.axis + ".json")
        capture.write(provenance, proof)
        trace["setup"][key + "_provenance"] = capture.pin(provenance)
    capture.write(destination, trace)
    proof["output"] = capture.pin(destination)
    capture.write(destination.with_name("derivation.json") if args.axis in ("yaw", "retained")
                  else args.output / "derivation.json", proof)
    print(dict(output=str(destination), models=sum(len(trace["setup"].get("effect_random_" + kind, []))
               for kind in (("births", "yaw") if args.axis == "retained" else (args.axis,))),
               lifetime_families=sum(len(trace["setup"].get("effect_" + kind, []))
               for kind in ("relative_rotations", "ballistic_particles"))))
