import unittest

from plan_workflow import plan


REPOSITORY = "example/resonance"
HEAD = "a" * 40
MERGE = "b" * 40


def pull_request(branch="release/v0.2.0", repository=REPOSITORY):
    return {"head": {"ref": branch, "sha": HEAD, "repo": {"full_name": repository}}}


class WorkflowPlanTests(unittest.TestCase):
    def test_release_pr_runs_checks_and_publishes_its_head_commit(self):
        result = plan("pull_request", "refs/pull/43/merge", MERGE, REPOSITORY,
                      {"pull_request": pull_request()})
        self.assertEqual(result, {"run_tests": True, "publish": True,
                                  "version": "v0.2.0", "commit": HEAD})

    def test_push_with_open_release_pr_delegates_to_pr(self):
        result = plan("push", "refs/heads/release/v0.2.0", HEAD, REPOSITORY,
                      {}, [pull_request()])
        self.assertFalse(result["run_tests"])
        self.assertFalse(result["publish"])

    def test_release_push_without_pr_still_builds_and_publishes(self):
        result = plan("push", "refs/heads/release/v0.2.0", HEAD, REPOSITORY, {})
        self.assertTrue(result["run_tests"])
        self.assertTrue(result["publish"])
        self.assertEqual(result["commit"], HEAD)

    def test_other_branch_or_fork_pr_cannot_suppress_release_push(self):
        for pr in [pull_request("release/v0.3.0"), pull_request(repository="fork/resonance")]:
            with self.subTest(pr=pr):
                result = plan("push", "refs/heads/release/v0.2.0", HEAD, REPOSITORY, {}, [pr])
                self.assertTrue(result["run_tests"])
                self.assertTrue(result["publish"])

    def test_normal_pr_keeps_merge_commit_testing_without_publication(self):
        result = plan("pull_request", "refs/pull/43/merge", MERGE, REPOSITORY,
                      {"pull_request": pull_request("feature/example")})
        self.assertTrue(result["run_tests"])
        self.assertFalse(result["publish"])
        self.assertEqual(result["commit"], MERGE)
        self.assertEqual(result["version"], "")

    def test_fork_release_pr_runs_tests_without_publication(self):
        for branch in ["release/v0.2.0", "release/example"]:
            with self.subTest(branch=branch):
                result = plan("pull_request", "refs/pull/43/merge", MERGE, REPOSITORY,
                              {"pull_request": pull_request(branch, "fork/resonance")})
                self.assertTrue(result["run_tests"])
                self.assertFalse(result["publish"])
                self.assertEqual(result["commit"], MERGE)
                self.assertEqual(result["version"], "")

    def test_main_push_and_manual_runs_only_test(self):
        for event, ref in [("push", "refs/heads/main"),
                           ("workflow_dispatch", "refs/heads/main"),
                           ("workflow_dispatch", "refs/heads/release/v0.2.0")]:
            with self.subTest(event=event, ref=ref):
                result = plan(event, ref, HEAD, REPOSITORY, {})
                self.assertTrue(result["run_tests"])
                self.assertFalse(result["publish"])

    def test_invalid_repository_release_names_are_rejected(self):
        for branch in ["release/v01.2.0", "release/v0.2", "release/example"]:
            with self.subTest(branch=branch), self.assertRaises(ValueError):
                plan("push", f"refs/heads/{branch}", HEAD, REPOSITORY, {})


if __name__ == "__main__":
    unittest.main()
