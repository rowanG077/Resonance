"""Bounded, read-only observations through Dolphin 2606's local GDB socket.

This is oracle evidence, never asset cooking or runtime input. Execute
breakpoints are Dolphin debugger breakpoints: no game instructions or memory
are rewritten. Debugger timing remains diagnostic until checked against an
ordinary replay.
"""
import json
from pathlib import Path
import socket
import struct
import time


BREAKPOINTS = {
    0x800B0654: "save_check",
    0x8006EEF4: "logo",
    0x8007D344: "movie_update",
    0x8006FF90: "present",
}


class Remote:
    def __init__(self, connection, deadline):
        self.connection = connection
        self.deadline = deadline

    def read(self, size):
        result = bytearray()
        while len(result) < size:
            remaining = self.deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("oracle trace exceeded its deadline")
            self.connection.settimeout(remaining)
            chunk = self.connection.recv(size - len(result))
            if not chunk:
                raise RuntimeError("Dolphin disconnected during oracle trace")
            result.extend(chunk)
        return bytes(result)

    def send(self, command):
        body = command.encode("ascii")
        self.connection.sendall(b"$" + body + f"#{sum(body) & 255:02x}".encode("ascii"))

    def receive(self):
        while (byte := self.read(1)) != b"$":
            if byte != b"+":
                raise RuntimeError(f"unexpected GDB packet prefix {byte!r}")
        body = bytearray()
        while (byte := self.read(1)) != b"#":
            body.extend(byte)
            if len(body) > 10000:
                raise RuntimeError("oversized GDB reply")
        checksum = int(self.read(2), 16)
        if checksum != sum(body) & 255:
            raise RuntimeError("GDB reply checksum mismatch")
        self.connection.sendall(b"+")
        return body.decode("ascii")

    def query(self, command):
        self.send(command)
        return self.receive()

    def memory(self, address, size):
        reply = self.query(f"m{address:x},{size:x}")
        if len(reply) != size * 2:
            raise RuntimeError(f"GDB memory read failed at {address:#x}: {reply}")
        return bytes.fromhex(reply)

    def breakpoint(self, address, enabled):
        if self.query(f"{'Z' if enabled else 'z'}1,{address:x},4") != "OK":
            raise RuntimeError(f"Dolphin rejected breakpoint {address:#x}")


def observe(remote):
    pc = int(remote.query("p40"), 16)
    lr = int(remote.query("p43"), 16)
    if pc not in BREAKPOINTS:
        raise RuntimeError(f"unexpected startup breakpoint PC {pc:#x}")
    globals_ = remote.memory(0x8035A628, 0x13C)
    save = remote.memory(0x80221CF8, 0x50)
    word = lambda address: struct.unpack_from(">I", globals_, address - 0x8035A628)[0]
    float_ = lambda address: struct.unpack_from(">f", globals_, address - 0x8035A628)[0]
    return {
        "function": BREAKPOINTS[pc], "pc": f"0x{pc:08x}", "lr": f"0x{lr:08x}",
        "presentation_counter": word(0x8035A628),
        "state_flags": struct.unpack_from(">H", globals_, 0x8035A762 - 0x8035A628)[0],
        "logo": {
            "phase": word(0x8035A634), "tick": word(0x8035A630),
            "next_phase": word(0x8035A638), "alpha": word(0x8035A644),
            "alpha_step": struct.unpack_from(">i", globals_, 0x18)[0],
        },
        "movie": {
            "presented_frames": word(0x8035A714),
            "overlay_alpha": float_(0x8035A6D8),
            "overlay_rate": float_(0x8035A6D4),
        },
        "save_check": {
            "mode": save[0x13], "screen": struct.unpack_from(">h", save, 0x14)[0],
            "phase": save[0x1F], "delay": save[0x1E],
            "pending": struct.unpack_from(">H", save, 0x1C)[0],
            "operation": struct.unpack_from(">h", save, 0x1A)[0],
            "slot": save[0x0C],
        },
    }


def observe_distance(remote, index):
    """Observe the original SDK length call and its return, without changing input."""
    if int(remote.query("p40"), 16) != 0x800FE6F8:
        raise RuntimeError("unexpected distance trace breakpoint")
    address = int(remote.query("p3"), 16)
    caller = int(remote.query("p43"), 16)
    row = {"index": index, "function": "vector_length", "lr": f"{caller:08x}",
           "input_bits": list(struct.unpack(">3I", remote.memory(address, 12))),
           "fpscr": remote.query("p46"),
           "presentation_counter": int.from_bytes(remote.memory(0x8035A628, 4), "big")}
    remote.breakpoint(caller, True)
    if not remote.query("c").startswith("T") or int(remote.query("p40"), 16) != caller:
        raise RuntimeError("Dolphin did not reach the vector length return")
    result = struct.unpack(">d", bytes.fromhex(remote.query("p21")))[0]
    row["result_bits"] = struct.unpack(">I", struct.pack(">f", result))[0]
    remote.breakpoint(caller, False)
    return row


def observe_geometry(remote, index, text):
    """Trace only 3C068..3C1D4/3D698; no hit admission/damage is inferred."""
    if int(remote.query("p40"), 16) != text + 0x3C068:
        raise RuntimeError("unexpected battle geometry breakpoint")
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    vector = lambda address: list(struct.unpack(">3I", remote.memory(address, 12)))
    owner, target, shape, point = reg(16), reg(26), reg(21), reg(23)
    bone_flags = int.from_bytes(remote.memory(word(target + 0x1A44) + point * 2, 2), "big")
    row = {"index": index, "function": "hurt_shape", "point": point,
           "shape": remote.memory(shape + 8, 1)[0],
           "radius_bits": word(shape), "height_bits": word(shape + 4),
           "ring_width_bits": word(shape + 0x14),
           "center_bits": vector(reg(15) + 0x14), "point_bits": vector(reg(1) + 180),
           "attack_scale_bits": word(word(owner + 4) + 0x84),
           "target_scale_bits": word(word(target + 4) + 0x84),
           "hurt_radius": (bone_flags & 15) * 10,
           "target_y_bits": word(target + 0x18C4),
           "presentation_counter": word(0x8035A628)}
    accepted, rejected = text + 0x3C1D4, text + 0x3D698
    for address in [accepted, rejected]:
        remote.breakpoint(address, True)
    if not remote.query("c").startswith("T") or reg(64) not in [accepted, rejected]:
        raise RuntimeError("Dolphin did not reach a battle geometry branch")
    row["overlaps"] = reg(64) == accepted
    for address in [accepted, rejected]:
        remote.breakpoint(address, False)
    return row


def model_tracks(remote, model):
    result = []
    address = int.from_bytes(remote.memory(model + 0x5FC, 4), "big")
    while address:
        if len(result) == 16 or address in [r["address"] for r in result]:
            raise RuntimeError("invalid original model track list")
        data = remote.memory(address, 52)
        replacement = int.from_bytes(data[48:52], "big")
        result.append({"address": address, "data": data.hex(),
                       "replacement": remote.memory(replacement, 52).hex()
                       if replacement and int.from_bytes(data[28:32], "big") & 0x80000000 else None})
        address = int.from_bytes(data[44:48], "big")
    return result


def damage_actor(remote, address):
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    profile, vitals = word(address + 4), word(address + 8)
    action = word(address + 0x104)
    return {"address": address, "vitals": remote.memory(vitals + 0x1C, 0x20).hex(),
            "profile": remote.memory(profile, 0xB8).hex(),
            "attack_elements": list(remote.memory(address + 0x1E2, 3)),
            "body_motion": remote.memory(address + 0x1046, 1)[0],
            "model_tracks": model_tracks(remote, word(address + 0x2B0)),
            "braking_bits": word(address + 0x18B0),
            "action_rule": remote.memory(action, 0x44).hex() if action else None,
            "state": {f"{start:x}": remote.memory(address + start, length).hex()
                      for start, length in [(0x1AC, 0x30), (0x280, 0x30),
                                            (0x106C, 0x60), (0x1180, 4), (0x1534, 2),
                                            (0x18C0, 0xB0), (0x1974, 8), (0x19CC, 0x14),
                                            (0x1A10, 8), (0x1AA8, 4)]}}


def observe_damage(remote, index, text, pool):
    """Record the original resolver's inputs, HP changes and shared RNG draws."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    actor = lambda address: damage_actor(remote, address)

    if reg(64) != text + 0x61578:
        raise RuntimeError("unexpected battle damage breakpoint")
    owner, target, rule, shape, descriptor, power = [reg(i) for i in range(3, 9)]
    caller = reg(67)
    row = {"index": index, "function": "damage", "owner": actor(owner),
           "target": actor(target), "rule": remote.memory(rule, 28).hex(),
           "shape": remote.memory(shape, 24).hex(), "power": power & 0xFFFF,
           "hit_direction_bits": list(struct.unpack(">3I", remote.memory(descriptor + 0x1C, 12))),
           "random_before": word(pool + 0x7A670),
           "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628),
           "battle_flags": remote.memory(pool + 0x15710, 0x1C).hex()}
    remote.breakpoint(caller, True)
    remote.breakpoint(text + 0x20704, True)
    if not remote.query("c").startswith("T") or reg(64) != text + 0x20704:
        raise RuntimeError("Dolphin did not reach the damage element selection")
    element_return = reg(67)
    remote.breakpoint(text + 0x20704, False)
    remote.breakpoint(element_return, True)
    if not remote.query("c").startswith("T") or reg(64) != element_return:
        raise RuntimeError("Dolphin did not return from damage element selection")
    row["selected_element"] = reg(3)
    remote.breakpoint(element_return, False)
    if not remote.query("c").startswith("T") or reg(64) != caller:
        raise RuntimeError("Dolphin did not reach the battle damage return")
    row.update(result=reg(3), amount=word(descriptor + 0x34),
               hp_after=word(word(target + 8) + 0x24),
               target_after=actor(target),
               random_after=word(pool + 0x7A670))
    remote.breakpoint(caller, False)
    return row


def observe_recovery(remote, index, text, pool, guarding=False, stunning=False):
    """Observe a complete actor recovery callback, including its continuation."""
    function, offset = (("stun", 0x2E848) if stunning else
                        ("guard", 0x2F284) if guarding else ("hurt", 0x2FB24))
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    vector = lambda a, n: list(struct.unpack(f">{n}I", remote.memory(a, 4*n)))
    if reg(64) != text + offset:
        raise RuntimeError(f"unexpected {function} controller breakpoint")
    actor, caller = reg(3), reg(67)
    profile = word(actor + 4)
    def state():
        state = {"activity": remote.memory(actor + 0x1AC, 1)[0],
                "guard_mode": remote.memory(actor + 0x1B1, 1)[0],
                "guard_state": remote.memory(actor + 0x282, 1)[0],
                "body_motion": remote.memory(actor + 0x1046, 1)[0],
                "auto_guard_chance": remote.memory(actor + 0x1080, 1)[0],
                "hitstun": int.from_bytes(remote.memory(actor + 0x1BE, 2), "big", signed=True),
                "hit_stop": remote.memory(actor + 0x10B0, 1)[0],
                "delay": remote.memory(actor + 0x1535, 1)[0],
                "kind": remote.memory(actor + 0x19CF, 1)[0],
                "position_bits": vector(actor + 0x18C0, 3),
                "origin_bits": vector(actor + 0x18B4, 3),
                "velocity_bits": vector(actor + 0x1908, 6),
                "pending_bits": vector(actor + 0x1974, 2),
                "braking_bits": word(actor + 0x18B0),
                "combo_hits": word(actor + 0x2A4),
                "combo_damage": word(actor + 0x2A8)}
        if stunning:
            particle = word(actor + 0x19AC)
            state.update(pulse=remote.memory(actor + 0x1AAF, 1)[0],
                         retained_count=remote.memory(actor + 0x19CE, 1)[0],
                         model_tracks=model_tracks(remote, word(actor + 0x2B0)),
                         particle={"address": particle,
                                   "declaration": remote.memory(particle + 0x28, 352).hex()}
                         if particle else None)
        return state
    row = {"index": index, "function": function, "actor": actor, "caller": caller,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "profile": remote.memory(profile, 0xB8).hex(),
           "guard_preference": remote.memory(word(actor + 8) + 2, 1)[0],
           "actor_flags": remote.memory(actor + 0x107C, 4).hex(),
           "captor": word(actor + 0x19D4), "unison": remote.memory(pool + 0x15728, 1)[0],
           "direction_bits": vector(actor + 0x1944, 3),
           "root_bits": vector(actor + 0x30C, 6),
           "root_enabled": bool(remote.memory(actor + 0x324, 1)[0] & 0x40),
           "random_before": word(pool + 0x7A670), "before": state()}
    if guarding:
        row["selection"] = {"range_bits": vector(actor + 0x1168, 2),
                            "distance_bits": word(actor + 0x18AC),
                            "retreat_bits": word(actor + 0x10A4),
                            "flags": remote.memory(actor + 0x1180, 4).hex()}
    remote.breakpoint(caller, True)
    if stunning:
        row["heading_bits"] = word(actor + 0x18D0)
        row["sounds"] = []
        remote.breakpoint(text + 0x9E38, True)
    while True:
        if not remote.query("c").startswith("T"):
            raise RuntimeError(f"Dolphin did not reach the {function} controller return")
        if reg(64) == caller:
            break
        if not stunning or reg(64) != text + 0x9E38:
            raise RuntimeError(f"unexpected {function} controller breakpoint")
        row["sounds"].append({"owner": reg(3), "index": reg(4), "priority": reg(5)})
        # The original handler emits at most one periodic sound per visit.
        remote.breakpoint(text + 0x9E38, False)
    row.update(after=state(), random_after=word(pool + 0x7A670))
    remote.breakpoint(caller, False)
    if stunning:
        remote.breakpoint(text + 0x9E38, False)
    return row


def observe_reaction(remote, index, text, pool):
    """Read the damage wrapper and complete contact return, without input changes."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    if reg(64) != text + 0x63470:
        raise RuntimeError("unexpected battle reaction breakpoint")
    descriptor, rule, shape, owner, target, power = [reg(i) for i in range(3, 9)]
    caller = reg(67)
    if caller != text + 0x3C398:
        raise RuntimeError("unexpected battle reaction caller")
    # 3BDF8 saves LR at caller SP + 4 before its 336-byte frame allocation.
    contact_caller = word(reg(1) + 340)
    row = {"index": index, "function": "reaction", "owner": damage_actor(remote, owner),
           "target": damage_actor(remote, target), "rule": remote.memory(rule, 28).hex(),
           "shape": remote.memory(shape, 24).hex(), "power": power & 0xFFFF,
           "incoming_direction_bits": list(struct.unpack(">3I", remote.memory(descriptor + 0x1C, 12))),
           "recoil_direction_bits": list(struct.unpack(">3I", remote.memory(descriptor + 0x28, 12))),
           "direction_mode": remote.memory(descriptor + 0x1A, 1)[0],
           "contact_position_bits": list(struct.unpack(">3I", remote.memory(reg(15) + 0x14, 12))),
           "random_before": word(pool + 0x7A670), "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628),
           "battle_flags": remote.memory(pool + 0x15710, 0x1C).hex()}
    row["tp_requests"] = []
    tp_entry = text + 0xA9B4
    for address, name in [(caller, "wrapper"), (contact_caller, "contact")]:
        remote.breakpoint(address, True)
        if name == "contact":
            remote.breakpoint(tp_entry, True)
        while True:
            if not remote.query("c").startswith("T"):
                raise RuntimeError(f"Dolphin did not reach the reaction {name} return")
            if reg(64) == address:
                break
            if reg(64) != tp_entry or len(row["tp_requests"]) == 16:
                raise RuntimeError("unexpected contact TP recovery breakpoint")
            row["tp_requests"].append({"caller": reg(67), "owner": damage_actor(remote, reg(3))})
        row[name] = {"result": reg(3), "amount": word(descriptor + 0x34),
                     "owner": damage_actor(remote, owner),
                     "target": damage_actor(remote, target),
                     "random": word(pool + 0x7A670)}
        remote.breakpoint(address, False)
        if name == "contact":
            remote.breakpoint(tp_entry, False)
    return row


def melee_pose(remote, actor):
    """Held original body/weapon matrices at contact submission, without sampling them."""
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")

    def model(address):
        resource = word(address)
        count = int.from_bytes(remote.memory(resource + 6, 2), "big")
        if not 1 <= count <= 256:
            raise RuntimeError("invalid battle pose bone count")
        nodes = struct.unpack(f">{count}I", remote.memory(word(resource + 0x18), count * 4))
        transforms = [word(node + 0x14) for node in nodes]
        drawing = [list(struct.unpack(">12I", remote.memory(transform + 0x18, 48)))
                   if transform else None for transform in transforms]
        matrices = [list(struct.unpack(">12I", remote.memory(word(node + 0x84), 48)))
                    for node in nodes]
        return {"resource": resource, "tracks": model_tracks(remote, address),
                "translation_bits": list(struct.unpack(">3I", remote.memory(address + 0xC, 12))),
                "rotation_bits": list(struct.unpack(">3I", remote.memory(address + 0x18, 12))),
                "scale_bits": list(struct.unpack(">3I", remote.memory(address + 0x34, 12))),
                "flags": remote.memory(address + 0x64, 1)[0],
                "matrices": matrices, "drawing_matrices": drawing}

    profile = word(actor + 4)
    count = remote.memory(profile + 0x1E4, 1)[0]
    if count > 8:
        raise RuntimeError("invalid battle weapon count")
    weapons = []
    base = word(actor + 0x1560)
    for index in range(count):
        part = base + index * 0xDA0
        weapons.append({"bone": remote.memory(part + 0xD78, 1)[0],
                        "model": model(part)})
    return {"clip": remote.memory(actor + 0x1046, 1)[0],
            "position_bits": list(struct.unpack(">3I", remote.memory(actor + 0x18C0, 12))),
            "heading_bits": word(actor + 0x18D0), "scale_bits": word(profile + 0x84),
            "body": model(actor + 0x2C0), "weapons": weapons}


def action_stream_state(remote, actor):
    """The four original counters and held model flag; reading never advances them."""
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    clocks = struct.unpack(">3h", remote.memory(actor + 0x1054, 6))
    track = word(word(actor + 0x2B0) + 0x5FC)
    for _ in range(16):
        if not track or remote.memory(track + 6, 2) == b"\x00\x06":
            break
        track = word(track + 0x2C)
    else:
        raise RuntimeError("invalid action model track list")
    return {"action_clock": int.from_bytes(remote.memory(actor + 0x1BE, 2), "big", signed=True),
            "command_clock": clocks[0], "hit_clock": clocks[1], "animation_clock": clocks[2],
            "animation_index": int.from_bytes(remote.memory(actor + 0x105A, 1), "big", signed=True),
            "clip": remote.memory(actor + 0x1046, 1)[0],
            "hit_stop": remote.memory(actor + 0x10B0, 1)[0],
            "model_flags": remote.memory(track + 0x20, 1)[0] if track else None}


def observe_animation_row(remote, index, text, pool):
    """Initial 2BC3C binding and subsequent 2B910 rows have different clock effects."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    actor, caller = reg(3), reg(67)
    initializing = reg(64) == text + 0x2BC3C
    before = action_stream_state(remote, actor)
    cursor = reg(4) if initializing else word(actor + 0x104C) + before["animation_index"] * 12
    row = {"index": index, "function": "animation_start" if initializing else "animation_row",
           "actor": actor, "side": remote.memory(actor + 0x107C, 1)[0] >> 7,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "record": remote.memory(cursor, 12).hex(), "before": before}
    remote.breakpoint(caller, True)
    if not remote.query("c").startswith("T") or reg(64) != caller:
        raise RuntimeError("Dolphin did not reach the animation row return")
    row["after"] = action_stream_state(remote, actor)
    if not initializing:
        row["result"] = reg(3)
    remote.breakpoint(caller, False)
    return row


def observe_melee(remote, index, text, pool):
    """Observe one original hit-stream update and the contacts it submits."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    if reg(64) == text + 0x2C8B0:
        return observe_commands(remote, index, text, pool)
    if reg(64) in (text + 0x2B910, text + 0x2BC3C):
        return observe_animation_row(remote, index, text, pool)
    if reg(64) != text + 0x2D564:
        raise RuntimeError("unexpected melee trace breakpoint")
    actor, caller = reg(3), reg(67)
    side = remote.memory(actor + 0x107C, 1)[0] >> 7
    table = pool + 0x7DC0C
    count = lambda: int.from_bytes(remote.memory(table + 0xA00 + side * 2, 2), "big")
    before = count()
    cursor = word(actor + 0x110)
    record = remote.memory(cursor, 32)
    row = {"index": index, "function": "melee", "actor": actor, "side": side,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "clock": int.from_bytes(remote.memory(actor + 0x1056, 2), "big", signed=True),
           "record": record.hex(), "cursor": cursor,
           "cache_before": remote.memory(actor + 0x1158, 16).hex(),
           "rule": remote.memory(word(actor + 0x100) + record[0x12] * 28, 28).hex()
                   if record[:2] != b"\xff\xff" else None}
    # The opening case's controlled Lloyd supplies the concrete normal-attack pose.
    if side == 0 and remote.memory(actor + 0x107E, 1)[0] >> 4 == 1:
        row["pose"] = melee_pose(remote, actor)
    remote.breakpoint(caller, True)
    if not remote.query("c").startswith("T") or reg(64) != caller:
        raise RuntimeError("Dolphin did not reach the melee return")
    contacts = []
    for slot in range(before, count()):
        entry = remote.memory(table + side * 0x500 + slot * 32, 32)
        descriptor = int.from_bytes(entry[8:12], "big")
        contacts.append({"entry": entry.hex(), "descriptor": remote.memory(descriptor, 56).hex()})
    row.update(clock_after=int.from_bytes(remote.memory(actor + 0x1056, 2), "big", signed=True),
               cursor_after=word(actor + 0x110), cache_after=remote.memory(actor + 0x1158, 16).hex(),
               contacts=contacts)
    remote.breakpoint(caller, False)
    return row


def observe_commands(remote, index, text, pool):
    """Observe command clocks and sound/voice requests, without changing playback."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    actor, caller = reg(3), reg(67)
    row = {"index": index, "function": "commands", "actor": actor,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "character": remote.memory(actor + 0x107E, 1)[0] >> 4,
           "side": remote.memory(actor + 0x107C, 1)[0] >> 7,
           "clip": remote.memory(actor + 0x1046, 1)[0],
           "clock": int.from_bytes(remote.memory(actor + 0x1054, 2), "big", signed=True),
           "random_before": word(pool + 0x7A670), "requests": []}
    sound, voice = text + 0x9E38, text + 0x71E78
    for address in (caller, sound, voice):
        remote.breakpoint(address, True)
    while True:
        if not remote.query("c").startswith("T"):
            raise RuntimeError("Dolphin did not reach the action command return")
        stopped = reg(64)
        if stopped == caller:
            break
        if stopped == sound:
            position = reg(3)
            row["requests"].append({"kind": "sound", "index": reg(4) & 0xFFFF,
                                    "priority": reg(5) & 0xFF,
                                    "screen_position_bits": word(position) if position else None})
        elif stopped == voice:
            row["requests"].append({"kind": "voice", "index": reg(4) & 0xFFFF,
                                    "priority": reg(6) & 0xFF, "mode": reg(7) & 0xFF})
        else:
            raise RuntimeError("unexpected nested action command visit")
    for address in (caller, sound, voice):
        remote.breakpoint(address, False)
    row["clock_after"] = int.from_bytes(remote.memory(actor + 0x1054, 2), "big", signed=True)
    row["random_after"] = word(pool + 0x7A670)
    return row


def model_chains(remote, model):
    """Retained native secondary-chain state; reading does not advance the solver."""
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    address = word(model + 0x61C)
    if not address:
        return None
    resource = word(model)
    bone_count = int.from_bytes(remote.memory(resource + 6, 2), "big")
    if not 1 <= bone_count <= 256:
        raise RuntimeError("invalid secondary model bone count")
    nodes = struct.unpack(f">{bone_count}I", remote.memory(word(resource + 0x18), bone_count * 4))
    matrices = [list(struct.unpack(">12I", remote.memory(word(node + 0x84), 48))) for node in nodes]
    chains, seen = [], set()
    while address:
        if address in seen or len(chains) >= 32:
            raise RuntimeError("invalid model chain list")
        seen.add(address)
        header = remote.memory(address, 20)
        segments, count, flags, _, blend, following, callback = struct.unpack(">IhBBIII", header)
        if not 2 <= count <= 128:
            raise RuntimeError("invalid model chain length")
        chains.append({"address": address, "flags": flags, "blend_bits": blend,
                       "callback": callback, "count": count,
                       "segments": remote.memory(segments, count * 64).hex()})
        address = following
    return {"chains": chains, "matrices": matrices, "flags": remote.memory(model + 0x64, 1)[0],
            "translation_bits": list(struct.unpack(">3I", remote.memory(model + 0xC, 12))),
            "rotation_bits": list(struct.unpack(">3I", remote.memory(model + 0x18, 12))),
            "scale_bits": list(struct.unpack(">3I", remote.memory(model + 0x34, 12))),
            "acceleration_bits": list(struct.unpack(">3I", remote.memory(model + 0x600, 12))),
            "wind_bits": list(struct.unpack(">3I", remote.memory(0x802CAAA0, 12))),
            "floor_enabled": bool(remote.memory(0x8035A60E, 1)[0])}


def observe_motion(remote, index, pool):
    """Record controller clocks and distinct native secondary-solver visits."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    pc = reg(64)
    if pc not in (0x8006D2E0, 0x80069088):
        raise RuntimeError("unexpected motion trace breakpoint")
    model, flags, caller = reg(3), reg(4), reg(67)
    state = model_chains if pc == 0x80069088 else model_tracks
    row = {"index": index, "function": "chains" if pc == 0x80069088 else "motion",
           "model": model, "flags": flags,
           "caller": caller, "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628), "before": state(remote, model)}
    remote.breakpoint(caller, True)
    while True:
        if not remote.query("c").startswith("T"):
            raise RuntimeError("Dolphin did not reach the model return")
        stopped = reg(64)
        if stopped == caller:
            break
        if pc == 0x8006D2E0 and stopped == 0x80069088:
            row.setdefault("chain_visits", []).append(observe_motion(remote, index, pool))
        else:
            raise RuntimeError("unexpected nested model visit")
    row["after"] = state(remote, model)
    remote.breakpoint(caller, False)
    return row


def observe_movement(remote, index, text, pool):
    """Original linear integration/braking inputs and outputs, before arena correction."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    functions = {0x24314: "integrate", 0x244D0: "integrate_brake", 0x24040: "brake"}
    function = functions[reg(64) - text]
    actor, caller = reg(3), reg(67)
    profile = word(actor + 4)
    model = word(actor + 0x2B0)
    track = word(model + 0x5FC)
    for _ in range(16):
        if not track or remote.memory(track + 6, 2) == b"\x00\x06":
            break
        track = word(track + 0x2C)
    else:
        raise RuntimeError("invalid movement model track list")
    def state():
        return {"position_bits": list(struct.unpack(">3I", remote.memory(actor + 0x18C0, 12))),
                "origin_bits": list(struct.unpack(">3I", remote.memory(actor + 0x18B4, 12))),
                "velocity_bits": list(struct.unpack(">6I", remote.memory(actor + 0x1908, 24)))}
    action = word(actor + 0x104)
    row = {"index": index, "function": function, "actor": actor, "caller": caller,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "activity": remote.memory(actor + 0x1AC, 1)[0],
                "guard_mode": remote.memory(actor + 0x1B1, 1)[0],
                "guard_state": remote.memory(actor + 0x282, 1)[0],
                "body_motion": remote.memory(actor + 0x1046, 1)[0],
           "actor_flags": remote.memory(actor + 0x107C, 1)[0],
           "profile_flags": word(profile + 0x5C),
           "profile_shape": int.from_bytes(remote.memory(profile + 0xB4, 2), "big"),
           "action_flags": word(action + 8) if action else 0,
           "braking_bits": word(actor + 0x18B0), "yaw_bits": word(actor + 0x2DC),
           "heading_bits": word(actor + 0x18D0),
           "root_bits": list(struct.unpack(">6I", remote.memory(actor + 0x30C, 24))),
           "root_enabled": bool(remote.memory(actor + 0x324, 1)[0] & 0x40),
           "model_flags": remote.memory(track + 0x20, 1)[0] if track else 0,
           "clip": remote.memory(actor + 0x1046, 1)[0],
           "clip18_present": bool(word(word(actor + 0x1010) + 18 * 20 + 4)),
           "direction_bits": list(struct.unpack(">3I", remote.memory(reg(4), 12)))
                             if function != "brake" else None,
           "before": state()}
    remote.breakpoint(caller, True)
    if not remote.query("c").startswith("T") or reg(64) != caller:
        raise RuntimeError("Dolphin did not reach the movement return")
    row.update(after=state(), result=reg(3) if function != "integrate" else None)
    remote.breakpoint(caller, False)
    return row


def observe_residents(remote, index, text, pool):
    """Observe both source spell slots around the late resident dispatch."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    if reg(64) != text + 0x3A978:
        raise RuntimeError("unexpected resident trace breakpoint")
    caller = reg(67)
    def projectiles():
        # 11FCC/11E8C group3 list. Adjacent resident visits bracket the intervening
        # projectile group; before/after this callback isolates new emissions.
        head = pool + 0x75C40
        address = word(head)
        result, seen = [], set()
        while address != head:
            if address in seen or len(seen) >= 416:
                raise RuntimeError("invalid original projectile group")
            seen.add(address)
            data = remote.memory(address, 0x284)
            if data[0x10] == 7:
                vector = lambda offset: list(struct.unpack_from(">3I", data, offset))
                child_count = data[0xD8]
                if child_count > 12:
                    raise RuntimeError("invalid original projectile child count")
                result.append({"address": address, "mode": data[0x14],
                               "owner": int.from_bytes(data[0x28:0x2C], "big"),
                               "target": int.from_bytes(data[0x2C:0x30], "big"),
                               "flags": int.from_bytes(data[0x30:0x34], "big"),
                               "age": struct.unpack_from(">h", data, 0x278)[0],
                               "lifetime": struct.unpack_from(">h", data, 0x34)[0],
                               "position_bits": vector(0x40),
                               "velocity_bits": vector(0x4C),
                               "acceleration_bits": vector(0x58),
                               "speed_bits": int.from_bytes(data[0x64:0x68], "big"),
                               "target_point_bits": vector(0x148),
                               "heading_bits": int.from_bytes(data[0x260:0x264], "big"),
                               "effects": data[0x84:0x88].hex(),
                               "ground_effect": data[0x36:0x38].hex(),
                               "feedback": data[0x283],
                               "children": list(struct.unpack_from(f">{child_count}I", data, 0xDC))})
            address = int.from_bytes(data[:4], "big")
        return result

    def slots():
        result = []
        for side, capacity in [(0, 4), (1, 8)]:
            count = remote.memory(pool + 0x15948 + side, 1)[0]
            if count > capacity:
                raise RuntimeError("invalid original battle roster")
            for roster in remote.memory(pool + 0x1565C + side * 8, count):
                actor = pool + 0x1CEA0 + (roster + side * 4) * 0x1BA0
                for slot, offset in enumerate([0x16AC, 0x17A4]):
                    data = remote.memory(actor + offset, 0xD8)
                    mode = data[0x35] >> 6
                    table = int.from_bytes(data[:4], "big")
                    result.append({"actor": actor, "side": side, "roster": roster, "slot": slot,
                                   "mode": mode, "phase": data[0x34],
                                   "age": int.from_bytes(data[0x36:0x38], "big", signed=True),
                                   "duration": int.from_bytes(data[0x92:0x94], "big", signed=True),
                                   "technique": int.from_bytes(data[0xD4:0xD6], "big", signed=True),
                                   "retained": data[0x8E], "hp": word(word(actor + 8) + 0x24),
                                   "callback": word(table + data[0x34] * 4) if mode and table else None,
                                   "origin_bits": list(struct.unpack_from(">3I", data, 0x20))})
        return result
    row = {"index": index, "function": "residents", "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628), "suspended": word(pool + 0x159E8) >> 24,
           "before": slots(), "projectiles_before": projectiles()}
    remote.breakpoint(caller, True)
    if not remote.query("c").startswith("T") or reg(64) != caller:
        raise RuntimeError("Dolphin did not reach the resident dispatch return")
    row["after"] = slots()
    row["projectiles_after"] = projectiles()
    remote.breakpoint(caller, False)
    return row


def observe_casting(remote, index, text, pool):
    """Casting initialization/countdown/release, with live model and spell-slot state."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    half = lambda a: int.from_bytes(remote.memory(a, 2), "big", signed=True)
    functions = {0x39974: "initialize", 0x3898C: "countdown", 0x385A0: "release"}
    function = functions[reg(64) - text]
    actor, caller = reg(3), reg(67)
    profile, technique, vitals = word(actor + 4), word(actor + 12), word(actor + 8)

    def state():
        model = word(actor + 0x2B0)
        track = word(model + 0x5FC)
        for _ in range(16):
            if not track or remote.memory(track + 6, 2) == b"\x00\x06":
                break
            track = word(track + 0x2C)
        else:
            raise RuntimeError("invalid casting model track list")
        data = remote.memory(track, 52) if track else None
        replacement = int.from_bytes(data[48:52], "big") if data else 0
        return {"phase": remote.memory(actor + 0x1AC, 8).hex(),
                "remaining": half(actor + 0x1BE), "elapsed": half(actor + 0x1056),
                "initial_remaining": half(actor + 0x1A0E),
                "clip": remote.memory(actor + 0x1046, 1)[0],
                "hit_stop": remote.memory(actor + 0x10B0, 1)[0],
                "hp": word(vitals + 0x24), "tp": half(vitals + 0x2A),
                "primary_mode": remote.memory(actor + 0x16E1, 1)[0] >> 6,
                "primary_phase": remote.memory(actor + 0x16E0, 1)[0],
                "track": data.hex() if data else None,
                "replacement": remote.memory(replacement, 52).hex()
                if replacement and int.from_bytes(data[28:32], "big") & 0x80000000 else None}

    row = {"index": index, "function": function, "actor": actor, "caller": caller,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "actor_flags": remote.memory(actor + 0x107C, 4).hex(),
           "profile": remote.memory(profile, 0xB8).hex(),
           "technique": remote.memory(technique, 0x58).hex(),
           "release_duration_bits": word(word(actor + 0x1010) + 12 * 20 + 8),
           "battle_flags": remote.memory(pool + 0x15710, 0x1C).hex(),
           "countdown_pause": half(pool + 0xD4),
           "random_before": word(pool + 0x7A670), "before": state()}
    row["requests"] = []
    sound, effect, release = (text + offset for offset in (0x9E38, 0x40210, 0x3A89C))
    for address in (caller, sound, effect, release):
        remote.breakpoint(address, True)
    for _ in range(128):
        if not remote.query("c").startswith("T"):
            raise RuntimeError("Dolphin did not reach a casting request or return")
        pc = reg(64)
        if pc == caller:
            break
        request = {"caller": reg(67) - text}
        if pc == sound:
            request.update(kind="sound", index=reg(4), priority=reg(5))
        elif pc == effect:
            scale = struct.unpack(">d", bytes.fromhex(remote.query("p22")))[0]
            request.update(kind="effect", bank=reg(4), member=reg(5), owner=reg(6),
                           target=reg(7), follow=reg(8), late=reg(10),
                           scale_bits=struct.unpack(">I", struct.pack(">f", scale))[0])
        elif pc == release:
            request.update(kind="release", owner=reg(3), technique=reg(5))
        else:
            raise RuntimeError("unexpected nested casting request")
        row["requests"].append(request)
    else:
        raise RuntimeError("casting request limit exceeded")
    row.update(after=state(), random_after=word(pool + 0x7A670))
    for address in (caller, sound, effect, release):
        remote.breakpoint(address, False)
    return row


def observe_effect_command(remote, text, pool):
    """Read each allocated particle after its command modifiers have completed."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    caller, create = reg(67), text + 0x413B8
    row = {"record": remote.memory(reg(4), 6).hex(),
           "random_before": word(pool + 0x7A670), "particles": [], "sounds": []}
    sound = text + 0x9DC4
    for address in [caller, create, sound]:
        remote.breakpoint(address, True)
    for _ in range(256):
        if not remote.query("c").startswith("T"):
            raise RuntimeError("Dolphin did not reach an effect emission breakpoint")
        if reg(64) == caller:
            break
        if reg(64) == sound:
            row["sounds"].append({"sound": reg(4) & 0xFFFF, "priority": reg(5) & 0xFF,
                                  "position_bits": list(struct.unpack(">3I", remote.memory(reg(3), 12)))})
            continue
        if reg(64) != create:
            raise RuntimeError("unexpected nested effect emission")
        source = remote.memory(reg(4), 352).hex()
        birth_return = reg(67)
        remote.breakpoint(birth_return, True)
        if not remote.query("c").startswith("T") or reg(64) != birth_return:
            raise RuntimeError("Dolphin did not return from particle allocation")
        pointer = reg(3)
        remote.breakpoint(birth_return, False)
        if pointer:
            row["particles"].append({"address": pointer, "source": source})
    else:
        raise RuntimeError("effect emission limit exceeded")
    for particle in row["particles"]:
        pointer = particle["address"]
        particle.update(prepared=remote.memory(pointer + 0x28, 352).hex(),
                        update_group=remote.memory(pointer + 0x11, 1)[0],
                        draw_group=remote.memory(pointer + 0x13, 1)[0],
                        draw_target=word(pointer + 0x24),
                        origin_bits=list(struct.unpack(">3I", remote.memory(pointer + 0x170, 12))),
                        heading_bits=word(pointer + 0x180))
    row["random_after"] = word(pool + 0x7A670)
    for address in [caller, create, sound]:
        remote.breakpoint(address, False)
    return row


def observe_effect(remote, index, text, pool):
    """One original effect timeline visit, including ordered command dispatches."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    half = lambda a: int.from_bytes(remote.memory(a, 2), "big", signed=True)
    context, caller = reg(3), reg(67)
    root = word(context + 0x44)

    def state():
        head = context + 0x54
        repeats, node = [], word(head)
        while node != head:
            if len(repeats) >= 64:
                raise RuntimeError("invalid effect repeat list")
            repeats.append({"record": remote.memory(word(node + 8), 6).hex(),
                            "remaining": half(node + 12), "interval": half(node + 14),
                            "counter": half(node + 16)})
            node = word(node)
        return {"age": half(context + 0x36), "cursor": half(context + 0x8C),
                "repeats": repeats, "random": word(pool + 0x7A670)}

    row = {"index": index, "context": context, "caller": caller - text,
           "root": root, "bank": word(context + 0x38),
           "bank_slot": half(context + 0x90),
           "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628), "before": state(), "commands": []}
    dispatch = text + 0x418B4
    for address in [dispatch, caller]:
        remote.breakpoint(address, True)
    for _ in range(1024):
        if not remote.query("c").startswith("T"):
            raise RuntimeError("Dolphin did not reach an effect breakpoint")
        if reg(64) == caller:
            break
        if reg(64) != dispatch or reg(3) != context:
            raise RuntimeError("unexpected nested effect timeline")
        row["commands"].append(observe_effect_command(remote, text, pool))
    else:
        raise RuntimeError("effect dispatch limit exceeded")
    row.update(after=state(), active=reg(3))
    for address in [dispatch, caller]:
        remote.breakpoint(address, False)
    return row


def observe_stun_roll(remote, index, text, pool):
    """Read the original chance, EX query, random roll and resulting stun entry."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    owner, target, rule = reg(16), reg(26), reg(22)
    profile = word(target + 4)

    def state():
        return {"activity": remote.memory(target + 0x1AC, 1)[0],
                "remaining": int.from_bytes(remote.memory(target + 0x1BE, 2), "big", signed=True),
                "armor": remote.memory(target + 0x1C7, 1)[0],
                "pulse": remote.memory(target + 0x1AAF, 1)[0],
                "retained_count": remote.memory(target + 0x19CE, 1)[0]}

    row = {"index": index, "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628),
           "owner": owner, "target": target,
           "chance": remote.memory(rule + 5, 1)[0],
           "bonus": remote.memory(owner + 0x1534, 1)[0],
           "resistance": remote.memory(profile + 0x2D, 1)[0],
           "immune": bool(word(profile + 0x1C) & 4),
           "shortened": bool(word(profile + 0x18) & 0x100),
           "before": state(), "random_before": word(pool + 0x7A670)}
    for offset in [0x3CFA8, 0x3CFDC, 0x3D01C]:
        address = text + offset
        remote.breakpoint(address, True)
        if not remote.query("c").startswith("T") or reg(64) != address:
            raise RuntimeError("Dolphin did not reach the stun roll boundary")
        if offset == 0x3CFA8:
            row["ex_bonus"] = bool(reg(3))
        elif offset == 0x3CFDC:
            row.update(effective_chance=reg(0), roll=reg(3))
        remote.breakpoint(address, False)
    row.update(after=state(), random_after=word(pool + 0x7A670))
    if row["after"]["activity"] == 15:
        particle = word(target + 0x19AC)
        if particle:
            row["particle"] = {"address": particle, "declaration": remote.memory(particle + 0x28, 352).hex()}
    return row


def observe_particle(remote, index, text, pool):
    """Observe a complete common particle update, including initialization."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    particle, returned = reg(3), reg(67)

    def state():
        return {"data": remote.memory(particle + 0x28, 352).hex(),
                "age": int.from_bytes(remote.memory(particle + 0x278, 2), "big", signed=True),
                "callback": word(particle + 0x14)}

    row = {"index": index, "particle": particle,
           "owner": word(particle + 0x18), "origin_binding": word(particle + 0x1C),
           "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628),
           "random_before": word(pool + 0x7A670), "before": state()}
    remote.breakpoint(returned, True)
    if not remote.query("c").startswith("T") or reg(64) != returned:
        raise RuntimeError("Dolphin did not reach the particle update return")
    row.update(after=state(), random_after=word(pool + 0x7A670))
    remote.breakpoint(returned, False)
    return row


def observe_particle_uv(remote, index, text, pool):
    """Observe only visits with a bound UV stream, through the common clock increment."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    particle = reg(29)  # 403F4 retains self in r29 and its prefix in r31.
    owner = word(particle + 0x18)
    root = word(particle + 0x17C)

    def state():
        data = remote.memory(particle, 0x284)
        row_index, age = data[0x282:0x284]
        return {"uv": list(struct.unpack_from(">4h", data, 0x30)),
                "palettes": [data[0x2B], data[0x2C]],
                "row": row_index, "age": age,
                "scroll": list(struct.unpack_from(">2h", data, 0x27C)),
                "particle_age": struct.unpack_from(">h", data, 0x278)[0]}

    before = state()
    row_index = before["row"] if before["row"] < 128 else before["row"] - 256
    row = {"index": index, "particle": particle, "root": root,
           "kind": remote.memory(particle + 0x28, 1)[0],
           "owner": owner, "hold_uv": bool(owner and remote.memory(owner + 0x1183, 1)[0] & 1),
           "key": remote.memory(root + row_index * 10, 10).hex(),
           "combat_tick": word(pool + 0x15704),
           "presentation_counter": word(0x8035A628),
           "random_before": word(pool + 0x7A670), "before": before}
    # 40E2C precedes stack restoration, after the optional UV clock increment.
    end = text + 0x40E2C
    remote.breakpoint(end, True)
    if not remote.query("c").startswith("T") or reg(64) != end:
        raise RuntimeError("Dolphin did not reach the particle UV return")
    row.update(after=state(), random_after=word(pool + 0x7A670))
    next_index = row["after"]["row"]
    next_index = next_index if next_index < 128 else next_index - 256
    row["next_key"] = remote.memory(root + next_index * 10, 10).hex()
    remote.breakpoint(end, False)
    return row



def observe_voice(remote, index, text, pool):
    """Read pending/playing voice state and the native idle/stop/play calls."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda a: int.from_bytes(remote.memory(a, 4), "big")
    actor, caller, pc = reg(3), reg(67), reg(64)
    function = {0x71674: "dispatch", 0x71D90: "relative", 0x71E78: "absolute"}[pc - text]
    state = lambda: remote.memory(actor + 0x1AB0, 24).hex()
    row = {"index": index, "function": function, "actor": actor,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "caller": caller, "profile_base": word(word(actor + 4) + 0x104),
           "screen_x_bits": word(actor + 0x1A2C), "before": state(), "calls": []}
    technique = word(actor + 0x0C)
    row["casting"] = {
        "character": remote.memory(actor + 0x107E, 1)[0] >> 4,
        "technique": int.from_bytes(remote.memory(technique, 2), "big") if technique else None,
        "remaining": int.from_bytes(remote.memory(actor + 0x1BE, 2), "big", signed=True),
        "last_voice_remaining": int.from_bytes(remote.memory(actor + 0x10BE, 2), "big", signed=True),
        "chant": int.from_bytes(remote.memory(actor + 0x1ABA, 2), "big"),
        "release": int.from_bytes(remote.memory(actor + 0x1ABC, 2), "big"),
    }
    if function != "dispatch":
        row["request"] = {"line": reg(4) & 0xFFFF, "priority": reg(6) & 0xFF,
                          "mode": reg(7) & 0xFF}
    idle, stop, play = 0x800F0460, 0x800F05D8, 0x800F036C
    for address in (caller, idle, stop, play):
        remote.breakpoint(address, True)
    for _ in range(16):
        if not remote.query("c").startswith("T"):
            raise RuntimeError("Dolphin did not reach a voice callback or return")
        pc = reg(64)
        if pc == caller:
            break
        if pc == idle:
            entry, resume = reg(3), reg(67)
            remote.breakpoint(resume, True)
            if not remote.query("c").startswith("T") or reg(64) != resume:
                raise RuntimeError("Dolphin did not return from voice status")
            row["calls"].append({"kind": "idle", "channel": entry, "result": reg(3)})
            remote.breakpoint(resume, False)
        elif pc == stop:
            row["calls"].append({"kind": "stop", "channel": reg(3)})
        elif pc == play:
            row["calls"].append({"kind": "play", "line": reg(3), "volume": reg(4),
                                  "pan": reg(5), "channel": reg(6)})
        else:
            raise RuntimeError("unexpected nested voice callback")
    else:
        raise RuntimeError("voice dispatch exceeded its callback bound")
    row["after"] = state()
    for address in (caller, idle, stop, play):
        remote.breakpoint(address, False)
    return row


def observe_weapon_flight(remote, index, text, pool):
    """Read one active equipped-weapon update, including its actual catch operand."""
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    vector = lambda address: list(struct.unpack(">3I", remote.memory(address, 12)))
    if reg(64) != text + 0x21F48:
        raise RuntimeError("unexpected detached-weapon breakpoint")
    actor, slot, model, stack = reg(23), reg(24), reg(25), reg(1)
    if slot not in (0, 1) or remote.memory(actor + 0x107E, 1)[0] >> 4 != 2:
        raise RuntimeError("invalid detached-weapon owner or slot")
    state = actor + 0x1B14
    hit = word(state + 0x30 + slot * 4)
    sample = lambda: {
        "detached": remote.memory(state + slot, 1)[0],
        "outbound": int.from_bytes(remote.memory(state + 4 + slot * 2, 2), "big", signed=True),
        "position_bits": vector(model + 0xC), "angles_bits": vector(model + 0x18),
        "direction_bits": vector(state + 8 + slot * 12),
        "speed_bits": word(state + 0x20 + slot * 4),
        "cooldowns": list(remote.memory(state + 0x40 + slot * 8, 8)),
        "trail_timer": remote.memory(actor + 0x1578 + slot, 1)[0],
    }
    row = {"index": index, "function": "weapon_flight", "actor": actor, "slot": slot,
           "combat_tick": word(pool + 0x15704), "presentation_counter": word(0x8035A628),
           "caller": reg(67), "stack": stack, "record": remote.memory(hit, 32).hex(),
           "before": sample(), "stack_point_before_bits": vector(stack + 0x30),
           "owner_position_bits": vector(actor + 0x18C0),
           "owner_heading_bits": word(actor + 0x18D0),
           "owner_forward_bits": vector(actor + 0x18E4),
           "hit_stop": remote.memory(actor + 0x10B0, 1)[0]}
    catch, after = text + 0x2213C, text + 0x2215C
    remote.breakpoint(catch, True)
    if not remote.query("c").startswith("T") or reg(64) != catch:
        raise RuntimeError("Dolphin did not reach the detached-weapon catch check")
    row["catch_point_bits"] = vector(reg(3))
    row["integrated"] = sample()
    remote.breakpoint(catch, False)
    remote.breakpoint(after, True)
    if not remote.query("c").startswith("T") or reg(64) != after:
        raise RuntimeError("Dolphin did not finish the detached-weapon catch check")
    # FPR1 retains the distance result across this comparison.
    distance = struct.unpack(">d", bytes.fromhex(remote.query("p21")))[0]
    row["catch_distance_bits"] = struct.unpack(">I", struct.pack(">f", distance))[0]
    row["after"] = sample()
    remote.breakpoint(after, False)
    return row


def trace(path, output, process, timeout, random_calls=None, distance_calls=None,
          geometry_calls=None, battle_text=None, damage_calls=None, battle_pool=None, melee_calls=None, motion_calls=None, movement_calls=None, resident_calls=None, casting_calls=None, effect_calls=None, reaction_calls=None, hurt_calls=None, guard_calls=None, particle_uv_calls=None, stun_roll_calls=None, stun_calls=None, particle_calls=None, voice_calls=None, weapon_flight_calls=None, weapon_stack_writes=False, weapon_stack_from_combat=0):
    if weapon_stack_writes:
        from weapon_stack_trace import trace as trace_stack
        return trace_stack(path, output, process, timeout, weapon_flight_calls, battle_text, battle_pool, weapon_stack_from_combat)
    breakpoints = ({battle_text + 0x21F48: "weapon_flight"} if weapon_flight_calls else
                   {battle_text + offset: name for offset, name in
                    [(0x71674, "voice_dispatch"), (0x71D90, "voice_relative"), (0x71E78, "voice_absolute")]} if voice_calls else
                   {battle_text + 0x403F4: "particle"} if particle_calls else
                   {battle_text + 0x2E848: "stun"} if stun_calls else
                   {battle_text + 0x3CF7C: "stun_roll"} if stun_roll_calls else
                   {battle_text + 0x40B28: "particle_uv"} if particle_uv_calls else
                   {battle_text + 0x2F284: "guard"} if guard_calls else
                   {battle_text + 0x2FB24: "hurt"} if hurt_calls else
                   {battle_text + 0x63470: "reaction"} if reaction_calls else
                   {battle_text + 0x420C4: "effect"} if effect_calls else
                   {battle_text + offset: name for offset, name in
                    [(0x39974, "initialize"), (0x3898C, "countdown"), (0x385A0, "release")]} if casting_calls else
                   {battle_text + 0x3A978: "residents"} if resident_calls else
                   {battle_text + offset: name for offset, name in
                    [(0x24314, "integrate"), (0x244D0, "integrate_brake"), (0x24040, "brake")]} if movement_calls else
                   {0x8006D2E0: "motion", 0x80069088: "chains"} if motion_calls else
                   {battle_text + offset: name for offset, name in
                    [(0x2D564, "melee"), (0x2C8B0, "commands"),
                     (0x2B910, "animation_row"), (0x2BC3C, "animation_start")]} if melee_calls else
                   {battle_text + 0x61578: "damage"} if damage_calls else
                   {battle_text + 0x3C068: "hurt_shape"} if geometry_calls else
                   {0x800FE6F8: "vector_length"} if distance_calls else
                   {0x80124BF4: "random"} if random_calls else BREAKPOINTS)
    bounded_calls = weapon_flight_calls or voice_calls or particle_calls or stun_calls or stun_roll_calls or particle_uv_calls or guard_calls or hurt_calls or reaction_calls or effect_calls or casting_calls or resident_calls or movement_calls or motion_calls or melee_calls or damage_calls or geometry_calls or distance_calls or random_calls
    deadline = time.monotonic() + timeout
    while not Path(path).exists():
        if process.poll() is not None:
            raise RuntimeError("Dolphin exited before opening its debugger socket")
        if time.monotonic() >= deadline:
            raise TimeoutError("Dolphin did not open its debugger socket")
        time.sleep(0.05)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.connect(str(path))
        remote = Remote(connection, deadline)
        # Dolphin starts the emulated CPU paused for this connection.
        if not remote.query("?").startswith("T"):
            raise RuntimeError("Dolphin did not report its initial stop")
        for address in breakpoints:
            remote.breakpoint(address, True)
        with output.open("x") as log:
            for index in range(4096):
                if not remote.query("c").startswith("T"):
                    raise RuntimeError("Dolphin did not stop at an execute breakpoint")
                if weapon_flight_calls:
                    row = observe_weapon_flight(remote, index, battle_text, battle_pool)
                elif voice_calls:
                    row = observe_voice(remote, index, battle_text, battle_pool)
                elif particle_calls:
                    row = observe_particle(remote, index, battle_text, battle_pool)
                elif stun_calls:
                    row = observe_recovery(remote, index, battle_text, battle_pool, stunning=True)
                elif stun_roll_calls:
                    row = observe_stun_roll(remote, index, battle_text, battle_pool)
                elif particle_uv_calls:
                    row = observe_particle_uv(remote, index, battle_text, battle_pool)
                elif hurt_calls or guard_calls:
                    row = observe_recovery(remote, index, battle_text, battle_pool, bool(guard_calls))
                elif reaction_calls:
                    row = observe_reaction(remote, index, battle_text, battle_pool)
                elif effect_calls:
                    row = observe_effect(remote, index, battle_text, battle_pool)
                elif casting_calls:
                    row = observe_casting(remote, index, battle_text, battle_pool)
                elif resident_calls:
                    row = observe_residents(remote, index, battle_text, battle_pool)
                elif movement_calls:
                    row = observe_movement(remote, index, battle_text, battle_pool)
                elif motion_calls:
                    row = observe_motion(remote, index, battle_pool)
                elif melee_calls:
                    row = observe_melee(remote, index, battle_text, battle_pool)
                elif damage_calls:
                    row = observe_damage(remote, index, battle_text, battle_pool)
                elif geometry_calls:
                    row = observe_geometry(remote, index, battle_text)
                elif distance_calls:
                    row = observe_distance(remote, index)
                elif random_calls:
                    if int(remote.query("p40"), 16) != 0x80124BF4:
                        raise RuntimeError("unexpected random trace breakpoint")
                    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
                    row = {"index": index, "function": "random",
                           "lr": f"{int(remote.query('p43'), 16):08x}",
                           "presentation_counter": word(0x8035A628),
                           "ui_clock": word(0x8035AA1C),
                           "random_state": word(0x8035A340)}
                    if row["lr"] == "8004e194":
                        context = int(remote.query("p1f"), 16)
                        row["script"] = {
                            "context_address": f"{context:08x}",
                            "divisor": int(remote.query("p1e"), 16),
                            "header": list(struct.unpack(">8I", remote.memory(context, 32))),
                        }
                    elif row["lr"] in ("80022d4c", "80022a38", "80022a50", "80022abc"):
                        actor = int(remote.query("p1f"), 16)
                        row["actor"] = {"address": f"{actor:08x}", "id": word(actor + 0xB8)}
                    elif row["lr"] in ("80086fe8", "80087108"):
                        particle = int(remote.query("p1f"), 16)
                        row["particle"] = {
                            "slot": (particle - word(0x8035A4FC)) // 0x6C,
                            "position": list(struct.unpack(">3f", remote.memory(particle + 4, 12))),
                            "turn_after": struct.unpack(">f", remote.memory(particle + 0x54, 4))[0],
                        }
                else:
                    row = {"index": index, **observe(remote)}
                log.write(json.dumps(row) + "\n")
                log.flush()
                if (index + 1 == bounded_calls if bounded_calls else
                        row["state_flags"] & 0x80 and row["movie"]["presented_frames"] >= 12):
                    break
            else:
                raise RuntimeError("startup trace reached its observation limit")
        for address in breakpoints:
            remote.breakpoint(address, False)
        # Continue the untouched replay. The next GDB event sees EOF and removes
        # the connection; no breakpoint remains and no emulated state is reset.
        remote.send("c")
        # Consume the command acknowledgement before closing. Otherwise Dolphin
        # can still be writing it when the peer disappears (SIGPIPE).
        if remote.read(1) != b"+":
            raise RuntimeError("Dolphin did not acknowledge trace continuation")
    return {"observations": index + 1, "complete": True,
            "diagnostic": True, "game_memory_modified": False, "debugger_enabled": True,
            "breakpoints": {f"0x{k:08x}": v for k, v in breakpoints.items()},
            "notes": "Execution was paused to read state. Compare with an ordinary replay before accepting timing."}
