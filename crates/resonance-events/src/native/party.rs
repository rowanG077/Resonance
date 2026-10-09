//! Native argument adaptation for persistent party services.
use super::{NativeHost, NativeResult, require};
use symphonia_script::NativeCall;
use symphonia_script_vm::Memory;

impl NativeHost<'_> {
    pub(super) fn party(
        &mut self,
        op: NativeCall,
        a: &[i32],
        memory: &mut Memory,
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
            NativeCall::ConfigureBattleRules => {
                use std::mem::replace;
                let rules = &mut party.battle_rules;
                value = Some(match a[0] {
                    0 => i32::from(replace(&mut rules.modifiers, a[1] as u16)),
                    1 => i32::from(replace(&mut rules.disabled_commands, a[1] as u8)),
                    2 => i32::from(replace(&mut rules.coliseum, a[1] != 0)),
                    3 => i32::from(replace(&mut rules.attack_adjustment, a[1] as i8)),
                    4 => i32::from(replace(&mut rules.defense_adjustment, a[1] as i8)),
                    5 => i32::from(replace(&mut rules.intelligence_adjustment, a[1] as i8)),
                    _ => return Err("unknown battle rule".into()),
                });
            }
            NativeCall::ConfigureMonsterKnowledge => {
                let id = u8::try_from(a[0])
                    .ok()
                    .filter(|&id| usize::from(id) < resonance_content::monster::MONSTER_COUNT)
                    .ok_or("unknown monster")?;
                require((-1..64).contains(&a[1]), "invalid monster knowledge flags")?;
                let previous = party
                    .monsters
                    .get(&id)
                    .map_or(0, |knowledge| knowledge.script_flags());
                value = Some(i32::from(previous));
                if a[1] != -1 {
                    let flags = if a[1] == 0 { 0 } else { previous | a[1] as u8 };
                    party
                        .monsters
                        .entry(id)
                        .or_default()
                        .set_script_flags(flags);
                }
            }
            NativeCall::ConfigureFigurine => {
                let id = u16::try_from(a[1]).map_err(|_| "invalid figurine")?;
                require(
                    usize::from(id) < resonance_content::figurine::FIGURINE_COUNT,
                    "unknown figurine",
                )?;
                value = Some(i32::from(party.figurines.contains(&id)));
                match a[0] {
                    0 => {
                        party.figurines.insert(id);
                    }
                    1 => {
                        party.figurines.remove(&id);
                    }
                    2 => {}
                    _ => return Err("unknown figurine operation".into()),
                }
            }
            NativeCall::RecipeProficiency => {
                const MASTERED: u8 = 8;
                const POINTS_PER_RANK: u8 = 3;
                let skill = party.members[member()?]
                    .cooking
                    .get_mut(a[1] as usize)
                    .ok_or("unknown recipe")?;
                if a[2] > 0 {
                    *skill = (i64::from(*skill) + i64::from(a[2]) * i64::from(POINTS_PER_RANK))
                        .min(i64::from(MASTERED)) as u8;
                }
                value = Some(i32::from(if *skill == MASTERED {
                    3
                } else {
                    *skill / POINTS_PER_RANK
                }));
            }
            NativeCall::SetEquippedTitle => {
                let index = member()?;
                let owner = (index as u16) << 8;
                value = Some(i32::from(owner | u16::from(party.members[index].title)));
                if a[1] >= 0 {
                    let packed = u16::try_from(a[1]).map_err(|_| "invalid title")?;
                    require(
                        packed & 0xff00 == owner
                            && self.resources.text.titles.contains_key(&packed),
                        "unknown title for this member",
                    )?;
                    party.members[index].title = packed as u8;
                }
            }
            NativeCall::ForgetTitle => {
                let packed = a[0] as u16;
                let member = party
                    .members
                    .get_mut(usize::from(packed >> 8))
                    .ok_or("unknown party member")?;
                member.titles.remove(&(packed as u8));
            }
            NativeCall::AddGrade => {
                party.grade_hundredths = (i64::from(party.grade_hundredths) + i64::from(a[0]) * 100)
                    .clamp(0, 99_999_999) as u32;
                value = Some(party.grade_hundredths as i32);
            }
            NativeCall::ConfigureExGem => {
                let index = member()?;
                let slot = usize::try_from(a[1])
                    .ok()
                    .filter(|&slot| slot < 4)
                    .ok_or("invalid EX gem slot")?;
                require((-1..=5).contains(&a[2]), "invalid EX gem level")?;
                let member = &mut party.members[index];
                self.registers[0] = i32::from(member.ex_gems[slot]);
                self.registers[1] = i32::from(member.ex_skills[slot]);
                if a[2] != -1 {
                    member.ex_gems[slot] = a[2] as u8;
                    member.ex_skills[slot] = 0;
                }
                if a[3] == 0 {
                    member.ex_skills[slot] = 0;
                } else if a[3] != -1 {
                    let skill = u8::try_from(a[3]).map_err(|_| "invalid EX skill")?;
                    if party.members[index].ex_skills[slot] != skill {
                        require(
                            party.set_ex_skill(data, index, slot, skill)?,
                            "EX skill does not match gem",
                        )?;
                    }
                }
                party.members[index].clamp_vitals();
            }
            NativeCall::SetCharacterName => {
                let index = member()?;
                let message = self
                    .resources
                    .messages
                    .get(a[1] as usize)
                    .ok_or("character name message is missing")?;
                let resolved = crate::dialogue::resolve(
                    message,
                    memory,
                    &self.resources.names(Some(party)),
                    &self.resources.text,
                    controlled_actor,
                )?;
                let mut name = String::new();
                for token in resolved.tokens {
                    let crate::dialogue::TextToken::Text { text } = token else {
                        return Err("character name contains a dialogue control".into());
                    };
                    name.push_str(&text);
                }
                require(
                    !name.is_empty()
                        && name.chars().count() <= 12
                        && !name.chars().any(char::is_control),
                    "invalid character name",
                )?;
                party.members[index].name = Some(name);
            }
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
                        require(
                            self.resources
                                .model(resonance_content::appearance::costume_resource(
                                    id as u32, a[1] as u8,
                                ))
                                .is_some(),
                            "character costume body is not cooked",
                        )?;
                        member.costume = a[1] as u8;
                        self.world.update_costumes(self.resources);
                    }
                }
            }
            NativeCall::SetRingTimer => party.travel.ring_timer = a[0] as u32,
            NativeCall::GetRingTimer => value = Some(party.travel.ring_timer as i32),
            NativeCall::GetFieldTicks => value = Some(party.travel.field_ticks as i32),
            NativeCall::ResetFieldTicks => party.travel.field_ticks = 0,
            NativeCall::ResetScenarioTicks => party.travel.scenario_ticks = 0,
            NativeCall::GetScenarioTicks => value = Some(party.travel.scenario_ticks as i32),
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
                let index = member()?;
                value = Some(((index as i32) << 8) | i32::from(party.members[index].title));
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
                value = Some(i32::from(party.stack_limit()));
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
            NativeCall::LearnTechnique | NativeCall::HasTechnique | NativeCall::ForgetTechnique => {
                let id = u16::try_from(a[1]).map_err(|_| "invalid technique")?;
                let index = member()?;
                require(
                    data.characters[index].allowed_techniques.contains(&id),
                    "technique is not available to this member",
                )?;
                match op {
                    NativeCall::LearnTechnique => {
                        party.members[index].techniques.insert(id);
                    }
                    NativeCall::HasTechnique => {
                        value = Some(i32::from(party.members[index].techniques.contains(&id)))
                    }
                    _ => party.remove_technique(index, id),
                }
            }
            NativeCall::HealParty => {
                const PERCENT: [i16; 5] = [100, 50, 10, 5, 1];
                match a[0] {
                    0 => party.heal(|| crate::world::random(random)),
                    23 => party.revive_incapacitated(),
                    mode @ 2..=22 => {
                        let change = match mode {
                            2..=6 => [PERCENT[(mode - 2) as usize], 0],
                            7..=11 => [0, PERCENT[(mode - 7) as usize]],
                            12..=16 => [-PERCENT[(mode - 12) as usize], 0],
                            17..=21 => [0, -PERCENT[(mode - 17) as usize]],
                            _ => [-20, 0],
                        };
                        if change[0] < 0 {
                            let leader = &party.members[usize::from(party.field_leader - 1)];
                            let amount = u32::from(leader.maximum_vitals()[0])
                                * u32::from(change[0].unsigned_abs())
                                / 100;
                            self.world
                                .damage_numbers
                                .push(amount as u16, self.world.tick);
                        }
                        party.adjust_vitals_percent(change);
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
                if matches!(a[0], 13 | 14) {
                    use crate::session_screen::{Request, Target};
                    require(
                        self.world.screen_request.is_none(),
                        "session screen already requested",
                    )?;
                    let operation = self.world.operations.begin()?;
                    self.world.screen_request = Some(Request {
                        target: if a[0] == 13 {
                            Target::Title
                        } else {
                            Target::GameOver
                        },
                        operation: operation.clone(),
                    });
                    *self.wait = Some(crate::operation::Wait::Complete(operation));
                    return Ok(NativeResult::Suspend);
                }
                if a[0] == 16 {
                    self.world
                        .start_new_game_plus(memory, data)
                        .map_err(|e| e.to_string())?;
                    return Ok(NativeResult::Continue(Some(0)));
                }
                if a[0] == 10 {
                    let shop = &self
                        .resources
                        .menu_data
                        .as_ref()
                        .ok_or("Grade Shop is not cooked")?
                        .grade_shop;
                    return Ok(NativeResult::Continue(Some(i32::from(
                        party.record_clear(shop).map_err(|e| e.to_string())?,
                    ))));
                }
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
