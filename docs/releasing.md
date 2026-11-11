# Publishing a release

Prepare a branch named `release/vX.X.X` locally in `nix develop`, for example:

```sh
git switch -c release/v0.4.0
# Set [workspace.package].version in Cargo.toml to 0.4.0, then:
cargo update --offline --workspace
# Ensure every entry links to its relevant PR before collecting.
qlog check --all
qlog collect --version 0.4.0 --insert CHANGELOG.md --delete
qlog clean
git diff -- CHANGELOG.md changelog Cargo.toml Cargo.lock
git add Cargo.toml Cargo.lock CHANGELOG.md changelog
git commit -m "Release version 0.4.0"
git push -u origin release/v0.4.0
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
Manual workflow runs only test by default; the explicit `publish_release` option
can resume publication on a `release/vX.Y.Z` branch.

Tests compile their test binaries and rendering example; distribution executables
and archives are built only for release branches in this repository. Private disc
images and cooked assets are not needed in CI.

Tests and rendering checks use `--profile release-fast`, which keeps level-3
optimization and disables LTO to reduce build/link time on all platforms,
including Windows MSVC. The final distribution executables use `--release`
with thin LTO and are packaged from `target/release`. Every architecture's build
and test job has a 180-minute (three-hour) timeout.

The installed-content residency regression checks field/world reentry, battle
handoff and bounded resource ownership. Optional diagnostics run separately:

```sh
python3 tools/residency_probe.py local/all-assets --trace
python3 tools/residency_probe.py local/all-assets --captures /tmp/residency --msaa 4 --audit-pipelines
python3 tools/residency_probe.py local/all-assets --trace --resolution 1920x1080 \
  --benchmark-rounds 11 --jsonl /tmp/transitions.jsonl
```

The benchmark mode repeats six fields and the overworld, entering the same battle
from both field 332 and the overworld on each round. It writes every completed
transition and renderer metadata as JSONL. Unlike the original diagnostic route,
battle benchmarks use the real-time simulation clock, with the shatter's minimum
duration disabled only in this diagnostic test. The outgoing-image capture,
transition pipeline warm-up, battle activation and opening-frame GPU completion
remain measured. Field entry fades settle outside the admission timer before
requesting the next transition. Save several independent launches, then run
`python3 tools/transition_histograms.py /tmp/run-*.jsonl --output /tmp/histograms`
with matplotlib and numpy installed. Histograms exclude each launch's first
route pass; the JSON summary retains those samples separately. Repeat visits
can still incur cache eviction, and the probe does not flush filesystem caches.

A Linux host without a usable Vulkan device can run the existing residency
regression through Mesa's headless software OpenGL renderer:

```sh
EGL_PLATFORM=surfaceless WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 \
  RESONANCE_TEST_ASSETS="$PWD/local/all-assets" \
  cargo test -p resonance-presentation --lib \
  field_ui::resident::tests::boot_and_field_reentry_reuse_uploaded_ui \
  -- --exact --ignored --test-threads=1
```

This checks rendering and resource lifetimes, but its timings describe the
software renderer rather than the player's GPU.

The probe uses a feature-gated, ignored internal driver so private scene hooks
stay out of the production API. It requires a working GPU backend. Screenshots
advance the scene and perform readbacks; omit `--captures` for timing runs.
Diagnostic scopes and schedule probes are excluded from ordinary tests and
distribution builds.
`--trace` also enables background blocking GPU polling for prompt transition
completion timestamps; ordinary runs use nonblocking polling.
Scene and battle timings use the completed transition record, including the
opening frame's GPU acknowledgement, before the driver requests another scene.

Review preparation changes by following each resource from capture to retirement,
including the ordinary path used when its preload misses. Check the same rules
across battle bodies, field bodies, actions, equipment, scenes, UI and audio:

| Boundary | Required behavior |
| --- | --- |
| Speculative preload fails, including discovery | Tolerant mode leaves a cache miss; strict mode preserves the error. |
| Failed resource is selected | The ordinary preparation path still validates it and reports its original error. |
| Preparation is cancelled | No cache entry is published; cancellation stays outside optional-error recovery. |
| Content or binding changes | Cached bytes, joint order and material state must still match the consumer. |
| Scene retires or cache evicts it | Scene state and resources release; fixed shared owners retain only their declared inventory. |
| GPU admission completes | Uploads, pipelines and the actual initial frame have completed before gameplay resumes. |
| Audio changes owners | Recover a stopped optional mixer, install the owning bank, and admit outgoing commands first; queue pressure preserves unsent commands and defers the handoff. |

Use a regression at the boundary that failed, then extend the existing installed
route for interactions; avoid duplicating every combination in separate tests.
Compile the diagnostic driver before relying on it:

```sh
cargo check -p resonance-presentation --tests --features preparation-trace
```

Record unavailable backend coverage explicitly.

For performance work, compare the same route, build profile, assets, resolution,
AA and backend before and after a change. Separate boot, first entry, reentry and
post-eviction entry; report several samples, including the median and slowest.
Use the scopes to identify repeated work or the phase delaying admission before
changing cache policy. Scope times are inclusive and workers overlap, so adding
them does not give transition latency. Initial save-fixture construction stays
outside the scene admission timer. Capture/readback runs are unsuitable for
latency comparisons. The driver uses manual simulation ticks; battle entry also
includes the fixed shatter animation, so separate it from preparation costs.
Treat reduced allocations or faster CPU preparation as supporting evidence;
validate the end-to-end GPU-ready transition and retained memory too.

The Windows pixel check uses Microsoft's pinned WARP software renderer and DXC
shader compiler, following
[wgpu's Windows CI setup](https://github.com/gfx-rs/wgpu/blob/trunk/.github/workflows/ci.yml).
Their SHA-256-verified DLLs
are staged beside the test example in `target/release-fast/examples`. The test
checks the loaded WARP driver version and still requires the DX12 CPU adapter
and all reference pixels. These test DLLs are not included in release archives.
Capture waits for the surface, sprite, movie and output pipelines to be ready
instead of assuming a fixed number of startup frames is enough. The Linux
regression check deliberately withholds the overlay shader for 80 frames and
then runs the same pixel assertions.

The shared renderer also replaces Bevy 0.20.1's embedded sprite shader with a
copy that uses a mutable local for the instance index. This works around
[Naga's DX12 instance-index alias bug](https://github.com/gfx-rs/wgpu/issues/4564),
which otherwise draws a translated sprite with another instance's transform.
The existing pixel check catches this: the white sprite must appear in the
upper-left corner, not at the center of the image.

For local Windows reproduction on an ARM Linux host, the Windows GNU build of
`ci_render.exe` can run through FEX and Wine/Proton's DX12 stack. Pass
`ci_render.exe OUTPUT_DIRECTORY [DELAY_FRAMES] --allow-hardware` to use Wine's
actual hardware adapter. Proton does not implement Microsoft's WARP adapter,
so this option explicitly relaxes only the CPU-adapter requirement; it keeps
DX12 and every pixel assertion. CI does not pass this option and continues to
verify the pinned native WARP driver. The original pixel failure was reproduced
under FEX/Wine before applying the shader workaround.

Each release platform runs those same tests before building and packaging release
executables. Every platform must pass before publication. New commits cancel
obsolete runs rather than waiting behind hours of older builds.

New changelog fragments live in `changelog/entries/`, with links to their relevant
PRs. See [qlog's documentation](https://github.com/qbaylogic/qlog) for entry management.
Local release preparation inserts the new notes into the top-level `CHANGELOG.md` and deletes
the consumed fragments. Older release sections remain in that file; individual
fragments are not archived. If more changes arrive after consolidation, add their
notes and PR links directly to the release section before pushing again.
CI validates that the release section exists and is nonempty and no pending
fragments remain. It does not consolidate, commit, push or dispatch another run.

After all builds pass, CI creates tag `vX.X.X` at the tested release-branch commit
and creates a GitHub release. For release PRs, checkout, testing, packaging and
tagging all use the PR head commit; GitHub's temporary merge commit is never
published. CI attaches a Windows ZIP, three Unix
`.tar.gz` archives, a matching `-symbols` archive for each platform, `README.md`
and `SHA256SUMS`. Only the matching release section from `CHANGELOG.md` becomes
the GitHub release body. The installation
guide remains the attached README and the README inside every archive. CI artifacts
remain available for 14 days; published GitHub release assets persist.

Only publication receives `contents: write`, using the automatic `GITHUB_TOKEN`.
No personal access token or additional secret is needed.
Repository or organization policy must permit GitHub Actions to create tags and
releases, and tag rules must permit this workflow to create `v*` tags.

If a build fails, fix the release branch and push again. No tag or release is
created before all platforms pass. If publication fails after creating a tag or
draft, rerun the failed job in the owning PR or push workflow at the same commit:
it can resume the draft and
replace incomplete uploads before publishing. A completed release is left alone
on reruns. An existing tag pointing to another commit causes an error; create a
new version branch rather than moving a published tag.

Pushes to `main`, ordinary PRs, fork PRs and default manual runs only run tests
and upload diagnostics. To retry publication, use **Re-run failed jobs** on the
release PR workflow, or on the release push workflow if there is no open PR.

Keep each release's `-symbols` archives for [debugging user crashes](crash-debugging.md).
CI verifies source resolution against the packaged executables before publication.
