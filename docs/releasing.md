# Publishing a release

Push a branch named `release/vX.X.X`, for example:

```sh
git switch -c release/v0.2.0
# Set [workspace.package].version in Cargo.toml to 0.2.0, then:
cargo update --offline --workspace
git add Cargo.toml Cargo.lock
git commit -m "Release version 0.2.0"
git push -u origin release/v0.2.0
```

The branch must contain the release workflow. Use three numeric version
components with no leading zeroes. Update the workspace version in `Cargo.toml`
and its lockfile before pushing. CI requires the workspace version and both
executables' `--version` output to match the release branch. The branch name
sets the GitHub release version and archive filenames.

The **Test and release** workflow runs the test matrix on every pull request,
push to `main`, and manual workflow run. It tests Windows x64, macOS Apple
Silicon, Linux x64 and Linux ARM64 on native GitHub runners, including synthetic
audio/render checks. Tests compile their test binaries and rendering example;
distribution executables and archives are built only on release-branch pushes.
Private disc images and cooked assets are not needed in CI.

On a push to `release/vX.X.X`, each platform runs those same tests before
building and packaging release executables. Every platform must pass before
publication.

After all builds pass, CI creates tag `vX.X.X` at the exact commit that triggered
the run and creates a GitHub release. It attaches a Windows ZIP, three Unix
`.tar.gz` archives, `README.md` and `SHA256SUMS`. The installation guide is both
the release body and the README inside every archive. CI artifacts remain
available for 14 days; published GitHub release assets persist.

Only the publication job receives `contents: write`; it uses the repository's
automatic `GITHUB_TOKEN`. No personal access token or additional secret is needed.
Repository or organization policy must permit GitHub Actions to create tags and
releases, and tag rules must permit this workflow to create `v*` tags.

If a build fails, fix the release branch and push again. No tag or release is
created before all platforms pass. If publication fails after creating a tag or
draft, rerun the failed job at the same commit: it can resume the draft and
replace incomplete uploads before publishing. A completed release is left alone
on reruns. An existing tag pointing to another commit causes an error; create a
new version branch rather than moving a published tag.

Pushes to `main`, pull requests and manual workflow runs only run the tests and
upload smoke-test diagnostics; they do not build distribution archives or create tags.
To retry publication, use **Re-run failed jobs** on the original release-branch
push run. A manual workflow run never publishes, even from a release branch.
