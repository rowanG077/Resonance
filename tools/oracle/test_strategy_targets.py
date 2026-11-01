import unittest

from strategy_targets import review


class StrategyTargetTests(unittest.TestCase):
    def test_decision_requires_the_declared_live_policy_and_known_recipient(self):
        row = dict(battle_actor_1_action=4 << 24, battle_actor_1_target=123,
                   actor_1_strategy=(8 << 24) | (5 << 16) | (2 << 8))
        self.assertTrue(review([row], {123: 6}, [6], 8)["passed"])
        self.assertFalse(review([row], {123: 6}, [6], 1)["passed"])
        self.assertFalse(review([row], {}, [6], 8)["passed"])
        row["battle_actor_1_action"] = 3 << 24
        self.assertFalse(review([row], {123: 6}, [6], 8)["passed"])
        self.assertFalse(review([], {123: 6}, [6], 8)["passed"])


if __name__ == "__main__":
    unittest.main()
