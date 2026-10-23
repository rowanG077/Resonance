# Catalogue arte captures

`battle_artes.py` uses one tracked manifest with capture recipes for 249
character/arte bindings. IDs are explicit even when characters share an action.
The active comparison inventory has 1,446 bindings: 249 party bindings and
1,197 enemy bindings.

`excluded_comparisons` removes 36 reviewed unused or unreachable bindings and
five redundant forced-action comparisons.
These cases are removed from comparison selection, not counted as passing or
left pending. An explained random or numerical mismatch does not justify
removing an entire binding: its other gameplay checks can still be useful.

The duplicate comparisons retain the same enemy and complete action execution.
These fixtures override selection weights, admit one action, and disable later
automatic selection. They do not test admission rejection or whether a second
action may repeat. The return flag also makes no difference in the retained
fixture because the actor stays at its formation position throughout.
These removals concern action execution, not natural selection or retreat behavior.

| Removed comparison | Retained comparison | Redundant coverage |
| --- | --- | --- |
| `enemy:218:1`, `enemy:218:2` | `enemy:218:0` | All 87 native frames and 84 events are identical. Return is unobservable at the actor's unchanged home position; no later selection observes repeat state. |
| `enemy:234:20` | `enemy:234:19` | All 514 source events are identical. Across 425 frames, only the owner's unchanged starting HP differs: 30,000 versus 60,000, satisfying each admission threshold. |
| `enemy:236:5` | `enemy:236:3` | All 384 source frames and 579 events are identical; native arrays are identical too. Selection weight is overridden. |
| `enemy:230:5` | `enemy:230:4` | All 168 native frames and 378 events are identical. No later selection observes the different once-per-battle state. |

From the repository root, inside the development environment:

```sh
python tools/oracle/battle_artes.py list --character 3
python tools/oracle/battle_artes.py prepare --case 3:68 \
  --output local/battle-capture-work/artes/explosion-attempt01
python tools/oracle/battle_artes.py run \
  --prepared local/battle-capture-work/artes/explosion-attempt01 \
  --disc /path/to/Disc1.rvz
python tools/oracle/battle_artes.py review \
  --output local/battle-capture-work/artes/explosion-attempt01
```

Preparation does not launch Dolphin. The manifest pins the base state, its movie,
the authored technique catalogue and disc hash. Use `--state` and `--techniques`
to relocate identical inputs. A mismatch stops preparation. Required source
assets and checkpoints are local inputs, not repository blobs.

Run captures under the existing bounded source-worker scheduler.
The adapter starts one worker and closes it on success or failure. It never
starts parallel jobs or retries. Captures, checkpoints and image dumps stay under the fresh attempt directory in
`local/battle-capture-work`. Keep the scheduler's short `TMPDIR` for Unix socket
paths; do not replace it with the long attempt path. Only Dolphin's live IPC uses
shared memory. A prior outcome prevents reuse of that attempt.

For several cases, reuse one resident worker:

```sh
python tools/oracle/battle_artes.py batch --case 7:159 --case 7:160 \
  --output local/battle-capture-work/artes/presea-attempt01 \
  --disc /path/to/Disc1.rvz
```

Each case gets a separate prepared input, capture and review. A mechanical
failure preserves that result and allows the next fresh case; a capture or
registration failure stops the batch. There is no automatic retry. The root
batch receipt records whether the shared worker closed.

After a stopped batch, select only uncaptured cases and pass
`--resume-from PATH_TO_OLD_BATCH` with a fresh `--output`. Matching prepared
inputs are hardlinked and a retained, validated warm checkpoint skips warmup.
An existing capture is refused so its evidence can be reviewed first.

The ordinary presets consolidate the retained martial, elemental, recovery and
summon fixture setup: authored character/technique grants, route-compatible
learning, one future B press, and controlled HP/TP. Recovery fixtures provide a
living, dead or conditioned target. Colette receives the measured post-seal
story state and the authored transformation costume command. Martial and summon
fixtures receive saved OverLimit; the receipt records this assistance. Known special inputs and unresolved
gates have `pending_setup` entries; `prepare` refuses those instead of trying an
ordinary shortcut. Their entries remain part of the required catalogue.

Regal's Eagle Dive, Eagle Rage, Eagle Fall and Triple Rage Kick use the measured
manual aerial preset: 32 polls holding up, eight neutral polls, then four polls
of B. Each binding has a retained successful source execution. Special guards
hold X for eight polls, then X and down for four polls. Lloyd's source pilot
verified this input, its 10% maximum TP debit, and normal recovery; the other
eight bindings have separate capture cases.

`runnable` means a preparation path exists. It does not mean that this particular
character/arte has run successfully. The retained coverage ledger is separate
from the manifest, and a source capture is separate from native comparison.

The automatic review checks the actual requested action identity, one expected
TP payment, recovery and retirement of observed owned projectiles. Interrupted
martial actions fail. A paid spell without an active phase and cleanup also
fails. Registered observations are compressed; the capture receipt and a small
set of transition images are retained. Registration failures preserve their raw
attempt output.

`mechanical_pass` is only the automated result. Review the named caption,
effect/contact or support result, and cleanup images before accepting the arte
receipt. Projectile births and unrelated HP changes alone do not prove the
requested outcome. Pixel parity, every alternate entry condition, natural
learning and native implementation correctness are not implied by a passing
source capture.

## Catalogue comparison in Rust

`battle::oracle::tests::every_arte_matches_dolphin` is an explicit integration test
in `resonance-presentation`. It enumerates learned player bindings and authored
enemy rows, removes the manifest's exclusions, obtains source observations through the shared Dolphin adapter, then
prepares and executes native actions with the same participant setup. Worker reuse
requires matching observers and boot input; a changed finalized input movie starts
a new worker. Missing setup, contact, effect observations or retirement fail the
case and stay in `report.json`.
The earlier `mechanical_pass` receipts are not accepted as equivalence traces.
The shared fixture gives the acting participant offensive stats of 400 and uses
the same one-minute observation budget for both executions. Damage is checked
against independently calculated bounds; the random streams need not match.

For a focused run:

```sh
RESONANCE_TEST_ASSETS="$PWD/local/all-assets" \
RESONANCE_DOLPHIN_DISC=/path/to/Disc1.rvz \
RESONANCE_ARTE_CASES=party:2:35 \
RESONANCE_ARTE_OUTPUT="$PWD/local/battle-capture-work/arte-equivalence/attempt01" \
cargo test --offline --locked -p resonance-presentation --lib \
  every_arte_matches_dolphin -- --ignored --nocapture
```

Omit `RESONANCE_ARTE_CASES` for the catalogue. `RESONANCE_ARTE_SHARD=0/16`
selects a deterministic shard; the coordinator may schedule all sixteen under the
existing aggregate memory cap. `RESONANCE_ARTE_REFERENCE` reuses an explicitly
selected source trace directory. Output directories must be new. The test does
not run during ordinary unit testing, recook assets, or run the workspace suite.

The per-case report from matched complete observations establishes what passed.
Manifest membership and successful preparation alone do not establish acceptance.

The comparison covers actual contact or benefit, movement, selected poses,
projectiles, deterministic effect models and cleanup. Body-bone geometry is
diagnostic because the sampled source matrix has no verified publication time.
Exact pixels/audio and random cosmetic trajectories are outside this test.
