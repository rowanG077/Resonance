# Battle implementation and evidence

The battle crate implements headless combat simulation. Stage 3 is in progress:
field requests suspend and retain their event owner, and the rendered opening
route completes both battles and returns free field control. Original-game state,
image and audio fidelity remain under validation.
Runtime startup now defaults to fault-tolerant diagnostics. Missing resources and
unsupported independent objects log errors and are skipped; repeated occurrences
are counted. `--paranoid` preserves fail-fast validation. Recordings include the
mode and diagnostic list, and recovered errors make them invalid for fidelity
acceptance. A simulation that skipped faulty gameplay cannot commit battle rewards;
missing presentation alone does not invalidate its gameplay result. Mandatory
scene failures return to the title without inventing a field-event outcome.
The error/atomicity statements below describe paranoid mode and strict library
constructors. Stage 3 fidelity runs continue to use paranoid mode.
The current increment provides runtime-only prepared
actions, owned VM tasks, action clocks, admission/TP payment, ordered presentation
requests, recipient recovery, independent standard projectile motion/lifetimes,
projectile clashes, actor hurt geometry, ordinary physical/magic damage and
on-contact guard selection/reduction, HP changes, per-target cooldowns and delayed
contact retirement, script-owned ordinary melee hit streams, and simulation-owned
body model playback with sampled hurt points and rigid weapon anchors, linear
actor integration/braking, script-owned motion values and action recovery,
independent primary/secondary spell slots, authored ordinary casting timing,
ordinary contact recoil/hitstun and body motion binding, and ordinary hurt/guard
countdown and recovery paths, plus stun entry, retained stars and recovery.
Ordinary stagger buildup, down/get-up clocks and contact protection now execute;
their remaining original branches and oracle limits are recorded below.
Hit elements now distinguish inherited, explicitly neutral and elemental rules;
inheritance observes the live attacker at contact time. Action armor now preserves
damage while suppressing guard resolution and interruption until the following
contact after its threshold is reached. Nonzero armor branches have source-derived
tests; the current opening Dolphin case only observes zero armor thresholds.
Nurse's stored casting/recovery sequence and Lightning's release callback are
readable source. Two scene slots retain independent lifetimes, with a separate
transition clock and actor drawing visibility. Actor/projectile particle
attachments survive their emitting effect and retain stable handles.
Genis's ordinary Lightning now executes through cold preparation, casting, its
original projectile/hit rule and effect 28 in headless tests. Resident initialization
replaces defeated or petrified targets in same-side roster order and retains the
selected origin. A controlled Dolphin cast verifies its fourteen effect emissions,
366 particle updates and sound-92 dispatch; full Lightning route and render/audio
playback acceptance remain pending.
Nurse's transition timing, sampled centers and complete common-37 particle
sequence match the controlled original observations. Its maintained source now
emits the flying models through the shared effect VM. Scene-owned model instances,
particle motion, animation and secondary motion match the three original instances
through their captured visits and particle expiry. Recipient program 6 now executes
before recovery, with profile scaling and recipient ownership. Its modifiers and
particle motion match all 51 original emissions and 2,091 visits in component tests.
Actor/stage tint and the stored-scene camera return now run in the same battle
state; the manual camera path matches 330 original updates bit for bit. Verified
cold loading now prepares the complete Raine Nurse casting and resident routes,
including their real body/scene resources and voice selection. Rendering and
device audio remain later integration work.
Lloyd’s motion bindings, normal hit windows, forward-speed command, sword sound
requests and recovery timing are maintained source. Arena constraints, root displacement, combos, guard
controls and automatic follow-ups, remaining actor reactions and complete hit-stop policies remain core work.
Full pose fidelity and model/controller dispatch ordering still need acceptance.
Cold preparation now binds Lloyd's Wooden Blades to their real attachment bones;
ordinary body matrices and submitted sword contacts match sampled Dolphin poses.
Hair/coat secondary motion now shares the field solver and retains history in the
battle model. Component state and driven matrices match original observations;
complete model initialization, controller history and drawn poses remain pending.
Stage 2's representative headless core is complete: Lloyd normal attacks, Genis
Lightning and Raine Nurse prepare and execute through the same owner and VM, with
current original-data and Dolphin component checks. Stage 3 field integration is
underway; complete encounter and played-route acceptance remain pending.
No complete arte or encounter has current Dolphin acceptance.
Fresh Stage 3 component captures cover Fire Ball's first three homing windows,
Zombie animation-stream clocks, and Colette's ordinary returning weapons. The
Fire Ball diagnostic replay matches 282,400 watched values; the clock replay
matches all 616 fields over 903 shared presentations. Their corresponding motion
and clock regressions execute in the core suite. These observations establish
component timing, not complete encounter or rendered fidelity. The two aerial
weapon captures match their ordinary replays across 755 and 762 presentations;
a further catch-point probe matches 759 presentations, and a later launch probe
matches 800 presentations. A subsequent pair of hurt/death probes each matches
its ordinary replay across 800 presentations and observes six interrupted
outbound visits, including the distinct death catch operand. Those semantics now
permit complete preparation for diagnostic encounter, audio and native replays.
Returns whose SDK hand distance is below 0.1 now accept the original facing
operand for verified Auto nonleader failed-facing and active local-hit-stop
callbacks. The value lasts one visit, uses the adjusted desired angle, and retains
the source's strict length gate and normalization count. Verified advancing,
non-completing opening martial and established normal callbacks retain the prior
flight direction; the same sequence's command-clock advance distinguishes them
from held visits. Original normal animation rows disable the root-Y transfer
writer, proving the advancing zero input independently of prior pose history.
Normal entry, landing and completion remain outside that admission.
Retained-death callbacks now keep the prior direction for proved sampled-heading
branches after a fresh body-pair evaluation. Admission uses the pre-callback
Dead state, retained-body binding and Auto nonleader selection, independently of
controller age, local hit-stop, landing and rest-motion blends. Direct non-tiny
trig arguments remain unproved. Other tiny-return and
unproved outbound callbacks still fail before changing the flight. Terminal
cleanup prevents continuation or outcome commitment after a fault. Remaining
tiny-return cases are required work; this is not full
weapon or Stage 3 acceptance. The latest game check passes 81 unit tests,
five authored-preparation tests and all 115 ordinary/cold battle-preparation tests.
The battle core passes all 433 tests, including fault isolation, the bounded tiny-return cases,
atomic rejection of unproved callbacks, the shared normal/martial facing gate and sampled body shake using
the original shared portrait countdown. The current presentation suite
passes 136 unit tests; four fatal/audio-owner checks also pass against
the cooked library (successful Load, failed Load/cancel, Quit and early audio failure). These exercise
actual owners with a controlled all-KO start and CPU placeholder images, not a
natural defeat or GPU fidelity. The current event
checkpoint passes 25 unit tests.
The importer passes 218 ordinary tests, plus selected original-data checks for
party profiles, opening enemy actions, enemy projectile banks, HUD, victory and
game-over resources.
The current HUD increment adds original enemy-group names, entry labels, orders,
radar, action notices and combo displays. Required enemy names retain the original
encoded-byte length for hidden-name placeholders. The Art 3 and enemy metadata,
plus unchanged authored source files, were published with the development partial
cooker; 501 dependency inventories were refreshed and verified. Core notice/combo
cues and display holds preserve their source ordering. Both-disc original HUD and
all 251 enemy-name checks pass. Strict native capture 10 completes 2,000 updates
with 85 captures, no diagnostics and no late reads. All 48 registered clocks
match; four sampled state comparisons and three full-frame pixel comparisons
pass independently. No combined state/image pair passes yet. At C5, the changed
pixel fraction falls from 0.071171875 to 0.013684896, with the same channel
tolerance 8 and maximum changed fraction 0.01. Radar, combo, later RNG/state and
mixed-audio differences remain open. The HUD build also completes the 12,000-update
rendered route again: requests 6 and 24 each commit Victory once, story reaches
3000, and the final checkpoint and all 161 sampled persistent party states are
identical to the preceding successful route. This extended native regression is
not a paired whole-route original comparison. Current receipts are
`stage3-opening-comparison-05/native-10/findings.json` and
`stage3-opening-route-native-03-result.json` under `local/battle-rewrite/`.
The following fidelity increment restores the two actor-common RNG draws on each
active casting-jitter visit and feeds their signed offsets to secondary motion.
Source watch 04 directly verifies those offsets and preserves all 1,144 preceding
watch fields over 2,000 VIs. It also proves ordinary entry playback uses rate 0.5
and performs one initial model update; both defects are corrected while retaining
one secondary-motion visit. Guard recovery now uses original party strategy and
enemy preference data. Camera target-highlight rates, prior-draw target-marker
positions and nearest radar sampling follow the checked source paths. Targeted
enemy/script publication and all 501 refreshed inventories verify; core, cold
preparation and the two-combat lifecycle tests pass. Strict native capture 11
completes 2,000 updates with 85 captures, no diagnostics and no late reads. Eleven
of 48 registered state/image pairs now pass together (combat ticks 5 through 55);
twelve images pass independently, including tick 60. The changed pixel fraction
at tick 5 is 0.0033203125 under the unchanged channel-8 / 0.01 gate. Detailed
capture 11b places the first state difference at tick 60 (an 18-degree Colette
heading gap) and the first RNG difference at tick 110. Body clocks now match;
remaining early coat-matrix differences trace to initialization placement. The
fixed-origin mixed-audio comparison still fails; source application-to-PCM timing
remains unresolved. Full rendered route 04 also completes both victories exactly
once and returns free field control at story 3000, with no diagnostics or late
reads. These are current native regressions, not complete paired-route acceptance.
Current results are in `stage3-opening-comparison-05/native-11/findings.json`,
`stage3-native11b-rng-state-audit.json` and
`stage3-opening-route-native-04-result.json`. Source evidence is in
`stage3-jitter-audit.md`, `stage3-body-clock-source-04.md` and the corresponding
camera, marker, guard and HUD receipts under `local/battle-rewrite/`.
The next core correction retains the sampled target direction and turns an actor
immediately on arrival before its next attack callback. Entry models now compose
once at the original origin placement before the first ordinary visit relocates
them through shared secondary-motion recovery. All 408 core tests, 70 game tests,
115 cold preparation tests, five authored checks and the real two-combat lifecycle
pass; formatting and affected all-target Clippy checks also pass. No recook is
needed for these runtime-only changes. Strict dense native capture 12 completes
2,000 updates and 253 observations without diagnostics or late reads. All 21
registered state/image pairs through combat tick 105 now pass together. RNG is
exact on every captured tick 1 through 109; the first difference is the correctly
timed first contact at tick 110, where native deals 51 unguarded damage instead
of the original guarded 12. Early entry and sampled combat body matrices pass
the unchanged 0.001 tolerance. Mixed audio still fails its fixed-origin gate.
Full rendered route 05 completes both victories once and returns free control at
story 3000, with no diagnostics or late reads; resulting party HP/TP/experience
and gald match route 04, while combat statistics and elapsed field ticks differ.
Receipts are `stage3-integration-checkpoint-23.json`,
`stage3-opening-comparison-05/native-12/findings.json`,
`stage3-native12-rng-state-audit.json`, `stage3-body-matrix-native12-04.json` and
`stage3-opening-route-native-05-result.json` under `local/battle-rewrite/`.
The tick-110 contact audit proves enemy guard chance belongs to the selected
row before attack admission: the original chooser publishes row zero first,
then its selected row, and contact reads that row even during approach. The
native chooser now publishes the existing signed guard byte at those same points;
the later authored attack assignment remains unchanged. Both new source-vector
regressions pass, bringing the core suite to 410 tests. Partial script publication
updates the one changed authored source and all 501 dependent inventories; all
published hashes, sizes and unchanged authored bytes verify. Cold integration,
formatting and affected all-target Clippy checks pass. Strict dense native capture
13 completes 2,000 updates and 253 observations without diagnostics or late reads.
The tick-110 hit now matches the original guarded 12 damage. RNG matches every
captured tick 1 through 171; 34 sampled state comparisons pass through tick 170.
The same 21 images and combined pairs pass through tick 105 under unchanged gates.
All sampled actor roots/headings remain within their registered tolerance through
tick 240. Later weapon/HUD pixels and mixed audio remain open. Full rendered route
06 completes both victories once and returns free control at story 3000, with no
diagnostics or late reads; its corrected guarded hit changes Lloyd's final HP and
combat statistics. This is a native regression, not paired whole-route acceptance.
Receipts are `stage3-integration-checkpoint-24.json`,
`stage3-opening-comparison-05/native-13/findings.json`,
`stage3-native13-rng-state-audit.json` and
`stage3-opening-route-native-06-result.json` under `local/battle-rewrite/`.
The tick-172 RNG difference came from release jitter expiring early: original
spell release re-arms it for the descriptor recovery duration. Ordinary and
stored release now call the existing jitter operation before recovery in
`casting.sym`. Original weapon-flight pitch uses `acos`, and drawing retains the
weapon placement sampled before the actor callback while contacts use its current
placement. Notice atlases now use the original nearest filtering. All 412 core,
70 game, 115 cold preparation, five authored, one two-combat lifecycle and 123
presentation tests pass, along with formatting and affected all-target Clippy.
Partial scripts publication verifies 528 publications and 501 refreshed inventories.
Strict native capture 14 completes all 2,000 updates and 253 observations, with
no diagnostics or late reads. All 48 sampled states through tick 240 now pass.
RNG is exact on every tick 1 through 220 and the later 225/230/235/240 samples.
There are 27 combined state/image passes under unchanged gates: ticks 5 through
110, then 120 through 140. Tick 115 and later image differences remain open.
Strict rendered route 07 completes requests 6 and 24 with Victory once each,
returning free control at story 3000 without diagnostics or late reads. Its
changed combat statistics and rewards remain native regression evidence.
Receipts are `stage3-integration-checkpoint-25.json`,
`stage3-opening-comparison-05/native-14/findings.json`,
`stage3-native14-rng-state-audit.json` and
`stage3-opening-route-native-07-result.json` under `local/battle-rewrite/`.
The extended original route now completes both victories and reaches story 3000
with cleared control flags. Its first 2,000 VIs preserve all shared state, video
packets and PCM prefixes. The observed second seed is 41305. Native route 08 uses
that seed and completes 12,000 updates with 256 captures, no diagnostics and no
late reads. All 95 RNG samples from combat ticks 241 through 335 match; the first
state failure is Genis's heading at tick 298 under the unchanged 0.01 gate.
First-battle held buttons and press edges match at every original combat tick;
the later behavioral divergence lies between ticks 336 and 414. Second-battle
inputs differ by two combat ticks, so its outcome comparison remains confounded.
See `stage3-opening-route-pair-plan-01/observation-receipt.json`,
`stage3-opening-route-native-08-result.json` and
`stage3-opening-route-compare-08/receipt.json`.
Integration 26 now includes the source command owner, strip/cursor rendering,
voice-stream pause, authored jump/backstep callbacks and early audio ownership.
It also corrects ordinary casting portrait timing, retained contact/admission
flashes and suppression of KK/PA body attachment meshes while preserving their
children. Idle facing and in-range admission now read the original cached target
direction; paired observations prove the former live-position operand was one
visit early. The two-disc UI and tint checks pass. Selected UI, tint, script and
sound-137 publication refreshes and verifies all 501 field inventories. The combined
suite passes 428 core, 80 game, 22 content and 133 presentation tests, plus all
115 cold preparation, five authored and one two-combat lifecycle checks. Four
fatal/audio-owner tests and the audio-before-model-failure test pass with real
assets. Formatting and affected all-target Clippy pass. Fresh native replay 15
completes 2,500 updates and 233 captures in paranoid mode without diagnostics or
late reads. All 231 original/native clock registrations match. The early 48 samples
all pass state comparison, with 35 passing images under the unchanged gate.
Dense later observations place the first state drift at combat tick 340 (Zombie X)
and the first RNG drift at tick 342 (one native draw behind). Entry P3 has a missing
white wash; P28 and P53 pass. Across all 231 paired samples, 150 pass state,
126 pass images and 125 pass both. The original marker probe proves current-body
follow on all 245 visits; native output follows the preceding body pose. These
findings are recorded in `stage3-native-opening-result-15.json` and
`stage3-opening-marker-watch-plan-01/source-follow-verification.json`.
Paired rendered acceptance remains open. Applied source
patches and publication receipts are recorded in `stage3-integration26-applied.json`
and `stage3-integration26-publications.json`.
The command pilot completes 2,150 updates and 127 captures without diagnostics or
late reads. All 126 command-state comparisons pass; 125 ordinary state checks and
112 images pass. The final Colette heading differs by 0.01140594 degrees against
the unchanged 0.01 gate. Animated icons obscure disabled-command crosses on some
held visits. See `stage3-command-owner-dtm-watch-plan-v4/native02-comparison-receipt-01.json`.
Integration 27 applies current-body marker following, guard release/countdown,
enemy normal braking and zero-chance follow-up RNG, selector model/camera/shading
order, the original entry fade, command-layer sort invalidation and critical-damage
actor labels. The combined unit suites pass with 433 core, 81 game, 22 content and
136 presentation tests. Selected profiles/scripts/UI publication, cold checks and
fresh native comparisons remain pending. Source patches and checks are recorded in
`stage3-integration27-applied.json`; no new rendered acceptance is claimed.
An instrumented source capture now pins the Music-85
API event at DSP writer frame 851800 without changing any observed state or PCM.
The callback/DMA trace matches all eight generated blocks to their predetermined
WAV ranges byte for byte. The fixed score-zero music comparison passes existing
thresholds; the original advances audio during synchronous module loading, leaving
different music age at the registered battle state. The fixed combat-origin mixed
audio gate still fails. The early audio preparation handoff now corrects
live ownership order; deterministic recorder loading time remains a separate clock
limitation. See `stage3-opening-comparison-05/music-dma-analysis-01.json`.
The opening escape audit finds formations 1 and 2 both use flags 5 and event
override 0, disabling ordinary escape without selecting forced escape. Their
all-party defeat path remains reachable and is covered separately by Game Over
tests. The command owner now connects logical menu action 3 to source pause,
navigation, cancel and disabled-Escape feedback; native pilot validation remains
pending. Grounded attack, arte, guard, movement and target-selection paths exist.
The guard-plus-up native pilot matches all 111 original samples exactly for
activity, position, heading, RNG and guard chance. The guard-plus-away pilot also
matches position and heading throughout, but differs on guard release at combat
tick 130 and then consumes an extra RNG draw. Both strict pilots complete 2,000
updates and 253 captures without diagnostics or late reads. Receipts are in
`stage3-opening-mobility-plan-v2/guard-up-native-01-receipt.json` and
`stage3-opening-mobility-plan-v2/guard-away-native-01-receipt.json`; image and mixed
audio comparisons remain separate work.
Override 0
does not select the later dedicated tutorial masks. The source receipts are
`local/battle-rewrite/stage3-escape-scope-audit-01.json` and
`local/battle-rewrite/stage3-tutorial-control-scope-audit-01.json`.
The updated common battle assets, party/enemy models and Nurse scene pass partial
publication. The latest cold batch and focused rerun pass all 31 component checks, including Ghost,
Zombie, Fire Ball, guard-break, opening martials, Lloyd profiles/poses/normals,
party projection, Nurse and every victory selector. The Lightning source fixture
now includes its original kind-5 particle motion mode, verified on both discs.
The task-recovery RNG regression is fixed without changing the original
observed expectations. The real field-event handoff, all 25 classroom checks and
seven title/setup/shop/EX/Unison regressions also pass with the refreshed library;
the handoff test supplies result callbacks and does not establish actual combat.
The real opening binding inventory selects 72 sounds, 48 streams and music
85/95/96. Both-disc partial audio publication passes with 236 publications and
501 verified refreshed inventories; authored archive casing now resolves to the
actual extracted path when hashing source inputs. Both real opening candidates
pass cold preparation and deterministic 240-update replays, and their complete
audio binding closure passes. The real suspended field event now runs both
headless combats, commits each victory once, rejects stale/duplicate callbacks,
and resumes free control at story progress 3000. The headless service acknowledges
presentation/audio requests; this does not establish rendered or audio fidelity.
Result construction now retires mutable combat controls while input validation
retains their immutable prepared bindings. Live rendered activation and the
played-route comparisons remain pending.
The first native run on the system GPU reached battle preparation, then stopped
because a common particle requested a 32-entry palette offset absent from the
published 256-entry pages. It produced no active battle images. Its eleven field
samples match the original player and camera transforms, but field animation and
prompt ages are not registered. Their image differences are diagnostics; the 48
registered combat-frame comparisons still await successful rendered activation.
Targeted effect/enemy publication now emits the source's overlapping palette
windows and records their spacing separately from the native format stride. All
501 dependency inventories were refreshed and checked. Importer pixel tests and
presentation lookup tests pass; the GPU replay must still validate activation.
The next native replay passed the palette gate and stopped at common particle 19:
its orbit billboard needed the original degree lookup table in draw preparation.
It again produced no active battle frames. Preparation now supplies all 450
original samples, including the distinct cosine extension beyond degree 359.
Both-disc original-byte and source-derived geometry tests pass, together with
targeted UI publication and all 501 refreshed inventories. An offline audit
of the actual prepared encounters finds no remaining particle-declaration or
ribbon admission restrictions; it does not replace GPU activation or fidelity checks.
Replay 04 passed the orbit gate, then exposed common particle 30's projected-scene
texture binding. Original slot 10 samples a captured scene with an atlas-0 alpha
mask; treating it as an ordinary two-image atlas incorrectly reports a missing
alpha image. The capture and drawing pass now compile and pass focused tests. All
35 selected textured declarations were audited; this is the only current binding
failure found by that audit. Replay 05 passed this preparation gate, then the GPU
rejected the retained-frame feedback composite: its fixed BGRA pipeline did not
match the final camera's RGBA sRGB target. The composite now prepares a pipeline
for each camera target format while retaining the fixed capture-image format.
Replay 06 passed this gate and produced all 48 registered combat-tick sidecars,
but their images still show the retained field: the camera handoff needs repair.
Their P/G/C clocks match the source registration; all state/pixel comparisons
fail, with RNG differing by combat tick 5 and actor movement by tick 15. A later
strict simulation stop exposes coincident approach endpoints. Tolerant replay 07
finishes the same 2,000 updates, records that error once and finalizes its WAV and
77 captures with `valid: false`. Neither run establishes battle image/audio or
played-route acceptance. Source analysis now defers coincident approach-side
validation until an eligible detour actually consumes it; the unknown value is
retained rather than replaced with an invented direction. All 390 core tests pass,
including later consumption and replacement of that unknown. The camera handoff
now preserves Bevy's computed target state, and empty HUD batches retain valid
GPU meshes while hiding their layers. Strict replay 08 now completes all 2,000
updates with 77 captures, no diagnostics and no late asset reads. Its battle
images show the arena and active combat. All compared state fields match at
combat ticks 5, 10, 15 and 20; later state and full-frame pixel differences remain
under investigation. This is successful GPU activation, not encounter acceptance.
The recorder now waits for battle GPU preparation before held readbacks, with
zero elapsed game time. This fixes a second-entry readiness race without relaxing
the screenshot state checks. Strict rendered route 02 completes 12,000 updates,
both victories once and free field return at story 3000, with 161 captures, no
diagnostics and no late reads. The final ordinary checkpoint records two battles
and two participations for each opening party member. This extended diagnostic
button schedule is a native route regression, not a paired whole-route movie.
Native 09 also completes the bounded opening comparison with eight extra RNG
observations; all prior battle state snapshots and the complete PCM are unchanged.
The original shadow UV scale and renderer winding are corrected, but all 48
full-frame pixel gates still fail and only combat ticks 5–20 pass state comparison.
The strict controlled defeat replay with Colette and Genis already at HP0 reaches
the implemented Game Over owner with no diagnostics or late reads. Its final
capture remains at the Game Over menu because the fixed Quit pulse precedes the
actual transition in this derived fixture. A separate preserved HP1 replay
finishes the ordinary battle-results path instead; neither controlled route is
claimed as a natural original encounter comparison.
Successor inputs after the observed ready Game Over screen now exercise all
three branches on the GPU. Quit and Load cancellation release the retained
caller and restore the title/audio owner; successful Load replaces it with the
prepared healthy field save. All three strict recordings complete without
diagnostics or late asset reads. Their source-relative phase/image audit remains
separate from whole-encounter fidelity: registered Game Over menu/fade frames
and Load pages meet the established pixel tolerance, and the restored party
matches all shared saved fields for all nine members. Title and field animation
clocks are not paired after those handoffs. The source controlled-Colette and
native Lloyd-only encounter fixtures differ.
The original entry screen break accounts for 62 missing random draws before
actor initialization. Its actual triangle records and dispatch timing are now
implemented, including the frozen field capture and overlay. `entry_voice.sym`
selects and queues the opening line before the first actor visit. Source branches,
RNG draws, pending-voice lifetime and legacy-history handling pass their tests;
fresh native fidelity is still pending. Entry voice selection also
depends on persistent previous-formation history. Direct reads of the pinned
opening, HP1 and loaded original savestates establish zero for those fixtures;
new successor inputs supplement only that observed datum. Legacy saves with
missing history remain unknown. Original fixtures and input schedules are retained.
The updated profiles, source scripts and audio pass partial publication with 501
refreshed dependency inventories. Real preparation selects exactly the maintained
73 sounds, 72 voice streams and music 85/95/96; all 114 cold preparation checks
and both real headless opening combats pass after this publication.
An instrumented Dolphin replay now registers written audio frames to each
observed presentation boundary. All 2,000 rows of 1,118 watched values, all 1,892
video packet records, the 48 decoded comparison frames and both final WAVs match
the retained original exactly. Source combat tick 5 maps to DSP frame 940480.
This establishes the source audio timing reference; the fixed-origin native
mixed-audio comparison still fails. The extended original watch records music 85
in slot 1 at VI1594 (DSP frame 852312), the first loader-counter increment at
VI1612, and screen-break P0 at VI1695 (DSP frame 906280). That counter changes
after substantial work inside the first REL callback; it does not mark callback
entry. Source instructions contain no authored 18-VI music delay. Presentation
now begins battle audio when the verified package is owned, before visual entry
readiness, and queues music once through that owner. This corrects ownership
order without claiming a PCM-origin fix: zero-time recorder preparation can
coalesce those commands, and mixed-audio fidelity remains open.
The grounded death pose now uses the original twelve-visit blend, while lethal
falling retains eight. Core checks verify the held animation clock, first advancing
sample and repeated-request behavior; all 366 core tests pass.
`casting.sym` owns countdown, early/fallback release poses, TP commitment,
recovery, periodic pulses and chant timing. Verified preparation now combines the
shared original technique catalogue with party profiles and resolves the chant
and release clips before activation. Genis uses the three original chant rows and
his actual 0.9 effect scale. Admission cost must agree with the selected technique.
Nurse's technique flags select stored-scene casting; preparation validates its
explicit scene binding against the original technique and required resources. Ordinary release now emits common burst 7/8, sound 123 and the spell
in source order, with prepared element tint. Casting modifiers, held release, general targeting and the complete pause policy
remain pending. Maintained source now schedules chant and release voices using
verified original durations and playback acknowledgments.
Original effect timelines and supported particle modifiers translate into that
same VM at loading. Script `show` now activates independent effect sequences and
common particles, including their real RNG consumption. Ordinary casting now uses
followed, scaled effects in the late group. Particle declarations retain their own
normal/late update group; late particles run after late effects, including those
emitted during that same group visit. Projectile birth and clash effects now
execute through the same effect VM; their real visual controllers, attachments and
rendering still need integration. Common particle UV frame/loop, palette-key and
scrolling updates now run in the same particle owner, with a distinct held clock.

Battle semantics cooking is part of the current work. The ordinary `cook-all`
pipeline now publishes the shared technique/learning/Unison catalogue, complete formation table, common action tables, all nine normal-attack groups, all eleven party profiles and the original chant rows, all nine standard party battle models, the weapon model bank, all 251 enemy body rigs/profiles, all 26
common projectile templates, common/technique effect source banks and element tint tables, and includes them in verified
field dependencies. Monster Book preparation shares its original common archive
bytes and publishes base/variant combat statistics and all nine affinities for
battle loading. Neutral affinity remains separate from the menu's eight elemental
weakness/resistance labels.
`battle::encounter::enemies` resolves every declared enemy resource, reserve,
initial actor variant, appearance and attachment selection without mutating the
session. The separate encounter preparer now assembles the opening roster,
source placement, controllers and immutable presentation dependencies. Both opening
candidates pass cold preparation and deterministic execution; actual GPU activation
and played-route fidelity remain pending. Non-Normal difficulty is explicitly
unadmitted.

The shared recoil table is now published by the normal battle table cooker.
Verified loading resolves its 19 impulses and actor weight/flag operands into the
core recoil operation. That operation covers immediate/delayed speed writes,
guard adjustment, launch/down overrides and pending-speed immunity. Contacts now select the recoil direction, assign ordinary hitstun/combo state,
interrupt actor tasks and request hurt/guard body motions. Ordinary hurt advances
its impulse delay and signed countdown; ordinary enemy/Auto guard advances its
countdown and braking. Both recover through shared motion/combo/guard reset writes.
Source admission accepts the implemented hitstun, delay and alternate-motion/lift
operands, stun chance, stagger and the down-hit flag; incomplete forced recoil,
conditions, impact presentation and EX routes
still fail preparation. Ordinary contact-tail RNG, reactions and model handoff execute for the selected
Stage 2 routes; remaining special reactions belong to shared mechanics.

## Source baseline

Implementation began on clean `main` at `9bfd330`. The source reference is
`/home/rowan.goemans/Documents/engineering/Tales-of-Symphonia-decomp`, revision
`384dd3889` (22 September 2026). Before the next subsystem, review relevant changes
since this revision. The active target is USA Rev 0 `US_r_Top2Btl`, module 1,
SHA-1 `99781a4ae9421e979202a05e98afa3b661a0da75`; field is `US_r_Top2field`, module 2,
SHA-1 `e3d8b6491f5bd4374ecb142a0c8af642d5e692a7`.

Authority is `config/GQSEAF/{US_r_Top2Btl,US_r_Top2field}/decomp_status.json`, plus
`config/GQSEAF/decomp_status.json` for DOL consumers. A source file alone is not a
proof: at this revision battle has 1,236 native/exact, 153 partial, 11 candidate
and eight inline-assembly/exact function entries. Native/exact functions below
are unlinked unless noted; their function proofs do not establish whole-TU matching.

| Source entry / consumer | Current evidence and implementation consequence |
| --- | --- |
| `fn_1_5878`, `fn_1_A0C`, `fn_1_45238`; `BTLusual.dat` member 1 | Native/exact selection and resource loading. All 1,000 96-byte formations are cooked, with original signed resource IDs, inactive slots, actor variants, appearances, attachments, coordinates and storage retained. Flag `0x01` restricts escape; other session gates still apply. |
| `fn_1_44388`, `fn_1_43AC0` | Enemy setup is partial (96.0577%); placement is candidate (0%). Original instructions at `44480..44694` confirm variant field copies and initial HP/TP zero defaults. Instructions at `43B04..43B18` and `43DF4..43ECC` establish automatic placement versus signed X/Z coordinates. Selection preserves these requirements; full initialization, difficulty scaling and placement execution are pending. |
| `fn_1_0` → `fn_1_6184` → phase table `lbl_1_data_20` | Module ownership and phase dispatch are native/exact. Do not replace all native phase gates with one global pause flag. Whole combat/model/contact dispatch still needs tracing. |
| `fn_1_649B8` → `fn_1_37ED0`, callback `fn_1_64950` → `fn_1_205AC` | All native/exact. Lightning captures its ground origin during initialization and emits bank mode 1, member 4 at age 20, using live heading. The source callback, deferred birth, unavailable-target replacement and Genis casting/contact/effect preparation execute in headless tests. Complete rendered route acceptance remains pending. |
| `fn_1_4DA50`, constants `rodata:1C80/2800` | Ground origin flattens target Y to zero, then nudges one unit toward the owner only at horizontal separation ≥0.5. Values verified in the current original REL. Headings are degrees. |
| `fn_1_12470` → `fn_1_11E8C`; `fn_1_1C40` → `fn_1_3A978` | Native/exact list allocation/group dispatch and late resident callbacks: projectiles prepend to group 3 and update newest first. Late spell emissions first initialize next update. This establishes the kernel's resident/projectile order, not full actor/model sampling. |
| `fn_1_14C24`, `fn_1_14064`, `fn_1_147E0`, `fn_1_14748` | Native/exact birth, motion, age and cleanup. Birth includes age-zero movement without contact; callback sees age before increment; the lifetime's last update submits contacts before marking retirement; next dispatch removes it. Standard ballistic/ground-clamp, clash disarming and ordinary contact-retirement subsets implemented. Debris responses and contact callbacks remain pending. |
| `fn_1_3D864`, `fn_1_3D6FC`, `fn_1_3BBD4` | Native/exact. Contacts retain submission order, at most 40 per side; party resolves before enemy. A strict 3D radius overlap consumes/disarms only the checking projectile's contact, emits common effect 11 at the midpoint, and latches feedback. The opposite record stays available. Shared projectile/melee submissions and clashes are implemented; actor reactions remain pending. |
| `fn_1_4DC18`, `fn_1_4DC44`; DOL `fn_800FE6F8` | REL wrappers native/exact; SDK vector length is linked inline-assembly/exact in `sdk/vec.c`. One hardware reciprocal-square-root refinement differs from host sqrt at collision boundaries. The kernel preserves operation order, estimate and observed NI flushing; 96 fresh original calls match bit-for-bit. This does not establish complete collision fidelity. |
| `fn_1_14CC8`, `fn_1_205AC`, `fn_1_60E60`, `fn_1_60E6C` | Native/exact at the current revision. All 26 common 400-byte projectile rows are cooked losslessly: allocation supplies owner/target/position/heading and resets instance storage; the selected allocator then binds the hit descriptor and overrides birth/trail resource slots. Preparation now reads verified rows and combines standard motion/contact parameters with caller bindings. Common action hit descriptors now load from verified source; specialized controllers and complete contact reactions remain pending. |
| `fn_1_13B8C`; `BTLusual.dat` member 7, row 4 | Initializer remains partial (87.58%). Lightning row is stationary, flags `0x44a`, lifetime 20, birth effect bank 1/member 28, and active-duration zero (unrestricted despite start 4). Parameters are pinned in `lightning-projectile.json`; jitter/aim/model/shadow initialization still needs instruction/oracle checks before broader support. |
| `fn_1_60694` → `fn_1_37B10`, callback `fn_1_60510` | Initializer and callback native/exact. Nurse resident lifetime is 250 (not casting descriptor 240); callback visits the live same-side roster at age 120, skips actor modes below 3, emits recipient program 6 then heals 40%, tint and non-caster voice 42. Model/recipient emissions, eligibility/recovery, healing tint and recipient voice requests execute through maintained source. Priority arbitration matches the original observations. |
| `fn_1_37E48` | Native/exact: callback observes the current age; completion comparison is inclusive, then age increments. Root VM return must not end the resident action. |
| `fn_1_3A89C`, `fn_1_3A978`, `fn_1_37ED0` | Native/exact. Released spells occupy two independent slots. Dispatch visits party before enemies and primary before secondary; initialization leaves age zero for the next callback. Ordinary slots survive caster recovery, interruption and death. Stored slots additionally own independent scene cleanup, validated by the Nurse lifecycle observations. |
| `fn_1_31C88`, `fn_1_39974`, `fn_1_3898C`, `fn_1_385A0`, `fn_1_1D864` | Native/exact. Casting callbacks have no attack-style blend/local-hit-stop gate. Initialization leaves the countdown intact; an occupied primary holds it. TP commits on entering release, before animation completion. The next release callback can create a resident and enter recovery. These decisions are maintained source; full casting admission, modifiers and presentation are pending. |
| `fn_1_420C4`, `fn_1_40210`, `fn_1_40188`, `fn_1_42494`, `fn_1_4273C` | Native/exact. Effect construction executes the first timeline visit immediately; ordinary later visits run newest first in group 4. Due main commands precede repeats, which retain insertion order. End returns before pending repeats, and payload timestamps are ignored. Load-time translation and effect activation share existing VM tasks; particles survive the timeline. |
| `fn_1_3F0C0`, `fn_1_403F4`, `fn_1_40E40`, `fn_1_41284`, `fn_1_41360`; `fn_1_418B4`, `fn_1_413B8` | Modifier execution and common particle update/initialization are native/exact. Emission is partial (92.3934%); allocation is candidate (0%). Original allocation instructions confirm 352-byte input copying and append allocation without initialization or RNG. Supported float modifiers consume signed battle RNG immediately; common controllers 4/5/7/11/15 initialize later in group 6. Common particle UV rows now load and execute with their own clock. Element tint and after-target draw bindings now prepare; other controllers, attachments and children remain pending. |
| `fn_1_11FCC`, `fn_1_123C8`, `fn_1_12470` | Native/exact shared 416-slot pool and append/prepend allocation. Particles append (oldest first); effects/projectiles prepend (newest first). Implemented object kinds share this capacity; unimplemented stage/UI/other occupants prevent full pool-pressure parity. Failed allocation skips modifiers and RNG. |
| `fn_1_27650`, `fn_1_2B910`, `fn_1_2BC3C`, REL data `11E0` | Early-pose helper partial 94.2623%, checked against original instructions; both row helpers native/exact. Auto/enemy casters can bind clip 12 before countdown completion. Genis's row clock holds on each transition. The script consumes verified original row parameters, published and bound during preparation. |
| `fn_1_1DE04` → `fn_1_1F814` → `fn_1_4DE40` | Native/exact. Recipient Lucky Healing draws once, then Healing boosts the percentage, then max-HP multiplication narrows to signed 16 bits. Weak caps healing without lowering existing above-cap HP. Historical recovery rejected signed narrowing; the new arithmetic retains it. |
| `fn_1_4DE40`, `fn_1_4DE7C` | Native/exact, one battle LCG with multiplier `0x41c64e6d`, increment `0x12d687`; unsigned/signed upper-half consumers differ. Unsigned recovery/critical/resistance draws and signed damage variation share the same stream. |
| `fn_1_24314`, `fn_1_244D0`, `fn_1_24040` | Native/exact integration and braking; fresh original constants establish ±0.55 thresholds, strict floor checks and coefficient order. Movement and braking arithmetic are implemented; root displacement, arena and complete controller selection remain pending. |
| `fn_1_3DA00`, `fn_1_295B8`, `fn_1_301A4` | Normal callback/recovery entry native/exact; recovery callback partial 99.835%. Original instructions confirm countdown-before-movement, the separate hover-height exit and no local-hit-stop gate. Core recovery timing is implemented; idle/AI transitions, animation selection and special recovery routes remain pending. |
| `fn_1_2C8B0`, `fn_1_27744`, actor transitions | `2C8B0` is the command dispatcher, partial at 24.2967%; command widths are checked against original instructions and the relocated jump table. `27744` is a native/exact actor transition despite its translation-unit label `battle_actor_commands_27744`. |
| `battle_unison_1639C` | `fn_1_1639C` and `fn_1_16578` native/exact setup/reset. Reassess historical staging, actor suspension, RNG and ownership assumptions before porting Unison. |
| `battle_stage_45448`, `battle_spline_4CB18` | `fn_1_45448`, `fn_1_4CB18`, `fn_1_4D098` native/exact. Stage color requests and indexed knot accumulation are recovered; old unknown classifications are obsolete. Stage color state now matches the Nurse capture; stage resources, drawing, remaining camera modes and spline execution remain pending. |
| `battle_texture_49DAC`, resource completion/tagged lookup | `fn_1_49DAC` native/exact; current allocation/loading sources inform preparation ownership. Existing generic asset cooking stays authoritative. |
| `fn_1_3BDF8`, instructions `3BFD8..3C1D4`; `fn_1_4DBA0` | Dispatcher C is only 3.6227% recovered. Original REL instructions establish box, cylinder, ground circle, ring and sphere tests; these now run in the core. Bounds include scaled hurt radius using fused multiply-add. Ground circle checks actor-root Y ≤0.1; ring radial bounds ignore attacker scale and accept negative width. The planar-distance helper is native/exact. Fresh branch tracing verifies box cases; other shapes retain source/test evidence only. |
| `fn_1_3BDF8`, instructions `3BE60..3BF94`, `3C23C`, `3CE98..3CEB0`, `3D690` | Ordinary projectile admission checks target availability and its per-target cache; the first admitted actor is latched before damage and ends this contact's search. The shared actor latch resets once per contact phase. Implemented for live, non-petrified actors; further actor modes, special descriptor categories and melee banks remain pending. |
| `fn_1_14064` cooldown tail | Native/exact: nonzero cooldowns decay after submission, before resolution. At one, increment the byte repeat counter; a nonzero limit gates decay, including the original counter wrap. Zero cooldown bypasses repeat counting. Implemented with focused tests; natural repeat-contact Dolphin comparison remains pending. |
| `fn_1_61578` → `fn_1_1D864`; wrappers `fn_1_63470`, `fn_1_6124C` | Damage resolver remains partial (57.0096%); HP application and reaction wrappers are native/exact. Fresh original disassembly establishes ordinary slash/thrust and magic arithmetic, critical order, affinity and signed HP arguments. Thirty-two admitted contacts, including repeated Fire Ball hits, match Dolphin damage, guard, TP and local pause observations. Ordinary guard transitions, stagger and recoil execute; EX/equipment/status modifiers and special reactions remain shared-mechanics work. Original `61E3C` adds half of the already-truncated physical amount; the partial C expression is not authoritative. |
| `fn_1_6124C`, `fn_1_63470`, `fn_1_2FB24`, `fn_1_2B18C`, `fn_1_1A9AC` | Native/exact. Recoil impulses/scalars and default guard preferences are cooked and verified on loading. Speed initialization matches eight natural contacts; ordinary hurt updates match 160 visits including four recoveries, with exact motion, countdown, combo, auto-guard and RNG values. Ordinary contact admission, hitstun assignment and model selection execute; special transitions remain shared-mechanics work. |
| `fn_1_1E12C`, `fn_1_21B18` | Corrected classifications: the first admits actions; the second enqueues floating battle numbers. Neither computes/applies HP damage. Original resolver calls `fn_1_1D864` to change HP. |

## One coverage backlog

Historical references are pinned, never merged wholesale:

- `9569cb6ebed185341f35fd9b007df9883271aebe` (`rowan/milestone4+cooking`):
  `docs/milestone-4-remaining.md` retains the full identity appendix and P/E/C/A/F/I/R/V
  case labels; battle tests and oracle fixtures remain available with `git show`.
- `0ba089696d05619ea71734c911d68ea3e2b03398` (`rowan/battle-cooking-wip`):
  original-table importers and `.sym` sequences are references. Their API/header,
  timing and lifecycle assumptions require adaptation to the current compiler.

Every historical subject remains pending in this rewrite, including subjects that
passed on an old branch. The following destinations retain all identified backlog
groups; source reachability must be reassessed before any exclusion. The historical
3,267-subject / 18,349-case counts are discovery lower bounds, not current coverage.

| Destination | Scope and retained backlog identities | Current status |
| --- | --- | --- |
| Stage 2: small core | Model sampling/order, melee commands, Lightning projectile 4, Nurse 120/250, RNG, damage, collision, repeated contacts, costs, interruption, separate clocks and surviving effects | Complete for the three representative headless routes; cold preparation, persistent simulation tests and pinned Dolphin component observations pass. Full natural encounter and rendered fidelity belong to later stages. |
| Stage 3: opening route | Field request/suspension, tutorial scripts, controls, companions/enemies, entrance, HUD, camera, audio, victory/rewards, defeat/game-over, escape and exactly-once return (I1–I5) | In progress; handoff unit tests and arena/profile/HUD original-data checks pass. Complete encounter preparation, playback and real-route Dolphin acceptance remain pending. |
| Current stage: battle cooking and preparation | Common/actor/action/formation tables, resource and model bindings, effect and audio dependencies, verified encounter loading | Formations, shared full enemy statistics, common action/projectile tables and common/technique effect sources cooked; requested effect members compile from verified files and enemy requirements resolve against the Monster Book. Remaining semantics stay in scope. Formation 90 references absent Sword Dancer variant 3; original bytes after the two declared variants are model data. Reachability and original behavior for this declaration remain unresolved; loading rejects it. |
| Stage 4: shared combat | All nine characters, local slots/control modes, targeting/menu transactions, directional combos, guard, aerial combat, recoil, casting/items, conditions, equipment/EX/title effects, Over Limit, tactics, difficulty (C1–C6) | Ordinary casting timing subset only; shared-mechanics acceptance missing |
| Stage 5: martial routes | Lloyd P1; Colette P2; Sheena physical/elemental seals P8–P9; Presea P11; Regal P12; all existing variants/use-count/hidden/aerial/followup branches P18 | Missing |
| Stage 5: magic/recovery | Colette P3; Raine cures/buffs/recovery P4–P6; Zelos/Kratos P7; Regal P13; Genis P14; ordinary Raine Photon/Ray/Holy Lance P17; all caster/enemy/retained/elemental branches P19 | Missing; Nurse callback is not ordinary Raine acceptance |
| Stage 5: summons/special ownership | Corrine/Heart/Purgatory Seal P10, all ten summon conditions and interruptions P20; Inspect Eye/Time Stop/Fairy Circle/Holy Binds P15; disabled/conditional learning rows P16 | Missing; no exclusion inferred from menu rows |
| Stage 5: Unison | Stardust Rain, Mjollnir, Prism Stars; remaining combinations/opener routes, recipe alternatives, party order, use-count changes and selection (U1–U5) | Missing |
| Stage 5: enemies/bosses | Policies 1–4, 6–10, 12, 14–19, 21–24, 26; Kilia model/action transitions; stationary/retreating approach; direct/stored spell variants; carried callbacks; natural Efreet grab/throw; Rodyle followups/phases (E1–E10) | Missing |
| Stages 4–5: models/effects | Animation/held poses, masks/layers, body/costume/weapons, detached rebinding, signed modifiers, capture anchors (A1–A5); palettes/blends/UV, owner/group/ground attachment, child birth order, pause, procedural geometry, screen texture, badges and complete frozen dependency closure (F1–F9) | Original timelines, independent effect activation, common particles and float modifier subset; full pool occupancy, attachments, other controllers and presentation missing |
| Stages 5–6: unresolved boundaries | Hit-rule `0x0400`, equipment passives, non-bone capture, simultaneous entrances/Unison/stored scenes, Pow Blade interruption, victory loops, reachable audio callbacks/key-off/stream traps and music loop boundaries (R1–R6) | Recheck current consumers before implementation |
| Stage 6: acceptance | Source inventory/branches, prepared/executed/image/audio/oracle evidence, all hit/miss/guard/expiry/clash/heal/revive conditions, opening discrepancies, full route regression and separate performance measurements (V1–V7); complete shared cooking (G4–G5) | Pending |

## Runtime and verification contract

`resonance-battle` has no Bevy, filesystem, compiler or persistent party dependency.
`resonance-game::battle::prepare` consumes verified `prepared::Files`, the existing process-local
`PreparationCache`, action bindings and a prepared-resource service. It returns a
complete `PreparedBattle` only after validation; callers retain the old field or
battle on any error. Encounter assembly and retained-field handoff are implemented;
complete played-route validation remains pending.
Sources are read through `prepared::Files::script_sources`, never reopened on disk.
The maintained Nurse and Lightning callbacks are embedded/published unchanged with the other scripts;
there is no serialized executable or persistent compiler cache.

The existing VM executes battle tasks. Shared `symphonia_script_vm::Tasks` owns
parentage, joins and retained results; each host keeps its scheduler. Battle children
run in increasing handle order, including newly spawned children in the same update;
a parent awaiting them resumes at its next eligible update. Parent return cancels
unfinished children. Returning a task does not expire its action. Interruption drops
all descendant VMs; script faults terminate the battle without delivering an outcome.
Immediate-ready waits continue synchronously; `next_update` always suspends.
Menu pause and normal-attack local hit-stop are implemented; complete model,
summon/Unison/time-stop gates remain pending.

Admission checks TP but does not debit it. The authored actor commits payment at
its original boundary with `pay_tp`; insufficient live TP returns false without
mutation, and duplicate commitment is an error. `battle::Spell` bindings must name
a zero-cost resident definition in the same fully prepared generation. A release
claims the selected slot independently of its caster's task ownership; occupied
slots return false. This prevents released spells from keeping an actor busy or
being cancelled when it recovers. Ordinary death cancels actor tasks only.

Actor-phase tasks run before the projectile group and contacts; resident tasks run
after contacts. Actor-task projectile emissions initialize in that same update;
resident emissions initialize on the next update. The projectile group runs before
resident tasks. Residents run party before enemy, primary before secondary. Their
initialization visit keeps age zero; active age zero follows on the next visit.
Emissions retain their verified
definition and sampled transform independently of the emitting action. Interruption
before emission cancels it; an already-emitted projectile survives task/action return
or interruption. Faults and terminal outcomes clear all transient state. Birth cues
capture the pre-movement transform, while the frame contains the resulting pose.
Script `show` executes the prepared effect's constructor immediately. Ordinary effects update
newest first after projectiles, then particles update oldest first, followed by late
effects before contacts. Late emissions initialize on the next particle visit.
Resident-created particles first initialize next update. Effects and particles
survive the caster; cancelling an effect stops its tasks without removing already
emitted particles. Particle frames expose simulated motion, geometry and colors.
Projectile birth constructs its effect before age-zero motion, with the owner as
both actor arguments and a followed projectile position. Later visits sample that
position; retirement retains its last position without following a reused handle.
Clash constructs common member 11 at the midpoint inside ordered contact resolution,
before later contacts consume RNG. Its particles first update on the next particle
pass. Preparation requires the selected source member for both call sites.
Original Lightning-28 and clash-11 controllers, attachment lifetimes and the full
pause policy remain pending; synthetic common-particle scheduling checks do not
establish these complete visual routes.

The contact phase runs between projectile dispatch and resident tasks. It samples
world contact offsets after motion and observes radius growth before resolution.
Clashes disarm future submissions without stopping motion; surviving projectiles
remain alive to their normal expiry. Ordinary non-survivors mark retirement on the
next active update, then disappear on the following dispatch. Both the lifetime's
last update and ordinary contact retirement can still submit contacts. Preparation
validates contact parameters; menu pause freezes the whole phase. Bounce/debris
responses, guard controller transitions and actor reactions are pending.

Actors now own an evaluated `Body`: model scale and world-space hurt points with
unscaled radii. Preparation validates this pose; an empty body has no hurt points. It also owns evaluated attachment anchors used
by melee contacts.
Simulation-owned model sampling updates this same pose before actor dispatch.
Active projectile contacts check geometry after clash resolution, using current
radius/height growth and body points. Each contact resolves its first eligible
opponent/point in roster order. All contacts share one per-update actor latch;
projectiles keep their own per-target cooldown and byte repeat counters. A hit
latches retirement feedback even when an affinity nullifies or absorbs damage.
The former `BattleFrame::overlaps` observation path has been removed. Ordered
`Cue::Hit` values now report computed amounts and actual HP changes from the core.
Menu pause freezes cooldowns and RNG as well as motion/tasks.

Prepared contacts bind a `HitRule` with slash/thrust/magic kind, normal/percentage/
fixed power, resolved element, nonlethal policy and guard/contact-pressure rules. Actors own combat stats and
neutral/elemental affinities. Emission retains the physical combo percentage;
magic scales intelligence before randomized resistance and ignores that percentage.
Ordinary physical hits consume one signed variation draw and two unsigned critical
rolls. Fixed physical power still consumes these rolls but clears the critical.
Affinity modifies the computed amount before the signed-16-bit HP argument; Weak
HP caps preserve existing above-cap HP. Dead actors lose actor tasks before
resident dispatch; released resident sequences survive while battle continues.
The existing terminal outcome is delivered once.

Actors also own their control mode, current activity and guard state. Physical
contact can select automatic guard after the critical rolls, using the enemy
action chance/window or party chance. Eligible zero/negative chances still consume
the draw. Ordinary guard accumulates five-bit pressure, checks the incoming vector
against facing, then reduces damage or breaks; projectile velocity is not normalized
and includes this update’s acceleration. Special guard reduces to 20 percent.
Magic consumes ordinary guard by halving the pre-variation amount. Block/break cues
replace the critical indication without discarding its damage multiplier.

These are on-contact calculations, not complete guard controls or controllers.
Activity/action-clock inputs are distinct from resident sequence ages. Attack age,
casting countdown/release time and recovery now update their actor activity;
guard entry/exit and the rest of the controllers remain pending. Casting stores
one counter on the actor, read/written by source and observed by guard resolution.
Party casting excludes automatic guard; enemy casting uses the action-row window.
Control modes also select the ordinary early release-pose branch. Guard
deflection, natural back-attack boundaries, EX/Over Limit/held-actor/poise/Unison
gates, dynamic spell percentage, equipment/cooking/condition/race modifiers,
special damage modes, reactions and hit-stop remain absent. Original encounter
preparation must not activate routes that require them. Generic terminal outcomes still lack the real
victory/defeat presentation, rewards and field policy.

Focused tests cover the kernel's contracts. They do not establish complete native
scheduling, complete melee actions, full collision behavior, resource effect playback, full arte behavior
or field handoff.
Source identity, preparation, execution and fidelity are distinct acceptance columns;
none may be inferred from another. Use the existing [oracle tools](../tools/oracle/README.md),
with up to 20 isolated silent Dolphin sessions, to close the pending evidence.

## Current evidence

Before the projectile increment, the workspace suite passed 611 tests with 187 ignored.
The previous affected suite passed 196 tests with 34 ignored, including 18 battle tests
and five preparation tests; all seven Python oracle tests passed.
Workspace Clippy with warnings denied, Rust formatting, and battle script checking/
formatting pass. The publication test covers the added source through the existing
publication path. Full cooking and playable-route fidelity have not been rerun.

The contact increment rechecked the unchanged decompilation revision above. Its
affected Rust suite passes 78 tests with 32 asset-dependent tests ignored, including
29 battle tests and five battle-preparation tests. All seven Python oracle tests
pass. The new regression cases cover asymmetric/simultaneous clashes, side/list
order, the 40-contact limit, 3D offsets, radius growth, active windows, pause,
surviving objects, inclusive expiry, malformed input and SDK distance rounding.
Workspace Clippy with warnings denied, formatting and diff checks also pass for
this increment. Full cooking and playable-route regression remain pending.

The subsequent actor-geometry increment passes 86 affected Rust tests with 32
asset-dependent tests ignored, including 37 battle tests. All seven Python oracle
tests pass. Coverage adds five shape rules, scaled/inclusive extents, ring direction,
root-height checks, dynamic height growth, current body poses, clash precedence,
roster/point order, pause, and malformed pose/shape validation.
Workspace Clippy with warnings denied, formatting and diff checks pass. The final
fixture check also passes after recording its ordinary-replay comparison hashes.

The restored battle savestate reader discovers the active REL BSS and actor/vital
storage instead of interpreting suspended field banks. A fresh silent Dolphin 2606
opening-attack capture is under `local/battle-rewrite/oracle-opening-03`. Its 400 VI
observations span native combat ticks 258–657 and gameplay ticks 317–716. The 249
observed LCG changes are consistent with 1,250 draws; those counts are inferred
from the stream, not per-action consumption acceptance. The reusable
`crates/battle/tests/fixtures/opening-random.json` pins eight transitions, input,
savestate, emulator, disc and profile identities. The native test checks that exact
arithmetic. Nurse/Lightning/melee timing and image/audio/handoff comparisons remain
pending. The capture's finalized WAVs are recorded evidence, not an audio pass.

A second fresh, silent capture is `local/battle-rewrite/oracle-projectiles-01`.
The unchanged second-opening-battle movie/checkpoint produced 1,150 VI observations.
`opening-projectile.json` pins a moving standard projectile's 31 observations at
VI samples 514–544 (combat ticks 739–769). A native regression starts from the first
observed initialized state and checks each subsequent position bit-for-bit, stored
age and retirement mode. This validates that motion/lifetime slice, not initialization,
contacts, an entire arte, or Lightning timing. The circular-list watcher also sees
sentinels/repeated nodes; only the identified head trajectory enters this fixture.

A fresh ordinary replay, `local/battle-rewrite/oracle-clashes-02`, records 1,150 VI
samples with contact lists and feedback. `opening-contact.json` pins the ordinary
projectile contact at VI 41 (combat tick 266), its final moving/contact update at
VI 42, and removal at VI 43. The native test starts from the already-latched state
and matches final motion bit-for-bit and retirement. Actor-hit admission/damage
and the cause of contact are outside that test. No clash-enabled projectile occurs
in this recording: natural projectile-clash fidelity remains open. The earlier
`oracle-clashes-01` requested more VIs than the retained movie contains and timed
out; it is not accepted evidence. Inputs and thresholds were unchanged for the
bounded rerun.

The separate silent diagnostic `local/battle-rewrite/oracle-distance-01` uses the
existing read-only GDB tracer's new `--trace-distance` option. Its 96 distinct
original SDK inputs/results are pinned in `opening-distance.json` and match the
native length calculation bit-for-bit. The same-index comparison of 383 VI samples
and 73 shared battle fields against the ordinary replay has no differences;
there was no input retiming. The SDK uses an estimate/refinement and FPSCR NI
flushing; host `sqrt` would change some strict collision boundary decisions.
The hardware estimate response points are documented in Dolphin's
[FloatUtils](https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/Common/FloatUtils.cpp),
with multiplier rounding in its
[interpreter helpers](https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/Core/PowerPC/Interpreter/Interpreter_FPUtils.h).
These observations establish arithmetic and the stated contact-lifecycle slice,
not clash visuals, audio, melee or complete combat fidelity.

`local/battle-rewrite/oracle-geometry-01` records 768 original geometry branches
with the read-only `--trace-geometry` option, using the same opening checkpoint
and input movie. All are box tests: 26 overlaps and 742 misses. The native test
matches every decision in `opening-geometry.json`. The trace pauses at `3C068`,
after extent calculation, and observes the acceptance/rejection branch before
hit rules execute. Across 1,021 VI samples, 73 shared battle fields match the
ordinary replay at identical indices, without retiming. This is geometry evidence,
not melee, damage or image/audio acceptance; the other four shapes need Dolphin
cases. The historical disassembly input was verified byte-for-byte against the
current original REL and the geometry instructions were disassembled again with
the existing inspection tool. Function SHA-256:
`017faaa16af6e2975570b14d3fc9d2f37444ddb2b4b6274e3a30488db63f2b65`.

The damage/contact increment rechecked the unchanged decompilation revision and
current target manifests. `local/battle-rewrite/oracle-damage-01` records eight
original resolver calls through the read-only `--trace-damage` option. Rows 4, 5
and 7 are unguarded physical cases; `opening-damage.json` pins their actor stats,
power, amount, HP and three shared RNG draws. All three match the new core.
At that increment, other rows still needed automatic guard selection and were
investigation evidence. The subsequent guard capture below now validates them. The diagnostic
and ordinary replay match 85 shared fields over 511 identical VI indices. Recorded
audio was silent and has not been scored for fidelity. Original resolver bytes
were freshly disassembled and verified against the current REL, SHA-256:
`51c9c35ab83848b9259277ea6d8072703e77f307301d308a8f5fe9c4911cf185`.
Authored durations use the strongly typed `ticks(2)` constructor,
including constants and variables.
After that refactor, 215 affected Rust tests pass with 34 asset-dependent tests
ignored, including 49 battle tests. All seven Python oracle tests pass. Workspace
Clippy with warnings denied passes. Full cooking and playable route validation
remain pending.
Magic, criticals, affinities, signed narrowing, repeated contacts, menu pause,
multiple targets, retained combo power and death cancellation have focused source-
derived tests; the three-call fixture does not establish those whole routes.

The guard increment rechecked the unchanged decompilation revision and current
manifest classifications. `local/battle-rewrite/oracle-guard-01` extends the
read-only resolver trace with the selected action row, facing/incoming vectors and
post-call target state. `opening-guard.json` pins all eight calls, including three
blocks, an action outside its guard window, hurt actors, a critical reduced by
guard, and a zero-chance enemy still consuming the fourth draw. All eight match
native amounts, HP, guard result, pressure, active state and RNG. The diagnostic
matches 85 watched fields over 524 identical VI indices against the ordinary
replay, without retiming. This replaces the earlier three-call limit for that
physical subset; it does not establish actor transitions or full guard routes.

The current affected suite passes 137 Rust tests with 32 asset-dependent tests
ignored, including 61 battle tests. All seven Python oracle tests pass. Tests add
window boundaries, signed chance, distinct control gates, pressure wrap, force-break
precedence, strict directional checks, magic guarding, pause, and ordinary
projectile retirement after guarding. Workspace Clippy with warnings denied,
formatting and diff checks pass. Full cooking and playable-route acceptance remain
pending.

The melee increment uses current `2C5B4`, `31C88`, `3D864` and `3DA00`
(native/exact) and checks the original instructions for partial `2D564` (95.159%)
and common-timer `2503C` (81.8514%). `battle::hit_window` is a reusable VM wait;
Lloyd's window timing/order lives in `scripts/battle/normal_lloyd.sym`. It uses a
separate signed hit-stream clock, preserves inclusive intervals and the held clock
when advancing to the next row, and cancels with its task. Melee origins and
projectiles share the existing ordered forty-contact lists, actor latch, collision,
guard and damage resolver. Melee retains actor-wide struck/cooldown arrays; new
windows clear both. Submitted origins survive later controller/task changes.

`local/battle-rewrite/oracle-melee-01` records 384 original hit-stream calls.
The diagnostic matches 85 watched fields over 761 identical VI indices against
the ordinary replay. `opening-melee.json` pins the finisher's 41 updates and 28
contacts. The maintained script matches clock/cursor progression and all submitted
origins, supplied from Dolphin's sampled weapon poses. This is not skeletal
sampling, full wall-clock action timing, combo, hit-stop or animation acceptance.
Original `2D564` bytes were freshly disassembled from the verified REL, SHA-256:
`29d07012ac5e48874896d7c8a690d5b9262ecabf9c552ae598a23bbfc5515716`.

The melee batch passes 119 affected Rust tests (69 battle tests), with 32
asset-dependent tests ignored, plus the authored-source publication test. All seven
Python oracle tests, workspace Clippy with warnings denied, formatting, diff checks
and the maintained normal-attack script check/format check pass.

The model increment rechecked the unchanged source revision and original DOL
instructions. `8006EB68` / `8006EBD0` and battle `2BE24` are native/exact;
`8006D2E0` is a managed partial candidate (93.502570% in its manifest note), and
`2C05C` remains partial because its relocation proof is incomplete. Original DOL
`8006D2E0..8006E520` SHA-256 is
`b1605079472a149177163da0d53aab9f58b9270142573e7c0786ca1b888abc30`.
The new playback preserves strict endpoint comparisons, sticky completion,
stopped tracks, held blend samples and single-wrap/reset behavior. Unlike the
historical runtime, reverse wrapping tests zero and large forward overshoots do
not use modulo. The shared sparse-curve evaluator supplies body and rigid weapon
points. Affine keys keep their matrices; root translation and drawn poses remain
separate observations. Missing tracks do not enter the per-bone blend branch.

`local/battle-rewrite/oracle-motion-01` records 2,048 original model visits;
`opening-motion.json` pins 853 primary body-controller updates. All 853 native
clock results match. The diagnostic matches 85 watched fields over 705 identical
VI indices against the ordinary replay, without retiming. This is controller-clock
evidence, not skeletal/attachment, image/audio or complete action acceptance.
Model definitions, motion bindings and initial pose sampling now validate before
activation. The game loader still needs concrete encounter resource services.
Animated/paired attachments, model scripts, dynamics, bone masks/scale controllers,
full pause policies and the complete actor/model dispatch order remain pending.

| Subject | Source identity | Preparation | Execution | Fidelity evidence |
| --- | --- | --- | --- | --- |
| Authored task host | Current shared VM; new `script battle;` host | Verified snapshot, cold/cache/import changes, failed replacement and asset binding tested | Waits, child ownership, joins, cancellation, limits and diagnostics tested | Native actor/model dispatch ordering remains pending |
| Ordinary party casting parameters | DOL technique catalogue, party profiles, Genis motion rows; `39974`/`3898C`/`385A0` and `2BC3C`/`2B910` at the revision above | Verified catalogue/profile/model bindings and load-time typed script access; incorrect cost, missing clips and mismatched stored-scene bindings fail before activation | Maintained casting source runs against real Genis motion resources; admission and live TP debit use the selected technique | 320 original Fire Ball casting calls match clocks/TP/release, with 317 held motion samples matching bit-for-bit; Lightning's equivalent clock case uses its source cost of 9, without claiming full Lightning oracle acceptance |
| Ordinary release effects | Exact `385A0`, `3F0C0`, `40E40`; emission/allocation partials checked against instructions and Dolphin | Common programs 7/8 and original integer modifiers prepare in the shared VM; element palette/colour tables load from verified files | Source emits burst, sound 123 then spell; tint, alpha ramps, size/UV changes and after-target draw binding; particles survive their emitting task | 320 casting request observations and 104 complete updates of one natural common-7 burst match; common-8 variant remains source-derived; renderer/audio pending |
| Nurse stored transition / recovery | `3898C`, `3EB74` / `3E42C`, `60694` / `60510`, `37B10` / `37E48` / `37DD8`; partials cross-checked against instructions | Stored package, four models, clips, effects and action source published and verified; common-37 and scene members 1–6 prepare; cold full Raine casting uses the verified body/scene and voice resources | Maintained source owns casting/payment/poses/emissions/recovery; core owns transition pause, two scene slots, independent model instances, visibility and cleanup | Original lifecycle/owner-model clocks, all 16 transition particles (491 updates), and three emitted scene models (543 particle updates through expiry, 531 playback visits) match; recipient modifiers/motion additionally match 51 emissions and 2,091 visits; complete arte and image/audio acceptance pending |
| Particle origin attachments | Exact `403F4`, `1B3F0` / `31C88`; `413B8` candidate and `418B4` partial cross-checked at `41494..414B4` and `41BE0..41C0C` | Declaration flag `0x400` becomes a prepared origin attachment; profile center offsets load from verified party/enemy records | Each particle follows at its own update-group visit; root and sampled-center bindings stay distinct; effect cancellation leaves it alive, missing projectile handles retain the last origin | 1,320 sampled/held actor centers match the maintained Nurse regression; complete transition particle motion/origins match original visits; moving attachment and petrification tests pass; bone bindings remain pending |
| Lightning release / standard projectile | Current callback, dispatch, motion, spell member 16 and projectile-4 row above | Cold preparation combines maintained Genis casting/Lightning sources, actual models, original hit and effect 28, including sound 92; presentation IDs are test bindings | Source-derived integration covers 9 TP payment, before/after-release interruption, two recipient contacts, particle tails and the original 90-update resident lifetime; effect 28 uses the shared VM | Fresh original ballistic trajectory matches native positions bit-for-bit and terminal age; no Lightning route, ribbon rendering or sound playback acceptance |
| Projectile clashes / contact retirement | Exact current contact/list/dispatch sources above; SDK length uses an assembly fallback | Prepared contacts validate finite parameters and require the bound clash effect member; original common-11 controllers are still missing | Kernel ordering, asymmetric disarming, cue placement, growth, windows, capacity, survival, delayed removal and melee clashes tested; guard deflection missing | 96 SDK calls and final post-contact motion match bit-for-bit; natural clash, pixels and audio remain pending |
| Actor hurt geometry | Original instructions of the partial dispatcher; exact planar helper | Body pose, sparse model data and shape parameters validated; concrete encounter bindings remain missing | Five shape tests feed shared melee/projectile admission; body and rigid weapon points now follow core model samples; full actor controller/dispatcher remains pending | All 768 observed box decisions match; other shapes and full actor-hit fidelity remain pending |
| Battle RNG | `fn_1_4DE40`, revision above | Seed supplied to the kernel | Fresh native test matches pinned observed stream transitions | Stream arithmetic and 32 admitted physical/magic contacts, including guard and repeated Fire Ball hits; complete encounter RNG consumption remains pending |
| Opening encounters | Retained original input; current source investigation above | Missing encounter preparation | Missing playable integration | Fresh original capture available; no native battle comparison |
| Ordinary damage/contact consumption | Current original `3BDF8` / `61578` instructions, exact HP and cooldown consumers | Verified hit element, actor stats and retained projectile power; original encounter importer remains missing | Physical/magic arithmetic, affinities, critical order, HP narrowing, recipient/cooldown order, pause, death and terminal delivery tested; persistent headless tests retain repeat-contact state | Thirty-two original per-contact observations match damage, guard, TP and local pause state, including repeated Fire Ball hits. These observations supply each call's inputs; they do not establish complete persistent encounter replay |
| On-contact guard | Current original resolver `61E48..620E4`, `62608..62638`, `62AC8..62E38`; SDK dot `fn_800FE73C` | Guard state/chance validated; ordinary guard rule and incoming velocity bound to contacts | Enemy action-window/airborne gates, party/manual selection, pressure wrap/break, special guard, magic ordering and ordinary controller recovery tested; special gates pending | Eight original physical calls, three blocks, match amount/HP/pressure/active state and RNG; no full guard route or audiovisual acceptance |
| Script-owned melee streams | Current `2C5B4` / original `2D564` / `3BDF8` / common timer `2503C` | Typed melee resources compiled/bound on load; body/rigid weapon anchors follow core model samples | Serial clocks, inclusive windows, per-target rearm, multi-origin hits, task cancellation, trades and projectile clashes tested | 41 original finisher stream updates / 28 origins match. Cold Lloyd tests combine maintained commands, sampled body/weapon contacts, voice requests, damage/TP and recovery; full controls, combo and encounter timing remain pending |
| Body model playback | Original `8006D2E0` instructions; native `8006EB68` / `8006EBD0` / `2BE24`; current `2C05C` | Skeletons, sparse motions, initial poses and every declared motion binding validated before activation | Shared pose sampling, rigid anchors, blend-gated actor clocks, strict endpoints, completion waits, cancellation, held drawing poses and failed replacement tested | All 853 captured body-controller clock updates match; full pose, dispatch, image/audio and attachment-family comparisons remain pending |

The model batch passes 129 affected Rust tests (78 battle), with 32 asset-dependent
tests ignored. All seven Python oracle tests, workspace Clippy with warnings denied,
Rust formatting and the maintained normal-script check/format check pass. Full
cooking and playable route acceptance remain pending.


The movement increment rechecked the unchanged decompilation revision. Original
`2C8B0` instructions confirm the reusable velocity/acceleration/gravity commands;
`301A4` instructions distinguish hover height from the flying flag when deciding
whether recovery can end above ground. `24314`, `244D0` and `24040` are native/exact.
The original `24040..24808` byte-range SHA-256 is
`59bf86d1979977b66393fee02aded5fdd9d62e24228a9500a6da7fcff1919aa9`.
The script owns Lloyd's age-four minimum forward speed and normal/finisher recovery
intervals. The parent and spawned hit stream share action age while movement runs
after actor commands. Model blends hold commands, not movement. Recovery holds
that age and counts separately, including through model blends and local hit-stop;
menu pause holds both. Actor common timers follow that actor's callback, before
projectiles and contacts.

`local/battle-rewrite/oracle-movement-01` records 2,048 original integration/braking
calls across idle, approach, attack, recovery, hurt, reposition, guard and stop
controllers. `opening-movement.json` pins their semantic inputs/outputs; all 2,048
match native position, origin snapshot, velocity/acceleration and braking results
bit-for-bit. The diagnostic matches 85 watched fields over 702 identical VI indices
against the ordinary replay, without retiming. Root displacement is zero in this
capture. These helper comparisons do not establish whole controllers, arena
constraints, animation root movement, reactions or full action fidelity.

| Subject | Source identity | Preparation | Execution | Fidelity evidence |
| --- | --- | --- | --- | --- |
| Actor motion and normal recovery | Current exact integration/braking and normal callback; original partial command/recovery instructions checked | Finite coefficients, direction, profile flags and recovery duration validated; real actor/resource preparation missing | Movement runs during command waits/blends; separate recovery clock, strict floor, pause, local stop, children/cancellation and maintained neutral/finisher combinations tested | 2,048 original helper calls match bit-for-bit; full attack/recovery, arena, root motion and audiovisual comparisons pending |

The movement batch passes 141 affected Rust tests (90 battle), with 32
asset-dependent tests ignored, plus the authored-source publication test. All seven
Python oracle tests, workspace Clippy with warnings denied, Rust formatting, diff
checks and maintained normal-script check/format check pass. Full cooking and
playable-route acceptance remain pending.

The resident/casting increment rechecked the unchanged decompilation revision.
`opening-residents.json` pins 110 active-slot observations from 320 original
resident dispatches, including initialization and inclusive expiry. All ordinary
resident ages and slot transitions match; 703 same-index VI observations match
85 watched fields against the ordinary replay. The current source corrected two
historical assumptions: released callbacks do not pay caster TP, and residents
dispatch in side/slot order rather than creation order. Stored/summon cleanup and
Nurse's retained scene are not covered by this ordinary-slot evidence.

`casting.sym` maintains the ordinary casting route and Genis's chant transitions.
The actor owns the casting counter, including its release-time reuse; source
updates it and guard resolution reads it. Nonblocking motion binding lets this
counter advance during a cross-fade, while an explicitly awaited blend still holds
that task. Normal attacks retain their existing blend/local-hit-stop command gate.
An occupied primary slot holds the countdown, but automatic early pose binding
still runs. TP commits before waiting for release-animation completion. The
resident created afterward survives caster recovery/interruption.

`local/battle-rewrite/oracle-casting-01` records 320 casting calls for native 204
(Genis Fire Ball), spanning two complete casts and a third prefix. The maintained
script matches all recorded body-motion bindings and clock fields, casting
counter values, TP changes and primary release boundaries. Parameters are read
from the original actor/technique records and REL chant table in the pinned
fixture; the test supplies simple geometry, not original curves or rendering.
The trace agrees with 85 watched fields over 1,150 identical VI indices against
the ordinary replay, without retiming. It does not establish casting modifiers,
local-hit-stop observations, global pause branches, held/stored casts, targeting,
voices/effects, or full Fire Ball/Lightning/Nurse behavior. Forty-one observed
calls also consume RNG, principally on presentation work; that consumption is not
yet reproduced by these casting helpers. Source and focused
tests cover the implemented local-stop, manual, occupied-slot and looping branches.

The partial early-pose helper was checked against freshly decoded original
instructions. `2C05C` chooses the playback start as loop origin when the original
loop parameter is zero; the Dolphin comparison caught and corrected that detail.
Original byte-range SHA-256 identities:

- `27650..27744`: `db4fe741d9dad11b49a307af6b0ab4b17d4976bc2797b13ab32ffe2c251f9b1d`.
- `2B910..2BC3C`: `ad244bfd2c61173b66f5c32ba19d17570579ff913424746afcb52820eb08de71`.
- `385A0..3A1E8`: `8e51ffe4794ea4341f18301602d14463b403297261c101eb1e4041a21f438965`.

| Subject | Source identity | Preparation | Execution | Fidelity evidence |
| --- | --- | --- | --- | --- |
| Ordinary released slots | Current exact `3A89C`, `3A978`, `37ED0`, `37E48` | Same-generation spell bindings and zero resident costs validated atomically | Slot occupancy/order, initialization, surviving caster interruption/death and explicit caster TP payment tested | 110 ordinary slot visits match; retained/stored scenes pending |
| Ordinary casting and Genis chant | Exact casting/row callbacks; partial early-pose/binding instructions checked | Maintained source compiled with typed parameters and verified motions; real caster table/resource loader missing | Countdown, motion transitions, early/manual release, TP boundary, occupied primary, loop completion, recovery and interruption tested | 320 original calls match counter, body clocks, TP and release; full caster/audiovisual acceptance pending |

The current resident/casting batch passes 156 affected Rust tests (104 battle),
with 32 asset-dependent tests ignored, plus the unchanged-source publication test.
All seven Python oracle tests, workspace Clippy with warnings denied, Rust/script
formatting, all four maintained battle module checks and diff checks pass. Full
cooking, concrete encounter loading, retained Nurse scenes and the playable route
remain pending. Stage 2 is not complete.

The effect-timeline increment rechecked the same decompilation revision and target
manifest. `resonance-content::battle_effect::Record` retains each original six-byte
command, including ignored operands. The importer reads all 52 common and 138
technique timelines without changing their bytes. This is a reader for the original
input. This earlier reader-only checkpoint is superseded by the effect-source
publication and verified loading work recorded below; remaining battle semantics
cooking is current-stage work.

`resonance_game::battle::effect_timeline::prepare` binds emitting records to prepared
synchronous command functions and lowers timeline control into the existing VM.
Repeats use its existing owned tasks, with no second interpreter or scheduler.
Main commands execute first, then repeats in insertion order; an end command
cancels repeats before their next emission. The payload's age is ignored, zero
counts emit once, nonpositive intervals emit the remaining count in one visit,
and descending main timestamps retain source order. Executables remain in memory;
maintained native control flow still belongs in `.sym` source.

`local/battle-rewrite/oracle-effects-02` contains 320 fresh original timeline visits.
`crates/game/tests/fixtures/effect-timelines.json` pins eight complete programs
(66 visits): common 1, 3, 11 and 17, and technique 3, 7, 8 and 25. Their dispatch
order, repeat timing and termination match the translated VM programs, using test
witnesses for particle commands. In particular, common 3 dispatches actors 34 and
35 before repeating actor 36 at ages 0, 2, 4 and 6. Construction visits age 0 and
the same combat update's group dispatch visits age 1. The debugger capture matches
85 watched fields across 703 identical VI indices against the ordinary replay.
No inputs were retimed. Disc, emulator, profile, state, input, source-range and
observation identities are retained in the fixture.

| Subject | Source identity | Preparation | Execution | Fidelity evidence |
| --- | --- | --- | --- | --- |
| Original effect timeline control | Current exact `420C4`, constructor `40210`, ordinary callback `40188` and bank binding `4273C` | Original records retained; load-time VM translation validates end, repeat payloads and command function bindings | Shared task host verifies ordering, waits, cancellation, menu pause and signed repeat edge cases | Eight programs/66 visits match dispatch and termination |
| Effect activation and common particles | Exact modifier/common-update sources; candidate `413B8` checked against original instructions; partial `418B4` checked with command snapshots and scaling instructions | Dependency-closed original records, particle parameters and modifier words; verified immutable effect banks and particle bindings | Immediate constructor, ordinary/late groups 4/7, oldest-first particles, followed origins, scale, inclusive expiry, surviving effects/particles and shared capacity tested | Nine natural common-3 constructors/27 particles match bindings, signed random yaw, Y velocity and RNG; their casting call times also match; continuous motion and pixels pending |

`effect_program::prepare` translates supported original emissions and float
modifiers to the shared VM. It validates every command dependency before activation.
`EffectBank` holds immutable member definitions; bindings reference bank IDs, so
cross-references do not create recursive owned program graphs. `PreparedBattle`
validates bank references, phases, native signatures and particle parameters.
Executables remain in memory. Missing members, unsupported controllers and malformed
modifier streams fail preparation or the checked script call boundary.

Script `show` now creates a real independent effect. Allocation uses the original
416-object capacity across implemented actors, projectiles, effects and particles.
Failed particle allocation skips all modifiers, including random draws. Unimplemented
stage/UI/other occupants still prevent full pool-pressure parity. Modifier scratch
values are shared by an effect's root/repeat tasks. Float set/add/subtract/multiply/
divide and literal-divisor signed random operations cover common programs 3 and 5.
There are no dummy draws. Stale/foreign particle handles and recursive activation
fault with the usual battle cleanup.

Common size/quad particles integrate their motion, angular/orbit values, geometry
and signed color fades in Rust. Initialization performs age-zero motion; inclusive
expiry marks retirement, followed by removal on the next dispatch. Caster or effect
cancellation does not remove emitted particles. Menu pause freezes all these clocks.
Particle rotation still needs SDK arithmetic comparison; attachments, retained
particle modifiers, UV, element palette selection, other controllers and rendering
resources remain pending.

`local/battle-rewrite/oracle-particle-emissions-01` adds nested particle-allocation
snapshots to 320 original effect visits. The checked-in `particle-emissions.json`
fixture pins nine natural common-3 constructors and 27 creations from Genis's
casting pulses. It verifies actual particle members, signed random yaw, Y velocity
and the before/after RNG state. The debugger replay matches 85 watched fields over
703 identical VI indices against the ordinary replay, without retiming.
`casting-particle-sources.json` now retains common programs 3/5/7/8 and their original source records,
particle parameters and modifier words; an asset-dependent test compares them
against the extracted original bank. No compiled program appears in either input.

Original byte-range SHA-256 identities for the emission comparison:

- `3F0C0..3FB84`: `b7e718292f08832c3e5c713b1a0229100abba9508a5f0d9633dc43dd40067bcd`.
- `413B8..41504`: `c0c1074ab7ca98a6fa1db517bfdcf13750fb550ee6433cc29ad511b5c2843bb2`.
- `418B4..420C4`: `fada0d5d1b343b1b6216356950ccca82f86a48c25fd859f7a52ac966bea3d661`.

`casting.sym` now emits the ordinary eight-callback pulse using the late group (7),
as requested by `3898C`. `show_following` refreshes the origin before each timeline
visit; emitted particles keep their own origin. Scale applies after modifiers.
Original `41E1C..41E7C` instructions confirm the unusual quad rule: only its first
three vertices and orbit vector scale; the fourth vertex and velocities do not.
The nine recorded pulse call times (ticks 298 through 362) match the casting script.
An integration test runs the authored casting task with original common-3 particles
through release and interruption, checking every birth and all 12 random draws.
The 320-call casting clock comparison still passes with pulse emission enabled.

The later release increment below supplies element tint; additional caster
effects/voices and full casting RNG order remain pending. Projectile birth/clash calls likewise need immediate activation at their
original dispatch points. The implemented global menu pause still holds both
effect groups; original group-specific pause gates remain separate work.
Continuous particle-state, image and audio comparisons, complete cooking and the
playable route remain pending. Stage 2 is not complete.

That casting/effect increment passed 181 Rust tests: 172 battle/game tests, eight original
effect-import tests (including both local disc checks), and the unchanged-source
publication test. The game suite then skipped 32 unrelated asset-dependent tests.
Seven Python oracle tests, Python syntax checks, workspace Clippy with warnings
denied, Rust/script formatting, the casting module check and diff checks passed.
Logs are `local/battle-rewrite/casting-pulses-{regressions,tests,publication,check,clippy}.log`,
`particle-import-tests.log` and `particle-python-tests.log`. Complete battle-effect
cooking and playable battle-route regression remain pending.

### Formation and enemy cooking

The formation/enemy `cook-all` checkpoint completed both discs with **zero failures**.
All 1,000 formations round-trip against original bytes; all 251 enemy packages
retain base/variant combat statistics and nine affinities. The common archive's
SHA-256 is `daf67bba141841c6a317c4bad950dc7dcfc5d3a90cfc7659d76b37e277af5d50`.
Both discs' source inventories name the formation and enemy publications. All
501 generated field inventories include the formation file with its verified
digest, and all 2,181 maintained `.sym` files were published unchanged.

The existing Monster Book labels, statistics and model bindings compare exactly
with the pre-change publication for all 251 records. Neutral affinity is retained
for battle but does not become an extra elemental icon in the book. Original
Ghost variant 1 statistics and neutral resistance match the existing pinned
`opening-damage.json` Dolphin observation. This reuses that valid observation;
it does not claim fresh battle playback or image/audio acceptance.

Validation passed 197 importer tests, the original-disc enemy/variant check,
70 game tests, all 33 cooked-game tests, and the existing source-inventory audit.
The new cold-load test reads the verified classroom snapshot and resolves all
1,000 formations. It reports formation 90's absent Sword Dancer variant 3;
reachability and original behavior for that declaration remain unresolved.
Workspace Clippy with warnings denied, formatting and diff checks passed.

Logs and comparison reports are under `local/battle-rewrite/formation-*`, including
`formation-full-cook.log`, `formation-cook-verification.json`,
`formation-monster-comparison.json` and `formation-game-asset-tests.log`.
The cook still reports 30 mixed battle source containers with remaining semantics;
these remain in scope. Full encounter activation, actor initialization, effects,
audio and field handoff are pending.

### Common and technique effect source cooking

At decompilation revision `384dd3889598f7d7608f0f644b2ff13e283131cf`, `4273C`
remains native/exact and supplies the six effect source sections. `418B4` remains
partial (92.3934%); `413B8` remains candidate (0%). Their original record addressing
and the previously checked instructions remain the source-layout reference; this
increment does not add native controller behavior. Types 22 and 24 retain opaque
body storage: their consumed shake/color parameters are in the shared prefix.
They are not classified as unused or unreachable.

The regular source-table publication now includes both `ef1` banks in
`BTLusual.dat`: common (52 timelines, 87 declarations) and techniques (138 timelines,
142 declarations). All controller unions, inactive operands, signed selectors,
colors, model animation storage, caption storage, UV rows/roots and referenced
modifier words are retained. Finite numbers remain readable JSON; inactive
non-finite union words preserve their bits. Original control records remain source
input, without VM modules or generated authored behavior.

The two original bank digests are respectively
`2c72d54fcdf809b93f10d0c9213790710b4f0144a4f55253ebab415f4e69a48b` and
`2a0b8378be586933a7a3e4d59eda0e1a073c59b9cbc6f2169f300edebe09c059`.
Equal supplied common archives share publications within the cook invocation;
distinct archives retain their own variant publications.

The game preparation path loads requested effect members from the verified
snapshot and translates their original input into the shared VM. The presentation
resource service supplies the source selection and ready rendering generation ID,
not executable modules. Existing common-particle admission checks remain at loading.
An unsupported requested controller fails the whole activation; unrelated source
controllers do not block a supported member. Complete graphics, audio, UV,
attachments, other controllers and remaining
battle semantics cooking stay in the current milestone backlog.

Current validation passed all 11 effect-source checks (including both original
banks on both discs), 176 battle/game tests and all 34 cooked-game tests. The cold
classroom snapshot loads the complete source publications; requested common
members 3 and 5 match the original parameter fixtures and the nine pinned Dolphin
particle/RNG observations through fresh native execution. Missing/malformed source
files and unsupported requested members fail preparation; live tasks and clocks
survive a failed replacement. This reuses the valid original capture and does not
claim new image/audio or continuous particle-state acceptance.

The full two-disc cook completed 69,502 conversion units, 1,745 duplicate resources
and zero failures. All 501 inventories generated by that run include both effect
banks with matching digests and sizes, and all 2,181 maintained `.sym` files match
their publications byte-for-byte. Workspace Clippy with warnings denied,
formatting, diff checks and the original source-inventory audit passed. The 30
remaining mixed battle containers still
have current-stage work; they are not excluded from the milestone.

Logs use `local/battle-rewrite/effect-source-*`; publication and inventory evidence
is in `effect-source-publications.json` and `effect-source-cook-verification.json`.
The preceding formation and casting results are earlier checkpoints, not substitutes
for these current loading and cooking results.


### Projectile source loading and effect activation

The common member-7 publication retains all 26 templates, independently of current
runtime admission. Its original table digest is
`e8d0849379683d64bd6a0e071c5ada42005c5ef258a9e360574a318ec86f9048`.
All 400 bytes of each row round-trip through JSON on both discs, including unknown
selectors, inactive floats and instance storage. The shared `FloatOperand` type
also replaces the redundant scalar wrapper in effect declarations; both existing
effect publications retain their exact bytes and digests.

`BattleResources` now supplies projectile source selection and caller hit/effect
bindings. The game preparation path reads the verified row itself, derives the
standard motion/contact definition and compiles its effect dependencies. The
loader uses `0x40` for floor clamping and honors zero active-duration as unrestricted.
It rejects unsupported controllers and absent birth/clash members before activation.
Lightning row 4 supplies a stationary lifetime-20 cylinder, radius 40 and height
400, with technique effect 28. Real caller hit-descriptor extraction, this effect's
remaining controllers and complete Lightning behavior are still current-stage work.

Birth and clash now execute effect constructors at their original call sites,
including their RNG consumption. Focused scheduling tests use original common-3
particle input: they verify pre-motion birth, later followed origins, pause,
retirement without stale references, and clash-constructor RNG before subsequent
contact damage. Those synthetic combinations do not replace the original
Lightning-28 or clash-11 visuals. Common hit effect 1 now admits particle kind 5
through the same recovered initialization/motion functions as the existing common
particles; UV, attachment and other controller gates remain explicit.

This checkpoint passes 181 ordinary battle/game tests, 35 cooked-game tests,
13 effect/projectile source checks (including original rows on both discs), the
source-inventory audit, workspace Clippy with warnings denied, formatting and diff
checks. Fresh native execution of common hit effect 1 matches the pinned Dolphin
emission visits and no-RNG behavior; the existing nine casting-particle observations
also pass through cold verified loading. Neither comparison establishes rendering,
full contact reactions or complete arte acceptance.

The full two-disc cook completed 69,502 conversion units, 1,745 duplicates and zero
failures. All 501 generated field inventories include the projectile table and both
effect banks with verified hashes/sizes; all 2,181 authored `.sym` publications are
unchanged. Evidence uses `local/battle-rewrite/projectile-source-*`, especially
`projectile-source-game-asset-tests.log` and `projectile-source-cook-verification.json`.
The 30 mixed battle containers retain current-stage work. Stage 2 remains partial.

### Common action source cooking and verified hit selection

At decompilation revision `384dd3889598f7d7608f0f644b2ff13e283131cf`, the
manifest classifies `34C` as partial (94.0972%) and `2C8B0` as partial (24.2967%).
Original REL instructions confirm the four-phase copies, the 28/32/12/2-byte
index strides, and every supported command's cursor increment. The relocated
jump table distinguishes real command branches from the non-advancing default.
`B87C` selects a martial member directly; `B7EC` subtracts 200 for a spell member.
`64950` passes Lightning's phase hit descriptor separately to `205AC`, which
selects projectile member 4 and binds that descriptor through `60E6C`.

The ordinary cook now publishes common members 8 and 9 as typed source records
in `battle/{martial,spell}-actions.json`. Each keeps 147 indexed slots: 114
non-null martial bundles and 42 non-null spell bundles. All bytes in every bundle
round-trip on both discs, including compact layouts, unused operands, short
terminators, command roots that share suffixes, and non-finite float bits.
Four-phase source copying does not establish a usable hit binding for compact
slots; their hit selection remains rejected pending the consuming route. This is
not an unreachable-content exclusion. Nurse (native 237, member 37) has a compact
record with duration 240 and no hit pool, while its exact initializer `60694` calls
`37B10` with lifetime 250. That native control flow belongs in maintained source,
not a generated interpretation of the compact record. The importer neither
generates native arte control flow nor publishes VM instructions. Actor-specific
action pools and full command activation remain open.

`ProjectileResource` now selects an action-table member, phase and relative hit
rule. Preparation reads that rule from the same verified snapshot; callers no
longer supply fabricated damage parameters or cooldowns. Completed damage/guard
parameters lower to the existing combat definitions. Missing records and unsupported
flags, inherited elements, power modes, reactions, conditions or impact presentation
fail before activation. Original instructions confirm flag 1 prevents defeat,
`0x80` forces a guard break and `0x1000` suppresses ordinary automatic breaks.

Lightning's original spell member 16 has duration 90 and a rule with flags `0x20`,
lightning element, 130% power, hitstun 20, contact cooldown 30, stun chance 20 and
stagger 1. The ordinary contact now prepares with its arte classification and
source-derived reaction parameters. Flag `0x20` suppresses on-contact TP gain; its
EX skill 134 damage bonus belongs only to the physical branch, which Lightning
skips. The cooked-projectile test now loads the actual descriptor and complete
effect-28 simulation dependencies. The integrated Genis test exercises both;
this remains source-derived execution, not Lightning Dolphin gameplay acceptance.

Instruction evidence is retained in
`local/battle-rewrite/action-source-instructions.txt` and
`action-command-widths.json`. Source and preparation logs use `action-source-*`
and `action-preparation-*`. Current validation passes 184 ordinary battle/game
checks, all 35 cooked-game checks, three action-source tests (including complete
both-disc bundle round trips and production publication), and the source-inventory
audit. Workspace Clippy with warnings denied, formatting and diff checks pass.

Cold verified loading matches all 16 phase scalars and complete first hit-rule
records for common martial members 1/2 and spell members 4/16 against the pinned
original Dolphin checkpoint (combat 224, VI 44345, input 88685). These observations
are recorded in `crates/game/tests/fixtures/common-action-sources.json`; they prove
loaded parameters, not complete arte execution. The new atomic-preparation test
also checks modified and missing hit data after a script cache hit while an older
battle's pending task continues unchanged.

The current full two-disc cook finishes 69,502 conversion units and 1,745 duplicate
resources with zero failures. All 501 generated field inventories include both
action tables and the existing projectile/effect sources with matching hashes and
sizes. All 2,181 authored source publications remain byte-for-byte unchanged.
The 30 remaining mixed battle containers still have current-stage work. Publication
and inventory evidence is in `action-source-publications.json` and
`action-source-cook-verification.json` under `local/battle-rewrite/`.

## Recoil source and operation increment

At decomp revision `384dd3889`, `6124C`, `63470` and `2FB24` are native/exact,
unlinked. The new `battle/recoil.json` publication retains all 19 forward/vertical
pairs from `data:5600`, the proximity threshold, both weight scales and guarded
speed. The normal battle table publisher owns it; the shared field inventory
includes it. Source round trips cover both discs and retain nonfinite/signed-zero
operands, while verified runtime preparation requires finite active parameters.
No native control flow or authored compiler output is published.

The core `Recoil` operation reproduces immediate and pending speed writes,
weight scaling, delayed release, permitted down/launch overrides of proximity
suppression, grounded positive zero, and guard adjustment. Clearing pending speeds
happens after the immediate copy and therefore does not clear live motion.
The delayed-release operation runs before the hurt controller's hit-stop gate;
it does not perform movement or decrement hitstun itself. Complete contact admission,
direction selection, actor-owned controller integration, model transitions and
contact-tail RNG/effects are still pending. These are current-stage work, not
excluded behavior or stage acceptance.

`opening-recoil.json` pins eight natural original-game contacts from
`oracle-reactions-02`: entry to `63470`, wrapper return and complete `3BDF8` return.
The shared recoil operation matches pending/live speed bits for five unguarded hits
and three guarded hits. Source-driven edge tests cover delayed impulses, suppression,
launch precedence and pending immunity; these edge tests are not additional Dolphin
observations. The capture's 704 VI samples match an ordinary replay across 85 fields
without retiming. Audio was recorded silently; no image/audio acceptance is claimed.
The fixture retains hitstun and the three RNG boundaries for the next controller
work, without claiming those complete paths execute in the new engine.

The recoil full-cook checkpoint completed 69,502 conversion units and 1,745
duplicates with zero failures. The 501 inventories produced by this run contain
matching hashes and sizes for the recoil and existing battle publications; all
2,181 authored sources remain unchanged. The 30 remaining mixed battle containers
still require current-stage work. Evidence is in
`local/battle-rewrite/recoil-cook-verification.json`. Subsequent small table/script
iterations use the user-requested temporary `local/dev-battle-cook` helper; full
cooks are reserved for integration checkpoints.

After that checkpoint, all 36 cold game tests and the original-source exclusion
inventory test passed, including recoil preparation from the verified field
snapshot. The temporary cooker regenerated the seven battle table publications
and four battle scripts through the existing publishers in 1.82 seconds, checking
503 existing inventories (501 map inventories plus two older startup inventories).
Its isolated changed-file check verified byte-identical production output, updated
hashes/sizes/totals, unchanged unrelated files, no-op behavior and failure cleanup.
The helper passes Clippy; workspace and helper formatting checks pass. These checks
establish development publication and loading, not complete battle acceptance.

## Ordinary hurt update and recovery

At the same decomp revision, `2FB24`, `2B18C` and `1A9AC` are native/exact,
unlinked. `Reaction` owns the pending impulse, recoil direction, signed countdown
and combo totals independently of interrupted action tasks. Actor dispatch releases
delayed impulses before the local hit-stop gate; floor correction and the countdown
still run while that gate holds movement. Ordinary recovery waits for landing or
the profile's airborne-recovery flag, resets motion and combos, then decrements
the new controller clock. Flying recovery retains the original negative-zero
gravity. Hurt cancels actor tasks while already released resident spells continue.

Recovery also consumes one signed RNG draw to reset party auto-guard chance.
Enemy action guard chance is separate state and remains unchanged. The existing
recoil publication now includes the eleven original default guard preferences;
verified loading resolves the preference contribution before combat. The temporary
cooker refreshes this publication and its existing integrity inventories.

`opening-hurt.json` pins 160 original visits from `oracle-hurt-04`, including four
returns to idle. Fresh `Battle::step` output matches position, live/previous motion,
braking, pending impulse, countdown, combos, activity, auto-guard chance and RNG.
The instrumented replay matches the ordinary replay at 700 identical VI indices
across 85 fields. These observations have no local hit-stop or delayed impulses;
source-driven tests cover those clocks, menu holds, forced airborne recovery,
signed byte arithmetic and task cancellation separately. Audio was recorded
silently; no image/audio or model-transition acceptance is claimed.

Original `31C88` instructions call the actor callback before the common `2503C`
update; `25668..25678` decrements local hit-stop afterward. This cross-check uses
the original REL because `2503C` remains partial, even though `31C88` is exact.

Full contact entry, direction selection, hitstun modifiers, guard/stun/knockdown,
captured and Unison branches, EX recovery, model binding/reset and contact-tail
effects/voices still need integration. Preparation continues rejecting incomplete
source reaction routes. Stage 2 remains partial.

Validation for this increment: 196 ordinary battle/game tests, the verified cold
recoil/recovery preparation test, two importer tests including both original discs,
and seven oracle-tool tests pass. Affected crates pass Clippy with warnings denied;
workspace formatting and diff checks pass. The targeted cook changed one table
and refreshed 501 inventories in 3.89 seconds, with full-cook coverage unchanged.
Logs are `local/battle-rewrite/hurt-{runtime,cold,source,oracle}-tests.log`,
`hurt-clippy.log` and `hurt-targeted-cook.log`.


## Contact entry and ordinary guard recovery

The decomp revision remains `384dd3889`. `4DA50`, `63470`, `2A710`, `2A540`,
`2F284` and `2B18C` are native/exact, unlinked. `3BDF8` remains partial; its
original instructions determine the dispatcher branch/order, not the reconstructed
C. SDK normalization `800FE620` shares the existing reciprocal-length refinement.

Contacts now resolve travel/owner/contact direction, recoil and ordinary hitstun
before updating combo totals. The SDK's 0.5 distance gate preserves a supplied
previous direction; contact setup supplies zero. An ordinary hurt entry clears
local hit-stop, sets motion coefficients, cancels the complete actor task tree
and restarts its selected body clip. Guard entry retains an already selected clip.
Drawing still uses the previously sampled pose until the next model visit.
Independent resident/effect sequences and emitted projectiles survive interruption.
Verified projectile loading selects recoil from the original shared table rather
than restricting contact rows to one hard-coded selector.

`opening-contact-entry.json` pins eight natural contacts from
`oracle-contact-entry-01`: five hurt entries and three blocks. Direction, live and
pending motion, base hitstun/combo writes, local stop, guard chance and body-motion
binding match these observations. Repeated hurt restarts the animation; repeated
guard retains it. The instrumented replay matches 652 identical VI samples across
85 fields. This is not acceptance of complete contact-tail RNG, stun/status,
effects, voice selection or full pose fidelity.

Ordinary manual/enemy guard now integrates and brakes along the recoil direction,
keeps its countdown frozen during local hit-stop and clears accumulated guard
pressure on recovery. Actor action requests remain busy through the recovery
visit. The initial `2A540(mode=0)` update clears the contact follow-up flag:
ordinary expiry therefore calls `2B18C` and returns idle, whereas automatic
follow-up selection takes a different branch. Guard recovery runs before motion
integration and decrements only a nonzero clock; hurt recovery runs after motion
and always decrements. Both share the same native recovery writes and signed RNG
operation, retaining flying negative zero.

`opening-guard-update.json` contains 117 ordinary enemy guard visits, including
two recoveries, from `oracle-guard-update-02`. Fresh `Battle::step` output matches
motion bits, countdown, active guard, combo totals, auto-guard chance and RNG.
The capture also retains three held semi-auto visits as pending observations;
they are not counted as supported ordinary guard. Its replay matches 781 identical
VI samples across the same 85 fields. Source-driven tests separately cover menu
pause, hit-stop, delayed impulses, action admission, combo reduction and guard
break's 45-update hurt entry. Audio was recorded silently without claiming fidelity.

Automatic follow-ups, held guard/counter inputs, facing, idle-pose handoff,
hitstun/EX modifiers, capture/Unison/proximity gates, stun/stagger/knockdown and the
contact tail still require integration. The complete Lloyd/Lightning/Nurse routes
remain unaccepted. No cooking schema or published source changed in this increment;
the existing complete library is reused without recooking.

This increment passes 205 ordinary battle/game tests, all four cold battle
preparation tests, and seven oracle-tool tests. Affected crates pass Clippy with
warnings denied; workspace formatting and diff checks pass. Logs are
`local/battle-rewrite/contact-guard-{final-tests,cold-tests,oracle-tests,clippy}.log`.


## Normal-attack source publication

At the same decomp revision, `3DF34` remains partial (99.2701%); its original
instructions confirm selection from `data:3A48` and the four-pointer action
binding. `1F548` is linked inline-assembly/exact and reads reach from the physical
24-byte descriptor array. Hit/animation/command binders `2DACC`, `2BC3C` and
`2D528` are native/exact, unlinked. The historical normal importer supplied layout
leads; its generated action representation was not restored.

The ordinary cooker now publishes `battle/normal-actions.json` before shared
field preparation. It retains all nine groups, seven selectors and action bindings
per group, and every physical descriptor, hit rule, hit/animation row and referenced
source command. The existing common action command decoder is shared. Float bits,
short command terminators, shared stream roots and unused storage round-trip.
Raine's action bindings alias descriptors 3 and 5 for selections 1 and 6;
the physical descriptors 1 and 6 remain available to the independent reach lookup.
No native control flow or authored executable program is generated.

Five source tests pass, including full table round trips on both original discs,
malformed-pool rejection and nonfinite/signed-zero preservation. The verified cold
loader matches three Lloyd normal hit rows and their shared rule against the
existing pinned `oracle-melee-01` observations. Source identities are in
`game/tests/fixtures/normal-action-sources.json`. This is source/loading evidence,
not complete execution acceptance for Lloyd or the other eight characters.

The user-authorized targeted cooker published the new table, updated both source
aliases and refreshed 501 existing field inventories in 4.40 seconds (503 checked).
All hashes, sizes, roles and inventory totals verify. The helper's isolated check
also covers adding this dependency to an older library, failure before installation,
unchanged unrelated files and a no-op rerun. The baseline full-cook coverage remains
unchanged. All 39 battle preparation tests, including five cold tests, pass.
Evidence uses `local/battle-rewrite/normal-action-source-tests.log`,
`normal-actions-{targeted-cook,cold-tests,helper-verification}.log` and
`normal-actions-publication-verification.json`.

Real normal melee preparation still needs weapon element/anchor selection,
stagger/status and the complete contact tail. Those operands remain preserved
rather than replaced by synthetic defaults. The normal scripts continue to own
sequence timing, motion requests and hit windows. Stage 2 remains incomplete.

The import, game and battle crates pass Clippy with warnings denied after this
publication change; workspace formatting and diff checks pass. The helper's
separate lint result is in `local/battle-rewrite/normal-actions-helper-clippy.log`.

## Live hit elements and action armor

Revision remains `384dd3889598f7d7608f0f644b2ff13e283131cf`. `20704`,
`63470`, `2B18C`, `3DA00` and `3A39C` are exact native targets, unlinked.
`61578` is partial (57.0096%); its armor branch was checked against original
instructions at `62A9C..62ACC`. `3BDF8` is only 3.6227% recovered: its
`3C87C` result-mask gate was checked against original instructions, not the
provisional C. The source identities and instruction hashes are retained in
`local/battle-rewrite/element-armor-source-audit.json`.

`HitElement` now represents inherited, explicit neutral and explicit elemental
hits. The existing shared elemental enum is reused. Actors retain their base
weapon/profile element, current enchantment and action override. `20704` priority
is explicit hit, action override, enchantment, base; inheritance is evaluated at
each contact, including for released projectiles after their owner is interrupted.
Malformed source elements still fail preparation. No numeric invalid element can
enter a prepared hit definition.

Fresh read-only Dolphin capture `oracle-live-element-01` records the actual
`20704` return inside each of eight damage calls. Explicit neutral and inherited
neutral match the native selector. The diagnostic replay matches ordinary
`oracle-clashes-01` at all 704 shared VI indices across 85 watched fields, without
retiming. `battle/tests/fixtures/opening-element-selection.json` pins the inputs,
profile, source, trace and comparison. Nonzero override priority and a changed
enchantment during projectile flight are source-derived native tests; this
opening replay does not observe those branches.

The source hit field formerly called `stagger_resistance` is now `armor_damage`.
Original `61578` adds it to a byte counter only while the old counter is below the
current threshold and the target is not unflinching. The threshold-crossing hit
still deals full damage without ordinary guard resolution, recoil, combo entry
or interruption. The following contact can interrupt. Addition wraps; absorbed
and immune contacts still update the counter. Automatic guard selection precedes
this check, and lethal damage replaces its suppression result. No extra RNG draw
is introduced.

The actor owns base/current armor thresholds and the received counter.
`battle::armor(threshold)` changes action armor in the shared VM. Enemy action
recovery clears temporary armor; ordinary recovery, completion and explicit
cancellation restore the base threshold and clear the counter. The native tests
exercise simultaneous scripted actions, threshold crossing, cancelled child work,
recovery, zero damage, byte wrapping, affinities, death and guard/RNG ordering.
The opening capture has zero armor thresholds; nonzero armor still needs a
representative natural Dolphin case. Real actor preparation must supply the
profile's base armor and action sources. This does not establish complete enemy
controllers or original arte acceptance.

Stun/stagger controllers, hitstun modifiers, the contact audio/effect tail and
concrete normal/caster resource preparation remain Stage 2 work. Inherited
selection and armor damage are no longer blanket loading refusals. Unimplemented
hit flags, stun/stagger, conditions and impact presentation remain explicit gates.

Validation for this increment: 214 battle/game tests pass, including all five
asset-dependent cold tests; five import tests round-trip both discs' original
normal/common action tables; seven oracle-tool tests pass. Affected import,
battle and game targets pass Clippy with warnings denied. Workspace formatting
and diff checks pass. The targeted `actions normals` refresh changed three
publications and 501 inventories in 4.30 seconds; all 1,503 refreshed member
hashes/sizes and inventory totals verify, with full-cook coverage unchanged.
The isolated helper check still reproduces all 12 selected production publications
and verifies failure cleanup, unchanged unrelated files and no-op behavior.
Logs: `local/battle-rewrite/element-armor-final-tests.log`,
`armor-action-source-tests.log`, `element-armor-clippy.log`,
`armor-actions-helper-verification.log`, `element-oracle-tests.log` and
`armor-publication-verification.json`. Full cooking was not repeated for this batch.

## Shared particle UV animation

At decompilation revision `384dd3889598f7d7608f0f644b2ff13e283131cf`,
`403F4`, `40E40` and `426A4` are exact native targets, unlinked. The partial
`418B4` emitter's selector agrees with `426A4`: the signed selector indexes UV
rows directly, not the separate root table. Original instruction ranges and
source identities are retained in `local/battle-rewrite/particle-uv-source-audit.json`.

Loading now resolves a requested particle's original UV rows and validates every
reachable successor and loop destination against the signed selector range.
Unused unsupported declarations do not block preparation. The existing common
particle update owns the row, clock and scrolling offsets; presentation receives
the current rectangle and palette selectors. Initialization, timed frame changes,
single loop jumps, palette keys and signed scrolling preserve the original
arithmetic. High-bit row 254 follows the original scrolling branch. An unflinching
owner holds only the UV clock increment; motion, fading and lifetime continue.
No gameplay RNG draw is added. Model-particle UV behavior remains outside the
admitted controller set.

Fresh read-only Dolphin capture `oracle-particle-uv-01` records 128 natural Common
particle 5 UV updates. Native execution matches their rectangles, palette values,
row selections, clocks and scrolling offsets. Its replay matches ordinary
`oracle-clashes-01` at all 705 shared VI indices across 85 watched fields, without
retiming. `battle/tests/fixtures/opening-particle-uv.json` pins that evidence.
These observations cover timed rows 0–3; loop, scrolling, palette-key and held-clock
branches have source-derived tests, not natural Dolphin coverage. This component
comparison does not accept particle 5's attachment or complete visual controller.

Common particle 19, used by stun entry, now prepares from the verified source bank.
Its original declaration and UV loop are pinned in `stun-particle-source.json` and
checked against both discs. This removes the UV loading blocker; the actor-owned
stun lifecycle, changing star count/spin, head attachment, recovery motion and
contact RNG tail still require integration. No complete stun or arte acceptance
is claimed. In particular, `2E848` writes node `0x94`, which is the declaration's
angular-velocity Z component (`0x6c`), not its separate `angle_step` operand.

The published `SourceBank` format is unchanged. Existing complete cooked assets
load directly; no recook or compatibility path was needed. Validation: 222
battle/game tests pass, including all five cold asset tests and eight new UV tests.
All 11 effect import tests and seven oracle-tool tests pass; the import tests also
check the new stun fixture against both discs. The import, battle and game crates
pass Clippy on all targets with warnings denied. Workspace formatting and diff
checks pass. Logs and the replay comparison are under
`local/battle-rewrite/particle-uv-*`. Stage 2 remains incomplete.


## Stun contact and actor-owned recovery

Decompilation revision remains `384dd3889598f7d7608f0f644b2ff13e283131cf`.
Native/exact, unlinked `29330`, `2E848`, `1BA44`, `2B18C`, `2A710`, `2F284`
and `9E38` establish entry, recovery, retained particle ownership and sound requests.
The partial contact dispatcher's original `3CEE0..3D01C` instructions establish
chance selection and RNG consumption. Identities are retained in
`local/battle-rewrite/stun-source-audit.json`.

Eligible contacts now consume the unsigned stun roll even for zero chance and
immune targets. Chance combines the hit operand, attacker bonus, target resistance
and EX bonus in original order. Successful entry interrupts actor work, clears
armor, starts the 180/90-update countdown and retains common particle 19 independently
of the cancelled action. Grounded human input can accelerate recovery; its two-visit
pulse continues in the air. The actor updates head attachment, spin, palette and
star count. Expiry clears retained stars, requests the appropriate recovery motion
and waits for its completion. Periodic sound requests are ordered cues; playback
still needs presentation integration. Subsequent hurt and death clear retained
actor particles while released action particles retain their independent lifetime.
Preparation validates required models, clips, head bindings and particle definitions
before activation; object-pool exhaustion does not cancel the stun or consume RNG.

Control values are now correctly interpreted as Manual=0, SemiAuto=1, Auto=2,
Enemy=3. Source `2B18C` gives Manual/SemiAuto a recovery countdown of 30;
Auto/Enemy receive zero. `2E848` accepts struggle only for Manual/SemiAuto,
and `2F284`'s ordinary automatic countdown applies to Auto/Enemy.
This corrects the earlier manual/Auto classification; held guard remains pending.

Fresh read-only `oracle-stun-rolls-01` records eight natural eligible contacts,
including zero-chance rolls. The follow-up `oracle-stun-entry-02` records 22 rolls,
including two successful entries and re-entry after interruption. Native execution
matches chance, RNG transitions, countdown, armor clearing and retained allocation.
The diagnostic replays match their respective ordinary inputs at 703 and 1,592
shared VI indices across 85 watched fields, without retiming.

`oracle-stun-update-01` supplies 200 controller visits across two natural stuns;
`oracle-stun-recovery-01` supplies another 180 visits through natural expiry.
Native comparisons match countdown, actor motion, model requests, palette/spin/star
count writes, ordered sound requests and recovery's guard/chance/combo/RNG reset.
The recovery visit deletes the retained particle and returns to idle on the
original update. The controller comparisons use original supplied head samples;
they do not establish native skeletal sampling. Their diagnostic replays match
ordinary recordings at 1,518 and 1,600 VI indices across 85 fields. Fixtures are
`opening-stun-{rolls,entry,update,recovery}.json`; captures, comparisons and complete
input/profile identities are retained under `local/battle-rewrite/`.

The first 20 isolated `stun-search-01` runs timed out after 1,344–1,539 VI samples
and yielded no successful stun. All 20 `stun-search-02` replays completed and found
six stuns across five distinct input cases. The recovery case keeps case 18's first
technique input and supplies no further attacks; it is a separate input identity.
The first entry diagnostic requested 24 calls but the battle ended after 22;
`oracle-stun-entry-02` reruns the same inputs with that observed finite bound.
Incomplete diagnostic runs are not acceptance evidence.

Accelerated/shortened recovery, human controls, airborne visits, hit-stop and pool
exhaustion retain source-derived tests. Profile mesh overrides and full stun
image/audio fidelity remain pending. This is shared-controller evidence, not
complete normal-attack, Lightning or Nurse acceptance.

Validation: 232 battle/game tests pass, including all five cold asset tests;
all 11 effect import tests and seven oracle-tool tests pass. Import, battle and
game all-target Clippy passes with warnings denied; workspace formatting and diff
checks pass. Logs are `local/battle-rewrite/stun-{final-tests,import-tests,oracle-tests,clippy,fmt}.log`.
The published effect source-bank format is unchanged, so this increment required
no recook. Full Stage 2, including complete contact semantics and concrete normal,
Lightning and Nurse preparation and fidelity, remains incomplete.

The next normal-contact boundary is stagger buildup and knockdown. Lloyd's source
hit row has stagger=2. Current `61578` instructions add the byte counter only
through the eligible damage branch; `3BDF8` tests its threshold before random stun.
`2B18C` clears buildup on ordinary recovery. Knockdown consumers `278D0`, `2EB74`,
`289F4` and `2E790` also handle pose completion, contact protection and enemy
recovery actions; admitting the source row requires these consumers, not dropping
the operand. The focused source audit is `stagger-source-audit.json` in the same
local evidence directory. The ordinary subset is implemented below; full actor
preparation and the conditional branches remain open.

## Ordinary stagger and verified normal contacts

The decompilation remains at `384dd3889`. The current target manifest classifies
`1F794`, `278D0`, `289F4`, `2B18C`, `2E790`, `2EB74` and `31C88` as exact native
code, unlinked; `50378` is exact inline assembly, also unlinked. `61578`, `3BDF8`
and `2503C` remain partial. Their relevant original instructions are retained in
the stagger source audit. `31C88` dispatches the activity callback before common
timers, including the newly initialized grounded window and recovery protection.

Damage resolution now adds stagger with byte wrapping on eligible ordinary and
broken-guard hits. Armor, absorption, immunity, unflinching and successful guard
suppress buildup. Reaching the profile threshold precedes the stun roll; profiles
that forbid knockdown still skip that roll. Entry interrupts actor tasks, preserves
released work, clears buildup and uses the prepared down duration. Grounded down
waits for animation completion before counting down, then selects get-up or takes
the original missing-motion recovery path. Drawing retains the model sampled
before the callback's request. Local hit-stop does not hold this controller;
the battle menu does. Ordinary recovery clears buildup.

Down and recovery protection retain their independent clocks. During the grounded
window, ordinary contacts deal one eighth damage and suppress reactions; the
original `0x40` hit flag permits ordinary damage/reactions. Outside that window,
downed enemies receive one quarter damage and party actors avoid damage. Recovery
protects party actors and reduces enemy damage to one eighth. Absorption keeps
its original priority over avoidance; lethal results replace protection flags.
Preparation requires down resources for susceptible actors and validates every
declared recovery clip before activation. A missing get-up clip is a supported
source path, distinct from a missing declared resource.

The existing `opening-guard.json` fixture now also compares stagger before/after
all eight original resolver calls. It uses the already verified, unchanged
`oracle-guard-01` trace: five buildup writes and three guarded non-writes match
fresh native execution, alongside HP, guard and RNG. No retained opening capture
contains activity 14/16. The down/get-up transitions, protection branches and
interruption lifecycle currently have source-derived tests, not a Dolphin
controller acceptance claim. Forced down/launch, EX recovery branches, enemy
recovery-action selection, dust, profile mesh overrides and complete pose/audio
fidelity remain pending. There is still no complete normal-attack acceptance.

`BattleResources::melee` now returns an original normal-contact selection and
prepared attachment groups. The single game preparation path loads the contact
from verified `NormalTable` records, follows selector/action aliases, preserves
group order, reads its rule and resolves recoil from the shared table. It rejects
stream terminators, missing groups and unsupported emission routes. Damage-kind
and shape decoding are shared with projectile loading; inactive ring operands
are not interpreted. Authored source still owns hit-window sequencing.
The cold cooked-library test resolves Lloyd's neutral, finisher and thrust rows
with original radii, cooldown, damage kind, hitstun and stagger. A changed source
row is rechecked on a script-cache hit; failed resource preparation leaves the
active generation intact. `2D564` remains partial (95.159%); original instructions
at `2D918..2DA34` confirm ordered group traversal and direction 1. `3D920` is exact
native code. Evidence is in `normal-contact-bindings-instructions.txt` and the
expanded source audit under `local/battle-rewrite`.

Validation: 243 battle/game tests pass, including all five cold asset tests.
Battle/game all-target Clippy passes with warnings denied; workspace formatting
and diff checks pass. Logs are `normal-preparation-final-tests.log`,
`normal-preparation-clippy.log` and `knockdown-focused-tests.log`
in the same evidence directory. This batch changes runtime/preparation only;
published source and cooked formats are unchanged, so no recook was needed.
Stage 2 remains incomplete, including concrete models/caster preparation and the
full normal, Lightning and Nurse execution/presentation comparisons.

## Party profile preparation

At the unchanged decompilation revision, native/exact `1CAA8` copies the party
template, prepares statistics through `CEA8`, then overwrites guard pressure with
maximum HP divided by 100 plus 3. `battle/party-profiles.json` now publishes all
eleven templates before shared field inventories. Import tests reconstruct every
byte from the typed fields and retained storage on both original discs, including
non-finite float operands. No executable behavior is published.

`battle::profile::party` consumes the verified snapshot and an owned actor
candidate. It supplies existing recoil, armor, stagger, stun, guard, scale and
movement traits while preserving session statistics and controls. Guard pressure
uses prepared maximum HP with the original halfword narrowing. Model and caster
bindings, equipment/condition modifiers and enemy profile preparation remain
unfinished; this helper is not complete encounter activation.

The unchanged, verified `oracle-guard-01` trace supplies Lloyd's profile in eight
owner snapshots. The extended `opening-guard.json` fixture retains the original
trace hash and input/clock identities. Fresh cold preparation matches their guard,
stagger, stun, scale and movement traits, including guard pressure 6 at maximum
HP 328. This verifies preparation values, not full actor or presentation fidelity.
The source classifications and consumers are recorded in
`local/battle-rewrite/profile-source-audit.json`.

The temporary cooker now supports `profiles`. Its production publisher took
4.16 seconds to add the table and refresh 501 inventories out of 503 checked,
without recooking other resources. Full-cook coverage remains unchanged.
Verification covers new dependency installation, aliases, integrity totals,
unchanged unrelated files, repeat runs, lock ownership and failure cleanup.

Validation: 246 battle/game tests pass, including all six cold asset tests, plus
both profile importer tests. Import/battle/game all-target Clippy passes with
warnings denied; formatting and diff checks pass. Evidence logs use the
`profile-` prefix under `local/battle-rewrite`. Stage 2 remains active and incomplete.


## Enemy body preparation

At the unchanged decompilation revision, `1BD18` and the indexed label helper
`5B7CC` are native/exact. The rig publisher uses the existing bounded model decoder
and preserves bind-channel presence, indexed bones and original transform kinds.
MO/DM classification retains byte arithmetic, including decorative names that carry
into flag bits and body-only zero-radius volumes. Radii remain unscaled until
contact calculation. AT group order and KK attachment slots are published as data.
`1BAF4` remains partial despite a 100% score; publishing body volumes does not
establish body-displacement execution.

Production Monster Book cooking now publishes the enemy profile and rig while its
original package is in hand. It also records hashes and sizes of that monster's
shared meshes, textures and sparse clips. The descriptor is a shared field
dependency; its model payloads load only for selected enemies. `Files` uses one
verifier and process-local byte cache for field loading and these additional
resources. An owned candidate can fail without replacing the suspended field or
active battle. Cached payloads still require disk integrity verification.

`battle::model::enemy` prepares ordinary base/variant statistics, all nine
resistance codes, shared profile traits, primary-body sparse clips and hurt points.
Enemy guard pressure comes from the template. It binds ordinary hurt, ground/air
guard, down/get-up and optional prepared stun resources. An absent reaction clip
leaves current playback intact; alternate hurt falls back to clip 3. Declared but
missing resources still fail complete battle preparation. Entry phase, placement
and root-axis policy remain supplied by the encounter/controller binding.

All 251 primary rigs on both discs match existing model bone order and sparse clip
bindings. Their transform kinds are all ordinary inherited transforms. The cold
Ghost/Zombie preparation test uses the existing verified opening resolver capture for
statistics and guard traits; model construction and sampling are exercised with
real sparse clips. This is preparation evidence, not full pose or presentation
acceptance. Difficulty, live conditions, weapon/animated attachments, special model
transitions, party battle models and concrete caster bindings remain unfinished.
Source classifications and hashes are in
`local/battle-rewrite/enemy-model-source-audit.json`.


Validation: 264 content/battle/game tests pass, including all seven cold asset
checks, plus both importer tests over the two original discs. The temporary
`enemy-models` refresh took about 10 seconds, updated 251 descriptors and 501
inventories, and retained full-cook coverage. Helper verification covers 264
production-identical publications, repeat runs, new dependency installation,
missing model resources, lock ownership and failure cleanup. Content/import/battle/game
all-target Clippy passes with warnings denied; formatting and diff checks pass. Evidence uses the
`enemy-` prefix under `local/battle-rewrite`. Stage 2 remains active; the next
model binding work was the party's independent battle animation banks and carried
weapons; the body-bank increment is recorded below.

## Party battle body preparation

At revision `384dd3889`, battle entry `12C` and body setup `1CCAC` are native/exact.
The DOL body/motion selectors `80081618`, `8008172C` and `800818E4` are also
native/exact, with unlinked function proofs; their tables in
`runtime_file_tables.c` are linked. Ordinary battle setup uses the cached body
wrapper and the separately selected battle CAB. Motion slot zero is CAB member
two; null members retain their slots. Special battle modes that take body members
from the CAB remain outside this ordinary binding. Initial-pose setup `52668`
remains partial, so entry playback and root-axis policy still come from the caller.
Source identities and classifications are recorded in
`local/battle-rewrite/party-model-source-audit.json`.

The production cooking graph now retains the required body and battle-motion
packages and publishes nine standard-costume descriptors from the shared decoded
models and curves. It does not reintroduce sampled animation frames or a second
geometry path. Both original body and motion aliases record each descriptor.
Field inventories contain descriptors; selected model loading verifies their
mesh, texture and sparse-clip dependencies using the existing byte cache and
integrity checks.

Party and enemy preparation share ordinary body binding and reaction resources.
Party preparation preserves session HP, TP, statistics and control mode, then
applies the original party profile, HP-derived guard pressure, indexed hurt bones
and motion slots. Complete generation validation still precedes activation.
All nine standard bodies on both discs have ordinary inherited bone transforms;
their skeleton order and sparse clips match shared rendering publications.
Cold loading exercises all nine, Lloyd's attack motion and Genis/Raine casting
motions, including a real initial sample. This is preparation evidence, not
Dolphin pose or full-action acceptance.

The temporary `party-models` refresh publishes the same descriptors and model
resources. The first refresh took about 20 seconds: 481 selected publications,
224 changed files and 501 refreshed inventories out of 503 checked. Full-cook
coverage remains unchanged. Carried weapons, animated attachment policies,
costume variants and concrete casting resources remain unfinished. In particular,
Lloyd's body has no AT contact groups; normal attacks require the carried weapon
anchors before full melee can run against original geometry.

Validation: all 42 game unit and 51 battle-preparation tests pass, including all
eight cold asset checks. Three rig importer tests cover classification and both
original discs; the separate production DAG test reproduces all nine targeted
party publications exactly. Helper verification covers 745 production-identical
publications, selected-only model payloads, repeat runs, dependency installation,
lock ownership and failure cleanup. Content/import/battle/game all-target Clippy
passes with warnings denied. Evidence uses the `party-` prefix under
`local/battle-rewrite`. Stage 2 remains active and incomplete.

## Weapon resource preparation

At the same source revision, resource selectors `159BC`, `16060` and `16154`,
weapon contact classification `153BC` and replacement `1F5FC` are native/exact.
The ordinary constructor `15B20` remains partial (90.508%); instance setup
`1C330` is also partial (84.2308%). This increment prepares data and does not
claim their complete controller behavior. The source audit is
`local/battle-rewrite/weapon-source-audit.json`.

Shared cooking publishes the bounded weapon bank through the existing model,
texture and sparse-curve tools. The source directory retains 156 resource slots
before EOF, including holes. Multi-part models preserve their slot selection and
primary, outline and extra layers. Local animation clips remain sparse. Weapon AT
groups use the third label byte, relative to the attachment instance; body AT
groups use the fourth byte. They now have distinct source classification in the
shared rig reader.

`battle::model::load_files` resolves equipment and battle-only weapon IDs and
verifies the selected model's dependencies. Source aliases, including the shared
Blue Shield/battle-only resource, retain their identities. Invalid equipment
domains and absent resources fail loading. This supplies resource preparation;
Lloyd's concrete weapon anchors, drawn attachments, trails and autonomous or
owner-linked weapon clocks remain unfinished. Genis's body-bank-linked weapon
motion must not be treated as a static weapon merely because its package has no
local clip. No new Dolphin pose or complete-action acceptance is claimed.

Both original discs produce the same bank: 153 populated slots, 267 model parts
and 18 primary parts with local animation. Importer checks preserve slot holes,
indexed skeletons, sparse clip bindings and both Wooden Blade contact groups.
The targeted `weapons` cook took 13.25 seconds, selecting 613 publications and
refreshing 501 field inventories without changing full-cook coverage. Combined
helper verification reproduces 1,358 publications exactly, preserves unrelated
files, and makes no changes on a repeat run. All 42 game unit and 53 preparation
tests pass, including nine cold checks. Content/import/battle/game all-target
Clippy passes with warnings denied; formatting and diff checks pass. Evidence uses
the `weapon-` prefix under `local/battle-rewrite`.

## Rigid weapon contacts

At unchanged decompilation revision `384dd3889598f7d7608f0f644b2ff13e283131cf`,
`153BC`, carried-weapon update `155CC`, drawing query `5B8BC` and gameplay query
`5B984` are native/exact. The DOL hierarchy composition unit `game_actor_8012A7B8`
is exact; `8006D2E0` remains a candidate and `2D564` remains partial. The existing
weapon source audit now records these classifications. Original contact queries
use gameplay matrices; a contact-only weapon bone need not have a drawing object.

`battle::model::rigid_weapon` composes a selected rigid weapon's contact bones into
the owning body's attachment offsets, returning instance-relative groups for the
normal-attack loader. It validates the weapon before appending anchors, so failed
binding leaves the candidate model intact. Sparse groups and duplicate contact
bones retain their order. Animated layers are rejected by this operation; the
caller must select the original controller policy, including Genis's owner-linked
route, which remains unfinished.

Fresh silent capture `local/battle-rewrite/oracle-weapon-pose-04` contains 384 melee
updates, including 272 Lloyd body poses and 544 sword-instance poses. Its 761
shared VI samples match the ordinary replay across all 85 watched fields without
retiming. `lloyd-weapon-poses.json` pins disc, emulator/profile, checkpoint, movie,
observations and source asset identities. Cold loading compares 20 unblended
finisher samples across four attacks: 1,140 ordinary body matrices, 80 sword points
and all 40 submitted contacts. Maximum absolute error is 0.00006104 for body
matrices and 0.00008774 for sword positions, below the 0.001 pose-check tolerance.
The normal-contact loader selects the same right/left anchor groups as the original.

The full-body comparison also exposed the expected missing secondary-motion
integration: hair and coat chains differ by up to 75.62108 world units. They remain
recorded in the fixture but are explicitly outside this ordinary-pose acceptance;
they need retained solver history, not a different tolerance. Drawing/trails,
cross-fades, complete action/dispatch timing, image and audio fidelity remain
unfinished. No complete normal attack is accepted by this comparison.

All 42 game unit and 55 preparation tests pass, including ten cold checks. The new
malformed-binding test verifies atomic failure for invalid bones/groups, transforms,
animated or absent layers and nonfinite data. Seven Python oracle tests, affected
all-target Clippy with warnings denied, formatting and diff checks pass. Evidence
uses `weapon-binding-` and `weapon-pose-` under `local/battle-rewrite`. No recooking
was required. Stage 2 remains active.

## Shared secondary motion

Secondary motion uses the same revision's authoritative
manifest: `game_chain_runtime_80066348`, `game_chain_runtime_80068064` (including
`69088`) and `8006CEB0` are native/exact. Older candidate notes do not override
those current classifications. `local/battle-rewrite/secondary-source-audit.json`
records source identities and the concrete remaining questions.

The first diagnostic observed only animation visits with flags 5, which skip
matrix composition; it is not solver evidence. The extended motion tracer now
observes `69088` directly and retains nested calls. Fresh silent capture
`oracle-secondary-02` records 361 visits with live chains, including 121 Lloyd
updates. All 702 shared VI samples match the ordinary replay across 85 fields.
Lloyd's animation sampling precedes chain simulation; his hair has four joints and
each coat chain has eleven. The retained snapshots include callback, parameters,
positions, previous positions, targets, velocity, wind and floor state.

The retained solver now lives in `resonance-content`, shared by field presentation
and simulation-owned battle models. Battle preparation resolves the existing
authored chains and character policies. Dynamics run after ordinary bone sampling
and before hurt points and attachments observe the pose. Battle uses Y-up gravity
and the original five-unit floor; the field keeps its Z-up coordinates. Collision
projection precedes floor clamping and velocity feedback. Only driven joints are
overwritten; terminal guides and untracked children retain their sampled matrices.
Menu pause preserves the model and its retained history. No recooking was needed.

Fresh `oracle-secondary-04` also records primary matrices before and after each
solver visit. All 702 shared VI samples match the ordinary replay across the same
85 watched fields. Capture 03 is rejected: Dolphin exited with SIGPIPE when the
debugger disconnected before consuming its continuation acknowledgement. Waiting
for that acknowledgement fixed capture 04 without changing inputs or game memory.
Builds and cooking stayed separate from captures.

The pinned fixtures compare 121 consecutive hair updates, plus twelve sampled
updates of all three chains and all 83 resulting body matrices. Independently
seeded state and driven-matrix comparisons have maximum absolute error
0.00003052. Keeping hair state continuously across all 121 updates gives maximum
error 0.00009156. Both use the unchanged 0.001 tolerance. These are component
comparisons with observed authored targets/matrices; they do not establish full
body sampling, initialization, controller timing or image fidelity. Host floating
point arithmetic is not bit-exact original SDK arithmetic. Special wind, disabled
chain and held-binding routes still need integration and applicable evidence.

All 161 battle and 17 content tests pass. The 42 game unit and 55 preparation
tests pass, including ten cold checks. Affected all-target Clippy passes with
warnings denied, as do formatting and diff checks. Presentation's 53 ordinary tests pass,
including the shared solver and affine-matrix regressions; 17 asset/device tests
remain ignored in that run. Seven oracle-tool tests also pass. Evidence uses the
`secondary-` prefix under `local/battle-rewrite`. Stage 2 remains active.

## Authored normal-attack sound requests

At the same source revision, command dispatcher `2C8B0` remains partial. Original
instructions `2D108..2D144` verify commands 28 and 27: sound requests call native/exact
`9E38` with a 16-bit index and priority 1; voice requests enqueue through native/exact
`71E78`. The second stored sound operand is not consumed. A voice request is not an
immediate sound, so its queue and playback policy remain separate work.

`normal_lloyd.sym` now requests prepared sound 60 at age eight for neutral attacks,
and ages eight and twelve for finishers. The shared `battle::sound` operation emits
ordered cues without consuming gameplay RNG. Loading resolves `battle::Sound`
assets through the existing resource service before activation. Script pauses
hold future requests; interruption cancels them without issuing a stop for sounds
already requested. No playback/mixing runtime or per-arte Rust controller was added.

Fresh silent capture `oracle-action-sound-01` contains 1,024 hit-stream/command
visits. Its 789 shared VI samples match the ordinary replay across all 85 watched
fields without retiming. The pinned `lloyd-action-sounds.json` fixture retains
twenty request visits from four neutral/finisher pairs. All twelve sword requests
match native scripted command ages, sound indices and priorities. All 228 observed
Lloyd command visits preserve gameplay RNG. The fixture also retains eight voice
requests, which do not yet execute in Resonance. Full action timing, screen-space
sound positioning, voices, audio resource selection and audible fidelity remain
unfinished; this comparison establishes sound requests only.

All 163 battle, 42 game unit and 56 preparation tests pass, including ten cold
checks. Tests cover menu/local-hit-stop pauses, interruption, invalid priorities
and bindings, and failed sound preparation while an active sequence continues.
Affected all-target Clippy passes with warnings denied, as do seven oracle tests,
formatting and diff checks. The temporary cooker published the four selected
battle sources in 3.87 seconds: one `.sym` file changed, with 501 inventories
refreshed. Published source bytes match the maintained file exactly. Evidence uses
the `action-sound-` prefix; Stage 2 remains active.


## Verified ordinary casting parameters

Rechecked decomp revision `384dd3889`. The existing DOL technique catalogue now
lives in shared content and publishes as `game/techniques.json`; menus and battle
use the same importer. Party publication retains all four original Genis chant
records, including the terminal, and the shared actor effect scale. Both discs'
profile/chant bytes and technique records round-trip without generated behavior.

`battle::Casting` preparation combines the selected character, technique and ready
body model. Typed native records expose passive parameters and module-local motion
references. `casting.sym` owns the countdown, concurrent chant, transitions and
release timing. Source `genis_lightning.sym` binds the ordinary route and its
initial sound request. Missing clips, incorrect admission cost and unsupported
stored-scene/special routes prevent activation. A new cast restarts its chant even
when the old release clip remains selected; invalid row access faults the sequence
and cancels its tasks. Nurse slot 99 requires the stored-scene branch of `3898C`,
so it is explicitly rejected by this ordinary loader.

The existing pinned `oracle-casting-01` trace, memory, input and initial-state hashes
were reverified before reuse. Fresh native cold preparation with Genis's actual
model matches all 320 Fire Ball clock/TP/release observations and 317 held motion
samples bit-for-bit. The same clock case also executes with Lightning's source
cost of 9 TP and 140-update base countdown. Its resident is deliberately a no-op
fixture: this does not establish full Lightning hit/effect or Nurse acceptance.

All 165 battle, 42 game unit and 58 preparation tests pass, including eleven cold
checks; two profile and six technique importer checks pass with original discs.
An initial cold run used the absent default `local/cooked` directory; rerunning
all eleven checks with `RESONANCE_TEST_ASSETS` set to `local/all-assets` passed.
Affected all-target Clippy, formatting and diff checks pass. The temporary cook
refreshed 258 selected publications (255 changed) and 503 inventories in 10.34
seconds after the helper build. Source publications match maintained bytes.
Evidence is under `local/battle-rewrite/casting-preparation-*`.

Ordinary release bursts, sound requests and element tint are covered by the next
increment. Voices and full audio/render dispatch remain pending. Nurse additionally
needs stored-scene preparation, scene clocks/ownership and its full resident
presentation. These remain current-stage work; no stage exit.


## Ordinary release bursts and element tint

The decompilation revision remains `384dd3889598f7d7608f0f644b2ff13e283131cf`.
`casting.sym` selects burst 7/8 and requests it before sound 123 and spell release.
Original modifier words now support the required signed-halfword/byte assignments,
including both alpha values, brightening, UVs and geometry count. Release particles
12/13/14 retain an after-target drawing binding while updating in group 6.
Element palettes and colour rows are passive REL data, published through the full
cook and loaded from the same verified snapshot. Tint replaces palette 0 and colour
1 RGB after modifiers/scaling; it preserves alpha and skips unmarked particles.

Fresh captures `oracle-cast-release-01`, `oracle-release-effects-01` and
`oracle-release-particles-02` pin 320 casting calls, 34 common-7 timeline visits
(two releases) and 104 common-particle updates through the first burst's inclusive
lifetimes. Post-emission records match the separate first-update capture byte for
byte. Native state matches all 104 updates, including exact float bits, tint,
geometry, colour and lifetime. This exposed and corrected yaw rotation's signed-zero
result using the original SDK matrix operation order. Each replay agrees with
`oracle-clashes-02` across 85 fields at 1,150, 1,127 and 700 identical VI indices,
respectively. Inputs and thresholds were unchanged. The first particle capture
attempt failed because sandboxed Xvfb could not bind its socket; the isolated,
muted capture succeeded outside that sandbox.

Fixtures are `casting-requests.json`, `release-particles.json`, `effect-tints.json`
and the expanded `casting-particle-sources.json`. The common-8 modifier variant
has source-derived execution tests, not Dolphin route acceptance. These observations
establish simulation/request behavior, not rendered images, audio playback, full
Lightning or stored Nurse. Stage 2 remains incomplete.

This increment passes 282 affected Rust tests: 165 battle, 42 game unit, 62
preparation (including eleven cold loads), and thirteen original effect importer
checks. Seven oracle-tool tests, the Genis script check, formatting and affected
all-target Clippy with warnings denied pass. Tint/script refreshes used only the
temporary production-publisher helper (about four seconds per refresh); no full
recook ran. Logs and the pinned evidence inventory are `local/battle-rewrite/release-*`.


## Arte contacts and TP recovery

The decompilation revision remains `384dd3889598f7d7608f0f644b2ff13e283131cf`.
Original `3C58C..3C858` instructions and exact `A9B4`/`1D864` establish +1 attacker
TP for ordinary landed contacts, capped at maximum TP. Arte contacts, guarded or
guard-breaking contacts, avoidance, absorption and immunity skip this recovery.
Armor and reduced down contacts still qualify; defeat replaces the damage result
flags before the decision. The core now applies this once per admitted contact.

Original `61B64..61BA0` confirms the physical arte EX bonus adds 20% after damage
variation and before accuracy, critical and power. It consumes no extra RNG and
does not affect magic. The prepared actor carries this trait; equipment/EX skill
selection remains shared-mechanics work. Original Lightning's hit flag now loads
without incorrectly blocking magic on a physical-only EX branch.

Fresh `oracle-contact-tp-01` captures 32 contacts, including nine repeated Fire Ball
hits. Native damage, guard/critical results, resolver RNG, target HP and attacker
TP match all 32. The observer records actual TP recovery calls, including capped
calls. The diagnostic replay matches 85 fields at 1,150 identical VI indices
against `oracle-clashes-02`, without retiming. `opening-contact-tp.json` pins the
inputs, emulator/profile/disc identities and observation hashes. This validates
admitted contacts, not collision timing, full contact-tail RNG, arte effects or
complete actions. EX-enabled damage cases and guard-break/protection/absorption
TP cases have source-derived tests; they have no new Dolphin route acceptance.

The same hit flag also contributes to final-kill bonus flags at `3D100..3D114`.
Exact `65CB0` accumulates that bonus, and current `57718` consumes it in item-drop
rolls. The complete bonus, result and display path belongs to stage 3's rewards
and remains missing; arte classification is retained for it. Effect 28's kind-18
particle and remaining modifier operations still need preparation; spatial sound
command support is described below.
Stage 2 is incomplete.

Validation passes 272 affected Rust tests (168 battle, 42 game and 62 preparation,
including eleven cold loads), seven oracle-tool tests, formatting and affected
all-target Clippy with warnings denied. No recook was necessary. Source audit,
replay comparison and validation logs are under `local/battle-rewrite/contact-tp-*`.


## Original effect sound commands

Original command 252 now resolves its sound dependency during effect preparation
and executes through the existing gameplay VM and `battle::sound`. Instructions
`41980..4199C` confirm the argument is the sound index, the operand narrows to a
priority byte, and the position comes from the effect context. The cue retains
that context's origin as its owner moves. Zero remains silent and needs no audio
resource. Missing sound preparation prevents activation, including dependencies
in an unexecuted script branch or a later original command.

Tests cover ordered sound/particle emission, repeated sound commands, priority
narrowing, menu pause, independent effect lifetime after owner interruption,
effect cancellation, unchanged RNG and failed preparation with a live generation.
The existing actor/casting sound tests still pass. The resource-service callback
adds no compiled publications or active-combat resource lookup.

`oracle-effect-sounds-01` observed 2,048 original effect visits and agrees with
the ordinary replay across 85 fields at 1,127 identical VI indices. This input
executed no command-252 sound requests, so it supplies observer regression evidence
only; sound-command Dolphin/audio acceptance remains pending. The observer now
records nested spatial sound requests for the next representative case.

Current validation passes 273 affected Rust tests, including eleven cold asset
loads, plus seven oracle-tool tests, formatting and affected all-target Clippy.
No recook was needed. Evidence is under `local/battle-rewrite/effect-sound-*`.
Lightning's remaining kind-18 particle uses common update `403F4` and draw
dispatch `7A8D0` → `777A0`; it also requires original integer/position modifiers.
These remain current Stage 2 work alongside full Lightning/Nurse preparation
and the remaining normal-action observations.

## Lightning effect and integrated Genis preparation

At the same decompilation revision, Techniques/28 now prepares all three original
particle declarations, its repeat commands, modifiers and sound 92. Kind 18 keeps
fixed ribbon dimensions and advances its phase using the signed period and age.
The shared effect context has four signed integer scratch cells: native `3F0C0`
opcode 3 adds with halfword wrapping; it is not an RNG operation. Byte assignment
and addition select segment count and phase. Offset modifiers retain the separate
signed RNG draws in X/Z order. No per-arte Rust controller or cooked executable
publication was added.

The original source fixture matches both discs through the production importer.
Headless tests cover all fourteen emissions, phase/count variants, random offsets,
inclusive particle lifetimes, scaling, pause, caster interruption, scratch isolation
and wrapping, and rejected unsupported writes. The cold integration test loads
Genis and two actual enemy bodies with required knockdown/stun resources. Maintained
casting and Lightning scripts execute with the original hit rule, projectile and
effect: payment is 9 TP, release is update 142, birth is 164, and the two recipients
are hit on updates 165/166. The resident slot lasts through callback age 90 even
after its task and particles finish. Pre-release interruption and insufficient TP
prevent release; post-release interruption preserves it.

Those Lightning timings are source-derived integration expectations, not new
Dolphin observations. Presentation bindings are test IDs; ribbon mesh generation,
its indexed drawing table, actual sound playback,
and a representative Lightning Dolphin route remain pending. Nurse's stored scene
and the other Stage 2 gaps remain open. Evidence and validation logs use
`local/battle-rewrite/lightning-*`. Only the maintained script comment was refreshed
in the cooked library: five selected script publications, one changed file, about
four seconds of cooking; no full recook ran.

Validation passes 291 affected Rust tests (169 battle, 42 game, 67 preparation
including twelve cold loads, and thirteen effect importer checks), formatting,
diff checks and all-target Clippy for content, battle, game and import. No new
Dolphin capture was taken for this increment.

Lightning initialization now calls the reusable `retarget_unavailable` operation
before capturing its origin. `37034` is partial (98.3607%); original instructions
`37068..37124` confirm the mask-zero selection branch. Native/exact `37ED0` calls it
with mask zero and flag one. It keeps an available target or selects the first living, non-petrified
actor on that side; if none exists, it retains the original target. This selection
consumes no RNG. `64950` later emits using the retained target and position. Tests
cover both owner sides, living/defeated/petrified targets, skipped candidates,
no replacement, and movement or death after initialization. The current simulation
uses actor order as roster order; inactive original actor modes remain outside its
prepared actor domain. The script was refreshed using the temporary script-only
publisher. The affected suite passes 170 battle, 42 game and 67 preparation tests,
including all twelve cold tests.

The controlled original-game fixture now grants Genis Lightning and disables Fire
Ball through the ordinary saved technique setting. Both Dolphin and native copies
record those changes and their source/output hashes. An initial grant-only fixture
still selected Fire Ball; its captures are not Lightning evidence. The corrected
fixture observes native arte 216, 9 TP paid at combat tick 163, release at 164, and
effect 28 at 186. Its sound 92 dispatch precedes the first particles. Two casts are
recorded; the first supplies the checked-in `lightning-observations.json` regression.

That regression compares all fourteen emissions and 366 particle updates through
their inclusive lifetimes, including exact floating-point bits, phase/count
modifiers, colors, origin/heading, ordered sound dispatch and RNG consumption.
Other actors' observed RNG draws are supplied at each effect-update boundary, so
this is component evidence, not a whole-battle RNG comparison. The effect trace,
particle trace and casting trace match the untraced replay on all 74 applicable
watched fields at identical VI indices (610, 610 and 443 shared samples). No inputs
were retimed. Capture identity, controlled changes and trace hashes accompany the
fixture; reports use `local/battle-rewrite/lightning-only-*` and
`oracle-lightning-only-*`. The captures retain native-resolution video and silent
file-only audio; mesh drawing and audio playback have not been compared.

Current affected checks pass 170 battle, 42 game and 68 preparation tests, including
all twelve cold tests. Battle/game/oracle all-target Clippy, workspace formatting
and diff checks pass. This increment used only the script publication refresh;
targeted battle cooking also applies at integration checkpoints. Stage 2 was incomplete at this increment.

## Nurse stored-scene observations

A controlled original-game fixture places Raine in the opening formation, grants
Nurse, disables First Aid and starts with three injured allies. The captures record
native technique 237, a 310-update initial casting countdown and payment of 28 TP
at combat tick 316. The stored transition occupies ticks 316–375; resource activation
and primary-slot release occur at 376, then resident initialization at 377. The
age-120 callback runs at 498 and heals the party from `[100, 50, 150]` to
`[231, 118, 315]` for maximum HP `[328, 172, 413]`, leaving the enemy unchanged.
The checked-in `nurse-recovery.json` fixture now verifies this callback's relative
timing, roster order and HP changes through the maintained Nurse script.

The retained lifecycle differs from an ordinary resident: callback age 250 runs at
628, preserves slot occupancy and selects cleanup phase 2; cleanup runs at 629 and
releases the stored resources. The maintained Nurse script now calls
`retain_resident()` during initialization. Its regression compares healing and
the observed 627–629 age/phase/occupancy boundaries, including completion on 629.
The core never resumes active tasks during retirement and supports interruption
before that completion. Stored resource and visibility ownership remain pending.
`1F87C` changes
actor model visibility, consumed by `51914`; it is not a global actor-clock pause.
`1C40` and `3E42C` separately govern the stored transition and resident dispatch.
Do not model the whole Nurse scene as a paused battle.

Casting, resident and effect diagnostics match their untraced baseline on all 105
named battle fields at unchanged VI indices (981, 1,039 and 1,050 shared samples).
Source classifications, fixture mutations and observation identities are retained
under `local/battle-rewrite/nurse-*` and `oracle-nurse-*`. `3EB74` and `3E580` remain
partial at 47.9592% and 90.3722%; the other recorded lifecycle entries are
native/exact. Original instructions `3EE20–3EE68` and `3E628–3E99C` now cross-check
the selected spell range and effect/texture/model/outline/motion/action bindings.
The Nurse package is selected from `BTLmagic.dat` by the original spell table.

The production scene publisher now cooks that bounded package into
`battle/scenes/237.json`, preserving seven original effect programs, two scene
textures, four model instances and the four-phase action bundle. All four model
slots share a body and outline. Their distinct animation ranges contain identical
bytes, so existing content-addressed publication shares one sparse clip while
preserving separate bindings. Both discs produce identical descriptors; every
clip validates against its rig. Selected preparation uses the existing verified
dependency loader. Scene controller admission, the 60-update transition, model
visibility, scene effects, camera/tint and voices remain current Stage 2 work.
Existing ordinary casting preparation continues to reject the stored-scene flag.

The resource increment passed the original-disc Nurse test and thirteen cold
preparation tests, including changed/missing motion rejection while a previous
generation keeps cached bytes alive. Its scene/script refresh took 4.45 seconds
and updated 501 inventories; full cooking was not repeated. That resource evidence
remains pinned in `local/battle-rewrite/nurse-scene-evidence.json`.

A checkpoint at combat tick 300 now supplies focused transition motion and particle
traces. They agree with their untraced baseline on all 105 battle/presentation fields
at unchanged VI indices: 752 shared motion samples and 733 particle samples. Body
visits at ticks 316–375 hold the three non-owners while Raine continues advancing.
120 paused Lloyd/Colette chain visits preserve all segment words across 660 chains.
All 275 particle visits observed during that interval carry the late-group flag.

Models now retain the sampled local pose across held visits, including pending
bindings and the final blend sample. World transforms and hurt/weapon points still
recompose; secondary dynamics retain position and momentum. Petrification uses this
held path. The stored owner/transition selection remains pending. A 304-visit
original clock fixture and focused model regressions cover these distinct clocks.

Particle loading now preserves flag `0x100` as the late update group. Original
instructions `413F8–41434` confirm that selection despite `413B8` still being a
candidate in the manifest. `123C8` appends particles, whereas `12470` prepends effect
programs: group 7 runs newest effects first, then particles in creation order,
including particles just appended by those effects. Both normal and late birth,
retirement and ordering paths have current regressions. The cooked source already
contains the flag, so this increment requires no recook. The battle-wide transition
gate and unsupported particle attachments/controllers remain pending.

Current checks pass 175 battle tests, 70 preparation tests (all thirteen cold tests
included), and the original effect-bank test on both discs. Content/import/battle/
game all-target Clippy, workspace formatting and diff checks pass. Current source,
capture identities, replay comparisons and native checks are pinned in
`local/battle-rewrite/nurse-transition-evidence.json`. Stage 2 is incomplete.

## Stored transition ownership and maintained Nurse sequence

At the same decompilation revision, the core now owns two stored-scene slots.
The casting script moves to the end-of-update callback during its transition;
actor callbacks, contacts, ordinary effects/particles and residents hold. The
owner's model and late effects/particles continue. Activation binds the slot to
the released resident, which initializes on the next update. Caster completion
does not free that resident or scene. Pending cancellation frees its slot;
active cleanup restores actor drawing visibility without changing combat clocks.

The maintained `casting::stored` and `nurse::run` scripts reproduce the original
fixture's payment at 316, transition through 375, activation at 376, initialization
at 377, healing at 498 and cleanup at 629. Raine's sampled motion clocks match the
pinned Dolphin visits bit for bit. Model visits precede the combat-counter increment
in `1C40`, so their recorded tick is one below the corresponding callback tick;
the test uses that fixed source-derived registration. Additional tests exercise
menu pause, simultaneous slots, held timers, late particles, interruption, slot
reuse and invalid scene calls. Recovery before scene activation faults instead of
running a recovery continuation in the transition phase.

These are timing and ownership checks with synthetic model/effect resources.
Full Nurse preparation still rejects stored casting until its scene controllers,
attachments, camera/tint and voices are ready. In particular, the transition effect
needs the original profile-derived center attachment; the current script's effect
binding is not yet presentation acceptance. No new image or audio comparison is
claimed. The verified original captures are reused with fresh native test output.

Current checks pass 181 battle and 70 preparation tests, including all thirteen
cold loads, plus battle/game all-target Clippy, workspace formatting and diff checks.
The script-only publisher changed two files and refreshed 501 inventories in
4.02 seconds. It published unchanged `.sym` source and no executable output.
Evidence is pinned in `local/battle-rewrite/nurse-transition-owner-evidence.json`.

Particle preparation now admits the original `0x400` origin binding. The particle
retains its actor/projectile handle independently of the emitting effect and reads
that position at each visit in its own update group. Heading and local motion stay
independent. Missing origin bindings fault; expired projectile handles hold the
last position and never follow another object. Tests cover moving actors, normal
and late groups, effect cancellation, menu pause and expired projectile handles.

The checked-in `nurse-followed-particle.json` pins the first common-61 transition
particle from the existing original trace. All 36 visits match floating-point
motion, geometry, origin and signed color fields through inclusive age 35. This
component test supplies the observed center position; it does not establish the
profile-derived center calculation or the complete common-37 effect. The original
declaration also matches the production importer on both discs. `403F4` is exact;
original instructions confirm the binding in candidate `413B8` and partial
`418B4`. Their classifications remain explicit in the source audit.

Current attachment validation passes 183 battle tests, 71 preparation tests
(including thirteen cold loads), and the original effect-bank test on both discs.
Content/import/battle/game all-target Clippy, workspace formatting and diff checks
pass. No recook was needed: the original declaration already contains this flag.
Evidence is in `local/battle-rewrite/nurse-attachment-evidence.json`. Stage 2 remains
incomplete; profile-center/bone attachments, remaining scene controllers and full
Nurse presentation are still required.

## Sampled actor centers and transition attachments

At decompilation revision `384dd3889598f7d7608f0f644b2ff13e283131cf`, exact
`1B3F0` and `31C88` establish the actor center: scale the profile point, rotate by
heading, add the root, then retain that world position before actor callbacks and
movement. Profile publication and loading now preserve this point. The reusable
`show_centered` operation supplies it to the maintained stored-casting script and
to independently surviving particles. Root-following effects retain their existing
behavior. Petrification holds callbacks but still refreshes the center.

This also corrects the earlier transition-entry ordering assumption. Although
`11E8C` continues traversing the actor list, each later `31C88` visit immediately
returns once an earlier actor begins a stored transition. Those later actors hold
their centers and timers on the entry update. The initiating actor finishes its
visit. Activation releases the owner after the actor pass, so sampling resumes
on the following update.

The fresh original replay matches all 105 previously watched battle/presentation
fields at 750 identical VI indices. All 2,996 center observations across two Nurse
transitions match the source-derived ordering. `nurse-centers.json` pins the first
transition through cleanup; the maintained Nurse regression compares 1,320 center
readings bit for bit. It supplies observed roots/headings at the actor-update
boundary, so this is center/ownership evidence, not full movement or presentation
acceptance. Separate tests cover moving root and center attachments, horizontal
offsets, scale, overflow, petrification and transition holds.

Current checks pass 186 battle, 72 preparation (including thirteen cold loads),
two original-disc profile tests and four oracle observer tests. All-target Clippy,
workspace formatting and diff checks pass. The temporary publisher refreshed only
profiles, enemy model descriptors and scripts in 10.47 seconds; no full cook ran.
Current identities and results are in `local/battle-rewrite/nurse-center-evidence.json`.
Stage 2 remains incomplete: remaining Nurse controllers/modifiers, bone attachments
and full scene presentation still require implementation and validation.

Nurse's two common-63 billboard trails now prepare and execute through the same
particle controller and VM as other effects. Exact `403F4` separates their radius
speed from the size/position/angle steps used between drawn segments. Those steps
are preserved as geometry parameters instead of becoming temporal acceleration.
The original reverse-trail modifier uses reusable angular-velocity and segment-angle
operations. `78CE0` remains partial (59.5646%); its original instructions confirm
the segment parameters' consumers, without establishing native drawing fidelity.

`nurse-trails.json` pins both original instances, with 41 visits each through
inclusive age 40. All 82 native updates match origins, motion, geometry parameters,
signed colors, palettes, unchanged RNG and expiry. A separate test covers nonzero
segment offsets, emission scale and rejecting an incompatible modifier destination.
The original declaration and modifier match the production importer on both discs.
The complete common-37 transition still needs its culling modifier, and trail mesh
drawing remains pending. This increment requires no recook. The affected core,
74 preparation tests (all thirteen cold loads), original effect-bank test and
all-target Clippy pass; identities are in `local/battle-rewrite/nurse-trail-evidence.json`.

Common-37 transition preparation is now complete. Original flag modifier 21 sets
the back-face culling request through a semantic particle operation; unsupported
flag masks still fail preparation. Exact `3F0C0`, `47FA4` and the main executable's
`GXGeometry` unit establish the write and its rendering-state consumer. Original
declarations retain the same value, including particles that start with culling
enabled. The particle frame exposes the request; renderer fidelity is still pending.

The complete transition regression now compares all 16 original particle instances
and 491 updates through tick 417. This includes common-64's six repeated emissions,
the culling-modified common-61 instance, both trails, and common-66's ordinary-group
hold: it is emitted at 375 but first updates at 377 after scene activation. Exact
float bits, ages, geometry parameters, UVs, colors, palettes, headings, origins,
culling and expiry agree. Menu pause is covered separately. The emitting effect
finishes before its particle tails. The harness uses the reusable scene operations
with the observed 60-update transition; full maintained Nurse timing and center
sampling remain covered by their separate original fixtures.

Cold preparation now verifies the full original common bank and loads member 37.
No new cooked format or recook is needed. The older Fire Ball and Lightning
fixtures additionally retain culling observations recovered from their pinned raw
traces (104 and 366 updates); their other observations and original identities are
unchanged. Current evidence is in
`local/battle-rewrite/nurse-transition-effect-evidence.json`. Full stored Nurse
activation still waits for scene models/controllers, attachments, camera/tint,
voices and applicable image/audio comparisons. Stage 2 is not complete.

Final transition checks pass 186 battle, 17 content, 75 preparation tests (all
thirteen cold loads included), and the original effect-bank test on both discs.
Content/import/battle/game all-target Clippy, workspace formatting and diff checks
pass. The current source audit and fresh native outputs are pinned separately from
the reused original capture evidence.


Nurse model playback now uses the same owner-independent animation clock, retained
local pose and secondary-motion calculations as battle actors. Actor-specific
reactions, hurt points and weapon anchors remain in the actor model path. The
shared solver now preserves the original zero root on the first chain visit:
`80069088` initially seeds only the child joints from the authored pose, then checks
the root's distance after attraction and gravity. Nurse's distant initial placement
therefore resets the entire chain. Previously Rust initialized the root to its
sampled target too early, producing a 0.102-unit first-visit difference.

The checked-in `nurse-scene-models.json` and original `nurse.motion` pin three model
instances and 177 consecutive visits each, at original combat ticks 378–554. Fresh
native execution matches all 531 playback clocks and the five retained joint
positions per model. Twenty-one representative visits additionally compare all
54 bone matrices before and after secondary motion, including entry and wraps.
Maximum matrix error is 0.000184 and chain-position error is 0.000428, within the
unchanged 0.001 tolerance. The original capture's input/state/profile and 752-VI
replay comparison remain unchanged. Cold loading checks all four published model
slots, both body/outline layers, skeletons, motion bytes and solver parameters
against this fixture. No cooking or publication format change is required.

This is component playback evidence. Tests supply the observed model placement;
they do not yet establish scene emission, lifetime, camera or image fidelity.
`3FC24` remains partial (99.6177%); original instructions confirm the flag-0x20
binding and advancing/held modes. Exact `3F0C0` opcode 22 binds a particle-owned
animation buffer into the selected scene model. Its four model slots must remain
independent instances even though their physical resources are shared. Those
bindings and model-particle dispatch are the next integration work. Source and
validation identities are recorded in `local/battle-rewrite/nurse-model-evidence.json`.

This increment passes 187 battle, 17 content, 75 preparation (including all thirteen
cold loads), and three field secondary-motion tests: 282 total. Battle/content/game
all-target Clippy, workspace formatting and diff checks pass. Stage 2 remains
incomplete; this increment does not open the stored-casting preparation gate.


Nurse model emission and playback are now integrated. Maintained `nurse.sym`
emits scene member 1, then one model member per same-side roster slot with the
original heading offsets. Loading verifies all scene rigs and clips and translates
model modifier 22 and the byte selector into reusable operations in the shared VM.
Each active scene owns independent playback/secondary-motion state; geometry and
clips remain immutable shared resources. Cleanup invalidates that scene's effect
and particle handles. Rebinding a clip while held preserves the displayed pose
until the next advancing model visit.

The regression executes the maintained Nurse entry after the observed transition
boundary. It matches all 543 original model-particle visits at ticks 378–558,
including exact motion/color bits and inclusive age 180, followed by expiry at 559.
The same execution matches 531 captured animation frames and 21 complete poses of
54 bones. Maximum matrix error remains 0.00018310547, below the unchanged 0.001
tolerance. Original captures and inputs were reused; native output is fresh.
Separate source-derived tests cover four party members, overlapping scenes,
transition/menu holds, interruption, stale handles and invalid model bindings.
Those tests do not establish a four-member or overlapping Dolphin route.

Current checks pass 188 battle, 17 content and 78 preparation tests, including all
thirteen cold loads. Content/battle/game all-target Clippy, workspace formatting
and diff checks pass. The temporary publisher refreshed only maintained script
sources and integrity inventories in 4.13 seconds; one source file changed. Source,
fixture, capture and execution identities are pinned in
`local/battle-rewrite/nurse-model-integration-evidence.json`. Stage 2 remains
incomplete: recipient program 6, camera/tint, voices, full stored-cast loading and
applicable image/audio comparisons remain current work.


Nurse recipient program 6 now prepares from the existing scene publication. The
maintained recovery loop emits it before healing each eligible actor. The reusable
`show_on` operation selects that recipient as effect owner and target and uses its
sampled center, live heading and verified profile effect scale. This scale is now
part of prepared actor state and stays independent of body scale. Original sound
104 runs in the same effect timeline. No new particle interpreter or controller
was needed for its glow, rings and sparks.

`nurse-recipient-particles.json` pins all 51 emissions for the original three
recipients and all 2,091 particle visits through expiry. The component test supplies
each emission's observed RNG and placement, executes its original modifiers, and
compares exact RNG consumption and particle motion/color/geometry bits. Repeated
ring commands retain their shared 30-degree scratch increment. Actor movement is
outside this component replay: its actor stays at the supplied center, and the
moving-center attachment regression remains separate. The original trace contains
an external RNG advance at tick 503, so these comparisons do not claim uninterrupted
whole-battle RNG acceptance.

The maintained callback regression also checks the original emission schedule,
effect/sound/healing order, distinct recipients, profile scaling, capped healing,
dead/petrified exclusions, menu hold and particle tails. Interrupting the stored
scene removes its recipient-owned effects as well as its models, preventing later
repeat emissions. Core/content checks pass 188/17 tests; all 80 preparation tests
passed with thirteen cold loads, followed by the added scene-interruption test and
expanded emission-schedule regression. All-target Clippy, formatting and diff checks
pass. The script-only publication took 3.80 seconds after clearing two rebuildable
compiler caches to recover temporary disk space. The source and execution record
is `local/battle-rewrite/nurse-recipient-evidence.json`.

The stored-casting preparation gate remains closed. Nurse still needs tint,
camera/scene presentation, voices and applicable image/audio validation; Lloyd and
Lightning retain the other Stage 2 gaps in the inventory. The fresh silent presentation replay matches all 105 existing fields at 752
identical VI samples. It records actor/model tint, stage tint, camera impulse and
voice state through both original scenes, with finalized audio files.
`nurse-presentation.json` retains 331 ticks around the first scene;
`local/battle-rewrite/nurse-presentation-evidence.json` pins the capture and replay
comparison. This is original observation evidence, without native tint/camera/voice
acceptance yet. Actor tint changes at recovery tick 498; the model first samples it
at 499, then follows the RGB recovery one visit behind. Non-caster voice requests
can be rejected by a higher-priority active voice. These boundaries govern the
next implementation batch.


Nurse healing tint now runs from maintained source. The production publisher
retains the original twelve RGBA requests from `21938` in the existing tint table;
loading binds that verified data to `battle::ActorTints`. The recovery loop selects
entry zero after healing. Ordinary living actors converge toward the prepared
stage ambient RGB by one per visit, including during local hit-stop. Stored
transitions and menu pauses hold recovery. Model sampling precedes the callback,
so presentation receives a new tint on the next update.

The `2503C` ordinary RGB branch remains within a partial reconstruction (81.8514%);
original instructions `25A30..25ABC` independently confirm its arithmetic.
`5200C` retains partial-tier status despite a 100% reported match; original
`52184..5218C` confirms the actor-to-model copy. The maintained casting regression
compares 993 actor-color samples over ticks 300–630 and 744 model-color samples
over ticks 300–547, including the complete healing fade. Source-derived tests
cover non-neutral ambient values, tint-table bounds, no RNG consumption, local
hit-stop, menu and transition holds. Both prepared actor and initial model colors
use the supplied ambient value.

The full fixture also exposes a separate Lloyd overlay at ticks 588–589, after
healing color recovery has finished: actor RGB remains `404040`, while model RGB
becomes `303030`. This overlay path is still unaccepted; the tint comparison does
not claim it or enemy dimming, condition colors, death fades, EX overrides or
profile opacity. These remain destinations in the shared presentation/reaction
work. Current source/instruction identities are recorded in
`local/battle-rewrite/nurse-tint-source-audit.json`. Stage tint, camera, voices,
full stored-cast loading and image/audio acceptance remain current Stage 2 work.

Tint checks pass 190 battle tests, 17 content tests, 81 preparation tests including
all cold loads, and the original-table comparison against both discs. All-target
Clippy, formatting and diff checks pass. The targeted scripts/tints cook updated
two files and refreshed 501 inventories in 3.93 seconds. Current implementation,
fixture and execution identities are in
`local/battle-rewrite/nurse-tint-evidence.json`.


Nurse stage colors now execute through the same battle owner. Maintained casting
source requests the entry color on channel one for 60 updates; maintained Nurse
source requests channel zero for 220 updates. Both use the original `101010ff`
constant and an RGB approach step of four. The stage visit occurs after object groups,
before contacts and resident callbacks. Only the highest active timer advances,
and overlapping ordinary requests retain the longest duration and darkest RGB.
After expiry, present stage models approach their prepared default RGB by two per
visit. Immediate requests copy RGBA and preserve the original model-slot flag
clearing behavior. Actor and stage color state remain separate.

`45604` and `45514` are native/exact. `45DD8` remains partial-tier at 0%; the original
instructions `45DF0..45E8C` and `45EB8..460E4` independently establish timer priority
and color approach. The partial stored-entry caller `3EB74` was checked at its
original call site. The maintained full casting regression matches stage model
RGBA, both targets, both timers, active flags and approach steps at all 331
captured ticks. Tests additionally cover overlapping requests, expiry priority,
non-neutral defaults, bounds, interruption and the kernel's menu hold.
Prepared stage metadata and model presence are still supplied at this component
boundary; these checks do not establish complete stage resource or rendering
acceptance. Core tests pass 195 cases and preparation passes all 81 cases including
cold loads; all-target Clippy, formatting and diff checks pass. The two maintained
script changes published in 3.94 seconds, without a full recook. Evidence is in
`local/battle-rewrite/nurse-stage-color-evidence.json`.

The temporary source publisher now reads existing maintained script paths directly
from the workspace. Once built, `target/debug/dev-battle-cook scripts` refreshes
source and inventories without rebuilding the importer or using embedded content
as a fallback. Its unchanged-source verification took 1.66 seconds; adding script
paths or changing publisher code still requires rebuilding the temporary helper.

A further silent original replay, `oracle-nurse-camera-01`, matches all 157 prior
battle/presentation fields at 750 identical VI indices. The new `nurse-camera.json`
fixture pins 331 ticks of camera parameters and flags, output transforms, stage
metadata and actor overlay counters. The earlier `camera_impulse` observation names
refer to temporary minimum radius/pitch constraints, not screen shake. Camera
flag `0x80` holds Nurse's 205-update constraint timer through tick 400; its first
decrement is at 401 and it reaches zero at 605. The exact `FE9C` consumer gates the
clock and clears that flag when its eye interpolation finishes. Camera
implementation must preserve this cause rather than prescribe a fixed delay.
The new overlay counters also confirm Lloyd's separate gray flash follows an
enemy hit at 587 (HP 231 to 204), with two subsequent model samples. These remain
original observations, not native camera/contact-overlay acceptance. Capture,
input, profile, audio and comparison identities are pinned in
`local/battle-rewrite/nurse-camera-evidence.json`. Stage 2 remains active.

## Stored-scene camera framing and verified profile bounds

The battle owns manual/semi-auto camera framing, entry and return. Camera movement
runs before model sampling and actor dispatch; the resulting eye, focus, pitch,
yaw and radius are exposed in `BattleFrame::camera`. Maintained Nurse source requests
its 205-update radius/pitch bounds after the stage color request. Bounds merge by
maximum duration/radius/pitch and survive completion or interruption of the requester.
Scene entry and return hold the timer; eye interpolation completes the return.
Interrupted entry also returns to ordinary framing. Menu pause holds all camera state.

The maintained Nurse regression seeds the camera once from original tick 300 and
supplies the previously recorded actor movement at each component boundary. All
2,970 pose values (nine at each of 330 updates, ticks 301–630) match Dolphin float
bits exactly, as do the phase, constraint duration and stored bounds. No inputs or
observation clocks were shifted. The camera clears its return flag at tick 400,
decrements the constraint first at 401, expires it at 605 and subsequently restores
the ordinary pitch. This verifies the camera component with supplied actor movement;
it does not establish natural actor controllers, camera initialization or rendering.

The current decompilation remains `384dd3889598f7d7608f0f644b2ff13e283131cf`.
`FE9C` and `5305C` are exact native functions. Original instructions were checked for
partial `F1CC`/`EE14` and the focus selector. `EE14`'s reconstructed C incorrectly
reads profile offset `F0` as a float: original `F00C`/`F0D4` use a signed-16 GQR5
load, whose configuration is established at DOL `80070D44..80070D4C`.
The production importer now publishes the signed radius halfword, unsigned yaw
adjustment and framing category; verified party/enemy preparation applies them.
The adjacent halfword remains independent source storage. No compatibility path is
provided for the previous cooked profiles.

Only scripts and profiles were refreshed using the ignored development helper:
257 publications checked, 253 changed files and 501 integrity inventories refreshed.
The publication phase took 10.53 seconds after its one-job rebuild; the full-cook
coverage report was preserved. Source-only adjustment uses the already-built helper.
Source audit and instruction identities are in
`local/battle-rewrite/nurse-camera-source-audit.json`; native evidence and test logs
use the same `nurse-camera-` prefix.

Validation passes 199 battle tests, 17 content tests, all 81 preparation tests
(including 13 cold cases), and three profile/rig checks using both original discs: 300
distinct tests. All-target Clippy for battle/content/game/import, workspace formatting
and diff checks pass. Native source/test identities and exact comparison counts are
in `local/battle-rewrite/nurse-camera-native-evidence.json`.

Automatic/dead-leader framing, target changes and their countdown, command-menu
framing, secondary bounds/focus overrides, actor-height tracking, arena 43's angle
restriction, projection and framebuffer comparisons remain unaccepted. Preparation
currently rejects an automatic camera leader. Nurse still requires voices, complete
stored-cast loading and presentation; the other Lloyd/Lightning gaps above remain.
Stage 2 is active and incomplete.


## Stage 2 integration checkpoint

The three representative routes now prepare and run together through the same
combat owner and gameplay VM. Lloyd uses verified body and Wooden Blade bindings,
normal command/hit windows, contact damage, TP recovery and action recovery.
Genis Lightning uses original casting parameters, projectile 4, effect 28 and
independent particle tails. Raine Nurse uses verified stored casting, body/scene
models, transition and recipient effects, three-target recovery, tint and camera
requests, and independent scene cleanup. Maintained source owns every sequence.
Cold tests also cover insufficient TP, failed candidate preparation, before/after
release interruption, menu holds and deterministic replay. The target placement
in the cold attack test is synthetic; it does not establish an encounter controller.

The production cooker publishes original voice storage flags and duration values
from BTLusual members 11/12, profile voice bases, and the ten original technique
voice lists. Verified loading selects cue or stream resources and actor-specific
lines before activation. The core queues requests by actor priority, dispatches
from the common actor visit, and accepts playback completion handles. Late
completion cannot stop a replacement. Tasks may end while queued/playing voices
survive; faults dispose of the battle's voice state.

Shared casting source schedules chants from the pre-decrement remaining clock,
using the original duration plus 20, and remembers attempted counts during holds.
The fallback depends on actual audio completion. Nurse retains its original
self-target chant override. Lloyd's neutral and finisher request absolute lines
1 and 3 at age four. These operations neither compile nor discover resources in
active combat. Audio mixing, spatial pan/volume and device playback remain the
presentation host's responsibility.

At decompilation revision `384dd3889598f7d7608f0f644b2ff13e283131cf`, `71674`
is a candidate with no native match tier, so original instructions establish the
voice dispatcher. The request, voice lookup and casting routines used here have
native exact status. The full Nurse arbitration replay preserves one initial
state per actor and matches 1,280 subsequent original calls, including seven plays
and two replacements. Only audio completion is supplied at the component boundary.
The captured companion with voice base 121 is Colette (character 2), not Genis.

Two further isolated silent Dolphin captures establish Nurse's chant at tick 234
(remaining 82) and release at 316, and Lightning's chant at 382 (remaining 57) and
release at 439. Both release voices replace the chant. Their ordinary replay
comparisons have no differences at identical VI indices: 747 samples across 103
fields for Nurse and 610 across 72 for Lightning. Native tests execute the
maintained chant helper and match those request/dispatch boundaries without
retiming. The fixtures retain profile, disc, input, state and capture identities.

Ordinary contacts do not create a generic local hit-stop. All 32 retained original
contact observations keep both local counters at zero. Original instructions gate
the two-update contact pause on an Over Limit target; its leader/global and
nonleader/model-track branches remain Stage 4 work. Likewise, the captured normal
attack movement visits have root movement disabled. Other root-driven actions,
full controls/AI, field entry/return, HUD, framebuffer and recorded-audio fidelity
remain assigned to their later stages.

Voice acceptance here covers the immediate slot with the priority pairs used by
these routes. The deferred slot, unequal request/playback priorities, character
voice counters and special centered/spatial audio branches remain shared-mechanics
and presentation work. Complete arte or natural encounter fidelity is not implied.

Stage 2's representative-core exit is satisfied. Current validation passes:

- 907 workspace tests; 221 asset/oracle-dependent tests remain ignored in that run.
- All 92 battle-preparation tests, including the 16 cold original-asset cases.
- Four profile/voice importer checks, including byte round trips on both discs.
- Eight Python oracle tests, workspace all-target Clippy with warnings denied,
  Rust formatting and maintained battle-script formatting.

The partial cooker refreshes source publications and both field-declaration and
preload hashes. An isolated smoke check verifies successful refresh and rejection
of stale metadata without mutation. The final seven-publication check took 2.50
seconds with no changed files. Battle development uses these targeted production
publishers at integration checkpoints; full cooking is outside this workflow.

The mistakenly started full cook was interrupted and is not acceptance evidence.
Its unfinished metadata was repaired through the existing production finalizer,
with zero asset conversions and preserved backups. No replacement full-cook report
was invented. The source-index recovery report records its historical provenance
and limitations. Current tests use verified runtime dependency inventories.
The two untouched legacy named field aliases remain outside the canonical map
catalogue and are recorded separately by the integrity audit.
The final audit checks 49,549 referenced files and confirms zero missing files or
integrity mismatches across all 501 canonical maps and their declared dependencies.

Logs and identities are retained under `local/battle-rewrite/stage2-*`, including
`stage2-integration-evidence.json`. Stage 3 is next: real field-event entry,
controls, encounter behavior, presentation, outcomes and exactly-once field return.
