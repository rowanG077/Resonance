use super::*;

#[test]
fn ids_keep_game_domains_and_open_numeric_boundaries() {
    assert_eq!(FieldId::Iselia0332.raw(), 332);
    assert_eq!(WorldLocationId::Iselia.raw(), 2);
    assert_eq!(FieldId::TitleDay.raw(), 474);
    assert_eq!(FieldId::TitleNight.raw(), 531);
    assert_eq!(FieldId::NewGameSettings.raw(), 5);
    assert_eq!(FieldId::Exire0189.resource_name(), Some("exire-0189"));
    assert_eq!(FieldId::Niflheim0524.raw(), 524);
    assert_eq!(FieldId::DerrisKharlan0136.raw(), 136);
    assert_eq!(MusicId::TheLandOfSylvarant.raw(), 2);
    assert_eq!(MusicBankId::Sylvarant.raw(), 0);
    assert_eq!(ItemId::MemoryGem.raw(), 491);
    assert_eq!(StoryFlag::SavePointTutorialSeen.raw(), 520);
    assert_eq!(StoryFlag::SwordDancerTailVisible.raw(), 147);
    assert_eq!(StoryFlag::SwordDancerWingsVisible.raw(), 148);
    assert_eq!(MonsterId::SwordDancer.raw(), 191);
    assert_eq!(FigurineId::Efreet.raw(), 80);
    assert_eq!(ItemId::MemoryGem.to_string(), "MemoryGem");
    assert_eq!("MemoryGem".parse::<ItemId>(), Ok(ItemId::MemoryGem));
    assert_eq!(FieldId::Iselia0332.resource_name(), Some("iselia-0332"));
    assert!(FieldId::try_from(60000).is_err());
    assert!(StoryFlag::try_from(60000).is_err());
    assert!("UnassignedModResource".parse::<ItemId>().is_err());
}

#[test]
fn playable_characters_and_caravan_stops_share_the_original_discriminants() {
    assert_eq!(
        PartyCharacterId::RESOURCE_NAMES,
        [
            "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos"
        ]
    );
    assert_eq!(CharacterId::Noishe.resource_name(), Some("noishe"));
    assert_eq!(CharacterId::Reserved0.resource_name(), None);
    assert_eq!(PartyCharacterId::ALL.len(), 9);
    assert_eq!(PartyCharacterId::from_index(0), Ok(PartyCharacterId::Lloyd));
    assert_eq!(PartyCharacterId::Kratos.index(), 8);
    assert!(PartyCharacterId::try_from(CharacterId::Noishe.raw()).is_err());
    assert!(PartyCharacterId::try_from(CharacterId::Reserved0.raw()).is_err());
    assert!(PartyCharacterId::from_index(9).is_err());
    assert_eq!(
        CaravanStopId::Triet.raw(),
        WorldLocationId::NovasCaravan54.raw()
    );
    assert_eq!(
        CaravanStopId::Palmacosta.raw(),
        WorldLocationId::NovasCaravan60.raw()
    );
}

#[test]
fn marker_names_are_local_to_each_world_and_treasure_kinds_name_the_models() {
    assert_eq!(
        SylvarantMarkerId::Luin.raw(),
        TetheAllaMarkerId::Ozette.raw()
    );
    assert_eq!(SylvarantMarkerId::Luin.resource_name(), Some("luin"));
    assert_eq!(TetheAllaMarkerId::Ozette.resource_name(), Some("ozette"));
    assert_eq!(SylvarantMarkerId::ManaCannon.raw(), 15);
    assert!(TetheAllaMarkerId::try_from(15).is_err());
    assert_eq!(MusicBankId::TetheAllaLate.raw(), 2);
    assert_eq!(TreasureKind::try_from(0_i32), Ok(TreasureKind::Chest));
    assert_eq!(
        TreasureKind::try_from(1_i32),
        Ok(TreasureKind::ReinforcedChest)
    );
    assert_eq!(TreasureKind::try_from(2_i32), Ok(TreasureKind::Bag));
    assert!(TreasureKind::try_from(256_i32).is_err());
    assert!(TreasureKind::try_from(-1_i32).is_err());
}

#[test]
fn field_system_selectors_keep_native_values_and_the_separate_extension() {
    assert_eq!(FieldSystemCommand::PlayTime.raw(), 1);
    assert_eq!(FieldSystemCommand::FieldLeader.raw(), 13);
    assert_eq!(FieldSystemCommand::SetSkitPrompts.raw(), 21);
    assert_eq!(FieldSystemCommand::LastBattleFormationCount.raw(), 0x100);
    for command in FieldSystemCommand::ALL {
        assert_eq!(FieldSystemCommand::try_from(command.raw()), Ok(command));
    }
    assert!(FieldSystemCommand::try_from(0).is_err());
    assert!(FieldSystemCommand::try_from(20).is_err());
}
