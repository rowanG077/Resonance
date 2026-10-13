"""Offline damage-scale and unsupported-input checks; no emulator required."""
from dataclasses import replace
import unittest

from damage_envelope import Context, UnsupportedDamage, envelope, hp_loss_bounds


class DamageEnvelopeTests(unittest.TestCase):
    def setUp(self):
        self.owner = dict(conditions=[], stats=dict(slash=301, thrust=201, defense=50,
            intelligence=301, accuracy=100, evasion=85, level=30))
        self.target = dict(conditions=[], stats=dict(slash=100, thrust=100, defense=50,
            intelligence=160, accuracy=80, evasion=85, level=11))
        self.context = Context(85, (0, 0, 0), 100, None, False, "normal", (), True)

    def test_physical_integer_stages_and_independent_critical(self):
        rule = dict(kind="slash", power={"percent": 130})
        bounds = envelope(rule, self.owner, self.target, self.context)
        # Odd attack halves to150; strength gives85, variance81..88,
        # accuracy76..83, then critical and authored power give98..161.
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (98, 161))
        ordinary = envelope(rule, self.owner, self.target, replace(self.context, critical=False))
        self.assertEqual((ordinary["minimum"], ordinary["maximum"]), (98, 107))
        self.assertEqual(hp_loss_bounds(bounds, 100, prevents_defeat=False), dict(minimum=98, maximum=100))
        self.assertEqual(hp_loss_bounds(bounds, 50, prevents_defeat=True), dict(minimum=49, maximum=49))

    def test_magic_power_precedes_random_resistance_and_floor(self):
        context = replace(self.context, strength_percent=80, critical=False)
        rule = dict(kind="magic", power={"percent": 150})
        bounds = envelope(rule, self.owner, self.target, context)
        # Intelligence451 minus resistance20..30, then80% and variance.
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (322, 357))
        self.target["stats"]["intelligence"] = 5000
        bounds = envelope(rule, self.owner, self.target, context)
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (1, 1))

    def test_floor_hit_and_unknown_modifiers_are_distinct(self):
        self.owner["stats"].update(slash=102, accuracy=65)
        self.target["stats"].update(defense=121, evasion=114)
        rule = dict(kind="slash", power={"percent": 130})
        bounds = envelope(rule, self.owner, self.target, replace(self.context, strength_percent=100))
        self.assertEqual((bounds["minimum"], bounds["maximum"]), (1, 1))
        for context in (replace(self.context, modifiers_verified=False),
                        replace(self.context, modifiers=("guard",)),
                        replace(self.context, affinity="unknown")):
            with self.assertRaises(UnsupportedDamage):
                envelope(rule, self.owner, self.target, context)
        with self.assertRaises(UnsupportedDamage):
            envelope(dict(kind="slash", power={"fixed": 30}), self.owner, self.target, self.context)


if __name__ == "__main__":
    unittest.main()
