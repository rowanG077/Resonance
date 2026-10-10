#!/usr/bin/env python3
"""Read replay identity and requested observations from a Dolphin 2606 checkpoint.

Header layout: Dolphin 2606 Source/Core/Core/State.h and State.cpp.
This development tool is independent of the Resonance runtime.
"""
import argparse
import ctypes
import ctypes.util
import hashlib
import json
from pathlib import Path
import struct


def inspect(path, library=None, *, field_origin=False, actors=False, particles=False, battle=False, party=False):
    data = path.read_bytes()
    if len(data) < 48 or data[:6] != b"GQSEAF":
        raise ValueError("expected a GQSEAF Dolphin state")
    cookie, length = struct.unpack_from("<II", data, 24)
    if cookie != 0xBAADBABE + 191 or not 1 <= length <= 256:
        raise ValueError("only the pinned Dolphin 2606 state version is supported")
    offset = 32 + length
    version = data[32:offset].rstrip(b"\0").decode("utf-8")
    header, compression, extra, size = struct.unpack_from("<HHIQ", data, offset)
    if header != 1 or compression != 1 or extra != 0 or not 0 < size <= 256 * 1024 * 1024:
        raise ValueError("unsupported state header or excessive decompressed size")
    offset += 16
    compressed_size, = struct.unpack_from("<I", data, offset)
    offset += 4
    if offset + compressed_size != len(data):
        raise ValueError("truncated state or unsupported multi-block state")
    library = library or ctypes.util.find_library("lz4") or "liblz4.so.1"
    codec = ctypes.CDLL(library)
    codec.LZ4_decompress_safe.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    codec.LZ4_decompress_safe.restype = ctypes.c_int
    buffer = ctypes.create_string_buffer(size)
    if codec.LZ4_decompress_safe(data[offset:], buffer, compressed_size, size) != size:
        raise ValueError("corrupt compressed state")
    raw = buffer.raw
    # State.cpp serializes platform/memory sizes, then MovieManager::DoState.
    if (raw[0] != 0 or struct.unpack_from("<I", raw, 1)[0] != 0x1800000
            or struct.unpack_from("<I", raw, 50)[0] != 0x42):
        raise ValueError("unsupported movie observation layout")
    movie_frame, movie_byte, movie_lag, movie_input = struct.unpack_from("<QQQQ", raw, 9)
    if movie_byte != movie_input * 8:
        raise ValueError("expected single-controller GameCube replay")
    candidates = []
    offset = 0
    while (offset := raw.find(b"GQSEAF", offset)) >= 0:
        if (raw[offset + 0x1c:offset + 0x20] == bytes.fromhex("c2339f3d")
                and raw[offset + 0x28:offset + 0x2c] == bytes.fromhex("01800000")
                and offset + 0x1800000 <= len(raw)):
            candidates.append(offset)
        offset += 1
    if len(candidates) != 1:
        raise ValueError("could not uniquely identify the GameCube main-memory observation")
    ram = memoryview(raw)[candidates[0]:candidates[0] + 0x1800000]
    result = {"dolphin_version": version,
              "movie": {"vi_frame": movie_frame, "input_count": movie_input,
                        "input_byte": movie_byte, "lag_frames": movie_lag},
              "state_sha256": hashlib.sha256(data).hexdigest()}
    result.update(observations(ram, field_origin=field_origin, actors=actors,
                               particles=particles, battle=battle, party=party))
    return result


def observations(ram, *, field_origin=False, actors=False, particles=False, battle=False, party=False):
    """Discover only the storage needed by the requested live watches."""
    if len(ram) != 0x1800000:
        raise ValueError("expected GameCube main RAM")
    u32 = lambda offset: struct.unpack_from(">I", ram, offset)[0]

    def pointer(address, size):
        offset = address - 0x80000000
        return offset if 0 <= offset <= len(ram) - size else None

    mode = struct.unpack_from(">H", ram, 0x35a762)[0] & 0x7f
    result = {"mode": mode}
    if mode in (9, 12):
        if field_origin or actors or particles or party:
            raise ValueError("field observations requested during battle")
        if battle:
            from battle_state import inspect_battle
            result["battle"] = inspect_battle(ram)
        return result

    for section, location, offset, code, name in (
            ("field", 0x35a768, 0x10d0, ">I", "map_id"),
            ("progress", 0x35a578, 0x40, ">i", "story")):
        address = u32(location)
        base = pointer(address, offset + 4)
        if base is None or base % 4:
            if field_origin:
                raise ValueError(f"invalid {section} origin")
        else:
            result[section] = {name: struct.unpack_from(code, ram, base + offset)[0],
                               "address": f"{address:08x}"}
    if party:
        settings = pointer(u32(0x35a768), 0xea5)
        if settings is None:
            raise ValueError("invalid party observation origin")
        half = lambda offset: struct.unpack_from(">H", ram, offset)[0]
        members = []
        for index in range(9):
            at = settings + 0x2b8 + index * 0x118
            members.append({
                "id": index + 1,
                "name": bytes(ram[at:at + 16]).split(b"\0")[0].decode("ascii", errors="replace"),
                "level": ram[at + 0x10], "experience": u32(at + 0x18),
                "hp": half(at + 0x12), "tp": half(at + 0x14),
                "max_hp": half(at + 0x36), "max_tp": half(at + 0x38),
                "base_stats": [half(at + offset) for offset in [0x26, 0x28, 0x2a, 0x2c, 0x34, 0x32, 0x30]],
                "luck": half(at + 0x2e) // 10,
                "equipment": [half(at + offset) for offset in [0x4a, 0x4c, 0x4e, 0x52, 0x54, 0x50]],
                "title": half(at + 0xe) & 0xff,
                "ex_skills": list(ram[at + 0xee:at + 0xf2]),
            })
        result["party_menu"] = {
            "formation": [member for member in ram[settings + 0xe9d:settings + 0xea5] if member],
            "members": members,
        }
    if actors:
        def actor(at):
            return {"address": at + 0x80000000,
                    "id": struct.unpack_from(">i", ram, at + 0xb8)[0]}
        result["controlled_actor"] = actor(0x2c7ea0)
        pool = pointer(u32(0x35a4e4), 0x860)
        if pool is not None:
            result["actors"] = []
            for slot in range(min(4096, (len(ram) - pool) // 0x860)):
                at = pool + slot * 0x860
                entry = u32(at)
                if entry == 0xffffffff:
                    break
                if entry:
                    result["actors"].append({"slot": slot, **actor(at)})
            else:
                raise ValueError("actor table has no bounded terminator")
        slots = pointer(u32(0x35a3e4), 3 * 0x13660)
        if slots is not None:
            result["dialogue_windows"] = [
                {"slot": slot, "address": slots + slot * 0x13660 + 0x80000000}
                for slot in range(3)]
    if particles:
        address = u32(0x35a4fc)
        if pointer(address, 2048 * 0x6c) is None:
            raise ValueError("initial state has no valid particle pool")
        result["particle_pool_address"] = f"{address:08x}"
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("state", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--lz4", help="Explicit LZ4 library path, if outside nix develop")
    parser.add_argument("--field-origin", action="store_true",
                        help="Require the field/story origin used by paired acceptance")
    parser.add_argument("--actors", action="store_true", help="Discover field actor and dialogue addresses")
    parser.add_argument("--particles", action="store_true", help="Discover the field particle pool")
    parser.add_argument("--battle", action="store_true", help="Read active battle observations")
    parser.add_argument("--party", action="store_true", help="Read checkpoint_fixture party-stat registration inputs")
    args = parser.parse_args()
    result = inspect(args.state, args.lz4, field_origin=args.field_origin,
                     actors=args.actors, particles=args.particles, battle=args.battle, party=args.party)
    args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + "\n")


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()



class State:
    """Decoded checkpoint, shared by observations and isolated renderer fixtures."""
    def __init__(self, path, library=None):
        self.path = Path(path)
        self.data = self.path.read_bytes()
        data = self.data
        if len(data) < 48 or data[:6] != b'GQSEAF':
            raise ValueError('expected a GQSEAF Dolphin state')
        cookie, length = struct.unpack_from('<II', data, 24)
        if cookie != 0xBAADBABE + 191 or not 1 <= length <= 256:
            raise ValueError('only the pinned Dolphin 2606 state version is supported')
        self.header = 32 + length
        self.version = data[32:self.header].rstrip(b'\0').decode('utf-8')
        header, compression, extra, size, packed = struct.unpack_from('<HHIQI', data, self.header)
        if (header != 1 or compression != 1 or extra != 0
                or not 0 < size <= 256 * 1024 * 1024 or self.header + 20 + packed != len(data)):
            raise ValueError('unsupported or truncated single-block state')
        self.codec = ctypes.CDLL(library or ctypes.util.find_library('lz4') or 'liblz4.so.1')
        for name in ['LZ4_decompress_safe', 'LZ4_compress_default']:
            fn = getattr(self.codec, name)
            fn.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
            fn.restype = ctypes.c_int
        self.raw = bytearray(size)
        buffer = (ctypes.c_char * size).from_buffer(self.raw)
        if self.codec.LZ4_decompress_safe(data[self.header+20:], buffer, packed, size) != size:
            raise ValueError('corrupt compressed state')
        candidates, at = [], 0
        while (at := self.raw.find(b'GQSEAF', at)) >= 0:
            if (self.raw[at+28:at+32] == bytes.fromhex('c2339f3d')
                    and self.raw[at+40:at+44] == bytes.fromhex('01800000')
                    and at + 0x1800000 <= len(self.raw)):
                candidates.append(at)
            at += 1
        if len(candidates) != 1:
            raise ValueError('cannot uniquely locate main memory')
        self.ram_offset = candidates[0]
        self.sha256 = hashlib.sha256(data).hexdigest()
        self.changes = []

    @property
    def ram(self):
        return memoryview(self.raw)[self.ram_offset:self.ram_offset+0x1800000]

    def fork(self):
        state = copy.copy(self)
        state.raw = self.raw.copy()
        state.changes = []
        return state

    def read(self, address, fmt='I'):
        return struct.unpack_from('>' + fmt, self.ram, address & 0x1FFFFFF)[0]

    def write(self, address, fmt, value):
        before = self.read(address, fmt)
        struct.pack_into('>' + fmt, self.ram, address & 0x1FFFFFF, value)
        self.changes.append([hex(address), fmt, before, value])

    def save(self, path):
        raw = (ctypes.c_char * len(self.raw)).from_buffer(self.raw)
        buf = ctypes.create_string_buffer(len(self.raw) + len(self.raw)//255 + 16)
        size = self.codec.LZ4_compress_default(raw, buf, len(self.raw), len(buf))
        if size <= 0:
            raise ValueError('state compression failed')
        Path(path).write_bytes(self.data[:self.header+16] + struct.pack('<I', size) + buf.raw[:size])
