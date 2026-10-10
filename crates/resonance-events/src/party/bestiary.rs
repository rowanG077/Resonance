//! Knowledge acquired about an encountered enemy; catalogue data stays in content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonsterKnowledge {
    pub seen: bool,
    pub scanned: bool,
    pub drops: [bool; 2],
    pub steal: bool,
    pub location: bool,
    /// Highest encountered repeat-battle variant, zero for the base statistics.
    pub variant: u8,
}

impl Default for MonsterKnowledge {
    fn default() -> Self {
        Self {
            seen: true,
            scanned: false,
            drops: [false; 2],
            steal: false,
            location: false,
            variant: 0,
        }
    }
}

impl MonsterKnowledge {
    /// Script bits describe discovery, scan, drops, steal and location in that order.
    pub fn script_flags(&self) -> u8 {
        [
            self.seen,
            self.scanned,
            self.drops[0],
            self.drops[1],
            self.steal,
            self.location,
        ]
        .into_iter()
        .enumerate()
        .fold(0, |flags, (bit, known)| flags | u8::from(known) << bit)
    }
    pub(crate) fn set_script_flags(&mut self, flags: u8) {
        let [seen, scanned, first_drop, second_drop, steal, location] =
            std::array::from_fn(|bit| flags & (1 << bit) != 0);
        self.seen = seen;
        self.scanned = scanned;
        self.drops = [first_drop, second_drop];
        self.steal = steal;
        self.location = location;
    }
}
