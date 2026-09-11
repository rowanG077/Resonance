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
        let party = self
            .world
            .party
            .as_mut()
            .ok_or("party is not initialized")?;
        let random = &mut self.world.random_state;
        let controlled_actor = self.world.controlled_actor;
        let mut value = None;
        let member = || -> Result<usize, String> {
            let id = if a[0] == crate::CONTROLLED_ACTOR {
                controlled_actor
            } else {
                a[0]
            };
            require((1..=9).contains(&id), "unknown party member")?;
            Ok(id as usize - 1)
        };
        match op {
            NativeCall::LearnRecipe | NativeCall::ForgetRecipe | NativeCall::HasRecipe => {
                let bit = 1 << (a[0] as u32 & 31);
                match op {
                    NativeCall::LearnRecipe => party.cooking.known |= bit,
                    NativeCall::ForgetRecipe => party.cooking.known &= !bit,
                    _ => value = Some((party.cooking.known & bit) as i32),
                }
            }
            NativeCall::FindPartyMember => {
                value = Some(if a[0] & !0xffff != 0 {
                    let slot = (a[0] & 0xffff) as usize;
                    require(slot < 8, "party slot out of range")?;
                    party.formation.get(slot).copied().map_or(0, i32::from)
                } else {
                    party
                        .formation
                        .iter()
                        .position(|&id| i32::from(id) == a[0])
                        .map_or(0, |index| index as i32 + 1)
                });
            }
            NativeCall::GetTitle if a[1] == 0 => {
                value = Some(i32::from(party.members[member()?].title));
            }
            NativeCall::LearnTitle | NativeCall::GetTitle => {
                let packed = if op == NativeCall::GetTitle {
                    a[1] as u16
                } else {
                    a[0] as u16
                };
                let index = usize::from(packed >> 8);
                let bit = packed & 255;
                require(
                    index < party.members.len() && self.resources.text.titles.contains_key(&packed),
                    "character title is not cooked",
                )?;
                let titles = &mut party.members[index].titles;
                if op == NativeCall::GetTitle {
                    value = Some(i32::from(titles.contains(&(bit as u8))));
                } else {
                    titles.insert(bit as u8);
                }
            }
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
            NativeCall::GetItemCount => {
                let id = u16::try_from(a[0]).map_err(|_| "invalid item")?;
                require(usize::from(id) < data.items.len(), "unknown item")?;
                value = Some(i32::from(party.items.get(&id).copied().unwrap_or(0)));
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
                party.heal(|| crate::world::random(random));
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
                let title = usize::from(party.members[index].title - 1);
                let growth = self
                    .resources
                    .menu_data
                    .as_ref()
                    .and_then(|menu| menu.titles.get(index)?.get(title))
                    .map(|title| title.growth);
                require(
                    title == 0 || growth.is_some(),
                    "equipped title growth is not cooked",
                )?;
                party.raise_level(data, index, level, growth, || crate::world::random(random))?;
            }
            NativeCall::ConfigureSession => {
                let setting = match a[0] {
                    3 => &mut party.settings.preferences.rumble,
                    4 => &mut party.settings.preferences.skit_notifications,
                    5 => &mut party.settings.preferences.stereo,
                    12 => &mut party.leader_locked,
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
    }
}
