"""A loaded state must execute its requested future controller movie."""
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch

import battle_fixture
from battle_fixture import validate_replayed_inputs


def movie(*polls):
    return bytes(256) + b"".join(bytes([value]) * 8 for value in polls)


class ReplayInputTests(unittest.TestCase):
    def test_rejects_the_boot_movies_stale_future(self):
        with self.assertRaisesRegex(RuntimeError, "different controller input"):
            validate_replayed_inputs(movie(0, 0, 4, 0), movie(0, 0, 2, 0), 2, 4)

    def test_history_metadata_and_unconsumed_future_do_not_change_the_check(self):
        requested = movie(1, 2, 4, 0)
        executed = bytearray(movie(9, 8, 4, 7))
        executed[81:97] = b"Vulkan".ljust(16, b"\0")
        validate_replayed_inputs(requested, executed, 2, 3)

    def test_accepts_an_unchanged_consumed_interval(self):
        validate_replayed_inputs(movie(0, 2, 4), movie(0, 2, 4), 1, 3)

    def test_rejects_missing_polls_and_backwards_cursors(self):
        for requested, executed, start, end in [
                (movie(0), movie(0, 2), 0, 2),
                (movie(0, 2), movie(0), 0, 2),
                (movie(0, 2), movie(0, 2), 2, 1),
                (movie(0), movie(0), -1, 0)]:
            with self.subTest(start=start, end=end):
                with self.assertRaisesRegex(RuntimeError, "truncated|invalid cursors"):
                    validate_replayed_inputs(requested, executed, start, end)

    def test_loading_a_fork_fails_before_dolphin_is_resumed(self):
        with TemporaryDirectory() as directory:
            output = Path(directory)
            state = output / "fork.s01"
            Path(str(state) + ".dtm").write_bytes(movie(0, 0, 4, 0))
            worker = battle_fixture.BattleFixture.__new__(battle_fixture.BattleFixture)
            worker.output = output
            with (patch.object(battle_fixture, "State"),
                  patch.object(battle_fixture, "inspect", return_value={"movie": {"input_count": 2}}),
                  patch.object(battle_fixture.effect_workers.DolphinFixture, "load") as load):
                for boot, error in [(movie(0, 0, 2, 0), "fresh worker"),
                                    (movie(0, 0, 4), "truncated")]:
                    with self.subTest(error=error):
                        (output / "input.dtm").write_bytes(boot)
                        with self.assertRaisesRegex(RuntimeError, error):
                            worker.load(state)
                load.assert_not_called()

    def test_compatible_load_retains_the_interval_for_snapshot_verification(self):
        with TemporaryDirectory() as directory:
            output = Path(directory)
            (output / "input.dtm").write_bytes(movie(0, 0, 4, 0))
            state = output / "compatible.s01"
            requested = movie(8, 9, 4, 0)
            Path(str(state) + ".dtm").write_bytes(requested)
            worker = battle_fixture.BattleFixture.__new__(battle_fixture.BattleFixture)
            worker.output = output
            worker.watcher = SimpleNamespace(rows=12)
            with (patch.object(battle_fixture, "State"),
                  patch.object(battle_fixture, "inspect", return_value={"movie": {"input_count": 2}}),
                  patch.object(battle_fixture.effect_workers.DolphinFixture, "load") as load):
                worker.load(state)
                load.assert_called_once_with(state)
            self.assertEqual(worker._replay_input, (requested, 2))


if __name__ == "__main__":
    unittest.main()
