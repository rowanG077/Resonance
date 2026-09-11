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


def trace(path, output, process, timeout, random_calls=None):
    breakpoints = {0x80124BF4: "random"} if random_calls else BREAKPOINTS
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
                if random_calls:
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
                if (index + 1 == random_calls if random_calls else
                        row["state_flags"] & 0x80 and row["movie"]["presented_frames"] >= 12):
                    break
            else:
                raise RuntimeError("startup trace reached its observation limit")
        for address in breakpoints:
            remote.breakpoint(address, False)
        # Continue the untouched replay. The next GDB event sees EOF and removes
        # the connection; no breakpoint remains and no emulated state is reset.
        remote.send("c")
    return {"observations": index + 1, "complete": True,
            "diagnostic": True, "game_memory_modified": False, "debugger_enabled": True,
            "breakpoints": {f"0x{k:08x}": v for k, v in breakpoints.items()},
            "notes": "Execution was paused to read state. Compare with an ordinary replay before accepting timing."}
