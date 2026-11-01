import unittest

from strategy_combos import review


def row(*, state=5, native=0, selected=1, tp=100, hp=30000, links=2, skill=1, level=30):
    return dict(owner_strategy=(1 << 24) | (skill << 16) | (1 << 8),
                battle_actor_0_flags=2 << 27, owner_enabled_0=0, owner_enabled_4=1,
                battle_actor_0_action=state << 24, owner_selected=selected, owner_native=native,
                battle_actor_4_hp=hp, battle_actor_0_tp_word=tp,
                battle_actor_1_action=3 << 24, battle_actor_4_action=3 << 24,
                owner_combo_word=(links << 16) | (3 << 5), owner_style_word=0,
                owner_vitals_level_word=level << 16, owner_saved_level_word=level << 24,
                level_difference_word=max(-8, min(8, (level + 20) // 2 - 20)) & 255,
                owner_learned_0=0, owner_learned_4=1)


class ComboReviewTests(unittest.TestCase):
    case = dict(position=1, skill=1, expected_native=10)

    def test_positive_requires_prior_effective_normal_and_correct_payment(self):
        rows = [row(), row(hp=29900, selected=2), row(native=10, hp=29900), row(native=10, tp=95, hp=29900)]
        self.assertTrue(review(rows, self.case, 1)["passed"])
        self.assertFalse(review([], self.case, 1)["passed"])
        self.assertFalse(review(rows[:3], self.case, 1)["passed"])
        self.assertFalse(review([row(native=10), row(native=10, tp=95, hp=29900)], self.case, 1)["passed"])
        self.assertFalse(review([row(), row(native=10, tp=95, hp=29900)], self.case, 1)["passed"])

    def test_negative_requires_actual_normal_continuations_and_contacts(self):
        case = dict(position=1, skill=1, expected_native=None)
        self.assertFalse(review([row(state=3, selected=0)], case, 1)["passed"])
        self.assertFalse(review([row(), row(hp=29900)], case, 1)["passed"])
        self.assertTrue(review([row(), row(selected=2, hp=29900)], case, 1)["passed"])

    def test_level_thirty_early_arte_is_rejected(self):
        rows = [row(links=0), row(links=0, hp=29900), row(native=10, links=0, tp=95, hp=29900)]
        self.assertFalse(review(rows, self.case, 1)["passed"])
        for r in rows:
            r.update(owner_vitals_level_word=1 << 16, owner_saved_level_word=1 << 24, level_difference_word=248)
        self.assertTrue(review(rows, {**self.case, "level": 1}, 1)["passed"])

    def test_edited_reserve_requires_the_observed_table_value(self):
        rows = [row(), row(hp=29900), row(native=10, tp=95, hp=29900)]
        case = {**self.case, "tp_reserve": 0}
        for r in rows:
            r["reserve_tail_word"] = 100 << 16
        self.assertFalse(review(rows, case, 1)["passed"])
        for r in rows:
            r["reserve_tail_word"] = 0
        self.assertTrue(review(rows, case, 1)["passed"])


if __name__ == '__main__':
    unittest.main()
