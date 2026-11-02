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

The **Test and release** workflow tests Windows x64, macOS Apple Silicon, Linux
x64 and Linux ARM64 on native GitHub runners, including synthetic audio/render
checks. Normal PRs run when opened, reopened or updated with new commits
(`synchronize`). `main` runs on push. Release branches with an open PR run their
checks and release build in the PR workflow, so the checks page shows real work.
The corresponding push workflow detects the open PR and skips its duplicate
matrix. Release branches without an open PR run directly on push.
Fork PRs run tests without publishing, including forks with a `release/*` branch.
Manual workflow runs remain available and never publish.

Tests compile their test binaries and rendering example; distribution executables
and archives are built only for release branches in this repository. Private disc
images and cooked assets are not needed in CI.

Tests and rendering checks use `--profile release-fast`, which keeps level-3
optimization and disables LTO to reduce build/link time on all platforms,
including Windows MSVC. The final distribution executables use `--release`
with thin LTO and are packaged from `target/release`. Every architecture's build
and test job has a 180-minute (three-hour) timeout.

The Windows pixel check uses Microsoft's pinned WARP software renderer and DXC
shader compiler, following
[wgpu's Windows CI setup](https://github.com/gfx-rs/wgpu/blob/trunk/.github/workflows/ci.yml).
Their SHA-256-verified DLLs
are staged beside the test example in `target/release-fast/examples`. The test
checks the loaded WARP driver version and still requires the DX12 CPU adapter
and all reference pixels. These test DLLs are not included in release archives.

Each release platform runs those same tests before building and packaging release
executables. Every platform must pass before publication. New commits cancel
obsolete runs rather than waiting behind hours of older builds.

After all builds pass, CI creates tag `vX.X.X` at the tested release-branch commit
and creates a GitHub release. For release PRs, checkout, testing, packaging and
tagging all use the PR head commit; GitHub's temporary merge commit is never
published. CI attaches a Windows ZIP, three Unix
`.tar.gz` archives, `README.md` and `SHA256SUMS`. The installation guide is both
the release body and the README inside every archive. CI artifacts remain
available for 14 days; published GitHub release assets persist.

Only the publication job receives `contents: write`; it uses the repository's
automatic `GITHUB_TOKEN`. No personal access token or additional secret is needed.
Repository or organization policy must permit GitHub Actions to create tags and
releases, and tag rules must permit this workflow to create `v*` tags.

If a build fails, fix the release branch and push again. No tag or release is
created before all platforms pass. If publication fails after creating a tag or
draft, rerun the failed job in the owning PR or push workflow at the same commit:
it can resume the draft and
replace incomplete uploads before publishing. A completed release is left alone
on reruns. An existing tag pointing to another commit causes an error; create a
new version branch rather than moving a published tag.

Pushes to `main`, ordinary PRs, fork PRs and manual workflow runs only run tests
and upload diagnostics. To retry publication, use **Re-run failed jobs** on the
release PR workflow, or on the release push workflow if there is no open PR.
