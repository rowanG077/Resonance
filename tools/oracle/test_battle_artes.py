"""Batch worker reuse must follow the requested input movie."""
from contextlib import chdir, redirect_stdout
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import battle_artes


class BatchTests(unittest.TestCase):
    def test_movie_changes_restart_workers_and_preserve_closure_receipts(self):
        cases = [dict(character=8, arte=arte, movie=movie)
                 for arte, movie in [(183, "ground"), (184, "ground"),
                                     (185, "aerial"), (186, "aerial"), (197, "ground")]]
        for failure in [None, "capture", "close"]:
            failed_case = "8:185" if failure == "capture" else None
            with self.subTest(failure=failure), TemporaryDirectory() as directory, chdir(directory):
                Path("local/battle-capture-work").mkdir(parents=True)
                args = SimpleNamespace(output=Path("local/battle-capture-work/batch"),
                                       case=[f"8:{case['arte']}" for case in cases],
                                       disc=Path("disc.rvz"), resume_from=None)

                def prepare(args, manifest):
                    case = battle_artes.selected(manifest, args.case)
                    args.output.mkdir()
                    battle_artes.write(args.output / "fixture.json", dict(
                        case=case, fixture=dict(marker_address=123),
                        movie_sha256=case["movie"], disc_sha256="disc"))
                    battle_artes.write(args.output / "locations.json", {})

                def capture(worker, output, record):
                    if output.name.replace("-", ":") == failed_case:
                        raise RuntimeError("capture failed")
                    return dict(complete=True, capture_complete=True)

                created = []

                def start_worker(*args, **kwargs):
                    worker = Mock()
                    created.append(worker)
                    if failure == "close" and len(created) == 3:
                        worker.close.side_effect = RuntimeError("incomplete memory observations")
                    return worker

                with (patch.object(battle_artes, "prepare_case", side_effect=prepare),
                      patch.object(battle_artes, "capture_case", side_effect=capture) as captures,
                      patch.object(battle_artes, "BattleFixture", side_effect=start_worker) as workers,
                      redirect_stdout(StringIO())):
                    report = battle_artes.batch(args, dict(cases=cases))

                self.assertEqual(workers.call_count, 3 if failed_case is None else 2)
                expected_workers = [call.args[0] for call in captures.call_args_list]
                self.assertIs(expected_workers[0], expected_workers[1])
                self.assertIsNot(expected_workers[1], expected_workers[2])
                if failed_case is None:
                    self.assertIs(expected_workers[2], expected_workers[3])
                    self.assertIsNot(expected_workers[3], expected_workers[4])
                for worker in set(expected_workers):
                    worker.close.assert_called_once_with(
                        complete=worker is not expected_workers[-1] or failed_case is None)
                self.assertEqual(report["complete"], failure is None)
                self.assertEqual(report["worker_closed"], failure != "close")
                self.assertEqual(battle_artes.read(args.output / "outcome.json"), report)
                if failure == "close":
                    self.assertEqual(report["error"], "incomplete memory observations")
                self.assertEqual(len({result["resident_worker"] for result in report["cases"]}),
                                 workers.call_count)
                for result in report["cases"]:
                    saved = battle_artes.read(args.output / result["case"].replace(":", "-") / "outcome.json")
                    self.assertEqual(saved, result)
                    self.assertEqual(saved["worker_closed"],
                                     failure != "close" or result["resident_worker"] != report["cases"][-1]["resident_worker"])


if __name__ == "__main__":
    unittest.main()
