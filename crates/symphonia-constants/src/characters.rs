ids! {
    pub enum CharacterId: u8 {
        Reserved0 = 0,
        Lloyd = 1 => "lloyd",
        Colette = 2 => "colette",
        Genis = 3 => "genis",
        Raine = 4 => "raine",
        Sheena = 5 => "sheena",
        Zelos = 6 => "zelos",
        Presea = 7 => "presea",
        Regal = 8 => "regal",
        Kratos = 9 => "kratos",
        Noishe = 10 => "noishe",
    }
}

ids! {
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    pub enum PartyCharacterId: u8 {
        Lloyd = CharacterId::Lloyd as u8 => CharacterId::Lloyd.resource_name().unwrap(),
        Colette = CharacterId::Colette as u8 => CharacterId::Colette.resource_name().unwrap(),
        Genis = CharacterId::Genis as u8 => CharacterId::Genis.resource_name().unwrap(),
        Raine = CharacterId::Raine as u8 => CharacterId::Raine.resource_name().unwrap(),
        Sheena = CharacterId::Sheena as u8 => CharacterId::Sheena.resource_name().unwrap(),
        Zelos = CharacterId::Zelos as u8 => CharacterId::Zelos.resource_name().unwrap(),
        Presea = CharacterId::Presea as u8 => CharacterId::Presea.resource_name().unwrap(),
        Regal = CharacterId::Regal as u8 => CharacterId::Regal.resource_name().unwrap(),
        Kratos = CharacterId::Kratos as u8 => CharacterId::Kratos.resource_name().unwrap(),
    }
}
