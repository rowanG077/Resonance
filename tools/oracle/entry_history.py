"""Retain observed noncombat placement and controller changes for native setup."""
import math
import struct

CONTROLLER_FIELDS = ((0x10, "end"), (0x14, "loop_start"), (0x18, "rate"),
                     (0x1c, "flags"), (0x24, "blend_duration"))


def number(word):
    value = struct.unpack(">f", struct.pack(">I", word))[0]
    if not math.isfinite(value):
        raise ValueError("noncombat controller or placement contains a nonfinite value")
    return value


def observe(rows, setup, started):
    initial = setup["initial_battle"]
    if started is None or initial["combat_tick"] != 0:
        return {}, []
    ready = initial["presentation_tick"]
    boundary = started["presentation_tick"] - started["combat_tick"]
    if boundary <= ready:
        return {}, []
    prelude = [row for row in rows if ready < row["battle_presentation_tick_word"] <= boundary]
    if not prelude:
        return {}, []
    actors = setup["source_actor_map"]
    positions = [actor["position"] for actor in setup["actors"]]
    previous = [dict(resource=pose["resource"], flags=pose["flags"],
                     end=pose["end"], loop_start=pose["loop_start"], rate=pose["rate"])
                for pose in setup["ready_model_observations"]]
    placements, transitions, missing = [], [], set()
    for row in prelude:
        tick = row["battle_presentation_tick_word"]
        if row["battle_combat_tick_word"] != 0:
            raise ValueError("noncombat setup history includes a combat visit")
        for actor, identity in enumerate(actors):
            slot = identity["source_slot"]
            position = [number(row[f"battle_actor_{slot}_{axis}_bits"]) for axis in "xyz"]
            if position != positions[actor]:
                placements.append(dict(after_updates=tick - ready, actor=actor, position=position))
                positions[actor] = position
            prefixes = [f"actor_{slot}_animation_{link}" for link in range(4)
                        if row.get(f"actor_{slot}_animation_{link}_layer_word", 0) & 0xffff == 6
                        and row.get(f"actor_{slot}_animation_{link}_resource", 0)]
            if len(prefixes) != 1:
                missing.add("noncombat setup lacks an unambiguous main animation controller")
                continue
            prefix = prefixes[0]
            if row[prefix + "_child"]:
                prefix += "_child"
            current = dict(resource=row[prefix + "_resource"])
            for _, name in CONTROLLER_FIELDS:
                key = prefix + "_" + name + "_word"
                if key in row:
                    current[name] = row[key] if name == "flags" else number(row[key])
            prior = previous[actor]
            changed = current["resource"] != prior["resource"] or any(
                name in current and name in prior and current[name] != prior[name]
                for name in ("rate", "end", "loop_start")) or (
                "flags" in current and "flags" in prior
                and current["flags"] & 10 != prior["flags"] & 10)
            previous[actor] = current
            if not changed:
                continue
            if any(name not in current for _, name in CONTROLLER_FIELDS):
                missing.add("noncombat setup animation transition lacks controller playback observations")
                continue
            clips = [int(clip) for clip, resource in setup["body_resources"][str(slot)].items()
                     if resource == current["resource"]]
            blend = current["blend_duration"]
            if len(clips) != 1 or not blend.is_integer() or not 0 <= blend <= 255:
                missing.add("noncombat setup animation transition has no exact clip or blend duration")
                continue
            transition = dict(after_updates=tick - ready, actor=actor, clip=clips[0],
                frame=number(row[prefix + "_frame_bits"]), end=current["end"],
                loop_start=current["loop_start"], rate=0.0 if current["flags"] & 2 else current["rate"],
                repeat=not bool(current["flags"] & 8), blend=int(blend))
            if slot < 4 and clips[0] >= 100:
                names = ("header", "descriptor", "duration")
                if all(prefix + "_motion_" + name in row for name in names):
                    transition["source_motion"] = {name: row[prefix + "_motion_" + name] for name in names}
                    transition["source_motion"]["duration"] = number(transition["source_motion"]["duration"])
                else:
                    missing.add("external entry motion lacks its observed resource binding")
            transitions.append(transition)
    return dict(entry_placements=placements, entry_model_transitions=transitions), sorted(missing)
