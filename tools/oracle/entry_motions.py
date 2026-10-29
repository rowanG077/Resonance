"""Bind observed external party motions from their authored package."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess

from battle_fixture import enemy_package
from state import State, digest


def register(trace, terminal, packages, enemy_ids, extracted, importer):
    setup = trace["setup"]
    transitions = {(pose["actor"], pose["clip"]) for pose in
                   setup.get("entry_model_transitions", [])
                   if pose["clip"] >= 100 and setup["actors"][pose["actor"]]["side"] == 0}
    ready = {(pose["actor"], pose["clip"]) for pose in setup.get("ready_models", [])
             if pose["clip"] >= 100 and setup["actors"][pose["actor"]]["side"] == 0}
    motions = transitions | ready
    if not motions:
        return
    state = State(terminal)
    authored = []
    for enemy in sorted(set(enemy_ids)):
        path = Path(packages) / f"{enemy:03}.bin"
        raw = path.read_bytes()
        base, proof = enemy_package(state, raw)
        authored.append((path, raw, base, proof))
    bindings = []
    for actor, clip in sorted(motions):
        source_slot = setup["source_actor_map"][actor]["source_slot"]
        initial = next(value for value in setup["initial_battle"]["actors"]
                       if value["slot"] == source_slot)
        resource = initial["address"] + 0x1018 + (clip - 100) * 20
        if setup["body_resources"][str(source_slot)][str(clip)] != resource:
            raise ValueError("external entry motion differs from its observed controller resource")
        observed = [pose.get("source_motion") for pose in setup.get("entry_model_transitions", [])
                    if pose["actor"] == actor and pose["clip"] == clip]
        if (actor, clip) in ready:
            pose = next(value for value in setup["ready_model_observations"] if value["actor"] == actor)
            if pose["resource"] != resource:
                raise ValueError("external ready motion differs from its observed controller resource")
            observed.append(pose.get("source_motion"))
        if not observed or any(value is None or value != observed[0] for value in observed):
            raise ValueError("external entry motion lacks one observed resource binding")
        binding = observed[0]
        header, descriptor = binding["header"], binding["descriptor"]
        if (state.read(resource) != header or state.read(resource + 4) != descriptor
                or state.read(resource + 8, "f") != binding["duration"]):
            raise ValueError("external entry motion resource was rebound before its retained snapshot")
        matches = []
        for path, raw, base, proof in authored:
            offsets = [struct.unpack_from(">I", raw, field)[0] for field in range(0x18, 0x1e8, 4)]
            for field in range(0x20, 0x160, 4):
                offset = struct.unpack_from(">I", raw, field)[0]
                if offset < 0x1e8 or base + offset != header:
                    continue
                end = min((value for value in offsets if offset < value < len(raw)), default=len(raw))
                motion = raw[offset:end]
                if (len(motion) < 36 or struct.unpack_from(">I", motion)[0] != 0x007b7960
                        or state.read(header + 4) != descriptor
                        or descriptor != header + struct.unpack_from(">I", motion, 4)[0]
                        or bytes(state.ram[header - 0x80000000 + 8:header - 0x80000000 + 20]) != motion[8:20]
                        or state.read(header + 20) != header + struct.unpack_from(">I", motion, 20)[0]):
                    raise ValueError("external entry motion has inconsistent package/controller binding")
                matches.append((path, (field - 0x20) // 4, offset, proof))
        # Aliased table entries have the same authored bytes and binding.
        if len({(path, offset) for path, _, offset, _ in matches}) != 1:
            raise ValueError("external entry motion has no unique authenticated package member")
        path, package_clip, offset, proof = matches[0]
        member = next(value for value in setup["party_members"]
                      if value["character"] == setup["party"][source_slot])
        output = Path(terminal).parent / f"entry-motion-{actor}-{clip}.glb"
        result = subprocess.run([str(importer), "bind-fixture-motion", "--extracted", str(extracted),
            "--package", str(path), "--package-clip", str(package_clip),
            "--character", str(member["character"]), "--costume", str(member["costume"]),
            "--output", str(output)], check=True, capture_output=True, text=True)
        receipt = json.loads(result.stdout)
        # These pins authenticate this external observation, not cooked assets.
        for key in ("package", "body", "motion"):
            receipt[key]["sha256"] = digest(receipt[key]["path"])
        source = receipt["source_motion"]
        native = Path(receipt["package"]["path"]).read_bytes()
        receipt["source_motion_sha256"] = hashlib.sha256(
            native[source["offset"]:source["offset"] + source["bytes"]]).hexdigest()
        if (receipt["package"]["sha256"] != proof["source_sha256"]
                or receipt["motion"]["sha256"] != digest(output)
                or binding["duration"] != receipt["duration_frames"]):
            raise ValueError("external entry motion receipt differs from the observed source")
        bindings.append(dict(actor=actor, clip=clip, motion=receipt["motion"],
            source=dict(resource=resource, header=header, descriptor=descriptor,
                        package_offset=offset, package_binding=proof), receipt=receipt))
    setup["entry_shared_motions"] = bindings
