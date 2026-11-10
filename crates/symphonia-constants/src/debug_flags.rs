//! Operand-specific aliases in the original field debug menu.
//! These deliberately overlap: the menu shares bits between unrelated features.
//! A verified binary/PC profile selects the applicable enum, never a whole-file alias.

ids! { pub enum DebugLightningFlag: u16 {
    /// The lightning ring selector cycles 000/001 -> 100 (blue), 100 -> 010
    /// (yellow), and all other combinations -> 001 (red). These are selection
    /// markers, not permanent upgrades, and bits 150..152 also mark actor slots.
    BlueSelected = 150,
    YellowSelected = 151,
    RedSelected = 152,
}}

ids! { pub enum DebugActorSlotFlag: u16 {
    /// ID1..ID8 in the debug menu reserve actors 32..39. Creation marks a slot;
    /// removal tests it and clears it before despawning the corresponding actor.
    /// The UI says "used" or "not present" according to the marker, not an engine
    /// actor-existence query. The first three overlap the lightning selectors.
    Actor32InUse = 150,
    Actor33InUse = 151,
    Actor34InUse = 152,
    Actor35InUse = 153,
    Actor36InUse = 154,
    Actor37InUse = 155,
    Actor38InUse = 156,
    Actor39InUse = 157,
}}
