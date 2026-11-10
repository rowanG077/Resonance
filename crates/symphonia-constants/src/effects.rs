//! Field-service sound cue IDs. Battle-bank-local cues are a different domain.
ids! {
    pub enum EffectId: u16 {
        SilentPlaceholder0000 = 0 => "silent-placeholder-0000",
        Navigate = 1 => "navigate",
        Confirm = 2 => "confirm",
        Cancel = 3 => "cancel",
        Error = 4 => "error",
        SilentPlaceholder0005 = 5 => "silent-placeholder-0005",
        ReceivedItem = 6 => "treasure-reward",
        SilentPlaceholder0008 = 8 => "silent-placeholder-0008",
        SilentPlaceholder0009 = 9 => "silent-placeholder-0009",
        SilentPlaceholder0010 = 10 => "silent-placeholder-0010",
        OpenChest = 28 => "treasure-open",
        OpenAlternateChest = 29 => "treasure-bag",
        Door = 30 => "door",
        RingPrepare = 31 => "ring-prepare",
        RingFire = 32 => "ring-fire",
        MenuOpen = 33 => "menu-open",
        // Cue 36 accompanies item/technique awards, but is also reused for
        // exchanges and Lloyd's Eternal Swordsman title. This names the cue,
        // not an inventory mutation. Preserve its existing editable binding.
        AcquisitionFanfare = 36 => "cue-0036",
        // Cue 37 accompanies title awards, recipes and pass-code notices.
        LearningFanfare = 37 => "cue-0037",
        Page = 38 => "page",
        RingElectric = 41 => "ring-electric",
        RingLightning = 42 => "ring-lightning",
        RingIce = 43 => "ring-ice",
        SilentPlaceholder0052 = 52 => "silent-placeholder-0052",
        SilentPlaceholder0053 = 53 => "silent-placeholder-0053",
        SilentPlaceholder0067 = 67 => "silent-placeholder-0067",
        SilentPlaceholder0068 = 68 => "silent-placeholder-0068",
        SilentPlaceholder0070 = 70 => "silent-placeholder-0070",
        /// Physical body-impact cue in hits, falls and staged collisions,
        /// including comic reprimands and combat cinematics. The animation,
        /// movement, attachments and any damage remain separate operations.
        /// Resident program144/sample94; retain the cue-0071 audio binding.
        BodyImpact = 71 => "cue-0071",
        SilentPlaceholder0074 = 74 => "silent-placeholder-0074",
        SilentPlaceholder0075 = 75 => "silent-placeholder-0075",
        SilentPlaceholder0078 = 78 => "silent-placeholder-0078",
        SilentPlaceholder0079 = 79 => "silent-placeholder-0079",
        /// Fanfare accompanying party joins, rewards and recipe notices, also
        /// retained in Gaoracchia's plant-clearing callbacks. Playing it does
        /// not add a party member, award an item or teach a recipe.
        /// Resident layer0x803c retains all11voices and the cue-0080 target.
        SuccessFanfare = 80 => "cue-0080",
        SilentPlaceholder0083 = 83 => "silent-placeholder-0083",
        SilentPlaceholder0086 = 86 => "silent-placeholder-0086",
        RingWind = 87 => "ring-wind",
        SilentPlaceholder0094 = 94 => "silent-placeholder-0094",
        SilentPlaceholder0098 = 98 => "silent-placeholder-0098",
        SilentPlaceholder0100 = 100 => "silent-placeholder-0100",
        SilentPlaceholder0102 = 102 => "silent-placeholder-0102",
        SilentPlaceholder0103 = 103 => "silent-placeholder-0103",
        Recovery = 104 => "recovery",
        RingBombPlace = 107 => "ring-bomb-place",
        RingWater = 110 => "ring-water",
        RingShatterVariant0118 = 118 => "ring-shatter-variant-0118",
        SilentPlaceholder0121 = 121 => "silent-placeholder-0121",
        SilentPlaceholder0125 = 125 => "silent-placeholder-0125",
        RingSound = 129 => "ring-sound",
        BattleEntry = 130 => "battle-entry",
        Remedy = 132 => "remedy",
        RecoveryVariant0145 = 145 => "recovery-variant-0145",
        SilentPlaceholder0146 = 146 => "silent-placeholder-0146",
        SilentPlaceholder0147 = 147 => "silent-placeholder-0147",
        SilentPlaceholder0148 = 148 => "silent-placeholder-0148",
        SilentPlaceholder0149 = 149 => "silent-placeholder-0149",
        SilentPlaceholder0150 = 150 => "silent-placeholder-0150",
        SilentPlaceholder0151 = 151 => "silent-placeholder-0151",
        /// Knock cue in Flanoir's visitor scenes and Heimdall's overnight
        /// conversations. The 28 requests retain their original spacing.
        /// Resident program137 selects sample89; neither number is the cue ID.
        DoorKnock = 156 => "cue-0156",
        SilentPlaceholder0159 = 159 => "silent-placeholder-0159",
        SilentPlaceholder0161 = 161 => "silent-placeholder-0161",
        SilentPlaceholder0162 = 162 => "silent-placeholder-0162",
        SilentPlaceholder0163 = 163 => "silent-placeholder-0163",
        /// Requested by Ymir Forest 0519..0521's small-fish route sequences.
        /// Some calls attach it to a movement carrier while the fish follows or
        /// is attached. This does not spawn a fish or complete a route/fruit step.
        /// Resident cue175 selects program825/sample406 with pitch offset -2.
        YmirFishMovement = 175 => "cue-0175",
        /// Low rumble accompanying scenery movement, tremors and cinematic
        /// effects, also attached temporarily to mechanism/statue sound hosts.
        /// Playback does not shake a camera/controller or move scenery.
        /// Resident program639/sample295 keeps its +2 pitch and release tail.
        LowRumble = 181 => "cue-0181",
        RingLightningVariant0186 = 186 => "ring-lightning-variant-0186",
        /// Electrical flash/discharge cue in Lightning Temple 0475..0478,
        /// including rod activation, the shock scene and alternating flashes.
        /// Resident program772/sample367. Playback does not activate a rod.
        ElectricalDischarge = 187 => "cue-0187",
        // Machine/console tone; shares sample130 with RingRadar but uses a
        // different program. Keep the distinct public cue and editable target.
        ElectronicBeep = 189 => "cue-0189",
        RingShatter = 191 => "ring-shatter",
        RingBombBlastPart1Variant0193 = 193 => "ring-bomb-blast-part-1-variant-0193",
        RingBombBlast = 194 => "ring-bomb-blast",
        /// Weapon drawing/readying cue beside prop visibility, attachment and
        /// combat animations. Also reused in attack/effect staging and Vidarr's
        /// rescue cinematic; playback does not equip a weapon or deal damage.
        /// Resident program759/sample356 keeps its original key-off tail.
        WeaponReady = 195 => "cue-0195",
        /// Accompanies Earth Temple's earthquake-triggered floor collapse and
        /// Gnomelette 3's synchronized collapse. The scripts move the falling
        /// carriers separately. Resident program76 selects sample45.
        StoneFloorCollapse = 210 => "cue-0210",
        SilentPlaceholder0225 = 225 => "silent-placeholder-0225",
        RingDarkness = 236 => "ring-darkness",
        MechanicalSlide = 239 => "cue-0239",
        /// Signal around switches, moving mechanisms and puzzle reactions,
        /// also reused in cinematic staging. It does not assert that a switch
        /// changed, a puzzle succeeded or an enemy should spawn.
        /// Resident program597/sample263 differs from ElevatorDepartureBeep306.
        MechanismSignal = 246 => "cue-0246",
        /// Machinery sound around lift arrivals and transport mechanisms,
        /// also reused in Salvation's cinematic sequence. Program516/sample221
        /// differs from MachineryMotion251/252's program518 and keeps its +6
        /// pitch, controls and tail. Movement, fades and stops stay separate.
        MachineryMotionVariant0248 = 248 => "cue-0248",
        /// Accompanies moving lifts, platforms, conveyors and other mechanisms.
        /// Cue251 selects resident program518/sample221, with pitch offset -5.
        /// Cue252 shares the program but retains a separate public sound group.
        /// Playing this cue does not start a mechanism or report its completion.
        MachineryMotion = 251 => "cue-0251",
        /// Alternate public cue for the same program518/sample221, used by
        /// Altamira transport and Tethe'alla Base block-pushing sequences.
        /// Preserve this cue's own group and editable target rather than folding
        /// its original252 operands into MachineryMotion251.
        MachineryMotionVariant0252 = 252 => "cue-0252",
        SilentPlaceholder0260 = 260 => "silent-placeholder-0260",
        RingManaVariant0262 = 262 => "ring-mana-variant-0262",
        RingRadar = 263 => "ring-radar",
        RingMana = 270 => "ring-mana",
        RingManaVariant0271 = 271 => "ring-mana-variant-0271",
        RingEarthquake = 272 => "ring-earthquake",
        RingEarthquakePart4Variant0273 = 273 => "ring-earthquake-part-4-variant-0273",
        /// Accompanies scripted magical appearances and disappearances, with
        /// opacity/scale fades, immediate hide/despawn and screen-dissolve uses.
        /// Playback itself does not alter visibility or move an actor.
        /// Resident layer0x80a4 uses programs811/812/813 and samples236/240;
        /// program275 in TeleportDissolve's layer is a different namespace.
        MagicalFade = 275 => "cue-0275",
        /// Common scripted jump/launch cue. Ozette also reuses it during
        /// Colette's stumble animation. Playback does not move an actor.
        /// Resident layer0x8084 selects programs460/461, not public cues460/461.
        JumpTakeoff = 282 => "cue-0282",
        /// Common landing impact cue, also reused in cinematic impact scenes.
        /// Resident layer0x8085 retains four voices, including two entries for
        /// program462 with different velocities. No collision result is implied.
        LandingImpact = 283 => "cue-0283",
        RingShrink = 287 => "ring-shrink",
        RingBubble = 289 => "ring-bubble",
        /// Alarm cue in ranch/base warning scenes, lock notices and Salvation's
        /// gate controls. Retain timed repetitions and alternative lock paths;
        /// playback does not change a lock, activate a warning or flash a camera.
        /// Resident layer0x8053 retains six distinct sample5 voice programs.
        AlarmSignal = 299 => "cue-0299",
        // Played three times before Altamira's elevator departure.
        ElevatorDepartureBeep = 306 => "cue-0306",
        // Shared by changed-mode and returned-to-normal notices. The cue is
        // a five-voice layer, independent of the subsequent ring mode write.
        RingModeChange = 320 => "cue-0320",
        RingBombDebris = 330 => "ring-bomb-debris",
        SilentPlaceholder0340 = 340 => "silent-placeholder-0340",
        SilentPlaceholder0341 = 341 => "silent-placeholder-0341",
        SilentPlaceholder0342 = 342 => "silent-placeholder-0342",
        SilentPlaceholder0343 = 343 => "silent-placeholder-0343",
        SilentPlaceholder0344 = 344 => "silent-placeholder-0344",
        SilentPlaceholder0348 = 348 => "silent-placeholder-0348",
        SilentPlaceholder0349 = 349 => "silent-placeholder-0349",
        SilentPlaceholder0351 = 351 => "silent-placeholder-0351",
        SilentPlaceholder0353 = 353 => "silent-placeholder-0353",
        SilentPlaceholder0356 = 356 => "silent-placeholder-0356",
        SilentPlaceholder0358 = 358 => "silent-placeholder-0358",
        SilentPlaceholder0359 = 359 => "silent-placeholder-0359",
        SilentPlaceholder0360 = 360 => "silent-placeholder-0360",
        SilentPlaceholder0361 = 361 => "silent-placeholder-0361",
        SilentPlaceholder0363 = 363 => "silent-placeholder-0363",
        SilentPlaceholder0364 = 364 => "silent-placeholder-0364",
        /// Proximity activation cue for teleport pads, accompanying their
        /// RisingOrbs emitter. This precedes using the pad; it is not an
        /// arrival notification. Resident layer0x8021 has two voices.
        TeleportPadActivation = 366 => "cue-0366",
        /// Teleport departure/dissolve cue. Also reused while Derris-Kharlan
        /// puzzle blocks rise and shrink away. Resident layer0x8022 has six
        /// voices, including two distinct entries for program275.
        TeleportDissolve = 367 => "cue-0367",
        RingLightningVariant0368 = 368 => "ring-lightning-variant-0368",
        SilentPlaceholder0369 = 369 => "silent-placeholder-0369",
        SilentPlaceholder0370 = 370 => "silent-placeholder-0370",
        SilentPlaceholder0371 = 371 => "silent-placeholder-0371",
        SilentPlaceholder0372 = 372 => "silent-placeholder-0372",
        /// Wonder Chef's smoke-covered arrivals and departures. All38 original
        /// requests precede a separate RisingSmoke emitter; actor visibility,
        /// recipe teaching and ingredient rewards remain separately scripted.
        /// Resident layer0x8025 retains all five voices and both samples.
        WonderChefPoof = 373 => "cue-0373",
        SilentPlaceholder0374 = 374 => "silent-placeholder-0374",
        SilentPlaceholder0375 = 375 => "silent-placeholder-0375",
        SilentPlaceholder0376 = 376 => "silent-placeholder-0376",
        SilentPlaceholder0377 = 377 => "silent-placeholder-0377",
        SilentPlaceholder0378 = 378 => "silent-placeholder-0378",
        SilentPlaceholder0379 = 379 => "silent-placeholder-0379",
        SilentPlaceholder0380 = 380 => "silent-placeholder-0380",
        SilentPlaceholder0381 = 381 => "silent-placeholder-0381",
        SilentPlaceholder0382 = 382 => "silent-placeholder-0382",
        /// Fire ambience on Balacruf torches, Thoda fire emitters and Niflheim
        /// flames, also used for hearths, burning Iselia and a Salvation effect.
        /// Resident program62/sample36 keeps its original volume modulation,
        /// loop and key-off tail. Playback does not light or extinguish a fire.
        FireAmbience = 384 => "cue-0384",
        /// Surf ambience at shores and during the Izoold boat departure, also
        /// reused in Kuchinawa's Mizuho scene. Resident layer0x80a7 has four
        /// distinct voices using sample411; positions and bindings stay separate.
        SurfAmbience = 385 => "cue-0385",
        /// Flowing-water ambience across rivers, streams and water-side fields.
        /// Resident layer0x8038 keeps two panned voices using sample201, with
        /// program481's delayed start and separate key-off path intact.
        FlowingWaterAmbience = 386 => "cue-0386",
        /// Waterfall ambience beside Toize and Latheon cascade geometry, with
        /// retained forest, hot-spring, boat and Mizuho scene uses. Distinct
        /// from FlowingWaterAmbience. Program768/sample364 keeps its vibrato,
        /// sustained playback and original key-off tail.
        WaterfallAmbience = 394 => "cue-0394",
        /// Drive sound during Asgard Ranch's animated production machinery,
        /// Toize Mine's lift route and Salvation's moving mechanisms. Spatial
        /// hosts and bound playback retain their separate movement commands.
        /// Resident program538/sample220 differs from MachineryMotion251/252;
        /// its sustained program and key-off tail keep the cue-0405 binding.
        MachineryDrive = 405 => "cue-0405",
        RingSplash = 414 => "ring-splash",
        SilentPlaceholder0417 = 417 => "silent-placeholder-0417",
        SilentPlaceholder0418 = 418 => "silent-placeholder-0418",
        SilentPlaceholder0419 = 419 => "silent-placeholder-0419",
        SilentPlaceholder0420 = 420 => "silent-placeholder-0420",
        SilentPlaceholder0421 = 421 => "silent-placeholder-0421",
        SilentPlaceholder0422 = 422 => "silent-placeholder-0422",
        SilentPlaceholder0439 = 439 => "silent-placeholder-0439",
        SilentPlaceholder0444 = 444 => "silent-placeholder-0444",
        SilentPlaceholder0450 = 450 => "silent-placeholder-0450",
        SilentPlaceholder0453 = 453 => "silent-placeholder-0453",
        SilentPlaceholder0454 = 454 => "silent-placeholder-0454",
        /// Ymir's pink-flower animal-call branches request this cue before
        /// the boar sequences. Playback itself does not summon an actor.
        /// Event bank se_ev03 selects program 663/sample 308.
        YmirBoarCall = 456 => "cue-0456",
        /// Blue-flower call cue before Ymir's small-fish route helpers.
        /// Their seed/progress checks remain separate from playing this sound.
        /// Event bank se_ev03 selects program 666/sample 309.
        YmirFishCall = 457 => "cue-0457",
        /// Ambient cue attached to the charging boar in Ymir's puzzle scenes.
        /// Program 662 reuses sample 308 with different processing from cue 456.
        YmirBoarMovement = 458 => "cue-0458",
        /// Played when Ymir's charging boar reaches its scripted impact point,
        /// alongside cue 220 and camera shake. This is not a collision query.
        /// Event bank se_ev03 selects program 661/sample 307.
        YmirBoarImpact = 459 => "cue-0459",
        /// White-flower call cue before Ymir's bird transport/passage helpers.
        /// Event bank se_ev03 selects program 668/sample 310.
        YmirBirdCall = 460 => "cue-0460",
        /// Default Ymir animal call, used by the introduction, unselected-target
        /// branches and fish/bird branches before their progress thresholds.
        /// Program 667 reuses sample 309 with different processing from cue 457.
        YmirDefaultAnimalCall = 461 => "cue-0461",
        SilentPlaceholder0467 = 467 => "silent-placeholder-0467",
        SilentPlaceholder0468 = 468 => "silent-placeholder-0468",
        SilentPlaceholder0476 = 476 => "silent-placeholder-0476",
        SilentPlaceholder0477 = 477 => "silent-placeholder-0477",
        SilentPlaceholder0478 = 478 => "silent-placeholder-0478",
        SilentPlaceholder0480 = 480 => "silent-placeholder-0480",
        SilentPlaceholder0481 = 481 => "silent-placeholder-0481",
        SilentPlaceholder0482 = 482 => "silent-placeholder-0482",
        SilentPlaceholder0483 = 483 => "silent-placeholder-0483",
        SilentPlaceholder0484 = 484 => "silent-placeholder-0484",
    }
}
