"""Choose one CI owner for release commits and keep PR checks visible."""

import json
import os
from pathlib import Path
import re
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parents[2]


def plan(event_name, ref, sha, repository, event, open_prs=()):
    branch = ref.removeprefix("refs/heads/")
    same_repository = True
    commit = sha
    if event_name == "pull_request":
        head = event["pull_request"]["head"]
        branch = head["ref"]
        same_repository = (head.get("repo") or {}).get("full_name") == repository
        if same_repository and branch.startswith("release/"):
            # Publish the branch commit, never GitHub's temporary PR merge commit.
            commit = head["sha"]
    release = same_repository and branch.startswith("release/")
    version = branch.removeprefix("release/") if release else ""
    if release and not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("Release branches must be named release/vX.X.X")
    delegated = event_name == "push" and release and any(
        pr["head"]["ref"] == branch
        and (pr["head"].get("repo") or {}).get("full_name") == repository
        for pr in open_prs
    )
    return {
        "run_tests": not delegated,
        "publish": release and not delegated and event_name in ("push", "pull_request"),
        "version": version,
        "commit": commit,
    }


def main():
    event_name = os.environ["GITHUB_EVENT_NAME"]
    repository = os.environ["GITHUB_REPOSITORY"]
    ref = os.environ["GITHUB_REF"]
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    open_prs = []
    if event_name == "push" and ref.startswith("refs/heads/release/"):
        owner = repository.split("/", 1)[0]
        result = subprocess.run(
            ["gh", "api", "--method", "GET", f"repos/{repository}/pulls",
             "-f", "state=open", "-f", f"head={owner}:{ref.removeprefix('refs/heads/')}",
             "-f", "per_page=100"],
            capture_output=True, text=True,
        )
        if result.returncode:
            raise RuntimeError(f"Cannot look up open release PRs: {result.stderr.strip()}")
        open_prs = json.loads(result.stdout)
    decision = plan(event_name, ref, os.environ["GITHUB_SHA"], repository, event, open_prs)
    if decision["version"]:
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
        version = manifest["workspace"]["package"]["version"]
        if f"v{version}" != decision["version"]:
            raise ValueError(f"Release {decision['version']} does not match Cargo.toml version {version}")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        for key, value in decision.items():
            value = str(value).lower() if isinstance(value, bool) else value
            output.write(f"{key}={value}\n")
    if not decision["run_tests"]:
        print("An open release PR owns CI; skipping the duplicate push matrix.")
    else:
        print(f"Testing {decision['commit']}; publish={decision['publish']}")


if __name__ == "__main__":
    main()
