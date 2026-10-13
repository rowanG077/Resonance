"""Catalogue-driven Dolphin observations for the Rust arte equivalence test.

One resident emulator, ordinary inputs, no per-arte case files. This adapter
reports evidence and missing observations; it never certifies equivalence.
"""
import argparse
from collections import Counter
import gc
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
from types import SimpleNamespace

import battle_artes
import battle_geometry
import damage_envelope
import enemy_artes
from battle_fixture import BattleFixture, DTM_BUTTONS, neutral_movie
from battle_state import presentation_locations, watch_locations
from enemy_catalogue import build_catalogue
from memory_watch import LOCATIONS
from state import State, digest, inspect

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
POOL = enemy_artes.BATTLE_POINTER
ACTORS, STRIDE = enemy_artes.ACTORS, enemy_artes.ACTOR_BYTES
PROJECTILES, PROGRAMS, PARTICLES = 16, 4, 12
MENU_ACTIVE = 0x80000000


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def pin(path):
    return {"path": str(Path(path).resolve()), "sha256": digest(path)}


def f32(word):
    value = struct.unpack(">f", struct.pack(">I", word))[0]
    return value if math.isfinite(value) else None


def resolve_cases(assets, descriptors):
    """Resolve authored payloads for the single Rust-generated selection."""
    techniques = read(assets / "game/techniques.json")
    placements = read(assets / "battle/unison.json")["placements"]
    result = []
    for descriptor in descriptors:
        binding = descriptor["binding"]
        if binding["side"] == "party":
            character, arte = binding["character"], binding["arte"]
            if arte not in techniques["learning"][character - 1]:
                raise ValueError("selected binding is absent from authored learning")
            definition = techniques["definitions"][arte]
            capabilities = definition["capabilities"]
            fixture = ("recovery" if capabilities["target"] in ("ally", "self_target", "party")
                       else "spell" if capabilities["spell"] else "martial")
            resolved = dict(id=f"party:{character}:{arte}", character=character,
                arte=arte, native=placements[arte]["native_id"], fixture=fixture,
                tp_cost=definition["tp_cost"], capabilities=capabilities,
                definition=definition)
        elif binding["side"] == "enemy":
            enemy, row = binding["enemy"], binding["row"]
            definition = read(assets / f"battle/enemies/{enemy:03}/definition.json")["actions"]["rows"][row]
            resolved = dict(id=f"enemy:{enemy}:{row}", enemy=enemy, action_row=row, definition=definition)
        else:
            raise ValueError("unknown selected binding side")
        if descriptor["id"] != resolved["id"]:
            raise ValueError("case ID disagrees with its binding")
        resolved["context"] = descriptor["context"]
        result.append(resolved)
    return result


def selected_cases(args):
    if args.cases_file:
        descriptors = read(args.cases_file)
    else:
        # Human CLI selection uses the same authored IDs. Context must be
        # supplied explicitly; the bridge does not classify the catalogue.
        if not args.case or not args.context:
            raise ValueError("--cases-file or explicit --case and --context are required")
        descriptors = []
        for identity in args.case:
            side, first, second = identity.split(":")
            binding = dict(side=side, **({"character": int(first), "arte": int(second)}
                if side == "party" else {"enemy": int(first), "row": int(second)}))
            descriptors.append(dict(id=identity, binding=binding, context=args.context))
    requested = set(args.case or [d["id"] for d in descriptors])
    selected = [d for d in descriptors if d["id"] in requested]
    if requested != {d["id"] for d in selected} or len({d["id"] for d in selected}) != len(selected):
        raise ValueError("unknown or duplicate selected case")
    return resolve_cases(args.assets, selected)


def locations(actor_slots=None, particle_count=PARTICLES, projectile_count=PROJECTILES, alternate_count=8, pool_base=None):
    actor_slots = list(range(12)) if actor_slots is None else actor_slots
    result = watch_locations({"pool": 1, "bss_index": 6,
                              "actors": [{"slot": i} for i in actor_slots]})
    result = {address: name for address, name in result.items()
              if not (name.startswith("battle_camera_") and name.endswith("_bits"))}
    for offset in (0x1f44, 0x1f5c):
        result[f"8035a768 {offset:x}"] = f"damage_session_{offset:x}"
    for offset in (0x15714, 0x15728):
        result[f"{POOL} {offset:x}"] = f"damage_battle_{offset:x}"
    result[f"{POOL} 7e60c"] = "contact_queue_counts"
    for side in range(2):
        for index in range(4):
            entry = 0x7dc0c + side * 0x500 + index * 0x20
            prefix = f"contact_queue_{side}_{index}"
            for offset, label in ((4,"owner"),(8,"context"),(12,"projectile")):
                result[f"{POOL} {entry + offset:x}"] = prefix + "_" + label
            for suffix, label in (("4","rule"),("8","template"),("34","nominal"),
                                  ("4 0","rule_flags_element"),("4 c","rule_power"),("8 8","contact_kind")):
                result[f"{POOL} {entry + 8:x} {suffix}"] = prefix + "_" + label
    for slot in actor_slots:
        base = ACTORS + slot * STRIDE
        for offset, name in ((0x104, "selected"), (0x107c, "control_word"), (0x104c, "animation"),
                             (0x1050, "commands"), (0x1054, "clock"),
                             (0x1174, "native"), (0x199c, "contact_target"),
                             (0x19a0, "effective_target"), (0x106c, "attack_power_word"),
                             (0x10c4, "proficiency_word"), (0x1a10, "cast_strength_word"),
                             (0x1b0, "target_state_word"), (0x10a8, "overlimit_gauge_word"),
                             (0x117c, "fresh_contact_word")):
            result[f"{POOL} {base + offset:x}"] = f"actor_{slot}_{name}"
        for offset in (0, 4, 8, 0x10, 0x14, 0x38, 0x3c, 0x5c):
            result[f"{POOL} {base + 4:x} {offset:x}"] = f"actor_{slot}_condition_{offset:x}"
        for offset in (0x1d0, 0x1d4, 0x1e0, 0x1e4, 0x280, 0x1aa8, 0x1910, 0x1914, 0x1918, 0x1944, 0x1948, 0x194c, 0x1974, 0x1978, 0x197c):
            result[f"{POOL} {base + offset:x}"] = f"actor_{slot}_raw_{offset:x}"
        for word in range(12):
            result[f"{POOL} {base + 0x2c0:x} 50 {word * 4:x}"] = f"actor_{slot}_root_{word}"
        controller = f"{POOL} {base + 0x2b0:x} 5fc"
        for link in range(2):
            node = controller + " 2c" * link
            prefix = f"actor_{slot}_animation_{link}"
            result[node] = prefix + "_address"
            for offset, name in ((0, "resource"), (4, "layer_word"), (8, "frame_bits"), (0x2c, "next"), (0x30, "child")):
                result[f"{node} {offset:x}"] = prefix + "_" + name
            result[node + " 30 0"] = prefix + "_child_resource"
            result[node + " 30 8"] = prefix + "_child_frame_bits"
        for offset, label in ((0x38, "bank"), (0x44, "program"), (0x34, "clock_word"), (0x8c, "cursor")):
            result[f"{POOL} {base + 0x15b4 + offset:x}"] = f"actor_{slot}_effect_{label}"
    groups = [("projectile", 0x75c44, projectile_count,
               (0x10, 0x14, 0x28, 0x2c, 0x30, *range(0x40, 0x58, 4), 0xac, 0xd8, 0x144, 0x16c, 0x170, 0x278, 0x280)),
              ("program", 0x75ecc, PROGRAMS, (0x10, 0x2c, 0x44, 0x48, 0x4c, 0x50, 0x5c, 0x60, 0x6c)),
              ("particle", 0x763dc, particle_count,
               (0x10, 0x1c, 0x20, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c,
                0x5c, 0x60, 0x64, 0x80, 0x84, 0x88, 0xb4, 0xd8, 0xdc, 0xe0,
                0xfc, 0x168, 0x16c, 0x170, 0x174, 0x178, 0x184, 0x278))]
    alternate_offsets = sorted(set(groups[1][3]) | set(groups[2][3]))
    groups.append(("alternate", 0x76664, alternate_count, alternate_offsets))
    for label, head, count, offsets in groups:
        for index in range(count):
            node = f"{POOL} {head:x}" + " 4" * index
            prefix = f"{label}_{index}"
            result[node] = prefix + "_address"
            result[f"{node} 4"] = prefix + "_next"
            for offset in offsets:
                result[f"{node} {offset:x}"] = f"{prefix}_{offset:x}"
            if label in ("particle", "alternate"):
                # The verified program-parent pointer addresses parent+0x28.
                result[f"{node} 184 38"] = prefix + "_parent_bank"
                result[f"{node} 184 44"] = prefix + "_parent_program"
                result[f"{node} 184 4"] = prefix + "_parent_owner"
            elif label == "projectile":
                result[f"{node} 16c 0"] = prefix + "_rule_flags_element"
                result[f"{node} 16c c"] = prefix + "_rule_power"
                result[f"{node} 170 8"] = prefix + "_contact_kind"
    # Retain the head also for normalization of older head-only observations.
    if not alternate_count:
        result[f"{POOL} 76664"] = "effect_alternate_head"
    if pool_base is not None:
        # Only the initialized pool anchor is fixed. Every dynamic linked-list
        # and actor-owned pointer suffix remains a live dereference.
        if not 0x80000000 <= pool_base <= 0x81780000:
            raise ValueError("invalid initialized battle pool anchor")
        compact = {}
        for address, label in result.items():
            if address.startswith(POOL + " "):
                first, *suffix = address[len(POOL) + 1:].split()
                address = " ".join([f"{pool_base + int(first,16):x}", *suffix])
            compact[address] = label
        result = compact
    return result


def observer_plan(slots, pool_base=None):
    """Allocate within the existing transport; every bounded list has overflow checks."""
    for alternate_count in (12, 8, 4, 2):
        for particle_count in range(24, 3, -1):
            watch = locations(slots, particle_count, 4, alternate_count, pool_base)
            try:
                return watch, packet_budget(watch)
            except ValueError:
                continue
    raise ValueError("actor and effect observations exceed the existing watcher budget")


def observed_body(row, slot, resources):
    clip = (row[f"battle_actor_{slot}_body_clip_word"] >> 8) & 255
    expected = resources.get(str(clip))
    for link in range(2):
        prefix = f"actor_{slot}_animation_{link}"
        if row.get(prefix + "_layer_word", 0) & 65535 != 6:
            continue
        if expected and row.get(prefix + "_resource") == expected:
            return clip, f32(row[prefix + "_frame_bits"])
        if expected and row.get(prefix + "_child") and row.get(prefix + "_child_resource") == expected:
            return clip, f32(row[prefix + "_child_frame_bits"])
    return clip, None


def signed16(value):
    return value - 65536 if value & 0x8000 else value


def projectile_contact(get):
    age = signed16(get(0x278) >> 16)
    start, duration = signed16(get(0xac) >> 16), signed16(get(0xac) & 65535)
    disarmed = bool(get(0xd8) & 0xf0)
    active = not disarmed and age != 0 and (duration == 0 or start <= age <= start + duration)
    return age, active, disarmed


def static_model_world(origin, offset, angles, scale, flags, elevation):
    """Ordinary model instance TRS; shared displayed scene matrices are not instances."""
    if flags & (0x40000000 | 0x20000):
        return None
    position = [a + b for a, b in zip(origin, offset)]
    if flags & 0x1000:
        position[1] = offset[1]
    if flags & 2:
        position[1] += elevation
    x, y, z = map(math.radians, angles)
    sx, sy, sz, cx, cy, cz = math.sin(x), math.sin(y), math.sin(z), math.cos(x), math.cos(y), math.cos(z)
    rotation = [[cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
                [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
                [-sy, cy * sx, cy * cx]]
    return [value for i in range(3) for value in
            [*[rotation[i][j] * scale[j] for j in range(3)], position[i]]]


def packet_budget(extra):
    all_locations = {**LOCATIONS, **presentation_locations(), **extra, "80000000": "case_marker"}
    maximum = 1 + sum(len(address.encode("ascii")) + 10 for address in all_locations)
    if maximum >= 60000:
        raise ValueError(f"MemoryWatcher worst-case datagram {maximum} exceeds 60000 bytes")
    return dict(addresses=len(all_locations), maximum_bytes=maximum, receiver_bytes=65536)


def sections(raw):
    count = struct.unpack_from(">I", raw)[0]
    if not 0 < count < len(raw) // 4:
        raise ValueError("invalid authored archive")
    offsets = struct.unpack_from(f">{count}I", raw, 4)
    for start in offsets:
        if start and not 4 + count * 4 <= start < len(raw):
            raise ValueError("invalid authored section")
        yield raw[start:min([end for end in offsets if end > start] + [len(raw)])] if start else None


def effect_banks(state, assets, packages, enemy_ids, usual):
    """Bind complete authored bytes, never an address or declaration ID alone."""
    sources = []
    common = list(sections(Path(usual).read_bytes()))
    for index, name in ((2, "common"), (3, "techniques")):
        sources.append((common[index], assets / f"battle/effects/{name}.json"))
    for enemy in enemy_ids:
        raw = (packages / f"{enemy:03}.bin").read_bytes()
        start = struct.unpack_from(">I", raw, 0x1cc)[0]
        offsets = struct.unpack_from(">116I", raw, 0x18)
        if start:
            end = min([v for v in offsets if v > start] + [len(raw)])
            sources.append((raw[start:end], assets / f"battle/enemies/{enemy:03}/effects.json"))
    banks = {}
    memory = bytes(state.ram)
    for raw, path in sources:
        if raw[:4] != b"ef1\0" or not path.exists():
            continue
        metadata = read(path)
        signature = hashlib.sha256(raw).hexdigest()
        if signature != metadata["source_sha256"]:
            raise ValueError(f"effect bank differs from cooked source: {path}")
        offsets = struct.unpack_from(">6H", raw, 8)
        end = min(v for v in offsets if v > offsets[0])
        if (end - offsets[0]) % 352:
            raise ValueError("truncated effect declarations")
        declarations = [raw[at:at + 352] for at in range(offsets[0], end, 352)]
        roots = [offsets[1] + struct.unpack_from(">h", raw, offsets[4] + 2 * i)[0]
                 for i in range(raw[4])]
        start = 0
        while (at := memory.find(raw, start)) >= 0:
            banks[at + 0x80000000] = dict(source_sha256=signature, roots=roots,
                declarations=declarations, metadata=metadata, path=str(path))
            start = at + len(raw)
    return banks


def tracked_list(row, label, count):
    nodes, seen = [], set()
    sentinel = row.get("battle_pool", 0) + {"projectile": 0x75c40,
                                           "program": 0x75ec8, "particle": 0x763d8,
                                           "alternate": 0x76660}[label]
    for index in range(count):
        prefix = f"{label}_{index}"
        address = row.get(prefix + "_address", 0)
        if address == sentinel:
            return nodes, True
        if not address:
            return nodes, False
        if address in seen:
            return nodes, False
        seen.add(address)
        nodes.append((index, address))
        if row.get(prefix + "_next", 0) == sentinel:
            return nodes, True
    return nodes, False


def observation_counts(row):
    return {label: sum(k.startswith(label + "_") and k.endswith("_address") for k in row)
            for label in ("projectile", "program", "particle", "alternate")}


def observation_tail_empty(row, counts):
    """No active or pending gameplay/effect nodes; numeric overlays may finish later."""
    for label in counts:
        if label == "alternate" and not counts[label]:
            if row.get("effect_alternate_head") != row["battle_pool"] + 0x76660:
                return False
            continue
        nodes, complete = tracked_list(row, label, counts[label])
        if not complete:
            return False
        for index, _ in nodes:
            kind = row[f"{label}_{index}_10"] >> 24
            if label == "projectile" or (label == "program" and kind != 6) or kind in (0, 1):
                return False
    return True


def completion_observer(source_owner, expected):
    """Inspect each newly collected row once; final normalization remains authoritative."""
    checked, admitted, counts = 0, False, None

    def done(rows):
        nonlocal checked, admitted, counts
        if rows and counts is None:
            counts = observation_counts(rows[0])
        for row in rows[checked:]:
            checked += 1
            action = row[f"battle_actor_{source_owner}_action"]
            if action >> 24 == 5 and (action >> 16) & 255 and all(
                    row[f"actor_{source_owner}_{key}"] & (65535 if key == "native" else 0xffffffff) == value
                    for key, value in expected.items()):
                admitted = True
            if admitted and action >> 24 == 3 and observation_tail_empty(row, counts):
                return True
        return False
    return done


def embedded_start(row, previous, slot):
    prefix = f"actor_{slot}_effect_"
    current = (row[prefix + "bank"], row[prefix + "program"])
    age = signed16(row[prefix + "clock_word"] & 65535)
    prior_age = signed16(previous[prefix + "clock_word"] & 65535)
    changed = current != (previous[prefix + "bank"], previous[prefix + "program"])
    return all(current) and age == 0 and (changed or prior_age > age)


def retired_parent_birth(parent, signature, age, previous):
    """A final spawn can outlive its scheduler within the same actor update."""
    prior = previous.get(parent - 0x28)
    return 0 <= age <= 1 and prior is not None and prior["signature"] == signature


def particle_readiness(header, copied):
    """Copied declarations precede subtype initialization; pending age/TRS are stale."""
    subtype, declared = (header >> 8) & 255, copied[0]
    if not declared or subtype not in (0, declared):
        raise ValueError("render particle subtype does not match its copied declaration")
    return subtype == declared


def declaration_matches(bank, copied, parent_root):
    """Exact copied prefix, including supported authored birth overrides."""
    def signature(raw):
        return raw[:3] + raw[4:24]
    actual = signature(copied)
    matches = {i for i, raw in enumerate(bank["declarations"]) if signature(raw) == actual}
    if parent_root not in bank["roots"]:
        return sorted(matches)
    commands = bank["metadata"]["programs"][bank["roots"].index(parent_root)]
    spawned = {c["operation"]["particle"] for c in commands if c["operation"]["kind"] == "spawn"}
    matches &= spawned
    for command in commands:
        op = command["operation"]
        if op["kind"] != "spawn":
            continue
        member, birth = op["particle"], op.get("birth", {})
        expected = bytearray(bank["declarations"][member][:24])
        changes = [(8 + axis * 2,2,value) for axis,value in enumerate(birth.get("uv", [])) if value]
        if birth.get("geometry_count"):
            changes.append((18,1,birth["geometry_count"]))
        if not changes:
            continue
        for offset, size, value in changes:
            number = value["min"]
            if number != value["max"] or not float(number).is_integer() or not 0 <= number < 1 << (size * 8):
                break
            expected[offset:offset + size] = int(number).to_bytes(size,"big")
        else:
            if signature(expected) == actual:
                matches.add(member)
    return sorted(matches)


def damage_context(state, battle):
    """Read conservative immutable prerequisites; volatile guards are watched."""
    history = bytes(state.ram[0x1fabd0:0x1fabf0])
    result = []
    for actor in battle["actors"]:
        at, blocked = actor["address"], []
        profile, saved, weapon = state.read(at + 4), state.read(at), state.read(at + 0x10)
        if saved:
            if not 0x80000000 <= saved <= 0x817ffe00:
                blocked.append("persistent actor pointer is invalid")
            elif any(state.read(saved + 0xee + i, "B") for i in range(4)):
                blocked.append("equipped EX damage modifiers")
            if actor["side"] == 0 and state.read(saved + 0x24, "b") >= 100:
                blocked.append("progression damage modifier")
        if weapon:
            if not 0x80000000 <= weapon <= 0x817fffe8:
                blocked.append("weapon pointer is invalid")
            elif 0x61 <= state.read(weapon + 0x14, "B") <= 0x6c:
                blocked.append("weapon species modifier")
        count = history.count(state.read(profile + 0x2a, "H") & 255)
        result.append(dict(blocked=blocked, familiarity=100 + (count >= 4) + (count >= 12) + 2 * (count >= 16)))
    return result


def contact_damage_range(row, prefix, setup, owner, target, previous_hp, strength, *, critical=None):
    contexts = setup.get("damage_context")
    if not contexts:
        raise damage_envelope.UnsupportedDamage("ready damage context was not observed")
    slots = [entry["source_slot"] for entry in setup["source_actor_map"]]
    for actor in (owner, target):
        p = f"actor_{slots[actor]}_"
        if contexts[actor]["blocked"]:
            raise damage_envelope.UnsupportedDamage("; ".join(contexts[actor]["blocked"]))
        conditions = [row[p + f"condition_{offset:x}"] for offset in (0x10,0x14,0x38,0x3c)]
        conditions += [row[p + f"raw_{offset:x}"] for offset in (0x1d0,0x1d4)]
        if any(conditions) or row[p + "raw_1aa8"] & 0x03000060 or row[p + "control_word"] & 0xe0:
            raise damage_envelope.UnsupportedDamage("live condition/equipment/charge modifier")
        if ((row[p + "raw_280"] >> 8) & 0x41) or ((row[p + "control_word"] >> 16) & 7) == 4:
            raise damage_envelope.UnsupportedDamage("live guard or special control class")
    if (row["damage_session_1f44"] >> 16 or row["damage_session_1f5c"] >> 8 or
        row["damage_battle_15728"] >> 24 or (row["damage_battle_15714"] >> 24) & 8):
        raise damage_envelope.UnsupportedDamage("session, Unison or All Divide modifier")
    p, t = f"actor_{slots[owner]}_", f"actor_{slots[target]}_"
    if row[t + "condition_5c"] & 4:
        raise damage_envelope.UnsupportedDamage("forced one-damage target")
    element = (row[prefix + "_rule_flags_element"] >> 8) & 255
    if element == 10:
        element = 0
    elif not element:
        packed = row[p + "raw_1e0"]
        element = ((packed >> 8) & 255) or (packed & 255)
        if not element:
            if setup["actors"][owner]["side"] == 0:
                element = row[p + "raw_1e4"] >> 24
            else:
                element = row[p + "condition_0"] >> 24
    if not 0 <= element <= 8:
        raise damage_envelope.UnsupportedDamage("unbound element")
    offset = 1 + element
    affinity = (row[t + f"condition_{offset // 4 * 4:x}"] >> (24 - (offset % 4) * 8)) & 255
    power_word = row[prefix + "_rule_power"]
    power_mode = (power_word >> 16) & 255
    power = "normal" if power_mode in (0,2) else {"percent": power_word & 65535} if power_mode == 1 else None
    kind_value = (row[prefix + "_contact_kind"] >> 16) & 255
    if kind_value > 2:
        raise damage_envelope.UnsupportedDamage("unbound contact damage kind")
    kind = ("slash", "thrust", "magic")[kind_value]
    strength = row[p + "cast_strength_word"] >> 24 if kind == "magic" else strength
    proficiency = tuple((row[p + "proficiency_word"] >> shift) & 255 for shift in (24,16,8))
    context = damage_envelope.Context(strength, proficiency, contexts[target]["familiarity"],
        False if kind == "magic" else critical, row[t + "target_state_word"] >> 24 == 17,
        "normal" if affinity == 0 else "unsupported", (), True)
    bounds = damage_envelope.envelope(dict(kind=kind, power=power), setup["actors"][owner], setup["actors"][target], context)
    bounds = damage_envelope.hp_loss_bounds(bounds, previous_hp, prevents_defeat=False)
    return [bounds["minimum"], bounds["maximum"]]


def projectile_damage_range(row, prefix, setup, owner, target, previous_hp):
    return contact_damage_range(row, prefix, setup, owner, target, previous_hp, row[prefix + "_144"] >> 16)


def contact_damage_branches(row, prefix, setup, owner, target, previous_hp, strength, loss):
    ordinary = contact_damage_range(row, prefix, setup, owner, target, previous_hp, strength, critical=False)
    magic = (row[prefix + "_contact_kind"] >> 16) & 255 == 2
    critical = None if magic else contact_damage_range(
        row, prefix, setup, owner, target, previous_hp, strength, critical=True)
    ordinary_matches = ordinary[0] <= loss <= ordinary[1]
    critical_matches = critical is not None and critical[0] <= loss <= critical[1]
    outcome = critical_matches if ordinary_matches != critical_matches else None
    return dict(critical=outcome, damage_branches=dict(ordinary=ordinary, critical=critical))


def attached_contact(row, setup, owner, target):
    """Bind this update's queue; equal consecutive nominal rolls remain valid."""
    if "contact_queue_counts" not in row:
        return None
    slot = setup["source_actor_map"][owner]["source_slot"]
    target_slot = setup["source_actor_map"][target]["source_slot"]
    if row.get(f"actor_{target_slot}_fresh_contact_word",0) >> 24 != 1:
        return None
    victim = setup["initial_battle"]["actors"][target]["address"]
    if row.get(f"actor_{slot}_contact_target") != victim:
        return None
    owner_address = setup["initial_battle"]["actors"][owner]["address"]
    candidates = {}
    for side in range(2):
        count = (row["contact_queue_counts"] >> (16 if side == 0 else 0)) & 65535
        if count > 4:
            raise damage_envelope.UnsupportedDamage("current collision queue exceeds its bounded observer")
        for index in range(count):
            prefix = f"contact_queue_{side}_{index}"
            # Multiple attacking owners need an actual per-target dispatcher
            # result. The generic neutral fixture must not infer that link.
            if row[prefix + "_owner"] != owner_address:
                return None
            if row[prefix + "_projectile"]:
                return None
            if not row[prefix + "_projectile"] and row[prefix + "_rule"] and row[prefix + "_template"]:
                candidates[(row[prefix + "_rule"],row[prefix + "_template"])] = prefix
    return next(iter(candidates.values())) if len(candidates) == 1 else None


def normalize(case, rows, setup, banks, expected, images):
    """Never promote appearance, action start or arbitrary HP loss to a hit."""
    owner, target = setup["owner"], setup["target"]
    source_owner = setup["source_actor_map"][owner]["source_slot"]
    addresses = {a["address"]: i for i, a in enumerate(setup["initial_battle"]["actors"])}
    slots = [a["slot"] for a in setup["initial_battle"]["actors"]]
    frames, events, missing = [], [], set()
    start_tick, selected_tick, returned, action = None, None, False, 0
    ordinal, previous_nodes, serial = {}, {}, Counter()
    previous_age, particle_identity, observed_particle_births, projectile_births = {}, {}, set(), set()
    previous_result, live_programs_old, program_serial, declaration_seen = {}, {}, 0, set()
    model_counts, model_ordinals, semantics = Counter(), {}, []
    diagnostics = {"Body bone0 world is the last completed display publication with an unobserved timestamp; body bone geometry is outside this test. Actor placement, bound clip/frame and deterministic effect transforms remain observed separately."}
    recovery_seen, program_history = False, {}
    counts = observation_counts(rows[0]) if rows else {}

    def follow(pointer, projectiles_by_address):
        if pointer is None:
            return {"kind": "unobserved"}
        if pointer == 0:
            return None
        for actor in setup["initial_battle"]["actors"]:
            for delta, kind in ((0x18c0, "actor"), (0x195c, "center")):
                if pointer == actor["address"] + delta:
                    return dict(kind=kind, actor=addresses[actor["address"]])
        for address, pid in projectiles_by_address.items():
            if pointer == address + 0x40:
                return dict(kind="projectile", projectile=pid)
        return {"kind": "unresolved"}


    def identity(label, address, present, age):
        key = (label, address)
        if address not in previous_nodes.get(label, set()) or age < previous_age.get(key, age):
            serial[label] += 1
            ordinal[key] = serial[label]
        previous_age[key] = age
        present.add(address)
        return ordinal[key]

    for index, row in enumerate(rows):
        if row.get("battle_pool") != setup["initial_battle"].get("pool", row.get("battle_pool")):
            raise ValueError("initialized battle pool changed during the compact observation")
        combat, presentation = row["battle_combat_tick_word"], row["battle_presentation_tick_word"]
        common = dict(presentation_tick=presentation, combat_tick=combat)
        actors, body_models = [], []
        for actor_id, slot in enumerate(slots):
            base = f"battle_actor_{slot}"
            actors.append(dict(actor=actor_id, side=int(slot >= 4), hp=row[base + "_hp"],
                tp=row[base + "_tp_word"] & 65535,
                position=[f32(row[base + f"_{axis}_bits"]) for axis in "xyz"],
                heading=f32(row[base + "_heading_bits"])))
            if ((row[f"actor_{slot}_control_word"] >> 16) & 7) == 4:
                missing.add("unrequested OverLimit altered the neutral execution fixture")
            clip, frame = observed_body(row, slot, setup.get("body_resources", {}).get(str(slot), {}))
            if frame is None and start_tick is not None:
                missing.add("requested body clip lacks a matched current/pending animation controller")
            body_models.append(dict(actor=actor_id, clip=clip,
                frame=frame, world=[f32(row[f"actor_{slot}_root_{word}"]) for word in range(12)]))
        observed = {key: row[f"actor_{source_owner}_{key}"] & (65535 if key == "native" else 0xffffffff) for key in expected}
        action_word = row[f"battle_actor_{source_owner}_action"]
        selected = action_word >> 24 == 5 and all(observed[k] == value for k, value in expected.items())
        if selected and selected_tick is None:
            selected_tick = combat
        # Phase zero is the pending selection. The ordinary action dispatcher
        # installs the bound tracks/cast and commits payment in phase one.
        active = selected and ((action_word >> 16) & 255) != 0
        if active and start_tick is None:
            action = 1
            events.append(dict(common, kind="started", owner=owner, action=action))
            if start_tick is None:
                start_tick = combat
        if start_tick is not None and row[f"battle_actor_{source_owner}_action"] >> 24 == 3 and not returned:
            returned = True
            events.append(dict(common, kind="completed", action=action))
            semantics.append(dict(common, kind="returned", owner=owner))
        if start_tick is not None and not recovery_seen and row[f"battle_actor_{source_owner}_action"] >> 24 == 8:
            recovery_seen = True
            semantics.append(dict(common, kind="recovery_started", owner=owner, action=1))
        if index and start_tick is not None:
            spent = (rows[index - 1][f"battle_actor_{source_owner}_tp_word"] & 65535) - actors[owner]["tp"]
            if spent > 0:
                semantics.append(dict(common, kind="payment", owner=owner, amount=spent))
        if start_tick is not None and (row["battle_menu_flags_word"] & MENU_ACTIVE or (row["battle_pause_flags_word"] >> 16)):
            # Scene-controlled pauses are distinguished in the raw observations;
            # they cannot serve as action execution or damage evidence.
            missing.add("menu_or_pause_observed; inspect scene/input context")
        projectiles, models, particles, present, contacts, projectile_prefixes = [], body_models, [], set(), [], {}
        nodes, complete = tracked_list(row, "projectile", counts["projectile"])
        if not complete:
            missing.add("projectile linked list exceeds observer or cycles")
        for node_index, address in nodes:
            prefix = f"projectile_{node_index}"
            get = lambda offset: row[f"{prefix}_{offset:x}"]
            lifecycle = row.get(prefix + "_14")
            if lifecycle is not None and lifecycle >> 24 != 1:
                # Allocated state0 has stale payload. State2 is only the final
                # initialized sample of an already observed generation.
                if lifecycle >> 24 != 2 or address not in previous_nodes.get("projectile", set()):
                    continue
            age, contact_active, disarmed = projectile_contact(get)
            pid = identity("projectile", address, present, age)
            projectile_prefixes[pid] = prefix
            projectile_owner = addresses.get(get(0x28))
            item = dict(id=pid, owner=projectile_owner, target=addresses.get(get(0x2c)),
                position=[f32(get(at)) for at in (0x40, 0x44, 0x48)],
                velocity=[f32(get(at)) for at in (0x4c, 0x50, 0x54)],
                age=age, contact_active=contact_active, disarmed=disarmed)
            projectiles.append(item)
            result = get(0x280) & 255
            if result and result != previous_result.get(pid, 0):
                contacts.append((pid, projectile_owner, item["target"], result))
            previous_result[pid] = result
            if pid not in projectile_births:
                events.append(dict(common, kind="projectile_started", owner=projectile_owner,
                    target=item["target"], projectile=pid,
                    action=action if projectile_owner == owner and start_tick is not None else None))
                projectile_births.add(pid)
        for address in previous_nodes.get("projectile", set()) - present:
            if complete:
                events.append(dict(common, kind="projectile_expired", projectile=ordinal[("projectile", address)]))
        projectile_addresses = {address: ordinal[("projectile", address)] for address in present}
        previous_nodes["projectile"] = present if complete else previous_nodes.get("projectile", set()) | present
        alternate_nodes, complete_alternate = tracked_list(row, "alternate", counts["alternate"]) if counts["alternate"] else ([], row.get("effect_alternate_head") == row["battle_pool"] + 0x76660)
        if not complete_alternate:
            missing.add("alternative group7 linked list exceeds observer or lacks payload observations")
        program_nodes, complete_programs = tracked_list(row, "program", counts["program"])
        program_records = [(f"program_{i}",address) for i,address in program_nodes]
        program_records += [(f"alternate_{i}",address) for i,address in alternate_nodes if row[f"alternate_{i}_10"] >> 24 == 0]
        complete_programs &= complete_alternate
        live_programs = {address for prefix,address in program_records if row[prefix + "_10"] >> 24 == 0}
        if not complete_programs:
            missing.add("program linked list exceeds observer or cycles")
        programs, program_now, previous_program_bindings = [], {}, live_programs_old.copy()
        for prefix, address in program_records:
            if row[prefix + "_10"] >> 24 != 0:
                # Kind6 is the separately dispatched numeric overlay family.
                if row[prefix + "_10"] >> 24 != 6:
                    missing.add("unclassified node in program list")
                continue
            base, root = row[prefix + "_60"], row[prefix + "_6c"]
            bank = banks.get(base)
            if bank is None or root - base not in bank["roots"]:
                missing.add("program bank/root identity incomplete")
                continue
            signature = (base, root, row[prefix + "_2c"])
            age = signed16(row[prefix + "_5c"] & 65535)
            prior = live_programs_old.get(address)
            if prior is None or prior["signature"] != signature or age < prior["age"]:
                program_serial += 1
                pid = program_serial
                event = dict(common, kind="program_started", id=pid, owner=addresses.get(signature[2]),
                             bank=bank["source_sha256"], program=bank["roots"].index(root - base))
                semantics.append(event)
                program_history[pid] = event
            else:
                pid = prior["id"]
            item = dict(id=pid, owner=addresses.get(signature[2]), bank=bank["source_sha256"],
                        program=bank["roots"].index(root - base), age=age,
                        origin=[f32(row[prefix + f"_{at:x}"]) for at in (0x48,0x4c,0x50)],
                        attachment=follow(row.get(prefix + "_44"), projectile_addresses))
            programs.append(item)
            program_now[address] = dict(id=pid, signature=signature, age=age)
        if complete_programs:
            for address, prior in live_programs_old.items():
                if address not in program_now or program_now[address]["id"] != prior["id"]:
                    original = program_history[prior["id"]]
                    events.append(dict(common, **{k:original[k] for k in ("id","owner","bank","program")}, kind="program_expired"))
            live_programs_old = program_now
        else:
            live_programs_old.update(program_now)
        # Embedded schedulers can remain bound after END. Only an observed
        # binding transition or clock reset proves a new start.
        if index:
            for actor_id, slot in enumerate(slots):
                if embedded_start(row, rows[index - 1], slot):
                    base, root = row[f"actor_{slot}_effect_bank"], row[f"actor_{slot}_effect_program"]
                    bank = banks.get(base)
                    if bank is None or root - base not in bank["roots"]:
                        missing.add("embedded program start lacks exact bank/root identity")
                    else:
                        semantics.append(dict(common, kind="program_started", owner=actor_id,
                            bank=bank["source_sha256"], program=bank["roots"].index(root - base)))
        particle_nodes, complete_particles = tracked_list(row, "particle", counts["particle"])
        if not complete_particles:
            diagnostics.add("cosmetic particle linked list exceeds bounded observer")
            missing.add("effect particle list is incomplete; timed declaration activations may be unobserved")
        particle_records = [(f"particle_{i}",address) for i,address in particle_nodes]
        particle_records += [(f"alternate_{i}",address) for i,address in alternate_nodes if row[f"alternate_{i}_10"] >> 24 == 1]
        complete_particles &= complete_alternate
        present, effect_models = set(), []
        for prefix, address in particle_records:
            get = lambda offset: row[f"{prefix}_{offset:x}"]
            if get(0x10) >> 24 != 1:
                continue
            copied = b"".join(struct.pack(">I", get(at)) for at in range(0x28, 0x40, 4))
            initialized = particle_readiness(get(0x10), copied)
            particle_age = signed16(get(0x278) >> 16) if initialized else 0
            # Do not let stale allocator age create a second generation when
            # this same copied declaration initializes on the next update.
            pid = identity("particle", address, present, particle_age)
            bank = banks.get(row[prefix + "_parent_bank"])
            matches = declaration_matches(bank,copied,row[prefix + "_parent_program"] - row[prefix + "_parent_bank"]) if bank else []
            parent_live = get(0x184) - 0x28 in live_programs
            parent_live |= retired_parent_birth(get(0x184),
                (row[prefix + "_parent_bank"], row[prefix + "_parent_program"], row[prefix + "_parent_owner"]),
                particle_age, previous_program_bindings)
            embedded = next((a for a in setup["initial_battle"]["actors"]
                             if a["address"] + 0x15b4 == get(0x184)), None)
            if embedded:
                slot = embedded["slot"]
                parent_live = bool(row[f"actor_{slot}_effect_program"] and
                    row[f"actor_{slot}_effect_bank"] == row[prefix + "_parent_bank"] and
                    row[f"actor_{slot}_effect_program"] == row[prefix + "_parent_program"])
            member = matches[0] if len(matches) == 1 else None
            if pid not in particle_identity and bank is not None and member is not None and parent_live:
                relative_program = row[prefix + "_parent_program"] - row[prefix + "_parent_bank"]
                if relative_program in bank["roots"]:
                    particle_identity[pid] = (bank, member, addresses.get(row[prefix + "_parent_owner"]))
            bound = particle_identity.get(pid)
            if bound is None:
                diagnostics.add("sampled particle birth identity incomplete")
            else:
                bank, member, particle_owner = bound
            metadata = bank["metadata"]["actors"][member] if bound else {}
            if bound:
                activation = (particle_owner, bank["source_sha256"], member, combat)
                if pid not in observed_particle_births and particle_age <= 1 and activation not in declaration_seen:
                    declaration_seen.add(activation)
                    semantics.append(dict(common, kind="declaration_started", owner=particle_owner,
                                          bank=bank["source_sha256"], member=member))
                elif pid not in observed_particle_births and particle_age > 1:
                    diagnostics.add("some declaration first samples occur after birth")
                observed_particle_births.add(pid)
            origin = [f32(get(at)) for at in (0x170, 0x174, 0x178)] if initialized else None
            offset = [f32(get(at)) for at in (0x5c, 0x60, 0x64)] if initialized else None
            model = None
            if initialized and metadata.get("kind") == "model_particle":
                binding = metadata.get("binding", {})
                model = dict(clip=binding.get("animation"), frame=None, world=None)
                model["world"] = static_model_world(origin, offset,
                    [f32(get(at)) for at in (0x80, 0x84, 0x88)],
                    [f32(get(at)) for at in (0xd8, 0xdc, 0xe0)], get(0x3c), f32(get(0x16c)))
                if model["world"] is None or binding.get("animation") is not None:
                    missing.add("attached or animated particle model requires its additional transform/clock observation")
            attachment = (follow(row.get(prefix + "_1c"), projectile_addresses)
                          if get(0x3c) & 0x400 else None)
            if model is not None:
                if pid not in model_ordinals:
                    key = (particle_owner, bank["source_sha256"], member)
                    model_counts[key] += 1
                    model_ordinals[pid] = model_counts[key]
                    if particle_age > 1:
                        missing.add("deterministic model birth was not observed")
                if isinstance(attachment, dict) and attachment["kind"] in ("unobserved", "unresolved"):
                    missing.add("deterministic model attachment incomplete")
                effect_models.append(dict(owner=particle_owner, bank=bank["source_sha256"], member=member,
                                          ordinal=model_ordinals[pid], attachment=attachment, model=model))
            particles.append(dict(id=pid, owner=particle_owner if bound else None,
                bank=bank["source_sha256"] if bank else None, member=member,
                initialized=initialized, origin=origin, offset=offset,
                owner_bone=(get(0xb4) >> 24) if initialized and get(0x3c) & 0x40000000 else None, model=model))
        # An incomplete prefix cannot establish death; retain generation identity.
        previous_nodes["particle"] = present if complete_particles else previous_nodes.get("particle", set()) | present
        if index and start_tick is not None:
            # Exact owner contact target plus the recipient's HP transition.
            # A simultaneous competing attacker makes attribution ambiguous.
            for recipient in actors:
                slot = slots[recipient["actor"]]
                loss = rows[index - 1][f"battle_actor_{slot}_hp"] - recipient["hp"]
                victim = next(at for at, value in addresses.items() if value == recipient["actor"])
                # A persistent prior victim pointer is not a new hit event.
                latch = (row[f"actor_{source_owner}_contact_target"] == victim and
                         rows[index - 1][f"actor_{source_owner}_contact_target"] != victim)
                competitors = [s for s in slots if s not in (source_owner, slot) and
                               row[f"actor_{s}_contact_target"] == victim and
                               rows[index - 1][f"actor_{s}_contact_target"] != victim]
                if loss > 0:
                    candidates = [entry for entry in contacts if entry[1:3] == (owner, recipient["actor"])]
                    try:
                        attached = attached_contact(row, setup, owner, recipient["actor"])
                    except damage_envelope.UnsupportedDamage as error:
                        attached = None
                        missing.add(str(error))
                    attributed = not competitors and (len(candidates) == 1 or latch or attached is not None)
                    projectile = candidates[0][0] if len(candidates) == 1 else None
                    event = dict(common, kind="hit" if attributed else "unattributed_hp_loss",
                        owner=owner if attributed else None, target=recipient["actor"], hp_change=-loss,
                        action=1 if attributed else None, projectile=projectile)
                    if attributed and projectile is not None:
                        try:
                            event["damage_range"] = projectile_damage_range(row, projectile_prefixes[projectile], setup,
                                owner, recipient["actor"], recipient["hp"] + loss)
                        except (damage_envelope.UnsupportedDamage, KeyError) as error:
                            missing.add(f"damage interval unavailable: {error}")
                    elif attributed:
                        try:
                            if attached is None:
                                raise damage_envelope.UnsupportedDamage("attached contact rule identity is not yet observed")
                            event["damage_range"] = contact_damage_range(row, attached, setup, owner,
                                recipient["actor"], recipient["hp"] + loss,
                                signed16(row[f"actor_{source_owner}_attack_power_word"] & 65535))
                        except (damage_envelope.UnsupportedDamage, KeyError) as error:
                            missing.add(f"damage interval unavailable: {error}")
                    if attributed and "damage_range" in event:
                        prefix = projectile_prefixes[projectile] if projectile is not None else attached
                        strength = (row[prefix + "_144"] >> 16 if projectile is not None else
                                    signed16(row[f"actor_{source_owner}_attack_power_word"] & 65535))
                        event.update(contact_damage_branches(row, prefix, setup, owner,
                            recipient["actor"], recipient["hp"] + loss, strength, loss))
                    events.append(event)
                elif loss < 0:
                    controlled = (case.get("context") in ("support", "revival") and recipient["actor"] == target and
                                  setup.get("opponent_control", {}).get("mode") == "manual" and
                                  all(a.get("conditions") == [] for a in setup["actors"]) and
                                  not any(row[f"battle_actor_{s}_action"] >> 24 == 5 for s in slots if s != source_owner))
                    semantics.append(dict(common, kind="recovered" if controlled else "unattributed_hp_gain",
                        owner=owner if controlled else None, target=recipient["actor"], action=1 if controlled else None, amount=-loss))
        frame = dict(common, action_age=(row[f"actor_{source_owner}_clock"] >> 16) if start_tick is not None and not returned else None,
            action_age_basis="observed authored action elapsed clock; null after returned idle",
            actors=actors, models=models, projectiles=projectiles, particles=particles,
            effect_models=effect_models, programs=programs,
            authored_action_clock=row[f"actor_{source_owner}_clock"],
            source_random_state=row["battle_random_state_word"],
            image=images[index] if images else None)
        frames.append(frame)
        if returned and row[f"battle_actor_{source_owner}_action"] >> 24 == 3 and observation_tail_empty(row, counts):
            break
    started = next((event for event in events if event["kind"] == "started"), None)
    if started is not None:
        # Keep preroll in raw observations. The shared comparison starts once,
        # at the witnessed requested action, without per-event realignment.
        first = next(i for i, frame in enumerate(frames) if
                     frame["presentation_tick"] == started["presentation_tick"])
        frames = frames[first:]
        events = [event for event in events if event["presentation_tick"] >= started["presentation_tick"]]
    if started:
        setup["input"]["request_after_updates"] = selected_tick - setup["initial_battle"]["combat_tick"]
        setup["input"]["request_delay_basis"] = "observed requested selection minus retained ready combat tick; initialized bound admission is separate"
        setup["input"]["bound_admission_after_updates"] = started["combat_tick"] - setup["initial_battle"]["combat_tick"]
        semantics += [e.copy() for e in events if e["kind"] in ("started", "hit", "unattributed_hp_loss", "projectile_started", "projectile_expired")]
        semantics = [e for e in semantics if e["presentation_tick"] >= started["presentation_tick"]]
        for event in semantics:
            event.pop("id", None)  # allocator identity is diagnostic, not program behavior
            event["clock"] = event.pop("combat_tick") - started["combat_tick"]
            event["presentation_clock"] = event.pop("presentation_tick") - started["presentation_tick"]
    limits = set(missing)
    if setup.get("opponent_control", {}).get("mode") != "manual":
        limits.add("opponent automatic fallback choices do not match the neutral native fixture")
    if any(e["kind"] == "hit" and "damage_range" not in e for e in semantics):
        limits.add("source rule/stat damage interval not yet supplied")
    if started is None:
        limits.add("requested action admission was not observed")
    if not returned:
        limits.add("requested action did not return to idle")
    if rows and not observation_tail_empty(row, counts):
        limits.add("capture ended before complete projectile/effect retirement; no persistent baseline was established")
    outcome = case.get("context", "attack")
    required = "hit" if outcome in ("attack", "aerial") else "recovered" if outcome in ("support", "revival") else None
    if required and not any(e["kind"] == required for e in semantics):
        limits.add(f"intended {outcome} outcome was not attributed to the requested action")
    # Every actually started authored program must have its emitted declaration
    # families positively observed; random instance counts/positions are diagnostic.
    banks_by_sha = {b["source_sha256"]:b for b in banks.values()}
    for event in semantics:
        if event["kind"] == "program_started":
            bank = banks_by_sha[event["bank"]]
            for command in bank["metadata"]["programs"][event["program"]]:
                operation = command["operation"]
                if operation["kind"] == "spawn" and not any(item[:3] == (event["owner"], event["bank"], operation["particle"]) for item in declaration_seen):
                    limits.add("started effect program has an unobserved declaration family")
    if case.get("context") not in ("attack", "support", "revival", "aerial"):
        limits.add("selected contextual outcome has no implemented source observer")
    return dict(case=case["id"], setup=setup, frames=frames,
        events=sorted(semantics, key=lambda e: (e["clock"], e["presentation_clock"])),
        assessment=dict(outcome=case.get("context", "attack"), rng_streams_matched=False,
                        diagnostic_limits=sorted(diagnostics), trace_schema=2),
        incomplete_observations=sorted(limits))


def party_case(case):
    result = dict(case)
    capabilities = case["capabilities"]
    if capabilities["target"] == "unavailable":
        raise ValueError("catalogue binding requires an unavailable ordinary-input context")
    if capabilities["revives"]:
        result["target_dead"] = True
    if capabilities["aerial"]:
        result["input"] = "aerial"
    if case["character"] == 2:
        result["colette_state"] = 1
    return result


def prepare_party_ready(worker, output, record, packages):
    """Arrange the neutral opponent before its first choice, then settle entry."""
    marker = record["fixture"]["case_marker"]
    path = output / "warm.s01"
    source = output / ("resume-warm.s01" if record.get("resume_warm") else "entry.s01")
    for attempt in range(9):
        worker.warmup(source, output / f"entry-warm-{attempt}", marker,
                      180 if not attempt and not record.get("resume_warm") else 30)
        worker.snapshot(path)
        # Actor storage can precede camera allocation. The full inspector
        # requires a real camera; use the registered pointer before invoking it.
        if not worker.watcher.latest.get("battle_camera_address"):
            source = path
            continue
        state = State(path)
        battle = inspect(state, battle=True)["battle"]
        if battle["combat_tick"]:
            raise ValueError("party checkpoint passed first choice before setup")
        if battle.get("camera", {}).get("address") and battle["actors"] and all(a["action"][0] == 13 for a in battle["actors"]):
            break
        source = path
    else:
        raise ValueError("party entry did not initialize")
    manual_opponents(state, battle)
    changes = list(state.changes)
    state.save(path)
    neutral_movie(source, Path(str(path) + ".new.dtm"), inspect(state)["movie"]["input_count"], polls=16000)
    Path(str(path) + ".dtm").unlink()
    Path(str(path) + ".new.dtm").rename(str(path) + ".dtm")
    # Entry ends through its ordinary path; opponents use declared neutral manual control.
    for attempt in range(16):
        worker.warmup(path, output / f"settle-{attempt}", marker, 30)
        worker.snapshot(path)
        state = State(path)
        battle = inspect(state, battle=True)["battle"]
        owner = next(a for a in battle["actors"] if a["slot"] == 0)
        if battle["phase"] == 3 and not battle["pause_flags"] and not battle["menu_flags"] & MENU_ACTIVE and owner["action"][0] == 3:
            break
    else:
        raise ValueError("ordinary combat idle was not attained after entry")
    # Reuse the learned/shortcut checks; its old scheduled input is deliberately far away.
    ready = battle_artes.prepare_ready(output, record)
    return ready, changes


def manual_opponents(state, battle, owner=0):
    """Declared manual fixture; preserve actor state, profile, damage and reactions."""
    for actor in battle["actors"]:
        if actor["slot"] != owner:
            at = actor["address"]
            state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38)
            state.write(at + 0x107d, "B", (state.read(at + 0x107d, "B") & 0x3f) | 0xc0)


def restore_authored_selection(state, battle, assets, packages):
    """Undo only the former bridge's guarding-target suppression, with preimages."""
    formation = read(assets / "battle/formations.json")["records"][battle["formation"]]
    enemy_ids = {formation["resources"][a["resource"]]["enemy"] for a in formation["actors"]}
    for enemy in enemy_ids:
        raw = (packages / f"{enemy:03}.bin").read_bytes()
        package, _ = enemy_artes.enemy_package(state, raw)
        start, rows = enemy_artes.section(raw, 10)
        for index in range(len(rows) // 68):
            at = package + start + index * 68
            flags, operand = struct.unpack_from(">IH", rows, index * 68 + 8)
            actual = (state.read(at + 8), state.read(at + 12, "H"))
            if actual not in ((flags, operand), (flags | 0x10000, 5)):
                raise ValueError("retained opponent selection differs beyond documented old assistance")
            if actual != (flags, operand):
                state.write(at + 8, "I", flags)
                state.write(at + 12, "H", operand)


def arrange_party(path, record, case):
    state = State(path)
    battle = inspect(state, battle=True)["battle"]
    if (battle["mode"] != 9 or battle["phase"] != 3 or battle["pause_flags"] or battle["menu_flags"] & MENU_ACTIVE):
        raise ValueError("source is not ordinary unpaused combat before input")
    owner = next(a for a in battle["actors"] if a["slot"] == 0)
    opponents = [a for a in battle["actors"] if a["side"] == 1]
    if len(opponents) != 1 or owner["action"][0] not in (2, 3):
        raise ValueError("generic party fixture requires one opponent and an idle owner")
    target = (next((a for a in battle["actors"] if a["slot"] == 1), owner)
              if case.get("target_dead") else owner) if case["fixture"] == "recovery" else opponents[0]
    for actor in battle["actors"]:
        x = 0.0 if actor["slot"] == 0 else 100.0 if actor["slot"] == target["slot"] else 1800.0
        for offset, value in ((0x18c0, x), (0x18c8, 0.0),
                              (0x195c, actor["center"][0] + x - actor["position"][0]),
                              (0x1964, actor["center"][2] - actor["position"][2])):
            state.write(actor["address"] + offset, "f", value)
    state.write(owner["address"] + 0x1990, "I", target["address"])
    state.save(path)
    # The input window is relative to the observed state, not a guessed entry duration.
    cursor = inspect(state)["movie"]["input_count"]
    movie = Path(str(path) + ".dtm")
    temporary = path.with_suffix(".input.dtm")
    neutral_movie(path, temporary, cursor, polls=16000)
    data = bytearray(temporary.read_bytes())
    press = cursor + 16
    if case.get("input") == "aerial":
        for poll in range(press, press + 32):
            data[256 + poll * 8 + 5] = 255
        press += 40
    for poll in range(press, press + 4):
        struct.pack_into("<H", data, 256 + poll * 8, 0x4000 | DTM_BUTTONS["b"])
    movie.unlink()
    movie.write_bytes(data)
    temporary.unlink()
    return inspect(state, battle=True)["battle"], 0, target["slot"], dict(press_poll=press, button="b", polls=4), state.changes


def arrange_enemy_contact(base, path, initialized, case, assets):
    """Place a direct melee fixture within its authored body-distance interval."""
    row = case["definition"]
    attack = row.get("execution", {}).get("attack", {})
    if not any("contact" in event for _, event in attack.get("events", [])):
        return None
    minimum = max(row["range"][0], row.get("approach_minimum", 0))
    maxima = [v for v in (row["range"][1], row.get("approach_range", 0)) if v > 0]
    if not maxima or min(maxima) < minimum:
        raise ValueError("direct melee has no bounded authored placement interval")
    maximum = min(maxima)
    battle, owner = initialized["battle"], initialized["actor"]
    target = next(a for a in battle["actors"] if a["slot"] == 0)
    # These matrices precede fixture position edits. A ready state's matrices
    # can still reflect its prior placement and must not supply root offsets.
    enemy = battle_geometry.read_initialized_body(base, owner, assets / f"battle/enemies/{case['enemy']:03}.json")
    party = battle_geometry.read_initialized_body(base, target, assets / f"battle/party/{battle['party_order'][0]:02}.json")
    gap = (minimum + maximum) / 2
    separation = battle_geometry.separation_for_gap(enemy, party, gap)
    state = State(path)
    current = {a["slot"]:a for a in inspect(state, battle=True)["battle"]["actors"]}
    for original, x in ((owner, separation / 2), (target, -separation / 2)):
        actor = current[original["slot"]]
        for offset, value in ((0x18c0,x),(0x18c8,0.0),
                              (0x195c,actor["center"][0]+x-actor["position"][0]),
                              (0x1964,actor["center"][2]-actor["position"][2])):
            state.write(actor["address"]+offset,"f",value)
    state.save(path)
    return dict(selection_approach_intersection=[minimum,maximum], target_surface_gap=gap,
                root_separation=separation, owner_geometry=enemy, target_geometry=party,
                changes=state.changes, scope="Initialized body-distance placement only; actual contact must still be observed.")


def ready_models(snapshot, battle):
    """Read the bound main controller, with no guessed phase or loop mode."""
    models, proof = [], []
    for index, actor in enumerate(battle["actors"]):
        address = actor["address"]
        clip = snapshot.read(address + 0x1046, "B")
        table = snapshot.read(address + (0x1010 if clip < 100 else 0x1018))
        resource = table + (clip if clip < 100 else clip - 100) * 20
        scene = snapshot.read(address + 0x2b0)
        controller, seen = snapshot.read(scene + 0x5fc), set()
        while controller and controller not in seen:
            seen.add(controller)
            if snapshot.read(controller + 6, "H") == 6:
                break
            controller = snapshot.read(controller + 0x2c)
        if not controller or snapshot.read(controller + 6, "H") != 6:
            raise ValueError("ready actor has no bound main animation controller")
        fields = {name: snapshot.read(controller + offset, "f") for name, offset in
                  (("frame",8),("start",12),("end",16),("loop_start",20),("rate",24),("blend_duration",36),("blend_tick",40))}
        flags, child = snapshot.read(controller + 0x1c), snapshot.read(controller + 0x30)
        if snapshot.read(controller) != resource or child:
            raise ValueError("ready animation requires unsupported pending preparation")
        if not all(math.isfinite(v) for v in fields.values()):
            raise ValueError("ready animation contains nonfinite values")
        models.append(dict(actor=index, clip=clip, frame=fields["frame"],
                           rate=0.0 if flags & 2 else fields["rate"], repeat=not bool(flags & 8),
                           loop_start=fields["loop_start"]))
        proof.append(dict(actor=index, source_slot=actor["slot"], controller=controller,
                          resource=resource, flags=flags, child=child, **fields))
    return models, proof


def learned_techniques(snapshot, character, learning):
    member = battle_artes.member(snapshot, character)
    mask = snapshot.read(member + 0x70, "Q")
    if mask >> len(learning):
        raise ValueError("learned technique mask has bits outside the authored learning table")
    bindings = [(i, arte) for i, arte in enumerate(learning) if mask & (1 << i)]
    if any(not arte for _, arte in bindings):
        raise ValueError("learned technique mask selects an empty authored slot")
    return sorted({arte for _, arte in bindings}), dict(character=character,
        member=member, mask=mask, offset=0x70, bindings=bindings)


def setup_record(case, state, battle, owner, target, assistance, inputs, entry_path, learning):
    snapshot = State(state)
    entry = State(entry_path)
    globals_address = entry.read(0x8035a578)
    if not 0x80000000 <= globals_address <= 0x817fffac:
        raise ValueError("retained entry has no valid preparation globals")
    party = [value for value in battle["party_order"] if value]
    members, learned_proof = [], []
    for character in party:
        # Same persistent session layout as state.py/capture.py; the field
        # inspector intentionally stops early while a battle is loaded.
        at = battle_artes.member(snapshot, character)
        mask = snapshot.read(at + 0x110)
        techniques, learned = learned_techniques(snapshot, character, learning[character - 1])
        learned_proof.append(learned)
        members.append(dict(character=character, level=snapshot.read(at + 0x10, "B"),
            techniques=techniques,
            equipment=[snapshot.read(at + offset, "H") for offset in (0x4a, 0x4c, 0x4e, 0x52, 0x54, 0x50)],
            base_stats=[snapshot.read(at + offset, "H") for offset in (0x26, 0x28, 0x2a, 0x2c, 0x34, 0x32, 0x30)],
            luck=snapshot.read(at + 0x2e, "H") // 10,
            technique_balance=snapshot.read(at + 0x10c, "b"),
            ex_skills=[snapshot.read(at + 0xee + i, "B") for i in range(4)],
            ex_gems=[snapshot.read(at + 0xea + i, "B") for i in range(4)],
            compound_ex_skills=[i for i in range(24) if mask & (1 << i)]))
    actors = []
    for index, actor in enumerate(battle["actors"]):
        vitals = actor["vitals_address"]
        if snapshot.read(actor["address"] + 0x10a8) or snapshot.read(actor["address"] + 0x107d, "B") & 7 == 4:
            raise ValueError("neutral comparison fixture has active OverLimit gauge/status")
        profile = snapshot.read(actor["address"] + 4)
        if not 0x80000000 <= profile <= 0x817fffc0:
            raise ValueError("invalid initialized actor profile")
        conditions = (snapshot.read(actor["address"] + 0x1d0, "Q") |
                      snapshot.read(profile + 0x10, "Q") | snapshot.read(profile + 0x38, "Q"))
        if conditions:
            raise ValueError("initial nonzero condition fields need typed normalization before native replay")
        stats = {name: snapshot.read(vitals + offset, "H") for name, offset in
                 (("slash", 0x2c), ("thrust", 0x2e), ("defense", 0x30),
                  ("intelligence", 0x32), ("accuracy", 0x34), ("evasion", 0x36))}
        stats["level"] = snapshot.read(vitals + 0x1d, "B")
        persistent = snapshot.read(actor["address"])
        if actor["side"] == 0 and not 0x80000000 <= persistent <= 0x817fffdb:
            raise ValueError("initialized actor lacks its observed active-style record")
        stats["technique_balance"] = snapshot.read(persistent + 0x24, "b") if actor["side"] == 0 else None
        actors.append(dict(actor=index, source_slot=actor["slot"], side=actor["side"],
            **{key: actor[key] for key in ("hp", "max_hp", "tp", "max_tp", "position", "heading")},
            stats=stats, conditions=[],
            source_vitals=bytes(snapshot.ram[actor["vitals_address"] - 0x80000000:actor["vitals_address"] - 0x80000000 + 128]).hex()))
    canonical = {a["source_slot"]: a["actor"] for a in actors}
    source_actor_map = [{"actor": a["actor"], "source_slot": a.pop("source_slot"),
                         "vitals_hex": a.pop("source_vitals")} for a in actors]
    session = entry.read(0x8035a768)
    event_flags = [i for i in range(2048) if entry.read(session + 0xc9d + i // 8, "B") & (1 << (i % 8))]
    manual = all((snapshot.read(a["address"] + 0x107c, "B") & 0x38) == 0 and
                 snapshot.read(a["address"] + 0x107d, "B") >> 6 == 3
                 for a in battle["actors"] if a["slot"] != owner)
    body_resources = {}
    for a in battle["actors"]:
        normal, shared = snapshot.read(a["address"] + 0x1010), snapshot.read(a["address"] + 0x1018)
        body_resources[str(a["slot"])] = {str(clip): (normal + clip * 20 if clip < 100 else shared + (clip - 100) * 20)
                                         for clip in range(256)}
    poses, pose_proof = ready_models(snapshot, battle)
    return dict(case=case["id"], owner=canonical[owner], target=canonical[target], formation=battle["formation"],
        arena=battle["arena"], initial_battle=battle, seed=battle["random_state"]["entry_seed"],
        party=party, party_members=members, learned_technique_observations=learned_proof,
        actors=actors, source_actor_map=source_actor_map,
        body_resources=body_resources, ready_models=poses, ready_model_observations=pose_proof,
        damage_context=damage_context(snapshot, battle),
        story=entry.read(globals_address + 0x40, "i"),
        colette_state=entry.read(globals_address + 0x4c, "i"),
        battle_rank=entry.read(session + 0x19f, "B"),
        event_flags=event_flags, devils_arms_unlocked=0x3fa in event_flags,
        victory_story_flags=[flag in event_flags for flag in (27, 28)],
        map=entry.read(session + 0x10d0), world_music=entry.read(globals_address + 0x50, "i"),
        overlimit_boost=bool(entry.read(session + 0x1e3c, "H") & 4),
        assistance=assistance, input=inputs, ready_state=pin(state), ready_movie=pin(Path(str(state) + ".dtm")),
        entry_state=pin(entry_path),
        initial_action_power=[dict(actor=i, attack_power=snapshot.read(a["address"] + 0x106e, "h"),
                                  casting_strength=snapshot.read(a["address"] + 0x1a10, "B"))
                              for i, a in enumerate(battle["actors"])],
        opponent_control={"mode": "manual" if manual else "enemy", "port": 3 if manual else None,
            "automatic_guard_disabled": False,
            "scope": "Manual mode also changes control-specific heading/input bookkeeping; profile and reactions preserved."},
        observation_layout=pin(ROOT / "local/battle-capture-work/arte-equivalence/static-effect-layout.json"),
        source_statistics="Retained initialized state is authoritative; do not assume native defaults match.")


def run(args):
    assets = args.assets.resolve()
    selected = selected_cases(args)
    if args.resume_ready and len(selected) != 1:
        raise ValueError("a retained ready checkpoint belongs to exactly one case")
    if args.resume_warm and (len(selected) != 1 or args.resume_ready):
        raise ValueError("a retained partial entry belongs to one party case and cannot also be ready")
    output = enemy_artes.output_path(args.output)
    output.mkdir(parents=True, exist_ok=False)
    # Only global source/profile defaults are reused, never the old per-ID cases.
    profile = read(battle_artes.MANIFEST)
    profile["inputs"]["techniques"] = pin(assets / "game/techniques.json")
    profile["fixture_defaults"].update(hp=9999, tp=9999, press_after_polls=8000)
    enemy_manifest, worker, marker, outcomes, watched_slots = None, None, None, [], None
    try:
        for case in selected:
            destination = output / case["id"].replace(":", "-")
            try:
                if args.resume_ready:
                    source = args.resume_ready.resolve()
                    record = read(source.parent / "fixture.json")
                    if "character" not in case or (record["case"]["character"], record["case"]["arte"]) != (case["character"], case["arte"]):
                        raise ValueError("retained ready fixture does not match the requested party binding")
                    destination.mkdir()
                    for name, original in (("ready.s01", source), ("ready.s01.dtm", Path(str(source) + ".dtm")),
                                           ("entry.s01", source.parent / "entry.s01"),
                                           ("entry.s01.dtm", source.parent / "entry.s01.dtm"),
                                           ("input.dtm", Path(str(source) + ".dtm"))):
                        (destination / name).hardlink_to(original)
                    record["resume_ready"] = pin(source)
                    record["resume_movie"] = pin(Path(str(source) + ".dtm"))
                    write(destination / "fixture.json", record)
                elif args.resume_warm:
                    source = args.resume_warm.resolve()
                    record = read(source.parent / "fixture.json")
                    if "character" not in case or (record["case"]["character"], record["case"]["arte"]) != (case["character"], case["arte"]):
                        raise ValueError("retained partial entry does not match requested party binding")
                    destination.mkdir()
                    for name in ("entry.s01", "entry.s01.dtm", "input.dtm"):
                        shutil.copyfile(source.parent / name, destination / name)
                    shutil.copyfile(source, destination / "resume-warm.s01")
                    shutil.copyfile(Path(str(source) + ".dtm"), destination / "resume-warm.s01.dtm")
                    record["resume_warm"] = pin(source)
                    generated = party_case(case)
                    write(destination / "fixture.json", record)
                elif "character" in case:
                    generated = party_case(case)
                    manifest = dict(profile, cases=[generated])
                    options = SimpleNamespace(case=f"{case['character']}:{case['arte']}", output=destination,
                        state=None, techniques=None, manifest=assets / "game/techniques.json")
                    battle_artes.prepare_case(options, manifest)
                    record = read(destination / "fixture.json")
                    record["assets"] = str(assets)
                    # Saved OverLimit was protective assistance in the old
                    # observation adapter. This ordinary comparison has none.
                    entry = State(destination / "entry.s01")
                    for character in record["party"]:
                        entry.write(battle_artes.member(entry, character) + battle_artes.GAUGE, "B", 0)
                    entry.save(destination / "entry.s01")
                    record["entry_sha256"] = digest(destination / "entry.s01")
                    record["assistance"] = None
                    write(destination / "fixture.json", record)
                else:
                    if enemy_manifest is None:
                        enemy_manifest = build_catalogue(args.packages, assets, enemy_artes.MANIFEST.with_name("battle-formations.json"))
                        enemy_manifest["fixture_defaults"]["party_hp"] = 9999
                    source_id = f"enemy{case['enemy']:03}:row{case['action_row']:02}"
                    options = SimpleNamespace(case=[source_id], enemy=None, output=destination,
                        capture_vis=args.max_vis, manifest=assets / f"battle/enemies/{case['enemy']:03}/definition.json",
                        images=False, retain_states=True, guard=False, enemy_x=100.0, enemy_hp_percent=100)
                    enemy_artes.prepare(options, enemy_manifest)
                    record = read(destination / "fixture.json")
                    entry = State(destination / "entry.s01")
                    settings = entry.read(battle_artes.SETTINGS)
                    for slot in range(4):
                        character = entry.read(settings + battle_artes.PARTY_ORDER + slot, "B")
                        if character:
                            entry.write(battle_artes.member(entry, character) + battle_artes.GAUGE, "B", 0)
                    entry.save(destination / "entry.s01")
                    record["ordinary_gauge_changes"] = entry.changes
                    record["entry_sha256"] = digest(destination / "entry.s01")
                    write(destination / "fixture.json", record)
                current_marker = record["fixture"]["marker_address"]
                slots = ([a["slot"] for a in inspect(State(args.resume_ready), battle=True)["battle"]["actors"]]
                         if args.resume_ready else list(range(len(record["party"]))) + [4]
                         if "character" in case else record["expected_slots"])
                if worker is not None and (slots != watched_slots or
                    digest(destination / "input.dtm") != worker.metadata["movie_sha256"]):
                    worker.close(complete=False)
                    worker = None
                if worker is None:
                    marker = current_marker
                    watch, budget = observer_plan(slots)
                    watched_slots = slots
                    write(destination / "locations.json", watch)
                    write(destination / "observer.json", budget)
                    worker = BattleFixture(args.disc, destination / "input.dtm", destination / "entry.s01",
                        destination / "worker", marker, watch, disc_sha256=profile["inputs"]["disc_sha256"], capture_images=False)
                elif marker != current_marker:
                    raise ValueError("resident fixture marker changed")
                if args.resume_ready:
                    state_path = destination / "ready.s01"
                    resumed = State(state_path)
                    battle = inspect(resumed, battle=True)["battle"]
                    restore_authored_selection(resumed, battle, assets, args.packages)
                    manual_opponents(resumed, battle)
                    # The retained input is hardlinked; replace before writing.
                    state_path.unlink()
                    resumed.save(state_path)
                    record["manual_fixture_changes"] = resumed.changes
                    write(destination / "fixture.json", record)
                    del resumed
                    battle, owner, target, inputs, arrangement = arrange_party(state_path, record, party_case(case))
                    assistance = {"retained_ready": record["resume_ready"], "source_fixture": pin(args.resume_ready.parent / "fixture.json"),
                                  "changes": record["manual_fixture_changes"] + arrangement}
                    expected = {"native": case["native"]}
                    formation = read(assets / "battle/formations.json")["records"][battle["formation"]]
                    enemy_ids = [formation["resources"][a["resource"]]["enemy"] for a in formation["actors"]]
                elif "character" in case:
                    state_path, preselection_changes = prepare_party_ready(worker, destination, record, args.packages)
                    battle, owner, target, inputs, assistance = arrange_party(state_path, record, generated)
                    assistance = preselection_changes + assistance
                    expected = {"native": case["native"]}
                    enemy_ids = [144]  # The global formation399 fixture is verified below.
                    formations = read(assets / "battle/formations.json")["records"]
                    formation = formations[battle["formation"]]
                    enemy_ids = [formation["resources"][a["resource"]]["enemy"] for a in formation["actors"]]
                else:
                    base, initialized = enemy_artes.ready(worker, destination, record)
                    record["initialized_state"] = str(destination / "initialized.s01")
                    child = destination / "row"
                    child.mkdir()
                    chosen = record["cases"][0]
                    state_path = enemy_artes.prepare_row(base, child, record, initialized, chosen)
                    fixture = read(child / "fixture.json")
                    geometry = arrange_enemy_contact(base, state_path, initialized, case, assets)
                    if geometry is not None:
                        write(child / "contact-placement.json", geometry)
                    battle = inspect(State(state_path), battle=True)["battle"]
                    owner, target = initialized["actor"]["slot"], 0
                    expected = ({"selected": fixture["expected"]["selected_row_pointer"], "native": chosen["native"]}
                        if chosen["kind"] == "common_spell" else dict(selected=fixture["expected"]["selected_row_pointer"],
                            animation=fixture["expected"]["animation_pointer"], commands=fixture["expected"]["command_pointer"]))
                    inputs, assistance = dict(button="neutral"), dict(selection=fixture["changes"],contact_placement=geometry)
                    enemy_ids = [p["enemy"] for p in record["packages"]]
                    del base
                state = State(state_path)
                manual_opponents(state, battle, owner)
                if args.offense is not None:
                    actor = next(a for a in battle["actors"] if a["slot"] == owner)
                    for offset in (0x2c, 0x2e, 0x32):
                        state.write(actor["vitals_address"] + offset, "H", args.offense)
                fixture_changes = list(state.changes)
                state.save(state_path)
                battle = inspect(state, battle=True)["battle"]
                assistance = dict(preparation=assistance, neutral_and_offense_changes=fixture_changes,
                                  offensive_stat_preset=args.offense)
                banks = effect_banks(state, assets, args.packages, enemy_ids, args.usual)
                setup = setup_record(case, state_path, battle, owner, target, assistance, inputs,
                    destination / "entry.s01", read(assets / "game/techniques.json")["learning"])
                setup["common_effect_bank"] = next(b["source_sha256"] for b in banks.values()
                    if Path(b["path"]).name == "common.json")
                setup["catalogue"] = pin(assets / ("game/techniques.json" if "character" in case else f"battle/enemies/{case['enemy']:03}/definition.json"))
                write(destination / "setup.json", setup)
                del state
                gc.collect()
                execution_watch, execution_budget = observer_plan(slots, setup["initial_battle"]["pool"])
                write(destination / "execution-locations.json", execution_watch)
                write(destination / "execution-observer.json", dict(execution_budget,
                    initialized_pool=setup["initial_battle"]["pool"],
                    scope="Fixed verified pool anchor; linked nodes and owned pointers remain dynamic; every observed pool value must match."))
                # Dolphin retains its boot movie input buffer across F1 state
                # loads. A changed sidecar alone is not an input replacement.
                if execution_watch != watch or digest(Path(str(state_path) + ".dtm")) != worker.metadata["movie_sha256"]:
                    boot_movie = worker.metadata["movie_sha256"]
                    worker.close(complete=False)
                    worker = None
                    write(destination / "movie-rebind.json", dict(previous_boot_movie_sha256=boot_movie,
                        finalized_movie=pin(Path(str(state_path) + ".dtm")),
                        reason="Start the execution resident with finalized input and verified compact pool observations; F1 preserves the prior playback buffer"))
                    watch = execution_watch
                    worker = BattleFixture(args.disc, Path(str(state_path) + ".dtm"), state_path,
                        destination / "execution-worker", marker, watch,
                        disc_sha256=profile["inputs"]["disc_sha256"], capture_images=False)
                capture = destination / "capture"
                worker.observe_until(state_path, capture, record["fixture"]["case_marker"], args.max_vis,
                    completion_observer(owner, expected))
                worker.snapshot(destination / "terminal.s01")
                rows = read(capture / "observations.json")
                trace = normalize(case, rows, setup, banks, expected, None)
                trace["setup"]["capture"] = pin(capture / "capture.json")
                write(destination / "trace.json", trace)
                outcomes.append(dict(case=case["id"], trace=pin(destination / "trace.json"), fixture_complete=True))
            except Exception as error:
                destination.mkdir(parents=True, exist_ok=True)
                failure = dict(case=case["id"], setup={}, frames=[], events=[],
                    assessment=dict(outcome=case["context"], error=str(error)),
                    incomplete_observations=[str(error)])
                write(destination / "trace.json", failure)
                outcomes.append(dict(case=case["id"], fixture_complete=False, error=str(error)))
                # Broken registration/process is not reusable. A setup failure
                # before playback may still leave a usable paused resident.
                if worker is not None and (worker.process.poll() is not None or worker.watcher.error or not worker.paused):
                    break
    finally:
        if worker is not None:
            worker.close(complete=False)
        write(output / "outcome.json", dict(cases=outcomes, requested=len(selected),
            worker_closed=worker is not None, equivalence_verified=False))
    return 0 if len(outcomes) == len(selected) and all(o["fixture_complete"] for o in outcomes) else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--disc", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--case", action="append")
    parser.add_argument("--cases-file", type=Path)
    parser.add_argument("--context", choices=("attack", "support", "revival", "aerial", "counter", "capture", "departure"))
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--max-vis", type=int, default=520)
    parser.add_argument("--offense", type=int, help="shared initialized slash/thrust/intelligence fixture value")
    parser.add_argument("--resume-ready", type=Path,
                        help="reuse a retained same-case idle checkpoint/movie after a pre-playback adapter failure")
    parser.add_argument("--resume-warm", type=Path,
                        help="continue a retained same-case partial entry before camera initialization")
    parser.add_argument("--packages", type=Path, default=ROOT / "local/battle-actions-recovery-20261011/input")
    parser.add_argument("--usual", type=Path, default=ROOT / "local/extracted/disc1/files/BTL/BTLusual.dat")
    args = parser.parse_args()
    if args.list:
        print(json.dumps([c["id"] for c in selected_cases(args)]))
        return 0
    if not args.disc or not args.output or not 0 < args.max_vis <= 3600:
        parser.error("--disc, --output and a bounded positive --max-vis are required")
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())
