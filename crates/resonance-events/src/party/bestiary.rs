//! Knowledge acquired about an encountered enemy; catalogue data stays in content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonsterKnowledge {
    pub seen: bool,
    pub scanned: bool,
    #[serde(default)]
    pub weakness_known: bool,
    #[serde(default)]
    pub resistance_known: bool,
    pub drops: [bool; 2],
    pub steal: bool,
    pub location: bool,
    /// Highest encountered repeat-battle variant, zero for the base statistics.
    pub variant: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elemental_knowledge_round_trips_script_flags_and_old_saves_default_to_unknown() {
        let old = r#"{"seen":true,"scanned":false,"drops":[false,false],"steal":false,"location":false,"variant":0}"#;
        let mut knowledge: MonsterKnowledge = serde_json::from_str(old).unwrap();
        assert!(!knowledge.weakness_known && !knowledge.resistance_known);
        for flags in [0, 1, 0x40, 0x80, 0xc0, 0xff] {
            knowledge.set_script_flags(flags);
            assert_eq!(knowledge.script_flags(), flags);
            let decoded: MonsterKnowledge =
                serde_json::from_slice(&serde_json::to_vec(&knowledge).unwrap()).unwrap();
            assert_eq!(decoded.script_flags(), flags);
        }
    }
}

impl Default for MonsterKnowledge {
    fn default() -> Self {
        Self {
            seen: true,
            scanned: false,
            weakness_known: false,
            resistance_known: false,
            drops: [false; 2],
            steal: false,
            location: false,
            variant: 0,
        }
    }
}

impl MonsterKnowledge {
    /// Script bits describe discovery, scan, drops, steal, location and elemental knowledge.
    pub fn script_flags(&self) -> u8 {
        [
            self.seen,
            self.scanned,
            self.drops[0],
            self.drops[1],
            self.steal,
            self.location,
            self.weakness_known,
            self.resistance_known,
        ]
        .into_iter()
        .enumerate()
        .fold(0, |flags, (bit, known)| flags | u8::from(known) << bit)
    }
    pub(crate) fn set_script_flags(&mut self, flags: u8) {
        let [
            seen,
            scanned,
            first_drop,
            second_drop,
            steal,
            location,
            weakness,
            resistance,
        ] = std::array::from_fn(|bit| flags & (1 << bit) != 0);
        self.seen = seen;
        self.scanned = scanned;
        self.drops = [first_drop, second_drop];
        self.steal = steal;
        self.location = location;
        self.weakness_known = weakness;
        self.resistance_known = resistance;
    }
}
