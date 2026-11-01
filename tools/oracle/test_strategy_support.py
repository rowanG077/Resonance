import unittest

from strategy_support import review


def row(*, native=236, tp=100, hp=400, state=5, enabled=1, other_action=3):
    return dict(battle_actor_1_action=state << 24, actor_1_native=native,
                actor_1_support_target=123, battle_actor_1_tp_word=(100 << 16) | tp,
                owner_strategy=(1 << 24) | (8 << 16) | (2 << 8),
                battle_actor_1_flags=(2 << 27) | (3 << 16),
                battle_actor_0_action=other_action << 24, battle_actor_4_action=3 << 24,
                battle_actor_0_hp=hp, battle_actor_1_hp=400,
                battle_actor_0_flags=3 << 16, actor_0_conditions_low=0, actor_1_conditions_low=0,
                owner_enabled_0=0, owner_enabled_4=enabled)


class SupportReviewTests(unittest.TestCase):
    case = dict(native=236, targets=[0], cost=8, benefit="hp")

    def test_admission_alone_never_passes(self):
        self.assertFalse(review([], self.case, {123: 0}, 1)["passed"])
        self.assertFalse(review([row()], self.case, {123: 0}, 1)["passed"])
        self.assertFalse(review([row(), row(tp=92)], self.case, {123: 0}, 1)["passed"])

    def test_payment_and_released_benefit_allow_retired_cast_pose(self):
        rows = [row(), row(tp=92), row(native=0, tp=92, hp=1000, state=6)]
        result = review(rows, self.case, {123: 0}, 1)
        self.assertTrue(result["passed"])
        self.assertEqual(result["benefit_changes"][0]["hp_after"], 1000)
        self.assertFalse(review(rows, self.case, {123: 1}, 1)["passed"])

    def test_benefit_before_payment_is_not_attributed_to_this_spell(self):
        rows = [row(), row(hp=1000), row(tp=92, hp=1000)]
        self.assertFalse(review(rows, self.case, {123: 0}, 1)["passed"])

    def test_replaced_spells_enable_edits_and_competing_actions_fail(self):
        for middle in (row(native=239, tp=92), row(tp=92, enabled=3), row(tp=92, other_action=5)):
            rows = [row(), middle, row(tp=92, hp=1000)]
            self.assertFalse(review(rows, self.case, {123: 0}, 1)["passed"])

    def test_later_pulses_require_their_own_transitions_and_one_cast(self):
        case = dict(self.case, pulses=4)
        rows = [row(), row(tp=92)]
        for hp in (700, 1000, 1300, 1600):
            pulse = row(tp=92, hp=hp)
            pulse["battle_actor_1_hp"] = hp
            rows.append(pulse)
        self.assertTrue(review(rows, case, {123: 0}, 1)["passed"])
        self.assertFalse(review(rows[:-1], case, {123: 0}, 1)["passed"])
        interrupted = rows[:3] + [row(tp=92, hp=700, state=3)] + rows[3:]
        self.assertFalse(review(interrupted, case, {123: 0}, 1)["passed"])

    def test_declared_ailments_and_live_table_edit_are_required(self):
        case = dict(self.case, cursed=[0], healing_threshold=50)
        rows = [row(), row(tp=92), row(tp=92, hp=1000)]
        for item in rows:
            item["actor_0_conditions_low"] = 0x80
            item["healing_tail_word"] = 50 << 24
        self.assertTrue(review(rows, case, {123: 0}, 1)["passed"])
        rows[1]["healing_tail_word"] = 70 << 24
        self.assertFalse(review(rows, case, {123: 0}, 1)["passed"])
        rows[1]["healing_tail_word"] = 50 << 24
        rows[0]["actor_0_conditions_low"] = 0
        self.assertFalse(review(rows, case, {123: 0}, 1)["passed"])


if __name__ == '__main__':
    unittest.main()
