//! Group conditions retain their order and gameplay IDs; participants use the
//! ordinary character mapping. Random gates name the required sample bits.
use super::*;
use crate::library::Mappings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedGroup {
    pub id: u8,
    pub character: String,
    pub voice: Option<crate::battle_voice::Sound>,
    pub participants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_leader: Option<String>,
    pub condition: Condition,
    pub selection: NamedSelection,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NamedSelection {
    Encounter { priority: u8, formations: Vec<u16> },
    Ordinary { priority: u8, gate: RandomBits },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomBits {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clear: Vec<u8>,
}
impl From<RandomGate> for RandomBits {
    fn from(gate: RandomGate) -> Self {
        let bits = |set| {
            (0..16)
                .filter(|&bit| {
                    gate.mask & (1 << bit) != 0 && (gate.accepted & (1 << bit) != 0) == set
                })
                .collect()
        };
        Self {
            set: bits(true),
            clear: bits(false),
        }
    }
}
impl RandomBits {
    pub fn prepare(&self) -> Result<RandomGate> {
        let mut gate = RandomGate {
            mask: 0,
            accepted: 0,
        };
        for (&bit, set) in self
            .set
            .iter()
            .map(|bit| (bit, true))
            .chain(self.clear.iter().map(|bit| (bit, false)))
        {
            ensure!(
                bit < 16 && gate.mask & (1 << bit) == 0,
                "duplicate or invalid random sample bit"
            );
            gate.mask |= 1 << bit;
            if set {
                gate.accepted |= 1 << bit;
            }
        }
        ensure!(gate.mask != 0, "empty victory random gate");
        Ok(gate)
    }
}
impl NamedGroup {
    pub fn from_group(group: Group, names: &Mappings) -> Result<Self> {
        let name = |id: u8| -> Result<String> { Ok(names.target(id.into())?.into()) };
        Ok(Self {
            id: group.id,
            character: name(group.character)?,
            voice: group.voice,
            participants: group
                .participants
                .into_iter()
                .map(name)
                .collect::<Result<_>>()?,
            required_leader: group.required_leader.map(name).transpose()?,
            condition: group.condition,
            selection: match group.selection {
                SelectionRule::Encounter {
                    priority,
                    formations,
                } => NamedSelection::Encounter {
                    priority,
                    formations,
                },
                SelectionRule::Ordinary { priority, gate } => NamedSelection::Ordinary {
                    priority,
                    gate: gate.into(),
                },
            },
        })
    }
    pub fn prepare(self, names: &Mappings) -> Result<Group> {
        let id = |name: String| -> Result<u8> { Ok(u8::try_from(names.id(&name)?)?) };
        let group = Group {
            id: self.id,
            character: id(self.character)?,
            voice: self.voice,
            participants: self
                .participants
                .into_iter()
                .map(id)
                .collect::<Result<_>>()?,
            required_leader: self.required_leader.map(id).transpose()?,
            condition: self.condition,
            selection: match self.selection {
                NamedSelection::Encounter {
                    priority,
                    formations,
                } => SelectionRule::Encounter {
                    priority,
                    formations,
                },
                NamedSelection::Ordinary { priority, gate } => SelectionRule::Ordinary {
                    priority,
                    gate: gate.prepare()?,
                },
            },
        };
        group.validate()?;
        Ok(group)
    }
}
