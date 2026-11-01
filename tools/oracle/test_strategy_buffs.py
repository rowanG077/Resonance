import unittest

from strategy_buffs import review


def row(action, native=0, selected=0):
    return dict(battle_actor_1_action=action << 24, actor_1_native=native,
                actor_1_selected=selected)


class StrategyBuffTests(unittest.TestCase):
    def test_buff_requires_the_first_actual_admission(self):
        self.assertTrue(review([row(3), row(5, 245)], True)["passed"])
        self.assertFalse(review([row(5, 254), row(5, 245)], True)["passed"])
        self.assertFalse(review([row(3, 245)], True)["passed"])

    def test_negative_case_requires_a_normal_decision(self):
        self.assertFalse(review([], False)["passed"])
        self.assertFalse(review([row(3)], False)["passed"])
        self.assertTrue(review([row(3), row(5, selected=123)], False)["passed"])
        self.assertFalse(review([row(5, 245), row(5, selected=123)], False)["passed"])


if __name__ == "__main__":
    unittest.main()
