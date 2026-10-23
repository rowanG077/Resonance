"""Read the loaded battle REL and its live state; no memory writes or asset conversion."""
import struct


def inspect_battle(ram):
    def offset(address, size=4):
        value = address - 0x80000000
        if not 0 <= value <= len(ram) - size:
            raise ValueError(f"battle observation outside main RAM: {address:08x}")
        return value

    def read(address, fmt=">I"):
        return struct.unpack_from(fmt, ram, offset(address, struct.calcsize(fmt)))[0]

    def values(address, fmt, count):
        return list(struct.unpack_from(">" + str(count) + fmt, ram,
                                      offset(address, struct.calcsize(">" + str(count) + fmt))))

    route = 0x802cb554
    result = {"mode": read(0x8035a762, ">H") & 0x7f,
              "route_flags": read(route, ">B"),
              "formation": read(route + 4), "arena": read(route + 8),
              "result": read(route + 0x20), "callback": read(0x8035a518),
              "module": read(0x8035a86c)}
    settings = read(0x8035a768)
    result["party_order"] = values(settings + 0xe9d, "B", 4)
    result["party_controls"] = values(settings + 0xea5, "B", 4)
    if not result["module"] or not result["callback"]:
        return result

    module = result["module"]
    if read(module) != 1:
        raise ValueError("expected active battle REL module 1")
    count = read(module + 0xc)
    if not 1 <= count <= 32:
        raise ValueError("invalid loaded battle REL section count")
    table = read(module + 0x10)
    sections = [{"index": i, "address": read(table + i * 8) & ~1,
                 "size": read(table + i * 8 + 4)} for i in range(count)]
    result["sections"] = sections
    # The original module declares its BSS section in OSModuleInfo + 0x33.
    bss_index = read(module + 0x33, ">B")
    if bss_index >= count or sections[bss_index]["size"] < 0x544:
        raise ValueError("invalid loaded battle REL BSS index")
    pool_pointer = sections[bss_index]["address"] + 0x540
    pool = read(pool_pointer)
    result.update(bss_index=bss_index, pool_pointer=pool_pointer, pool=pool)
    if not pool:
        return result
    offset(pool, 0x98980)
    # Battle dispatch, presentation and combat clocks.
    result.update(dispatch=read(pool + 0x15564, ">B"),
                  phase=read(pool + 0x156d9, ">B"),
                  presentation_tick=read(pool + 0x156fc),
                  gameplay_tick=read(pool + 0x15700),
                  combat_tick=read(pool + 0x15704),
                  pause_flags=read(pool + 0x15710, ">H"),
                  menu_flags=read(pool + 0x159c4),
                  actors=[])
    result["random_state"] = {"entry_seed": read(pool + 0x7a66c),
                              "state": read(pool + 0x7a670)}
    result["input_masks"] = dict(zip(("attack", "technique", "guard"),
                                     values(pool + 0x1566c, "H", 3)))
    # Stored scenes retain their associated spell lifetime.
    result["stored_transition"] = {
        "owner": read(pool + 0x159dc), "remaining": read(pool + 0x159e4, ">h"),
        "flags": read(pool + 0x159e8, ">B"), "slot": read(pool + 0x159e9, ">B"),
        "occupied": values(pool + 0x159ea, "B", 2),
    }
    result["stored_scenes"] = []
    for slot in range(2):
        scene = pool + 0x7edc0 + slot * 0x50a0
        result["stored_scenes"].append({
            "slot": slot, "resource": read(scene + 4), "owner": read(scene + 0x15c),
            "completion": read(scene + 0x160), "flags": read(scene + 0x164, ">H"),
            "technique": read(scene + 0x166, ">h"),
        })
    # Two actor banks and their vital records.
    for side in range(2):
        count = read(pool + 0x15948 + side, ">B")
        if count > (4 if side == 0 else 8):
            raise ValueError("battle actor bank exceeds its party/enemy capacity")
        seen = set()
        for index in range(count):
            roster_slot = read(pool + 0x1565c + side * 8 + index, ">B")
            if roster_slot >= (4 if side == 0 else 8) or roster_slot in seen:
                raise ValueError("invalid battle roster slot")
            seen.add(roster_slot)
            slot = roster_slot + side * 4
            actor = pool + 0x1cea0 + slot * 0x1ba0
            vitals = read(actor + 8)
            record = {"side": side, "slot": slot, "address": actor,
                      "kind": read(actor + 0x107e, ">B") >> 4,
                      "action": values(actor + 0x1ac, "B", 8),
                      "flags": values(actor + 0x107c, "B", 4),
                      "position": values(actor + 0x18c0, "f", 3),
                      "return_position": values(actor + 0x192c, "f", 3),
                      "formation_offset": values(actor + 0x1938, "f", 3),
                      "heading": read(actor + 0x18d0, ">f"),
                      "center": values(actor + 0x195c, "f", 3),
                      "target": read(actor + 0x1990), "vitals_address": vitals}
            profile = read(actor + 4)
            if profile:
                record.update(center_offset=values(profile + 0x60, "f", 3),
                              model_scale=read(profile + 0x84, ">f"))
            if vitals:
                record.update(max_hp=read(vitals + 0x20), hp=read(vitals + 0x24),
                              max_tp=read(vitals + 0x28, ">H"), tp=read(vitals + 0x2a, ">H"))
            result["actors"].append(record)
    # Active camera, applied shake and projection/view matrices.
    camera = read(pool + 0x198)
    result["camera"] = {"address": camera,
                        "eye": values(camera, "f", 3),
                        "target": values(camera + 12, "f", 3),
                        "up": values(camera + 24, "f", 3),
                        "fov_y": read(camera + 0x24, ">f"),
                        "shake": values(pool + 0x15640, "f", 3),
                        "view_matrix": values(pool + 0x1556c, "f", 12),
                        "projection_matrix": values(pool + 0x1559c, "f", 16),
                        "viewport": values(pool + 0x1560c, "f", 6)}
    return result


def presentation_locations(pointer="8035a86c 10 30 540"):
    """Scene camera and tint fields verified across two active scene snapshots."""
    return {f"{pointer} {offset:x}": name for offset, name in [
        (0xb0, "battle_scene_minimum_distance_bits"),
        (0xb4, "battle_scene_minimum_elevation_bits"),
        (0xbc, "battle_scene_camera_remaining_word"),
        (0x3376c, "battle_scene_tint_rgba"),
        (0x33770, "battle_scene_secondary_tint_rgba"),
        (0x33774, "battle_scene_tint_remaining_word"),
        (0x32004, "battle_stage_tint_rg_word"),
        (0x32008, "battle_stage_tint_ba_word"),
    ]}


def projectile_locations(pointer="8035a86c 10 30 540"):
    """Initialized projectile owner, lifetime, contact geometry and age fields."""
    locations = {}
    for slot in range(16):
        node = f"{pointer} 75c44" + " 4" * slot
        locations[node] = f"projectile_{slot}_address"
        for offset in (0x10, 0x28, 0x2C, 0x30, 0x34, 0x3C, 0x6C, 0x70, 0x74):
            locations[f"{node} {offset:x}"] = f"projectile_{slot}_{offset:x}"
    return locations


def watch_locations(battle):
    """Follow observed storage; retain pointer words so relocation is detectable."""
    locations = {"8035a760": "global_mode_word", "802cb554": "battle_route_flags_word",
                 "802cb558": "battle_formation", "802cb55c": "battle_arena",
                 "802cb574": "battle_result", "8035a518": "battle_callback",
                 "8035a86c": "battle_module", "8035a768 ea5": "party_control_types_word"}
    if not battle.get("pool"):
        return locations
    # Reloading a battle may relocate its module and BSS. Follow the section
    # table instead of watching the previous battle's absolute pool pointer.
    pointer = f'8035a86c 10 {battle["bss_index"] * 8:x} 540'
    locations.update(presentation_locations(pointer))
    locations[pointer] = "battle_pool"
    for at, name in [(0x15564, "dispatch"), (0x156d8, "phase"), (0x156fc, "presentation_tick"),
                     (0x15700, "gameplay_tick"), (0x15704, "combat_tick"),
                     (0x15710, "pause_flags"), (0x15948, "roster_counts"),
                     (0x159c4, "menu_flags"), (0x7a66c, "random_entry_seed"),
                     (0x7a670, "random_state"), (0x159dc, "stored_owner"),
                     (0x159e4, "stored_countdown"), (0x159e8, "stored_slots")]:
        locations[f"{pointer} {at:x}"] = f"battle_{name}_word"
    for slot in range(2):
        scene = 0x7edc0 + slot * 0x50a0
        for at, name in [(4, "resource"), (0x15c, "owner"), (0x160, "completion"),
                         (0x164, "flags_technique")]:
            locations[f"{pointer} {scene + at:x}"] = f"battle_stored_{slot}_{name}_word"
    locations[f"{pointer} 198"] = "battle_camera_address"
    for index in range(10):
        locations[f"{pointer} 198 {index * 4:x}"] = f"battle_camera_{index}_bits"
    for actor in battle["actors"]:
        base = 0x1cea0 + actor["slot"] * 0x1ba0
        label = f'battle_actor_{actor["slot"]}'
        for at, name in [(0x1ac, "action"), (0x1b0, "action_phase"),
                         (0x1bc, "cast_clock_word"), (0x1044, "body_clip_word"),
                         (0x16e0, "primary_phase_age_word"),
                         (0x17d8, "secondary_phase_age_word"),
                         (0x1ac4, "stored_slot_word"),
                         (0x107c, "flags"), (0x18c0, "x_bits"),
                         (0x18c4, "y_bits"), (0x18c8, "z_bits"), (0x1990, "target")]:
            locations[f"{pointer} {base + at:x}"] = f"{label}_{name}"
        for at, name in [(0x18d0, "heading"), (0x195c, "center_x"),
                         (0x1960, "center_y"), (0x1964, "center_z")]:
            locations[f"{pointer} {base + at:x}"] = f"{label}_{name}_bits"
        locations[f"{pointer} {base + 8:x}"] = f"{label}_vitals"
        for at, name in [(0x20, "max_hp"), (0x24, "hp"), (0x28, "tp_word")]:
            locations[f"{pointer} {base + 8:x} {at:x}"] = f"{label}_{name}"
    return locations
