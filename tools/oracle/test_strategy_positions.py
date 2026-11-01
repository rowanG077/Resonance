import struct
import unittest

from strategy_positions import review


def row(position=3, skill=5, x=0., state=3, native=0, selected=0):
    return dict(owner_strategy=(1 << 24) | (skill << 16) | (position << 8),
                battle_actor_1_flags=2 << 27, battle_actor_1_action=state << 24,
                battle_actor_1_x_bits=struct.unpack('>I', struct.pack('>f', x))[0],
                owner_native=native, owner_selected=selected)


class PositionReviewTests(unittest.TestCase):
    def test_idle_requires_live_policy_and_stationary_actor(self):
        case = dict(position=3, skill=5, expected="idle")
        self.assertTrue(review([row()], case)["passed"])
        self.assertFalse(review([], case)["passed"])
        self.assertFalse(review([row(position=2)], case)["passed"])
        self.assertFalse(review([row(x=2)], case)["passed"])
        self.assertFalse(review([row(state=5, selected=1)], case)["passed"])

    def test_positive_decisions_and_braking_require_observed_actions(self):
        case = dict(position=3, skill=5, expected="heal")
        self.assertFalse(review([row()], case)["passed"])
        self.assertTrue(review([row(state=5, native=236)], case)["passed"])
        self.assertFalse(review([row(state=5, native=245)], case)["passed"])
        case = dict(position=3, skill=5, expected="brake", switch=True)
        self.assertTrue(review([row(x=25, state=10), row(x=30)], case)["passed"])
        self.assertFalse(review([row(x=25), row(x=30, state=4)], case)["passed"])


if __name__ == '__main__':
    unittest.main()
