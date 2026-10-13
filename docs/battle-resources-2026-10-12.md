# Battle resource recovery

The latest visibility batch published **53 selected files (10,548,765 bytes)**
to `local/all-assets`: 51 arena metadata additions plus preload/provenance updates.
Its [receipt](../local/battle-visibility-completion-20261012/publication.json) retains
backups, hash guards and 874 unchanged child inventory identities. The default
remains at 86 arenas and 22 deferred rows. Arena 26's bounded rendering check used
retained capture `7b79676f`; the separate [active-runtime receipt](../local/battle-visibility-completion-20261012/active-runtime.json)
records runtime `408d8952` and Tempest capture `49594e5c`. Vidarr completion remains
unaccepted.
Earlier publications and their distinct capture identities remain documented below.

Parallel, bounded Dolphin observations and selected native captures support these
changes. No full recook, workspace validation or commit is part of this recovery.
The checks establish their recorded behavior, not complete battle fidelity through
Triet Ruins. The [formation coverage inventory](../local/battle-resource-next-20261012/formation-coverage.md)
distinguishes source entry footage, native encounter checks, isolated components
and unresolved behavior across the retained 82 formation/arena pairs.

## Initial published batch

The earlier publication added 315 selected files (28,410,738 bytes), with all nine
selected native cases passing. Its backups and evidence remain under
`local/battle-resource-closure-20261012/`. The following table records that batch's
scope; later corrections are described below.

| Area | Result |
| --- | --- |
| Normal attacks | Restored per-character directional continuation tables and Colette's three authored weapon-flight profiles. Removed the shared substituted values. |
| Tiger Blade | Uses existing movement, contact, effect, landing, and recovery events. All 21 sampled poses/heights match the retained source observations; horizontal error is below 0.00011. Both strikes connect in the contact fixture. |
| First Aid | Restored the native recovery recipe and authored healing classification. The native fixture pays 8 TP once and restores 123 of 413 maximum HP at release + 10. Timing and percentage come from our saved implementation; source recovery timing remains unconfirmed. |
| Giant Bee | Added measured partial-body animation using the existing model player. Contact rows now wait for the preceding window and keep their original expiry. The native rerun passes; fixed body bones and moving appendage bones demonstrate independent playback. |
| Delayed particles | Size changes address the intended emission's existing particle handle. This does not complete enemy 71: its attachment transform and a scalar destination remain unresolved. |
| Weapons | Primary and independent attachment layers retain their authored signed U/V scrolling. Frame and sequence channels remain unsupported. |
| Ordinary victories | All 45 timelines prepare and complete the focused playback check. Colette's two finite celebrations return to idle. Group performances are a separate open question. |
| Arenas | Added 17 selected arenas, bringing the candidate to 85 of 86. Arena 3's texture binding remains unresolved. Full cooking records that typed unsupported binding without swallowing unrelated decode errors. |
| Stored skills | Extracted Grand Fall and Falcon's Crest, including their models, effects, cut-in and attachment metadata. Native controllers and unresolved contacts remain open; extraction alone does not complete the archive. |

### Initial evidence and limits

- `checks.json` records 111 passing focused tests in 14 groups. Two further
  focused checks cover all ordinary victory assets and the typed arena failure.
  All 15 affected enemy-importer tests pass after the contact correction.
- `native-validation.json` records Tiger Blade timing/contact, First Aid,
  weapon scrolling, arenas 16/73/18, victory/field return, and Giant Bee.
  All nine cases have no diagnostics or unprepared resource reads.
- The three arena captures show prepared scenery and textures. The result fixture
  returns to a rendered field with control. Neither is a claim of pixel parity.
- Weapon screenshots cover Colette's visible layers. Genis is outside the selected
  framing; source measurements and the focused runtime test establish UV rates.
- `resource_coverage/` inspects selected metadata. Of 251 enemy definitions,
  94 have no recursive action rejection and 87 also have no policy rejection.
  Eight route enemy identities retain explicit metadata blockers. Giant Bee's
  initial runtime failure and passing rerun are both retained.
- Completing both discs' ordinary-victory resources reduces the default library's
  deferred rows from 30 to 28. The remaining rows stay open. Alternative REL
  data layouts are documented rather than silently treated as consumed.

## Second published batch

This publication installed **311 files (16,496,844 bytes)**, bringing the default
library to 86 arenas and 24 deferred rows. Its runtime identity begins `b1cabcc9`;
backups and evidence remain under `local/battle-resource-next-20261012/` and its
[publication record](../local/battle-resource-next-20261012/publication.json).
Parallel component checks cover weapon
atlas playback, arena 3, Wind Blade, Tempest, enemy 71, airborne hit waits and
deadline extensions. The weapon atlas, Wind Blade, arena and field-return checks
pass within their documented scopes. Special victory group 43 passes the focused
selection, voice and completion fixture.

The publication also restores special victory selection and Botta's measured
low-health walking override. Focused gameplay and importer checks pass.
Action JSON is refreshed from retained inputs to avoid stale contact
metadata, without rebuilding enemy artwork.

Native comparisons exposed additional integration defects: Tempest retained its
landing pose during recovery and selected the ordinary weapon-trail material;
enemy 71 lacked its independent flame skeleton; and duplicate enemy actors reused
attacks bound to the first actor's carried weapon. The bounded reruns pass for
these corrections. A matched trail-material comparison established alpha blending
for Tempest's highlighted trail; its angular geometry remains unresolved.

Stalagmite now imports its three distinct contact rules, including an explicit
sound that equals the existing Earth-hit sound. Botta's projectile recoil
overrides lower into the existing action hit data. No new serialized contact
field is needed. The selected refresh took 2.43 seconds and reused enemy artwork.
The updated importer, sound-equivalence and trail-material checks pass.
Stalagmite's bounded capture confirms the three rock formations, contact rules,
particle edits and sounds. It also exposed a missing spell banner and foreground
scenery obscuring actors. Rendered checks now confirm the scenery fade and the
restored banner's 315-update hold and 16-update fade. The native title disappears
at release+331; the retained source title disappears at release+332. This
one-frame display-phase difference remains explicit; exact boundary parity is
not claimed. The banner check covers one arena29 stored-spell fixture, and the
scenery check covers in-frame actors at the sampled camera positions.
The party-capacity fixture and existing menu navigation/control checks now pass.
The fixture's earlier failure requested an attack during the entry camera's
settling hold; the corrected fixture waits for the advancing combat clock.

Vidarr's opening and rescue reuse the existing event runtime with battle-owned
actors, camera, dialogue and audio. The existing fourth party member stays absent
until the rescue activates it; saved formation and actor identities are retained.
Enemy 98's guarding-target and effective-hit follow-up conditions are restored.
The consolidated check pass contains 35 focused tests, including the normal event
publisher and main's two dialogue-positioning tests. Both actual cooked event
scripts complete in the CPU replay. The final Vidarr native capture, attempt 05,
completes 2,856 updates with 20 reviewed images, no application diagnostics,
unprepared reads or renderer errors. All five spoken samples are identified in
the mixed audio. Opening/rescue lifecycle, reserve activation, spent TP retention,
formation restoration, dialogue and the sampled rendering pass. Combat resumes
after rescue and advances to tick 660.

That Vidarr result covers the exact-route battle fixture, not the preceding field
handoff, boss victory or return to the field. Source/event animation phases are
not aligned exactly. Party HP was not damaged in this fixture, so damaged-HP and
dead-member rescue behavior remain untested. Audio sample presence and script
completion do not establish auditory quality or exact source-duration parity.
The [accepted assessment](../local/battle-resource-next-20261012/boss-controllers/native-fixture/native-assessment.json)
retains these limits and the earlier failed attempts.

Bowman's selected Heavenly Nails fixture passes admission, three-arrow births at
relative ticks 0/2/4, visible models/shadows and launch/name cues. The matched
source contrast finds no flight-output change when the concrete point-history
allocation is removed. Neither triple fixture hits a target: this establishes
flight behavior, not triple-contact damage, ordinary follow-up selection,
whole-frame parity or encounter completion.

## Published actions batch

The [22-file publication](../local/battle-resource-actions-20261012/publication.json)
installed 10,497,115 bytes with runtime `2803fab1`, adding Nurse, Demon Fang and Exbelua data,
audio and artwork corrections. Earlier failed attempts remain in the evidence;
publication does not replace their assessments or transfer every earlier capture
to the latest binary.

[Nurse attempt 02](../local/battle-resource-actions-20261012/nurse/native/native-validation.json)
passes the bounded four-recipient component on `0ffb0d00`: models first advance at
release +62, healing occurs at +182, and the spell completes at +313. Raine pays
28 TP once; each living recipient gains 40% of maximum HP. The unwanted helper
meshes are gone, and sampled model positions match within 0.003 world units.
Camera/darkening, exact glow appearance, audible playback, recovery voices and
acquisition remain outside acceptance. The failed first attempt is preserved.

[Demon Fang's combined assessment](../local/battle-resource-next-20261012/demon-fang-native/combined-assessment.json)
accepts release/recovery and functioning impact artwork using two `8b326dbe`
recordings. The original cast samples sound at input +8, projectile release +16,
recovery start +47 and resumed idle +58; its contact is guarded for 13 damage,
so its original assessment remains partial. The separately controlled retry
disables only the two scorpions' guard-chance fields and observes an unguarded
54-damage contact with the authored ground ring and vertical light. This combined
result does not establish source damage, ordinary enemy AI, chain-input timing,
trail expiry or whole-frame parity.

[Exbelua attempt 05](../local/battle-resource-actions-20261012/exbelua/native-assessment.json)
passes entry, one ordinary attack, sampled arena/model/effect rendering, and the
controlled below-half-HP Insane Cell component on `0ffb0d00`. The latter preserves
action selection, changes initial HP only, and moves Lloyd into range: five
body-center contacts reduce HP 328→269→212→152→95→36, followed by recovery and
idle. The attachment-255, palette and shared-texture startup failures are
superseded. At that publication, pulse spacing remained open: source offsets were
46/62/78/94/110, while the native recipe uses 46/61/76/91/106. Sparse captures do
not resolve that discrepancy; the subsequent Triet component below does. Source damage, camera, remaining actions, full
fight and field return are not accepted.

The two BTLskit deferred rows are now classified as **deliberately excluded from
the native result sequence**. Native results use ordinary staged poses and the
54 prepared group descriptors/voices; the unused template hash/bounds dependency
is removed. A positively identified group-43 result and its
[template-rate contrast](../local/battle-resource-next-20261012/group-victory/template-rate-ab/report.md)
support that design for the measured case. This is not template execution,
universal proof of unused source fields, or acceptance of all 54 performances.

Separate retained follow-ups refine the route gaps. [Fire Warrior](../local/battle-resource-actions-20261012/enemy71-contact/REPORT.md)
lands a sword contact then released-burst contact, with independent owners and
recovery; exact source timing/damage remains unaccepted. [Triet 141/142](../local/battle-resource-next-20261012/triet-formations/REPORT.md)
shows natural Bird hits and Element Fire Ball, but missing Bird passive flame,
different Element haze and no Pot attacks. [Bowman's active-window check](../local/battle-resource-actions-20261012/bowman-followup/active-window/REPORT.md)
finds no overlap at the two sampled kick poses and a facing mismatch; it does not
prove an effective hit or justify enlarging contacts. These captures use
`8b326dbe`, not the latest publication binary.

## Published Triet component batch

This [publication](../local/battle-triet-completion-20261012/publication.json)
installed 15 files / 4,312,385 bytes and verified 131 dependencies. Its runtime
was `91547880`, with component captures on `999dc8a8`.

The [martial report](../local/battle-triet-completion-20261012/martial/REPORT.md)
and [Sonic/Beast contact report](../local/battle-triet-completion-20261012/sonic-beast/near/REPORT.md)
record actual native hits: Kratos Demon Fang deals 66 damage; Colette Ray Thrust
deals 49; Sonic Thrust hits at input+18 for 79; Beast hits at +16 for 47 and its
second phase damages both targets, with knockdown/stun. Release, TP payment and
recovery have bounded evidence. Sonic's close-contact tail is interrupted by an
enemy; its earlier distant component supplies uninterrupted recovery evidence.
Controlled placement/idle/guard fixtures do not establish source damage, RNG,
repeat-hit cooldown or natural acquisition. Beast's dark halo and rare voice remain open.

[Exbelua's timing check](../local/battle-triet-completion-20261012/exbelua/REPORT.md)
now brackets all five exact processed action offsets **46/62/78/94/110**, matching
the retained source timing. HP falls 328→269→212→152→95→36; recovery starts at
combat 266/action age 166 with 45 updates remaining. Contact duration, source
damage, other actions, boss completion and field return remain outside this check.

The [stored-spell camera assessment](../local/battle-triet-completion-20261012/camera/assessment.json)
accepts caster framing and return relative to the native release placement.
At this stage scenery dimming and the late Nurse release swirl were missing;
the subsequent prelude check below supersedes that failure. Internal shot
retirement, concurrent casts and Eruption's ordinary pitch remain unaccepted.

[Aura captures](../local/battle-triet-completion-20261012/auras/REPORT.md) show
Fire Bird's moving flame, independent duplicate owners, and Element's core/haze/ring.
They exposed body scale 0.9 versus effect scale 1.0. The subsequent scale and
renewal checks below supersede that defect without accepting every aura lifecycle.
One Brown Pot now performs its rock action and returns to approach/idle; exact
shot spacing, contact and all variants remain unobserved.
The later [Bird/Pot audit](../local/battle-triet-completion-20261012/auras/next-pass/REPORT.md)
identifies the observed action as the long volley, with four persistent rocks
and eventual cleanup. Melee, close volley and hit/poison outcomes are unobserved,
not absent native policies. No additional Bird/Pot implementation defect was isolated.

## Published cumulative arte batch

This [publication](../local/battle-arte-completion-20261012/publication.json) installed
23 files / 11,053,160 bytes and verified 326 dependencies, merging resident changes
with Pow Hammer and Angel Feathers scene 267. Runtime `af40c469` and captures
`cebb2892` remain distinct; Aqua/Icicle and Spider reused `9540f72f` evidence.
Source provenance records the selected scene on both discs; partial
archive coverage stays deferred. Six selected sounds and eleven voices are listed
in the receipt; no full recook or broad validation was used.

| Component | Accepted scope and remaining limit |
| --- | --- |
| [Raine support](../local/battle-arte-completion-20261012/support/REPORT.md) | Recover cures live poison; Sharpness/Barrier apply magnitude 10 buffs for 1800 updates; Charge adds 15 TP at release+141. Each pays once, dispatches 35 updates after the paid pose begins, draws its artwork and cleans up. The renderer allocation failure is fixed. Source cure timing and audible parity remain open; the later seal batch addresses stat artwork. |
| [Aqua Edge / Icicle](../local/battle-resident-completion-20261012/water-ice/REPORT.md) | Living-target contacts, authored motion and cleanup pass in retained resident captures. The short chant follow-up confirms looping Genis 32 followed by paid Cast 12 and the unchanged 35-update release interval; it does not repeat the spell lifecycles. |
| [Plain Pow Hammer](../local/battle-resource-next-20261012/colette-early-artes/pow/native/findings.json) | One 8 TP payment, visible release, 55-damage contact, rebound, changing model orientation, floor-child effects and retirement pass. Ice/Toss alternatives and source contact/damage parity are unverified. |
| [Angel Feathers 56](../local/battle-arte-completion-20261012/angel-feathers/REPORT.md) | The first-seal arte now has a native scene 267 consumer: one 20 TP payment, three feather births/contacts/ground effects and cleanup pass. Exact source trajectories/contact timing and damage are not matched. |
| [Spider 134 reception](../local/battle-resource-next-20261012/spider134/native-reception/native-result/assessment.json) | Contact timing/counts pass at offsets 2/8. Damage remains 18/23 versus source 7/20, with different reactions. The [damage audit](../local/battle-resource-next-20261012/spider134/native-reception/damage-audit.md) leaves reference live mitigation unclassified; no formula defect is isolated. Definitions 135/139 remain withheld. |
| [Element aura renewal](../local/battle-triet-completion-20261012/auras/renewal-fixed/semantic-assessment.json) | Corrected Y 18/Y 45 scale and continuous three-particle sets pass at both former gaps. No repeated Bird/Pot action sweep or general lifecycle claim. |
| [Stored prelude](../local/battle-arte-completion-20261012/stored-prelude/assessment.json) | Common 37 restores the persistent white swirl. Matched-camera pixels confirm scenery-only dimming through transition 60, leaving opaque HUD/actor regions unchanged. Active colors/restoration are addressed below; exact swirl geometry/phase/luminance remain open. |

The [formation-field audit](../local/battle-arte-completion-20261012/formation-fields/REPORT.md)
finds all 1,000 authored position modes already represented correctly: 935 automatic,
65 authored, with zero active-coordinate mismatches. No extra flag or runtime code
is needed. Pool byte 28 remains unknown.

The separate [Ring learning observation](../local/battle-arte-completion-20261012/ring-learning/REPORT.md)
confirms ordinary Ring Whirlwind 45 acquisition at level 12 with first-seal Colette
state 1. Ring's corrected gate/action and Item Thief's binding were absent from
this arte publication; the subsequent seal batch supplies them.

## Published seal batch

This [receipt](../local/battle-seal-completion-20261012/publication.json) records
263 files / 20,559,141 bytes and 5,249 dependencies; the required Monster metadata
refresh accounts for 252 files. Runtime `168a08db` and capture `9a02d481` remain
the identities of this batch.

The [review](../local/battle-seal-completion-20261012/review.json) accepts ten
changed-feature cases and five focused regressions, with two selected cooks.
The initial missing-sound failures and exact first candidate remain archived;
resolved cue 736 replaces the incorrectly selected raw cue 235. Stalagmite's
passing first-candidate capture is reused because the correction changes only
audio metadata/preload identities. Temporary importer/capture registrations
were removed. No full recook or full validation ran.

| Component | Accepted scope and remaining limit |
| --- | --- |
| [Ring Whirlwind](../local/battle-seal-completion-20261012/ring/attempt02/REPORT.md) | Pays 11 TP once; motion 60, hits at input +24/+30/+36, paired ring artwork, recovery +81 and completion +92 pass. Supplementary motion provenance is retained. Natural native learning, exact source pose/damage and trail boundaries are outside this capture. |
| [Item Thief](../local/battle-seal-completion-20261012/item-thief/RESULTS.md) | Controlled success/failure/full-stack cases pay 10 TP and change active item inventory 0→1/0→0/20→20, with the corresponding poses, notices, voices and recovery. Natural probability, bonuses, depletion and persistent save commitment remain outside capture acceptance. |
| [Stat feedback](../local/battle-arte-completion-20261012/support-ui/native-attempt02/REPORT.md) | Sharpness/Barrier restore outlined italic two-line labels, the authored gradient, fist/shield and increase icons. The arrow is still static; exact anchoring, timing and simultaneous buffs remain unverified. |
| [Nurse color](../local/battle-seal-completion-20261012/nurse-color/native-validation.json) | Sampled scenery dims to approximately one quarter of prior same-camera brightness, then restores while the spell action remains live. Actors/HUD remain bright; every fade update and concurrent/interrupted ownership are untested. |
| [Angel color](../local/battle-seal-completion-20261012/angel-color/native-result.json) | Sampled scenery reaches black while actors/HUD/effects remain visible, then restores. The black target is based on source images; no authored scene 267 color field was invented. Exact source fade boundaries remain unproved. |
| [Eruption](../local/battle-seal-completion-20261012/eruption/native-validation.json) / [Stalagmite](../local/battle-seal-completion-20261012/stalagmite/native-validation.json) | Typed tint cues, visible scenery darkening/restoration and exact action completion at release +273/+308 pass. No live renderer RGB diagnostic or repeated damage/full-fight acceptance is claimed. |

The separate [Vidarr tail attempt](../local/battle-seal-completion-20261012/vidarr-completion/attempt02/native-assessment.json)
did not reach Results within 5,000 updates. Result and field-return images are
absent; completion remains unaccepted. The earlier opening/rescue result is
unchanged, and this batch introduces no new Vidarr source change.

## Published visibility metadata and Tempest runtime

The visibility publication restores missing authored `scenery_fade` in 51 arena
descriptors, preserving every preexisting descriptor byte and all artwork. It
also repairs three missing stage provenance links without closing archive coverage.
The [arena 26 repeat](../local/battle-visibility-completion-20261012/vidarr-visibility/native-assessment.json)
shows fighters again at diagnostic +120/+600. Both unobstructed prefix images
and the entire compressed/decompressed simulation recording match the prior run.
The other 50 arenas have metadata/identity review, not new visual acceptance.
The later [terminal diagnostic](../local/battle-seal-completion-20261012/vidarr-completion/attempt04-terminal/native-assessment.json)
shows the Results cap ended during ongoing combat after Lloyd and Colette died.
Boss HP reaches 2,903/6,000, below the rescue floor, with the event absent and clock
running. This excludes the suspected event/floor hold; it does not establish an
AI cause. Victory and the actual Martel handoff remain unobserved scope.

The newer runtime separately includes the [Tempest faceting correction](../local/battle-seal-completion-20261012/tempest-geometry/native/REPORT.md).
Four selected frames show a rounded crescent without new visible spikes or loops,
with cleanup at +110. Recorded gameplay, poses, RNG and existing camera values
match prior evidence; the capture uses the preceding asset publication. Source
trail history/extent, Remove Blur and unseen curve overshoot remain unestablished.
The active-runtime receipt keeps this `49594e5c` capture separate from the older
Vidarr binary. No art/audio recook or additional full validation ran.

## Remaining work

- Exbelua's remaining actions, contact durations, source damage/camera and
  boss completion/field return remain open after the accepted component.
- Spider's reference live damage inputs and accepted-hit classification remain
  unknown. Contact timing/counts pass; neither a formula defect nor RNG alone is
  established as the cause. Definitions 135/139 remain outside retained rosters.
- Keep Bird phase/death/contact comparisons and Pot's unobserved melee, close
  volley and hit/poison outcomes explicit. The retained audit found no additional
  policy defect. Bowman's facing/effective-hit gap remains open.
- Keep Ring/Item Thief's natural-acquisition, source-parity and unmeasured policy
  limits explicit; their native bindings and bounded components are published.
- Concurrent/interrupted scene colors, exact effect geometry, moving stat arrows,
  label anchoring and simultaneous buffs remain open after the sampled corrections.
- Nurse's presentation/audio limits and Demon Fang's unmeasured timing/damage
  limits remain explicit. Other group performances are not accepted by group 43;
  restoring the deliberately excluded BTLskit interpreter is not a pending task.
- Vidarr victory and the actual Martel handoff remain unobserved; the bounded
  Results wait did not demonstrate a lifecycle defect. Tempest source history/extent
  and Remove Blur, plus the remaining
  lighting/effect, damage, action and boss-return gaps remain open; Stalagmite
  retirement is accepted at +308.

The 22 deferred resource rows remain part of the active goal. Resource counts and
passing components do not establish complete encounter or route fidelity.

Workers reuse saved implementation and observations before starting another probe.
Physical controller testing remains the agreed manual follow-up.
