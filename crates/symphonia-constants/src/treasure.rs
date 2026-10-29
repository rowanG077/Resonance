//! Kind selectors for native treasure creation; custom models use per-field slots.
ids! {
    pub enum TreasureKind: u8 {
        Chest = 0,
        ReinforcedChest = 1,
        Bag = 2,
        CustomModel0 = 3,
        CustomModel1 = 4,
    }
}

impl TryFrom<i32> for TreasureKind {
    type Error = &'static str;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        u8::try_from(value)
            .ok()
            .and_then(|id| Self::try_from(id).ok())
            .ok_or("invalid treasure kind")
    }
}
