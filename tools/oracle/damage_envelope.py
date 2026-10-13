"""Independent integer bounds for ordinary, unguarded damage observations.

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
    if context.affinity != "normal":
        raise UnsupportedDamage("only verified normal affinity is supported")
    if context.critical is not None and type(context.critical) is not bool:
        raise UnsupportedDamage("critical outcome must be observed or explicitly unknown")
    if type(context.target_accuracy_override) is not bool:
        raise UnsupportedDamage("target accuracy state is unverified")
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
    strength = integer(context.strength_percent, "strength", 65535 if physical else 255)
    owner, target = attacker["stats"], defender["stats"]
    amounts = []
    if physical:
        attack = percent(owner[kind], 100 + attack_bonus)
        accuracy = percent(owner["accuracy"], 100 + accuracy_bonus)
        accuracy = percent(accuracy, context.familiarity_percent)
        delta = (accuracy - target["evasion"] - 5) >> 1
        if not -32768 <= delta <= 32767:
            raise UnsupportedDamage("accuracy intermediate exceeds signed halfword range")
        accuracy_percent = 100 if context.target_accuracy_override else 90 + max(-10, min(10, delta))
        base = percent(max((attack >> 1) - target["defense"], 1), strength)
        criticals = (False, True) if context.critical is None else (context.critical,)
        for variance in (96, 104):
            amount = percent(percent(base, variance), accuracy_percent)
            for critical in criticals:
                amounts.append(percent(amount + (amount >> 1 if critical else 0), power_percent))
    else:
        if context.critical is not False:
            raise UnsupportedDamage("ordinary magic requires verified absence of critical modifiers")
        intelligence = percent(owner["intelligence"], 100 + intelligence_bonus)
        intelligence = percent(intelligence, power_percent)
        for extra_resistance in (0, max(target["level"] - 1, 0)):
            resistance = (target["intelligence"] >> 3) + extra_resistance
            base = percent(intelligence - resistance, strength)
            amounts.extend(percent(base, variance) for variance in (96, 104))
    if max(amounts) > 32767:
        raise UnsupportedDamage("nominal damage exceeds the supported HP-application range")
    return {"minimum": max(1, min(amounts)), "maximum": max(1, max(amounts)),
            "preconditions": ["bound ordinary hit", "initialized statistics", "normal affinity",
                              "no active conditions", "verified unguarded damage modifiers"]}


def hp_loss_bounds(bounds, hp, *, prevents_defeat):
    """Cap nominal bounds for a separately attributed hit's observed HP loss."""
    integer(hp, "pre-hit HP", (1 << 31) - 1)
    if type(prevents_defeat) is not bool:
        raise UnsupportedDamage("defeat prevention is unverified")
    available = max(0, hp - int(prevents_defeat))
    return {key: min(bounds[key], available) for key in ("minimum", "maximum")}
