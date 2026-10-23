"""Typed support outcomes from independently observed actor state.

Only mutable conditions belong here. Intrinsic and equipment layers stay
separate, and callers must establish attribution before using these events.
"""


CONDITION_BITS = {
    0: "poison_mild", 1: "poison_severe", 2: "stun", 3: "paralysis",
    4: "weak", 5: "petrified", 7: "curse", 8: "reduce_item_effect",
    9: "heavy", 10: "physical_affliction", 11: "physical_affliction",
    12: "attack_up", 13: "defense_up", 14: "accuracy_up",
    15: "magic_attack_up", 16: "magic_defense_up", 17: "attack_down",
    18: "defense_down", 19: "accuracy_down", 20: "magic_attack_down",
    21: "magic_defense_down", 22: "evasion_down", 23: "physical_protection",
    24: "magical_protection", 25: "revive", 26: "defense_halved",
    27: "regenerate_hp", 28: "regenerate_tp", 29: "acuity", 30: "flare",
    31: "guard", 32: "quartz", 33: "physical_barrier", 34: "magical_barrier",
    35: "holy_song", 36: "recovery_down", 37: "enchanted", 40: "short_stun",
    41: "ailment_resistance", 42: "kill_hp_recovery", 43: "kill_tp_recovery",
    44: "tp_third", 45: "tp_half", 47: "casting_speed", 56: "movement_boost",
}
ELEMENTS = (None, "water", "wind", "fire", "earth", "lightning", "ice", "light", "darkness")
AILMENTS = frozenset((
    "poison_mild", "poison_severe", "paralysis", "weak", "petrified", "curse",
    "reduce_item_effect", "heavy", "physical_affliction", "attack_down",
    "defense_down", "accuracy_down", "magic_attack_down", "magic_defense_down",
    "evasion_down",
))
BENEFICIAL_CONDITIONS = frozenset((
    "attack_up", "defense_up", "accuracy_up", "magic_attack_up", "magic_defense_up",
    "physical_barrier", "magical_barrier", "holy_song", "revive", "flare",
))
INITIAL_CURE_CONDITIONS = frozenset(("curse", "weak"))


def word(row, key):
    value = row[key]
    if type(value) is not int or not 0 <= value <= 0xffffffff:
        raise ValueError(f"invalid observed support word: {key}")
    return value


def conditions(high, low):
    """Decode the observed mutable bitmap without merging condition layers."""
    flags = (high << 32) | low
    unknown = flags & ~sum(1 << bit for bit in CONDITION_BITS)
    if unknown:
        raise ValueError(f"unsupported mutable support conditions: {unknown:#x}")
    return sorted({name for bit, name in CONDITION_BITS.items() if flags & (1 << bit)})


def observed_conditions(row, slot):
    return conditions(word(row, f"actor_{slot}_condition_10"),
                      word(row, f"actor_{slot}_condition_14"))


def competing_action(row, setup, owner):
    return any(word(row, f"battle_actor_{actor['source_slot']}_action") >> 24 == 5
               for actor in setup["source_actor_map"] if actor["actor"] != owner)


def snapshot(row, slot):
    prefix = f"actor_{slot}_"
    element = word(row, prefix + "raw_1e0") & 255
    if element >= len(ELEMENTS):
        raise ValueError(f"unsupported observed enchantment element: {element}")
    guard = (word(row, prefix + "raw_280") >> 8) & 255
    return {
        "conditions": observed_conditions(row, slot),
        "tp": word(row, f"battle_actor_{slot}_tp_word") & 65535,
        "enchantment": ELEMENTS[element],
        "special_guard": guard & 0x41 == 0x41,
    }


def time_stop(row, slot):
    remaining = word(row, "battle_time_stop_word") >> 16
    return remaining if word(row, f"actor_{slot}_time_stop_flags") & 1 else 0


def validate_initial(value, *, caster=False):
    """Cure fixtures use lasting ailments on an ally, with no unseen timer."""
    allowed = frozenset() if caster else INITIAL_CURE_CONDITIONS
    if not set(value["conditions"]) <= allowed:
        raise ValueError("initial support conditions need an independently observed lifetime")
    if value["enchantment"] is not None or value["special_guard"]:
        raise ValueError("initial support stance or enchantment needs an observed lifetime")


def transition(before, after, *, owner, target, common=None):
    """Project changes after the caller has verified sole-action attribution.

    Loss of beneficial conditions and enchantment removal remain visible for
    comparison, but do not qualify as benefits. TP payment stays separate.
    """
    base = dict(common or {}, owner=owner, target=target, action=1)
    events = []
    for field, kind in (("conditions", "condition_changed"),
                        ("enchantment", "enchantment_changed")):
        if before[field] != after[field]:
            events.append(dict(base, kind=kind, before=before[field], after=after[field]))
    if after["tp"] > before["tp"]:
        events.append(dict(base, kind="tp_recovered", amount=after["tp"] - before["tp"]))
    if after["special_guard"] and not before["special_guard"]:
        events.append(dict(base, kind="special_guard_started"))
    return events


def beneficial(event):
    """A completed support case must contain an attributable typed benefit."""
    kind = event["kind"]
    if kind == "time_stop_started":
        return event["ticks"] > 0
    if kind in ("recovered", "tp_recovered"):
        return event["amount"] > 0
    if kind == "condition_changed":
        before, after = set(event["before"]), set(event["after"])
        return bool((before - after) & AILMENTS or (after - before) & BENEFICIAL_CONDITIONS)
    if kind == "enchantment_changed":
        return event["after"] is not None and event["after"] != event["before"]
    return kind == "special_guard_started"


def cast_recipient(rows, setup, expected, frames, events):
    """Bind the released spell's recipient separately from the combat target."""
    owner = setup["owner"]
    releases = [event for event in events if event["kind"] == "recovery_started"
                and event["owner"] == owner]
    starts = [event for event in events if event["kind"] == "started"
              and event["owner"] == owner]
    if len(releases) != 1 or len(starts) != 1 or not frames:
        raise ValueError("support cast lacks one observed admission and release")
    release = releases[0]
    if release["action"] != starts[0]["action"]:
        raise ValueError("support release belongs to a different action")
    matches = [index for index, row in enumerate(rows)
               if row["battle_combat_tick_word"] == frames[0]["combat_tick"] + release["clock"]
               and row["battle_presentation_tick_word"] == frames[0]["presentation_tick"] + release["presentation_clock"]]
    if len(matches) != 1 or matches[0] == 0:
        raise ValueError("support release has no unique adjacent source observation")
    index = matches[0]
    before, after = rows[index - 1], rows[index]
    slot = setup["source_actor_map"][owner]["source_slot"]
    prefix = f"actor_{slot}_"
    if (word(before, f"battle_actor_{slot}_action") >> 24 != 5 or
            word(after, f"battle_actor_{slot}_action") >> 24 != 8 or
            any(word(row, prefix + "native") & 65535 != expected["native"]
                for row in (before, after))):
        raise ValueError("support recipient lacks the bound cast-to-recovery transition")
    pointer = word(after, prefix + "spell_support_target")
    selections = (prefix + "support_target", f"battle_actor_{slot}_target")
    selected = next((field for field in selections
                     if all(word(row, field) == pointer for row in (before, after))), None)
    if selected is None:
        raise ValueError("released spell recipient differs from both stable selections")
    recipients = [index for index, actor in enumerate(setup["initial_battle"]["actors"])
                  if actor["address"] == pointer]
    if len(recipients) != 1 or setup["actors"][recipients[0]]["side"] != setup["actors"][owner]["side"]:
        raise ValueError("support recipient is not a unique allied actor")
    return dict(recipient=recipients[0], source_slot=slot,
                clock=release["clock"], presentation_clock=release["presentation_clock"],
                support_pointer=word(after, prefix + "support_target"),
                spell_support_pointer=pointer, selected_field=selected,
                combat_target_unchanged=True)


def seal_drain(row, previous, setup, events, common, vital):
    """Bind seal recovery to its observed last unsuppressed contact."""
    native, percent = {"hp": (72, 10), "tp": (73, 3)}[vital]
    owner = setup["owner"]
    slot = setup["source_actor_map"][owner]["source_slot"]
    prefix, body = f"actor_{slot}_", f"battle_actor_{slot}_"
    if (word(previous, body + "cast_clock_word") & 65535 != 15 or
            word(row, body + "cast_clock_word") & 65535 != 16 or
            word(row, prefix + "native") & 65535 != native or
            word(row, body + "action") >> 24 != 5 or
            setup["opponent_control"]["mode"] != "manual" or
            setup["damage_context"][owner]["blocked"] or
            any(member["compound_ex_skills"] or any(member["ex_skills"])
                for member in setup["party_members"])):
        return None
    for actor in setup["source_actor_map"]:
        observed = f"actor_{actor['source_slot']}_"
        if (any(word(row, observed + field) for field in (
                "condition_10", "condition_14", "condition_38", "condition_3c",
                "raw_1d0", "raw_1d4")) or
                (actor["actor"] != owner and
                 word(row, f"battle_actor_{actor['source_slot']}_action") >> 24 == 5)):
            return None
    contact = next((event for event in reversed(events)
                    if event["kind"] in ("hit", "unattributed_hp_loss")), None)
    started = next((event for event in events if event["kind"] == "started"), None)
    damage = word(row, prefix + "last_damage_word") & 65535
    if (contact is None or started is None or contact["kind"] != "hit" or
            contact["owner"] != owner or contact["projectile"] is not None or
            not 0 < damage < 32768 or damage != -contact["hp_change"] or
            contact["combat_tick"] >= common["combat_tick"]):
        return None
    victim = setup["initial_battle"]["actors"][contact["target"]]["address"]
    if (word(row, prefix + "contact_target") != victim or
            word(row, prefix + "effective_target") != victim):
        return None
    if vital == "tp":
        before, after = (word(sample, body + "tp_word") & 65535 for sample in (previous, row))
        maximum = word(row, body + "tp_word") >> 16
    else:
        before, after = word(previous, body + "hp"), word(row, body + "hp")
        maximum = word(row, body + "max_hp")
    amount = min(max(1, damage * percent // 100), maximum - before)
    if word(previous, body + "hp") <= 0 or amount <= 0 or after - before != amount:
        return None
    return dict(common, kind=f"{vital}_drained", owner=owner, target=owner, action=1,
                amount=amount, contact_clock=contact["combat_tick"] - started["combat_tick"],
                contact_presentation_clock=contact["presentation_tick"] - started["presentation_tick"])


def enemy_debuffs(case, previous, row, setup, events, common, admission):
    """Observe Retribution's two direct ailments before periodic damage begins."""
    if (case.get("enemy"), case.get("action_row")) != (239, 5):
        return []
    owner = setup["owner"]
    owner_slot = setup["source_actor_map"][owner]["source_slot"]
    clock = f"battle_actor_{owner_slot}_cast_clock_word"
    if word(previous, clock) & 65535 != 119 or word(row, clock) & 65535 != 120:
        return []
    # The caller has bound the requested action's live selection and tracks.
    if (word(row, f"actor_{owner_slot}_target_state_word") >> 24 != 3
            or setup["opponent_control"]["mode"] != "manual"
            or row["battle_combat_tick_word"] != previous["battle_combat_tick_word"] + 1
            or not any(event["kind"] == "program_started" and event["owner"] == owner
                       and event["program"] == 7 and event["bank"] is not None
                       and event["combat_tick"] == admission for event in events)
            or competing_action(row, setup, owner)):
        raise ValueError("enemy debuff lacks its sole owned action callback")
    groups = ({"curse", "paralysis", "poison_severe"},
              {"attack_down", "defense_down", "weak"})
    changes = []
    for actor in setup["source_actor_map"]:
        target, slot = actor["actor"], actor["source_slot"]
        if setup["actors"][target]["side"] == setup["actors"][owner]["side"]:
            continue
        before, after = [observed_conditions(sample, slot) for sample in (previous, row)]
        if before == after:
            continue
        if (before or len(after) != 2 or any(len(set(after) & group) != 1 for group in groups)
                or word(row, f"battle_actor_{slot}_hp") <= 0
                or word(previous, f"battle_actor_{slot}_hp") != word(row, f"battle_actor_{slot}_hp")
                or word(row, f"actor_{slot}_fresh_contact_word") >> 24):
            raise ValueError("enemy debuff lacks its two living opponent ailment transitions")
        changes.append(dict(common, kind="condition_changed", owner=owner,
                            target=target, action=1, before=before, after=after))
    return changes


def spell_debuffs(case, rows, setup, frames, events):
    """Attribute non-damaging status changes to an owned, released spell scene."""
    execution = case.get("definition", {}).get("execution", {})
    native = case.get("native", execution.get("spell") if isinstance(execution, dict) else None)
    if native != 265:
        return []
    owner = setup["owner"]
    owner_slot = setup["source_actor_map"][owner]["source_slot"]
    owner_address = setup["initial_battle"]["actors"][owner]["address"]
    registrations = setup.get("stored_scenes", {}).get("registrations", [])
    banks = {scene["bank_sha256"] for scene in registrations
             if scene["native"] == native and scene["owner"] == owner_address}
    starts = [event for event in events if event["kind"] == "started" and event["owner"] == owner]
    if len(starts) != 1 or len(banks) != 1 or not frames:
        raise ValueError("spell debuff lacks one admitted action and its bound scene")
    bank = banks.pop()
    by_clock = {(row["battle_combat_tick_word"], row["battle_presentation_tick_word"]): index
                for index, row in enumerate(rows)}
    scene_frames = [(frame, program) for frame in frames for program in frame["programs"]
                    if program["owner"] == owner and program["bank"] == bank and program["program"] == 1]
    if not scene_frames or len({program["id"] for _, program in scene_frames}) != 1:
        raise ValueError("spell debuff lacks one observed scene instance")
    opening, scene = scene_frames[0]
    admission = rows[by_clock[(opening["combat_tick"], opening["presentation_tick"])]]
    if scene["age"] > 1 or word(admission, f"actor_{owner_slot}_native") & 65535 != native:
        raise ValueError("spell debuff scene was not observed at its bound release")
    changes = []
    for frame in frames:
        index = by_clock[(frame["combat_tick"], frame["presentation_tick"])]
        if index == 0:
            continue
        previous, row = rows[index - 1:index + 1]
        for actor in setup["source_actor_map"]:
            target, slot = actor["actor"], actor["source_slot"]
            if setup["actors"][target]["side"] == setup["actors"][owner]["side"]:
                continue
            before, after = [observed_conditions(sample, slot) for sample in (previous, row)]
            if before == after:
                continue
            if (setup["opponent_control"]["mode"] != "manual"
                    or competing_action(row, setup, owner)
                    or not any(program["id"] == scene["id"] for program in frame["programs"])
                    or word(row, f"battle_actor_{slot}_hp") <= 0
                    or word(previous, f"battle_actor_{slot}_hp") != word(row, f"battle_actor_{slot}_hp")
                    or "defense_down" in before
                    or set(after) != set(before) | {"defense_down"}):
                raise ValueError("spell debuff lacks its sole owned scene and living opponent transition")
            changes.append(dict(kind="condition_changed", owner=owner, target=target,
                action=starts[0]["action"], before=before, after=after,
                clock=frame["combat_tick"] - frames[0]["combat_tick"],
                presentation_clock=frame["presentation_tick"] - frames[0]["presentation_tick"]))
    return changes


def seal_branches(case, rows, setup, frames, events, banks):
    """Register witnessed conditional seal emissions without forcing their rolls."""
    native = case.get("native")
    if native is None:
        execution = case.get("definition", {}).get("execution")
        physical = execution.get("physical") if isinstance(execution, dict) else None
        native = (physical or {}).get("technique")
    supported = {
        technique: binding
        for techniques, binding in (
            ((63, 64, 65), ("defense_down", 134)),
            ((66, 67, 68), ("evasion_down", 59)),
            ((69, 70, 71), ("accuracy_down", 135)),
            ((77,), ("recovery_down", 85)),
        )
        for technique in techniques
    }
    if native not in supported:
        return [], []
    condition, program = supported[native]
    owner = setup["owner"]
    slot = setup["source_actor_map"][owner]["source_slot"]
    bank = {value["source_sha256"] for value in banks.values()
            if value["path"].endswith("/techniques.json")}
    if len(bank) != 1 or not frames:
        raise ValueError("seal callback lacks its bound effect bank or execution window")
    binding = dict(owner=owner, bank=bank.pop(), program=program)
    matches = lambda event: all(event.get(key) == value for key, value in binding.items())
    emissions = [event for event in events if event["kind"] == "program_started" and matches(event)]
    onset = frames[0]["combat_tick"]
    observed = {row["battle_combat_tick_word"]: index for index, row in enumerate(rows)}
    if native == 77:
        # Force Seal applies directly to its selected enemy without a strike.
        changes = []
        for frame in frames:
            index = observed[frame["combat_tick"]]
            if not index:
                continue
            previous, row = rows[index - 1:index + 1]
            for entry in setup["source_actor_map"]:
                target, target_slot = entry["actor"], entry["source_slot"]
                before, after = [observed_conditions(sample, target_slot)
                                 for sample in (previous, row)]
                if before == after:
                    continue
                address = setup["initial_battle"]["actors"][target]["address"]
                phase = f"battle_actor_{slot}_cast_clock_word"
                attachments = {active["attachment"]["actor"] for active in frame["programs"]
                    if matches(active) and (active.get("attachment") or {}).get("kind") == "actor"}
                if (target != setup["target"] or
                        setup["actors"][target]["side"] == setup["actors"][owner]["side"] or
                        word(row, f"battle_actor_{slot}_target") != address or
                        word(previous, phase) & 65535 != 60 or word(row, phase) & 65535 != 61 or
                        word(row, f"actor_{slot}_native") & 65535 != native or
                        word(row, f"battle_actor_{slot}_action") >> 24 != 5 or
                        competing_action(row, setup, owner) or
                        word(previous, f"battle_actor_{target_slot}_hp") != word(row, f"battle_actor_{target_slot}_hp") or
                        condition in before or set(after) != set(before) | {condition} or
                        attachments != {owner, target}):
                    raise ValueError("direct seal condition lacks its owned selected-target callback")
                changes.append(dict(kind="condition_changed", owner=owner, target=target,
                    action=1, before=before, after=after, clock=frame["combat_tick"] - onset,
                    presentation_clock=frame["presentation_tick"] - frames[0]["presentation_tick"]))
        if (len(changes) != 1 or not any(event["kind"] == "started" and event["owner"] == owner for event in events) or
                not any(event["kind"] == "payment" and event["owner"] == owner and event["amount"] > 0 for event in events)):
            raise ValueError("direct seal condition lacks its sole paid action outcome")
        return [], changes
    attempts, changes = [], []
    for frame in frames:
        index = observed[frame["combat_tick"]]
        if index == 0:
            continue
        row, previous = rows[index], rows[index - 1]
        clock = frame["combat_tick"] - onset
        prior_hits = [event for event in events if event["kind"] == "hit"
                      and event["owner"] == owner and event["clock"] < clock]
        phase = f"battle_actor_{slot}_cast_clock_word"
        melee = (word(previous, phase) & 65535 == 15 and word(row, phase) & 65535 == 16
                 and word(row, f"battle_actor_{slot}_action") >> 24 == 5
                 and word(row, f"actor_{slot}_native") & 65535 == native)
        contacts = ([prior_hits[-1]] if melee and prior_hits and prior_hits[-1]["projectile"] is None else [])
        if native in (64, 67, 70):
            contacts += [event for event in prior_hits if event["projectile"] is not None
                         and event["clock"] + 1 == clock]
        for contact in contacts:
            target = contact["target"]
            target_slot = setup["source_actor_map"][target]["source_slot"]
            address = setup["initial_battle"]["actors"][target]["address"]
            if contact["projectile"] is None:
                if word(row, f"actor_{slot}_effective_target") != address:
                    raise ValueError("seal callback lacks its owned effective melee victim")
            elif not any(shot["id"] == contact["projectile"] and shot["owner"] == owner
                         and shot["target"] == target for shot in frame["projectiles"]):
                raise ValueError("seal callback lacks its live owned projectile")
            if contact["hp_change"] >= 0 or setup["actors"][target]["side"] == setup["actors"][owner]["side"]:
                raise ValueError("seal callback lacks an effective opposing contact")
            if competing_action(row, setup, owner):
                raise ValueError("seal callback has a competing actor action")
            common = dict(clock=clock, presentation_clock=frame["presentation_tick"] - frames[0]["presentation_tick"])
            emitted = [event for event in emissions if all(event[key] == value for key, value in common.items())]
            if len(emitted) > 1:
                raise ValueError("seal callback has ambiguous effect emissions")
            if emitted and not any(matches(active) and active["attachment"] == dict(kind="center", actor=target)
                                   for active in frame["programs"]):
                raise ValueError("seal emission lacks its bound victim attachment")
            before, after = [observed_conditions(sample, target_slot)
                             for sample in (previous, row)]
            if (emitted and set(after) != set(before) | {condition}) or (not emitted and before != after):
                raise ValueError("seal condition transition disagrees with its observed emission")
            if before != after:
                changes.append(dict(common, kind="condition_changed", owner=owner, target=target,
                                    action=1, before=before, after=after))
            attempts.append(dict(common, target=target, contact_clock=contact["clock"], emitted=bool(emitted)))
    if not attempts or len(emissions) != sum(attempt["emitted"] for attempt in attempts):
        raise ValueError("seal effect branch lacks a witnessed callback attempt")
    return ([] if native in (65, 68, 71) else [dict(binding, attempts=attempts)]), changes
