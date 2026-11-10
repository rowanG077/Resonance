ids! {
    pub enum GlobalId: u16 {
        Story = 0,
        /// Word 0x44 requests Colette's automatic wing actor. The original
        /// native reader checks Colette actor 2 and a nonzero word before
        /// creating accessory actor 90021; its update retires the accessory
        /// when the request clears or its Colette owner is no longer present.
        /// This request is separate from angel progression, animation, shadow
        /// and collision settings, and does not guarantee immediate visibility.
        ColetteWingsRequested = 1,
        /// Also consulted by the Hima/unicorn route. Character appearance reads
        /// this same word at 0x4c as Colette's angel/crystal progression.
        ColetteAngelProgress = 3,
        WorldMusicBank = 4,
        /// Word 0x54 is the base of the indexed Katz expedition status array.
        /// Receptionists read 0 as idle, 1 as returned and values above 1 as
        /// still away. Starting an expedition writes 36000; collecting its
        /// result or cancelling clears the selected cell. Indexing and the
        /// separate result bytes at 0xa4 remain explicit script operations.
        KatzExpeditionStatusBase = 5,
        PalmacostaRanchAssaultRoute = 30,
        PalmacostaRanchDorrRoute = 31,
        /// The retail field debug menu names these words explicitly (messages
        /// 224-230, 240-246); normal script writes confirm the same quest scopes.
        FireSealProgress = 32,
        WaterSealProgress = 33,
        WindSealProgress = 34,
        LightSealProgress = 35,
        /// BoltzmanTowerKeyStage: early Sophia invitation and Piccaring/key search.
        BoltzmansBookRouteA = 36,
        /// PietroInfiltrationStage: Sophia's death report and the later ranch bargain.
        BoltzmansBookRouteB = 37,
        /// Initial Luin reaction before PalmacostaFirstArrivalComplete selects this route.
        AsgardRanchFirstPartRouteA = 38,
        /// Initial Luin reaction at/after that cutoff selects the later route.
        AsgardRanchFirstPartRouteB = 39,
        AsgardRanch = 40,
        BookOfRegeneration = 41,
        /// Word 0xe8, displayed as t_ice by the original debug menu. The
        /// gloves/lake/Celsius route shares this physical storage with an earlier
        /// Palmacosta 0206 battle counter; that counter's 0/20 are not pact phases.
        CelsiusPactRoute = 42,
        /// Word 0xec, displayed as t_earth. Tracks Gnomelette prerequisites,
        /// escort/chasm attempts, Gnome's pact and the later five-Gnomelette gate.
        GnomePactRoute = 43,
        /// Word 0xf0, displayed as t_shadow by the original debug menu.
        /// Includes the Blue Candle/Kate rescue prerequisite as well as the
        /// five-fragment temple puzzle and Shadow pact. Kate's separate route
        /// word and the individual fragment flags retain their own meanings.
        ShadowPactRoute = 44,
        LuinRebuilding = 46,
        LuinDonationProgress = 47,
        /// Word 0x100, displayed as t_spike in the original debug menu.
        /// Volt's aftermath sets 1 for the Undine/Volt link; completing the
        /// Sylph/Gnome and Efreet/Celsius pairs increments this count. The
        /// scripts use its value to select earthquake scenes and dialogue;
        /// Linkite Tree also checks 3 alongside the separate Shadow pact flag.
        /// This is a count, not a particular spirit's quest stage.
        SeveredManaLinkCount = 48,
        /// Words 0x104/0x108, displayed as s_lig_0x/s_lig_0y. Tower of Mana
        /// 0366 saves actor 480's X/Y positions and reuses them for spawning,
        /// beam geometry and puzzle checks. Values remain ordinary coordinates.
        /// The debug menu reuses 0x104/0x108/0x10c/0x110/0x114 for a saved
        /// return field, X, Y, Z and heading. These are shared storage cells.
        TowerOfManaLightPuzzle0X = 49,
        TowerOfManaLightPuzzle0Y = 50,
        /// Words 0x10c/0x110, s_lig_1x/s_lig_1y: actor 481's saved X/Y.
        TowerOfManaLightPuzzle1X = 51,
        TowerOfManaLightPuzzle1Y = 52,
        /// Words 0x114/0x118, s_lig_2x/s_lig_2y: actor 482's saved X/Y.
        TowerOfManaLightPuzzle2X = 53,
        TowerOfManaLightPuzzle2Y = 54,
        /// Words 0x11c/0x120, s_lig_3x/s_lig_3y: actor 483's saved X/Y.
        TowerOfManaLightPuzzle3X = 55,
        TowerOfManaLightPuzzle3Y = 56,
        /// Flanoir 317 assigns a party character ID; Heimdall 484 reads it.
        FlanoirCompanion = 57,
        /// Word 0x128, displayed as t_kate in the original debug menu.
        /// Tracks the Coliseum/prison/Ozette rescue route separately from the
        /// Blue Candle word. An earlier Seles gossip scene also writes input2000.
        KateRescueRoute = 58,
        /// Word 0x12c, labelled s_leaf / Valley of the medicinal herb in the
        /// original debug menu. Tracks the Ymir fruit, Heimdall admission and
        /// Latheon Mana Leaf Herb route. Earlier Palmacosta scripts reuse this
        /// storage as warp-room state; its room inputs are not quest stages.
        ManaLeafHerbRoute = 59,
        LinkiteTree = 60,
        /// Word 0x138: Palmacosta orienteering's selected-checkpoint bit mask.
        /// The selector rejects an already selected bit before setting it.
        /// The count and current checkpoint are separate halfwords at
        /// 0x134/0x136; retain the original count>=4 and checkpoint-5 exits.
        PalmacostaOrienteeringCheckpointMask = 62,
        /// Word 0x13c, displayed as sub_no. A shared side-event selector/state,
        /// including arena modes and encoded conversation/search counters.
        /// Clearing it does not itself cancel tasks or clear their saved flags.
        SideEvent = 63,
        /// Words 0x15c..0x178 save the eight formation members' costume
        /// indices across the Altamira beach and Meltokio banquet scenes.
        /// Cell 0 also serves as single-character costume scratch for Raine,
        /// Sheena and Colette in other scenes. Querying costume -1 returns the
        /// previous index; the saved values and formation bytes stay separate.
        SavedCostume0 = 71,
        SavedCostume1 = 72,
        SavedCostume2 = 73,
        SavedCostume3 = 74,
        SavedCostume4 = 75,
        SavedCostume5 = 76,
        SavedCostume6 = 77,
        SavedCostume7 = 78,
        /// Word 0x17c saves Lloyd's equipped-title query result for restoration
        /// after the Altamira beach and Meltokio banquet costume changes.
        SavedLloydEquippedTitle = 79,
        /// Word 0x188: Sea-Katz's Red Light/Green Light highest cleared level,
        /// displayed as such in bridge 0077 message 143. Retained game code
        /// raises it to the number of completed levels, up to three.
        SeaKatzHighestClearedLevel = 82,
        /// Word 0x18c starts three indexed per-level Red Light/Green Light
        /// records. A successful round saves its play-loop tick count if the
        /// old record is zero or greater. Message 143 formats these as times.
        SeaKatzBestTimeBase = 83,
        /// Word 0x19c holds the opaque result of loading LloydNeutral in the
        /// test skit. The original loader returns a resource-slot handle,
        /// consumed by the resource wait and portrait creation calls.
        TestSkitLloydNeutralHandle = 87,
        /// Word 0x1a0: Uncle Game's highest cleared-stage count in Triet 0485.
        /// Five unlocks high-speed mode; that mode adds five to its record.
        /// The test skit reuses this physical word as a Lloyd portrait handle.
        /// Its values therefore retain their original scalar expressions.
        UncleGameBestClearedStage = 88,
        /// Word 0x1a4 accumulates correct Uncle Game selections. The retained
        /// correct-answer helper increments it; the display and Midlife Crisis
        /// title gate read it independently of the best-stage record.
        UncleGameCorrectSelections = 89,
        /// Word 0x1a8 starts two Emotional Balloon round records in retained
        /// Izoold 0354 code: mode 4 (Non-stop) and mode 5 (Endless), indexed by
        /// mode minus four. Each keeps a maximum. The test skit also writes 1
        /// into this physical word; it has no quest-stage value domain.
        EmotionalBalloonBestRoundsBase = 90,
        /// Word 0x1b0 keeps the largest cafeteria Gald reward in Palmacosta
        /// 0070. Zero accepts the first result; the same result is paid as Gald.
        PalmacostaCafeteriaBestGaldReward = 92,
        /// Word 0x1b4 keeps the smallest completed orienteering field-tick
        /// time in Palmacosta 0056. Zero accepts the first result. This is
        /// independent of the checkpoint mask and the in-progress flag.
        PalmacostaOrienteeringBestTimeTicks = 93,
        /// Words 0x308/0x30c/0x310 hold the common battle event's initial eye
        /// position (0,1000,4000). SetPose passes these three signed coordinates
        /// to the original camera initializer before pitch and heading.
        /// They are coordinate storage, with no quest-stage value domain.
        BattleCameraEyeX = 178,
        BattleCameraEyeY = 179,
        BattleCameraEyeZ = 180,
    }
}

ids! {
    /// Alternate global indices at verified original Palmacosta instructions.
    /// These share storage with later quest routes; field-wide replacement
    /// would incorrectly rename the retained quest-debug routines.
    pub enum PalmacostaGlobalId: u16 {
        /// Word0xe8 counts returns from the warp-room enemy battle handler.
        /// Increment is outside its victory-only despawn branch. At20 it can
        /// display a hint while the two ranch routes are still early. This is
        /// the later Celsius route's storage, not a count of victories.
        WarpBattleCount = 42,
        /// Word0x12c is copied to halfword0x0818 to select warp-room layouts
        /// 0..8. Portal handlers update it; the card-door entries write7.
        /// It is not a card-bit mask or later Mana Leaf Herb quest progress.
        WarpRoomState = 59,
    }
}

ids! {
    /// Optional names for signed-byte memory addresses, not their stored values.
    /// These retain the original shared storage, indexing and signed narrowing.
    pub enum ByteOffset: u16 {
        /// Indexed Katz expedition result bytes, separate from status words.
        /// Keep the original payload, 0x80 flag and signed-byte operations.
        KatzExpeditionResultBase = 0xa4,
        /// Eight saved formation character bytes. The normal setters query
        /// formation slots 0..7; restoration reads these bytes independently
        /// of the saved costume words. These are physical shared cells.
        SavedFormation0 = 0x150,
        SavedFormation1 = 0x151,
        SavedFormation2 = 0x152,
        SavedFormation3 = 0x153,
        /// The debug menu also writes a truncated copy of its return heading
        /// here; its actual return warp uses a separate full-word heading.
        /// Preserve this original signed-byte store.
        SavedFormation4 = 0x154,
        SavedFormation5 = 0x155,
        SavedFormation6 = 0x156,
        SavedFormation7 = 0x157,
        /// Saved FieldLeader query byte used with the formation snapshots.
        SavedFieldLeader = 0x158,
        /// Separate indexed formation backup in Hima 0300 and Meltokio 0399.
        /// Hima restores and clears its cells; retain the original indexing.
        SecondarySavedFormationBase = 0x180,
    }
}

ids! {
    /// Optional names for signed-halfword addresses, not quest value domains.
    pub enum HalfwordOffset: u16 {
        /// Palmacosta orienteering selector count, distinct from its bit mask.
        /// The original count>=4 early exit and increment remain unchanged.
        PalmacostaOrienteeringCheckpointCount = 0x134,
        /// Current orienteering checkpoint; the selector's checkpoint5 early
        /// exit writes14, while its count>=4 exit writes15. Keep these values.
        PalmacostaOrienteeringCurrentCheckpoint = 0x136,
        /// Heimdall 0284 maps the selected Flanoir companion to actor IDs2..8
        /// and uses this halfword to query that actor's X/Y/Z positions.
        HeimdallCompanionActor = 0x146,
    }
}

ids! {
    /// GlobalId::KateRescueRoute, word 0x128. Separate from ShadowPactRoute's
    /// Blue Candle prerequisite and from the individual rescue/affinity flags.
    pub enum KateRescueStage: u32 {
        /// Meltokio 0399's Sebastian entry gate tests zero before his report.
        ExecutionReportNotHeardInput = 0,
        /// Meltokio 0399 sub_0A9F reports Kate's planned execution and suggests
        /// the Coliseum passage, but asks the party to visit the laboratory first.
        /// Retained Meltokio test choices can also select this phase directly.
        KateExecutionReportHeard = 1000,
        /// Rescue-registration dispatch input. Laboratory 0394's conversation
        /// writes it from1000, and Coliseum0390's test menu also selects it.
        /// Meltokio0376's earlier Seles/new-Chosen gossip writes the same input;
        /// this number alone does not prove that the rescue plan was heard.
        ColiseumRescueRegistrationInput = 2000,
        /// All eight entrant branches of Meltokio0390 sub_0354 write this after
        /// accepted registration, before transfer to the waiting room.
        ColiseumRescueRegistrationAccepted = 3000,
        /// Waiting-room0391's readiness scene writes this before entering the
        /// arena. It is not the result of the subsequent rescue bout.
        ColiseumRescueBoutReady = 4000,
        /// Arena0392 sub_02DA is called on the non-Defeat branch, writes this,
        /// restores10percent HP/TP and returns to the waiting room's prison exit.
        ColiseumRescueBoutCleared = 5000,
        /// Prison0411 sub_02D3 finishes Kate's escape scene and party restoration,
        /// removes her cell actor and transfers to Zelos's house0380.
        KatePrisonEscapeComplete = 6000,
        /// House0380 sub_0F4C: Kate requests escort to her ruined birthplace,
        /// Ozette, and Lloyd agrees. The family discussion there remains later.
        KateOzetteEscortAgreed = 7000,
        /// Tower of Mana0361's retained Kate-flag-set-to10000 choice writes it.
        /// No normal writer or literal reader establishes a separate milestone.
        RetainedKate10000Preset = 10000,
        /// Ozette0433 and retained0425 finish Kate's Pope/half-elf discussion.
        /// The shared Shadow route also advances separately in that scene.
        KateOzetteFamilyDiscussionComplete = 11000,
    }
}

ids! {
    /// GlobalId::SideEvent, word0x13c. These identify selectors and encoded
    /// states, not global story milestones. Some ranges also count conversations
    /// or search contacts; increments retain their original arithmetic.
    pub enum SideEventState: u32 {
        /// No side-event state selected. Scripts clear this at many handoffs;
        /// this store alone does not cancel tasks or reset quest flags.
        Idle = 0,
        /// Mizuho0326's retained exit scene uses >=2000, not20000. This is
        /// the original inclusive reader bound, not a duel-permission milestone.
        MizuhoExitSceneLowerBound = 2000,
        /// Zelos's house0399 accepts the noble-party invitations, snapshots
        /// the party and starts the formal-costume/banquet route.
        NoblePartyInvitationsAccepted = 10100,
        /// Castle0405 finishes the audience/hall-entry scene and transfers0401.
        NoblePartyHallEntry = 10101,
        /// Hall0401 records its terrace invitation and starts the conversation
        /// counter here. Individual companion conversations increment the word.
        NoblePartyConversationBase = 10102,
        NoblePartyOneConversation = 10103,
        NoblePartyTwoConversations = 10104,
        NoblePartyThreeConversations = 10105,
        /// Four conversations from10102 reach this threshold. The hall/terrace
        /// departure helpers also select it before returning to Zelos's house;
        /// it is both a counter target and a return-dispatch input.
        NoblePartyConversationTarget = 10106,
        /// Mizuho0326 reports Kuchinawa's duel request and begins its permission
        /// route. The duel, result flags and Corrine's bell remain separate.
        KuchinawaDuelReported = 20000,
        /// Mizuho0327 and the retained0326 conversation accept Lloyd as observer
        /// and direct the party to tell Orochi when ready.
        KuchinawaObserverPermissionGranted = 20001,
        /// Orochi's ready choice in0326 selects this before the isle transfer.
        KuchinawaDuelDeparture = 20003,
        /// Waiting-room0391 uses >=80000 for its Coliseum dispatch. Preserve
        /// the unbounded comparison; this boundary is not itself an entry mode.
        ColiseumDispatchLowerBound = 80000,
        /// Solo Coliseum admission modes; original prices5000/10000/30000.
        ColiseumSingleBeginner = 80101,
        ColiseumSingleIntermediate = 80102,
        ColiseumSingleAdvanced = 80103,
        /// Three-member team modes; the original menus offer Beginner15000 and
        /// Advanced40000, with no intermediate team selector in these writers.
        ColiseumTeamBeginner = 80111,
        ColiseumTeamAdvanced = 80112,
        /// Toize mine0415's Tabatha/Presea scene, also selected by retained0414.
        PreseaExsphereVoicesVisit = 120200,
        /// Sybak0434 accepts the lost-child search. Subsequent contacts count
        /// above this base; saved per-person information flags remain separate.
        SybakLostChildSearchBase = 130000,
        /// Record301's first denial assigns130000+1, even if other contacts
        /// already changed the counter; other contacts increment. Preserve that
        /// assignment rather than repairing it into an increment. This value
        /// can also be reached through other inquiry sequences.
        SybakLostChildCounterOne = 130001,
        /// Search counter base+3. Reunion entry requires this value together
        /// with the relevant story, party and not-yet-reunited checks. It does
        /// not identify three particular people or prove the reunion occurred.
        SybakLostChildCounterTarget = 130003,
        /// Palmacosta academy0068/0069 and retained0072/0073 prepare the study
        /// session for Genis's examination, clearing the selector afterward.
        PalmacostaAcademyStudySession = 140100,
        /// Helpful study responses increment140100. These are preparation
        /// points, not the examination's displayed score or a title award.
        PalmacostaStudyOnePreparationPoint = 140101,
        PalmacostaStudyTwoPreparationPoints = 140102,
        PalmacostaStudyThreePreparationPoints = 140103,
        /// The study scene adds100, retaining its0..3 preparation points for
        /// the examination results. Zero shows Mighty400/Genis398; one/two
        /// show398/398; three show Mighty398/Genis400. The arithmetic and both
        /// inclusive tie-input comparisons are retained.
        PalmacostaExamZeroPreparationPoints = 140200,
        PalmacostaExamOnePreparationPoint = 140201,
        PalmacostaExamTwoPreparationPoints = 140202,
        PalmacostaExamThreePreparationPoints = 140203,
        KratosPalmacostaSwordLesson = 160100,
        KratosAsgardSwordLesson = 160200,
        KratosLuinSwordLesson = 160300,
        KratosHimaFinalSwordLesson = 160400,
        /// Altamira0027 starts Presea's mascot-debut sequence with George.
        PreseaMascotDebut = 220100,
        /// Castle0402 selects the return from Regal's cooking challenge; its
        /// good/bad dish ending flags retain which outcome was chosen.
        RegalCookingChallengeReturn = 230000,
        /// Castle0404's maid substitution and its later return to the entrance.
        ColetteMaidSubstitution = 240000,
        ColetteMaidJobReturn = 240001,
        /// Reader-only input at Martel0307's exit: Colette says she must do
        /// her best alone and is moved back inside. No literal writer recovered.
        ColetteSoloTempleExitBlockedInput = 270100,
        /// Janet0398's repeat response before the separate Horace conversation.
        PreseaJanetRepeatVisit = 390100,
        /// Janet's first conversation has been recorded, without the later
        /// Horace-conditioned followup. This does not complete the whole quest.
        PreseaJanetFirstVisit = 390101,
        /// Horace's flag is already set; Janet's followup is selected for the
        /// automatic exit scene, which advances390103 and marks the second visit.
        PreseaJanetHoraceFollowupPending = 390102,
        PreseaJanetSecondVisitReturn = 390103,
        /// Altamira's mother accepts help finding Mary, Jo, Beth and Diana.
        AltamiraLostDaughtersSearch = 470100,
        /// All daughters have reunited; the hotel reward handoff is pending.
        AltamiraLostDaughtersHotelReward = 470101,
        /// Retained Triet0487 inn branch transfers to0525 for the Kratos war
        /// discussion. This is a side-scene selector, not a main story stage.
        TrietKratosWarDiscussion = 520000,
    }
}

ids! {
    /// GlobalId::CelsiusPactRoute, word 0xe8. Gloves and CelsiusTear inventory,
    /// the frozen-lake geometry and pact flags remain separate from this word.
    /// No zero/20 variants: earlier Palmacosta code uses them as counter values.
    pub enum CelsiusPactStage: u32 {
        /// Ice 0324 entry advances zero here without an introductory dialogue.
        TempleEntryHandled = 100,
        /// Flanoir 0320 tells a visitor with one or two PenguinistQuills to
        /// bring three; Ice 0324 sub_4BE0 discusses the unfrozen lake, CelsiusTear
        /// and protective gloves. Both can write this information phase, which
        /// does not establish glove possession. The seal-test menu also writes it.
        PenguinistGlovesRequirementExplained = 1000,
        /// Ice 0324 event 2001's three successful freeze branches write this.
        /// The retained ICE=2000 test also selects the phase directly.
        LakeFrozen = 2000,
        /// Ice 0325's retained ICE=3000 seal-test choice writes this number.
        /// No normal event or literal reader establishes a further milestone.
        RetainedSealTest3000 = 3000,
        /// Ice 0325 entry uses <40000 for the initial Celsius encounter.
        /// This is an exclusive dispatch limit, with no recovered literal writer.
        InitialBattleInputLimit = 40000,
        /// Ice 0325 sub_075E: Defeat(3) revives the party, restores 10 percent
        /// HP/TP and selects the Celsius retry encounter.
        CelsiusBattleRetryAvailable = 41000,
        /// Ice 0325 sub_075E: Sapphire, summon techniques, pact flag and
        /// the relevant departure dialogue precede this write.
        CelsiusPactRewardsComplete = 50000,
        /// Ice 0323 sub_244B handles the post-pact exit earthquake and party
        /// discussion before writing this, just before the return fade.
        ExitEarthquakeHandled = 51000,
    }
}

ids! {
    /// GlobalId::GnomePactRoute, word 0xec. Individual Gnomelette/potion,
    /// curry ownership, escort retry and exit-earthquake flags remain separate.
    pub enum GnomePactStage: u32 {
        /// Earth 0235's initial entry input; Toize 0413's Gnomelette response
        /// also reads zero. The entry helper advances it to 100.
        InitialTempleEntryInput = 0,
        /// Earth 0235 sub_0B96 advances the first entry without a dialogue.
        TempleEntryHandled = 100,
        /// Earth 0235's Gnomelette 5 refuses passage while Big Brother is away
        /// and directs the party to the mine. His separate returned flag is read.
        MissingGnomeletteSearchRequested = 1000,
        /// Earth 0235's gate conversation permits passage after Big Brother
        /// returns. The mine's potion and return flag are separate prerequisites.
        GnomeletteGatePassageGranted = 2000,
        /// Earth 0236 event 3100 introduces the spicy-food request and writes
        /// this before testing Curry recipe/ingredients. The recipe is not granted.
        SpicyFoodRequested = 11000,
        /// Earth 0236 event 3100 completes a curry scene and removes its trigger
        /// before this write; ingredient decrements follow it separately.
        GnomeletteCurryServed = 12000,
        /// Earth 0236 event 3200: Gnomelette 3 offers help at the chasm and
        /// says to meet at the secret passage. The escort is not yet completed.
        GnomeletteChasmHelpAgreed = 22000,
        /// Earth 0236 event 3300 introduces Big Brother's shortcut. Earth 0235
        /// also restores this escort-ready phase when re-entering from 23100;
        /// its retained reset event selects the same input.
        GnomeletteEscortReady = 23000,
        /// Earth 0235 sub_1759 starts the escort; sub_1C0C resumes it after
        /// the potion interruption. Area re-entry can reset it to 23000.
        GnomeletteEscortActive = 23100,
        /// Earth 0235 event 1406 stops Gnomelette 3 after enemy contact,
        /// displays his distress and restores his interaction label.
        EscortStoppedAfterEnemyContact = 23200,
        /// Earth 0235 sub_1887 finds at least one of PalmaPotion, FlanoirPotion
        /// or MizuhoPotion before opening the potion choice. None is consumed yet.
        EscortPotionChoiceAvailable = 23300,
        /// The other sub_1887 branch has none of those three potions. This is
        /// the unavailable-potion state, not proof that a potion was handed over.
        EscortPotionUnavailable = 23400,
        /// Earth 0235's escort reaches its final waypoint and removes actor1204.
        /// Earth 0237's retained test event also writes this phase directly.
        GnomeletteEscortDestinationReached = 23500,
        /// Earth 0237 sub_0DA6 says Do it right, records this failed earthquake
        /// attempt and then offers retry/leave choices. The chasm is still open.
        JointEarthquakeAttemptFailed = 23600,
        /// Earth 0237 sub_0A7C completes the falling-path/landing sequence,
        /// removes the Gnomelette and clears its earthquake-retry flag.
        ChasmPathDropped = 24000,
        /// Earth 0237 entry uses <40000 for the initial Gnome encounter.
        /// This is an exclusive dispatch limit, not a recovered event at 40000.
        InitialBattleInputLimit = 40000,
        /// Earth 0237 sub_21EE's defeat branch selects this retry phase and
        /// marks TempleOfEarthArenaReloadPending before restoring the field view.
        GnomeBattleRetryAvailable = 41000,
        /// Earth 0237 sub_21EE: pact flag, Ruby, summon techniques and the
        /// relevant departure conversation precede this write.
        GnomePactRewardsComplete = 50000,
        /// Earth 0235 event 1400 finishes the post-pact exit earthquake
        /// discussion and camera restoration before the return fade.
        ExitEarthquakeHandled = 55000,
        /// Earth 0236 event 3400 reunites the five Gnomelettes, removes actors
        /// 601..605 and the blocking trigger, then writes this opened-gate phase.
        AllGnomelettesPassageOpened = 60000,
    }
}

ids! {
    /// GlobalId::ShadowPactRoute, word 0xf0. These are route phases and
    /// explicitly identified dispatch inputs, separate from pact/fragment bits.
    pub enum ShadowPactStage: u32 {
        /// Darkness 0125's entrance dispatch selects the first darkness
        /// explanation at zero. This names the observed initial input.
        InitialTempleVisitInput = 0,
        /// Darkness 0125 sub_057A: the party cannot proceed in the darkness
        /// and Sheena proposes consulting the Elemental Research Laboratory.
        /// Meltokio 0380's retained Shadow-1000 test also selects this phase.
        LaboratoryHelpForDarknessProposed = 1000,
        /// Meltokio 0380 sub_1371: Lloyd proposes going to the laboratory and
        /// Raine warns that they are wanted. This precedes the laboratory visit.
        FirstLaboratoryVisitPlanned = 2000,
        /// Meltokio 0380's retained Shadow-3000 debug preset writes this.
        /// Its exit dispatch selects the laboratory-revisit conversation only
        /// alongside main-story RegalAltamiraVisitRequested. No normal writer
        /// for this input is recovered; do not invent an expedition milestone.
        LaboratoryRevisitDispatchInput = 3000,
        /// Meltokio 0380 sub_14B0: Lloyd and Sheena hope the laboratory's
        /// expedition has returned. Its door can still use the locked response.
        LaboratoryRevisitPlanned = 4000,
        /// Meltokio 0394 sub_1613: staff discuss the Blue Candle and Kate's
        /// execution; the party agrees to use the Coliseum passage to rescue her.
        BlueCandleKateRescuePlanAgreed = 10000,
        /// Ozette 0433 sub_115D: Kate explains her relationship to the Pope
        /// and the party discusses half-elves before returning for the candle.
        /// This is later than the separate prison-rescue flag and route word.
        KateOzetteDiscussionComplete = 20000,
        /// Meltokio 0394 sub_1CA5 thanks the party and awards BlueCandle.
        BlueCandleObtained = 30000,
        /// Darkness 0125 sub_0C30: the Blue Candle lights the entrance and
        /// the party remarks that the Summon Spirit's mana feels unstable.
        BlueCandleUsedAtEntrance = 31000,
        /// Darkness 0126 sub_3E4B explains the five physical fragments, their
        /// aversion to light and the need to lead them back to the seal.
        ShadowFragmentsExplained = 32000,
        /// Darkness 0125 sub_114A: the controlled character agrees to inspect
        /// the passage alone. This does not assert that the passage is traversed.
        SoloPassageEntryAgreed = 32100,
        /// Darkness 0126 sub_4607: the first changed-ring trial and Lloyd's
        /// Did anything change remark finish before this write.
        DarknessRingFirstTrialComplete = 32200,
        /// Darkness 0126 sub_47FA moves linked block 3101 to (-400,105,-133),
        /// waits for the motion and restores the camera before writing this.
        FirstLinkedBlockShiftComplete = 32300,
        /// Darkness 0126 sub_57B3 writes this before its second block sequence,
        /// which moves linked block 3102's Y toward -1715. It marks the start.
        SecondLinkedBlockMoveStarted = 32400,
        /// Darkness 0126 sub_28DF: all five gathered-fragment flags are set,
        /// then the party discusses taking them down to the bottom-level seal.
        FiveShadowsGatheredOnUpperFloor = 32500,
        /// Darkness 0126's monitor requires all five Z values below -1900 and
        /// all five Y values above -1100, then starts the lower-floor handoff.
        FiveShadowsReadyForLowerFloor = 33000,
        /// Darkness 0127 entry restoration and its retained event 8000 accept
        /// this alongside 33000. No direct literal writer is recovered.
        LowerFloorAlternateEntryInput = 33001,
        /// Darkness 0127's monitor requires all five fragments below Z -8000,
        /// then removes the transition triggers and starts the final seal scenes.
        FiveShadowsReachedFinalSealLevel = 34000,
        /// Darkness 0127 sub_2522: battle outcome Defeat(3) takes Shadow's
        /// Leave response, revives/restores the party and writes this retry phase.
        ShadowBattleRetryAvailable = 41000,
        /// Darkness 0127 sub_2522: the pact, ShadowPactFormed bit, Amethyst,
        /// summon-technique awards and departure discussion precede this write.
        ShadowPactRewardsComplete = 50000,
    }
}

ids! {
    /// GlobalId::ManaLeafHerbRoute, word 0x12c. This route spans Ymir,
    /// Heimdall and Latheon; it is independent of the main story word and of
    /// item possession. Failed Ymir deliveries can restore FruitRouteReady.
    /// Earlier Palmacosta warp-room state reuses the storage without these phases.
    pub enum ManaLeafHerbStage: u32 {
        /// Ymir 0519 sub_32F1: Raine identifies Heimdall ahead and Genis recalls
        /// his birthplace. The blue seed and fruit search still follow.
        YmirApproachToHeimdallConfirmed = 1000,
        /// Ymir 0520 event 5100: the selected character notices a round blue
        /// object. It is examined before the blue-seed pickup, not the fruit.
        YmirBlueObjectNoticed = 1010,
        /// Ymir 0520 event 3600 grants BlueSeed and discusses planting it.
        YmirBlueSeedObtained = 1100,
        /// Ymir 0519's flower-inspection dispatcher accepts 1101..1399 for
        /// the one-more-seed-space dialogue. This is a reader boundary, not a
        /// recovered event at 1101; the authored inclusive comparison remains.
        YmirOneSeedSpaceDialogueLowerBound = 1101,
        /// Ymir 0519 sub_2D48 grows the blue flowers after seed planting.
        YmirBlueSeedPlanted = 1200,
        /// Ymir 0521 sub_3CEF: a small fish brings the chest to (-3126,-462,-64).
        /// WhiteSeed is awarded separately by the chest's pickup event.
        YmirWhiteSeedChestRecovered = 1300,
        /// Ymir 0521 event 3400 awards WhiteSeed, retaining its <1400 guard.
        YmirWhiteSeedObtained = 1400,
        /// Ymir 0519's flower-inspection dispatcher accepts 1401..2999 for
        /// the no-space-to-plant dialogue. No literal writer sets this boundary.
        YmirNoSeedSpaceDialogueLowerBound = 1401,
        /// Ymir 0519 sub_2FFE grows the white flowers after seed planting.
        YmirWhiteSeedPlanted = 1500,
        /// Ymir 0520 sub_717E starts the fruit delivery puzzle at this phase.
        /// Multiple failed fish routes and re-entry paths reset it here while
        /// leaving seed progress intact; this does not assert a fruit pickup.
        YmirFruitRouteReady = 1600,
        /// Ymir 0520 sub_39D4: the boar knocks down the fruit, marks
        /// FruitKnockdownMarked and places interaction actor 3904 at (1042,-208,112).
        YmirFruitKnockedDown = 1700,
        /// Ymir 0520 sub_421E advances from 1700 after the boar sequence,
        /// placing fruit interaction actor 3904 at (2027,6,-50).
        YmirFirstBoarFruitShiftComplete = 1710,
        /// Ymir 0520 sub_2432 advances from 1710 after the fish route,
        /// placing fruit interaction actor 3904 at (-818,614,112).
        YmirFishFruitTransferComplete = 1720,
        /// Ymir 0520 sub_2FD9 advances from 1720, removes interaction actor
        /// 3904 and places carrier 3601 at (-2500,541,-70). This is an alternate
        /// transfer route, distinct from the carrier placement at phase 1800.
        YmirFruitFirstTransferRouteReady = 1725,
        /// Ymir 0520 sub_4BFF advances from 1720 after its boar sequence.
        YmirSecondBoarFruitShiftComplete = 1730,
        /// Ymir 0520 sub_2FD9 advances from 1730, removes interaction actor
        /// 3904 and places carrier 3601 at (-2532,-53,-70).
        YmirFruitSecondTransferRouteReady = 1800,
        /// Ymir 0521 sub_502B writes this before moving carrier 3900 and
        /// small fish 3005 to (2000,-83,-70) and (2080,-83,-82), respectively.
        YmirFinalFishRouteStarted = 1810,
        /// Ymir 0521 sub_2E87 advances from 1810 only on the successful route
        /// past the boulder, requiring both boulder-hit flags. Failure resets 1600.
        YmirFruitCarriedPastBoulder = 1900,
        /// Ymir 0519 event 4800 writes this before the return approach moves
        /// carrier 3700 and fish 3004. The pickup is not yet available.
        YmirFruitReturnRouteStarted = 1910,
        /// Ymir 0519 sub_3817 advances from 1910 after installing pickup actor
        /// 3404 at (-429,576,-70) and removing the fish. Awarding the fruit follows.
        YmirFruitPickupAvailable = 1920,
        /// Ymir 0519 event 3404 marks YmirFruitObtained and grants YmirFruit.
        YmirFruitObtained = 2000,
        /// Both reward-capacity branches of Ymir 0521's boy conversation write
        /// this after handing over YmirFruit; the MetalSphere reward can be full.
        YmirFruitGivenToBoy = 3000,
        /// Heimdall 0282 sub_0A6F: the entrance exchange with Kratos and the
        /// guard ends with Raine and Genis waiting outside under the half-elf ban.
        HeimdallHalfElvesDeniedEntry = 4000,
        /// Heimdall 0289 sub_02C4: the elder explains Latheon's location and
        /// provides his staff for the watchman, after discussing Colette's illness.
        ElfEldersStaffReceived = 5000,
        /// Latheon 0492 event 4001 explains feeding the air-blowing flowers.
        LatheonAirFlowersExplained = 6000,
        /// Latheon 0499's first Storyteller interaction accepts the inclusive
        /// range 0..8000. This is its upper reader boundary, not a quest event.
        StorytellerFirstAudienceUpperBound = 8000,
        /// Latheon 0499 sub_01FB: the Storyteller accepts the elder's proof
        /// and agrees to lead the party to the herb, which remains uncollected.
        StorytellerHerbGuidanceAgreed = 9000,
        /// Latheon 0497 sub_058D: the Storyteller points to the cave ahead.
        StorytellerHerbCaveDirectionsHeard = 10000,
        /// Reader-only alternate input accepted alongside 10000 by Latheon
        /// 0495's cave-approach and 0499's Storyteller dispatchers. No direct
        /// literal writer is recovered.
        LatheonWaterfallApproachAlternateInput = 11000,
        /// Latheon 0495 sub_06C3: its three waterfall/cave dialogue variants
        /// converge here, including the variant still looking for a way across.
        /// This does not assert that the waterfall has already been split.
        LatheonWaterfallCaveApproachDiscussed = 12000,
        /// Latheon 0496 event 4003 defeats the guardian and awards ManaLeafHerb.
        ManaLeafHerbObtained = 13000,
        /// Latheon 0499 sub_08C1: after returning with the herb, the Storyteller
        /// explains Mithos, his companions and the divided worlds. Many later
        /// side-event readers use this threshold; it is not Colette's cure itself.
        StorytellerMithosHistoryHeard = 14000,
        /// Ymir 0519's flower-inspection dispatcher accepts 3000..999999 for
        /// the fruit-search aftermath remark. This is an upper reader bound,
        /// distinct from the equal-valued controlled-actor selector namespace.
        YmirFruitAftermathDialogueUpperBound = 999999,
    }
}

ids! {
    /// The word at 0x4c. Fire writes 1; Wind/Water/Light read 1 for Remiel's
    /// second-seal dialogue. Shared camp 0522 then adds 100 for each subsequent
    /// seal night. The first Fire camp uses a separate main-story marker.
    /// Retained camp 0240 assigns 201 inside a condition; preserve that operator.
    pub enum ColetteAngelStage: u32 {
        BeforeFirstSeal = 0,
        FirstSealReleased = 1,
        /// Camp 0522 sub_065F at input 1 discusses Colette being unable to sleep, then adds 100. Wind 0510 sub_0447 reads resulting 101 for Remiel's third-seal dialogue. This is the second seal's night, rather than the first Fire camp. The original debug menus retain their exact presets.
        SecondSealNightComplete = 101,
        /// The shared camp's 101 branch discovers Colette's bleeding hand without pain and detours to the private coffee scene in retained 0240. The normal +100 continuation reaches 201; Remiel's next blessing opens the route to the Tower of Salvation. Camp 0240 also assigns 201 inside a condition, while 0522 compares its updated value with 201 before setting Luin ruined. Preserve that distinct assignment quirk.
        ThirdSealNightComplete = 201,
        /// Shared camp 0522 handles input 201 after the fourth pre-Tower seal, with Colette's voice loss and Sheena explaining Tethealla, then adds 100 to reach 301. Lake Umacy's cannot-speak debug option also assigns 301. The first Fire camp and the later Tower soul-loss phase remain separate.
        FourthSealNightComplete = 301,
        /// Dog-naming, NPC greetings, academy/fake-Chosen side events and an Undine-pact response compare against 300. Most readers pair >=300 with <=SoulLost for Colette's silent branch, or <300 with >SoulLost for speech eligibility; the pact response uses <300 alone. Palmacosta 0056 shows an ellipsis instead of her greeting inside the range. This is an inclusive reader boundary, with no recovered constant setter or event at 300.
        VoicelessEligibilityLowerBound = 300,
        SoulLost = 1000,
        SoulRestored = 2000,
        CrystalDiseaseRevealed = 3000,
        CrystalDiseaseCured = 10000,
        /// Iselia 0344 and 0346 entry call their Colette/Lloyd village-entry appeal helper only for ColetteAngelProgress <301 AND >=20000. Those conditions are mutually exclusive for an unchanged word. Preserve the original >=20000 operand and eager AND; do not repair it to 2000 or OR. The name describes the retained helper-selection bound, not a reachable angel milestone or recovered setter.
        IseliaEntryAppealLowerBound = 20000,
    }
}

ids! {
    /// GlobalId::FireSealProgress; seal scenes, camp and later pact scenes remain distinct.
    pub enum FireSealStage: u32 {
        /// Triet Ruins 0221 sub_0A13 runs formation 12, sets FireSealGuardianDefeated and writes this before Colette's altar prayer, Remiel's blessing and the separate main FireSealReleased stage. The retained Efreet debug choice also writes it directly. This word alone is not a completed pact or angel night.
        GuardianBattleAftermath = 21000,
        /// Triet Ruins 0221 sub_4C8E writes this after Efreet accepts the request to test Sheena, before party reorganization and formation 23 or 39. Failed trials preserve this phase. The different battle formation and dialogue depend on the original main-story branch.
        EfreetTrialIntroduced = 41000,
        /// Triet Ruins 0221 sub_54BA writes this before the vow, Garnet award, summon techniques and EfreetPactFormed bit. It remains through that success scene and is used by shared timer helpers. Preserve this early write; the separate pact flag and rewards are not implied to have been set at the assignment itself.
        EfreetVictoryAftermathStarted = 50000,
        /// Triet Ruins 0218 sub_3DF7 writes this after the exterior earthquake discussion only on its word-0x0100-equals-2 branch, before restoration. The Gnome/Sylph conditions select different dialogue. Preserve that original condition and the separate pending-earthquake flag.
        ExitEarthquakeDiscussionComplete = 51000,
    }
}

ids! {
    /// GlobalId::WaterSealProgress; seal scenes, camp and later pact scenes remain distinct.
    pub enum WaterSealStage: u32 {
        /// Thoda dock 0082 sub_059D tests zero before the first washtub and Raine-water-fear introduction. Subsequent departures use the >=1000 branch. This is the initial WaterSealProgress word, separate from other temples' equal-valued states.
        BeforeWashtubIntroduction = 0,
        /// Thoda dock 0082 sub_059D writes this after the first washtub introduction and party restoration, before clearing SightseeingBoatDeparturePending and playing the travel cinematic. The arrival conversation on Thoda uses a separate later phase.
        FirstWashtubDepartureMarked = 1000,
        /// Thoda 0006 sub_25F5 plays the first washtub-arrival discussion, including Raine's relief and the optional Sheena/Altamira exchange. It writes this before the common restoration helper; the lookout sign and oracle stone are later phases.
        FirstThodaArrivalConversationComplete = 10000,
        /// Thoda 0006 sub_2A58 discusses the geyser and Colette recognizes a familiar sign on the lookout platform. Party restoration precedes this write. The oracle-stone entrance sequence has not yet completed.
        GeyserLookoutSignNoticed = 11000,
        /// Thoda 0006 sub_8943 identifies the oracle stone and handles the entrance scene with route-dependent book, Sheena, Corrine and Noishe material. Both setting paths write this before the shared helper. The BookConsulted and statue-retrieval branches keep their distinct dialogue; entering room 0007 is handled separately.
        OracleStoneEntryMarked = 12000,
        /// Thoda 0007 at the oracle-entry phase can play Kratos's assassin warning, Sheena's pact/Colette-consent discussion, or silently advance under its original conditions. All five setting paths share this entry-handled value; it does not guarantee that the consent discussion or any particular dialogue played.
        InitialTempleEntryHandled = 13000,
        /// Thoda 0010 sub_0B71 writes this after formation 13 and before the guardian bit, altar prayer and Remiel's blessing. The second/third/fourth seal dialogue depends on ColetteAngelProgress. This early phase must not be treated as a completed Undine pact or a fixed ordinal seal release.
        GuardianBattleAftermath = 20000,
        /// Thoda 0010 sub_0B71 writes this after the branch-specific Remiel/blessing and party discussion, before the common helper and optional first Undine trial. Colette's angel progress determines which ordinal seal was released; this value does not identify a fixed second seal or a completed camp night.
        SealReleaseConversationComplete = 21000,
        /// Thoda 0010 failure helpers sub_6320 and sub_63FA advance the pre-camp 21000 phase here after Undine says the party is not yet worthy. Later trial failures from 31000/40000 use the distinct 41000 phase. Repeated retries and the original battle-result conditions remain unchanged.
        UndineTrialFailedBeforeAngelNight = 22000,
        /// Thoda 0010 sub_64C0 writes this from 21000 or 22000 after the Aquamarine, water summon techniques and UndinePactFormed bit. The following angel-toxicosis camp has separate pending and completed phases. This is the early pact route, not Fire's equal-numbered progress.
        UndinePactCompleteBeforeAngelNight = 23000,
        /// Thoda 0006 sub_A0DC discusses Colette's post-seal illness, agrees to rest and writes this before transfer to camp 0522. The camp debug menu also directly selects 30000 for each of its three angel-night presets. This is not proof that the night conversation has finished.
        AngelNightCampPending = 30000,
        /// Camp 0522 sub_065F and retained camp 0240 sub_0048 update Colette's angel progression, advance WaterSealProgress from 30000 here and return to Thoda. Room 0240 retains its assignment-in-condition quirk. These are camp continuations, not merely debug presets or proof of an Undine pact.
        PostSealAngelNightComplete = 31000,
        /// Thoda 0007 entry at 31000 can play sub_0CF9's later Sheena/pact introduction under its original story, consent, party and pact conditions, or silently advance. Both setting paths use 40000; it does not prove the Undine trial was won.
        UndineRevisitEntryHandled = 40000,
        /// Thoda 0010 failure helpers sub_6320 and sub_63FA write this from 31000 or 40000 after Undine's not-yet-worthy response. The earlier pre-camp failed-trial phase is 22000. Preserve the distinction and all retry behavior.
        UndineTrialFailedAfterAngelNight = 41000,
        /// Thoda 0010 sub_64C0 writes this from 31000, 40000 or 41000 after the Aquamarine, summon-technique awards and UndinePactFormed bit. The earlier 21000/22000 success path instead writes 23000. Fire's equal-valued phase is written earlier in its different pact ceremony.
        UndinePactCompleteAfterAngelNight = 50000,
    }
}

ids! {
    /// GlobalId::WindSealProgress; early Asgard phases preserve the optional Balacruf +500 offset.
    pub enum WindSealStage: u32 {
        /// Balacruf 0507 entry tests zero before sub_236B discusses the building and adds 500. Asgard 0031 separately tests below 1000 before its ruin-history conversation and adds 1000. The optional prior Balacruf visit therefore offsets the early Asgard phases by 500; this word is not a set of independent flags.
        BeforeAsgardRuinHistory = 0,
        /// Asgard 0031 event_1_03ED_r10 calls sub_0FB4 then adds 1000. The ordinary phase is 1000, or 1500 after the optional Balacruf visit. The following Harley/Linar bomb incident has its own increment; do not replace these additions with assignments.
        RuinHistoryConversationComplete = 1000,
        /// Asgard 0031 events_1_07D3_r14 and_1_07D4_r15 run alternate Harley/Linar introductions, sub_1FD1 and sub_31DE, then add 1000. From the preceding phase this yields 2000 or 2500. Asgard 0029 tests those exact values before the follow-up discussion.
        RuinBombIncidentComplete = 2000,
        /// Asgard 0029 entry accepts 2000 or 2500 before sub_1240 discusses the two men and the attempted destruction. The 2500 alternative retains the optional earlier Balacruf +500 offset. Entry then adds 1000 on either path; this is not a separate bomb scene or literal setter.
        RuinBombIncidentCompleteAfterBalacrufVisit = 2500,
        /// Asgard 0029 entry calls sub_1240 from 2000 or 2500, then adds 1000 and starts timer 33. Asgard 0046 accepts the resulting 3000..3999 range for Aisha explaining the sacrifice. Preserve the 3500 alternative and the separate permission-refused bit.
        BombIncidentFollowupComplete = 3000,
        /// Asgard 0046 sub_02AA explains Aisha's sacrifice, Linar opening the seal and the Balacruf writing, then adds 1000. The ordinary result is 4000 or 4500. Asgard 0031 uses that range for Raine volunteering as dancer; shared timer helpers also select 4000 <= progress < 5000.
        AishaSacrificeExplanationComplete = 4000,
        /// Asgard 0031 event_1_07D1_r13 accepts 4000..4500 before sub_5BDE's Raine-as-dancer agreement. The 4500 endpoint is the same explanation after the optional earlier Balacruf visit adds 500; Asgard 0046 adds 1000 to either incoming phase. Preserve the cumulative additions and the separate permission-refused flag.
        AishaSacrificeExplanationCompleteAfterBalacrufVisit = 4500,
        /// Asgard 0031 sub_5BDE has Raine volunteer as the dancer and the mayor consent, then adds 1000 before transfer to 0047. The resulting 5000..5999 range dispatches sub_3BA6's ritual and formation 18, and persists through its aftermath until inn 0033 advances it. It does not mean the guardian battle was already won at the addition.
        RaineDancerAgreementComplete = 5000,
        /// Asgard inn 0033 sub_0278 says Raine is still researching with Linar, then adds 1000, heals the party and removes Raine. The resulting 6000..6999 range selects the return conversation in 0046, where the map is deciphered and awarded. The inn addition itself does not grant the map.
        RaineResearchReturnPending = 6000,
        /// Asgard 0046 sub_0823 explains the deciphered map, restores Raine and grants MapOfBalacruf before adding 1000. Ordinary progression reaches 7000 or 7500. Balacruf 0507 explicitly accepts either, while timer helpers independently require current map possession and progress below 8000.
        BalacrufMapReceived = 7000,
        /// Balacruf 0507 entry explicitly accepts 7500 alongside 7000 for sub_2E03. This is the cumulative Asgard progression with the optional earlier sub_236B +500 visit offset. Map ownership remains an independent check in the shared timer helpers; no constant 7500 setter was recovered.
        BalacrufMapReceivedAfterEarlierVisit = 7500,
        /// Shared field timer helpers require 7000 <= WindSealProgress < 8000 and current MapOfBalacruf possession before selecting timer 33 state 3. This exclusive comparison bound includes both 7000 and 7500; it is not a recovered event or constant setter at 8000.
        BalacrufMapTimerUpperBound = 8000,
        /// Balacruf 0507 sub_2E03 instructs Colette to climb the central stairway and offer the Map at the altar, then writes 10000. The later oracle-stone opening and initial interior conversation remain separate phases.
        BalacrufAltarPlanAgreed = 10000,
        /// Balacruf 0507 event_1_07D0_r9 shows the Map responding, identifies the oracle stone and has Colette touch it. Scenery animation opens the entrance; the script writes 11000 before its shared helper and transfer to 0508. Preserve the original animations and transfer order.
        OracleStoneEntranceOpened = 11000,
        /// Balacruf 0508 sub_2394 plays Lloyd's exploration enthusiasm, Kratos's response and Colette hearing the wind, then writes 11400. Event_2_03E9_r1 later advances it before continuing deeper; this is not the guardian or Sheena battle aftermath.
        FirstInteriorConversationComplete = 11400,
        /// Balacruf 0508 event_2_03E9_r1 writes 11500 from 11400 before testing TowerOfManaSheenaEncounterSeen. The unseen branch transfers to 0507, whose entry selects sub_3C7E; the other branch directly writes 12000. Preserve the subsequent original transfer to 0509 and do not infer that the exterior cutaway has completed at this write.
        InitialInteriorExitMarked = 11500,
        /// Balacruf 0507 sub_3C7E writes 12000 after the Noishe growl and Sheena calling Corrine. Balacruf 0508 event_2_03E9_r1 also writes it when the TowerOfManaSheenaEncounterSeen bit skips that exterior cutaway. This phase does not prove that the cutaway played or that Sheena was defeated.
        ExteriorSheenaCutawayHandled = 12000,
        /// Balacruf 0510 entry calls sub_0447 for WindSealProgress <= 20000. That helper runs formation 14 and the branch-dependent Remiel blessing before writing 21000. This inclusive entry bound has no recovered constant setter and is not Water's equal-valued guardian-aftermath phase.
        GuardianEntryUpperBound = 20000,
        /// Balacruf 0510 sub_0447 runs formation 14 and Remiel's branch-dependent blessing/dialogue, then writes 21000 before its common helper and WindSealGuardianDefeated bit. ColetteAngelProgress 1/101/201 selects the second/third/fourth seal dialogue. Preserve this late write and the separate guardian flag.
        SealReleaseConversationComplete = 21000,
        /// Balacruf 0508 event_1_10D9_r29 runs formation 17, discusses Sheena's other world and shows her escape, then writes 22000 before its common helper and BalacrufSheenaEncounterSeen bit. The Tower Key/main-story branch selects different introductory dialogue; this does not change the assignment timing.
        SheenaBattleConversationComplete = 22000,
        /// Balacruf 0507 sub_415D discusses Colette's post-seal illness under its original angel-progress branches, agrees to rest and writes 30000 before camp 0522. The night completion and later Sylph pact remain separate phases.
        AngelNightCampPending = 30000,
        /// Camp 0522 sub_065F and retained camp 0240 sub_0048 update Colette's angel progression, advance WindSealProgress from 30000 to 31000 and return to Balacruf 0507. Camp 0240 retains its assignment-in-condition quirk. This does not establish a Sylph pact or a fixed ordinal night.
        PostSealAngelNightComplete = 31000,
        /// Balacruf 0510 sub_4BF5 tests battle result 3, plays Sylph's failure response and writes 41000. Entry permits retries at this value. Unlike Fire's equal-numbered trial-introduction phase, this is written after a failed trial, not before the battle.
        SylphTrialFailed = 41000,
        /// Balacruf 0510 sub_4BF5 writes 50000 near the success scene's end, after the vow, Opal, wind summon techniques, SylphPactFormed bit and conditional Elemental Summoner title. Battle formation, main-story dialogue and separate mana-pair earthquake bits keep their original conditions. Fire's equal-valued marker is written earlier in its ceremony.
        SylphPactComplete = 50000,
        /// Balacruf 0507 event_1_0834_r16 writes 60000 after the exterior earthquake and Lloyd/Raine/Sheena discussion, before restoration. Gnome-related dialogue and the earthquake trigger remain separately conditional; this is not the pact flag itself.
        ExitEarthquakeDiscussionComplete = 60000,
        /// Asgard 0031 sub_0FB4 tests this exact value to select Lloyd's correct ruin-history answer, crediting a guide, instead of Genis answering. No recovered constant setter or ordinary quest milestone uses 999999. The name describes this retained reader only and does not assert a normal reachable progression phase.
        RuinHistoryCorrectAnswerSelector = 999999,
    }
}

ids! {
    /// GlobalId::LightSealProgress; team/puzzle, guardian, camp and later Light-pact state remain separate.
    pub enum LightSealStage: u32 {
        /// Tower exterior 0361 entry tests zero, sets TowerOfManaVisited and selects sub_0766 or sub_1DAA according to the main story. Hima 0304 also reads zero for its dialogue. Normal introduction helpers later write 1000; Tower Key possession and other quest words remain independent.
        BeforeTowerIntroduction = 0,
        /// Luin 0448 event_0_0067_r11 accepts LightSealProgress == 1 or Desian Orb possession before the lost-Tower-Key monster hint, with additional book, timer and key conditions. No recovered constant setter uses 1. Preserve this original exact reader rather than replacing it with the introduction value 1000 or claiming a normal reachable milestone.
        LuinLostTowerKeyHintSelector = 1,
        /// Tower exterior 0361 sub_0766 and sub_1DAA write 1000 after their route-dependent introduction and lock/key discussion. The retained key-grant debug choice also sets it when progress <=1000. It does not prove that the key was acquired, the door opened or every introduction branch played.
        TowerIntroductionHandled = 1000,
        /// Shared field timer helpers test LightSealProgress >=1001 under BoltzmansBookRouteA ==40000 before selecting timer 49 state 3. This inclusive boundary excludes the 1000 introduction marker. No recovered event or constant setter uses 1001; preserve the original comparison and separate book-route condition.
        PostIntroductionTimerLowerBound = 1001,
        /// Library 0362 sub_09E3 identifies the oracle stone, demonstrates the three circles and agrees that some party members must remain behind. It writes 10000 before the shared restoration helper; party selection and the departure opening remain separate operations. ThreeMemberTeamAEnabled selects the original two- or three-member advance plan.
        LibraryTeamSplitPlanAgreed = 10000,
        /// Library 0362 sub_27D9 writes 11000 if progress is lower, before its shared timer helper, door-opening dialogue and party departure handling. This is the beginning of the split-party departure, not proof that the later puzzle or regrouping is complete. The separate team-selection lock and original party choices remain intact.
        SplitPartyDepartureStarted = 11000,
        /// Rooms 0363 sub_0391 and 0364 sub_04AD write 11500, set their respective return-location pending bits and transfer to exterior 0361. Their entry conditions retain the Balacruf encounter, Sheena-presence and room-0364 team-size checks. The exterior cutaway is still pending at this assignment.
        ExteriorSheenaCutawayPending = 11500,
        /// Exterior 0361 sub_392A shows Sheena moving toward the tower, writes 12000 after the fade, then clears the originating room bit and returns to 0363 or 0364. This silent cutaway is separate from the later Sheena battle and its encounter-seen flag.
        ExteriorSheenaCutawayComplete = 12000,
        /// Room 0365 event_0_FFFFFFFF_r4 handles actor 400 while progress <=12000, animates scenery and calls sub_0308 to remove that actor and start event 700, then writes 12100. Event 700 draws the light path and can call sub_0575; the subsequent path activation has its own early marker.
        Room0365LightSourceActivated = 12100,
        /// Room 0365 sub_0575 writes 12200 at its start, before the camera, light-path effects, actor replacements and collision restoration. The reflected-ray condition in event_2_02BC_r2 calls it. Preserve this early assignment; 12200 is not proof that the activation animation has finished.
        Room0365LightPathActivationStarted = 12200,
        /// Room 0366 entry at progress <=12200 initializes four pairs of persistent puzzle coordinates in words 0x0104..0x0120, then writes 12250. This marks the room setup, before the remote-team conversation and later light-puzzle solution. It does not grant an item or complete the puzzle.
        Room0366MirrorPositionsInitialized = 12250,
        /// Room 0366 event_2_044C_r7 writes 12300 when progress <13000 after its setup helper and before the first projector/remote-team conversation. The dialogue and Raine-team variants retain their original conditions. The later agreement and optional control switch occur after this early write.
        FirstRemoteTeamConversationStarted = 12300,
        /// Room 0366 event_2_044C_r7 writes 13000 after the two teams discuss the opened door, proceeding and regrouping. It then calls the team helper before offering the control-switch choice. It is not proof that the other team has already reached the library or that the full party has reunited.
        RemoteTeamAdvanceAgreed = 13000,
        /// Library 0362 entry in the 13000..13499 dispatch range calls sub_3437 to reconstruct team members. If its first team-member slot at i16[0x0816] is nonzero and progress ==13000, entry writes 13050 before further team/door setup. Preserve that guard; this entry marker is not full-party reunion.
        LibraryTeamReentryMarked = 13050,
        /// Room 0363 event_0_FFFFFFFF_r5 handles actor 400 while progress <13100, animates scenery and calls sub_03B4 to remove that actor and start light-path event 800, then writes 13100 and Target400RemovalMarked. The next reflected-path activation is a separate phase.
        Room0363LightSourceActivated = 13100,
        /// Room 0363 sub_0687 accepts exactly 13100, then writes 13200 before its light-path camera sequence, scenery actor replacements and collision restoration. Preserve the early assignment and exact prior-state test; it does not mean the animation has completed at that instruction.
        Room0363LightPathActivationStarted = 13200,
        /// Room 0366 event_0_FFFFFFFF_r15 handles actor 200 below 13300, animates scenery, sets the field countdown to 360 and calls sub_3453 to remove the actor and start light-path event 900. It then writes 13300. The three illuminated targets and final path activation have a later phase.
        Room0366LightSourceActivated = 13300,
        /// Room 0366 sub_3DB5 requires the three illuminated-target actors 603/604/605. After its light-path and platform sequence it writes 13400 before replacing the movable puzzle actors and restoring control. Its countdown-zero check only selects a sound and does not prevent completion. Team communication and reunion still follow.
        Room0366LightPuzzleComplete = 13400,
        /// Room 0366 sub_2AEF tells Lloyd that the other team has reached the inner area and asks him to regroup. After its remote-team and leader handling it writes 13500. Event_1_044D_r8 subsequently selects sub_2FA1; the full roster has not yet been restored at this assignment.
        RegroupCallComplete = 13500,
        /// Room 0366 sub_2FA1 performs the pedestal/door sequence and restores the saved party roster from bytes 0x0150..0x0155, then writes 13600. Entry uses >=13600 for the reunited setup. This is before the guardian and Remiel seal-release conversation, not a completed seal or Light pact.
        PartyReunited = 13600,
        /// Guardian room 0369 entry calls sub_09A4 when LightSealProgress <=20000 and LightSealGuardianDefeated is clear. The helper runs formation 15 and the seal-release scene before writing 21000. This inclusive bound has no recovered constant setter and is distinct from Water's equal-valued aftermath marker.
        GuardianEntryUpperBound = 20000,
        /// Guardian room 0369 sub_09A4 runs formation 15, the altar prayer, Luna's request for Aska and Remiel's angel-progress-dependent blessing. It writes 21000 after those conversations, before its common helper and LightSealGuardianDefeated bit. Second/third/final-pre-Tower dialogue remains conditional; this does not establish a Luna/Aska pact.
        SealReleaseConversationComplete = 21000,
        /// Library 0362 event_1_0FA0_r18 runs formation 17 and Sheena's other-world/escape conversation, then writes 22000 before its common helper and TowerOfManaSheenaEncounterSeen bit. SheenaEncounterBoltzmanBookVariantSelected chooses the introduction independently; the value is separate from the earlier silent exterior cutaway.
        SheenaBattleConversationComplete = 22000,
        /// Exterior 0361 sub_3A96 plays the angel-progress-dependent post-seal illness discussion, agrees to rest and writes 30000 before transfer to camp 0522. The seal ordinal and Sheena-presence variants remain conditional. This assignment does not mean the night conversation has completed.
        AngelNightCampPending = 30000,
        /// Camp 0522 sub_065F and retained camp 0240 sub_0048 update Colette's angel progression, advance LightSealProgress from 30000 to 31000 and return to the Tower of Mana exterior. Camp 0240 preserves its assignment-in-condition quirk. A Light pact and its later main-story progression remain separate.
        PostSealAngelNightComplete = 31000,
        /// Library 0362 entry accepts PartyReunited..99999 for its reunited scenery setup and TeamSelectionLocked bit. This inclusive reader bound has no recovered constant setter. Room 0366's guardian-room access uses a different, wider upper bound.
        LibraryReunitedSetupUpperBound = 99999,
        /// Room 0366 event_2_0BC0_r6 permits transfer to guardian room 0369 for PartyReunited..999999, after the room helper and camera setup. The other branch displays the blocked response. Preserve this wider inclusive bound separately from library 0362's 99999; neither is an additional completed seal or pact.
        GuardianRoomAccessUpperBound = 999999,
    }
}

ids! {
    /// GlobalId::BoltzmansBookRouteA, word 0xd0; distinct from book ownership and Pietro's cure.
    pub enum BoltzmanTowerKeyStage: u32 {
        /// Hima 0300 entry requires main story before PalmacostaFirstArrivalComplete and this word zero before the Sheena/Sophia exchange and private invitation in sub_0E8C/sub_1294. It then writes 1000. Three later Hima dialogue readers distinguish zero from nonzero without proving book or key ownership.
        BeforeSophiaInvitation = 0,
        /// Hima 0300 entry calls sub_0E8C and sub_1294, sets SophiaPrivateDiscussionInvited and writes 1000. Both friendship-choice branches invite the party to hear Sophia privately. The inn welcome, actual explanation of Pietro and acceptance bit occur later.
        SophiaPrivateDiscussionInvited = 1000,
        /// Hima 0303 event_0_00C8_r5 at 1000 has Sophia welcome the party to the inn, then writes 2000. Hima 0304 entry later selects sub_01FF at this value under the original early-story condition. This is not yet proof that the Pietro/Boltzman explanation has played.
        SophiaInnWelcomeComplete = 2000,
        /// Hima 0304 sub_01FF explains cursed Pietro and Boltzman, retains the separate help-choice bit, then writes 3000 on the LightSealProgress-zero branch after directions to the tower and Luin. The alternative 10000 path handles an already introduced tower. This marker does not imply acceptance of the separate healing request or possession of the book.
        TowerSearchPlannedBeforeFirstVisit = 3000,
        /// Hima 0304 sub_01FF writes 10000 after the already-visited-tower/Luin-key discussion. Tower 0361 sub_1DAA also advances 3000 to 10000 at its START, before the introduction and lock/key branches. Preserve that early write and the branch for an already possessed key; this phase does not guarantee any particular inquiry dialogue played.
        TowerKeyInquiryPhase = 10000,
        /// Luin church 0465 sub_018B says Pastor Piccaring holds the key and is on pilgrimage; the party agrees to follow him. It writes 20000 before the common helper and separate PiccaringPilgrimageLearned bit. The optional Pietro-help response remains independent of this progress word.
        PiccaringPilgrimageLearned = 20000,
        /// Asgard 0029 event_0_00D1_r23 at 20000 has the NPC say Pastor Piccaring went on pilgrimage to Balacruf Mausoleum, then writes 30000. This names learning his destination, not arriving there, obtaining the Tower Key or receiving Boltzman's Book.
        PiccaringBalacrufDestinationLearned = 30000,
        /// Balacruf 0507 event_0_00CE_r18 grants Tower Key before writing 40000, and sets TowerKeyReceivedFromBalacrufPastor later. The shared timer helpers read this value and independently test LightSealProgress >=1001. This records the key receipt rather than current inventory ownership, Boltzman's Book receipt or Pietro's cure.
        TowerKeyReceivedFromPiccaring = 40000,
    }
}

ids! {
    /// GlobalId::BoltzmansBookRouteB, word 0xd4; distinct from book ownership and Pietro's cure.
    pub enum PietroInfiltrationStage: u32 {
        /// Hima 0303 sub_02D4 has Sophia say Pietro passed away, deny belongings and give his grave location, then writes 11000 after restoration. The later grave scene reveals Pietro alive and exposes her lie. Name the report rather than an actual death; ranch-route and subsequent grave-trigger conditions remain separate.
        SophiaReportedPietroDead = 11000,
        /// Hima 0300 event_1_07D8_r9 at 11000 calls sub_165B: Pietro is alive, Sophia agrees to help infiltrate the ranch in exchange for curing him, explains the blocked escape route and grants Desian Orb. After the helper returns, the caller sets SophiaBoltzmanBookQuestExplained and writes 12000. The healing bargain has been agreed, not fulfilled; book receipt and Pietro's cure remain separate.
        RanchInfiltrationDealAgreed = 12000,
    }
}

ids! {
    /// GlobalId::AsgardRanchFirstPartRouteA, word 0xd8; first infiltration before the separate AsgardRanch word.
    pub enum AsgardRanchEarlyRouteStage: u32 {
        /// Luin 0447 entry plays the ruined-city reaction and sets LuinDestructionWitnessed, then advances the selected route to 1000 if lower. The main-story cutoff PalmacostaFirstArrivalComplete selects the early or later route. Other Luin introduction and reconstruction words remain independent.
        LuinDestructionWitnessed = 1000,
        /// Luin 0448 entry handles the attack-aftermath introduction and route-dependent Sheena conversation, including the original optional join responses, then writes 2000 in the route selected by the main-story cutoff. Ranch 0208 debug presets also select 2000 directly. This does not guarantee that Sheena joined or that a ranch infiltration has begun.
        LuinAftermathIntroductionComplete = 2000,
        /// Ranch exterior 0208 sub_0780 proposes disguising the party as Desians and hiding behind the trees to ambush the patrol. It advances this route from 2000 to 11000 after the discussion. The patrol battle and uniform preparation remain later phases.
        DesianDisguiseAmbushPlanAgreed = 11000,
        /// Ranch 0209 sub_0098 runs formation 27, discusses returning to Luin before discovery and advances this route from 11000 to 12000. Camp 0522 and retained 0241 subsequently handle the usable uniform and Raine disguise. This value does not mean that the camp preparation is finished.
        PatrolAmbushBattleAftermath = 12000,
        /// Camp 0522 entry calls sub_38CA at 12000, where Raine takes the usable female Desian uniform and everyone else will pose as prisoners, then advances this route to 21000 before returning to 0208. Retained camp 0241 sub_0103 and exterior 0208 debug presets also write it. The gate bluff is still a separate scene.
        RaineDisguisePreparationComplete = 21000,
        /// Ranch exterior 0208 sub_0CF6 has disguised Raine present the wanted Lloyd as a captured criminal and obtain permission to enter. It advances this route from 21000 to 31000 before its common helper. The interior establishing cutaway and Botta/Kvar encounters occur later.
        WantedPrisonerGateBluffComplete = 31000,
        /// Ranch 0213 entry at 31000 runs sub_1C8B: camera movement establishes the interior, then this route advances to 31500 before transfer to 0211. The cutaway has no dialogue and is not a completed Botta encounter or a defeated Kvar.
        InnerRanchArrivalCutawayComplete = 31500,
        /// Ranch 0211 sub_0F0D has Colette hear voices and warn the party, then advances this route from 31500 to 32000 BEFORE Botta appears, retreats in the presence of Kratos, and Kvar confronts the party. Preserve that early assignment rather than treating it as a completed confrontation.
        VoicesBeforeBottaEncounterHeard = 32000,
        /// Ranch 0211 sub_0F0D runs the Botta/Kratos exchange, Kvar revealing the Angelus Project and Colette helping the party leave the room. It advances this route from 32000 to 33000 near the end, before its common helper and the 0213 scene. The later Exsphere-host revelation and escape from the ranch remain separate.
        BottaKvarRoomEscapeComplete = 33000,
        /// Ranch 0213 sub_1D96 has Kvar reveal Exsphere cultivation, Anna and the Key Crest-less transformation. Sheena uses her final Guardian to get the party out under both Sheena-presence variants. The route advances from 33000 to 34000 after that scene. This is not a won Kvar boss battle or proof of a newly joined Sheena.
        ExsphereOriginsAndSheenaEscapeComplete = 34000,
        /// Ranch exterior 0208 sub_1274 thanks Sheena and agrees to return to Luin, then advances this route from 34000 to 35000. The camp Exsphere discussion and Luin aftermath follow. The later second-infiltration word GlobalId::AsgardRanch remains separate.
        ReturnToLuinAfterEscapeAgreed = 35000,
        /// Camp 0522 sub_3BC0 and retained 0241 offer the original Exsphere-response choice. The rejection/private-reflection branch calls sub_40B1 or sub_148D, where Kratos discusses being useful to someone who can end the cycle, then advances this route from 35000 to 40000. The agreement branch can return to Luin at 35000 instead; preserve both aftermath entry phases.
        KratosPrivateExsphereDiscussionComplete = 40000,
        /// The early route has no recovered constant setter at 41000. House of Salvation 0084 and Luin 0448 compare below it for NPC dialogue, Hima 0300 tests >=41000, and Luin's Pietro-hint NPC also accepts exact 41000. Normal early-route aftermath advances from 35000 or 40000 directly to 50000. This names the retained reader boundary, not an invented early-route event.
        PostEscapeNpcDialogueBoundary = 41000,
        /// Luin 0448 entry accepts 35000 or 40000, handles the Exsphere aftermath and conditionally joins Sheena, restores the party through sub_40F2 and writes the early route to 50000. Ranch 0217 has a retained party debug preset at the same value. Luin exits later decide to return to Hima using a separate bit without changing this word. It is earlier than the later route's equal-valued Hima decision.
        LuinEscapeAftermathComplete = 50000,
    }
}

ids! {
    /// GlobalId::AsgardRanchFirstPartRouteB, word 0xdc; first infiltration before the separate AsgardRanch word.
    pub enum AsgardRanchLaterRouteStage: u32 {
        /// Luin 0447 entry plays the ruined-city reaction and sets LuinDestructionWitnessed, then advances the selected route to 1000 if lower. The main-story cutoff PalmacostaFirstArrivalComplete selects the early or later route. Other Luin introduction and reconstruction words remain independent.
        LuinDestructionWitnessed = 1000,
        /// Luin 0448 entry handles the attack-aftermath introduction and route-dependent Sheena conversation, including the original optional join responses, then writes 2000 in the route selected by the main-story cutoff. Ranch 0208 debug presets also select 2000 directly. This does not guarantee that Sheena joined or that a ranch infiltration has begun.
        LuinAftermathIntroductionComplete = 2000,
        /// Ranch exterior 0208 sub_0780 proposes disguising the party as Desians and hiding behind the trees to ambush the patrol. It advances this route from 2000 to 11000 after the discussion. The patrol battle and uniform preparation remain later phases.
        DesianDisguiseAmbushPlanAgreed = 11000,
        /// Ranch 0209 sub_0098 runs formation 27, discusses returning to Luin before discovery and advances this route from 11000 to 12000. Camp 0522 and retained 0241 subsequently handle the usable uniform and Raine disguise. This value does not mean that the camp preparation is finished.
        PatrolAmbushBattleAftermath = 12000,
        /// Camp 0522 entry calls sub_38CA at 12000, where Raine takes the usable female Desian uniform and everyone else will pose as prisoners, then advances this route to 21000 before returning to 0208. Retained camp 0241 sub_0103 and exterior 0208 debug presets also write it. The gate bluff is still a separate scene.
        RaineDisguisePreparationComplete = 21000,
        /// Ranch exterior 0208 sub_0CF6 has disguised Raine present the wanted Lloyd as a captured criminal and obtain permission to enter. It advances this route from 21000 to 31000 before its common helper. The interior establishing cutaway and Botta/Kvar encounters occur later.
        WantedPrisonerGateBluffComplete = 31000,
        /// Ranch 0213 entry at 31000 runs sub_1C8B: camera movement establishes the interior, then this route advances to 31500 before transfer to 0211. The cutaway has no dialogue and is not a completed Botta encounter or a defeated Kvar.
        InnerRanchArrivalCutawayComplete = 31500,
        /// Ranch 0211 sub_0F0D has Colette hear voices and warn the party, then advances this route from 31500 to 32000 BEFORE Botta appears, retreats in the presence of Kratos, and Kvar confronts the party. Preserve that early assignment rather than treating it as a completed confrontation.
        VoicesBeforeBottaEncounterHeard = 32000,
        /// Ranch 0211 sub_0F0D runs the Botta/Kratos exchange, Kvar revealing the Angelus Project and Colette helping the party leave the room. It advances this route from 32000 to 33000 near the end, before its common helper and the 0213 scene. The later Exsphere-host revelation and escape from the ranch remain separate.
        BottaKvarRoomEscapeComplete = 33000,
        /// Ranch 0213 sub_1D96 has Kvar reveal Exsphere cultivation, Anna and the Key Crest-less transformation. Sheena uses her final Guardian to get the party out under both Sheena-presence variants. The route advances from 33000 to 34000 after that scene. This is not a won Kvar boss battle or proof of a newly joined Sheena.
        ExsphereOriginsAndSheenaEscapeComplete = 34000,
        /// Ranch exterior 0208 sub_1274 thanks Sheena and agrees to return to Luin, then advances this route from 34000 to 35000. The camp Exsphere discussion and Luin aftermath follow. The later second-infiltration word GlobalId::AsgardRanch remains separate.
        ReturnToLuinAfterEscapeAgreed = 35000,
        /// Camp 0522 sub_3BC0 and retained 0241 offer the original Exsphere-response choice. The rejection/private-reflection branch calls sub_40B1 or sub_148D, where Kratos discusses being useful to someone who can end the cycle, then advances this route from 35000 to 40000. The agreement branch can return to Luin at 35000 instead; preserve both aftermath entry phases.
        KratosPrivateExsphereDiscussionComplete = 40000,
        /// Luin 0448 entry accepts 35000 or 40000, handles the Exsphere aftermath and conditional Sheena join, restores the party through sub_4161 and writes the later route to 41000. Shared timer and NPC readers use this phase. The exit's Pietro/Hima decision has not yet completed; the early route uses 50000 for its corresponding aftermath.
        LuinEscapeAftermathComplete = 41000,
        /// Luin 0447 and 0448 exit events at later-route 41000 discuss heightened ranch security, Sheena identifies Pietro in Hima and the party agrees to go there. They advance this word to 50000 and set PietrosHimaLocationLearned at their original different positions around restoration. Hima's Sophia report and subsequent infiltration bargain use the separate PietroInfiltrationStage word. The early route's 50000 is written before its Hima decision.
        HimaPietroVisitAgreed = 50000,
    }
}

ids! {
    /// GlobalId::PalmacostaRanchAssaultRoute; original progress is independent of other quest words.
    pub enum PalmacostaRanchAssaultStage: u32 {
        /// Palmacosta 0200 sub_0091 writes this after choosing to attack the ranch despite Neil's trap warning. The Neil-assault conversation-pending bit and the main-story trap-response stage remain separate writes.
        AssaultChosen = 1000,
        /// Ranch 0198 sub_0A17: Neil asks to accompany the assault, Lloyd agrees and the party discusses preparation and the inability to leave. This precedes opening the entrance doors, and clears the separate pending-conversation bit.
        NeilAssaultEscortAgreed = 1100,
        /// Both 0198 door interactions animate opposite scenery-node pairs, despawn actors 250/401 and then write this. Preserve the two original animation-resource slot pairs and the shared phase.
        RanchEntranceOpened = 1200,
        /// Ranch 0204 sub_05BF assigns Neil and the arriving army to free the captives while the party rescues Chocolat. The conditional Blue Card flag/item grant precedes this route update. It does not mean every captive has already evacuated.
        NeilCaptiveRescuePlanAgreed = 2000,
        /// Ranch 0206 sub_0E07 runs formation 20, has Chocolat thank the party and agree to guide them toward the control room, then advances the assault word here. The 0207 debug menu also directly selects this phase before the next scene.
        ChocolatRescuedAndGuiding = 3000,
        /// Ranch 0207 sub_02E1 writes this at its start, before the control-room dialogue and Magnius revealing the surrounding guards. Debug choices in 0201/0207 also write it directly. It is not the later captive-projector observation or a won battle.
        ControlRoomAmbushStarted = 4000,
        /// Ranch 0201 sub_1052 explains the distant-image projector and shows Neil and the captives trapped, then advances from ControlRoomAmbushStarted. The following Chocolat abduction and guard battle occur later.
        CaptiveTrapProjectionSeen = 4100,
        /// Ranch 0207 sub_0EF4 marks ChocolatTakenByMagnius, runs formation 21 and advances here from CaptiveTrapProjectionSeen before calling the Magnius confrontation. The Pass award in this helper is conditional on the Dorr-route word, not a reward implied by this assault phase.
        ChocolatAbductionGuardBattleAftermath = 5000,
        /// Ranch 0207 sub_2A3C advances to this at its start, before Magnius's challenge and formation 22. Preserve this early write rather than interpreting it as proof of a defeated boss.
        MagniusConfrontationStarted = 6000,
        /// Ranch 0207 sub_2A3C writes this after formation 22 and Magnius's defeat conversation, before the Chocolat-removal cutaway in 0198. The cutaway has a separate completion value.
        MagniusBattleAftermath = 6100,
        /// Ranch 0198 sub_1A09 shows Chocolat escorted away by guards, including the guard strike and Lloyd's offscreen Dammit line. It writes this after the cutaway fade and before returning to 0207. The 0207 debug menu can select the same phase directly.
        ChocolatRemovalCutawayComplete = 6200,
        /// Ranch 0207 sub_348F opens the captive route, discusses dangerous Exspheres and Dirk, starts the ten-minute self-destruct escape and plays Rodyle's Magnius cutaway before this write. Neil's later confirmation that the captives reached Palmacosta is a separate phase.
        SelfDestructEscapeActive = 7000,
        /// Ranch 0198 sub_1612: Neil says the captives have all been moved to Palmacosta and the party warns him about the explosion, then this is written. The party's own escape aftermath in 0199 still follows.
        NeilCaptiveEvacuationConfirmed = 8000,
        /// Ranch 0199 sub_04F1: after the escape, Neil agrees to contact Dirk about Exsphere removal and asks to accompany the party to confront Dorr. This intermediate write is immediately followed by DorrVisitAfterRanchReady; preserve both original assignments.
        NeilDorrVisitJoinAgreed = 9000,
        /// Ranch 0199 sub_04F1 writes this immediately after NeilDorrVisitJoinAgreed, before restoration. It marks the post-ranch journey to Dorr, and is the authored terrain's assault-route destroyed-ranch threshold. Dorr's confrontation and farewell have not yet completed.
        DorrVisitAfterRanchReady = 10000,
        /// Palmacosta 0066 event 2000 calls sub_019D: Dorr explains Clara, Kilia is exposed, formation 19 runs, Dorr asks for help and the Pass is granted. The helper returns before this write. The optional Clara response and the later Neil farewell remain distinct.
        DorrKiliaConfrontationComplete = 14000,
        /// Palmacosta 0065 sub_22A6: Neil discusses reorganizing the government, the party vows to find Chocolat and their farewell concludes before this phase and the main PalmacostaCrisisAftermath stage. This is the assault-first route's final aftermath, separate from Dorr-first ranch progression.
        CrisisAftermathComplete = 15000,
    }
}

ids! {
    /// GlobalId::PalmacostaRanchDorrRoute; original progress is independent of other quest words.
    pub enum PalmacostaRanchDorrStage: u32 {
        /// Palmacosta 0200 sub_0091 writes this after choosing to return to the city and investigate Dorr instead of attacking first. Neil stays to watch the ranch. The separate route-choice bit and main-story stage retain their original order.
        DorrInvestigationChosen = 1000,
        /// Ranch 0198 event_0_0064_r13 calls sub_10C1 for 1000..4000: Neil says he will wait for the party's return. The exact 5000 branch instead begins the card-key infiltration discussion. This is the inclusive pre-card-key dialogue bound, with no recovered constant setter at 4000.
        NeilWaitBeforeCardKeyUpperBound = 4000,
        /// Palmacosta 0066 event 2000 calls sub_28BB: Dorr's motive is revealed, Kilia's formation 19 runs, Dorr supplies the Card Key and passcode 3341, and the party resolves to rescue Chocolat. This is written after the helper. Helping Clara remains an optional response, not a fact implied by this value.
        DorrConfrontationCardKeyReceived = 5000,
        /// Ranch 0198 sub_0E46: Neil infers Dorr's fate, agrees to aid the Governor-General's last wish and Raine proposes using Dorr's key. This follows the dialogue and precedes the restoration helpers; it is not an assault-route stage or an evacuated ranch.
        NeilCardKeyInfiltrationAgreed = 6000,
        /// Ranch 0204 sub_05BF assigns Neil and the army to free the captives while the party rescues Chocolat. The conditional Blue Card flag/item grant precedes the Dorr-route update from 6000 to 7000. The captives are not yet confirmed evacuated.
        NeilCaptiveRescuePlanAgreed = 7000,
        /// Ranch 0206 sub_0E07 runs formation 20, has Chocolat thank the party and agree to guide them to the control room, then advances the Dorr-route word from 7000 to 8000. The 0207 debug menu also directly selects 8000 before the following scene.
        ChocolatRescuedAndGuiding = 8000,
        /// Ranch 0207 sub_02E1 advances the Dorr-route word from 8000 at its start, before the control-room dialogue and Magnius revealing the surrounding guards. The later projector observation has its own 9100 phase; this is not a won battle.
        ControlRoomAmbushStarted = 9000,
        /// Ranch 0201 sub_1052 explains the distant-image projector and shows Neil and the captives trapped, then advances the Dorr word from 9000 to 9100. The 0207 debug menu can select this phase directly. The following Chocolat abduction and guard battle occur later.
        CaptiveTrapProjectionSeen = 9100,
        /// Ranch 0207 sub_0EF4 marks ChocolatTakenByMagnius, runs formation 21 and advances the Dorr word from 9100 before calling the Magnius confrontation. The original 9100 condition also enables the Pass award in this helper; the corresponding assault phase does not imply that reward.
        ChocolatAbductionGuardBattleAftermath = 10000,
        /// Ranch 0207 sub_2A3C advances the Dorr word from 10000 at its start, before Magnius's challenge and formation 22. This is the confrontation start, not proof that the boss was defeated.
        MagniusConfrontationStarted = 11000,
        /// Ranch 0207 sub_2A3C advances the Dorr word after formation 22 and Magnius's defeat conversation. The Chocolat-removal cutaway in 0198 follows and has its own completion value.
        MagniusBattleAftermath = 11100,
        /// Ranch 0198 sub_1A09 shows Chocolat escorted away by guards, including the guard strike and Lloyd's offscreen Dammit line. It advances the Dorr word from 11100 after the cutaway fade, before returning to 0207. The direct 6200 debug preset belongs to the assault word, not this value.
        ChocolatRemovalCutawayComplete = 11200,
        /// Ranch 0207 sub_348F opens the captive route, discusses Exsphere removal and Dirk, starts the ten-minute self-destruct escape and plays Rodyle's Magnius cutaway. It then advances the Dorr word from 11200. Neil's later confirmation of captive evacuation remains separate.
        SelfDestructEscapeActive = 12000,
        /// Ranch 0198 sub_1612: Neil says all captives have been moved to Palmacosta and the party warns him about the explosion, then advances the Dorr word from 12000. The party's own escape aftermath in 0199 still follows.
        NeilCaptiveEvacuationConfirmed = 13000,
        /// Ranch 0199 sub_04F1: after escaping, Neil hears the Exsphere-removal request for Dirk and promises to receive the party in Palmacosta. This intermediate marker is immediately followed by CrisisAftermathComplete; preserve both writes.
        RanchEscapeAftermathMarked = 14000,
        /// Ranch 0199 sub_04F1 writes this immediately after RanchEscapeAftermathMarked, then writes the main PalmacostaCrisisAftermath stage. Dorr was confronted before the ranch on this route. Its timing differs from the assault word's equal-valued village farewell.
        CrisisAftermathComplete = 15000,
    }
}

ids! {
    /// GlobalId::AsgardRanch; original progress is independent of other quest words.
    pub enum AsgardRanchStage: u32 {
        /// Asgard ranch 0208 entry interaction 7003 tests this initial value before marking the transfer into 0211. It is the AsgardRanch word, separate from the two earlier Luin/Pietro route words and the main story.
        BeforeOrbEntry = 0,
        /// Asgard 0208 sub_188C discusses Pietro's mechanism and Desian Orb, then writes this before transfer to 0211. The silent entry interaction also advances zero to this same phase before that transfer. It does not prove the later guard battle or team selection completed.
        OrbEntryMarked = 1000,
        /// Asgard 0211 sub_3676 runs formation 29 and writes this before displaying the ranch map, explaining the guard/conveyor systems and asking Lloyd to choose teams. It is the briefing start, not completed party assignment.
        RanchMapBriefingStarted = 2000,
        /// Asgard 0211 sub_4B04 completes the chosen and remaining members' reactions and the optional refresher explanation, then writes this before restoration. The 0213 debug menu also directly chooses this phase. The team-slot flags and subsequent puzzle state remain separate.
        TeamsAssigned = 3000,
        /// Asgard 0212 terminal 3100 at TeamsAssigned asks to shut down the system that turns captives into monsters. Choosing Shutdown writes this before the in-progress/completed announcement. Preserve the original early write and the cancel paths.
        MonsterProductionShutdownRequested = 3010,
        /// Asgard 0213 sub_428B writes this only when all six LongRangeFireTarget400..405Cleared flags are set. These puzzle bits remain independent; the stage does not imply the support team has already deactivated both guard switches.
        ConveyorTargetsCleared = 3020,
        /// Asgard 0214 trigger 6001 saves team/leader flags, changes the selected group and writes this before transferring to 0211 for the support team. The 0211 debug preset can also write it directly; do not infer the normal team-switch operations from that preset alone.
        SupportTeamArrivalPending = 3100,
        /// Asgard 0211 sub_0921 stages the support team's Let's get started / do our part lines, then writes this. Their following guard encounter, captive rescue and switch operations have separate phases.
        SupportTeamActivated = 3110,
        /// Retained base 0210 sub_0826 and interaction 300 both advance here after their guard battles, using formations 30 and 1 respectively. Preserve the differing encounters and their shared continuation, rather than claiming all ranch guards were defeated.
        SupportTeamGuardBattleAftermath = 3200,
        /// Asgard 0216 sub_0487 runs formation 30, the selected support leader identifies the party and tells captives to follow them, and the group departs before this write. The Dorr-route Chocolat/Iselia inquiry remains conditional. Guard-switch and transporter phases are still separate.
        CaptiveEvacuationStarted = 4000,
        /// Asgard 0214 trigger 6001 at CaptiveEvacuationStarted displays Guard system operational / Transmission halted and the leader's blocked-system line. It writes this before transfer to 0215. This does not mean the guard switches have been deactivated.
        TransporterGuardSystemDenied = 4100,
        /// Asgard 0215 entry advances the 4100 return here before transferring back to 0214. Its sub_0F0C also restores the attacking party/leader and writes this before the same destination. Preserve both entry paths and their different positions.
        TransporterReturnPending = 4200,
        /// Asgard 0214 sub_0CE7: Lloyd says the transporter moved and calls out to Kvar, then writes this. The following travel and Kvar confrontation occur later; this value is not proof that Kvar was defeated.
        KvarTransporterActivated = 5000,
        /// Asgard 0208 sub_15C2 chooses Asgard rather than ruined Luin so Colette can rest, then writes this. It is the post-ranch rest departure and the authored blocked-ranch threshold; the inn humanity discussion remains a later phase.
        AsgardRestDepartureAgreed = 6000,
        /// Asgard inn 0039 entry at 6000 calls sub_00FD: the party discusses Colette losing human sensations and her resolve to regenerate the world, with an optional Unicorn Horn suggestion. It clears the Sophia invitation bit and writes this before the following helper.
        ColetteHumanityDiscussionComplete = 11000,
        /// Ranch 0213 entry accepts ConveyorTargetsCleared..999999 before hiding scenery nodes 21..41, distinct from the earlier 3000/3010 setup. This names that cleared-conveyor scenery reader's inclusive upper limit; it has no recovered constant setter or new ranch event.
        ClearedConveyorSceneryUpperBound = 999999,
    }
}

ids! {
    /// GlobalId::BookOfRegeneration; each quest word has an independent value domain.
    pub enum BookOfRegenerationStage: u32 {
        /// Hakonesia 0124 uses 0..9999 for Koton's pre-pursuit passage-fee dialogue and collection-touch response. The upper limit is one below HakonesiaImposterPursuitAgreed. Zero is the inclusive lower bound of those readers; no book-receipt event or constant setter at zero is asserted.
        PrePursuitDialogueLowerBound = 0,
        /// Palmacosta 0054 event 2001 writes this alongside the main PalmaPotionReplacementAgreed stage after Colette agrees to replace the imposters' potion. This quest word later tracks the book; it does not identify book possession.
        PalmaPotionReplacementAgreed = 1000,
        /// Palmacosta 0063 sub_22AA writes this after Desians threaten Chocolat and Cacao over gel prices, and the shop is restored. It is the shop encounter during the potion sequence, not a book handover.
        CacaoShopDesianThreatSeen = 2000,
        /// Palmacosta 0054 event 2003 writes this after sub_207F's potion delivery, alongside the main PalmaPotionDelivered stage. The Dorr/pastor information flags then determine whether the same event can advance to the Hakonesia pursuit plan.
        PalmaPotionDelivered = 3000,
        /// Five setting paths in Palmacosta 0054/0056/0065 identify the false Chosen and plan to pursue them to Hakonesia Peak for the Book of Regeneration. Their prior Dorr, pastor and potion conditions remain distinct. This is the pursuit plan, not Koton's later agreement to show the book.
        HakonesiaImposterPursuitAgreed = 10000,
        /// Hakonesia 0124 event_0_00C9_r1 handles exact pursuit phase 10000 separately, then accepts 10001..39999 for sub_06F5: Koton asks for money and the Spiritua Statue. The first ordinary phase in that range is KotonStatueExchangeRequested. This inclusive reminder bound has no recovered setter at 10001.
        StatueDemandReminderLowerBound = 10001,
        /// Hakonesia 0124 sub_02F8: Koton says he bought the book from the Chosen and demands the House of Salvation's Spiritua Statue before he will consider showing it. The original restoration helper follows the write.
        KotonStatueExchangeRequested = 20000,
        /// Salvation 0095 sub_16D8 and 0099 sub_144E reveal the displayed statue is a fake, locate the real one at Thoda and discuss freezing the geyser with Genis's magic. The 0095 branch can still lack Icicle; this phase does not prove that technique or statue has been obtained.
        ThodaStatueRecoveryPlanAgreed = 30000,
        /// Thoda 0006 sub_30CC writes this after Genis's Icicle line and fade, before restoring control and spawning the timed retrieval events 7000/7100/7200. It persists afterward into Koton's exchange branch. This early write is not proof that the statue retrieval has already succeeded.
        ThodaStatueRetrievalPhase = 40000,
        /// Hakonesia 0124 event 201 writes this after sub_0719 on the pre-Rheaird-departure branch. Koton accepts the statue in dialogue but explicitly only allows the party to read the book; Colette or Raine reads the seal passages. Preserve the separate late-conversation flag and later Golden Armor conditions; this is not an inventory grant of the book.
        BookConsulted = 50000,
        /// Retained shared timer helpers use the exclusive comparison BookOfRegeneration < 50001 with a >= HakonesiaImposterPursuitAgreed lower bound. The range includes BookConsulted 50000. No constant setter for this limit is recovered; it is not an additional quest event.
        ConsultationTimerUpperBound = 50001,
    }
}

ids! {
    /// GlobalId::LinkiteTree; each quest word has an independent value domain.
    pub enum LinkiteTreeStage: u32 {
        /// Tower of Mana 0361 sub_539B tests zero separately from EarlyHospitalityReceived before its Aska/Nova search discussion. The zero branch says Nova was absent when the party met the family; the 500 branch instead identifies the zoologist and his family. NovaSearchSuggested independently selects the repeat reminder. This is the initial tracked caravan progress, with no claim that the family has never been encountered.
        BeforeCaravanProgress = 0,
        /// Nova caravan 0243 entry before the first-seal departure phase calls sub_02A9's early rest invitation and hospitality flag, then writes 500. The full Nova family inquiry starts later at 10000.
        EarlyHospitalityReceived = 500,
        /// Nova caravan 0243 entry after Ossa and the three retained 0239 destination branches call the Nova introduction and write this base. Nova explains the glowing bird and directs Lloyd to ask his family. Later first interviews add Sarah 1000, May 3000 and Alduin 6000 to this word.
        FamilyInquiryStarted = 10000,
        /// The standard family-inquiry base 10000 plus Sarah's first-interview addition 1000. Sarah explains the bird must live somewhere inaccessible on foot. Both caravan copies recognize this value when suppressing a duplicate Sarah interview; it is a computed combination, not a constant-written milestone.
        SarahClueCollected = 11000,
        /// The standard inquiry base plus May's addition 3000. Her first interview grants a Linkite Nut and discusses its sound. Both caravan copies recognize this value when suppressing a duplicate May interview; later clue combinations have their own values.
        MayNutReceived = 13000,
        /// Computed as the inquiry base 10000 plus Sarah 1000 and May 3000. Both caravan copies test this combination in the Sarah and May repeat-interview gates. Preserve the additions and independent original item operation, rather than replacing them with a literal assignment.
        SarahClueAndMayNutCollected = 14000,
        /// The standard inquiry base plus Alduin's addition 6000. He recounts the Ossa peak, strong wind, the tree-nut melody and the vanished tree. Both caravan copies recognize this value when suppressing a duplicate Alduin interview.
        AlduinClueCollected = 16000,
        /// Computed as inquiry base 10000 plus Sarah 1000 and Alduin 6000. Both caravan copies test this in their repeat-interview gates. It does not include May's nut interview and grant.
        SarahAndAlduinCluesCollected = 17000,
        /// Computed as inquiry base 10000 plus Alduin 6000 and May 3000. Both caravan copies test this in their repeat-interview gates. It does not include Sarah's inaccessible-location clue.
        AlduinClueAndMayNutCollected = 19000,
        /// The standard base 10000 plus Sarah 1000, May 3000 and Alduin 6000. Either caravan copy calls the summary helper when the last first interview reaches this exact sum. The requirements memo and later revival/flute plans are separate writes; preserve the original arithmetic and equality checks.
        FamilyCluesCollected = 20000,
        /// Nova 0239 sub_0AA6 and 0243 sub_1433 write this on their no-Wing-Pack branch after Raine summarizes the inaccessible tree, melody, wind and revival problem. NovaAskaRequirementsRecorded is set separately. Obtaining Rheairds later does not by itself rewrite this quest word.
        InitialAskaRequirementsRecorded = 22000,
        /// Linkite Tree 0523 sub_20BD identifies the dead tree and has Raine and Lloyd decide to return to Nova to reconfirm the information, then writes this. This is the request for a renewed discussion, not a revived tree or completed flute plan.
        DeadTreeReconfirmationRequested = 22500,
        /// Six Nova conversation paths write this after discussing the nut, Exsphere, Unicorn Horn and summon-spirit revival plan, when the flute-idea branch has not advanced to 30000. The party can still lack the tree location, Exsphere shard or Sylph pact; the separate flags and item checks remain authoritative.
        RevivalPlanRecorded = 25000,
        /// Nine paths across both Nova copies and the tree scene write this after Lloyd proposes making a flute like Mithos's with Presea's help. They depend on the main PalmacostaMithosFluteDebriefComplete stage, but can still lack the tree or revival resources. This is a plan, not an ocarina grant or Aska pact.
        FluteConstructionPlanAgreed = 30000,
        /// Linkite Tree 0523 event 100 writes this after the revival/construction helpers and the Linkite Ocarina item grant. The subsequent helper, field reload and Aska-related handling remain separate. This word also selects the restored-tree world model; it does not establish the Light pact.
        LinkiteOcarinaReceived = 40000,
    }
}

ids! {
    /// GlobalId::LuinRebuilding, word 0x00f8. Credited gald lives separately in
    /// GlobalId::LuinDonationProgress (0x00fc); exit handlers apply queued goals.
    pub enum LuinRebuildingStage: u32 {
        /// Original Luin and world readers use equality with zero for the undamaged town. The authored world model also selects normal Luin below Ruined. This is the initial reconstruction word, not the main story or donation total.
        Undamaged = 0,
        /// Original Luin entries treat 1000..3000 as ruins and shared timer helpers use this lower bound. Camp 0522 sub_065F writes it when the updated angel progress equals 201; retained camp 0240 sub_0048 instead assigns 201 inside its condition and consequently writes it on that path. These are camp-sequence writes, not merely debug presets. Preserve the 0240 assignment bug and the later, separate Pietro pledge/donation stages.
        Ruined = 1000,
        /// Hima 0304 event 201, after PietroHealed, writes this after Pietro vows to use his saved life to rebuild Luin. Luin creates Pietro from this threshold; the first reconstruction-fund conversation advances to ReconstructionFundActive.
        PietroRebuildingPledgeHeard = 1500,
        /// Pietro's first Luin fund conversation writes this at its start, before explaining the need for people and money or asking for a donation. Reaching 5000 credited gald only queues an update; leaving town then applies FundingGoal5000Applied.
        ReconstructionFundActive = 1600,
        /// All eight retained Luin exit handlers advance from ReconstructionFundActive to this when the local RebuildingUpdateOnExitRequested bit is set. The donation helper queues that update at 5000 cumulative credited gald. The next goal is 17000; these numbers are credited totals, not rebuilding stage IDs or required single payments.
        FundingGoal5000Applied = 2000,
        /// Luin exits advance from FundingGoal5000Applied to this after the queued 17000 credited-gald goal. The next goal is 38500. The original snapshot permits one rebuilding-phase transition per exit, with the separate donation counter and request bit unchanged.
        FundingGoal17000Applied = 3000,
        /// Luin exits apply the queued 38500 credited-gald goal after FundingGoal17000Applied. Pietro's 10000..99999 dialogue says restoration is beginning little by little, and the entry switches to the early reconstruction layout. The next funding goal is 53500.
        EarlyReconstructionStarted = 10000,
        /// Luin exits advance from EarlyReconstructionStarted after the queued 53500 credited-gald goal. This remains within the early reconstruction layout/dialogue range. The next goal is 73500.
        FundingGoal53500Applied = 11000,
        /// Luin exits advance from FundingGoal53500Applied after the queued 73500 credited-gald goal. This remains within the early reconstruction range. The next goal is 98500.
        FundingGoal73500Applied = 12000,
        /// Luin exits advance from FundingGoal73500Applied after the queued 98500 credited-gald goal. This remains within the early reconstruction range. The next goal is 133500, which enables work throughout the city.
        FundingGoal98500Applied = 13000,
        /// Authored scripts/world/rules.sym preserves this existing model boundary between LuinRebuilding and Luin. No direct original field assignment or comparison at 40000 is recovered. This names that authored rendering threshold, not an additional normal rebuilding phase.
        WorldModelRestoredBoundary = 40000,
        /// Luin exits apply the queued 133500 credited-gald goal after FundingGoal98500Applied. Pietro's 100000..199999 dialogue says work can begin in all districts. The next 173500 goal is additionally gated by main-story SylvarantGateReturnDebriefComplete.
        AllDistrictsRebuilding = 100000,
        /// Luin exits apply the queued 173500 credited-gald goal from AllDistrictsRebuilding. Its donation helper requires main-story SylvarantGateReturnDebriefComplete before queuing that goal. Pietro's dialogue says basic everyday functions are restored; the next goal is 218500.
        BasicFunctionsRestored = 200000,
        /// Luin exits apply the queued 218500 credited-gald goal from BasicFunctionsRestored. Pietro's dialogue says city functions are almost completely restored, with a little more to go. The next goal is 268500.
        CityFunctionsNearlyRestored = 300000,
        /// Luin exits apply the queued 268500 credited-gald goal from CityFunctionsNearlyRestored. Pietro announces that Luin has been rebuilt and separately explains the expansion plan under LuinExpansionPlanExplained. This phase does not imply that later expansion or statue funding is complete.
        CityRebuilt = 400000,
        /// Luin exits apply the queued 343500 credited-gald goal from CityRebuilt. Pietro describes the expansion as proceeding smoothly. The next goal is 443500, leading to the expanded-city statue phase.
        ExpansionInProgress = 500000,
        /// Luin exits apply the queued 443500 credited-gald goal from ExpansionInProgress. Subsequent 448500, 453500 and 458500 credited-gald goals build Sheena, Lloyd and Raine statues through separate flags while this word remains unchanged. Pietro's final thanks also requires the Raine statue flag; this value alone does not mean all statues or thanks are complete.
        ExpandedCityStatuePhase = 600000,
        /// Seven retained Luin transfer readers accept ExpansionInProgress..699999 and choose the expanded layouts 0455/0456/0457. The interval includes ExpandedCityStatuePhase; its upper endpoint has no recovered constant setter or additional reconstruction milestone. Statue flags and credited gald remain separate.
        ExpandedLayoutTransferUpperBound = 699999,
    }
}

ids! {
    /// Reviewed bases used with a dynamic offset, separate from individual bits.
    /// Darkness 0126 iterates 0..5 over flags 623..627 and actors 2301..2305.
    pub enum StoryFlagBase: u16 {
        /// World entry explicitly clears offsets 0..50. These are reused by
        /// unrelated scenes; no single scene meaning applies to the whole bank.
        WorldResetSceneBank150 = 150,
        TempleOfDarknessGatheredShadows = 623,
        /// Long-range map flags starting at 900. The sixteen mapping-service
        /// copies set offsets 0..10 in their all-Sylvarant loop. Individual
        /// region/stone identities and the native travel grid remain separate.
        LongRangeSylvarantBank = 900,
        /// Long-range map flags starting at 911. The sixteen mapping-service
        /// copies set only offsets 0..9: that loop leaves flags 921 and 922
        /// untouched. This base does not mean every Tethe'alla flag was unlocked.
        LongRangeTetheallaBank = 911,
        /// Thirty persistent named-dog flags, 1214..1243. Completion helpers
        /// count offsets 0..29 and award Colette's Dog Lover title only at 30
        /// when it is not already owned. The retained Thoda debug setter can
        /// set the whole bank without the individual dog-naming conversations.
        DogNamingBank = 1214,
        WorldResetSceneBank1700 = 1700,
        WorldResetSceneBank1800 = 1800,
    }
}

ids! {
    pub enum StoryStage: u32 {
        /// New-game settings sub_07E2 resets this word after the GRADE Shop and startup helper, immediately before entering Iselia 0340. This does not itself reset other globals or flags. Retained Ossa 0351 Normal and Mana 0361 route-A debug choices also write zero.
        NewGameStart = 0,
        /// New-game settings sub_0428 writes this after the completion-data notice and session command 10, before opening save menu 9997. On entry this value selects sub_07E2's GRADE Shop/new-game flow. It identifies clear data, not proof that saving or GRADE purchases have already completed.
        GameClearData = 1,
        /// Iselia 0340 sub_0DC0: the oracle light interrupts Raine's Day of Prophecy lesson, and she leaves to check the chapel while telling the pupils to stay and study. The stage is set after that classroom sequence, before the pastor warning and party escort.
        DayOfProphecyClassInterrupted = 1000,
        /// Iselia 0340 sub_29E3: after the dying pastor warns of the temple attack, both choices ultimately send Lloyd and Genis with Colette. Their party join and skit instructions precede this write. It is the escort departure from class, not receipt of the oracle.
        ColetteFirstTempleEscortStarted = 2000,
        /// Iselia 0332 sub_0925: Frank explains the Desians passed through the village to the temple, says Phaidra is preparing the ritual there, and agrees Lloyd and Genis may accompany Colette. Both prior classroom dialogue variants converge before this stage.
        FrankTempleAttackWarningHeard = 2500,
        /// Iselia 0332 sub_0F32: the party completes formations 1 and 2, discusses Lloyd's Exsphere, magical versus physical enemies, and target selection, then sets this before proceeding to the temple. This is the pair of first outdoor combat tutorials, separate from the later Training Manual.
        IseliaFirstCombatTutorialsComplete = 3000,
        /// Martel 0306 sub_023B: after seeing the temple's light and commotion, the applicable pastor/escort dialogue and final approach complete before this write. The Botta confrontation and Kratos hiring occur afterward.
        MartelTempleApproachComplete = 101000,
        /// Martel 0306: after the entrance battles and Botta's retreat, Phaidra hires Kratos to guard Colette through the oracle trial. Both Lloyd reply choices permit him and Genis to accompany them. sub_2E06 and its caller both write this same stage; keep the duplicate writes.
        KratosHiredForOracleTrial = 103000,
        /// Martel 0307 sub_0CD3: Kratos criticizes Lloyd's self-taught swordplay; the basic manual topics are marked learned and the Training Manual item is awarded before this stage. It precedes the golem and Sorcerer's Ring sequences.
        TrainingManualReceived = 104000,
        /// Martel 0308 sub_0F17: the first golem battle and falling-block explanation precede this stage and MartelTempleGolemPuzzleStarted flag. The retained camera/debug interaction also forces the same stage for its create/remove-golem menu, so the stage does not assert a currently live golem.
        MartelTempleGolemPuzzleStarted = 105000,
        /// Martel 0308 block monitor advances from 105000 only when floor-block flags 5003 and 5004 are both set. Entry treats this stage or later as placing both of those required blocks. The other three floor-block flags remain independent; this is not Sorcerer's Ring possession.
        MartelTempleRequiredFloorBlocksPlaced = 106000,
        /// Martel 0308 sub_1BB4: either sealed-door dialogue variant introduces the Church's Sorcerer's Ring, then marks SorcerersRingObtained, grants the item and displays the X Button instruction before this stage.
        SorcerersRingReceived = 107000,
        /// Martel 0307 sub_2FD1: the ring-door sequence moves the nodes selected by resource slots 56 and 57 apart by -200/+200, removes the old barrier actors, and plays Lloyd's Is this all the ring does remark before this stage. The oracle altar scene has not yet happened.
        MartelTempleFirstRingDoorOpened = 108000,
        /// Martel 0309 sub_0E9F: Remiel identifies Colette as the seventh Chosen, grants the Cruxis Crystal, reveals the Tower of Salvation and instructs her to head south to the Fire Seal. This stage follows the oracle sequence, before Kratos leads her away.
        ColetteOracleReceived = 109000,
        /// Martel 0309 sub_2FFC: Kratos and Colette leave the altar, Colette invites Lloyd and Genis to her house, and the boys discuss her claimed angel father before this write. Raine's temple reprimand remains a later stage.
        ColetteTempleDepartureComplete = 110000,
        /// Martel 0307 sub_323D: Raine finds and reprimands the boys for leaving class, tells them to go home, and says she has permission to continue studying the temple. The later laugh reaction is a separate stage.
        RaineTempleReprimandComplete = 111000,
        /// Martel 0307 sub_3C57: after Raine's research laugh, Lloyd asks what it was and Genis says he is better off not knowing, then this stage is set. It is that short departure reaction, not a second oracle.
        RaineTempleLaughHeard = 112000,
        /// Iselia 0336 sub_05B6: the mayor approves Kratos and Raine as Colette's escorts, Phaidra grants the Collector's Book, and Lloyd and Genis are told to stay home. This is the finished escort council scene at Colette's house.
        IseliaJourneyEscortCouncilComplete = 201000,
        /// Iselia 0331 sub_07FE: Genis gives Colette birthday cookies, Lloyd promises to finish her necklace by tomorrow, and Genis asks to travel home together to visit a friend. Colette's later terrace conversation and the finished necklace remain separate stages.
        ColetteBirthdayNecklacePromised = 202000,
        /// Iselia 0330 sub_0BB5: Lloyd retrieves Noishe and the gatekeeper relays the mayor's warning to stay away from the human ranch. Lloyd and Genis deny visiting it before this stage is written.
        IseliaRanchWarningHeard = 202500,
        /// Iselia Forest 0192 sub_05B4: Noishe flees the monster-filled forest while Lloyd and Genis discuss his fear. The write precedes their ranch detour agreement and also marks the forest visited.
        NoisheFledIseliaForest = 301000,
        /// Iselia Forest 0192 region 3001: Genis says his friend at the ranch must hear about the oracle, and Lloyd agrees to accompany him despite the treaty. This marks the agreed detour, before reaching Marble.
        GenisRanchVisitAgreed = 301500,
        /// Iselia Human Ranch 0193 sub_0660 completes the first arrival camera/actor setup and marks the shared visit bit after this stage. Marble's introduction, Key Crest explanation and removal to the back of the ranch happen later.
        FirstIseliaRanchArrivalComplete = 302000,
        /// Iselia Human Ranch 0193 region 3002: after Marble's introduction and the Key Crest discussion, guards take her to the back for punishment and Lloyd proposes finding higher ground. This is set before the climb tutorial, not after her rescue.
        MarbleTakenAwayByDesians = 303000,
        /// Iselia Human Ranch 0193 region 3003: Lloyd notices the climbable route and the Action Button instruction appears before this stage. The subsequent spawned rescue scene checks this exact stage before Genis attacks from above and Lloyd acts as decoy.
        IseliaRanchClimbTutorialShown = 304000,
        /// Iselia Human Ranch 0193 spawned event 3004 writes this immediately after formation 6 returns from Lloyd's guard battle. The remaining escape animation still runs before stage 306000; preserve this intermediate write rather than combining the two stages.
        MarbleRescueGuardBattleComplete = 305000,
        /// The same ranch rescue event finishes the escape animation and writes this immediately before returning to Iselia Forest 0192. Forest entry dispatches the apology/homework/farewell scene from this stage; it advances separately to 307000.
        MarbleRescueForestAftermathPending = 306000,
        /// Iselia Forest 0192 sub_1121: Lloyd reassures Genis about being seen at the ranch, asks him to do his homework, and sends him back to the village before setting this. The surveillance cutaway and equipment return are later stages.
        GenisSentHomeAfterMarbleRescue = 307000,
        /// Iselia Human Ranch 0193 sub_6D17: the Desian cutaway orders analysis of the gate surveillance and asks how a mere human made that jump. It sets this before returning to the forest; it does not claim the requested analysis is complete.
        LloydGateSurveillanceInvestigationOrdered = 307500,
        /// Iselia Forest 0192 entry at 307500 shows Received Genis equipment and advances to this stage. Keep this return separate from sending Genis home and from his later permanent journey with Lloyd.
        GenisEquipmentReturnedAfterRanchVisit = 308000,
        /// Dirk's House 0372 sub_24D0: Lloyd says he is home to his mother at her grave, the camera sequence completes, and this stage is set before control returns. The Key Crest argument with Dirk follows in room 0374.
        AnnaGraveGreetingComplete = 501000,
        /// Dirk's House 0374 sub_02A4: Lloyd asks for Marble's Key Crest, admits visiting the ranch, and Dirk explains Lloyd's special Exsphere was his murdered mother's keepsake. Their argument ends with this stage, before the terrace invitation.
        DirkExsphereKeepsakeExplanationHeard = 502000,
        /// Dirk's House 0373: both the immediate Sure branch and the retained Just a minute branch set this after Colette proposes talking on the terrace. The first immediately calls the conversation; the other leaves her waiting at the bench. It marks availability/pending talk, not completion.
        ColetteBirthdayTerraceTalkPending = 503000,
        /// Dirk's House 0373 sub_0A70 finishes Colette's birthday/angel-father terrace conversation, including her stated noon departure, then writes this as Lloyd decides to finish her present. The morning finished-necklace stage is separate.
        ColetteBirthdayTerraceTalkComplete = 504000,
        /// Dirk's House 0375 sub_0218: Lloyd announces It is finished and decides to talk to Dirk before this stage. The preceding terrace scene supplies the birthday necklace context; travel supplies and the Key Crest are not received until 506000.
        ColetteBirthdayNecklaceFinished = 505000,
        /// Dirk's House 0372 sub_284E: Dirk hands over the Key Crest and journey supplies (two Apple Gels, a Life Bottle, 500 Gald and the Sylvarant Map). Genis then reports Colette has already left, and they leave for the village before this stage is written.
        DirkKeyCrestAndTravelSuppliesReceived = 506000,
        /// Iselia 0330 sub_10F5: on returning after Dirk's farewell, the gatekeeper says Phaidra is looking for Lloyd. Lloyd acknowledges the summons before this stage; Colette's farewell letter has not yet been read.
        PhaidraDepartureSummonsHeard = 801000,
        /// Iselia 0336 sub_0EE4: Frank passes on Colette's letter explaining her early departure, then begins to reveal what the Chosen's journey really entails. A rumble interrupts him before this stage and the Desian-raid camera sequence.
        ColetteFarewellLetterRead = 802000,
        /// Iselia 0342 entry at 802000 calls sub_0819, stages Lloyd/Genis's shocked reaction and a long camera pan over the Desian raid using scenery banks 311/312. It writes this immediately before changing to 0341 for Forcystus's entrance; it does not mark the Marble battle won.
        IseliaDesianRaidPanComplete = 803000,
        /// Iselia 0341 sub_0800 stages Forcystus (loaded by name), his Desian escort and his advance through the burning village. It writes this immediately before the transfer back to 0342; that entry restores the camera/layout before the separate free-roam stage 901000.
        ForcystusIseliaEntranceComplete = 804000,
        /// Iselia 0342 entry at 804000 restores the post-entrance layout/camera and sets this before enabling input. Burning Iselia 0341 uses it to stage the Forcystus approach triggers and formation 8/Marble confrontation. It precedes the mayor's banishment declaration, not a defeated Forcystus marker.
        BurningIseliaExplorationActive = 901000,
        /// Iselia 0341 sub_299F: after the Marble confrontation, the mayor banishes Lloyd and Genis, Phaidra asks them to protect Colette, and Genis accepts Marble's Exsphere. This stage precedes their departure animation/title and the existing IseliaExile stage 903000.
        IseliaBanishmentDeclared = 902000,
        IseliaExile = 903000,
        FirstCampComplete = 1000000,
        TrietWantedPostersSeen = 1001000,
        /// Triet 0485 sub_3E48 and retained 0525 sub_2DDF discuss Lloyd's wanted poster, set TrietWantedPosterExamined and add 500 to Story. From TrietWantedPostersSeen this computes 1001500. Both copies use it as the inclusive poster/crater dialogue upper bound and as an exact ellipsis-response selector. The crater conversation sets its own flag without advancing Story. Preserve the addition and the interaction's captured pre-dialogue story value.
        TrietWantedPosterExamined = 1001500,
        /// Triet 0485 trigger 2060 invokes sub_1E3F at this exact value: the guards identify Lloyd, formation 9 runs and the scene writes LloydCapturedInTriet. The 0485/0525 exit reminders use this as their exclusive upper limit. No constant setter for this trigger value is recovered.
        TrietWantedCaptureTrigger = 1002000,
        /// Triet 0485 sub_1E3F: after formation 9 and Lloyd's complacent remark, he is seized and Genis begs not to be hurt. This stage follows the capture sequence and precedes arrival at the base, not merely winning the wanted-poster guard battle.
        LloydCapturedInTriet = 1003000,
        /// Base 0281 sub_040C sets this after Genis tricks the guards into releasing him and meets Noishe. Base 0268's recapture branch also restores it, clears Cell300Open and sends Lloyd back to the cell. The shared phase means Lloyd is imprisoned; it cannot mean only GenisFreed or an irreversible rescue milestone.
        SylvarantBaseLloydImprisoned = 1101000,
        /// Base 0267 sub_05CA: Lloyd wakes in the cell, hears the execution threat, notices his equipment is gone and decides to hit the guard with the Sorcerer's Ring. The stage is set after that plan, before the guard is driven off.
        SylvarantBaseCellEscapePlanReady = 1102000,
        /// Base 0267 ring-contact event for actor 1000 moves and despawns the hurt guard, then writes this. The subsequent fades and 100-tick cell-door animation run before Cell300Open is set. Keep the stage write earlier than the completed opening animation.
        SylvarantBaseCellGuardDrivenOff = 1103000,
        /// Base 0267 actor interaction 1096 displays Recovered equipment and writes this after restoring Lloyd's gear. The following prison-break guard battle has its own stage.
        SylvarantBaseLloydEquipmentRecovered = 1104000,
        /// Base 0267 sub_0C84: a guard calls out the prison break; formation 10 returns before this write. It precedes the electric-orb mechanism demonstration, the rotating-door room and the later Botta boss fight.
        SylvarantBaseFirstEscapeBattleComplete = 1105000,
        /// Base 0268 sub_18CE: hiding from searching guards lets Lloyd watch their electrify-and-move-over-panels door mechanism. This stage is set after their demonstration and puzzle setup, before Lloyd's final red-outfit complaint. It does not mean Lloyd has solved or passed the passage himself.
        SylvarantBaseOrbMechanismDemonstrated = 1105100,
        /// Entries to both base 0267 and 0269 advance from 1105100 while ElectricOrbPassageEnabled is set, then clear that passage latch. This is the common first passage traversal, including returning toward the cells, not a permanent all-doors-unlocked stage.
        SylvarantBaseFirstOrbPassageTraversed = 1105200,
        /// Base 0270 entry advances from 1105200 before Lloyd's Doors in this room are locked too remark. It is entry into the rotating-door puzzle, before the first rotation feedback.
        SylvarantBaseRotatingDoorRoomEntered = 1105300,
        /// Base 0270 sub_1DD7 handles the selected entrance/hangar/control-room passage, then advances from 1105300 before Lloyd comments on the spinning center and lit button. The later angle-hint stage remains separate.
        SylvarantBaseFirstDoorRotationComplete = 1105400,
        /// Base 0270 sub_1DD7 advances from 1105400 before Lloyd proposes that the central mechanism rotates by the angle on the floor. That branch is checked before the first-rotation branch, so one invocation does not advance twice. The name does not claim the explanation has already finished.
        SylvarantBaseRotationAngleHintStarted = 1105500,
        /// Sylvarant Base 0274 entry uses < for blocker actor 200 and >= for exit record 6001; sub_03DA and sub_043F use >= for southern touch exits 7010..7013 to room 0281. This is the observed access threshold, with no recovered constant setter.
        SylvarantBase0274SouthExitThreshold = 1106000,
        /// Base 0275 sub_0424: a guard spots Lloyd and he reacts Uh-oh. The write precedes the transfer to 0276 and the Yuan/Botta confrontation; it is not a cleared hangar or a defeated boss marker.
        SylvarantBaseGuardsSpotLloyd = 1107000,
        /// Base 0276 sub_029A: Lloyd meets Yuan, the Chosen's group arrives, formation 11 ends, and Raine opens an escape route before this write. Botta flees rather than being killed; the exterior/Noishe departure stage is later.
        SylvarantBaseBottaBattleComplete = 1108000,
        /// Base 0281 sub_0AE5: the party reunites with Noishe, notices the Exsphere on the recovered weapon and agrees to discuss it in Triet. A retained Triet 0525 Menu Dog helper also forces this phase while inviting the player outside, so that debug write does not itself prove another base escape occurred.
        ReturningToTrietAfterBaseEscape = 1109000,
        /// Triet 0485 exit trigger 2050 enables mapped input at this exact value; retained 0525 has an empty equality branch. Authored world rules also use it as the Skit02 upper bound. No recovered constant writer establishes a broader completed-story event.
        TrietExitInputReleaseMarker = 1201000,
        /// Triet 0485 sub_3029: exhausted Genis asks to rest and Lloyd proposes going to the inn to talk. The write precedes Raine's Exsphere/Key Crest explanation and the night repair.
        TrietPostEscapeInnRestRequested = 1202000,
        /// Triet 0527 sub_0188: Raine produces a worn Key Crest among her research materials and Lloyd agrees to repair it. This write occurs before the intervening work/fade sequence and Lloyd's It is done remark. It is not delivery of the repaired crest.
        RaineKeyCrestRepairAgreed = 1203000,
        /// Triet room variants 0490 sub_045A and 0529 sub_0445 both finish Lloyd handing Raine her repaired Key Crest and her accepting the Exsphere. Their separate doorway good-night event advances to 1205000 afterward.
        RaineKeyCrestDelivered = 1204000,
        /// Triet room variants 0490 and 0529: the exit event at 1204000 exchanges Lloyd/Raine's good-night lines and her delighted Exsphere reaction before this write. This is separate from the earlier delivery and the later choice about following Kratos.
        RaineTrietNightFarewellComplete = 1205000,
        /// Both Triet inn variants 0487 and 0526 set this for either following Kratos outside or leaving him alone. Only the latter sets TrietKratosNightDepartureSeen at this point. Do not label the shared stage as following him, seeing his advice scene or choosing to rest.
        TrietKratosNightDepartureChoiceResolved = 1206000,
        /// Triet 0485/0525 write this after Kratos's optional sword-advice scene and transfer to daytime room 0488. Room 0527 also writes it after choosing sleep and transfers to 0488. That room applies its morning audio fade from this stage. It is their shared morning phase, not proof of the optional advice conversation.
        TrietFirstInnMorning = 1207000,
        /// Triet 0485 and retained 0525 departure helpers give the Monster List and agree to visit the Fire Seal at the ruins southwest of town before this write. The 0485 variant also marks RaineExsphereReceived and shows her party join; preserve the differing helper bodies rather than assuming those extra writes in both variants.
        TrietFirstSealDepartureReady = 1208000,
        GuardTechniqueTutorialComplete = 1301000,
        /// Triet Ruins 0218 sub_2BC7: Raine identifies the oracle stone, Colette retracts the entrance slab and the blocker/slab bits are set before the party prepares to enter. This stage follows the Archaeological Mania title and entrance sequence, not the Fire Seal battle.
        TrietRuinsEntranceOpened = 1302000,
        /// Each of room 0220's three torch-approach events writes this before Lloyd notices a torch and Raine suggests lighting it. The early write suppresses repeating the introduction while its dialogue runs; it does not prove a fire switch has been activated.
        TrietRuinsTorchIntroductionStarted = 1302100,
        /// Retained room 0220 ring handler unused_17F8 advances to this from lower story values in five fire-switch branches (5020/5031/5022/5023/5024). Some write before testing that switch's current activation flag. This marks switch interaction progress, not all torches lit or the entire puzzle solved, and does not establish that the retained helper is live in normal play.
        TrietRuinsFireSwitchUseMarked = 1302200,
        /// Triet Ruins 0221 sub_0A13: after formation 12, the Fire Seal guardian flag and seal-progress word advance; Remiel grants Colette her first angel power and announces the next seal to the east before this main-story write. This is the first seal, separate from the later Efreet pact.
        FireSealReleased = 1303000,
        /// Triet Ruins 0218 sub_37D4: Colette becomes ill after receiving wings, Kratos recommends rest and Lloyd proposes camping before this stage and the camp transfer. Camp 0250's Angel Toxicosis1 debug selection can also force it. It does not mark the night complete.
        FirstAngelNightCampPending = 1304000,
        /// Camp 0250 sub_141D plays Colette's walk departure, Lloyd/Genis exchange and private cough scene, restores and heals the party, then writes this before the timer helper and world-map transfer. This is the first camp's departure-ready phase, not the start of the walk and not proof of an update to ColetteAngelProgress. The later shared camp nights use their separate word and +100 updates.
        FirstAngelNightCampDepartureReady = 1305000,
        /// Ossa 0347 region 3003: the assassin challenges the Chosen and falls through the hidden maintenance passage; the party discusses whether to help her before this write. No battle is fought in this first encounter; her return and formation 16 are later.
        OssaAssassinFirstEncounterComplete = 1402000,
        /// Ossa 0350 region 3000: the assassin catches up, formation 16 ends, she threatens to return and the party resumes seeking a ship before this write. The Unison Attack tutorial at 1403000 remains its own event.
        OssaAssassinBattleComplete = 1402500,
        UnisonAttackTutorialComplete = 1403000,
        /// Izoold 0353 entry advances from UnisonAttackTutorialComplete when first arriving to seek sea passage. This stage precedes the Aifread rumor, Lyla's request and the Max negotiation.
        IzooldFirstArrivalMarked = 1501000,
        /// Izoold 0353 actor 507 explains that no passenger ships are running, that Aifread provided his boat ride, and that Aifread has returned to his hometown before this stage. The next scene lets Genis relay the rumor to Lyla.
        AifreadDepartureRumorHeard = 1502000,
        /// Izoold 0357 actor 101 at 1502000: Genis and Lloyd tell Lyla Aifread went home, and she announces this is her chance before this write. Delivering her letter or securing passage has not happened yet.
        LylaToldAifreadReturnedHome = 1503000,
        /// Izoold 0354 sub_156E converges here after either accepting Lyla's letter request or declining and having Raine/Kratos pressure Max into providing passage. Only the acceptance branch grants and marks LylasLetterReceived immediately. This common stage is the ship agreement, not proof of possessing the letter.
        MaxSeaPassageSecured = 1504000,
        /// Izoold 0354 sub_2B52: Max agrees to set sail and the party's small-boat/Genis excitement sequence runs before this write and the continuing voyage transfer. It precedes Palmacosta's dock arrival.
        IzooldSeaVoyageStarted = 1505000,
        /// Palmacosta 0055 sub_1084: Max delivers the party and plans to seek a warship escort; Lloyd thanks him and the group sets out to find the next seal. Retained Dirk 0370 uses this number for a different scene and requires its scoped stage enum.
        PalmacostaFirstArrivalComplete = 1601000,
        /// Palmacosta 0054 region 2001: Colette collides with the potion carriers and agrees to buy a replacement before this stage and BookOfRegeneration step 1000. Retained Dirk 0371 uses the same number for an unrelated Key Crest repair night and receives a scoped alias.
        PalmaPotionReplacementAgreed = 1602000,
        /// Palmacosta 0054 region 2003 calls sub_207F while the party has Palma Potion. The potion is handed over and removed, the carriers reveal their plan to sell Dorr's treasure to a Hakonesia collector, and this stage precedes BookOfRegeneration step 3000 and its conditional follow-up dialogue.
        PalmaPotionDelivered = 1604000,
        /// Hakonesia 0123 sub_0D89: a traveler warns that the Desians and Magnius are heading to Palmacosta and recommends waiting at the peak. This stage follows the warning, before the city execution confrontation.
        MagniusPalmacostaApproachWarningHeard = 1701000,
        /// Palmacosta 0058 sub_0725: Magnius attempts Cacao's execution, the party intervenes, and formation 18 returns before this write. Magnius has left his soldiers to fight; the stage does not mark Magnius himself defeated or the ranch destroyed.
        PalmacostaCacaoRescueBattleComplete = 1802000,
        /// Palmacosta 0064 sub_0488: Cacao and Chocolat thank the party, explain their lost family, and discuss Chocolat's pilgrimage-guide job and faith in Martel. Both the prior-tour and new-explanation variants converge before this stage.
        CacaoFamilyPostRescueThanksComplete = 1803000,
        /// Palmacosta 0054 sub_2903: Neil identifies the real Chosen, invites the party to meet Dorr, and Colette agrees. He says he will wait in front of the government building before this stage. It is the invitation, not the completed audience.
        NeilDorrAudienceInvitationAccepted = 1804000,
        /// Palmacosta 0056 region 4010: Neil says the party has been expected and asks them to follow, then writes this for the government-building approach. Room 0065 dispatches the Dorr audience from this phase afterward.
        NeilGovernmentBuildingEscortStarted = 1805000,
        /// Palmacosta 0065 finishes the Dorr audience and Book of Regeneration explanation, sets DorrRegenerationBookLossDiscussed and writes this. Palmacosta 0054 also skips directly here from 1803000 when that discussion flag is already set. The stage does not prove a fresh audience occurred on that entry or that the book was recovered.
        DorrRegenerationBookDiscussionHandled = 1806000,
        /// Ranch 0198 entry accepts DayOfProphecyClassInterrupted..1901000 before sub_1589: Lloyd warns that this is a human ranch and not to get too close. The next exact rescue-request phase selects the ranch-entrance pan instead. This inclusive approach-warning bound has no recovered constant setter.
        PalmacostaRanchApproachWarningUpperBound = 1901000,
        /// Both Hakonesia 0123 sub_07CB and Salvation House 0083 sub_0781 deliver Dorr's request to rescue the kidnapped Chocolat in concert with the militia attack. The ordinary agree/prioritize-seals replies converge on helping her and this stage. Neil's later trap revelation and assault-versus-Dorr choice remain separate.
        ChocolatRanchRescueRequestAccepted = 1902000,
        /// Palmacosta Ranch 0198 sub_10E6 hides the courtyard actors, pans over the patrolling Desians and restores the entrance layout before this stage. It records that initial ranch approach sequence, not defeating its guards or learning of Dorr's trap.
        PalmacostaRanchEntrancePanComplete = 2001000,
        /// Ranch 0200 sub_0091: Neil reveals Dorr's trap and the party selects either an immediate assault or returning to question Dorr. The chosen route goes into its separate assault/Dorr progress word; both paths share this main-stage write, so it cannot identify one choice.
        PalmacostaRanchTrapResponseChosen = 2002000,
        /// The Dorr-last route writes this after Neil's government/recovery farewell in Palmacosta 0065; the ranch-last route writes it after the escape aftermath and Neil's return to town in 0199. Chocolat is still missing and the party promises to find her. Lake Umacy's debug menu also forces it for route A; it is a shared aftermath phase, not proof of rescuing Chocolat or completing every local quest.
        PalmacostaCrisisAftermath = 2100000,
        /// Either Hakonesia 0123 pass-check trigger advances a lower story value here after the guards recognize the road pass and permit passage. Mana 0361 and Lake Umacy debug menus also use this value for route B. It is the northern-route phase, not proof that the Palmacosta ranch/Dorr storyline was completed first.
        HakonesiaNorthernRouteActive = 2101000,
        /// Hima 0300 sub_3A08: with the last seal ahead, Raine asks everyone to split up for the night and Kratos announces a morning departure. The party is dispersed before this stage; the individual companion conversations and Colette's private talk are later.
        HimaPreTowerNightStarted = 2201000,
        /// Hima 0300 sub_3D3A: choosing to stay and talk with Colette rather than visit the others calls the shared affinity-bonus helper, then writes this. The night conversation itself is dispatched from Hima 0301 entry afterward; this is not its completion.
        HimaColettePrivateTalkChosen = 2202000,
        /// Hima 0301 entry at 2202000 calls the hilltop conversation about Colette becoming an angel, Lloyd's unfinished gift and whether she regrets her choice, then writes this. The morning attack on Kratos and dragon passenger selection occur later.
        HimaColettePreTowerTalkComplete = 2203000,
        /// Asgard 0037, Luin 0458, Palmacosta 0059 and Triet 0487 inn handlers require the main story below this value, at least TrietFirstSealDepartureReady, and Colette below FourthSealNightComplete before their angel-night handling. This is an exclusive eligibility limit, not a recovered completed event.
        AngelNightInnUpperBound = 2204000,
        /// Hima 0300 sub_43BA: Lloyd interrupts the attack on Kratos, they discuss the fleeing assassin, Kratos warns Lloyd not to die and the Assassin's Ring item is found. This write follows that morning sequence and party restoration, before dragon boarding.
        HimaMorningAssassinEncounterComplete = 2205000,
        /// Hima 0301 region 3000 at 2205000 calls the pair-selection scene: Kratos rides with Colette, and Lloyd chooses Genis, Raine or Sheena. The distinct companion flags are set by that choice before this shared stage, which precedes the flight handoff.
        HimaDragonPassengersChosen = 2206000,
        /// Retained shared skit helpers use <= with this value for WhyIsColette and KratosAndNoishe, alongside their original angel-progress, party, Hima and affinity conditions. Preserve those independent requirements. No constant setter identifies a milestone at this inclusive upper limit.
        PreTowerCompanionSkitsUpperBound = 2301000,
        /// Salvation 0147 sub_0264 restores the selected dragon companion at arrival; Lloyd asks where Colette and the others are and Raine says they already went inside. This stage precedes the interior Remiel/Kratos sequence, not the later escape.
        FirstTowerDragonArrivalComplete = 2302000,
        /// Palmacosta 0057 trigger 3001 requires the main story below this value for IsabelleSteamshipIntroduced, alongside her flag, Colette angel progress and both Asgard ranch route conditions. This is an exclusive introduction limit, with no recovered constant setter.
        IsabelleSteamshipIntroductionUpperBound = 2303000,
        /// Salvation interior variants 0150/0535 write this after their initial-tower sequence and Kratos equipment removal, then set ColetteAngelProgress to SoulLost and transfer to base 0277. Preserve the variants' differing helpers and the separate angel-progression write. This main phase queues the Renegade-base awakening, rather than asserting a particular battle outcome on every retained variant.
        FirstTowerEscapeBaseReturnPending = 2306000,
        /// Base 0277 sub_017B: Lloyd wakes after the Tower escape, the party explains the Renegades and Kratos/Cruxis, and an attendant invites him to meet the leader before this stage. Yuan's ensuing confrontation is later.
        RenegadeBaseAwakeningBriefingComplete = 2401000,
        /// Base 0276 sub_25AE: Yuan explains Cruxis and the Renegades' aim, tries to seize Lloyd and falters from his Hima wound. Sheena helps the party escape, Colette is re-added and the shared block-transfer position is recorded before this intermediate stage. The common reset helper runs before the separate 2403000 write.
        RenegadeLeaderConfrontationEscapeComplete = 2402000,
        /// The same 0276 confrontation helper writes this after its common restoration helper, immediately before healing the party and changing to 0275. Keep it separate from intermediate 2402000; it starts the second base-escape exploration and subsequent block/ring puzzle phases.
        RenegadeBaseSecondEscapeStarted = 2403000,
        /// Base 0279 event 3001 monitors actor 303 at X/Y (525,450) while the story is below this value. It lowers the block to Z -145 and the scenery node selected by resource slot 6 by Y -600, restores the camera and then writes this. It is this particular block/switch operation, not all escape puzzles complete.
        SylvarantBase0279Block303Lowered = 2403100,
        /// Base 0280 event 3000 sees movable actor 303 at (75,1650), replaces it with actor 304, lowers it to Z -445 and moves the scenery node selected by resource slot 11 before this stage. The actor-ID replacement and original position test remain; it is separate from the ring-door phase.
        SylvarantBase0280Block303Lowered = 2403200,
        /// Base 0280 ring-contact handler for actor 203, in Sylvarant Electric Orb mode and below this stage, plays the controlled character's door-open reaction before this write. It then removes actors 203/318 and calls the passage helper. The adjacent timed energy-charge contacts are separate; do not move this write past that cleanup or treat it as the entire escape complete.
        SylvarantBase0280RingDoorReleaseMarked = 2403300,
        /// Base 0268 sub_0D4C: the party decides to seek Tethe'alla's Exsphere/Cruxis Crystal research to save Colette. Sheena explains that Rheairds can cross the dimensional distortion and should be on the base before this stage. The craft boarding and departure are later.
        TetheAllaRheairdPlanAgreed = 2404000,
        /// Base 0278 sub_03B8: the party finds the Rheairds, Sheena urges them to hurry and Lloyd announces departure for Tethe'alla before this stage and the following flight helper. RheairdsObtained and the Tethe'alla music-bank write follow separately; preserve that order.
        RheairdsFirstDepartureStarted = 2405000,
        /// This read-only boundary lies between the initial Sylvarant stages and Fooji arrival at 10001000. Tower 0147 exit 1000 blocks departure with Lloyd dialogue below it, while Luin 0447 uses <= for actor 101 under the ranch-route conditions. Keep the differing operators; no constant assignment to this number is recovered.
        TetheAllaPhaseBoundary = 10000000,
        /// Fooji 0416 sub_0878: after the emergency landing, the party discusses the empty Rheairds, Volt as fuel and the two worlds' seemingly duplicated Tower/Holy Ground history. They decide to leave the craft and move on before this write. The RheairdsObtained bit is cleared afterward, not at the stage assignment itself.
        FoojiRheairdLandingDiscussionComplete = 10001000,
        /// Meltokio 0376 sub_107D: Sheena leaves to report to Mizuho's chief, explains that her letter asks the King to help Colette and grants Sheena's Letter before this stage. The subsequent party-departure helper remains separate.
        SheenasLetterReceivedInMeltokio = 10101000,
        /// Meltokio 0376 sub_1A16: Genis notices the dog, Colette reacts violently, Lloyd recalls she used to love dogs and the watching Zelos comments. The dedicated dog-encounter flag and this main stage are written after that scene, before the fuller Zelos introduction.
        MeltokioColetteDogEncounterComplete = 10101500,
        /// Meltokio 0376 sub_1F90: the party meets Zelos and his followers, Colette startles him with her strength and Raine notices his Exsphere. This stage follows the street encounter; his status as Tethe'alla's Chosen and later supervision deal are not yet revealed.
        MeltokioFirstZelosEncounterComplete = 10102000,
        /// Meltokio 0377 sub_25B2: castle guards refuse an audience because the King is ill, explain the church's prayer ritual and change shifts. The party resolves to ask at the church before this stage. It is not permission to enter the castle.
        MeltokioRoyalAudienceDenied = 10103000,
        /// Meltokio Cathedral 0395 sub_0277: the priest directs Presea's sacred wood to the royal chambers, and Lloyd proposes pretending to help carry it to get inside. The party agrees to ask Presea before this stage; she has not yet accepted their help.
        SacredWoodCastleEntryPlanChosen = 10104000,
        /// Meltokio 0377 sub_0DF2: after Vharley leaves, the party asks Presea to help them reach the King by letting them carry her sacred wood. She agrees, Lloyd struggles with its weight and they decide to follow her before this stage. The gate authorization is later.
        PreseaSacredWoodDeliveryAgreed = 10105000,
        /// Meltokio 0377 sub_1C31: guards question Presea's companions, Raine says they are helping with the sacred wood and Presea confirms it. The guards permit them through and comment on her unusual speech before this stage. The interior party-join sequence remains separate.
        MeltokioCastleEntryAuthorized = 10106000,
        /// Meltokio 0405 sub_0A93: the wood is left inside and the party decides taking Presea along is less suspicious. Her join and initial equipment/EX-gem setup precede this stage. It is her first castle party join, not the much later PreseaAwakened milestone.
        PreseaJoinedInMeltokioCastle = 10200000,
        /// Meltokio 0405 sub_0EC5: a guard challenges the party, Raine claims the Pope ordered them to help with prayer preparations and the guard says he will ask. This stage marks the resulting royal-chamber approach, before delivering Sheena's letter to the King.
        RoyalChamberGuardConsultsPope = 10201000,
        /// Meltokio 0410 sub_016D: Zelos is revealed as Tethe'alla's Chosen, Lloyd hands Sheena's letter to the King and the party is sent to the Crimson Chamber before this stage. The narrative handover does not add an item-consumption operation absent from the original helper.
        SheenasLetterDeliveredToTetheAllaKing = 10202000,
        /// Meltokio 0409 sub_01B0: the Pope's attack fails and Raine negotiates to save Colette. Both Lloyd replies converge on the bargain; travel is authorized under Zelos's supervision and a cathedral meeting is arranged before this stage. Zelos has not joined the party yet.
        TetheAllaTravelUnderZelosApproved = 10203000,
        /// Meltokio Cathedral 0395 sub_0D39: Zelos explains the Research Academy permission and the party agrees to travel to Sybak via the Grand Tethe'alla Bridge, dropping Presea off on the way. This stage is written before the following join message and Zelos equipment/EX-gem setup, not after them.
        ZelosSybakJourneyAgreed = 10204000,
        /// Grand Tethealla Bridge 0078 sub_06A2: Zelos describes the bridge and its 3000 Exspheres; the party explains their human origin and asks about Zelos and Presea. This stage follows that first bridge discussion, before the Sybak diagnosis.
        GrandBridgeExsphereDiscussionComplete = 10301000,
        /// Sybak 0434 sub_0884: Presea asks to hurry to Ozette, Genis apologizes and Zelos and Lloyd promise to finish their business quickly before this stage. The Academy reception and Colette diagnosis are later.
        SybakPreseaHomeRequestHeard = 10401000,
        /// Sybak 0437 sub_02B5: an Academy attendant recognizes Zelos and invites the party inside. The hall and room-door selection bits precede this stage; the Cruxis Crystal explanation follows separately.
        SybakAcademyReceptionComplete = 10402000,
        /// Sybak variants 0437 sub_048A and 0439 sub_4D79 explain Colette's parasitic Cruxis Crystal and propose a Key Crest before this shared stage. Only the 0439 version awards Ill-Fated Girl; the stage does not assert that title was granted in both variants.
        ColetteCruxisCrystalDiagnosisExplained = 10403000,
        /// Sybak 0434 sub_0A91: the junk collector demands 10000 Gald, but Zelos reveals his identity and obtains the Key Crest without that payment. This write follows the collector's concession, before Lloyd thanks Zelos and proposes borrowing a lab; preserve the absence of an added item or money operation.
        SybakJunkKeyCrestSecured = 10404000,
        /// Sybak 0434 sub_0A91: after obtaining the junk Key Crest and thanking Zelos, Lloyd says he will borrow an Academy lab to fix it. This second write follows that plan; lab permission and the repair remain later phases.
        SybakKeyCrestLabPlanAgreed = 10405000,
        /// Both Sybak 0435 sub_063B and 0437 sub_1CFC have the Academy attendant authorize use of the previous room to make the Key Crest before this stage. The 0437 variant calls the necklace and arrest scene directly afterward.
        SybakKeyCrestLabAuthorized = 10406000,
        /// Sybak 0439 sub_53EB: Lloyd tells the others to wait at the entrance while he repairs the Key Crest. This write occurs during the fade, before the camera returns and his completion remark. This intermediate phase does not mean Colette has recovered.
        SybakKeyCrestRepairStarted = 10406500,
        /// Sybak 0437 sub_06EA: the necklace does not immediately restore Colette; Papal Knights declare the travelers traitors, identify Raine and Genis as half-elves and order their detention. ColetteCruxisCrystalInactive is cleared before this stage; the basement and Kate escape scene follow later.
        SybakPapalArrestOrdered = 10407000,
        /// Both Sybak 0437 sub_1F77 and 0443 sub_039E reveal Presea's experiment, bring Sheena back and have Kate promise a hidden escape route in exchange for returning with the rescued half-elves. This write precedes the passage exit and party-join operations.
        KateSybakEscapePassageOffered = 10408000,
        /// Sybak variants 0437 sub_1F77 and 0443 sub_039E write this after Kate's escape route and Sheena's join, as the party heads for the bridge to rescue Raine and Genis. Only 0443 grants the Tethealla Map here. Bridge 0075's retained event-start debug menu also forces this pursuit phase; it is not a completed rescue.
        SybakRescuePursuitReady = 10409000,
        /// Bridge 0075 sub_0D55: after the drawbridge jump, Undine rescue and formation 37, the party accepts its half-elf friends, discusses recovering the Rheairds and re-adds Genis and Raine. This stage follows their join, before the return to Fooji.
        GrandBridgeHalfElfRescueComplete = 10502000,
        /// Fooji 0416 sub_1725: Colette refuses removal of Lloyd's Key Crest, speaks again and frees the party. This stage is written after her thanks and Pronyma's threat, immediately before formation 38; it does not mark that battle won.
        ColetteRestoredPronymaBattlePending = 10601000,
        /// Fooji 0416 sub_1725: following formation 38 and Kratos and Yuan's departure, Colette confirms her senses have returned and the party chooses to seek help in Meltokio. This stage follows Lloyd's welcome-back exchange and affinity helpers; SoulRestored is then written again and SideEventsSuppressed cleared separately.
        FoojiColetteRecoveryAftermathComplete = 10602000,
        /// Meltokio 0380 exit 7100 invokes sub_12EA through this value, where Lloyd says there is nothing to do here. Sewer-entry phases and the later laboratory choice are handled separately by the same event. This is an inclusive reminder limit, with no recovered constant setter.
        MeltokioSewerEntranceReminderUpperBound = 10700000,
        /// Meltokio 0376 sub_3CCB: a guard refuses passage because the party is wanted, and Zelos offers to show a special entrance. This stage follows that offer, before the sewer introduction.
        MeltokioSecretEntrancePlanOffered = 10701000,
        /// Meltokio 0380 sub_09EF: Zelos explains the underground sewer entrance, the party discusses his late-night use of it and Presea detects enemies. This stage follows their decision to proceed carefully, before the shrink and block puzzles.
        MeltokioSewerInfiltrationStarted = 10801000,
        /// Meltokio sewer 0466 ring-change interaction 3500 advances here only from the infiltration start. It first toggles between Fire and Shrink modes, then writes this and enables tutorial trigger 1505. This stage does not guarantee the current mode is Shrink, since both toggle branches converge.
        Sewer0466FirstRingChangeHandled = 10801008,
        /// Meltokio sewer 0466 trigger 1505 writes this and removes itself before calling sub_436C. That helper selects Lloyd, enables his small form when needed and explains that small characters can walk on spider webs. The assignment precedes the tutorial, rather than recording its completion.
        Sewer0466SpiderWebTutorialStarted = 10801024,
        /// Meltokio sewer 0466 trigger 1506 writes this before sub_44DD stages the party at the locked door. Zelos admits it was open before, Presea suggests the city lockdown and Lloyd proposes finding a way to open it. The stage is the start of that discussion, not unlocking the door.
        Sewer0466LockedDoorDiscussionStarted = 10801040,
        /// Meltokio sewer 0466 trigger 1507 writes this before sub_482E introduces the trash compactor and its transportable blocks. Later arithmetic adds low-bit block-placement state to the same word; preserve those calculations rather than treating this as an independent permanent flag.
        Sewer0466TrashCompactorExplanationStarted = 10801120,
        /// During the pre-lever phase this is TrashCompactorExplanationStarted 10801120 plus the three block-placement additions 1, 2 and 4. Entry and all three handlers compare >= before suppressing actor 1100. Later lever phases also satisfy that comparison, so it is not a general all-blocks-present flag. Preserve the original subsequent 0x0828 resets.
        Sewer0466AllBlocksSumThreshold = 10801127,
        /// Sewer 0466 uses this boundary to restore blocks 3002/3003 and the first gate, allow the lower exit and suppress lever 1200. The lever event adds 74 to the current story word; it does not assign this number. Starting at 10801126 yields this value, while 10801127 yields 10801201. The original equality-only 0x0828 reset and all comparison operators remain intact.
        Sewer0466FirstLeverThreshold = 10801200,
        /// Meltokio sewer 0466 interaction 1300 requires normal size, removes its interaction actor and moves the scenery part selected by resource slot 26 by (160,50,0). After the movement waits it writes this; the exit to 0467 at Z 775 requires this threshold. This is distinct from interaction 1200, which changes the story word arithmetically.
        Sewer0466UpperPassageOpened = 10801280,
        /// Meltokio sewer 0467 event 3608 waits for movable block 3002 to fall, replaces it with collidable actor 3003 at (1145,1900,-540), restores the camera and releases its resource before this stage. Entry restores that fixed block from this phase.
        Sewer0467Block3003Placed = 10801300,
        /// Meltokio sewer 0467 interaction 1302 requires normal size, removes its triggers and collider 3105, rotates the scenery part selected by resource slot 66 by Z -100 and waits 60 ticks before this stage. Returning on the upper route later starts the first spider-web hint at a separate value.
        Sewer0467Barrier3105Removed = 10801400,
        /// Meltokio sewer 0467 entry advances from barrier-removed phase 10801400 only when the controlled character is above Z 700. This write precedes sub_5A1C, whose camera points toward (888,1504,775) and Lloyd notices a new spider web. It is the hint start, not another block placement.
        Sewer0467FirstSpiderWebHintStarted = 10801401,
        /// Meltokio sewer 0467 event 3609 waits for block 3002 to descend below Z 0, replaces it with collidable actor 3004 at (245,2950,-20) and restores the camera before this stage. The later relocation to (-355,2950,-40) has its own phase.
        Sewer0467Block3004FirstPlacementComplete = 10801500,
        /// Meltokio sewer 0467 event 3612 writes this immediately on entry, before waiting for block 3002 to descend, replacing it with actor 3004 at (-355,2950,-40), removing collider 3109 and moving the scenery part selected by resource slot 84 by Y 630. Keep this early write; it is not proof that the relocation animation has finished.
        Sewer0467Block3004RelocationStarted = 10801600,
        /// Meltokio sewer 0467 entry advances from block-relocation phase 10801600 only when the controlled character is above Z 700. It writes this before sub_5B79 points the camera toward (-400,3048,775) and Lloyd remarks on another spider web.
        Sewer0467SecondSpiderWebHintStarted = 10801601,
        /// Meltokio sewer 0467 event 3610 waits for the movable block to reach Z 370 or below, replaces it with collidable actor 3005 at (-505,4150,350), restores the camera and writes this. The alternate landing at (-655,4300,350) writes 10801701 instead.
        Sewer0467Block3005PlacementAComplete = 10801700,
        /// Meltokio sewer 0467 event 3611 replaces the falling block with collidable actor 3005 at (-655,4300,350) before this stage. Preserve the original entry restoration, which uses movable actor 3002 at (-505,4150,350) for both landing phases; do not silently correct that discrepancy.
        Sewer0467Block3005PlacementBComplete = 10801701,
        /// Meltokio sewer 0467 event 3613 waits for the falling block to pass Z -520, replaces it with collidable interaction actor 3006 at (245,3700,-540), restores the camera and writes this. This phase enables interaction actor 1303; opening the exit remains a separate stage.
        Sewer0467Block3006Placed = 10801800,
        /// Meltokio sewer 0467 interaction 1303 requires normal size, removes its actor, operates the scenery parts selected by resource slots 56 through 59 and completes the camera sequence before this stage. The transition to sewer 0468 is gated by this value; it is not the ensuing ambush outcome.
        Sewer0467ExitPassageOpened = 10801900,
        /// Sewer 0467 entry handles exact exit phase 10801900 separately, then accepts 10801901..EndingClearDataPending for the post-exit scenery, blocks 3003/3004/3006 and record 1510 setup. This inclusive reader bound is one above the exit phase and has no recovered constant setter; preserve >= and the original literal.
        Sewer0467PostExitSetupLowerBound = 10801901,
        /// Sewer 0468 entry starts event 3702 only through this value. That event watches the controlled actor coordinates while the story is below MeltokioSewerAmbushStarted, then calls the ambush scene sub_191F. The value gates watcher creation; no constant setter for it is recovered.
        Sewer0468AmbushWatcherUpperBound = 10802000,
        /// Meltokio sewer 0468 sub_191F writes this at the beginning, before restoring normal size, the prisoners' ambush dialogue, formation 40 and the unnamed Regal hostage scene. This early assignment must not be named as a completed battle or city arrival.
        MeltokioSewerAmbushStarted = 10803000,
        /// Meltokio 0380 sub_15CD: after reaching the city through the sewer, Sheena names the Elemental Research Laboratory as the place to meet her contacts and explains its connection to Corrine. Lloyd agrees to go there before this stage.
        MeltokioElementalLaboratoryVisitPlanned = 10803100,
        /// Meltokio 0394 sub_04E7: Sheena introduces Kuchinawa, explains the need to cross the bridge and goes to speak with the laboratory staff. This intermediate stage precedes the direct call to the Elemental Cargo proposal scene.
        KuchinawaMetAtElementalLaboratory = 10901000,
        /// Meltokio 0394 sub_0943: the laboratory proposes an Undine-powered EC, agrees to prepare it in a day and the party decides to rest at Zelos's mansion. This stage is the preparation agreement, not receiving the finished craft.
        ModifiedElementalCargoCommissioned = 10902000,
        /// Meltokio 0399 sub_02D5: Sebastian welcomes Zelos and Sir Bud, agrees to ignore the order to report their return and offers hospitality. Party-state capture precedes this stage, while the evening actor setup follows; it does not assert the overnight rest is finished.
        ZelosMansionFirstEveningStarted = 10903000,
        /// Meltokio 0399 sub_07AC: Lloyd proposes returning to the Elemental Research Laboratory, notices Colette's stiff shoulder and restores the party before this stage. The subsequent laboratory handover and Wing Pack item are separate.
        ZelosMansionFirstMorningComplete = 10904000,
        /// Meltokio 0394 sub_0E59: staff explain Kuchinawa has taken the EC to the bridge beach, Corrine says goodbye and the Wing Pack is granted before this stage. The EC itself is demonstrated at the bridge afterward.
        WingPackReceivedAtElementalLaboratory = 10905000,
        /// Grand Tethealla Bridge 0078 sub_10DD writes this after Sheena directs the party to Kuchinawa beneath the right staircase. Door event 2500 also writes it after Lloyd unlocks the route and the party discusses the bridge's Exspheres. Both reach the EC-pickup approach; this value does not identify which of those scenes ran.
        GrandBridgeEcPickupApproachActive = 11001000,
        /// Bridge 0078 sub_1E22: Kuchinawa hands over the EC, Lloyd tries the Wing Pack, Sheena receives Kuchinawa's charm and the EC controls are explained. This stage follows that sequence and precedes the common helper and ElementalCargoObtained bit; preserve that ordering.
        GrandBridgeElementalCargoDepartureStarted = 11002000,
        /// Sybak 0434 sub_116E: Kratos overpowers Lloyd and warns Colette to remove the Key Crest, which she refuses. The party decides to see Kate, marks KratosSybakConfrontationSeen and writes this before the renewed Academy visit.
        SybakKratosKeyCrestWarningHeard = 11101000,
        /// Both Sybak 0437 sub_28BE and 0443 sub_1DCD fulfill the promise to return with the rescued half-elves, explain Presea's experimental Key Crest and refer the party to Altessa beyond Gaoracchia. Their dialogue differs, but both choose that journey before this shared stage.
        KateAltessaReferralReceived = 11102000,
        /// Gaoracchia 0511 sub_05FE: Lloyd remarks on the creepy forest, Colette notes its darkness and Zelos comments on her cheerfulness. This stage follows the first arrival exchange, before the Papal Knights confrontation.
        GaoracchiaFirstArrivalDiscussionComplete = 11201000,
        /// Gaoracchia 0511 event 3002: the Papal Knights threaten Zelos, formation 41 returns and the party decides to hurry toward Altessa before reinforcements arrive. This stage precedes the later encounter with Regal.
        GaoracchiaPapalKnightsBattleComplete = 11203000,
        /// Gaoracchia 0517 event 3000: after formation 42 against the unnamed Regal, Corrine reports approaching soldiers. Sheena chooses refuge in Mizuho and Colette carries the captive before this stage and the following transition helper. It is not Regal's party join.
        GaoracchiaMizuhoRetreatAgreed = 11205000,
        /// Mizuho 0326 sub_0ED7: Orochi questions Sheena for bringing outsiders, directs her to the Vice-Chief and tells the party to wait at the Chief's house. The captive wakes and accepts his situation before this stage; the alliance audience follows later.
        MizuhoFirstArrivalReceptionComplete = 11301000,
        /// Mizuho 0327 sub_0A64: Lloyd and Tiga agree to seek prosperity for both worlds, Mizuho offers its intelligence network and Tiga appoints Sheena its representative. This stage follows the agreement but precedes the cleanup helper and Lloyd's Gentle Idealist title award.
        MizuhoAllianceAgreed = 11303000,
        /// Mizuho 0326 sub_1A6F: Regal identifies himself, swears not to betray the party and is accepted as a companion. His join, equipment and conditional EX-gem setup precede this stage; his earlier captive arrival is separate.
        RegalJoinedInMizuho = 11304000,
        /// Mizuho 0326 event 3050: Orochi says the pursuers have left, Lloyd proposes Altessa and Presea asks to go home first. Raine agrees to visit Ozette before this stage. It records the chosen destination, not arrival or completion of Presea's treatment.
        MizuhoOzetteDepartureAgreed = 11305000,
        /// Ozette variants 0425 sub_0783 and 0426 sub_26AD have Genis urge Lloyd to follow Presea and write this. The 0425 variant removes her party member first; 0426 calls its wood-delivery encounter afterward. It is the pursuit phase, not the completed encounter at her home.
        OzetteFollowPreseaStarted = 11501000,
        /// Ozette variants 0426 sub_05B1 and 0433 sub_03FF show Presea identifying the party as porters and returning to her work despite Genis's Key Crest request. After the half-elf wood broker leaves, Regal proposes talking with Presea before this shared stage.
        OzettePreseaWoodDeliveryEncounterComplete = 11502000,
        /// Ozette 0431 sub_02A7: the party discovers the condition of the person in Presea's bed and Raine explains her unawareness as an Exsphere effect. They agree to leave Presea at home and consult Altessa before this stage and the following helper. Her treatment and awakening are later.
        PreseaHomeWaitAgreed = 11503000,
        /// Both Ozette 0425 sub_09CC and 0426 sub_27BC have Lloyd check on Colette after she stumbles. She insists she is unharmed before this stage. It precedes the first Altessa visit and does not establish that her underlying condition has been cured.
        OzetteColetteStumbleDiscussionComplete = 11504000,
        /// Altessa exterior 0138 entry calls sub_05B9 when arriving at story 11504000. That helper only gates input, writes this and releases input; it contains no audience or Key Crest repair. The interior meeting with Altessa and Tabatha follows afterward.
        AltessaFirstVisitStarted = 11701000,
        /// Altessa interior 0139 sub_0AAB: Altessa refuses to help Presea, and Tabatha advises finding inhibitor ore while promising to persuade him. This stage follows that advice; the party has neither located the mine nor obtained the ore yet.
        TabathaInhibitorOreAdviceReceived = 11702000,
        /// Altessa exterior 0138 sub_05C3: Regal offers to guide the party to an Exsphere mine on the southern continent, where inhibitor ore is found near the surface. They agree to go before this stage; the Toize guard-system battle remains later.
        InhibitorOreMineJourneyAgreed = 11704000,
        /// Toize entrance 0415 sub_057B: the party destroys the malfunctioning guard system in formation 43 and discusses searching for inhibitor ore. The first-visit branch sets ToizeValleyMineVisited and this main phase after the battle and common helper. Preserve the first-visit condition.
        ToizeMineFirstGuardBattleComplete = 11801000,
        /// Toize 0412 Bomb-mode handler: a blast within 400 XY units of actor 102 plays its breaking animation, fades it, removes actors 102 and 103 and updates trigger 1001 before this stage. The same handler's non-Bomb cart-switch branch uses separate flags and does not write this value.
        ToizeMine0412BombBarrierRemoved = 11803000,
        /// Toize mining area 0414 sub_08CB: Regal identifies the inhibitor ore area and suggests searching wooden boxes; Lloyd and Zelos choose to blow them up. This stage follows that introduction and its helpers, before the item is found.
        ToizeInhibitorOreSearchStarted = 11804000,
        /// Toize 0414 interaction 130, while below this stage, grants one Inhibitor Ore, displays the found-item message and calls sub_0B56 before writing this. It precedes the subsequent helper and the Vharley confrontation on the return route.
        ToizeInhibitorOreReceived = 11805000,
        /// Toize 0412 sub_1BEF: Regal confronts Vharley over the Pope's broken promise; Vharley flees and Regal acknowledges his murder conviction. Lloyd and Colette offer support before the cleanup helpers and this stage. It does not record capturing Vharley or clearing Regal's crime.
        ToizeVharleyRegalConfrontationComplete = 11806000,
        /// Ozette 0425 sub_0C0D: the Knights identify the wanted party and demand Colette alive. Formation 44 returns before this intermediate stage; the apology, Colette's fever and Rodyle's abduction are still ahead in the same helper.
        OzettePapalKnightsBattleComplete = 11901000,
        /// Ozette 0425 sub_0C0D: Colette falls ill, Presea intervenes and Rodyle takes Colette. The script removes Colette from the party before this stage and sets SideEventsSuppressed afterward. Preserve that order and the earlier post-battle phase.
        RodyleColetteAbductionAftermath = 11902000,
        PreseaAwakened = 11903000,
        /// Ozette variants 0426 sub_0F83 and 0433 sub_0765: after her father's burial, Presea explains Vharley's experiment and both response choices welcome her and Regal on Colette's rescue. Kratos directs them to the Rheairds, and they choose Mizuho before this shared stage; PreseaWelcomedAfterAwakening is set afterward.
        PreseaPostAwakeningRescueJourneyAgreed = 11904000,
        /// Mizuho 0327 sub_2413: Tiga reports locating the Rheairds but requires Sheena to form the Volt pact first, explaining her previous failure and the Chief's coma. This stage follows the assignment and her refusal; her agreement to try again is separate.
        MizuhoVoltTrialAssigned = 12001000,
        /// Mizuho 0326 event 3200: Corrine and Lloyd reassure Sheena about her previous Volt failure, and Lloyd promises to stop Volt if needed. She agrees to try again before this stage; no pact or temple battle has happened yet.
        SheenaVoltRetryAgreed = 12002000,
        /// Mizuho 0327 sub_2E0E: Tiga says Orochi will bring the Rheaird location to the temple and directs the party north to the Temple of Lightning. This stage follows that departure briefing, before the temple arrival.
        MizuhoLightningTempleDepartureBriefed = 12003000,
        /// Temple of Lightning 0475 sub_1340: Lloyd notices the easy entry and missing oracle stone, and Raine explains that a flourishing-world temple is not currently functioning as a seal. This stage follows that first arrival discussion, before the Volt approach and pact.
        LightningTempleFirstArrivalDiscussionComplete = 12101000,
        /// Temple of Lightning 0475 sub_1522: after Corrine's sacrifice, formation 46 and Sheena's vow, the party learns that opposing pacts sever the worlds' mana link. VoltPactFormed, Corrine's Bell, Sardonyx, the lightning techniques and Sheena's Summoner title are granted before this stage. The later Orochi briefing is separate.
        VoltPactAftermathComplete = 12104000,
        /// Temple of Lightning 0475 event 3000: Orochi reports the Rheaird guardian signal beneath an icy fjord southeast of the temple and offers to prepare the underwater approach. The party agrees to retrieve the craft and rescue Colette; the cleanup helpers precede this stage, not the base infiltration itself.
        RheairdUnderseaBaseLocationReceived = 12105000,
        /// Tethealla base entrance 0251 sub_016B: Orochi identifies the guard as a Mizuho infiltrator and the on-duty guard permits entry. The party decides to retrieve the Rheairds before this stage and the following helper; the pass-code briefing is later.
        TetheAllaBaseInfiltrationAuthorized = 12201000,
        /// Tethealla base 0255 sub_03E8: the party notices Zelos missing; he returns claiming to have checked for traps, and Orochi invites them to hear the infiltration procedure. This stage follows that exchange and precedes the actual pass-code briefing.
        TetheAllaBaseZelosReconnaissanceReturnComplete = 12202000,
        /// Tethealla base 0252 sub_034E: Orochi explains the hangar lock, its three separately held pass codes and the limitation on interworld flight without a base energy charge. The party chooses to retrieve the Rheairds before this stage; the codes and craft have not yet been acquired.
        TetheAllaBasePassCodeSearchBriefed = 12203000,
        /// Tethealla base 0254 Tethealla Electric Orb contact with actor 43, from X greater than -1325 and Z at least 150, starts its animation and shows the scenery nodes selected by resource slots 19 and 20 before this stage. The ring-contact disable, 30-tick wait and sound cleanup follow. Target 45 uses a separate story flag.
        TetheAllaBase0254OrbTarget43Activated = 12203100,
        /// Tethealla base 0254 Tethealla Electric Orb contact with actor 44 starts its animation and shows the scenery nodes selected by resource slots 21 and 22 before this stage. The contact disable, wait and sound cleanup follow. This phase is distinct from target 43 and the separately flagged target 45.
        TetheAllaBase0254OrbTarget44Activated = 12203200,
        RheairdsRecovered = 12204000,
        /// Flying Dragon 0137 sub_0269: after formation 48, Presea interrupts the magic-circle trap and Lloyd urges Colette to live as the platform collapses. This stage follows Colette's agreement, before the separate landing debrief and party rejoin in field 0242.
        FlyingDragonColetteRescueEscapeStarted = 12301000,
        /// Authored world::rules shows a blocked Flying Dragon marker from RheairdsRecovered through 12400000 inclusive and hides it outside that range. No direct original .ssb operand or setter at 12400000 was recovered. This names the existing authored visibility threshold, not a game event; the normal landing debrief writes ColetteRejoinedAfterFlyingDragonRescue at 12401000.
        FlyingDragonWorldVisibilityUpperBound = 12400000,
        /// Field 0242 sub_0089: the party survives the Flying Dragon escape, discusses Rodyle's Mana Cannon and chooses to seek Gnome next. Colette's party rejoin and the common helper precede this stage; the following helper and world transfer remain afterward.
        ColetteRejoinedAfterFlyingDragonRescue = 12401000,
        OzetteAttack = 12601000,
        /// Ozette variants 0425 sub_3AEF and 0433 sub_1DA5 find the ruined village, meet the half-elf survivor Mithos and hear that Altessa blames himself. The party decides to follow Altessa and take Mithos along before this shared stage. This escort agreement does not add Mithos as an ordinary playable party member.
        MithosOzetteSurvivorEscortAgreed = 12701000,
        /// Altessa exterior 0138 sub_11F1: Altessa explains his Cruxis work, Rodyle's coercion, the Presea experiment and why he blames himself for Ozette. Presea cannot forgive him yet but agrees to listen to his information about Cruxis before this stage and the common helper.
        AltessaOzetteExperimentConfessionHeard = 12801000,
        /// Altessa interior 0139 sub_3225: Colette wakes Lloyd and announces that the Professor has gone. Lloyd reacts before this stage. The note, southern Rheaird sighting and decision to search for Raine are discussed in the following exterior sequence.
        RainesAltessaDepartureDiscovered = 12802000,
        /// Altessa exterior 0138 sub_28E2: the party learns Raine left toward Altamira and both initial responses ultimately let Mithos accompany the search on Genis's Rheaird. Altessa offers Presea a better Key Crest and Raine is removed from the party before this stage; SideEventsSuppressed follows separately.
        RaineSearchWithMithosAgreed = 12803000,
        /// Altamira 0011 sub_0E3B: Regal refuses to enter and Lloyd agrees to meet him on the way out. The helper removes Regal from the party, then advances here only if the previous story value is 12803000. The dialogue can occur on other visits without forcing this main-story phase.
        RegalAltamiraWaitStarted = 12901000,
        /// Altamira 0011 sub_12C5: George mistakes Presea for Alicia, reports Alicia's death and directs the party to her grave on the Lezareno Sky Terrace. He gives the Employee ID before this stage. The apparent discrepancy over which sister is older remains in the original dialogue.
        AliciaGraveEmployeeIdReceived = 12902000,
        /// Both Altamira 0024 reception helpers sub_0592 and sub_06D2 recognize George's identification and permit the party inside to visit the Sky Terrace before this stage. It is entry permission, not the conversation with Alicia's Exsphere.
        LezarenoSkyTerraceEntryAuthorized = 12902500,
        /// Altamira 0026 sub_03A9: Alicia's consciousness speaks from the grave's Exsphere and asks Presea to find Master Bryant. Lloyd and Genis agree to help before this stage. This records the search request, not finding Bryant or learning Regal's full history.
        AliciaBryantSearchRequestAccepted = 12903000,
        OtherworldlyGateSearch = 12904000,
        /// Otherworldly Gate 0234 sub_00DB: Raine explains where she and Genis were abandoned, Kuchinawa attacks and formation 52 returns. Zelos interrupts Sheena's surrender and the party flees through the gate. This stage precedes the Sylvarant music-bank write and clearing SideEventsSuppressed; the arrival debrief is later.
        KuchinawaGateEscapeToSylvarantStarted = 13001000,
        /// Palmacosta academy 0072 event_0_012D_r3 accepts PalmacostaCrisisAftermath..13100099 for the instructor's dedicate-ourselves-to-study response. Later return ranges use the do-not-interfere-with-class response. This inclusive NPC-dialogue bound has no recovered constant setter or new main-story event.
        PalmacostaAcademyCrisisDialogueUpperBound = 13100099,
        /// Field 0238 sub_0A59: the party identifies its Sylvarant landing, discusses Kuchinawa's connection to the Pope and rejects Sheena's self-sacrifice. They decide to investigate the Desians and ask Neil to look after Mithos before this stage and the common helper.
        SylvarantGateReturnDebriefComplete = 13101000,
        MithosPanpipeReceived = 13201000,
        /// Palmacosta Ranch 0199 sub_22BF writes this before calling sub_09F8, the Yuan and Botta alliance audience. The assignment starts that discussion; it does not mean the Great Seed explanation or alliance decision has already completed.
        RenegadeAllianceAudienceStarted = 13301000,
        /// Palmacosta Ranch 0199 sub_09F8: Yuan explains the Great Seed, Martel and his leadership of the Renegades. Lloyd agrees to cooperate on Rodyle's ranch and the Mana Cannon, accepting temporary loss of dimensional travel. The restoration helper precedes this stage; Botta still waits for departure confirmation.
        RenegadeGreatSeedAllianceAgreed = 13302000,
        /// Palmacosta Ranch 0199 Botta interaction 111: only choosing readiness has Lloyd agree to leave and Botta say to follow him before this stage. RheairdsObtained is cleared afterward. The wait and canceled-choice paths do not advance here.
        BottaIslandRanchDepartureStarted = 13303000,
        /// This is exactly one below IslandRanchManaReactorBriefingComplete 13401000. Entries 0269, 0271, 0272 and 0275 use a strict > comparison when restoring their later ranch scenery and access. Name the original predecessor literal and retain the operator; it is not a separate completed event.
        IslandRanchBriefingPredecessor = 13400999,
        /// Island Ranch 0222 sub_0134: Botta explains that the mana reactor must remain intact, and the party decides to seek the control room on the top floor. This stage follows that entrance briefing, before the Rodyle and flooding sequences.
        IslandRanchManaReactorBriefingComplete = 13401000,
        /// Island Ranch control room 0232 sub_015B: Rodyle taunts the party and introduces his underwater projector show, then writes this before transferring to room 0225 for the flooding cutaway. It does not record defeating Rodyle or saving Botta.
        IslandRanchFloodProjectionStarted = 13401100,
        /// Island Ranch flooding variants 0223 sub_01FE and 0225 sub_0274 write this after the prisoner and flood cutaway, then return to 0232. Control-room entry at this same value starts sub_0627, including formation 53 and Botta's sacrifice; that helper writes the value again before transferring to escape room 0231. Preserve the shared phase and repeated write: it cannot mean Botta has already died on every entry.
        IslandRanchFloodingSequenceActive = 13401200,
        /// Island Ranch 0231 sub_014D: after formation 54, the flute and mysterious attack disperse the dragons, and Mithos calls the party onto the Rheairds. This stage follows their decision to board and precedes the RheairdsObtained bit. The rescue explanation is a separate Flying Dragon scene.
        MithosIslandRanchRheairdRescueStarted = 13401300,
        /// Flying Dragon 0137 sub_2A06: Mithos explains borrowing a Rheaird and describes the golden bird, while the party discusses Aska and the flute. They choose to return to Neil in Palmacosta and Genis thanks Mithos before this stage.
        MithosRheairdRescueDebriefComplete = 13406000,
        /// Palmacosta 0065 sub_3251: Neil welcomes the survivors, Genis discovers the broken flute and Mithos identifies its extinct-tree origin. Both Nova-search-flag dialogue variants converge on reporting Botta's fate to Yuan and seeking Mithos's return to Tethealla before this stage.
        PalmacostaMithosFluteDebriefComplete = 13501000,
        /// Sylvarant base 0281 sub_0E2C: Mithos asks to wait alone outside, promises not to leave and agrees the party should return the borrowed Rheaird. This stage precedes the interior Yuan report and dimensional-travel restoration.
        MithosWaitsOutsideSylvarantBase = 13502000,
        RheairdsDimensionalTravelUnlocked = 13503000,
        /// Sylvarant base 0281 sub_1016: Mithos greets the returning party and Lloyd agrees to take him to Altessa before this stage and the following helper. This is not the earlier dimensional-travel unlock or Regal's later Altamira request.
        MithosAltessaReturnAgreed = 13504000,
        /// Meltokio 0376 exit 2009 uses <= with this value, when word 0x00F0 is 2000/3000/4000, for Lloyd and Sheena saying the door is locked and the occupants may be on an expedition. The later branches animate the door and enter 0394. This is an inclusive conditional access limit, with no recovered constant setter.
        Meltokio0394LockedDoorUpperBound = 13600000,
        /// Altessa exterior 0138 sub_3E87: the party leaves Mithos with Altessa, tells Regal about Alicia and hears his request to go to Altamira. This stage precedes the restoration helper and Mithos placement bit. Meltokio 0376 and 0380 retained event-test menus also force it for their test presets, so the value alone does not prove that conversation ran on a debug path.
        RegalAltamiraVisitRequested = 13601000,
        /// Shared field timer dispatch uses this as the exclusive Ozette-range upper bound, inclusive Rheairds/seal-pact-range upper bound and inclusive next-range lower bound. The normal Light-pact approach is 20001000. Dirk 0374 event_0_00C9_r2 also tests <20000000 for its earlier dialogue/figurine path with separate Asgard, book and introduction-bit checks. No constant setter was recovered at this bound. Preserve the <= and >= overlap; this value is not a completed Light pact.
        LightPactEraBoundary = 20000000,
        /// Tower of Mana 0361 entry calls the write-only sub_5393 with the seven prior pact flags, LinkiteTree step 40000, RegalFullNameRevealed, story in 13601000..20001000 and arrival word 11000. This activates the final Light-pact approach phase; no pact battle or reward occurs in that helper.
        LightPactTowerApproachActive = 20001000,
        /// Tower of Mana 0366 event 2000: Kratos warns that losing the Great Seed's last protection is dangerous, but Yuan holds him off and sends Lloyd to form the Light pact. After the cleanup helper this stage and the two Light/Aska sequence bits are written before the transfer to room 0369. They precede the battle.
        YuanHoldsOffKratosForLightPact = 20002000,
        /// Tower of Mana 0369 sub_420B: formation 58 returns, Sheena makes her vow to Luna and Aska, and Topaz plus the Light techniques are awarded. Yuan and Kratos react before this stage; the Light/Aska bits are written again and the Great Seed movie plays afterward. The stage is not the earlier warning encounter.
        LightPactRewardsReceived = 20003000,
        /// Field 0238 sub_18ED: Kratos explains the runaway Great Seed and the party agrees to stop Iselia's mana reactor while Sheena prepares the Mana Cannon. Kratos joins, his Traitor title is learned twice and his equipment/EX-gem setup precedes this stage. Keep the duplicate title operation and selected-team plan.
        KratosJoinedForIseliaManaReactorOperation = 20101000,
        /// Altessa exterior 0138 sub_4E6A: Mithos thinks he hears Genis, the ground shakes and Mithos and Tabatha react to the danger, ending with his Ma...r... line. The actor, resource and camera cleanup precede this stage and the transfer to Iselia Ranch 0193. This names that cutaway rather than claiming the later Mana Cannon has already fired.
        AltessaMithosTabathaQuakeCutawayComplete = 20201000,
        /// Iselia Human Ranch 0193 entry creates the Rune Helm chest only when the main story is strictly greater than this value. The chest has its separate treasure flag; this boundary alone does not mean the helm has been obtained. No constant setter to this story number is recovered.
        IseliaRanchRuneHelmSpawnBoundary = 20301000,
        /// Iselia Ranch 0193 sub_6FFA: Lloyd explains why he is keeping Kratos nearby; Kratos offers to fly over the shut gate. The gate-entry animation, barrier actor removal, scenery visibility changes and party restoration precede this stage. Team selection happens in room 0194 afterward.
        IseliaRanchKratosGateEntryComplete = 20302000,
        /// Iselia Ranch 0194 sub_12B7: Lloyd confirms his three companions and the rescue leader explains that everyone stays together until reaching Chocolat. Branch-specific encouragement and facility preparation advice precede this stage. The actual captive rescue and team split are later.
        IseliaRanchReactorTeamChosen = 20303000,
        /// Iselia Ranch 0195 sub_0383: formation 59 frees Chocolat and the captives; the survivors thank the party and report that the other rooms escaped. This intermediate stage is written before the rescue-team farewell and Lloyd's separate reactor team is installed.
        IseliaRanchCaptiveGuardBattleComplete = 20304000,
        /// Iselia Ranch 0195 sub_0383: the selected rescue leader takes the captives away and Lloyd tells Chocolat to regard the Chosen as her rescuer if necessary. The party is replaced with the selected reactor team before this second stage and the common helper. Forcystus and the reactor shutdown remain ahead.
        IseliaRanchRescueTeamDeparted = 20305000,
        /// Iselia Ranch reactor room 0197 sub_09EB: affinity-selected companions debate Forcystus and formation 60 returns. Lloyd says they can now stop the reactor before this intermediate stage. The machinery operation follows, and Forcystus's final attack occurs later outside.
        IseliaRanchForcystusBattleComplete = 20306000,
        /// Iselia Ranch 0197 sub_09EB: after the Forcystus battle, the retained companion branches operate or break the reactor machinery. The common actor cleanup precedes this stage and sub_30E0. It is the reactor shutdown phase, not the subsequent exterior ambush or Mana Cannon firing.
        IseliaRanchManaReactorStopped = 20307000,
        /// Iselia Ranch 0193 sub_7988: the teams reunite, Forcystus launches his final attack and Colette hides her altered body. Kratos urges Lloyd to send the message before this stage; CrystalDiseaseRevealed is written to ColetteAngelProgress separately, then the script transfers to Island Ranch 0080. Do not merge the two saved-word writes.
        ColetteCrystalDiseaseRevealedAtIseliaRanch = 20308000,
        ManaCannonFired = 20401000,
        /// Sylvarant base 0276 sub_40CE, dispatched at ManaCannonFired, shows Yuan's silent reaction. After its resource and camera cleanup and common helper, this stage is written before the transfer to Altessa interior 0139. No additional report dialogue or reward occurs in the helper.
        YuanPostManaCannonSilentCutawayComplete = 20501000,
        /// Altessa interior 0139 sub_3520: Mithos stirs in bed with Altessa and Tabatha nearby, and Altessa tells him not to get up. The animation, fade, resource and camera cleanup precede this stage and the return transfer to Iselia Ranch 0193.
        MithosAltessaBedsideCutawayComplete = 20601000,
        /// Sylvarant base 0276 sub_41E8, dispatched at 20601000, shows Yuan saying the news may be good for the others but not for him. The common restoration helper precedes this stage and the transfer back to Iselia Ranch 0193. It remains separate from his earlier silent cutaway.
        YuanPostManaCannonReactionComplete = 20602000,
        IseliaRanchRescueComplete = 20701000,
        /// Iselia 0343/0344/0346 entry accepts ReturningToTrietAfterBaseEscape..20800000 for the village-entry refusal handling. Room 0343 calls Colette's appeal; 0344/0346 keep their original <301 AND >=20000 angel-word guard and otherwise show the prohibition. The later Phaidra briefing/reconciliation phases remain separate. This inclusive reader bound has no recovered constant setter.
        IseliaEntryRefusalUpperBound = 20800000,
        /// Iselia 0336 sub_156E: Phaidra learns the truth about Cruxis and reports that the Tower of Salvation disappeared with the giant tree after the earthquake. The party discusses blame falling on Colette and decides to investigate the village while Kratos stays behind. The party-restoration helper precedes this stage.
        PhaidraTowerDisappearanceBriefingComplete = 20801000,
        /// Iselia 0346 sub_0ABE: Chocolat and the children challenge the Mayor, the villagers welcome the party and rescued captives, and Chocolat thanks Lloyd for helping Marble. This stage follows that reconciliation, before the common restoration helpers.
        IseliaWelcomesPartyAndCaptives = 20802000,
        /// Iselia 0344 sub_0A05: Raine says the village may still have hope and dismisses the idea of lecturing the Mayor. The conversation and its restoration helpers precede this stage; arranging Colette's visit to Dirk happens afterward.
        IseliaRaineMayorAftermathComplete = 20803000,
        /// Iselia 0344 sub_0D10: Colette says she is feeling better, Kratos reports his escort request from Phaidra and Frank, and Lloyd agrees to consult Dirk about Cruxis Crystals. Party restoration precedes this stage; Kratos has not yet returned to Cruxis.
        ColetteDirkConsultationAgreed = 20804000,
        /// Dirk 0374 sub_0BDC: Dirk admits he does not understand Cruxis Crystals and recommends asking a Tethealla dwarf. He offers an overnight rest while Kratos declines before this stage. The exterior farewell and companion night scenes are separate.
        DirkCruxisCrystalConsultationComplete = 20901000,
        /// Dirk exterior 0372 sub_322B: Lloyd questions Kratos before his departure, Sheena returns and the highest-affinity Kratos or Sheena can receive additional dialogue. The common restoration helper precedes this stage. It starts the remaining night phase, rather than proving every companion conversation occurred.
        DirkPostTreeNightStarted = 20902000,
        /// The eighteen selected-companion handlers for Colette, Genis, Raine, Zelos, Presea and Regal across Dirk 0372/0374/0375 write this after their extended conversation, before returning to exterior 0372. Dirk 0374 also writes it when Lloyd accepts bed, and 0373 entry writes it after its retained Affection level event placeholder. All twenty writers queue the morning; the value does not identify a particular completed conversation.
        DirkPostTreeMorningPending = 20903000,
        /// Dirk exterior 0372 sub_3D8C: the party chooses to return to Tethealla and seek Altessa's help for Colette despite doubts about Kratos. The night-to-morning and party restoration helpers precede this stage and the final common helper.
        AltessaReturnForColetteAgreed = 20904000,
        /// Altessa interior 0139 sub_3806: the party hears how Mithos protected Tabatha during the earthquake. Altessa diagnoses Colette's crystal-rejection illness and recommends Ancient War records; they agree to visit Sybak's archive with Mithos before this stage. The cure recipe is not yet known.
        AltessaCrystalDiseaseDiagnosisHeard = 21001000,
        /// Sybak archive 0445 sub_029E: the unicorn record offers a clue but Raine's healing arts alone cannot cure Colette. Both Kratos-trust responses converge on consulting the royal Kharlan War records in Meltokio, while Mithos offers to return alone. This stage precedes the restoration helpers.
        SybakRoyalWarRecordsReferralReceived = 21101000,
        /// Altamira 0027 George event 200 uses >= with this value for sub_0BBD when Regal is present and lacks Eternal Sinner. The conversation about Vharley, Alicia and the emptiness of revenge grants that title. A separate later >= RuneCrestMaterialsSearchActive branch has its own archive/title handling. This is a reader-only eligibility boundary.
        RegalEternalSinnerConversationLowerBound = 21201000,
        /// Meltokio sewer 0468 event 3600: the party overhears Vharley's poison arrangement, confronts him and connects the Pope, Rodyle and the Exsphere experiments. Lloyd resolves to pursue the Pope before this stage; the common helper and PopeExsphereConspiracyLearned flag follow separately. Do not insert a battle or inventory operation absent from the scene.
        VharleyPopePoisonPlotConfrontationComplete = 21202000,
        /// Meltokio castle approach 0377 sub_22AE: Lloyd and Regal claim to have captured the Chosen and wanted travelers for delivery to the Pope. The guards accept the explanation despite recognizing the sacred-wood infiltrators before this stage.
        MeltokioPrisonerRuseEntryAuthorized = 21301000,
        /// Meltokio 0396 sub_0237: the Pope reveals the antidote location under threat, Colette finds it, and the party confronts his treatment of Kate and half-elves. Guards arrive as he escapes and Presea offers to handle them before this stage. Preserve the narrative antidote discovery without adding an inventory grant.
        PopeAntidoteConfrontationComplete = 21302000,
        /// Meltokio 0403 sub_0848: Zelos presents Colette as Spiritua, orders the Pope and his soldiers captured and the party's wanted status revoked, and the guards agree. Colette's Spiritua Reborn title precedes this stage; the King's treatment and archive permission are later.
        SpirituaRuseWantedRevocationOrdered = 21303000,
        /// Meltokio royal chamber 0410 sub_07B0: Raine administers the antidote, the King regains consciousness and grants access to the second-floor war archive. This stage follows that authorization, before the restoration helpers and before discovering the Rune Crest recipe.
        TetheAllaRoyalArchiveAccessGranted = 21304000,
        /// Meltokio archive 0408 sub_0210 writes this after discovering the Rune Crest recipe and choosing to seek its materials in Heimdall or Altamira. Royal hall 0405 sub_11E3 writes the same value again after Hilda obtains and grants the King's Letter for Heimdall. This is the shared material-search phase, not proof that the letter has been received on every setting path.
        RuneCrestMaterialsSearchActive = 21305000,
        /// Shared field timer dispatch tests <21306000 for the range starting at 20000000, and >=21306000 for the later range. The earlier range includes the royal archive/material-search period; the later range starts with SeekingZelosCruxisCrystal and separately requires Zircon and Mana Leaf Herb for timer 149 state 1. No constant setter or completed event at 21306000 was recovered. Preserve the partition, item checks and the distinct real RuneCrestMaterialsSearchActive value 21305000.
        RuneCrestMaterialsTimerBoundary = 21306000,
        SeekingZelosCruxisCrystal = 21400000,
        /// SE Abbey 0118 sub_0302: Zelos asks Seles to return his Cruxis Crystal, she agrees and their awkward farewell follows before this stage. The separate acquisition helper in abbey 0117 advances to ZelosCruxisCrystalRecovered afterward; do not merge the two phases or invent an item grant here.
        SelesCruxisCrystalReturnAgreed = 21400500,
        ZelosCruxisCrystalRecovered = 21401000,
        /// Tethealla Tower approach 0148 sub_0707: Zelos approaches the entrance and the helper creates eighteen stair actors 201 through 218, then restores the party, camera and input transition before this stage. The subsequent cure-journey entry discussion has a separate value.
        TetheAllaTowerStairwayCreated = 21402000,
        /// Tethealla Tower approach 0148 sub_09BF: the party recalls the regeneration journey, Raine urges Zelos to open the way and Presea notices his unusual behavior. Party and camera restoration precede this stage, which dispatches the interior Kratos encounter afterward.
        ZelosCureJourneyTowerEntryPrepared = 21502000,
        /// Altamira 0011, Meltokio 0379 and Sybak 0434 require story < this value for the Rosa money-delivery, forced-engagement and Joshua-reply branches. Each preserves its own lower bound, route flags, side-event suppression and party conditions. No constant setter establishes an event at this exclusive upper limit.
        RosaJoshuaQuestUpperBound = 21503000,
        /// Tower 0150 entry at 21502000 calls the Kratos encounter and formation 65, retaining both battle-result continuations and his warning against resistance. Entry then sets the TetheallaLate music bank and this stage before transferring to prison room 0500. The stage does not assert victory over Kratos or an escape from captivity.
        SecondTowerKratosPrisonTransferPending = 21505000,
        /// Welgaia prison 0500 sub_5954: Lloyd, Zelos and Regal discuss being captured again and kept alive while Colette's treatment is prepared. This stage follows that exchange and immediately precedes the cell-door helper sub_5DCD.
        WelgaiaPrisonAwakeningDiscussionComplete = 21601000,
        /// Welgaia prison 0500 sub_5DCD: Lloyd, Sheena, Colette and Presea cannot open the cell, but Regal uses his hands to break the barrier and explains his vow. Lloyd calls for an escape before this stage and the following helper. The vow is not a new restriction added by the decompiler.
        RegalWelgaiaCellDoorBroken = 21601100,
        /// Welgaia prison 0500 sub_6998: the transporter fails and Raine suspects its destination. Colette offers to fly over and investigate, Lloyd cautions her and Raine agrees to let her handle it before this stage. The obstructing box is identified in room 0501 afterward.
        ColetteWelgaiaTransporterScoutingStarted = 21601200,
        /// Welgaia 0501 sub_2ADD: Colette flies to the transporter destination and says the box must be in the way. This stage follows that discovery and precedes the scout-proxy actor cleanup. Removing the obstruction is a separate phase.
        ColetteWelgaiaTransporterBoxLocated = 21601300,
        /// Welgaia 0501 sub_2EEC: Colette's clearing sequence removes actors 400, 401 and 402; she says That should do it and walks back before this stage. The party's successful transporter response is recorded separately in prison room 0500.
        ColetteWelgaiaTransporterObstructionCleared = 21601400,
        /// Welgaia prison 0500 sub_6F05: Lloyd remarks on Colette's quick return, Raine confirms the problem appears resolved and Colette agrees. Lloyd says to go before this stage. The later angel-city prisoner disguise is not yet chosen.
        WelgaiaTransporterRestored = 21602000,
        /// Welgaia 0501 sub_2549: Lloyd notices the angels and Raine proposes pretending to be Colette's prisoners, since she is an angel. Lloyd agrees before this stage and the party, camera and field-control helpers.
        WelgaiaColettePrisonerDisguiseAgreed = 21603000,
        /// Welgaia 0504 sub_0146: the party poses as research assistants and then as Kratos's couriers after his call. The attendant finally grants one Mana Fragment before this stage and the restoration helper. The identification-requirement dialogue remains conditional on its own flag.
        WelgaiaManaFragmentReceived = 21604000,
        /// Welgaia 0503 sub_5927: angels discover the party at the emergency exit, and Lloyd urges everyone to flee. After the common helper and the pursuers' report exchange, this stage is written before clearing GiantTransporterHintShown and the exit helper. It is not the later weightless-tower escape complete.
        WelgaiaEmergencyExitEscapeStarted = 21605000,
        /// Retained shared skit helpers require story <= this value and >= PopeAntidoteConfrontationComplete for A Fine Chosen, with Lloyd and Zelos present and the skit unviewed. This is the inclusive skit limit, not a recovered written milestone.
        AFineChosenSkitUpperBound = 21701000,
        /// Tower 0155 sub_02CA: Lloyd starts floating, Raine and Genis explain Gnome and gravity, and Raine warns about stopping only on contact. Zelos demonstrates the danger before Lloyd agrees to be careful and this stage is written.
        TowerWeightlessnessTutorialComplete = 21702000,
        /// Tower 0159 interaction 101, while below this phase, flashes its console, announces disengaging the Red Gate lock and changes the scenery nodes selected by resource slots 6 and 7 before this stage. Console actor 101 is removed afterward. Preserve the ordered story threshold rather than replacing it with an independent Boolean.
        TowerRedGateUnlockMarked = 21703000,
        /// Tower 0164 interaction 101, while below this phase, flashes its console, announces disengaging the Blue Gate lock and changes the scenery nodes selected by resource slots 7 and 8 before this stage. Console actor 101 is removed afterward; its progress threshold remains distinct from the Red Gate.
        TowerBlueGateUnlockMarked = 21704000,
        /// Tower 0161 interaction 101, while below this phase, grants one Elevator Key and displays its found-item message before this stage. The elevator slot in room 0162 uses a separate later phase.
        TowerElevatorKeyReceived = 21705000,
        /// Tower 0162 interaction 110, between key-received phase 21705000 and this value, displays Used Elevator Key, creates passage trigger 1100 and removes actor 101 before this stage. Actor 110 is removed afterward. Preserve the absence of an extra item-consumption call.
        TowerElevatorKeyUsed = 21710000,
        /// Tower 0536 entry at 21710000 calls the Eternal Sword encounter: Yggdrasill explains his plan, formation 66 returns, and Pronyma interrupts before he leaves. The party chooses to hurry Colette's treatment. This stage follows the whole helper before the transfer to 0150, not a permanent defeat of Yggdrasill.
        YggdrasillEternalSwordEncounterAftermath = 21802000,
        /// Altessa interior 0139 sub_45A7: Raine explains the Rune Crest process, Altessa and Tabatha complete treatment and report that Colette will recover when she wakes. The party celebrates and Lloyd goes to bed before this stage. CrystalDiseaseCured is then written to ColetteAngelProgress separately before the exterior transfer.
        ColetteRuneCrestTreatmentComplete = 21901000,
        /// Altessa bedroom 0140 sub_0545: Lloyd cannot move, Yuan asks whether he wants to meet his father and Lloyd demands to know what happened to Dirk. The scene and actor cleanup precede this stage, immediately before transfer to exterior room 0143 for the Kratos revelation.
        YuanLloydFatherMeetingTransferPending = 21902000,
        /// Gaoracchia 0515 uses <= for the tail introduction, while Iselia Forest 0233 uses > for the wings introduction. Their first-Sword-Dancer, prior-introduction and party requirements remain separate. This boundary is not itself proof of any Sword Dancer battle result; no constant setter to this number is recovered.
        SwordDancerWingsIntroductionBoundary = 21903000,
        /// Altessa exterior 0143 sub_07B1: Yuan reveals Kratos as Lloyd's father, Kratos protects him and Colette helps him regain resolve. Mithos exposes himself as Yggdrasill, attacks Altessa and Tabatha, and formation 68 returns. Affinity-dependent Zelos staging remains intact before this shared stage and restoration helper.
        MithosIdentityAltessaAttackAftermath = 21904000,
        /// All three Altessa exterior 0143 aftermath helpers, for Yuan, Altessa and Tabatha, set their own conversation bit and write this only when all three bits are set. Yuan explains Origin's seal and the Eternal Sword restriction, while the other two scenes address the injured victims. Any conversation can be last; this stage does not identify that order.
        AltessaAttackAftermathConversationsComplete = 21904100,
        /// Altessa interior 0139 sub_60C5 shows the injured Altessa in bed and Raine beside him, with no dialogue beyond her ellipsis emote. Its fade, actor and camera cleanup precede this stage and transfer to room 0142. The following doctor-search discussion is separate; the stage does not mean Altessa is healed.
        AltessaRaineBedsideCutawayComplete = 21904200,
        /// Altessa 0142 sub_0480: Raine says she administered first aid but needs a doctor, Sheena recalls the Flanoir physician and the party agrees to fetch him for Altessa. This stage precedes the common helper and the journey; it does not assert the doctor has arrived.
        FlanoirDoctorSearchAgreed = 21905000,
        /// Flanoir 0310 sub_11DD: Lloyd and Sheena notice Zelos has wandered off and decide to see the doctor first. This stage precedes the restoration helpers; it is not the doctor agreement or the nighttime companion selection.
        ZelosMissingAtFlanoirDoctorVisit = 22001000,
        /// Flanoir 0322 sub_01B4: the physician agrees to see Altessa on payment upon success, with travel costs separate, and requests three escorts. Affinity-based escort selection precedes Lloyd agreeing to wait in the city and this stage. Preserve the original per-character branches and flags, rather than assuming every branch sets identical escort metadata.
        FlanoirDoctorEscortArranged = 22002000,
        /// The eight Flanoir 0317 invitation helpers each have two writes: the rank-four fallback and an accepted invitation both store the chosen FlanoirCompanion and this stage before transfer to 0313. Declining dispatches another invitation instead. The shared stage identifies selection, while the companion word identifies whom; the private conversation is still ahead.
        FlanoirNightCompanionSelected = 22003000,
        /// All eight private-night helpers in Flanoir 0313 write this after their selected conversation and fade, before restoration and return to the inn. Only Colette grants Snow Hare, and only the Kratos helper sets flag 26 at its start. The shared main stage must not imply either route-specific operation occurred for all companions.
        FlanoirNightConversationComplete = 22004000,
        /// Flanoir 0314 sub_05AD: the returning escorts report Altessa is safe, Lloyd proposes striking Cruxis and releasing Origin, and the party agrees Colette will accompany them. This intermediate stage precedes the final Zelos trust exchange and the separate departure-ready write.
        FlanoirCruxisAssaultPlanAgreed = 22005000,
        /// Flanoir 0314 sub_05AD writes this after its final Lloyd and Zelos exchange, whose wording depends on the selected Flanoir companion. It follows the separate assault-plan stage and precedes the restoration helper; the Tower arrival is later.
        FlanoirCruxisAssaultDepartureReady = 22006000,
        /// Retained shared skit helpers use >= this value and <= LateJourneySkitsUpperBound for Maxwell, requiring Sheena and Presea present and the skit unviewed. This is a skit eligibility threshold, not the Maxwell pact or a recovered constant-written event.
        MaxwellSkitLowerBound = 22082000,
        /// Tower approach 0148 entry advances here only from Flanoir assault-departure phase 22006000. The blocked route, enemy pressure and Yuan escape invitation are handled afterward, so this arrival marker is not their completion.
        FinalTowerAssaultArrival = 22101000,
        /// Tower approach 0148 sub_0E28: Lloyd finds the way blocked, Raine warns about their escape route and Sheena sees more pursuers. Yuan calls Lloyd to another route and Colette urges him to follow before this stage.
        YuanTowerEscapeRouteOffered = 22102000,
        /// Tower 0151 sub_0A44: Yuan explains why his exposure to Yggdrasill forces him to work with the party, and Lloyd thanks him before continuing. The party restoration helper precedes this stage and the next common helper; Zelos's betrayal encounter occurs afterward.
        YuanTowerRescueThanksComplete = 22103000,
        /// Tower 0150 entry calls sub_37DB, preserving either formation 71 against Zelos on the Kratos route or formation 70 with the Mizuho intervention on the other route. This first aftermath write follows the helper and is immediately followed by 22105000. It does not identify which branch or battle occurred.
        ZelosBetrayalEncounterAftermath = 22104000,
        /// Tower 0150 entry writes this immediately after the separate 22104000 aftermath value. It then applies the route-conditional timer and transfers to 0168. Preserve both consecutive writes and the differing Zelos/Kratos route handling.
        FinalTowerDescentPending = 22105000,
        /// Tower 0168 entry at 22105000 calls its retained empty sub_0575, writes this and calls sub_05EB. The stage marks this entrance into the final descent, not a completed companion rescue or a new reward hidden in the empty helper.
        Tower0168FinalDescentStarted = 22201000,
        /// Tower 0183 trigger 2000, only at descent phase 22201000, calls sub_0FEE for Lloyd's one-way-road warning before this stage. The trap crossing and final door sequence have their own later phases.
        Tower0183OneWayWarningHeard = 22212000,
        /// Tower 0183 trigger 2002 at 22212000 calls sub_115D, which carries Lloyd across the trap and selects Snow Hare, Zelos crystal, Kratos pendant or the other continuation. The helper restores him before this stage and sub_1AD0. Preserve the distinct companion-dependent animations and dialogue.
        Tower0183TrapCrossingComplete = 22213000,
        /// Tower 0183 trigger 2001 at 22213000 calls sub_18D6: actor 3100 fades, scenery parts selected by resource slots 26 and 27 move apart, and actor 300 or 301 approaches according to the Kratos-route bit. Lloyd continues before this stage and the common helper.
        Tower0183FinalDoorSequenceComplete = 22214000,
        /// Tower 0184 helpers sub_3581 and sub_6C35 reunite Lloyd with the rescuing Zelos or Kratos and write this before Martel's farewell and formation 73 against Mithos. Kratos's equipment swap also follows the write. The value remains through the aftermath, so it cannot mean Mithos was already defeated on every setting path.
        GreatSeedHallRescueAlliesReunited = 22301000,
        /// Tower approach 0148 entry at 22301000 optionally questions Zelos about the Eternal Ring, then has Lloyd decide to go to Heimdall despite uncertainty about fighting Kratos. Those helpers precede this stage; the later village rest and duel are separate.
        HeimdallOriginSealJourneyAgreed = 22302000,
        /// Heimdall 0282/0283 event_1_0BB8_r8 use NewGameStart..22401000 for the early Raine/Genis separation and rejoin choices. HeimdallHalfElvesWaitingApart independently selects the welcome-back dialogue; exact rest phase 22402000 and later return phases have separate handling. This inclusive bound has no recovered constant setter.
        HeimdallEarlySeparationUpperBound = 22401000,
        /// Heimdall 0282 sub_11D6: the Elder temporarily admits Raine and Genis, and the party persuades Lloyd to rest before facing Kratos. The party and common restoration helpers precede this stage. It records the rest agreement, not the night conversation or Origin pact.
        HeimdallPreDuelRestAgreed = 22402000,
        /// Heimdall 0284 sub_0169 writes this before staging the selected companion's walk to room 0285. On the Kratos selection it instead proceeds through its direct morning-pending branch. Keep this early write and the route distinction.
        HeimdallPreDuelCompanionNightStarted = 22402200,
        /// Heimdall 0285 sub_055D: the chosen companion invites the sleepless Lloyd outside and he accepts before this stage. The Kratos selection uses Colette for this invitation condition; preserve the original selection logic rather than inventing a Kratos visit here.
        HeimdallStarTalkInvitationAccepted = 22402500,
        /// Heimdall 0284 sub_070B writes this after the selected star conversation. The Kratos branch of sub_0169 also writes it directly before its rest and morning transition, without that outdoor conversation. Both paths prepare the morning; the stage does not prove a specific night discussion occurred.
        HeimdallPreDuelMorningPending = 22403000,
        /// Heimdall 0282 exit 1000 advances from 22403000 here on the Ymir route, and 0283 exit 1001 writes it from 22403000 or again from this value before Torent-area room 0479. The shared phase marks leaving the village after the night, not necessarily entering Torent or beginning the duel.
        HeimdallPreDuelVillageExitMarked = 22404000,
        /// Origin site 0484 sub_01A9: formation 74 ends, Lloyd forgives Kratos, Kratos releases his seal and Yuan supplies mana to keep him alive. Yuan urges Sheena to form the Origin pact before this stage and sub_2105. The pact itself remains a later operation.
        KratosDuelOriginSealReleased = 22501000,
        /// Heimdall 0292 entry at 22501000 calls sub_054C: a camera track and scenery bank -1179643 show the village destruction while five elf actors react. Actor cleanup precedes this stage and sub_14EC. The original scene state remains in place for subsequent elf rescues; this is not proof they have all been saved.
        Heimdall0292DestructionCutawayComplete = 22601000,
        /// Heimdall 0291 sub_0693 plays camera bank -1179645, scenery bank -1179646, the destruction event and timed sounds. It stops the track and clears the original scenery channel before this stage, then performs sound cleanup. The elf-rescue conditions remain separate.
        Heimdall0291CollapseCutawayComplete = 22602000,
        /// Heimdall 0291 trigger 4200 requires all three elf-rescue flags. Yuan, Kratos and Tabatha explain Derris-Kharlan's movement, the missing companion and the need for the Ring of the Pact. Lloyd chooses Dirk and agrees Kratos may accompany them before this stage and the restoration helper.
        DirkEternalRingJourneyAgreed = 22605000,
        /// Dirk 0374 sub_0E86: Kratos supplies the gathered materials and Dirk forges the Ring of the Pact. The route-dependent Kratos join and Judgment setup precede this stage; the Eternal Ring inventory grant and conditional Judgment title follow. Preserve that ordering rather than naming the stage as item receipt.
        DirkEternalRingForged = 22701000,
        /// Salvation 0096 event_0_0083_r4 uses <=22701999 for its Dumb word-game sequence, then >=22701999 for the Aisha/Izoold and later word chains. Preserve the overlapping inclusive bounds and branch order. This names the dialogue boundary, not the next Dirk departure event or a recovered setter at 22701999.
        SalvationWordGameLateDialogueBoundary = 22701999,
        /// Dirk exterior 0372 sub_40AC stages the available party, Lloyd calls everyone onward and the common restoration helper runs before this stage. The following helper and Kratos-route handling remain afterward; Eternal Sword acquisition is a separate later phase.
        DirkFinalJourneyDepartureReady = 22702000,
        /// Retained shared skit helpers use <= this value for Colette Dressed Up, My Sister, I'm a Pirate and Maxwell. Each keeps its original lower bound, party, costume, viewed-skit and quest-flag conditions. This inclusive availability limit has no recovered constant setter.
        LateJourneySkitsUpperBound = 22704000,
        /// Tower 0152 sub_0A46 grants Lloyd's Eternal Swordsman title and marks EternalSwordObtained. Lloyd asks the sword to carry the party to Mithos and their captured friend, then the TetheallaLate bank and this stage are written before sub_4459. The destination trap is later.
        EternalSwordDerrisKharlanDepartureStarted = 22801000,
        /// Derris-Kharlan 0129 sub_0ACC: the trap removes Lloyd's companions, the sword assures him they are still present somewhere in the city and he resolves to find them. This stage precedes the common helper and subsequent party cleanup; it does not mean the Derris Emblem has been found.
        DerrisKharlanCompanionTrapTriggered = 22802000,
        /// Welgaia 0502 sub_433E: Lloyd and the available rescued companion note the abandoned city and Mithos's rejection of his own people. Lloyd refuses to abandon his friends and chooses to continue before this stage. The rescued-companion flag controls its original staging and dialogue.
        AbandonedWelgaiaArrivalDiscussionComplete = 22901000,
        /// Welgaia 0502 sub_4824 retains an alternate companion-trap scene: the available party disappears, Lloyd hears that they are elsewhere in the city and chooses to find them. This stage precedes the common helper and party cleanup. Keep it distinct from the analogous 0129 trap phase.
        WelgaiaCompanionTrapTriggered = 22902000,
        /// Welgaia 0500..0503 entry readers accept AbandonedWelgaiaArrivalDiscussionComplete..22903000 for companion-search staging: missing Chipped Dagger/Spider Figurine/Mirror Shard triggers, room 0502's actor 320 and record 521. Their item flags remain independent. The three rescue helpers each add 1000; the bound alone does not establish which companions or items were recovered.
        WelgaiaCompanionSearchSetupUpperBound = 22903000,
        /// Welgaia 0502 entry at this value dispatches one of eight missing-companion memory-return scenes when MithosMemoryReturnPending is set. The ordinary arrival writes 22901000; the Chipped Dagger, Spider Figurine and Mirror Shard rescue helpers each add 1000, once per respective flag. Retained trap phase 22902000 is separate. Preserve the arithmetic and flags instead of replacing them with a literal assignment.
        WelgaiaCompanionMemoryReturnPhase = 22904000,
        /// Welgaia 0502 sub_360D explains that the Derris Emblem opens the path to Mithos and disarms the trap, with Zelos/Kratos dialogue variants. The Emblem is granted and its found message displayed before this stage. The common helper and SideEventsSuppressed clear follow.
        DerrisEmblemReceived = 22905000,
        /// Dirk 0374 sub_7AEF selects season 9 in word 0x0954 at story >= this value. The caller processes pellets and the reward helpers select season-dependent figurine pools; messages 754/755 explicitly label season 9 common/rare. This is a figurine availability threshold, with no recovered constant setter.
        FigurineSeasonNineLowerBound = 22907000,
        /// Derris-Kharlan 0136 sub_0318: the party confirms its resolve to recover the Great Seed, reunite the worlds and reject sacrifice. Their shared decision to proceed precedes this stage and the restoration helper. The final battles are still ahead.
        FinalMithosBattleApproachReady = 23001000,
        /// Izoold 0353 entry and region 1500 accept RegalAltamiraVisitRequested..24002000 for Clara's monster setup and sub_77BB restoration scene. SideEventsSuppressed, word 0x013c and ClaraRestoredToHuman remain independent gates. This inclusive availability bound has no recovered constant setter and does not mean Clara has already been cured.
        ClaraIzooldRestorationUpperBound = 24002000,
        /// Derris-Kharlan 0136 sub_0E8C: formations 79 and 81 return, Lloyd destroys Mithos's crystal after his farewell and asks the Eternal Sword to restore the two worlds. This stage precedes SideEventsSuppressed and the ensuing world-link attempt; the Great Seed has not yet been awakened.
        FinalMithosDefeatedWorldRestorationRequested = 25001000,
        /// Tower 0152 sub_193C: after the worlds return to their true form, the sword warns that the land still needs a link and that retrieving Derris-Kharlan may destroy Lloyd's body. Lloyd insists and the sword agrees before this stage. The following camera sequence and failed seed-awakening attempt are distinct.
        EternalSwordDerrisKharlanLinkAttemptStarted = 30000000,
        /// Great Seed Awakening sub_0095 constructs actors 200, 210, 220 and 260 for its first ending camera sequence and waits for the track to finish. It writes this before its concluding sounds, stops the camera and returns to Tower 0152. No dialogue or completed awakening is asserted by this intermediate value.
        GreatSeedFirstEndingCameraSequenceComplete = 30000100,
        /// Tower 0152 sub_35C6: Lloyd says the mana is being deflected, Colette says the Great Seed is already dead and Lloyd pleads for it to wake. This stage follows that failed attempt; the later final wish uses a separate value.
        GreatSeedAwakeningFailureMarked = 30000200,
        /// Great Seed Awakening sub_02B6: Lloyd and Colette see the Eternal Sword return and make their final request to awaken the seed and raise the Giant Kharlan Tree. This stage follows that wish, before the epilogue chain beginning at Iselia; it is separate from the earlier failed attempt.
        GreatSeedFinalAwakeningWishComplete = 30001000,
        /// Iselia 0344 entry at 30001000 calls Colette's homecoming with Phaidra and Frank, then writes this before transfer to Altessa interior 0139. The stage queues the Raine and Genis epilogue rather than claiming it has already played.
        EpilogueAltessaVisitPending = 30002000,
        /// Altessa interior 0139 entry at 30002000 calls Raine and Genis's discussion of their new journey and enduring Iselia home. It then writes this before transfer to Mizuho 0326. The Mizuho epilogue is still pending.
        EpilogueMizuhoVisitPending = 30003000,
        /// Mizuho 0326 entry at 30003000 plays Sheena's homecoming and writes this only when ZelosDied is clear, before transfer to Meltokio 0405. The Zelos-dead route skips that visit and writes the separate Altamira-pending value.
        EpilogueMeltokioVisitPending = 30004000,
        /// Meltokio 0405 writes this after Zelos's royal peace-emissary discussion, while the Zelos-dead branch in Mizuho 0326 writes it directly after Sheena's homecoming. Both transfer to Altamira 0026. The shared value therefore does not prove the Meltokio epilogue was played.
        EpilogueAltamiraVisitPending = 30005000,
        /// Altamira 0026 entry at 30005000 plays Presea and Regal's scene at Alicia's grave, including the mine closure and rebuilding plans. It writes this before transfer to Tower 0152 for Lloyd and Kratos's farewell.
        EpilogueKratosFarewellPending = 30006000,
        /// Tower 0152 entry at 30006000 calls the farewell in which Kratos accepts responsibility for Cruxis and Lloyd calls him Dad. This stage is written before transfer to field 0360, whose final Kratos line and media wait have not yet completed.
        EpilogueKratosFinalWordsPending = 30007000,
        /// Field 0360 sub_0087 presents Kratos's final wish that Lloyd not die before him, waits for the media and fades out. It restores the controlled actor's visibility before this stage, then transfers to field 0532. Preserve that final handoff.
        EpilogueKratosFinalWordsComplete = 30008000,
        /// Altamira 0026 entry selects ThankForAll for EternalSwordDerrisKharlanLinkAttemptStarted..31000000 (and separately at RegalAltamiraVisitRequested), otherwise DanceInTheSunshine. This inclusive ending-music reader bound has no recovered constant setter or new epilogue event.
        AltamiraEndingMusicUpperBound = 31000000,
        /// Dirk exterior 0372 entry writes this after sub_455B's farewell at Anna's grave and its fade, before transferring to new-game settings. That field dispatches this value to the completion-data sequence; the separate clear-data marker is written there before opening the save menu.
        EndingClearDataPending = 99999999,
        /// Ossa 0350 entry uses OssaAssassinBattleComplete..999999999 for scenery animation (0,0,278,120,100,2). Ossa 0347 entry also retains a late range ending here with an empty body. Preserve the different lower bounds and the no-op branch. This is a local entry-dispatch limit, not the greatest valid Story value or a recovered event/setter.
        OssaEntryDispatchUpperBound = 999999999,
    }
}

ids! {
    pub enum StoryFlag: u16 {
        /// Pact completion events award the matching summon technique and gem:
        /// Triet Ruins 221, Thoda 10, Balacruf 510, Earth 237, Lightning 475,
        /// Ice 325 and Darkness 127. These are pacts, not the earlier seals.
        EfreetPactFormed = 1,
        UndinePactFormed = 2,
        SylphPactFormed = 3,
        GnomePactFormed = 4,
        VoltPactFormed = 5,
        CelsiusPactFormed = 6,
        ShadowPactFormed = 7,
        /// The Mana 0361 debug event explicitly marks the Aska quest cleared with this bit. Normal 0366 also sets it, together with flag 9, before entering the Light-pact battle; 0369 sets both again afterward. Preserve that early write rather than treating this as proof the pact battle was won.
        AskaQuestCompletionMarked = 8,
        /// Tower of Mana 0366 sets this alongside AskaQuestCompletionMarked after Kratos is stopped and Yuan sends the party onward, before changing to room 0369 for the Luna/Aska battle. Room 0369 writes it again after the Light pact, Topaz and summon-technique rewards, before the Great Seed movie. This names the observed Light-pact sequence marker; its early write does not prove the battle was won or the pact formed, and no literal reader is recovered.
        LightPactSequenceMarked = 9,
        /// Heimdall 484 and Exire 190: Origin and Maxwell pact events.
        OriginPactFormed = 10,
        MaxwellPactFormed = 11,
        VeriusReawakened = 12,
        EternalSwordObtained = 13,
        /// Ozette 425 applies Lloyd's Key Crest, restores Presea's costume and advances to the Presea-awakened story stage.
        PreseaAwakened = 17,
        /// Appearance selectors: content/src/appearance.rs::costume_frame.
        RaineExsphereReceived = 18,
        ColetteCruxisCrystalInactive = 19,
        /// Niflheim 0524 sub_3873 subtracts ten additional Soulfire from word 0x08c0 when this bit is set. Soulfire reward dialogue and the zero-flame checks establish the word; no literal setter identifies how this switch is enabled.
        NiflheimExtraSoulfireDrainEnabled = 21,
        /// Tethe'alla Base 266: Rheaird control explanation. Tethe'alla Bridge
        /// 78: Elemental Cargo control explanation.
        RheairdsObtained = 22,
        ElementalCargoObtained = 23,
        ColetteCruxisCrystalReceived = 24,
        GenisExsphereReceived = 25,
        /// Flanoir 0313 sub_742A sets this at the start of Kratos's private conversation, before its dialogue. It is the only recovered setter. The 0150 Tower betrayal uses it to select the Zelos battle/death branch rather than the Mizuho intervention, and later readers select Kratos-route staging. Other companions complete their night conversations without setting it; this is a route selection, not a generic night-complete bit.
        FlanoirKratosRouteSelected = 26,
        RegalFullNameRevealed = 27,
        PreseaCostumeChanged = 28,
        LloydExsphereUncovered = 29,
        /// First-entry/name-caption bits, set by the corresponding field entry.
        /// Room numbers distinguish separate original flags for the same location.
        DirksHouseVisited = 30,
        IseliaVisited = 31,
        TrietVisited = 32,
        IzooldVisited = 33,
        PalmacostaVisited = 34,
        AsgardVisited = 35,
        LuinVisited = 36,
        HimaVisited = 37,
        MeltokioVisited = 38,
        SybakVisited = 39,
        MizuhoVisited = 40,
        OzetteVisited = 41,
        AltessasHouseVisited = 42,
        HeimdallVisited = 43,
        FlanoirVisited = 44,
        AltamiraVisited = 45,
        ExireVisited = 46,
        KatzVillageVisited = 47,
        IslandRanch0079Visited = 48,
        OtherworldlyGateVisited = 49,
        IseliaForestVisited = 50,
        /// Shared original visit marker: Iselia Ranch 0193 records it after the early visit, and Palmacosta Ranch 0201 also sets it on entry. Map-service helpers consult this same bit. Preserve the original two-ranch coupling.
        Iselia0193OrPalmacosta0201VisitRecorded = 51,
        MartelTempleVisited = 52,
        SylvarantBaseVisited = 53,
        TrietRuinsVisited = 54,
        OssaTrailVisited = 55,
        PalmacostaRanchVisited = 56,
        ThodaGeyserVisited = 57,
        BalacrufMausoleumVisited = 58,
        AsgardRanchVisited = 59,
        TowerOfManaVisited = 60,
        TowerOfSalvation0147Visited = 61,
        TowerOfSalvation0148Visited = 62,
        FoojiMountainsVisited = 63,
        TetheAllaBridgeVisited = 64,
        Meltokio0466Visited = 65,
        GaoracchiaForestVisited = 66,
        TetheAllaBaseVisited = 67,
        ToizeValleyMineVisited = 68,
        TempleOfLightningVisited = 69,
        TempleOfIceVisited = 70,
        TempleOfEarthVisited = 71,
        TempleOfDarknessVisited = 72,
        IslandRanch0222Visited = 73,
        YmirForestVisited = 74,
        TowerOfSalvation0501Visited = 75,
        TowerOfSalvation0153Visited = 76,
        LatheonGorgeVisited = 77,
        TowerOfSalvation0168Visited = 78,
        TowerOfSalvation0184Visited = 79,
        Heimdall0479Visited = 80,
        TowerOfSalvation0152Visited = 81,
        DerrisKharlanVisited = 82,
        HakonesiaPeakVisited = 85,
        LakeUmacyVisited = 86,
        /// Linkite Tree 523: set on the first field entry.
        LinkiteTreeVisited = 87,
        HouseOfSalvation0081Visited = 88,
        ThodaDockVisited = 89,
        HouseOfSalvation0083Visited = 90,
        HouseOfSalvation0084Visited = 91,
        VirginiasHouseVisited = 92,
        HotSpringVisited = 96,
        HouseOfGuidance0091Visited = 98,
        SEAbbeyVisited = 99,
        HouseOfGuidance0093Visited = 100,
        /// Luin 0447/0448 set this when displaying the ruined-city caption. The recovery-caption branch also sets it to suppress a later ruined caption, so it does not prove the ruined caption was actually shown.
        LuinRuinedCaptionHandled = 102,
        ManualMovementLearned = 104,
        ManualBasicAttacksLearned = 105,
        ManualDefenseLearned = 106,
        ManualOtherActionsLearned = 107,
        ManualTargetingLearned = 108,
        ManualEXSkillSettingsLearned = 109,
        ManualSpecialFeaturesLearned = 110,
        ManualCompoundEXSkillsLearned = 111,
        ManualCombosLearned = 112,
        ManualSpecialAttackCombosLearned = 113,
        ManualStatusEffectsLearned = 114,
        ManualCuringTableLearned = 115,
        ManualStrategySettingsLearned = 116,
        ManualSettingOrdersLearned = 117,
        ManualIssuingOrdersLearned = 118,
        ManualSpAttackShortcutsLearned = 119,
        ManualUnisonAttackSettingsLearned = 120,
        ManualUsingUnisonAttacksLearned = 121,
        ManualLinkingSpecialAttacksLearned = 122,
        ManualEffectOfAttributesLearned = 123,
        ManualElementalBasicsLearned = 124,
        ManualOverLimitsLearned = 125,
        ManualGuardBreakLearned = 126,
        ManualGradeLearned = 127,
        ManualTechnicalSmashLearned = 128,
        ManualAdvancedUAttacksLearned = 129,
        ManualSummoningSpiritsLearned = 130,
        ManualSpecialAttackChangesLearned = 131,
        ManualBattleAdviceLearned = 132,
        ManualReversingRanksLearned = 133,
        /// Triet 485 trigger calls sub_49A6 then sets this even when Lloyd declines the customizer's invitation (170-180).
        TrietCustomizerPromptDismissed = 134,
        /// Triet 485 customizer introduces himself and requests a Beast Hide, then offers Soft Leather and the customization tutorial (182-202).
        TrietBeastHideCustomizationRequested = 135,
        /// Triet 485 sets this only after the special customization menu returns with ItemId::HardLeather in inventory; messages 204-210 explain the free armor.
        TrietHardLeatherCustomizationComplete = 136,
        /// Meltokio 392: first-clear prizes for each Coliseum division.
        ColiseumSoloBeginnerCleared = 137,
        ColiseumSoloIntermediateCleared = 138,
        ColiseumSoloAdvancedCleared = 139,
        ColiseumTeamNormalCleared = 140,
        ColiseumTeamManiaCleared = 141,
        /// The recovering-city entry caption sets this bit in Luin 0447/0448 at rebuilding step >=10000 and in 0449/0450 at >=300000; the older caption/visited bits are updated alongside it.
        LuinRecoveryCaptionShown = 142,
        /// Palmacosta 63 suggests the academy cafeteria waitress job to Colette (90-94); the separate shop-visit flag is always set after shopping.
        PalmacostaWaitressJobSuggested = 143,
        /// Palmacosta 63 item-shop interaction always sets this after opening the shop; the low-funds waitress suggestion occurs only on a qualifying first visit.
        MarbleItemShopVisited = 144,
        /// Palmacosta 56 sets this when the orienteering race starts and clears it on completion; other Palmacosta fields use it to provide checkpoints.
        PalmacostaOrienteeringInProgress = 145,
        /// Ossa Trail 352: Sword Dancer battle 87 ends and its actor despawns.
        FirstSwordDancerDefeated = 146,
        SwordDancerTailVisible = 147,
        SwordDancerWingsVisible = 148,
        /// Seal bosses: Triet Ruins 221, Thoda 10, Balacruf 510 and Mana 369.
        /// These flags are set after the guardian battles, before angel events.
        FireSealGuardianDefeated = 200,
        WaterSealGuardianDefeated = 201,
        WindSealGuardianDefeated = 202,
        LightSealGuardianDefeated = 203,
        /// Iselia 340, messages 179-194: optional Genis/Colette temple discussion.
        IseliaGenisTempleConversationSeen = 204,
        /// Entering room 0375 from 0372 clears this bit; the return to 0372 sets it. Dirk's room 0370 consults it at story 1602000 before its next scene.
        DirksHouseReturnedFrom0375 = 205,
        /// Martel 307 messages 38-55: the sealed door needs the temple's Sorcerer's Ring.
        MartelTempleSealedDoorExamined = 206,
        /// Martel Temple 308 tutorial and debug golem-spawn paths set story 105000, reveal the first floor block and enable the replenishing golem puzzle.
        MartelTempleGolemPuzzleStarted = 207,
        /// Martel Temple 308 dropped-block position selects floor block actor 5002, makes it visible and enables its collision; entry restores the same state from this bit.
        MartelTemple0308FloorBlock5002Placed = 208,
        /// Martel Temple 308 dropped-block position selects floor block actor 5003, makes it visible and enables its collision; entry restores the same state from this bit.
        MartelTemple0308FloorBlock5003Placed = 209,
        /// Martel Temple 308 dropped-block position selects floor block actor 5004, makes it visible and enables its collision; entry restores the same state from this bit.
        MartelTemple0308FloorBlock5004Placed = 210,
        /// Martel Temple 308 dropped-block position selects floor block actor 5005, makes it visible and enables its collision; entry restores the same state from this bit.
        MartelTemple0308FloorBlock5005Placed = 211,
        /// Martel Temple 308 dropped-block position selects floor block actor 5006, makes it visible and enables its collision; entry restores the same state from this bit.
        MartelTemple0308FloorBlock5006Placed = 212,
        /// Burning Iselia 343 school approach shows a defender threatened by the Desians and Genis asks Lloyd to save him (7-9).
        IseliaSchoolDesianThreatSeen = 213,
        /// Iselia 343 formation 136 victory rescues the school defender, who gives two Apple Gels and one Orange Gel and points toward remaining villagers (10-29).
        IseliaSchoolDefenderRescued = 214,
        /// Triet 485 messages 135-140: Lloyd and Genis examine Lloyd's wanted poster.
        TrietWantedPosterExamined = 215,
        /// Triet 491 messages 23-29: the oracle points to the oasis destroyed by Efreet.
        TrietOracleRevealedRuinsLocation = 216,
        /// Triet 485 messages 434-436: Genis reacts to damage and bloodstains left by Desians.
        TrietDesianDamageConversationSeen = 217,
        /// Triet 485 messages 389-394: the Chosen's impression is mistaken for Desian damage.
        TrietChosenCraterConversationSeen = 218,
        /// Triet 489 messages 20-26: Lloyd's nighttime conversation ends with Colette's promise.
        TrietColetteNightConversationSeen = 219,
        /// Triet 490 messages 45-51: Raine discusses Lloyd's exile and changing the world.
        TrietRaineExileConversationSeen = 220,
        /// Triet 490 messages 65-71: Lloyd and Raine hear Genis talking in his sleep.
        TrietGenisSleepConversationSeen = 221,
        /// Triet 487 messages 222-229: Lloyd notices Kratos leaving and chooses whether to follow.
        TrietKratosNightDepartureSeen = 222,
        /// Triet 485 messages 128-134: Kratos warns Lloyd about vulnerabilities in his swordsmanship.
        TrietKratosSwordAdviceSeen = 223,
        /// Triet 527: Lloyd chooses to go to sleep during the inn's nighttime event.
        TrietNightRestChosen = 224,
        /// Izoold 354: the setting event grants ItemId::LylaSLetter.
        LylasLetterReceived = 225,
        /// Izoold 354 message 387: Max's fishing boat is the only ship still operating.
        IzooldMaxBoatRumorHeard = 226,
        /// Palmacosta Ranch 202: this interaction grants ItemId::PurpleCard.
        PalmacostaRanchPurpleCardObtained = 227,
        /// Palmacosta Ranch 202: Colette investigates a sparkle and receives ItemId::RedCard.
        PalmacostaRanchRedCardObtained = 228,
        /// Palmacosta Ranch 202/204: either source grants ItemId::BlueCard and sets this bit.
        PalmacostaRanchBlueCardObtained = 229,
        /// Triet Ruins 220 stone block 5001 placed at z=-150; entry restores the socket at x=-1275, y=75. The two sockets have separate bits; retain the original x-test/else behavior.
        TrietRuins0220Block5001AtSocketXMinus1275 = 230,
        /// Triet Ruins 220 checks block actor 5002 at z=-150 and records its placed state; entry restores its fixed socket position.
        TrietRuins0220Block5002Placed = 231,
        /// Triet Ruins 220 fire contact with target 5020 lights its fire effect and raises platform 5010; entry restores the lit state, and early temple entry can reset these puzzle bits.
        TrietRuins0220FireSwitch5020Activated = 232,
        /// Triet Ruins 220 fire contact with target 5031 lights its fire effect and raises platform 5011; entry restores the lit state, and early temple entry can reset these puzzle bits.
        TrietRuins0220FireSwitch5031Activated = 233,
        /// Triet Ruins 220 fire contact with target 5022 lights its fire effect and raises platform 5012; entry restores the lit state, and early temple entry can reset these puzzle bits.
        TrietRuins0220FireSwitch5022Activated = 234,
        /// Triet Ruins 220 fire contact with target 5023 lights its fire effect and raises platform 5013; entry restores the lit state, and early temple entry can reset these puzzle bits.
        TrietRuins0220FireSwitch5023Activated = 235,
        /// Triet Ruins 220 fire contact with target 5024 lights its fire effect and raises platform 5014; entry restores the lit state, and early temple entry can reset these puzzle bits.
        TrietRuins0220FireSwitch5024Activated = 236,
        /// Luin 465 messages 15-29: Piccaring has the Tower of Mana key and is on a pilgrimage.
        PiccaringPilgrimageLearned = 237,
        /// Luin 448: set in the post-1601000 route before the affinity check that determines whether Sheena joins.
        LuinSheenaLateRouteConversationSeen = 238,
        /// The only observed literal writer is the Mana 0361 debug menu, which grants Boltzman's Book and sets this bit, or clears it on request. Mana 0362 reads it to select Sheena's already-obtained-the-book encounter dialogue. This is separate from actual inventory ownership.
        SheenaEncounterBoltzmanBookVariantSelected = 239,
        /// Mana 362 messages 170-176: the encounter ends with Kratos refusing to explain his recognition of Sheena.
        TowerOfManaSheenaEncounterSeen = 240,
        /// Mana 361 message 1: the examined entrance is locked.
        TowerOfManaLockedDoorExamined = 241,
        /// Hima 300 both answers to Sophia's question about Sheena end with an invitation to speak privately at her house (41-66), before this first-visit marker.
        SophiaPrivateDiscussionInvited = 242,
        /// Hima 304 messages 28-35: Lloyd agrees to seek Boltzman's healing technique.
        PietroHealingRequestAccepted = 243,
        /// Palmacosta 0065 uses this bit to create Clara actor 202 alongside Neil and enable her three responses in his follow-up. No literal setter establishes whether it denotes healing or some other prerequisite.
        ClaraAtNeilFollowupEnabled = 245,
        /// The original Mana 0361 debug menu displays Getting the Tower of Mana key and sets this bit. No literal reader is present in the corpus; this name records the explicit debug operation rather than inventing a live quest condition.
        TowerOfManaDebugKeyAcquisitionMarked = 246,
        /// Palmacosta Ranch 207 messages 65-73: Magnius orders Chocolat to be taken away.
        ChocolatTakenByMagnius = 248,
        /// House of Salvation 95 message 269: the unicorn may appear on Lake Umacy's surface.
        UnicornRumorHeard = 249,
        /// Lake Umacy 359: the unicorn appears, but the party cannot reach it.
        LakeUmacyUnicornSighted = 250,
        /// Lake Umacy 359: the party learns it needs the water spirit and heads for Thoda Geyser.
        UnicornRequiresUndineLearned = 251,
        /// Lake Umacy 359 messages 414-420: the party has obtained the horn to heal Pietro.
        UnicornHornObtained = 252,
        /// Palmacosta 67 Marche discusses the Book of Regeneration, its lack of a church copy and the impostors, then this conversation marker is set.
        PastorMarcheRegenerationBookDiscussed = 255,
        /// Palmacosta 65 initial and post-ranch audience variants explain Dorr gave the Book of Regeneration to impostors; the common audience-exit routine and entry continuation set this marker.
        DorrRegenerationBookLossDiscussed = 256,
        /// Hakonesia 124 messages 7-13: Koton recommends the paid Asgard ruin tour.
        KotonPassageFeeLearned = 258,
        /// House of Salvation 99: the Grand Priest identifies where the Spiritua Statue is displayed.
        SpirituaStatueLocationLearned = 259,
        /// House of Salvation 95/99: Colette talks with Sheena about her prayer to kill the Chosen.
        SheenaChurchPrayerConversationSeen = 260,
        /// House of the Goddess 105: the alternate Sheena prayer conversation has its own flag.
        SheenaGoddessPrayerConversationSeen = 261,
        /// Luin 453/456: Sheena plays hide-and-seek with the children before speaking with Lloyd.
        SheenaLuinChildrenEncounterSeen = 262,
        /// Balacruf 508 messages 100-106: the Sheena encounter ends with Kratos refusing to explain his recognition.
        BalacrufSheenaEncounterSeen = 263,
        /// Martel 308: the setting event grants ItemId::SorcererSRing and explains its use.
        SorcerersRingObtained = 264,
        /// Palmacosta 57 both party-Sheena and hostile-Sheena variants introduce the steamship Isabelle and its allegedly ancient technology.
        IsabelleSteamshipIntroduced = 265,
        /// Thoda Dock 97 messages 127-133: Candy explains the sightseeing boat and its 200 Gald fare.
        ThodaBoatFareExplained = 266,
        /// Thoda Dock 97 messages 114-117: Candy recognizes the Chosen and waives the fare.
        ThodaBoatChosenRecognized = 267,
        /// After the Pastor Marche book discussion, Palmacosta 0056 runs the applicable Dorr/impostor/Book of Regeneration follow-up and records this bit. The branch depends on prior book-route progress; it is not proof the book has been recovered.
        PalmacostaPastorBookFollowupHandled = 268,
        /// After Sheena falls into the Ossa trapdoor, this bit enables Lloyd saying she may have attacked the party at the inn. No literal setter is present, so the name describes this optional reference rather than claiming an inn event completed.
        OssaInnAttackReferenceEnabled = 269,
        /// Palmacosta 67 messages 332-338: Chocolat offers a discounted future pilgrimage tour.
        ChocolatDiscountTourOffered = 270,
        /// Triet Ruins 219: actors 7551/7552 despawn and the obstruction-opening animation completes.
        TrietRuinsEntranceObstructionCleared = 271,
        /// Thoda 6 messages 484-489: the party examines the missing stairs and Kratos explains their response to the Chosen.
        ThodaRetractingStairsExamined = 272,
        /// Triet 525 messages 12-18: Genis reminds Lloyd about finding the party and examining the wanted poster.
        TrietDepartureWarningSeen = 273,
        /// Triet 491 sets this after paying, refusing or lacking the 100 Gald fee; it does not prove the fortune-teller's location clue was heard.
        TrietFortuneTellerFeeOfferSeen = 274,
        /// Iselia 338: Genis grants three each of Bread, Lettuce and Beef after the cooking introduction.
        CookingTutorialSuppliesReceived = 275,
        /// Luin rebuilding step 600000 constructs the statues in order 277,278,279 after the corresponding donation thresholds. This bit restores Sheena's statue scenery and its related interactions; statue-examined bits are separate.
        LuinSheenaStatueBuilt = 277,
        /// Luin rebuilding step 600000 constructs the statues in order 277,278,279 after the corresponding donation thresholds. This bit restores Lloyd's statue scenery and its related interactions; statue-examined bits are separate.
        LuinLloydStatueBuilt = 278,
        /// Luin rebuilding step 600000 constructs the statues in order 277,278,279 after the corresponding donation thresholds. This bit restores Raine's statue scenery and its related interactions; statue-examined bits are separate.
        LuinRaineStatueBuilt = 279,
        /// Both Toize 413 first Gnomelette conversations introduce his journey to find potion and run the offer/refuse menu; return to the temple is a separate flag.
        ToizeGnomelettePotionRequestIntroduced = 280,
        /// Toize 413 messages 88-94: Gnomelette 1 finishes the potion and leaves for the Temple of Earth.
        GnomeletteOneReturnedToTemple = 281,
        /// Altessa 139 messages 647-653: Gnomelette 2 agrees to go home after Lloyd explains they are looking for him.
        GnomeletteTwoReturnedToTemple = 282,
        /// Toize 412 messages 75-81: the party discusses the mine and helping Presea.
        ToizeMineEntryConversationSeen = 289,
        /// Toize Valley Mine 0412 entry hides scenery actor 999998 node 8 when this bit is clear. When set it retains the resource default; no literal setter or verified node identity establishes what object this represents.
        ToizeValley0412SceneryNode8Retained = 290,
        /// Toize 412 messages 53-59: Regal explains the Sorcerer's Ring's delayed explosive.
        ToizeBombRingExplained = 291,
        /// Toize 412: first set while the party discusses hitting the inaccessible lever; also set by its activation event.
        ToizeMineCartSwitchDiscovered = 292,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 111's breaking animation, fades it, removes it and contact actor 121, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget111Destroyed = 293,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 112's breaking animation, fades it, removes it and contact actor 122, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget112Destroyed = 294,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 113's breaking animation, fades it, removes it and contact actor 123, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget113Destroyed = 295,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 114's breaking animation, fades it, removes it and contact actor 124, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget114Destroyed = 296,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 115's breaking animation, fades it, removes it and contact actor 125, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget115Destroyed = 297,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 116's breaking animation, fades it, removes it and contact actor 126, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget116Destroyed = 298,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 117's breaking animation, fades it, removes it and contact actor 127, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget117Destroyed = 299,
        /// Toize Mine 413 Bomb-mode blast within 400 units plays actor 118's breaking animation, fades it, removes it and contact actor 128, then records this bit; entry omits that destroyed target.
        ToizeMine0413BombTarget118Destroyed = 300,
        /// Toize 0413: starting a boulder run sets this latch; finishing the run or either collision sequence clears it. Entry clears it as well, and the trigger refuses another run while it is set.
        ToizeMineRollingBoulderActive = 301,
        /// Altamira 27: George unlocks the mining records room; Altamira 25:
        /// Sheena exchanges Corrine's bell for the zircon shipment document.
        LezarenoMiningArchivesUnlocked = 302,
        KuchinawaDuelPromised = 303,
        /// Sybak 437, messages 297-303: zircon is in the unlocked next room.
        SybakZirconStorageUnlocked = 304,
        /// Heimdall 292 messages 23-30: the elf thanks the party after being rescued.
        Heimdall0292ElfRescued = 305,
        /// Heimdall 291 messages 18-23: the elf thanks the party; Kratos checks whether all villagers are rescued.
        Heimdall0291ElfRescued = 306,
        /// Heimdall 291: a brother and sister escape together; the elder evacuation in room 295 and a room-291 transition also set this same bit.
        HeimdallSiblingsOrElderRescued = 307,
        /// Salvation 503: either reunion variant grants ItemId::MirrorShard and sets this flag.
        MirrorShardObtained = 308,
        /// Salvation 501: the setting event grants ItemId::SpiderFigurine.
        SpiderFigurineObtained = 309,
        /// Salvation 500: either reunion variant grants ItemId::ChippedDagger and sets this flag.
        ChippedDaggerObtained = 310,
        /// Salvation 503: set after battle formation 76, mapped to the-neglected by the original formation data.
        TheNeglectedDefeated = 311,
        /// Salvation 501: set after battle formation 77, mapped to the-fugitive by the original formation data.
        TheFugitiveDefeated = 312,
        /// Salvation 500: set after battle formation 78, mapped to the-judged by the original formation data.
        TheJudgedDefeated = 313,
        /// Sybak 434 Kratos confronts Lloyd, warns Colette about the Key Crest and leaves the party to seek Kate (58-91).
        KratosSybakConfrontationSeen = 314,
        /// Both Ozette 426 and 433 variants officially welcome Presea into the group after her awakening and advance story to 11904000.
        PreseaWelcomedAfterAwakening = 315,
        /// Meltokio 376, messages 173-202; Altamira 11, messages 59-64.
        KratosSacredWoodWarningSeen = 316,
        RegalInformedOfAliciasGrave = 318,
        /// Altamira 24 messages 103-112: the wounded guard reports Vharley's intrusion and the party enters; an unused later reveal variant also sets this bit.
        LezarenoVharleyConfrontationStarted = 319,
        /// Both room 0468 exits set this before restoring full player size and changing to Meltokio 0380; that destination consumes and clears it to run the story-dependent exit scene.
        Meltokio0468ExitScenePending = 320,
        /// Toize Mine 414 Bomb-mode explosions mark target 111 destroyed, and entry omits its model/collider when this bit is set.
        ToizeMine0414BombTarget111Destroyed = 321,
        /// Toize Mine 414 Bomb-mode explosions mark target 112 destroyed, and entry omits its model/collider when this bit is set. Targets 112-114 are destroyed together by any blast on their group.
        ToizeMine0414BombTarget112Destroyed = 322,
        /// Toize Mine 414 Bomb-mode explosions mark target 113 destroyed, and entry omits its model/collider when this bit is set. Targets 112-114 are destroyed together by any blast on their group.
        ToizeMine0414BombTarget113Destroyed = 323,
        /// Toize Mine 414 Bomb-mode explosions mark target 114 destroyed, and entry omits its model/collider when this bit is set. Targets 112-114 are destroyed together by any blast on their group.
        ToizeMine0414BombTarget114Destroyed = 324,
        /// Toize Mine 414 Bomb-mode explosions mark target 115 destroyed, and entry omits its model/collider when this bit is set. Targets 115,117,118 are coupled; hitting 116 also destroys any remaining members of that group.
        ToizeMine0414BombTarget115Destroyed = 325,
        /// Toize Mine 414 Bomb-mode explosions mark target 116 destroyed, and entry omits its model/collider when this bit is set.
        ToizeMine0414BombTarget116Destroyed = 326,
        /// Toize Mine 414 Bomb-mode explosions mark target 117 destroyed, and entry omits its model/collider when this bit is set. Targets 115,117,118 are coupled; hitting 116 also destroys any remaining members of that group.
        ToizeMine0414BombTarget117Destroyed = 327,
        /// Toize Mine 414 Bomb-mode explosions mark target 118 destroyed, and entry omits its model/collider when this bit is set. Targets 115,117,118 are coupled; hitting 116 also destroys any remaining members of that group.
        ToizeMine0414BombTarget118Destroyed = 328,
        /// Tower of Salvation 165 sets this on the accepted Go down choice and clears it on the accepted Go up choice; entry restores the corresponding platform/collision state.
        TowerOfSalvation0165LowerLevelSelected = 329,
        /// Luin 448 wounded Sheena explains the Desian raid and abduction of the population; two Asgard Ranch debug menus can also force this aftermath marker.
        LuinAttackAftermathIntroduced = 330,
        /// World entry clears this bit; world region event 44 sets it before routing through Luin rebuilding variants, including the field 0241 detour. This records the selected original entrance, not a Luin quest milestone.
        LuinWorldEntrance44Selected = 331,
        /// Salvation 0152 sub_0695 omits the Presea-or-Regal dais observation and Lloyd identifying the Eternal Sword when this bit is set. The following final-battle readiness prompt still runs. No literal setter establishes an earlier milestone.
        FinalSwordDaisDescriptionSuppressed = 332,
        /// Balacruf 0509: pinwheel sequence 1,4,2,3,5 requests the door sequence; this persistent flag is set after the door opens and entry restores access to seal room 0510.
        BalacrufMausoleum0509SealRoomDoorOpened = 333,
        /// Ymir 519 messages 66-72: the party discusses why the new ring ability only works in certain places.
        YmirRingAbilityExamined = 334,
        /// Ymir 519 messages 111-117: the party connects the ring's animal call with flowers and tree stumps.
        YmirAnimalCallingExplained = 335,
        /// Ymir 519: the fruit pickup grants ItemId::YmirFruit and sets this flag.
        YmirFruitObtained = 336,
        /// Salvation 502 messages 237-243: the party lacks identification and plans to use the emergency route.
        WelgaiaIdentificationRequirementLearned = 337,
        /// Salvation 503 messages 188-194: the party examines the terminal/emergency route and discusses the Mana Fragment prerequisite.
        WelgaiaEmergencyRouteExamined = 338,
        /// Balacruf 0509: sequence 5,1,4,3,2 opens BlueStoneDoor actor 201 and spawns encounter actor 8102, formation 42. The flag prevents that trap from triggering again; it does not mark a won battle.
        BalacrufMausoleum0509TrapDoor201Opened = 339,
        /// Balacruf 0509: sequence 1,2,4,3,5 opens BlueStoneDoor actor 202 and spawns encounter actor 8103, formation 44. The flag prevents that trap from triggering again; it does not mark a won battle.
        BalacrufMausoleum0509TrapDoor202Opened = 340,
        /// Both Hima quest routes explain Pietro's curse, the Boltzman healing technique and the Tower of Mana; this does not imply the unconditional promise tracked by PietroHealingRequestAccepted.
        SophiaBoltzmanBookQuestExplained = 341,
        /// Hima 304: Pietro is cured and explains the Angelus Project.
        PietroHealed = 342,
        /// Hima 300: Clara's key is identified as the Tower of Mana key and ItemId::TowerKey is granted.
        TowerOfManaKeyObtained = 343,
        /// Hima 304 messages 169-175: Raine tells Sophia she found the technique but cannot use it yet.
        PietroHealingTechniqueNotMasteredExplained = 344,
        /// Hima 301 initial lookout conversation introduces the merchant's planned Dragon Tours business (3-33); transport booking is separate.
        HimaDragonToursIntroduced = 345,
        /// Hima 300 messages 178-184: Lloyd proposes using the dragon-tour operator's dragons to reach Salvation.
        HimaDragonRidePlanMade = 346,
        /// Hima 300 messages 185-188: Raine proposes observing the Tower from the mountain behind the village.
        HimaTowerViewSuggested = 347,
        /// Both Hima 301 booking variants arrange three free dragons for the Chosen for the next morning, whether or not the earlier business introduction occurred.
        HimaDragonTransportArranged = 348,
        /// Latheon 497: the party chooses to drop the rock to split the waterfall and the scenery sequence sets this bit.
        LatheonWaterfallRockDropped = 349,
        /// Hima 300 messages 222-228: Lloyd reassures Genis during the nighttime farewell conversations.
        HimaGenisNightConversationSeen = 350,
        /// Hima 300 messages 236-242: Raine discusses Colette's angel transformation and sends Lloyd to her.
        HimaRaineNightConversationSeen = 351,
        /// Hima 300 messages 254-260: Lloyd and Sheena discuss the two worlds and two Chosen.
        HimaSheenaNightConversationSeen = 352,
        /// Hima 300: Kratos's farewell conversation ends with his warning not to do anything regrettable.
        HimaKratosNightConversationSeen = 353,
        /// Luin 447 messages 51-54: Lloyd and Colette react to the destroyed town on entry.
        LuinDestructionWitnessed = 354,
        /// Bridge 75 messages 25-31: Genis compares Tethe'alla's supposedly thin mana with Sylvarant.
        TetheAllaBridgeManaConversationSeen = 355,
        /// Bridge 76 and Fooji 418: the party discusses the sudden monsters and possible mana decline/Cruxis involvement.
        TetheAllaMonsterAppearanceDiscussed = 356,
        /// Sybak 0443 creates door record 6002 toward room 0435 only when this bit is clear. Transfers to/from 0435 and the Sheena reunion set it; the exit toward room 0442 clears it. Preserve the retail clear-then-set sequence on one 0435 exit.
        Sybak0443Door6002Suppressed = 357,
        /// Heimdall 0282/0283 use this state while Genis and Raine wait apart from the party because half-elves cannot use village facilities. The helpers snapshot the original formation and remove them; rejoining or leaving clears the bit.
        HeimdallHalfElvesWaitingApart = 358,
        /// Room 0466 switch 1100: after its first scenery animation sequence, the flag is set. Subsequent uses skip the camera sequence and use shorter animation timing; it does not encode the current switch position.
        Meltokio0466Switch1100FirstUseSequenceSeen = 359,
        /// Palmacosta Ranch 198 messages 75-79: the password entry succeeds and the lock disengages.
        PalmacostaRanchPasswordAccepted = 360,
        /// Meltokio 376 messages 70-76: Colette's interaction with the dog is observed by the mysterious man.
        MeltokioColetteDogEncounterSeen = 361,
        /// Exire 189 messages 16-22: Raine realizes the strange elf woman's house may be her mother's.
        VirginiasHouseLocated = 362,
        /// Exire 188 messages 35-41: Virginia fails to recognize Raine and Genis during the reunion.
        VirginiaReunionSeen = 363,
        /// Exire 189 messages 44-52: the elder explains Virginia and Kloitz's history and offers Virginia's diary.
        VirginiasHistoryHeard = 364,
        /// Exire 185: the setting event grants ItemId::VirginiaSDiary.
        VirginiasDiaryObtained = 365,
        /// Iselia variants: Colette refuses to re-enter while the villagers continue excluding Lloyd.
        IseliaLloydEntryRefusalSeen = 366,
        /// Iselia 342 messages 1-7: Lloyd confronts the Desians attacking surviving villagers.
        IseliaDesianAttackEncounterSeen = 367,
        /// Iselia 341 messages 179-185: Genis laments losing another possible home before fighting the Desians.
        IseliaGenisLostHomeConversationSeen = 369,
        /// Ymir 521 messages 56-63: the party agrees to search with guidance from the forest butterflies.
        YmirMissingChildRescueRequested = 370,
        /// Ymir 0519: the animal sequence removes blocker 3800 and creates the Solar Guard chest, then sets this bit. Entry restores that chest; the opened-treasure flag is separate.
        YmirSolarGuardChestUncovered = 371,
        /// Altessa's house 0139: once Gnomelette 1 has returned, Gnomelette 2 agrees to go home. Both dialogue variants set this bit; entry omits him and Earth 0236 checks his departure separately from his returned-to-temple flag.
        GnomeletteTwoDepartedAltessasHouse = 372,
        /// Iselia 331 messages 13-19: Genis reminds Lloyd about the Chosen's test and Colette's house.
        IseliaColetteHouseReminderSeen = 373,
        /// Iselia 330 messages 23-29: the detour reminder directs Lloyd to Genis's house before Colette's.
        IseliaGenisHouseReminderSeen = 374,
        /// Bridge 76 messages 23-30: the blocked passage forces the party to return to Sybak.
        TetheAllaBridgeClosureLearned = 375,
        /// Meltokio 468 messages 103-109: the party connects the Pope, Vharley and Rodyle's experiments.
        PopeExsphereConspiracyLearned = 376,
        /// Heimdall 283 messages 296-304: an elf explains that Aionis and the elves came from Derris-Kharlan.
        AionisOriginLearned = 377,
        /// Thoda 7 messages 35-41: Lloyd explicitly asks whether Colette still agrees to release the seal.
        ThodaSealReleaseConsentDiscussed = 378,
        /// Altamira 0011: the flashback shows Vharley demanding the mine and revealing Alicia as a failed Exsphere experiment; its completion changes to room 0026, which reads this flag to continue Regal's story.
        RegalAliciaExsphereFlashbackSeen = 379,
        /// Gaoracchia 0513: clearing animation for barrier actor 710 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0513Barrier710Cleared = 380,
        /// Gaoracchia 0513: clearing animation for barrier actor 713 removes its blocking collider; the entry branch replays the cleared state from this bit. Retail entry uses actor 714 rather than the clearing branch's actor 713; this mismatch is preserved.
        GaoracchiaForest0513Barrier713Cleared = 381,
        /// Gaoracchia 0513: clearing animation for barrier actor 714 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0513Barrier714Cleared = 382,
        /// Gaoracchia 0513: clearing animation for barrier actor 715 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0513Barrier715Cleared = 383,
        /// Gaoracchia 0512: clearing animation for barrier actor 710 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0512Barrier710Cleared = 384,
        /// Gaoracchia 0514: clearing animation for barrier actor 720 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0514Barrier720Cleared = 385,
        /// Gaoracchia 0515: clearing animation for barrier actor 710 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0515Barrier710Cleared = 386,
        /// Gaoracchia 0515: clearing animation for barrier actor 712 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0515Barrier712Cleared = 387,
        /// Gaoracchia 0515: clearing animation for barrier actor 713 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0515Barrier713Cleared = 388,
        /// Gaoracchia 0513: clearing animation for barrier actor 711 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0513Barrier711Cleared = 389,
        /// Gaoracchia 0513: clearing animation for barrier actor 712 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0513Barrier712Cleared = 390,
        /// Gaoracchia 0515: clearing animation for barrier actor 711 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0515Barrier711Cleared = 391,
        /// Gaoracchia 0514: clearing animation for barrier actor 721 removes its blocking collider; the entry branch replays the cleared state from this bit.
        GaoracchiaForest0514Barrier721Cleared = 392,
        /// All seven Latheon room variants explain feeding flowers Kirima and enable the feeding menu; this introduction does not prove any fruit was fed.
        LatheonFlowerFeedingIntroduced = 393,
        /// Latheon 0492: the lower-entry branch builds flowers/carriers 3000..3002/3400..3402 and sets this bit; the upper-entry layout clears it. The flight controller selects its interaction loop from this layout state.
        LatheonGorge0492LowerFlightLayoutActive = 394,
        /// Returning from Latheon 0494 to 0492 at Z 1906/2097 sets this upper-entry layout selector; the lower returns from 0493 or the middle exit clear it. Room 0492 builds the matching flower/flight layout.
        LatheonGorge0492UpperEntrySelected = 395,
        /// The early caravan visitor scene has Lloyd and Genis describe their journey as a pilgrimage and accept hospitality; later resident dialogue in caravan variants 0239/0243 remembers this meeting.
        NovaCaravanEarlyHospitalityReceived = 396,
        /// Toize 0415: the first trap scene shows the Papal Knights complaining about Bryant's traps; Zelos recognizes them and Lloyd urges the party to run. Later visits use only the short warning.
        ToizeMinePapalKnightsEncounterSeen = 397,
        /// Iselia Ranch 0194: Mana-mode ring contact removes target actor(s) 730, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0194ManaTarget730Activated = 398,
        /// Iselia Ranch 0194: Mana-mode ring contact removes target actor(s) 731, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0194ManaTarget731Activated = 399,
        /// Iselia Ranch 0194: Mana-mode ring contact removes target actor(s) 732And733, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0194ManaTarget732And733Activated = 400,
        /// Iselia Ranch 0194: the lift's upward journey to Z 300 sets this bit and its downward journey to Z -600 clears it. Entry initializes it from the player's arrival level before positioning lift actor 740.
        IseliaRanch0194LiftAtUpperLevel = 401,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 730, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget730Activated = 402,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 731, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget731Activated = 403,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 732, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget732Activated = 404,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 733, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget733Activated = 405,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 734, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget734Activated = 406,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 735, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget735Activated = 407,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 736, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget736Activated = 408,
        /// Iselia Ranch 0196: Mana-mode ring contact removes target actor(s) 737, sets this bit and creates the corresponding persistent mana effect; other ring modes do not activate it.
        IseliaRanch0196ManaTarget737Activated = 409,
        /// Latheon 0492 entry creates script record 4025 only while this bit is clear. If set and flag 411 is clear, it instead calls the helper creating record 4026. No literal writer establishes a broader puzzle-completion meaning.
        LatheonGorge0492Record4025Suppressed = 410,
        /// Latheon 0492 entry calls sub_42D2 to create script record 4026 only with flag 410 set and this bit clear. No literal setter establishes the trigger's wider puzzle meaning.
        LatheonGorge0492Record4026Suppressed = 411,
        /// All seven Latheon room variants identify the Kirima plant and suggest collecting some; actual pickups follow separate repeated code.
        LatheonKirimaPlantIntroduced = 412,
        /// All seven Latheon room variants identify Amango and discuss its possible use despite flowers preferring Kirima; actual pickups are separate.
        LatheonAmangoPlantIntroduced = 413,
        /// Dirk 374 and Altessa 139 both explicitly award an Exsphere Shard.
        ExsphereShardObtained = 414,
        /// Hakonesia: a rescued captive reports someone resembling Chocolat was sent to Iselia Human Ranch; the party discusses waiting for an opportunity to rescue her, then records this report.
        ChocolatsIseliaRanchLocationLearned = 415,
        /// Triet's first intruder aftermath has Lloyd explain that a strange man entered, Raine warn the group and Sheena deny involvement if present. Later incidents use a shorter reaction after this flag is set.
        TrietInnIntruderIncidentSeen = 416,
        /// The Triet inn receptionist sets this before sending the party to room 0488 for the mandatory first intruder scene or a later random incident; the room's incident handler clears it afterward.
        TrietInnIntruderScenePending = 417,
        /// Asgard and Palmacosta inn routes share this bit: Colette screams, says a strange person approached and that she struck him, then the room handler clears the bit before returning to reception.
        ColetteInnIntruderScenePending = 418,
        /// Luin's inn routes a random night incident through room 0459; Kratos says their friend was back and hardly a threat, then the bit is cleared before returning to reception.
        LuinKratosIntruderScenePending = 419,
        /// Palmacosta: Neil discusses the freed captives and asks the party not to expose Dorr's betrayal yet; both answer choices converge, and the party wishes the Acting Governor-General luck before this bit is set.
        NeilActingGovernorConversationSeen = 420,
        /// One shared first-interaction bonus: accepting Colette's good-night offer awards +120 affinity, talking to Genis or Raine awards +60, and talking to Kratos awards +180. Any one sets this latch and prevents the other three bonuses; conversations can still repeat.
        DirksHouseFirstEveningAffinityBonusClaimed = 421,
        /// Across four Luin variants, Pietro thanks the party for rebuilding the city at steps 400000..499999 and proposes expanding it as a sign of resistance. This bit records that explanation, separately from the final completion thanks.
        LuinExpansionPlanExplained = 422,
        /// Balacruf 0508: blue switch block 5000 reaches socket (-2475,525,-10), is replaced with a stationary collision block, and entry restores that placed state.
        BalacrufMausoleum0508Block5000Placed = 423,
        /// Balacruf 0508: blue switch block 5001 reaches socket (1275,3525,-10), is replaced with a stationary collision block, and entry restores that placed state.
        BalacrufMausoleum0508Block5001Placed = 424,
        /// Toize 0413: wall switch 102 toggles this bit and its pressed animation; the boulder trigger runs only while the bit is clear, matching Regal's explanation that the switch stops the boulders.
        ToizeMineBoulderTrapDisabled = 425,
        /// Toize 0413: the first boulder chase moves Lloyd out of its path; Zelos complains and Regal explains the wall switch. This bit skips that introduction on later runs.
        ToizeMineBoulderTrapIntroduced = 427,
        /// Toize 0413: the boulder crushes Bacura actor 140, replacing it with wreckage actor 142 and an Evil Eye pickup trigger. Entry restores the wreckage; obtaining the Devil's Arm uses a separate flag.
        ToizeMineBacuraDestroyed = 429,
        /// Latheon 0492 flower actor 3000: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3000Blowing = 430,
        /// Latheon 0492 flower actor 3001: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3001Blowing = 431,
        /// Latheon 0492 flower actor 3002: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3002Blowing = 432,
        /// Latheon 0492 flower actor 3003: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0492Flower3003Blowing = 433,
        /// Latheon 0492 flower actor 3004: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3004Blowing = 434,
        /// Latheon 0492 flower actor 3005: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3005Blowing = 435,
        /// Latheon 0492 flower actor 3006: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3006Blowing = 436,
        /// Latheon 0492 flower actor 3007: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3007Blowing = 437,
        /// Latheon 0492 flower actor 3008: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0492Flower3008Blowing = 438,
        /// Latheon 0492 flower actor 3009: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3009Blowing = 439,
        /// Latheon 0492 flower actor 3010: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0492Flower3010Blowing = 440,
        /// Latheon 0492 flower actor 3011: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0492Flower3011Blowing = 441,
        /// Latheon 0492 bubble-flight segment 12, carrier actor 3412: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0492BubbleFlightSegment12Active = 442,
        /// Latheon 0492 bubble-flight segment 13, carrier actor 3413: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0492BubbleFlightSegment13Active = 443,
        /// Latheon 0492 bubble-flight segment 14, carrier actor 3414: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0492BubbleFlightSegment14Active = 444,
        /// Latheon 0493 flower actor 3000: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0493Flower3000Blowing = 445,
        /// Latheon 0493 flower actor 3001: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0493Flower3001Blowing = 446,
        /// Latheon 0493 flower actor 3002: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0493Flower3002Blowing = 447,
        /// Latheon 0493 bubble-flight segment 3, carrier actor 3403: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0493BubbleFlightSegment3Active = 448,
        /// Latheon 0494 flower actor 3000: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0494Flower3000Blowing = 449,
        /// Latheon 0494 flower actor 3001: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0494Flower3001Blowing = 450,
        /// Latheon 0494 flower actor 3002: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0494Flower3002Blowing = 451,
        /// Latheon 0494 flower actor 3003: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0494Flower3003Blowing = 452,
        /// Latheon 0494 flower actor 3004: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0494Flower3004Blowing = 453,
        /// Latheon 0494 flower actor 3005: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0494Flower3005Blowing = 454,
        /// Latheon 0494 flower actor 3006: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0494Flower3006Blowing = 455,
        /// Latheon 0494 flower actor 3007: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0494Flower3007Blowing = 456,
        /// Latheon 0494 bubble-flight segment 8, carrier actor 3408: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0494BubbleFlightSegment8Active = 457,
        /// Latheon 0494 bubble-flight segment 9, carrier actor 3409: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0494BubbleFlightSegment9Active = 458,
        /// Latheon 0494 bubble-flight segment 10, carrier actor 3410: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0494BubbleFlightSegment10Active = 459,
        /// Latheon 0498 flower actor 3000: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0498Flower3000Blowing = 460,
        /// Latheon 0498 flower actor 3001: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0498Flower3001Blowing = 461,
        /// Latheon 0498 flower actor 3002: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0498Flower3002Blowing = 462,
        /// Latheon 0498 flower actor 3003: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0498Flower3003Blowing = 463,
        /// Latheon 0495 flower actor 3000: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3000Blowing = 464,
        /// Latheon 0495 flower actor 3001: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it. Room 0496 also uses this bit for flower actor 3000 in its feeding callback; preserve that original reuse.
        LatheonGorge0495Flower3001Blowing = 465,
        /// Latheon 0495 flower actor 3002: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3002Blowing = 466,
        /// Latheon 0495 flower actor 3003: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3003Blowing = 467,
        /// Latheon 0495 flower actor 3004: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0495Flower3004Blowing = 468,
        /// Latheon 0495 flower actor 3005: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3005Blowing = 469,
        /// Latheon 0495 flower actor 3006: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0495Flower3006Blowing = 470,
        /// Latheon 0495 flower actor 3007: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0495Flower3007Blowing = 471,
        /// Latheon 0495 flower actor 3008: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3008Blowing = 472,
        /// Latheon 0495 flower actor 3009: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3009Blowing = 473,
        /// Latheon 0495 flower actor 3010: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0495Flower3010Blowing = 474,
        /// Latheon 0495 flower actor 3011: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3011Blowing = 475,
        /// Latheon 0495 flower actor 3012: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3012Blowing = 476,
        /// Latheon 0495 flower actor 3013: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0495Flower3013Blowing = 477,
        /// Latheon 0495 bubble-flight segment 14, carrier actor 3414: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective. Its retail condition reads flower flag 454 from another room; preserve that original cross-room dependency.
        LatheonGorge0495BubbleFlightSegment14Active = 478,
        /// Latheon 0495 bubble-flight segment 15, carrier actor 3415: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0495BubbleFlightSegment15Active = 479,
        /// Latheon 0495 bubble-flight segment 16, carrier actor 3416: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0495BubbleFlightSegment16Active = 480,
        /// Latheon 0497 flower actor 3000: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0497Flower3000Blowing = 481,
        /// Latheon 0497 flower actor 3001: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0497Flower3001Blowing = 482,
        /// Latheon 0497 flower actor 3002: Kirima feeding sets this wind/healthy state; permitted Amango feeding clears it.
        LatheonGorge0497Flower3002Blowing = 483,
        /// Latheon 0497 flower actor 3003: Kirima feeding sets this wind/healthy state; this callback refuses Amango instead of clearing it.
        LatheonGorge0497Flower3003Blowing = 484,
        /// Latheon 0497 bubble-flight segment 4, carrier actor 3404: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0497BubbleFlightSegment4Active = 485,
        /// Latheon 0497 bubble-flight segment 5, carrier actor 3405: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0497BubbleFlightSegment5Active = 486,
        /// Latheon 0497 bubble-flight segment 6, carrier actor 3406: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0497BubbleFlightSegment6Active = 487,
        /// Latheon 0497 bubble-flight segment 7, carrier actor 3407: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0497BubbleFlightSegment7Active = 488,
        /// Latheon 0497 bubble-flight segment 8, carrier actor 3408: set when that extra wind segment starts and cleared by the flight controller; this is transient route state, not a cleared dungeon objective.
        LatheonGorge0497BubbleFlightSegment8Active = 489,
        /// Island Ranch 0226: all 13 tiles in the trigger-500 grid must be active; the completion helper sets this bit, removes its tile triggers and opens the corresponding route. Entry restores the solved arrangement.
        IslandRanch0226TileGrid500Solved = 490,
        /// Island Ranch 0226: all 28 tiles in the trigger-400 grid must be active; the completion helper sets this bit, removes its tile triggers and opens the corresponding route. Entry restores the solved arrangement.
        IslandRanch0226TileGrid400Solved = 491,
        /// At orienteering checkpoint 14 the attendant gives a voucher using this flag rather than an inventory item; the Wonder Chef takes it and clears the bit before his quiz. Starting the related route also clears it.
        PalmacostaOrienteeringVoucherHeld = 492,
        /// Flanoir: either the merchant's initial explanation or his immediate request when the party already has Celsius' Tear sets this bit. Both accepting and declining that request count; it also gates Penguinist Gloves guidance elsewhere.
        CelsiusTearMerchantIntroductionSeen = 493,
        /// All three Ice 0324 routes use Celsius' Tear to freeze the lake and permit crossing; they set this flag, advance the local quest word to 2000 and remove blocker 5014. Entry and nearby enemy setup restore the frozen-lake state.
        TempleOfIceLakeFrozen = 494,
        /// Island Ranch 0230: the script displays Lock 1 disengaged, sets this bit and removes trigger 400; all three lock bits are required to open the main door.
        IslandRanch0230Lock1Disengaged = 495,
        /// Island Ranch 0230: the script displays Lock 2 disengaged, sets this bit and removes trigger 401; all three lock bits are required to open the main door.
        IslandRanch0230Lock2Disengaged = 496,
        /// Island Ranch 0230: the script displays Lock 3 disengaged, sets this bit and removes trigger 402; all three lock bits are required to open the main door.
        IslandRanch0230Lock3Disengaged = 497,
        /// Earth 0235: the earthquake sequence drops scenery actor 1300, then sets this bit and removes it and collision actor 1305; entry omits the obstacle.
        TempleOfEarth0235Obstacle1300Collapsed = 498,
        /// Earth 0235: the earthquake sequence changes the passage scenery and removes collision actor 1306; entry and the scenery-visibility helper restore the cleared state from this bit.
        TempleOfEarth0235Passage1306Cleared = 499,
        /// Earth 0235: the earthquake sequence animates obstacle actor 1301 to collapse, hides its blocking parts and sets this bit; entry seeks its animation to the final state.
        TempleOfEarth0235Obstacle1301Collapsed = 500,
        /// Room 0475: after the first ring-changer use, Lloyd and Raine discuss blue electricity and what it can destroy.
        TempleOfLightningBlueElectricityExplanationSeen = 502,
        /// Room 0475: after the switch shocks Lloyd, Raine explains that lightning strikes higher ground; requires Raine, Sheena and story 12101000.
        TempleOfLightningLightningRodExplanationSeen = 503,
        /// Room 0476: both the water-shock and rod-switch paths call the same Raine/Presea explanation about a higher lightning rod before setting this flag.
        TempleOfLightningElectrifiedWaterExplanationSeen = 504,
        /// Room 0478: the controlled character comments on the dark walkway, then the A Button hanging-on instruction is shown before this flag is set.
        TempleOfLightningDarkPathExplanationSeen = 505,
        /// Palmacosta: accepting an escort to either Hakonesia Peak or the House of Salvation sets this bit before travel; the selected destination plays the arrival event and clears it.
        PalmacostaEscortArrivalPending = 507,
        /// The food peddler's escort travel sets this bit in either direction between Luin and Hima; the destination plays its arrival scene and clears it. It is not a permanent completed-escort flag.
        LuinHimaEscortArrivalPending = 508,
        /// The Sylph-pact route sets this bit before returning through room 0508; entry creates trigger 4314, whose earthquake/mana-flow conversation clears it and removes that trigger.
        BalacrufEarthquakeConversationPending = 510,
        /// Meltokio 0401: after the receptionist invites Lloyd to meet a companion on the rear terrace and he selects whom he expects, this bit routes both terrace exits to room 0402.
        Meltokio0401TerraceInvitationReceived = 513,
        /// Luin 0447 region 1101, during rebuilding stages 1000..2999, plays scenery bank -1179648 on channel 0, waits 100 ticks, then sets this. Entry tests it to restore bank -1179645. Original scene.json and mesh part 0 identify these two animation banks with joints ATrap01_001..007; their GLB clips are local-motion-0 and local-motion-3. The name retains that original model-group identity without inventing a building or quest reward. Preserve the post-wait write.
        LuinATrapAnimationPlayed = 514,
        /// Luin 0447 region 1102, during rebuilding stages 1000..2999, sets this before spawning dust event 3000 and playing scenery bank -1179647 on channel 1. It plays sounds 171/172, later clears that channel and plays bank -1179646 on channel 2; entry restores bank -1179646 on channel 1. Original scene.json and mesh part 0 identify the two banks with Btrap_A00..04/Btrap_B01..06, using local-motion-1/2. This is set at sequence start, unlike ATrap bit 514, not proof that the animation finished.
        LuinBTrapAnimationStarted = 515,
        /// Ice 324 introduces the ring's freezing function before selecting SorcerersRingMode::Ice.
        IceRingIntroductionSeen = 516,
        /// Thoda 7 introduces the ring's long-distance water function (messages 61-63).
        WaterRingIntroductionSeen = 517,
        /// Balacruf 508 first wind-mode change plays the Lloyd/Genis introduction once (177-181).
        WindRingIntroductionSeen = 518,
        /// Gaoracchia 511 selects sunlight mode and runs sub_0F16 only on the first activation.
        SunlightRingIntroductionSeen = 519,
        SavePointTutorialSeen = 520,
        /// Sylvarant Base 270 sets this after either accepting or declining the memory-gem explanation; it does not prove the explanation was read.
        MemoryGemTutorialPromptDismissed = 521,
        /// Heimdall 0291: the frightened boy refuses evacuation without his sister; Lloyd hears his request before setting this flag.
        HeimdallMissingSisterRequestHeard = 522,
        /// Heimdall 0291: the sister says she cannot find her brother before he has been located; this changes her later dialogue but does not start the escort.
        HeimdallMissingBrotherRequestHeard = 523,
        /// Heimdall 0291: after the brother is found, his sister asks Lloyd to lead her out; she is respawned with following behavior and this flag enables their reunion.
        HeimdallSisterFollowing = 524,
        /// Heimdall before the Kratos duel: set when Lloyd answers Colette that he is ready for bed; distinct from her optional reassurance conversation.
        HeimdallColetteGoodNightAccepted = 525,
        /// Heimdall before the Kratos duel: Lloyd declines bed and Colette reassures him through her good luck and optimism; set after this optional conversation.
        HeimdallColetteReassuranceConversationSeen = 526,
        /// Heimdall before the Kratos duel, story 22402000: Genis shows Mithos' Cruxis Crystal and tells Lloyd not to do anything he will regret.
        HeimdallGenisMithosCrystalConversationSeen = 527,
        /// Heimdall before the Kratos duel: set after Lloyd asks Raine to leave the village and she reminds him Kratos is waiting; her optional decision conversation has a separate flag.
        HeimdallRaineLeaveVillageReminderSeen = 528,
        /// Heimdall before the Kratos duel: Raine discusses releasing Origin and preparing to kill Kratos; Lloyd says he will decide that night.
        HeimdallRaineKratosDecisionConversationSeen = 529,
        /// Heimdall before the Kratos duel: Sheena tries to cheer Lloyd up about a father and son fighting to the death, and Lloyd promises to think carefully.
        HeimdallSheenaKratosDuelConversationSeen = 530,
        /// Heimdall before the Kratos duel: Zelos criticizes Kratos and gives Lloyd his magic words, whatever will be, will be.
        HeimdallZelosMagicWordsConversationSeen = 531,
        /// Heimdall before the Kratos duel: Presea questions whether saving the world defies nature and encourages Lloyd to choose what he believes.
        HeimdallPreseaChooseYourPathConversationSeen = 532,
        /// Heimdall before the Kratos duel: Regal discusses choosing between what Lloyd desires and what he must do.
        HeimdallRegalDesireOrDutyConversationSeen = 533,
        /// Hima: Lloyd notices a commotion in front of the inn while seeking the Tower Key. This enables the follow-up scene; it does not grant the key.
        HimaInnCommotionNoticed = 534,
        /// Zelos' house 0399: Genis stammers that Presea is pretty and she returns the compliment; later interactions use brief embarrassed reactions.
        MeltokioGenisPreseaComplimentSeen = 535,
        /// Derris Kharlan 530 has eight companion variants, each setting this bit after the selected companion rejoins; it is not Colette-specific.
        DerrisKharlanCompanionRescued = 536,
        /// Tower of Salvation 502 has eight companion-rescue variants sharing this bit; the chosen companion describes the crystal's influence ending.
        TowerOfSalvationCompanionRescued = 537,
        /// Meltokio 392 declares the party winners over warriors from another world (935-937), then records Garr, Farah and Meredy monster knowledge.
        ColiseumCameoTeamDefeated = 539,
        /// Altamira hotel: the player pays for a room and chooses to go out at night; the next hotel-entry greeting asks about last night and clears this transient flag. Room 0533 also clears it.
        AltamiraNightOutReturnPending = 540,
        /// Base 0253: the raising trigger and powered lift control set this bit as the lift goes to its upper level; the lowering trigger clears it. The upper control also reports a burnt smell.
        TetheAllaBase0253LiftRaised = 542,
        /// Base 0253: block actor 330 reaches (75,1275,900), the placement sequence sets this flag and entry restores the placed arrangement.
        TetheAllaBase0253Block330Placed = 543,
        /// Base 0254: the electric-orb ring activates target actor 45; its animation and lift controls read this bit. The original all-on/all-off debug menu also changes it.
        TetheAllaBase0254OrbTarget45Activated = 544,
        /// Derris-Kharlan 0134: defeating encounter actor 8105 reveals chest actor 1000 and sets this bit. Opening it separately grants the Past Stone; this flag is not item ownership.
        DerrisKharlanPastStoneChestRevealed = 545,
        /// Derris-Kharlan 0132: defeating encounter actor 8001 reveals chest actor 1000 and sets this bit. Opening it separately grants the Future Stone; this flag is not item ownership.
        DerrisKharlanFutureStoneChestRevealed = 546,
        /// Triet 486 explains the old city's destruction by Efreet's hellfire (185-203).
        TrietEfreetHistoryHeard = 547,
        /// Derris Kharlan 130 Past Stone door scene ends by reading the anger, fire and lightning inscription (24-33).
        DerrisKharlanPastDoorInscriptionRead = 548,
        /// Derris Kharlan 130 Future Stone door scene ends by reading the earth, wind, water and light inscription (34-43).
        DerrisKharlanFutureDoorInscriptionRead = 549,
        /// Tower of Mana 361 suggests looking for Professor Nova near his hometown Asgard (205-211).
        NovaSearchSuggested = 550,
        /// Returning to Mizuho for the promised Kuchinawa duel without Zircon prompts Sheena's rebuke. The first occurrence applies -180 affinity and sets this bit; later returns do not repeat that penalty.
        MizuhoMissingZirconAffinityPenaltyApplied = 551,
        /// Derris-Kharlan 0130: interaction with blocker 1200 while holding the Vinheim Key removes the blocker and opens the double door; entry restores that unlocked state.
        DerrisKharlanVinheimDoorUnlocked = 552,
        /// Tower of Mana 362 grants Boltzman's Book on three routes, but sets this only while ColetteAngelStage is FourthSealNightComplete (301).
        BoltzmansBookObtainedDuringAngelCrisis = 553,
        /// Lake Umacy 359 sets this alongside UnicornHornObtained only while ColetteAngelStage is FourthSealNightComplete (301).
        UnicornHornObtainedDuringAngelCrisis = 554,
        /// Dirk's House 374 introduces Dirk's customization service; 316-318 ask what to customize and close the interaction.
        DirkCustomizationIntroduced = 555,
        /// Luin 448 early story branch sets this immediately before adding Sheena to the party; the late-route conversation uses a separate flag.
        SheenaJoinedOnEarlyLuinRoute = 556,
        /// At stories 22802000/22904000, the selected companion guides Lloyd through Mithos' memory in field 0238. Return entry in Derris-Kharlan 0530 or Salvation 0502 consumes and clears this bit for the aftermath dialogue.
        MithosMemoryReturnPending = 557,
        /// Altessa's House 139 offers customization as repayment (467-469).
        AltessaCustomizationIntroduced = 558,
        /// House of Salvation 81 unlocks Iselia-region long-range mode (28); the optional explanations may be declined before this flag is set.
        IseliaLongRangeModeIntroduced = 559,
        /// Both Toize 0413 variants set this when Lloyd notices the large blocking boulder: the early route explains the mine's special item, while the later route recalls blowing up the previous boulder.
        ToizeMineBlockingBoulderNoticed = 560,
        /// Entry into bridge room 0076 records this bit separately from its location-caption visited flag; both bridge variants consult it before their crossing event outside story 10409000.
        TetheAllaBridge0076Entered = 561,
        /// All eight Flanoir-companion branches set this after the reunion/rescue conversation on returning from Mithos' memory at story 22802000; entry suppresses its memory trigger once this conversation has run.
        DerrisKharlanCompanionRescueConversationSeen = 562,
        /// All eight companion memory-entry helpers in Derris-Kharlan 0530 set this before changing to field 0238. That field reads it to return to 0530 instead of the default Salvation 0502 route.
        MithosMemoryDerrisKharlanReturnSelected = 563,
        /// Bridge 0076: Lloyd comments that the raised bridge cannot be crossed, then this bit prevents repetition on entry.
        TetheAllaRaisedBridgeCommentSeen = 565,
        /// Ymir 0520: after the child-rescue request, the party notices the unreachable Ymir Fruit, dangerous fish and the possibility of asking the little fish to help; this records the hint.
        YmirFruitFishHelpHintSeen = 566,
        /// Ymir 0521: the one-time fish observation has Lloyd ask whether it is guarding its territory; this removes trigger 5200 and records that introduction.
        YmirTerritorialFishNoticed = 567,
        /// Balacruf 0508: wind-ring target 3006 toggles its lighting/ambient emitter 5706 and this bit; both target states together open the entrance gate. Entry restores the active emitters, while the opposite wind interaction clears them.
        BalacrufMausoleum0508WindTarget3006Active = 568,
        /// Balacruf 0508: wind-ring target 3007 toggles its lighting/ambient emitter 5707 and this bit; both target states together open the entrance gate. Entry restores the active emitters, while the opposite wind interaction clears them.
        BalacrufMausoleum0508WindTarget3007Active = 569,
        /// Heimdall: before entering Torent Forest, the elder explains Kratos' search for Aionis and tells Lloyd not to forget his ninth companion, then removes the conversation trigger.
        HeimdallElderAionisConversationSeen = 570,
        /// Salvation 0171: the first floor-tilting sequence sets this flag and explains that the floor needs a counterweight. Later tilts skip this introduction; this is not the balanced-floor state.
        TowerOfSalvationTiltingFloorIntroduced = 571,
        /// Fooji 0418: Lloyd admits he does not know where to go; Sheena explains Meltokio is the Imperial City north of the mountain and the trigger is removed.
        FoojiMeltokioDirectionsHeard = 572,
        /// Room 0475 rod actor 4111: set at bone Z offset 0 and cleared at -230; restored on entry and used to choose the lightning target.
        TempleOfLightning0475Rod4111Raised = 573,
        /// Room 0475 rod actor 4112: set at bone Z offset 0 and cleared at -230; restored on entry and used to choose the lightning target. The debug menu can also force this state.
        TempleOfLightning0475Rod4112Raised = 574,
        /// Room 0475 rod actor 4113: set at bone Z offset 0 and cleared at -230; restored on entry and used to choose the lightning target.
        TempleOfLightning0475Rod4113Raised = 575,
        /// Room 0475: lightning from raised rod 4111 launches and destroys actor 4220, removes its collision actor 523, then sets this persistent obstacle flag.
        TempleOfLightning0475Obstacle4220Destroyed = 576,
        /// Room 0475: matching ring electricity destroys stack block 305; the remaining actors are lowered and entry restores their positions from these flags.
        TempleOfLightning0475StackBlock305Destroyed = 577,
        /// Room 0475: matching ring electricity destroys stack block 315; the remaining actors are lowered and entry restores their positions from these flags.
        TempleOfLightning0475StackBlock315Destroyed = 578,
        /// Room 0475: matching ring electricity destroys stack block 323; the remaining actors are lowered and entry restores their positions from these flags.
        TempleOfLightning0475StackBlock323Destroyed = 579,
        /// Room 0475: destroying the last stack block 323 lowers rod 4114 and creates confirmed trigger 4400; entry restores that trigger while the Volt pact is incomplete.
        TempleOfLightning0475VoltApproachEnabled = 580,
        /// Room 0476 rod 102: its switch sets this at bone Z offset 0 and clears it at -230; the raised rod intercepts lightning before rod 106 or 101.
        TempleOfLightning0476Rod102Raised = 581,
        /// Room 0476: lightning activates the apparatus through rod 106. Room 0477 uses the same flag to run the connected large machine and its ambient sound.
        TempleOfLightningMachineryPowered = 582,
        /// Room 0476 rod 106: its switch sets this at bone Z offset 0 and clears it at -230, after the rod has landed; raising it with rod 102 down can activate the apparatus.
        TempleOfLightning0476Rod106Raised = 583,
        /// Room 0477: the machine sequence strikes and animates obstacle actor 700, then despawns it and collision actor 711; entry omits both when set.
        TempleOfLightning0477Obstacle700Removed = 584,
        /// Room 0477: Lloyd, Raine and Genis discuss the newly powered large machine before this flag is set; later interactions proceed directly to its controls.
        TempleOfLightningPoweredMachineConversationSeen = 585,
        /// Room 0476 rod 101: its switch sets this at bone Z offset 0 and clears it at -230; when rods 102 and 106 are down, lightning can electrify the water.
        TempleOfLightning0476Rod101Raised = 586,
        /// Room 0476: destroying support actor 311 with matching electricity moves rod 106 to (1981,2188,450); this flag restores its landed position and enables its switch.
        TempleOfLightning0476Rod106Landed = 587,
        /// Triet Ruins 220 stone block 5000 placed at z=-150; entry restores the socket at x=-1275, y=75. The two sockets have separate bits; retain the original x-test/else behavior.
        TrietRuins0220Block5000AtSocketXMinus1275 = 588,
        /// Triet Ruins 220 stone block 5000 placed at z=-150; entry restores the socket at x=-1125, y=75. The two sockets have separate bits; retain the original x-test/else behavior.
        TrietRuins0220Block5000AtSocketXMinus1125 = 589,
        /// Triet Ruins 220 stone block 5001 placed at z=-150; entry restores the socket at x=-1125, y=75. The two sockets have separate bits; retain the original x-test/else behavior.
        TrietRuins0220Block5001AtSocketXMinus1125 = 590,
        /// Nova's Caravan 239/243: Nova records the requirements to meet Aska.
        NovaAskaRequirementsRecorded = 591,
        /// Welgaia 0501: either of two speakers starts the shared discussion about reversing mana flow, sustaining the Great Seed and Yggdrasill's planned Age of Half-elves; this bit prevents repeating it.
        WelgaiaManaManagementConversationHeard = 592,
        /// Welgaia data terminal: selecting World Prolongation Project displays the account of splitting the world and limiting prosperity, then sets this bit. Reading Derris-Kharlan alone does not set it.
        WelgaiaWorldProlongationDataRead = 593,
        /// Welgaia data terminal: selecting Eternal Sword displays its pact with Origin and power over time and space, then sets this bit independently of the World Prolongation entry.
        WelgaiaEternalSwordDataRead = 594,
        /// After both required terminal entries have been read, Lloyd calls the information mind-numbing and Raine urges the party to leave; the emergency-route variant shares this conclusion flag.
        WelgaiaDataBankConclusionSeen = 595,
        /// Salvation 0147 entry and its inner-door transition set this bit and clear flag 597; interior room 0149 uses it to choose the matching camera arrangement. This is separate from the location-visited bit.
        TowerOfSalvationEnteredVia0147 = 596,
        /// Salvation 0148 entry and its inner-door transition set this bit and clear flag 596; interior room 0149 reads it when choosing the exit back to 0148.
        TowerOfSalvationEnteredVia0148 = 597,
        /// Luin's pastor says a monster has the Tower Key. With the Light Seal/Desian Orb route active, this bit records the hint and may start timer 47; Hima uses it when updating that route timer.
        LuinTowerKeyMonsterHintRecorded = 598,
        /// Balacruf 0508 gate-opening helper sets this bit only to play sound 319 once; the two wind-target flags determine whether the gate is currently open.
        BalacrufMausoleum0508GateOpeningChimePlayed = 599,
        /// At story 11301000, entering room 0327 through the front sets this bit; its guard calls the party scoundrels and drives them out, clearing it and requesting Orochi's hint with flag 601.
        MizuhoGuardRejectionPending = 600,
        /// After the guard rejects front-door entry, this bit causes Orochi to explain the hidden door in the wall on his right on return to room 0326; that hint clears the bit.
        MizuhoOrochiHiddenDoorHintPending = 601,
        /// Room 0476 interaction key 400: set on use and consulted by both ring changers to unlock later electricity variants; this records use, not the current ring mode.
        TempleOfLightning0476RingChanger400Used = 602,
        /// Room 0476 interaction key 410: set on use and consulted by both ring changers to unlock later electricity variants; this records use, not the current ring mode.
        TempleOfLightning0476RingChanger410Used = 603,
        /// Luin party-discussion and resident-dialogue routes all direct the party to the escaped ranch prisoner Pietro in Hima and set this bit; it also enables the peddler escort route.
        PietrosHimaLocationLearned = 604,
        /// Meltokio 0376: the captain refuses the wanted party entry, apologizes to Zelos and Presea, and urges them to hide after monsters appear in Tethe'alla.
        MeltokioWantedPartyGateWarningSeen = 605,
        /// Latheon 0495: Zelos complains about the climb, Presea criticizes him, and Colette encourages everyone to keep going before this flag is set.
        LatheonAscentEncouragementConversationSeen = 606,
        /// Latheon 0495: the party discusses a cave hidden beyond the still-flowing waterfall; this flag selects a different follow-up after the waterfall is split.
        LatheonHiddenCaveDiscussionSeen = 607,
        /// All six setting paths grant 5,000 Gald after accepting the Celsius' Tear merchant's request, then remove the merchant. The retail script does not decrement the tear item here.
        CelsiusTearMerchantRewardReceived = 608,
        /// Toize 0413: Regal identifies the spinning Naploosa Bacura, explains that ordinary attacks cannot harm it and suggests a powerful impact; the first explanation sets this bit.
        ToizeMineBacuraExplained = 609,
        /// Dorr's mansion: the party finds the room empty, Colette hears voices below and Kratos directs them to the basement; entry suppresses repeating this conversation with this bit.
        DorrMansionBasementVoicesHeard = 610,
        /// Palmacosta: Kilia nearly says she must report to Pro..., then requests another display of the Chosen's angel wings. With or without Dorr's interruption, the first conversation sets this bit.
        KiliaAngelWingsRequestSeen = 611,
        /// Toize 0413: the player-collision branch sets this before the boulder knockdown; it suppresses the Bacura collision branch for that run. A new run and the ladder/return helper clear it.
        ToizeMineBoulderHitPlayerThisRun = 612,
        /// The Gnome-pact route sets this after the world-separation discussion; Earth 0235 creates earthquake conversation trigger 1400 while the quest word is 50000. The trigger progresses the route rather than clearing this persistent request bit.
        TempleOfEarthExitEarthquakeRequested = 613,
        /// Gnome battle completion sets this before reloading Earth 0237 on both the lost-battle and won-battle paths. Entry hides the arena node, initializes local state and clears the bit; it is not pact completion.
        TempleOfEarthArenaReloadPending = 614,
        /// All four Luin variants set this after Pietro's final thanks at rebuilding step 600000: he credits the residents and intends to help other Desian victims. Later scenes omit him and refer to his journey.
        LuinRebuildingCompletionThanksHeard = 615,
        /// Darkness 0126: moving the linked block to Y -1245 starts event 4011 and removes collision actor 3001; entry restores the corresponding scenery state.
        TempleOfDarkness0126Block3001Removed = 616,
        /// Darkness 0126: when block 3000 reaches (-975,-1475), a one-time sequence repositions small Shadow 2304 beyond it and restores the block; entry uses this released arrangement.
        TempleOfDarknessShadow2304ReleasedFromBlock = 617,
        /// Darkness 0126: when small Shadow 2302 reaches Y >= -789, it is moved to Y -496 and resumes following; this bit suppresses repeating that move and selects its restored entry position.
        TempleOfDarknessShadow2302MoveSequenceSeen = 618,
        /// Darkness 0127: all five small Shadows 2101..2105 passing Z -3765 records this descent checkpoint. Entry restores them near Z -4942; reaching the final seal level is a later quest-word transition.
        TempleOfDarkness0127FiveShadowsDescended = 619,
        /// Darkness 0126: linked blocks 3101/3102 reaching their fully shifted coordinates sets this persistent latch and suppresses earlier interaction triggers; scratch flags 160/161 still describe their current position.
        TempleOfDarknessLinkedBlocksFullyShiftedOnce = 620,
        /// Darkness 0126: crossing lower-block trigger 5004 removes it and sets this bit; the block controller then stops recreating triggers 5002/5004 for this passage.
        TempleOfDarkness0126LowerBlockTrigger5004Crossed = 621,
        /// Darkness 0126: controller event_2_0FAA_r37 loops index 0..4 and sets flag 623+index for Shadow 2301+index reaching X <= -684 and Z <= -1800. Helper sub_28DF moves it to the gathering point; entry restores Shadow 2301 from flag 623, and all five bits advance the gathering stage to 32500.
        TempleOfDarknessShadow2301Gathered = 623,
        /// Darkness 0126: controller event_2_0FAA_r37 loops index 0..4 and sets flag 623+index for Shadow 2301+index reaching X <= -684 and Z <= -1800. Helper sub_28DF moves it to the gathering point; entry restores Shadow 2302 from flag 624, and all five bits advance the gathering stage to 32500.
        TempleOfDarknessShadow2302Gathered = 624,
        /// Darkness 0126: controller event_2_0FAA_r37 loops index 0..4 and sets flag 623+index for Shadow 2301+index reaching X <= -684 and Z <= -1800. Helper sub_28DF moves it to the gathering point; entry restores Shadow 2303 from flag 625, and all five bits advance the gathering stage to 32500.
        TempleOfDarknessShadow2303Gathered = 625,
        /// Darkness 0126: controller event_2_0FAA_r37 loops index 0..4 and sets flag 623+index for Shadow 2301+index reaching X <= -684 and Z <= -1800. Helper sub_28DF moves it to the gathering point; entry restores Shadow 2304 from flag 626, and all five bits advance the gathering stage to 32500.
        TempleOfDarknessShadow2304Gathered = 626,
        /// Darkness 0126: controller event_2_0FAA_r37 loops index 0..4 and sets flag 623+index for Shadow 2301+index reaching X <= -684 and Z <= -1800. Helper sub_28DF moves it to the gathering point; entry restores Shadow 2305 from flag 627, and all five bits advance the gathering stage to 32500.
        TempleOfDarknessShadow2305Gathered = 627,
        /// Dirk's house: Kratos thanks Dirk for raising Lloyd and changing his way of life; Dirk replies that he is Lloyd's father too. Either interaction can invoke this conversation.
        DirkKratosFatherhoodConversationSeen = 628,
        /// Base 0278 early Lloyd route: Lloyd wonders what the machine does, uses its shop menu, then recognizes that it sells items before setting this one-time introduction flag.
        SylvarantBaseVendingMachineIntroduced = 629,
        /// Triet room 0488 entry at story 1207000 applies its audio fade only once and records this bit; it does not mark an additional dialogue or reward.
        TrietInnMorningAudioFadeApplied = 630,
        /// After the Luin rescue, Sheena promises to repay her debt and leaves despite Colette's concern. This bit also suppresses a later solitary Sheena prayer scene if she has not joined.
        LuinSheenaRescueFarewellSeen = 631,
        /// After the attack on Altessa at story 21904000, room 0143 saves the eight formation slots and leader, snapshots the party and removes Raine while she treats the wounded. This bit prevents repeating that preparation on reentry.
        AltessaRecoveryPartyPrepared = 632,
        /// Luin variants: donation progress reaches the current rebuilding goal.
        LuinDonationGoalReached = 633,
        /// Koton's late Book of Regeneration routes share this bit: Lloyd can say the book is no longer needed, or Raine can read the seal locations after all seals are released. It does not prove the text was read on both routes.
        KotonLateBookConversationHandled = 634,
        /// Asgard: the mayor refuses Raine permission to study the ruins, blames a local scholar for harming tourism and directs her to Linar.
        AsgardRuinsResearchPermissionRefused = 635,
        /// The early-ranch route sets this before the wounded Sheena explains Luin's destruction in room 0448; room 0447 consults it to avoid repeating the first aftermath introduction.
        LuinEarlyAftermathIntroductionSeen = 636,
        /// Base 0270: Raine or Genis tells Lloyd the apparatus can be controlled with that machine; Lloyd reacts and the party asks if he said something. Both companion variants share this bit.
        SylvarantBaseMachineControlConversationSeen = 637,
        /// Derris-Kharlan 0135: block 3000 reaches its designated slot and sinks to Z -150; entry restores the placed state and room 0130 omits the corresponding movable piece.
        DerrisKharlan0135Block3000Placed = 638,
        /// Derris-Kharlan 0135: block 3001 reaches its designated slot and sinks to Z -150; entry restores the placed state and room 0130 omits the corresponding movable piece.
        DerrisKharlan0135Block3001Placed = 639,
        /// Derris-Kharlan 0135: block 3002 reaches its designated slot and sinks to Z -150; entry restores the placed state and room 0130 omits the corresponding movable piece.
        DerrisKharlan0135Block3002Placed = 640,
        /// Derris-Kharlan 0135: block 3003 reaches its designated slot and sinks to Z -150; entry restores the placed state and room 0130 omits the corresponding movable piece.
        DerrisKharlan0135Block3003Placed = 641,
        /// Derris-Kharlan 0135: block 3004 reaches its designated slot and sinks to Z -150; entry restores the placed state and room 0130 omits the corresponding movable piece.
        DerrisKharlan0135Block3004Placed = 642,
        /// Derris-Kharlan 0135: block 3005 reaches its designated slot and sinks to Z -150; entry restores the placed state and room 0130 omits the corresponding movable piece.
        DerrisKharlan0135Block3005Placed = 643,
        /// Derris-Kharlan 0135: block 3006 reaches its designated slot and sinks to Z -32; entry restores the placed state and room 0130 omits the corresponding movable piece. The two final blocks 3006/3007 together enable the passage; preserve the retail entry actor IDs.
        DerrisKharlan0135Block3006Placed = 644,
        /// Derris-Kharlan 0135: block 3007 reaches its designated slot and sinks to Z -32; entry restores the placed state and room 0130 omits the corresponding movable piece. The two final blocks 3006/3007 together enable the passage; preserve the retail entry actor IDs.
        DerrisKharlan0135Block3007Placed = 645,
        /// Dorr's mansion helper sets this as he greets the Chosen and thanks her for saving people from the Desians; Kilia's dialogue uses it to distinguish whether Dorr participates.
        DorrRanchRescueThanksIntroduced = 646,
        /// Palmacosta 0056: after Dorr reassures a child, residents explain his resistance to Desians and raising Kilia alone; Lloyd and Colette praise the city and this bit prevents repeating the introduction.
        DorrPublicReputationIntroduced = 647,
        /// Meltokio 398 sub_0CE4 has Janet sing the childhood song Presea taught her and explain its significance; leaving immediately does not set this.
        JanetChildhoodSongHeard = 648,
        /// Balacruf 0507: the pastor hands over ItemId::TowerKey for the Boltzman route, advances its quest counter and sets this bit; later interactions use his farewell.
        TowerKeyReceivedFromBalacrufPastor = 650,
        /// After the relevant Sylph-pact route, this flag selects earthquake reactions from people outside Balacruf 0507; the exit event there clears it. It is a temporary reaction state, not a conversation-complete flag.
        BalacrufEarthquakeReactionsPending = 651,
        /// Ymir 0519: the blue-seed planting helper sets this bit, consumes Blue Seed and starts growth at quest step 1200. The fish-route helpers across rooms 0519..0521 read this planted state.
        YmirBlueSeedPlanted = 653,
        /// Ymir 0519: the white-seed planting helper sets this marker, consumes White Seed and starts its growth sequence at quest step 1500; this does not represent seed inventory.
        YmirWhiteSeedPlanted = 654,
        /// Hima 0301: Lloyd chooses Genis as his dragon passenger; Salvation 0147 reads the same flag to stage that companion at arrival.
        HimaDragonCompanionGenisChosen = 655,
        /// Hima 0301: Lloyd chooses Raine as his dragon passenger; Salvation 0147 reads the same flag to stage that companion at arrival.
        HimaDragonCompanionRaineChosen = 656,
        /// Hima 0301: Lloyd chooses Sheena as his dragon passenger; Salvation 0147 reads the same flag to stage that companion at arrival.
        HimaDragonCompanionSheenaChosen = 657,
        /// Hima 0300: Lloyd tells Raine he is ready for the Tower of Salvation and the scene changes to the dragon departure area; declining does not set this flag.
        HimaTowerDepartureConfirmed = 658,
        /// At story 2302000, Lloyd notices the bodies and Raine suggests the coffins contain Chosen who failed regeneration. The scene removes its trigger and records this bit.
        TowerOfSalvationFailedChosenCoffinsExplained = 659,
        /// The unregistered word-chain event in Salvation 0096 reaches Wonder Chef and grants Spirit Bottle on first completion; this bit prevents a second reward. The already-rewarded branch also writes the same bit.
        SalvationWordChainRewardReceived = 660,
        /// The Earth ring station sets this for the first changed-function scene: the party is startled by the quake and Raine explains it could reshape the dungeon.
        EarthquakeRingIntroduced = 661,
        /// Iselia Ranch 0194: the first Mana ring station use prompts Lloyd's warm-light observation and Kratos' explanation that releasing too much bodily mana causes exhaustion.
        ManaRingIntroduced = 662,
        /// Exire 0185: after a resident recognizes Virginia's name and discourages a reunion, Raine asks Lloyd to find her; this enables the search scene with the elder.
        ExireVirginiaSearchRequested = 663,
        /// Exire 0188: with Virginia's diary already obtained, Raine learns Virginia fled; Lloyd and Genis reassure her that they will meet again.
        ExireVirginiaDepartureAfterDiarySeen = 664,
        /// Exire 0188: the late-story route with the search requested but Virginia's house not located reveals Virginia has fled; Raine says she abandoned them again. Distinct from the post-diary version in flag 664.
        ExireVirginiaDepartureBeforeReunionSeen = 665,
        /// Room 0475: matching ring electricity destroys block actor 303 and removes collision actor 503; entry omits that block when set.
        TempleOfLightning0475Block303Destroyed = 667,
        /// Room 0475: matching ring electricity destroys block actor 313 and removes collision actor 513; entry omits that block when set.
        TempleOfLightning0475Block313Destroyed = 668,
        /// Room 0475: matching ring electricity destroys block actor 314 and removes collision actor 514; entry omits that block when set.
        TempleOfLightning0475Block314Destroyed = 669,
        /// Pastor Marche's late greeting asks Colette about regeneration and wishes her luck; her hesitant response and this bit suppress that greeting on subsequent interactions.
        PastorMarcheLateGreetingSeen = 670,
        /// Tower of Salvation 150 sets this in Zelos's death scene after his final request to destroy his Cruxis Crystal (213-238); Seles's revenge variant consults it.
        ZelosDied = 671,
        /// The Asgard and Palmacosta ranch routes share Raine's explanation that a refresher is a magitechnology healing machine. Either route sets this one-time introduction bit; only the Palmacosta variant includes immediate use.
        RefresherHealingMachineExplained = 672,
        /// After solo advanced coliseum completion, Seles and Tokunaga hurry to Zelos' match, notice him and depart. This introduction bit enables the later Seles challenge branch at the coliseum.
        MeltokioSelesMatchArrivalSeen = 673,
        /// Flanoir: only declining the merchant's initial automatic request sets this bit; later direct interactions offer another chance to accept.
        CelsiusTearMerchantInitialOfferDeclined = 674,
        /// Altamira 0012: the downward lift event sets this at the lower landing and the upward event clears it. Boarding the casino/theater transport also sets this same bit; entry restores the lift from it.
        Altamira0012LiftLowered = 675,
        /// Both Flanoir 0312 trigger variants show Zelos wondering about the Aionis he ingested and the ring of the pact; Lloyd interrupts him and the triggers are removed.
        FlanoirZelosAionisSoliloquySeen = 676,
        /// Pietro's healing scene sets this alongside the separate PietroHealed bit. It keeps the recovery aftermath arrangement in Hima until the Luin entry that stages Pietro clears it.
        HimaPietroRecoveryAftermathActive = 677,
        /// On leaving Ice 0323 at the OzetteAttack story stage, this bit is set before world cinematic 521. Returning through entry uses it to show the party reacting to the lightning in Ozette's direction.
        TempleOfIceOzetteAttackFollowupRequested = 678,
        /// Room 0475 rod 4112 switch: set on either raising or lowering the rod, independently of its current raised state in flag 574.
        TempleOfLightning0475Rod4112SwitchUsed = 679,
        /// Triet: Genis reminds Lloyd to look for Raine and the others as the party tries to leave before the ruins oracle is consulted; this route records the reminder separately from flag 681.
        TrietCompanionSearchReminderSeen = 680,
        /// Triet: Genis reminds Lloyd to inspect the wanted poster when the party tries to leave; this route records that reminder separately from the companion-search reminder.
        TrietWantedPosterReminderSeen = 681,
        /// The Lloyd Irving statue interaction sets this after either its inscription or the optional Kratos/Anna birthplace conversation; examining it does not guarantee the inscription was displayed.
        LuinLloydStatueExamined = 682,
        /// Luin 0457: examining the statue displays Raine Sage's inscription about supporting the hearts of the people, then records this bit.
        LuinRaineStatueExamined = 683,
        /// Luin 0455: examining the statue displays Sheena Fujibayashi's inscription about saving people from Desians, then records this bit.
        LuinSheenaStatueExamined = 684,
        /// Both Luin variants share this one-time post-infiltration discussion: Kvar will increase security, so Raine suggests returning to Hima to consult Pietro.
        LuinReturnToHimaDecisionSeen = 685,
        /// Niflheim 524 Living Armor victory checks this one-shot flag before unconditionally granting ItemId::PersianBoots (264).
        NiflheimPersianBootsReceived = 686,
        /// The Efreet-pact sequence sets this if the Celsius pact already exists; Triet Ruins 0218 plays the exit earthquake/mana-flow discussion, clears the bit and advances Fire Seal progress.
        TrietRuinsExitEarthquakePending = 687,
        /// The Celsius-pact sequence sets this if the Efreet pact already exists; Ice 0323 plays the exit earthquake/mana-flow discussion and clears the bit.
        TempleOfIceExitEarthquakePending = 688,
        /// Linkite Tree 0523: Lloyd examines the tree and says it is dead. Nova's caravan dialogue uses this bit to distinguish having already seen the dead tree from still needing to find it.
        DeadLinkiteTreeExamined = 689,
        /// Room 0476: set immediately before the EXGemLv2 chest falls after its supporting block is destroyed; entry restores the interactive chest at its lower position. This is separate from the opened-treasure flag.
        TempleOfLightning0476EXGemLv2ChestLoweringStarted = 710,
        /// Room 0476: set immediately before the ThunderScepter chest falls after its supporting block is destroyed; entry restores the interactive chest at its lower position. This is separate from the opened-treasure flag.
        TempleOfLightning0476ThunderScepterChestLoweringStarted = 711,
        /// Shared retail flag: room 0476 sets this before lowering the Duel Helm chest, and room 0477 also sets it before lowering the Battle Cloak chest. Only room 0476 reads it; preserve this cross-room write.
        TempleOfLightningDuelHelmOrBattleCloakChestLoweringStarted = 712,
        /// Room 0476: set immediately before the EXGemLv3 chest falls after its supporting block is destroyed; entry restores the interactive chest at its lower position. This is separate from the opened-treasure flag.
        TempleOfLightning0476EXGemLv3ChestLoweringStarted = 713,
        /// Room 0477: set after the Battle Cloak chest lands and becomes interactive; entry restores it at the lower position. Its lowering sequence also writes shared flag 712.
        TempleOfLightning0477BattleCloakChestLanded = 714,
        /// triet-ruins-0220: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        TrietRuins0220MemoryGemObtained = 800,
        /// thoda-geyser-0008: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        ThodaGeyser0008MemoryGemObtained = 801,
        /// balacruf-mausoleum-0508: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        BalacrufMausoleum0508MemoryGemObtained = 802,
        /// tower-of-mana-0364: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        TowerOfMana0364MemoryGemObtained = 803,
        /// sylvarant-base-0269: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        SylvarantBase0269MemoryGemObtained = 804,
        /// palmacosta-ranch-0203: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        PalmacostaRanch0203MemoryGemObtained = 805,
        /// asgard-ranch-0213: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        AsgardRanch0213MemoryGemObtained = 806,
        /// meltokio-0466: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        Meltokio0466MemoryGemObtained = 807,
        /// gaoracchia-forest-0515: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        GaoracchiaForest0515MemoryGemObtained = 808,
        /// tethealla-base-0253: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        TetheAllaBase0253MemoryGemObtained = 809,
        /// toize-valley-mine-0412: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        ToizeValleyMine0412MemoryGemObtained = 810,
        /// temple-of-lightning-0477: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        TempleOfLightning0477MemoryGemObtained = 811,
        /// temple-of-darkness-0126: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        TempleOfDarkness0126MemoryGemObtained = 813,
        /// latheon-gorge-0496: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        LatheonGorge0496MemoryGemObtained = 816,
        /// heimdall-0481: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        Heimdall0481MemoryGemObtained = 819,
        /// iselia-human-ranch-0194: one-shot enemy Memory Gem reward sets this only after the item fits in inventory and is granted.
        IseliaHumanRanch0194MemoryGemObtained = 822,
        TrietRuins0220MemoryCircleUnlocked = 850,
        ThodaGeyser0009MemoryCircleUnlocked = 851,
        /// Memory Circle unlock bits supplied to create_sealed_save_point.
        BalacrufMausoleum0508MemoryCircleUnlocked = 852,
        TowerOfMana0366MemoryCircleUnlocked = 853,
        SylvarantBase0270MemoryCircleUnlocked = 854,
        PalmacostaRanch0206MemoryCircleUnlocked = 855,
        AsgardRanch0214MemoryCircleUnlocked = 856,
        Meltokio0467MemoryCircleUnlocked = 857,
        GaoracchiaForest0516MemoryCircleUnlocked = 858,
        TetheallaBase0254MemoryCircleUnlocked = 859,
        ToizeValleyMine0413MemoryCircleUnlocked = 860,
        TempleOfLightning0476MemoryCircleUnlocked = 861,
        TempleOfDarkness0127MemoryCircleUnlocked = 863,
        LatheonGorge0495MemoryCircleUnlocked = 866,
        Heimdall0482MemoryCircleUnlocked = 869,
        IslandRanch0230MemoryCircleUnlocked = 871,
        IseliaHumanRanch0196MemoryCircleUnlocked = 872,
        /// Salvation 0081 explicitly unlocks Long-range Mode in the Iselia region and sets this bit. The map-making services can also grant it when filling in the Iselia/Triet map group.
        LongRangeIseliaRegionUnlocked = 900,
        LongRangeTrietDesertFlag1 = 901,
        LongRangeOssaMountainsAndIzooldFlag1 = 902,
        LongRangeNEPalmacostaAndHakonesiaPeakFlag1 = 903,
        LongRangeEPalmacostaAndThodaFlag1 = 904,
        LongRangeEPalmacostaAndThodaFlag2 = 905,
        LongRangeELuinAndAsgardFlag1 = 906,
        LongRangeELuinAndAsgardFlag2 = 907,
        LongRangeWLuinAndHimaFlag1 = 908,
        LongRangeKatzIslandFlag1 = 909,
        /// The native world travel-permission grid uses region code 10 in 38
        /// Sylvarant cells. REL fn_2_1AB88 queries world*11 + region through DOL
        /// fn_800810D4, which tests event bit 900 + index: world 0/code 10 is 910.
        /// This grid is separate from the named stone-discovery table, which has
        /// no entry for this region. Katz map-service group 8 and its all-Sylvarant
        /// loop set this bit. Keep the native region index without guessing a
        /// geographical name or treating it as another Katz Island flag.
        LongRangeSylvarantRegion10Unlocked = 910,
        LongRangeMeltokioRegionFlag1 = 911,
        LongRangeMeltokioRegionFlag2 = 912,
        LongRangeSybakAndNEArchipelagoFlag1 = 913,
        LongRangeSybakAndNEArchipelagoFlag2 = 914,
        LongRangeGaoracchiaAndFlanoirFlag1 = 915,
        LongRangeAltamiraAndYmirFlag1 = 916,
        LongRangeGaoracchiaAndFlanoirFlag2 = 917,
        LongRangeAltamiraAndYmirFlag2 = 918,
        LongRangeAltamiraAndYmirFlag3 = 919,
        LongRangeTheMiningMountainRangeFlag1 = 920,
        LongRangeTheMiningMountainRangeFlag2 = 921,
        LongRangeSybakAndNEArchipelagoFlag3 = 922,
        /// All 151 original reads require this bit to be zero before optional events; story segments set it while party members are unavailable, then clear it when normal exploration resumes.
        SideEventsSuppressed = 1000,
        /// Meltokio 399 Sebastian delivers royal outfits and invitations, with Lloyd sent to collect his missing outfit (132-159).
        NoblePartyInvitationsReceived = 1001,
        /// Meltokio 399 Lloyd and Zelos discuss the previous evening's party (160-165).
        NoblePartyAftermathSeen = 1002,
        /// Mizuho 326 Orochi discusses Kuchinawa's duel challenge; Sheena goes to report the details to Tiga (861-874).
        KuchinawaDuelReported = 1003,
        /// Mizuho 329 completes the duel scene on either victory or defeat; both paths return Corrine's Bell.
        KuchinawaDuelComplete = 1005,
        /// Altessa's House 139 Raine collapses with a fever and the party splits up to find doctors (686-729).
        RaineOzetteColdEventStarted = 1006,
        /// Altessa's House 138 Genis and Mithos leave for Fooji to find Fandalia nectar; Lloyd may follow or stay (314-343).
        GenisMithosFlowerTripStarted = 1007,
        /// Fooji 418 Lloyd secretly watches Genis and Mithos fight a monster on their flower trip (39-63).
        GenisMithosFoojiBattleSeen = 1008,
        /// Fooji 416 Genis and Mithos find the flower and discuss their friendship before returning to cure Raine (325-374).
        FandaliaFlowerFound = 1009,
        /// Altessa's House 139 both Lloyd-followed and Lloyd-stayed scenes confirm the nectar cured Raine's fever (730-813).
        RaineOzetteColdCured = 1010,
        /// Martel Temple 307 Sheena senses Corrine and asks to go farther inside (92-99).
        CorrinePresenceSensedAtMartelTemple = 1011,
        /// Within the late Corrine presence/bell/Sheena prerequisites, Martel 0309 entry uses this bit to spawn Corrine actor 20 and light actor 21 instead of calling sub_51EC. No literal setter establishes completion of the later Verius scene.
        MartelTempleCorrineApparitionEnabled = 1012,
        /// Abyssion gives Nebilim and Nebilim's Key after the party accepts his request; Flanoir and unused Hakonesia variants agree.
        DevilsArmsQuestStarted = 1013,
        /// Six location variants share Abyssion's conversation about the Devil's Arms causing Presea's symptoms (Altamira 547-565).
        DevilsArmsPreseaSymptomsDiscussed = 1014,
        /// Six variants introduce the bankrupt kendama seller before the buy/refuse choices; this flag does not prove a purchase.
        DisasterKendamaSellerMet = 1015,
        /// Six variants discuss the Devil's Arms choosing a person and whispering to them (Altamira 572-589).
        DevilsArmsVoicesDiscussed = 1016,
        /// Flanoir and unused Hakonesia variants suggest Koton owns the last Devil's Arm (Flanoir 658-671).
        DevilsArmsKotonSearchSuggested = 1017,
        /// Seven location variants show Presea recovering from possession and the Necronomicon being broken apart (Altamira 626-647).
        DevilsArmsPreseaPossessionSceneSeen = 1018,
        /// Meltokio 390 and unused 391 receptionist scene introduces the controlled party member to the arena; the flag is set before the optional rules explanation, so it does not mean rules were read or registration completed.
        ColiseumReceptionistIntroductionSeen = 1019,
        /// Meltokio 380 Tokunaga asks Zelos about someone defeating him at the Coliseum (559-586).
        TokunagaColiseumInquirySeen = 1020,
        /// Meltokio 380 Zelos spots Seles while Tokunaga insists she could not be there (587-604).
        SelesSpottedInMeltokio = 1021,
        /// Meltokio 392 sets this before Seles's first challenge, whose waiting-room follow-up explains she wanted to accompany Zelos; this does not imply victory.
        SelesTravelChallengeStarted = 1022,
        /// Meltokio 392 first Seles challenge victory branch has her concede defeat and requests a waiting-room conversation (538-563).
        SelesTravelChallengeWon = 1023,
        /// Both Meltokio 390 and 391 waiting-room variants explain Seles's wish to travel with Zelos and his concern for her (930-983 in 390).
        ZelosSelesColiseumReunionSeen = 1024,
        /// Meltokio 392 sets this before Seles declares she will avenge her brother and starts formation 82; this does not imply victory.
        SelesRevengeChallengeStarted = 1025,
        /// Meltokio 392 revenge challenge victory branch has Seles concede and Tokunaga explain her grief over her brother's death (594-621).
        SelesRevengeChallengeWon = 1026,
        /// Meltokio 399 and unused 400 variant deliver the king's urgent summons because Princess Hilda's life is in danger.
        PrincessRescueSummonsReceived = 1027,
        /// Meltokio 405 king explains the princess-for-Zelos exchange and the party agrees to the bridge operation (276-313).
        PrincessHostageExchangePlanAgreed = 1028,
        /// Tethe'alla Bridge 75 reveals the poisoned hostage is Kate; Raine takes her to Meltokio for treatment (193-216).
        KateRescuedAtBridge = 1029,
        /// Both Meltokio 399 and 400 variants have Zelos say he will stop hating half-elves after recognizing Kate and his mother's killer as the Pope's victims.
        ZelosHalfElfPrejudiceDiscussionSeen = 1030,
        /// Gaoracchia 517 princess rescue finale awards TitleId::ZelosPrincessGuard before returning to report to the king.
        ZelosPrincessGuardTitleReceived = 1031,
        /// Meltokio 399 and unused 400 follow-up says Kate is resting and the king requests a castle visit.
        PrincessRescueRoyalAudienceRequested = 1032,
        /// Meltokio 405 royal audience confirms the Pope's dismissal, discusses half-elf laws and announces the celebration (386-419).
        PrincessRescueRoyalAudienceComplete = 1033,
        /// Meltokio 405 after Zelos's death, before the rescue briefing, the king dismisses the party and Hilda blames them for losing Zelos (316-335).
        PrincessRescueDismissalBeforeBriefingSeen = 1034,
        /// Meltokio 405 after Zelos's death between briefing and bridge, the king reproaches the party for missing the exchange (337-344).
        PrincessRescueMissedExchangeReprimandSeen = 1035,
        /// Meltokio 405 after Zelos's death following the bridge but before the rescue, the king reproaches the party for abandoning Hilda (346-353).
        PrincessRescueAbandonmentReprimandSeen = 1036,
        /// Meltokio 405 after Hilda was rescued but Zelos died before the audience, the king thanks the party and agrees to help protect the people (356-383).
        PrincessRescueRoyalAudienceAfterZelosDeathSeen = 1037,
        /// Both Luin 453 and 456 boat-offer variants set this after Lloyd declines and the seller invites him to reconsider.
        AifreadBoatOfferDeclined = 1038,
        /// Both Luin 453 and 456 variants reveal the supposed boat purchase was a 3,000 Gald lease and deduct the payment.
        AifreadBoatLeaseSigned = 1039,
        /// Both Luin variants set this after Lloyd refuses Aifread's errand and learns monthly rent will otherwise be charged.
        AifreadErrandInitiallyRefused = 1040,
        /// Both Luin 453 and 456 Aifread scene variants award TitleId::LloydArrghMeHearties before setting this flag.
        LloydArrghMeHeartiesTitleReceived = 1041,
        /// Shared interior 95 uses outer room 81's flag 192 to record this house's Auguste-search conversation; it can be any of the three clues depending on visit order.
        AugusteSearchHouseOfSalvation0081Visited = 1042,
        /// Shared interior 95 uses outer room 83's flag 194 to record this house's Auguste-search conversation; clue progression is tracked separately.
        AugusteSearchHouseOfSalvation0083Visited = 1043,
        /// Shared interior 95 uses outer room 84's flag 195 to record this house's Auguste-search conversation; clue progression is tracked separately.
        AugusteSearchHouseOfSalvation0084Visited = 1044,
        /// Any of the three House of Salvation branches may first explain Spiritua's ring and the pilgrim descended from the Seven Saints (275).
        SpirituasRingPilgrimClueHeard = 1045,
        /// Any of the three house branches may next identify High Priest Auguste and his plan to offer Spiritua's treasure at the end of his pilgrimage (277-287).
        AugusteOfferingPilgrimageClueHeard = 1046,
        /// Any remaining house branch may give the final clue that Auguste is at the Thoda boat-dock House of Salvation at Aifread's request (313-321).
        AugusteAtThodaDockReported = 1047,
        /// Thoda Dock 82 and unused House of Salvation 95 variant grant ItemId::SpirituaSRing in Auguste's Aifread scene.
        SpirituasRingReceived = 1048,
        /// Hima 300 grants ItemId::AifreadSLetter and directs the party to Izoold.
        AifreadsLetterReceived = 1049,
        /// Izoold 357 Lyla sends Max with the party to raise the debt payment by selling to Koton, then changes to Hakonesia Peak.
        AifreadDebtFundraisingStarted = 1050,
        /// Hakonesia Peak 124 Koton pays the negotiated 100,000,000 Gald to Max, and the party returns to Lyla (203-220).
        AifreadDebtMoneyRaised = 1051,
        /// Izoold 357 Max pays Lyla, who explicitly marks the debt as paid (257-262).
        AifreadDebtRepaid = 1052,
        /// Unused Izoold 354 Max finale grants ItemId::AifreadSHat before setting this flag.
        AifreadsHatReceived = 1053,
        /// Mizuho 326 and unused 327 variant have Orochi report moaning from the unconscious chief's house.
        IgaguriMoaningReported = 1054,
        /// Both Mizuho variants hear Igaguri counting missing guardians and decide to investigate the Temple of Lightning.
        IgaguriTempleInvestigationStarted = 1055,
        /// Lightning Temple 475 Sheena recognizes Igaguri's counting voice and Lloyd suggests going farther in (295-299).
        IgaguriVoiceHeardAtLightningTemple = 1056,
        /// Lightning Temple 475 Igaguri's spirit goes ahead to the village and the party follows (350-361).
        IgaguriSpiritReturnedToMizuho = 1057,
        /// Both Mizuho variants reunite Sheena with the awakened Igaguri and leave them alone together.
        SheenaIgaguriReunionSeen = 1058,
        /// Both Mizuho variants leave Sheena in the village overnight for the chief's orders; the subsequent ceremony awards the separate Successor title flag.
        SheenaSuccessionRitualStarted = 1059,
        /// Mizuho 326 and unused 327 variant award TitleId::SheenaSuccessor after Igaguri entrusts Sheena to the party.
        SheenaSuccessorTitleReceived = 1060,
        /// Both Mizuho variants explain the twelve guardians, the three given to Sheena, and why Igaguri's spirit was searching for the missing ones.
        IgaguriGuardiansExplained = 1061,
        /// Altessa's House 139 sets this after choosing a companion to leave behind so Tabatha can ride a Rheaird to Toize Valley Mine.
        TabathaMineExcursionStarted = 1062,
        /// Toize 414 and unused 415 variant have Tabatha explain the living Exspheres' loneliness and Presea resolve to understand them.
        TabathaExsphereVoicesSceneSeen = 1063,
        /// Altessa's House 139 post-mine conversation has Presea reflect that hatred gives birth to nothing, then restores the companion who stayed behind.
        TabathaMineExcursionComplete = 1064,
        /// Sybak 434 the party agrees to find the lost child's mother (897-920).
        SybakLostChildSearchStarted = 1065,
        /// Sybak 434 soldiers find the child's mother and Zelos offers her work at his house before this finale flag (1006-1043).
        SybakLostChildReunited = 1066,
        /// All four academy scene variants set this after the challenge dialogue whether Genis accepts or refuses; acceptance uses another flag.
        PalmacostaAcademyChallengeOffered = 1067,
        /// Eight setting paths across four academy variants arrange intensive training with Raine, on accepting the challenge or trying again; this is not the exam-win flag.
        PalmacostaAcademyStudySessionRequested = 1068,
        /// All four Palmacosta academy variants award TitleId::GenisHonorRoll in the examination finale.
        GenisHonorRollTitleReceived = 1069,
        /// Asgard 31 exposes the impostors, returns the townspeople's donations and accepts an overnight stay; both mercy-choice paths converge before this marker.
        AsgardFakeChosenExposed = 1070,
        /// Balacruf 507 the impostors leave for another city and the party discusses whether they are bad people (366-379).
        BalacrufFakeChosenEncounterSeen = 1071,
        /// Both Luin 454 and 457 variants have Colette release Christopher and ask him to stop his brother's fraud.
        LuinFakeChosenMercySceneSeen = 1072,
        /// Asgard 29 impostors defend Colette from townspeople blaming her for the failed regeneration and thank her for saving Christopher (588-674).
        FakeChosenDefendedColette = 1073,
        /// Palmacosta 0056 gates the first Kratos sword-lesson helper on this bit being clear, separately from its completion flag 1075 and the other story, party and orienteering prerequisites. No literal setter establishes why it is blocked.
        KratosPalmacostaSwordLessonBlocked = 1074,
        /// Field 238 lesson returns to Palmacosta 56 after Kratos explains the vulnerability of Lloyd's two-sword style (336-351).
        KratosPalmacostaSwordLessonComplete = 1075,
        /// Asgard 33 inn stay sets this before sub_2832 starts the next Kratos lesson; completion in field 242 uses flag 1077.
        KratosAsgardSwordLessonStarted = 1076,
        /// Field 242 lesson ends with Lloyd comparing Kratos to a big brother and returns to Asgard 33 (65-80).
        KratosAsgardSwordLessonComplete = 1077,
        /// Luin 447 sets this only when Lloyd accepts Kratos's practice offer, then changes to field 238.
        KratosLuinSwordLessonStarted = 1078,
        /// Field 238 Kratos teaches Lloyd to pause and breathe when frustrated, then returns to Luin 447 (359-374).
        KratosLuinSwordLessonComplete = 1079,
        /// Hima 300 sets this only when Lloyd accepts the lesson, then changes to field 242.
        KratosHimaSwordLessonStarted = 1080,
        /// Field 242 final lesson has Lloyd thank Kratos as his master, then returns to Hima 300 (84-99).
        KratosHimaSwordLessonComplete = 1081,
        /// Sybak 434 resident reports Kratos entering the academy; the scene and unused caller both set this before the investigate/ignore choice.
        KratosSybakVisitsReported = 1082,
        /// Sybak 434 sets this only when Lloyd chooses to investigate Kratos's academy visits via the rear entrance (891-896).
        KratosSybakInvestigationChosen = 1083,
        /// Sybak 443 Kate reports Kratos repaired an Ancient War machine and produced adamantite (200-227).
        KratosAdamantiteRefiningReported = 1084,
        /// Ozette 427 and unused 425 variant report Kratos seeking wood hot enough to melt Aionis, before the worry/ignore choice.
        KratosAionisFirewoodSearchReported = 1085,
        /// Meltokio 377 trigger and called scene set this when Lloyd sees Kratos leaving the castle, before the pursue/ignore choice.
        KratosCastleExitSeen = 1086,
        /// Meltokio 377 sets this only when Lloyd decides to follow Kratos rather than ignore him (507-510).
        KratosCastlePursuitChosen = 1087,
        /// Meltokio 376 Lloyd confronts Kratos about sacred wood and Aionis, and Kratos urges him to cure Colette quickly (700-734).
        KratosSacredWoodConfrontationSeen = 1088,
        /// Altessa's House 142 guard scene, its caller and unused 138 copy advise not to tire Altessa and lead the party inside.
        AltessaRecoveryVisitStarted = 1089,
        /// Altessa's House 139 convalescing Altessa explains Aionis's mana conversion and suggests Kratos or Pronyma may know its location (833-852).
        AltessaAionisRecoveryAdviceHeard = 1090,
        /// Altessa's House 142 and unused 138 copy show Zelos speaking secretly with Kratos and denying it when Lloyd arrives.
        ZelosKratosSecretConversationSeen = 1091,
        /// Heimdall 295 elder reveals Kratos begged the elves to give the party Mana Leaf Herb (22-41).
        KratosManaLeafAppealRevealed = 1092,
        /// Hima 300 Yuan is found looking for something important and refuses to explain (743-756).
        YuanSearchInHimaSeen = 1102,
        /// Fooji 416 Raine reads ancient elven letters on the engagement ring and Sheena identifies it as what Yuan was seeking (309-324).
        YuansEngagementRingFound = 1103,
        /// Palmacosta Ranch 199 Yuan explicitly thanks Lloyd for returning his ring; the party discusses Yuan and Martel (233-290).
        YuansEngagementRingReturned = 1104,
        /// Meltokio 379 awards TitleId::ZelosMaskedSwordsman before setting this flag.
        ZelosMaskedSwordsmanTitleReceived = 1105,
        /// Altamira 27 starts the Klonoa mascot substitution and Altamira 11 presentation also sets this flag; Dream Traveler completion is separate.
        AltamiraMascotDebutStarted = 1106,
        /// Altamira 27 awards and equips TitleId::PreseaDreamTraveler in the costume scene.
        PreseaDreamTravelerTitleReceived = 1107,
        /// Meltokio 377 Regal accepts the cooking battle and learns the stew recipe before changing to the contest location.
        RegalCookingChallengeStarted = 1108,
        /// Meltokio 402 sub_2B8B praises Regal's dish and the rival acknowledges overestimating his own skill (430-451).
        RegalCookingChallengeGoodDishEnding = 1109,
        /// Meltokio 402 sub_2E5A serves a demonic-flavored dish, the rival flees and Raine laughs (452-471); this is not simply a contest-loss flag.
        RegalCookingChallengeBadDishEnding = 1110,
        /// Meltokio 377 awards TitleId::RegalGodOfTheKitchen in the cooking finale.
        RegalGodOfTheKitchenTitleReceived = 1111,
        /// Meltokio 404 Colette agrees to replace the maid before beginning the castle job (326-333).
        ColetteMaidJobStarted = 1112,
        /// Meltokio 377 Colette reports the maid work was praised, her pay rose and she received new clothes; the scene equips her Maid title.
        ColetteMaidJobComplete = 1113,
        /// Heimdall 291 sub_7464 has Crowley teach Genis Meteor Storm and awards TitleId::GenisManaMaster, before the trigger sets this flag.
        GenisCrowleyLessonComplete = 1131,
        /// Meltokio 411 prisoner explains Levin's role in Regal's fighting style and directs him to the hot spring, before the visit/decline choice.
        LevinLocationReported = 1146,
        /// Meltokio 411 sets this only when Lloyd agrees to visit Levin and help Regal clear his hesitation (115-118).
        LevinVisitAgreed = 1147,
        /// Hot Spring 89 Levin and Lloyd reassure Regal about his vow not to fight with his hands (51-66).
        RegalLevinReunionSeen = 1148,
        /// Ozette 433 Ralph offers Presea stories of Sieg and urges her to trust her companions (387-400).
        PreseaRalphReunionSeen = 1149,
        SandWormSightingHeard = 1150,
        CursedSwordRumorHeard = 1151,
        CaravanRoaming = 1152,
        /// All six Nova caravan variants award TitleId::RaineMonsterCollector before setting this flag.
        RaineMonsterCollectorTitleReceived = 1153,
        /// Mizuho 327 Tiga links Maxwell to the floating city and its Stone of Power, and the party agrees to search for it (545-590).
        TigaMaxwellClueHeard = 1155,
        /// Exire 190 first stone interaction reads the elemental-proof inscription in either the informed or uninformed dialogue variant.
        MaxwellStoneInscriptionRead = 1156,
        /// Both Exire 190 setting paths require four equipped elemental jewels and call sub_3E32, where Maxwell appears; an Origin pact is separately required to proceed.
        MaxwellSummonedAtExire = 1157,
        /// Exire 190 sets this before requiring Sheena in the active formation and starting Maxwell battle formation 84; pact completion uses its own flag.
        MaxwellChallengeStarted = 1158,
        /// Hakonesia Peak 124 sub_55EF introduces Koton's collector challenge and notices Genis's book before this flag; collection completion is separate.
        KotonCollectorsBookConversationSeen = 1159,
        /// Both Hakonesia Peak 124 collection-completion variants award TitleId::GenisItemCollector.
        GenisItemCollectorTitleReceived = 1160,
        /// Iselia 340 awards TitleId::GenisBrotherlyLove before setting this flag.
        GenisBrotherlyLoveTitleReceived = 1161,
        /// Iselia 340 Raine explains she came to say goodbye to her classroom (340-345), only on the first such conversation.
        RaineClassroomFarewellDiscussed = 1162,
        /// Iselia 340 early-story exit trigger (story 202000-301000) runs the classroom farewell (336-339) and sets this; it is not the later Brotherly Love event.
        FirstIseliaClassroomDepartureSeen = 1163,
        /// Both Heimdall 282 and 291 variants have Elan identify Noishe as an ancient protozoan and ask Lloyd to bring him back.
        NoisheProtozoanNatureExplained = 1165,
        /// Asgard 49 awards TitleId::GenisFigurineCollector before setting this flag.
        GenisFigurineCollectorTitleReceived = 1170,
        /// Dirk's House 374 introduces projecting thoughts onto pellets for the Figurine Book and sets this before the optional first crafting attempt (645-660).
        DirkFigurineCraftingIntroduced = 1171,
        /// Both setting paths in Meltokio 398 have Presea encourage Janet's music practice; the Horace-known variant differs in wording.
        PreseaJanetFirstConversationSeen = 1172,
        /// Meltokio 379 sets this after either asking Presea about Janet's resemblance or simply leaving; it does not prove the optional explanation was heard.
        PreseaJanetFirstVisitAftermathSeen = 1173,
        /// Meltokio 398 exit from the second Janet visit sets this after either the song conversation or leaving immediately.
        PreseaJanetSecondVisitComplete = 1174,
        /// Meltokio 379 Lloyd and Presea discuss Janet remembering her and Presea's painful happiness (491-498).
        PreseaJanetReunionAftermathSeen = 1175,
        /// Sybak 435 Presea recognizes Horace and his sister Janet but does not reveal her own identity (382-393).
        PreseaHoraceConversationSeen = 1176,
        /// Sybak 435 sets this after either asking Presea why she will not tell Horace the truth or simply leaving; the choice paths converge.
        PreseaHoraceVisitAftermathSeen = 1177,
        /// Triet 485 awards TitleId::RaineSisterlyLove after the conversation about her care for Genis.
        RaineSisterlyLoveTitleReceived = 1178,
        /// Ozette 425 sub_720F has Wells call Presea a monster and ask the party to take her out of the village; either response reaches this marker.
        PreseaOstracismInOzetteSeen = 1185,
        /// Altamira 11 sub_6F61 has Wells apologize for driving Presea out and awards TitleId::PreseaMatureKid before this flag.
        PreseaMatureKidTitleReceived = 1186,
        /// Mizuho 326 shop introduction asks for Puninja's lost account book; the well-return scene also sets this marker, so it does not by itself prove completion.
        PuninjaNotebookRequestKnown = 1187,
        /// Mizuho 326 sub_A3EB returns the lost account book and unconditionally grants ItemId::SecretNotebook before the caller sets this flag.
        PuninjaSecretNotebookReceived = 1188,
        /// Sybak 434 Joshua grants ItemId::PinkPearlRing (718-722).
        PinkPearlRingReceived = 1189,
        /// Altamira 11 and unused 17 variant introduce the missing daughters before Lloyd's accept/refuse choice.
        AltamiraLostDaughtersRequestHeard = 1190,
        /// Both mother-dialogue variants set this only after Lloyd agrees to search Altamira for her daughters.
        AltamiraLostDaughtersSearchAccepted = 1191,
        /// Altamira 15 Beth first says she will return to her mother (637-638); the later return requires a separate interaction.
        AltamiraBethFirstReturnRequestHeard = 1192,
        /// Altamira 15 Beth finally returns after the repeat request (641-642).
        AltamiraBethReturnedToMother = 1193,
        /// Altamira 11 and unused 17 mother variants explain that Beth may need to be asked several times before returning.
        AltamiraBethRepeatRequestHintHeard = 1194,
        /// Altamira 17 and unused 11 variant have Diana agree to return to her mother; the surrounding scene requires a charming man.
        AltamiraDianaReturnedToMother = 1195,
        /// Altamira 14 Mary agrees to go home once everyone else has returned (380-381).
        AltamiraMaryReturnedToMother = 1197,
        /// Altamira 14 Jo agrees to return to her mother (382-383).
        AltamiraJoReturnedToMother = 1198,
        /// Both mother-finale variants confirm all daughters are found and arrange a reward at the hotel; swimsuit pickup is separate.
        AltamiraLostDaughtersReunited = 1199,
        /// Altamira 16 awards TitleId::LloydBeachBoy in the beach vacation introduction before choosing companions.
        LloydBeachBoyTitleReceived = 1200,
        /// Altamira 14 after the third selected companion interaction, sub_464C restores the saved roster, costumes, leader and Lloyd's title, then returns to the hotel.
        AltamiraBeachOutingComplete = 1201,
        /// Triet 525 night scene discusses whether human conflicts can end and Lloyd's determination to work for that future (211-224).
        TrietKratosWarDiscussionSeen = 1202,
        /// Altamira 11 Rosa asks the party to deliver money to Joshua before the accept/refuse choice (737-751).
        RosaMoneyDeliveryRequested = 1203,
        /// Altamira 11 Lloyd accepts Rosa's request, receives 5,000 Gald and agrees to report to her in Meltokio (756-765).
        RosaMoneyDeliveryAccepted = 1204,
        /// Sybak 434 Joshua refuses Rosa's payment as charity and Lloyd agrees to return it (771-784).
        JoshuaRefusedRosasMoney = 1205,
        /// Meltokio 379 reveals Rosa was forced to marry Cromwell to repay her parents' debt and returns the 5,000 Gald (542-554).
        RosasForcedEngagementRevealed = 1206,
        /// Sybak 434 tell-the-truth branch explains Rosa's sacrifice, and Joshua leaves for Cromwell's house (807-820).
        JoshuaToldOfRosasEngagement = 1207,
        /// Sybak 434 keep-the-truth-hidden branch only reports the money was returned, leaving Joshua to concentrate on his studies (821-832).
        JoshuaMoneyReturnOnlyEndingChosen = 1208,
        /// Meltokio 378 reunion finale thanks the party and discusses Joshua resuming his studies (499-512).
        JoshuaRosaReunited = 1209,
        /// Sybak 434 Sheena returns ItemId::PinkPearlRing to Joshua, removing it from inventory as a memory of Rosa (861-872).
        JoshuaPinkPearlRingReturned = 1210,
        /// Asgard 31 Aisha asks Raine to visit the mayor and the party changes to his house (268-279).
        RaineAsgardMayorAudienceRequested = 1211,
        /// Asgard 50 Raine agrees to perform the ceremonial dance again and the party proceeds to the dais (245-258).
        RaineAsgardDanceAgreed = 1212,
        /// Asgard 31 awards TitleId::RaineMaiden after the dance scene.
        RaineMaidenTitleReceived = 1213,
        /// Colette's dog-naming event: Bob; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogBobNamed = 1214,
        /// Colette's dog-naming event: Lulu; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogLuluNamed = 1215,
        /// Colette's dog-naming event: Hal; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogHalNamed = 1216,
        /// Colette's dog-naming event: Murry; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogMurryNamed = 1217,
        /// Colette's dog-naming event: Teddy; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogTeddyNamed = 1218,
        /// Colette's dog-naming event: Pepe; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogPepeNamed = 1219,
        /// Colette's dog-naming event: Kenny; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogKennyNamed = 1220,
        /// Colette's dog-naming event: Kitty; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogKittyNamed = 1221,
        /// Colette's dog-naming event: Binky; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogBinkyNamed = 1222,
        /// Colette's dog-naming event: Caramel; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogCaramelNamed = 1223,
        /// Colette's dog-naming event: Monmon; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogMonmonNamed = 1224,
        /// Colette's dog-naming event: Simon; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogSimonNamed = 1225,
        /// Colette's dog-naming event: Coco; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogCocoNamed = 1226,
        /// Colette's dog-naming event: Cookie; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogCookieNamed = 1227,
        /// Colette's dog-naming event: Boo; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogBooNamed = 1228,
        /// Colette's dog-naming event: Rockie; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogRockieNamed = 1229,
        /// Colette's dog-naming event: Poochi; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogPoochiNamed = 1230,
        /// Colette's dog-naming event: Penny; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogPennyNamed = 1231,
        /// Colette's dog-naming event: Tiggy; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogTiggyNamed = 1232,
        /// Colette's dog-naming event: Bunz; the flag is set after her naming dialogue, with Colette as field leader. All 2 field variant(s) agree.
        DogBunzNamed = 1233,
        /// Colette's dog-naming event: Pookie; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogPookieNamed = 1234,
        /// Colette's dog-naming event: Chibi; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogChibiNamed = 1235,
        /// Colette's dog-naming event: Pudding; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogPuddingNamed = 1236,
        /// Colette's dog-naming event: Sammy; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogSammyNamed = 1237,
        /// Colette's dog-naming event: Kalcy; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogKalcyNamed = 1238,
        /// Colette's dog-naming event: Chappy; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogChappyNamed = 1239,
        /// Colette's dog-naming event: Turbie; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogTurbieNamed = 1240,
        /// Colette's dog-naming event: Lucky; the flag is set after her naming dialogue, with Colette as field leader. All 4 field variant(s) agree.
        DogLuckyNamed = 1241,
        /// Colette's dog-naming event: Chuchu; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogChuchuNamed = 1242,
        /// Colette's dog-naming event: Cammy; the flag is set after her naming dialogue, with Colette as field leader. All 1 field variant(s) agree.
        DogCammyNamed = 1243,
        /// Sybak 445 first book inspection hears the warning that the sealed book leads to Niflheim; Origin's warning is separate (543-560).
        NiflheimCursedBookRumorHeard = 1244,
        /// Sybak 445 Origin warns the party it may be trapped in the book before the enter/ask-for-help choice (568-574).
        OriginNiflheimEntryWarningHeard = 1245,
        /// Both Sybak 445 setting paths mark entry immediately before changing to Niflheim 524, including the direct reentry path.
        NiflheimEntered = 1246,
        /// Sybak 445 Origin confirms the book can no longer harm anyone and the party reflects on human evil after destroying it (590-606).
        NiflheimDestructionAftermathSeen = 1247,
        /// Niflheim 524 Origin explains strengthening Soulfire, burning the book and the danger of the flame running out (189-200).
        NiflheimSoulfireRulesExplained = 1248,
        /// Niflheim 524 unseen speaker insists outsiders must obey its world's rules and Presea notes the book's danger (201-208).
        NiflheimWorldRulesConfrontationSeen = 1249,
        /// Niflheim 524 companion variants reassure Lloyd that Soulfire prevents the monsters from readily harming them (ending 225-228).
        NiflheimSoulfireProtectionDiscussed = 1250,
        /// Niflheim 524 another sealed enemy opposes burning the book; the party realizes more than one monster was sealed inside (231-242).
        NiflheimMultipleSealedMonstersRevealed = 1251,
        /// Niflheim 524 sets this after the Hell Knight battle, formation 98, which has game-over-on-defeat behavior (245-250).
        NiflheimHellKnightDefeated = 1252,
        /// Niflheim 524 sets this after Living Armor, formation 99, and before choosing whether to burn Soulfire; it does not prove the book was burned.
        NiflheimLivingArmorDefeated = 1253,
        /// Niflheim 524 sets this with the Living Armor victory; Sybak 445 clears it after the book-destruction aftermath, unlike the permanent victory marker.
        NiflheimFinaleAftermathPending = 1254,
        /// Niflheim 524 sets this only on the Burn the Soulfire choice after Living Armor; the Don't burn choice leaves it unset.
        NiflheimBookBurned = 1255,
        /// Izoold 353 Raine restores Clara's human form; Clara explicitly confirms she has returned to normal (460-473).
        ClaraRestoredToHuman = 1256,
        /// Altessa's House 138 sets this only when Lloyd lets Genis and Mithos go alone rather than secretly following their Fandalia flower trip.
        LloydStayedDuringFlowerTrip = 1257,
        /// All three Hot Spring 89 first-reception variants set this, including the maintenance-closed and baths-open cases; it does not mean the baths were used.
        HotSpringReceptionIntroduced = 1258,
        /// Hot Spring 89 first men's bath ends by suggesting the women bathe afterward (135-144).
        HotSpringFirstMensBathSeen = 1259,
        /// Hot Spring 89 later men's bath has the group joke about Zelos's thoughts (145-154), separate from the first bath and peeping incident.
        HotSpringRepeatMensBathSeen = 1260,
        /// Hot Spring 89 peeping incident ends by awarding Lloyd's Peeping Tom title; Colette and Sheena title awards are part of the same scene.
        HotSpringPeepingIncidentSeen = 1261,
        /// Luin 456 Kratos thanks Lloyd for rebuilding Anna's birthplace and they promise to protect it (468-481).
        AnnasLuinBirthplaceRevealed = 1262,
        /// Altessa's House 139 selects character 2, Colette, to wait behind and lend Tabatha a Rheaird; completion uses this same bit to restore her.
        ColetteStayedBehindForTabathaMineTrip = 1263,
        /// Altessa's House 139 selects character 3, Genis, to wait behind and lend Tabatha a Rheaird; completion restores him using this bit.
        GenisStayedBehindForTabathaMineTrip = 1264,
        /// Altessa's House 139 selects character 4, Raine, to wait behind and lend Tabatha a Rheaird; completion restores her using this bit.
        RaineStayedBehindForTabathaMineTrip = 1265,
        /// Altessa's House 139 selects character 5, Sheena, to wait behind and lend Tabatha a Rheaird; completion restores her using this bit.
        SheenaStayedBehindForTabathaMineTrip = 1266,
        /// Altessa's House 139 selects character 6, Zelos, to wait behind and lend Tabatha a Rheaird; completion restores him using this bit.
        ZelosStayedBehindForTabathaMineTrip = 1267,
        /// Altessa's House 139 selects character 8, Regal, to wait behind and lend Tabatha a Rheaird; completion restores him using this bit.
        RegalStayedBehindForTabathaMineTrip = 1268,
        /// Hakonesia Peak 124 Koton grants HeartOfChaos and the party prepares to return to the Temple of Darkness (465-468).
        KotonDevilsArmExchangeComplete = 1269,
        /// Darkness Temple 127 Abyssion requests the collected Devil's Arms before sub_58BA begins the final encounter; this is not the battle-win marker.
        AbyssionFinalMeetingStarted = 1270,
        SoulEaterAcquired = 1271,
        /// Sybak 445 sets this after Origin's book-destruction finale and clearing the pending-aftermath flag, immediately before restoring the library scene.
        NiflheimBookDestructionComplete = 1272,
        /// Meltokio 378 Genis asks the party to bring a Katz to Vice, beginning the costume search (545-558).
        GenisKatzCostumeSearchStarted = 1273,
        /// Both Meltokio 389 Katz variants refuse to help Noah's friend and direct Genis to the village elder without revealing the village location.
        KatzElderVisitSuggested = 1274,
        /// Katz Village 419 elder gives Genis the suit Noah used while hunting Katz, to try deceiving Noah; the later Katz title uses a separate marker.
        KatzDisguiseReceived = 1275,
        /// Meltokio 378 awards TitleId::GenisKatzKatzKatz before setting this flag.
        GenisKatzKatzKatzTitleReceived = 1276,
        /// Palmacosta 63 Cacao asks the party to rescue Chocolat and Lloyd promises to do so (0-9).
        CacaoChocolatRescuePromiseSeen = 1277,
        /// Palmacosta 63 the party reports Marble's fate and that Chocolat was moved to another ranch; Lloyd renews his rescue promise (22-35).
        CacaoMarbleDeathReported = 1278,
        /// Altessa's House 139 Mithos and the party discuss the extinct Linkite Tree and he offers to help them (512-525).
        MithosLinkiteTreeConversationSeen = 1279,
        /// Triet Ruins 219 grants a Spirit Bottle immediately before this one-shot pickup flag.
        TrietRuins0219SpiritBottlePickupTaken = 1280,
        /// Meltokio 468 grants an Elixir immediately before this one-shot pickup flag.
        Meltokio0468ElixirPickupTaken = 1281,
        /// Latheon Gorge 496 grants an Elixir immediately before this one-shot pickup flag.
        LatheonGorge0496ElixirPickupTaken = 1283,
        /// Both Thoda Dock 97 Candy interactions share this flag; the variant after Regal's identity reveal can discuss Lezareno's annual turnover.
        CandyAtThodaDockMet = 1284,
        /// Heimdall 292 an elf explains the cursed book and Sacred Stone and asks Lloyd to burn the book before its sealed evils escape (93-105).
        NiflheimBookDestructionRequested = 1285,
        /// All six Devil's Arms scene copies set this after granting ItemId::EvilEye; a copied task may contain several consecutive arm scenes.
        EvilEyeDevilsArmObtained = 1286,
        DevilsArmAcquired = 1287,
        /// Flanoir and unused Hakonesia variants grant Nebilim and Nebilim's Key before this acquisition marker, separate from the general quest-start flag.
        NebilimDevilsArmObtained = 1288,
        /// All six kendama-seller variants grant ItemId::Disaster before this purchase marker; seller introduction has a separate flag.
        DisasterDevilsArmObtained = 1289,
        /// All six Devil's Arms scene copies associate this marker with the ItemId::Fafnir grant.
        FafnirDevilsArmObtained = 1290,
        /// All six Devil's Arms scene copies associate this marker with the ItemId::GatesOfHell grant.
        GatesOfHellDevilsArmObtained = 1291,
        /// All six Devil's Arms scene copies associate this marker with the ItemId::Diablos grant.
        DiablosDevilsArmObtained = 1292,
        /// All six Devil's Arms scene copies associate this marker with the ItemId::Apocalypse grant.
        ApocalypseDevilsArmObtained = 1293,
        /// Hakonesia Peak 124 grants ItemId::HeartOfChaos before setting this acquisition marker alongside Koton's exchange-complete flag.
        HeartOfChaosDevilsArmObtained = 1294,
        /// Either Genis exit reminder sets this shared latch; the oracle's ruins-location conversation in room 0491 clears it. It is temporary reminder suppression, not a permanent story milestone.
        TrietExitReminderSuppressed = 1731,
        /// Zelos Personal-skill interactions share numbered slots across towns.
        /// A slot records that its flirt/gift interaction has been used.
        ZelosFlirtSlot00Used = 1800,
        ZelosFlirtSlot01Used = 1801,
        ZelosFlirtSlot02Used = 1802,
        ZelosFlirtSlot03Used = 1803,
        ZelosFlirtSlot04Used = 1804,
        ZelosFlirtSlot05Used = 1805,
        ZelosFlirtSlot06Used = 1806,
        ZelosFlirtSlot07Used = 1807,
        ZelosFlirtSlot08Used = 1808,
        ZelosFlirtSlot09Used = 1809,
        ZelosFlirtSlot10Used = 1810,
        ZelosFlirtSlot11Used = 1811,
        ZelosFlirtSlot12Used = 1812,
        ZelosFlirtSlot13Used = 1813,
        ZelosFlirtSlot14Used = 1814,
        ZelosFlirtSlot15Used = 1815,
        ZelosFlirtSlot16Used = 1816,
        ZelosFlirtSlot17Used = 1817,
        ZelosFlirtSlot18Used = 1818,
        ZelosFlirtSlot19Used = 1819,
        ZelosFlirtSlot20Used = 1820,
        ZelosFlirtSlot21Used = 1821,
        ZelosFlirtSlot22Used = 1822,
        ZelosFlirtSlot23Used = 1823,
        ZelosFlirtSlot24Used = 1824,
        ZelosFlirtSlot25Used = 1825,
        ZelosFlirtSlot26Used = 1826,
        ZelosFlirtSlot27Used = 1827,
        ZelosFlirtSlot28Used = 1828,
        ZelosFlirtSlot29Used = 1829,
        ZelosFlirtSlot30Used = 1830,
        ZelosFlirtSlot31Used = 1831,
        ZelosFlirtSlot32Used = 1832,
        ZelosFlirtSlot33Used = 1833,
        ZelosFlirtSlot34Used = 1834,
        ZelosFlirtSlot35Used = 1835,
        ZelosFlirtSlot36Used = 1836,
        /// Exit routines sum the relevant flirt slots into town completion bits,
        /// then sum towns into Sylvarant/Tethe'alla and finally overall completion.
        ZelosFlirtAllComplete = 1850,
        ZelosFlirtSylvarantComplete = 1851,
        ZelosFlirtTetheAllaComplete = 1852,
        ZelosFlirtIseliaComplete = 1853,
        ZelosFlirtTrietComplete = 1854,
        ZelosFlirtIzooldComplete = 1855,
        ZelosFlirtPalmacostaComplete = 1856,
        ZelosFlirtAsgardComplete = 1857,
        ZelosFlirtLuin0447Complete = 1858,
        ZelosFlirtLuin0449Complete = 1859,
        ZelosFlirtLuin0452Complete = 1860,
        ZelosFlirtLuin0455Complete = 1861,
        ZelosFlirtHimaComplete = 1862,
        ZelosFlirtMeltokioComplete = 1863,
        ZelosFlirtSybakComplete = 1864,
        ZelosFlirtOzetteComplete = 1865,
        ZelosFlirtAltamiraComplete = 1866,
        ZelosFlirtFlanoirComplete = 1867,
        ZelosFlirtHeimdallComplete = 1868,
        ZelosFlirtExireComplete = 1869,
        ZelosFlirtHouseOfSalvation0081Complete = 1870,
        ZelosFlirtThodaDockComplete = 1871,
        ZelosFlirtHouseOfSalvation0084Complete = 1873,
        ZelosFlirtHotSpringComplete = 1874,
        ZelosFlirtHouseOfGuidance0091Complete = 1875,
        ZelosFlirtSEAbbeyComplete = 1876,
        ZelosFlirtHouseOfGuidance0093Complete = 1877,
        ZelosFlirtThodaGeyserComplete = 1878,
        ZelosFlirtBalacrufMausoleumComplete = 1879,
        ZelosFlirtMizuhoComplete = 1880,
    }
}

ids! {
    /// Reader-only debug access in the retained Thoda dog-name helper.
    pub enum Field0006StoryStage: u32 {
        /// Thoda 0006 sub_C4C0 exposes its Debug Flag checker / All flags ON menu only at this exact story value, before the normal dog-name count and title check. The all-on choice sets the thirty dog flags starting at 1214. This reader-only debug selector is local to 0006, with no recovered constant setter.
        DogFlagsDebugMenu = 1208001,
    }
}

ids! {
    /// A field-local preset in the retained Fire Temple debug menu.
    pub enum Field0221StoryStage: u32 {
        /// Triet Ruins 0221 debug event 1001: selecting the Already made the pact with Celsius preset writes this main-story number and then sets CelsiusPactFormed. It is not the normal Efreet pact-completion writer or a recovered Tethe'alla chapter event. The field-specific alias preserves that unusual debug assignment without inventing a global milestone.
        CelsiusPactDebugPreset = 10002000,
    }
}

ids! {
    /// Story-word presets in Ossa's retained bottle-test menu, not global milestones.
    pub enum Field0351StoryStage: u32 {
        /// Ossa 0351 event 101, Checker choice 3 (Dark Bottle test), writes this directly to the story word. Choice 4 (Normal) resets it to zero.
        DarkBottleTestPreset = 100,
        /// Ossa 0351 event 101, Checker choice 2 (Holy Bottle test), writes this directly to the story word. Preserve this unusual debug use rather than inventing a prologue milestone.
        HolyBottleTestPreset = 200,
    }
}

ids! {
    /// Local phases in retained Dirk scenes, sharing the original global word.
    pub enum Field0370StoryStage: u32 {
        /// Retained Dirk's House 0370 sub_0374: Lloyd mistakes the prone Dirk for dead, Dirk says he is alive and scolds him, then this stage is written. The same value marks Palmacosta dock arrival in the main route; only this field's explicit enum supplies the prototype meaning.
        DirkFoundAlive = 1601000,
        /// Shared with room 0371: its repair-night discussion sets this before
        /// returning here. Room 0370 dispatches from the saved story word.
        GenisKeyCrestRepairNightAgreed = 1602000,
        /// Retained Dirk's House 0370 sub_0929: Dirk finishes fitting Genis's Key Crest, says his spells can be stronger and exchanges the Dwarven Vow farewell before this stage. It belongs to this older Dirk sequence; do not invent a Palmacosta event for the value.
        GenisKeyCrestFittedAndDepartureComplete = 1603000,
    }
}

ids! {
    /// Local phases in retained Dirk scenes, sharing the original global word.
    pub enum Field0371StoryStage: u32 {
        /// Shared with room 0370: its prone-Dirk greeting sets this before this
        /// room dispatches the repair-night discussion from its saved story word.
        DirkFoundAlive = 1601000,
        /// Retained Dirk's House 0371 sub_00CB: after discussing the Desian damage and following Colette for atonement or revenge, Dirk agrees to stay up making a Key Crest for Genis with Lloyd's help. This stage is not Palmacosta's potion dispute despite sharing its number.
        GenisKeyCrestRepairNightAgreed = 1602000,
        /// Shared with room 0370's final Key Crest fitting and departure scene.
        GenisKeyCrestFittedAndDepartureComplete = 1603000,
    }
}
