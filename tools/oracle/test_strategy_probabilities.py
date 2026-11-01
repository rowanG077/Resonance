import unittest

from strategy_probabilities import advance, cases, draws, review


def row(*, seed=6, age=15, native=0, tp=100, skill=1, level=15):
    return dict(owner_strategy=(1 << 24) | (skill << 16) | (1 << 8),
                battle_actor_0_flags=(2 << 27) | (3 << 16),
                battle_actor_0_tp_word=(100 << 16) | tp,
                owner_enabled_0=0, owner_enabled_4=1,
                owner_level_word=level << 16, owner_saved_level_word=level << 24,
                owner_learned_0=0, owner_learned_4=1,
                battle_actor_1_action=3 << 24, battle_actor_4_action=3 << 24,
                owner_selected=123, owner_native=native,
                battle_actor_0_cast_clock_word=age, battle_random_state_word=seed)


class ProbabilityReviewTests(unittest.TestCase):
    initial = dict(age=14, checkpoint=15, selected=123)

    def test_seed_search_covers_each_declared_residue(self):
        for phase, count in (("level", 8), ("reserve", 4), ("throw", 6), ("throw-level", 5)):
            generated = cases(phase)
            self.assertEqual(len(generated), count)
            for case in generated:
                self.assertEqual(tuple(x % limit for x, limit in zip(draws(case["seed"], len(case["residues"])), case["moduli"])), case["residues"])

    def test_admission_needs_checkpoint_rng_and_payment(self):
        case = cases("level")[0]
        rows = [row(), row(seed=advance(6), native=10), row(seed=advance(6), native=10, tp=95)]
        self.assertTrue(review(rows, case, self.initial, 1)["passed"])
        self.assertFalse(review(rows[:2], case, self.initial, 1)["passed"])
        rows[0]["battle_random_state_word"] = 7
        self.assertFalse(review(rows, case, self.initial, 1)["passed"])

    def test_negative_requires_progress_and_an_actual_random_visit(self):
        case = cases("level")[1]
        rows = [row(seed=case["seed"]), row(seed=advance(case["seed"]), age=16)]
        self.assertTrue(review(rows, case, self.initial, 1)["passed"])
        self.assertFalse(review(rows[:1], case, self.initial, 1)["passed"])
        rows[1]["battle_random_state_word"] = case["seed"]
        self.assertFalse(review(rows, case, self.initial, 1)["passed"])

    def test_direct_reserve_boundary_has_one_gate_draw_and_one_choice_draw(self):
        initial = dict(self.initial, age=15)
        for case in (cases("reserve")[0], cases("reserve")[2]):
            state = advance(case["seed"])
            if case["admitted"]:
                state = advance(state)
            native = 10 if case["admitted"] else 0
            rows = [row(seed=state, age=16, native=native, tp=30, skill=6, level=30),
                    row(seed=state, age=17, native=native, tp=25 if native else 30, skill=6, level=30)]
            self.assertTrue(review(rows, case, initial, 1)["passed"])
            rows[0]["battle_random_state_word"] = advance(state)
            self.assertFalse(review(rows, case, initial, 1)["passed"])


if __name__ == "__main__":
    unittest.main()
