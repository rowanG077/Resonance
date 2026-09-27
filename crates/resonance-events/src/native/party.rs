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
            NativeCall::SetCharacterCostume => {
                let id = if a[0] == crate::CONTROLLED_ACTOR {
                    controlled_actor
                } else {
                    a[0]
                };
                value = Some(0);
                if (1..=9).contains(&id) {
                    let member = &mut party.members[id as usize - 1];
                    value = Some(i32::from(member.costume));
                    if a[1] != -1 {
                        require((0..5).contains(&a[1]), "unknown character costume")?;
                        // Lloyd and Colette share meshes for costumes 0 and 3.
                        // Other costume meshes are not available yet.
                        require(
                            a[1] == 0 || (id <= 2 && a[1] == 3),
                            "character costume body is not cooked",
                        )?;
                        member.costume = a[1] as u8;
                    }
                }
            }
            NativeCall::SetRingTimer => party.travel.ring_timer = a[0] as u32,
            NativeCall::GetRingTimer => value = Some(party.travel.ring_timer as i32),
            NativeCall::GetFieldTicks => value = Some(party.travel.field_ticks as i32),
            NativeCall::SetFieldCountdown => party.travel.field_countdown = a[0] as u32,
            NativeCall::GetFieldCountdown => value = Some(party.travel.field_countdown as i32),
            NativeCall::RankCharacterAffinity => {
                // Lloyd is excluded; ties favor the lower member ID.
                let mut candidates: Vec<usize> = (1..9).filter(|&id| a[id] as u8 != 0).collect();
                candidates.sort_by_key(|&id| (std::cmp::Reverse(party.members[id].affinity), id));
                require(!candidates.is_empty(), "affinity ranking has no candidates")?;
                let rank = a[0].max(1) as usize;
                value = Some(candidates[rank.min(candidates.len()) - 1] as i32 + 1);
            }
            NativeCall::ConfigureSorcerersRing => {
                let old: [u8; 2] = party.travel.sorcerers_ring.into();
                self.registers[0] = i32::from(old[0]);
                self.registers[1] = i32::from(old[1]);
                if a[0] != -1 {
                    party.travel.sorcerers_ring = [a[0] as u8, a[1] as u8].try_into()?;
                }
                value = Some(i32::from(old[0]));
            }
            NativeCall::SnapshotParty => match a[0] {
                0 => party.travel.saved_formation = party.formation.clone(),
                1 => party.travel.saved_formation.clear(),
                _ => {}
            },
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
            NativeCall::AddPartyMember if a[0] == 0 => value = Some(1),
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
            NativeCall::RemovePartyMember => {
                let id = a[0] as u8;
                value = Some(
                    if let Some(index) = party.formation.iter().position(|&p| p == id) {
                        party.formation.remove(index);
                        0
                    } else {
                        1
                    },
                );
                if !party.formation.contains(&party.field_leader) {
                    party.field_leader = party.formation.first().copied().unwrap_or(0);
                }
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
            NativeCall::IsTreasureOpened | NativeCall::MarkTreasureOpened => {
                let flag = u16::try_from(a[0]).map_err(|_| "invalid treasure flag")?;
                require(flag < 1024, "invalid treasure flag")?;
                if op == NativeCall::IsTreasureOpened {
                    value = Some(i32::from(party.travel.opened_treasures.contains(&flag)));
                } else {
                    party.travel.opened_treasures.insert(flag);
                }
            }
            NativeCall::GetItemStackLimit => {
                value = Some(i32::from(
                    resonance_content::session::DEFAULT_ITEM_STACK_LIMIT,
                ));
            }
            NativeCall::ChangeItemCount => {
                value = Some(i32::from(party.change_item(
                    data,
                    u16::try_from(a[0]).map_err(|_| "invalid item")?,
                    a[1] as i8,
                )?))
            }
            NativeCall::GetEquippedItem => {
                value = Some(
                    party
                        .members
                        .get(member()?)
                        .and_then(|m| {
                            usize::try_from(a[1])
                                .ok()
                                .and_then(|slot| m.equipment.get(slot))
                        })
                        .copied()
                        .map_or(0, i32::from),
                );
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
                const FULL_RECOVERY: i32 = 0;
                const DAMAGE_TENTH: i32 = 14;
                const DAMAGE_TWENTIETH: i32 = 15;
                match a[0] {
                    FULL_RECOVERY => party.heal(|| crate::world::random(random)),
                    23 => party.revive_incapacitated(),
                    DAMAGE_TENTH | DAMAGE_TWENTIETH => {
                        let percent = if a[0] == DAMAGE_TENTH { 10 } else { 5 };
                        let leader = &party.members[usize::from(party.field_leader - 1)];
                        let amount =
                            u32::from(leader.maximum_vitals()[0]) * u32::from(percent) / 100;
                        self.world
                            .damage_numbers
                            .push(amount as u16, self.world.tick);
                        party.damage_hp_percent(percent);
                    }
                    _ => return Err("unsupported party recovery mode".into()),
                }
            }
            NativeCall::AddGald => value = Some(party.add_gald(a[0]) as i32),
            NativeCall::IsSkitViewed => {
                require((0..=860).contains(&a[0]), "invalid skit history index")?;
                value = Some(i32::from(party.viewed_skits.contains(&(a[0] as u16))));
            }
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
                const GAME_CLEARS: i32 = 11;
                const MENU_DISABLED: i32 = 15;
                if a[0] == GAME_CLEARS {
                    // This query ignores its second argument, including zero.
                    return Ok(NativeResult::Continue(Some(i32::from(party.game_clears))));
                }
                if a[0] == MENU_DISABLED {
                    let previous = self.world.menu_blocked();
                    if a[1] != -1 {
                        self.world.menu_disabled = a[1] & 1 != 0;
                    }
                    return Ok(NativeResult::Continue(Some(i32::from(previous))));
                }
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
