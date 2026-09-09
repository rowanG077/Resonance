//! Native argument adaptation for persistent party services.
use super::{NativeHost, NativeResult, require};
use symphonia_script::NativeCall;
use symphonia_script_vm::Memory;

impl NativeHost<'_> {
    pub(super) fn party(
        &mut self,
        op: NativeCall,
        a: &[i32],
        _memory: &mut Memory,
    ) -> Result<NativeResult, String> {
        let data = self
            .resources
            .session_data
            .as_ref()
            .ok_or("session definitions are missing")?;
        let mut party = self.world.party.take().ok_or("party is not initialized")?;
        let result = (|| {
            let mut value = None;
            let member = || -> Result<usize, String> {
                let id = if a[0] == crate::CONTROLLED_ACTOR {
                    self.world.controlled_actor
                } else {
                    a[0]
                };
                require((1..=9).contains(&id), "unknown party member")?;
                Ok(id as usize - 1)
            };
            match op {
                NativeCall::AddPartyMember => {
                    let id = member()? as u8 + 1;
                    value = Some(
                        if party.formation.contains(&id) || party.formation.len() >= 8 {
                            1
                        } else {
                            party.formation.push(id);
                            0
                        },
                    );
                }
                NativeCall::AdjustCharacterAffinity => {
                    let index = member()?;
                    let affinity = &mut party.members[index].affinity;
                    *affinity = affinity.saturating_add(a[1]).clamp(-10000, 10000);
                    value = Some(*affinity);
                }
                NativeCall::ChangeItemCount => {
                    value = Some(i32::from(party.change_item(
                        data,
                        u16::try_from(a[0]).map_err(|_| "invalid item")?,
                        a[1] as i8,
                    )?))
                }
                NativeCall::EquipItem => party.equip(
                    data,
                    member()?,
                    u16::try_from(a[1]).map_err(|_| "invalid equipment item")?,
                )?,
                NativeCall::UnequipItem => party.unequip(
                    data,
                    member()?,
                    usize::try_from(a[1]).map_err(|_| "invalid equipment slot")?,
                )?,
                NativeCall::LearnTechnique => {
                    let id = u16::try_from(a[1]).map_err(|_| "invalid technique")?;
                    let index = member()?;
                    require(
                        data.characters[index].allowed_techniques.contains(&id),
                        "technique is not available to this member",
                    )?;
                    party.members[index].techniques.insert(id);
                }
                NativeCall::HealParty => {
                    require(a[0] == 0, "unsupported party recovery mode")?;
                    party.heal(|| self.world.random());
                }
                NativeCall::AddGald => value = Some(party.add_gald(a[0]) as i32),
                NativeCall::RaisePartyMemberLevel => {
                    let index = member()?;
                    let level = if a[1] == -1 {
                        let members: Vec<_> = party
                            .formation
                            .iter()
                            .map(|id| usize::from(*id) - 1)
                            .filter(|id| *id != index)
                            .collect();
                        require(!members.is_empty(), "cannot average an empty party")?;
                        (members
                            .iter()
                            .map(|id| u32::from(party.members[*id].level))
                            .sum::<u32>()
                            / members.len() as u32) as u8
                    } else {
                        u8::try_from(a[1]).map_err(|_| "invalid target level")?
                    };
                    party.raise_level(data, index, level, || self.world.random())?;
                }
                NativeCall::ConfigureSession => {
                    let setting = match a[0] {
                        3 => &mut party.settings.rumble,
                        4 => &mut party.settings.skit_titles,
                        5 => &mut party.settings.stereo,
                        _ => return Err("unsupported session setting".into()),
                    };
                    value = Some(i32::from(*setting));
                    if a[1] != -1 {
                        *setting = a[1] & 1 != 0;
                    }
                }
                NativeCall::ConfigureBattleControl => {
                    let index = if (1..=4).contains(&a[0]) {
                        (a[0] - 1) as usize
                    } else {
                        0
                    };
                    value = Some(i32::from(party.settings.battle_controls[index]));
                    if (1..=4).contains(&a[0]) && a[1] != -1 {
                        party.settings.battle_controls[index] = (a[1] & 7) as u8;
                    }
                }
                _ => return Err("unknown party operation".into()),
            }
            Ok(NativeResult::Continue(value))
        })();
        self.world.party = Some(party);
        result
    }
}
