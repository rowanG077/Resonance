"""Offline checks for fixture selection after declared health mutations."""
import unittest

import single_action_fixture


class Memory:
    def __init__(self, actors):
        self.values = {}
        self.writes = []
        for actor in actors:
            at = actor["address"]
            self.values.update({(actor["vitals_address"] + 0x24, "I"): actor["hp"],
                (at + 0x107d, "B"): 0, (at + 0x1990, "I"): actor["target"],
                (at + 0x18c0, "f"): actor["position"][0],
                (at + 0x18c8, "f"): actor["position"][1]})

    def read(self, at, fmt):
        return self.values[at, fmt]

    def write(self, at, fmt, value):
        self.values[at, fmt] = value
        self.writes.append((at, fmt, value))


def actor(slot, side, x, target):
    return dict(slot=slot, side=side, address=0x80000000 + slot * 0x2000,
                vitals_address=0x81000000 + slot * 0x100, hp=100,
                position=[x, 0], target=0x80000000 + target * 0x2000)


class FixtureTargetTests(unittest.TestCase):
    def test_current_health_mutation_repairs_only_invalid_opponent(self):
        actors = [actor(0, 0, 470, 7), actor(4, 1, 600, 0),
                  actor(5, 1, 350, 0), actor(7, 1, 150, 0)]
        state = Memory(actors)
        # The inspection predates fixture writes. Read the current memory,
        # including moved placement, instead of stale actor HP/positions.
        state.values[actors[3]["vitals_address"] + 0x24, "I"] = 0
        state.values[actors[2]["address"] + 0x18c8, "f"] = 350
        proof = single_action_fixture.normalize_fixture_targets(state, dict(actors=actors))
        self.assertEqual(proof["repairs"], [dict(actor=0, previous_target=7, target=4)])
        self.assertEqual(state.writes, [(actors[0]["address"] + 0x1990, "I", actors[1]["address"])])

    def test_valid_distant_opponent_and_dead_ally_selection_are_preserved(self):
        actors = [actor(0, 0, 0, 5), actor(1, 0, 5, 2), actor(2, 0, 10, 4),
                  actor(4, 1, 20, 0), actor(5, 1, 100, 0)]
        state = Memory(actors)
        state.values[actors[2]["vitals_address"] + 0x24, "I"] = 0
        self.assertEqual(single_action_fixture.normalize_fixture_targets(
            state, dict(actors=actors))["repairs"], [])
        self.assertEqual(state.writes, [])

    def test_defeated_availability_is_not_a_valid_live_target(self):
        actors = [actor(0, 0, 0, 4), actor(4, 1, 10, 0), actor(5, 1, 20, 0)]
        state = Memory(actors)
        state.values[actors[1]["address"] + 0x107d, "B"] = 0xc2
        self.assertEqual(single_action_fixture.normalize_fixture_targets(
            state, dict(actors=actors))["repairs"], [dict(actor=0, previous_target=4, target=5)])
        state.values[actors[2]["vitals_address"] + 0x24, "I"] = 0
        with self.assertRaisesRegex(ValueError, "without a living opponent"):
            single_action_fixture.normalize_fixture_targets(state, dict(actors=actors))


if __name__ == "__main__":
    unittest.main()
