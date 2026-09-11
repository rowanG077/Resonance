//! Knowledge acquired about an encountered enemy; catalogue data stays in content.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MonsterKnowledge {
    pub scanned: bool,
    pub drops: [bool; 2],
    pub steal: bool,
    pub location: bool,
    /// Highest encountered repeat-battle variant, zero for the base statistics.
    pub variant: u8,
}
