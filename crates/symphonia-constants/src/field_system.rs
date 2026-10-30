//! Named field-system selectors. 0x100 is Resonance’s combined-battle extension.
ids! {
    pub enum FieldSystemCommand: u16 {
        PlayTime = 1,
        UndiscoveredMonsters = 2,
        CheckCollectorsBook = 3,
        CheckMonsterBook = 4,
        CheckFigurineBook = 5,
        CollectorsBookComplete = 6,
        MonsterBookComplete = 7,
        FigurineBookComplete = 8,
        SetCollectorsBookComplete = 9,
        SetMonsterBookComplete = 10,
        SetFigurineBookComplete = 11,
        LastBattleDuration = 12,
        LastBattleFormationCount = 0x100,
        FieldLeader = 13,
        SetFieldLeader = 14,
        SuppressTransitionFade = 15,
        SelectExitDoor = 16,
        BattleCount = 17,
        FrameFeedback = 18,
        SetDoorInteractionRadius = 19,
        SetSkitPrompts = 21,
    }
}
