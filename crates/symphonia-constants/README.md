# Symphonia constants

This leaf crate owns known numeric game identities. One `ids!` declaration supplies
an explicit `#[repr(...)]` enum, iteration, checked numeric conversion, symbolic
parsing/printing and optional initial resource names. It depends only on optional
Serde support, not the importer, runtime or script compiler.

```rust
use symphonia_constants::{FieldId, ItemId};
assert_eq!(ItemId::MemoryGem.raw(), 491);
assert_eq!(ItemId::try_from(491), Ok(ItemId::MemoryGem));
assert_eq!(ItemId::MemoryGem.to_string(), "MemoryGem");
assert_eq!(FieldId::Iselia0332.resource_name(), Some("iselia-0332"));
```

`resonance-script-content` generates `.sym` definitions from `KnownId` during
cooking and for development checks. Add or correct IDs here, not in generated
script files or parallel importer name tables. Derived subsets such as playable
party characters and caravan stops reference the original enum's discriminants.
Use Rust documentation comments (`///`) on enums and members for reviewed notes
that modders should see. `KnownId` retains them and the cooker publishes them as
ordinary comments beside the corresponding `.sym` declarations. Story notes
include original writer/reader context, shared-bit limitations and the remaining
unidentified operands; adding documentation does not declare a new identity.

Technique, recipe, EX skill, packed title, skit, portrait and shared model/motion
catalogs also supply the importer's publication names. `FieldModelId` and
`FieldAnimationId` preserve distinct resource domains; small field-owned IDs and
loaded handles require the scene's context. Motion resource labels identify the
native clip, without guessing its behavior. Scenario selectors include signed
sentinels; `KnownId::script_value()` preserves their sign in generated definitions.
`FieldOverlayId` separately identifies the shared radial-burst and ornate-ring
images. Its publication names also drive overlay cooking, scene bindings and
shared texture banks. `scenario::WorldTransferId` names the 3000/3001 overworld
transfer encodings; both take the world-service branch and do not distinguish
Sylvarant from Tethealla. Physical field IDs remain in `fields::FieldId`.
Sound controls are separate from cue identities: `SoundControl::StopSlot` (-1)
names only `play_sound` calls with a known, non-default slot. The default 255 or
an unknown slot does not prove that branch, and `play_sound_simple` uses a
different handler. `SoundSlotOperation` names the volume/pan/stop operations of
`configure_sound`; unrecognized operations, including the retained 255 calls,
remain numeric. Equal values in music, sound controls and sound cues keep their
own domains.

`FormationId` names all 1000 original `BTLusual.dat` formation requests from their
enemy groups and multiplicities. Reused compositions keep distinct IDs through
`VariantNNNN` suffixes; matching enemy groups do not imply matching positions or
battle settings. The native-table audit reconstructs every name from the original
enemy resources and actor rows. Formation requests and weighted encounter pools
remain separate domains, and these enum names do not prescribe editable asset
paths or quest progress.

`BattleOutcome` names the native escape, victory and defeat result codes. The
runtime's outcome representation references these discriminants. Script word
`0x24` is shared with other services; its comparisons gain outcome names only
when control flow proves a battle origin, including copies of the result.
Aliasing writes, arithmetic, unrelated calls and external task entries discard
that proof. The native audit verifies the original result handoff and writers.

`TriggerKind` names touch (1), automatic (2) and confirmed (3) trigger records
queried by `trigger_exists`. The runtime shares these discriminants. `EventKind`
remains the separate event-registry namespace: a confirmed trigger has record
type 3 and uses event registry 2. The original query and constructor instructions
are audited against the executable.

`DialogueFlag::Yellow` selects native fixed frame tint `E8C800FF`, after Green
and Red precedence. `TimedClose` selects the native 300-count message-end close
timer, subject to the parser's persistent, automatic-page and choice branches.
The original executable audit verifies both flag consumers and the color data.

`ActorProperty::EmitterParameter0` through `EmitterParameter9` identify the ten
recipe-dependent constructor slots of `create_effect_emitter`. Native property
handlers access these only for emitter actors. Slots 0..3 are signed halfwords;
slots 4..9 are floats whose scalar writes first truncate to unsigned 16 bits,
unlike their signed constructor arguments. Names identify the parameter slot
without imposing one recipe's meaning on the other recipes.
`EmitterRecipe` separately names the constructor's renderer selector, such as
`Fire`, `Smoke`, `Rain`, `Portal` and `ExpandingColumn`. Names follow implemented
emitter behaviors. This domain is distinct from cooking recipes, sound cues and
the billboard sprites emitted by a recipe. Only reviewed observed values are
declared; unknown and mod-added selectors remain open numeric values.

`OverlayActorId::LocationCaption` names the special `create_overlay` renderer
selection key, independently of the image resource supplied to that call.

`FieldNNNNFlagBase` declares an additive flag family within that field. Derris
Kharlan rooms 0130 and 0135 share `BlockTransferPending` at base 164 plus block
index 0..7, separate from their transfer-side selector and permanent placed bits.
The decompiler uses only explicit families in the selected field's definitions;
an individual flag with an equal value does not declare an indexed family.
Thirteen party-split fields declare `PartyAssignmentBank = 150`, retaining the
team selector at `150 + character_id*3` and slot bits at offsets one/two. Their
notes distinguish Iselia's Reactor/Rescue slots 0..3 from Asgard/Mana's A/B slots
1..3, exclusions and overlapping saved-leader fields. Bank reset loops do not
gain individual character meanings.

`StoryFlagBase::DogNamingBank` identifies the thirty named-dog flags used by
Colette's completion count. The long-range map bases identify the Sylvarant and
Tethe'alla banks; the original mapping loops set only 900..910 and 911..920.
Their bank names do not turn those loops into an unlock of every region flag.

`PalmacostaGlobalId` supplies alternate names at verified original word loads:
`WarpRoomState` shares index 59 with the later Mana Leaf Herb route, and
`WarpBattleCount` shares index 42 with the Celsius route. The room input is
0..8, rather than a card-bit mask. Profiles select the early roles by complete
image hash and instruction address; later quest-debug uses in the same script
retain their route names. The aliases change source names only.

Enum names are stable symbols. Resource names are initial editable asset paths;
localized captions are separate data. Unknown or mod-added IDs remain raw numeric
values at asset/script boundaries; failed `TryFrom` does not make them invalid
resources. Edition adapters may translate source IDs into these common identities.

Only established story flag names and unresolved values used by behavior are
listed. Numeric gaps do not get invented placeholder identities. Model node names
are strings from model data; `MODEL_NODES` records only reviewed semantic aliases.
`ColetteAngelStage` belongs specifically to `GlobalId::ColetteAngelProgress`;
unrelated quest counters do not share its stage names. Reviewed story and loot
meanings have original instruction references in the script tools'
[`scenario-identity-evidence.tsv`](../symphonia-script-tools/data/scenario-identity-evidence.tsv).
Its `note_id` references the shared explanations in
[`scenario-identity-notes.tsv`](../symphonia-script-tools/data/scenario-identity-notes.tsv).
`LuinRebuildingStage` belongs specifically to `GlobalId::LuinRebuilding`.
Its queued funding stages are separate from the numeric credited-gald counter
`GlobalId::LuinDonationProgress`. The authored world model's 40000 threshold is
labelled separately from phases with original field operands; stage 600000
persists through the independently flagged statue rewards.
`BookOfRegenerationStage` and `LinkiteTreeStage` likewise belong only to their
respective globals. Their shared number 40000 means the Thoda retrieval phase
in the former and Linkite Ocarina receipt in the latter. The Linkite family
interview combinations preserve their weighted additions; plan stages do not
replace item, pact or location checks.
`PalmacostaRanchAssaultStage`, `PalmacostaRanchDorrStage` and `AsgardRanchStage`
replace the former shared `QuestStep` names for their respective words. Equal
values can identify different parts of each route; consecutive writes and early
scene-start markers are retained. Still-unreviewed quest words stay numeric.
`FireSealStage` and `WaterSealStage` distinguish guardian, blessing, camp and
later pact progression. Their equal 21000 and 50000 values have different write
timing. Camp 0522's equality test and retained 0240's assignment-in-condition
before the Luin ruins marker are separately evidenced and preserved.
`WindSealStage` also preserves the optional early Balacruf visit's +500 offset
through Asgard's +1000 progress updates. Its timer/entry bounds and retained
history-answer selector name readers separately from event completion. Sylph's
failed-trial and pact-completion markers retain their different timing from Fire.
`LightSealStage` separates the library team plan, light-puzzle activation,
regroup call, restored roster, seal release and camp. Early animation-start writes
remain early. Its retained Luin hint selector is 1, distinct from the normal 1000
introduction marker; comparison-only bounds do not claim recovered setters.
`BoltzmanTowerKeyStage` names the early invitation and Piccaring/key search;
`PietroInfiltrationStage` names Sophia's death report and later ranch bargain.
The report is not Pietro's actual death. Neither word proves book receipt or
Pietro's cure, and the key-inquiry phase retains its early assignment and the
branch for an already possessed key.
The first Asgard infiltration uses `AsgardRanchEarlyRouteStage` and
`AsgardRanchLaterRouteStage`, selected at Luin on opposite sides of the initial
Palmacosta story cutoff. Their shared infiltration scenes keep their write
timing, but 50000 means Luin's escape aftermath in the early route and the later
route's subsequent Hima decision. The early route's 41000 is a retained NPC
reader boundary without a recovered setter. The separate `AsgardRanchStage`
still tracks the second infiltration and Kvar battle.
Colette's 101/201/301 phases follow the second/third/fourth seal nights; the
first Fire camp instead ends with main-story `FirstAngelNightCampDepartureReady`.
The 300 speech-eligibility and 20000 Iselia-appeal bounds name readers without
inventing setters. Iselia's original `<301 && >=20000` guard and camp 0240's
assignment-in-condition to 201 remain intact.
Main-story comparison bounds name their reader roles as well. The Light-pact
era boundary preserves overlapping inclusive timer ranges, and the Rune Crest
materials boundary preserves the later item-dependent checks. The Flying Dragon
world-visibility upper bound belongs to an existing authored rule; it has no
recovered original operand and is not a newly discovered story milestone.
Triet's wanted-poster examination adds 500 to the initial poster phase, producing
`StoryStage::TrietWantedPosterExamined` at 1001500. The separate crater conversation
sets its own flag without adding to Story. Snapshot reader names also cover the
Iselia/Heimdall entry restrictions, Palmacosta dialogue, sewer scenery, Clara's
restoration availability, Welgaia's companion-search setup and Altamira's ending
music. Salvation's late word-game boundary retains its overlapping inclusive
comparisons. Ossa's 999999999 entry limit includes one empty retained branch;
the separate empty equality at 1401000 still has no established event meaning
and remains numeric.
Copied globals can use these names in comparisons when control-flow analysis
proves their origin on every path. The temporary remains shared memory and gains
no permanent type. Snapshot evidence records its load, source-global address and
comparison operator separately from direct-global uses; unknown range limits
remain numeric until reviewed.
Inclusive stage upper limits one below a supplied name can use the checked
`bound_before(Name)` constant constructor. It emits the original single literal
and adds no invented enum value or runtime subtraction. This inference does not
apply to unrelated identifiers or silently replace an exact reviewed name.
Other snapshot-only limits name their observed readers: Koton's pre-pursuit and
statue-demand dialogue, Neil's pre-card-key wait, cleared-conveyor scenery, Luin's
expanded-layout transfers and the Tower of Mana's reunited setup and guardian-room
access. The Tower's inclusive 99999 and 999999 limits remain distinct. These
limits have no recovered constant setters. Linkite's zero phase separately records
the initial caravan-progress state; it does not mean the family was never met.
Wind's 4500 explanation phase preserves the optional earlier Balacruf +500 visit
offset and the original subsequent +1000 progression.
`StoryFlagBase` separately names reviewed bases used with dynamic offsets, such
as the five gathered Shadows in Darkness. Individual flag names do not imply
that nearby numeric IDs form an indexed family.
A scene alias can also identify a proven misdirected write without giving it a
working storage meaning. Asgard room 0214's `LeaderLowBitSaveMiswrite` names the
original set of bit 172 in its low-bit save branch. The false branch still clears
170, while restoration in room 0215 reads 171. The alias keeps that original bug
visible and remains confined to room 0214.
Heimdall room 0482's retained helper names its lost-notice enable and actor-201
cleanup gates by their readers. The separate whole-helper suppression bit keeps
its broader scope. These aliases establish no setter meanings, actor identity
or normal-play reachability; adjacent cleanup-only slots remain numeric.
Overworld exploration callbacks also have names for their observed room-0247
transfer marks. Only nineteen written indices are declared; the two worlds
share indices 07, 25 and 31. The latter includes both a Sand Worm callback and
an item callback, without implying either reward or victory. The Triet caravan
entry has a separate mark. Room 0247 has no recovered reader of these flags and
both exits retain landmark 54, so the aliases do not invent return selection or
permanent completion states.

`LongRangeSylvarantRegion10Unlocked` identifies bit 910 from the native travel
grid and persistent-flag query. Sylvarant grid code 10 maps to `900 + 10`; it has
no entry in the separate named stone-discovery table. The name preserves that
native region index without guessing a geographic area, and the original
mapping-service writes remain intact.

The `WorldResetSceneBank*` bases describe the overworld entry script's three
50-bit reset loops. Scene-specific meanings of the reused bits remain separate
research work. Names for flags with no known literal setter describe their
observed reader role, without asserting a quest-completion condition.
Reviewed scene aliases live in `scene_flags.rs` and publish as
`story::FieldNNNNFlag` enums. The decompiler selects them only for the matching
field resource; they still address the same shared session bit. In particular,
bit 150's stair-transfer latch in Asgard and its first-conversation affinity
bonus in Martel Temple must not become a single global milestone name.
`story::WorldFlag` supplies overworld-only aliases. Its Triet arrival bit 1749
is reused as a portal route selector by the later Derris field aliases; neither
the enum namespaces nor the names introduce independent flag storage.

`FieldEquipmentModelId` is the opt-in list of generic field equipment placeholders.
Only these models follow the equipped item when attached to a playable character's
weapon joint. Other models, including scene-local props and specific story swords,
keep their authored appearance. Add reviewed placeholder identities to this enum;
unknown IDs remain valid models and are preserved by default.

`ParticleRecipe` names the separate billboard factory shared by `create_particle`
and `create_effect_object`. It includes their verified sprite descriptors and
leaf/refraction/texture-slot variants. `EmitterRecipe` continues to describe actor
emitters; equal numbers in these catalogues are independent identities. Unknown
sprite inputs retain their numeric values in recovered source.
