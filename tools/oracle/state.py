#!/usr/bin/env python3
"""Read named title observations from a Dolphin 2606 checkpoint; never execute it.

Header layout: Dolphin 2606 Source/Core/Core/State.h and State.cpp.
Game offsets: fn_80072AD8.c, fn_800708C8.c and their original symbols.
This development tool is independent of the Resonance runtime.
"""
import argparse
import ctypes
import ctypes.util
import hashlib
import json
import math
from pathlib import Path
import struct


def inspect(path, library=None):
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
    u32 = lambda offset: struct.unpack_from(">I", ram, offset)[0]
    fields = {
        "selected": 0x35a010, "pulse_tick": 0x35a058,
        "presentation_counter": 0x35a628, "opacity": 0x35a6bc,
        "reveal_counter": 0x35a6c4, "idle_counter": 0x35a6c8,
    }
    result = {"dolphin_version": version,
              "movie": {"vi_frame": movie_frame, "input_count": movie_input,
                        "input_byte": movie_byte, "lag_frames": movie_lag},
              "state_sha256": hashlib.sha256(data).hexdigest(),
              "title": {key: u32(address) for key, address in fields.items()}}
    result["title"]["revealed"] = bool(ram[0x35a6c0])
    result["title"]["state_flags"] = struct.unpack_from(">H", ram, 0x35a762)[0]
    result["action_prompt"] = {
        "id": u32(0x35a460), "opacity": u32(0x35a464), "remaining": u32(0x35a468),
    }
    result["field_presentation"] = {
        "scene_flags": u32(0x35a760), "control_flags": u32(0x35a73c),
    }
    # fn_80124BF4: observe the shared generator without advancing it.
    result["random_state"] = u32(0x35a340)
    # Gameplay MT19937 is independent of field animation and particle effects.
    next_word = u32(0x35a7e0)
    remaining = struct.unpack_from(">i", ram, 0x35a1e0)[0]
    result["gameplay_random_cursor"] = {"pointer": next_word, "remaining": remaining}
    if remaining < 0:
        result["gameplay_random"] = "uninitialized"
    elif 0x802ce560 <= next_word <= 0x802cef20 and (next_word - 0x802ce560) % 4 == 0:
        index = (next_word - 0x802ce560) // 4
        if remaining != 624 - index:
            raise ValueError("inconsistent gameplay random cursor")
        result["gameplay_random"] = {"state": {
            "index": index, "words": list(struct.unpack_from(">624I", ram, 0x2ce560))}}
    result["blink_sequence"] = [
        {"frame": ram[at], "ticks": struct.unpack_from(">H", ram, at + 2)[0] + 1}
        for at in range(0x1e3840, 0x1e3850, 4)]
    globals_base = u32(0x35a578) - 0x80000000
    if 0 <= globals_base <= len(ram) - 0x400:
        result["progress"] = {"story": struct.unpack_from(">i", ram, globals_base + 0x40)[0],
                              "address": f"{globals_base + 0x80000000:08x}"}
    result["script_instances"] = []
    if 0 <= globals_base <= len(ram) - 0xC41C:
        count = struct.unpack_from(">H", ram, globals_base + 0x5814)[0]
        if count > 300:
            raise ValueError("script registry exceeds its instance table")
        result["script_entries"] = [
            dict(zip(("kind", "key", "pc"), struct.unpack_from(">3I", ram, globals_base + 0x2160 + i * 12)))
            for i in range(count)]
        for slot in range(32):
            at = globals_base + 0x581C + slot * 0x360
            if ram[at + 2] == 1:
                result["script_instances"].append({
                    "slot": slot, "address": f"{at + 0x80000000:08x}",
                    "flags": ram[at + 3], "program_address": f"{u32(at + 4):08x}",
                    "pc": u32(at + 8),
                    "wait_value": struct.unpack_from(">i", ram, at + 12)[0],
                    "wait_mode": u32(at + 16),
                    "kind": struct.unpack_from(">H", ram, at + 0x346)[0],
                })
    font = u32(0x35a5a8) - 0x80000000
    if 0 <= font <= len(ram) - 0x14400:
        result["font_sha256"] = hashlib.sha256(ram[font:font + 0x14400]).hexdigest()
    # Common model record and resolved model used by the field save-point object.
    result["save_point_resource"] = {
        "record": bytes(ram[0x2bd2cc:0x2bd310]).hex(),
        "model_address": u32(0x35a4cc),
    }
    scene_texture = [u32(0x2c8710 + i*4) for i in range(8)]
    result["refraction_scene_texture"] = {
        "words": scene_texture,
        "width": (scene_texture[2] & 1023) + 1,
        "height": ((scene_texture[2] >> 10) & 1023) + 1,
        "format": scene_texture[5],
    }
    # fn_80104FA8 is lwz r3,-0x75e4(r13), with r13=0x80362000.
    # fn_800DE37C uses this VI clock and retains a short integer cursor trail.
    result["choice_cursor"] = {
        "clock": u32(0x35aa1c),
        "trail_anchor": list(struct.unpack_from(">2h", ram, 0x35a1d0)),
        "trail_remaining": struct.unpack_from(">h", ram, 0x35a7d8)[0],
    }
    menu = 0x221cf8
    message = u32(menu + 0x20) - 0x80000000
    result["save_menu"] = {
        "bank": ram[menu + 0xc], "slot": ram[menu + 0xd],
        "first_slot": struct.unpack_from(">h", ram, menu + 0xe)[0],
        "scroll": struct.unpack_from(">h", ram, menu + 0x10)[0],
        "choice": ram[menu + 0x12], "mode": ram[menu + 0x13],
        "screen": struct.unpack_from(">h", ram, menu + 0x14)[0],
        "confirmation_alpha": ram[menu + 0x28], "notice_alpha": ram[menu + 0x29],
        "message": (bytes(ram[message:message + 1024]).split(b"\0")[0].decode("shift_jis")
                    if 0 <= message <= len(ram) - 1024 else None),
    }
    result["field_clearance"] = struct.unpack_from(">H", ram, 0x35a008)[0]
    settings = u32(0x35a768) - 0x80000000
    skit_title = u32(0x35a798) - 0x80000000
    if (0 <= skit_title <= len(ram) - 256
            and 0 <= settings <= len(ram) - 0x1f5a):
        result["skit_prompt"] = {
            "id": u32(0x35a794),
            "title": bytes(ram[skit_title:skit_title + 256]).split(b"\0")[0].decode("shift_jis"),
            "opacity": u32(0x35a454), "remaining": u32(0x35a458),
            "refresh_tick": u32(0x35a450),
            "field_ticks": u32(settings + 0x1e94),
            "availability_flags": ram[settings + 0x1e91],
            "selection_mode": ram[settings + 0x1e90],
            "cooldown": struct.unpack_from(">h", ram, settings + 0x1f58)[0],
        }
    if 0 <= settings <= len(ram) - 0x10d4:
        # The field loader selects its resource-table row from this session word.
        result["field"] = {"map_id": u32(settings + 0x10d0),
                           "address": f"{settings + 0x80000000:08x}"}
        half = lambda at: struct.unpack_from(">H", ram, at)[0]
        result["party_menu"] = {
            "gald": u32(settings),
            "encounters": half(settings + 4), "max_combo": half(settings + 6),
            "saved_play_ticks": u32(settings + 8),
            "session_ticks": (u32(0x35aa1c) - u32(0x35a7dc)) & 0xffffffff,
            "formation": [member for member in ram[settings + 0xe9d:settings + 0xea5] if member],
            "items": {str(i): ram[settings + 0xead + i] for i in range(1, 528) if ram[settings + 0xead + i]},
            "found_items": [i for i in range(1, 528) if u32(settings + 0x1124 + i // 32 * 4) & (1 << (i % 32))],
            "recent_items": [i for i in struct.unpack_from(">32H", ram, settings + 0x10e4) if i],
            "leader_locked": bool(ram[settings + 0xea9] & 8),
            "members": [],
        }
        for index in range(9):
            member = settings + 0x2b8 + index * 0x118
            result["party_menu"]["members"].append({
                "id": index + 1,
                "name": bytes(ram[member:member + 16]).split(b"\0")[0].decode("ascii", errors="replace"),
                "level": ram[member + 0x10], "experience": u32(member + 0x18),
                "hp": half(member + 0x12), "tp": half(member + 0x14),
                "max_hp": half(member + 0x36), "max_tp": half(member + 0x38),
                "base_stats": [half(member + offset) for offset in [0x26, 0x28, 0x2a, 0x2c, 0x34, 0x32, 0x30]],
                "luck": half(member + 0x2e) // 10,
                "equipment": [half(member + offset) for offset in [0x4a, 0x4c, 0x4e, 0x52, 0x54, 0x50]],
                "title": half(member + 0xe) & 0xff,
                "titles_mask": u32(member + 0x20),
                "technique_balance": struct.unpack_from("b", ram, member + 0x10c)[0],
                "cooking": list(ram[member + 0xf4:member + 0x10c]),
                "ex_skills": list(ram[member + 0xee:member + 0xf2]),
            })
    if 0 <= settings <= len(ram) - 0x1e23:
        result["party_menu"]["field_leader"] = ram[settings + 0x1e22] + 1
        result["party_menu"]["travel"] = {
            "current_location": half(0x2cb52e) or None,
            "visited_locations": [world * 256 + i + 1 for world in range(2) for i in range(128)
                                  if u32(settings + 0x1df8 + world * 16 + i // 32 * 4) & (1 << (i % 32))],
            "visited_shops": [i for i in range(52) if u32(settings + 0x1de8 + i // 32 * 4) & (1 << (i % 32))],
        }
        result["cooking"] = {
            "known": u32(settings + 0x1e18),
            "full": bool(ram[settings + 0x1e1c]),
            "recipe": ram[settings + 0x1e1d],
            "chef": ram[settings + 0x1e1e],
            "last_success": bool(ram[settings + 0x1e1f]),
        }
    if 0 <= settings <= len(ram) - 0x1de8:
        # Synopsis records keep an OS timestamp (epoch 2000); the time base
        # runs at one quarter of the bus clock stored in low memory.
        frequency = u32(0xf8) // 4
        result["event_records"] = {}
        for index in range(200):
            entry = settings + 0x1168 + index * 16
            if ram[entry] != 0:
                ticks = struct.unpack_from(">Q", ram, entry + 8)[0]
                result["event_records"][str(index)] = {
                    "value": ram[entry], "extra": ram[entry + 1],
                    "level": ram[entry + 2], "calendar_ticks": ticks,
                    "recorded_at": 946684800 + ticks // frequency if frequency else None,
                }
    if 0 <= settings <= len(ram) - 0x200:
        result["dialogue_settings"] = {
            "text_speed": ram[settings + 0x190],
            "theme": ram[settings + 0x197],
            "colors": [list(ram[settings + offset:settings + offset + 4])
                       for offset in [0x1a4, 0x1a8, 0x1ac]],
        }
    slots = u32(0x35a3e4) - 0x80000000
    if 0 <= slots <= len(ram) - 3 * 0x13660:
        result["dialogue_windows"] = []
        for index in range(3):
            base = slots + index * 0x13660
            half = lambda at: struct.unpack_from(">h", ram, base + at)[0]
            result["dialogue_windows"].append({
                "slot": index, "address": base + 0x80000000,
                "status": ram[base + 0x13543],
                "flags": half(0x13540) & 0xffff,
                "body_size": [half(0x1353c), half(0x1353e)],
                "body_rect": [half(at) for at in [0x1363e, 0x13640, 0x13642, 0x13644]],
                "speaker_width": half(0x13548), "height_offset": half(0x13654),
                "attachment_height": half(0x13638),
                "speaker": bytes(ram[base + 0x13320:base + 0x13338]).split(b"\0")[0].hex(),
                "body": bytes(ram[base + 0x13338:base + 0x13538]).split(b"\0")[0].hex(),
            })
    # fn_8006EEF4 logo phases and fn_800B081C's startup save check. These
    # observations distinguish presented startup frames from pure VI waits.
    result["startup"] = {
        "logo_phase": u32(0x35a634), "logo_tick": u32(0x35a630),
        "logo_next_phase": u32(0x35a638),
        "logo_alpha_step": struct.unpack_from(">i", ram, 0x35a640)[0],
        "logo_alpha": u32(0x35a644),
        "save_check": {
            "mode": ram[0x221cf8 + 0x13],
            "screen": struct.unpack_from(">h", ram, 0x221cf8 + 0x14)[0],
            "phase": ram[0x221cf8 + 0x1f],
            "delay": ram[0x221cf8 + 0x1e],
            "pending": struct.unpack_from(">H", ram, 0x221cf8 + 0x1c)[0],
        },
    }
    mode = u32(0x35a670) - 0x80000000
    if 0 <= mode <= len(ram) - 60:
        result["render_mode"] = {
            "width": struct.unpack_from(">H", ram, mode + 4)[0],
            "height": struct.unpack_from(">H", ram, mode + 6)[0],
            "copy_filter": list(ram[mode + 50:mode + 57]),
        }
    def floating(offset):
        value = struct.unpack_from(">f", ram, offset)[0]
        # Inactive animation channels can contain uninitialized memory. JSON
        # null marks a nonfinite observation; it must never become a pose value.
        return value if math.isfinite(value) else None
    result["projection_viewport"] = [floating(0x2cab00 + i*4) for i in range(6)]
    result["cinematic"] = {
        "movie_index": u32(0x35a71c), "presented_frames": u32(0x35a714),
        "return_state": ram[0x35a719], "skippable": bool(ram[0x35a718]),
        "overlay_alpha": floating(0x35a6d8), "overlay_rate": floating(0x35a6d4),
    }
    result["fade"] = {
        "alpha": floating(0x35a40c), "rate": floating(0x35a408),
        "color": ram[0x35a406],
    }
    result["focus"] = {
        "camera_near": floating(0x35a038), "camera_far": floating(0x35a03c),
        "foreground_enabled": bool(ram[0x35a4f0]),
        "background_enabled": bool(ram[0x35a4f1]),
        "foreground_plane": floating(0x35a4e8),
        "background_plane": floating(0x35a4ec),
    }
    result["fog"] = {
        "start": floating(0x2c8700), "end": floating(0x2c8704),
        "rgba": list(ram[0x2c8708:0x2c870c]), "kind": u32(0x2c870c),
    }
    camera_index = ram[0x35a594]
    if camera_index >= 4:
        raise ValueError("unexpected active camera index")
    camera_base = 0x2c9084 + camera_index * 0x94
    result["field_camera"] = {
        "index": camera_index,
        "fov_degrees": floating(camera_base + 0x58),
        "motion_fov_degrees": floating(0x2c8ee8 + 0x104),
        "flags": ram[camera_base + 0x10],
        "actor": u32(camera_base),
        "offset": [floating(camera_base + 4 + i*4) for i in range(3)],
        "anchor": [floating(camera_base + 0x4c + i*4) for i in range(3)],
        "rates": [floating(camera_base + 0x5c + i*4) for i in range(2)],
        "clock": floating(0x2c9524),
        "position": [floating(0x2c8ed8 + i*4) for i in range(3)],
        "target": [floating(0x2c8ecc + i*4) for i in range(3)],
        "view_matrix": [floating(0x2caf50 + i*4) for i in range(12)],
    }
    # Camera properties are persistent settings; the live fractional orbit is
    # separate oracle state (fn_8004A3F8, fn_8005F1E8).
    axes = [bool(ram[camera_base + 0x10] & bit) for bit in (4, 2, 1)]
    result["field_camera"]["position_settled"] = bool(ram[0x35a589])
    result["field_camera"]["target_settled"] = bool(ram[0x35a588])
    result["field_camera"]["oracle_origin"] = {
        "settings": {
            "axes": axes,
            "fixed_position": [0.0 if axes[i] else floating(camera_base + 0x34 + i*4) for i in range(3)],
            "angles": [floating(camera_base + 0x14 + i*4) for i in range(3)],
            "offset": result["field_camera"]["offset"],
            "distance": floating(camera_base + 0x20),
            "fov_degrees": result["field_camera"]["fov_degrees"],
            "position_bounds": [[floating(camera_base + 0x64 + i*8 + j*4) for j in range(2)] for i in range(3)],
            "target_bounds": [[floating(camera_base + 0x7c + i*8 + j*4) for j in range(2)] for i in range(3)],
            "position_rate": floating(0x2c9084 + 0x5c),
            "target_rate": floating(0x2c9084 + 0x60),
        },
        "angles": [floating(0x2c8ec0 + i*4) for i in range(3)],
        "distance": floating(0x35a590),
        "position": result["field_camera"]["position"],
        "target": result["field_camera"]["target"],
    }
    # fn_800262D0 draws this bounded actor table by signed layer, preserving
    # table order within each layer. Read the model and animation observations
    # directly; this is evidence inspection, never asset conversion/playback.
    def pointer(address, size):
        offset = address - 0x80000000
        return offset if 0 <= offset <= len(ram) - size else None

    bank = pointer(u32(0x35a490), 8)
    if bank is not None:
        count = u32(bank)
        if not 1 <= count <= 512 or pointer(bank + 0x80000000, 4 + count * 4) is None:
            raise ValueError("invalid field model bank")
        result["field_models"] = []
        if count > 1:
            ids = pointer(bank + 0x80000000 + (u32(bank + 4) & ~3), (count - 1) * 2)
            if ids is None:
                raise ValueError("invalid field model IDs")
            for index in range(1, count):
                address = bank + 0x80000000 + (u32(bank + 4 + index*4) & ~3)
                if pointer(address, 4) is None:
                    raise ValueError("invalid field model resource")
                result["field_models"].append({
                    "id": struct.unpack_from(">H", ram, ids + (index - 1)*2)[0],
                    "resource_address": f"{address:08x}",
                })

    runtime = pointer(u32(0x35a578), 0x2f70 + 200*0x34)
    if runtime is not None:
        result["field_triggers"] = []
        for index in range(200):
            at = runtime + 0x2f70 + index*0x34
            kind = u32(at)
            if kind not in (1, 2, 3):
                continue
            result["field_triggers"].append({
                "kind": kind, "key": u32(at + 4), "shape": ram[at + 12],
                "metadata": [*struct.unpack_from(">HH", ram, at + 16), u32(at + 20)],
                "coordinates": list(struct.unpack_from(">13h", ram, at + 24)),
            })

    # fn_800A2100 / fn_800A0E24 configure two standard-reverb callbacks.
    result["audio_setup"] = {
        "synth_clock": struct.unpack_from(">Q", ram, 0x35ac70)[0],
        "macro_clock": struct.unpack_from(">Q", ram, 0x35acd0)[0],
        "volume_groups": [
            {"group": group, **{name: floating(0x30817c + group*0x30 + i*4)
                for i, name in enumerate(["value", "target", "previous", "progress", "step", "value_b"])}}
            for group in range(32)
        ],
        "song_effect_type": ram[0x35a7b9],
        "auxiliary_reverbs": [
            {name: floating(base + 0x140 + i*4) for i, name in enumerate(
                ["coloration", "mix", "time", "damping", "pre_delay"])}
            for base in [0x2cc464, 0x2cc310]
        ],
    }
    settings = pointer(u32(0x35a768), 0x195)
    if settings is not None:
        result["audio_setup"]["volumes"] = dict(zip(
            ["music", "effects", "voices"], ram[settings + 0x192:settings + 0x195]))

    sequence = pointer(u32(0x35ac2c), 0x1540)
    if sequence is not None:
        result["audio_setup"]["sequence_clock"] = {
            "active": ram[sequence + 0x1538],
            "bpm_1024": u32(sequence + 0x1510),
            "tick_deltas": [{"fraction": u32(sequence + 0x1514 + i*8),
                             "tick": u32(sequence + 0x1518 + i*8)} for i in range(2)],
            "times": [{"fraction": u32(sequence + 0x1528 + i*8),
                       "tick": u32(sequence + 0x152c + i*8)} for i in range(2)],
            "loop_count": struct.unpack_from(">H", ram, sequence + 0x153c)[0],
        }

    # musyx_hw_dspctrl_80146DB0.c allocates count * 0xF8 DSP voices.
    # Inactive slots retain parameters; record their state rather than implying
    # they are playing. These observations are never inputs to cooked assets.
    voice_count = ram[0x35ad7d]
    if voice_count > 128:
        raise ValueError("unexpected DSP voice count")
    free_voices = []
    slot = ram[0x35acf9]
    while slot != 255:
        if slot >= voice_count or slot in free_voices:
            raise ValueError("invalid free voice queue")
        free_voices.append(slot)
        slot = ram[0x316068 + slot*4 + 1]
    result["audio_setup"]["free_voices"] = free_voices
    dsp_voices = pointer(u32(0x35ad48), voice_count * 0xf8)
    synth_voices = pointer(u32(0x35ac60), voice_count * 0x458)
    if dsp_voices is not None:
        voices = []
        for index in range(voice_count):
            at = dsp_voices + index * 0xf8
            pb = pointer(u32(at), 0xc4)
            voice = {
                "slot": index, "state": ram[at + 0xf0],
                "priority": u32(at + 0x1c),
                "links": {
                    name: (u32(at + offset) - 0x80000000 - dsp_voices) // 0xf8
                    if u32(at + offset) else None
                    for name, offset in [("next", 0xc), ("previous", 0x10)]
                },
                "sample_id": struct.unpack_from(">H", ram, at + 0x70)[0],
                "sample_length": u32(at + 0x84),
                "source_type": struct.unpack_from(">H", ram, at + 0xcc)[0],
                "source_filter": struct.unpack_from(">H", ram, at + 0xce)[0],
                "low_pass": {
                    "enabled": bool(ram[at + 0xd4]),
                    "coefficients": list(struct.unpack_from(">2H", ram, at + 0xd6)),
                },
                # Each millisecond's pitch entry is meaningful only when bit 3
                # of its change mask is set; the remaining storage is retained.
                "updates": [
                    {"change_mask": u32(at + 0x24 + i*4),
                     "pitch_16_16": u32(at + 0x38 + i*4)
                     if u32(at + 0x24 + i*4) & 8 else None}
                    for i in range(5)
                ],
                "mix_gains": list(struct.unpack_from(">9H", ram, at + 0x4c)),
                "envelope": {
                    "kind": ram[at + 0xa4], "phase": ram[at + 0xa5],
                    "remaining_ms": u32(at + 0xa8),
                    "value": u32(at + 0xac),
                    "attack_ms": u32(at + 0xb8),
                    "decay_ms": u32(at + 0xbc),
                    "sustain": struct.unpack_from(">H", ram, at + 0xc0)[0],
                    "release_ms": u32(at + 0xc4),
                },
            }
            if pb is not None:
                voice["parameter_block"] = {
                    "source_type": struct.unpack_from(">H", ram, pb + 8)[0],
                    "source_filter": struct.unpack_from(">H", ram, pb + 10)[0],
                    "state": struct.unpack_from(">H", ram, pb + 14)[0],
                    "pitch_16_16": u32(pb + 0xa6),
                    "mix": {
                        name: {"gain": struct.unpack_from(">H", ram, pb + 0x12 + i*4)[0],
                               "delta": struct.unpack_from(">h", ram, pb + 0x14 + i*4)[0]}
                        for i, name in enumerate([
                            "left", "right", "aux_a_left", "aux_a_right",
                            "aux_b_left", "aux_b_right", "aux_b_surround", "surround",
                            "aux_a_surround",
                        ])
                    },
                }
            if synth_voices is not None:
                synth = synth_voices + index * 0x458
                voice["synth"] = {
                    "macro_clock": {
                        "pitch_update": struct.unpack_from(">Q", ram, synth + 0x24)[0],
                        "volume_update": struct.unpack_from(">Q", ram, synth + 0x2c)[0],
                        "started": struct.unpack_from(">Q", ram, synth + 0x90)[0],
                        "wait_until": struct.unpack_from(">Q", ram, synth + 0x98)[0],
                        "resumed": struct.unpack_from(">Q", ram, synth + 0xa0)[0],
                    },
                    "macro_state": u32(synth + 0x4c),
                    "priority": ram[synth + 0x10c],
                    "priority_age": u32(synth + 0x110),
                    "vibrato": {
                        "semitones": ram[synth + 0x144], "cents": ram[synth + 0x145],
                        "period": u32(synth + 0x148), "phase": u32(synth + 0x14c),
                        "value": struct.unpack_from(">i", ram, synth + 0x150)[0],
                        "enabled": bool(struct.unpack_from(">Q", ram, synth + 0x114)[0] & 0x2000),
                    },
                    "lfo": {
                        "phase": u32(synth + 0x1c0), "period": u32(synth + 0x1c4),
                        "value": struct.unpack_from(">h", ram, synth + 0x1c8)[0],
                    },
                    "original_key": ram[synth + 0x131],
                    "key": struct.unpack_from(">H", ram, synth + 0x12e)[0],
                    "cents": struct.unpack_from(">b", ram, synth + 0x130)[0],
                    "midi_channel": ram[synth + 0x121],
                    "volume_16_16": u32(synth + 0x158),
                    "initial_volume_16_16": u32(synth + 0x15c),
                    "group_volume": floating(synth + 0x160),
                    "pan_16_16": u32(synth + 0x164),
                    "surround_pan_16_16": u32(synth + 0x168),
                    "volume_selector": struct.unpack_from(">H", ram, synth + 0x244)[0],
                    "volume_curve": ram[synth + 0x196],
                    "auxiliary_selectors": {
                        name: [
                            {"source": ram[synth + base + i*8],
                             "flags": ram[synth + base + i*8 + 1],
                             "scale": struct.unpack_from(">i", ram, synth + base + i*8 + 4)[0]}
                            for i in range(min(4, ram[synth + base + 0x22]))
                        ]
                        for name, base in [("pre_a", 0x344), ("post_a", 0x368),
                                           ("pre_b", 0x38c), ("post_b", 0x3b0)]
                    },
                    "auxiliary_a_scale_offset": list(ram[synth + 0x194:synth + 0x196]),
                }
            voices.append(voice)
        result["audio_setup"]["dsp_voices"] = voices

    def model_nodes(actor):
        # fn_8012B390 creates the node table, fn_8006CEB0 fills world/skin
        # matrices. These snapshots let us distinguish pose and shading errors.
        model = pointer(u32(actor + 0x100), 0x70)
        if model is None:
            return []
        count = struct.unpack_from(">H", ram, model + 6)[0]
        if count > 512:
            raise ValueError("unexpected model node count")
        table = pointer(u32(model + 0x18), count * 4)
        if table is None:
            raise ValueError("invalid model node table")
        nodes = []
        for index in range(count):
            node = pointer(u32(table + index * 4), 0x9c)
            if node is None:
                raise ValueError("invalid model node")
            matrices = {}
            for name, field in [("world", 0x84), ("skin", 0x88), ("inverse_bind", 0x90)]:
                matrix = pointer(u32(node + field), 48)
                if matrix is not None:
                    matrices[name] = [floating(matrix + i*4) for i in range(12)]
            nodes.append({
                "index": index, "id": struct.unpack_from(">H", ram, node)[0],
                "hierarchical": ram[node + 2] == 1,
                "draw_priority": ram[node + 3],
                "animated_flags": ram[node + 0x4c],
                "animated_scale": [floating(node + 0x50 + i*4) for i in range(3)],
                "animated_rotation": [floating(node + 0x5c + i*4) for i in range(4)],
                "animated_translation": [floating(node + 0x6c + i*4) for i in range(3)],
                **matrices,
            })
        return nodes

    def secondary_chains(actor):
        # fn_80069088 operates on the model state embedded at actor + 0x100.
        chain = pointer(u32(actor + 0x71c), 20)
        chains, seen = [], set()
        while chain is not None:
            if chain in seen or len(chains) >= 32:
                raise ValueError("invalid secondary-chain list")
            seen.add(chain)
            count = struct.unpack_from(">h", ram, chain + 4)[0]
            if not 1 <= count <= 128:
                raise ValueError("invalid secondary-chain length")
            segments = pointer(u32(chain), count * 64)
            if segments is None:
                raise ValueError("invalid secondary-chain segments")
            joints = []
            for index in range(count):
                at = segments + index * 64
                joints.append({"node": u32(at + 60),
                    **{name: [floating(at + offset + i*4) for i in range(3)]
                       for name, offset in [("position", 0), ("previous", 12),
                                            ("target", 24), ("velocity", 36)]},
                    "length": floating(at + 48), "gravity": floating(at + 52),
                    "damping": floating(at + 56)})
            chains.append({"flags": ram[chain + 6], "attraction": floating(chain + 8),
                           "callback": f"{u32(chain + 16):08x}", "joints": joints})
            chain = pointer(u32(chain + 12), 20)
        return chains

    def actor_observation(at):
        callback = u32(at)
        actor = {
            "address": at + 0x80000000,
            "id": struct.unpack_from(">i", ram, at + 0xb8)[0],
            "draw_callback": f"{callback:08x}",
            "layer": struct.unpack_from(">b", ram, at + 0x94)[0],
            "position": [floating(at + 4 + i*4) for i in range(3)],
            "presentation_position": [floating(at + 0x1c + i*4) for i in range(3)],
            "rotation": [floating(at + 0x50 + i*4) for i in range(3)],
            "heading_current": floating(at + 0x40),
            "heading_target": floating(at + 0x74),
            "turn_step": floating(at + 0x78),
            "turn_speed": floating(at + 0x80),
            "behavior": struct.unpack_from(">H", ram, at + 0x96)[0],
            "autonomy": {
                "kind": ram[at + 0x9a],
                "override": struct.unpack_from(">H", ram, at + 0x98)[0],
                "timer": struct.unpack_from(">i", ram, at + 0xb0)[0],
                "speed": floating(at + 0x790),
                "home": [floating(at + 0x838 + i*4) for i in range(3)],
                "radius": floating(at + 0x844),
                "floor_attributes": u32(at + 0xa8)},
            "scale": [floating(at + 0x5c + i*4) for i in range(3)],
            "flags_9c": ram[at + 0x9c],
            "shadow": {"flags": ram[at + 0x9b], "node": ram[at + 0x9f],
                "alpha": ram[at + 0xa0], "radius": floating(at + 0xa4),
                "ground_height": floating(at + 0x7c),
                "rotation": [floating(at + 0x68 + i*4) for i in range(3)]},
            "model_alpha": ram[at + 0x6e9],
            "appearance_channels": list(ram[at + 0xc4:at + 0xdb]),
            "lighting": {"position": [floating(at + 0x140 + i*4) for i in range(3)],
                "kind": ram[at + 0x165], "alpha": ram[at + 0x166],
                "ambient": list(ram[at + 0x6e6:at + 0x6ea]),
                "colors": list(ram[at + 0x6ee:at + 0x6f6])},
            "resource_address": f"{u32(at + 0xc0):08x}",
        }
        track = pointer(u32(at + 0x6fc), 0x34)
        tracks, seen = [], set()
        while track is not None and track not in seen and len(tracks) < 32:
            seen.add(track)
            tracks.append({"address": f"{track + 0x80000000:08x}",
                "time": floating(track + 8), "start": floating(track + 12),
                "end": floating(track + 16), "loop_start": floating(track + 20),
                "speed": floating(track + 24), "flags": u32(track + 28),
                "state_flags": ram[track + 32],
                "blend_duration": floating(track + 36), "blend_tick": floating(track + 40)})
            track = pointer(u32(track + 44), 0x34)
        actor["animation_tracks"] = tracks
        resource = pointer(u32(at + 0xc0), 0x80)
        actor["script_animation_slots"] = (
            [slot for slot in range(12, 0x80, 4)
             if u32(resource + slot) and u32(at + 0xc0) + u32(resource + slot) == u32(at + 0x82c)]
            if resource is not None and ram[at + 0x824] else [])
        if callback in (0x8001a6fc, 0x8000e720):
            actor["model_nodes"] = model_nodes(at)
            if callback == 0x8001a6fc:
                actor["secondary_chains"] = secondary_chains(at)
        elif callback == 0x8007e1bc:
            # Location lettering uses one shared animation controller.
            state = 0x2cb3e8
            short = lambda offset: struct.unpack_from(">H", ram, state + offset)[0]
            actor["location_caption"] = {
                "hold_remaining": floating(at + 0x7c), "alpha": ram[at + 0x851],
                "width": u32(state), "progress": u32(state + 4),
                "complete": bool(ram[state + 8]), "frame": short(10),
                "phase": short(12), "bar_alpha": short(14),
                "phase_done": bool(ram[state + 16]), "multi": bool(ram[state + 17]),
                "main_width": short(18), "main_alpha": short(20),
                "entries_started": bool(ram[state + 22]), "count": short(24),
                "total_width": short(26),
                "entry_alpha": [short(28 + i*2) for i in range(min(10, short(24)))],
                "entry_width": [short(48 + i*2) for i in range(min(10, short(24)))],
            }
        elif callback == 0x800157e8:
            actor["emote"] = {
                "actor": struct.unpack_from(">i", ram, at + 0x740)[0],
                "kind": struct.unpack_from(">H", ram, at + 0x98)[0],
                "remaining": struct.unpack_from(">i", ram, at + 0xb0)[0],
                "radius": floating(at + 0x790),
                "clock": u32(at + 0x834),
            }
        return actor

    result["controlled_actor"] = actor_observation(0x2c7ea0)
    # The field exit service approaches a scenery door, plays the opening pose,
    # turns the door node and fades before allowing the destination to activate.
    door = 0x2c8948
    result["field_exit"] = {
        "flags": ram[door], "phase": ram[door + 1],
        "approach_ticks": ram[door + 2],
        "angle_limit": floating(door + 4), "opened_angle": floating(door + 8),
        "width": floating(door + 12), "heading": floating(door + 16),
        "approach_position": [floating(door + 0x24 + i*4) for i in range(3)],
        "node_address": f"{u32(door + 0x30):08x}",
        "interaction_radius": floating(0x35a55c),
    }
    result["secondary_global_force"] = [floating(0x2caaa0 + i * 4) for i in range(3)]
    pool = pointer(u32(0x35a4e4), 0x860)
    if pool is not None:
        actors = []
        for slot in range(min(4096, (len(ram) - pool) // 0x860)):
            at = pool + slot * 0x860
            callback = u32(at)
            if callback == 0xffffffff:
                break
            if callback == 0:
                continue
            actor = {"slot": slot, **actor_observation(at)}
            actors.append(actor)
        else:
            raise ValueError("actor table has no bounded terminator")
        result["actors"] = actors
    # Static field groups have their own animation/texture controllers; they
    # do not appear in the script-created actor table.
    groups = []
    for resource, script_actor, at in [(0, 999996, 0x2bf4c0), (2, 999998, 0x2bdba0),
                                      (None, 999997, 0x2bec60), (12, 999980, 0x2be400)]:
        if not u32(at + 0x100):
            continue
        tracks = []
        seen = set()
        track = pointer(u32(at + 0x6fc), 0x34)
        while track is not None and track not in seen and len(tracks) < 32:
            seen.add(track)
            if u32(track):
                tracks.append({
                    "address": f"{track + 0x80000000:08x}",
                    "time": floating(track + 8), "end": floating(track + 16),
                    "speed": floating(track + 24), "flags": u32(track + 28),
                })
            track = pointer(u32(track + 44), 0x34)
        count = ram[at + 0x16c]
        if count > 8:
            raise ValueError("unexpected field texture-matrix count")
        groups.append({"resource": resource, "script_actor": script_actor,
                       "address": f"{at + 0x80000000:08x}", "animation_tracks": tracks,
                       "model_nodes": model_nodes(at),
                       "texture_matrices": [{
                           "texture": struct.unpack_from(">H", ram, at + 0x1a0 + i*0x34)[0],
                           "matrix": [floating(at + 0x170 + i*0x34 + j*4) for j in range(12)],
                       } for i in range(count)]})
    result["field_groups"] = groups
    result["field_texture_effect"] = {
        "flags": ram[0x2bfd20],
        "textures": [u32(0x2bfd24 + i*4) for i in range(3)],
        "timers": [floating(0x2bfd60 + i*4) for i in range(3)],
        "progress": [floating(0x2bfd80 + i*4) for i in range(3)],
        "steps": floating(0x35b354), "step_size": floating(0x35b358),
        "first_interval": floating(0x35b360), "scroll_speed": floating(0x35b364),
        "second_interval": floating(0x35b368),
    }
    # The title event creates kind-10 feather glow particles in this pool.
    # Read the transient emote sprites after draw submission. Width is cleared
    # by the renderer, but placement/UV/height retain the submitted observation.
    emote_pool = u32(0x35a4f4) - 0x80000000
    if 0 <= emote_pool <= len(ram) - 40 * 28:
        result["emote_sprites"] = [{
            "slot": index,
            "position": list(struct.unpack_from(">hhh", ram, emote_pool + index * 28)),
            "angle": struct.unpack_from(">h", ram, emote_pool + index * 28 + 6)[0],
            "uv": list(ram[emote_pool + index * 28 + 8:emote_pool + index * 28 + 12]),
            "color": list(ram[emote_pool + index * 28 + 12:emote_pool + index * 28 + 16]),
            "size": list(struct.unpack_from(">HH", ram, emote_pool + index * 28 + 20)),
            "mode": ram[emote_pool + index * 28 + 24],
        } for index in range(40) if u32(emote_pool + index * 28 + 20) != 0]
    pool = u32(0x35a4fc) - 0x80000000
    result["particle_pool_address"] = f"{pool + 0x80000000:08x}"
    if 0 <= pool <= len(ram) - 0x800 * 0x6c:
        particles = []
        for index in range(0x800):
            at = pool + index * 0x6c
            timer = struct.unpack_from(">h", ram, at)[0]
            if timer < 0:
                continue
            particles.append({
                "slot": index, "timer": timer, "flags": ram[at + 2],
                "position": [floating(at + 4 + i*4) for i in range(3)],
                "velocity": [floating(at + 0x38 + i*4) for i in range(3)],
                "rotation": [floating(at + 0x10 + i*4) for i in range(3)],
                "angular_velocity": [floating(at + 0x44 + i*4) for i in range(3)],
                "size": [floating(at + 0x28), floating(at + 0x2c)],
                "size_delta": floating(at + 0x54),
                "fade_value": struct.unpack_from(">h", ram, at + 0x58)[0],
                "fade_delta": struct.unpack_from(">h", ram, at + 0x5a)[0],
                "motion_flags": ram[at + 0x5c],
                "draw_mode": ram[at + 0x5d],
                "rotation_mode": ram[at + 0x5e],
                "rgba": list(ram[at + 0x20:at + 0x24]),
                "uv_bytes": list(ram[at + 0x1c:at + 0x20]),
                "recipe_address": f"{u32(at + 0x30):08x}",
                "callback_address": f"{u32(at + 0x68):08x}",
            })
        result["particles"] = particles
    # fn_80018278 submits transient ground-shadow quads into a separate pool.
    # Keep their transforms/texture descriptor as observations, never assets.
    result["shadow_texture_words"] = [u32(0x2bff64 + i*4) for i in range(8)]
    words = result["shadow_texture_words"]
    width, height = (words[2] & 1023) + 1, ((words[2] >> 10) & 1023) + 1
    texture = (words[3] & 0xffffff) << 5
    if words[5] == 3 and texture + width*height*2 <= len(ram):
        result["shadow_texture"] = {"width": width, "height": height,
            "format": "IA8", "data_sha256": hashlib.sha256(
                ram[texture:texture+width*height*2]).hexdigest()}
    pool = pointer(u32(0x35a500), 0x4a * 0x6c)
    if pool is not None:
        result["contact_shadows"] = []
        for index in range(0x4a):
            at = pool + index * 0x6c
            timer = struct.unpack_from(">h", ram, at)[0]
            if timer < 0:
                continue
            result["contact_shadows"].append({
                "slot": index, "timer": timer, "flags": ram[at + 2],
                "position": [floating(at + 4 + i*4) for i in range(3)],
                "rotation": [floating(at + 0x10 + i*4) for i in range(3)],
                "half_size": [floating(at + 0x28), floating(at + 0x2c)],
                "rgba": list(ram[at + 0x20:at + 0x24]),
                "uv_bytes": list(ram[at + 0x1c:at + 0x20]),
                "texture_address": u32(at + 0x24),
            })
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("state", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--lz4", help="Explicit LZ4 library path, if outside nix develop")
    args = parser.parse_args()
    args.output.write_text(json.dumps(inspect(args.state, args.lz4), indent=2, allow_nan=False) + "\n")
