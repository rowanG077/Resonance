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
import subprocess
import traceback
from types import SimpleNamespace

import battle_artes
import battle_geometry
from battle_geometry import model_world as static_model_world
import damage_envelope
import enemy_artes
import enemy_prerequisites
import effect_entry
import effect_model_joints
import reaction_inputs
from named_content import Snapshot as ContentSnapshot, scene_effect_range, same_evidence
import entry_history
import entry_motions
import support_outcomes
import secondary_bodies
import single_action_fixture
from battle_fixture import BattleFixture, DTM_BUTTONS, neutral_movie
from battle_state import presentation_locations, watch_locations
from enemy_catalogue import COMMON_SPELL_START, PACKAGE_HEADER_BYTES, build_catalogue, section as enemy_section
from memory_watch import LOCATIONS, MEM1_DATAGRAM_BYTES, MEM1_HEADER, MEM1_WORD
from state import State, digest, inspect

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
POOL = enemy_artes.BATTLE_POINTER
ACTORS, STRIDE = enemy_artes.ACTORS, enemy_artes.ACTOR_BYTES
PROJECTILES, PROGRAMS, PARTICLES = 16, 4, 12
BODY_CONTROLLERS = 4
ACTOR_CONTACT_CAPACITY = 16
PROJECTILE_CONTACT_WORDS = (("16c 0", "rule_flags_element"), ("16c 4", "rule_timing"),
    ("16c 8", "rule_conditions"), ("16c c", "rule_power"), ("170 8", "contact_kind"),
    ("c8", "cooldown_0"), ("cc", "cooldown_1"), ("15c", "last_target"))
OBJECT_POOL_CAPACITY, OBJECT_BYTES, OBJECT_FREE_HEAD = 416, 0x288, 0x754a4
OBJECT_SENTINELS = {"projectile": 0x75c40, "program": 0x75ec8,
                    "particle": 0x763d8, "alternate": 0x76660}
MENU_ACTIVE = 0x80000000
STUN_ACTION, STUN_DECLARATION = 15, 19
PARTY_FIXTURE_RADIUS = 850.0
MAX_RECIPIENT_POLLS = 2 * 3600  # Two controller polls per update, within the capture budget.
ENEMY_ENGAGEMENT_DISTANCE = 300.0


def read(path):
    return json.loads(Path(path).read_text())


def read_capture_observations(receipt_path):
    """Verify complete raw samples, including consecutive single-action segments."""
    receipt_path = Path(receipt_path)
    receipt = read(receipt_path)
    observations = receipt_path.with_name("observations.json")
    try:
        data = observations.read_bytes()
    except FileNotFoundError:
        data = subprocess.check_output(["zstd", "-q", "-d", "-c", str(observations) + ".zst"])
    rows = json.loads(data)
    if not receipt.get("complete") or not rows:
        raise ValueError("raw observation receipt is incomplete")
    signature = hashlib.sha256(data).hexdigest()
    if "combined_observations_sha256" in receipt:
        if (signature != receipt["combined_observations_sha256"]
                or len(rows) != receipt["combined_observations"]):
            raise ValueError("combined observation receipt differs")
        segments = receipt.get("segments")
        if segments:
            offset = 0
            for path in segments:
                segment = read_capture_observations(path)
                if rows[offset:offset + len(segment)] != segment:
                    raise ValueError("combined observations differ from their source segment")
                offset += len(segment)
            if offset != len(rows):
                raise ValueError("combined observation count differs from its source segments")
        else:
            samples = receipt.get("samples", [])
            if len(samples) != 1 or signature != samples[0]["observations_sha256"]:
                raise ValueError("raw observation receipt differs")
    else:
        samples = receipt.get("samples", [])
        if (len(samples) != 1 or signature != samples[0]["observations_sha256"]
                or len(rows) != samples[0]["observations"]):
            raise ValueError("raw observation receipt differs")
    if any(row["presentation_counter"] != previous["presentation_counter"] + 1
           or row["case_marker"] != previous["case_marker"]
           for previous, row in zip(rows, rows[1:])):
        raise ValueError("raw observations are not consecutive within one fixture")
    return rows


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def pin(path):
    return {"path": str(Path(path).resolve()), "sha256": digest(path)}


def observation_pin(path):
    """Retain provenance for either plain or losslessly compressed samples."""
    path = Path(path)
    return pin(path if path.exists() else Path(str(path) + ".zst"))


def f32(word):
    value = struct.unpack(">f", struct.pack(">I", word))[0]
    return value if math.isfinite(value) else None


def movement_only(definition):
    if capabilities := definition.get("capabilities"):
        return not capabilities["offensive"] and not capabilities["spell"] and capabilities["target"] == "enemy"
    execution = definition.get("execution") or {}
    attack = execution.get("attack", execution.get("physical", {}).get("attack", {}))
    kinds = {kind for _, event in attack.get("events", []) for kind in event}
    displacement = {"forward", "set_position", "position_from_target", "offset_position"}
    return bool(kinds & displacement) and kinds <= displacement | {
        "effect", "pose", "vertical", "acceleration", "gravity", "braking", "turn_motion", "sound", "voice"}


def has_requested_body_contacts(names, groups):
    return any(name[:2].lower() == "at" and
               (len(name) < 4 or name[3] not in "0123456789" or int(name[3]) in groups)
               for name in names)


def presentation_requirements(definition, body=None, null_motions=()):
    execution = definition.get("execution") or {}
    events = execution.get("attack", execution.get("physical", {}).get("attack", {})).get("events", [])
    poses, effects, groups = [], [], set()
    retained_pose = False
    for _, event in events:
        if "pose" in event:
            if event["pose"]["clip"] in null_motions:
                retained_pose = True
            else:
                poses.append(dict(clip=event["pose"]["clip"], first_bone=None))
        elif "partial_pose" in event:
            part = event["partial_pose"]
            poses.append(dict(clip=part["pose"]["clip"], first_bone=part["first_bone"]))
        elif "effect" in event:
            effects.append(event["effect"])
        elif event in ({"offset_position": {"height": 0, "retreat": 0}},
                       {"vertical": 0.0}, {"gravity": 0.0}):
            pass
        elif "contact" in event:
            contact = event["contact"]
            if (body is None or any(
                    len(part["rig"]["skeleton"]["bones"]) != 1
                    or part["rig"]["skeleton"]["bones"][0]["name"] != "default"
                    or part["rig"].get("attachments") or part.get("layer_skeletons")
                    for part in body.get("attachments", {}).values()) or body.get("weapon_variants")
                    or body.get("secondary_body") or "groups" not in contact
                    or not contact["groups"] or any(group >= 12 for group in contact["groups"])
                    or not body["rig"]["skeleton"]["bones"]
                    or has_requested_body_contacts((bone["name"] for bone in body["rig"]["skeleton"]["bones"]), contact["groups"])):
                raise ValueError("presentation contact has possible melee bindings")
            groups.update(contact["groups"])
        elif set(event) - {"sound", "voice"}:
            raise ValueError("presentation context requires an authored pose/effect-only action")
    if not poses and not effects and not retained_pose:
        raise ValueError("presentation context requires an authored pose/effect-only action")
    requirements = dict(poses=poses, effects=effects)
    if retained_pose and not poses:
        requirements["retained_pose"] = True
    if groups:
        requirements["empty_contact_groups"] = sorted(groups)
    return requirements


def observe_presentation_contacts(case, setup, states, assets):
    """Verify inert body contacts against initialized rigs at both action boundaries."""
    groups = case.get("presentation", {}).get("empty_contact_groups", [])
    if not groups:
        return
    address = setup["initial_battle"]["actors"][setup["owner"]]["address"]
    samples = []
    for label, state in states:
        actor = next(actor for actor in inspect(state, battle=True)["battle"]["actors"]
                     if actor["address"] == address)
        names = battle_geometry.read_initialized_body(state, actor, assets, enemy=case["enemy"])["names"]
        if has_requested_body_contacts(names["labels"], groups):
            raise ValueError("presentation body has initialized melee markers")
        counts = [state.read(address + 0x10c8 + 12 * group + 11, "B") for group in groups]
        if any(counts):
            raise ValueError("presentation contact group is not empty")
        samples.append(dict(stage=label, counts=counts))
    setup["presentation_contact_bindings"] = dict(groups=groups, samples=samples)


def resolve_cases(assets, descriptors):
    """Resolve authored payloads for the single Rust-generated selection."""
    content = ContentSnapshot(assets)
    techniques = read(assets / "game/techniques.json")
    placements = read(assets / "battle/unison.json")["placements"]
    contact_fixtures = read(Path(__file__).with_name("fixtures") / "enemy_contacts.json")
    items = None
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
            enemy_definition = content.enemy_rules(enemy)
            definition = enemy_definition["actions"]["rows"][row]
            resolved = dict(id=f"enemy:{enemy}:{row}", enemy=enemy, action_row=row, definition=definition)
            execution = definition.get("execution")
            execution = execution if isinstance(execution, dict) else {}
            if "spell" in execution and any(
                    item["capabilities"]["healing"] and placements[arte]["native_id"] == execution["spell"]
                    for arte, item in enumerate(techniques["definitions"])):
                resolved["healing"] = True
            attack = execution.get("attack", execution.get("physical", {}).get("attack", {}))
            for _, event in attack.get("events", []):
                if isinstance(event, dict) and "use_item" in event:
                    if items is None:
                        items = read(assets / "game/menu-data.json")["items"]
                    item = items[event["use_item"]["item"]]["field_use"]
                    if item and item["kind"] == "recover":
                        if item["hp"]:
                            resolved["healing"] = True
                        if item["tp"]:
                            resolved["restores_tp"] = True
            if any("defeat" in event for _, event in attack.get("events", [])):
                resolved["self_defeat"] = dict(retain_body=enemy_definition["profile"]["traits"]["retain_defeated_body"])
            if any(event == "stun" for _, event in attack.get("events", [])):
                resolved["self_stun"] = True
        else:
            raise ValueError("unknown selected binding side")
        if descriptor["id"] != resolved["id"]:
            raise ValueError("case ID disagrees with its binding")
        resolved["context"] = descriptor["context"]
        if fixture := contact_fixtures.get(resolved["id"]):
            resolved["contact_fixture"] = fixture
        if resolved["context"] == "departure" and definition.get("execution") != "leave_arena":
            raise ValueError("departure context requires an authored leave-arena action")
        if resolved["context"] == "presentation":
            if "enemy" in resolved:
                body = content.enemy_artwork(enemy)
                body = dict(body, rig=content.json(body["rig"]), attachments={
                    slot: content.document(path, ("model",))["model"]
                    for slot, path in body["attachments"].items()})
                resolved["presentation"] = presentation_requirements(definition, body, enemy_definition["null_motions"])
            elif resolved["arte"] == 222:  # Sacred Light only presents its stored scene.
                resolved["presentation"] = dict(poses=[], effects=[1])
            else:
                raise ValueError("presentation requires an authored presentation action")
        if resolved["context"] == "movement" and not movement_only(definition):
            raise ValueError("movement context requires an authored non-damaging movement action")
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


def locations(actor_slots=None, particle_count=PARTICLES, projectile_count=PROJECTILES,
              alternate_count=8, pool_base=None, *, program_count=PROGRAMS, contact_count=4,
              particle_transforms=True, particle_addresses=None, secondary_slots=(),
              knockdown_impacts=False, contact_slot=None, owner_bones=None, object_addresses=None,
              model_joint_plan=()):
    if particle_addresses is not None and (pool_base is None or len(particle_addresses) != particle_count):
        raise ValueError("fixed particle slots require their initialized pool and exact capacity")
    actor_slots = list(range(12)) if actor_slots is None else actor_slots
    result = watch_locations({"pool": 1, "bss_index": 6,
                              "actors": [{"slot": i} for i in actor_slots]})
    result = {address: name for address, name in result.items()
              if not (name.startswith("battle_camera_") and name.endswith("_bits"))}
    for offset in (0x1f44, 0x1f5c):
        result[f"8035a768 {offset:x}"] = f"damage_session_{offset:x}"
    for offset in (0x15714, 0x15728):
        result[f"{POOL} {offset:x}"] = f"damage_battle_{offset:x}"
    result[f"{POOL} 15718"] = "battle_time_stop_word"
    result[f"{POOL} 7e60c"] = "contact_queue_counts"
    for side in range(2):
        for index in range(contact_count):
            entry = 0x7dc0c + side * 0x500 + index * 0x20
            prefix = f"contact_queue_{side}_{index}"
            for offset, label in ((4,"owner"),(8,"context"),(12,"projectile")):
                result[f"{POOL} {entry + offset:x}"] = prefix + "_" + label
    if contact_slot is not None:
        if contact_slot not in actor_slots:
            raise ValueError("contact owner must be an observed actor")
        for index in range(min(contact_count, ACTOR_CONTACT_CAPACITY)):
            context = ACTORS + contact_slot * STRIDE + 0x1184 + index * 0x38
            prefix = f"actor_{contact_slot}_contact_{index}"
            for suffix, label in (("4", "rule"), ("8", "template"), ("34", "nominal"),
                                  ("c", "x_bits"), ("10", "y_bits"), ("14", "z_bits"),
                                  ("4 0", "rule_flags_element"), ("4 8", "rule_conditions"),
                                  ("4 c", "rule_power"),
                                  ("8 8", "contact_kind")):
                first, *rest = suffix.split()
                address = f"{POOL} {context + int(first, 16):x}"
                result[" ".join([address, *rest])] = prefix + "_" + label
    for slot in actor_slots:
        base = ACTORS + slot * STRIDE
        for offset, name in ((0x104, "selected"), (0x107c, "control_word"), (0x104c, "animation"),
                             (0x1050, "commands"), (0x1054, "clock"),
                             (0x1174, "native"), (0x199c, "contact_target"),
                             (0x19d4, "capture_attachment"), (0x19d8, "captured_recipient"),
                             (0x198c, "support_target"), (0x16bc, "spell_support_target"),
                             (0x19a0, "effective_target"), (0x106c, "attack_power_word"),
                             (0x18ac, "target_distance_bits"),
                             (0x10c4, "proficiency_word"), (0x1a10, "cast_strength_word"),
                             (0x1b0, "target_state_word"), (0x10a8, "overlimit_gauge_word"),
                             (0x117c, "fresh_contact_word"), (0x1c4, "protection_window_word"),
                             (0x1180, "time_stop_flags")):
            result[f"{POOL} {base + offset:x}"] = f"actor_{slot}_{name}"
        for offset in (0, 4, 8, 0x10, 0x14, 0x38, 0x3c, 0x5c):
            result[f"{POOL} {base + 4:x} {offset:x}"] = f"actor_{slot}_condition_{offset:x}"
        for offset in (0x1d0, 0x1d4, 0x1e0, 0x1e4, 0x280, 0x1aa8):
            result[f"{POOL} {base + offset:x}"] = f"actor_{slot}_raw_{offset:x}"
        for index, offset in enumerate((0x1a14, 0x1a18)):
            result[f"{POOL} {base + offset:x}"] = f"actor_{slot}_stat_adjustment_{index}"
        controller = f"{POOL} {base + 0x2b0:x} 5fc"
        for link in range(BODY_CONTROLLERS):
            node = controller + " 2c" * link
            prefix = f"actor_{slot}_animation_{link}"
            for offset, name in ((0, "resource"), (4, "layer_word"), (8, "frame_bits"), (0x30, "child")):
                result[f"{node} {offset:x}"] = prefix + "_" + name
            result[node + " 30 0"] = prefix + "_child_resource"
            result[node + " 30 8"] = prefix + "_child_frame_bits"
            if slot < 4:
                for offset, name in ((0, "header"), (4, "descriptor"), (8, "duration")):
                    result[f"{node} 0 {offset:x}"] = prefix + "_motion_" + name
                    result[f"{node} 30 0 {offset:x}"] = prefix + "_child_motion_" + name
            for offset, name in entry_history.CONTROLLER_FIELDS:
                result[f"{node} {offset:x}"] = prefix + "_" + name + "_word"
                result[f"{node} 30 {offset:x}"] = prefix + "_child_" + name + "_word"
            if knockdown_impacts:
                result[node + " 20"] = prefix + "_impact_state_word"
        if knockdown_impacts:
            result[f"{POOL} {base + 0x19cc:x}"] = f"actor_{slot}_recoil_word"
            result[f"{POOL} {base + 0x2ac:x}"] = f"actor_{slot}_last_damage_word"
        for offset, label in ((0x38, "bank"), (0x44, "program"), (0x34, "clock_word"), (0x8c, "cursor")):
            result[f"{POOL} {base + 0x15b4 + offset:x}"] = f"actor_{slot}_effect_{label}"
    for slot, bones in (owner_bones or {}).items():
        base = ACTORS + slot * STRIDE
        for bone in bones:
            matrix = f"{POOL} {base + 0x2b0:x} 0 18 {bone * 4:x} 84"
            for word in range(12):
                result[f"{matrix} {word * 4:x}"] = f"actor_{slot}_bone_{bone}_{word}"
    result.update(effect_model_joints.locations(POOL, model_joint_plan))
    retained_slots = effect_model_joints.retained_slots(model_joint_plan)
    for slot in actor_slots:
        for retained in retained_slots:
            offset = ACTORS + slot * STRIDE + 0x15b4 + 0x98 + retained * 8
            result[f"{POOL} {offset:x}"] = f"actor_{slot}_retained_{retained}"
    result.update(secondary_bodies.locations(POOL, ACTORS, STRIDE, secondary_slots))
    groups = [("projectile", OBJECT_SENTINELS["projectile"] + 4, projectile_count,
               (0x10, 0x14, 0x28, 0x2c, 0x30, *range(0x40, 0x58, 4), 0xac, 0xd8, 0x144, 0x16c, 0x170, 0x19c, 0x278, 0x280)),
              ("program", OBJECT_SENTINELS["program"] + 4, program_count, (0x10, 0x14, 0x18, 0x2c, 0x30, 0x44, 0x48, 0x4c, 0x50, 0x5c, 0x60, 0x6c, 0x278, 0x280)),
              ("particle", OBJECT_SENTINELS["particle"] + 4, particle_count,
               (0x10, 0x1c, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c,
                0x5c, 0x60, 0x64, 0x80, 0x84, 0x88, 0xb4, 0xd8, 0xdc, 0xe0, 0xfc,
                0x100, 0x108, 0x134, 0x13c, 0x168,
                0x16c, 0x170, 0x174, 0x178, 0x180, 0x184, 0x278))]
    if not particle_transforms:
        label, head, count, _ = groups[2]
        groups[2] = (label, head, count,
                     (0x10, 0x1c, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0xfc, 0x184, 0x278))
    alternate_offsets = sorted(set(groups[1][3]) | set(groups[2][3]))
    groups.append(("alternate", OBJECT_SENTINELS["alternate"] + 4, alternate_count, alternate_offsets))
    if object_addresses is not None:
        if pool_base is None or len(object_addresses) != OBJECT_POOL_CAPACITY:
            raise ValueError("complete object observation requires the verified full pool")
        offsets = sorted({0, 4} | {at for _, _, _, fields in groups for at in fields}
                         | {0xc0 + retained * 8 for retained in retained_slots})
        for index, address in enumerate(object_addresses):
            for offset in offsets:
                result[f"{address + offset:x}"] = f"object_{index}_{offset:x}"
        for label, head, _, _ in groups:
            result[f"0{pool_base + head:x}"] = label + "_head"
            result[f"0{pool_base + head - 4:x}"] = label + "_tail"
        # Only initialized projectile nodes use these rule/template pointers.
        # Observe both ends and require coverage of every live projectile.
        for index in range(projectile_count):
            head = OBJECT_SENTINELS["projectile"] + 4
            node = (f"{POOL} {head:x}" + " 4" * index if index < (projectile_count + 1) // 2
                    else f"{POOL} {head - 4:x}" + " 0" * (projectile_count - 1 - index))
            prefix = f"projectile_rule_{index}"
            result[node] = prefix + "_address"
            for suffix, name in PROJECTILE_CONTACT_WORDS:
                result[f"{node} {suffix}"] = prefix + "_" + name
        groups = []
    for label, head, count, offsets in groups:
        if label == "particle" and particle_addresses is not None:
            result[f"{pool_base + head:x}"] = "particle_head"
            result[f"{pool_base + head - 4:x}"] = "particle_tail"
            for index, address in enumerate(particle_addresses):
                prefix = f"particle_{index}"
                result[f"{address:x}"] = prefix + "_prev"
                result[f"{address + 4:x}"] = prefix + "_next"
                # The pool also contains scheduler nodes. Their typed raw bank,
                # root and owner words prove parent identity without union reads.
                for offset in sorted(set(offsets) | {0x60, 0x6c}):
                    result[f"{address + offset:x}"] = f"{prefix}_{offset:x}"
            continue
        for index in range(count):
            # Split the same bounded list between its two ends to shorten the
            # pointer paths. Normalization joins addresses and checks both links.
            node = (f"{POOL} {head:x}" + " 4" * index if index < (count + 1) // 2
                    else f"{POOL} {head - 4:x}" + " 0" * (count - 1 - index))
            prefix = f"{label}_{index}"
            result[node] = prefix + "_address"
            # Padded hex keeps these words distinct from a neighboring slot's
            # pointer expression, so both names survive the address-keyed map.
            result[f"{node} 04"] = prefix + "_next"
            result[f"{node} 00"] = prefix + "_prev"
            for offset in offsets:
                result[f"{node} {offset:x}"] = f"{prefix}_{offset:x}"
            if label == "particle":
                # The verified program-parent pointer addresses parent+0x28.
                result[f"{node} 184 38"] = prefix + "_parent_bank"
                result[f"{node} 184 44"] = prefix + "_parent_program"
                result[f"{node} 184 4"] = prefix + "_parent_owner"
            elif label == "projectile":
                for suffix, name in PROJECTILE_CONTACT_WORDS:
                    result[f"{node} {suffix}"] = prefix + "_" + name
    # Retain the head also for normalization of older head-only observations.
    if not alternate_count:
        result[f"{POOL} {OBJECT_SENTINELS['alternate'] + 4:x}"] = "effect_alternate_head"
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


def observer_plan(slots, pool_base=None, capacities=None, *, particle_transforms=True,
                  particle_addresses=None, secondary_slots=(), knockdown_impacts=False, contact_slot=None,
                  owner_bones=None, object_addresses=None, model_joint_plan=()):
    """Allocate within the existing transport; every bounded list has overflow checks."""
    def plan(projectiles, programs, particles, alternates, contacts):
        counts = dict(projectile=projectiles, program=programs, particle=particles,
                      alternate=alternates, contact=contacts)
        if any(count < 1 for count in counts.values()) or contacts > 0x500 // 0x20:
            raise ValueError("observer capacities must be positive and fit the contact queues")
        watch = locations(slots, particles, projectiles, alternates, pool_base,
                          program_count=programs, contact_count=contacts,
                          particle_transforms=particle_transforms, particle_addresses=particle_addresses,
                          secondary_slots=secondary_slots, knockdown_impacts=knockdown_impacts,
                          contact_slot=contact_slot, owner_bones=owner_bones, object_addresses=object_addresses,
                          model_joint_plan=model_joint_plan)
        return watch, dict(packet_budget(watch), capacities=counts, list_layout="complete_object_pool" if object_addresses is not None else "head_and_tail",
                           particle_transforms=particle_transforms, secondary_slots=list(secondary_slots),
                           knockdown_impacts=knockdown_impacts, contact_slot=contact_slot,
                           **({"model_joints": model_joint_plan} if model_joint_plan else {}),
                           **({"object_addresses": object_addresses}
                              if object_addresses is not None else {}),
                           **({"particle_addresses": particle_addresses, "particle_layout": "fixed_pool"}
                              if particle_addresses is not None else {}))

    if capacities is not None:
        return plan(*capacities)
    if object_addresses is not None:
        return plan(64, OBJECT_POOL_CAPACITY, OBJECT_POOL_CAPACITY, OBJECT_POOL_CAPACITY, 40)
    for alternate_count in (12, 8, 4, 2):
        for particle_count in ([len(particle_addresses)] if particle_addresses is not None else range(32, 3, -1)):
            try:
                return plan(4, 4, particle_count, alternate_count, 4)
            except ValueError:
                continue
    raise ValueError("actor and effect observations exceed the existing watcher budget")


def fixed_particle_slots(state, pool, count):
    """Register live slots and the ready free prefix from a complete pool partition."""
    if not 1 <= count <= OBJECT_POOL_CAPACITY:
        raise ValueError("fixed particle capacity exceeds the authored source pool")
    free, live, seen = [], [], set()

    def register(address, addresses):
        if (address in seen or address & 3 or
                not 0x80000000 <= address <= 0x81800000 - OBJECT_BYTES):
            raise ValueError("invalid or shared ready object-pool slot")
        if len(seen) >= OBJECT_POOL_CAPACITY:
            raise ValueError("ready object-pool chains exceed the authored capacity")
        seen.add(address)
        addresses.append(address)

    address = state.read(pool + OBJECT_FREE_HEAD)
    while address:
        register(address, free)
        # Retirement relinks this field without clearing the old type/group.
        address = state.read(address + 4)
    groups = []
    for group in range(8):
        sentinel = pool + OBJECT_FREE_HEAD + 4 + group * OBJECT_BYTES
        previous, address = sentinel, state.read(sentinel + 4)
        start = len(live)
        while address != sentinel:
            register(address, live)
            if state.read(address) != previous or state.read(address + 0x11, "B") != group:
                raise ValueError("ready live-object group or backlink differs")
            previous, address = address, state.read(address + 4)
        if state.read(sentinel) != previous:
            raise ValueError("ready live-object group tail differs")
        groups.append(len(live) - start)
    if len(seen) != OBJECT_POOL_CAPACITY:
        raise ValueError("ready free and live chains do not cover the complete object pool")
    ordered = sorted(seen)
    if any(b - a < OBJECT_BYTES for a, b in zip(ordered, ordered[1:])):
        raise ValueError("ready source object slots overlap")
    if len(live) > count:
        raise ValueError("ready live objects exceed the requested fixed particle profile")
    return dict(ready=pin(state.path), pool=pool, free_count=len(free), live_count=len(live),
                live_group_counts=groups,
                initial_headers_zero=not any(state.read(address + 0x10) for address in free),
                addresses=(live + free)[:count])


def object_pool_projection(addresses, pool):
    """Publish complete typed live lists from direct, bounded pool reads.

    Freed union storage has no observable meaning. Keep every live node and
    copied parent binding, so snapshots stay small without hiding list overflow.
    """
    indices = {address: index for index, address in enumerate(addresses)}
    if len(indices) != OBJECT_POOL_CAPACITY:
        raise ValueError("object projection requires the complete verified pool")
    groups = {label: ((offset - OBJECT_FREE_HEAD - 4) // OBJECT_BYTES, offset)
              for label, offset in OBJECT_SENTINELS.items()}
    cached = None

    def project(values):
        nonlocal cached
        if cached is None:
            cached = dict(fields=tuple(key[len("object_0_"):] for key in values if key.startswith("object_0_")),
                ordinary=tuple(key for key in values if not key.startswith(("object_", "projectile_rule_"))),
                rules=tuple(key[:-len("_address")] for key in values
                            if key.startswith("projectile_rule_") and key.endswith("_address")),
                actors=tuple(int(key.split("_")[1]) for key in values
                             if key.startswith("actor_") and key.endswith("_effect_bank")))
        row = {key: values[key] for key in cached["ordinary"]}
        row["object_lists"] = {label: [] for label in groups}
        if values.get("battle_pool") != pool:
            row["object_pool_error"] = "initialized battle object pool is not active"
            return row
        rules = {}
        for prefix in cached["rules"]:
            address = values[prefix + "_address"]
            fields = {name: values[prefix + "_" + name] for _, name in PROJECTILE_CONTACT_WORDS
                      if prefix + "_" + name in values}
            if address in rules and rules[address] != fields:
                raise ValueError("duplicate projectile rule observations differ")
            rules[address] = fields
        claimed = set()
        for label, (group, offset) in groups.items():
            sentinel = pool + offset
            previous, address = sentinel, values[label + "_head"]
            while address != sentinel:
                if address not in indices or address in claimed:
                    raise ValueError("live object list is cyclic, shared or outside the verified pool")
                index = indices[address]
                raw = f"object_{index}_"
                if values[raw + "0"] != previous or (values[raw + "10"] >> 16) & 255 != group:
                    raise ValueError("live object list backlink or group differs")
                claimed.add(address)
                prefix = f"{label}_{index}"
                row[prefix + "_address"] = address
                row[prefix + "_prev"] = previous
                row[prefix + "_next"] = values[raw + "4"]
                row["object_lists"][label].append([index, address])
                row.update({prefix + "_" + field: values[raw + field] for field in cached["fields"]})
                kind = values[raw + "10"] >> 24
                if label == "projectile":
                    if address not in rules:
                        raise ValueError("live projectile lacks its observed rule/template words")
                    row.update({prefix + "_" + name: value for name, value in rules[address].items()})
                elif kind == 1:
                    parent = values[raw + "184"]
                    parent_index = indices.get(parent - 0x28)
                    if parent_index is not None and values[f"object_{parent_index}_10"] >> 24 == 0:
                        signature = tuple(values[f"object_{parent_index}_{at:x}"] for at in (0x60, 0x6c, 0x2c))
                    else:
                        signature = (0, 0, 0)
                        for slot in cached["actors"]:
                            actor_address = pool + ACTORS + slot * STRIDE
                            if parent == actor_address + 0x15b4:
                                signature = (values[f"actor_{slot}_effect_bank"],
                                             values[f"actor_{slot}_effect_program"], actor_address)
                                break
                    for name, value in zip(("bank", "program", "owner"), signature):
                        row[prefix + "_parent_" + name] = value
                previous, address = address, values[raw + "4"]
            if values[label + "_tail"] != previous:
                raise ValueError("live object list tail differs")
        return row
    return project


def observed_body(row, slot, resources, initial_override=None):
    clip = (row[f"battle_actor_{slot}_body_clip_word"] >> 8) & 255
    expected = resources.get(str(clip))
    for link in range(BODY_CONTROLLERS):
        prefix = f"actor_{slot}_animation_{link}"
        if row.get(prefix + "_layer_word", 0) & 65535 != 6:
            continue
        # A replacement can reuse the outgoing clip's resource.
        if expected and row.get(prefix + "_child") and row.get(prefix + "_child_resource") == expected:
            return clip, f32(row[prefix + "_child_frame_bits"])
        if expected and row.get(prefix + "_resource") == expected:
            return clip, f32(row[prefix + "_frame_bits"])
    if initial_override and clip == initial_override["body_clip"]:
        for link in range(BODY_CONTROLLERS):
            prefix = f"actor_{slot}_animation_{link}"
            if (row.get(prefix + "_layer_word", 0) & 65535 == 6
                    and not row.get(prefix + "_child")
                    and row.get(prefix + "_resource") == initial_override["resource"]):
                return initial_override["clip"], f32(row[prefix + "_frame_bits"])
    return clip, None


def signed16(value):
    return value - 65536 if value & 0x8000 else value


def projectile_contact(get):
    age = signed16(get(0x278) >> 16)
    start, duration = signed16(get(0xac) >> 16), signed16(get(0xac) & 65535)
    disarmed = bool(get(0xd8) & 0xf0)
    active = not disarmed and age != 0 and (duration == 0 or start <= age <= start + duration)
    return age, active, disarmed



def ready_owner_bones(snapshot, battle, banks):
    """Watch only authored attachment joints with initialized evaluated matrices."""
    bones = sorted({member["template"]["owner_bone"] for bank in banks.values()
                    for member in bank["metadata"]["actors"]
                    if member.get("kind") == "model_particle"
                    and member.get("template", {}).get("owner_bone") is not None})
    result = {}
    for actor in battle["actors"]:
        if not bones:
            break
        descriptor = snapshot.read(actor["address"] + 0x2b0)
        scene = snapshot.read(descriptor)
        count, table = snapshot.read(scene + 6, "H"), snapshot.read(scene + 0x18)
        if not 0 < count <= 256 or not 0x80000000 <= table <= 0x81800000 - count * 4:
            raise ValueError("attachment joint table is not initialized")
        selected = [bone for bone in bones if bone < count]
        for bone in selected:
            joint = snapshot.read(table + bone * 4)
            if not 0x80000000 <= joint <= 0x81800000 - 0x88:
                raise ValueError("attachment joint is outside memory")
            matrix = snapshot.read(joint + 0x84)
            if not 0x80000000 <= matrix <= 0x81800000 - 48 or matrix & 3:
                raise ValueError("attachment joint matrix is not initialized")
        if selected:
            result[actor["slot"]] = selected
    return result


def observed_effect_animation(get, binding):
    """Read the selected instance controller, including program-owned replacements."""
    if binding.get("animation") is None and not binding.get("program_animation"):
        return None, None
    controller = 0x100
    if binding.get("program_animation"):
        next_slot = (get(0xfc) >> 8) & 255
        if next_slot not in (0, 1):
            raise ValueError("effect model controller selector is invalid")
        controller += (1 - next_slot) * 0x34
    descriptor = get(0x168)
    if not binding.get("program_animation") and "slot" in binding:
        # A birth modifier can replace the displayed model after its motion
        # starts. The controller still belongs to the declaration's model.
        descriptor += (binding["slot"] - (get(0xfc) >> 24)) * 0x7e0
    resource = get(controller)
    # A program may start an animation later; an empty instance controller
    # currently displays the model's bind pose.
    if not resource and binding.get("animation") is None:
        return None, None
    # Motion resources are entries in this model's descriptor, not shared mesh data.
    offset = resource - descriptor - 0x640
    if (descriptor & 3 or not 0x80000000 <= descriptor <= 0x81800000 - 0x7e0
            or not 0 <= offset <= 0x7e0 - 0x640 - 20 or offset % 20):
        raise ValueError("effect model animation lacks an exact descriptor entry")
    clip, frame = offset // 20, f32(get(controller + 8))
    if (frame is None or not math.isfinite(frame) or frame < 0 or
            (not binding.get("program_animation") and clip != binding.get("animation"))):
        raise ValueError("effect model animation has an invalid clip or frame")
    return clip, frame


def packet_budget(extra):
    all_locations = {**LOCATIONS, **presentation_locations(), **extra, "80000000": "case_marker"}
    maximum = len(MEM1_HEADER) + 1 + MEM1_WORD.size * len(all_locations)
    if maximum > MEM1_DATAGRAM_BYTES:
        raise ValueError(f"MemoryWatcher worst-case datagram {maximum} exceeds {MEM1_DATAGRAM_BYTES} bytes")
    return dict(addresses=len(all_locations), maximum_bytes=maximum, receiver_bytes=MEM1_DATAGRAM_BYTES)


def sections(raw):
    count = struct.unpack_from(">I", raw)[0]
    if not 0 < count < len(raw) // 4:
        raise ValueError("invalid authored archive")
    offsets = struct.unpack_from(f">{count}I", raw, 4)
    for start in offsets:
        if start and not 4 + count * 4 <= start < len(raw):
            raise ValueError("invalid authored section")
        yield raw[start:min([end for end in offsets if end > start] + [len(raw)])] if start else None


def effect_program_end(raw, start, end):
    """Read a bounded scheduler endpoint, skipping repeat-command payloads."""
    payload = False
    for at in range(start, end - 5, 6):
        age, command, _, _ = struct.unpack_from(">hBBH", raw, at)
        if payload:
            if command >= 254:
                return None
            payload = False
        elif command == 254:
            return age if age >= 0 else None
        else:
            payload = command == 255
    return None


def effect_bank(raw, path, assets, evidence_directory):
    content = ContentSnapshot(assets, evidence_directory)
    entry = Path(path).relative_to(Path(assets)).as_posix()
    metadata = content.effects(entry)
    signature = hashlib.sha256(raw).hexdigest()
    if raw[:4] != b"ef1\0":
        raise ValueError(f"invalid original effect bank for {path}")
    offsets = struct.unpack_from(">6H", raw, 8)
    end = offsets[0] if offsets[0] in offsets[1:] else min(v for v in offsets if v > offsets[0])
    if (end - offsets[0]) % 352:
        raise ValueError("truncated effect declarations")
    roots = [offsets[1] + struct.unpack_from(">h", raw, offsets[4] + 2 * i)[0]
             for i in range(raw[4])]
    return dict(source_sha256=signature, metadata=metadata, path=str(path), entry=entry,
        evidence=content.evidence(entry, raw),
        declarations=[raw[at:at + 352] for at in range(offsets[0], end, 352)],
        uv_descriptors=raw[offsets[3]:min([v for v in offsets if v > offsets[3]] + [len(raw)])],
        roots=roots, program_ends=[effect_program_end(raw, root,
            min([at for at in (*offsets, *roots) if at > root] + [len(raw)])) for root in roots],
        immediate_end_programs=[i for i, root in enumerate(roots)
            if raw[root:root + 6] == bytes([0, 0, 254, 0, 0, 0])])


def register_effect_inputs(setup, banks):
    inputs = setup.setdefault("effect_inputs", {})
    for bank in banks.values():
        entry, evidence = bank["entry"], bank["evidence"]
        if entry in inputs and not same_evidence(inputs[entry], evidence):
            raise ValueError(f"effect input changed during observation: {entry}")
        inputs[entry] = evidence


def effect_banks(state, assets, packages, enemy_ids, usual, evidence_directory):
    """Bind complete authored bytes, never an address or declaration ID alone."""
    sources = []
    common = list(sections(Path(usual).read_bytes()))
    for index, name in ((2, "common"), (3, "techniques")):
        sources.append((common[index], assets / f"battle/effects/{name}/effects.json"))
    content = ContentSnapshot(assets, evidence_directory)
    for enemy in enemy_ids:
        raw = (packages / f"{enemy:03}.bin").read_bytes()
        start = struct.unpack_from(">I", raw, 0x1cc)[0]
        offsets = struct.unpack_from(">116I", raw, 0x18)
        if start:
            end = min([v for v in offsets if v > start] + [len(raw)])
            entry = content.resolve("battle/enemies/mappings.json", enemy, "enemy.json")
            if entry is None:
                raise ValueError(f"unmapped original enemy {enemy}")
            definition = content.document(entry, ("rules", "artwork", "effects"))
            if definition.get("effects"):
                sources.append((raw[start:end], assets / definition["effects"]))
    banks = {}
    memory = bytes(state.ram)
    for raw, path in sources:
        if raw[:4] != b"ef1\0" or not path.exists():
            continue
        bank = effect_bank(raw, path, assets, evidence_directory)
        start = 0
        while (at := memory.find(raw, start)) >= 0:
            banks[at + 0x80000000] = bank
            start = at + len(raw)
    return banks


def ready_stage_effect(snapshot, assets, setup, banks, evidence_directory):
    """Bind complete stage bytes and retain a proven first-cycle scenery clock."""
    battle = setup["initial_battle"]
    content = ContentSnapshot(assets, evidence_directory)
    path = content.resolve("battle/arenas/mappings.json", setup["arena"], "arena.json")
    if path is None:
        raise ValueError("unmapped original arena")
    stage = content.arena(path)
    if not stage.get("effects"):
        return
    metadata = content.effects(stage["effects"])
    if not metadata["actors"] and not any(metadata["programs"]):
        # An empty authored bank has no declarations or scheduled work to observe.
        return
    archive = snapshot.read(battle["pool"] + 0x33778)
    if not 0x80000000 <= archive <= 0x817fffd0:
        raise ValueError("ready stage archive is outside memory")
    count = snapshot.read(archive)
    if not 11 <= count <= 128:
        raise ValueError("ready stage section count is invalid")
    offsets = [snapshot.read(archive + 4 + index * 4) for index in range(count)]
    start = offsets[9]
    end = min(value for value in offsets if value > start)
    if not 4 + count * 4 <= start < end <= 0x81800000 - archive:
        raise ValueError("ready stage effect range is invalid")
    base = archive + start
    raw = bytes(snapshot.ram[base - 0x80000000:archive + end - 0x80000000])
    bank = effect_bank(raw, assets / stage["effects"], assets, evidence_directory)
    banks[base] = bank
    interval = snapshot.read(archive + offsets[0] + 0x2d4, "H")
    if interval != (stage.get("recurring_interval") or 0):
        raise ValueError("ready scenery interval differs from its stage")
    if not interval:
        return
    counter = snapshot.read(battle["pool"] + 0x33786, "H")
    if battle["combat_tick"] != 0 or not counter == battle["presentation_tick"] < interval:
        setup["ambient_phase_error"] = "ready scenery is not an observed first cycle"
        return
    root = bank["roots"][1]
    end = min(value for value in struct.unpack_from(">6H", raw, 8) if value > root)
    program_end = effect_program_end(raw, root, end)
    if program_end is None or program_end <= 0:
        setup["ambient_phase_error"] = "ready scenery has no bounded program endpoint"
        return
    setup["ready_ambient_effect"] = dict(bank=bank["source_sha256"], program=1,
        interval=interval, counter=counter, elapsed_updates=counter, program_end=program_end)


def detached_contact_bindings(state, usual, enemy_packages=()):
    """Bind body-center contact records to complete authored bytes in ready memory."""
    directory = list(sections(Path(usual).read_bytes()))[8]
    count = struct.unpack_from(">I", directory)[0]
    offsets = struct.unpack_from(f">{count}I", directory, 4)
    memory, records = bytes(state.ram), {}
    def bind(raw, base, rule, template, *, rule_raw=None, rule_base=None):
        rule_raw = raw if rule_raw is None else rule_raw
        rule_base = base if rule_base is None else rule_base
        record = dict(rule=rule_base + rule, template=base + template,
            rule_flags_element=struct.unpack_from(">I", rule_raw, rule)[0],
            rule_conditions=struct.unpack_from(">I", rule_raw, rule + 8)[0],
            rule_power=struct.unpack_from(">I", rule_raw, rule + 12)[0],
            contact_kind=struct.unpack_from(">I", raw, template + 8)[0])
        if records.setdefault((record["rule"], record["template"]), record) != record:
            raise ValueError("conflicting authored body-center contact identity")

    common_rules = {}
    def loaded_common_rules(native):
        if native not in common_rules:
            start = offsets[native]
            end = min([offset for offset in offsets if offset > start] + [len(directory)])
            raw = directory[start:end]
            rules, contacts = struct.unpack_from(">2I", raw)
            if not 16 <= rules <= contacts <= len(raw):
                raise ValueError("invalid common action rule section")
            bases, cursor = [], 0
            while (address := memory.find(raw, cursor)) >= 0:
                bases.append(address + 0x80000000)
                cursor = address + len(raw)
            common_rules[native] = raw, rules, contacts, bases
        return common_rules[native]

    for start in sorted(set(offsets) - {0, len(directory)}):
        end = min([offset for offset in offsets if offset > start] + [len(directory)])
        raw = directory[start:end]
        rules, contacts, poses, commands = struct.unpack_from(">4I", raw)
        if not 16 <= rules <= contacts <= poses <= commands <= len(raw) or (poses - contacts) % 32:
            raise ValueError("invalid authored contact sections")
        templates = []
        for at in range(contacts, poses, 32):
            if struct.unpack_from(">h", raw, at)[0] < 0 or raw[at + 2] >= 128 or raw[at + 4] != 255:
                continue
            rule = rules + raw[at + 18] * 28
            if rule + 28 > contacts:
                raise ValueError("body-center rule exceeds its authored section")
            templates.append((rule, at + 8))
        if not templates:
            continue
        cursor = 0
        while (address := memory.find(raw, cursor)) >= 0:
            base = address + 0x80000000
            for rule, template in templates:
                bind(raw, base, rule, template)
            cursor = address + len(raw)
    sources = []
    for path in sorted(set(map(Path, enemy_packages))):
        raw = path.read_bytes()
        if len(raw) < PACKAGE_HEADER_BYTES or raw[:4] != b"em8\0":
            raise ValueError("invalid authored enemy contact package")
        header = raw[:PACKAGE_HEADER_BYTES]
        address = memory.find(header)
        if address < 0 or memory.find(header, address + 1) >= 0:
            raise ValueError("enemy contact package header is absent or not unique")
        rules, rule_bytes = enemy_section(raw, 8)
        contacts, contact_bytes = enemy_section(raw, 16)
        for offset, data in ((rules, rule_bytes), (contacts, contact_bytes)):
            if memory[address + offset:address + offset + len(data)] != data:
                raise ValueError("enemy contact section differs from authored bytes")
        if len(contact_bytes) % 32:
            raise ValueError("invalid authored enemy contact section")
        base = address + 0x80000000
        for at in range(contacts, contacts + len(contact_bytes), 32):
            if struct.unpack_from(">h", raw, at)[0] < 0 or raw[at + 2] >= 128 or raw[at + 4] != 255:
                continue
            rule = rules + raw[at + 18] * 28
            if rule + 28 > rules + len(rule_bytes):
                raise ValueError("enemy body-center rule exceeds its authored section")
            bind(raw, base, rule, at + 8)
        # A shared technique can use the enemy's authored body contacts while
        # retaining the common technique's damage rules.
        actions, action_bytes = enemy_section(raw, 10)
        if len(action_bytes) % enemy_artes.ACTION_BYTES:
            raise ValueError("invalid authored enemy action section")
        for action in range(actions, actions + len(action_bytes), enemy_artes.ACTION_BYTES):
            native = struct.unpack_from(">H", raw, action + 64)[0]
            if not 0 < native < min(count, COMMON_SPELL_START):
                continue
            contact_index = struct.unpack_from(">H", raw, action + 28)[0]
            if (memory[address + action + 64:address + action + 66] != raw[action + 64:action + 66]
                    or memory[address + action + 28:address + action + 30] != raw[action + 28:action + 30]):
                raise ValueError("enemy common action binding differs from authored bytes")
            common, common_start, common_end, common_bases = loaded_common_rules(native)
            at = contacts + contact_index * 32
            while at + 32 <= contacts + len(contact_bytes):
                if struct.unpack_from(">h", raw, at)[0] < 0:
                    break
                if raw[at + 2] < 128 and raw[at + 4] == 255:
                    rule = common_start + raw[at + 18] * 28
                    if rule + 28 > common_end:
                        raise ValueError("enemy body-center rule exceeds its common action section")
                    for common_base in common_bases:
                        bind(raw, base, rule, at + 8, rule_raw=common, rule_base=common_base)
                at += 32
        sources.append(dict(enemy=int(path.stem), package=pin(path), address=base,
            rules=[rules, rules + len(rule_bytes)], contacts=[contacts, contacts + len(contact_bytes)]))
    result = dict(source=pin(usual), records=list(records.values()))
    if sources:
        result["enemy_packages"] = sources
    return result

def stored_effect_banks(case, rows, setup, terminal_path, assets, usual, evidence_directory):
    """Register loaded scene bytes against the authored package and retained RAM."""
    owner = setup["initial_battle"]["actors"][setup["owner"]]["address"]
    execution = case.get("definition", {}).get("execution")
    attack = execution.get("attack", {}) if isinstance(execution, dict) else {}
    release_ids = {event["release_spell"]["technique"]
        for _, event in attack.get("events", [])
        if isinstance(event, dict) and "release_spell" in event}
    physical_release = len(release_ids) == 1
    requested_native = next(iter(release_ids)) if physical_release else case.get("native")
    content = ContentSnapshot(assets, evidence_directory)
    observed = {(slot, row[f"battle_stored_{slot}_resource_word"],
                 row[f"battle_stored_{slot}_flags_technique_word"] & 65535)
                for row in rows for slot in range(2)
                if row.get(f"battle_stored_{slot}_resource_word") and
                   row[f"battle_stored_{slot}_owner_word"] == owner and
                   row["battle_stored_slots_word"] & (0xff00 >> (slot * 8))}
    attached_banks = set()
    if not observed:
        if requested_native is None or content.scene_path(requested_native) is None:
            return {}
        counts = observation_counts(rows[0]) if rows else {}
        for row in rows:
            for name in ("program", "alternate"):
                for index, _ in tracked_list(row, name, counts[name])[0]:
                    prefix = f"{name}_{index}"
                    if row[prefix + "_10"] >> 24 == 0 and row[prefix + "_2c"] == owner:
                        attached_banks.add(row[prefix + "_60"])
        if not attached_banks:
            return {}
        observed = {(None, None, requested_native)}
    module = Path(usual).parent.parent / "US_r_Top2Btl.rel"
    skill = case.get("arte") is not None and content.scene_path(case["arte"], skill=True) is not None
    archive = Path(usual).with_name("BTLskill.dat" if skill else "BTLmagic.dat")
    raw = module.read_bytes()
    table = struct.unpack_from(">I", raw, 0x10)[0]
    directory = (struct.unpack_from(">I", raw, table + 5 * 8)[0] & ~1) + (0x11c4 if skill else 0xfe0)
    offsets = struct.unpack_from(">4I" if skill else ">121I", raw, directory)
    terminal = State(terminal_path)
    banks, records = {}, []
    for slot, resource, native in sorted(observed):
        if native != requested_native or not skill and not 201 <= native < 200 + len(offsets):
            raise ValueError("loaded stored scene does not match the requested physical action")
        content = ContentSnapshot(assets, evidence_directory)
        path = content.scene_path(case["arte"] if skill else native, skill=skill)
        if path is None:
            raise ValueError("observed stored scene has no current ID mapping")
        scene, _ = content.scene(path)
        candidates = range(1, len(offsets)) if skill else [native - 200]
        matches = []
        for index in candidates:
            start = offsets[index]
            if not start and index != 1:
                continue
            end = next((v for v in offsets[index + 1:] if v > start), archive.stat().st_size)
            with archive.open("rb") as stream:
                stream.seek(start)
                package = stream.read(end - start)
            member, end_member = scene_effect_range(package)
            data = package[member:end_member]
            addresses = attached_banks if resource is None else [resource + member]
            if any(0 <= address - 0x80000000 <= len(terminal.ram) - len(data)
                   and terminal.ram[address - 0x80000000:address - 0x80000000 + len(data)] == data
                   for address in addresses):
                matches.append((start, end, package))
        if len(matches) != 1:
            raise ValueError("stored scene archive has no unique byte-exact observed effect bank")
        start, end, package = matches[0]
        member, end_member = scene_effect_range(package)
        data = package[member:end_member]
        bank = effect_bank(data, assets / scene["effects"], assets, evidence_directory)
        for address in sorted(attached_banks) if resource is None else [resource + member]:
            at = address - 0x80000000
            if not 0 <= at <= len(terminal.ram) - len(data) or terminal.ram[at:at + len(data)] != data:
                if resource is None:
                    continue
                raise ValueError("stored effect bank is not retained byte-exactly in source RAM")
            banks[address] = bank
            records.append(dict(slot=slot, owner=owner, native=native, resource=address - member,
                bank=address, package_range=[start, end], package_sha256=hashlib.sha256(package).hexdigest(),
                scene=content.evidence(path, package), effects=bank["evidence"], effects_range=[member, end_member],
                bank_sha256=bank["source_sha256"],
                kind="skill" if skill else "spell", physical_release=physical_release))
    if records:
        setup["stored_scenes"] = dict(module=pin(module), archive=pin(archive),
                                      terminal=pin(terminal_path), registrations=records)
        register_effect_inputs(setup, banks)
    return banks


def stored_scene_active(row):
    return bool(row.get("battle_stored_slots_word", 0) & 0xff00ffff)


def owned_stored_pause(row, setup):
    """Qualify bounded load/fade phases against a byte-verified owned package."""
    pause = row.get("battle_pause_flags_word", 0) >> 16
    if pause not in (3, 65, 128):
        return False
    word = row.get("battle_stored_slots_word", 0)
    slot = (word >> 16) & 255
    countdown = signed16(row.get("battle_stored_countdown_word", 0) >> 16)
    if slot >= 2 or not word & (0xff00 >> (slot * 8)):
        return False
    flags = row.get(f"battle_stored_{slot}_flags_technique_word", 0)
    skill = flags >> 16 == 1
    if flags >> 16 not in (0, 1, 2) or pause != 128 and pause != (65 if skill else 3):
        return False
    countdown_limit = 30 if skill else 60
    if pause == 128 and flags & 65535 == 296:
        # Holy Judgment's portrait holds its owned scene for 71 updates.
        countdown_limit = 71
    if ((pause == 128 and word & 255 != 1) or
            (pause != 128 and word >> 24 != 1) or
            not 0 <= countdown < countdown_limit):
        return False
    owner = setup["initial_battle"]["actors"][setup["owner"]]["address"]
    source_slot = setup["source_actor_map"][setup["owner"]]["source_slot"]
    global_owner = row.get("battle_stored_owner_word")
    if ((global_owner != owner and not (pause == 128 and countdown == 0 and global_owner == 0)) or
            row.get(f"battle_stored_{slot}_owner_word") != owner or
            (row.get(f"battle_actor_{source_slot}_stored_slot_word", 0) >> 8) & 255 != slot):
        return False
    resource = row.get(f"battle_stored_{slot}_resource_word", 0)
    return any(
        (flags >> 16 == (1 if skill else 2) or
         flags >> 16 == 0 and pause == 3 and resource == 0 and entry.get("physical_release", False)) and
        (entry["slot"], entry["owner"], entry["native"]) == (slot, owner, flags & 65535) and
        entry.get("kind", "spell") == ("skill" if skill else "spell") and
        (resource == entry["resource"] or resource == 0 and pause != 128)
        for entry in setup.get("stored_scenes", {}).get("registrations", []))


def tracked_list(row, label, count, particle_addresses=None):
    if "object_lists" in row:
        records = row["object_lists"].get(label, [])
        return [tuple(record) for record in records], not row.get("object_pool_error")
    if label == "alternate" and not count:
        # Older captures observed only this group's head; only an empty list is complete.
        return [], row.get("effect_alternate_head") == row["battle_pool"] + OBJECT_SENTINELS[label]
    nodes, seen = [], set()
    sentinel = row.get("battle_pool", 0) + OBJECT_SENTINELS[label]
    fixed = label == "particle" and "particle_head" in row
    if fixed and (particle_addresses is None or len(particle_addresses) != count or
                  len(set(particle_addresses)) != count):
        return [], False
    backwards = [f"{label}_{index}_prev" in row for index in range(count)]
    if any(backwards):
        if not all(backwards):
            return [], False
        payloads = {str(index): {} for index in range(count)}
        group = label + "_"
        for key, value in row.items():
            if key.startswith(group):
                index, separator, field = key[len(group):].partition("_")
                if separator and index in payloads:
                    payloads[index][field] = value
        by_address = {}
        for index in range(count):
            prefix = f"{label}_{index}_"
            address = particle_addresses[index] if fixed else row.get(prefix + "address", 0)
            if not address:
                return [], False
            if address == sentinel:
                continue
            values = payloads[str(index)]
            previous = by_address.get(address)
            if previous is not None and previous[1] != values:
                return [], False
            by_address.setdefault(address, (index, values))
        address, previous = row.get("particle_head" if fixed else f"{label}_0_address"), sentinel
        while address != sentinel:
            if address in seen or address not in by_address:
                return nodes, False
            index, values = by_address[address]
            if values.get("prev") != previous:
                return nodes, False
            nodes.append((index, address))
            seen.add(address)
            previous, address = address, values.get("next")
        complete = (fixed or seen == by_address.keys()) and (
            row.get("particle_tail" if fixed else f"{label}_{count - 1}_address") == previous)
        return nodes, complete
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
    if "object_lists" in row:
        return dict.fromkeys(OBJECT_SENTINELS, OBJECT_POOL_CAPACITY)
    return {label: sum(k.startswith(label + "_") and k.endswith(
                "_10" if label == "particle" and "particle_head" in row else "_address") for k in row)
            for label in OBJECT_SENTINELS}


def observation_tail_empty(row, counts, particle_addresses=None):
    """No active or pending effects; finished schedulers may remain allocated."""
    if stored_scene_active(row):
        return False
    for label in counts:
        nodes, complete = tracked_list(row, label, counts[label], particle_addresses)
        if not complete:
            return False
        for index, _ in nodes:
            kind = row[f"{label}_{index}_10"] >> 24
            # Shutdown only releases retained children; it cannot execute commands.
            # Every live child still blocks completion through its own closed list.
            if (label in ("program", "alternate") and kind == 0
                    and row.get(f"{label}_{index}_14") == 0x02000000):
                continue
            if label == "projectile" or kind != 6:
                return False
    return True


def recurring_effects(events, banks, setup=None, onset=None, scenery_pause=None):
    """Only independently witnessed, finite cosmetic emissions form a baseline."""
    authored = {bank["source_sha256"]: bank for bank in banks.values()}
    starts = {}
    for event in events:
        if event["kind"] == "program_started" and event["owner"] is not None:
            key = (event["owner"], event["bank"], event["program"])
            starts.setdefault(key, []).append(event)
    result = []
    for (owner, signature, program), observations in starts.items():
        ambient = setup.get("ready_ambient_effect") if setup else None
        scenery = (ambient if ambient and owner == 0 and
                   (ambient["bank"], ambient["program"]) == (signature, program) else None)
        registrations = [entry for entry in setup.get("ready_passive_effects", [])
                         if entry["actor"] == owner and entry["program"] == program] if setup else []
        if setup:
            registrations.extend(dict(entry, program=entry["slot"])
                                 for entry in setup.get("ready_attachment_effects", [])
                                 if entry["actor"] == owner and entry["slot"] == program)
        ends = authored[signature].get("program_ends", [])
        endpoint = ends[program] if program < len(ends) else None
        if not scenery and registrations:
            # A scheduler already present in the first sample has no witnessed
            # birth. Its later generations still prove the registered phase.
            observations = [event for event in observations if not event.get("observed_at_entry")]
        if scenery:
            # Establish recurrence on the uninterrupted clock. Later emissions
            # remain observed and compared, including their paused phase.
            if scenery_pause is not None:
                observations = [event for event in observations if event["presentation_tick"] < scenery_pause]
            ready = setup["initial_battle"]["presentation_tick"]
            aged = [event for event in observations if event.get("observed_age", 0) > 1]
            prelude = [event for event in observations if onset is not None and event["presentation_tick"] < onset]
            if not prelude or any(event["presentation_tick"] >= onset for event in aged) or any(
                    event.get("observed_age") != (
                        scenery["counter"] + event["presentation_tick"] - ready - 1
                    ) % scenery["interval"] + 1
                    for event in prelude):
                continue
            times = [event["presentation_tick"] for event in observations
                     if event.get("observed_age") == 1]
        else:
            times = [event["presentation_tick"] - event.get("frozen_updates", 0)
                     for event in observations]
        # A retained registration independently supplies the period and phase.
        # Observe one complete recurrence; unregistered emissions need two.
        if len(times) < (2 if scenery or registrations else 3):
            continue
        intervals = {b - a for a, b in zip(times, times[1:])}
        if len(intervals) != 1 or min(intervals) <= 0:
            continue
        commands = authored[signature]["metadata"]["programs"][program]
        if scenery:
            if min(intervals) != scenery["interval"] or any(
                    (time - ready + scenery["counter"] - 1) % scenery["interval"]
                    for time in times):
                continue
        elif setup is None:
            if len(commands) != 1:
                continue
        else:
            # Empty initialized pools and a zero counter prove a first generation
            # even when selection precedes the first scheduled body emission.
            lists = setup.get("ready_effect_lists", [])
            pristine = (len(lists) == 4 and
                        {item["kind"] for item in lists} == {"projectile", "program", "particle", "alternate"} and
                        all(item["next"] == item["previous"] == item["sentinel"] for item in lists) and
                        any(entry["counter"] == 0 for entry in registrations))
            if onset is None or not (observations[0]["presentation_tick"] < onset or pristine) or not any(
                    (observations[0]["presentation_tick"] < onset or entry["counter"] == 0) and
                    entry["interval"] == min(intervals) and
                    0 <= entry["counter"] <= entry["interval"] and all(
                        (time - setup["initial_battle"]["presentation_tick"]
                         - entry["interval"] + entry["counter"] - 1) % entry["interval"] == 0
                        for time in times) for entry in registrations):
                continue
        if not commands or any(command["operation"]["kind"] != "spawn" for command in commands):
            continue
        scheduler = None
        if not scenery:
            first_ages = {event.get("observed_age", 0) for event in observations}
            if endpoint is not None and len(first_ages) == 1:
                first_age, = first_ages
                if 0 <= first_age <= endpoint:
                    scheduler = dict(program_end=endpoint, observed_start_age=first_age)
        if not scenery and not scheduler and any(command["at"] != 0 for command in commands):
            continue
        members = sorted({command["operation"]["particle"] for command in commands})
        if not scenery and not scheduler and len(members) != len(commands):
            continue
        definitions = [authored[signature]["metadata"]["actors"][member] for member in members]
        if any(definition["kind"] not in ("particle", "model_particle")
               or not definition.get("template", {}).get("lifetime") for definition in definitions):
            continue
        if any(not any(event["kind"] == "declaration_started" and
                       (event["owner"], event["bank"], event["member"]) == (owner, signature, member)
                       for event in events) for member in members):
            continue
        for member, definition in zip(members, definitions):
            offsets = [command["at"] for command in commands if command["operation"]["particle"] == member]
            if scenery and any(not 0 <= at <= scenery["program_end"] for at in offsets):
                continue
            if scheduler:
                if any(not 0 <= at <= scheduler["program_end"] for at in offsets):
                    continue
                first_age = max(1, scheduler["observed_start_age"])
                pending_ages = [at + 1 for at in offsets] if scheduler["program_end"] else []
                offsets = [max(0, at + 1 - first_age) for at in offsets]
            result.append(dict(owner=owner, bank=signature, program=program, member=member,
                               lifetime=definition["template"]["lifetime"], interval=min(intervals),
                               observed_starts=times,
                               **({"clock_basis": "actor_updates", "presentation_starts":
                                   [event["presentation_tick"] for event in observations]}
                                  if not scenery and any(event.get("frozen_updates", 0) for event in observations) else {}),
                               **({"scheduler": dict(scheduler, spawn_offsets=offsets,
                                    **({"pending_ages": pending_ages} if pending_ages else {}))} if scheduler else {}),
                               **({"scenery": dict(program_end=scenery["program_end"],
                                                   spawn_offsets=offsets)} if scenery else {})))
    return result


def baseline_tail(row, counts, baseline, programs, particles, particle_addresses=None, frozen_updates=None, scenery_frozen=0):
    """Retain baseline observations; only their retirement requirement differs."""
    if not baseline or stored_scene_active(row):
        return False
    live_programs = live_particles = 0
    shutdown = []
    for label, count in counts.items():
        nodes, complete = tracked_list(row, label, count, particle_addresses)
        if not complete:
            return False
        for index, _ in nodes:
            kind = row[f"{label}_{index}_10"] >> 24
            if label == "projectile" or kind not in (0, 1, 6):
                return False
            live_programs += kind == 0
            live_particles += kind == 1
            if kind == 0:
                shutdown.append(row.get(f"{label}_{index}_14") == 0x02000000)
    if live_programs != len(programs) or live_particles != len(particles):
        return False
    presentation = row["battle_presentation_tick_word"]
    frozen_updates = frozen_updates or {}
    for program, finished in zip(programs, shutdown):
        if finished:
            continue
        if not any((program["owner"], program["bank"], program["program"]) ==
                   (entry["owner"], entry["bank"], entry["program"]) and
                   ((1 <= program["age"] <= entry["scenery"]["program_end"] and
                     (presentation - scenery_frozen - program["age"] + 1 - entry["observed_starts"][-1]) % entry["interval"] == 0)
                    if "scenery" in entry else
                    (entry.get("scheduler", {}).get("observed_start_age", 0) <= program["age"] <=
                     entry.get("scheduler", {}).get("program_end", 0) and
                     (presentation - frozen_updates.get(program["owner"], 0) - program["age"] + entry.get("scheduler", {}).get("observed_start_age", 0)
                      - entry["observed_starts"][-1]) % entry["interval"] == 0))
                   for entry in baseline):
            return False
    births = Counter()
    for particle, age in particles:
        identity = (particle["owner"], particle["bank"], particle["member"])
        birth = presentation - age + 1 if particle["initialized"] else presentation
        matching = [entry for entry in baseline if
                    identity == (entry["owner"], entry["bank"], entry["member"])]
        if matching and all("scenery" in entry for entry in matching):
            birth -= scenery_frozen
        elif matching and all("scenery" not in entry for entry in matching):
            birth -= frozen_updates.get(particle["owner"], 0)
        allowed = max((
                ([max(1, at) % entry["interval"] for at in entry["scenery"]["spawn_offsets"]] if "scenery" in entry else
                 [at % entry["interval"] for at in entry.get("scheduler", {}).get("spawn_offsets", [0])]).count(
                     (birth - entry["observed_starts"][-1]) % entry["interval"])
                if particle["initialized"] and 1 <= age <= entry["lifetime"] else
                int(not particle["initialized"] and age == 0 and "scenery" in entry and
                    0 in entry["scenery"]["spawn_offsets"] and
                    (presentation - scenery_frozen - entry["observed_starts"][-1]) % entry["interval"] == 0)
                for entry in matching), default=0)
        births[identity, birth] += 1
        if births[identity, birth] > allowed:
            return False
    return True


def completion_observer(source_owner, expected, normalize_tail=None, *, particle_addresses=None,
                        departure=False):
    """Wait for an empty tail or a positively observed persistent baseline.

    Full final normalization remains authoritative at the bounded capture end.
    Doubling the interval between baseline checks keeps total work linear while
    allowing a recurring baseline to become identifiable after the first return.
    """
    checked, admitted, counts, next_baseline_check = 0, False, None, 0
    departure_hp = None

    def done(rows):
        nonlocal checked, admitted, counts, next_baseline_check, departure_hp
        if rows and counts is None:
            counts = observation_counts(rows[0])
        for row in rows[checked:]:
            checked += 1
            action = row[f"battle_actor_{source_owner}_action"]
            bound = all(row[f"actor_{source_owner}_{key}"] &
                        (65535 if key == "native" else 0xffffffff) == value
                        for key, value in expected.items())
            if bound and ((departure and action >> 24 == 20) or
                          (not departure and action >> 24 == 5 and (action >> 16) & 255)):
                admitted = True
                if departure_hp is None:
                    departure_hp = row[f"battle_actor_{source_owner}_hp"]
            primary = row.get(f"battle_actor_{source_owner}_primary_phase_age_word", 0xc00000)
            secondary = row.get(f"battle_actor_{source_owner}_secondary_phase_age_word", 0xc00000)
            if ((primary | secondary) >> 16) & 0xc0:
                continue
            if row.get("battle_time_stop_word", 0) >> 16:
                continue
            departed = (departure and bound and action >> 24 == 18
                        and departure_hp is not None and departure_hp > 0
                        and row[f"battle_actor_{source_owner}_hp"] == departure_hp
                        and not row[f"actor_{source_owner}_control_word"] & 0x10000)
            terminal = departed or (not departure and (action >> 24 == 3 or
                (action >> 24 == 18 and primary >> 24 == 2 and normalize_tail is not None)))
            if not admitted or not terminal:
                continue
            if observation_tail_empty(row, counts, particle_addresses):
                return True
            if normalize_tail is not None and checked >= next_baseline_check:
                next_baseline_check = 2 * checked
                if normalize_tail(rows[:checked]):
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


def observed_birth_tick(row, previous, prefix, signature, setup, prior_particles,
                        programs, previous_programs):
    """A fresh particle and one owned scheduler step prove an emission tick."""
    observer = setup.get("observer", {})
    if (previous is None or not prefix.startswith("particle_")
            or signed16(row[prefix + "_278"] >> 16) != 1
            or row["battle_combat_tick_word"] != previous["battle_combat_tick_word"] + 1
            or row["battle_presentation_tick_word"] != previous["battle_presentation_tick_word"] + 1):
        return None
    parent = row[prefix + "_184"]
    if observer.get("particle_layout") == "fixed_pool" or "object_lists" in row:
        address = (row[prefix + "_address"] if "object_lists" in row else
                   observer["particle_addresses"][int(prefix.split("_")[1])])
        changed = (address not in prior_particles
            or any(row[f"{prefix}_{at:x}"] != previous[f"{prefix}_{at:x}"]
                   for at in (*range(0x28, 0x40, 4), 0x184))
            or signed16(previous[prefix + "_278"] >> 16) != 1)
    else:
        # Linked-list indices can move; only an absent-to-present node proves a birth.
        address = row.get(prefix + "_address")
        changed = bool(address) and address not in prior_particles
    actors = setup["initial_battle"]["actors"]
    if not changed or signature[2] not in {actor["address"] for actor in actors}:
        return None
    embedded = next((actor for actor in actors if actor["address"] + 0x15b4 == parent), None)
    if embedded:
        key = f"actor_{embedded['slot']}_effect_"
        if (signature[2] != embedded["address"] or
                tuple(previous[key + field] for field in ("bank", "program")) != signature[:2]):
            return None
        current_age = signed16(row[key + "clock_word"] & 65535)
        previous_age = signed16(previous[key + "clock_word"] & 65535)
    else:
        current, prior = programs.get(parent - 0x28), previous_programs.get(parent - 0x28)
        if (not current or not prior or current["id"] != prior["id"]
                or current["signature"] != signature or prior["signature"] != signature):
            return None
        current_age, previous_age = current["age"], prior["age"]
    return previous_age if previous_age >= 0 and current_age == previous_age + 1 else None


def declaration_matches(bank, copied, parent_root, *, model_binding=None, initialized=False, age=0,
                        birth_tick=None, size=None, owner_bone=None, angles=None, offset=None, heading=None,
                        model_attachment=None):
    """Exact copied prefix, including supported authored birth overrides."""
    retained = int.from_bytes(copied[20:24], "big") & 0x40000
    if retained and copied[5] not in (*range(8), 255):
        return []

    def signature(raw):
        prefix = bytearray(raw[:24])
        prefix[3] = 0
        if int.from_bytes(prefix[20:24], "big") & 0x40000:
            # Retained particles receive a live scheduler slot after copying.
            prefix[5] = 0
        if prefix[0] == 4 and int.from_bytes(prefix[20:24], "big") & 0x100000:
            # Drawing a segmented ring sets this cached flag in the copied header.
            prefix[23] &= ~0x10
        return bytes(prefix)
    actual = signature(copied)

    def first_sprite_uv(member):
        raw = bank["declarations"][member]
        if not (initialized and age == 1 and raw[0] != 3 and len(raw) > 0x32 and raw[0x32] < 128):
            return None
        at = raw[0x32] * 10
        descriptor = bank.get("uv_descriptors", b"")[at:at + 10]
        if len(descriptor) == 10 and 1 <= descriptor[0] < 128 and descriptor[2:4] != b"\x83\0":
            return descriptor[2:10]
        return None

    def matches_integer(value, number, size):
        low, high, step = value["min"], value["max"], value.get("step", 0)
        if not all(math.isfinite(v) and float(v).is_integer() for v in (low, high, step)):
            return False
        if not 0 <= low <= number <= high < 1 << (size * 8) or step < 0:
            return False
        distance = high - number if value.get("reverse", False) else number - low
        return low == high or bool(step and distance % step == 0)

    def matches_integer_sums(sums, choices):
        """Solve authored integer sums without dropping shared random choices."""
        if not sums:
            return True
        number, terms = sums[0]

        def assign(index, remainder, selected):
            if index == len(terms):
                return remainder == 0 and matches_integer_sums(sums[1:], selected)
            term = terms[index]
            low, high, step = term["min"], term["max"], term.get("step", 0)
            if (not all(math.isfinite(v) and float(v).is_integer() for v in (low, high, step))
                    or low > high or low != high and (step <= 0 or (high - low) % step)):
                return False
            count = int((high - low) / step) if low != high else 0
            prior = selected.get(term.get("draw")) if "draw" in term and count else None
            if prior is not None and prior[0] != count:
                return False
            indices = [int(prior[1])] if prior is not None else range(count + 1)
            remaining = terms[index + 1:]
            minimum = sum(value["min"] for value in remaining)
            maximum = sum(value["max"] for value in remaining)
            for choice in indices:
                value = high - step * choice if term.get("reverse", False) else low + step * choice
                if not minimum <= remainder - value <= maximum:
                    continue
                next_choices = selected
                if "draw" in term and count and prior is None:
                    next_choices = selected.copy()
                    next_choices[term["draw"]] = (count, choice)
                if assign(index + 1, remainder - value, next_choices):
                    return True
            return False
        return assign(0, number, choices)

    def matches_member(member, expected, *, uv_override=False, model_override=None, animation_override=None,
                       size_override=(), palette_override=None, angles_override=(), angular_velocity_override=(),
                       yaw_heading=0, animation_starts=None, attachment_bone=None, motion_birth=None):
        raw = bank["declarations"][member]
        actors = bank.get("metadata", {}).get("actors", [])
        metadata = actors[member] if member < len(actors) else {}
        descriptor = metadata.get("template", {})
        template = descriptor.get("template", descriptor)
        state = template.get("state", {})
        if model_attachment is not None:
            attachment = metadata.get("binding", {}).get("attachment")
            if attachment is None or (attachment["joint"], attachment["retained"]) != model_attachment:
                return False
        rectangle = first_sprite_uv(member)
        if rectangle is not None:
            # The first UV frame replaces even an authored birth override.
            expected = bytearray(expected)
            expected[8:16] = rectangle
        if owner_bone is not None:
            expected_bone = template.get("owner_bone") if attachment_bone is None else attachment_bone
            if expected_bone != owner_bone:
                return False
        if offset is not None and heading is not None and initialized and age == 1 and raw[0] != 3:
            birth = motion_birth or {}
            overridden = any(birth.get(key) for key in ("offset", "velocity", "polar_offset", "polar_velocity"))
            overridden |= any(value["field"]["kind"] in ("offset", "velocity") for value in birth.get("sums", []))
            if (not overridden and owner_bone is None
                    and all(template.get(key) is None for key in ("owner_bone", "follow_bone"))
                    and not any(template.get(key) for key in
                    ("origin_motion", "ground_clamp", "ground_restitution"))
                    and len(state.get("offset", [])) == len(state.get("velocity", [])) == 3):
                sine, cosine = math.sin(math.radians(heading)), math.cos(math.radians(heading))
                local = [offset[0] * cosine - offset[2] * sine, offset[1],
                         offset[0] * sine + offset[2] * cosine]
                # Opposite authored directions distinguish mirrored births
                # without requiring identical trigonometric rounding.
                for observed, start, velocity in zip(local, state["offset"], state["velocity"]):
                    if start * velocity >= 0 and observed * (start + velocity) < 0:
                        return False
        if (len(raw) > 0x32 and raw[0x32] >= 128
                and not int.from_bytes(raw[20:24], "big") & 0x400000):
            # Untinted declarations without a UV track keep their birth palette.
            palette = raw[3] if palette_override is None else palette_override
            if copied[3] != palette:
                return False
        if (raw[0] == 15 and len(raw) >= 264 and model_binding is not None
                and struct.unpack_from(">f", raw, 260)[0] == 0):
            # This union word is the fourth quad vertex's stationary X coordinate.
            if int.from_bytes(raw[212:216], "big") != model_binding:
                return False
        if size is not None and initialized and age in (0, 1):
            geometry = state.get("geometry", {}).get("Size")
            if geometry:
                for axis, actual_size in enumerate(size):
                    overridden = axis < len(size_override) and size_override[axis] is not None
                    if (not overridden and geometry["velocity"][axis] == 0 and
                            geometry["acceleration"][axis] == 0 and
                            (len(raw) < 188 or actual_size !=
                             struct.unpack_from(">f", raw, 176 + 4 * axis)[0])):
                        return False
        if angles is not None and initialized and age in (0, 1) and raw[0] != 3:
            for axis, actual_angle in enumerate(angles):
                overridden = descriptor.get("fixed_orientation", False) or (
                    axis == 1 and yaw_heading != 0) or (
                    axis < 2 and descriptor.get("follow_orientation", False)) or any(
                    axis < len(values) and values[axis] is not None
                    for values in (angles_override, angular_velocity_override))
                if (not overridden and len(state.get("angular_velocity", [])) == 3
                        and len(state.get("angles", [])) == 3
                        and state["angular_velocity"][axis] == 0
                        and actual_angle != state["angles"][axis]):
                    return False
        model = raw[0] == 3
        if model and model_binding is not None:
            binding = bytearray(raw[212:216])
            if len(binding) != 4:
                return False
            if model_override is not None:
                observed_model = model_binding >> 24
                if not matches_integer(model_override, observed_model, 1):
                    return False
                binding[0] = observed_model
            if not int.from_bytes(raw[20:24], "big") & 0x24 and animation_override is None:
                # Disabled animation selectors do not change the selected model.
                binding[1] = (model_binding >> 16) & 255
            if initialized:
                # Holding an existing frame keeps its controller; only a new
                # animation switches to the other controller slot.
                binding[2] = (int(animation_override is not None and animation_override.get("rate") != 0)
                              if animation_starts is None else animation_starts % 2)
                if animation_override is not None and model_binding >> 24 != animation_override["model"]:
                    return False
            if int.from_bytes(binding, "big") != model_binding:
                return False
        if signature(expected) == actual:
            return True
        if not (initialized and age in (0, 1) and len(raw) > 0x32 and not uv_override):
            return False
        selector = raw[0x32] if raw[0x32] < 128 else raw[0x32] - 256
        descriptors = bank.get("uv_descriptors", b"")
        descriptor = descriptors[selector * 10:selector * 10 + 10] if selector >= 0 else b""
        if len(descriptor) != 10:
            return False
        if not model:
            return False
        if model_binding is None or descriptor[:2] != b"\x80\0":
            return False
        step_x, step_y, width, height = struct.unpack_from(">4h", descriptor, 2)
        x, y = struct.unpack_from(">2h", expected, 8)
        # Only the observed first-update scroll branch is registered here.
        # Palette replacement, wrapping and later first samples remain unsupported.
        if not (-width < step_x < width and -height < step_y < height
                and 0 <= x < width and 0 <= y < height
                and -width < x + step_x < width and -height < y + step_y < height):
            return False
        expected = bytearray(expected)
        expected[8:16] = struct.pack(">4h", x + age * step_x, y + age * step_y, width, height)
        return signature(expected) == actual

    if parent_root not in bank["roots"]:
        return [i for i, raw in enumerate(bank["declarations"]) if matches_member(i, raw[:24])]
    commands = bank["metadata"]["programs"][bank["roots"].index(parent_root)]
    spawn_times, birth_motions = [], {}
    for command in commands:
        op = command["operation"]
        if op["kind"] == "spawn":
            spawn_times.append(command.get("at"))
        elif (op["kind"] == "set_motion" and "animation" in op
              and command.get("at") is not None and op["spawn"] < len(spawn_times)
              and spawn_times[op["spawn"]] == command["at"]):
            birth_motions.setdefault(op["spawn"], []).append(op["animation"])
    matches = set()
    spawn_index = -1
    for command in commands:
        op = command["operation"]
        if op["kind"] != "spawn":
            continue
        spawn_index += 1
        member, birth = op["particle"], op.get("birth", {})
        animations = ([birth["animation"]] if "animation" in birth else []) + birth_motions.get(spawn_index, [])
        rectangle = first_sprite_uv(member)
        uv_sums = [value for value in birth.get("sums", [])
                   if value["field"]["kind"] == "uv" and rectangle is None]
        size_overrides = list(birth.get("size", [None] * 3))
        for value in birth.get("sums", []):
            if value["field"]["kind"] == "size":
                size_overrides[value["field"]["axis"]] = value
        expected = bytearray(bank["declarations"][member][:24])
        if op.get("blend") is not None:
            if op["blend"] not in (0, 1):
                continue
            expected[1] = op["blend"]
        if birth.get("cull_back"):
            expected[20] |= 0x10
        if birth.get("ribbon_depth"):
            expected[23] |= 0x20
        changes = [(8 + axis * 2,2,value) for axis,value in enumerate(birth.get("uv", []))
                   if value and rectangle is None]
        if birth.get("lifetime"):
            duration = dict(birth["lifetime"])
            # Finite lifetimes store the final active age in the copied header.
            for edge in ("min", "max"):
                if duration[edge] > 0:
                    duration[edge] -= 1
            changes.append((16,2,duration))
        if birth.get("geometry_count"):
            changes.append((18,1,birth["geometry_count"]))
        if birth.get("phase"):
            changes.append((19,1,birth["phase"]))
        if birth.get("palette"):
            changes.append((3,1,birth["palette"]))
        choices = {}
        for byte_offset, width, value in changes:
            number = int.from_bytes(copied[byte_offset:byte_offset + width], "big")
            if not matches_integer(value, number, width):
                break
            if "draw" in value and value["min"] != value["max"]:
                distance = value["max"] - number if value.get("reverse", False) else number - value["min"]
                choice = ((value["max"] - value["min"]) / value["step"], distance / value["step"])
                if choices.setdefault(value["draw"], choice) != choice:
                    break
            expected[byte_offset:byte_offset + width] = number.to_bytes(width,"big")
        else:
            sums = []
            for value in uv_sums:
                byte_offset = 8 + value["field"]["axis"] * 2
                number = int.from_bytes(copied[byte_offset:byte_offset + 2], "big")
                expected[byte_offset:byte_offset + 2] = copied[byte_offset:byte_offset + 2]
                sums.append((number, value["terms"]))
            if not matches_integer_sums(sums, choices):
                continue
            if matches_member(member, expected, uv_override=bool(birth.get("uv") or uv_sums),
                              model_override=birth.get("model"),
                              animation_override=animations[-1] if animations else None,
                              animation_starts=sum(animation.get("rate") != 0 for animation in animations),
                              size_override=size_overrides, angles_override=birth.get("angles", ()),
                              angular_velocity_override=birth.get("angular_velocity", ()),
                              yaw_heading=birth.get("yaw_heading", 0),
                              attachment_bone=birth.get("attachment_bone"),
                              motion_birth=birth,
                              palette_override=copied[3] if birth.get("palette") else op.get("palette")):
                matches.add(member)
    if len(matches) > 1 and birth_tick is not None:
        timed = matches & {command["operation"]["particle"] for command in commands
            if command.get("at") == birth_tick and command["operation"]["kind"] == "spawn"}
        if len(timed) == 1:
            return sorted(timed)
    return sorted(matches)


def stun_particle_identity(row, prefix, copied, setup, banks, *, initialized=True):
    """Bind an actor-owned stun sprite without inventing an effect-program parent."""
    if row[prefix + "_184"] or (initialized and signed16(row[prefix + "_278"] >> 16) not in (0, 1)):
        return None
    owners = [(index, actor) for index, actor in enumerate(setup["initial_battle"]["actors"])
              if actor["address"] + enemy_artes.POSITION_X == row[prefix + "_1c"]]
    if len(owners) != 1:
        return None
    owner, actor = owners[0]
    slot = actor["slot"]
    if row[f"battle_actor_{slot}_action"] >> 24 != STUN_ACTION:
        return None
    remaining = signed16(row[f"battle_actor_{slot}_cast_clock_word"] & 65535)
    if remaining <= 0:
        return None
    bank = next((bank for bank in banks.values()
                 if bank["source_sha256"] == setup.get("common_effect_bank")), None)
    if bank is None or len(bank["declarations"]) <= STUN_DECLARATION:
        return None
    if not initialized:
        return (bank, STUN_DECLARATION, owner) if copied == bank["declarations"][STUN_DECLARATION][:24] else None
    metadata = bank["metadata"]["actors"][STUN_DECLARATION]
    template = metadata.get("template", {})
    animation = template.get("uv_animation") or {}
    frames = animation.get("frames", [])
    if metadata["kind"] != "particle" or template.get("lifetime") is not None or not frames:
        return None
    change = frames[0]["change"]
    if change["kind"] != "rectangle":
        return None
    expected = bytearray(bank["declarations"][STUN_DECLARATION][:24])
    expected[8:16] = struct.pack(">4h", *change["rect"])
    count = min(remaining // 45 + 1, 4)
    expected[18] = 4 if count == 3 else count
    if copied[:3] + copied[4:] != expected[:3] + expected[4:]:
        return None
    return bank, STUN_DECLARATION, owner


def damage_context(state, battle):
    """Read conservative immutable prerequisites; volatile guards are watched."""
    history = bytes(state.ram[0x1fabd0:0x1fabf0])
    result = []
    for actor in battle["actors"]:
        at, blocked = actor["address"], []
        profile, saved, weapon = state.read(at + 4), state.read(at), state.read(at + 0x10)
        weapon_species = None
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
            else:
                effect = state.read(weapon + 0x14, "B")
                if 0x61 <= effect <= 0x6c:
                    weapon_species = effect - 0x60
        species = state.read(profile + 0x2a, "H")
        count = history.count(species & 255)
        result.append(dict(blocked=blocked, species=species, weapon_species=weapon_species,
            guard_reduction=state.read(profile + 0x2f, "B"),
            guard_pressure_limit=state.read(profile + 0x58, "H"),
            familiarity=100 + (count >= 4) + (count >= 12) + 2 * (count >= 16)))
    return result


def guard_kind(row, slot):
    flags = (row[f"actor_{slot}_raw_280"] >> 8) & 0x41
    return ("special" if flags & 0x40 else "normal") if flags & 1 else None


def contact_guard(row, previous, setup, target):
    slot = setup["source_actor_map"][target]["source_slot"]
    before, after = guard_kind(previous, slot), guard_kind(row, slot)
    scenario = setup.get("recipient_input")
    if before is None and after is None:
        return None
    if scenario is None or scenario["actor"] != target:
        raise damage_envelope.UnsupportedDamage("guard contact lacks an observed recipient input")
    mode = scenario["mode"]
    permitted = {None, "normal", "special"} if mode == "special" else {None, "normal"} if mode == "normal" else {None}
    if before not in permitted or after not in permitted:
        raise damage_envelope.UnsupportedDamage("incoming contact has an unrequested guard stance")
    outcome = after
    if before != after:
        contact = queued_contact(row, setup, setup["owner"], target, previous)
        if (before != "normal" or after is not None or contact is None
                or not (contact["rule_flags_element"] >> 16) & 0x80
                or row[f"battle_actor_{slot}_action"] >> 24 not in (9, 14)):
            raise damage_envelope.UnsupportedDamage("guard transition lacks an attributed breaking hit")
        outcome = "broken"
    for observed in (previous, row):
        control = observed[f"actor_{slot}_control_word"]
        if (control >> 24) & 0x38 or (control >> 16) & 0xc0:
            raise damage_envelope.UnsupportedDamage("recipient is not on manual controller zero")
        if (observed[f"actor_{slot}_raw_280"] >> 28) & 7:
            raise damage_envelope.UnsupportedDamage("recipient has contact protection beyond guarding")
    if after == "normal":
        limit = setup["damage_context"][target]["guard_pressure_limit"]
        if not 0 < (row[f"actor_{slot}_control_word"] & 31) < limit:
            raise damage_envelope.UnsupportedDamage("normal guard pressure did not admit an unbroken contact")
    return outcome


def contact_protection_shift(row, previous, contact, setup, target):
    """Bind the recipient's protection at contact, including a witnessed landing."""
    slot = setup["source_actor_map"][target]["source_slot"]
    prefix = f"actor_{slot}_"
    before, after = ((observed[prefix + "raw_280"] >> 28) & 7
                     for observed in (previous, row))
    if before in (0, 1) and after in (0, 1):
        return 0
    window = prefix + "protection_window_word"
    if (before in (0, 1) and after == 2
            and row[f"battle_actor_{slot}_action"] >> 24 == 14
            and previous[f"battle_actor_{slot}_action"] >> 24 != 14
            and row[f"battle_actor_{slot}_hp"] < previous[f"battle_actor_{slot}_hp"]):
        if window not in previous or window not in row:
            raise damage_envelope.UnsupportedDamage("knockdown entry window was not observed")
        initialized = (row[window] >> 16) & 255
        if not row[window] >> 24 and initialized == 0:
            # The contact entered knockdown; its down callback has not run yet.
            return 0
        if (previous[f"battle_actor_{slot}_action"] >> 24 == 9
                and previous[prefix + "target_state_word"] >> 24 == 13
                and row[prefix + "target_state_word"] >> 24 == 15
                and previous.get(prefix + "recoil_word", 0) & 3
                and previous[f"battle_actor_{slot}_y_bits"] == 0
                and row[f"battle_actor_{slot}_y_bits"] == 0
                and previous[window] >> 16 == 0
                and row[window] >> 24 and initialized == 1
                and row["battle_combat_tick_word"] == previous["battle_combat_tick_word"] + 1):
            # Hurt movement landed and initialized the down window before this
            # update's contact pass. The new hit did not create that protection.
            return 0 if (contact["rule_flags_element"] >> 16) & 0x40 else 3
        raise damage_envelope.UnsupportedDamage("knockdown entry order is unverified")
    if before != 2 or after != 2:
        raise damage_envelope.UnsupportedDamage("contact protection is not a stable knockdown")
    if any(window not in observed or not observed[window] >> 24 for observed in (previous, row)):
        raise damage_envelope.UnsupportedDamage("knockdown contact window is not observed active")
    return 0 if (contact["rule_flags_element"] >> 16) & 0x40 else 3


def hostile_condition_outcome(setup, events):
    """Recognize an attributed harmful condition as an offensive outcome."""
    owner = setup["owner"]
    actions = {event["action"] for event in events
               if event["kind"] == "started" and event.get("owner") == owner}
    return any(event["kind"] == "condition_changed" and event.get("owner") == owner
               and event.get("action") in actions
               and type(event.get("target")) is int
               and 0 <= event["target"] < len(setup["actors"])
               and setup["actors"][event["target"]]["side"] != setup["actors"][owner]["side"]
               and (set(event["after"]) - set(event["before"]))
                   & (support_outcomes.AILMENTS | {"recovery_down"})
               for event in events)


def observed_outcome(context, setup, events):
    """An observed owned affliction supersedes an obsolete presentation label."""
    return "attack" if context == "presentation" and hostile_condition_outcome(setup, events) else context


def recovery_protected_contact(row, previous, setup, owner, target):
    """Observe a fresh attached contact avoided during party recovery."""
    slot = setup["source_actor_map"][target]["source_slot"]
    owner_slot = setup["source_actor_map"][owner]["source_slot"]
    victim = setup["initial_battle"]["actors"][target]["address"]
    if (setup["actors"][target]["side"] != 0
            or row[f"battle_actor_{slot}_hp"] != previous[f"battle_actor_{slot}_hp"]
            or row[f"actor_{owner_slot}_contact_target"] != victim
            or previous[f"actor_{owner_slot}_contact_target"] == victim
            or any(((observed[f"actor_{slot}_raw_280"] >> 28) & 7) != 4
                   for observed in (previous, row))):
        return None
    for actor in setup["source_actor_map"]:
        other = actor["source_slot"]
        if (other not in (slot, owner_slot)
                and row[f"actor_{other}_contact_target"] == victim
                and previous[f"actor_{other}_contact_target"] != victim):
            raise damage_envelope.UnsupportedDamage("protected contact has competing attackers")
    contact = queued_contact(row, setup, owner, target, previous)
    if contact is None or contact["projectile"] or "detached_address" in contact:
        raise damage_envelope.UnsupportedDamage("protected contact lacks a unique attached rule")
    return dict(kind="hit", owner=owner, target=target, action=1, projectile=None,
                hp_change=0, protection="recovery", damage_range=[0, 0], critical=False,
                damage_branches=dict(ordinary=[0, 0], critical=None))


def protected_projectile_contact(row, previous, setup, owner, target, projectile_ids):
    """Bind a fresh zero-HP projectile collision to observed party protection."""
    slot = setup["source_actor_map"][target]["source_slot"]
    prefix = f"actor_{slot}_"
    if (setup["actors"][target]["side"] != 0
            or setup["actors"][owner]["side"] == 0
            or row[f"battle_actor_{slot}_hp"] <= 0
            or row[f"battle_actor_{slot}_hp"] != previous[f"battle_actor_{slot}_hp"]
            or row.get(prefix + "fresh_contact_word", 0) >> 24 != 1
            or row["battle_combat_tick_word"] != previous["battle_combat_tick_word"] + 1):
        return None
    modes = [((sample[prefix + "raw_280"] >> 28) & 7) for sample in (previous, row)]
    if modes == [2, 2] and all(prefix + "protection_window_word" in sample
            and sample[prefix + "protection_window_word"] >> 24 == 0
            for sample in (previous, row)):
        protection = "down_window_closed"
    elif modes == [4, 4]:
        protection = "recovery"
    else:
        return None
    contact = queued_contact(row, setup, owner, target, previous)
    if contact is None or not contact["projectile"]:
        return None
    address = contact["projectile"] - 0x28
    pid = projectile_ids.get(address)
    if pid is None:
        raise damage_envelope.UnsupportedDamage("protected projectile has no observed identity")
    victim = setup["initial_battle"]["actors"][target]["address"]
    recipient = (row[prefix + "control_word"] >> 24) & 7
    cooldown, shift = f"cooldown_{recipient // 4}", 24 - recipient % 4 * 8
    duration = contact.get("rule_timing", 0) >> 24
    nodes, complete = tracked_list(previous, "projectile", observation_counts(previous)["projectile"])
    before = [f"projectile_{index}" for index, at in nodes if at == address]
    if not complete or len(before) != 1:
        raise damage_envelope.UnsupportedDamage("protected projectile lacks its adjacent live observation")
    old = before[0]
    if (previous[old + "_14"] >> 24 != 1
            or previous[old + "_28"] != contact["owner"]
            or previous[old + "_16c"] != contact["rule"]
            or previous[old + "_170"] != contact["template"]
            or signed16(previous[old + "_278"] >> 16) + 1 != contact["projectile_age"]
            or contact.get("last_target") != victim
            or contact["projectile_result"] not in (1, 17)
            or not contact["nominal"]
            or duration <= 1 or cooldown not in contact or old + "_" + cooldown not in previous
            or (contact[cooldown] >> shift) & 255 != duration
            or (previous[old + "_" + cooldown] >> shift) & 255 > 1):
        raise damage_envelope.UnsupportedDamage("protected projectile lacks a fresh recipient cooldown reset")
    return dict(kind="hit", owner=owner, target=target, action=1, projectile=pid,
                hp_change=0, protection=protection, damage_range=[0, 0], critical=False,
                damage_branches=dict(ordinary=[0, 0], critical=None))


def contact_damage_range(row, previous, contact, setup, owner, target, previous_hp, strength, *, critical=None):
    contexts = setup.get("damage_context")
    if not contexts:
        raise damage_envelope.UnsupportedDamage("ready damage context was not observed")
    slots = [entry["source_slot"] for entry in setup["source_actor_map"]]
    guarded = contact_guard(row, previous, setup, target)
    if guarded == "broken":
        guarded = None
    protection_shift = contact_protection_shift(row, previous, contact, setup, target)
    applied = contact_condition(row, previous, contact, setup, owner, target)
    magic = (contact["contact_kind"] >> 16) & 255 == 2
    stat_adjustments, magic_modifiers = [0, 0, 0, 0], []
    physical_stats = {"attack_up", "attack_down", "defense_up", "defense_down",
                      "accuracy_up", "accuracy_down", "evasion_down"}
    for actor in (owner, target):
        p = f"actor_{slots[actor]}_"
        if contexts[actor]["blocked"]:
            raise damage_envelope.UnsupportedDamage("; ".join(contexts[actor]["blocked"]))
        # A contact applies its ailment after calculating this hit's damage.
        mutable = previous if actor == target and applied is not None else row
        flags = ((mutable[p + "condition_10"] << 32) | mutable[p + "condition_14"]
                 | (row[p + "condition_38"] << 32) | row[p + "condition_3c"]
                 | (row[p + "raw_1d0"] << 32) | row[p + "raw_1d4"])
        # Conditions are role-sensitive: a caster's barrier cannot change its
        # outgoing hit, and physical stat buffs do not scale magical damage.
        admitted = {"poison_mild", "poison_severe", "paralysis", "movement_boost", "heavy",
                    "reduce_item_effect"}
        if actor == owner:
            admitted.update(("physical_barrier", "magical_barrier", "defense_up", "defense_down",
                             "magic_defense_up", "magic_defense_down", "evasion_down"))
        else:
            admitted.update(("attack_up", "attack_down", "accuracy_up", "accuracy_down",
                             "magic_attack_up", "magic_attack_down"))
        if magic:
            admitted.update(physical_stats | {"holy_song"})
            modifiers = (("magic_attack_up", "magic_attack_down", "physical_protection", "magical_protection")
                         if actor == owner else ("magic_defense_down", "magic_defense_up"))
            for name in modifiers:
                if any(flags & (1 << bit) for bit, condition in support_outcomes.CONDITION_BITS.items()
                       if condition == name):
                    magic_modifiers.append(name)
            admitted.update(modifiers)
        else:
            admitted.update(("magic_attack_up", "magic_attack_down", "magic_defense_up", "magic_defense_down"))
            bindings = ((0, ("attack_up", "attack_down"), 0, 16),
                        (2, ("accuracy_up", "accuracy_down"), 1, 16)) if actor == owner else (
                        (1, ("defense_up", "defense_down"), 0, 0),
                        (3, ("evasion_down",), 1, 0))
            for index, names, word, shift in bindings:
                if any(flags & (1 << bit) for bit, name in support_outcomes.CONDITION_BITS.items()
                       if name in names):
                    key = p + f"stat_adjustment_{word}"
                    if key not in row:
                        raise damage_envelope.UnsupportedDamage("live physical stat adjustment was not observed")
                    stat_adjustments[index] = signed16((row[key] >> shift) & 65535)
                admitted.update(names)
        observed = sum(1 << bit for bit, name in support_outcomes.CONDITION_BITS.items() if name in admitted)
        conditions = flags & ~observed
        # The pending charge flag needs an equipped compound EX skill to alter
        # damage; the context check above excludes those skills already.
        if conditions or row[p + "raw_1aa8"] & 0x03000040 or row[p + "control_word"] & 0xe0:
            raise damage_envelope.UnsupportedDamage("live condition/equipment/charge modifier")
        if (((row[p + "raw_280"] >> 8) & 0x41) and (actor != target or guarded is None)
                or actor == target and ((row[p + "control_word"] >> 16) & 7) == 4):
            raise damage_envelope.UnsupportedDamage("live guard or special control class")
    if (row["damage_session_1f44"] >> 16 or row["damage_session_1f5c"] >> 8 or
        row["damage_battle_15728"] >> 24 or (row["damage_battle_15714"] >> 24) & 8):
        raise damage_envelope.UnsupportedDamage("session, Unison or All Divide modifier")
    p, t = f"actor_{slots[owner]}_", f"actor_{slots[target]}_"
    if row[t + "condition_5c"] & 4:
        raise damage_envelope.UnsupportedDamage("forced one-damage target")
    element = (contact["rule_flags_element"] >> 8) & 255
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
    kind_value = (contact["contact_kind"] >> 16) & 255
    if kind_value > 2:
        raise damage_envelope.UnsupportedDamage("unbound contact damage kind")
    kind = ("slash", "thrust", "magic")[kind_value]
    power_word = contact["rule_power"]
    power_mode = (power_word >> 16) & 255
    power = "normal" if power_mode in (0,2) else {"percent": power_word & 65535} if power_mode == 1 else None
    if power_mode == 4:
        # Several collision volumes can carry the same hit. Bind the accepted
        # contact before recovering its pre-hit strength from adjacent samples.
        before = signed16(previous[p + "attack_power_word"] & 65535)
        after = signed16(row[p + "attack_power_word"] & 65535)
        adjusted = before + signed16(power_word & 65535)
        fresh = sum(value >> 24 == 1 for key, value in row.items()
                    if key.startswith("actor_") and key.endswith("_fresh_contact_word"))
        bound = queued_contact(row, setup, owner, target, previous)
        projectile = bool(contact.get("projectile"))
        bound_matches = (bound is not None and all(bound.get(key) == value for key, value in contact.items()))
        if (kind == "magic" or not bound_matches
                or row["battle_combat_tick_word"] != previous["battle_combat_tick_word"] + 1
                or fresh != 1
                or not 10 <= before <= 32767 or not -32768 <= adjusted <= 32767
                or after != max(10, adjusted)
                or strength != (contact.get("projectile_strength") if projectile else after)):
            raise damage_envelope.UnsupportedDamage("post-contact power adjustment is not uniquely observed")
        # A projectile retains its launch strength. The adjustment changes
        # only its owner's later attacks, even if launch and hit power differ.
        strength, power = strength if projectile else before, "normal"
    weapon_species = contexts[owner].get("weapon_species")
    species_match = False
    if kind != "magic" and weapon_species is not None:
        if "species" not in contexts[target]:
            raise damage_envelope.UnsupportedDamage("weapon species target was not observed")
        species_match = weapon_species == contexts[target]["species"]
    strength = row[p + "cast_strength_word"] >> 24 if kind == "magic" else strength
    proficiency = tuple((row[p + "proficiency_word"] >> shift) & 255 for shift in (24,16,8))
    context = damage_envelope.Context(strength, proficiency, contexts[target]["familiarity"],
        False if kind == "magic" else critical, previous[t + "target_state_word"] >> 24 == 17,
        {0: "normal", 1: "weak", 2: "resistant"}.get(affinity, "unsupported"), (), True,
        weapon_species_match=species_match, guard=guarded,
        guard_reduction=contexts[target]["guard_reduction"] if guarded == "normal" else None,
        protection_shift=protection_shift, stat_adjustments=tuple(stat_adjustments),
        magic_modifiers=tuple(magic_modifiers))
    bounds = damage_envelope.envelope(dict(kind=kind, power=power), setup["actors"][owner], setup["actors"][target], context)
    bounds = damage_envelope.hp_loss_bounds(bounds, previous_hp, prevents_defeat=False)
    return [bounds["minimum"], bounds["maximum"]]


def contact_fields(row, prefix):
    fields = {name: row[prefix + "_" + name]
              for name in ("rule_flags_element", "rule_power", "contact_kind")}
    if prefix + "_rule_conditions" in row:
        fields["rule_conditions"] = row[prefix + "_rule_conditions"]
    if prefix.startswith("projectile_"):
        fields.update(projectile=row[prefix + "_address"] + 0x28,
                      projectile_strength=row[prefix + "_144"] >> 16)
    return fields


def contact_condition(row, previous, contact, setup, owner, target):
    """Attribute a newly applied ailment only to its fresh, bound damaging hit."""
    slot = setup["source_actor_map"][target]["source_slot"]
    source = setup["source_actor_map"][owner]["source_slot"]
    prefix = f"actor_{slot}_"
    before = (previous[prefix + "condition_10"], previous[prefix + "condition_14"])
    after = (row[prefix + "condition_10"], row[prefix + "condition_14"])
    if before == after:
        return None
    mask = contact.get("rule_conditions", 0)
    if (not mask or mask & (mask - 1) or before[0] != after[0]
            or before[1] & mask or after[1] != before[1] | mask
            or not contact["rule_power"] >> 24
            or row[prefix + "fresh_contact_word"] >> 24 != 1
            or row[f"actor_{source}_contact_target"] != setup["initial_battle"]["actors"][target]["address"]
            or row[f"battle_actor_{slot}_hp"] >= previous[f"battle_actor_{slot}_hp"]
            or contact_guard(row, previous, setup, target) in ("normal", "special")):
        raise damage_envelope.UnsupportedDamage("condition change is not a new bound contact ailment")
    name = support_outcomes.CONDITION_BITS.get(mask.bit_length() - 1)
    if name not in support_outcomes.AILMENTS:
        raise damage_envelope.UnsupportedDamage("contact condition is not a supported ailment")
    return dict(kind="condition_changed", owner=owner, target=target, action=1,
                before=support_outcomes.conditions(*before),
                after=support_outcomes.conditions(*after))


def projectile_damage_range(row, previous, prefix, setup, owner, target, previous_hp):
    return contact_damage_range(row, previous, contact_fields(row, prefix), setup, owner, target,
                                previous_hp, row[prefix + "_144"] >> 16)


def contact_damage_branches(row, previous, contact, setup, owner, target, previous_hp, strength, loss):
    ordinary = contact_damage_range(row, previous, contact, setup, owner, target, previous_hp, strength, critical=False)
    magic = (contact["contact_kind"] >> 16) & 255 == 2
    critical = None if magic else contact_damage_range(
        row, previous, contact, setup, owner, target, previous_hp, strength, critical=True)
    ordinary_matches = ordinary[0] <= loss <= ordinary[1]
    critical_matches = critical is not None and critical[0] <= loss <= critical[1]
    outcome = critical_matches if ordinary_matches != critical_matches else None
    if contact_guard(row, previous, setup, target) in ("normal", "special"):
        outcome = False  # Guarded contacts suppress critical feedback after damage calculation.
    return dict(critical=outcome, damage_branches=dict(ordinary=ordinary, critical=critical))


def detached_contact(row, setup, prefix):
    owner, rule, template = (row[prefix + f"_{offset:x}"] for offset in (0x18, 0x2c, 0x30))
    phase = row[prefix + "_14"] >> 24
    age, lifetime = (signed16(row[prefix + "_278"] >> shift & 65535) for shift in (16, 0))
    if (row[prefix + "_10"] >> 24 != 5 or phase not in (1, 2) or age <= 0 or lifetime < 0
            or phase != (2 if lifetime and age > lifetime else 1)
            or lifetime and age > lifetime + 1
            or owner not in {actor["address"] for actor in setup["initial_battle"]["actors"]}):
        raise damage_envelope.UnsupportedDamage("standalone contact has invalid owner or lifecycle")
    matches = [record for record in setup.get("detached_contacts", {}).get("records", [])
               if (record["rule"], record["template"]) == (rule, template)]
    if len(matches) != 1:
        raise damage_envelope.UnsupportedDamage("standalone contact lacks exact authored rule/template binding")
    return dict(matches[0], owner=owner, nominal=row[prefix + "_5c"])


def queue_context(row, setup, prefix):
    """Join counted queue entries to live typed storage, never dereference stale entries."""
    entry = {name: row[prefix + "_" + name] for name in ("owner", "context", "projectile")}
    if entry["projectile"]:
        count = observation_counts(row)["projectile"]
        nodes, complete = tracked_list(row, "projectile", count)
        if not complete:
            raise damage_envelope.UnsupportedDamage("contact projectile list is incomplete")
        matches = [f"projectile_{index}" for index, address in nodes
                   if entry["projectile"] == address + 0x28
                   and entry["context"] == address + 0x168
                   and row[f"projectile_{index}_28"] == entry["owner"]
                   and row[f"projectile_{index}_14"] >> 24 in (1, 2)]
        if len(matches) != 1:
            raise damage_envelope.UnsupportedDamage("queue contact has no unique live projectile context")
        source = matches[0]
        entry.update(rule=row[source + "_16c"], template=row[source + "_170"],
                     nominal=row[source + "_19c"],
                     projectile_result=row[source + "_280"] & 255,
                     projectile_age=signed16(row[source + "_278"] >> 16))
        for name in ("cooldown_0", "cooldown_1", "last_target", "rule_timing"):
            if source + "_" + name in row:
                entry[name] = row[source + "_" + name]
    else:
        actors = [actor for actor in setup["initial_battle"]["actors"]
                  if actor["address"] == entry["owner"]]
        if len(actors) != 1:
            raise damage_envelope.UnsupportedDamage("attached contact owner is not observed")
        actor = actors[0]
        delta = entry["context"] - actor["address"] - 0x1184
        if delta < 0 or delta % 0x38 or delta // 0x38 >= ACTOR_CONTACT_CAPACITY:
            nodes, complete = tracked_list(row, "program", observation_counts(row)["program"])
            matches = [f"program_{index}" for index, address in nodes if entry["context"] == address + 0x28]
            if not complete or len(matches) != 1:
                raise damage_envelope.UnsupportedDamage("queue contact is not a bound actor context or live standalone contact")
            contact = detached_contact(row, setup, matches[0])
            if contact["owner"] != entry["owner"]:
                raise damage_envelope.UnsupportedDamage("standalone contact owner differs from queue")
            result = row.get(matches[0] + "_280")
            return dict(entry, **contact, detached_address=entry["context"] - 0x28,
                        detached_age=signed16(row[matches[0] + "_278"] >> 16),
                        detached_result=None if result is None else result >> 8 & 255)
        source = f"actor_{actor['slot']}_contact_{delta // 0x38}"
        if source + "_rule" not in row:
            raise damage_envelope.UnsupportedDamage("attached contact exceeds its bounded observer")
        entry.update({name: row[source + "_" + name] for name in ("rule", "template", "nominal")})
    entry.update(contact_fields(row, source))
    if not entry["rule"] or not entry["template"]:
        raise damage_envelope.UnsupportedDamage("contact lacks an initialized rule or template")
    return entry


def queued_contact(row, setup, owner, target, previous=None):
    """Bind this update's queue; equal consecutive nominal rolls remain valid."""
    if "contact_queue_counts" not in row:
        return None
    slot = setup["source_actor_map"][owner]["source_slot"]
    target_slot = setup["source_actor_map"][target]["source_slot"]
    if row.get(f"actor_{target_slot}_fresh_contact_word", 0) >> 24 != 1:
        return None
    victim = setup["initial_battle"]["actors"][target]["address"]
    if row.get(f"actor_{slot}_contact_target") != victim:
        return None
    owner_address = setup["initial_battle"]["actors"][owner]["address"]

    def entries(observed):
        result = []
        for side in range(2):
            count = (observed["contact_queue_counts"] >> (16 if side == 0 else 0)) & 65535
            capacity = sum(key.startswith(f"contact_queue_{side}_") and key.endswith("_owner")
                           for key in observed)
            if count > capacity:
                raise damage_envelope.UnsupportedDamage("collision queue exceeds its bounded observer")
            for index in range(count):
                prefix = f"contact_queue_{side}_{index}"
                if observed[prefix + "_owner"] != owner_address:
                    return None
                result.append(queue_context(observed, setup, prefix))
        return result

    current = entries(row)
    if current is None:
        return None
    candidates = {(entry["rule"], entry["template"], entry["projectile"]): entry for entry in current}
    if len(candidates) == 1:
        return next(iter(candidates.values()))
    # A fresh victim hit may repeat the same nominal damage and result latch.
    # If every other queued projectile still has its cleared collision result,
    # exactly one live context can own that hit; no changed-number assumption is needed.
    if candidates and all(entry["projectile"] and entry["projectile_result"] in (0, 1, 17)
                          for entry in candidates.values()):
        connected = [entry for entry in candidates.values() if entry["projectile_result"]]
        if len(connected) == 1:
            return connected[0]
    if previous is None or not candidates:
        return None
    prior_entries = entries(previous)
    if prior_entries is None:
        return None
    # A per-recipient cooldown reset proves a fresh projectile collision even
    # when its damage roll and collision latch equal their previous values.
    if row["battle_combat_tick_word"] == previous["battle_combat_tick_word"] + 1:
        recipient = (row[f"actor_{target_slot}_control_word"] >> 24) & 7
        cooldown_word, shift = f"cooldown_{recipient // 4}", 24 - recipient % 4 * 8
        reset = []
        for entry in candidates.values():
            if not entry["projectile"] or entry.get("last_target") != victim:
                continue
            duration = entry.get("rule_timing", 0) >> 24
            before = [old for old in prior_entries if all(old.get(field) == entry.get(field)
                for field in ("context", "owner", "rule", "template", "projectile"))]
            if (len(before) == 1 and cooldown_word in entry and cooldown_word in before[0]
                    and 0 < before[0]["projectile_age"] < entry["projectile_age"]
                    and duration > 1 and (entry[cooldown_word] >> shift) & 255 == duration
                    and (before[0][cooldown_word] >> shift) & 255 <= 1):
                reset.append(entry)
        if len(reset) == 1:
            return reset[0]
    # Several contexts may share owner/victim. A changed nominal result or a
    # newly allocated node's collision result must identify exactly one.
    fields = ("context", "owner", "rule", "template", "projectile")
    prior = {}
    for entry in prior_entries:
        key = tuple(entry[field] for field in fields)
        prior.setdefault(key, set()).add(entry["nominal"])
    changed = []
    unique = {tuple(entry[field] for field in fields): entry for entry in current}
    for key, entry in unique.items():
        values = prior.get(key, set())
        if not values and entry["projectile"]:
            nodes, complete = tracked_list(previous, "projectile", observation_counts(previous)["projectile"])
            matches = [f"projectile_{index}" for index, address in nodes
                       if entry["projectile"] == address + 0x28
                       and entry["context"] == address + 0x168
                       and previous[f"projectile_{index}_28"] == entry["owner"]
                       and previous[f"projectile_{index}_16c"] == entry["rule"]
                       and previous[f"projectile_{index}_170"] == entry["template"]
                       and previous[f"projectile_{index}_14"] >> 24 == 1
                       and 0 < signed16(previous[f"projectile_{index}_278"] >> 16)
                           <= entry["projectile_age"]]
            if (not complete or len(matches) != 1
                    or row["battle_combat_tick_word"] != previous["battle_combat_tick_word"] + 1):
                return None
            values = {previous[matches[0] + "_19c"]}
        if not values and "detached_address" in entry:
            nodes, complete = tracked_list(previous, "program", observation_counts(previous)["program"])
            if (not complete or entry["detached_address"] in {address for _, address in nodes}
                    or row["battle_combat_tick_word"] != previous["battle_combat_tick_word"] + 1
                    or entry["detached_age"] not in (1, 2)
                    or entry["detached_result"] not in (0, 1, 17)):
                return None
            # New detached contexts inherit an unspecified nominal value;
            # their collision result is explicitly cleared at allocation.
            if entry["detached_result"]:
                changed.append(entry)
            continue
        if not values and not entry["projectile"]:
            index = (entry["context"] - owner_address - 0x1184) // 0x38
            prefix = f"actor_{slot}_contact_{index}_"
            # Attached slots retain their nominal damage between queue visits,
            # including when the next authored contact replaces their template.
            nominal = previous.get(prefix + "nominal")
            if (0 <= index < ACTOR_CONTACT_CAPACITY and nominal is not None
                    and row["battle_combat_tick_word"] == previous["battle_combat_tick_word"] + 1):
                values = {nominal}
        if not key[0] or len(values) != 1:
            return None
        if entry["nominal"] not in values:
            changed.append(entry)
    return changed[0] if len(changed) == 1 else None


def knockdown_impact(row, previous, setup, owner, target, events, common):
    """Bind a consumed landing impulse, separately from a new attack contact."""
    if not (setup.get("knockdown_impacts") or setup.get("observer", {}).get("knockdown_impacts")):
        return None
    slot = setup["source_actor_map"][target]["source_slot"]
    prefix, body = f"actor_{slot}_", f"battle_actor_{slot}_"
    if not previous[prefix + "recoil_word"] & 1 or row[prefix + "recoil_word"] & 1:
        return None
    height = f32(row[body + "y_bits"])
    if (height is None or height > 0 or row[body + "action"] >> 24 != 14 or
            (row[body + "body_clip_word"] >> 8) & 255 != 8):
        return None
    controller = next((prefix + f"animation_{link}" for link in range(BODY_CONTROLLERS)
                       if row.get(prefix + f"animation_{link}_layer_word", 0) & 65535 == 6), None)
    if controller is None:
        raise ValueError("knockdown impact has no observed body controller")
    if row[controller + "_impact_state_word"] & 0x80000000:
        return None
    # Weak does not change this fraction of an already recorded hit.
    conditions = support_outcomes.conditions(row[prefix + "condition_10"],
                                            row[prefix + "condition_14"])
    if (setup["damage_context"][target]["blocked"]
            or any(condition != "weak" for condition in conditions)
            or any(row[prefix + field] for field in (
                "condition_38", "condition_3c", "raw_1d0", "raw_1d4"))):
        return None
    contact = next((event for event in reversed(events) if event.get("target") == target
                    and event["kind"] in ("hit", "unattributed_hp_loss", "knockdown_impact")), None)
    prior_damage = signed16(previous[prefix + "last_damage_word"] >> 16)
    if (contact is None or contact["kind"] != "hit" or contact["owner"] != owner or
            prior_damage <= 0 or prior_damage != -contact["hp_change"]):
        return None
    amount = prior_damage * 25 // 100
    remaining = max(1, previous[body + "hp"] - amount)
    last_damage = signed16(row[prefix + "last_damage_word"] >> 16)
    if row[prefix + "fresh_contact_word"] >> 24:
        # A later contact overwrites the landing damage latch in this update.
        # Its independently observed damage must account for the entire remainder.
        if not 0 < last_damage < remaining:
            return None
        remaining -= last_damage
    elif last_damage != amount:
        return None
    if amount <= 0 or row[body + "hp"] != remaining:
        return None
    started = next(event for event in events if event["kind"] == "started")
    return dict(common, kind="knockdown_impact", owner=owner, target=target, action=1,
                amount=amount, contact_clock=contact["combat_tick"] - started["combat_tick"],
                contact_presentation_clock=contact["presentation_tick"] - started["presentation_tick"])


def poison_pulse(row, previous, setup, owner, target, events, common):
    """Attribute periodic HP loss through its observed condition and feedback."""
    slot = setup["source_actor_map"][target]["source_slot"]
    prefix = f"actor_{slot}_"
    before = previous[prefix + "condition_14"] & 3
    after = row[prefix + "condition_14"] & 3
    if not before or before != after:
        return None
    condition = "poison_severe" if before & 2 else "poison_mild"
    applied = next((event for event in reversed(events)
                    if event["kind"] == "condition_changed" and event["target"] == target
                    and (condition in event["before"]) != (condition in event["after"])), None)
    if (applied is None or applied["owner"] != owner or condition not in applied["after"]
            or condition in applied["before"]):
        return None
    emissions = [event for event in events if event["kind"] == "program_started"
                 and event["owner"] == target and event["bank"] == setup.get("common_effect_bank")
                 and event["program"] == 44 and event["combat_tick"] == common["combat_tick"]
                 and event["presentation_tick"] == common["presentation_tick"]]
    if len(emissions) != 1:
        return None
    hp = previous[f"battle_actor_{slot}_hp"]
    maximum = setup["actors"][target]["max_hp"]
    nominal = max(1, maximum * (2 if before & 2 else 1) // 100)
    amount = min(nominal, hp - 1)
    if amount <= 0 or hp - row[f"battle_actor_{slot}_hp"] < amount:
        return None
    return dict(common, kind="poison_pulse", owner=owner, target=target, action=1,
                condition=condition, amount=amount)


def enemy_pending_entry(rows, setup, expected, onset):
    """Bind a witnessed selection or approach before its action tracks exist."""
    if "selected" not in expected:
        return None
    owner = setup["owner"]
    slot = setup["source_actor_map"][owner]["source_slot"]
    prefix = f"battle_actor_{slot}_"
    first = onset
    while first and (rows[first - 1][prefix + "action"] in (4 << 24, 5 << 24, 10 << 24) and
                     rows[first - 1][f"actor_{slot}_selected"] == expected["selected"]):
        first -= 1
    if first == onset:
        return None
    recipient = setup.get("recipient_input", {})
    host = recipient.get("target_slot")
    prerequisites = setup.get("assistance", {}).get("preparation", {}).get("prerequisites", {})
    if recipient.get("controller") == "owner":
        if (recipient.get("actor") != owner or recipient.get("source_slot") != slot
                or recipient.get("mode") != "stick" or recipient.get("port") != 0
                or not prerequisites.get("owner_control") or not prerequisites.get("origin_bounds")
                or first == 0 or not single_action_fixture.prerequisite_ready(rows[first - 1], slot, prerequisites)):
            raise ValueError("owner movement lacks its routed input and observed origin prerequisite")
        press, release = recipient["press_after_updates"], recipient["release_after_updates"]
        walking = [row for row in rows[:first]
                   if press <= row["battle_combat_tick_word"] - setup["initial_battle"]["combat_tick"] < release]
        if (len(walking) < 2 or any(row[prefix + "action"] >> 24 not in (3, 4) for row in walking)
                or any((row[f"actor_{slot}_control_word"] >> 24) & 0x38
                       or (row[f"actor_{slot}_control_word"] >> 16) & 0xc0 for row in walking)
                or all(row[prefix + "x_bits"] == walking[0][prefix + "x_bits"]
                       and row[prefix + "z_bits"] == walking[0][prefix + "z_bits"] for row in walking)):
            raise ValueError("owner movement lacks ordinary manual walking observations")
        return rows[first]["battle_combat_tick_word"] - setup["initial_battle"]["combat_tick"]
    if deployment := prerequisites.get("defense_deployment"):
        core = deployment["core"]
        initial_core = next(actor for actor in setup["initial_battle"]["actors"] if actor["slot"] == core)
        if (setup.get("defense_deployed") is not False or deployment["initial_phase"] != 0
                or initial_core["hp"] * 100 > initial_core["max_hp"] * deployment["hp_percent"]
                or not any(observed_body(row, core, setup["body_resources"][str(core)])[0] == 32
                           for row in rows[:first])):
            raise ValueError("attached action lacks its observed core deployment prelude")
        # The ordinary encounter controller moves the attached arm throughout
        # deployment. Replay that controller, then the witnessed selection;
        # no resulting arm position or pose is restored during combat.
        return rows[first]["battle_combat_tick_word"] - setup["initial_battle"]["combat_tick"]
    if (recipient.get("mode") == "attack" and host is not None
            and prerequisites.get("release_host") == host):
        # Replay the physical host hit and complete recovery before selection;
        # do not restore the carried actor's resulting position or facing.
        releases = [index for index in range(1, first)
                    if rows[index - 1][f"actor_{slot}_raw_280"] & 0x80000
                    and not rows[index][f"actor_{slot}_raw_280"] & 0x80000]
        if len(releases) != 1:
            raise ValueError("recipient attack lacks one observed participant release")
        index = releases[0]
        before, released = rows[index - 1], rows[index]
        if (before[f"battle_actor_{host}_hp"] <= released[f"battle_actor_{host}_hp"]
                or released[f"battle_actor_{host}_action"] >> 24 not in (9, 14, 18)
                or released[prefix + "action"] >> 24 != 8
                or rows[first - 1][prefix + "action"] >> 24 != 3
                or not any(row[prefix + "action"] >> 24 == 2 for row in rows[index:first])):
            raise ValueError("participant release lacks observed host damage and recovery")
        return rows[first]["battle_combat_tick_word"] - setup["initial_battle"]["combat_tick"]
    ready = setup["actors"][owner]
    # Entry initialization may place and orient actors before combat starts.
    # Later motion is checked against that last observed noncombat boundary.
    entry = [row for row in rows[:first]
             if row["battle_combat_tick_word"] == setup["initial_battle"]["combat_tick"]]
    heading = f32(entry[-1][prefix + "heading_bits"]) if entry else ready["heading"]
    combat_rows = [(index, row) for index, row in enumerate(rows[:onset + 1])
                   if row["battle_combat_tick_word"] > setup["initial_battle"]["combat_tick"]]
    if (combat_rows and combat_rows[0][0] < first
            and combat_rows[0][1]["battle_combat_tick_word"] == setup["initial_battle"]["combat_tick"] + 1
            and f32(combat_rows[0][1][prefix + "heading_bits"]) == ready["heading"]):
        # Some entry poses temporarily face a fixed direction, then restore
        # the exact ready facing at the first ordinary combat visit.
        heading = ready["heading"]
    headings = [f32(row[prefix + "heading_bits"]) for _, row in combat_rows]
    hovering = all(row.get(f"actor_{slot}_condition_5c", 0) & 3 == 3
                   and row.get(f"actor_{slot}_raw_280", 0) & 0xe00000
                   for row in rows[first:onset + 1])
    approaching = any(row[prefix + "action"] == 4 << 24 for row in rows[first:onset])
    if all(value == heading for value in headings) and not hovering and not approaching:
        return None
    if any(f32(row[prefix + "heading_bits"]) != heading
           for index, row in combat_rows if index < first):
        raise ValueError("enemy facing changed before its observed pending entry")
    # Settled floating actors bob during selection, then hold their height once
    # the action binds. Replay that observed input phase, without restoring Y.
    phase = prerequisites.get("health_phase")
    height = prerequisites.get("hover_height")
    if phase is not None and ready["hp"] * 100 <= ready["max_hp"] * phase["hp_percent"]:
        height = phase["hover_height"]
    # Noncombat placement is replayed through its observed entry history.
    # Use its final position as the origin of subsequent ordinary movement.
    origin = ([f32(entry[-1][prefix + axis + "_bits"]) for axis in "xyz"]
              if entry else ready["position"])
    rising = False
    if height is not None:
        heights = [f32(row[prefix + "y_bits"]) for index, row in combat_rows if index < first]
        rising = (bool(heights) and all(value is not None and min(origin[1], height) <= value <= max(origin[1], height)
                                      for value in heights)
                  and any(row.get(f"actor_{slot}_condition_5c", 0) & 0x100001 == 0x100001
                          for index, row in combat_rows if index < first))
    axes = (0, 2) if hovering or rising else (0, 1, 2)
    # A contiguous selected walk is itself the observed input. Native replay
    # starts that approach at its first update and still checks bound admission.
    stationary = [row for index, row in combat_rows if not approaching or index < first]
    if any(any(f32(row[prefix + "xyz"[axis] + "_bits"]) != origin[axis]
               for axis in axes) for row in stationary):
        raise ValueError("enemy pending entry has an unsupported movement prelude")
    delay = rows[first]["battle_combat_tick_word"] - setup["initial_battle"]["combat_tick"]
    if delay <= 0:
        raise ValueError("enemy pending entry does not follow its ready snapshot")
    return delay


def enemy_approach_targets(rows, setup, delay, onset):
    """Retain target choices made before a hostile action binds."""
    owner = setup["owner"]
    if setup["actors"][setup["target"]]["side"] == setup["actors"][owner]["side"]:
        return []
    slot = setup["source_actor_map"][owner]["source_slot"]
    initial = setup["initial_battle"]["combat_tick"]
    selected = [row for row in rows[:onset]
                if initial + delay <= row["battle_combat_tick_word"]]
    if not selected or selected[0]["battle_combat_tick_word"] != initial + delay:
        raise ValueError("enemy approach target lacks its first selected update")
    addresses = {actor["address"]: index
                 for index, actor in enumerate(setup["initial_battle"]["actors"])}
    changes = []
    for row in selected:
        target = addresses.get(row[f"battle_actor_{slot}_target"])
        if target is None or setup["actors"][target]["side"] == setup["actors"][owner]["side"]:
            raise ValueError("enemy approach target is not an observed opponent")
        if not changes or changes[-1]["target"] != target:
            changes.append(dict(after_updates=row["battle_combat_tick_word"] - initial, target=target))
    if changes[-1]["target"] != setup["target"]:
        raise ValueError("enemy target changed at bound admission without an observed approach choice")
    return changes


def bind_action_target(row, setup, events=(), *, allied_spell=False):
    """Use the admitted action's observed selection, before any contact outcome."""
    slot = setup["source_actor_map"][setup["owner"]]["source_slot"]
    support_item = any("use_item" in event and not event["use_item"]["party"]
                       for _, event in events)
    support_target = support_item or allied_spell
    source_field = f"actor_{slot}_support_target" if support_target else f"battle_actor_{slot}_target"
    pointer = row[source_field]
    recipient = next((index for index, actor in enumerate(setup["initial_battle"]["actors"])
                      if actor["address"] == pointer), None)
    if recipient is None:
        raise ValueError("admitted action target is not an observed actor")
    if support_target and setup["actors"][recipient]["side"] != setup["actors"][setup["owner"]]["side"]:
        raise ValueError("support recipient is not an allied actor")
    setup.setdefault("requested_target", setup["target"])
    setup["action_target_observation"] = dict(recipient=recipient, pointer=pointer,
        combat_tick=row["battle_combat_tick_word"],
        presentation_tick=row["battle_presentation_tick_word"])
    if support_target:
        setup["action_target_observation"]["source_field"] = source_field
    setup["target"] = recipient
    return recipient


def normalize(case, rows, setup, banks, expected, images):
    """Never promote appearance, action start or arbitrary HP loss to a hit."""
    owner, target = setup["owner"], setup["target"]
    execution = case.get("definition", {}).get("execution")
    attack = execution.get("attack", {}) if isinstance(execution, dict) else {}
    departure = case.get("context") == "departure"
    if departure and execution != "leave_arena":
        raise ValueError("departure context requires an authored leave-arena action")
    departure_hp = None
    scripted_defeat = case.get("self_defeat")
    if scripted_defeat is not None:
        setup["self_defeat"] = scripted_defeat
    if case.get("self_stun"):
        setup["self_stun"] = True
    setup.pop("effect_baseline", None)
    setup.pop("chance_effects", None)
    setup.pop("immediate_end_programs", None)
    setup.pop("pre_admission_actors", None)
    setup.pop("effect_model_parents", None)
    source_owner = setup["source_actor_map"][owner]["source_slot"]
    addresses = {a["address"]: i for i, a in enumerate(setup["initial_battle"]["actors"])}
    slots = [a["slot"] for a in setup["initial_battle"]["actors"]]
    frames, events, missing = [], [], set()
    for key in ("stored_scene_observation_error", "ambient_phase_error"):
        if setup.get(key):
            missing.add(setup[key])
    start_tick, selected_tick, returned, action = None, None, False, 0
    other_selection = None
    ordinal, previous_nodes, serial = {}, {}, Counter()
    previous_age, particle_identity, observed_particle_births, projectile_births = {}, {}, set(), set()
    previous_particle_signature = {}
    previous_result, live_programs_old, program_serial, declaration_seen = {}, {}, 0, set()
    model_counts, model_ordinals, semantics = Counter(), {}, []
    unobserved_model_births = set()
    diagnostics = {"Body bone0 world is the last completed display publication with an unobserved timestamp; body bone geometry is outside this test. Actor placement, bound clip/frame and deterministic effect transforms remain observed separately."}
    recovery_seen, program_history = False, {}
    baseline, retired = [], False
    frozen_updates = Counter()
    scenery_frozen, scenery_pause = 0, None
    support = setup.get("support")
    recipient_input = setup.get("recipient_input")
    if recipient_input is not None:
        # Movie polls are sampled after the battle update. The corresponding
        # input is consumed by the following update, not the sampled one.
        recipient_input["poll_to_update_offset"] = 2
    recipient_guard_seen = False
    target_attack_checked = False
    self_defeated, handler_started = False, False
    counts = observation_counts(rows[0]) if rows else {}
    particle_addresses = setup.get("observer", {}).get("particle_addresses")
    spell_origins = {
        entry["owner"] + 0x16cc
        for entry in setup.get("stored_scenes", {}).get("registrations", [])
        if entry["native"] == 217 and entry["kind"] == "spell"
        and entry["owner"] == setup["initial_battle"]["actors"][owner]["address"]
    }

    def follow(pointer, projectiles_by_address):
        if pointer is None:
            return {"kind": "unobserved"}
        if pointer == 0:
            return None
        if pointer in spell_origins:
            return dict(kind="spell", action=1)
        for actor in setup["initial_battle"]["actors"]:
            for delta, kind in ((0x18c0, "actor"), (0x195c, "center")):
                if pointer == actor["address"] + delta:
                    return dict(kind=kind, actor=addresses[actor["address"]])
        for address, pid in projectiles_by_address.items():
            if pointer == address + 0x40:
                return dict(kind="projectile", projectile=pid)
        return {"kind": "unresolved"}


    def identity(label, address, present, age, signature=None):
        key = (label, address)
        changed = signature is not None and previous_particle_signature.get(address, signature) != signature
        if address not in previous_nodes.get(label, set()) or age < previous_age.get(key, age) or changed:
            serial[label] += 1
            ordinal[key] = serial[label]
        if signature is not None:
            previous_particle_signature[address] = signature
        previous_age[key] = age
        present.add(address)
        return ordinal[key]

    for index, row in enumerate(rows):
        if row.get("battle_pool") != setup["initial_battle"].get("pool", row.get("battle_pool")):
            raise ValueError("initialized battle pool changed during the compact observation")
        combat, presentation = row["battle_combat_tick_word"], row["battle_presentation_tick_word"]
        # Scene transitions suppress the stage update on both boundary samples.
        if index and ((row.get("battle_pause_flags_word", 0) | rows[index - 1].get("battle_pause_flags_word", 0)) >> 16) & 255:
            scenery_frozen += 1
            if scenery_pause is None:
                scenery_pause = presentation
        # Stored transitions freeze actor-local passive counters. The caster
        # and actors visited later can miss different numbers of updates.
        if index and (row.get("battle_pause_flags_word", 0) | rows[index - 1].get("battle_pause_flags_word", 0)) >> 16:
            for actor_id, slot in enumerate(slots):
                clock = f"battle_actor_{slot}_cast_clock_word"
                if clock in row and clock in rows[index - 1] and row[clock] >> 16 == rows[index - 1][clock] >> 16:
                    frozen_updates[actor_id] += 1
        # Later pauses cannot change an already completed physical input interval.
        if (recipient_input is not None and recipient_input.get("repeat_every_updates")
                and combat == setup["initial_battle"]["combat_tick"]
                and row["battle_actor_0_action"] >> 24 in (5, 10)):
            missing.add("repeated recipient input admitted an attack before combat")
        if (recipient_input is not None and combat > setup["initial_battle"]["combat_tick"]
                and presentation - setup["initial_battle"]["presentation_tick"]
                <= recipient_input["release_after_polls"] // 2 + 1):
            entry_vis = (presentation - setup["initial_battle"]["presentation_tick"]
                         - (combat - setup["initial_battle"]["combat_tick"]))
            for polls, updates in (("press_after_polls", "press_after_updates"),
                                   ("down_after_polls", "down_after_updates"),
                                   ("release_after_polls", "release_after_updates")):
                count = recipient_input.get(polls)
                delay = None if count is None else count // 2 - entry_vis + 2
                if (polls == "press_after_polls" and delay is not None and delay <= 0
                        and (period := recipient_input.get("repeat_every_updates")) is not None):
                    # Keep the original poll interval as evidence. Entry blocks
                    # attacks; replay begins with its first post-entry press.
                    delay += ((1 - delay + period - 1) // period) * period
                if delay is not None and delay <= 0:
                    missing.add("recipient input was scheduled before combat")
                if updates in recipient_input and recipient_input[updates] != delay:
                    missing.add("recipient input interval crossed an unobserved combat pause")
                recipient_input[updates] = delay
        if recipient_input is not None and combat > setup["initial_battle"]["combat_tick"]:
            elapsed = presentation - setup["initial_battle"]["presentation_tick"]
            entry_vis = elapsed - (combat - setup["initial_battle"]["combat_tick"])
            for interval in recipient_input.get("stick_intervals", []):
                if elapsed > interval["release_after_polls"] // 2 + 1:
                    continue
                for edge in ("press", "release"):
                    key = f"{edge}_after_updates"
                    delay = interval[f"{edge}_after_polls"] // 2 - entry_vis + 2
                    if delay <= 0 or key in interval and interval[key] != delay:
                        missing.add("recipient movement interval crossed an unobserved combat pause")
                    interval[key] = delay
        common = dict(presentation_tick=presentation, combat_tick=combat)
        actors, body_models, secondary_models = [], [], []
        for actor_id, slot in enumerate(slots):
            base = f"battle_actor_{slot}"
            actors.append(dict(actor=actor_id, side=int(slot >= 4), hp=row[base + "_hp"],
                tp=row[base + "_tp_word"] & 65535,
                position=[f32(row[base + f"_{axis}_bits"]) for axis in "xyz"],
                heading=f32(row[base + "_heading_bits"])))
            if scripted_defeat is not None and actor_id == owner:
                actors[-1]["defeated"] = ((row[f"actor_{slot}_control_word"] >> 16) & 7) == 2
            if departure and actor_id == owner:
                actors[-1]["departed"] = (row[base + "_action"] >> 24 == 18
                    and row[base + "_hp"] > 0 and not row[f"actor_{slot}_control_word"] & 0x10000)
            if case.get("context") == "capture":
                try:
                    address = setup["initial_battle"]["actors"][actor_id]["address"]
                    captors = [i for i, source_slot in enumerate(slots)
                               if row[f"actor_{source_slot}_captured_recipient"] == address]
                    attachment = row[f"actor_{slot}_capture_attachment"]
                    if len(captors) > 1 or bool(captors) != bool(attachment):
                        raise ValueError("capture links are not reciprocal")
                    actors[-1]["capture"] = ([captors[0], bool(row[f"actor_{slot}_raw_280"] & 0x04000000)]
                                             if captors else None)
                except (KeyError, ValueError) as error:
                    missing.add(f"capture state unavailable: {error}")
            if recipient_input is not None and actor_id == recipient_input["actor"]:
                actors[-1]["guard"] = guard_kind(row, slot)
                recipient_guard_seen |= actors[-1]["guard"] == recipient_input["mode"]
            if support is not None:
                try:
                    actors[-1]["support"] = support_outcomes.snapshot(row, slot)
                    if support.get("time_stop", False):
                        if setup["actors"][actor_id].get("time_stop") != 0:
                            raise ValueError("time-stop fixture lacks an observed zero initial state")
                        actors[-1]["time_stop"] = support_outcomes.time_stop(row, slot)
                except (KeyError, ValueError) as error:
                    missing.add(f"support observation unavailable: {error}")
            active_overlimit = ((row[f"actor_{slot}_control_word"] >> 16) & 7) == 4
            if "overlimit_remaining" in setup["actors"][actor_id]:
                actors[-1]["overlimit_remaining"] = (row[f"actor_{slot}_overlimit_gauge_word"] >> 16) if active_overlimit else 0
            elif active_overlimit:
                missing.add("unrequested OverLimit altered the neutral execution fixture")
            clip, frame = observed_body(row, slot, setup.get("body_resources", {}).get(str(slot), {}),
                                        setup.get("body_initial_overrides", {}).get(str(slot)))
            if frame is None and start_tick is not None:
                missing.add("requested body clip lacks a matched current/pending animation controller")
            body_models.append(dict(actor=actor_id, clip=clip, frame=frame))
            if f"actor_{slot}_root_0" in row:
                body_models[-1]["world"] = [f32(row[f"actor_{slot}_root_{word}"]) for word in range(12)]
        for binding in setup.get("secondary_bodies", []):
            try:
                secondary_models.append(secondary_bodies.observe(row, binding))
            except (KeyError, ValueError) as error:
                missing.add(f"secondary body observation unavailable: {error}")
        observed = {key: row[f"actor_{source_owner}_{key}"] & (65535 if key == "native" else 0xffffffff) for key in expected}
        action_word = row[f"battle_actor_{source_owner}_action"]
        if (other_selection is None and start_tick is None and "selected" in expected
                and action_word >> 24 in (4, 5, 10) and observed["selected"] != expected["selected"]):
            target_slot = slots[setup["target"]]
            other_selection = dict(combat_tick=combat, presentation_tick=presentation,
                owner_action=action_word >> 24, owner_phase=action_word >> 16 & 255,
                selected=observed["selected"], expected=expected["selected"],
                target_distance=f32(row.get(f"actor_{source_owner}_target_distance_bits", 0)),
                target_action=row[f"battle_actor_{target_slot}_action"] >> 24,
                target_phase=row[f"actor_{target_slot}_target_state_word"] >> 24)
        if (start_tick is None and "selected" in expected and action_word >> 24 == 5 and
                ((action_word >> 16) & 255) != 0 and
                observed["selected"] != expected["selected"]):
            missing.add("owner admitted another action before the requested row; single-action fixture is incomplete")
        if returned and (action_word >> 24 == 5 or
                         ("selected" in expected and observed["selected"] != expected["selected"]
                          and action_word >> 24 not in (2, 3))):
            missing.add("owner admitted another action after returning idle; single-action fixture is incomplete")
        if returned and action_word >> 24 == 4:
            missing.add("owner began another approach after returning idle; single-action fixture is incomplete")
        selected = action_word >> 24 == (20 if departure else 5) and all(
            observed[k] == value for k, value in expected.items())
        input_selected = selected or ("character" in case and action_word >> 24 == 5
            and setup["input"].get("selection_native") is not None
            and observed.get("native") == setup["input"]["selection_native"])
        if input_selected and selected_tick is None:
            selected_tick = combat
        pending_selected = ("selected" in expected and action_word >> 24 in (4, 10)
                            and observed["selected"] == expected["selected"])
        if (input_selected or pending_selected) and not target_attack_checked:
            target_attack_checked = True
            if "target_attacking" in case.get("definition", {}).get("requirements", {}).get("predicates", []):
                # This is a selection prerequisite. A following approach may
                # finish after the opponent's attack has already ended.
                target_slot = slots[setup["target"]]
                target_action = row[f"battle_actor_{target_slot}_action"] >> 24
                target_phase = row[f"actor_{target_slot}_target_state_word"] >> 24
                setup["target_attack_admission"] = dict(combat_tick=combat,
                    actor=setup["target"], action=target_action, phase=target_phase)
                if target_action != 5 or target_phase != 3:
                    missing.add("required target attack was not observed at row selection")
        # Phase zero is the pending selection. The ordinary action dispatcher
        # installs the bound tracks/cast and commits payment in phase one.
        active = selected and (departure or ((action_word >> 16) & 255) != 0)
        started_now = active and start_tick is None
        if started_now:
            if "selected" in expected:
                try:
                    target = bind_action_target(row, setup, attack.get("events", []),
                        allied_spell=isinstance(execution, dict) and "spell" in execution
                            and case.get("definition", {}).get("target_allies", False))
                except (KeyError, ValueError) as error:
                    missing.add(str(error))
            baseline = recurring_effects(semantics, banks)
            if baseline:
                setup["effect_baseline"] = baseline
            action = 1
            events.append(dict(common, kind="started", owner=owner, action=action))
            if departure:
                departure_hp = actors[owner]["hp"]
            start_tick = combat
        primary = row.get(f"battle_actor_{source_owner}_primary_phase_age_word", 0xc00000)
        secondary = row.get(f"battle_actor_{source_owner}_secondary_phase_age_word", 0xc00000)
        handler_started |= start_tick is not None and primary >> 24 == 1
        if (scripted_defeat is not None and start_tick is not None and not self_defeated
                and action_word >> 24 == 18 and actors[owner]["defeated"]):
            self_defeated = True
            semantics.append(dict(common, kind="self_defeated", owner=owner, action=1))
        defeated_terminal = self_defeated and (
            (scripted_defeat is not None and actors[owner]["defeated"] and
             (scripted_defeat["retain_body"] or row[f"actor_{source_owner}_raw_280"] & 0x04000000))
            or (handler_started and primary >> 24 == 2 and not ((primary | secondary) >> 16) & 0xc0))
        if defeated_terminal:
            returned = True
        departed_terminal = (departure and start_tick is not None and actors[owner]["departed"]
            and departure_hp > 0 and actors[owner]["hp"] == departure_hp
            and all(observed[k] == value for k, value in expected.items()))
        if departure and start_tick is not None and (departure_hp <= 0 or
                actors[owner]["hp"] != departure_hp or
                any(observed[k] != value for k, value in expected.items())):
            missing.add("living departure lost its health or selected-row binding")
        if departed_terminal and not returned:
            returned = True
            semantics.append(dict(common, kind="departed", owner=owner, action=1))
        if not departure and start_tick is not None and row[f"battle_actor_{source_owner}_action"] >> 24 == 3 and not returned:
            returned = True
            events.append(dict(common, kind="completed", action=action))
            semantics.append(dict(common, kind="returned", owner=owner))
        self_stun_exit = (case.get("self_stun") and index > 0 and action_word >> 24 == 2
                         and rows[index - 1][f"battle_actor_{source_owner}_action"] >> 24 == 15)
        if start_tick is not None and not recovery_seen and (action_word >> 24 == 8 or self_stun_exit):
            recovery_seen = True
            semantics.append(dict(common, kind="recovery_started", owner=owner, action=1))
        if index and start_tick is not None:
            spent = (rows[index - 1][f"battle_actor_{source_owner}_tp_word"] & 65535) - actors[owner]["tp"]
            if spent > 0:
                semantics.append(dict(common, kind="payment", owner=owner, amount=spent))
        if start_tick is not None and (row["battle_menu_flags_word"] & MENU_ACTIVE or
                (row["battle_pause_flags_word"] >> 16 and not owned_stored_pause(row, setup))):
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
        alternate_nodes, complete_alternate = tracked_list(row, "alternate", counts["alternate"])
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
                if row[prefix + "_10"] >> 24 == 5:
                    try:
                        detached_contact(row, setup, prefix)
                    except (damage_envelope.UnsupportedDamage, KeyError) as error:
                        missing.add(f"standalone contact observation unavailable: {error}")
                    continue
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
            origin = tuple(row[prefix + f"_{at:x}"] for at in (0x48,0x4c,0x50))
            attachment = row.get(prefix + "_44")
            lifecycle_word = row.get(prefix + "_14")
            lifecycle = None if lifecycle_word is None else lifecycle_word >> 24
            prior = live_programs_old.get(address)
            # Detached origins are fixed. Relocation at age zero proves that
            # the allocator reused a node between two observed updates.
            relocated = (prior is not None and age == prior["age"] == 0
                         and attachment == prior["attachment"] == 0 and origin != prior["origin"])
            # A terminal node can be freed and reinitialized with the same
            # binding and age before the next observation.
            reinitialized = prior is not None and prior["lifecycle"] == 2 and lifecycle == 1
            if prior is None or prior["signature"] != signature or age < prior["age"] or relocated or reinitialized:
                program_serial += 1
                pid = program_serial
                event = dict(common, kind="program_started", id=pid, owner=addresses.get(signature[2]),
                             bank=bank["source_sha256"], program=bank["roots"].index(root - base),
                             observed_age=age, observed_at_entry=index == 0,
                             frozen_updates=frozen_updates.get(addresses.get(signature[2]), 0))
                semantics.append(event)
                program_history[pid] = event
            else:
                pid = prior["id"]
            item = dict(id=pid, owner=addresses.get(signature[2]), bank=bank["source_sha256"],
                        program=bank["roots"].index(root - base), age=age,
                        origin=[f32(value) for value in origin],
                        attachment=follow(attachment, projectile_addresses))
            programs.append(item)
            program_now[address] = dict(id=pid, signature=signature, age=age,
                                        origin=origin, attachment=attachment, lifecycle=lifecycle)
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
        particle_nodes, complete_particles = tracked_list(row, "particle", counts["particle"], particle_addresses)
        if not complete_particles:
            diagnostics.add("cosmetic particle linked list exceeds bounded observer")
            missing.add("effect particle list is incomplete; timed declaration activations may be unobserved")
        particle_records = [(f"particle_{i}",address) for i,address in particle_nodes]
        particle_records += [(f"alternate_{i}",address) for i,address in alternate_nodes if row[f"alternate_{i}_10"] >> 24 == 1]
        complete_particles &= complete_alternate
        present, effect_models, particle_ages = set(), [], []
        model_parent_observations, model_parent_transitions = [], []
        fixed_program_bindings = {
            address: tuple(row[f"particle_{i}_{at:x}"] for at in (0x60, 0x6c, 0x2c))
            for i, address in enumerate(particle_addresses or []) if row[f"particle_{i}_10"] >> 24 == 0
        }
        for prefix, address in particle_records:
            get = lambda offset: row[f"{prefix}_{offset:x}"]
            if get(0x10) >> 24 != 1:
                continue
            copied = b"".join(struct.pack(">I", get(at)) for at in range(0x28, 0x40, 4))
            initialized = particle_readiness(get(0x10), copied)
            particle_age = signed16(get(0x278) >> 16) if initialized else 0
            # Do not let stale allocator age create a second generation when
            # this same copied declaration initializes on the next update.
            parent = get(0x184)
            # An allocation can be reused between samples without an age drop.
            # Declaration type, bank slot and owning scheduler stay fixed for
            # a live particle, including its pending initialization sample.
            pid = identity("particle", address, present, particle_age,
                           (copied[0], copied[2], parent))
            embedded = next((a for a in setup["initial_battle"]["actors"]
                             if a["address"] + 0x15b4 == parent), None)
            if particle_addresses is not None or (prefix.startswith("alternate_") and "object_lists" not in row):
                # Pool and alternate entries share storage with other node types.
                # Join typed words instead of following an unchecked union pointer.
                binding = program_now.get(parent - 0x28)
                signature = fixed_program_bindings.get(parent - 0x28,
                    binding["signature"] if binding else (0, 0, 0))
                parent_live = binding is not None and binding["signature"] == signature
                parent_live |= retired_parent_birth(parent, signature, particle_age, previous_program_bindings)
                if embedded:
                    slot = embedded["slot"]
                    signature = (row[f"actor_{slot}_effect_bank"],
                                 row[f"actor_{slot}_effect_program"], embedded["address"])
                    parent_live = all(signature)
            else:
                signature = tuple(row[prefix + "_parent_" + field] for field in ("bank", "program", "owner"))
                parent_live = parent - 0x28 in live_programs
                parent_live |= retired_parent_birth(parent, signature, particle_age, previous_program_bindings)
                if embedded:
                    slot = embedded["slot"]
                    parent_live = bool(row[f"actor_{slot}_effect_program"] and
                        (row[f"actor_{slot}_effect_bank"], row[f"actor_{slot}_effect_program"]) == signature[:2])
            completed_birth = effect_entry.completed_condition_birth(
                setup, index, presentation, address, parent, copied, particle_age)
            if completed_birth is not None:
                # The scheduler slot may already be free or reused. Its exact
                # ready birth binds this particle independently of that slot.
                signature, parent_live = completed_birth, True
            base, root, parent_owner = signature
            bank = banks.get(base)
            owner_bone = ((get(0xb4) >> 24) if initialized and get(0x3c) & 0x40000000
                          and prefix + "_b4" in row else None)
            model_attachment = ((get(0xb4) >> 24, (get(0xb4) >> 16) & 255)
                                if initialized and copied[0] == 3 and get(0x3c) & 0x20000
                                and prefix + "_b4" in row else None)
            matches = declaration_matches(bank, copied, root - base,
                model_binding=row.get(prefix + "_fc"), initialized=initialized, age=particle_age,
                owner_bone=owner_bone, model_attachment=model_attachment,
                angles=[f32(get(at)) for at in (0x80, 0x84, 0x88)]
                    if all(prefix + "_" + format(at, "x") in row for at in (0x80, 0x84, 0x88)) else None
                ) if bank and pid not in particle_identity else []
            if len(matches) > 1 and initialized and parent_live:
                birth_tick = observed_birth_tick(row, rows[index - 1] if index else None,
                    prefix, signature, setup, previous_nodes.get("particle", set()),
                    program_now, previous_program_bindings)
                matches = declaration_matches(bank, copied, root - base,
                    model_binding=row.get(prefix + "_fc"), initialized=True,
                    age=particle_age, birth_tick=birth_tick, owner_bone=owner_bone,
                    model_attachment=model_attachment,
                    size=[f32(get(at)) for at in (0xd8, 0xdc, 0xe0)]
                        if setup.get("observer", {}).get("particle_transforms", True) else None,
                    offset=[f32(get(at)) for at in (0x5c, 0x60, 0x64)]
                        if prefix + "_180" in row else None,
                    heading=f32(get(0x180)) if prefix + "_180" in row else None)
            member = matches[0] if len(matches) == 1 else None
            if pid not in particle_identity and bank is not None and member is not None and parent_live:
                if root - base in bank["roots"]:
                    particle_identity[pid] = (bank, member, addresses.get(parent_owner))
            if pid not in particle_identity:
                controlled = stun_particle_identity(row, prefix, copied, setup, banks, initialized=initialized)
                if controlled is not None:
                    particle_identity[pid] = controlled
            bound = particle_identity.get(pid)
            if bound is None:
                diagnostics.add("sampled particle birth identity incomplete")
                if start_tick is not None and initialized and particle_age <= 1:
                    missing.add("initialized effect birth lacks exact bank/declaration/owner identity")
            else:
                bank, member, particle_owner = bound
            metadata = bank["metadata"]["actors"][member] if bound else {}
            # Allocation can precede initialization by an update. Compare the
            # first initialized observation, as the native particle publisher does.
            if bound and initialized:
                activation = (particle_owner, bank["source_sha256"], member, combat, presentation)
                if pid not in observed_particle_births and particle_age <= 1 and activation not in declaration_seen:
                    declaration_seen.add(activation)
                    semantics.append(dict(common, kind="declaration_started", owner=particle_owner,
                                          bank=bank["source_sha256"], member=member))
                elif pid not in observed_particle_births and particle_age > 1:
                    diagnostics.add("some declaration first samples occur after birth")
                observed_particle_births.add(pid)
            transforms = setup.get("observer", {}).get("particle_transforms", True)
            origin = [f32(get(at)) for at in (0x170, 0x174, 0x178)] if initialized and transforms else None
            offset = [f32(get(at)) for at in (0x5c, 0x60, 0x64)] if initialized and transforms else None
            model = None
            if initialized and metadata.get("kind") == "model_particle":
                binding = metadata.get("binding", {})
                model = dict(clip=binding.get("animation"), frame=None, world=None)
                try:
                    model["clip"], model["frame"] = observed_effect_animation(get, binding)
                except KeyError:
                    missing.add("effect model controller was not observed")
                except ValueError as error:
                    missing.add(str(error))
                if transforms:
                    parent_transform = None
                    if get(0x3c) & 0x20000:
                        observed_parent = effect_model_joints.observed(row, prefix, bank["source_sha256"], binding,
                            setup.get("observer", {}).get("model_joints", []),
                            program_records + particle_records, setup["initial_battle"]["actors"], f32, relation=True)
                        if observed_parent is not None:
                            parent_transform = observed_parent["world"]
                            model_parent_observations.append((pid, observed_parent))
                    elif get(0x3c) & 0x40000000 and particle_owner is not None:
                        bone = get(0xb4) >> 24
                        parent_prefix = f"actor_{slots[particle_owner]}_bone_{bone}_"
                        if all(parent_prefix + str(word) in row for word in range(12)):
                            parent_transform = [f32(row[parent_prefix + str(word)]) for word in range(12)]
                            if any(value is None for value in parent_transform):
                                parent_transform = None
                    model["world"] = static_model_world(origin, offset,
                        [f32(get(at)) for at in (0x80, 0x84, 0x88)],
                        [f32(get(at)) for at in (0xd8, 0xdc, 0xe0)], get(0x3c), f32(get(0x16c)), parent_transform)
                else:
                    missing.add("sprite-only observer encountered an effect model")
                if model["world"] is None:
                    missing.add("attached particle model requires its additional transform observation")
            attachment = (follow(row.get(prefix + "_1c"), projectile_addresses)
                          if get(0x3c) & 0x400 else None)
            if get(0x3c) & (0x20000000 | 0x40000000):
                bone = row.get(prefix + "_b4")
                attachment = (dict(kind="bone", actor=particle_owner, bone=bone >> 24)
                              if bound and particle_owner is not None and bone is not None
                              and bone & 0x00ffffff == 0 else dict(kind="unobserved"))
            if model is not None:
                if pid not in model_ordinals:
                    key = (particle_owner, bank["source_sha256"], member)
                    model_counts[key] += 1
                    model_ordinals[pid] = model_counts[key]
                    if particle_age > 1 and not effect_entry.retained_birth(
                            setup, index, presentation, address, parent, copied, signature, particle_age):
                        unobserved_model_births.add(pid)
                if start_tick is not None and pid in unobserved_model_births:
                    missing.add("deterministic model birth was not observed")
                if isinstance(attachment, dict) and attachment["kind"] in ("unobserved", "unresolved"):
                    missing.add("deterministic model attachment incomplete")
                effect_models.append(dict(owner=particle_owner, bank=bank["source_sha256"], member=member,
                                          ordinal=model_ordinals[pid], attachment=attachment, model=model))
            particles.append(dict(id=pid, owner=particle_owner if bound else None,
                bank=bank["source_sha256"] if bank else None, member=member,
                initialized=initialized, age=particle_age, origin=origin, offset=offset,
                angles=([f32(get(at)) for at in (0x80, 0x84, 0x88)]
                        if initialized and transforms else None),
                model_slot=(get(0xfc) >> 24) if model is not None else None,
                owner_bone=(get(0xb4) >> 24) if initialized and transforms and get(0x3c) & 0x40000000 else None, model=model))
            particle_ages.append((particles[-1], particle_age))
        # Resolve after enumeration: a child's live parent can appear later in the list.
        for child, observation in model_parent_observations:
            parent_id = ordinal.get(("particle", observation["parent"]))
            if observation["parent"] not in present or parent_id not in model_ordinals:
                missing.add("attached model parent has no current initialized identity")
                continue
            def model_key(pid):
                bank, member, actor = particle_identity[pid]
                return dict(owner=actor, bank=bank["source_sha256"], member=member,
                            ordinal=model_ordinals[pid])
            relation = dict(child=model_key(child), parent=model_key(parent_id), joint=observation["joint"],
                            child_particle=child, parent_particle=parent_id)
            relations = setup.setdefault("effect_model_parents", [])
            prior = [item for item in relations if item["child"] == relation["child"]]
            if prior and prior != [relation]:
                missing.add("attached model changed its live parent identity")
            elif not prior:
                relations.append(relation)
            parent_model = next((item for item in effect_models
                if all(item[key] == value for key, value in relation["parent"].items())), None)
            previous_model = next((item for item in frames[-1]["effect_models"]
                if all(item[key] == value for key, value in relation["parent"].items())), None) if frames else None
            if (parent_model is not None and previous_model is not None
                    and parent_model["model"]["clip"] != previous_model["model"]["clip"]):
                outgoing = [pose for pose in observation["controllers"]
                            if pose["clip"] == previous_model["model"]["clip"]]
                if len(outgoing) == 1:
                    model_parent_transitions.append(dict(child=relation["child"],
                        model=observation["model"], outgoing=outgoing[0]))
        # An incomplete prefix cannot establish death; retain generation identity.
        previous_nodes["particle"] = present if complete_particles else previous_nodes.get("particle", set()) | present
        if index and start_tick is not None:
            if active:
                try:
                    semantics.extend(support_outcomes.enemy_debuffs(
                        case, rows[index - 1], row, setup, semantics, common, start_tick))
                except (KeyError, ValueError) as error:
                    missing.add(f"enemy debuff attribution unavailable: {error}")
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
                controlled_support = (support is not None and
                    recipient["side"] == actors[owner]["side"] and
                    not any(row[f"battle_actor_{s}_action"] >> 24 == 5
                            for s in slots if s != source_owner))
                if (case.get("context") == "revival" and recipient["actor"] == target
                        and recipient["side"] == actors[owner]["side"]
                        and setup.get("opponent_control", {}).get("mode") == "manual"
                        and rows[index - 1][f"battle_actor_{slot}_hp"] == 0
                        and rows[index - 1][f"battle_actor_{slot}_action"] >> 24 == 18
                        and (rows[index - 1][f"actor_{slot}_control_word"] >> 16) & 7 == 2
                        and (row[f"actor_{slot}_control_word"] >> 16) & 7 == 3
                        and not any(row[f"battle_actor_{s}_action"] >> 24 == 5
                                    for s in slots if s != source_owner)):
                    semantics.append(dict(common, kind="revived", owner=owner,
                                          target=target, action=1))
                if (support is not None and support.get("time_stop", False) and
                        recipient["side"] != actors[owner]["side"] and
                        not any(row[f"battle_actor_{s}_action"] >> 24 == 5
                                for s in slots if s != source_owner)):
                    before_stop = support_outcomes.time_stop(rows[index - 1], slot)
                    if not before_stop and recipient["time_stop"]:
                        semantics.append(dict(common, kind="time_stop_started", owner=owner,
                            target=recipient["actor"], action=1, ticks=recipient["time_stop"]))
                if controlled_support and "support" in recipient:
                    try:
                        previous_support = support_outcomes.snapshot(rows[index - 1], slot)
                        semantics.extend(support_outcomes.transition(previous_support, recipient["support"],
                            owner=owner, target=recipient["actor"], common=common))
                    except (KeyError, ValueError) as error:
                        missing.add(f"support transition unavailable: {error}")
                if (case["id"] == "party:5:136" and recipient["actor"] == owner
                        and recipient["tp"] > (rows[index - 1][f"battle_actor_{slot}_tp_word"] & 65535)):
                    try:
                        drain = support_outcomes.seal_drain(row, rows[index - 1], setup, events, common, "tp")
                        if drain is not None:
                            semantics.append(drain)
                    except (KeyError, ValueError) as error:
                        missing.add(str(error))
                if loss > 0:
                    if not competitors:
                        pulse = poison_pulse(row, rows[index - 1], setup, owner,
                                             recipient["actor"], semantics, common)
                        if pulse is not None:
                            semantics.append(pulse)
                            loss -= pulse["amount"]
                            if not loss:
                                continue
                        try:
                            impact = knockdown_impact(row, rows[index - 1], setup, owner,
                                recipient["actor"], events, common)
                        except (KeyError, ValueError) as error:
                            missing.add(f"knockdown impact observation unavailable: {error}")
                            impact = None
                        if impact is not None:
                            events.append(impact)
                            loss -= min(impact["amount"],
                                        rows[index - 1][f"battle_actor_{slot}_hp"] - 1)
                            if not loss:
                                continue
                    if (support is not None and support["terminal"] == "self_defeated" and
                            recipient["actor"] == owner and recipient["hp"] == 0 and
                            row[f"battle_actor_{source_owner}_action"] >> 24 == 18 and
                            (row[f"actor_{source_owner}_control_word"] >> 16) & 7 == 2):
                        self_defeated = True
                        semantics.extend((dict(common, kind="self_cost", owner=owner, target=owner,
                                               action=1, amount=loss),
                                          dict(common, kind="self_defeated", owner=owner, action=1)))
                        continue
                    candidates = [entry for entry in contacts if entry[1:3] == (owner, recipient["actor"])]
                    attached = None
                    try:
                        queued = queued_contact(row, setup, owner, recipient["actor"], rows[index - 1])
                        if queued is not None:
                            pointer = queued["projectile"]
                            if not pointer:
                                attached = queued
                            else:
                                # The queue points to the embedded attack context.
                                pid = ordinal.get(("projectile", pointer - 0x28))
                                # A repeat hit may leave the projectile's result
                                # latch unchanged. This update's queue still binds
                                # its live projectile to the fresh recipient hit.
                                # The dispatcher's fresh victim is the actual
                                # collision recipient; the projectile's aimed
                                # target can be an ally or a different enemy.
                                if any(shot["id"] == pid and shot["owner"] == owner for shot in projectiles):
                                    if not any(entry[0] == pid for entry in candidates):
                                        candidates.append((pid, owner, recipient["actor"], None))
                    except (damage_envelope.UnsupportedDamage, KeyError) as error:
                        missing.add(f"contact observation unavailable: {error}")
                    attributed = not competitors and (len(candidates) == 1 or latch or attached is not None)
                    projectile = candidates[0][0] if len(candidates) == 1 else None
                    event = dict(common, kind="hit" if attributed else "unattributed_hp_loss",
                        owner=owner if attributed else None, target=recipient["actor"], hp_change=-loss,
                        action=1 if attributed else None, projectile=projectile)
                    if attributed and recipient_input is not None:
                        try:
                            event["guard"] = contact_guard(row, rows[index - 1], setup, recipient["actor"])
                        except damage_envelope.UnsupportedDamage as error:
                            missing.add(f"guard outcome unavailable: {error}")
                    if attributed and projectile is not None:
                        try:
                            event["damage_range"] = projectile_damage_range(row, rows[index - 1], projectile_prefixes[projectile], setup,
                                owner, recipient["actor"], recipient["hp"] + loss)
                        except (damage_envelope.UnsupportedDamage, KeyError) as error:
                            missing.add(f"damage interval unavailable: {error}")
                    elif attributed:
                        try:
                            if attached is None:
                                raise damage_envelope.UnsupportedDamage("attached contact rule identity is not yet observed")
                            event["damage_range"] = contact_damage_range(row, rows[index - 1], attached, setup, owner,
                                recipient["actor"], recipient["hp"] + loss,
                                signed16(row[f"actor_{source_owner}_attack_power_word"] & 65535))
                        except (damage_envelope.UnsupportedDamage, KeyError) as error:
                            missing.add(f"damage interval unavailable: {error}")
                    if attributed and "damage_range" in event:
                        prefix = projectile_prefixes[projectile] if projectile is not None else None
                        contact = contact_fields(row, prefix) if prefix is not None else attached
                        strength = (row[prefix + "_144"] >> 16 if prefix is not None else
                                    signed16(row[f"actor_{source_owner}_attack_power_word"] & 65535))
                        event.update(contact_damage_branches(row, rows[index - 1], contact, setup, owner,
                            recipient["actor"], recipient["hp"] + loss, strength, loss))
                        applied = contact_condition(row, rows[index - 1], contact, setup, owner, recipient["actor"])
                        if applied is not None:
                            semantics.append(dict(common, **applied))
                    events.append(event)
                elif loss == 0:
                    try:
                        protected = protected_projectile_contact(row, rows[index - 1], setup,
                            owner, recipient["actor"],
                            {row[prefix + "_address"]: pid
                             for pid, prefix in projectile_prefixes.items()})
                        if protected is None and latch:
                            protected = recovery_protected_contact(row, rows[index - 1], setup,
                                                                  owner, recipient["actor"])
                        if protected is not None:
                            events.append(dict(common, **protected))
                    except (damage_envelope.UnsupportedDamage, KeyError) as error:
                        missing.add(f"protected contact unavailable: {error}")
                elif loss < 0:
                    if case["id"] == "party:5:135" and recipient["actor"] == owner:
                        try:
                            drain = support_outcomes.seal_drain(row, rows[index - 1], setup, events, common, "hp")
                            if drain is not None:
                                semantics.append(drain)
                                continue
                        except (KeyError, ValueError) as error:
                            missing.add(str(error))
                    controlled = (case.get("context") in ("support", "revival") and
                                  (controlled_support or recipient["actor"] == target) and
                                  setup.get("opponent_control", {}).get("mode") == "manual" and
                                  all(a.get("conditions") == [] for a in setup["actors"]) and
                                  not any(row[f"battle_actor_{s}_action"] >> 24 == 5 for s in slots if s != source_owner))
                    semantics.append(dict(common, kind="recovered" if controlled else "unattributed_hp_gain",
                        owner=owner if controlled else None, target=recipient["actor"], action=1 if controlled else None, amount=-loss))
        frame = dict(common, action_age=((combat - start_tick if departure else
            row[f"actor_{source_owner}_clock"] >> 16) if start_tick is not None and not returned else None),
            action_age_basis=("elapsed departure combat updates; null after departure" if departure else
                              "observed authored action elapsed clock; null after returned idle"),
            actors=actors, models=models, projectiles=projectiles, particles=particles,
            **({"secondary_models": secondary_models} if "secondary_bodies" in setup else {}),
            effect_models=effect_models, programs=programs,
            **({"model_parent_transitions": model_parent_transitions} if model_parent_transitions else {}),
            authored_action_clock=row[f"actor_{source_owner}_clock"],
            source_random_state=row["battle_random_state_word"],
            image=images[index] if images else None)
        if case.get("context") == "presentation":
            parts = []
            resources = setup["body_resources"][str(source_owner)]
            for link in range(BODY_CONTROLLERS):
                prefix = f"actor_{source_owner}_animation_{link}"
                layer = row.get(prefix + "_layer_word", 0)
                if layer & 65535 != 2:
                    continue
                resource = row[prefix + "_child_resource"] if row[prefix + "_child"] else row[prefix + "_resource"]
                clips = [int(clip) for clip, address in resources.items() if address == resource]
                if len(clips) != 1:
                    missing.add("partial pose has no unique authored clip")
                    continue
                bits = row[prefix + "_child_frame_bits"] if row[prefix + "_child"] else row[prefix + "_frame_bits"]
                value = f32(bits)
                if value is None:
                    missing.add("partial pose frame is not finite")
                    continue
                parts.append(dict(actor=owner, first_bone=layer >> 16, clip=clips[0], frame=value))
            frame["partial_models"] = parts
        frames.append(frame)
        if returned or started_now:
            started_at = next(event["presentation_tick"] for event in events if event["kind"] == "started")
            registered = recurring_effects(semantics, banks, setup, started_at, scenery_pause)
            keys = {(entry["owner"], entry["bank"], entry["program"], entry["member"]) for entry in baseline}
            baseline.extend(entry for entry in registered if
                (entry["owner"], entry["bank"], entry["program"], entry["member"]) not in keys)
            if baseline:
                setup["effect_baseline"] = baseline
        retired = observation_tail_empty(row, counts, particle_addresses) or baseline_tail(
            row, counts, baseline, programs, particle_ages, particle_addresses, frozen_updates, scenery_frozen)
        retired &= not ((primary | secondary) >> 16) & 0xc0
        if support is not None and support.get("time_stop", False):
            retired &= not any(actor.get("time_stop", 0) for actor in actors)
        if returned and (row[f"battle_actor_{source_owner}_action"] >> 24 == 3 or
                         defeated_terminal or departed_terminal) and retired:
            break
    started = next((event for event in events if event["kind"] == "started"), None)
    history, history_gaps = entry_history.observe(rows, setup, started)
    setup.update(history)
    missing.update(history_gaps)
    if started is not None:
        # Keep preroll in raw observations. The shared comparison starts once,
        # at the witnessed requested action, without per-event realignment.
        first = next(i for i, frame in enumerate(frames) if
                     frame["presentation_tick"] == started["presentation_tick"])
        if first and frames[first - 1]["presentation_tick"] + 1 == started["presentation_tick"]:
            previous = frames[first - 1]
            setup["pre_admission_actors"] = dict(
                presentation_tick=previous["presentation_tick"],
                combat_tick=previous["combat_tick"],
                actors=[{key: actor[key] for key in ("actor", "position", "heading")}
                        for actor in previous["actors"]])
        if departure:
            try:
                setup["departure_prerequisite"] = enemy_prerequisites.departure_observation(rows[:first + 1], setup)
            except (KeyError, ValueError) as error:
                missing.add(f"departure prerequisite unavailable: {error}")
        setup["input"].pop("pending_entry_after_updates", None)
        setup.pop("approach_target_changes", None)
        try:
            pending_entry = enemy_pending_entry(rows, setup, expected, first)
            if pending_entry is not None:
                setup["input"]["pending_entry_after_updates"] = pending_entry
                changes = enemy_approach_targets(rows, setup, pending_entry, first)
                if changes:
                    setup["approach_target_changes"] = changes
        except ValueError as error:
            missing.add(str(error))
        frames = frames[first:]
        events = [event for event in events if event["presentation_tick"] >= started["presentation_tick"]]
    if started:
        setup["input"]["request_after_updates"] = selected_tick - setup["initial_battle"]["combat_tick"]
        setup["input"]["request_delay_basis"] = "observed requested selection minus retained ready combat tick; initialized bound admission is separate"
        setup["input"]["bound_admission_after_updates"] = started["combat_tick"] - setup["initial_battle"]["combat_tick"]
        semantics += [e.copy() for e in events if e["kind"] in ("started", "hit", "knockdown_impact", "unattributed_hp_loss", "projectile_started", "projectile_expired")]
        semantics = [e for e in semantics if e["presentation_tick"] >= started["presentation_tick"]]
        for event in semantics:
            event.pop("id", None)  # allocator identity is diagnostic, not program behavior
            event.pop("observed_age", None)
            event.pop("observed_at_entry", None)
            event.pop("frozen_updates", None)
            event["clock"] = event.pop("combat_tick") - started["combat_tick"]
            event["presentation_clock"] = event.pop("presentation_tick") - started["presentation_tick"]
    else:
        # Raw observations retain the prelude, but no relative event stream
        # exists until the requested action is actually admitted.
        semantics = []
    if support is not None and case.get("capabilities", {}).get("spell"):
        try:
            recipient = support_outcomes.cast_recipient(rows, setup, expected, frames, semantics)
            setup.setdefault("requested_target", setup["target"])
            setup["support_recipient_observation"] = dict(
                recipient, previous_fixture_target=setup["requested_target"])
            setup["target"] = recipient["recipient"]
        except (KeyError, ValueError) as error:
            missing.add(f"support spell recipient unavailable: {error}")
    try:
        chance_effects, condition_changes = support_outcomes.seal_branches(case, rows, setup, frames, semantics, banks)
        if chance_effects:
            setup["chance_effects"] = chance_effects
        semantics.extend(condition_changes)
    except (KeyError, ValueError) as error:
        missing.add(f"seal branch attribution unavailable: {error}")
    try:
        semantics.extend(support_outcomes.spell_debuffs(case, rows, setup, frames, semantics))
    except (KeyError, ValueError) as error:
        missing.add(f"spell debuff attribution unavailable: {error}")
    limits = set(missing)
    if case["id"] == "party:5:135" and not any(event["kind"] == "hp_drained" for event in semantics):
        limits.add("Life Seal drain was not observed")
    if case["id"] == "party:5:136" and not any(event["kind"] == "tp_drained" for event in semantics):
        limits.add("Spirit Seal drain was not observed")
    if recipient_input is not None and recipient_input["mode"] in ("normal", "special") and not recipient_guard_seen:
        limits.add("requested recipient guard was never observed")
    if setup.get("opponent_control", {}).get("mode") != "manual":
        limits.add("opponent automatic fallback choices do not match the neutral native fixture")
    if any(e["kind"] == "unattributed_hp_loss" for e in semantics):
        limits.add("HP loss could not be attributed to an observed contact")
    if any(e["kind"] == "hit" and "damage_range" not in e for e in semantics):
        limits.add("source rule/stat damage interval not yet supplied")
    if started is None:
        limits.add("requested action admission was not observed")
    if not returned:
        limits.add("living departure was not observed" if departure else
                   "self-defeated action handler completion was not observed" if self_defeated else
                   "requested action did not return to idle")
    if rows and not retired:
        limits.add("capture ended before complete projectile/effect retirement; no persistent baseline was established")
    outcome = observed_outcome(case.get("context", "attack"), setup, semantics)
    required = ("hit" if outcome in ("attack", "aerial", "capture") else
                "revived" if outcome == "revival" else
                "departed" if outcome == "departure" else
                "recovered" if outcome == "support" and support is None else None)
    execution = case.get("definition", {}).get("execution")
    execution = execution if isinstance(execution, dict) else {}
    authored = execution.get("attack", execution.get("physical", {}).get("attack", {})).get("events", [])
    if (required == "hit" and scripted_defeat is not None and authored
            and all(isinstance(event, dict) and set(event) <= {"pose", "partial_pose", "sound", "voice", "defeat"}
                    for _, event in authored)):
        required = "self_defeated"
        setup["self_defeat"] = dict(scripted_defeat, terminal_only=True)
    if required == "hit" and hostile_condition_outcome(setup, semantics):
        required = None
    if required and not any(e["kind"] == required for e in semantics):
        limits.add(f"intended {outcome} outcome was not attributed to the requested action")
    if outcome == "presentation":
        expected_presentation = case.get("presentation") or presentation_requirements(case["definition"])
        if groups := expected_presentation.get("empty_contact_groups"):
            observed = setup.get("presentation_contact_bindings")
            if observed != dict(groups=groups, samples=[dict(stage=stage, counts=[0] * len(groups))
                                                       for stage in ("ready", "terminal")]):
                limits.add("inert presentation contacts lack initialized boundary observations")
        setup["presentation"] = expected_presentation
        if expected_presentation.get("retained_pose"):
            ready = next(model for model in setup["ready_models"] if model["actor"] == owner)
            models = [model for frame in frames if frame["action_age"] is not None
                      for model in frame["models"] if model["actor"] == owner]
            if (not models or any(model["clip"] != ready["clip"] for model in models)
                    or len({model["frame"] for model in models}) < 2):
                limits.add("empty authored pose did not preserve advancing ready motion")
        for pose in expected_presentation["poses"]:
            field = "models" if pose["first_bone"] is None else "partial_models"
            samples = [model["frame"] for frame in frames if frame["action_age"] is not None
                       for model in frame[field] if model["actor"] == owner and model["clip"] == pose["clip"]
                       and model.get("first_bone") == pose["first_bone"]]
            if len(set(samples)) < 2:
                limits.add("authored presentation pose did not visibly advance")
        for effect in expected_presentation["effects"]:
            if not any(e["kind"] == "program_started" and e["owner"] == owner and e["program"] == effect for e in semantics):
                limits.add("authored presentation effect did not start")
    if outcome == "movement":
        if not movement_only(case.get("definition", {})):
            limits.add("movement context requires an authored non-damaging movement row")
        if not any(frame["action_age"] is not None and
                   frame["actors"][owner]["position"] != setup["actors"][owner]["position"]
                   for frame in frames):
            limits.add("intended movement outcome was not observed during the requested action")
    if outcome == "capture":
        target = setup["target"]
        if not any(link is not None and link[0] == owner
                   for frame in frames if (link := frame["actors"][target].get("capture"))):
            limits.add("selected action did not capture its recipient")
        if not frames or frames[-1]["actors"][target].get("capture") is not None:
            limits.add("captured recipient was not released")
    if support is not None and not any(support_outcomes.beneficial(event) for event in semantics):
        limits.add("intended support benefit was not attributed to the requested action")
    # Every actually started authored program must have its emitted declaration
    # families positively observed; random instance counts/positions are diagnostic.
    banks_by_sha = {b["source_sha256"]:b for b in banks.values()}
    immediate_ends = {(event["bank"], event["program"]) for event in semantics
        if event["kind"] == "program_started" and event["program"] in
        banks_by_sha[event["bank"]].get("immediate_end_programs", [])}
    if immediate_ends:
        setup["immediate_end_programs"] = [dict(bank=bank, program=member,
            first_command=[0, 0, 254, 0, 0, 0]) for bank, member in sorted(immediate_ends)]
    for event in semantics:
        if event["kind"] == "program_started":
            bank = banks_by_sha[event["bank"]]
            for command in bank["metadata"]["programs"][event["program"]]:
                operation = command["operation"]
                if operation["kind"] == "spawn" and not any(item[:3] == (event["owner"], event["bank"], operation["particle"]) for item in declaration_seen):
                    limits.add("started effect program has an unobserved declaration family")
    if case.get("context") not in ("attack", "support", "revival", "aerial", "movement", "capture", "presentation", "departure"):
        limits.add("selected contextual outcome has no implemented source observer")
    return dict(case=case["id"], setup=setup, frames=frames,
        events=sorted(semantics, key=lambda e: (e["clock"], e["presentation_clock"])),
        assessment=dict(outcome=outcome, rng_streams_matched=False,
                        diagnostic_limits=sorted(diagnostics), trace_schema=3,
                        **({"other_selection": other_selection} if other_selection is not None else {}),
                        declaration_clock="first_initialized_particle", retirement_complete=retired,
                        terminal_complete=returned),
        incomplete_observations=sorted(limits))


def party_case(case):
    result = dict(case)
    capabilities = case["capabilities"]
    if capabilities["target"] == "unavailable":
        raise ValueError("catalogue binding requires an unavailable ordinary-input context")
    if capabilities["revives"]:
        result["target_dead"] = True
        result["cycle_support_target"] = True
    if (case["character"], case["arte"]) == (5, 135):
        result["owner_missing_hp"] = True
    if (case["character"], case["arte"]) == (5, 143):
        # Cyclone needs an ordinary launchable recipient, not the heavy dummy.
        result["formation"] = 0
    if (case["character"], case["arte"]) == (3, 84):
        # Give the surrounding lances a broad-bodied ordinary recipient.
        result["formation"] = 120
        # The first lance begins at x=100. Keep a nonzero planar aim vector
        # instead of exercising the source's undefined coincident aim case.
        result["contact_fixture"] = dict(case.get("contact_fixture", {}), target_x=400.0)
    if case.get("context") == "aerial":
        result["input"] = "aerial"
    if case["character"] == 2:
        result["colette_state"] = 1
    if (case["character"] == 5 and 235 <= case["arte"] <= 245
            or (case["character"], case["arte"]) == (3, 248)):
        result["saved_overlimit"] = True
    if (case["character"], case["arte"]) == (3, 248):
        result["input_arte"] = 80
        result["technique_uses"] = {80: 50}
    if (case["character"], case["arte"]) == (2, 251):
        result["input_arte"] = 57
        result["technique_uses"] = {57: 50, 58: 50}
        # Exercise the trained cast replacement through its ordinary input.
        # The two runtimes have independent random streams.
        result["casting_random_state"] = 0
        result["native_seed"] = 12
    if case.get("context") == "support":
        if capabilities["target"] not in ("ally", "party", "self_target"):
            raise ValueError("support fixture has no supported ordinary recipient selection")
        if case["native"] == 34 and capabilities["target"] == "self_target":
            result["input"] = "guard_down"
        if case["character"] == 4 and case["arte"] in (101, 102, 103, 104, 105, 106):
            result["support_condition"] = "curse" if case["arte"] <= 103 else "weak"
        if (case["character"], case["arte"]) in ((2, 59), (4, 116)):
            result["support_missing_tp"] = True
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
        if battle.get("camera", {}).get("address") and battle["actors"] and all(
                a["action"][0] == 13 or (a["hp"] == 0 and a["action"][0] == 18)
                for a in battle["actors"]):
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
            # A peer's entry controller can still be preparing after the owner
            # becomes idle. Let that ordinary transition finish before taking
            # a checkpoint whose model controllers must all be reproducible.
            try:
                ready_models(state, battle)
            except ValueError as error:
                if str(error) == "ready animation requires unsupported pending preparation":
                    continue
                raise
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


def settle_enemy_entry(worker, output, record, base, initialized, max_vis, *, minimum_gameplay_tick=0):
    """Finish scripted entry under neutral controls before arranging a fixture."""
    battle, owner = initialized["battle"], initialized["actor"]
    at = owner["address"]
    control = (base.read(at + 0x107c, "B") & 0x38,
               base.read(at + 0x107d, "B") & 0xc0)
    state = base.fork()
    manual_opponents(state, battle, owner=-1)
    neutral = output / "neutral-entry.s01"
    state.save(neutral)
    shutil.copyfile(str(output / "initialized.s01") + ".dtm", str(neutral) + ".dtm")
    changes = state.changes
    slots = record["expected_slots"]
    def idle(row):
        return (row["battle_combat_tick_word"] > 0
                and row["battle_gameplay_tick_word"] >= minimum_gameplay_tick
                and all(row[f"battle_actor_{slot}_action"] >> 24 == 3 for slot in slots))
    marker = record["fixture"]["case_marker"]
    rows = worker.observe_until(neutral, output / "settled-entry", marker,
                               max_vis, lambda observed: idle(observed[-1]))
    if not idle(rows[-1]):
        raise ValueError("neutral scripted entry did not reach ordinary idle combat")
    settled = output / "settled-entry.s01"
    worker.snapshot(settled)
    state = State(settled)
    current = inspect(state, battle=True)["battle"]
    actor = next(actor for actor in current["actors"] if actor["slot"] == owner["slot"])
    at = actor["address"]
    state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38 | control[0])
    state.write(at + 0x107d, "B", state.read(at + 0x107d, "B") & 0x3f | control[1])
    state.save(settled)
    record["settled_entry"] = dict(capture=pin(output / "settled-entry/capture.json"),
        neutral_controls=changes, restored_owner_control=state.changes,
        minimum_gameplay_tick=minimum_gameplay_tick,
        observed_gameplay_tick=current["gameplay_tick"], observed_owner_hp=actor["hp"],
        scope="Unchanged entry and any required neutral combat updates complete before fixture preparation; no action, timer or animation is forced.")
    write(output / "fixture.json", record)
    return settled, enemy_artes.initialized_state(settled, output, record, idle=True)


def refresh_enemy_placement(worker, output, record, state, state_path, battle, owner):
    """Publish relocated geometry and then its distances before the first AI choice."""
    actor = next(actor for actor in battle["actors"] if actor["slot"] == owner)
    at = actor["address"]
    control = (state.read(at + 0x107c, "B") & 0x38,
               state.read(at + 0x107d, "B") & 0xc0)
    neutral = state.fork()
    manual_opponents(neutral, battle, owner=-1)
    neutral_path = output / "neutral-placement.s01"
    neutral.save(neutral_path)
    shutil.copyfile(str(state_path) + ".dtm", str(neutral_path) + ".dtm")
    # Distance uses the previous display publication and is updated after
    # control input. Publish the moved body, then refresh its distance while
    # the controller is still neutral.
    refreshed_tick = battle["combat_tick"] + 2
    rows = worker.observe_until(neutral_path, output / "placement-refresh",
        record["fixture"]["case_marker"], 3,
        lambda observed: observed[-1]["battle_combat_tick_word"] >= refreshed_tick, stepped=True)
    if (rows[-1]["battle_combat_tick_word"] != refreshed_tick
            or rows[-1][f"battle_actor_{owner}_action"] >> 24 != 3):
        raise ValueError("idle fixture geometry and distances did not publish during two neutral combat updates")
    worker.snapshot(state_path)
    refreshed = State(state_path)
    refreshed.write(at + 0x107c, "B", refreshed.read(at + 0x107c, "B") & ~0x38 | control[0])
    refreshed.write(at + 0x107d, "B", refreshed.read(at + 0x107d, "B") & 0x3f | control[1])
    refreshed.save(state_path)
    return refreshed, dict(capture=pin(output / "placement-refresh/capture.json"),
        neutral_controls=neutral.changes, restored_owner_control=refreshed.changes,
        combat_tick=rows[-1]["battle_combat_tick_word"],
        scope="Two ordinary neutral updates publish placed geometry and its distances before AI selection; no distance cache, action or timer is assigned.")


def prepare_recipient_input(state, path, battle, specification, learning, *, owner=None):
    """Prepare physical input; contacts remain ordinary gameplay."""
    mode, release, down = specification["mode"], specification["release_after_polls"], specification.get("down_after_polls")
    press = specification.get("press_after_polls")
    stick = specification.get("stick")
    repeat = specification.get("repeat_every_polls")
    intervals = specification.get("stick_intervals", [])
    if battle["combat_tick"] != 0 or battle["party_order"] != [1, 0, 0, 0]:
        raise ValueError("recipient input requires Lloyd before combat")
    if mode not in ("none", "normal", "special", "attack", "stick") or type(release) is not int or not 0 < release <= MAX_RECIPIENT_POLLS or release % 2:
        raise ValueError("recipient input mode/release interval is invalid")
    if ((mode in ("attack", "stick") and (type(press) is not int or press % 2
                                           or not (0 if repeat is not None else 1) <= press < release))
            or mode not in ("attack", "stick") and press is not None):
        raise ValueError("recipient input needs one bounded press interval")
    if repeat is not None and (mode != "attack" or type(repeat) is not int
                               or repeat < 4 or repeat % 2 or repeat > release - press):
        raise ValueError("recipient attack repetition needs an even bounded poll interval")
    if ((mode == "stick" and stick not in ([127, 0], [-127, 0], [0, 127], [127, 127], [-127, 127]))
            or mode != "stick" and stick is not None):
        raise ValueError("recipient stick input requires a horizontal direction or upward jump")
    if ((mode == "special" and (type(down) is not int or down % 2 or not 8 < down < release - 4))
            or mode != "special" and down is not None):
        raise ValueError("special guard needs one bounded downward input after its guard lead")
    target_slot = specification.get("target_slot")
    if target_slot is not None and (mode not in ("attack", "stick") or type(target_slot) is not int
            or not any(a["slot"] == target_slot and a["side"] == 1 for a in battle["actors"])):
        raise ValueError("recipient attack target is not an observed enemy")
    previous_release = release
    for interval in intervals:
        begin, end = interval["press_after_polls"], interval["release_after_polls"]
        if (type(begin) is not int or type(end) is not int or begin % 2 or end % 2
                or not previous_release <= begin < end <= MAX_RECIPIENT_POLLS
                or interval["stick"] not in ([127, 0], [-127, 0])):
            raise ValueError("recipient movement intervals must be bounded and consecutive")
        previous_release = end
    controller = specification.get("controller", "recipient")
    if controller not in ("recipient", "owner") or controller == "owner" and (mode != "stick" or owner is None):
        raise ValueError("owner fixture input requires an observed owner and bounded stick movement")
    actor_slot = owner if controller == "owner" else 0
    actor_index, actor = next((index, actor) for index, actor in enumerate(battle["actors"])
                             if actor["slot"] == actor_slot)
    at = actor["address"]
    if state.read(at + 0x282, "B") & 1:
        raise ValueError("recipient already guards before input")
    state.write(at + 0x107c, "B", state.read(at + 0x107c, "B") & ~0x38)
    state.write(at + 0x107d, "B", state.read(at + 0x107d, "B") & 0x3f)
    member = battle_artes.member(state, 1)
    if controller == "recipient" and state.read(at) != member:
        raise ValueError("recipient persistent record is not Lloyd")
    if mode not in ("attack", "stick"):
        learned = state.read(member + 0x70, "Q")
        state.write(member + 0x70, "Q", learned | (1 << learning[0].index(34)))
    cursor = inspect(state)["movie"]["input_count"]
    movie = Path(str(path) + ".dtm")
    temporary = path.with_suffix(".guard.dtm")
    neutral_movie(path, temporary, cursor, polls=16000)
    data = bytearray(temporary.read_bytes())
    if mode not in ("none", "stick"):
        button = "a" if mode == "attack" else "x"
        for poll in range(cursor + (press or 0), cursor + release):
            if repeat is not None and (poll - cursor - press) % repeat >= 2:
                continue
            struct.pack_into("<H", data, 256 + poll * 8, 0x4000 | DTM_BUTTONS[button])
    if stick is not None:
        for poll in range(cursor + press, cursor + release):
            data[256 + poll * 8 + 4:256 + poll * 8 + 6] = bytes(value + 128 for value in stick)
    if down is not None:
        for poll in range(cursor + down, cursor + down + 4):
            data[256 + poll * 8 + 5] = 0
    for interval in intervals:
        for poll in range(cursor + interval["press_after_polls"], cursor + interval["release_after_polls"]):
            data[256 + poll * 8 + 4:256 + poll * 8 + 6] = bytes(value + 128 for value in interval["stick"])
    movie.unlink()
    movie.write_bytes(data)
    temporary.unlink()
    return dict(actor=actor_index, source_slot=actor_slot, controller=controller,
                mode=mode, port=0, release_after_polls=release,
                down_after_polls=down, press_after_polls=press,
                **({"repeat_every_polls": repeat, "repeat_every_updates": repeat // 2}
                   if repeat is not None else {}),
                **({"stick": stick} if stick is not None else {}),
                **({"target_slot": target_slot} if target_slot is not None else {}),
                **({"stick_intervals": [dict(interval) for interval in intervals]} if intervals else {}),
                **({"learned_arte": 34} if mode not in ("attack", "stick") else {}),
                scope="Physical recipient input with observed admission; no forced action or contact result.")


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
    if case.get("owner_missing_hp"):
        state.write(owner["vitals_address"] + 0x24, "I", owner["max_hp"] // 2)
    opponents = [a for a in battle["actors"] if a["side"] == 1]
    if len(opponents) != 1 or owner["action"][0] not in (2, 3):
        raise ValueError("generic party fixture requires one opponent and an idle owner")
    target = (next((a for a in battle["actors"] if a["slot"] == 1), owner)
              if case.get("target_dead") else owner) if case["fixture"] == "recovery" else opponents[0]
    support = case.get("context") == "support"
    if support:
        if case["capabilities"]["target"] == "self_target":
            target = owner
        else:
            target = next((a for a in battle["actors"] if a["side"] == owner["side"] and
                           a["slot"] != owner["slot"] and a["hp"] > 0), None)
            if target is None:
                raise ValueError("support fixture requires a distinct living ally")
        if condition := case.get("support_condition"):
            profile = state.read(target["address"] + 4)
            expected = {"curse": 1 << 7, "weak": 1 << 4}[condition]
            current = state.read(profile + 0x10, "Q")
            if current not in (0, expected):
                raise ValueError("cure fixture recipient has unexpected mutable conditions")
            if current == 0:
                state.write(profile + 0x10, "Q", expected)
        if case.get("support_missing_tp"):
            for actor in battle["actors"]:
                if actor["side"] == owner["side"] and actor["slot"] != owner["slot"]:
                    state.write(actor["vitals_address"] + 0x2a, "H", actor["max_tp"] // 2)
    bystanders = [a["slot"] for a in battle["actors"] if a["slot"] not in (owner["slot"], target["slot"])]
    target_x = case.get("contact_fixture", {}).get("target_x", 100.0)
    if not math.isfinite(target_x):
        raise ValueError("party contact fixture target position is not finite")
    for actor in battle["actors"]:
        # Keep the whole fixture inside the arena. Parking idle actors beyond
        # its boundary adds a clamp/steering transition before the arte starts.
        x = 0.0 if actor["slot"] == owner["slot"] else target_x if actor["slot"] == target["slot"] else 600.0
        z = (200.0 * (bystanders.index(actor["slot"]) - (len(bystanders) - 1) / 2)
             if actor["slot"] in bystanders else 0.0)
        if support and actor["side"] == owner["side"]:
            x = 0.0 if actor["slot"] == owner["slot"] else 300.0 if actor["slot"] == target["slot"] else -200.0 * actor["slot"]
            z = 0.0
        battle_geometry.place_actor(state, actor, x, z)
    state.write(owner["address"] + 0x1990, "I", target["address"])
    if "casting_random_state" in case:
        state.write(battle["pool"] + 0x7a670, "I", case["casting_random_state"])
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
    guard_down = case.get("input") == "guard_down"
    if guard_down:
        for poll in range(press - 8, press + 4):
            struct.pack_into("<H", data, 256 + poll * 8, 0x4000 | DTM_BUTTONS["x"])
    for poll in range(press, press + 4):
        if guard_down:
            data[256 + poll * 8 + 5] = 0
        else:
            struct.pack_into("<H", data, 256 + poll * 8, 0x4000 | DTM_BUTTONS["b"])
    if case.get("cycle_support_target"):
        # The control stick selects an ally while the caster is stationary.
        for poll in range(press + 16, press + 20):
            data[256 + poll * 8 + 4] = 255
    movie.unlink()
    movie.write_bytes(data)
    temporary.unlink()
    inputs = dict(press_poll=press, button="guard_down" if guard_down else "b", polls=4)
    if case.get("cycle_support_target"):
        inputs["support_cycle_after_polls"] = 16
    if case.get("input") == "aerial":
        inputs.update(aerial_lead_polls=40, aerial_up_polls=32)
    if guard_down:
        inputs["guard_lead_polls"] = 8
    return inspect(state, battle=True)["battle"], 0, target["slot"], inputs, state.changes


def clear_enemy_anchor(geometry, positions, owner, separation, anchor):
    """Translate the selected pair leftward outside neutral body exclusions."""
    forbidden = []
    for selected, relative_x in ((owner, 0.0), (0, -separation)):
        for neutral in set(positions) - {owner, 0}:
            for body in geometry[selected]["spheres"]:
                for other in geometry[neutral]["spheres"]:
                    dz = (anchor[2] + body["root_offset"][2]
                          - positions[neutral][2] - other["root_offset"][2])
                    radius = body["radius"] + other["radius"]
                    if abs(dz) >= radius:
                        continue
                    center = (positions[neutral][0] + other["root_offset"][0]
                              - body["root_offset"][0] - relative_x)
                    half = math.sqrt(radius * radius - dz * dz)
                    forbidden.append([center - half, center + half])
    x = anchor[0]
    for _ in range(len(forbidden) + 1):
        intersections = [lo for lo, hi in forbidden if lo <= x <= hi]
        if not intersections:
            return [x, anchor[1], anchor[2]], forbidden
        # Positive clearance avoids float32 tangency at an exclusion boundary.
        x = min(intersections) - 1.0
    raise ValueError("bounded body exclusion translation did not converge")


def in_arena_recipient_gap(enemy, party, anchor, minimum, gap):
    """Keep the recipient inside the arena without leaving the authored interval."""
    if abs(anchor[2]) >= PARTY_FIXTURE_RADIUS:
        raise ValueError("selected pair's depth leaves no room for a party recipient")
    edge = -math.sqrt(PARTY_FIXTURE_RADIUS ** 2 - anchor[2] ** 2)
    separation = battle_geometry.separation_for_gap(enemy, party, gap)
    if anchor[0] - separation >= edge:
        return gap, separation
    available = battle_geometry.surface_gap(enemy, party, [anchor[0], anchor[2]], [edge, anchor[2]])
    if available <= minimum:
        raise ValueError("authored minimum distance cannot fit the recipient inside the arena")
    gap = (minimum + min(gap, available)) / 2
    return gap, battle_geometry.separation_for_gap(enemy, party, gap)


def origin_bounded_anchor(anchor, minimum, maximum):
    """Fit radial selection bounds while preserving the selected front rank."""
    distance = math.hypot(anchor[0], anchor[2])
    if minimum <= distance <= maximum:
        return anchor
    lower = min(minimum + 1.0, maximum)
    upper = max(maximum - 1.0, lower)
    radius = max(lower, min(distance, upper))
    if distance < minimum:
        # Increasing depth preserves the admission rank along X.
        depth = math.sqrt(max(0.0, radius * radius - anchor[0] * anchor[0]))
        return [anchor[0], anchor[1], math.copysign(depth, anchor[2])]
    return [anchor[0] * radius / distance, anchor[1], anchor[2] * radius / distance]


def enemy_placement_interval(row):
    """Selection precedes approach; their distance bounds need not intersect."""
    minimum = max(0, row["range"][0])
    maximum = row["range"][1] or math.inf
    if maximum < minimum:
        raise ValueError("authored enemy selection distance interval is empty")
    approach_minimum, approach_maximum = row.get("approach_minimum", 0), row.get("approach_range", 0)
    if approach_maximum > 0:
        # Reversed approach bounds intentionally require an ordinary retreat.
        start = max(minimum, approach_minimum if approach_minimum <= approach_maximum else 0)
        end = min(maximum, approach_maximum)
        if start < end:
            return start, end
    # Start at a selectable distance, then retain the observed approach. A
    # zero approach maximum disables that distance gate entirely.
    return minimum, maximum


def enemy_contact_gap(attack, enemy, party, minimum, maximum, attacking_target):
    """Choose an admitted distance which the authored contact can actually reach."""
    contacts = [event["contact"] for _, event in attack.get("events", []) if "contact" in event]
    # A positive selection minimum must survive ordinary idle-pose movement.
    # Reserve one contact radius inside that interval, rather than placing the
    # pair at an almost exact boundary that can select a fallback action.
    clearance = min((maximum - minimum) / 2,
                    min((contact["radius"] for contact in contacts),
                        default=min(body["radius"] for body in party["spheres"]))
                    if minimum > 0 else 1.0)
    gap = maximum - clearance if attacking_target and math.isfinite(maximum) else minimum + clearance
    if attacking_target:
        return gap
    for _, event in attack.get("events", []):
        projectile = event.get("projectile", {}).get("definition")
        if not projectile:
            continue
        motion = projectile["motion"]
        velocity, acceleration = projectile["velocity"], projectile["acceleration"]
        if (motion["origin"] != "actor" or motion.get("steering")
                or velocity[1] <= 0 or acceleration[1] >= 0
                or velocity[2] <= 0 or acceleration[0] != 0 or acceleration[2] != 0):
            continue
        # Lobbed projectiles pass above a touching recipient. Place the target
        # on the descending nominal arc, using its initialized body center.
        # Jitter and actual contact remain observed, never assumed to hit.
        body = party["spheres"][0]
        position = list(projectile["spawn_offset"])
        speed = list(velocity)
        contact = projectile["contact"]
        active = projectile["active"] or [0, projectile["lifetime"]]
        for tick in range(active[1] + 1):
            position = [p + v for p, v in zip(position, speed)]
            speed = [v + a for v, a in zip(speed, acceleration)]
            if (tick < active[0] or speed[1] >= 0
                    or position[1] + contact["offset"][1] > body["root_offset"][1]):
                continue
            separation = position[2] + contact["offset"][2] + body["root_offset"][0]
            desired = battle_geometry.surface_gap(enemy, party, [separation, 0], [0, 0])
            return max(minimum + clearance, min(desired, maximum - clearance))
    return gap


def arrange_enemy_contact(base, path, initialized, case, assets):
    """Place the selected pair, optionally away from a collinear collision normal."""
    result = _arrange_enemy_contact(base, path, initialized, case, assets)
    offset = case.get("contact_fixture", {}).get("recipient_z_offset")
    if offset is None:
        return result
    if result is None or not math.isfinite(offset):
        raise ValueError("lateral fixture requires bounded authored placement and finite offset")
    state = State(path)
    target = next(actor for actor in inspect(state, battle=True)["battle"]["actors"] if actor["slot"] == 0)
    state.write(target["address"] + 0x18c8, "f", target["position"][2] + offset)
    state.write(target["address"] + 0x1964, "f", target["center"][2] + offset)
    state.save(path)
    actors = {actor["slot"]: actor for actor in inspect(State(path), battle=True)["battle"]["actors"]}
    owner, recipient = actors[initialized["actor"]["slot"]], actors[0]
    gap = battle_geometry.surface_gap(result["owner_geometry"], result["target_geometry"],
        [owner["position"][0], owner["position"][2]],
        [recipient["position"][0], recipient["position"][2]])
    lower, upper = result["placement_interval"]
    if not lower < gap or (upper is not None and not gap < upper):
        raise ValueError("lateral fixture body gap is outside the authored interval")
    if math.hypot(recipient["position"][0], recipient["position"][2]) >= 850:
        raise ValueError("lateral fixture recipient is outside the arena interior")
    result["lateral_fixture"] = dict(recipient_z_offset=offset, observed_body_gap=gap,
        changes=state.changes, proof=enemy_placement_proof(base, path, initialized, assets),
        scope="Only initial recipient root/center Z changes. Ordinary updates publish geometry and target direction before AI admission; authored actions, contacts and timing remain live.")
    return result


def _arrange_enemy_contact(base, path, initialized, case, assets):
    """Place a selected pair within its authored distance and origin bounds."""
    row = case["definition"]
    battle, owner = initialized["battle"], initialized["actor"]
    neutral_enemies = [actor for actor in battle["actors"]
                       if actor["side"] == 1 and actor["slot"] != owner["slot"]]
    admission_slots = min(3, sum(actor["side"] == 0 for actor in battle["actors"]) + 1)
    behind_front_rank = sum(actor["position"][0] < owner["position"][0]
                            for actor in neutral_enemies) >= admission_slots
    execution = row.get("execution", {})
    if not isinstance(execution, dict):
        return None
    attack = execution.get("attack", execution.get("physical", {}).get("attack", {}))
    spell = "spell" in execution and case.get("context") == "attack"
    if not behind_front_rank and not any(row["range"]) and case.get("context") != "movement" and not spell and not any("contact" in event or "projectile" in event
                             for _, event in attack.get("events", [])):
        return None
    minimum, maximum = enemy_placement_interval(row)
    target = next(a for a in battle["actors"] if a["slot"] == 0)
    # Bind the unedited body poses. Geometry resolves their published model
    # transform before applying the current actor transform.
    enemy = battle_geometry.read_initialized_body(base, owner, assets, enemy=case["enemy"])
    party = battle_geometry.read_initialized_body(base, target, assets, character=battle["party_order"][0])
    # Leave room for the required opponent attack to miss while the selected
    # enemy remains within its authored approach interval.
    attacking_target = "target_attacking" in row["requirements"]["predicates"]
    # An admission radius is not the reach of the emitted contact. Start near
    # its close edge so stationary recipients are inside short melee/projectile
    # paths, without changing an action's selection or movement rules.
    gap = enemy_contact_gap(attack, enemy, party, minimum, maximum, attacking_target)
    fixture = case.get("contact_fixture", {})
    if fixture.get("placement") == "selection":
        # Conditional actions can need a physical prerequisite while outside
        # melee reach. Their subsequent approach is still observed normally.
        minimum, maximum = max(0, row["range"][0]), row["range"][1] or math.inf
    if fixture.get("placement") == "far":
        if not math.isfinite(maximum):
            raise ValueError("far contact fixture requires a bounded approach interval")
        gap = maximum - min(1.0, (maximum - minimum) / 2)
    if "surface_gap" in fixture:
        gap = fixture["surface_gap"]
    # Keep the same facing arrangement if the requested body gap would put
    # the recipient behind the owner's root. A forward body may still have
    # a valid close placement even when coincident roots are far apart.
    try:
        battle_geometry.separation_for_gap(enemy, party, gap)
    except ValueError as error:
        if str(error) != "right-hand placement requires a negative root separation":
            raise
        root_clearance = min(e["radius"] + p["radius"]
                             for e in enemy["spheres"] for p in party["spheres"])
        separated_gap = battle_geometry.surface_gap(enemy, party, [root_clearance, 0], [0, 0])
        gap = min(separated_gap, maximum - min(1.0, (maximum - minimum) / 2))
    if not minimum <= gap <= maximum or not math.isfinite(gap):
        raise ValueError("contact fixture distance is outside its authored placement interval")
    separation = battle_geometry.separation_for_gap(enemy, party, gap)
    state = State(path)
    current = {a["slot"]:a for a in inspect(state, battle=True)["battle"]["actors"]}
    prepared_positions = {slot: actor["position"] for slot, actor in current.items()}
    if "recipient_anchor" in fixture:
        # Attached actors move with their ordinary encounter controller. Place
        # only the recipient beside the witnessed settled anchor.
        x, z = fixture["recipient_anchor"]
        if not all(math.isfinite(value) for value in (x, z)):
            raise ValueError("contact fixture recipient anchor is not finite")
        gap, separation = in_arena_recipient_gap(enemy, party, [x, 0, z], minimum, gap)
        actor = current[target["slot"]]
        x -= separation
        battle_geometry.place_actor(state, actor, x, z)
        # Undo the generic row fixture's owner translation; the attachment
        # begins exactly where the initialized encounter placed it.
        for offset, value in ((0x18c0, owner["position"][0]), (0x18c8, owner["position"][2]),
                              (0x195c, owner["center"][0]), (0x1964, owner["center"][2])):
            state.write(owner["address"] + offset, "f", value)
        state.save(path)
        return dict(placement_interval=[minimum, maximum if math.isfinite(maximum) else None],
            target_surface_gap=gap, root_separation=separation,
            recipient_anchor=fixture["recipient_anchor"], owner_geometry=enemy, target_geometry=party,
            changes=state.changes, scope="Recipient placement beside an observed attachment anchor; owner deployment remains ordinary.")
    # Station policies replace the encounter position before choosing an action.
    policy = ContentSnapshot(assets).enemy_rules(case["enemy"])["actions"]["policy"]
    behavior = policy.get("behavior") or {}
    stations = (behavior.get("three_stations") or {}).get("stations")
    health_stage = behavior.get("health_stage") or {}
    initial_station = stations[0] if stations else health_stage.get("initial_station")
    anchor = list(initial_station["position"] if initial_station else owner["position"])
    initial_actions = health_stage.get("initial_actions")
    fixed_heading = initial_actions is None or initial_actions["start"] <= case["action_row"] < initial_actions["end"]
    if initial_station and fixed_heading:
        state.write(owner["address"] + 0x18d0, "f", initial_station["heading"])
    origin_bounds = [predicate["origin_distance"] for predicate in row["requirements"]["predicates"]
                     if isinstance(predicate, dict) and "origin_distance" in predicate]
    origin_minimum = max([0] + [bounds["minimum"] for bounds in origin_bounds if bounds["minimum"] is not None])
    origin_maximum = min([math.inf] + [bounds["maximum"] for bounds in origin_bounds if bounds["maximum"] is not None])
    if "owner_anchor" in fixture:
        x, z = fixture["owner_anchor"]
        if not all(math.isfinite(value) for value in (x, z)):
            raise ValueError("contact fixture owner anchor is not finite")
        anchor = [x, anchor[1], z]
    anchor = origin_bounded_anchor(anchor, origin_minimum, origin_maximum)
    gap, separation = in_arena_recipient_gap(enemy, party, anchor, minimum, gap)
    for slot, x, z in ((owner["slot"], anchor[0], anchor[2]),
                       (target["slot"], anchor[0] - separation, anchor[2])):
        battle_geometry.place_actor(state, current[slot], x, z)
    state.save(path)
    placement = dict(placement_interval=[minimum,maximum if math.isfinite(maximum) else None], target_surface_gap=gap,
                root_separation=separation, owner_geometry=enemy, target_geometry=party,
                changes=state.changes, scope="Initialized body-distance placement only; actual contact must still be observed.")
    # Neutral enemies still occupy the front rank used by ordinary admission.
    # Keep the selected pair ahead of that rank, rather than disabling the
    # source's admission rules or waiting for neutral actors to attack.
    behind_front_rank = (separation > ENEMY_ENGAGEMENT_DISTANCE and
                        sum(actor["position"][0] < anchor[0] for actor in neutral_enemies) >= admission_slots)
    try:
        enemy_placement_proof(base, path, initialized, assets)
        if not behind_front_rank and (not attacking_target or len(battle["actors"]) == 2):
            return placement
    except ValueError as error:
        if str(error) != "selected fixture placement overlaps an untouched encounter actor":
            raise
    identities = {a["slot"]: a["enemy"] for a in initialized["identities"]}
    geometry = {owner["slot"]: enemy, target["slot"]: party}
    for actor in battle["actors"]:
        slot = actor["slot"]
        if slot in geometry:
            continue
        identity = (dict(enemy=identities[slot]) if slot >= 4
                    else dict(character=battle["party_order"][slot]))
        geometry[slot] = battle_geometry.read_initialized_body(base, actor, assets, **identity)
    prepared_anchor = anchor if origin_bounds or initial_station or "owner_anchor" in fixture else prepared_positions[owner["slot"]]
    if attacking_target or behind_front_rank:
        # Put the required attack beyond the neutral encounter actors as well.
        # The same body exclusions choose the nearest clear position.
        prepared_anchor = list(prepared_anchor)
        prepared_anchor[0] = min(position[0] for slot, position in prepared_positions.items()
                                 if slot not in (owner["slot"], target["slot"]))
    for _ in range(len(prepared_positions) + 1):
        chosen, exclusions = clear_enemy_anchor(geometry, prepared_positions, owner["slot"], separation, prepared_anchor)
        bounded = origin_bounded_anchor(chosen, origin_minimum, origin_maximum)
        if bounded != chosen:
            prepared_anchor = bounded
            continue
        fitted_gap, fitted_separation = in_arena_recipient_gap(enemy, party, chosen, minimum, gap)
        if fitted_separation == separation:
            break
        gap, separation = fitted_gap, fitted_separation
        prepared_anchor = chosen
    else:
        raise ValueError("body clearance and arena placement did not converge")
    distance = math.hypot(chosen[0], chosen[2])
    for predicate in row["requirements"]["predicates"]:
        if isinstance(predicate, dict) and "origin_distance" in predicate:
            bounds = predicate["origin_distance"]
            if ((bounds["minimum"] is not None and distance < bounds["minimum"])
                    or (bounds["maximum"] is not None and distance > bounds["maximum"])):
                raise ValueError("clear placement would violate authored origin-distance bounds")
    state = State(path)
    current = {a["slot"]: a for a in inspect(state, battle=True)["battle"]["actors"]}
    for slot, x in ((owner["slot"], chosen[0]), (target["slot"], chosen[0] - separation)):
        battle_geometry.place_actor(state, current[slot], x, chosen[2])
    state.save(path)
    proof = enemy_placement_proof(base, path, initialized, assets)
    placement["changes"].extend(state.changes)
    placement.update(target_surface_gap=gap, root_separation=separation)
    placement["clear_anchor_fixture"] = dict(rejected_anchor=anchor, prepared_anchor=prepared_anchor,
        selected_anchor=chosen, forbidden_anchor_intervals=exclusions, selected_body_gap=gap, proof=proof,
        scope="Place only the selected pair before combat within the authored distance interval and party arena; preserve heights, headings, authored tracks and neutral positions.")
    return placement


def enemy_placement_proof(base, path, initialized, assets):
    """Untouched encounter actors stay in place and clear of the selected pair."""
    before = initialized["battle"]
    current = {a["slot"]:a for a in inspect(State(path), battle=True)["battle"]["actors"]}
    moved = {0, initialized["actor"]["slot"]}
    enemies = {a["slot"]:a["enemy"] for a in initialized["identities"]}
    geometry = {}
    carried = set()
    content = ContentSnapshot(assets)
    host = next((actor for actor in before["actors"] if actor["side"] == 1), None)
    if (before["combat_tick"] == 0 and host is not None
            and content.enemy_rules(enemies[host["slot"]])["profile"]["traits"]["hosts_participants"]):
        carried = {actor["slot"] for actor in before["actors"]
                   if actor["slot"] in enemies and actor["slot"] > host["slot"]
                   and content.enemy_rules(enemies[actor["slot"]])["profile"].get("hosted_body")
                   and current[actor["slot"]]["position"] == actor["position"]}
    for actor in before["actors"]:
        slot = actor["slot"]
        identity = (dict(enemy=enemies[slot]) if slot >= 4
                    else dict(character=before["party_order"][slot]))
        geometry[slot] = battle_geometry.read_initialized_body(base, actor, assets, **identity)
        if slot not in moved and current[slot]["position"] != actor["position"]:
            raise ValueError("neutral actor placement changed from its initialized encounter")
    gaps = []
    for selected in sorted(moved):
        for other in sorted(set(current) - moved):
            a, b = current[selected]["position"], current[other]["position"]
            gap = min(math.hypot(a[0] + x["root_offset"][0] - b[0] - y["root_offset"][0],
                                 a[2] + x["root_offset"][2] - b[2] - y["root_offset"][2])
                      - x["radius"] - y["radius"]
                      for x in geometry[selected]["spheres"] for y in geometry[other]["spheres"])
            attached = selected in carried or other in carried
            if gap < 0 and not attached:
                raise ValueError("selected fixture placement overlaps an untouched encounter actor")
            gaps.append(dict(selected=selected, untouched=other, surface_gap=gap,
                             **({"authored_attachment": True} if attached else {})))
    return dict(positions=[dict(slot=slot, position=actor["position"]) for slot,actor in current.items()],
        geometry=list(geometry.values()), gaps=gaps,
        scope="Only selected owner and recipient move; all other initialized encounter positions are preserved.")


def ready_models(snapshot, battle):
    """Read the bound main controller, with no guessed phase or loop mode."""
    models, proof = [], []
    for index, actor in enumerate(battle["actors"]):
        address = actor["address"]
        clip = snapshot.read(address + 0x1046, "B")
        body_clip = clip
        table = snapshot.read(address + 0x1010) if clip < 100 else address + 0x1018
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
        initial_override = None
        if snapshot.read(controller) != resource and not child:
            profile = snapshot.read(address + 4)
            initial, override = snapshot.read(profile + 0x97, "B"), snapshot.read(profile + 0xc1, "B")
            table = snapshot.read(address + 0x1010) if override < 100 else address + 0x1018
            override_resource = table + (override if override < 100 else override - 100) * 20
            if override and clip == initial and snapshot.read(controller) == override_resource:
                clip, resource = override, override_resource
                initial_override = dict(profile=profile, body_clip=body_clip, clip=clip, resource=resource)
        if snapshot.read(controller) != resource or child:
            raise ValueError("ready animation requires unsupported pending preparation")
        if not all(math.isfinite(v) for v in fields.values()):
            raise ValueError("ready animation contains nonfinite values")
        status = snapshot.read(address + 0x280, "B")
        defeated_hidden = bool(status & 4) and actor["hp"] <= 0 and (
            snapshot.read(address + 0x107d, "B") & 7 == 2
            and snapshot.read(address + 0x1ac, "B") == 18
            and snapshot.read(address + 0x1b0, "B") == 16)
        models.append(dict(actor=index, clip=clip, frame=fields["frame"],
                           rate=0.0 if flags & 2 else fields["rate"], repeat=not bool(flags & 8),
                           loop_start=fields["loop_start"], defeated_hidden=defeated_hidden))
        proof.append(dict(actor=index, source_slot=actor["slot"], controller=controller,
                          resource=resource, flags=flags, child=child, body_clip=body_clip,
                          initial_override=initial_override, status=status, **fields))
        if actor["side"] == 0 and clip >= 100:
            proof[-1]["source_motion"] = dict(header=snapshot.read(resource),
                descriptor=snapshot.read(resource + 4), duration=snapshot.read(resource + 8, "f"))
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


def ready_attachment_effects(snapshot, battle):
    """Retain carried-emitter phase from the ready checkpoint, before any input."""
    effects = []
    for actor, observed in enumerate(battle["actors"]):
        address = observed["address"]
        profile = snapshot.read(address + 4)
        count = snapshot.read(profile + 0x1e4, "B")
        table = snapshot.read(address + 0x1560)
        if count > 8 or count and not 0x80000000 <= table <= 0x81800000 - count * 0xda0:
            raise ValueError("invalid ready attachment table")
        for slot in range(count):
            unit = table + slot * 0xda0
            if not snapshot.read(unit + 0xd84):
                continue
            metadata = snapshot.read(unit + 0xd80)
            if not 0x80000000 <= metadata <= 0x817fffc0:
                raise ValueError("invalid ready attachment metadata")
            raw_interval = snapshot.read(metadata + 10, "B")
            if not raw_interval:
                continue
            counter = snapshot.read(unit + 0xd90)
            interval = raw_interval + 1
            if counter > interval:
                raise ValueError("ready carried-effect counter exceeds its recurrence")
            effects.append(dict(actor=actor, slot=slot, interval=interval, counter=counter))
    return effects


def ready_passive_effects(snapshot, battle):
    """Observe recurring body-effect clocks before any fixture input."""
    effects = []
    for actor, observed in enumerate(battle["actors"]):
        address = observed["address"]
        profile = snapshot.read(address + 4)
        slots = [(snapshot.read(profile + 0xbe, "H"), snapshot.read(profile + 0xc0, "B"),
                  snapshot.read(address + 0x19a4, "H"))]
        slots.extend((snapshot.read(profile + 0xc2 + i, "B"), snapshot.read(profile + 0xc8 + i, "B"),
                      snapshot.read(address + 0x19a6 + i, "B")) for i in range(6))
        for interval, program, counter in slots:
            if not interval:
                continue
            if counter > interval:
                raise ValueError("ready body-effect counter exceeds its recurrence")
            effects.append(dict(actor=actor, program=program, interval=interval, counter=counter))
    return effects


def ready_idle_effects(snapshot, battle):
    """Retain paused idle-emitter clocks independently of unconditional body effects."""
    effects = []
    for actor, observed in enumerate(battle["actors"]):
        address = observed["address"]
        profile = snapshot.read(address + 4)
        interval = snapshot.read(profile + 0xe8, "B")
        if not interval:
            continue
        counter = snapshot.read(address + 0x19cc, "B")
        if counter > interval:
            raise ValueError("ready idle-effect counter exceeds its recurrence")
        effects.append(dict(actor=actor, program=snapshot.read(profile + 0xe9, "B"),
                            interval=interval, counter=counter))
    return effects


def preparation_context(snapshot, entry):
    """Combine retained field globals with the current persistent session."""
    globals_address = entry.read(0x8035a578)
    if not 0x80000000 <= globals_address <= 0x817fffac:
        raise ValueError("retained entry has no valid preparation globals")
    session = snapshot.read(0x8035a768)
    if not 0x80000000 <= session <= 0x817fe1c2:
        raise ValueError("ready checkpoint has no valid persistent session")
    event_flags = [i for i in range(2048) if snapshot.read(session + 0xc9d + i // 8, "B") & (1 << (i % 8))]
    return dict(story=entry.read(globals_address + 0x40, "i"),
        colette_state=entry.read(globals_address + 0x4c, "i"),
        battle_rank=snapshot.read(session + 0x19f, "B"),
        event_flags=event_flags, devils_arms_unlocked=0x3fa in event_flags,
        victory_story_flags=[flag in event_flags for flag in (27, 28)],
        map=snapshot.read(session + 0x10d0), world_music=entry.read(globals_address + 0x50, "i"),
        overlimit_boost=bool(snapshot.read(session + 0x1e3c, "H") & 4))


def ready_motion(snapshot, actor):
    """Observe the complete moving state before the first fixture input."""
    address = actor["address"]
    phase = snapshot.read(address + 0x28e, "H")
    if phase >= 360:
        raise ValueError("ready hover phase is outside its cycle")
    return dict(position=actor["position"], heading=actor["heading"],
        **{key: [snapshot.read(address + offset + axis * 4, "f") for axis in range(3)]
           for key, offset in (("facing_direction", 0x18e4), ("movement_direction", 0x18f0),
                               ("target_direction", 0x18fc))},
        **{key: snapshot.read(address + offset, "f") for key, offset in
           (("forward_step", 0x1908), ("forward", 0x1910), ("vertical", 0x1914),
            ("acceleration", 0x1918), ("gravity", 0x191c))},
        hover_settled=bool(snapshot.read(address + 0x281, "B") >> 5), hover_phase=phase,
        floor_armed=bool(snapshot.read(address + 0x1183, "B") & 4))


def setup_record(case, state, battle, owner, target, assistance, inputs, entry_path, learning):
    snapshot = State(state)
    entry = State(entry_path)
    context = preparation_context(snapshot, entry)
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
            costume=snapshot.read(at + 0x11, "B"),
            techniques=techniques,
            technique_uses={arte: snapshot.read(at + 0x88 + index * 2, "H")
                            for index, arte in enumerate(learning[character - 1])
                            if arte and snapshot.read(at + 0x88 + index * 2, "H")},
            equipment=[snapshot.read(at + offset, "H") for offset in (0x4a, 0x4c, 0x4e, 0x52, 0x54, 0x50)],
            base_stats=[snapshot.read(at + offset, "H") for offset in (0x26, 0x28, 0x2a, 0x2c, 0x34, 0x32, 0x30)],
            luck=snapshot.read(at + 0x2e, "H") // 10,
            technique_balance=snapshot.read(at + 0x10c, "b"),
            ex_skills=[snapshot.read(at + 0xee + i, "B") for i in range(4)],
            ex_gems=[snapshot.read(at + 0xea + i, "B") for i in range(4)],
            compound_ex_skills=[i for i in range(24) if mask & (1 << i)]))
    support = case.get("context") == "support"
    actors = []
    for index, actor in enumerate(battle["actors"]):
        vitals = actor["vitals_address"]
        active_overlimit = snapshot.read(actor["address"] + 0x107d, "B") & 7 == 4
        overlimit = snapshot.read(actor["address"] + 0x10a8, "h")
        if not 0 <= overlimit <= (1250 if active_overlimit else 1000):
            raise ValueError("initialized OverLimit gauge is outside its supported range")
        profile = snapshot.read(actor["address"] + 4)
        if not 0x80000000 <= profile <= 0x817fffc0:
            raise ValueError("invalid initialized actor profile")
        mutable = snapshot.read(profile + 0x10, "Q")
        layered = snapshot.read(actor["address"] + 0x1d0, "Q") | snapshot.read(profile + 0x38, "Q")
        if layered or mutable and not support:
            raise ValueError("initial nonzero condition fields need typed normalization before native replay")
        observed_support = None
        observed_time_stop = None
        if support:
            slot, address = actor["slot"], actor["address"]
            observed_support = support_outcomes.snapshot({
                f"actor_{slot}_condition_10": mutable >> 32,
                f"actor_{slot}_condition_14": mutable & 0xffffffff,
                f"actor_{slot}_raw_1e0": snapshot.read(address + 0x1e0),
                f"actor_{slot}_raw_280": snapshot.read(address + 0x280),
                f"battle_actor_{slot}_tp_word": snapshot.read(vitals + 0x28),
            }, slot)
            support_outcomes.validate_initial(observed_support, caster=slot == owner)
            observed_time_stop = support_outcomes.time_stop({
                "battle_time_stop_word": snapshot.read(battle["pool"] + 0x15718),
                f"actor_{slot}_time_stop_flags": snapshot.read(address + 0x1180),
            }, slot)
            if observed_time_stop:
                raise ValueError("support fixture begins during an existing time stop")
        stats = {name: snapshot.read(vitals + offset, "H") for name, offset in
                 (("slash", 0x2c), ("thrust", 0x2e), ("defense", 0x30),
                  ("intelligence", 0x32), ("accuracy", 0x34), ("evasion", 0x36))}
        stats["level"] = snapshot.read(vitals + 0x1d, "B")
        persistent = snapshot.read(actor["address"])
        if actor["side"] == 0 and not 0x80000000 <= persistent <= 0x817fffdb:
            raise ValueError("initialized actor lacks its observed active-style record")
        stats["technique_balance"] = snapshot.read(persistent + 0x24, "b") if actor["side"] == 0 else None
        movement_speed = snapshot.read(actor["address"] + 0x1910, "f")
        actors.append(dict(actor=index, source_slot=actor["slot"], side=actor["side"],
            motion=ready_motion(snapshot, actor),
            **{key: actor[key] for key in ("hp", "max_hp", "tp", "max_tp", "position", "heading")},
            **{key: [snapshot.read(actor["address"] + offset + axis * 4, "f") for axis in range(3)]
               for key, offset in (("facing_direction", 0x18e4), ("movement_direction", 0x18f0),
                                   ("target_direction", 0x18fc))},
            reaction_direction=[snapshot.read(actor["address"] + 0x1944 + axis * 4, "f") for axis in range(3)],
            **({"movement_speed": movement_speed} if movement_speed != 0 else {}),
            stats=stats, conditions=observed_support["conditions"] if support else [],
            **({"support": observed_support, "time_stop": observed_time_stop} if support else {}),
            overlimit_charge=0 if active_overlimit else overlimit,
            effect_clock=snapshot.read(actor["address"] + 0x1bc, "H"),
            **({"overlimit_remaining": overlimit} if active_overlimit else {}),
            source_vitals=bytes(snapshot.ram[actor["vitals_address"] - 0x80000000:actor["vitals_address"] - 0x80000000 + 128]).hex()))
    canonical = {a["source_slot"]: a["actor"] for a in actors}
    source_actor_map = [{"actor": a["actor"], "source_slot": a.pop("source_slot"),
                         "vitals_hex": a.pop("source_vitals")} for a in actors]
    manual = all((snapshot.read(a["address"] + 0x107c, "B") & 0x38) == 0 and
                 snapshot.read(a["address"] + 0x107d, "B") >> 6 == 3
                 for a in battle["actors"] if a["slot"] != owner)
    body_resources = {}
    for a in battle["actors"]:
        normal, shared = snapshot.read(a["address"] + 0x1010), a["address"] + 0x1018
        body_resources[str(a["slot"])] = {str(clip): (normal + clip * 20 if clip < 100 else shared + (clip - 100) * 20)
                                         for clip in range(256)}
    poses, pose_proof = ready_models(snapshot, battle)
    inputs = dict(inputs)
    if "character" in case and inputs.get("button") == "b":
        shortcut = snapshot.read(battle_artes.member(snapshot, case["character"]) + battle_artes.SHORTCUT, "H")
        inputs.update(selection_arte=shortcut, selection_native=snapshot.read(0x80202f90 + shortcut * 88, "H"))
    native_seed = party_case(case).get("native_seed") if "character" in case else None
    return dict(case=case["id"], owner=canonical[owner], target=canonical[target], formation=battle["formation"],
        arena=battle["arena"], initial_battle=battle, seed=battle["random_state"]["entry_seed"],
        **({"native_seed": native_seed} if native_seed is not None else {}),
        party=party, party_members=members, learned_technique_observations=learned_proof,
        actors=actors, source_actor_map=source_actor_map,
        **({"support": {"time_stop": True,
                       "terminal": "self_defeated" if (case.get("character"), case.get("arte")) == (2, 59)
                       else "returned"}} if support else {}),
        body_resources=body_resources, ready_models=poses, ready_model_observations=pose_proof,
        **effect_entry.observe(snapshot, battle),
        secondary_bodies=secondary_bodies.ready(snapshot, battle),
        ready_attachment_effects=ready_attachment_effects(snapshot, battle),
        ready_passive_effects=ready_passive_effects(snapshot, battle),
        ready_idle_effects=ready_idle_effects(snapshot, battle),
        body_initial_overrides={str(p["source_slot"]):p["initial_override"] for p in pose_proof
                                if p["initial_override"] is not None},
        damage_context=damage_context(snapshot, battle),
        **context,
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
    recipient_input = read(args.recipient_input) if args.recipient_input else None
    if recipient_input is not None and (len(selected) != 1 or "enemy" not in selected[0]):
        raise ValueError("recipient input requires one enemy action")
    if args.resume_ready and len(selected) != 1:
        raise ValueError("a retained ready checkpoint belongs to exactly one case")
    if args.resume_warm and (len(selected) != 1 or args.resume_ready):
        raise ValueError("a retained partial entry belongs to one party case and cannot also be ready")
    if args.resume_initialized and (len(selected) != 1 or "enemy" not in selected[0] or
                                    args.resume_ready or args.resume_warm or args.resume_enemy_idle):
        raise ValueError("a retained initialized entry belongs to one enemy case")
    if args.resume_enemy_idle and (len(selected) != 1 or "enemy" not in selected[0] or
                                  args.resume_ready or args.resume_warm):
        raise ValueError("a retained idle encounter belongs to one enemy case")
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
            entry_path = destination / "entry.s01"
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
                    entry = State(destination / "entry.s01")
                    for character in record["party"]:
                        gauge = 100 if generated.get("saved_overlimit") and character == case["character"] else 0
                        entry.write(battle_artes.member(entry, character) + battle_artes.GAUGE, "B", gauge)
                    entry.save(destination / "entry.s01")
                    record["entry_sha256"] = digest(destination / "entry.s01")
                    record["assistance"] = None
                    write(destination / "fixture.json", record)
                else:
                    if enemy_manifest is None:
                        enemy_manifest = build_catalogue(args.packages, assets, enemy_artes.MANIFEST.with_name("battle-formations.json"))
                        enemy_manifest["fixture_defaults"]["party_hp"] = 9999
                        write(output / "enemy-catalogue.json", enemy_manifest)
                    source_id = f"enemy{case['enemy']:03}:row{case['action_row']:02}"
                    hp_percent = case["definition"]["requirements"]["hp_percent"]
                    hp_percent = 100 if hp_percent is None else hp_percent
                    if not 1 <= hp_percent <= 100:
                        raise ValueError("selected row has no supported living HP fixture")
                    options = SimpleNamespace(case=[source_id], enemy=None, output=destination,
                        capture_vis=args.max_vis, manifest=output / "enemy-catalogue.json",
                        difficulty=case.get("contact_fixture", {}).get("difficulty"),
                        formation=case.get("contact_fixture", {}).get("formation"),
                        party_minimum=case.get("contact_fixture", {}).get("party_minimum"),
                        images=False, retain_states=True, guard=False, enemy_x=100.0, enemy_hp_percent=hp_percent)
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
                preparation_watch, budget = observer_plan(slots)
                # Entry preparation needs actor state, not effect-parent unions.
                # The complete typed pool observer is installed after ready.
                preparation_watch = {address: name for address, name in preparation_watch.items()
                                     if "_parent_" not in name}
                budget.update(packet_budget(preparation_watch))
                if worker is not None and (slots != watched_slots or
                    watch != preparation_watch or
                    digest(destination / "input.dtm") != worker.metadata["movie_sha256"]):
                    worker.close(complete=False)
                    worker = None
                if worker is None:
                    marker = current_marker
                    watch = preparation_watch
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
                    if args.resume_initialized or args.resume_enemy_idle:
                        source = (args.resume_initialized or args.resume_enemy_idle).resolve()
                        if args.resume_initialized:
                            retained = read(source.parent / "ready.json")
                            if digest(source) != retained["state_sha256"] or digest(Path(str(source) + ".dtm")) != retained["movie_sha256"]:
                                raise ValueError("retained initialized state/movie differs from its receipt")
                        for suffix in ("", ".dtm"):
                            shutil.copyfile(Path(str(source) + suffix), destination / ("initialized.s01" + suffix))
                        record["resume_enemy_idle" if args.resume_enemy_idle else "resume_initialized"] = pin(source)
                        write(destination / "fixture.json", record)
                        base, initialized = enemy_artes.initialized_state(destination / "initialized.s01", destination, record,
                                                                         idle=args.resume_enemy_idle is not None)
                        if args.resume_enemy_idle:
                            poses, _ = ready_models(base, initialized["battle"])
                            if not all(pose["clip"] == 0 and pose["repeat"] for pose in poses):
                                raise ValueError("retained idle encounter has an unfinished body pose")
                            retained_setup = source.parent / "setup.json"
                            entry_path = enemy_artes.pinned(read(retained_setup)["entry_state"])
                            record["resume_context"] = pin(retained_setup)
                            write(destination / "fixture.json", record)
                    else:
                        base, initialized = enemy_artes.ready(worker, destination, record)
                    initialized_path = destination / "initialized.s01"
                    if case.get("contact_fixture", {}).get("settle_entry") and not initialized.get("settled_idle"):
                        initialized_path, (base, initialized) = settle_enemy_entry(
                            worker, destination, record, base, initialized, args.max_vis,
                            minimum_gameplay_tick=case["contact_fixture"].get("minimum_gameplay_tick", 0))
                    record["initialized_state"] = str(initialized_path)
                    child = destination / "row"
                    child.mkdir()
                    chosen = record["cases"][0]
                    state_path = enemy_artes.prepare_row(base, child, record, initialized, chosen)
                    fixture = read(child / "fixture.json")
                    geometry = (None if args.resume_enemy_idle else
                                arrange_enemy_contact(base, state_path, initialized, case, assets))
                    if geometry is not None:
                        write(child / "contact-placement.json", geometry)
                    placement = enemy_placement_proof(base, state_path, initialized, assets)
                    write(child / "placement.json", placement)
                    battle = inspect(State(state_path), battle=True)["battle"]
                    owner, target = initialized["actor"]["slot"], 0
                    expected = ({"selected": fixture["expected"]["selected_row_pointer"]}
                        if case["context"] == "departure" else
                        {"selected": fixture["expected"]["selected_row_pointer"], "native": chosen["native"]}
                        if chosen["kind"] == "common_spell" else dict(selected=fixture["expected"]["selected_row_pointer"],
                            animation=fixture["expected"]["animation_pointer"], commands=fixture["expected"]["command_pointer"]))
                    inputs, assistance = dict(button="neutral"), dict(selection=fixture["changes"],
                        contact_placement=geometry, placement=placement)
                    if fixture.get("tp_budget") is not None:
                        assistance["tp_budget"] = fixture["tp_budget"]
                    enemy_ids = [p["enemy"] for p in record["packages"]]
                    del base
                state = State(state_path)
                if "enemy" in case:
                    assistance["prerequisites"] = enemy_prerequisites.prepare(
                        state, battle, initialized, case, assets)
                manual_opponents(state, battle, owner)
                recipient_specification = enemy_prerequisites.recipient_input(
                    case, recipient_input or case.get("contact_fixture", {}).get("recipient_input"),
                    assistance.get("prerequisites") if "enemy" in case else None)
                recipient = (prepare_recipient_input(state, state_path, battle, recipient_specification,
                    read(assets / "game/techniques.json")["learning"], owner=owner) if recipient_specification is not None else None)
                if recipient is not None and recipient["mode"] in ("attack", "stick"):
                    recipient_actor = next(a for a in battle["actors"] if a["slot"] == target)
                    input_target = recipient.get("target_slot", owner)
                    selected_actor = next(a for a in battle["actors"] if a["slot"] == input_target)
                    state.write(recipient_actor["address"] + 0x1990, "I", selected_actor["address"])
                if "enemy" in case:
                    assistance["target_normalization"] = single_action_fixture.normalize_fixture_targets(
                        state, battle)
                if args.offense is not None:
                    actor = next(a for a in battle["actors"] if a["slot"] == owner)
                    for offset in (0x2c, 0x2e, 0x32):
                        state.write(actor["vitals_address"] + offset, "H", args.offense)
                fixture_changes = list(state.changes)
                state.save(state_path)
                if "enemy" in case and geometry is not None and initialized.get("settled_idle"):
                    state, assistance["placement_refresh"] = refresh_enemy_placement(
                        worker, destination, record, state, state_path,
                        inspect(state, battle=True)["battle"], owner)
                battle = inspect(state, battle=True)["battle"]
                assistance = dict(preparation=assistance, neutral_and_offense_changes=fixture_changes,
                                  offensive_stat_preset=args.offense)
                banks = effect_banks(state, assets, args.packages, enemy_ids, args.usual, destination / "content-evidence")
                setup = setup_record(case, state_path, battle, owner, target, assistance, inputs,
                    entry_path,
                    read(assets / "game/techniques.json")["learning"])
                if "character" in case and not args.resume_ready:
                    warm_path = destination / "warm.s01"
                    warm = State(warm_path)
                    warm_battle = inspect(warm, battle=True)["battle"]
                    if any(warm_battle[key] != battle[key] for key in
                           ("presentation_tick", "combat_tick")):
                        raise ValueError("ready condition history advanced during fixture placement")
                    visits, proof = effect_entry.completed_condition_visits(warm, warm_battle, banks)
                    if visits:
                        setup["ready_condition_visits"] = visits
                        setup["ready_condition_history"] = dict(snapshot=pin(warm_path), observations=proof)
                    del warm
                if "enemy" in case and assistance["preparation"].get("prerequisites", {}).get("defense_deployment"):
                    setup["defense_deployed"] = False
                ready_stage_effect(state, assets, setup, banks, destination / "content-evidence")
                register_effect_inputs(setup, banks)
                setup["detached_contacts"] = detached_contact_bindings(state, args.usual,
                    [args.packages / f"{enemy:03}.bin" for enemy in enemy_ids])
                if recipient is not None:
                    setup["recipient_input"] = recipient
                    setup["opponent_control"].update(mode="manual", port=3,
                        scope="Opponents are manual; the declared recipient receives controller zero input.")
                setup["common_effect_bank"] = next(b["source_sha256"] for b in banks.values()
                    if b["entry"] == "battle/effects/common/effects.json")
                if "character" in case:
                    catalogue = "game/techniques.json"
                else:
                    current = ContentSnapshot(assets, destination / "content-evidence")
                    enemy = current.resolve("battle/enemies/mappings.json", case["enemy"], "enemy.json")
                    if enemy is None:
                        raise ValueError("unmapped observed enemy")
                    catalogue = current.document(enemy, ("rules", "artwork", "effects"))["rules"]
                setup["catalogue"] = pin(assets / catalogue)
                setup["knockdown_impacts"] = True
                object_pool = fixed_particle_slots(state, battle["pool"], OBJECT_POOL_CAPACITY)
                owner_bones = ready_owner_bones(state, battle, banks)
                model_joint_plan = effect_model_joints.prepare(
                    dict(case, native=expected.get("native", case.get("native"))), assets,
                    args.usual, destination / "content-evidence")
                del state
                gc.collect()
                particle_addresses = None
                execution_watch, execution_budget = observer_plan(slots, setup["initial_battle"]["pool"],
                    args.observer_capacities, particle_transforms=not args.sprite_only,
                    particle_addresses=particle_addresses,
                    secondary_slots=[b["source_slot"] for b in setup["secondary_bodies"]],
                    knockdown_impacts=True, contact_slot=owner,
                    object_addresses=object_pool["addresses"],
                    owner_bones=owner_bones, model_joint_plan=model_joint_plan)
                execution_budget["object_pool"] = {key: value for key, value in object_pool.items() if key != "addresses"}
                setup["observer"] = execution_budget
                write(destination / "setup.json", setup)
                write(destination / "execution-locations.json", execution_watch)
                write(destination / "execution-observer.json", dict(execution_budget,
                    initialized_pool=setup["initial_battle"]["pool"],
                    scope="Read every verified object-pool slot directly; retain complete typed live lists, parent bindings and observed projectile rules."))
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
                        disc_sha256=profile["inputs"]["disc_sha256"], capture_images=False,
                        observation_projector=object_pool_projection(object_pool["addresses"], battle["pool"]))
                capture = destination / "capture"
                def completed_tail(captured):
                    assessment = normalize(case, captured, setup, banks, expected, None)["assessment"]
                    return assessment["retirement_complete"] and assessment["terminal_complete"]
                completed = completion_observer(owner, expected, completed_tail,
                    particle_addresses=particle_addresses, departure=case["context"] == "departure")
                if "enemy" in case:
                    prerequisites = setup["assistance"]["preparation"].get("prerequisites")
                    preparation = ({"prerequisites": prerequisites}
                                   if prerequisites and prerequisites.get("owner_control") else {})
                    single_action_fixture.capture(worker, state_path, capture,
                        record["fixture"]["case_marker"], args.max_vis, owner, expected, completed,
                        departure=case["context"] == "departure", **preparation)
                else:
                    worker.observe_until(state_path, capture, record["fixture"]["case_marker"], args.max_vis, completed)
                worker.snapshot(destination / "terminal.s01")
                rows = read(capture / "observations.json")
                try:
                    banks.update(stored_effect_banks(dict(case, native=expected.get("native", case.get("native"))),
                        rows, setup, destination / "terminal.s01", assets, args.usual, destination / "content-evidence"))
                except (OSError, ValueError, KeyError, struct.error) as error:
                    setup["stored_scene_observation_error"] = f"stored scene registration unavailable: {error}"
                if case.get("presentation", {}).get("empty_contact_groups"):
                    observe_presentation_contacts(case, setup,
                        [("ready", State(setup["ready_state"]["path"])),
                         ("terminal", State(destination / "terminal.s01"))], assets)
                trace = normalize(case, rows, setup, banks, expected, None)
                reaction_inputs.register(trace, assets, args.usual, destination / "content-evidence")
                try:
                    entry_motions.register(trace, destination / "terminal.s01", args.packages,
                        enemy_ids, args.extracted, ROOT / "target/debug/resonance-import")
                except (OSError, ValueError, KeyError, struct.error, subprocess.CalledProcessError) as error:
                    detail = error.stderr.strip() if isinstance(error, subprocess.CalledProcessError) else str(error)
                    trace["incomplete_observations"].append(f"external entry motion setup unavailable: {detail}")
                trace["setup"]["capture"] = pin(capture / "capture.json")
                write(destination / "trace.json", trace)
                from projectile_launches import register as register_projectile_launches
                trace = register_projectile_launches(destination / "trace.json", assets, args.usual, args.packages)
                from effect_pitch import register as register_effect_pitch
                trace = register_effect_pitch(destination / "trace.json", assets, args.usual)
                outcomes.append(dict(case=case["id"], trace=pin(destination / "trace.json"), fixture_complete=True))
            except Exception as error:
                destination.mkdir(parents=True, exist_ok=True)
                failure = dict(case=case["id"], setup={}, frames=[], events=[],
                    assessment=dict(outcome=case["context"], error=str(error), traceback=traceback.format_exc()),
                    incomplete_observations=[str(error)])
                write(destination / "trace.json", failure)
                outcomes.append(dict(case=case["id"], fixture_complete=False, error=str(error)))
                # Keep failures local to one case. A broken worker cannot be
                # reused, but independent cases can start a fresh one.
                if worker is not None and (worker.process.poll() is not None or worker.watcher.error or not worker.paused):
                    worker.close(complete=False)
                    worker = None
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
    parser.add_argument("--context", choices=("attack", "support", "revival", "aerial", "counter", "capture", "departure", "movement", "presentation"))
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--max-vis", type=int, default=520)
    parser.add_argument("--observer-capacities", nargs=5, type=int,
                        metavar=("PROJECTILES", "PROGRAMS", "PARTICLES", "ALTERNATES", "CONTACTS"),
                        help="explicit per-list execution capacities; oversized packets fail")
    parser.add_argument("--sprite-only", action="store_true",
                        help="omit cosmetic sprite transforms; any observed effect model remains incomplete")
    parser.add_argument("--observe-knockdown-impacts", action="store_true",
                        help="observe landing impulse consumption and its separate nonlethal damage")
    parser.add_argument("--offense", type=int, help="shared initialized slash/thrust/intelligence fixture value")
    parser.add_argument("--recipient-input", type=Path,
                        help="bounded physical recipient input for an enemy comparison")
    parser.add_argument("--resume-ready", type=Path,
                        help="reuse a retained same-case idle checkpoint/movie after a pre-playback adapter failure")
    parser.add_argument("--resume-warm", type=Path,
                        help="continue a retained same-case partial entry before camera initialization")
    parser.add_argument("--resume-initialized", type=Path,
                        help="reuse a verified enemy entry before its first combat choice")
    parser.add_argument("--resume-enemy-idle", type=Path,
                        help="reuse an idle enemy encounter with its actual positions, vitals and clocks")
    parser.add_argument("--packages", type=Path, default=ROOT / "local/battle-actions-recovery-20261011/input")
    parser.add_argument("--extracted", type=Path, default=ROOT / "local/extracted/disc1",
                        help="authored party bodies for observed external entry motions")
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
