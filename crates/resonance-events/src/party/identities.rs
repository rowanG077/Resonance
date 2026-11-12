//! Saved state names the mod entries it holds, because their numeric IDs
//! follow the enabled mods. Loading renumbers the state to the current IDs.
use super::*;
use resonance_content::session::Identities;

/// Follow one saved ID. `Ok(None)` is an entry whose mod is gone.
fn follow(moved: &BTreeMap<u16, Option<u16>>, id: u16) -> Option<u16> {
    moved.get(&id).copied().unwrap_or(Some(id))
}

impl Party {
    /// Move every saved item and technique reference to its current ID, then
    /// adopt the current table so the next save describes this library.
    ///
    /// Held items, equipment and learned artes from a mod that is no longer
    /// enabled are an error: dropping them would silently lose them on the
    /// next save. Mere history of such an entry is forgotten.
    pub(super) fn renumber(&mut self, data: &SessionData) -> anyhow::Result<()> {
        let saved = std::mem::replace(&mut self.identities, data.identities.clone());
        if let Some(moved) = Identities::renumbering(&saved.items, &data.identities.items) {
            let held = |id: u16| {
                follow(&moved, id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "the save holds item {:?} from a mod that is not enabled",
                        saved.items[&id]
                    )
                })
            };
            // Two saved IDs never share a name, so the counts cannot merge.
            self.items = std::mem::take(&mut self.items)
                .into_iter()
                .map(|(item, count)| Ok((held(item)?, count)))
                .collect::<anyhow::Result<_>>()?;
            for member in &mut self.members {
                for item in &mut member.equipment {
                    *item = held(*item)?;
                }
            }
            self.found_items = self
                .found_items
                .iter()
                .filter_map(|&item| follow(&moved, item))
                .collect();
            self.recent_items = self
                .recent_items
                .iter()
                .filter_map(|&item| follow(&moved, item))
                .collect();
        }
        if let Some(moved) = Identities::renumbering(&saved.techniques, &data.identities.techniques)
        {
            let known = |id: u16| {
                follow(&moved, id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "the save has learned arte {:?} from a mod that is not enabled",
                        saved.techniques[&id]
                    )
                })
            };
            for member in &mut self.members {
                member.techniques = member
                    .techniques
                    .iter()
                    .map(|&technique| known(technique))
                    .collect::<anyhow::Result<_>>()?;
                // Everything below refers to a learned arte, checked above.
                member.disabled_techniques = member
                    .disabled_techniques
                    .iter()
                    .filter_map(|&technique| follow(&moved, technique))
                    .collect();
                member.technique_uses = std::mem::take(&mut member.technique_uses)
                    .into_iter()
                    .filter_map(|(technique, uses)| Some((follow(&moved, technique)?, uses)))
                    .collect();
                for technique in &mut member.shortcuts {
                    *technique = follow(&moved, *technique).unwrap_or(0);
                }
                member.unison_arte = member
                    .unison_arte
                    .and_then(|technique| follow(&moved, technique));
                member.arte_bindings.retain_mut(|binding| {
                    match follow(&moved, binding.technique) {
                        Some(technique) => {
                            binding.technique = technique;
                            true
                        }
                        None => false,
                    }
                });
                for shortcut in &mut member.assist_shortcuts {
                    *shortcut = shortcut.and_then(|mut shortcut| {
                        shortcut.technique = follow(&moved, shortcut.technique)?;
                        Some(shortcut)
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[(u16, &str)]) -> BTreeMap<u16, String> {
        entries
            .iter()
            .map(|&(id, name)| (id, name.to_owned()))
            .collect()
    }

    #[test]
    fn loading_follows_mod_entries_to_their_current_ids() {
        let mut data = super::super::tests::data();
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.identities = Identities {
            items: names(&[(600, "berry"), (601, "zest")]),
            techniques: names(&[(300, "ghost-wolf"), (301, "lunar-fang")]),
        };
        party.items = BTreeMap::from([(1, 3), (600, 2), (601, 5)]);
        party.found_items = BTreeSet::from([600]);
        party.recent_items = vec![601, 1, 600];
        party.members[0].equipment[0] = 601;
        party.members[0].techniques = BTreeSet::from([7, 300]);
        party.members[0].technique_uses = BTreeMap::from([(300, 12), (301, 4)]);
        party.members[0].shortcuts = [300, 0, 301, 7];
        party.members[0].unison_arte = Some(301);
        // Another mod now sorts first, so both saved entries moved and swapped order.
        data.identities = Identities {
            items: names(&[(600, "almond"), (601, "zest"), (602, "berry")]),
            techniques: names(&[(300, "lunar-fang"), (301, "ghost-wolf")]),
        };
        party.renumber(&data).unwrap();
        assert_eq!(party.items, BTreeMap::from([(1, 3), (602, 2), (601, 5)]));
        assert_eq!(party.found_items, BTreeSet::from([602]));
        assert_eq!(party.recent_items, [601, 1, 602]);
        assert_eq!(party.members[0].equipment[0], 601);
        assert_eq!(party.members[0].techniques, BTreeSet::from([7, 301]));
        assert_eq!(
            party.members[0].technique_uses,
            BTreeMap::from([(301, 12), (300, 4)])
        );
        assert_eq!(party.members[0].shortcuts, [301, 0, 300, 7]);
        assert_eq!(party.members[0].unison_arte, Some(300));
        assert_eq!(party.identities, data.identities);
    }

    #[test]
    fn a_disabled_mods_held_entries_are_errors_and_its_history_is_forgotten() {
        let data = super::super::tests::data();
        let saved = Identities {
            items: names(&[(600, "berry")]),
            techniques: names(&[(300, "ghost-wolf")]),
        };
        // Only history: the party once found the item and used the arte.
        let mut party = Party::new(&data, Default::default()).unwrap();
        party.identities = saved.clone();
        party.found_items = BTreeSet::from([1, 600]);
        party.recent_items = vec![600, 1];
        party.members[0].technique_uses = BTreeMap::from([(300, 3), (7, 1)]);
        party.members[0].shortcuts = [300, 0, 0, 7];
        party.renumber(&data).unwrap();
        assert_eq!(party.found_items, BTreeSet::from([1]));
        assert_eq!(party.recent_items, [1]);
        assert_eq!(party.members[0].technique_uses, BTreeMap::from([(7, 1)]));
        assert_eq!(party.members[0].shortcuts, [0, 0, 0, 7]);
        assert!(party.identities.is_empty());

        let mut holding = Party::new(&data, Default::default()).unwrap();
        holding.identities = saved.clone();
        holding.items.insert(600, 2);
        let error = holding.renumber(&data).unwrap_err().to_string();
        assert!(
            error.contains("\"berry\"") && error.contains("not enabled"),
            "{error}"
        );

        let mut learned = Party::new(&data, Default::default()).unwrap();
        learned.identities = saved;
        learned.members[0].techniques.insert(300);
        let error = learned.renumber(&data).unwrap_err().to_string();
        assert!(error.contains("\"ghost-wolf\""), "{error}");
    }
}
