import unittest

from strategy_characters import cases, review
from test_strategy_combos import row


def observed(**values):
    result = row(**values)
    result["battle_actor_0_flags"] |= 3 << 16
    return result


class CharacterReviewTests(unittest.TestCase):
    def test_throw_combo_requires_its_opening_stage_contact_and_payment(self):
        case = dict(skill=1, position=1, staged=True, expected_native=45,
                    cost=11, expected_opening=2, expected_stage=0)
        rows = [observed(links=0), observed(hp=29900, links=0),
                observed(native=45, links=0, hp=29900),
                observed(native=45, links=0, hp=29900, tp=89)]
        for r in rows:
            r["owner_style_word"] = 2 << 16
        self.assertTrue(review(rows, case, 1)["passed"])
        self.assertFalse(review(rows[:3], case, 1)["passed"])
        rows[0]["owner_style_word"] = 0
        self.assertFalse(review(rows, case, 1)["passed"])

    def test_overlimit_negative_requires_live_overlimit_and_a_normal(self):
        case = dict(skill=1, position=1, overlimit=True, expected_native=None)
        rows = [observed()]
        self.assertFalse(review(rows, case, 1)["passed"])
        rows[0]["battle_actor_0_flags"] = (2 << 27) | (4 << 16)
        self.assertTrue(review(rows, case, 1)["passed"])
        rows[0]["battle_actor_0_action"] = 3 << 24
        self.assertFalse(review(rows, case, 1)["passed"])

    def test_opening_comparison_rejects_a_later_matching_arte(self):
        rows = [observed(), observed(native=10, tp=95)]
        case = dict(skill=1, position=1, expected_first=10, cost=5)
        self.assertFalse(review(rows, case, 1)["passed"])
        self.assertTrue(review(rows, {**case, "expected_first": 0}, 1)["passed"])

    def test_summon_requires_active_admission_then_payment_and_consumption(self):
        case = dict(skill=1, position=1, overlimit=True, technique=237,
                    expected_native=286, cost=100)
        rows = [observed(native=286), observed(native=286, tp=0)]
        rows[0]["battle_actor_0_flags"] = (2 << 27) | (4 << 16)
        self.assertTrue(review(rows, case, 1)["passed"])
        rows[1]["battle_actor_0_flags"] = (2 << 27) | (4 << 16)
        self.assertFalse(review(rows, case, 1)["passed"])
        rows[0]["battle_actor_0_flags"] = (2 << 27) | (3 << 16)
        self.assertFalse(review(rows, case, 1)["passed"])

    def test_filtered_opening_idle_requires_a_sustained_stationary_window(self):
        case = dict(skill=1, position=1, expected_idle=True)
        rows = [observed(state=3, selected=0) for _ in range(180)]
        for r in rows:
            r.update(battle_actor_0_x_bits=0, battle_actor_0_y_bits=0, battle_actor_0_z_bits=0)
        self.assertTrue(review(rows, case, 1)["passed"])
        self.assertFalse(review(rows[:1], case, 1)["passed"])
        rows[50]["battle_actor_0_x_bits"] = 1
        self.assertFalse(review(rows, case, 1)["passed"])

    def test_edited_default_requires_the_live_character_table_value(self):
        case = dict(skill=1, position=1, expected_first=10, cost=5, default_position=5)
        rows = [observed(native=10), observed(native=10, tp=95)]
        for r in rows:
            r["default_position_word"] = 5 << 24
        self.assertTrue(review(rows, case, 1)["passed"])
        rows[1]["default_position_word"] = 1 << 24
        self.assertFalse(review(rows, case, 1)["passed"])

    def test_mixed_openings_distinguish_fallbacks_and_bind_the_selected_cost(self):
        case = dict(skill=1, position=1, native_pool=[1, 2], native_costs={1: 4, 2: 16},
                    allow_normal_opening=True)
        self.assertTrue(review([observed()], case, 1)["passed"])
        self.assertFalse(review([observed()], {**case, "allow_normal_opening": False}, 1)["passed"])
        rows = [observed(native=2), observed(native=2, tp=84)]
        self.assertTrue(review(rows, case, 1)["passed"])
        rows[1]["battle_actor_0_tp_word"] = 96
        self.assertFalse(review(rows, case, 1)["passed"])

    def test_elemental_inputs_are_verified_through_admission_before_discovery(self):
        case = cases(3, "elemental")[0]
        rows = [observed(state=3, native=0, selected=0), observed(native=200), observed(native=200, tp=92)]
        for r in rows:
            r.update(owner_strategy=(1 << 24) | (1 << 16) | (5 << 8),
                     target_traits_word=0, target_affinities_0=0,
                     target_affinities_4=1 << 24, owner_level_word=20 << 16,
                     owner_saved_level_word=20 << 24, level_difference_word=0)
        rows[-1]["target_traits_word"] = 0x20
        self.assertTrue(review(rows, case, 1)["passed"])
        rows[1]["target_traits_word"] = 0x20
        self.assertFalse(review(rows, case, 1)["passed"])


if __name__ == "__main__":
    unittest.main()
