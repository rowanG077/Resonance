//! Recover optional selections and history without inventing progression.
use super::*;

fn formation(members: &mut Vec<u8>, count: usize) {
    let mut seen = BTreeSet::new();
    members.retain(|&id| id > 0 && usize::from(id) <= count && seen.insert(id));
    members.truncate(8);
}

fn strategy(choices: &mut [u8; 3]) {
    for (choice, count) in choices
        .iter_mut()
        .zip(resonance_content::menu_data::STRATEGY_COUNTS)
    {
        if usize::from(*choice) >= count {
            *choice = 0;
        }
    }
}

impl Party {
    pub(super) fn recover_selections(&mut self, data: &SessionData) {
        formation(&mut self.formation, self.members.len());
        if self.formation.is_empty() {
            // A saved leader still identifies a usable member if the order is lost.
            self.formation.push(
                if (1..=self.members.len()).contains(&usize::from(self.field_leader)) {
                    self.field_leader
                } else {
                    initial_leader()
                },
            );
        }
        if !self.formation.contains(&self.field_leader) {
            self.field_leader = self.formation[0];
        }
        formation(&mut self.travel.saved_formation, self.members.len());
        self.travel.opened_treasures.retain(|id| *id < 1024);
        let location = |id: &u16| (1..=98).contains(id) || (257..=337).contains(id);
        self.travel.current_location = self.travel.current_location.filter(location);
        self.travel.visited_locations.retain(location);
        self.travel.visited_shops.retain(|id| *id < 52);
        self.viewed_skits.retain(|id| (1..=860).contains(id));
        self.figurines
            .retain(|&id| usize::from(id) < resonance_content::figurine::FIGURINE_COUNT);
        self.monsters.retain(|&id, knowledge| {
            if knowledge.variant >= 16 {
                knowledge.variant = 0;
            }
            usize::from(id) < resonance_content::monster::MONSTER_COUNT
        });
        if usize::from(self.cooking.recipe) >= resonance_content::menu_data::RECIPE_COUNT {
            self.cooking.recipe = 0;
        }
        if usize::from(self.cooking.chef) >= self.members.len() {
            self.cooking.chef = self.field_leader - 1;
        }
        self.battle_rules.modifiers &= crate::battle::Rules::SUPPORTED_MODIFIERS;
        for (control, default) in self
            .settings
            .battle_controls
            .iter_mut()
            .zip(Settings::default().battle_controls)
        {
            if *control > 2 {
                *control = default;
            }
        }
        let preferences = &mut self.settings.preferences;
        if preferences.battle_rank > 2 {
            preferences.battle_rank = Default::default();
        }
        preferences.message_speed = preferences.message_speed.min(9);
        preferences.window = preferences.window.min(2);
        preferences.background = preferences.background.min(5);
        for channel in 0..preferences.volumes.channels().len() {
            let volume = preferences.volumes.channel_mut(channel);
            *volume = (*volume).min(127);
        }
        for size in &mut preferences.resolution {
            *size = (*size).clamp(1, 8192);
        }
        if preferences
            .button_map
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            != (0..7).collect()
        {
            preferences.button_map = [0, 1, 2, 3, 4, 5, 6];
        }
        if preferences.keymap.validate().is_err() {
            preferences.keymap = Default::default();
        }
        if let Some(presets) = &mut self.strategy_presets {
            for preset in presets {
                preset.name = preset
                    .name
                    .chars()
                    .filter(|c| c.is_ascii_graphic() || *c == ' ')
                    .take(7)
                    .collect();
                if preset.name.is_empty() {
                    preset.name = "Custom".into();
                }
                for choices in &mut preset.members {
                    strategy(choices);
                }
            }
        }
        for (index, member) in self.members.iter_mut().enumerate() {
            if let Some(name) = &mut member.name {
                *name = name.chars().filter(|c| !c.is_control()).take(12).collect();
                if name.is_empty() {
                    member.name = None;
                }
            }
            if member.costume >= 5 {
                member.costume = 0;
            }
            let title_count = data
                .rules
                .as_ref()
                .map_or(1, |rules| rules.titles.get(index).map_or(0, Vec::len));
            member
                .titles
                .retain(|&id| id != 0 && usize::from(id) <= title_count);
            if member.title == 0 || usize::from(member.title) > title_count {
                member.title = initial_title();
            }
            strategy(&mut member.strategy);
        }
    }
}
