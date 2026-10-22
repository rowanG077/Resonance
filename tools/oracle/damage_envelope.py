"""Independent integer bounds for ordinary damage observations.

Callers bind the actual hit rule and verify live modifiers at contact. Unknown
affinity, conditions or modifiers are unsupported, never an enlarged tolerance.
"""
from dataclasses import dataclass


class UnsupportedDamage(ValueError):
    pass


@dataclass(frozen=True)
class Context:
    strength_percent: int
    proficiency: tuple[int, int, int]  # Attack, accuracy, intelligence bonuses.
    familiarity_percent: int
    critical: bool | None  # None includes both independently possible outcomes.
    target_accuracy_override: bool
    affinity: str
    modifiers: tuple[str, ...]
    modifiers_verified: bool
    weapon_species_match: bool = False
    guard: str | None = None
    guard_reduction: int | None = None
    protection_shift: int = 0
    stat_adjustments: tuple[int, int, int, int] = (0, 0, 0, 0)  # Attack, defense, accuracy, evasion percentages.
    magic_modifiers: tuple[str, ...] = ()


def integer(value, name, maximum):
    if type(value) is not int or not 0 <= value <= maximum:
        raise UnsupportedDamage(f"{name} is outside the supported integer range")
    return value


def percent(value, scale):
    product = value * scale
    if not -(1 << 31) <= product < (1 << 31):
        raise UnsupportedDamage("damage intermediate exceeds signed integer range")
    # Integer percentage stages truncate toward zero, including negative magic bases.
    return product // 100 if product >= 0 else -((-product) // 100)


def envelope(rule, attacker, defender, context):
    """Return nominal minimum/maximum; HP caps belong to the observed application.

Actors use the setup record's {stats, conditions} shape. Rule kind and power use
the authored JSON representation. Context must describe this contact, rather
than assume that ready-state strength or modifiers remain unchanged.
"""
    if context.modifiers_verified is not True or context.modifiers:
        raise UnsupportedDamage("live damage modifiers are unverified or unsupported")
    if context.affinity not in ("normal", "weak", "resistant"):
        raise UnsupportedDamage("only verified ordinary damage affinities are supported")
    if context.critical is not None and type(context.critical) is not bool:
        raise UnsupportedDamage("critical outcome must be observed or explicitly unknown")
    if type(context.target_accuracy_override) is not bool:
        raise UnsupportedDamage("target accuracy state is unverified")
    if type(context.weapon_species_match) is not bool:
        raise UnsupportedDamage("weapon species applicability is unverified")
    for actor in (attacker, defender):
        if actor.get("conditions") != []:
            raise UnsupportedDamage("active conditions are unverified or unsupported")
        if not isinstance(actor.get("stats"), dict):
            raise UnsupportedDamage("initialized combat statistics are missing")
        for name in ("slash", "thrust", "defense", "intelligence", "accuracy", "evasion", "level"):
            integer(actor["stats"].get(name), name, 255 if name == "level" else 32767)
    if len(context.proficiency) != 3:
        raise UnsupportedDamage("three proficiency bonuses are required")
    attack_bonus, accuracy_bonus, intelligence_bonus = (
        integer(value, "proficiency", 255) for value in context.proficiency)
    integer(context.familiarity_percent, "familiarity", 104)
    if context.familiarity_percent not in (100, 101, 102, 104):
        raise UnsupportedDamage("familiarity is unverified")
    kind, power = rule.get("kind"), rule.get("power")
    if power == "normal":
        power_percent = 100
    elif isinstance(power, dict) and set(power) == {"percent"}:
        power_percent = integer(power["percent"], "hit power", 65535)
    else:
        raise UnsupportedDamage("only ordinary percentage hit power is supported")
    physical = kind in ("slash", "thrust")
    if not physical and kind != "magic":
        raise UnsupportedDamage("damage kind is unbound")
    if context.guard is not None:
        if not physical or context.guard not in ("normal", "special"):
            raise UnsupportedDamage("only observed ordinary physical guarding is supported")
        if context.guard == "normal":
            guard_percent = 100 - integer(context.guard_reduction, "guard reduction", 100)
        elif context.guard_reduction is not None:
            raise UnsupportedDamage("special guard cannot use a profile reduction override")
        else:
            guard_percent = 20
    elif context.guard_reduction is not None:
        raise UnsupportedDamage("guard reduction requires an observed guard")
    else:
        guard_percent = 100
    if type(context.protection_shift) is not int or context.protection_shift not in (0, 2, 3):
        raise UnsupportedDamage("contact protection reduction is unverified")
    if context.protection_shift and context.guard is not None:
        raise UnsupportedDamage("combined protection and guard is unsupported")
    strength = integer(context.strength_percent, "strength", 65535 if physical else 255)
    owner, target = attacker["stats"], defender["stats"]
    if (len(context.stat_adjustments) != 4 or any(type(value) is not int or not -32768 <= value <= 32767
                                                for value in context.stat_adjustments)):
        raise UnsupportedDamage("live physical stat percentages are unverified")
    magic_factors = {"magic_attack_up": (12, 10), "magic_attack_down": (10, 12),
                     "magic_defense_down": (12, 10), "magic_defense_up": (10, 12),
                     "physical_protection": (80, 100), "magical_protection": (80, 100)}
    if (any(name not in magic_factors for name in context.magic_modifiers)
            or len(set(context.magic_modifiers)) != len(context.magic_modifiers)
            or physical and context.magic_modifiers or not physical and any(context.stat_adjustments)):
        raise UnsupportedDamage("condition modifiers do not match the contact damage kind")

    def adjusted(value, index):
        value += percent(value, context.stat_adjustments[index])
        if not 0 <= value <= 32767:
            raise UnsupportedDamage("condition-adjusted statistic exceeds signed halfword range")
        return value

    amounts = []
    if physical:
        attack = adjusted(percent(owner[kind], 100 + attack_bonus), 0)
        defense = adjusted(target["defense"], 1)
        accuracy = adjusted(percent(owner["accuracy"], 100 + accuracy_bonus), 2)
        evasion = adjusted(target["evasion"], 3)
        accuracy = percent(accuracy, context.familiarity_percent)
        delta = (accuracy - evasion - 5) >> 1
        if not -32768 <= delta <= 32767:
            raise UnsupportedDamage("accuracy intermediate exceeds signed halfword range")
        accuracy_percent = 100 if context.target_accuracy_override else 90 + max(-10, min(10, delta))
        base = percent(max((attack >> 1) - defense, 1), strength)
        criticals = (False, True) if context.critical is None else (context.critical,)
        for variance in (96, 104):
            amount = percent(percent(base, variance), accuracy_percent)
            for critical in criticals:
                powered = percent(amount + (amount >> 1 if critical else 0), power_percent)
                amounts.append(powered + (powered >> 3 if context.weapon_species_match else 0))
    else:
        if context.critical is not False:
            raise UnsupportedDamage("ordinary magic requires verified absence of critical modifiers")
        intelligence = percent(owner["intelligence"], 100 + intelligence_bonus)
        intelligence = percent(intelligence, power_percent)
        for extra_resistance in (0, max(target["level"] - 1, 0)):
            resistance = (target["intelligence"] >> 3) + extra_resistance
            base = percent(intelligence - resistance, strength)
            protected = False
            for name in magic_factors:
                if name not in context.magic_modifiers:
                    continue
                if name in ("physical_protection", "magical_protection"):
                    if protected:
                        continue
                    protected = True
                numerator, denominator = magic_factors[name]
                value = base * numerator
                base = value // denominator if value >= 0 else -((-value) // denominator)
            amounts.extend(percent(base, variance) for variance in (96, 104))
    if context.affinity == "weak":
        amounts = [amount + (amount >> 1) for amount in amounts]
    elif context.affinity == "resistant":
        amounts = [amount >> 1 for amount in amounts]
    amounts = [percent(amount, guard_percent) >> context.protection_shift for amount in amounts]
    if max(amounts) > 32767:
        raise UnsupportedDamage("nominal damage exceeds the supported HP-application range")
    return {"minimum": max(1, min(amounts)), "maximum": max(1, max(amounts)),
            "preconditions": ["bound ordinary hit", "initialized statistics", f"{context.affinity} affinity",
                              "no active conditions", "verified damage modifiers",
                              f"{context.guard or 'no'} guard"]}


def hp_loss_bounds(bounds, hp, *, prevents_defeat):
    """Cap nominal bounds for a separately attributed hit's observed HP loss."""
    integer(hp, "pre-hit HP", (1 << 31) - 1)
    if type(prevents_defeat) is not bool:
        raise UnsupportedDamage("defeat prevention is unverified")
    available = max(0, hp - int(prevents_defeat))
    return {key: min(bounds[key], available) for key in ("minimum", "maximum")}


def native_contact_branches(event):
    """Use the native contact's pre-damage inputs, never the source's state."""
    inputs = event.get("damage_inputs")
    if not isinstance(inputs, dict) or "unsupported" in inputs:
        raise UnsupportedDamage((inputs or {}).get("unsupported", "native contact inputs missing"))
    context = Context(**inputs["context"])
    avoided = inputs.get("avoided_protection")
    if avoided is not None:
        if (avoided not in ("recovery", "down_window_closed")
                or event.get("protection") != avoided or event.get("hp_change") != 0
                or event.get("critical") is not False or context.guard is not None
                or context.protection_shift != 0):
            raise UnsupportedDamage("avoided contact disagrees with observed protection")
        return dict(damage_range=[0, 0], damage_branches=dict(ordinary=[0, 0], critical=None))
    if event.get("protection") is not None:
        raise UnsupportedDamage("unobserved native contact protection")
    from dataclasses import replace
    physical = inputs["rule"]["kind"] in ("slash", "thrust")
    branches = {}
    for critical in (False, True) if physical else (False,):
        bounds = envelope(inputs["rule"], inputs["attacker"], inputs["defender"],
                          replace(context, critical=critical))
        bounds = hp_loss_bounds(bounds, inputs["hp_before"], prevents_defeat=inputs["prevents_defeat"])
        branches["critical" if critical else "ordinary"] = [bounds["minimum"], bounds["maximum"]]
    branches.setdefault("critical", None)
    values = [value for branch in branches.values() if branch is not None for value in branch]
    return dict(damage_range=[min(values), max(values)], damage_branches=branches)


def normalize_native_trace(trace):
    if trace.get("setup", {}).get("contact_damage_observations") is not True:
        raise UnsupportedDamage("native contact observation hook was not enabled")
    events = trace["events"] + trace["setup"].get("chain_history", {}).get("events", [])
    for event in events:
        if event.get("kind") != "hit":
            continue
        try:
            event.update(native_contact_branches(event))
        except (UnsupportedDamage, KeyError, TypeError, ValueError) as error:
            trace["incomplete_observations"].append(
                f"native contact damage at {event.get('clock')}: {error}")
    trace["setup"]["contact_damage_normalized"] = True
    return trace


if __name__ == "__main__":
    import argparse
    import json
    from pathlib import Path
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--normalize-native", type=Path, required=True)
    args = parser.parse_args()
    trace = normalize_native_trace(json.loads(args.normalize_native.read_text()))
    temporary = args.normalize_native.with_suffix(".tmp")
    temporary.write_text(json.dumps(trace, separators=(",", ":")) + "\n")
    temporary.replace(args.normalize_native)
