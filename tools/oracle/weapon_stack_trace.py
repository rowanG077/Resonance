"""Bounded read-only attribution of the original detached-weapon catch operand.

Dolphin 2606 implements GDB Z2 as an emulator memory check, without changing
game memory. See the pinned 2606 Source/Core/Core/PowerPC/GDBStub.cpp. This
observer watches the twelve catch-point bytes only during an active owner's
state callback and common tail. Its results remain diagnostic until replayed.
"""
import json
from pathlib import Path
import socket
import struct
import time

from startup_trace import Remote, observe_weapon_flight


def observe(remote, index, text, pool, from_combat=0):
    reg = lambda i: int(remote.query(f"p{i:x}"), 16)
    word = lambda address: int.from_bytes(remote.memory(address, 4), "big")
    vector = lambda address: list(struct.unpack(">3I", remote.memory(address, 12)))
    entry, tail = text + 0x31E9C, text + 0x31EB0
    if reg(64) != entry:
        raise RuntimeError("unexpected weapon stack entry")
    if word(pool + 0x15704) < from_combat:
        return None
    actor = reg(31)
    if remote.memory(actor + 0x107E, 1)[0] >> 4 != 2:
        return None
    active = remote.memory(actor + 0x1B14, 2)
    if not any(active):
        return None
    if sum(bool(value) for value in active) != 1:
        raise RuntimeError("weapon stack case unexpectedly has two active slots")
    point = reg(1) - 144
    sample = lambda: {
        "point_bits": vector(point),
        "detached_bytes": list(remote.memory(actor + 0x1B14, 2)),
        "action_bytes": list(remote.memory(actor + 0x1AC, 8)),
        "position_bits": vector(actor + 0x18C0),
        "body_yaw_bits": word(actor + 0x2DC),
        "previous_velocity_bits": list(struct.unpack(">2I", remote.memory(actor + 0x1908, 8))),
        "velocity_bits": list(struct.unpack(">4I", remote.memory(actor + 0x1910, 16))),
        "hit_stop": remote.memory(actor + 0x10B0, 1)[0],
    }
    row = {"index": index, "function": "weapon_stack", "actor": actor,
           "point_address": point, "callback": reg(12), "combat_tick": word(pool + 0x15704),
           "inherited_gpr24_to_26": [reg(i) for i in range(24, 27)],
           "before": sample(), "writes": []}

    def watch(enabled):
        if remote.query(f"{'Z' if enabled else 'z'}2,{point:x},c") != "OK":
            raise RuntimeError("Dolphin rejected the weapon-stack write watchpoint")

    remote.breakpoint(entry, False)
    remote.breakpoint(tail, True)
    watch(True)
    for _ in range(256):
        reply = remote.query("c")
        if not reply.startswith("T"):
            raise RuntimeError("Dolphin did not stop during weapon stack attribution")
        pc = reg(64)
        if pc == tail:
            break
        event = {"pc": pc, "lr": reg(67), "stack": reg(1),
                 "ctr": reg(68), "gpr3_to_12": [reg(i) for i in range(3, 13)],
                 "instructions": remote.memory(pc - 4, 12).hex(),
                 "point_bits": vector(point)}
        if row["writes"] and event == row["writes"][-1]:
            raise RuntimeError("weapon stack watchpoint repeated without advancing")
        row["writes"].append(event)
    else:
        raise RuntimeError("weapon stack writer bound exceeded")
    watch(False)
    remote.breakpoint(tail, False)
    row["after"] = sample()
    if any(remote.memory(actor + 0x1B14, 2)):
        flight = text + 0x21F48
        remote.breakpoint(flight, True)
        if not remote.query("c").startswith("T") or reg(64) != flight:
            raise RuntimeError("active weapon did not reach its flight update")
        row["flight"] = observe_weapon_flight(remote, index, text, pool)
        remote.breakpoint(flight, False)
    remote.breakpoint(entry, True)
    return row


def trace(path, output, process, timeout, calls, text, pool, from_combat=0):
    deadline = time.monotonic() + timeout
    while not Path(path).exists():
        if process.poll() is not None:
            raise RuntimeError("Dolphin exited before opening its debugger socket")
        if time.monotonic() >= deadline:
            raise TimeoutError("Dolphin did not open its debugger socket")
        time.sleep(0.05)
    entry = text + 0x31E9C
    observations = 0
    end_reason = "visit_limit"
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.connect(str(path))
        remote = Remote(connection, deadline)
        if not remote.query("?").startswith("T"):
            raise RuntimeError("Dolphin did not report its initial stop")
        remote.breakpoint(entry, True)
        with output.open("x") as log:
            for _ in range(8192):
                if not remote.query("c").startswith("T"):
                    raise RuntimeError("Dolphin did not stop at actor dispatch")
                row = observe(remote, observations, text, pool, from_combat)
                if row is None:
                    continue
                log.write(json.dumps(row) + "\n")
                log.flush()
                observations += 1
                if "flight" not in row:
                    end_reason = "retired_by_callback"
                    break
                if row["flight"]["after"]["detached"] == 0:
                    end_reason = "caught"
                    break
                if observations == calls:
                    break
            else:
                raise RuntimeError("weapon stack dispatch bound exceeded")
        remote.breakpoint(entry, False)
        remote.send("c")
        if remote.read(1) != b"+":
            raise RuntimeError("Dolphin did not acknowledge trace continuation")
    return {"observations": observations, "complete": True,
            "end_reason": end_reason,
            "from_combat": from_combat,
            "diagnostic": True, "game_memory_modified": False, "debugger_enabled": True,
            "breakpoints": {f"0x{entry:08x}": "active weapon owner dispatch"},
            "write_watchpoint": "12 bytes at the current actor-dispatch SP minus144",
            "notes": "Read-only writer attribution; compare an unchanged ordinary replay before accepting timing."}
