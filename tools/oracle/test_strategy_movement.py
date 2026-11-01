import struct
import unittest

from strategy_movement import review


def row(*, action=3, x=0., age=0, hp=30000):
    word = lambda value: int.from_bytes(struct.pack(">f", value))
    return dict(owner_strategy=(1 << 24) | (5 << 16) | (3 << 8),
        battle_actor_1_flags=2 << 27, battle_actor_1_action=action << 24,
        battle_actor_0_action=3 << 24, battle_actor_4_action=3 << 24,
        battle_actor_1_x_bits=word(x), battle_actor_1_z_bits=word(0.),
        owner_selected=1 if action == 5 else 0, owner_native=0,
        owner_home_x=word(0.), owner_home_z=word(0.), owner_age_word=age,
        battle_actor_4_hp=hp, owner_run_speed_word=word(0.01))


class MovementReviewTests(unittest.TestCase):
    def test_idle_negative_requires_a_real_window_and_rejects_attacks(self):
        case = dict(mode="idle", skill=5, position=3, expected="stay")
        self.assertFalse(review([], case)["passed"])
        self.assertTrue(review([row(), row()], case)["passed"])
        self.assertFalse(review([row(), row(action=5)], case)["passed"])

    def test_return_requires_recovery_contact_completion_and_progress(self):
        case = dict(mode="return", skill=5, position=3, expected_home=[0., 0.])
        rows = [row(action=5, x=200.), row(action=8, x=200., hp=29900),
                row(action=6, x=200., hp=29900), row(action=3, x=40., hp=29900)]
        self.assertTrue(review(rows, case)["passed"])
        self.assertFalse(review(rows[:-1], case)["passed"])
        self.assertFalse(review(rows[1:], case)["passed"])
        rows[-1] = row(action=3, x=220., hp=29900)
        self.assertFalse(review(rows, case)["passed"])

    def test_timeout_is_bound_to_the_first_approach_and_its_exit_age(self):
        case = dict(mode="timeout", skill=5, position=3, run_speed=0.01)
        rows = [row(action=4, age=0), row(action=4, age=299),
                row(action=4, age=300), row(action=10, age=301)]
        self.assertTrue(review(rows, case)["passed"])
        self.assertFalse(review(rows[:-1], case)["passed"])
        rows[-1] = row(action=10, age=30)
        self.assertFalse(review(rows, case)["passed"])


if __name__ == "__main__":
    unittest.main()
