//! Meanings of shared flag bits in particular original field resources.
//! These are aliases for the same session bits, not separate scene storage.
//! `FieldNNNNFlag` is also the opt-in naming convention for cooked definitions.

ids! { pub enum WorldFlag: u16 {
    /// The overworld entry for landmark 45 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 45. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Triet Desert and setting that region's separate
    /// permanent mode flag(s). Entry/exit do not consume this selector, and the original
    /// independent tests and duplicate registry keys remain. It is not a permanent
    /// region-unlocked flag.
    TrietDesertStoneMarkerSelected = 151,
    /// The overworld entry for landmark 46 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 46. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Ossa Mountains and Izoold and setting that
    /// region's separate permanent mode flag(s). Entry/exit do not consume this
    /// selector, and the original independent tests and duplicate registry keys remain.
    /// It is not a permanent region-unlocked flag.
    OssaIzooldStoneMarkerSelected = 152,
    /// The overworld entry for landmark 47 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 47. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Thoda and eastern Palmacosta and setting that
    /// region's separate permanent mode flag(s). Entry/exit do not consume this
    /// selector, and the original independent tests and duplicate registry keys remain.
    /// It is not a permanent region-unlocked flag.
    ThodaPalmacostaEastStoneMarkerSelected = 153,
    /// The overworld entry for landmark 48 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 48. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Hakonesia Peak and northeastern Palmacosta and
    /// setting that region's separate permanent mode flag(s). Entry/exit do not consume
    /// this selector, and the original independent tests and duplicate registry keys
    /// remain. It is not a permanent region-unlocked flag.
    HakonesiaPalmacostaNortheastStoneMarkerSelected = 154,
    /// The overworld entry for landmark 49 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 49. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Luin and Asgard and setting that region's separate
    /// permanent mode flag(s). Entry/exit do not consume this selector, and the original
    /// independent tests and duplicate registry keys remain. It is not a permanent
    /// region-unlocked flag.
    LuinAsgardStoneMarkerSelected = 155,
    /// The overworld entry for landmark 50 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 50. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Katz Island and setting that region's separate
    /// permanent mode flag(s). Entry/exit do not consume this selector, and the original
    /// independent tests and duplicate registry keys remain. It is not a permanent
    /// region-unlocked flag.
    KatzIslandStoneMarkerSelected = 156,
    /// The overworld entry for landmark 51 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 51. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Hima and western Luin and setting that region's
    /// separate permanent mode flag(s). Entry/exit do not consume this selector, and the
    /// original independent tests and duplicate registry keys remain. It is not a
    /// permanent region-unlocked flag.
    HimaLuinWestStoneMarkerSelected = 157,
    /// The overworld entry for landmark 52 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 52. This value has no Long-range Mode unlock branch in the
    /// stone interaction; the alias describes its observed return destination only.
    /// Entry/exit do not consume this selector, and the original independent tests and
    /// duplicate registry keys remain. It is not a permanent region-unlocked flag.
    Landmark52ReturnSelected = 158,
    /// The overworld entry for landmark 62 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 62. This value has no Long-range Mode unlock branch in the
    /// stone interaction; the alias describes its observed return destination only.
    /// Entry/exit do not consume this selector, and the original independent tests and
    /// duplicate registry keys remain. It is not a permanent region-unlocked flag.
    ExSkillTutorialReturnSelected = 159,
    /// These exploration callbacks mark their indexed slot immediately before
    /// transferring to Nova's room 247. No reader of the marks is recovered in
    /// that room, and both exits always return to landmark 54. The names describe
    /// observed transfer provenance, not return selection, item receipt or event
    /// completion. Only the original observed slot writes are declared here.
    /// World event 306: TetheallaExploration00Item.
    Exploration00Caravan0247TransferMarked = 160,
    /// World event 64: SylvarantExploration01Item1.
    Exploration01Caravan0247TransferMarked = 161,
    /// World event 308: TetheallaExploration02Item.
    Exploration02Caravan0247TransferMarked = 162,
    /// World event 309: TetheallaExploration03Item.
    Exploration03Caravan0247TransferMarked = 163,
    /// World event 68: SylvarantExploration05Item2.
    Exploration05Caravan0247TransferMarked = 165,
    /// World events 70 and 313: exploration slot 07 in the two worlds.
    Exploration07Caravan0247TransferMarked = 167,
    /// World event 74: SylvarantExploration11Item4.
    Exploration11Caravan0247TransferMarked = 171,
    /// World event 75: SylvarantExploration12Item5.
    Exploration12Caravan0247TransferMarked = 172,
    /// World event 76: SylvarantExploration13Item6.
    Exploration13Caravan0247TransferMarked = 173,
    /// World event 78: SylvarantExploration15Item7.
    Exploration15Caravan0247TransferMarked = 175,
    /// World event 323: TetheallaExploration17Item.
    Exploration17Caravan0247TransferMarked = 177,
    /// World event 81: SylvarantExploration18Item8.
    Exploration18Caravan0247TransferMarked = 178,
    /// World event 83: SylvarantExploration20Item9.
    Exploration20Caravan0247TransferMarked = 180,
    /// World event 328: TetheallaExploration22Item.
    Exploration22Caravan0247TransferMarked = 182,
    /// World events 88 and 331: exploration slot 25 in the two worlds.
    Exploration25Caravan0247TransferMarked = 185,
    /// World event 333: TetheallaExploration27Item.
    Exploration27Caravan0247TransferMarked = 187,
    /// World event 91: SylvarantExploration28Item11.
    Exploration28Caravan0247TransferMarked = 188,
    /// World event 335: TetheallaExploration29Item.
    Exploration29Caravan0247TransferMarked = 189,
    /// World events 94 (Sylvarant Sand Worm) and 337 (Tethealla item) share
    /// slot 31's mark. Both original callbacks transfer to room 247; this is
    /// neither proof of a won encounter nor an awarded item.
    Exploration31Caravan0247TransferMarked = 191,
    /// The corresponding overworld caravan entry sets this shared exterior selector
    /// before entering Nova's room 243. Both room exits read the same selector to return
    /// to overworld landmark 53; it is also the reviewed selector for this exterior in
    /// the shared Salvation/Guidance group. This names the applicable world write, not a
    /// visit or Long-range Mode milestone. Other equal-valued field scratch uses retain
    /// their own contexts.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// World event 54 (the Triet caravan point) writes this before the same
    /// room-247 transfer. That room has no recovered reader and its two exits
    /// always return to landmark 54. This names the entry callback's write,
    /// without claiming a consumed return selector or permanent visit.
    TrietCaravan0247TransferMarked = 193,
    /// The corresponding overworld caravan entry sets this shared exterior selector
    /// before entering Nova's room 239. Both room exits read the same selector to return
    /// to overworld landmark 55; it is also the reviewed selector for this exterior in
    /// the shared Salvation/Guidance group. This names the applicable world write, not a
    /// visit or Long-range Mode milestone. Other equal-valued field scratch uses retain
    /// their own contexts.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// The corresponding overworld caravan entry sets this shared exterior selector
    /// before entering Nova's room 239. Both room exits read the same selector to return
    /// to overworld landmark 57; it is also the reviewed selector for this exterior in
    /// the shared Salvation/Guidance group. This names the applicable world write, not a
    /// visit or Long-range Mode milestone. Other equal-valued field scratch uses retain
    /// their own contexts.
    ExteriorHotSpring0089Selected = 196,
    /// The corresponding overworld caravan entry sets this shared exterior selector
    /// before entering Nova's room 239. Both room exits read the same selector to return
    /// to overworld landmark 58; it is also the reviewed selector for this exterior in
    /// the shared Salvation/Guidance group. This names the applicable world write, not a
    /// visit or Long-range Mode milestone. Other equal-valued field scratch uses retain
    /// their own contexts.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// World region event 3 sets bit 1749 immediately before changing to Triet 485. That
    /// entry consumes it for the optional early Raine title scene and clears it. This
    /// overworld alias does not imply a Derris route or title completion.
    TrietEntranceArrivalPending = 1749,
} }

ids! { pub enum Field0006Flag: u16 {
    /// The first return trigger calls Genis's Want to go back now scene only while this
    /// is clear; both taking the boat and staying longer set it. Reading sign actor 202
    /// (Up ahead: Thoda Geyser) also sets it without that question. When set the trigger
    /// instead creates the regular return record to dock 82. The name describes question
    /// suppression, not a completed boat trip or proof that the conversation played.
    ReturnTripQuestionSuppressed = 170,
} }

ids! { pub enum Field0007Flag: u16 {
    /// The passage from Thoda room 7 to room 9 sets this; the corresponding return
    /// passage clears it. Room 9 entry uses it to play sound 386 on channel 60. Entry
    /// does not consume the bit, so this describes an enabled passage sound rather than
    /// a one-shot arrival or camera flag.
    UpperPassageSoundEnabled = 160,
} }

ids! { pub enum Field0008Flag: u16 {
    /// Room 9's upper jug puzzle sets and reverses shared bit 152. Room 8 entry restores
    /// a scenery Z offset of 250 while clear; while set it chooses the offset/collision
    /// arrangement using the separate locked-block mark 154. Keep the jug state distinct
    /// from a permanent Water Seal milestone and retain the original independent
    /// restoration logic.
    UpperWaterJugTipped = 152,
    /// Room 9 marks this after movable block 6000 reaches its X-only right-socket
    /// condition. Room 8 entry, after the upper-jug branch, uses clear for Z offset 0
    /// with collision actors 200/201, or set for Z offset 200. This is the same shared
    /// block placement mark, not a second jug or a newly completed seal. Preserve its
    /// original dependency on the preceding 152 test.
    Block6000LockedAtRightSocket = 154,
    /// The passage from Thoda room 8 to room 9 sets this; the corresponding return
    /// passage clears it. Room 9 entry uses it to play sound 386 on channel 80. Entry
    /// does not consume the bit, so this describes an enabled passage sound rather than
    /// a one-shot arrival or camera flag.
    LowerPassageSoundEnabled = 161,
} }

ids! { pub enum Field0009Flag: u16 {
    /// Inspection explicitly says torch lit/unlit. Ring contact with target 102 outside
    /// Water mode creates flame emitter 300 and records this; Water contact removes the
    /// flame and clears it. If the upper water jug was tipped, extinguishing either
    /// torch also reverses that jug and clears 152.
    Torch102Lit = 150,
    /// Inspection explicitly says torch lit/unlit. Ring contact with target 103 outside
    /// Water mode creates flame emitter 301 and records this; Water contact removes the
    /// flame and clears it. If the upper water jug was tipped, extinguishing either
    /// torch also reverses that jug and clears 152.
    Torch103Lit = 151,
    /// With both torch flags set, Water-mode contact with upper target 100 animates jug
    /// actor 6002 and sets this. Inspection says the water weight causes it to lean. The
    /// lower jug requires this bit. Extinguishing either torch reverses actor 6002 and
    /// clears it; do not treat it as permanent completion.
    UpperWaterJugTipped = 152,
    /// Water-mode contact with lower target 101 requires the upper jug tipped, animates
    /// jug actor 6003 and sets this before raising three scenery nodes by 600.
    /// Inspection confirms the water-weight leaning state. The distinct Water Seal
    /// guardian flag also restores an accessible layout independently.
    LowerWaterJugTipped = 153,
    /// Monitor 4000 checks only movable actor 6000's X coordinate for 525, then sets
    /// this, replaces it with fixed collision actor 6001 at (525,-1925,-605) and stops
    /// monitoring. Entry restores the fixed actor and adjusts the upper-jug mechanism
    /// layout. Preserve the original X-only condition.
    Block6000LockedAtRightSocket = 154,
    /// Lighting the second of the two torches while this bit is clear makes the
    /// controlled actor react and turn toward upper jug target 100, then sets it. Either
    /// lighting order shares the same marker. It gates that reaction, not the lower jug
    /// or Water Seal completion.
    BothTorchesReactionSeen = 155,
    /// The passage from Thoda room 7 to room 9 sets this; the corresponding return
    /// passage clears it. Room 9 entry uses it to play sound 386 on channel 60. Entry
    /// does not consume the bit, so this describes an enabled passage sound rather than
    /// a one-shot arrival or camera flag.
    UpperPassageSoundEnabled = 160,
    /// The passage from Thoda room 8 to room 9 sets this; the corresponding return
    /// passage clears it. Room 9 entry uses it to play sound 386 on channel 80. Entry
    /// does not consume the bit, so this describes an enabled passage sound rather than
    /// a one-shot arrival or camera flag.
    LowerPassageSoundEnabled = 161,
} }

ids! { pub enum Field0011Flag: u16 {
    /// Altamira 11's ELB elevator clears this for its upper landing and sets it while
    /// lowering the scenery node by 270 and moving collision actor 300 down. Entry
    /// restores the lower offset when set. The outbound handler to room 24 also sets it
    /// before leaving. This is the selected landing, not an enabled/permanently repaired
    /// lift; preserve all original motion and attachment calls.
    ElevatorLowerLandingSelected = 150,
} }

ids! { pub enum Field0013Flag: u16 {
    /// Altamira 13's ELB elevator sets this when raising the platform and clears it when
    /// lowering; its debug UP/DOWN menu uses the same polarity. Entry restores Z offset
    /// 15 when set or -299+15 when clear, while the raising sequence targets offset 2.
    /// Keep that original restore-versus-motion offset difference and the independent
    /// actor-300/controlled-character positions.
    ElevatorUpperLandingSelected = 152,
} }

ids! { pub enum Field0014Flag: u16 {
    /// These switches enable each companion's beach actor before outing completion; they
    /// do not independently prove costume/title ownership.
    ColetteBeachActorEnabled = 1700,
    GenisBeachActorEnabled = 1701,
    RaineBeachActorEnabled = 1702,
    SheenaBeachActorEnabled = 1703,
    ZelosBeachActorEnabled = 1704,
    PreseaBeachActorEnabled = 1705,
    RegalBeachActorEnabled = 1706,
    KratosBeachActorEnabled = 1707,
    /// Each first conversation increments one shared counter; its third distinct
    /// companion conversation completes the outing. Repeat dialogue still runs.
    ColetteBeachConversationSeen = 1710,
    GenisBeachConversationSeen = 1711,
    RaineBeachConversationSeen = 1712,
    SheenaBeachConversationSeen = 1713,
    ZelosBeachConversationSeen = 1714,
    PreseaBeachConversationSeen = 1715,
    RegalBeachConversationSeen = 1716,
    KratosBeachConversationSeen = 1717,
} }

ids! { pub enum Field0015Flag: u16 {
    /// The elemental-railway choice explicitly offers Altamira Entrance. Choosing Yes
    /// attaches the controlled actor and attendant to railway actor 300, sets this mark
    /// before its departure animation, and finally changes to room 11 at
    /// (-1968,3009,-60). There is no recovered literal reader in these Altamira rooms,
    /// so the name records the departure write rather than asserting a completed trip or
    /// an elevator setting.
    RailwayToEntranceDepartureMarked = 151,
} }

ids! { pub enum Field0016Flag: u16 {
    /// Altamira 16 sets this for Colette from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    ColetteBeachParticipantSelected = 1700,
    /// Altamira 16 sets this for Genis from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    GenisBeachParticipantSelected = 1701,
    /// Altamira 16 sets this for Raine from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    RaineBeachParticipantSelected = 1702,
    /// Altamira 16 sets this for Sheena from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    SheenaBeachParticipantSelected = 1703,
    /// Altamira 16 sets this for Zelos from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    ZelosBeachParticipantSelected = 1704,
    /// Altamira 16 sets this for Presea from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    PreseaBeachParticipantSelected = 1705,
    /// Altamira 16 sets this for Regal from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    RegalBeachParticipantSelected = 1706,
    /// Altamira 16 sets this for Kratos from either preselected companion word
    /// (0x0828/0x082c) or the direct beach-choice helper. The selected-participant
    /// reward helper reads it; field 14 uses the same bit to enable that beach actor.
    /// Kratos' branch has dialogue without a swimwear title, so this is not a
    /// title-owned flag.
    KratosBeachParticipantSelected = 1707,
} }

ids! { pub enum Field0033Flag: u16 {
    /// Asgard 33/34 set this before their connecting stair transfer; the destination's
    /// entry walk clears it after positioning the controlled actor.
    StairTransferPending = 150,
} }

ids! { pub enum Field0034Flag: u16 {
    StairTransferPending = 150,
} }

ids! { pub enum Field0068Flag: u16 {
    /// Palmacosta 68/71 share the corresponding stair-transfer/entry-walk latch.
    StairTransferPending = 150,
} }

ids! { pub enum Field0071Flag: u16 {
    StairTransferPending = 150,
} }

ids! { pub enum Field0075Flag: u16 {
    /// The bridge approach from room 75 selects this before entering shared crossing 77.
    /// Crossing entry independently restores that side's plank/scenery arrangement and
    /// initializes its progress counter to 4 and the opposite counter to 0. The
    /// crossing's two outward handlers clear both selectors on their actual field-exit
    /// branches; their internal repositioning branches retain them. This is a current
    /// crossing layout, separate from the permanent Bridge0076Entered flag. The room-78
    /// approach also selects the 0075-side layout while entering at Y 0, so the name
    /// does not assert arrival exclusively from room 75. Room 75's selected jump to 76
    /// also clears both bits. Keep its original choice fallthrough and subsequent calls.
    Bridge0075SideLayoutSelected = 150,
    /// The bridge approach from room 76 selects this before entering shared crossing 77.
    /// Crossing entry independently restores that side's plank/scenery arrangement and
    /// initializes its progress counter to 4 and the opposite counter to 0. The
    /// crossing's two outward handlers clear both selectors on their actual field-exit
    /// branches; their internal repositioning branches retain them. This is a current
    /// crossing layout, separate from the permanent Bridge0076Entered flag. Room 75's
    /// selected jump to 76 also clears both bits. Keep its original choice fallthrough
    /// and subsequent calls.
    Bridge0076SideLayoutSelected = 151,
    /// After Kate's bridge rescue, sub_659B sets the separate KateRescuedAtBridge story
    /// flag, then selects the left upstairs room layout before changing to Meltokio 400.
    /// That shared room tests bit 181 for its scenery/return door and the later
    /// Kate/Zelos conversation, and clears it on return to Meltokio 399 at
    /// (-287,396,425). This destination-layout alias is not a permanent rescue-complete
    /// flag.
    UpstairsLeftRoomSelected = 181,
} }

ids! { pub enum Field0076Flag: u16 {
    /// The bridge approach from room 76 selects this before entering shared crossing 77.
    /// Crossing entry independently restores that side's plank/scenery arrangement and
    /// initializes its progress counter to 4 and the opposite counter to 0. The
    /// crossing's two outward handlers clear both selectors on their actual field-exit
    /// branches; their internal repositioning branches retain them. This is a current
    /// crossing layout, separate from the permanent Bridge0076Entered flag.
    Bridge0076SideLayoutSelected = 151,
} }

ids! { pub enum Field0077Flag: u16 {
    /// The bridge approach from room 75 selects this before entering shared crossing 77.
    /// Crossing entry independently restores that side's plank/scenery arrangement and
    /// initializes its progress counter to 4 and the opposite counter to 0. The
    /// crossing's two outward handlers clear both selectors on their actual field-exit
    /// branches; their internal repositioning branches retain them. This is a current
    /// crossing layout, separate from the permanent Bridge0076Entered flag. The room-78
    /// approach also selects the 0075-side layout while entering at Y 0, so the name
    /// does not assert arrival exclusively from room 75.
    Bridge0075SideLayoutSelected = 150,
    /// The bridge approach from room 76 selects this before entering shared crossing 77.
    /// Crossing entry independently restores that side's plank/scenery arrangement and
    /// initializes its progress counter to 4 and the opposite counter to 0. The
    /// crossing's two outward handlers clear both selectors on their actual field-exit
    /// branches; their internal repositioning branches retain them. This is a current
    /// crossing layout, separate from the permanent Bridge0076Entered flag.
    Bridge0076SideLayoutSelected = 151,
} }

ids! { pub enum Field0078Flag: u16 {
    /// The bridge approach from room 75 selects this before entering shared crossing 77.
    /// Crossing entry independently restores that side's plank/scenery arrangement and
    /// initializes its progress counter to 4 and the opposite counter to 0. The
    /// crossing's two outward handlers clear both selectors on their actual field-exit
    /// branches; their internal repositioning branches retain them. This is a current
    /// crossing layout, separate from the permanent Bridge0076Entered flag. The room-78
    /// approach also selects the 0075-side layout while entering at Y 0, so the name
    /// does not assert arrival exclusively from room 75.
    Bridge0075SideLayoutSelected = 150,
} }

ids! { pub enum Field0081Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0082Flag: u16 {
    /// Dock 97's accepted boat choices set this before entering dock 82; the early paid
    /// branch separately checks and removes 200 Gald, while recognized-Chosen branches
    /// are free. Dock 82 entry tests it to run the departure helper, which clears it
    /// before cinematic 514 to Thoda Geyser room 6. This is a pending departure,
    /// separate from the fare/recognition flags and from returning from the island.
    SightseeingBoatDeparturePending = 156,
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0083Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0084Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0089Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0091Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0092Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0093Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for ThodaDock0082 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorThodaDock0082Selected = 193,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for SeAbbey0092 in the shared exterior/interior/caravan group; entering
    /// another exterior clears it.
    ExteriorSeAbbey0092Selected = 198,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0095Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
} }

ids! { pub enum Field0096Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HouseOfSalvation0084 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0084Selected = 195,
} }

ids! { pub enum Field0097Flag: u16 {
    /// Dock 97's accepted boat choices set this before entering dock 82; the early paid
    /// branch separately checks and removes 200 Gald, while recognized-Chosen branches
    /// are free. Dock 82 entry tests it to run the departure helper, which clears it
    /// before cinematic 514 to Thoda Geyser room 6. This is a pending departure,
    /// separate from the fare/recognition flags and from returning from the island.
    SightseeingBoatDeparturePending = 156,
} }

ids! { pub enum Field0111Flag: u16 {
    /// Guidance 111/112 set bit 150 before their connecting passage transfer.
    /// Destination entry uses temporary actor 3000 while walking the controlled actor
    /// in; the helper clears the bit, restores the entrance trigger and returns control.
    /// Exterior-variant selectors 196/197/199 remain separate.
    EntranceWalkPending = 150,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0112Flag: u16 {
    /// Guidance 111/112 set bit 150 before their connecting passage transfer.
    /// Destination entry uses temporary actor 3000 while walking the controlled actor
    /// in; the helper clears the bit, restores the entrance trigger and returns control.
    /// Exterior-variant selectors 196/197/199 remain separate.
    EntranceWalkPending = 150,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0115Flag: u16 {
    /// Abbey 115/116 transfer helpers set this; entry walks clear it, restore the
    /// entrance touch trigger, and return control from temporary actor 3000.
    EntranceWalkPending = 150,
} }

ids! { pub enum Field0116Flag: u16 {
    EntranceWalkPending = 150,
} }

ids! { pub enum Field0117Flag: u16 {
    /// Abbey 117/118 use the same entrance-walk protocol in separate resources.
    EntranceWalkPending = 150,
} }

ids! { pub enum Field0118Flag: u16 {
    EntranceWalkPending = 150,
} }

ids! { pub enum Field0125Flag: u16 {
    /// Field 125 sets this before entering 126; the destination selects Lloyd as leader
    /// and clears it. Party snapshot/removal is separate script code.
    LloydLeaderSelectionPending = 150,
} }

ids! { pub enum Field0126Flag: u16 {
    LloydLeaderSelectionPending = 150,
    /// Darkness-mode contact with light target 2101 first calls sub_53D5 to restore all
    /// five lights and clear 151..155, then sets this bit, changes its scenery nodes and
    /// disables its ring contact. Nearby baby Shadows stop at that light only while the
    /// bit is clear; extinguishing it clears their matching obstacle reference. Entry
    /// restores the stored light state. This is a current light selection, not a
    /// permanent dungeon milestone.
    Light2101Extinguished = 151,
    /// Darkness-mode contact with light target 2102 first calls sub_53D5 to restore all
    /// five lights and clear 151..155, then sets this bit, changes its scenery nodes and
    /// disables its ring contact. Nearby baby Shadows stop at that light only while the
    /// bit is clear; extinguishing it clears their matching obstacle reference. Entry
    /// restores the stored light state. This is a current light selection, not a
    /// permanent dungeon milestone.
    Light2102Extinguished = 152,
    /// Darkness-mode contact with light target 2103 first calls sub_53D5 to restore all
    /// five lights and clear 151..155, then sets this bit, changes its scenery nodes and
    /// disables its ring contact. Nearby baby Shadows stop at that light only while the
    /// bit is clear; extinguishing it clears their matching obstacle reference. Entry
    /// restores the stored light state. This is a current light selection, not a
    /// permanent dungeon milestone.
    Light2103Extinguished = 153,
    /// Darkness-mode contact with light target 2104 first calls sub_53D5 to restore all
    /// five lights and clear 151..155, then sets this bit, changes its scenery nodes and
    /// disables its ring contact. Nearby baby Shadows stop at that light only while the
    /// bit is clear; extinguishing it clears their matching obstacle reference. Entry
    /// restores the stored light state. This is a current light selection, not a
    /// permanent dungeon milestone.
    Light2104Extinguished = 154,
    /// Darkness-mode contact with light target 2105 first calls sub_53D5 to restore all
    /// five lights and clear 151..155, then sets this bit, changes its scenery nodes and
    /// disables its ring contact. Nearby baby Shadows stop at that light only while the
    /// bit is clear; extinguishing it clears their matching obstacle reference. Entry
    /// restores the stored light state. This is a current light selection, not a
    /// permanent dungeon milestone.
    Light2105Extinguished = 155,
    /// Darkness-mode contact with either block 3101 or 3102 resets all four linked pairs
    /// via sub_5538, records this pair and replaces its actors with movable scenery
    /// actors, then starts monitor 4000. Entry restores this mode and the stored two-bit
    /// displacement. The bits 156..159 enable movement; they do not mean a pair reached
    /// its final position. The introductory push helper sub_4607 also sets 156 before
    /// demonstrating this first pair.
    LinkedBlocks3101And3102Movable = 156,
    /// Darkness-mode contact with either block 3103 or 3104 resets all four linked pairs
    /// via sub_5538, records this pair and replaces its actors with movable scenery
    /// actors, then starts monitor 4001. Entry restores this mode and the stored two-bit
    /// displacement. The bits 156..159 enable movement; they do not mean a pair reached
    /// its final position.
    LinkedBlocks3103And3104Movable = 157,
    /// Darkness-mode contact with either block 3105 or 3106 resets all four linked pairs
    /// via sub_5538, records this pair and replaces its actors with movable scenery
    /// actors, then starts monitor 4002. Entry restores this mode and the stored two-bit
    /// displacement. The bits 156..159 enable movement; they do not mean a pair reached
    /// its final position.
    LinkedBlocks3105And3106Movable = 158,
    /// Darkness-mode contact with either block 3107 or 3108 resets all four linked pairs
    /// via sub_5538, records this pair and replaces its actors with movable scenery
    /// actors, then starts monitor 4003. Entry restores this mode and the stored two-bit
    /// displacement. The bits 156..159 enable movement; they do not mean a pair reached
    /// its final position. The 3107 branch contains two consecutive writes setting 159;
    /// both original calls remain.
    LinkedBlocks3107And3108Movable = 159,
    /// The restore helper decodes flags 160/161 as displacement 0,150,300,450 for linked
    /// blocks 3101/3102; this bit contributes 150. Monitor 4000 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. Restoration places 3101 at Y -45 +
    /// displacement and 3102 at Y -1715 - displacement; their normal recorded positions
    /// match those four offsets.
    LinkedBlocks3101And3102OffsetBit150 = 160,
    /// The restore helper decodes flags 160/161 as displacement 0,150,300,450 for linked
    /// blocks 3101/3102; this bit contributes 300. Monitor 4000 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. Restoration places 3101 at Y -45 +
    /// displacement and 3102 at Y -1715 - displacement; their normal recorded positions
    /// match those four offsets.
    LinkedBlocks3101And3102OffsetBit300 = 161,
    /// The restore helper decodes flags 162/163 as displacement 0,150,300,450 for linked
    /// blocks 3103/3104; this bit contributes 150. Monitor 4001 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. Restoration places 3103 at X -1125 -
    /// displacement and 3104 at X 2150 - displacement. Only the monitor branch with
    /// controlled Y -925 writes these position bits.
    LinkedBlocks3103And3104OffsetBit150 = 162,
    /// The restore helper decodes flags 162/163 as displacement 0,150,300,450 for linked
    /// blocks 3103/3104; this bit contributes 300. Monitor 4001 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. Restoration places 3103 at X -1125 -
    /// displacement and 3104 at X 2150 - displacement. Only the monitor branch with
    /// controlled Y -925 writes these position bits.
    LinkedBlocks3103And3104OffsetBit300 = 163,
    /// The restore helper decodes flags 164/165 as displacement 0,150,300,450 for linked
    /// blocks 3105/3106; this bit contributes 150. Monitor 4002 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. Preserve the original asymmetric checks:
    /// the first-side writer tests controlled X -1520 while restored actor 3105 is at X
    /// -2000, and its last Y test is -1715 versus the restore formula -1245 - 450 =
    /// -1695. The other-side tests use actor 3106 at X -875.
    LinkedBlocks3105And3106OffsetBit150 = 164,
    /// The restore helper decodes flags 164/165 as displacement 0,150,300,450 for linked
    /// blocks 3105/3106; this bit contributes 300. Monitor 4002 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. Preserve the original asymmetric checks:
    /// the first-side writer tests controlled X -1520 while restored actor 3105 is at X
    /// -2000, and its last Y test is -1715 versus the restore formula -1245 - 450 =
    /// -1695. The other-side tests use actor 3106 at X -875.
    LinkedBlocks3105And3106OffsetBit300 = 165,
    /// The restore helper decodes flags 166/167 as displacement 0,150,300,450 for linked
    /// blocks 3107/3108; this bit contributes 150. Monitor 4003 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. The first-side writer tests controlled X
    /// -1670 while restored actor 3107 is at X -1610. It records only offsets 0,150,300;
    /// the restore helper still handles both bits set as 450. Do not add a missing
    /// fourth writer or normalize the coordinates.
    LinkedBlocks3107And3108OffsetBit150 = 166,
    /// The restore helper decodes flags 166/167 as displacement 0,150,300,450 for linked
    /// blocks 3107/3108; this bit contributes 300. Monitor 4003 records discrete
    /// positions and updates the coupled actor. The pair's separate movement bit gates
    /// restoration of this stored displacement. The first-side writer tests controlled X
    /// -1670 while restored actor 3107 is at X -1610. It records only offsets 0,150,300;
    /// the restore helper still handles both bits set as 450. Do not add a missing
    /// fourth writer or normalize the coordinates.
    LinkedBlocks3107And3108OffsetBit300 = 167,
    /// Region 5003 removes its own trigger and sets this before the controlled character
    /// points out the Shadow fragment and suggests moving the red-eyed rock to the
    /// front. Entry requires it clear before recreating the trigger, along with the
    /// separate progression and fully-shifted-once conditions. It records the hint
    /// starting, not the block being moved or a Shadow gathered.
    RedEyedRockHintStarted = 179,
} }

ids! { pub enum Field0128Flag: u16 {
    /// Entry in Derris 128 sets this route selector; confirming the teleport back to
    /// Salvation 152 clears it. Bit 1749 selects the 0530 corridor connections: set
    /// means the north portal leads to Salvation 502 and south to Derris 129; clear
    /// means north to Derris 130 and south to Salvation 502. These are alternate routes,
    /// not disabled portals.
    DerrisKharlan0530RouteVia0129Selected = 1749,
} }

ids! { pub enum Field0129Flag: u16 {
    /// Entry in Derris 129 sets this route selector. Bit 1749 selects the 0530 corridor
    /// connections: set means the north portal leads to Salvation 502 and south to
    /// Derris 129; clear means north to Derris 130 and south to Salvation 502. These are
    /// alternate routes, not disabled portals.
    DerrisKharlan0530RouteVia0129Selected = 1749,
} }

ids! {
    /// Dynamic block-transfer marks shared by hub 0130 and receiving room 0135.
    pub enum Field0130FlagBase: u16 {
        /// Add block index 0..7 (hub actor 1000+index, receiver actor 3000+index).
        /// The hub sets a mark at either inscription door; room 0135 spawns the
        /// marked block and clears that mark. Bit 172 separately chooses the
        /// transfer side, including its original clear inside the future-side loop.
        /// These marks are requests, separate from permanent block placement flags.
        BlockTransferPending = 164,
    }
}
ids! {
    /// Dynamic block-transfer marks shared by hub 0130 and receiving room 0135.
    pub enum Field0135FlagBase: u16 {
        /// Add block index 0..7. Entry consumes the hub's marks into actors
        /// 3000+index at X -1282 or 1282, then clears each consumed mark.
        /// Transfer-side bit 172 is independent and is cleared inside the
        /// future-side branch, preserving the original ordering for queued blocks.
        BlockTransferPending = 164,
    }
}

ids! { pub enum Field0130Flag: u16 {
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the EXGemLv4 chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133EXGemLv4LayoutSelected = 159,
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the BlueShield chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133BlueShieldLayoutSelected = 160,
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
    /// The Derris hub/side-corridor protocol records the middle level layout in this
    /// bit. Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. This selects a current
    /// traversal/layout mode, not a permanently opened floor or completed portal.
    HubMiddleLevelLayoutSelected = 162,
    /// The Derris hub/side-corridor protocol records the upper level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. This selects a current
    /// traversal/layout mode, not a permanently opened floor or completed portal.
    HubUpperLevelLayoutSelected = 163,
    /// The hub's future-door transfer at X 1125 sets this; its past-door transfer at X
    /// -1125 clears it, alongside marking the transferred block by index at 164..171.
    /// Room 135 places a marked block at X 1282 when set or -1282 when clear, then
    /// clears the consumed block mark. The future branch also clears 172 inside that
    /// loop, so multiple queued blocks retain the original ordering-dependent behavior.
    /// It is a transfer-side selector, not a stone acquired or block permanently placed.
    FutureSideBlockTransferSelected = 172,
    /// Accepting the Sacred Stone return-to-destroyed-seal-room prompt clears this route
    /// selector, alongside the reused 150..199 bits. Bit 1749 selects the 0530 corridor
    /// connections: set means the north portal leads to Salvation 502 and south to
    /// Derris 129; clear means north to Derris 130 and south to Salvation 502. These are
    /// alternate routes, not disabled portals.
    DerrisKharlan0530RouteVia0129Selected = 1749,
} }

ids! { pub enum Field0131Flag: u16 {
    /// The corresponding passage from room 131 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 131's middle landing at (-1418,814,1004); the return helper clears this
    /// bit. The selected corridor variant exposes the distinct ShieldRing treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0131MiddleSelected = 150,
    /// The corresponding passage from room 131 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 131's upper landing at (-1418,814,2010); the return helper clears this
    /// bit. The selected corridor variant exposes the distinct DemonSSeal treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0131UpperSelected = 151,
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
    /// The Derris hub/side-corridor protocol records the middle level layout in this
    /// bit. Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. This selects a current
    /// traversal/layout mode, not a permanently opened floor or completed portal.
    HubMiddleLevelLayoutSelected = 162,
    /// The Derris hub/side-corridor protocol records the upper level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. This selects a current
    /// traversal/layout mode, not a permanently opened floor or completed portal.
    HubUpperLevelLayoutSelected = 163,
} }

ids! { pub enum Field0132Flag: u16 {
    /// The corresponding passage from room 132 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 132's lower landing at (1344,811,0); the return helper clears this bit.
    /// The selected corridor variant exposes the distinct EnergyTablets treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0132LowerSelected = 157,
    /// The corresponding passage from room 132 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 132's middle landing at (1344,811,1004); the return helper clears this
    /// bit. The selected corridor variant exposes the distinct PrismGuard treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0132MiddleSelected = 158,
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
    /// The Derris hub/side-corridor protocol records the middle level layout in this
    /// bit. Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. This selects a current
    /// traversal/layout mode, not a permanently opened floor or completed portal.
    HubMiddleLevelLayoutSelected = 162,
    /// The Derris hub/side-corridor protocol records the upper level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. This selects a current
    /// traversal/layout mode, not a permanently opened floor or completed portal.
    HubUpperLevelLayoutSelected = 163,
} }

ids! { pub enum Field0133Flag: u16 {
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (71,67,1005); the return helper clears this bit. The return
    /// reselects room 133's BlueShield layout using bit 160. The original variant tests
    /// are independent and are not replaced by an exclusive selector.
    CorridorBlueShieldEastSelected = 152,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-2411,74,3505); the return helper clears this bit. The return
    /// reselects room 133's BlueShield layout using bit 160. The original variant tests
    /// are independent and are not replaced by an exclusive selector.
    CorridorBlueShieldWestSelected = 153,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-1172,1323,2005); the return helper clears this bit. The return
    /// reselects room 133's BlueShield layout using bit 160. The selected corridor
    /// variant exposes the distinct Elixir treasure identities; opened-save bits remain
    /// separate. The original variant tests are independent and are not replaced by an
    /// exclusive selector.
    CorridorBlueShieldNorthSelected = 154,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-2395,68,504); the return helper clears this bit. The return
    /// reselects room 133's EXGemLv4 layout using bit 159. The original variant tests
    /// are independent and are not replaced by an exclusive selector.
    CorridorEXGemWestSelected = 155,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-1170,1298,2005); the return helper clears this bit. The return
    /// reselects room 133's EXGemLv4 layout using bit 159. The selected corridor variant
    /// exposes the distinct SpiritBottle and MortalityCloak treasure identities;
    /// opened-save bits remain separate. The original variant tests are independent and
    /// are not replaced by an exclusive selector.
    CorridorEXGemNorthSelected = 156,
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the EXGemLv4 chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133EXGemLv4LayoutSelected = 159,
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the BlueShield chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133BlueShieldLayoutSelected = 160,
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
    /// The Derris hub/side-corridor protocol records the middle level layout in this
    /// bit. Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. This selects a current
    /// traversal/layout mode, not a permanently opened floor or completed portal.
    HubMiddleLevelLayoutSelected = 162,
} }

ids! { pub enum Field0134Flag: u16 {
    /// The corresponding passage from room 131 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 131's middle landing at (-1418,814,1004); the return helper clears this
    /// bit. The selected corridor variant exposes the distinct ShieldRing treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0131MiddleSelected = 150,
    /// The corresponding passage from room 131 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 131's upper landing at (-1418,814,2010); the return helper clears this
    /// bit. The selected corridor variant exposes the distinct DemonSSeal treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0131UpperSelected = 151,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (71,67,1005); the return helper clears this bit. The return
    /// reselects room 133's BlueShield layout using bit 160. The original variant tests
    /// are independent and are not replaced by an exclusive selector.
    CorridorBlueShieldEastSelected = 152,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-2411,74,3505); the return helper clears this bit. The return
    /// reselects room 133's BlueShield layout using bit 160. The original variant tests
    /// are independent and are not replaced by an exclusive selector.
    CorridorBlueShieldWestSelected = 153,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-1172,1323,2005); the return helper clears this bit. The return
    /// reselects room 133's BlueShield layout using bit 160. The selected corridor
    /// variant exposes the distinct Elixir treasure identities; opened-save bits remain
    /// separate. The original variant tests are independent and are not replaced by an
    /// exclusive selector.
    CorridorBlueShieldNorthSelected = 154,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-2395,68,504); the return helper clears this bit. The return
    /// reselects room 133's EXGemLv4 layout using bit 159. The original variant tests
    /// are independent and are not replaced by an exclusive selector.
    CorridorEXGemWestSelected = 155,
    /// The corresponding passage from room 133 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 133 at (-1170,1298,2005); the return helper clears this bit. The return
    /// reselects room 133's EXGemLv4 layout using bit 159. The selected corridor variant
    /// exposes the distinct SpiritBottle and MortalityCloak treasure identities;
    /// opened-save bits remain separate. The original variant tests are independent and
    /// are not replaced by an exclusive selector.
    CorridorEXGemNorthSelected = 156,
    /// The corresponding passage from room 132 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 132's lower landing at (1344,811,0); the return helper clears this bit.
    /// The selected corridor variant exposes the distinct EnergyTablets treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0132LowerSelected = 157,
    /// The corresponding passage from room 132 sets this before entering shared corridor
    /// 134. That corridor uses it for its scenery/collision variant and selects a return
    /// to room 132's middle landing at (1344,811,1004); the return helper clears this
    /// bit. The selected corridor variant exposes the distinct PrismGuard treasure
    /// identities; opened-save bits remain separate. The original variant tests are
    /// independent and are not replaced by an exclusive selector.
    CorridorTo0132MiddleSelected = 158,
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the EXGemLv4 chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133EXGemLv4LayoutSelected = 159,
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the BlueShield chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133BlueShieldLayoutSelected = 160,
} }

ids! { pub enum Field0135Flag: u16 {
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the EXGemLv4 chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133EXGemLv4LayoutSelected = 159,
    /// Room 133 has two independently tested scenery/passage layouts. This bit selects
    /// the layout containing the BlueShield chest at (-1246,1202,5), with its own
    /// collision, enemy and exit records. Hub/corridor/room-135 passages set it before
    /// entering 133; the corresponding outward handlers clear it. The two layouts share
    /// room 133 but have different corridor return routes; the treasure's own save flag
    /// remains separate.
    Room0133BlueShieldLayoutSelected = 160,
    /// The hub's future-door transfer at X 1125 sets this; its past-door transfer at X
    /// -1125 clears it, alongside marking the transferred block by index at 164..171.
    /// Room 135 places a marked block at X 1282 when set or -1282 when clear, then
    /// clears the consumed block mark. The future branch also clears 172 inside that
    /// loop, so multiple queued blocks retain the original ordering-dependent behavior.
    /// It is a transfer-side selector, not a stone acquired or block permanently placed.
    FutureSideBlockTransferSelected = 172,
} }

ids! { pub enum Field0136Flag: u16 {
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
} }

ids! {
    /// Known uses of shared session bits in Altessa's House 0138.
    /// Unidentified bit 199 is cleared by the retained Ozette Cold departure at
    /// word PC 0x5E93. Its set-state meaning has not been recovered; it remains numeric.
    pub enum Field0138Flag: u16 {
    /// The Alicia/Bryant discussion at Altessa's house ends at story 13601000 and sets
    /// this. At that story stage room 138 creates Mithos at (-645,1480,103) while room
    /// 139 skips his ordinary (-92,23,-3) placement. Those readers explicitly retain
    /// their story-stage guards; the mark is a temporary placement selector, not
    /// permanent absence or a completed Mithos quest.
    MithosRoom0138PlacementSelected = 180,
} }

ids! {
    /// Known uses of shared session bits in Altessa's House 0139.
    /// Unidentified bit 198 is cleared by the retained recovery conversation at
    /// word PC 0x72CB. Its set-state meaning has not been recovered; it remains numeric.
    pub enum Field0139Flag: u16 {
    /// The Alicia/Bryant discussion at Altessa's house ends at story 13601000 and sets
    /// this. At that story stage room 138 creates Mithos at (-645,1480,103) while room
    /// 139 skips his ordinary (-92,23,-3) placement. Those readers explicitly retain
    /// their story-stage guards; the mark is a temporary placement selector, not
    /// permanent absence or a completed Mithos quest.
    MithosRoom0138PlacementSelected = 180,
} }

ids! { pub enum Field0143Flag: u16 {
    /// At Altessa attack-aftermath story stage 21904000, the Yuan conversation (messages
    /// 177..214) sets this after its dialogue. Its interaction handler uses this to
    /// avoid repeating that scene; the other aftermath conversations have independent
    /// marks. Each completion tests all three marks and advances to 21904100 only when
    /// all are set. Yuan's mark also suppresses his actor on re-entry, while the other
    /// two retain shorter repeat replies.
    YuanAttackAftermathConversationSeen = 150,
    /// At Altessa attack-aftermath story stage 21904000, the Altessa conversation
    /// (messages 215..224) sets this after its dialogue. Its interaction handler uses
    /// this to avoid repeating that scene; the other aftermath conversations have
    /// independent marks. Each completion tests all three marks and advances to 21904100
    /// only when all are set. Yuan's mark also suppresses his actor on re-entry, while
    /// the other two retain shorter repeat replies.
    AltessaAttackAftermathConversationSeen = 151,
    /// At Altessa attack-aftermath story stage 21904000, the Tabatha conversation
    /// (messages 225..230) sets this after its dialogue. Its interaction handler uses
    /// this to avoid repeating that scene; the other aftermath conversations have
    /// independent marks. Each completion tests all three marks and advances to 21904100
    /// only when all are set. Yuan's mark also suppresses his actor on re-entry, while
    /// the other two retain shorter repeat replies.
    TabathaAttackAftermathConversationSeen = 152,
} }

ids! { pub enum Field0152Flag: u16 {
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
} }

ids! { pub enum Field0155Flag: u16 {
    /// Field 155 sets this before entering 156; the destination walks the actor from
    /// (27,-80,-1) to (27,80,-1), clears the bit and restores input.
    Salvation0156EntranceWalkPending = 150,
} }

ids! { pub enum Field0156Flag: u16 {
    EntranceWalkPending = 150,
} }

ids! { pub enum Field0168Flag: u16 {
    /// Interaction 100 toggles the two door-panel translations between zero and
    /// +140/-140 Y, removes/recreates passage collision actor 101 and sets/clears this.
    /// Entry restores the panel layout and blocker. It is a reversible door position,
    /// not a permanent dungeon milestone.
    Door101Open = 150,
    /// Contact on the barrier hit-target group 200..210 calls the removal helper: it
    /// scales visual actor 102 to zero X over 120 ticks, removes collision actor 103 and
    /// the hit-target actors, then sets this bit. Entry omits that barrier while set.
    /// The visual actor need not be despawned for the passage to be clear.
    Barrier102Cleared = 151,
} }

ids! { pub enum Field0169Flag: u16 {
    /// Contact on the barrier hit-target group 200..221 calls the removal helper: it
    /// scales visual actor 102 to zero X over 120 ticks, removes collision actor 103 and
    /// the hit-target actors, then sets this bit. Entry omits that barrier while set.
    /// The visual actor need not be despawned for the passage to be clear.
    Barrier102Cleared = 152,
    /// Contact on the barrier hit-target group 300..310 calls the removal helper: it
    /// scales visual actor 104 to zero X over 120 ticks, removes collision actor 105 and
    /// the hit-target actors, then sets this bit. Entry omits that barrier while set.
    /// The visual actor need not be despawned for the passage to be clear.
    Barrier104Cleared = 153,
    /// The shared 400..419 contact helper sub_0E10 records this on hit 1. It returns
    /// through its common exit after marking a stage, so one contact does not fall
    /// through all stages. A subsequent hit while the second mark is set clears the
    /// barrier and records separate bit 156; the original marks remain set.
    Barrier106And108Hit1Marked = 154,
    /// The shared 400..419 contact helper sub_0E10 records this on hit 2. It returns
    /// through its common exit after marking a stage, so one contact does not fall
    /// through all stages. A subsequent hit while the second mark is set clears the
    /// barrier and records separate bit 156; the original marks remain set.
    Barrier106And108Hit2Marked = 155,
    /// A hit on target group 400..419 while the second-hit marker 155 is set scales
    /// visual actors 106 and 108 to zero X, removes passage collider 107 and the hit
    /// targets, then sets this. Entry restores the absent barrier from 156; hit-history
    /// bits 154/155 remain distinct.
    Barrier106And108Cleared = 156,
} }

ids! { pub enum Field0170Flag: u16 {
    /// Contact on the barrier hit-target group 200..210 calls the removal helper: it
    /// scales visual actor 100 to zero X over 120 ticks, removes collision actor 101 and
    /// the hit-target actors, then sets this bit. Entry omits that barrier while set.
    /// The visual actor need not be despawned for the passage to be clear.
    Barrier100Cleared = 160,
    /// Contact on the barrier hit-target group 300..310 calls the removal helper: it
    /// scales visual actor 102 to zero X over 120 ticks, removes collision actor 103 and
    /// the hit-target actors, then sets this bit. Entry omits that barrier while set.
    /// The visual actor need not be despawned for the passage to be clear.
    Barrier102Cleared = 161,
    /// Contact on the barrier hit-target group 400..410 calls the removal helper: it
    /// scales visual actor 104 to zero X over 120 ticks, removes collision actor 105 and
    /// the hit-target actors, then sets this bit. Entry omits that barrier while set.
    /// The visual actor need not be despawned for the passage to be clear.
    Barrier104Cleared = 162,
} }

ids! { pub enum Field0171Flag: u16 {
    /// Contact with controller target 101 records hit 1, changes the corresponding nodes
    /// on actor 100 and starts its feedback event. Each stage jumps to the common exit
    /// before testing the next, so separate contacts advance the sequence. A third
    /// contact changes the bridge mode via bit 165 rather than overwriting this history.
    Controller101Hit1Marked = 163,
    /// Contact with controller target 101 records hit 2, changes the corresponding nodes
    /// on actor 100 and starts its feedback event. Each stage jumps to the common exit
    /// before testing the next, so separate contacts advance the sequence. A third
    /// contact changes the bridge mode via bit 165 rather than overwriting this history.
    Controller101Hit2Marked = 164,
    /// The third controller-101 contact lowers scene node 0 by 690 Z, removes controller
    /// visual actor 100, restores the alternate scenery and creates its collision
    /// arrangement, then sets this. Entry, tilt helpers and passage-trigger monitors
    /// branch on this persistent bridge mode; the two tilt-state bits remain
    /// independent.
    BridgeLoweredModeEnabled = 165,
    /// This bit contributes 1 to the bridge tilt selector held in 166/167. Before
    /// lowered mode, state 0 reads the original working word at 0x081C for its X
    /// rotation; states 1/2 restore -35/+35 degrees. Keep that working-word read rather
    /// than substituting a literal zero. In lowered mode, states 0/1/2 restore
    /// paired-node X rotations +35/0/+35, with different collision and passage triggers.
    /// Thus the bit is a selector component, not an unconditional positive/negative
    /// angle. Normal tilt/reset helpers set one bit or clear both; retained helper
    /// unused_2B05 also writes this state.
    BridgeTiltStateBit1 = 166,
    /// This bit contributes 2 to the bridge tilt selector held in 166/167. Before
    /// lowered mode, state 0 reads the original working word at 0x081C for its X
    /// rotation; states 1/2 restore -35/+35 degrees. Keep that working-word read rather
    /// than substituting a literal zero. In lowered mode, states 0/1/2 restore
    /// paired-node X rotations +35/0/+35, with different collision and passage triggers.
    /// Thus the bit is a selector component, not an unconditional positive/negative
    /// angle. Normal tilt/reset helpers set one bit or clear both; retained helper
    /// unused_2B05 also writes this state.
    BridgeTiltStateBit2 = 167,
} }

ids! { pub enum Field0193Flag: u16 {
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Colette (ID 2) this is that
    /// team selector. Readers in 193/197 use it as an affinity-candidate mask and for
    /// the corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    ColetteRescueTeamSelected = 156,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Genis (ID 3) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    GenisRescueTeamSelected = 159,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Raine (ID 4) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    RaineRescueTeamSelected = 162,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Zelos (ID 6) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    ZelosRescueTeamSelected = 168,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Presea (ID 7) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    PreseaRescueTeamSelected = 171,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Regal (ID 8) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    RegalRescueTeamSelected = 174,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Kratos (ID 9) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    KratosRescueTeamSelected = 177,
} }

ids! { pub enum Field0197Flag: u16 {
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Colette (ID 2) this is that
    /// team selector. Readers in 193/197 use it as an affinity-candidate mask and for
    /// the corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    ColetteRescueTeamSelected = 156,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Genis (ID 3) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    GenisRescueTeamSelected = 159,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Raine (ID 4) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    RaineRescueTeamSelected = 162,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Zelos (ID 6) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    ZelosRescueTeamSelected = 168,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Presea (ID 7) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    PreseaRescueTeamSelected = 171,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Regal (ID 8) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    RegalRescueTeamSelected = 174,
    /// Iselia room 194 encodes each character at 150 + ID*3. Its UI labels array 0x0880
    /// Reactor Team and 0x0888 Rescue Team (messages 164/166); the encoder clears the
    /// first bit for reactor and sets it for rescue. For Kratos (ID 9) this is that team
    /// selector. Readers in 193/197 use it as an affinity-candidate mask and for the
    /// corresponding party/dialogue staging. The next two bits encode slot 0..3 with
    /// weights 2/1; the decoder skips Sheena (ID 5). This is a packed selection, not
    /// affinity, presence or completed rescue.
    KratosRescueTeamSelected = 177,
} }

ids! { pub enum Field0198Flag: u16 {
    /// Approach trigger 600 sets this immediately before the shared Neil meeting. Helper
    /// sub_0649 reads it to position Lloyd and companions for that approach, clears it,
    /// removes all three approach triggers and enters room 200. It selects scene staging
    /// rather than quest completion.
    NeilMeetingApproach600Pending = 150,
    /// Approach trigger 601 sets this immediately before the shared Neil meeting. Helper
    /// sub_0649 reads it to position Lloyd and companions for that approach, clears it,
    /// removes all three approach triggers and enters room 200. It selects scene staging
    /// rather than quest completion.
    NeilMeetingApproach601Pending = 151,
    /// Approach trigger 602 sets this immediately before the shared Neil meeting. Helper
    /// sub_0649 reads it to position Lloyd and companions for that approach, clears it,
    /// removes all three approach triggers and enters room 200. It selects scene staging
    /// rather than quest completion.
    NeilMeetingApproach602Pending = 152,
    /// Room 200's assault choice sets bit 153. Entry in 198 consumes it for Neil asking
    /// to join, advances the assault route to 1100 and clears the bit. The Dorr route
    /// has a separate marker/global counter.
    NeilAssaultJoinConversationPending = 153,
} }

ids! { pub enum Field0200Flag: u16 {
    /// The attack-the-ranch choice sets the assault-route global to 1000 and bit 153;
    /// room 198 consumes this for Neil's request to accompany the party. It is a pending
    /// follow-up, not a won battle or completed assault.
    NeilAssaultJoinConversationPending = 153,
    /// The return-to-Palmacosta choice sets the Dorr-route global to 1000 and this bit
    /// after the party agrees to question Dorr and leave Neil watching the ranch. No
    /// literal reader in room 200 establishes a later completion meaning.
    DorrReturnChoiceMarked = 154,
} }

ids! { pub enum Field0201Flag: u16 {
    /// The Radar ring-station handler tests this latch and calls the introduction only
    /// while clear. The helper sets it before Lloyd notices the changed surroundings and
    /// Raine explains seeing otherwise invisible things. It marks the introduction being
    /// started, not current radar activation or a persistent visited state.
    RadarIntroductionStarted = 176,
} }

ids! { pub enum Field0202Flag: u16 {
    /// The corresponding path control in 202 creates scene actor 1010 and removes its
    /// blocking collision actors when enabled, then sets bit 172; its reversal removes
    /// the scene actor, reinstalls collision and clears the bit. Entry restores this
    /// layout. The all-paths-off layout uses actor 1013 separately.
    Path1010Enabled = 172,
    /// The corresponding path control in 202 creates scene actor 1011 and removes its
    /// blocking collision actors when enabled, then sets bit 173; its reversal removes
    /// the scene actor, reinstalls collision and clears the bit. Entry restores this
    /// layout. The all-paths-off layout uses actor 1013 separately.
    Path1011Enabled = 173,
    /// The corresponding path control in 202 creates scene actor 1012 and removes its
    /// blocking collision actors when enabled, then sets bit 174; its reversal removes
    /// the scene actor, reinstalls collision and clears the bit. Entry restores this
    /// layout. The all-paths-off layout uses actor 1013 separately.
    Path1012Enabled = 174,
    /// When movable block 1000 reaches (-900,150,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1004 and records bit 177; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1000UpperSocketLocked = 177,
    /// When movable block 1000 reaches (-1050,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1004 and records bit 178; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1000LeftSocketLocked = 178,
    /// When movable block 1000 reaches (-750,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1004 and records bit 179; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1000RightSocketLocked = 179,
    /// When movable block 1000 reaches (-900,-150,0), monitor 5100 replaces it with
    /// fixed collision-enabled actor 1004 and records bit 180; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1000LowerSocketLocked = 180,
    /// When movable block 1001 reaches (-900,150,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1005 and records bit 181; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1001UpperSocketLocked = 181,
    /// When movable block 1001 reaches (-1050,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1005 and records bit 182; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1001LeftSocketLocked = 182,
    /// When movable block 1001 reaches (-750,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1005 and records bit 183; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1001RightSocketLocked = 183,
    /// When movable block 1001 reaches (-900,-150,0), monitor 5100 replaces it with
    /// fixed collision-enabled actor 1005 and records bit 184; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1001LowerSocketLocked = 184,
    /// When movable block 1002 reaches (-900,150,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1006 and records bit 185; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1002UpperSocketLocked = 185,
    /// When movable block 1002 reaches (-1050,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1006 and records bit 186; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1002LeftSocketLocked = 186,
    /// When movable block 1002 reaches (-750,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1006 and records bit 187; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1002RightSocketLocked = 187,
    /// When movable block 1002 reaches (-900,-150,0), monitor 5100 replaces it with
    /// fixed collision-enabled actor 1006 and records bit 188; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1002LowerSocketLocked = 188,
    /// When movable block 1003 reaches (-900,150,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1007 and records bit 189; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1003UpperSocketLocked = 189,
    /// When movable block 1003 reaches (-1050,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1007 and records bit 190; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1003LeftSocketLocked = 190,
    /// When movable block 1003 reaches (-750,0,0), monitor 5100 replaces it with fixed
    /// collision-enabled actor 1007 and records bit 191; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1003RightSocketLocked = 191,
    /// When movable block 1003 reaches (-900,-150,0), monitor 5100 replaces it with
    /// fixed collision-enabled actor 1007 and records bit 192; entry restores that
    /// block/socket combination. Upper/lower and left/right name the local coordinate
    /// cross, not world directions. The original independent tests and per-block bits
    /// are retained rather than normalized into one exclusive selector.
    Block1003LowerSocketLocked = 192,
} }

ids! { pub enum Field0203Flag: u16 {
    /// The radar setup recreates hidden pickup actor 2000 only while this session mark
    /// is clear. Its reward event checks the separate permanent treasure flag
    /// PalmacostaRanch0203AppleGel and the AppleGel stack limit, then sets this mark,
    /// grants one item, removes the target and marks the treasure opened. A full stack
    /// or already-opened treasure does not set this mark. Keep the radar target
    /// suppression separate from inventory and the permanent treasure state.
    AppleGelRadarPickupSuppressed = 164,
    /// The radar setup recreates hidden pickup actor 2001 only while this session mark
    /// is clear. Its reward event checks the separate permanent treasure flag
    /// PalmacostaRanch0203OrangeGelChest235 and the OrangeGel stack limit, then sets
    /// this mark, grants one item, removes the target and marks the treasure opened. A
    /// full stack or already-opened treasure does not set this mark. Keep the radar
    /// target suppression separate from inventory and the permanent treasure state.
    OrangeGel235RadarPickupSuppressed = 165,
    /// The radar setup recreates hidden pickup actor 2002 only while this session mark
    /// is clear. Its reward event checks the separate permanent treasure flag
    /// PalmacostaRanch0203LifeBottle and the LifeBottle stack limit, then sets this
    /// mark, grants one item, removes the target and marks the treasure opened. A full
    /// stack or already-opened treasure does not set this mark. Keep the radar target
    /// suppression separate from inventory and the permanent treasure state.
    LifeBottleRadarPickupSuppressed = 166,
    /// The radar setup recreates hidden pickup actor 2003 only while this session mark
    /// is clear. Its reward event checks the separate permanent treasure flag
    /// PalmacostaRanch0203OrangeGelChest237 and the OrangeGel stack limit, then sets
    /// this mark, grants one item, removes the target and marks the treasure opened. A
    /// full stack or already-opened treasure does not set this mark. Keep the radar
    /// target suppression separate from inventory and the permanent treasure state.
    OrangeGel237RadarPickupSuppressed = 167,
} }

ids! { pub enum Field0205Flag: u16 {
    /// The passage interaction sets this after all three Purple/Red/Blue card
    /// prerequisites are met. Entry and subsequent interactions use it to restore/bypass
    /// the opened passage; missing-card prompts leave it unset. The individual
    /// card-acquired flags remain separate.
    CardLockedPassageOpened = 157,
    /// Retained unused_13C3 creates dummy target actor 2000 and event 5010 only while
    /// this bit is clear. Its interaction removes the actor without a normal item grant.
    /// No literal setter here establishes that a treasure was collected or that this
    /// unused helper runs in normal play.
    Target2000Suppressed = 163,
} }

ids! { pub enum Field0206Flag: u16 {
    /// The radar helper creates target actor 2000 only while this bit is clear and its
    /// local selector matches. That target's interaction starts formation 20 in arena
    /// 79. No literal setter records a defeated ambush here, so the alias describes
    /// suppression rather than battle completion.
    AmbushTarget2000Suppressed = 168,
    /// Room 206 records this only on the successful LifeBottle pickup, alongside its
    /// distinct permanent treasure flag. The radar helper reads it to suppress target
    /// actor 2001; full inventory leaves it clear. It is not an inventory-ownership
    /// check.
    LifeBottleRadarTargetSuppressed = 169,
    /// Room 206 records this only on the successful AppleGel pickup, alongside its
    /// distinct permanent treasure flag. The radar helper reads it to suppress target
    /// actor 2002; full inventory leaves it clear. It is not an inventory-ownership
    /// check.
    AppleGelRadarTargetSuppressed = 170,
    /// The radar helper creates target actor 2003 only while this bit is clear and its
    /// local selector matches. That target's interaction starts formation 20 in arena
    /// 79. No literal setter records a defeated ambush here, so the alias describes
    /// suppression rather than battle completion.
    AmbushTarget2003Suppressed = 171,
    /// Room 206 records this only on the successful OrangeGel pickup, alongside its
    /// distinct permanent treasure flag. The radar helper reads it to suppress target
    /// actor 2004; full inventory leaves it clear. It is not an inventory-ownership
    /// check.
    OrangeGelRadarTargetSuppressed = 193,
    /// The radar helper creates target actor 2005 only while this bit is clear and its
    /// local selector matches. That target's interaction starts formation 20 in arena
    /// 79. No literal setter records a defeated ambush here, so the alias describes
    /// suppression rather than battle completion.
    AmbushTarget2005Suppressed = 194,
    /// Room 206 records this only on the successful MelangeGel pickup, alongside its
    /// distinct permanent treasure flag. The radar helper reads it to suppress target
    /// actor 2006; full inventory leaves it clear. It is not an inventory-ownership
    /// check.
    MelangeGelRadarTargetSuppressed = 195,
    /// The radar helper creates target actor 2007 only while this bit is clear and its
    /// local selector matches. That target's interaction starts formation 20 in arena
    /// 79. No literal setter records a defeated ambush here, so the alias describes
    /// suppression rather than battle completion.
    AmbushTarget2007Suppressed = 196,
} }

ids! { pub enum Field0208Flag: u16 {
    /// Entry in 215 at Asgard progress 3200 marks this. Room 208 uses it with progress
    /// <=3200 to switch from its initial four scenery blocks to the alternate
    /// three-block layout plus movable block 213; later progress also selects the
    /// alternate layout independently. This is a layout selector, not a permanent visit
    /// flag.
    EntranceAlternateBlockLayoutSelected = 185,
} }

ids! { pub enum Field0210Flag: u16 {
    /// Asgard room 215's first exit toward this base displays Lock disengaged and sets
    /// the shared mark. Room 210 uses it for blocker 201 and the confirmed-lock versus
    /// ordinary passage to 215, plus the applicable approach trigger sets in its setup
    /// helpers. These reads do not set or consume it. Preserve the original extra story
    /// prerequisites and keep the message marker separate from ranch completion.
    SylvarantBaseExitLockMessageShown = 189,
} }

ids! { pub enum Field0211Flag: u16 {
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    LloydTeamBit = 153,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    LloydSlotBit2 = 154,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    LloydSlotBit1 = 155,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    ColetteTeamBit = 156,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    ColetteSlotBit2 = 157,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    GenisSlotBit2 = 160,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    RaineTeamBit = 162,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    RaineSlotBit1 = 164,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    SheenaTeamBit = 165,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    SheenaSlotBit2 = 166,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    SheenaSlotBit1 = 167,
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 8. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit8 = 168,
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 1. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit1 = 171,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    RegalTeamBit = 174,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    KratosSlotBit2 = 178,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    KratosSlotBit1 = 179,
} }

ids! { pub enum Field0213Flag: u16 {
    /// Entry offsets scenery door panels 42/43 by +100/-100 when this bit is set. The
    /// three outward passage handlers restore both offsets to zero and clear it. No
    /// literal setter here establishes a separate quest milestone.
    DoorPanels42And43Open = 182,
    /// The ring changer calls the introduction while this bit is clear. The helper sets
    /// it before Lloyd explains that the increased fire can reach farther; it is
    /// separate from the current ring mode and the six target flags.
    LongRangeFireIntroductionStarted = 186,
    /// Long-range fire hitting target 400 hides the corresponding scenery node, disables
    /// ring contact, removes emitter 9100, and sets bit 190. Entry restores the cleared
    /// state. When all six target bits are set, the shared helper removes the remaining
    /// blockers and advances Asgard progress to 3020.
    LongRangeFireTarget400Cleared = 190,
    /// Long-range fire hitting target 401 hides the corresponding scenery node, disables
    /// ring contact, removes emitter 9101, and sets bit 191. Entry restores the cleared
    /// state. When all six target bits are set, the shared helper removes the remaining
    /// blockers and advances Asgard progress to 3020.
    LongRangeFireTarget401Cleared = 191,
    /// Long-range fire hitting target 402 hides the corresponding scenery node, disables
    /// ring contact, removes emitter 9102, and sets bit 192. Entry restores the cleared
    /// state. When all six target bits are set, the shared helper removes the remaining
    /// blockers and advances Asgard progress to 3020.
    LongRangeFireTarget402Cleared = 192,
    /// Long-range fire hitting target 403 hides the corresponding scenery node, disables
    /// ring contact, removes emitter 9103, and sets bit 193. Entry restores the cleared
    /// state. When all six target bits are set, the shared helper removes the remaining
    /// blockers and advances Asgard progress to 3020.
    LongRangeFireTarget403Cleared = 193,
    /// Long-range fire hitting target 404 hides the corresponding scenery node, disables
    /// ring contact, removes emitter 9104, and sets bit 194. Entry restores the cleared
    /// state. When all six target bits are set, the shared helper removes the remaining
    /// blockers and advances Asgard progress to 3020.
    LongRangeFireTarget404Cleared = 194,
    /// Long-range fire hitting target 405 hides the corresponding scenery node, disables
    /// ring contact, removes emitter 9105, and sets bit 195. Entry restores the cleared
    /// state. When all six target bits are set, the shared helper removes the remaining
    /// blockers and advances Asgard progress to 3020.
    LongRangeFireTarget405Cleared = 195,
} }

ids! { pub enum Field0214Flag: u16 {
    /// Asgard room 215 restores the saved field leader from 168..171 with weights
    /// 8/4/2/1. Room 214 saves the selected leader into those shared bits; this bit
    /// contributes 8. Its original low-bit branch sets 172 rather than 171, or clears
    /// 170 rather than 171. Retain that mismatch and the decoder's unchanged use of 171.
    /// The lone write to 172 is named separately for its misdirected save operation;
    /// it is not the decoder's low bit and these names do not repair the save.
    RestoredFieldLeaderBit8 = 168,
    /// Asgard room 215 restores the saved field leader from 168..171 with weights
    /// 8/4/2/1. Room 214 saves the selected leader into those shared bits; this bit
    /// contributes 4. Its original low-bit branch sets 172 rather than 171, or clears
    /// 170 rather than 171. Retain that mismatch and the decoder's unchanged use of 171.
    /// The lone write to 172 is named separately for its misdirected save operation;
    /// it is not the decoder's low bit and these names do not repair the save.
    RestoredFieldLeaderBit4 = 169,
    /// Asgard room 215 restores the saved field leader from 168..171 with weights
    /// 8/4/2/1. Room 214 saves the selected leader into those shared bits; this bit
    /// contributes 2. Its original low-bit branch sets 172 rather than 171, or clears
    /// 170 rather than 171. Retain that mismatch and the decoder's unchanged use of 171.
    /// The lone write to 172 is named separately for its misdirected save operation;
    /// it is not the decoder's low bit and these names do not repair the save.
    RestoredFieldLeaderBit2 = 170,
    /// Room 214 event_2_1771_r3 tests the captured leader ID modulo 2, then sets
    /// 172 on the true path. The false path clears 170 instead, and room 215
    /// restores weight 1 from 171. This alias describes only the observed
    /// misdirected write, not a working low-bit encoding or a corrected address.
    /// No reader or matching clear of 172 is recovered in this team protocol.
    LeaderLowBitSaveMiswrite = 172,
} }

ids! { pub enum Field0215Flag: u16 {
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 8. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit8 = 168,
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 4. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit4 = 169,
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 2. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit2 = 170,
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 1. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit1 = 171,
    /// The left sliding-section helper toggles this bit and its scenery layout. Set
    /// places collision/section actor 3000 at X -607; clear places it at X -1247. Entry
    /// restores the matching position. Inward describes local movement toward the middle
    /// without inferring a world-map direction or treating either position as
    /// completion.
    LeftSlidingSectionInward = 183,
    /// The right sliding-section helper toggles this bit and its scenery layout. Set
    /// places collision/section actor 3001 at X 2840; clear places it at X 3480. Entry
    /// restores the matching position. Inward describes local movement toward the middle
    /// without inferring a world-map direction or treating either position as
    /// completion.
    RightSlidingSectionInward = 184,
    /// Entry in 215 at Asgard progress 3200 marks this. Room 208 uses it with progress
    /// <=3200 to switch from its initial four scenery blocks to the alternate
    /// three-block layout plus movable block 213; later progress also selects the
    /// alternate layout independently. This is a layout selector, not a permanent visit
    /// flag.
    EntranceAlternateBlockLayoutSelected = 185,
    /// The support-team helper for switch record 4000 marks this and checks the other
    /// switch. Dialogue identifies the remaining opposite-side switch; entry restores
    /// its scenery/trigger state. Once both are marked at the applicable stage, the
    /// helpers rejoin the teams and restore the encoded leader.
    LeftSupportSwitchActivated = 187,
    /// The support-team helper for switch record 4003 marks this and checks the other
    /// switch. Dialogue identifies the remaining opposite-side switch; entry restores
    /// its scenery/trigger state. Once both are marked at the applicable stage, the
    /// helpers rejoin the teams and restore the encoded leader.
    RightSupportSwitchActivated = 188,
    /// On the first traversal to Sylvarant Base 210, the handler displays Lock
    /// disengaged, sets this bit and changes field. Entry uses it to choose the
    /// confirmed-lock versus normal passage record; subsequent traversals omit the
    /// message.
    SylvarantBaseExitLockMessageShown = 189,
} }

ids! { pub enum Field0217Flag: u16 {
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    LloydSlotBit1 = 155,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    ColetteTeamBit = 156,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    ColetteSlotBit2 = 157,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    GenisSlotBit2 = 160,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    RaineTeamBit = 162,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    RaineSlotBit1 = 164,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    SheenaTeamBit = 165,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    SheenaSlotBit2 = 166,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    SheenaSlotBit1 = 167,
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 8. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit8 = 168,
    /// Asgard 215 restores the field leader from flags 168..171 with weights 8,4,2,1;
    /// this bit contributes weight 1. The party decoder skips character IDs 6,7,8 whose
    /// triple-bit ranges overlap this storage. Preserve the original debug writes and
    /// bit packing.
    RestoredFieldLeaderBit1 = 171,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    RegalTeamBit = 174,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    KratosSlotBit2 = 178,
    /// The Asgard team protocol uses base 150 + character ID*3: first bit selects team A
    /// (clear) or B (set), followed by slot bits with weights 2 and 1 for slots 1..3.
    /// Initializers/debug presets write this bit; the decoder in 215 populates the
    /// matching team arrays. It skips IDs 6,7,8, so the retained Regal team-bit debug
    /// write does not assign Regal in normal play.
    KratosSlotBit1 = 179,
} }

ids! { pub enum Field0218Flag: u16 {
    /// Colette's oracle-stone entrance scene sets this alongside the separate slab mark
    /// 151. Before story 1302000, entry creates collision blocker 200 only while this is
    /// clear. From story 1302000 the entry instead uses its open-layout override. This
    /// flag suppresses the entrance blocker; it does not replace the independent
    /// slab/exit state or the permanent story stage.
    SealEntranceBlockerSuppressed = 150,
    /// The oracle-stone scene translates scenery part 84 to Y offset -250, creates
    /// passage 6001 to room 219 and sets this alongside blocker mark 150. Before story
    /// 1302000 entry restores offset 0 while clear or -250 plus the passage while set;
    /// later story stages independently restore the open layout. Preserve the two
    /// separate bits and their original horizontal translation.
    SealEntranceSlabRetracted = 151,
} }

ids! { pub enum Field0220Flag: u16 {
    /// Retained region event 5051 sets this, removes its own trigger and installs
    /// boundary 5050; crossing 5050 clears it and reinstalls 5051. The two original
    /// boundaries use their distinct coordinates. The retained ring-contact handler
    /// initializes word 0x08B0 and tests this but has no further game action on either
    /// path. This names the observed boundary latch only, without inventing a live
    /// puzzle effect.
    Boundary5051SideMarked = 152,
} }

ids! { pub enum Field0226Flag: u16 {
    /// Island Ranch 227 restores lower elevator actor 300 at Z 3530 when this bit alone
    /// is selected among 150..152. Its first ascent sets it; returning from 228 sets it
    /// and clears 151/152. The zero selection restores the bottom landing. This is
    /// platform position, not a floor-unlocked milestone.
    LowerElevatorMiddleLandingSelected = 150,
    /// Island Ranch 227 restores lower elevator actor 300 at Z 7870 when this bit alone
    /// is selected among 150..152; the passage level is Z 7970. The ascent after the
    /// revolt and return from room 229 set it and clear 150. The original differing
    /// platform/passage heights remain unchanged.
    LowerElevatorUpperLandingSelected = 151,
} }

ids! { pub enum Field0227Flag: u16 {
    /// Island Ranch 227 restores lower elevator actor 300 at Z 3530 when this bit alone
    /// is selected among 150..152. Its first ascent sets it; returning from 228 sets it
    /// and clears 151/152. The zero selection restores the bottom landing. This is
    /// platform position, not a floor-unlocked milestone.
    LowerElevatorMiddleLandingSelected = 150,
    /// Island Ranch 227 restores lower elevator actor 300 at Z 7870 when this bit alone
    /// is selected among 150..152; the passage level is Z 7970. The ascent after the
    /// revolt and return from room 229 set it and clear 150. The original differing
    /// platform/passage heights remain unchanged.
    LowerElevatorUpperLandingSelected = 151,
    /// All three lower-elevator restoration branches in Island Ranch 227 require this
    /// bit clear. No recovered normal setter establishes a fourth landing; the debug
    /// down-reset and return from room 228 clear it. The alias names only suppression of
    /// actor/layout restoration, not a puzzle completion or reserved floor.
    LowerElevatorRestoreSuppressed = 152,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 10330 minus 100 when this
    /// bit alone is selected among 153..155. Returns from 229/230 select this lower
    /// landing; the ascent clears it in favor of 154. Debug upper reset clears all
    /// three, while its jump preset sets 153.
    UpperElevatorLowerLandingSelected = 153,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 12830 minus 100 when this
    /// bit alone is selected among 153..155. Moving from either adjacent landing selects
    /// it. Room 230 selects it only after all three distinct lock flags are set, before
    /// returning through the middle passage.
    UpperElevatorMiddleLandingSelected = 154,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 23360 minus 100 when this
    /// bit alone is selected among 153..155. The ascent and return from room 232 select
    /// it; descending clears it in favor of 154. It records a landing, not ranch
    /// completion.
    UpperElevatorUpperLandingSelected = 155,
    /// The first lower-elevator ascent records this immediately before the conversation
    /// in which Raine proposes a captive revolt to make the Desians move the elevator
    /// higher. It suppresses repeating that introduction; the actual revolt marker is
    /// 157.
    ElevatorRevoltPlanIntroduced = 156,
    /// After all four room-228 switches are pressed, helper sub_0678 sets this at the
    /// start of the prisoner escape/Desian distraction scene. Room 228 restores the
    /// opened scene layout; room 227 allows the lower elevator ascent to its upper
    /// landing. The bit is set before the cinematic finishes.
    PrisonerRevoltStarted = 157,
    /// The first change to Sound ring mode calls sub_116F, which sets this before Lloyd,
    /// Raine and Genis discuss the machinery resonating with the ring as a key. It gates
    /// the introduction, separately from the actual ring mode.
    SoundRingIntroductionStarted = 170,
} }

ids! { pub enum Field0228Flag: u16 {
    /// Island Ranch 227 restores lower elevator actor 300 at Z 3530 when this bit alone
    /// is selected among 150..152. Its first ascent sets it; returning from 228 sets it
    /// and clears 151/152. The zero selection restores the bottom landing. This is
    /// platform position, not a floor-unlocked milestone.
    LowerElevatorMiddleLandingSelected = 150,
    /// Island Ranch 227 restores lower elevator actor 300 at Z 7870 when this bit alone
    /// is selected among 150..152; the passage level is Z 7970. The ascent after the
    /// revolt and return from room 229 set it and clear 150. The original differing
    /// platform/passage heights remain unchanged.
    LowerElevatorUpperLandingSelected = 151,
    /// All three lower-elevator restoration branches in Island Ranch 227 require this
    /// bit clear. No recovered normal setter establishes a fourth landing; the debug
    /// down-reset and return from room 228 clear it. The alias names only suppression of
    /// actor/layout restoration, not a puzzle completion or reserved floor.
    LowerElevatorRestoreSuppressed = 152,
    /// After all four room-228 switches are pressed, helper sub_0678 sets this at the
    /// start of the prisoner escape/Desian distraction scene. Room 228 restores the
    /// opened scene layout; room 227 allows the lower elevator ascent to its upper
    /// landing. The bit is set before the cinematic finishes.
    PrisonerRevoltStarted = 157,
    /// Accepting the Push the switch prompt for actor 300 sets bit 160, changes that
    /// actor's node visibility and reduces its radius from 100 to 30. Entry restores it.
    /// Each interaction checks all four switch bits and starts the prisoner revolt only
    /// after all are set; declining leaves the bit clear.
    Switch300Pressed = 160,
    /// Accepting the Push the switch prompt for actor 301 sets bit 161, changes that
    /// actor's node visibility and reduces its radius from 100 to 30. Entry restores it.
    /// Each interaction checks all four switch bits and starts the prisoner revolt only
    /// after all are set; declining leaves the bit clear.
    Switch301Pressed = 161,
    /// Accepting the Push the switch prompt for actor 302 sets bit 162, changes that
    /// actor's node visibility and reduces its radius from 100 to 30. Entry restores it.
    /// Each interaction checks all four switch bits and starts the prisoner revolt only
    /// after all are set; declining leaves the bit clear.
    Switch302Pressed = 162,
    /// Accepting the Push the switch prompt for actor 303 sets bit 163, changes that
    /// actor's node visibility and reduces its radius from 100 to 30. Entry restores it.
    /// Each interaction checks all four switch bits and starts the prisoner revolt only
    /// after all are set; declining leaves the bit clear.
    Switch303Pressed = 163,
} }

ids! { pub enum Field0229Flag: u16 {
    /// Island Ranch 227 restores lower elevator actor 300 at Z 3530 when this bit alone
    /// is selected among 150..152. Its first ascent sets it; returning from 228 sets it
    /// and clears 151/152. The zero selection restores the bottom landing. This is
    /// platform position, not a floor-unlocked milestone.
    LowerElevatorMiddleLandingSelected = 150,
    /// Island Ranch 227 restores lower elevator actor 300 at Z 7870 when this bit alone
    /// is selected among 150..152; the passage level is Z 7970. The ascent after the
    /// revolt and return from room 229 set it and clear 150. The original differing
    /// platform/passage heights remain unchanged.
    LowerElevatorUpperLandingSelected = 151,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 10330 minus 100 when this
    /// bit alone is selected among 153..155. Returns from 229/230 select this lower
    /// landing; the ascent clears it in favor of 154. Debug upper reset clears all
    /// three, while its jump preset sets 153.
    UpperElevatorLowerLandingSelected = 153,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 12830 minus 100 when this
    /// bit alone is selected among 153..155. Moving from either adjacent landing selects
    /// it. Room 230 selects it only after all three distinct lock flags are set, before
    /// returning through the middle passage.
    UpperElevatorMiddleLandingSelected = 154,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 23360 minus 100 when this
    /// bit alone is selected among 153..155. The ascent and return from room 232 select
    /// it; descending clears it in favor of 154. It records a landing, not ranch
    /// completion.
    UpperElevatorUpperLandingSelected = 155,
} }

ids! { pub enum Field0230Flag: u16 {
    /// Island Ranch 227 restores upper elevator actor 301 at Z 10330 minus 100 when this
    /// bit alone is selected among 153..155. Returns from 229/230 select this lower
    /// landing; the ascent clears it in favor of 154. Debug upper reset clears all
    /// three, while its jump preset sets 153.
    UpperElevatorLowerLandingSelected = 153,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 12830 minus 100 when this
    /// bit alone is selected among 153..155. Moving from either adjacent landing selects
    /// it. Room 230 selects it only after all three distinct lock flags are set, before
    /// returning through the middle passage.
    UpperElevatorMiddleLandingSelected = 154,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 23360 minus 100 when this
    /// bit alone is selected among 153..155. The ascent and return from room 232 select
    /// it; descending clears it in favor of 154. It records a landing, not ranch
    /// completion.
    UpperElevatorUpperLandingSelected = 155,
} }

ids! { pub enum Field0232Flag: u16 {
    /// Island Ranch 227 restores upper elevator actor 301 at Z 10330 minus 100 when this
    /// bit alone is selected among 153..155. Returns from 229/230 select this lower
    /// landing; the ascent clears it in favor of 154. Debug upper reset clears all
    /// three, while its jump preset sets 153.
    UpperElevatorLowerLandingSelected = 153,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 12830 minus 100 when this
    /// bit alone is selected among 153..155. Moving from either adjacent landing selects
    /// it. Room 230 selects it only after all three distinct lock flags are set, before
    /// returning through the middle passage.
    UpperElevatorMiddleLandingSelected = 154,
    /// Island Ranch 227 restores upper elevator actor 301 at Z 23360 minus 100 when this
    /// bit alone is selected among 153..155. The ascent and return from room 232 select
    /// it; descending clears it in favor of 154. It records a landing, not ranch
    /// completion.
    UpperElevatorUpperLandingSelected = 155,
} }

ids! { pub enum Field0236Flag: u16 {
    /// Earth room 237's blocked-path scene has Lloyd say they cannot get past and Raine
    /// suggest another way around, then sets this and removes trigger 1501. Entry
    /// recreates that trigger only while clear and before quest step 22000. Room 236's
    /// Gnomelette-3 dialogue tests the same mark for its chasm-specific reply. It
    /// records noticing the detour, not collapsing the path or reaching Gnome.
    ChasmDetourNoticed = 150,
} }

ids! { pub enum Field0237Flag: u16 {
    /// Earth room 237's blocked-path scene has Lloyd say they cannot get past and Raine
    /// suggest another way around, then sets this and removes trigger 1501. Entry
    /// recreates that trigger only while clear and before quest step 22000. Room 236's
    /// Gnomelette-3 dialogue tests the same mark for its chasm-specific reply. It
    /// records noticing the detour, not collapsing the path or reaching Gnome.
    ChasmDetourNoticed = 150,
    /// Gnomelette 3's synchronized-earthquake helper sets this before checking the ring
    /// mode. With Earthquake selected, its retry loop requires the quest step not 24000
    /// and this bit set. The successful collapse aftermath and declining Want to keep
    /// going clear it. The wrong-ring branch returns without clearing it, so this is
    /// permission for the challenge loop rather than proof that a timed attempt is
    /// currently running or completed.
    GnomeletteEarthquakeRetryEnabled = 199,
} }

ids! { pub enum Field0238Flag: u16 {
    /// The overworld entry for landmark 45 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 45. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Triet Desert and setting that region's separate
    /// permanent mode flag(s). Entry/exit do not consume this selector, and the original
    /// independent tests and duplicate registry keys remain. It is not a permanent
    /// region-unlocked flag.
    TrietDesertStoneMarkerSelected = 151,
    /// The overworld entry for landmark 46 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 46. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Ossa Mountains and Izoold and setting that
    /// region's separate permanent mode flag(s). Entry/exit do not consume this
    /// selector, and the original independent tests and duplicate registry keys remain.
    /// It is not a permanent region-unlocked flag.
    OssaIzooldStoneMarkerSelected = 152,
    /// The overworld entry for landmark 47 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 47. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Thoda and eastern Palmacosta and setting that
    /// region's separate permanent mode flag(s). Entry/exit do not consume this
    /// selector, and the original independent tests and duplicate registry keys remain.
    /// It is not a permanent region-unlocked flag.
    ThodaPalmacostaEastStoneMarkerSelected = 153,
    /// The overworld entry for landmark 48 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 48. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Hakonesia Peak and northeastern Palmacosta and
    /// setting that region's separate permanent mode flag(s). Entry/exit do not consume
    /// this selector, and the original independent tests and duplicate registry keys
    /// remain. It is not a permanent region-unlocked flag.
    HakonesiaPalmacostaNortheastStoneMarkerSelected = 154,
    /// The overworld entry for landmark 49 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 49. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Luin and Asgard and setting that region's separate
    /// permanent mode flag(s). Entry/exit do not consume this selector, and the original
    /// independent tests and duplicate registry keys remain. It is not a permanent
    /// region-unlocked flag.
    LuinAsgardStoneMarkerSelected = 155,
    /// The overworld entry for landmark 50 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 50. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Katz Island and setting that region's separate
    /// permanent mode flag(s). Entry/exit do not consume this selector, and the original
    /// independent tests and duplicate registry keys remain. It is not a permanent
    /// region-unlocked flag.
    KatzIslandStoneMarkerSelected = 156,
    /// The overworld entry for landmark 51 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 51. The stone interaction also tests this selection before
    /// announcing Long-range Mode for Hima and western Luin and setting that region's
    /// separate permanent mode flag(s). Entry/exit do not consume this selector, and the
    /// original independent tests and duplicate registry keys remain. It is not a
    /// permanent region-unlocked flag.
    HimaLuinWestStoneMarkerSelected = 157,
    /// The overworld entry for landmark 52 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 52. This value has no Long-range Mode unlock branch in the
    /// stone interaction; the alias describes its observed return destination only.
    /// Entry/exit do not consume this selector, and the original independent tests and
    /// duplicate registry keys remain. It is not a permanent region-unlocked flag.
    Landmark52ReturnSelected = 158,
    /// The overworld entry for landmark 62 sets this before changing to shared field
    /// 238. Both retained exit events with key 1000 independently test it and return to
    /// overworld landmark 62. This value has no Long-range Mode unlock branch in the
    /// stone interaction; the alias describes its observed return destination only.
    /// Entry/exit do not consume this selector, and the original independent tests and
    /// duplicate registry keys remain. It is not a permanent region-unlocked flag.
    ExSkillTutorialReturnSelected = 159,
} }

ids! { pub enum Field0239Flag: u16 {
    /// Selector for HouseOfSalvation0083 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0083Selected = 194,
    /// Selector for HotSpring0089 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHotSpring0089Selected = 196,
    /// Selector for HouseOfGuidance0091 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0091Selected = 197,
} }

ids! { pub enum Field0243Flag: u16 {
    /// Selector for HouseOfSalvation0081 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfSalvation0081Selected = 192,
    /// Selector for HouseOfGuidance0093 in the shared exterior/interior/caravan group;
    /// entering another exterior clears it.
    ExteriorHouseOfGuidance0093Selected = 199,
} }

ids! { pub enum Field0250Flag: u16 {
    /// At story 1304000 the camp interaction with Colette plays its first conversation
    /// about the broken birthday present and her loss of appetite only while this is
    /// clear, then marks it. Entry clears all four conversation marks at that stage,
    /// without clearing the shared affinity latch 164. Colette's repeat starts the next
    /// scene and advances the story; the other companions retain shorter repeat replies.
    /// This is the current camp conversation state, not a global story-completion flag.
    ColetteBirthdayGiftConversationSeen = 160,
    /// At story 1304000 the camp interaction with Genis plays its first conversation
    /// about the tofu curry and Raine's cooking only while this is clear, then marks it.
    /// Entry clears all four conversation marks at that stage, without clearing the
    /// shared affinity latch 164. Colette's repeat starts the next scene and advances
    /// the story; the other companions retain shorter repeat replies. This is the
    /// current camp conversation state, not a global story-completion flag.
    GenisTofuCurryConversationSeen = 161,
    /// At story 1304000 the camp interaction with Kratos plays its first conversation
    /// about Exspheres and Lloyd's parents only while this is clear, then marks it.
    /// Entry clears all four conversation marks at that stage, without clearing the
    /// shared affinity latch 164. Colette's repeat starts the next scene and advances
    /// the story; the other companions retain shorter repeat replies. This is the
    /// current camp conversation state, not a global story-completion flag.
    KratosExsphereAndParentsConversationSeen = 162,
    /// At story 1304000 the camp interaction with Raine plays its first conversation
    /// about Colette's transformation into an angel only while this is clear, then marks
    /// it. Entry clears all four conversation marks at that stage, without clearing the
    /// shared affinity latch 164. Colette's repeat starts the next scene and advances
    /// the story; the other companions retain shorter repeat replies. This is the
    /// current camp conversation state, not a global story-completion flag.
    RaineAngelTransformationConversationSeen = 163,
    /// One shared latch gates the camp's first affinity adjustment: Colette +60; Genis
    /// choice 1 +60 or choice 2 -60; Kratos choice 1 +60 or choice 2 +120; Raine choice
    /// 1 +60. Raine choice 2 neither adjusts affinity nor claims the latch. The four
    /// individual conversation marks are independent, and entry clears those at story
    /// 1304000 without clearing this latch. Preserve the negative adjustment and the
    /// branch that leaves eligibility available; this is not an unconditional
    /// first-conversation bonus.
    CampConversationAffinityAdjustmentClaimed = 164,
} }

ids! { pub enum Field0252Flag: u16 {
    /// After defeating the pass-code guard in room 261, this bit records THE POWER OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodePowerOfAcquired = 170,
    /// After defeating the pass-code guard in room 261, this bit records MANA IS when
    /// that guard is defeated 2nd. The three guards choose their supplied phrase from
    /// the previously acquired groups; the password depends on defeat order. Orochi in
    /// 252 repeats the exact phrase from this bit, and terminal 254 compares it with the
    /// separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeManaIsAcquired = 171,
    /// After defeating the pass-code guard in room 261, this bit records THE WHISPERING
    /// OF THE GIANT TREE when that guard is defeated 3rd. The three guards choose
    /// their supplied phrase from the previously acquired groups; the password depends
    /// on defeat order. Orochi in 252 repeats the exact phrase from this bit, and
    /// terminal 254 compares it with the separate current-selection bits 180..188. A
    /// phrase acquired is not a phrase selected or a door unlocked.
    PassCodeWhisperingOfGiantTreeAcquired = 172,
    /// After defeating the pass-code guard in room 264, this bit records THE LIFE OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLifeOfAcquired = 173,
    /// After defeating the pass-code guard in room 264, this bit records THE SEED IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeSeedIsAcquired = 174,
    /// After defeating the pass-code guard in room 264, this bit records THE RADIANCE OF
    /// LIFE when that guard is defeated 3rd. The three guards choose their supplied
    /// phrase from the previously acquired groups; the password depends on defeat order.
    /// Orochi in 252 repeats the exact phrase from this bit, and terminal 254 compares
    /// it with the separate current-selection bits 180..188. A phrase acquired is not a
    /// phrase selected or a door unlocked.
    PassCodeRadianceOfLifeAcquired = 175,
    /// After defeating the pass-code guard in room 253, this bit records THE LIGHT OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLightOfAcquired = 176,
    /// After defeating the pass-code guard in room 253, this bit records THE STAR IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeStarIsAcquired = 177,
    /// After defeating the pass-code guard in room 253, this bit records THE FOUNDATION
    /// OF THE WORLD when that guard is defeated 3rd. The three guards choose their
    /// supplied phrase from the previously acquired groups; the password depends on
    /// defeat order. Orochi in 252 repeats the exact phrase from this bit, and terminal
    /// 254 compares it with the separate current-selection bits 180..188. A phrase
    /// acquired is not a phrase selected or a door unlocked.
    PassCodeFoundationOfWorldAcquired = 178,
} }

ids! { pub enum Field0253Flag: u16 {
    /// After defeating the pass-code guard in room 261, this bit records THE POWER OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodePowerOfAcquired = 170,
    /// After defeating the pass-code guard in room 261, this bit records MANA IS when
    /// that guard is defeated 2nd. The three guards choose their supplied phrase from
    /// the previously acquired groups; the password depends on defeat order. Orochi in
    /// 252 repeats the exact phrase from this bit, and terminal 254 compares it with the
    /// separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeManaIsAcquired = 171,
    /// After defeating the pass-code guard in room 261, this bit records THE WHISPERING
    /// OF THE GIANT TREE when that guard is defeated 3rd. The three guards choose
    /// their supplied phrase from the previously acquired groups; the password depends
    /// on defeat order. Orochi in 252 repeats the exact phrase from this bit, and
    /// terminal 254 compares it with the separate current-selection bits 180..188. A
    /// phrase acquired is not a phrase selected or a door unlocked.
    PassCodeWhisperingOfGiantTreeAcquired = 172,
    /// After defeating the pass-code guard in room 264, this bit records THE LIFE OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLifeOfAcquired = 173,
    /// After defeating the pass-code guard in room 264, this bit records THE SEED IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeSeedIsAcquired = 174,
    /// After defeating the pass-code guard in room 264, this bit records THE RADIANCE OF
    /// LIFE when that guard is defeated 3rd. The three guards choose their supplied
    /// phrase from the previously acquired groups; the password depends on defeat order.
    /// Orochi in 252 repeats the exact phrase from this bit, and terminal 254 compares
    /// it with the separate current-selection bits 180..188. A phrase acquired is not a
    /// phrase selected or a door unlocked.
    PassCodeRadianceOfLifeAcquired = 175,
    /// After defeating the pass-code guard in room 253, this bit records THE LIGHT OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLightOfAcquired = 176,
    /// After defeating the pass-code guard in room 253, this bit records THE STAR IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeStarIsAcquired = 177,
    /// After defeating the pass-code guard in room 253, this bit records THE FOUNDATION
    /// OF THE WORLD when that guard is defeated 3rd. The three guards choose their
    /// supplied phrase from the previously acquired groups; the password depends on
    /// defeat order. Orochi in 252 repeats the exact phrase from this bit, and terminal
    /// 254 compares it with the separate current-selection bits 180..188. A phrase
    /// acquired is not a phrase selected or a door unlocked.
    PassCodeFoundationOfWorldAcquired = 178,
} }

ids! {
    /// Known uses of shared session bits in Tethe'alla Base 0254.
    /// Unidentified bits 152 and 153 occur in the All flags ON/OFF debug choices.
    /// No gameplay reader has been recovered in this image; they remain numeric.
    pub enum Field0254Flag: u16 {
    /// The room-254 lift helper records the upper landing selection for lift actor 150
    /// before moving it. Its two passenger levels are Z 2560 and 5860; the collision
    /// actor is placed 100 below the selected level. Entry restores that position and
    /// the corresponding scenery translation. Set selects the upper landing and clear
    /// selects the lower landing.
    Lift150UpperLandingSelected = 160,
    /// The room-254 lift helper records the lower landing selection for lift actor 151
    /// before moving it. Its two passenger levels are Z 460 and 2560; the collision
    /// actor is placed 100 below the selected level. Entry restores that position and
    /// the corresponding scenery translation. This lift has inverted polarity: set
    /// selects the lower landing and clear selects the upper landing. Its operation also
    /// requires the distinct orb-target-45 flag.
    Lift151LowerLandingSelected = 161,
    /// The room-254 lift helper records the upper landing selection for lift actor 152
    /// before moving it. Its two passenger levels are Z 10 and 460; the collision actor
    /// is placed 100 below the selected level. Entry restores that position and the
    /// corresponding scenery translation. Set selects the upper landing and clear
    /// selects the lower landing. Preserve the original mismatch: entry translates the
    /// scenery node by 460, while the raising helper translates it by 450; both place
    /// the lift collision at Z 360. Its helper also checks story stage 12203100 before
    /// allowing operation.
    Lift152UpperLandingSelected = 162,
    /// The room-254 lift helper records the upper landing selection for lift actor 153
    /// before moving it. Its two passenger levels are Z 10 and 2560; the collision actor
    /// is placed 100 below the selected level. Entry restores that position and the
    /// corresponding scenery translation. Set selects the upper landing and clear
    /// selects the lower landing. Its helper also checks story stage 12203200 before
    /// allowing operation.
    Lift153UpperLandingSelected = 163,
    /// After defeating the pass-code guard in room 261, this bit records THE POWER OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodePowerOfAcquired = 170,
    /// After defeating the pass-code guard in room 261, this bit records MANA IS when
    /// that guard is defeated 2nd. The three guards choose their supplied phrase from
    /// the previously acquired groups; the password depends on defeat order. Orochi in
    /// 252 repeats the exact phrase from this bit, and terminal 254 compares it with the
    /// separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeManaIsAcquired = 171,
    /// After defeating the pass-code guard in room 261, this bit records THE WHISPERING
    /// OF THE GIANT TREE when that guard is defeated 3rd. The three guards choose
    /// their supplied phrase from the previously acquired groups; the password depends
    /// on defeat order. Orochi in 252 repeats the exact phrase from this bit, and
    /// terminal 254 compares it with the separate current-selection bits 180..188. A
    /// phrase acquired is not a phrase selected or a door unlocked.
    PassCodeWhisperingOfGiantTreeAcquired = 172,
    /// After defeating the pass-code guard in room 264, this bit records THE LIFE OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLifeOfAcquired = 173,
    /// After defeating the pass-code guard in room 264, this bit records THE SEED IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeSeedIsAcquired = 174,
    /// After defeating the pass-code guard in room 264, this bit records THE RADIANCE OF
    /// LIFE when that guard is defeated 3rd. The three guards choose their supplied
    /// phrase from the previously acquired groups; the password depends on defeat order.
    /// Orochi in 252 repeats the exact phrase from this bit, and terminal 254 compares
    /// it with the separate current-selection bits 180..188. A phrase acquired is not a
    /// phrase selected or a door unlocked.
    PassCodeRadianceOfLifeAcquired = 175,
    /// After defeating the pass-code guard in room 253, this bit records THE LIGHT OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLightOfAcquired = 176,
    /// After defeating the pass-code guard in room 253, this bit records THE STAR IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeStarIsAcquired = 177,
    /// After defeating the pass-code guard in room 253, this bit records THE FOUNDATION
    /// OF THE WORLD when that guard is defeated 3rd. The three guards choose their
    /// supplied phrase from the previously acquired groups; the password depends on
    /// defeat order. Orochi in 252 repeats the exact phrase from this bit, and terminal
    /// 254 compares it with the separate current-selection bits 180..188. A phrase
    /// acquired is not a phrase selected or a door unlocked.
    PassCodeFoundationOfWorldAcquired = 178,
    /// Terminal 220 records this for the 1st pass-code prompt when the player chooses
    /// THE POWER OF. Validation compares the selection with acquired phrase bit 170, not
    /// a fixed room/order answer. Success, incorrect-code and cancellation paths clear
    /// all selection bits 180..188. The original independent flag tests and control flow
    /// remain intact.
    PassCodePowerOfSelected = 180,
    /// Terminal 220 records this for the 1st pass-code prompt when the player chooses
    /// THE LIFE OF. Validation compares the selection with acquired phrase bit 173, not
    /// a fixed room/order answer. Success, incorrect-code and cancellation paths clear
    /// all selection bits 180..188. The original independent flag tests and control flow
    /// remain intact.
    PassCodeLifeOfSelected = 181,
    /// Terminal 220 records this for the 1st pass-code prompt when the player chooses
    /// THE LIGHT OF. Validation compares the selection with acquired phrase bit 176, not
    /// a fixed room/order answer. Success, incorrect-code and cancellation paths clear
    /// all selection bits 180..188. The original independent flag tests and control flow
    /// remain intact.
    PassCodeLightOfSelected = 182,
    /// Terminal 220 records this for the 2nd pass-code prompt when the player chooses
    /// MANA IS. Validation compares the selection with acquired phrase bit 171, not a
    /// fixed room/order answer. Success, incorrect-code and cancellation paths clear all
    /// selection bits 180..188. The original independent flag tests and control flow
    /// remain intact.
    PassCodeManaIsSelected = 183,
    /// Terminal 220 records this for the 2nd pass-code prompt when the player chooses
    /// THE SEED IS. Validation compares the selection with acquired phrase bit 174, not
    /// a fixed room/order answer. Success, incorrect-code and cancellation paths clear
    /// all selection bits 180..188. The original independent flag tests and control flow
    /// remain intact.
    PassCodeSeedIsSelected = 184,
    /// Terminal 220 records this for the 2nd pass-code prompt when the player chooses
    /// THE STAR IS. Validation compares the selection with acquired phrase bit 177, not
    /// a fixed room/order answer. Success, incorrect-code and cancellation paths clear
    /// all selection bits 180..188. The original independent flag tests and control flow
    /// remain intact.
    PassCodeStarIsSelected = 185,
    /// Terminal 220 records this for the 3rd pass-code prompt when the player chooses
    /// THE WHISPERING OF THE GIANT TREE. Validation compares the selection with acquired
    /// phrase bit 172, not a fixed room/order answer. Success, incorrect-code and
    /// cancellation paths clear all selection bits 180..188. The original independent
    /// flag tests and control flow remain intact.
    PassCodeWhisperingOfGiantTreeSelected = 186,
    /// Terminal 220 records this for the 3rd pass-code prompt when the player chooses
    /// THE RADIANCE OF LIFE. Validation compares the selection with acquired phrase bit
    /// 175, not a fixed room/order answer. Success, incorrect-code and cancellation
    /// paths clear all selection bits 180..188. The original independent flag tests and
    /// control flow remain intact.
    PassCodeRadianceOfLifeSelected = 187,
    /// Terminal 220 records this for the 3rd pass-code prompt when the player chooses
    /// THE FOUNDATION OF THE WORLD. Validation compares the selection with acquired
    /// phrase bit 178, not a fixed room/order answer. Success, incorrect-code and
    /// cancellation paths clear all selection bits 180..188. The original independent
    /// flag tests and control flow remain intact.
    PassCodeFoundationOfWorldSelected = 188,
    /// The terminal sets this only on the successful pass-code path, after clearing the
    /// nine current-selection bits and immediately before removing blocker 230 and
    /// sliding the two door panels by -100/+100 X. Entry restores the opened panels and
    /// omits the blocker. It is distinct from merely acquiring all three code phrases.
    PassCodeDoorUnlockStarted = 189,
} }

ids! { pub enum Field0257Flag: u16 {
    /// The passage from room 257 sets this immediately before entering block room 265.
    /// Both room-265 exits clear it along with the other entry marker. Room 265 reads it
    /// to restore block actor 200 at (975,20,0) when set, versus (-975,920,0) when
    /// clear.
    BlockRoomEntryFrom0257Marked = 155,
} }

ids! { pub enum Field0260Flag: u16 {
    /// The passage from room 260 sets this immediately before entering block room 265.
    /// Both room-265 exits clear it along with the other entry marker. No recovered
    /// reader uses bit 156 to select a second layout; the alias records only its
    /// explicit entry provenance, not an unproven camera or puzzle effect.
    BlockRoomEntryFrom0260Marked = 156,
} }

ids! { pub enum Field0261Flag: u16 {
    /// After defeating the pass-code guard in room 261, this bit records THE POWER OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodePowerOfAcquired = 170,
    /// After defeating the pass-code guard in room 261, this bit records MANA IS when
    /// that guard is defeated 2nd. The three guards choose their supplied phrase from
    /// the previously acquired groups; the password depends on defeat order. Orochi in
    /// 252 repeats the exact phrase from this bit, and terminal 254 compares it with the
    /// separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeManaIsAcquired = 171,
    /// After defeating the pass-code guard in room 261, this bit records THE WHISPERING
    /// OF THE GIANT TREE when that guard is defeated 3rd. The three guards choose
    /// their supplied phrase from the previously acquired groups; the password depends
    /// on defeat order. Orochi in 252 repeats the exact phrase from this bit, and
    /// terminal 254 compares it with the separate current-selection bits 180..188. A
    /// phrase acquired is not a phrase selected or a door unlocked.
    PassCodeWhisperingOfGiantTreeAcquired = 172,
    /// After defeating the pass-code guard in room 264, this bit records THE LIFE OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLifeOfAcquired = 173,
    /// After defeating the pass-code guard in room 264, this bit records THE SEED IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeSeedIsAcquired = 174,
    /// After defeating the pass-code guard in room 264, this bit records THE RADIANCE OF
    /// LIFE when that guard is defeated 3rd. The three guards choose their supplied
    /// phrase from the previously acquired groups; the password depends on defeat order.
    /// Orochi in 252 repeats the exact phrase from this bit, and terminal 254 compares
    /// it with the separate current-selection bits 180..188. A phrase acquired is not a
    /// phrase selected or a door unlocked.
    PassCodeRadianceOfLifeAcquired = 175,
    /// After defeating the pass-code guard in room 253, this bit records THE LIGHT OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLightOfAcquired = 176,
    /// After defeating the pass-code guard in room 253, this bit records THE STAR IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeStarIsAcquired = 177,
    /// After defeating the pass-code guard in room 253, this bit records THE FOUNDATION
    /// OF THE WORLD when that guard is defeated 3rd. The three guards choose their
    /// supplied phrase from the previously acquired groups; the password depends on
    /// defeat order. Orochi in 252 repeats the exact phrase from this bit, and terminal
    /// 254 compares it with the separate current-selection bits 180..188. A phrase
    /// acquired is not a phrase selected or a door unlocked.
    PassCodeFoundationOfWorldAcquired = 178,
} }

ids! { pub enum Field0264Flag: u16 {
    /// After defeating the pass-code guard in room 261, this bit records THE POWER OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodePowerOfAcquired = 170,
    /// After defeating the pass-code guard in room 261, this bit records MANA IS when
    /// that guard is defeated 2nd. The three guards choose their supplied phrase from
    /// the previously acquired groups; the password depends on defeat order. Orochi in
    /// 252 repeats the exact phrase from this bit, and terminal 254 compares it with the
    /// separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeManaIsAcquired = 171,
    /// After defeating the pass-code guard in room 261, this bit records THE WHISPERING
    /// OF THE GIANT TREE when that guard is defeated 3rd. The three guards choose
    /// their supplied phrase from the previously acquired groups; the password depends
    /// on defeat order. Orochi in 252 repeats the exact phrase from this bit, and
    /// terminal 254 compares it with the separate current-selection bits 180..188. A
    /// phrase acquired is not a phrase selected or a door unlocked.
    PassCodeWhisperingOfGiantTreeAcquired = 172,
    /// After defeating the pass-code guard in room 264, this bit records THE LIFE OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLifeOfAcquired = 173,
    /// After defeating the pass-code guard in room 264, this bit records THE SEED IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeSeedIsAcquired = 174,
    /// After defeating the pass-code guard in room 264, this bit records THE RADIANCE OF
    /// LIFE when that guard is defeated 3rd. The three guards choose their supplied
    /// phrase from the previously acquired groups; the password depends on defeat order.
    /// Orochi in 252 repeats the exact phrase from this bit, and terminal 254 compares
    /// it with the separate current-selection bits 180..188. A phrase acquired is not a
    /// phrase selected or a door unlocked.
    PassCodeRadianceOfLifeAcquired = 175,
    /// After defeating the pass-code guard in room 253, this bit records THE LIGHT OF
    /// when that guard is defeated 1st. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeLightOfAcquired = 176,
    /// After defeating the pass-code guard in room 253, this bit records THE STAR IS
    /// when that guard is defeated 2nd. The three guards choose their supplied phrase
    /// from the previously acquired groups; the password depends on defeat order. Orochi
    /// in 252 repeats the exact phrase from this bit, and terminal 254 compares it with
    /// the separate current-selection bits 180..188. A phrase acquired is not a phrase
    /// selected or a door unlocked.
    PassCodeStarIsAcquired = 177,
    /// After defeating the pass-code guard in room 253, this bit records THE FOUNDATION
    /// OF THE WORLD when that guard is defeated 3rd. The three guards choose their
    /// supplied phrase from the previously acquired groups; the password depends on
    /// defeat order. Orochi in 252 repeats the exact phrase from this bit, and terminal
    /// 254 compares it with the separate current-selection bits 180..188. A phrase
    /// acquired is not a phrase selected or a door unlocked.
    PassCodeFoundationOfWorldAcquired = 178,
} }

ids! { pub enum Field0265Flag: u16 {
    /// The passage from room 257 sets this immediately before entering block room 265.
    /// Both room-265 exits clear it along with the other entry marker. Room 265 reads it
    /// to restore block actor 200 at (975,20,0) when set, versus (-975,920,0) when
    /// clear.
    BlockRoomEntryFrom0257Marked = 155,
    /// The passage from room 260 sets this immediately before entering block room 265.
    /// Both room-265 exits clear it along with the other entry marker. No recovered
    /// reader uses bit 156 to select a second layout; the alias records only its
    /// explicit entry provenance, not an unproven camera or puzzle effect.
    BlockRoomEntryFrom0260Marked = 156,
} }

ids! { pub enum Field0267Flag: u16 {
    /// Before the electric-orb passage is enabled, room 268's battle handler on result 1
    /// sets this and returns Lloyd to room 267 at (2252,1082,0). Room 267 entry calls
    /// the dialogue I'd better avoid fighting alone, then clears the request. This is a
    /// pending reminder, separate from the result-3 recapture that resets the story and
    /// cell door.
    AvoidFightingAloneReminderPending = 153,
    /// Room 268's guard puzzle requires both pad counters above 2 with the Electric Orb
    /// ring. Its solution sets this before the opening animation, disables the guards
    /// temporarily, removes blocker 200 and creates passage 7010 to room 269. While set
    /// it also selects ordinary battle handling instead of solo recapture logic and
    /// enables that passage/camera layout before story 1105200. Entries in 267/269
    /// consume it only at story 1105100, advancing to 1105200; later layout access
    /// follows story progression independently.
    ElectricOrbPassageEnabled = 154,
    /// Prison room 267 restores cell-door scenery track 0 when this is set; while clear
    /// it creates interaction 300 and collision blocker 200. That interaction opens the
    /// cell, sets this mark and removes the interaction/blocker. The three doors remain
    /// independent. Hitting the guard during Lloyd's escape opens this first door too.
    /// Room 268's result-3 recapture resets the story to 1101000, restores the Fire ring
    /// and clears only this cell-door mark before returning Lloyd to his cell.
    Cell300Open = 173,
    /// Prison room 267 restores cell-door scenery track 1 when this is set; while clear
    /// it creates interaction 301 and collision blocker 202. That interaction opens the
    /// cell, sets this mark and removes the interaction/blocker. The three doors remain
    /// independent.
    Cell301Open = 174,
    /// Prison room 267 restores cell-door scenery track 2 when this is set; while clear
    /// it creates interaction 302 and collision blocker 204. That interaction opens the
    /// cell, sets this mark and removes the interaction/blocker. The three doors remain
    /// independent.
    Cell302Open = 175,
} }

ids! { pub enum Field0268Flag: u16 {
    /// Before the electric-orb passage is enabled, room 268's battle handler on result 1
    /// sets this and returns Lloyd to room 267 at (2252,1082,0). Room 267 entry calls
    /// the dialogue I'd better avoid fighting alone, then clears the request. This is a
    /// pending reminder, separate from the result-3 recapture that resets the story and
    /// cell door.
    AvoidFightingAloneReminderPending = 153,
    /// Room 268's guard puzzle requires both pad counters above 2 with the Electric Orb
    /// ring. Its solution sets this before the opening animation, disables the guards
    /// temporarily, removes blocker 200 and creates passage 7010 to room 269. While set
    /// it also selects ordinary battle handling instead of solo recapture logic and
    /// enables that passage/camera layout before story 1105200. Entries in 267/269
    /// consume it only at story 1105100, advancing to 1105200; later layout access
    /// follows story progression independently.
    ElectricOrbPassageEnabled = 154,
    /// Changing the ring to SylvarantElectricOrb calls the introduction only while this
    /// is clear; the helper sets it before Lloyd explains that the fire changed into
    /// electricity and may stop the machines (messages 195..200). The same shared bit is
    /// subsequently used as Cycle Lock rotation component 45 in other rooms; this
    /// field-local alias describes the introduction switch and must not leak to those
    /// rotation reads.
    ElectricOrbIntroductionStarted = 160,
    /// Prison room 267 restores cell-door scenery track 0 when this is set; while clear
    /// it creates interaction 300 and collision blocker 200. That interaction opens the
    /// cell, sets this mark and removes the interaction/blocker. The three doors remain
    /// independent. Hitting the guard during Lloyd's escape opens this first door too.
    /// Room 268's result-3 recapture resets the story to 1101000, restores the Fire ring
    /// and clears only this cell-door mark before returning Lloyd to his cell.
    Cell300Open = 173,
} }

ids! { pub enum Field0269Flag: u16 {
    /// Room 268's guard puzzle requires both pad counters above 2 with the Electric Orb
    /// ring. Its solution sets this before the opening animation, disables the guards
    /// temporarily, removes blocker 200 and creates passage 7010 to room 269. While set
    /// it also selects ordinary battle handling instead of solo recapture logic and
    /// enables that passage/camera layout before story 1105200. Entries in 267/269
    /// consume it only at story 1105100, advancing to 1105200; later layout access
    /// follows story progression independently.
    ElectricOrbPassageEnabled = 154,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -45. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit45 = 160,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -90. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit90 = 161,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -180. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit180 = 162,
    /// At story 2404000 the exit from room 269 to room 278 sets this. Room 278 entry at
    /// that same story stage tests it before the Rheaird departure scene: Lloyd
    /// identifies the craft, Sheena urges haste, and the scene advances to 2405000 and
    /// sets the separate RheairdsObtained flag. The mark itself is not cleared there;
    /// preserve the story-stage guard rather than converting it into a permanently
    /// acquired-craft flag.
    RheairdDepartureFrom0269Marked = 190,
} }

ids! { pub enum Field0270Flag: u16 {
    /// The Cycle Lock encoder sub_2191 and decoder sub_226F store the room rotation in
    /// three bits with contributions -45, -90 and -180 degrees. This bit contributes -45
    /// degrees; all eight combinations map to 0 through -315 in 45-degree steps. Entry
    /// attaches the scenery at that decoded angle, and exits depend on the same angle.
    /// This is a packed rotation component, not an absolute-angle flag or permanent
    /// unlocked door.
    CycleLockRotationBit45 = 160,
    /// The Cycle Lock encoder sub_2191 and decoder sub_226F store the room rotation in
    /// three bits with contributions -45, -90 and -180 degrees. This bit contributes -90
    /// degrees; all eight combinations map to 0 through -315 in 45-degree steps. Entry
    /// attaches the scenery at that decoded angle, and exits depend on the same angle.
    /// This is a packed rotation component, not an absolute-angle flag or permanent
    /// unlocked door.
    CycleLockRotationBit90 = 161,
    /// The Cycle Lock encoder sub_2191 and decoder sub_226F store the room rotation in
    /// three bits with contributions -45, -90 and -180 degrees. This bit contributes
    /// -180 degrees; all eight combinations map to 0 through -315 in 45-degree steps.
    /// Entry attaches the scenery at that decoded angle, and exits depend on the same
    /// angle. This is a packed rotation component, not an absolute-angle flag or
    /// permanent unlocked door.
    CycleLockRotationBit180 = 162,
} }

ids! { pub enum Field0271Flag: u16 {
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -45. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit45 = 160,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -90. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit90 = 161,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -180. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit180 = 162,
} }

ids! { pub enum Field0272Flag: u16 {
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -45. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit45 = 160,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -90. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit90 = 161,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -180. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit180 = 162,
} }

ids! { pub enum Field0273Flag: u16 {
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -45. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit45 = 160,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -90. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit90 = 161,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -180. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit180 = 162,
} }

ids! { pub enum Field0275Flag: u16 {
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -45. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit45 = 160,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -90. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit90 = 161,
    /// The room-270 Cycle Lock encoder/decoder stores a packed rotation using bit
    /// contributions -45/-90/-180 degrees; this bit contributes -180. Room 269
    /// initializes [160,161,162] to [0,1,0] (-90 degrees) before entry; room 275
    /// initializes [1,1,1] (-315 degrees). Side rooms 271/272/273 test the same stored
    /// orientation, including retained empty branches; room 271 creates its 1500-Gald
    /// chest for [160,161,162] = [1,0,0] (-45 degrees). Keep all original tests, even
    /// those with empty bodies. This is not a separate unlocked passage or the room-268
    /// Electric Orb introduction.
    CycleLockRotationBit180 = 162,
} }

ids! { pub enum Field0276Flag: u16 {
    /// The Yuan/Botta scene in room 276 initializes the shared block-300 transfer
    /// triplet to 101 (164 set,165 clear,166 set) before advancing through story
    /// 2402000/2403000. The decoder in rooms 279/280 assigns these bits to valid
    /// position, lower local row and right local column respectively: the preset maps to
    /// (-675,150) in 279 or (-680,1655,850) in 280 with the platform raised. This is an
    /// initial saved block position, not three unrelated quest milestones; the
    /// platform-height flag remains separate.
    Block300TransferPositionRecorded = 164,
    /// The Yuan/Botta scene in room 276 initializes the shared block-300 transfer
    /// triplet to 101 (164 set,165 clear,166 set) before advancing through story
    /// 2402000/2403000. The decoder in rooms 279/280 assigns these bits to valid
    /// position, lower local row and right local column respectively: the preset maps to
    /// (-675,150) in 279 or (-680,1655,850) in 280 with the platform raised. This is an
    /// initial saved block position, not three unrelated quest milestones; the
    /// platform-height flag remains separate.
    Block300TransferLowerRow = 165,
    /// The Yuan/Botta scene in room 276 initializes the shared block-300 transfer
    /// triplet to 101 (164 set,165 clear,166 set) before advancing through story
    /// 2402000/2403000. The decoder in rooms 279/280 assigns these bits to valid
    /// position, lower local row and right local column respectively: the preset maps to
    /// (-675,150) in 279 or (-680,1655,850) in 280 with the platform raised. This is an
    /// initial saved block position, not three unrelated quest milestones; the
    /// platform-height flag remains separate.
    Block300TransferRightColumn = 166,
} }

ids! { pub enum Field0278Flag: u16 {
    /// At story 2404000 the exit from room 269 to room 278 sets this. Room 278 entry at
    /// that same story stage tests it before the Rheaird departure scene: Lloyd
    /// identifies the craft, Sheena urges haste, and the scene advances to 2405000 and
    /// sets the separate RheairdsObtained flag. The mark itself is not cleared there;
    /// preserve the story-stage guard rather than converting it into a permanently
    /// acquired-craft flag.
    RheairdDepartureFrom0269Marked = 190,
} }

ids! { pub enum Field0279Flag: u16 {
    /// Powered switch 200 in room 280 sets this to lower the transfer platform node from
    /// Z offset 400 to -600, moving any transferred blocks and their collision actors
    /// down by 1000. Reversing clears it. Entry restores that height; room 279 omits its
    /// platform collision and transferred blocks while lowered. The original ring-timer
    /// prerequisite remains separate.
    BlockTransferPlatformLowered = 163,
    /// Room 279 snapshots block actor 300 only while that actor exists. It clears this
    /// triplet, then sets its first bit only for a block at one of the transfer
    /// platform's four XY positions: (-825,150), (-675,150), (-825,0), (-675,0). The
    /// next bits select row and column. Room 280 restores this block using the same
    /// triplet; a missing source actor leaves the stored triplet untouched.
    Block300TransferPositionRecorded = 164,
    /// The stored transfer-position triplet for block 300 uses this bit for the lower
    /// local row: clear means Y 150 and set means Y 0 in room 279. Room 280 maps those
    /// rows to Y 1655 and 1505. The triplet's first bit separately records a valid
    /// platform position; this row bit alone does not put a block on the platform.
    Block300TransferLowerRow = 165,
    /// The stored transfer-position triplet for block 300 uses this bit for the right
    /// local column: clear means X -825 and set means X -675 in room 279. Room 280 maps
    /// those columns to X -830 and -680. The triplet's first bit separately records a
    /// valid platform position. These directions name local coordinates rather than
    /// world-map orientation.
    Block300TransferRightColumn = 166,
    /// Room 279 snapshots block actor 301 only while that actor exists. It clears this
    /// triplet, then sets its first bit only for a block at one of the transfer
    /// platform's four XY positions: (-825,150), (-675,150), (-825,0), (-675,0). The
    /// next bits select row and column. Room 280 restores this block using the same
    /// triplet; a missing source actor leaves the stored triplet untouched.
    Block301TransferPositionRecorded = 167,
    /// The stored transfer-position triplet for block 301 uses this bit for the lower
    /// local row: clear means Y 150 and set means Y 0 in room 279. Room 280 maps those
    /// rows to Y 1655 and 1505. The triplet's first bit separately records a valid
    /// platform position; this row bit alone does not put a block on the platform.
    Block301TransferLowerRow = 168,
    /// The stored transfer-position triplet for block 301 uses this bit for the right
    /// local column: clear means X -825 and set means X -675 in room 279. Room 280 maps
    /// those columns to X -830 and -680. The triplet's first bit separately records a
    /// valid platform position. These directions name local coordinates rather than
    /// world-map orientation.
    Block301TransferRightColumn = 169,
} }

ids! { pub enum Field0280Flag: u16 {
    /// Powered switch 200 in room 280 sets this to lower the transfer platform node from
    /// Z offset 400 to -600, moving any transferred blocks and their collision actors
    /// down by 1000. Reversing clears it. Entry restores that height; room 279 omits its
    /// platform collision and transferred blocks while lowered. The original ring-timer
    /// prerequisite remains separate.
    BlockTransferPlatformLowered = 163,
    /// Room 279 snapshots block actor 300 only while that actor exists. It clears this
    /// triplet, then sets its first bit only for a block at one of the transfer
    /// platform's four XY positions: (-825,150), (-675,150), (-825,0), (-675,0). The
    /// next bits select row and column. Room 280 restores this block using the same
    /// triplet; a missing source actor leaves the stored triplet untouched.
    Block300TransferPositionRecorded = 164,
    /// The stored transfer-position triplet for block 300 uses this bit for the lower
    /// local row: clear means Y 150 and set means Y 0 in room 279. Room 280 maps those
    /// rows to Y 1655 and 1505. The triplet's first bit separately records a valid
    /// platform position; this row bit alone does not put a block on the platform.
    Block300TransferLowerRow = 165,
    /// The stored transfer-position triplet for block 300 uses this bit for the right
    /// local column: clear means X -825 and set means X -675 in room 279. Room 280 maps
    /// those columns to X -830 and -680. The triplet's first bit separately records a
    /// valid platform position. These directions name local coordinates rather than
    /// world-map orientation.
    Block300TransferRightColumn = 166,
    /// Room 279 snapshots block actor 301 only while that actor exists. It clears this
    /// triplet, then sets its first bit only for a block at one of the transfer
    /// platform's four XY positions: (-825,150), (-675,150), (-825,0), (-675,0). The
    /// next bits select row and column. Room 280 restores this block using the same
    /// triplet; a missing source actor leaves the stored triplet untouched.
    Block301TransferPositionRecorded = 167,
    /// The stored transfer-position triplet for block 301 uses this bit for the lower
    /// local row: clear means Y 150 and set means Y 0 in room 279. Room 280 maps those
    /// rows to Y 1655 and 1505. The triplet's first bit separately records a valid
    /// platform position; this row bit alone does not put a block on the platform.
    Block301TransferLowerRow = 168,
    /// The stored transfer-position triplet for block 301 uses this bit for the right
    /// local column: clear means X -825 and set means X -675 in room 279. Room 280 maps
    /// those columns to X -830 and -680. The triplet's first bit separately records a
    /// valid platform position. These directions name local coordinates rather than
    /// world-map orientation.
    Block301TransferRightColumn = 169,
    /// Powered switch 201 requires a positive field countdown before offering its
    /// toggle. Set lowers its scenery node by 250 and removes passage blocker actor 315;
    /// clear restores zero translation and blocker 315 at (210,1325,0). Entry restores
    /// that state. This is separate from the block-transfer platform and its ring-timer
    /// prerequisite.
    Barrier315Lowered = 181,
} }

ids! { pub enum Field0282Flag: u16 {
    /// Heimdall room 282's passage and room 284's two rest/departure helpers set this
    /// immediately before entering room 285 at (326,-217,0), using their original
    /// differing headings. No recovered literal reader in room 285 assigns an extra
    /// scene or reward to the mark. The alias records this common arrival write without
    /// claiming an effect, inn milestone or consumed request.
    Room0285ArrivalMarked = 150,
} }

ids! { pub enum Field0284Flag: u16 {
    /// Heimdall room 282's passage and room 284's two rest/departure helpers set this
    /// immediately before entering room 285 at (326,-217,0), using their original
    /// differing headings. No recovered literal reader in room 285 assigns an extra
    /// scene or reward to the mark. The alias records this common arrival write without
    /// claiming an effect, inn milestone or consumed request.
    Room0285ArrivalMarked = 150,
} }

ids! { pub enum Field0300Flag: u16 {
    /// Hima's five pre-Tower conversation helpers pass Colette/Genis/Raine/Sheena/Kratos
    /// IDs to sub_439B. It awards +60 only while this shared latch is clear, then sets
    /// it. Colette's accept-to-talk branch also advances the story to 2202000. The latch
    /// gates one shared bonus, not all conversation dialogue, a chosen companion or five
    /// independent rewards.
    PreTowerConversationAffinityBonusClaimed = 150,
    /// The overworld Hima entry and the Luin escort-arrival branch mark this.
    /// Inn-approach event 3000 uses it with SophiaBoltzmanBookQuestExplained and not
    /// PietroHealed to select the Pietro/book/horn reminders, then clears it at the end
    /// of the event regardless of which branch ran. Accepting the return escort to Luin
    /// also clears it before leaving. This is a current reminder switch, not healing
    /// Pietro or completing the book quest; the equal-valued caravan selector remains a
    /// separate world use.
    PietroInnReminderEnabled = 199,
} }

ids! { pub enum Field0309Flag: u16 {
    /// One shared +60 affinity opportunity across the Colette, Genis and Kratos
    /// conversations after the oracle. Dialogue repeats; only the bonus is gated.
    OracleConversationAffinityBonusClaimed = 150,
} }

ids! {
    /// Known uses of shared session bits in Flanoir 0317.
    /// Unidentified bit 171 is cleared by the debug refresh at word PC 0x44B3.
    /// No matching invitation reader or writer has been recovered; it remains numeric.
    pub enum Field0317Flag: u16 {
    /// Helper sub_213A sets this before dispatching the affinity-rank-2 visitor,
    /// provided that character has not already been declined. Individual invitation
    /// handlers test these dispatch markers to advance after rejection. This marks an
    /// attempted rank dispatch, not the visitor's acceptance or a completed night
    /// conversation; the debug reset clears it.
    NightInvitationRank2Dispatched = 172,
    /// Helper sub_2370 sets this before dispatching the affinity-rank-3 visitor,
    /// provided that character has not already been declined. Individual invitation
    /// handlers test these dispatch markers to advance after rejection. This marks an
    /// attempted rank dispatch, not the visitor's acceptance or a completed night
    /// conversation; the debug reset clears it.
    NightInvitationRank3Dispatched = 173,
    /// Helper sub_25A6 sets this before dispatching the affinity-rank-4 visitor,
    /// provided that character has not already been declined. Individual invitation
    /// handlers test these dispatch markers to advance after rejection. This marks an
    /// attempted rank dispatch, not the visitor's acceptance or a completed night
    /// conversation; the debug reset clears it.
    NightInvitationRank4Dispatched = 174,
    /// Colette's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    ColetteNightInvitationDeclined = 181,
    /// Genis's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    GenisNightInvitationDeclined = 182,
    /// Raine's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    RaineNightInvitationDeclined = 183,
    /// Sheena's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    SheenaNightInvitationDeclined = 184,
    /// Zelos's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    ZelosNightInvitationDeclined = 185,
    /// Presea's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    PreseaNightInvitationDeclined = 186,
    /// Regal's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    RegalNightInvitationDeclined = 187,
    /// Kratos's night-visit helper sets this on declining the invitation and then
    /// advances to another affinity-rank visitor. Dispatch skips a character with its
    /// declined bit set. Cleanup sub_27DC applies -3*60 affinity for each marked
    /// character, then clears all eight decline bits. Acceptance uses a separate +3*60
    /// adjustment and selects FlanoirCompanion; reaching the rank-4 visitor forces the
    /// selection before the decline prompt.
    KratosNightInvitationDeclined = 188,
} }

ids! { pub enum Field0322Flag: u16 {
    /// The doctor-departure scene chooses companions with affinity ranks 7,6,5,4, using
    /// IDs 2..8 and excluding Kratos. For Presea, its staging branch records bit 161 and
    /// removes that character from the party after the party snapshot. The dialogue asks
    /// for three escorts although four rank groups are present in the original code;
    /// preserve that code rather than changing the count.
    PreseaDoctorEscortMarked = 161,
    /// The doctor-departure scene chooses companions with affinity ranks 7,6,5,4, using
    /// IDs 2..8 and excluding Kratos. For Zelos, its staging branch records bit 162 and
    /// removes that character from the party after the party snapshot. The dialogue asks
    /// for three escorts although four rank groups are present in the original code;
    /// preserve that code rather than changing the count.
    ZelosDoctorEscortMarked = 162,
    /// The doctor-departure scene chooses companions with affinity ranks 7,6,5,4, using
    /// IDs 2..8 and excluding Kratos. For Raine, its staging branch records bit 163 and
    /// removes that character from the party after the party snapshot. The dialogue asks
    /// for three escorts although four rank groups are present in the original code;
    /// preserve that code rather than changing the count.
    RaineDoctorEscortMarked = 163,
    /// The doctor-departure scene chooses companions with affinity ranks 7,6,5,4, using
    /// IDs 2..8 and excluding Kratos. For Regal, its staging branch records bit 164 and
    /// removes that character from the party after the party snapshot. The dialogue asks
    /// for three escorts although four rank groups are present in the original code;
    /// preserve that code rather than changing the count.
    RegalDoctorEscortMarked = 164,
    /// The doctor-departure scene chooses companions with affinity ranks 7,6,5,4, using
    /// IDs 2..8 and excluding Kratos. For Sheena, its staging branch records bit 165 and
    /// removes that character from the party after the party snapshot. The dialogue asks
    /// for three escorts although four rank groups are present in the original code;
    /// preserve that code rather than changing the count.
    SheenaDoctorEscortMarked = 165,
    /// The doctor-departure scene chooses companions with affinity ranks 7,6,5,4, using
    /// IDs 2..8 and excluding Kratos. For Genis, its staging branch records bit 166 and
    /// removes that character from the party after the party snapshot. The dialogue asks
    /// for three escorts although four rank groups are present in the original code;
    /// preserve that code rather than changing the count. The rank-5 Genis branch
    /// removes him without setting 166; ranks 7,6,4 set the marker. The alias names the
    /// stored mark and does not fill in the missing write.
    GenisDoctorEscortMarked = 166,
    /// The doctor-departure scene chooses companions with affinity ranks 7,6,5,4, using
    /// IDs 2..8 and excluding Kratos. For Colette, its staging branch records bit 167
    /// and removes that character from the party after the party snapshot. The dialogue
    /// asks for three escorts although four rank groups are present in the original
    /// code; preserve that code rather than changing the count.
    ColetteDoctorEscortMarked = 167,
} }

ids! { pub enum Field0323Flag: u16 {
    /// Ice-ring contact with droplet actor 500 sets this before sub_29EF removes that
    /// actor and forms ice block 502; the supplied model nodes identify suiteki and
    /// koori_block. Formation creates the block at Z 1050 and starts event 4501, whose
    /// landing monitor retains its exact 744/745/750 checks. Re-entry with this bit set
    /// instead restores the block directly at Z 900. Do not normalize those different
    /// heights or turn this start mark into a requirement that the animation has
    /// finished.
    IceBlock502FormationStarted = 158,
} }

ids! { pub enum Field0324Flag: u16 {
    /// Helper sub_41F2 requires statue angles 270, 0-or-360, 90 and 270, shifts the door
    /// node by -200 X, removes blocker 5012, disables all four statue interactions and
    /// sets this bit. Entry restores both the open door and the solved angles. The clue
    /// conversation uses a separate flag.
    StatuePuzzleDoorOpened = 150,
    /// Ice-mode ring contact with target 5010 sets this before removing its blocker and
    /// forming ice actor 5002 at Z -150. The corresponding oscillating target task stops
    /// while it is set. All four normal room exits clear both temporary block bits;
    /// these are separate from the permanently frozen lake flag.
    IceBlock5002FormationStarted = 154,
    /// Ice-mode ring contact with target 5011 sets this before removing its blocker and
    /// forming ice actor 5001 at Z 600. The corresponding oscillating target task stops
    /// while it is set. All four normal room exits clear both temporary block bits;
    /// these are separate from the permanently frozen lake flag.
    IceBlock5001FormationStarted = 155,
    /// The first interaction with actor 204 discusses The answer lies in the place with
    /// a view, then sets this. Later interactions still show the clue text. It records
    /// the introductory conversation, not the statue puzzle solution or door opening.
    DoorRiddleConversationSeen = 165,
} }

ids! { pub enum Field0326Flag: u16 {
    /// The village entrance sequence from Mizuho 326 attaches and hides the controlled
    /// actor, sets this and changes to room 327 at (458,-64,2). Room 327 entry uses it
    /// to start scenery track 0 with duration 250; after returning field control it
    /// waits 5 ticks and clears the request. Preserve that early clear rather than
    /// interpreting it as animation completion or a permanent door-open flag.
    VillageEntranceArrivalAnimationPending = 150,
    /// At local stage 20001, the seven companion conversations share one +60 affinity
    /// opportunity using this single bit. Dialogue repeats; it is not one bonus per
    /// character. Accepting Orochi's duel-departure prompt clears the latch before
    /// changing to 0329.
    KuchinawaDuelConversationAffinityBonusClaimed = 1700,
} }

ids! { pub enum Field0327Flag: u16 {
    /// The village entrance sequence from Mizuho 326 attaches and hides the controlled
    /// actor, sets this and changes to room 327 at (458,-64,2). Room 327 entry uses it
    /// to start scenery track 0 with duration 250; after returning field control it
    /// waits 5 ticks and clears the request. Preserve that early clear rather than
    /// interpreting it as animation completion or a permanent door-open flag.
    VillageEntranceArrivalAnimationPending = 150,
} }

ids! { pub enum Field0361Flag: u16 {
    /// Room 363's conditional Sheena encounter helper sets Light Seal progress 11500,
    /// records this and redirects to room 361. Its staging helper sub_392A
    /// consumes/clears this approach selector and advances to 12000. Encounter
    /// prerequisites include no earlier Balacruf encounter and Sheena absent; the bit
    /// does not itself record a completed battle or recruitment.
    SheenaEncounterFrom0363Pending = 182,
    /// Room 364's conditional Sheena encounter helper sets Light Seal progress 11500,
    /// records this and redirects to room 361. Its staging helper sub_392A
    /// consumes/clears this approach selector and advances to 12000. Encounter
    /// prerequisites include no earlier Balacruf encounter and Sheena absent; the bit
    /// does not itself record a completed battle or recruitment.
    SheenaEncounterFrom0364Pending = 183,
    /// The first room-362 entry sets this before sampling Sheena's party presence into
    /// bit 185. Room 361 tests it before restoring the saved party during the applicable
    /// Light Seal stages. It marks split-party setup; it does not prove that every team
    /// slot or leader word has been initialized.
    SplitPartySetupMarked = 184,
    /// Only the first room-362 setup samples whether Sheena is present and sets this.
    /// Set allows team A's third member and the selectable three-plus-three layout;
    /// clear uses the initial two-person Lloyd/Colette exploration team with three
    /// companions on the circles. The value is a setup snapshot, not a live Sheena
    /// party-membership check. Room 361 uses it when restoring the six-member saved
    /// party.
    ThreeMemberTeamAEnabled = 185,
    /// The room-362 companion interactions allow team selection when Light Seal progress
    /// is below 11000 or this bit is clear. Confirming a choice sets it; entry also sets
    /// it in fixed story/staging modes. With the applicable progress and set bit, entry
    /// restores the existing split rather than reopening selection. Room 361 clears it
    /// on entry. The early-stage override remains explicit.
    TeamSelectionLocked = 186,
    /// The two team-switch helpers set this when selecting team B and clear it when
    /// selecting team A, restoring each team's saved leader. Room-entry layouts and
    /// passages test this selector. Room 361 also clears it on entry, and room 366
    /// clears it on a canceled communication choice; keep those independent writes
    /// instead of asserting that the flag always matches the current party array.
    TeamBSelected = 187,
    /// The normal descent from room 362 sets this before returning to 361. Room 361
    /// requires it clear for the conditional Aska/Linkite/Nova reminder chain. The other
    /// quest prerequisites and stages remain independent; this marker suppresses that
    /// entrance reminder rather than proving Aska was found or a pact formed.
    AskaEntranceReminderSuppressed = 195,
} }

ids! { pub enum Field0362Flag: u16 {
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    LloydTeamBit = 153,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    LloydSlotBit2 = 154,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    LloydSlotBit1 = 155,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    ColetteTeamBit = 156,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    ColetteSlotBit2 = 157,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    ColetteSlotBit1 = 158,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    GenisTeamBit = 159,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    GenisSlotBit2 = 160,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    GenisSlotBit1 = 161,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    RaineTeamBit = 162,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    RaineSlotBit2 = 163,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    RaineSlotBit1 = 164,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 8. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit8 = 168,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 4. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit4 = 169,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 2. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit2 = 170,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 1. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit1 = 171,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 8. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit8 = 172,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 4. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit4 = 173,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 2. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit2 = 174,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 1. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit1 = 175,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    KratosTeamBit = 177,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    KratosSlotBit2 = 178,
    /// The tower stores each eligible character at 150 + character ID*3: clear/set of
    /// the first bit selects team A/B, followed by slot bits weighted 2/1 for slots
    /// 1..3. Team A initially explores with Lloyd/Colette; team B initially holds the
    /// three entrance circles. Decoders in 361/362/366 skip IDs 6,7,8 because their
    /// triplets overlap the two saved-leader words. Preserve the original initializers
    /// and dynamically indexed updates.
    KratosSlotBit1 = 179,
    /// The first room-362 entry sets this before sampling Sheena's party presence into
    /// bit 185. Room 361 tests it before restoring the saved party during the applicable
    /// Light Seal stages. It marks split-party setup; it does not prove that every team
    /// slot or leader word has been initialized.
    SplitPartySetupMarked = 184,
    /// Only the first room-362 setup samples whether Sheena is present and sets this.
    /// Set allows team A's third member and the selectable three-plus-three layout;
    /// clear uses the initial two-person Lloyd/Colette exploration team with three
    /// companions on the circles. The value is a setup snapshot, not a live Sheena
    /// party-membership check. Room 361 uses it when restoring the six-member saved
    /// party.
    ThreeMemberTeamAEnabled = 185,
    /// The room-362 companion interactions allow team selection when Light Seal progress
    /// is below 11000 or this bit is clear. Confirming a choice sets it; entry also sets
    /// it in fixed story/staging modes. With the applicable progress and set bit, entry
    /// restores the existing split rather than reopening selection. Room 361 clears it
    /// on entry. The early-stage override remains explicit.
    TeamSelectionLocked = 186,
    /// The two team-switch helpers set this when selecting team B and clear it when
    /// selecting team A, restoring each team's saved leader. Room-entry layouts and
    /// passages test this selector. Room 361 also clears it on entry, and room 366
    /// clears it on a canceled communication choice; keep those independent writes
    /// instead of asserting that the flag always matches the current party array.
    TeamBSelected = 187,
    /// Both the normal room-362 passage and its split-party departure scene set this
    /// before entering 364 at (-59,700,-2). Room 364 suppresses the immediate
    /// transition, walks the controlled actor to (-59,108,-2) and clears the marker. It
    /// records arrival staging, not an opened shortcut.
    Room0364EntranceWalkPending = 190,
    /// The normal descent from room 362 sets this before returning to 361. Room 361
    /// requires it clear for the conditional Aska/Linkite/Nova reminder chain. The other
    /// quest prerequisites and stages remain independent; this marker suppresses that
    /// entrance reminder rather than proving Aska was found or a pact formed.
    AskaEntranceReminderSuppressed = 195,
} }

ids! { pub enum Field0363Flag: u16 {
    /// The contact handler for target actor 400, while Light Seal progress is below
    /// 13100, scales its scenery node to zero, runs the removal/effect helper, advances
    /// progress to 13100 and sets this. No recovered literal reader uses bit 180 to
    /// restore a separate layout; the alias records the explicit target-removal mark,
    /// not completion of the Light Seal.
    Target400RemovalMarked = 180,
    /// Room 363's conditional Sheena encounter helper sets Light Seal progress 11500,
    /// records this and redirects to room 361. Its staging helper sub_392A
    /// consumes/clears this approach selector and advances to 12000. Encounter
    /// prerequisites include no earlier Balacruf encounter and Sheena absent; the bit
    /// does not itself record a completed battle or recruitment.
    SheenaEncounterFrom0363Pending = 182,
    /// The two team-switch helpers set this when selecting team B and clear it when
    /// selecting team A, restoring each team's saved leader. Room-entry layouts and
    /// passages test this selector. Room 361 also clears it on entry, and room 366
    /// clears it on a canceled communication choice; keep those independent writes
    /// instead of asserting that the flag always matches the current party array.
    TeamBSelected = 187,
} }

ids! { pub enum Field0364Flag: u16 {
    /// Room 364's conditional Sheena encounter helper sets Light Seal progress 11500,
    /// records this and redirects to room 361. Its staging helper sub_392A
    /// consumes/clears this approach selector and advances to 12000. Encounter
    /// prerequisites include no earlier Balacruf encounter and Sheena absent; the bit
    /// does not itself record a completed battle or recruitment.
    SheenaEncounterFrom0364Pending = 183,
    /// Only the first room-362 setup samples whether Sheena is present and sets this.
    /// Set allows team A's third member and the selectable three-plus-three layout;
    /// clear uses the initial two-person Lloyd/Colette exploration team with three
    /// companions on the circles. The value is a setup snapshot, not a live Sheena
    /// party-membership check. Room 361 uses it when restoring the six-member saved
    /// party.
    ThreeMemberTeamAEnabled = 185,
    /// The two team-switch helpers set this when selecting team B and clear it when
    /// selecting team A, restoring each team's saved leader. Room-entry layouts and
    /// passages test this selector. Room 361 also clears it on entry, and room 366
    /// clears it on a canceled communication choice; keep those independent writes
    /// instead of asserting that the flag always matches the current party array.
    TeamBSelected = 187,
    /// Both the normal room-362 passage and its split-party departure scene set this
    /// before entering 364 at (-59,700,-2). Room 364 suppresses the immediate
    /// transition, walks the controlled actor to (-59,108,-2) and clears the marker. It
    /// records arrival staging, not an opened shortcut.
    Room0364EntranceWalkPending = 190,
} }

ids! { pub enum Field0366Flag: u16 {
    /// The team decoder uses bit 150 + character ID*3, with clear for team A and set for
    /// team B. Room 366 explicitly tests Raine's ID-4 bit to stage her in the first
    /// remote-team communication scene. This is team membership, separately from either
    /// saved leader and from Raine's affinity.
    RaineTeamBit = 162,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 8. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit8 = 168,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 4. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit4 = 169,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 2. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit2 = 170,
    /// The tower decodes team A's stored leader from flags 168..171 with weights
    /// 8/4/2/1; this bit contributes 1. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamALeaderBit1 = 171,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 8. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit8 = 172,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 4. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit4 = 173,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 2. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit2 = 174,
    /// The tower decodes team B's stored leader from flags 172..175 with weights
    /// 8/4/2/1; this bit contributes 1. These words overlap the skipped character-ID
    /// triplets 6,7,8. Room 362's sub_37DD has a mismatched team-A low-bit save: an odd
    /// leader sets bit 172 instead of 171, and the else branch clears 170 instead of
    /// 171. Room 366's sub_3226 uses 171 correctly. Names describe the decoded storage
    /// roles and preserve both original writers, including that cross-word effect.
    TeamBLeaderBit1 = 175,
    /// Only the first room-362 setup samples whether Sheena is present and sets this.
    /// Set allows team A's third member and the selectable three-plus-three layout;
    /// clear uses the initial two-person Lloyd/Colette exploration team with three
    /// companions on the circles. The value is a setup snapshot, not a live Sheena
    /// party-membership check. Room 361 uses it when restoring the six-member saved
    /// party.
    ThreeMemberTeamAEnabled = 185,
    /// The two team-switch helpers set this when selecting team B and clear it when
    /// selecting team A, restoring each team's saved leader. Room-entry layouts and
    /// passages test this selector. Room 361 also clears it on entry, and room 366
    /// clears it on a canceled communication choice; keep those independent writes
    /// instead of asserting that the flag always matches the current party array.
    TeamBSelected = 187,
    /// The passage into room 368 at the lower landing records this: lower Z about 0 from
    /// 366, middle Z 2989 from 367, or upper Z 5965/5985 from 366/367. Room 368 selects
    /// the corresponding one-of-three scenery visibility layout and clears the marker.
    /// The three original tests are independent, so simultaneous marked arrivals are not
    /// normalized into an exclusive enum.
    LowerLandingArrivalPending = 197,
    /// The passage into room 368 at the upper landing records this: lower Z about 0 from
    /// 366, middle Z 2989 from 367, or upper Z 5965/5985 from 366/367. Room 368 selects
    /// the corresponding one-of-three scenery visibility layout and clears the marker.
    /// The three original tests are independent, so simultaneous marked arrivals are not
    /// normalized into an exclusive enum.
    UpperLandingArrivalPending = 199,
} }

ids! { pub enum Field0367Flag: u16 {
    /// The passage into room 368 at the middle landing records this: lower Z about 0
    /// from 366, middle Z 2989 from 367, or upper Z 5965/5985 from 366/367. Room 368
    /// selects the corresponding one-of-three scenery visibility layout and clears the
    /// marker. The three original tests are independent, so simultaneous marked arrivals
    /// are not normalized into an exclusive enum.
    MiddleLandingArrivalPending = 198,
    /// The passage into room 368 at the upper landing records this: lower Z about 0 from
    /// 366, middle Z 2989 from 367, or upper Z 5965/5985 from 366/367. Room 368 selects
    /// the corresponding one-of-three scenery visibility layout and clears the marker.
    /// The three original tests are independent, so simultaneous marked arrivals are not
    /// normalized into an exclusive enum.
    UpperLandingArrivalPending = 199,
} }

ids! { pub enum Field0368Flag: u16 {
    /// The passage into room 368 at the lower landing records this: lower Z about 0 from
    /// 366, middle Z 2989 from 367, or upper Z 5965/5985 from 366/367. Room 368 selects
    /// the corresponding one-of-three scenery visibility layout and clears the marker.
    /// The three original tests are independent, so simultaneous marked arrivals are not
    /// normalized into an exclusive enum.
    LowerLandingArrivalPending = 197,
    /// The passage into room 368 at the middle landing records this: lower Z about 0
    /// from 366, middle Z 2989 from 367, or upper Z 5965/5985 from 366/367. Room 368
    /// selects the corresponding one-of-three scenery visibility layout and clears the
    /// marker. The three original tests are independent, so simultaneous marked arrivals
    /// are not normalized into an exclusive enum.
    MiddleLandingArrivalPending = 198,
    /// The passage into room 368 at the upper landing records this: lower Z about 0 from
    /// 366, middle Z 2989 from 367, or upper Z 5965/5985 from 366/367. Room 368 selects
    /// the corresponding one-of-three scenery visibility layout and clears the marker.
    /// The three original tests are independent, so simultaneous marked arrivals are not
    /// normalized into an exclusive enum.
    UpperLandingArrivalPending = 199,
} }

ids! { pub enum Field0380Flag: u16 {
    /// Solo beginner/intermediate result helpers in Meltokio 392 set this before
    /// returning to town. Entry in 380 uses it, with the current coliseum/Zelos/story
    /// prerequisites, to check the Tokunaga/Seles follow-ups, then clears it even when
    /// those prerequisites prevent a scene. It is separate from permanent class-clear
    /// flags.
    ColiseumSoloResultEntryCheckPending = 1700,
} }

ids! { pub enum Field0390Flag: u16 {
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Colette. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    ColetteColiseumAffinityPick = 161,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Genis. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    GenisColiseumAffinityPick = 162,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Raine. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    RaineColiseumAffinityPick = 163,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Sheena. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    SheenaColiseumAffinityPick = 164,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Zelos. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    ZelosColiseumAffinityPick = 165,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Presea. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    PreseaColiseumAffinityPick = 166,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Regal. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    RegalColiseumAffinityPick = 167,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Kratos. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    KratosColiseumAffinityPick = 168,
    /// The first registration scene in room 390 selects Lloyd, snapshots the party,
    /// removes the other characters and sets bit 170 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    LloydColiseumDebutEntrantSelected = 170,
    /// The first registration scene in room 390 selects Colette, snapshots the party,
    /// removes the other characters and sets bit 171 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    ColetteColiseumDebutEntrantSelected = 171,
    /// The first registration scene in room 390 selects Genis, snapshots the party,
    /// removes the other characters and sets bit 172 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    GenisColiseumDebutEntrantSelected = 172,
    /// The first registration scene in room 390 selects Raine, snapshots the party,
    /// removes the other characters and sets bit 173 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    RaineColiseumDebutEntrantSelected = 173,
    /// The first registration scene in room 390 selects Sheena, snapshots the party,
    /// removes the other characters and sets bit 174 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    SheenaColiseumDebutEntrantSelected = 174,
    /// The first registration scene in room 390 selects Zelos, snapshots the party,
    /// removes the other characters and sets bit 175 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag. The Zelos
    /// branch records the bit before its party snapshot; that original ordering is
    /// preserved.
    ZelosColiseumDebutEntrantSelected = 175,
    /// The first registration scene in room 390 selects Regal, snapshots the party,
    /// removes the other characters and sets bit 176 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    RegalColiseumDebutEntrantSelected = 176,
    /// The first registration scene in room 390 selects Presea, snapshots the party,
    /// removes the other characters and sets bit 177 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    PreseaColiseumDebutEntrantSelected = 177,
} }

ids! { pub enum Field0391Flag: u16 {
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Colette. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    ColetteColiseumAffinityPick = 161,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Genis. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    GenisColiseumAffinityPick = 162,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Raine. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    RaineColiseumAffinityPick = 163,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Sheena. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    SheenaColiseumAffinityPick = 164,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Zelos. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    ZelosColiseumAffinityPick = 165,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Presea. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    PreseaColiseumAffinityPick = 166,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Regal. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    RegalColiseumAffinityPick = 167,
    /// Each solo-registration helper clears bits 161..168 and stores rank 1 from
    /// affinity among currently present companions, mapping IDs 2..9 to Colette, Genis,
    /// Raine, Sheena, Zelos, Presea, Regal and Kratos. This bit records Kratos. The
    /// retained encouragement helper uses the saved pick: Lloyd entrants get that
    /// companion; a non-Lloyd entrant matching the pick gets Lloyd instead. Cancellation
    /// and the encouragement exit clear the picks. It records a snapshot selection, not
    /// an affinity bonus or the entrant. Its visible call site follows an unconditional
    /// return in a retained helper, so the alias does not establish that the
    /// encouragement scene runs in normal play.
    KratosColiseumAffinityPick = 168,
    /// The first registration scene in room 390 selects Lloyd, snapshots the party,
    /// removes the other characters and sets bit 170 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    LloydColiseumDebutEntrantSelected = 170,
    /// The first registration scene in room 390 selects Colette, snapshots the party,
    /// removes the other characters and sets bit 171 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    ColetteColiseumDebutEntrantSelected = 171,
    /// The first registration scene in room 390 selects Genis, snapshots the party,
    /// removes the other characters and sets bit 172 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    GenisColiseumDebutEntrantSelected = 172,
    /// The first registration scene in room 390 selects Raine, snapshots the party,
    /// removes the other characters and sets bit 173 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    RaineColiseumDebutEntrantSelected = 173,
    /// The first registration scene in room 390 selects Sheena, snapshots the party,
    /// removes the other characters and sets bit 174 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    SheenaColiseumDebutEntrantSelected = 174,
    /// The first registration scene in room 390 selects Zelos, snapshots the party,
    /// removes the other characters and sets bit 175 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag. The Zelos
    /// branch records the bit before its party snapshot; that original ordering is
    /// preserved.
    ZelosColiseumDebutEntrantSelected = 175,
    /// The first registration scene in room 390 selects Regal, snapshots the party,
    /// removes the other characters and sets bit 176 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    RegalColiseumDebutEntrantSelected = 176,
    /// The first registration scene in room 390 selects Presea, snapshots the party,
    /// removes the other characters and sets bit 177 before setting coliseum progress to
    /// 3000 and changing to room 391. At progress 5000, room 391 reads it to restore
    /// that character and show the need to save Kate before returning to 390. This
    /// family orders Regal before Presea, unlike the separate affinity-pick bits. It is
    /// a debut entrant marker, not a victory or permanent availability flag.
    PreseaColiseumDebutEntrantSelected = 177,
} }

ids! { pub enum Field0392Flag: u16 {
    /// Solo beginner/intermediate result helpers in Meltokio 392 set this before
    /// returning to town. Entry in 380 uses it, with the current coliseum/Zelos/story
    /// prerequisites, to check the Tokunaga/Seles follow-ups, then clears it even when
    /// those prerequisites prevent a scene. It is separate from permanent class-clear
    /// flags.
    ColiseumSoloResultEntryCheckPending = 1700,
} }

ids! { pub enum Field0399Flag: u16 {
    /// The corresponding upstairs doorway in Meltokio 399 sets this before entering
    /// shared room 400. Entry selects its scenery and exit record, whose handler clears
    /// the mark and returns to 399 at (292,396,425). Right/left refer to local-X door
    /// positions. The two flags retain independent tests and are not replaced with an
    /// exclusive selector.
    UpstairsRightRoomSelected = 180,
    /// The corresponding upstairs doorway in Meltokio 399 sets this before entering
    /// shared room 400. Entry selects its scenery and exit record, whose handler clears
    /// the mark and returns to 399 at (-287,396,425). Right/left refer to local-X door
    /// positions. The two flags retain independent tests and are not replaced with an
    /// exclusive selector. This left-room selection also gates the Kate/Zelos prejudice
    /// conversation under its other quest/party conditions; that scene and retained
    /// unused_25FF clear it before returning. The conversation's permanent story mark
    /// remains separate.
    UpstairsLeftRoomSelected = 181,
    /// Meltokio 401/402 select Colette for the banquet terrace, either by explicit
    /// choice or highest-affinity fallback, then use this bit for the corresponding
    /// terrace scene. Room 402 omits the selected companion from its ordinary hall
    /// actors. The 399 aftermath clears the selector; it is not permanent title
    /// ownership. The eighth selection branch also awards affinity to Kratos in the
    /// original script.
    ColetteBanquetTerraceCompanionSelected = 1700,
    /// Meltokio 401/402 select Genis for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    GenisBanquetTerraceCompanionSelected = 1701,
    /// Meltokio 401/402 select Raine for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    RaineBanquetTerraceCompanionSelected = 1702,
    /// Meltokio 401/402 select Sheena for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    SheenaBanquetTerraceCompanionSelected = 1703,
    /// Meltokio 401/402 select Zelos for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    ZelosBanquetTerraceCompanionSelected = 1704,
    /// Meltokio 401/402 select Presea for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    PreseaBanquetTerraceCompanionSelected = 1705,
    /// Meltokio 401/402 select Regal for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    RegalBanquetTerraceCompanionSelected = 1706,
    /// Meltokio 401/402 set this after Colette's first banquet outfit/title
    /// conversation, which grants FairLady; subsequent interactions keep repeat
    /// dialogue. Room 399 clears it in the banquet aftermath, so it is a transient
    /// conversation latch rather than permanent title ownership.
    ColetteBanquetTitleConversationSeen = 1707,
    /// Meltokio 401/402 set this after Genis's first banquet outfit/title conversation,
    /// which grants EasterSunday; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    GenisBanquetTitleConversationSeen = 1708,
    /// Meltokio 401/402 set this after Raine's first banquet outfit/title conversation,
    /// which grants GlamorousBeauty; subsequent interactions keep repeat dialogue. Room
    /// 399 clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    RaineBanquetTitleConversationSeen = 1709,
    /// Meltokio 401/402 set this after Sheena's first banquet outfit/title conversation,
    /// which grants YouLookGreat; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    SheenaBanquetTitleConversationSeen = 1710,
    /// Meltokio 401/402 set this after Zelos's first banquet outfit/title conversation,
    /// which grants Narcissist; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    ZelosBanquetTitleConversationSeen = 1711,
    /// Meltokio 401/402 set this after Presea's first banquet outfit/title conversation,
    /// which grants LittleMadam; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    PreseaBanquetTitleConversationSeen = 1712,
    /// Meltokio 401/402 set this after Regal's first banquet outfit/title conversation,
    /// which grants Dandy; subsequent interactions keep repeat dialogue. Room 399 clears
    /// it in the banquet aftermath, so it is a transient conversation latch rather than
    /// permanent title ownership.
    RegalBanquetTitleConversationSeen = 1713,
} }

ids! { pub enum Field0400Flag: u16 {
    /// The corresponding upstairs doorway in Meltokio 399 sets this before entering
    /// shared room 400. Entry selects its scenery and exit record, whose handler clears
    /// the mark and returns to 399 at (292,396,425). Right/left refer to local-X door
    /// positions. The two flags retain independent tests and are not replaced with an
    /// exclusive selector.
    UpstairsRightRoomSelected = 180,
    /// The corresponding upstairs doorway in Meltokio 399 sets this before entering
    /// shared room 400. Entry selects its scenery and exit record, whose handler clears
    /// the mark and returns to 399 at (-287,396,425). Right/left refer to local-X door
    /// positions. The two flags retain independent tests and are not replaced with an
    /// exclusive selector. This left-room selection also gates the Kate/Zelos prejudice
    /// conversation under its other quest/party conditions; that scene and retained
    /// unused_25FF clear it before returning. The conversation's permanent story mark
    /// remains separate.
    UpstairsLeftRoomSelected = 181,
} }

ids! { pub enum Field0401Flag: u16 {
    /// Meltokio 401/402 select Colette for the banquet terrace, either by explicit
    /// choice or highest-affinity fallback, then use this bit for the corresponding
    /// terrace scene. Room 402 omits the selected companion from its ordinary hall
    /// actors. The 399 aftermath clears the selector; it is not permanent title
    /// ownership. The eighth selection branch also awards affinity to Kratos in the
    /// original script.
    ColetteBanquetTerraceCompanionSelected = 1700,
    /// Meltokio 401/402 select Genis for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    GenisBanquetTerraceCompanionSelected = 1701,
    /// Meltokio 401/402 select Raine for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    RaineBanquetTerraceCompanionSelected = 1702,
    /// Meltokio 401/402 select Sheena for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    SheenaBanquetTerraceCompanionSelected = 1703,
    /// Meltokio 401/402 select Zelos for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    ZelosBanquetTerraceCompanionSelected = 1704,
    /// Meltokio 401/402 select Presea for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    PreseaBanquetTerraceCompanionSelected = 1705,
    /// Meltokio 401/402 select Regal for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    RegalBanquetTerraceCompanionSelected = 1706,
    /// Meltokio 401/402 set this after Colette's first banquet outfit/title
    /// conversation, which grants FairLady; subsequent interactions keep repeat
    /// dialogue. Room 399 clears it in the banquet aftermath, so it is a transient
    /// conversation latch rather than permanent title ownership.
    ColetteBanquetTitleConversationSeen = 1707,
    /// Meltokio 401/402 set this after Genis's first banquet outfit/title conversation,
    /// which grants EasterSunday; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    GenisBanquetTitleConversationSeen = 1708,
    /// Meltokio 401/402 set this after Raine's first banquet outfit/title conversation,
    /// which grants GlamorousBeauty; subsequent interactions keep repeat dialogue. Room
    /// 399 clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    RaineBanquetTitleConversationSeen = 1709,
    /// Meltokio 401/402 set this after Sheena's first banquet outfit/title conversation,
    /// which grants YouLookGreat; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    SheenaBanquetTitleConversationSeen = 1710,
    /// Meltokio 401/402 set this after Zelos's first banquet outfit/title conversation,
    /// which grants Narcissist; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    ZelosBanquetTitleConversationSeen = 1711,
    /// Meltokio 401/402 set this after Presea's first banquet outfit/title conversation,
    /// which grants LittleMadam; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    PreseaBanquetTitleConversationSeen = 1712,
    /// Meltokio 401/402 set this after Regal's first banquet outfit/title conversation,
    /// which grants Dandy; subsequent interactions keep repeat dialogue. Room 399 clears
    /// it in the banquet aftermath, so it is a transient conversation latch rather than
    /// permanent title ownership.
    RegalBanquetTitleConversationSeen = 1713,
} }

ids! { pub enum Field0402Flag: u16 {
    /// Meltokio 401/402 select Colette for the banquet terrace, either by explicit
    /// choice or highest-affinity fallback, then use this bit for the corresponding
    /// terrace scene. Room 402 omits the selected companion from its ordinary hall
    /// actors. The 399 aftermath clears the selector; it is not permanent title
    /// ownership. The eighth selection branch also awards affinity to Kratos in the
    /// original script.
    ColetteBanquetTerraceCompanionSelected = 1700,
    /// Meltokio 401/402 select Genis for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    GenisBanquetTerraceCompanionSelected = 1701,
    /// Meltokio 401/402 select Raine for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    RaineBanquetTerraceCompanionSelected = 1702,
    /// Meltokio 401/402 select Sheena for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    SheenaBanquetTerraceCompanionSelected = 1703,
    /// Meltokio 401/402 select Zelos for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    ZelosBanquetTerraceCompanionSelected = 1704,
    /// Meltokio 401/402 select Presea for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    PreseaBanquetTerraceCompanionSelected = 1705,
    /// Meltokio 401/402 select Regal for the banquet terrace, either by explicit choice
    /// or highest-affinity fallback, then use this bit for the corresponding terrace
    /// scene. Room 402 omits the selected companion from its ordinary hall actors. The
    /// 399 aftermath clears the selector; it is not permanent title ownership. The
    /// eighth selection branch also awards affinity to Kratos in the original script.
    RegalBanquetTerraceCompanionSelected = 1706,
    /// Meltokio 401/402 set this after Colette's first banquet outfit/title
    /// conversation, which grants FairLady; subsequent interactions keep repeat
    /// dialogue. Room 399 clears it in the banquet aftermath, so it is a transient
    /// conversation latch rather than permanent title ownership.
    ColetteBanquetTitleConversationSeen = 1707,
    /// Meltokio 401/402 set this after Genis's first banquet outfit/title conversation,
    /// which grants EasterSunday; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    GenisBanquetTitleConversationSeen = 1708,
    /// Meltokio 401/402 set this after Raine's first banquet outfit/title conversation,
    /// which grants GlamorousBeauty; subsequent interactions keep repeat dialogue. Room
    /// 399 clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    RaineBanquetTitleConversationSeen = 1709,
    /// Meltokio 401/402 set this after Sheena's first banquet outfit/title conversation,
    /// which grants YouLookGreat; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    SheenaBanquetTitleConversationSeen = 1710,
    /// Meltokio 401/402 set this after Zelos's first banquet outfit/title conversation,
    /// which grants Narcissist; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    ZelosBanquetTitleConversationSeen = 1711,
    /// Meltokio 401/402 set this after Presea's first banquet outfit/title conversation,
    /// which grants LittleMadam; subsequent interactions keep repeat dialogue. Room 399
    /// clears it in the banquet aftermath, so it is a transient conversation latch
    /// rather than permanent title ownership.
    PreseaBanquetTitleConversationSeen = 1712,
    /// Meltokio 401/402 set this after Regal's first banquet outfit/title conversation,
    /// which grants Dandy; subsequent interactions keep repeat dialogue. Room 399 clears
    /// it in the banquet aftermath, so it is a transient conversation latch rather than
    /// permanent title ownership.
    RegalBanquetTitleConversationSeen = 1713,
} }

ids! { pub enum Field0411Flag: u16 {
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 170 stages Lloyd. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior. Room 411's event-test choice also sets this
    /// Lloyd bit and quest word 0x0128 to 5000 without clearing the other entrant bits.
    LloydColiseumDebutEntrantSelected = 170,
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 171 stages Colette. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior.
    ColetteColiseumDebutEntrantSelected = 171,
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 172 stages Genis. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior.
    GenisColiseumDebutEntrantSelected = 172,
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 173 stages Raine. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior.
    RaineColiseumDebutEntrantSelected = 173,
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 174 stages Sheena. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior.
    SheenaColiseumDebutEntrantSelected = 174,
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 175 stages Zelos. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior.
    ZelosColiseumDebutEntrantSelected = 175,
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 176 stages Regal. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior.
    RegalColiseumDebutEntrantSelected = 176,
    /// The coliseum debut entrant flags from rooms 390/391 select the matching
    /// Kate-rescue dialogue in room 411; bit 177 stages Presea. The order retains Regal
    /// at 176 and Presea at 177. Each branch temporarily selects that character and
    /// restores the saved party/leader afterward. The original branch tests are
    /// independent and do not clear the selection, so multiple set bits must retain
    /// their repeated staging behavior.
    PreseaColiseumDebutEntrantSelected = 177,
} }

ids! { pub enum Field0412Flag: u16 {
    /// Ring-contact actor 200 toggles this cable-switch state and separately sets the
    /// permanent ToizeMineCartSwitchDiscovered flag. Entry restores the switch pose.
    /// When set, lift actor 101 takes the extra curved branch to (3004,3450,-150) and
    /// back before continuing its normal loop; when clear that detour is skipped.
    /// Dialogue 31/35/40 describes the split lift cable, old tunnel and lever. This is
    /// the current route setting, not discovery or a permanently opened route.
    Lift101BranchDetourEnabled = 150,
    /// Interaction with control actor 202 toggles this, light node moa_d00_light00 and
    /// rotating actor 203. Entry and the toggle both restore the same lift-101 duration
    /// table: clear uses 325/60/60/162/162/60/60/275/275/325, set uses
    /// 108/20/20/54/54/20/20/91/91/108. Preserve these exact rounded durations rather
    /// than deriving them by division. The route bit 150 is independent.
    Lift101FastModeEnabled = 151,
} }

ids! { pub enum Field0413Flag: u16 {
    /// Room 413 entry sets scenery node moa_d01_light00 visible exactly when this bit is
    /// set and pauses control actor 105 while clear. Its interaction toggles the same
    /// node, actor animation and bit. This is that tunnel light/control state; the
    /// boulder trap, bombed obstacles and treasure milestones use separate flags.
    TunnelLight00Enabled = 152,
    /// Room 413 entry sets scenery node moa_d01_light01 visible exactly when this bit is
    /// set and pauses control actor 106 while clear. Its interaction toggles the same
    /// node, actor animation and bit. This is that tunnel light/control state; the
    /// boulder trap, bombed obstacles and treasure milestones use separate flags.
    TunnelLight01Enabled = 153,
} }

ids! { pub enum Field0425Flag: u16 {
    /// The two story-11806000 approach triggers select the same Papal Knight
    /// confrontation. Record 5999 at Z 626/627 sets this for the lower staging with
    /// Lloyd at Z 628; record 5998 at Z 807 clears it for the upper staging with Lloyd
    /// at Z 807. The helper reads it for camera/party placement, not battle victory or
    /// Presea awakening.
    PapalKnightSceneLowerApproachSelected = 170,
} }

ids! { pub enum Field0434Flag: u16 {
    /// Sybak lost-child inquiry: record 301, I do not know any dirty kid like this. The
    /// response sets this latch during the active search. The original 1701 branch
    /// assigns stage 130001, rather than incrementing the current value. Preserve the
    /// 1701 reset and the separate 1712/1713 mismatch in room 436.
    LostChildDenialAtRecord301Heard = 1701,
    /// Sybak lost-child inquiry: record 302, do not bring the filthy child near my
    /// products. The response sets this latch during the active search. With the bit
    /// clear the original branch increments local stage word 0x013c; later repeats do
    /// not increment it. Preserve the 1701 reset and the separate 1712/1713 mismatch in
    /// room 436.
    LostChildProductsComplaintHeard = 1702,
    /// Sybak lost-child inquiry: record 203, I do not know and please leave. The
    /// response sets this latch during the active search. With the bit clear the
    /// original branch increments local stage word 0x013c; later repeats do not
    /// increment it. Preserve the 1701 reset and the separate 1712/1713 mismatch in room
    /// 436.
    LostChildDismissalAtRecord203Heard = 1703,
    /// Sybak lost-child inquiry: record 20, goodness what a dirty child. The response
    /// sets this latch during the active search. With the bit clear the original branch
    /// increments local stage word 0x013c; later repeats do not increment it. Preserve
    /// the 1701 reset and the separate 1712/1713 mismatch in room 436.
    LostChildDirtyChildRemarkHeard = 1704,
    /// Sybak lost-child inquiry: record 21, has the child ever taken a bath. The
    /// response sets this latch during the active search. With the bit clear the
    /// original branch increments local stage word 0x013c; later repeats do not
    /// increment it. Preserve the 1701 reset and the separate 1712/1713 mismatch in room
    /// 436.
    LostChildBathComplaintHeard = 1705,
    /// Sybak lost-child inquiry: record 22, does this child from the Meltokio slums have
    /// permission. The response sets this latch during the active search. With the bit
    /// clear the original branch increments local stage word 0x013c; later repeats do
    /// not increment it. Preserve the 1701 reset and the separate 1712/1713 mismatch in
    /// room 436.
    LostChildEntryPermissionQuestionHeard = 1706,
    /// Sybak lost-child inquiry: record 23, the smell will stick so keep the child away.
    /// The response sets this latch during the active search. With the bit clear the
    /// original branch increments local stage word 0x013c; later repeats do not
    /// increment it. Preserve the 1701 reset and the separate 1712/1713 mismatch in room
    /// 436.
    LostChildOdorComplaintHeard = 1707,
    /// Sybak lost-child inquiry: record 24, what would I know about this child's mother.
    /// The response sets this latch during the active search. With the bit clear the
    /// original branch increments local stage word 0x013c; later repeats do not
    /// increment it. Preserve the 1701 reset and the separate 1712/1713 mismatch in room
    /// 436.
    LostChildMotherInquiryHeard = 1708,
    /// Sybak lost-child inquiry: record 25, do not enter the Academy because the smell
    /// will spread. The response sets this latch during the active search. With the bit
    /// clear the original branch increments local stage word 0x013c; later repeats do
    /// not increment it. Preserve the 1701 reset and the separate 1712/1713 mismatch in
    /// room 436.
    LostChildAcademyEntryWarningHeard = 1709,
} }

ids! { pub enum Field0436Flag: u16 {
    /// The helper shared by interaction records 58/59 hears the demand to take the dirty
    /// child away, increments local search stage once while this latch is clear, and
    /// sets it. The other inquiry responses use separate latches.
    LostChildTakeHimAwayRemarkHeard = 1710,
    /// Interaction record 20 hears the suggestion to ask someone poor, increments local
    /// search stage while this latch is clear, and sets it.
    LostChildAskSomeonePoorRemarkHeard = 1711,
    /// Interaction record 21 tests bit 1712 before incrementing local search stage for
    /// its Meltokio-slums remark, but the retail script writes 1713 afterward. No
    /// literal writer sets 1712 in this scene. Preserve this mismatch rather than
    /// turning it into a functioning once-only inquiry latch.
    LostChildSlumsRemarkCountSuppressed = 1712,
    /// The retail record-21 slums remark writes bit 1713 although its increment guard
    /// tests 1712. Room 445 uses this same 1713 bit to suppress counting its
    /// library-entry refusal. Preserve the resulting cross-room coupling.
    LibraryInquiryCountSuppressionMarked = 1713,
} }

ids! { pub enum Field0437Flag: u16 {
    /// Sybak uses shared hall resource 438 for different layouts. This mode selects the
    /// lower entrance to room 437 at (-476,-347,2), room-439 door variants, and the
    /// passage to the hall selected by 163. Hall entry restores its own scenery, NPC
    /// group and passage records without consuming the selector. The relevant outer exit
    /// clears it; transitions between the 161/163 halls set one and clear the other. The
    /// 6003 transition is blocked at early story stages 10406000/10406500, so retain
    /// those guards and all independent mode tests.
    HallTo0437LowerSelected = 161,
    /// Sybak uses shared hall resource 438 for different layouts. This mode selects the
    /// upper entrance to room 437 at (-482,367,525) and room-440 door variants. Hall
    /// entry restores its own scenery, NPC group and passage records without consuming
    /// the selector. The relevant outer exit clears it; transitions between the 161/163
    /// halls set one and clear the other. The 6003 transition is blocked at early story
    /// stages 10406000/10406500, so retain those guards and all independent mode tests.
    HallTo0437UpperSelected = 162,
    /// Room-438 door event 6011 sets this before entering shared room 439. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (85,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration. The initial
    /// scholar-guided Key Crest visit from room 437 directly selects this variant
    /// together with hall bit 161; room 439 also tests it before its story-stage scene
    /// dispatch.
    Room0439CenterDoorSelected = 172,
} }

ids! { pub enum Field0438Flag: u16 {
    /// Sybak uses shared hall resource 438 for different layouts. This mode selects the
    /// lower entrance to room 437 at (-476,-347,2), room-439 door variants, and the
    /// passage to the hall selected by 163. Hall entry restores its own scenery, NPC
    /// group and passage records without consuming the selector. The relevant outer exit
    /// clears it; transitions between the 161/163 halls set one and clear the other. The
    /// 6003 transition is blocked at early story stages 10406000/10406500, so retain
    /// those guards and all independent mode tests.
    HallTo0437LowerSelected = 161,
    /// Sybak uses shared hall resource 438 for different layouts. This mode selects the
    /// upper entrance to room 437 at (-482,367,525) and room-440 door variants. Hall
    /// entry restores its own scenery, NPC group and passage records without consuming
    /// the selector. The relevant outer exit clears it; transitions between the 161/163
    /// halls set one and clear the other. The 6003 transition is blocked at early story
    /// stages 10406000/10406500, so retain those guards and all independent mode tests.
    HallTo0437UpperSelected = 162,
    /// Sybak uses shared hall resource 438 for different layouts. This mode selects room
    /// 442 at (1065,9,400), room-441 door variants, and the passage to the hall selected
    /// by 161. Hall entry restores its own scenery, NPC group and passage records
    /// without consuming the selector. The relevant outer exit clears it; transitions
    /// between the 161/163 halls set one and clear the other. The 6003 transition is
    /// blocked at early story stages 10406000/10406500, so retain those guards and all
    /// independent mode tests.
    HallTo0442Selected = 163,
    /// Room-438 door event 6010 sets this before entering shared room 439. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (956,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration. This variant
    /// also contains the Zircon storage pickup, gated by its separate story/unlock and
    /// inventory checks.
    Room0439RightDoorSelected = 171,
    /// Room-438 door event 6011 sets this before entering shared room 439. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (85,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration. The initial
    /// scholar-guided Key Crest visit from room 437 directly selects this variant
    /// together with hall bit 161; room 439 also tests it before its story-stage scene
    /// dispatch.
    Room0439CenterDoorSelected = 172,
    /// Room-438 door event 6012 sets this before entering shared room 439. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (-736,65,2).
    /// Right/center/left follow the hall's local X. The hall-layout bits remain
    /// separate, and the original independent tests are not replaced by an exclusive
    /// room enumeration.
    Room0439LeftDoorSelected = 173,
    /// Room-438 door event 6013 sets this before entering shared room 440. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (956,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0440RightDoorSelected = 174,
    /// Room-438 door event 6014 sets this before entering shared room 440. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (85,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0440CenterDoorSelected = 175,
    /// Room-438 door event 6015 sets this before entering shared room 440. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (-736,65,2).
    /// Right/center/left follow the hall's local X. The hall-layout bits remain
    /// separate, and the original independent tests are not replaced by an exclusive
    /// room enumeration.
    Room0440LeftDoorSelected = 176,
    /// Room-438 door event 6016 sets this before entering shared room 441. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (956,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0441RightDoorSelected = 177,
    /// Room-438 door event 6017 sets this before entering shared room 441. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (85,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0441CenterDoorSelected = 178,
    /// Room-438 door event 6018 sets this before entering shared room 441. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (-736,65,2).
    /// Right/center/left follow the hall's local X. The hall-layout bits remain
    /// separate, and the original independent tests are not replaced by an exclusive
    /// room enumeration.
    Room0441LeftDoorSelected = 179,
} }

ids! { pub enum Field0439Flag: u16 {
    /// Room-438 door event 6010 sets this before entering shared room 439. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (956,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration. This variant
    /// also contains the Zircon storage pickup, gated by its separate story/unlock and
    /// inventory checks.
    Room0439RightDoorSelected = 171,
    /// Room-438 door event 6011 sets this before entering shared room 439. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (85,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration. The initial
    /// scholar-guided Key Crest visit from room 437 directly selects this variant
    /// together with hall bit 161; room 439 also tests it before its story-stage scene
    /// dispatch.
    Room0439CenterDoorSelected = 172,
    /// Room-438 door event 6012 sets this before entering shared room 439. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (-736,65,2).
    /// Right/center/left follow the hall's local X. The hall-layout bits remain
    /// separate, and the original independent tests are not replaced by an exclusive
    /// room enumeration.
    Room0439LeftDoorSelected = 173,
} }

ids! { pub enum Field0440Flag: u16 {
    /// Room-438 door event 6013 sets this before entering shared room 440. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (956,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0440RightDoorSelected = 174,
    /// Room-438 door event 6014 sets this before entering shared room 440. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (85,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0440CenterDoorSelected = 175,
    /// Room-438 door event 6015 sets this before entering shared room 440. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (-736,65,2).
    /// Right/center/left follow the hall's local X. The hall-layout bits remain
    /// separate, and the original independent tests are not replaced by an exclusive
    /// room enumeration.
    Room0440LeftDoorSelected = 176,
} }

ids! { pub enum Field0441Flag: u16 {
    /// Room-438 door event 6016 sets this before entering shared room 441. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (956,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0441RightDoorSelected = 177,
    /// Room-438 door event 6017 sets this before entering shared room 441. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (85,65,2). Right/center/left
    /// follow the hall's local X. The hall-layout bits remain separate, and the original
    /// independent tests are not replaced by an exclusive room enumeration.
    Room0441CenterDoorSelected = 178,
    /// Room-438 door event 6018 sets this before entering shared room 441. That room
    /// independently selects the corresponding scenery/collision and NPC variant; its
    /// exit clears the selector and returns to hall 438 at (-736,65,2).
    /// Right/center/left follow the hall's local X. The hall-layout bits remain
    /// separate, and the original independent tests are not replaced by an exclusive
    /// room enumeration.
    Room0441LeftDoorSelected = 179,
} }

ids! { pub enum Field0442Flag: u16 {
    /// Sybak uses shared hall resource 438 for different layouts. This mode selects room
    /// 442 at (1065,9,400), room-441 door variants, and the passage to the hall selected
    /// by 161. Hall entry restores its own scenery, NPC group and passage records
    /// without consuming the selector. The relevant outer exit clears it; transitions
    /// between the 161/163 halls set one and clear the other. The 6003 transition is
    /// blocked at early story stages 10406000/10406500, so retain those guards and all
    /// independent mode tests.
    HallTo0442Selected = 163,
} }

ids! { pub enum Field0445Flag: u16 {
    /// The library-entry refusal increments local search stage only while 1713 is clear,
    /// then sets it. Room 436's record-21 remark also writes 1713 while testing 1712, so
    /// this bit does not prove the library conversation has already occurred.
    LostChildLibraryInquiryCountSuppressed = 1713,
} }

ids! { pub enum Field0447Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit does nothing; only exits 455/456 contain the extra
    /// statue-setting branches. Do not unify the eight exit implementations or treat
    /// this request as completion of all rebuilding.
    RebuildingUpdateOnExitRequested = 199,
} }

ids! { pub enum Field0448Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit does nothing; only exits 455/456 contain the extra
    /// statue-setting branches. Do not unify the eight exit implementations or treat
    /// this request as completion of all rebuilding.
    RebuildingUpdateOnExitRequested = 199,
} }

ids! { pub enum Field0449Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit does nothing; only exits 455/456 contain the extra
    /// statue-setting branches. Do not unify the eight exit implementations or treat
    /// this request as completion of all rebuilding.
    RebuildingUpdateOnExitRequested = 199,
} }

ids! { pub enum Field0450Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit does nothing; only exits 455/456 contain the extra
    /// statue-setting branches. Do not unify the eight exit implementations or treat
    /// this request as completion of all rebuilding.
    RebuildingUpdateOnExitRequested = 199,
} }

ids! { pub enum Field0452Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit does nothing; only exits 455/456 contain the extra
    /// statue-setting branches. Do not unify the eight exit implementations or treat
    /// this request as completion of all rebuilding.
    RebuildingUpdateOnExitRequested = 199,
} }

ids! { pub enum Field0453Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit does nothing; only exits 455/456 contain the extra
    /// statue-setting branches. Do not unify the eight exit implementations or treat
    /// this request as completion of all rebuilding.
    RebuildingUpdateOnExitRequested = 199,
    /// The Aifread actor-67 interaction sets this before dispatching any of its
    /// lease/errand/refusal branches. Entry in both Luin variants requires it clear to
    /// spawn Aifread alongside the other quest, inventory and Gald prerequisites. It
    /// suppresses reentry spawning until reset, without proving a successful errand.
    AifreadReentrySpawnSuppressed = 1701,
} }

ids! { pub enum Field0455Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit instead marks the first missing statue in
    /// Sheena/Lloyd/Raine order, stopping after one; preserve that conditional behavior
    /// and the donation thresholds.
    RebuildingUpdateOnExitRequested = 199,
} }

ids! { pub enum Field0456Flag: u16 {
    /// The Luin donation helpers in rooms 447/449/452/455 set this at each reached
    /// donation target alongside LuinDonationGoalReached. Town exit event 1000 tests
    /// this request and advances the current LuinRebuilding stage once through
    /// 1600,2000,3000,10000,11000,12000,13000,100000,200000,300000,400000,500000,600000.
    /// It reads the stage before dispatch and does not explicitly clear 199. At final
    /// stage 600000 this exit instead marks the first missing statue in
    /// Sheena/Lloyd/Raine order, stopping after one; preserve that conditional behavior
    /// and the donation thresholds.
    RebuildingUpdateOnExitRequested = 199,
    /// The Aifread actor-67 interaction sets this before dispatching any of its
    /// lease/errand/refusal branches. Entry in both Luin variants requires it clear to
    /// spawn Aifread alongside the other quest, inventory and Gald prerequisites. It
    /// suppresses reentry spawning until reset, without proving a successful errand.
    AifreadReentrySpawnSuppressed = 1701,
} }

ids! { pub enum Field0466Flag: u16 {
    /// The 466 low exit sets this before entering 467 at Z 0; its upper exit clears it
    /// before entering at Z 775. Room 468 also sets it before entering 467 at Z 345.
    /// Room 467 hides scenery nodes 40/41 when selected. Preserve the explicit latch
    /// rather than recomputing it from a modded position.
    Sewer0467LowerEntryLayoutSelected = 150,
    /// Meltokio sewer bit 151 is the player-shrink state/request latch. Ring-use
    /// handlers set it, reset the transition counter and use radius 14/session mode 12;
    /// normalizing exits and areas clear it and restore normal size/radius/session
    /// state. Passage and movement handlers read it. It is distinct from the ring itself
    /// being configured for Shrink and does not prove the size animation has finished.
    PlayerShrinkEnabled = 151,
    /// The shrunken-player ride on actor 3001 in room 467 sets this before changing to
    /// 466. Room 466 entry consumes and clears it in sub_3878 while attaching the player
    /// to continuation actor 3705; the helper has two scene-state branches. It is a
    /// transition request, not a completed puzzle.
    RideArrivalFrom0467Pending = 152,
    /// Sewer 466 sets bit 153 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the EXGemLv2 treasure passage (separate treasure flag
    /// 89), its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 466. This is a passage variant, not an opened-treasure flag.
    EXGemLv2PassageSelected = 153,
    /// Sewer 466 sets bit 154 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the GreatAx treasure passage (separate treasure flag 93),
    /// its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 466. This is a passage variant, not an opened-treasure flag.
    GreatAxPassageSelected = 154,
} }

ids! { pub enum Field0467Flag: u16 {
    /// The 466 low exit sets this before entering 467 at Z 0; its upper exit clears it
    /// before entering at Z 775. Room 468 also sets it before entering 467 at Z 345.
    /// Room 467 hides scenery nodes 40/41 when selected. Preserve the explicit latch
    /// rather than recomputing it from a modded position.
    Sewer0467LowerEntryLayoutSelected = 150,
    /// Meltokio sewer bit 151 is the player-shrink state/request latch. Ring-use
    /// handlers set it, reset the transition counter and use radius 14/session mode 12;
    /// normalizing exits and areas clear it and restore normal size/radius/session
    /// state. Passage and movement handlers read it. It is distinct from the ring itself
    /// being configured for Shrink and does not prove the size animation has finished.
    PlayerShrinkEnabled = 151,
    /// The shrunken-player ride on actor 3001 sets this before changing to 466. The
    /// destination consumes it for the continued ride and clears it; no material or
    /// object identity for the moving prop is inferred from its numeric resource slot.
    Sewer0466RideArrivalPending = 152,
    /// Sewer 467 sets bit 155 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the EXGemLv1 treasure passage (separate treasure flag
    /// 87), its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 467. This is a passage variant, not an opened-treasure flag.
    EXGemLv1PassageSelected = 155,
} }

ids! { pub enum Field0468Flag: u16 {
    /// The 466 low exit sets this before entering 467 at Z 0; its upper exit clears it
    /// before entering at Z 775. Room 468 also sets it before entering 467 at Z 345.
    /// Room 467 hides scenery nodes 40/41 when selected. Preserve the explicit latch
    /// rather than recomputing it from a modded position.
    Sewer0467LowerEntryLayoutSelected = 150,
    /// Meltokio sewer bit 151 is the player-shrink state/request latch. Ring-use
    /// handlers set it, reset the transition counter and use radius 14/session mode 12;
    /// normalizing exits and areas clear it and restore normal size/radius/session
    /// state. Passage and movement handlers read it. It is distinct from the ring itself
    /// being configured for Shrink and does not prove the size animation has finished.
    PlayerShrinkEnabled = 151,
    /// Sewer 468 sets bit 156 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the Thunderbolt treasure passage (separate treasure flag
    /// 91), its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 468. This is a passage variant, not an opened-treasure flag.
    ThunderboltPassageSelected = 156,
    /// Sewer 468 sets bit 157 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the CardOfLightning treasure passage (separate treasure
    /// flag 92), its treasure actor and its return trigger. Leaving clears the selector
    /// and returns to room 468. This is a passage variant, not an opened-treasure flag.
    CardOfLightningPassageSelected = 157,
} }

ids! { pub enum Field0469Flag: u16 {
    /// Meltokio sewer bit 151 is the player-shrink state/request latch. Ring-use
    /// handlers set it, reset the transition counter and use radius 14/session mode 12;
    /// normalizing exits and areas clear it and restore normal size/radius/session
    /// state. Passage and movement handlers read it. It is distinct from the ring itself
    /// being configured for Shrink and does not prove the size animation has finished.
    PlayerShrinkEnabled = 151,
    /// Sewer 466 sets bit 153 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the EXGemLv2 treasure passage (separate treasure flag
    /// 89), its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 466. This is a passage variant, not an opened-treasure flag.
    EXGemLv2PassageSelected = 153,
    /// Sewer 466 sets bit 154 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the GreatAx treasure passage (separate treasure flag 93),
    /// its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 466. This is a passage variant, not an opened-treasure flag.
    GreatAxPassageSelected = 154,
    /// Sewer 467 sets bit 155 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the EXGemLv1 treasure passage (separate treasure flag
    /// 87), its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 467. This is a passage variant, not an opened-treasure flag.
    EXGemLv1PassageSelected = 155,
    /// Sewer 468 sets bit 156 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the Thunderbolt treasure passage (separate treasure flag
    /// 91), its treasure actor and its return trigger. Leaving clears the selector and
    /// returns to room 468. This is a passage variant, not an opened-treasure flag.
    ThunderboltPassageSelected = 156,
    /// Sewer 468 sets bit 157 on the shrunken-player passage transfer to shared room
    /// 469. That entry selects the CardOfLightning treasure passage (separate treasure
    /// flag 92), its treasure actor and its return trigger. Leaving clears the selector
    /// and returns to room 468. This is a passage variant, not an opened-treasure flag.
    CardOfLightningPassageSelected = 157,
} }

ids! { pub enum Field0475Flag: u16 {
    /// The Lightning ring changers set only bit 150 for Blue electricity and clear the
    /// other two color bits. They clear all three for Fire. Ring-target handlers test
    /// the same color bits; the debug menu can force Yellow/Red. The alias records the
    /// current electricity selection, separately from changer-used milestones.
    BlueElectricitySelected = 150,
    /// The Lightning ring changers set only bit 151 for Yellow electricity and clear the
    /// other two color bits. They clear all three for Fire. Ring-target handlers test
    /// the same color bits; the debug menu can force Yellow/Red. The alias records the
    /// current electricity selection, separately from changer-used milestones.
    YellowElectricitySelected = 151,
    /// The Lightning ring changers set only bit 152 for Red electricity and clear the
    /// other two color bits. They clear all three for Fire. Ring-target handlers test
    /// the same color bits; the debug menu can force Yellow/Red. The alias records the
    /// current electricity selection, separately from changer-used milestones.
    RedElectricitySelected = 152,
    /// Matching Blue electricity removes block actor 301 in 475 and marks bit 157; its
    /// separate collision actor is removed when present. These scratch flags are
    /// distinct from the permanent stacked-block and approach flags. The original 304
    /// write has no literal restoration reader; preserve that difference rather than
    /// adding one.
    Block301Destroyed = 157,
    /// Matching Blue electricity removes block actor 302 in 475 and marks bit 158; its
    /// separate collision actor is removed when present. These scratch flags are
    /// distinct from the permanent stacked-block and approach flags. The original 304
    /// write has no literal restoration reader; preserve that difference rather than
    /// adding one.
    Block302Destroyed = 158,
    /// Matching Blue electricity removes block actor 304 in 475 and marks bit 160; its
    /// separate collision actor is removed when present. These scratch flags are
    /// distinct from the permanent stacked-block and approach flags. The original 304
    /// write has no literal restoration reader; preserve that difference rather than
    /// adding one.
    Block304Destroyed = 160,
    /// Matching Yellow electricity removes block actor 311 in 475 and marks bit 162; its
    /// separate collision actor is removed when present. These scratch flags are
    /// distinct from the permanent stacked-block and approach flags. The original 304
    /// write has no literal restoration reader; preserve that difference rather than
    /// adding one.
    Block311Destroyed = 162,
    /// Matching Yellow electricity removes block actor 312 in 475 and marks bit 163; its
    /// separate collision actor is removed when present. These scratch flags are
    /// distinct from the permanent stacked-block and approach flags. The original 304
    /// write has no literal restoration reader; preserve that difference rather than
    /// adding one.
    Block312Destroyed = 163,
    /// Matching Red electricity removes block actor 321 in 475 and marks bit 167; its
    /// separate collision actor is removed when present. These scratch flags are
    /// distinct from the permanent stacked-block and approach flags. The original 304
    /// write has no literal restoration reader; preserve that difference rather than
    /// adding one.
    Block321Destroyed = 167,
    /// Matching Red electricity removes block actor 322 in 475 and marks bit 168; its
    /// separate collision actor is removed when present. These scratch flags are
    /// distinct from the permanent stacked-block and approach flags. The original 304
    /// write has no literal restoration reader; preserve that difference rather than
    /// adding one.
    Block322Destroyed = 168,
    /// The retail debug menu's Set necessary NPC talk ON option sets this bit alongside
    /// the rod-4112 raised flag. No literal reader in this field establishes an
    /// additional normal-play milestone; preserve the explicit debug operation.
    DebugNecessaryNpcTalkMarked = 170,
    /// The four dark-path fall-return camera helpers test this bit before their
    /// descending camera motion, rumble and release of the temporary actor-4301
    /// animation. A set bit suppresses those cinematic portions. No literal writer here
    /// establishes how this switch is enabled.
    DarkPathFallCinematicSuppressed = 194,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 196 chooses the LowerRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y -3750. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerRightPassageSelected = 196,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 197 chooses the LowerLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y -3750. Lower/upper and left/right refer
    /// to local coordinate groups, not world compass or camera orientation. A passage
    /// can be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerLeftPassageSelected = 197,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 198 chooses the UpperLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperLeftPassageSelected = 198,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 199 chooses the UpperRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperRightPassageSelected = 199,
} }

ids! { pub enum Field0476Flag: u16 {
    /// The Lightning ring changers set only bit 150 for Blue electricity and clear the
    /// other two color bits. They clear all three for Fire. Ring-target handlers test
    /// the same color bits; the debug menu can force Yellow/Red. The alias records the
    /// current electricity selection, separately from changer-used milestones.
    BlueElectricitySelected = 150,
    /// The Lightning ring changers set only bit 151 for Yellow electricity and clear the
    /// other two color bits. They clear all three for Fire. Ring-target handlers test
    /// the same color bits; the debug menu can force Yellow/Red. The alias records the
    /// current electricity selection, separately from changer-used milestones.
    YellowElectricitySelected = 151,
    /// The Lightning ring changers set only bit 152 for Red electricity and clear the
    /// other two color bits. They clear all three for Fire. Ring-target handlers test
    /// the same color bits; the debug menu can force Yellow/Red. The alias records the
    /// current electricity selection, separately from changer-used milestones.
    RedElectricitySelected = 152,
    /// The one-time interaction for rod actor 103 ends with its bone offset restored to
    /// zero, sets bit 172, and installs collision actor 123. Entry uses the bit to
    /// restore the raised/collision layout; with it clear the rod bone starts at -230.
    /// Retain the separate permanent rod-101/102/106 flags.
    Rod103Raised = 172,
    /// The one-time interaction for rod actor 104 ends with its bone offset restored to
    /// zero, sets bit 173, and installs collision actor 124. Entry uses the bit to
    /// restore the raised/collision layout; with it clear the rod bone starts at -230.
    /// Retain the separate permanent rod-101/102/106 flags.
    Rod104Raised = 173,
    /// The one-time interaction for rod actor 105 ends with its bone offset restored to
    /// zero, sets bit 174, and installs collision actor 125. Entry uses the bit to
    /// restore the raised/collision layout; with it clear the rod bone starts at -230.
    /// Retain the separate permanent rod-101/102/106 flags.
    Rod105Raised = 174,
    /// Water electrified by raised rod 101 runs the controlled character's
    /// electric-shock response and records this bit. Attempting that dangerous rod later
    /// also reads it before the warning/response branches. The party's explanation uses
    /// a separate permanent flag.
    ElectrifiedWaterShockReactionSeen = 176,
    /// Matching Blue electricity removes block actor 301 in 476 and sets bit 177; entry
    /// omits that block when the bit is set and restores any associated collision state.
    /// The scratch destruction flags are separate from the machinery-power and rod-106
    /// landing milestones.
    Block301Destroyed = 177,
    /// Matching Blue electricity removes block actor 303 in 476 and sets bit 179; entry
    /// omits that block when the bit is set and restores any associated collision state.
    /// The scratch destruction flags are separate from the machinery-power and rod-106
    /// landing milestones.
    Block303Destroyed = 179,
    /// Matching Blue electricity removes block actor 304 in 476 and sets bit 180; entry
    /// omits that block when the bit is set and restores any associated collision state.
    /// The scratch destruction flags are separate from the machinery-power and rod-106
    /// landing milestones.
    Block304Destroyed = 180,
    /// Matching Blue electricity removes block actor 305 in 476 and sets bit 181; entry
    /// omits that block when the bit is set and restores any associated collision state.
    /// The scratch destruction flags are separate from the machinery-power and rod-106
    /// landing milestones.
    Block305Destroyed = 181,
    /// Matching Blue electricity removes block actor 306 in 476 and sets bit 182; entry
    /// omits that block when the bit is set and restores any associated collision state.
    /// The scratch destruction flags are separate from the machinery-power and rod-106
    /// landing milestones.
    Block306Destroyed = 182,
    /// Matching Red electricity removes block actor 321 in 476 and sets bit 184; entry
    /// omits that block when the bit is set and restores any associated collision state.
    /// The scratch destruction flags are separate from the machinery-power and rod-106
    /// landing milestones.
    Block321Destroyed = 184,
    /// Matching Red electricity removes block actor 322 in 476 and sets bit 185; entry
    /// omits that block when the bit is set and restores any associated collision state.
    /// The scratch destruction flags are separate from the machinery-power and rod-106
    /// landing milestones.
    Block322Destroyed = 185,
    /// After the water-shock state, the rod-101 helper can run Genis and Raine's
    /// explanation that this rod electrifies the water and should not be raised. The
    /// helper sets this bit after the conversation; later attempts can use the shorter
    /// warning.
    Rod101WaterWarningHeard = 187,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 196 chooses the LowerRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y -3750. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerRightPassageSelected = 196,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 197 chooses the LowerLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y -3750. Lower/upper and left/right refer
    /// to local coordinate groups, not world compass or camera orientation. A passage
    /// can be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerLeftPassageSelected = 197,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 198 chooses the UpperLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperLeftPassageSelected = 198,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 199 chooses the UpperRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperRightPassageSelected = 199,
} }

ids! { pub enum Field0477Flag: u16 {
    /// The shared Lightning ring changers in 475/476 set bit 151 for Yellow electricity
    /// and clear it for the other settings. Room 477 tests it during Lightning-ring
    /// target handling; this is a current color selector, not a permanent ring-upgrade
    /// flag.
    YellowElectricitySelected = 151,
    /// With machinery-power flag clear, Lloyd, Genis and Raine discuss the huge inactive
    /// machine and a possible remote control apparatus, then set this bit. Later
    /// unpowered interactions show only the short not-on response; the powered-machine
    /// introduction is separate.
    UnpoweredMachineDiscussionSeen = 189,
    /// The machine result branch with local result 1 runs the first
    /// look-around/head-turn/exclamation sequence around the result-1 prop animation and
    /// sets this bit. Repeats still perform the result behavior and dialogue but omit
    /// those first reaction gestures. This does not mark the machinery puzzle solved.
    MachineResultReactionIntroduced = 191,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 196 chooses the LowerRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y -3750. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerRightPassageSelected = 196,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 197 chooses the LowerLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y -3750. Lower/upper and left/right refer
    /// to local coordinate groups, not world compass or camera orientation. A passage
    /// can be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerLeftPassageSelected = 197,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 198 chooses the UpperLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperLeftPassageSelected = 198,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 199 chooses the UpperRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperRightPassageSelected = 199,
} }

ids! { pub enum Field0478Flag: u16 {
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 196 chooses the LowerRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y -3750. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerRightPassageSelected = 196,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 197 chooses the LowerLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y -3750. Lower/upper and left/right refer
    /// to local coordinate groups, not world compass or camera orientation. A passage
    /// can be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathLowerLeftPassageSelected = 197,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 198 chooses the UpperLeft passage variant in shared room 478, with
    /// local helper positions around X -700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperLeftPassageSelected = 198,
    /// The dark-path transfers select exactly one of bits 196..199 and clear the other
    /// three. Bit 199 chooses the UpperRight passage variant in shared room 478, with
    /// local helper positions around X 700, Y 2160. Lower/upper and left/right refer to
    /// local coordinate groups, not world compass or camera orientation. A passage can
    /// be entered from either end; ordinary exits clear all selectors, while fall
    /// returns retain one for the return layout/cinematic in 475.
    DarkPathUpperRightPassageSelected = 199,
} }

ids! { pub enum Field0479Flag: u16 {
    /// The forest entry omits the bush-baby actor 201 and its introduction trigger after
    /// this latch is set. Trigger 6100 sets it at the start of the movement/emote
    /// sequence and removes the trigger. The model resource is shared with the later
    /// forest chase; this does not mark successful tracking or a permanent quest
    /// completion.
    BushBabyIntroductionTriggered = 199,
} }

ids! { pub enum Field0481Flag: u16 {
    /// The retained unused_046F helper in forest 482 sets this when clearing its
    /// route-190 state. Entry in 481 clears the bit and displays Lost sight of the bush
    /// baby. The alias records the pending notice without claiming the unused helper is
    /// reached in normal play.
    BushBabyLostNoticePending = 197,
} }

ids! {
    /// Known uses of shared session bits in Heimdall 0482.
    /// Unidentified bits 191, 192 and 193 are tested and cleared by retained helper
    /// unused_046F. Those branches have no other effects and do not establish the
    /// bits' set-state meanings; they remain numeric.
    pub enum Field0482Flag: u16 {
    /// Retained unused_046F consumes this mark before setting the separate pending
    /// notice at 197. Room 481 consumes 197 and displays Lost sight of the bush baby.
    /// This names that reader's eligibility role, not an unrecovered producer state
    /// or proof that the retained helper is reached in normal play. The outer 196
    /// guard can suppress the whole helper independently.
    BushBabyLostNoticeEnabled = 190,
    /// With both suppression bits 195/196 clear, unused_046F tests this mark,
    /// clears it, removes touch triggers 6100 and despawns actor 201. A later test
    /// can also clear it without removing the actor. This names the cleanup gate;
    /// no setter, actor identity or completed-chase state is inferred.
    RetainedActor201CleanupEnabled = 194,
    /// A set mark skips only the actor-201/trigger-6100 removal block. With 196
    /// clear, the later notice and state cleanup still run and consume 195.
    /// This is a reader role in unused_046F, not a recovered producer milestone.
    RetainedActor201CleanupSuppressed = 195,
    /// A set mark bypasses the entire retained unused_046F helper, preserving its
    /// state and suppressing both actor cleanup and the lost-notice handoff.
    /// No original setter or normal-play reachability is established.
    RetainedCleanupSuppressed = 196,
    /// The retained unused_046F helper in forest 482 sets this when clearing its
    /// route-190 state. Entry in 481 clears the bit and displays Lost sight of the bush
    /// baby. The alias records the pending notice without claiming the unused helper is
    /// reached in normal play.
    BushBabyLostNoticePending = 197,
} }

ids! { pub enum Field0485Flag: u16 {
    /// World region event 3 sets this before entering Triet 485. Triet entry checks it
    /// among the early Raine Sisterly Love prerequisites, then always clears it before
    /// returning control. It does not mean that the title event completed. Later Derris
    /// routes reuse this same session bit.
    WorldEntranceArrivalPending = 1749,
} }

ids! { pub enum Field0495Flag: u16 {
    /// Latheon 497 sets this before returning to 495; the hidden-cave discussion in 495
    /// also sets it before reloading that room. Entry 495 uses auxiliary camera end
    /// distance 7500 instead of 5000 and transition value 3450 instead of 2300, then
    /// clears the bit. No additional waterfall/puzzle-completion state is implied.
    WideEntryCameraPending = 168,
} }

ids! { pub enum Field0497Flag: u16 {
    /// Latheon 497 sets this before returning to 495; the hidden-cave discussion in 495
    /// also sets it before reloading that room. Entry 495 uses auxiliary camera end
    /// distance 7500 instead of 5000 and transition value 3450 instead of 2300, then
    /// clears the bit. No additional waterfall/puzzle-completion state is implied.
    Latheon0495WideEntryCameraPending = 168,
} }

ids! {
    /// Known uses of shared session bits in Tower of Salvation 0502.
    /// Unidentified bit 155 is cleared by both Derris-Kharlan exit handlers at
    /// word PCs 0xF47 and 0xF79. No set-state meaning has been recovered here.
    /// Equal bits in other fields have separate meanings; this operand remains numeric.
    pub enum Field0502Flag: u16 {
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
    /// Region 522 in room 502 stages Genis pointing out a giant transporter, Raine
    /// identifying it and Lloyd wondering whether it returns to the ground, then sets
    /// this and removes the trigger. Entry requires the bit clear before restoring the
    /// hint trigger. Room 503 helper sub_5927 clears it at story stage 21605000 before
    /// changing to room 153; it is a repeatable hint marker, not a permanent discovery
    /// flag.
    GiantTransporterHintShown = 180,
    /// The 502 event entering the north end of 0530 sets this; the event entering its
    /// south end clears it. Bit 1749 selects the 0530 corridor connections: set means
    /// the north portal leads to Salvation 502 and south to Derris 129; clear means
    /// north to Derris 130 and south to Salvation 502. These are alternate routes, not
    /// disabled portals.
    DerrisKharlan0530RouteVia0129Selected = 1749,
} }

ids! { pub enum Field0503Flag: u16 {
    /// Region 522 in room 502 stages Genis pointing out a giant transporter, Raine
    /// identifying it and Lloyd wondering whether it returns to the ground, then sets
    /// this and removes the trigger. Entry requires the bit clear before restoring the
    /// hint trigger. Room 503 helper sub_5927 clears it at story stage 21605000 before
    /// changing to room 153; it is a repeatable hint marker, not a permanent discovery
    /// flag.
    GiantTransporterHintShown = 180,
} }

ids! { pub enum Field0505Flag: u16 {
    /// Room 505's Choose a floor menu maps this bit to 1F. A successful selection clears
    /// the other four floor bits, sets this and swaps the floor-specific angel
    /// population; selecting the already marked floor is rejected. The five floor tests
    /// remain independent in the original code. Entry at (-6,-273) also sets floor 1
    /// without clearing other floor bits, and the normal exit clears 150. Floor 1
    /// removes exit blocker 410. Room 506 combines this with its left-side entrance
    /// marker to show the Apocalypse book.
    ElevatorFloor1Selected = 150,
    /// Room 505's Choose a floor menu maps this bit to 2F. A successful selection clears
    /// the other four floor bits, sets this and swaps the floor-specific angel
    /// population; selecting the already marked floor is rejected. The five floor tests
    /// remain independent in the original code. Floors 2..5 restore exit blocker 410.
    /// Entry and menu changes retain their original differing story-stage limits for the
    /// angel population.
    ElevatorFloor2Selected = 151,
    /// Room 505's Choose a floor menu maps this bit to 3F. A successful selection clears
    /// the other four floor bits, sets this and swaps the floor-specific angel
    /// population; selecting the already marked floor is rejected. The five floor tests
    /// remain independent in the original code. Floors 2..5 restore exit blocker 410.
    /// Entry and menu changes retain their original differing story-stage limits for the
    /// angel population.
    ElevatorFloor3Selected = 152,
    /// Room 505's Choose a floor menu maps this bit to 4F. A successful selection clears
    /// the other four floor bits, sets this and swaps the floor-specific angel
    /// population; selecting the already marked floor is rejected. The five floor tests
    /// remain independent in the original code. Floors 2..5 restore exit blocker 410.
    /// Entry and menu changes retain their original differing story-stage limits for the
    /// angel population.
    ElevatorFloor4Selected = 153,
    /// Room 505's Choose a floor menu maps this bit to 5F. A successful selection clears
    /// the other four floor bits, sets this and swaps the floor-specific angel
    /// population; selecting the already marked floor is rejected. The five floor tests
    /// remain independent in the original code. Floors 2..5 restore exit blocker 410.
    /// Entry and menu changes retain their original differing story-stage limits for the
    /// angel population.
    ElevatorFloor5Selected = 154,
    /// Room 505's left passage 401 sets this before entering shared side room 506; its
    /// right passage 402 clears it. Room 506 returns to X -676 when set or +676 when
    /// clear. Floor 1 plus the left entrance enables the Apocalypse book. This is an
    /// entrance selector, separately from which elevator floor is selected and from the
    /// acquired Devil's Arm flag.
    SideRoomLeftEntranceSelected = 155,
} }

ids! { pub enum Field0506Flag: u16 {
    /// Room 505's Choose a floor menu maps this bit to 1F. A successful selection clears
    /// the other four floor bits, sets this and swaps the floor-specific angel
    /// population; selecting the already marked floor is rejected. The five floor tests
    /// remain independent in the original code. Entry at (-6,-273) also sets floor 1
    /// without clearing other floor bits, and the normal exit clears 150. Floor 1
    /// removes exit blocker 410. Room 506 combines this with its left-side entrance
    /// marker to show the Apocalypse book.
    ElevatorFloor1Selected = 150,
    /// Room 505's left passage 401 sets this before entering shared side room 506; its
    /// right passage 402 clears it. Room 506 returns to X -676 when set or +676 when
    /// clear. Floor 1 plus the left entrance enables the Apocalypse book. This is an
    /// entrance selector, separately from which elevator floor is selected and from the
    /// acquired Devil's Arm flag.
    SideRoomLeftEntranceSelected = 155,
} }

ids! { pub enum Field0508Flag: u16 {
    /// Fire-mode contact lights torch 3000, creates flame emitter 5700 and switches its
    /// scenery nodes; Wind contact extinguishes it. Entry restores this state and the
    /// matching inscription can be read only while lit. Set means the recorded lit state
    /// and clear means extinguished. The lighting helper records the bit only when
    /// obstruction actor 5000 is absent; temporary flame effects do not prove the stored
    /// bit was set.
    Torch3000Lit = 150,
    /// Fire-mode contact lights torch 3001, creates flame emitter 5701 and switches its
    /// scenery nodes; Wind contact extinguishes it. Entry restores this state and the
    /// matching inscription can be read only while lit. Set means the recorded lit state
    /// and clear means extinguished. The lighting helper records the bit only when
    /// obstruction actor 5001 is absent; temporary flame effects do not prove the stored
    /// bit was set.
    Torch3001Lit = 151,
    /// Fire-mode contact lights torch 3002, creates flame emitter 5702 and switches its
    /// scenery nodes; Wind contact extinguishes it. Entry restores this state and the
    /// matching inscription can be read only while lit. Set means the recorded lit state
    /// and clear means extinguished.
    Torch3002Lit = 152,
    /// Fire-mode contact lights torch 3003, creates flame emitter 5703 and switches its
    /// scenery nodes; Wind contact extinguishes it. Entry restores this state and the
    /// matching inscription can be read only while lit. Set means the recorded lit state
    /// and clear means extinguished.
    Torch3003Lit = 153,
    /// Fire-mode contact lights torch 3004, creates flame emitter 5704 and switches its
    /// scenery nodes; Wind contact extinguishes it. Entry restores this state and the
    /// matching inscription can be read only while lit. Set means the recorded lit state
    /// and clear means extinguished. The lighting helper records the bit only when
    /// obstruction actor 5001 is absent; temporary flame effects do not prove the stored
    /// bit was set.
    Torch3004Lit = 154,
    /// Fire-mode contact lights torch 3005, creates flame emitter 5705 and switches its
    /// scenery nodes; Wind contact extinguishes it. Entry restores this state and the
    /// matching inscription can be read only while lit. The original polarity is
    /// inverted: clear means lit and set means extinguished. Preserve those operations
    /// rather than normalizing them.
    Torch3005Extinguished = 155,
    /// Fire-mode contact lights torch 3008, creates flame emitter 5708 and switches its
    /// scenery nodes; Wind contact extinguishes it. Entry restores this state and the
    /// matching inscription can be read only while lit. The original polarity is
    /// inverted: clear means lit and set means extinguished. Preserve those operations
    /// rather than normalizing them.
    Torch3008Extinguished = 158,
} }

ids! { pub enum Field0509Flag: u16 {
    /// The five-windmill order 1,4,2,3,5 with the permanent seal-door flag clear spawns
    /// event 2001 and sets this request. Other checks clear it. Event 2000 tests it
    /// before opening the passage to field 510 and setting the distinct permanent
    /// door-open flag. The original branch skips the clear after a valid sequence;
    /// retain that control flow.
    SealRoomDoorOpeningRequested = 165,
} }

ids! { pub enum Field0513Flag: u16 {
    /// The corresponding passage from room 513 marks this before entering shared room
    /// 518. Room 518 tests it in its return event, clears it and returns to room 513 at
    /// (2825, 3906, -110). Upper/lower distinguish local-Y passages. Preserve the
    /// independent tests, original call order and default self-transfer branches; this
    /// selector is consumed on return, not on entry.
    ReturnTo0513UpperPassageSelected = 150,
    /// The corresponding passage from room 513 marks this before entering shared room
    /// 518. Room 518 tests it in its return event, clears it and returns to room 513 at
    /// (1371, -5195, -114). Upper/lower distinguish local-Y passages. Preserve the
    /// independent tests, original call order and default self-transfer branches; this
    /// selector is consumed on return, not on entry.
    ReturnTo0513LowerPassageSelected = 151,
    /// The upper local-Y passage from Gaoracchia room 513 sets this before entering
    /// shared room 514 at (101, 3682, -35). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers. This branch creates actor 711 but also scales
    /// actor 710 without creating it; retain the original calls.
    EntryFrom0513UpperPassagePending = 160,
    /// The middle local-Y passage from Gaoracchia room 513 sets this before entering
    /// shared room 514 at (63, 47, 30). Room 514 independently tests and clears it after
    /// creating the matching blocker/collision layout. Upper/middle/lower describe the
    /// three passage coordinates on that side, not world-map directions. The consumed
    /// request leaves its spawned layout in place; it is separate from the permanent
    /// light-cleared barriers.
    EntryFrom0513MiddlePassagePending = 161,
    /// The lower local-Y passage from Gaoracchia room 513 sets this before entering
    /// shared room 514 at (71, -2809, 0). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0513LowerPassagePending = 162,
} }

ids! { pub enum Field0514Flag: u16 {
    /// The upper local-Y passage from Gaoracchia room 513 sets this before entering
    /// shared room 514 at (101, 3682, -35). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers. This branch creates actor 711 but also scales
    /// actor 710 without creating it; retain the original calls.
    EntryFrom0513UpperPassagePending = 160,
    /// The middle local-Y passage from Gaoracchia room 513 sets this before entering
    /// shared room 514 at (63, 47, 30). Room 514 independently tests and clears it after
    /// creating the matching blocker/collision layout. Upper/middle/lower describe the
    /// three passage coordinates on that side, not world-map directions. The consumed
    /// request leaves its spawned layout in place; it is separate from the permanent
    /// light-cleared barriers.
    EntryFrom0513MiddlePassagePending = 161,
    /// The lower local-Y passage from Gaoracchia room 513 sets this before entering
    /// shared room 514 at (71, -2809, 0). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0513LowerPassagePending = 162,
    /// The upper local-Y passage from Gaoracchia room 515 sets this before entering
    /// shared room 514 at (3870, 2369, 181). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0515UpperPassagePending = 163,
    /// The middle local-Y passage from Gaoracchia room 515 sets this before entering
    /// shared room 514 at (3900, -857, 122). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0515MiddlePassagePending = 164,
    /// The lower local-Y passage from Gaoracchia room 515 sets this before entering
    /// shared room 514 at (3900, -3860, 73). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0515LowerPassagePending = 165,
} }

ids! { pub enum Field0515Flag: u16 {
    /// The corresponding passage from room 515 marks this before entering shared room
    /// 518. Room 518 tests it in its return event, clears it and returns to room 515 at
    /// (3341, 3092, 72). Upper/lower distinguish local-Y passages. Preserve the
    /// independent tests, original call order and default self-transfer branches; this
    /// selector is consumed on return, not on entry.
    ReturnTo0515UpperPassageSelected = 152,
    /// The upper local-Y passage from Gaoracchia room 515 sets this before entering
    /// shared room 514 at (3870, 2369, 181). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0515UpperPassagePending = 163,
    /// The middle local-Y passage from Gaoracchia room 515 sets this before entering
    /// shared room 514 at (3900, -857, 122). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0515MiddlePassagePending = 164,
    /// The lower local-Y passage from Gaoracchia room 515 sets this before entering
    /// shared room 514 at (3900, -3860, 73). Room 514 independently tests and clears it
    /// after creating the matching blocker/collision layout. Upper/middle/lower describe
    /// the three passage coordinates on that side, not world-map directions. The
    /// consumed request leaves its spawned layout in place; it is separate from the
    /// permanent light-cleared barriers.
    EntryFrom0515LowerPassagePending = 165,
} }

ids! { pub enum Field0518Flag: u16 {
    /// The corresponding passage from room 513 marks this before entering shared room
    /// 518. Room 518 tests it in its return event, clears it and returns to room 513 at
    /// (2825, 3906, -110). Upper/lower distinguish local-Y passages. Preserve the
    /// independent tests, original call order and default self-transfer branches; this
    /// selector is consumed on return, not on entry.
    ReturnTo0513UpperPassageSelected = 150,
    /// The corresponding passage from room 513 marks this before entering shared room
    /// 518. Room 518 tests it in its return event, clears it and returns to room 513 at
    /// (1371, -5195, -114). Upper/lower distinguish local-Y passages. Preserve the
    /// independent tests, original call order and default self-transfer branches; this
    /// selector is consumed on return, not on entry.
    ReturnTo0513LowerPassageSelected = 151,
    /// The corresponding passage from room 515 marks this before entering shared room
    /// 518. Room 518 tests it in its return event, clears it and returns to room 515 at
    /// (3341, 3092, 72). Upper/lower distinguish local-Y passages. Preserve the
    /// independent tests, original call order and default self-transfer branches; this
    /// selector is consumed on return, not on entry.
    ReturnTo0515UpperPassageSelected = 152,
} }

ids! { pub enum Field0519Flag: u16 {
    /// Room 519 tests this before creating enemy actor 8200. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8200RespawnSuppressed = 150,
    /// Room 519 tests this before creating enemy actor 8201. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8201RespawnSuppressed = 151,
    /// Room 519 tests this before creating enemy actor 8202. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8202RespawnSuppressed = 152,
    /// Room 519 tests this before creating enemy actor 8203. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8203RespawnSuppressed = 153,
    /// Ymir room 521 sets this side mark before calling an animal from local X 2689 and
    /// moving it to X 1636 against the boulder. Either first hit leaves cracks; both
    /// independent marks shatter blocker 3500 and permit the little fish to pass.
    /// Inspection messages 201/203/205 distinguish blocking/shattered/cracked, and
    /// entries in 519/521 restore the corresponding water-path scenery. The side names
    /// refer to local X, not a world-map compass direction; quest step at least 2000
    /// separately bypasses the obstruction.
    BoulderHitFromPositiveXMarked = 173,
    /// Ymir room 521 sets this side mark before calling an animal from local X -1191 and
    /// moving it to X 985 against the boulder. Either first hit leaves cracks; both
    /// independent marks shatter blocker 3500 and permit the little fish to pass.
    /// Inspection messages 201/203/205 distinguish blocking/shattered/cracked, and
    /// entries in 519/521 restore the corresponding water-path scenery. The side names
    /// refer to local X, not a world-map compass direction; quest step at least 2000
    /// separately bypasses the obstruction.
    BoulderHitFromNegativeXMarked = 174,
    /// The carrier actor 3002 departure helpers set this before crossing between Ymir
    /// rooms 519 and 520. Both destination entries consume it and select their arrival
    /// controller/camera state; the two directions use different state numbers. This is
    /// a shared transfer request, separate from the flags that skip a departure
    /// animation. The original independent entry tests are retained.
    CarrierTransferBetween0519And0520Pending = 175,
    /// The carrier actor 3002 departure helpers set this before crossing between Ymir
    /// rooms 519 and 521. Both destination entries consume it and select their arrival
    /// controller/camera state; the two directions use different state numbers. This is
    /// a shared transfer request, separate from the flags that skip a departure
    /// animation. The original independent entry tests are retained.
    CarrierTransferBetween0519And0521Pending = 177,
    /// The ordinary passage helpers between rooms 519 and 521 set this before
    /// change_field. Both entries clear it and choose their passage arrival controller
    /// state (519 state 8; 521 state 6). This request is separate from the carrier
    /// transfer between the same rooms and does not assert a completed animal ride.
    PassageTransferBetween0519And0521Pending = 178,
    /// Room 521 exit 2000 sets this at quest step 1900 before returning to 519 at
    /// (-2078,1544,316). Room 519 entry at that step consumes it to start event 4800.
    /// The same bit also gates the carrier departure animation from 519 to 520: when set
    /// that animation is skipped, while the transfer bit and change_field still execute.
    /// Keep that cross-use and the conditional clear; this is not an independent
    /// carrier-completion flag.
    FruitRouteArrivalFrom0521Marked = 180,
    /// The carrier departure helper in room 519 tests this before its actor-3002
    /// approach, attachment, motion and fade sequence. When set, that departure
    /// animation is skipped, but the helper still sets the separate transfer request and
    /// changes to room 521. No recovered literal setter establishes that this is a
    /// previously completed ride, so the name describes the observed suppression switch
    /// only.
    CarrierDepartureTo0521Suppressed = 184,
    /// The blue-flower setup helpers in 519/521 hide flower actor 3201 while this switch
    /// is set. Room 521 entry clears it before sub_508B moves camera actor 90020 along
    /// the path, grows flower 3201 back from scale zero and returns to 519 at
    /// (-451,-227,0). There is no recovered literal setter; this names the pending
    /// tour/restoration branch, not planting or possession of Blue Seed.
    BlueFlowerCameraTourPending = 188,
} }

ids! { pub enum Field0520Flag: u16 {
    /// Room 520 tests this before creating enemy actor 8200. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8200RespawnSuppressed = 154,
    /// Room 520 tests this before creating enemy actor 8201. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8201RespawnSuppressed = 155,
    /// Room 520 tests this before creating enemy actor 8202. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8202RespawnSuppressed = 156,
    /// Room 520 tests this before creating enemy actor 8203. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite. This enemy also requires Ymir quest step at least 1100 before
    /// spawning.
    Enemy8203RespawnSuppressed = 157,
    /// Room 521 event 2011 sets this immediately before entering room 520 at
    /// (-2571,748,0). Room 520 clears it in each of four ordinary exits and in regions
    /// 1030/1031/1032. There is no recovered literal reader, so this name records the
    /// marked route without asserting an active camera or collision effect.
    WestPassageArrivalFrom0521Marked = 170,
    /// At Ymir quest step 1600 the animal-call sequence knocks actor 3601 from
    /// (2259,626,762) down to Z -70, sets this mark and advances the step to 1700. Its
    /// dialogue identifies the unreachable Ymir Fruit and suggests help from the little
    /// fish. No recovered literal reader makes this mark an inventory or permanently
    /// obtained-fruit flag; StoryFlag::YmirFruitObtained remains separate.
    FruitKnockdownMarked = 171,
    /// Room 520 at quest step 1700 drops EncounterWorm actor 3900, waits for its event,
    /// moves the dangerous fish actor 3003 to (345,311,-50), starts event 4600, sets
    /// this mark and advances to 1710. Room 521 explicitly clears the same shared mark
    /// when its White Seed chest sequence advances to 1400. There is no recovered
    /// literal reader, so it records that lure sequence rather than guaranteeing the
    /// fish remains distracted or the fruit has arrived.
    WormLureMarked = 172,
    /// The carrier actor 3002 departure helpers set this before crossing between Ymir
    /// rooms 519 and 520. Both destination entries consume it and select their arrival
    /// controller/camera state; the two directions use different state numbers. This is
    /// a shared transfer request, separate from the flags that skip a departure
    /// animation. The original independent entry tests are retained.
    CarrierTransferBetween0519And0520Pending = 175,
    /// The carrier actor 3002 departure helpers set this before crossing between Ymir
    /// rooms 520 and 521. Both destination entries consume it and select their arrival
    /// controller/camera state; the two directions use different state numbers. This is
    /// a shared transfer request, separate from the flags that skip a departure
    /// animation. The original independent entry tests are retained.
    CarrierTransferBetween0520And0521Pending = 176,
    /// Room 520 exits 2021/2023 set this before entering 521 only at Ymir steps 1725 or
    /// 1800. Room 521 at 1725 tests it to run the fish-interception failure sequence,
    /// which resets the quest step to 1600; that branch does not clear the bit. At 1800
    /// entry instead unconditionally clears it and starts event 4103. Preserve this
    /// step-dependent protocol rather than treating the mark as successful fruit
    /// delivery or an always-consumed arrival request.
    FruitRouteArrivalFrom0520Marked = 179,
    /// The carrier departure helper in room 520 tests this before its actor-3002
    /// approach, attachment, motion and fade sequence. When set, that departure
    /// animation is skipped, but the helper still sets the separate transfer request and
    /// changes to room 519. No recovered literal setter establishes that this is a
    /// previously completed ride, so the name describes the observed suppression switch
    /// only.
    CarrierDepartureTo0519Suppressed = 181,
    /// The carrier departure helper in room 520 tests this before its actor-3002
    /// approach, attachment, motion and fade sequence. When set, that departure
    /// animation is skipped, but the helper still sets the separate transfer request and
    /// changes to room 521. No recovered literal setter establishes that this is a
    /// previously completed ride, so the name describes the observed suppression switch
    /// only.
    CarrierDepartureTo0521Suppressed = 182,
} }

ids! { pub enum Field0521Flag: u16 {
    /// Room 521 tests this before creating enemy actor 8200. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8200RespawnSuppressed = 158,
    /// Room 521 tests this before creating enemy actor 8201. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8201RespawnSuppressed = 159,
    /// Room 521 tests this before creating enemy actor 8202. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8202RespawnSuppressed = 160,
    /// Room 521 tests this before creating enemy actor 8203. Its battle event saves the
    /// contacted actor ID, calls start_enemy_battle, then sets the corresponding actor
    /// mark outside the result-2 despawn condition. The mark suppresses the next entry
    /// spawn; it does not prove victory, and naming it must not add a battle-result
    /// prerequisite.
    Enemy8203RespawnSuppressed = 161,
    /// Room 521 event 2011 sets this immediately before entering room 520 at
    /// (-2571,748,0). Room 520 clears it in each of four ordinary exits and in regions
    /// 1030/1031/1032. There is no recovered literal reader, so this name records the
    /// marked route without asserting an active camera or collision effect.
    WestPassageArrivalFrom0521Marked = 170,
    /// Room 520 at quest step 1700 drops EncounterWorm actor 3900, waits for its event,
    /// moves the dangerous fish actor 3003 to (345,311,-50), starts event 4600, sets
    /// this mark and advances to 1710. Room 521 explicitly clears the same shared mark
    /// when its White Seed chest sequence advances to 1400. There is no recovered
    /// literal reader, so it records that lure sequence rather than guaranteeing the
    /// fish remains distracted or the fruit has arrived.
    WormLureMarked = 172,
    /// Ymir room 521 sets this side mark before calling an animal from local X 2689 and
    /// moving it to X 1636 against the boulder. Either first hit leaves cracks; both
    /// independent marks shatter blocker 3500 and permit the little fish to pass.
    /// Inspection messages 201/203/205 distinguish blocking/shattered/cracked, and
    /// entries in 519/521 restore the corresponding water-path scenery. The side names
    /// refer to local X, not a world-map compass direction; quest step at least 2000
    /// separately bypasses the obstruction.
    BoulderHitFromPositiveXMarked = 173,
    /// Ymir room 521 sets this side mark before calling an animal from local X -1191 and
    /// moving it to X 985 against the boulder. Either first hit leaves cracks; both
    /// independent marks shatter blocker 3500 and permit the little fish to pass.
    /// Inspection messages 201/203/205 distinguish blocking/shattered/cracked, and
    /// entries in 519/521 restore the corresponding water-path scenery. The side names
    /// refer to local X, not a world-map compass direction; quest step at least 2000
    /// separately bypasses the obstruction.
    BoulderHitFromNegativeXMarked = 174,
    /// The carrier actor 3002 departure helpers set this before crossing between Ymir
    /// rooms 520 and 521. Both destination entries consume it and select their arrival
    /// controller/camera state; the two directions use different state numbers. This is
    /// a shared transfer request, separate from the flags that skip a departure
    /// animation. The original independent entry tests are retained.
    CarrierTransferBetween0520And0521Pending = 176,
    /// The carrier actor 3002 departure helpers set this before crossing between Ymir
    /// rooms 519 and 521. Both destination entries consume it and select their arrival
    /// controller/camera state; the two directions use different state numbers. This is
    /// a shared transfer request, separate from the flags that skip a departure
    /// animation. The original independent entry tests are retained.
    CarrierTransferBetween0519And0521Pending = 177,
    /// The ordinary passage helpers between rooms 519 and 521 set this before
    /// change_field. Both entries clear it and choose their passage arrival controller
    /// state (519 state 8; 521 state 6). This request is separate from the carrier
    /// transfer between the same rooms and does not assert a completed animal ride.
    PassageTransferBetween0519And0521Pending = 178,
    /// Room 520 exits 2021/2023 set this before entering 521 only at Ymir steps 1725 or
    /// 1800. Room 521 at 1725 tests it to run the fish-interception failure sequence,
    /// which resets the quest step to 1600; that branch does not clear the bit. At 1800
    /// entry instead unconditionally clears it and starts event 4103. Preserve this
    /// step-dependent protocol rather than treating the mark as successful fruit
    /// delivery or an always-consumed arrival request.
    FruitRouteArrivalFrom0520Marked = 179,
    /// Room 521 exit 2000 sets this at quest step 1900 before returning to 519 at
    /// (-2078,1544,316). Room 519 entry at that step consumes it to start event 4800.
    /// The same bit also gates the carrier departure animation from 519 to 520: when set
    /// that animation is skipped, while the transfer bit and change_field still execute.
    /// Keep that cross-use and the conditional clear; this is not an independent
    /// carrier-completion flag.
    FruitRouteArrivalFrom0521Marked = 180,
    /// The carrier departure helper in room 521 tests this before its actor-3002
    /// approach, attachment, motion and fade sequence. When set, that departure
    /// animation is skipped, but the helper still sets the separate transfer request and
    /// changes to room 520. No recovered literal setter establishes that this is a
    /// previously completed ride, so the name describes the observed suppression switch
    /// only.
    CarrierDepartureTo0520Suppressed = 183,
    /// The carrier departure helper in room 521 tests this before its actor-3002
    /// approach, attachment, motion and fade sequence. When set, that departure
    /// animation is skipped, but the helper still sets the separate transfer request and
    /// changes to room 519. No recovered literal setter establishes that this is a
    /// previously completed ride, so the name describes the observed suppression switch
    /// only.
    CarrierDepartureTo0519Suppressed = 185,
    /// The room-521 passage helper at quest step at least 1500 tests this and sets it if
    /// clear immediately before returning to 519 at (0,-436,0). That branch has no other
    /// side effect, and no other literal reader establishes a reward, new route or
    /// completed puzzle; it records this particular passage use separately from pending
    /// transfer bit 178.
    PassageFrom0521To0519Marked = 187,
    /// The blue-flower setup helpers in 519/521 hide flower actor 3201 while this switch
    /// is set. Room 521 entry clears it before sub_508B moves camera actor 90020 along
    /// the path, grows flower 3201 back from scale zero and returns to 519 at
    /// (-451,-227,0). There is no recovered literal setter; this names the pending
    /// tour/restoration branch, not planting or possession of Blue Seed.
    BlueFlowerCameraTourPending = 188,
} }

ids! { pub enum Field0522Flag: u16 {
    /// Camp room 0522 entry calls sub_065F when a Wind, Light or Water seal-progress
    /// word is 30000. The helper disables input, stops music, heals the party, selects
    /// Lloyd and sets this before the first-seal insomnia, second-night loss-of-pain, or
    /// Sheena two-world explanation branch selected by ColetteAngelProgress. The later
    /// stage increments and return transfers remain separate. No literal reader is
    /// recovered; the name records the common sequence-start write, not completion of an
    /// angel night.
    AngelNightCampSequenceStarted = 195,
    /// After the Igaguri astral-body scene, seven companion interactions share one +60
    /// first-conversation affinity bonus. Any one sets bit 1700; the third distinct
    /// conversation clears bits 1700..1707 and returns to Mizuho 326. Repeat dialogue
    /// remains available.
    IgaguriAftermathFirstConversationAffinityBonusClaimed = 1700,
    /// Colette's Igaguri/astral-body aftermath conversation increments shared counter
    /// 0x0884 once while this latch is clear. The third distinct conversation clears all
    /// these latches and returns to Mizuho 326; the separate 1700 latch allows only one
    /// affinity bonus.
    ColetteIgaguriAftermathConversationCounted = 1701,
    /// Genis's Igaguri/astral-body aftermath conversation increments shared counter
    /// 0x0884 once while this latch is clear. The third distinct conversation clears all
    /// these latches and returns to Mizuho 326; the separate 1700 latch allows only one
    /// affinity bonus.
    GenisIgaguriAftermathConversationCounted = 1702,
    /// Raine's Igaguri/astral-body aftermath conversation increments shared counter
    /// 0x0884 once while this latch is clear. The third distinct conversation clears all
    /// these latches and returns to Mizuho 326; the separate 1700 latch allows only one
    /// affinity bonus.
    RaineIgaguriAftermathConversationCounted = 1703,
    /// Zelos's Igaguri/astral-body aftermath conversation increments shared counter
    /// 0x0884 once while this latch is clear. The third distinct conversation clears all
    /// these latches and returns to Mizuho 326; the separate 1700 latch allows only one
    /// affinity bonus.
    ZelosIgaguriAftermathConversationCounted = 1704,
    /// Presea's Igaguri/astral-body aftermath conversation increments shared counter
    /// 0x0884 once while this latch is clear. The third distinct conversation clears all
    /// these latches and returns to Mizuho 326; the separate 1700 latch allows only one
    /// affinity bonus.
    PreseaIgaguriAftermathConversationCounted = 1705,
    /// Regal's Igaguri/astral-body aftermath conversation increments shared counter
    /// 0x0884 once while this latch is clear. The third distinct conversation clears all
    /// these latches and returns to Mizuho 326; the separate 1700 latch allows only one
    /// affinity bonus.
    RegalIgaguriAftermathConversationCounted = 1706,
    /// Kratos's Igaguri/astral-body aftermath conversation increments shared counter
    /// 0x0884 once while this latch is clear. The third distinct conversation clears all
    /// these latches and returns to Mizuho 326; the separate 1700 latch allows only one
    /// affinity bonus.
    KratosIgaguriAftermathConversationCounted = 1707,
} }

ids! { pub enum Field0530Flag: u16 {
    /// The Derris hub/side-corridor protocol records the lower level layout in this bit.
    /// Side rooms 131/132 use landings at Z 0/1004/2010, while hub 130 uses
    /// corresponding lower/middle/upper scenery and camera bounds. Their return handlers
    /// usually clear the other two bits; several hub and portal entries set only this
    /// bit, so all original independent tests remain. The lower layout also enables hub
    /// event 3000 and is selected by the Sacred Stone portal from Salvation 152, the
    /// applicable Salvation 502/0530 route and the return from room 136.
    HubLowerLevelLayoutSelected = 161,
    /// Entry chooses both end portals from this selector. Bit 1749 selects the 0530
    /// corridor connections: set means the north portal leads to Salvation 502 and south
    /// to Derris 129; clear means north to Derris 130 and south to Salvation 502. These
    /// are alternate routes, not disabled portals.
    RouteVia0129Selected = 1749,
} }

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0193FlagBase: u16 {
        /// Iselia Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects Reactor/Rescue Team. The next two bits encode slot 0..3 with weights 2/1.
        /// The original decoder skips Sheena (ID 5). These are packed selections, not
        /// affinity or rescue completion.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0194FlagBase: u16 {
        /// Iselia Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects Reactor/Rescue Team. The next two bits encode slot 0..3 with weights 2/1.
        /// The original decoder skips Sheena (ID 5). These are packed selections, not
        /// affinity or rescue completion.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0195FlagBase: u16 {
        /// Iselia Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects Reactor/Rescue Team. The next two bits encode slot 0..3 with weights 2/1.
        /// The original decoder skips Sheena (ID 5). These are packed selections, not
        /// affinity or rescue completion.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0197FlagBase: u16 {
        /// Iselia Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects Reactor/Rescue Team. The next two bits encode slot 0..3 with weights 2/1.
        /// The original decoder skips Sheena (ID 5). These are packed selections, not
        /// affinity or rescue completion.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0210FlagBase: u16 {
        /// Asgard Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap saved leaders. Keep the saved-leader fields
        /// and room0214 low-bit miswrite independent; a bank alias does not repair that
        /// encoding.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0211FlagBase: u16 {
        /// Asgard Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap saved leaders. Keep the saved-leader fields
        /// and room0214 low-bit miswrite independent; a bank alias does not repair that
        /// encoding.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0214FlagBase: u16 {
        /// Asgard Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap saved leaders. Keep the saved-leader fields
        /// and room0214 low-bit miswrite independent; a bank alias does not repair that
        /// encoding.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0215FlagBase: u16 {
        /// Asgard Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap saved leaders. Keep the saved-leader fields
        /// and room0214 low-bit miswrite independent; a bank alias does not repair that
        /// encoding.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0216FlagBase: u16 {
        /// Asgard Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap saved leaders. Keep the saved-leader fields
        /// and room0214 low-bit miswrite independent; a bank alias does not repair that
        /// encoding.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0217FlagBase: u16 {
        /// Asgard Ranch assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap saved leaders. Keep the saved-leader fields
        /// and room0214 low-bit miswrite independent; a bank alias does not repair that
        /// encoding.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0361FlagBase: u16 {
        /// Tower of Mana assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap two saved-leader words. Room0361 resets
        /// offsets0..29; the bank alias does not assign a character meaning to every bit.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0362FlagBase: u16 {
        /// Tower of Mana assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap two saved-leader words. Room0361 resets
        /// offsets0..29; the bank alias does not assign a character meaning to every bit.
        PartyAssignmentBank = 150,
    }
}

ids! {
    /// Field-local bank for the original party-split protocol.
    pub enum Field0366FlagBase: u16 {
        /// Tower of Mana assignment bank. At 150 + character ID*3, clear/set of the first bit
        /// selects team A/B; the next two bits encode slots 1..3 with weights 2/1. Decoders
        /// skip IDs 6,7,8 whose triplets overlap two saved-leader words. Room0361 resets
        /// offsets0..29; the bank alias does not assign a character meaning to every bit.
        PartyAssignmentBank = 150,
    }
}
