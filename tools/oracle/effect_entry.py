"""Effect-list evidence at the initial, independently replayable battle visit."""

LISTS = (("projectile", 0x75c40), ("program", 0x75ec8),
         ("particle", 0x763d8), ("alternate", 0x76660))


def observe(state, battle):
    if battle["combat_tick"] != 0 or battle["presentation_tick"] > 1:
        return {}
    pool = battle["pool"]
    lists = [dict(kind=kind, sentinel=pool + offset,
                  next=state.read(pool + offset), previous=state.read(pool + offset + 4))
             for kind, offset in LISTS]
    if battle["presentation_tick"] == 0:
        if any(item["next"] != item["sentinel"] or item["previous"] != item["sentinel"]
               for item in lists):
            raise ValueError("initial battle visit already contains unobserved effects")
        return dict(ready_effect_lists=lists)
    births = []
    particle_list = lists[2]
    address = particle_list["next"]
    while address != particle_list["sentinel"]:
        if not 0x80000000 <= address <= 0x817ffd78 or len(births) >= 416:
            raise ValueError("initial particle list is invalid or cyclic")
        age = state.read(address + 0x278) >> 16
        if state.read(address + 0x10) >> 24 != 1 or age != 1:
            raise ValueError("first effect visit contains a particle beyond its birth")
        parent = state.read(address + 0x184)
        if not 0x80000028 <= parent <= 0x817fff90:
            raise ValueError("first particle birth has no valid program parent")
        births.append(dict(address=address, parent=parent, age=age,
            copied=b"".join(state.read(address + offset).to_bytes(4, "big")
                            for offset in range(0x28, 0x40, 4)).hex(),
            signature=[state.read(parent - 0x28 + offset) for offset in (0x60, 0x6c, 0x2c)]))
        address = state.read(address)
    return dict(ready_effect_lists=lists, ready_particle_births=births)


def retained_birth(setup, index, presentation, address, parent, copied, signature, age):
    """Recognize the same age-one birth at the first resumed observation."""
    return (index == 0 and age == 2
            and presentation == setup["initial_battle"]["presentation_tick"] + 1
            and any(birth["address"] == address and birth["parent"] == parent
                    and birth["age"] == 1 and birth["copied"] == copied.hex()
                    and birth["signature"] == list(signature)
                    for birth in setup.get("ready_particle_births", [])))


def completed_condition_visits(state, battle, banks):
    """Retain an already completed ready aura visit, before fixture relocation.

    A snapshot can stop between actor visits. An age-one aura and its post-visit
    actor clock prove which ordinary condition visit has already happened. Only
    the actor placement and clock are replay inputs; particles are not imported.
    """
    if battle["combat_tick"] == 0:
        return [], []
    common = [(base, bank) for base, bank in banks.items()
              if bank["entry"] == "battle/effects/common/effects.json"]
    if len(common) != 1:
        raise ValueError("ready condition history needs one verified common bank")
    base, bank = common[0]
    root = base + bank["roots"][46]
    actors = {actor["address"]: (index, actor)
              for index, actor in enumerate(battle["actors"])}
    sentinel = battle["pool"] + 0x763d8
    address = state.read(sentinel)
    visits, proof, seen = {}, [], set()
    while address != sentinel:
        if not 0x80000000 <= address <= 0x817ffd78 or address in seen or len(seen) >= 416:
            raise ValueError("ready condition particle list is invalid or cyclic")
        seen.add(address)
        age = state.read(address + 0x278) >> 16
        parent = state.read(address + 0x184)
        if (state.read(address + 0x10) >> 24 == 1 and age == 1
                and 0x80000028 <= parent <= 0x817fff90):
            signature = [state.read(parent - 0x28 + offset) for offset in (0x60, 0x6c, 0x2c)]
            if signature[:2] == [base, root] and signature[2] in actors:
                index, actor = actors[signature[2]]
                clock = state.read(actor["address"] + 0x1bc, "H")
                active = state.read(actor["address"] + 0x107d, "B") & 7 == 4
                if not active or clock == 0 or (clock - 1) % 4:
                    raise ValueError("ready aura birth disagrees with its completed actor clock")
                visits[index] = dict(actor=index, position=actor["position"])
                proof.append(dict(actor=index, source_slot=actor["slot"], particle=address,
                    parent=parent, signature=signature, age=age, effect_clock=clock,
                    copied=b"".join(state.read(address + offset).to_bytes(4, "big")
                                    for offset in range(0x28, 0x40, 4)).hex(),
                    presentation_tick=battle["presentation_tick"], combat_tick=battle["combat_tick"]))
        address = state.read(address)
    return [visits[index] for index in sorted(visits)], proof


def completed_condition_birth(setup, index, presentation, address, parent, copied, age):
    """Bind the first resumed birth to its completed ready visit.

    Its short scheduler can already be retired at the resumed sample. Require
    the same allocation, parent, copied declaration and consecutive age before
    using the independently observed ready signature.
    """
    if (index != 0 or age != 2
            or presentation != setup["initial_battle"]["presentation_tick"] + 1):
        return None
    for birth in setup.get("ready_condition_history", {}).get("observations", []):
        if (birth["particle"] == address and birth["parent"] == parent
                and birth["age"] == 1 and birth.get("copied") == copied.hex()):
            return tuple(birth["signature"])
    return None
