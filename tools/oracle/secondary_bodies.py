"""Optional, independently bound secondary-rig controller observations."""
import math
import struct

from battle_geometry import escape_ascii


def ready(state, battle):
    result = []
    for actor, observed in enumerate(battle["actors"]):
        address = observed["address"]
        profile = state.read(address + 4)
        if not state.read(profile + 0xb4, "H") & 0x400:
            continue
        slot, offset, count = (state.read(profile + at, "B") for at in (0xbc, 0xbd, 0x1e8))
        record = state.read(address + 0x153c)
        if not 0 <= slot < count <= 6 or not offset or not 0x80000000 <= record < 0x817ff800:
            raise ValueError("invalid initialized secondary-body binding")
        model = state.read(record)
        skeleton, bone_count = state.read(model), state.read(model + 4)
        if not 0 < bone_count <= 256 or state.read(skeleton + 4) != bone_count:
            raise ValueError("invalid initialized secondary skeleton")
        names, cursor = [], state.read(skeleton + 0x1c) - 0x80000000
        for _ in range(bone_count):
            raw = bytes(state.ram[cursor:cursor + 128]).split(b"\0", 1)
            if len(raw) != 2 or not raw[0] or not raw[0].isascii():
                raise ValueError("secondary bone label is not bounded ASCII")
            names.append(escape_ascii(raw[0]))
            cursor += len(raw[0]) + 1
        table = state.read(address + 0x1010)
        resources = {str(clip): table + clip * 20 for clip in range(100)
                     if state.read(table + clip * 20 + 4)}
        controller = state.read(record + 0x5fc)
        bound = state.read(controller)
        clips = [int(clip) for clip, resource in resources.items() if resource == bound]
        flags, child = state.read(controller + 0x1c), state.read(controller + 0x30)
        if len(clips) != 1 or state.read(controller + 6, "H") not in (0, 6) or child:
            raise ValueError("secondary ready controller has no unique settled clip")
        fields = {name: state.read(controller + at, "f") for name, at in
                  (("frame", 8), ("loop_start", 20), ("rate", 24))}
        if not all(math.isfinite(value) for value in fields.values()):
            raise ValueError("nonfinite secondary ready controller")
        result.append(dict(actor=actor, source_slot=observed["slot"], model_slot=slot,
            motion_offset=offset, bones=names, resources=resources,
            ready=dict(clip=clips[0], **fields, repeat=not bool(flags & 8),
                       stopped=bool(flags & 2)),
            observation=dict(profile=profile, record=record, model=model, skeleton=skeleton,
                             controller=controller, resource=bound, flags=flags, child=child)))
    return result


def locations(pool, actor_base, stride, slots):
    result = {}
    for slot in slots:
        record = f"{pool} {actor_base + slot * stride + 0x153c:x}"
        prefix = f"actor_{slot}_secondary"
        result[record] = prefix + "_record"
        result[record + " 64"] = prefix + "_visibility"
        for link in range(2):
            node = record + " 5fc" + " 2c" * link
            for at, name in ((0, "resource"), (4, "layer"), (8, "frame"), (0x30, "child")):
                result[f"{node} {at:x}"] = f"{prefix}_{link}_{name}"
            result[node + " 30 0"] = f"{prefix}_{link}_child_resource"
            result[node + " 30 8"] = f"{prefix}_{link}_child_frame"
    return result


def observe(row, binding):
    prefix = f"actor_{binding['source_slot']}_secondary"
    if row[prefix + "_record"] != binding["observation"]["record"]:
        raise ValueError("registered secondary rig was replaced")
    resources = {resource: int(clip) for clip, resource in binding["resources"].items()}
    for link in range(2):
        node = f"{prefix}_{link}"
        if row[node + "_layer"] & 65535 not in (0, 6):
            continue
        selected = node + "_child" if row[node + "_child"] else node
        clip = resources.get(row[selected + "_resource"])
        if clip is None:
            continue
        frame = struct.unpack(">f", struct.pack(">I", row[selected + "_frame"]))[0]
        visible = (row[prefix + "_visibility"] >> 16) & 255
        if not math.isfinite(frame) or visible not in (0, 1):
            raise ValueError("invalid secondary frame/visibility")
        # Logical visibility, before camera-dependent draw culling.
        hidden = row[f"actor_{binding['source_slot']}_raw_280"] & 0x04000000
        return dict(actor=binding["actor"], clip=clip, frame=frame,
                    visible=bool(visible and not hidden))
    raise ValueError("secondary clip lacks a matched current/pending controller")
