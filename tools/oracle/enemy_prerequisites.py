"""Prepare declared enemy-action prerequisites without forcing an action."""
import json

import battle_geometry

SESSION_POINTER = 0x8035A768
EVENT_FLAGS_OFFSET = 0xC9D
EVENT_FLAG_COUNT = 2048
HP_OFFSET = 0x24
TP_OFFSET = 0x2A
ACTOR_AVAILABILITY = 0x107D
ACTOR_ACTION = 0x1AC
ACTOR_TARGET_PHASE = 0x1B0
ACTOR_STATUS = 0x280
ACTOR_CONTACTS = 0x1184
CONTACT_BYTES = 0x38
CONTACT_CAPACITY = 16
DEFEATED_AVAILABILITY = 2
DEFEATED_ACTION = 18
DEFEATED_TARGET_PHASE = 16
HOST_ATTACK_GAP = 10.0
RECIPIENT_PRESS_POLLS = 240
RECIPIENT_RELEASE_POLLS = 1000
RECIPIENT_ATTACK_PERIOD_POLLS = 16
PREREQUISITE_ATTACK_RELEASE_POLLS = 400
PREREQUISITE_RETREAT_POLLS = 40
PREREQUISITE_SELECT_POLLS = 20
PREREQUISITE_RETURN_POLLS = 80
DEFENSE_CORE = 211
DEFENSE_ARM = 213
DEFENSE_COMPONENTS = [211, 212, 212, 213, 213, 213, 213, 214]
DEFENSE_DEPLOYMENT_HP_PERCENT = 66
DEFENSE_DEPLOYMENT_PHASE = 0x1B16


def plan(case, battle, identities, definitions):
    """Choose fixture health and story inputs from typed authored requirements."""
    owner = next(actor for actor in battle["actors"]
                 if identities.get(actor["slot"]) == case["enemy"])
    peers = [actor for actor in battle["actors"] if actor["side"] == owner["side"]]
    row = case["definition"]
    requirements = row["requirements"]
    absent = {predicate["ally_absent"] for predicate in requirements.get("predicates", [])
              if isinstance(predicate, dict) and "ally_absent" in predicate}
    defeated = {actor["slot"] for actor in peers
                if actor["slot"] != owner["slot"] and identities[actor["slot"]] in absent}
    profile = definitions[case["enemy"]]["profile"]
    release_host = None
    if hosted := profile.get("hosted_body"):
        host = peers[0]
        host_profile = definitions[identities[host["slot"]]]["profile"]
        if (host["slot"] != owner["slot"] and host_profile["traits"]["hosts_participants"]
                and any(hosted[key] for key in ("release_on_hurt", "release_on_defeat", "release_below_half_health"))):
            defeated.add(host["slot"])
            release_host = host["slot"]
    if (maximum := requirements.get("max_enemies")) is not None:
        if maximum < 1:
            raise ValueError("enemy count prerequisite excludes the action owner")
        alive = [actor for actor in peers if actor["hp"] > 0 and actor["slot"] not in defeated]
        removable = [actor for actor in reversed(alive) if actor["slot"] != owner["slot"]]
        defeated.update(actor["slot"] for actor in removable[:max(0, len(alive) - maximum)])
    if case.get("context") == "revival" and not defeated:
        recipients = [actor for actor in peers if actor["slot"] != owner["slot"]]
        if not recipients:
            raise ValueError("revival fixture requires an allied recipient")
        defeated.add(recipients[0]["slot"])
    health = {slot: 0 for slot in sorted(defeated)}
    threshold = requirements.get("hp_percent")
    behavior = definitions[case["enemy"]]["actions"]["policy"].get("behavior") or {}
    health_phase = None
    for kind, percentage in (("health_stage", "hp_percent"), ("transformation", "airborne_hp_percent")):
        if stage := behavior.get(kind):
            actions = stage["final_actions"]
            if actions["start"] <= case["action_row"] < actions["end"]:
                threshold = min(threshold if threshold is not None else 100, stage[percentage])
                if kind == "transformation":
                    health_phase = dict(hp_percent=stage[percentage], hover_height=stage["hover_height"])
    if threshold is not None:
        health[owner["slot"]] = min(owner["hp"], max(1, owner["max_hp"] * threshold // 100))
    defense_deployment = None
    if case["enemy"] == DEFENSE_ARM:
        if sorted(identities[actor["slot"]] for actor in peers) != DEFENSE_COMPONENTS:
            raise ValueError("Defense System arm requires its complete authored encounter")
        core = next(actor for actor in peers if identities[actor["slot"]] == DEFENSE_CORE)
        health[core["slot"]] = min(core["hp"], core["max_hp"] * DEFENSE_DEPLOYMENT_HP_PERCENT // 100)
        defense_deployment = dict(core=core["slot"], hp_percent=DEFENSE_DEPLOYMENT_HP_PERCENT)
    if case.get("healing"):
        for actor in peers:
            if actor["slot"] not in defeated:
                health[actor["slot"]] = min(health.get(actor["slot"], actor["hp"]),
                                            max(1, actor["max_hp"] // 2))
    mana = {}
    if case.get("restores_tp"):
        for actor in peers:
            if actor["slot"] not in defeated:
                cost = row["tp"] if actor["slot"] == owner["slot"] else 0
                mana[actor["slot"]] = min(actor["tp"], max(cost, actor["max_tp"] // 2))
    physical_defeats = set(defeated) if case.get("context") == "departure" else set()
    if release_host is not None:
        physical_defeats.add(release_host)
    if len(physical_defeats) > 1:
        raise ValueError("physical prerequisite needs an authored formation with one companion")
    for slot in physical_defeats:
        health[slot] = 1
        defeated.remove(slot)
    return dict(health=health, mana=mana, required_flag=row.get("required_flag"),
                owner=owner["slot"], defeated=sorted(defeated),
                release_host=release_host,
                physical_defeats=sorted(physical_defeats),
                health_phase=health_phase,
                defense_deployment=defense_deployment,
                hover_height=profile["ground_offset"] if profile["traits"]["flying"] else None,
                target_attacking="target_attacking" in requirements.get("predicates", []))


def prepare(state, battle, initialized, case, assets):
    identities = {actor["slot"]: actor["enemy"] for actor in initialized["identities"]}
    definitions = {enemy: json.loads((assets / f"battle/enemies/{enemy:03}/definition.json").read_text())
                   for enemy in set(identities.values())}
    inputs = plan(case, battle, identities, definitions)
    first_change = len(state.changes)
    actors = {actor["slot"]: actor for actor in battle["actors"]}
    if deployment := inputs["defense_deployment"]:
        phase = state.read(actors[deployment["core"]]["address"] + DEFENSE_DEPLOYMENT_PHASE, "B")
        if phase != 0:
            raise ValueError("Defense System fixture must precede ordinary deployment")
        deployment["initial_phase"] = phase
    for slot, hp in inputs["health"].items():
        state.write(actors[slot]["vitals_address"] + HP_OFFSET, "I", hp)
    for slot, tp in inputs["mana"].items():
        state.write(actors[slot]["vitals_address"] + TP_OFFSET, "H", tp)
    for slot in inputs["defeated"]:
        at = actors[slot]["address"]
        # A completed defeated participant retains its roster identity. HP
        # alone does not change its availability or suppress its controller.
        state.write(at + ACTOR_AVAILABILITY, "B",
                    state.read(at + ACTOR_AVAILABILITY, "B") & ~7 | DEFEATED_AVAILABILITY)
        state.write(at + ACTOR_ACTION, "I", DEFEATED_ACTION << 24)
        state.write(at + ACTOR_TARGET_PHASE, "B", DEFEATED_TARGET_PHASE)
        state.write(at + ACTOR_STATUS, "B", state.read(at + ACTOR_STATUS, "B") & 15 | 6)
    if inputs["physical_defeats"]:
        slot = inputs["physical_defeats"][0]
        host = actors[slot]
        recipient = actors[0]
        host_body = battle_geometry.read_initialized_body(
            state, host, assets / f"battle/enemies/{identities[slot]:03}.json")
        recipient_body = battle_geometry.read_initialized_body(
            state, recipient, assets / f"battle/party/{battle['party_order'][0]:02}.json")
        separation = battle_geometry.separation_for_gap(host_body, recipient_body, HOST_ATTACK_GAP)
        x, z = host["position"][0] - separation, host["position"][2]
        battle_geometry.place_actor(state, recipient, x, z)
    recipient_approach = case.get("contact_fixture", {}).get("recipient_approach", False)
    owner_movement = case.get("contact_fixture", {}).get("owner_movement", False)
    if inputs["physical_defeats"] or inputs["target_attacking"] or recipient_approach or owner_movement:
        owner = actors[inputs["owner"]]["address"]
        inputs["owner_control"] = dict(mode=state.read(owner + 0x107c, "B") & 0x38,
                                       port=state.read(owner + ACTOR_AVAILABILITY, "B") & 0xc0)
        state.write(owner + 0x107c, "B", state.read(owner + 0x107c, "B") & ~0x38)
        state.write(owner + ACTOR_AVAILABILITY, "B",
                    state.read(owner + ACTOR_AVAILABILITY, "B") & 0x3f | 0xc0)
        inputs["selection_range"] = case["definition"]["range"]
        if recipient_approach or owner_movement:
            walking = case["contact_fixture"]["recipient_input"]
            if walking["mode"] != "stick" or walking["stick"] != [127, 0]:
                raise ValueError("movement prerequisite requires an ordinary forward walk")
            inputs["selection_after_presentation_tick"] = (battle["presentation_tick"]
                + walking["release_after_polls"] // 2 + 1)
            if owner_movement:
                if walking.get("controller") != "owner":
                    raise ValueError("owner movement requires physical owner input")
                inputs["origin_bounds"] = [predicate["origin_distance"]
                    for predicate in case["definition"]["requirements"]["predicates"]
                    if isinstance(predicate, dict) and "origin_distance" in predicate]
        intervals = (recipient_input(case, None, inputs) or {}).get("stick_intervals", [])
        if intervals:
            inputs["selection_after_presentation_tick"] = (battle["presentation_tick"]
                + intervals[0]["release_after_polls"] // 2 + 1)
    if (flag := inputs["required_flag"]) is not None:
        if not 0 <= flag < EVENT_FLAG_COUNT:
            raise ValueError("enemy action requires an unsupported event flag")
        session = state.read(SESSION_POINTER)
        if not 0x80000000 <= session <= 0x81800000 - EVENT_FLAGS_OFFSET - EVENT_FLAG_COUNT // 8:
            raise ValueError("enemy prerequisite session lies outside main RAM")
        address = session + EVENT_FLAGS_OFFSET + flag // 8
        state.write(address, "B", state.read(address, "B") | (1 << (flag % 8)))
    return dict(**inputs, changes=state.changes[first_change:],
                scope="Declared vitals, defeated-peer availability and story inputs. Physical prerequisites retain their full hit/recovery prelude before owner AI is enabled.")


def recipient_input(case, explicit, prerequisites=None):
    """Use physical input for the selected row's target-state prerequisite."""
    if explicit is not None or "enemy" not in case:
        return explicit
    physical = (prerequisites or {}).get("physical_defeats", [])
    host = physical[0] if physical else (prerequisites or {}).get("release_host")
    requirements = case["definition"]["requirements"]
    if host is None and "target_attacking" not in requirements.get("predicates", []):
        return (dict(mode="normal", release_after_polls=RECIPIENT_RELEASE_POLLS)
                if requirements.get("target_guarding") else None)
    # Repeated presses span entry so the target is already attacking when the
    # first ordinary enemy choice is made. Entry itself still blocks attacks.
    result = dict(mode="attack", press_after_polls=0 if host is None else RECIPIENT_PRESS_POLLS,
                release_after_polls=RECIPIENT_RELEASE_POLLS,
                **({"repeat_every_polls": RECIPIENT_ATTACK_PERIOD_POLLS} if host is None else {}),
                **({"target_slot": host} if host is not None else {}))
    if host is not None and case["definition"]["range"][0] > 0:
        # Released participants land beside the attacker. Walk away with
        # ordinary controller input so a ranged row can become admissible.
        result["release_after_polls"] = PREREQUISITE_ATTACK_RELEASE_POLLS
        retreat_end = PREREQUISITE_ATTACK_RELEASE_POLLS + PREREQUISITE_RETREAT_POLLS
        return_start = retreat_end + PREREQUISITE_SELECT_POLLS
        result["stick_intervals"] = [
            dict(press_after_polls=PREREQUISITE_ATTACK_RELEASE_POLLS,
                 release_after_polls=retreat_end, stick=[-127, 0]),
            dict(press_after_polls=return_start,
                 release_after_polls=return_start + PREREQUISITE_RETURN_POLLS, stick=[127, 0])]
    return result


def departure_observation(rows, setup):
    """Attribute companion deaths to fresh queued melee contacts before departure."""
    inputs = setup["assistance"]["preparation"]["prerequisites"]
    slots = inputs["physical_defeats"]
    actor = setup["recipient_input"]["actor"]
    source = setup["source_actor_map"][actor]["source_slot"]
    attacker = setup["initial_battle"]["actors"][actor]["address"]
    targets = {observed["slot"]: index for index, observed in
               enumerate(setup["initial_battle"]["actors"]) if observed["slot"] in slots}
    if not targets or any(setup["actors"][index]["hp"] <= 0 for index in targets.values()):
        raise ValueError("departure lacks living prepared companions")
    contacts = []
    for before, after in zip(rows, rows[1:]):
        for slot, target in targets.items():
            if not (before[f"battle_actor_{slot}_hp"] > 0 and after[f"battle_actor_{slot}_hp"] == 0):
                continue
            victim = setup["initial_battle"]["actors"][target]["address"]
            if (after[f"actor_{slot}_fresh_contact_word"] >> 24 != 1
                    or after[f"actor_{source}_contact_target"] != victim
                    or after[f"actor_{slot}_control_word"] >> 16 & 7 != 2):
                raise ValueError("departure companion death lacks a fresh recipient hit")
            queued = []
            for side in range(2):
                count = after["contact_queue_counts"] >> (16 if side == 0 else 0) & 65535
                for index in range(count):
                    prefix = f"contact_queue_{side}_{index}"
                    if (after[prefix + "_owner"] == attacker and not after[prefix + "_projectile"]
                            and 0 <= after[prefix + "_context"] - attacker - ACTOR_CONTACTS
                                < CONTACT_CAPACITY * CONTACT_BYTES
                            and (after[prefix + "_context"] - attacker - ACTOR_CONTACTS) % CONTACT_BYTES == 0):
                        queued.append(after[prefix + "_context"])
            if not queued:
                raise ValueError("departure companion death lacks a queued physical attack")
            contacts.append(dict(actor=actor, target=target,
                after_updates=after["battle_combat_tick_word"] - setup["initial_battle"]["combat_tick"]))
    if sorted(contact["target"] for contact in contacts) != sorted(targets.values()):
        raise ValueError("departure did not observe every prepared companion's defeat")
    return dict(fatal_contacts=contacts)
