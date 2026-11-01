"""Expand the current named technique document for numeric oracle comparisons."""
import copy

ELEMENTS = [None, "water", "wind", "fire", "earth", "lightning", "ice", "light", "darkness"]
CAPABILITIES = dict(family=None, regal_family=None, spell=False, aerial=False,
                    uses_weapon_reach=False, chains_without_contact=False,
                    target="enemy", ai_target_scope="single", summon=False, offensive=False, revives=False, healing=False)
LEARNING = dict(route=None, parent=None, technical_successor=None, strike_successor=None,
                parent_uses=0, prerequisites=[], excludes=[], requires_story_unlock=False)
CASTING = dict(fixed_timing=False, support_target=False, effects="standard")


def prepare(document):
    mappings = document["mappings"]
    identities = {name: int(identity) for identity, name in mappings.items()}
    if len(identities) != len(mappings) or not identities or min(identities.values()) <= 0:
        raise ValueError("techniques require unique names and nonzero IDs")
    ordinary = dict(tp_cost=0, element=0, learning=LEARNING, capabilities=CAPABILITIES,
                    admission_flash=None, learn_on_level_up=False, casting=CASTING,
                    action_range=0, cast_time_adjustment=0, recovery_ticks=0, required_level=0)
    unavailable = copy.deepcopy(ordinary)
    unavailable["capabilities"].update(target="unavailable", uses_weapon_reach=True)
    definitions = [copy.deepcopy(unavailable) for _ in range(max(identities.values()) + 1)]
    for name, identity in identities.items():
        source = document["techniques"][name]["rules"]
        row = copy.deepcopy(ordinary)
        row.update(copy.deepcopy(source))
        row["element"] = ELEMENTS.index(source.get("element"))
        row["capabilities"] = {**CAPABILITIES, **source.get("capabilities", {})}
        row["casting"] = {**CASTING, **source.get("casting", {})}
        learning = {**copy.deepcopy(LEARNING), **source.get("learning", {})}
        for field in ("parent", "technical_successor", "strike_successor"):
            if learning[field] is not None:
                learning[field] = identities[learning[field]]
        learning["excludes"] = [identities[name] for name in learning["excludes"]]
        learning["prerequisites"] = [dict(any_of=[identities[name] for name in group["any_of"]],
                                             minimum_uses=group["minimum_uses"])
                                        for group in learning["prerequisites"]]
        row["learning"] = learning
        definitions[identity] = row
    characters = document["characters"]
    if sorted(map(int, characters)) != list(range(1, 10)):
        raise ValueError("techniques require nine character learning lists")
    return dict(definitions=definitions, learning=[
        [identities[name] for name in document["learning"][characters[str(identity)]]]
        for identity in range(1, 10)])


def native_learning(document):
    """GameCube observer lists; appended elemental variants have no native slots."""
    return [[identity for identity in row if identity < 253]
            for row in prepare(document)["learning"]]
