"""Offline damage-scale and unsupported-input checks; no emulator required."""
from dataclasses import asdict, replace
from copy import deepcopy
import unittest

from damage_envelope import (Context, UnsupportedDamage, envelope, hp_loss_bounds,
                             native_contact_branches, normalize_native_trace)


class DamageEnvelopeTests(unittest.TestCase):
    def setUp(self):
        self.owner = dict(conditions=[], stats=dict(slash=301, thrust=201, defense=50,
            intelligence=301, accuracy=100, evasion=85, level=30))
        self.target = dict(conditions=[], stats=dict(slash=100, thrust=100, defense=50,
            intelligence=160, accuracy=80, evasion=85, level=11))
        self.context = Context(85, (0, 0, 0), 100, None, False, "normal", (), True)

    def test_native_contact_uses_observed_stun_and_protection(self):
        owner, target = deepcopy(self.owner), deepcopy(self.target)
        owner["stats"].update(slash=400, accuracy=66)
        target["stats"].update(defense=30, evasion=66)
        event = dict(kind="hit", hp_change=-52, critical=False, damage_inputs=dict(
            rule=dict(kind="slash", power=dict(percent=30)), attacker=owner, defender=target,
            context=asdict(replace(self.context, strength_percent=100, target_accuracy_override=True)),
            avoided_protection=None, hp_before=9999, prevents_defeat=False))
        self.assertEqual(native_contact_branches(event)["damage_branches"],
                         dict(ordinary=[48, 52], critical=[73, 79]))
        event["damage_inputs"]["context"]["target_accuracy_override"] = False
        self.assertEqual(native_contact_branches(event)["damage_branches"],
                         dict(ordinary=[42, 45], critical=[63, 68]))
        event["damage_inputs"]["context"]["protection_shift"] = 3
        self.assertEqual(native_contact_branches(event)["damage_branches"],
                         dict(ordinary=[5, 5], critical=[7, 8]))
        for field, value in (("target_accuracy_override", None), ("protection_shift", 1),
                             ("modifiers_verified", False)):
            wrong = deepcopy(event)
            wrong["damage_inputs"]["context"][field] = value
            with self.assertRaises(UnsupportedDamage):
                native_contact_branches(wrong)
        event["damage_inputs"]["context"]["protection_shift"] = 0
        event["hp_change"] = 0
        for protection in ("recovery", "down_window_closed"):
            event["protection"] = event["damage_inputs"]["avoided_protection"] = protection
            self.assertEqual(native_contact_branches(event)["damage_branches"],
                             dict(ordinary=[0, 0], critical=None))
            wrong = deepcopy(event)
            wrong["hp_change"] = -1
            with self.assertRaises(UnsupportedDamage):
                native_contact_branches(wrong)
        trace = dict(setup=dict(contact_damage_observations=True), events=[event], incomplete_observations=[])
        normalize_native_trace(trace)
        self.assertEqual(trace["incomplete_observations"], [])
        self.assertTrue(trace["setup"]["contact_damage_normalized"])
        del event["damage_inputs"]
        normalize_native_trace(trace)
        self.assertEqual(len(trace["incomplete_observations"]), 1)

    def test_physical_integer_stages_and_independent_critical(self):
        rule = dict(kind="slash", power={"percent": 130})
        bounds = envelope(rule, self.owner, self.target, self.context)
        # Odd attack halves to150; strength gives85, variance81..88,
        # accuracy76..83, then critical and authored power give98..161.
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (98, 161))
        ordinary = envelope(rule, self.owner, self.target, replace(self.context, critical=False))
        self.assertEqual((ordinary["minimum"], ordinary["maximum"]), (98, 107))
        for guard, reduction, expected in (("normal", 75, ((24, 26), (37, 40))),
                                           ("special", None, ((19, 21), (29, 32)))):
            branches = []
            for critical in (False, True):
                guarded = envelope(rule, self.owner, self.target,
                    replace(self.context, critical=critical, guard=guard, guard_reduction=reduction))
                branches.append((guarded["minimum"], guarded["maximum"]))
            self.assertEqual(tuple(branches), expected)
            self.assertLess(branches[0][1] + 1, branches[1][0])
        species = envelope(rule, self.owner, self.target, replace(self.context, weapon_species_match=True))
        self.assertEqual((species["minimum"], species["maximum"]), (110, 181))
        for affinity, expected in (("weak", (165, 271)), ("resistant", (55, 90))):
            bounds_with_affinity = envelope(rule, self.owner, self.target,
                replace(self.context, weapon_species_match=True, affinity=affinity))
            self.assertEqual((bounds_with_affinity["minimum"], bounds_with_affinity["maximum"]), expected)
        self.assertEqual(hp_loss_bounds(bounds, 100, prevents_defeat=False), dict(minimum=98, maximum=100))
        self.assertEqual(hp_loss_bounds(bounds, 50, prevents_defeat=True), dict(minimum=49, maximum=49))

    def test_knockdown_reduction_preserves_distinct_damage_branches(self):
        owner = dict(conditions=[], stats=dict(slash=400, thrust=400, defense=230,
            intelligence=400, accuracy=66, evasion=72, level=50))
        target = dict(conditions=[], stats=dict(slash=122, thrust=112, defense=30,
            intelligence=43, accuracy=78, evasion=66, level=3))
        context = replace(self.context, strength_percent=100, protection_shift=3)
        for critical, expected in ((False, (17, 19)), (True, (26, 28))):
            bounds = envelope(dict(kind="slash", power="normal"), owner, target,
                              replace(context, critical=critical))
            self.assertEqual((bounds["minimum"], bounds["maximum"]), expected)
        for invalid in (replace(context, protection_shift=1),
                        replace(context, protection_shift=True),
                        replace(context, guard="special")):
            with self.assertRaises(UnsupportedDamage):
                envelope(dict(kind="slash", power="normal"), owner, target, invalid)

    def test_magic_power_precedes_random_resistance_and_floor(self):
        context = replace(self.context, strength_percent=80, critical=False)
        rule = dict(kind="magic", power={"percent": 150})
        bounds = envelope(rule, self.owner, self.target, context)
        # Intelligence451 minus resistance20..30, then80% and variance.
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (322, 357))
        for affinity, expected in (("weak", (483, 535)), ("resistant", (161, 178))):
            bounds_with_affinity = envelope(rule, self.owner, self.target, replace(context, affinity=affinity))
            self.assertEqual((bounds_with_affinity["minimum"], bounds_with_affinity["maximum"]), expected)
        self.target["stats"]["intelligence"] = 5000
        bounds = envelope(rule, self.owner, self.target, context)
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (1, 1))

    def test_observed_conditions_change_their_specific_damage_terms(self):
        owner = dict(conditions=[], stats=dict(slash=200, thrust=200, defense=20,
            intelligence=100, accuracy=100, evasion=100, level=1))
        target = dict(conditions=[], stats=dict(slash=200, thrust=200, defense=20,
            intelligence=0, accuracy=100, evasion=100, level=0))
        context = replace(self.context, strength_percent=100, proficiency=(0, 0, 0),
                          familiarity_percent=100, critical=False)
        for modifiers, expected in (((), (96, 104)),
                (("magic_attack_up",), (115, 124)),
                (("magic_attack_down",), (79, 86)),
                (("magic_attack_up", "magic_defense_up"), (96, 104)),
                (("physical_protection", "magic_attack_up", "magical_protection"), (92, 99))):
            bounds = envelope(dict(kind="magic", power="normal"), owner, target,
                              replace(context, magic_modifiers=modifiers))
            self.assertEqual((bounds["minimum"], bounds["maximum"]), expected)
        bounds = envelope(dict(kind="slash", power="normal"), owner, target,
                          replace(context, stat_adjustments=(0, -20, 0, 0)))
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (69, 75))

    def test_floor_hit_and_unknown_modifiers_are_distinct(self):
        self.owner["stats"].update(slash=102, accuracy=65)
        self.target["stats"].update(defense=121, evasion=114)
        rule = dict(kind="slash", power={"percent": 130})
        bounds = envelope(rule, self.owner, self.target, replace(self.context, strength_percent=100))
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (1, 1))
        for context in (replace(self.context, modifiers_verified=False),
                        replace(self.context, modifiers=("guard",)),
                        replace(self.context, guard="unobserved"),
                        replace(self.context, guard="normal"),
                        replace(self.context, guard="normal", guard_reduction=101),
                        replace(self.context, guard="special", guard_reduction=75),
                        replace(self.context, guard_reduction=75),
                        replace(self.context, affinity="unknown")):
            with self.assertRaises(UnsupportedDamage):
                envelope(rule, self.owner, self.target, context)
        with self.assertRaises(UnsupportedDamage):
            envelope(dict(kind="slash", power={"fixed": 30}), self.owner, self.target, self.context)
        with self.assertRaises(UnsupportedDamage):
            envelope(dict(kind="magic", power="normal"), self.owner, self.target,
                     replace(self.context, critical=False, guard="special"))


if __name__ == "__main__":
    unittest.main()
