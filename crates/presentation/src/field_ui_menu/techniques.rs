use super::*;
use resonance_game::menu::techniques::{Focus, Page};

pub(super) fn target_cursor(menu: &Menu, slot: usize) -> [f32; 2] {
    target_cursor_page(menu.tech_page(), slot)
}

fn target_cursor_page(page: Page<'_>, slot: usize) -> [f32; 2] {
    let count = page.party.formation.len();
    let panel_x = target_panel_x(count);
    [
        panel_x + 16. + (slot / 4) as f32 * 188.,
        110. + (slot % 4) as f32 * 65.,
    ]
}

fn target_panel_x(count: usize) -> f32 {
    if count > 4 { 236. } else { 428. }
}

fn slot_y(slot: usize, unison: bool) -> f32 {
    if unison {
        120. + slot as f32 * 26.
    } else {
        104. + slot as f32 * 22. + slot.saturating_sub(3) as f32 * 20.
    }
}

impl Drawing<'_> {
    fn arte_bindings_page(&mut self, page: Page<'_>) -> Result<[f32; 2]> {
        let state = page.state;
        let member = page.member();
        let owner = page.member_index();
        self.opacity = 255;
        self.offset = [0.; 2];
        self.heading("Artes")?;
        self.framed([16., 60., 604., 254.], true)?;
        self.text(page.character_name(owner), [40., 64.], 24., GOLD)?;
        let mode = page
            .party
            .settings
            .battle_controls
            .get(state.character)
            .copied()
            .unwrap_or(2);
        self.text(
            ["Manual", "Semi-auto", "Auto"][usize::from(mode)],
            [460., 68.],
            18.,
            if state.focus == Focus::Control {
                GOLD
            } else {
                WHITE
            },
        )?;
        let mut cursor = [32., 76.];
        if state.focus == Focus::Character {
            self.highlight([24., 64., 240., 26.], 255);
        }
        if state.focus == Focus::Control {
            self.highlight([450., 64., 158., 26.], 255);
            cursor = [448., 76.];
        }

        let list = page.technique_list();
        for (offset, &id) in list.iter().skip(state.first).take(8).enumerate() {
            let y = 100. + offset as f32 * 25.;
            if state.first + offset == state.row
                && matches!(
                    state.focus,
                    Focus::List
                        | Focus::BindingError
                        | Focus::ReplaceBinding { .. }
                        | Focus::Target
                        | Focus::CannotForget
                        | Focus::Forget { .. }
                )
            {
                self.highlight([28., y, 580., 24.], 255);
                cursor = [32., y + 8.];
            }
            let learned = member.techniques.contains(&id);
            self.fitted_text(
                &page.data.technique_text(id)?.name,
                [40., y + 1.],
                [18., 22.],
                252.,
                if !learned { DISABLED } else { WHITE },
            )?;
            if let Some(binding) = member.arte_bindings.iter().find(|b| b.technique == id) {
                self.chord_icons(&binding.chord, [310., y], 24., 530.)?;
                if binding
                    .chord
                    .validate(&page.party.settings.preferences.keymap)
                    .is_err()
                {
                    self.text("!", [532., y], 16., 2)?;
                }
            } else {
                self.text_size("-", [310., y + 2.], [14., 20.], GOLD)?;
            }
            if member.disabled_techniques.contains(&id) {
                self.text("AI off", [550., y], 12., DISABLED)?;
            }
        }
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [300., 86.])?;
        }
        if state.first + 8 < list.len() {
            self.scroll_arrow(SCROLL_DOWN, [300., 300.])?;
        }

        self.framed([16., 320., 604., 108.], true)?;
        use resonance_content::input::StandardAction as A;
        let map = &page.party.settings.preferences.keymap;
        if let Some(capture) = &state.capture {
            self.fitted_text(
                "Hold your combo, then release all inputs.",
                [28., 330.],
                [16., 20.],
                580.,
                GOLD,
            )?;
            self.binding_icons(capture.inputs(), "+", [28., 358.], 28., 600.)?;
            self.fitted_text(
                "Release the opener first if it is not part of the combo.",
                [28., 398.],
                [14., 20.],
                580.,
                WHITE,
            )?;
        } else {
            for (action, label, x, right) in [
                (A::Confirm, "Bind", 28., 120.),
                (A::Menu, "Use", 140., 232.),
                (A::Alternate, "AI", 252., 328.),
                (A::Start, "Clear", 348., 470.),
                (A::Cancel, "Back", 490., 608.),
            ] {
                self.input_hint(map, action, label, [x, 330.], right)?;
            }
            if let Some(message) = &state.binding_message {
                for (line, text) in self
                    .wrap_notice(message, 16., 576.)?
                    .iter()
                    .take(3)
                    .enumerate()
                {
                    self.text_size(text, [28., 358. + line as f32 * 22.], [16., 20.], WHITE)?;
                }
            } else if let Some(selected) = page.selected_technique() {
                let text = page.data.technique_text(selected.technique)?;
                for (line, text) in self
                    .wrap_notice(&text.description, 16., 576.)?
                    .iter()
                    .take(2)
                    .enumerate()
                {
                    self.text_size(text, [28., 358. + line as f32 * 22.], [16., 20.], WHITE)?;
                }
                self.text_size(
                    &format!(
                        "TP: {}",
                        page.technique_cost(owner, selected.technique).unwrap_or(0)
                    ),
                    [520., 402.],
                    [16., 20.],
                    GOLD,
                )?;
            }
        }
        if state.focus == Focus::BindingError {
            let error = state
                .binding_error
                .as_deref()
                .context("Missing binding error")?;
            let message = self.wrap_notice(error, 24., 540.)?.join("\n") + "\n";
            let height = message.split('\n').count() as f32 * 26.;
            self.popup(&message, None, 255)?;
            let y = 224. + height / 2. - 26.;
            self.input_hint(map, A::Confirm, "OK", [280., y], 400.)?;
            return Ok([280., y + 8.]);
        }
        if let Focus::ReplaceBinding { yes } = state.focus {
            let pending = state
                .pending_binding
                .as_ref()
                .context("Missing combo replacement")?;
            let name = page.character_name(pending.previous.character);
            let arte = &page.data.technique_text(pending.previous.technique)?.name;
            let message =
                format!("This combo is attached to {name}'s {arte}. Do you want to replace it?");
            let message = self.wrap_notice(&message, 24., 540.)?.join("\n");
            return self
                .popup(&message, Some(yes), 255)?
                .context("Missing replacement choices");
        }
        if state.focus == Focus::Target {
            return self.tech_target_page(page);
        }
        Ok(cursor)
    }
    fn tech_hint(
        &mut self,
        data: &resonance_content::menu_data::MenuData,
        key: &str,
        button: usize,
        right: f32,
        y: f32,
    ) -> Result<f32> {
        let label = data.label(key)?;
        let x = right - self.text_width(label, 20.)? - 24.;
        self.button(button, [x, y])?;
        self.text_size(label, [x + 24., y], [20.; 2], WHITE)?;
        Ok(x)
    }
    pub(super) fn techniques(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        self.techniques_page(menu.tech_page())
    }

    pub(crate) fn techniques_page(&mut self, page: Page<'_>) -> Result<[f32; 2]> {
        if !page.state.unison {
            return self.arte_bindings_page(page);
        }
        let data = page.data;
        let party = page.party;
        let owner = page.tech_member_index();
        // Assist pages render the selected caster's live vitals and learned
        // rows while retaining the primary character's shortcut header.
        let member = &party.members[owner];
        let state = page.state;
        let focus = state.focus;
        let underlying = match focus {
            Focus::Target | Focus::CannotForget | Focus::Forget { .. } => state.return_to,
            _ => focus,
        };
        let auto = page.tech_columns() == 2;
        self.opacity = 255;
        self.offset = [0.; 2];
        self.heading(menu_label(&self.spec.labels, "tech")?)?;
        self.frame([16., 60., if auto { 604. } else { 292. }, 250.])?;
        if !auto {
            self.frame([318., 60., 302., 250.])?;
        }
        self.framed([16., 320., 604., 108.], true)?;
        let header = matches!(focus, Focus::Character | Focus::AssistCharacter);
        self.highlight([24., 60., 144., 24.], if header { 255 } else { 127 });
        self.portrait(owner, member, [176., 20.])?;
        self.text(page.character_name(owner), [32., 60.], 24., WHITE)?;
        let [hp, tp] = member.maximum_vitals();
        self.gauge(false, [24., 85.], [16., 18.], member.hp, hp, Compact)?;
        self.gauge(true, [128., 85.], [16., 18.], member.tp, tp, Compact)?;
        let mut anchor = [32., 68.];
        let formation = party
            .formation
            .iter()
            .position(|&id| usize::from(id - 1) == owner)
            .unwrap();
        if formation < 4 {
            self.frame([328., 18., 292., 32.])?;
            let mut x = 340.;
            for (index, key) in ["tech_manual", "tech_semi_auto", "tech_auto_mode"]
                .into_iter()
                .enumerate()
            {
                let label = data.label(key)?;
                let width = self.text_width(label, 18.)?;
                let selected = index == usize::from(party.settings.battle_controls[formation]);
                if focus == Focus::Control && selected {
                    self.highlight([x, 22., width, 20.], 255);
                    anchor = [x, 30.];
                }
                self.text_size(
                    label,
                    [x, 22.],
                    [18., 20.],
                    if selected { WHITE } else { DISABLED },
                )?;
                x += width + 12.;
            }
        }
        if !auto {
            for slot in 0..if state.unison { 4 } else { 6 } {
                let y = slot_y(slot, state.unison);
                let active =
                    slot == state.slot && matches!(underlying, Focus::Shortcuts | Focus::List);
                if active {
                    self.highlight(
                        [24., y, 276., 20.],
                        if focus == Focus::Shortcuts { 255 } else { 127 },
                    );
                    if focus == Focus::Shortcuts {
                        anchor = [32., y + 8.];
                    }
                }
                let id = if slot < 4 {
                    member.shortcuts[slot]
                } else {
                    if let Some(shortcut) = member.assist_shortcuts[slot - 4] {
                        self.text_size(
                            page.character_name(shortcut.character),
                            [64., if slot == 4 { 192. } else { 232. }],
                            [20.; 2],
                            WHITE,
                        )?;
                        shortcut.technique
                    } else {
                        0
                    }
                };
                if id != 0 {
                    self.text_size(&data.technique_text(id)?.name, [64., y], [18., 20.], WHITE)?;
                }
                let button_y = y - if slot >= 4 { 10. } else { 0. };
                if slot == 3 {
                    self.button(if active { 35 } else { 4 }, [16., button_y])?;
                    self.button(if active { 36 } else { 3 }, [40., button_y])?;
                } else {
                    self.button(
                        if active {
                            [0, 33, 34, 32, 37, 38][slot]
                        } else {
                            [0, 1, 2, 32, 25, 26][slot]
                        },
                        [28., button_y],
                    )?;
                }
            }
        }
        let list = page.technique_list();
        let list_selected = matches!(underlying, Focus::List | Focus::AssistList);
        self.text_size(
            &format!(
                "{}/{}",
                if auto || list_selected {
                    (state.row + 1).to_string()
                } else {
                    "-".into()
                },
                list.len()
            ),
            if auto { [24., 108.] } else { [326., 64.] },
            [16.; 2],
            WHITE,
        )?;
        let visible = if auto { 12 } else { 8 };
        let position = |offset: usize| {
            if auto {
                [
                    28. + (offset % 2) as f32 * 272.,
                    128. + (offset / 2) as f32 * 24.,
                ]
            } else {
                [322., 112. + offset as f32 * 22.]
            }
        };
        if list_selected {
            let [x, y] = position(state.row - state.first);
            self.highlight(
                [x, y, if auto { 272. } else { 290. }, 20.],
                if focus == Focus::Target || matches!(focus, Focus::Forget { .. }) {
                    127
                } else {
                    255
                },
            );
            anchor = [x, y + 8.];
        }
        let starts = self.vertex_counts();
        for (offset, &id) in list.iter().skip(state.first).take(visible).enumerate() {
            let [x, y] = position(offset);
            let tech = &data.techniques[usize::from(id)];
            let tech_text = data.technique_text(id)?;
            let color = if !member.techniques.contains(&id) {
                1
            } else if auto && member.disabled_techniques.contains(&id) {
                DISABLED
            } else if !state.unison && !page.ready(owner, id) {
                2
            } else {
                WHITE
            };
            self.text_size(&tech_text.name, [x + 24., y], [18., 20.], color)?;
            if tech.rank == 0 {
                self.text("B", [x, y], 24., 2)?;
            } else {
                self.sprite(
                    self.spec
                        .sprite(Sprite::TechRanks, usize::from(tech.rank - 1))?,
                    [x, y],
                );
            }
        }
        self.clip_rows(starts, if auto { [128., 272.] } else { [112., 288.] });
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, if auto { [306., 108.] } else { [457., 96.] })?;
        }
        if state.first + visible < list.len() {
            self.scroll_arrow(SCROLL_DOWN, if auto { [306., 266.] } else { [457., 280.] })?;
        }
        match focus {
            Focus::Shortcuts if page.selected_technique().is_some() => {
                self.tech_hint(data, "tech_remove", 10, 612., 80.)?;
            }
            Focus::List if auto => {
                let left = self.tech_hint(data, "tech_auto", 12, 600., 64.)?;
                self.tech_hint(data, "tech_execute", 6, left - 8., 64.)?;
                self.tech_hint(data, "tech_forget", 10, 604., 92.)?;
            }
            Focus::List if !state.unison => {
                let left = self.tech_hint(data, "tech_execute", 12, 612., 80.)?;
                self.tech_hint(data, "tech_forget", 10, left - 8., 80.)?;
            }
            Focus::Character if formation < 4 => {
                let mut y = if auto { 64. } else { 80. };
                if page.tech_unison_available() {
                    self.tech_hint(data, "tech_unison", 12, 600., y)?;
                    y += 28.;
                }
                self.tech_hint(data, "tech_control", 18, 600., y)?;
            }
            _ => (),
        }
        if let Some(selected) = page.tech_description() {
            self.plane = 3;
            self.opacity = 255;
            self.technique_description_cost(
                party,
                data,
                selected,
                page.at_save_point(),
                page.technique_cost(selected.character, selected.technique),
            )?;
        }
        self.plane = 5;
        if focus == Focus::Target {
            self.opacity = 255;
            return self.tech_target_page(page);
        }
        self.opacity = 255;
        if state.unison {
            let label = data.label("tech_unison_title")?;
            let width = self.text_width(label, 20.)?;
            let x = ((640. - width) / 2.).floor();
            self.plane = 6;
            self.shade([x - 4., 16., x + width + 8., 52.]);
            self.plane = 7;
            self.frame([x - 4., 16., width + 8., 32.])?;
            self.text_size(label, [x, 20.], [20.; 2], WHITE)?;
        } else if matches!(focus, Focus::AssistCharacter | Focus::AssistList) {
            let select = data.label("tech_select")?;
            let shortcut = data.label("tech_shortcut")?;
            let start = self.text_width(select, 20.)?;
            let width = start + 24. + self.text_width(shortcut, 20.)?;
            let x = ((640. - width) / 2.).floor();
            self.plane = 6;
            self.shade([x - 4., 16., x + width + 4., 52.]);
            self.plane = 7;
            self.frame([x - 4., 16., width + 8., 32.])?;
            self.text_size(select, [x, 20.], [20.; 2], WHITE)?;
            self.button(if state.slot == 4 { 37 } else { 38 }, [x + start, 20.])?;
            self.text_size(shortcut, [x + start + 24., 20.], [20.; 2], WHITE)?;
        }
        self.opacity = 255;
        Ok(anchor)
    }

    pub(super) fn tech_cursors(&mut self, menu: &Menu) {
        self.tech_cursors_page(menu.tech_page());
    }

    pub(crate) fn tech_cursors_page(&mut self, page: Page<'_>) {
        let state = page.state;
        if !state.unison {
            if page.tech_target_visible() {
                self.cursor([24., 108. + (state.row - state.first) as f32 * 25.], 127);
            }
            return;
        }
        let plane = self.plane.max(5);
        self.plane = 5;
        if page.tech_target_visible() {
            let row = state.row - state.first;
            let position = if page.tech_columns() == 2 {
                [28. + (row % 2) as f32 * 272., 136. + (row / 2) as f32 * 24.]
            } else {
                [322., 120. + row as f32 * 22.]
            };
            self.cursor(position, 127);
        }
        if !matches!(state.focus, Focus::Character | Focus::AssistCharacter) {
            self.cursor([32., 68.], 127);
        }
        if page.tech_columns() == 1
            && matches!(
                state.focus,
                Focus::List | Focus::Target | Focus::CannotForget | Focus::Forget { .. }
            )
        {
            self.cursor([32., slot_y(state.slot, state.unison) + 8.], 127);
        }
        self.plane = plane;
    }

    fn technique_description_cost(
        &mut self,
        party: &resonance_events::party::Party,
        data: &resonance_content::menu_data::MenuData,
        selected: resonance_events::party::TechniqueShortcut,
        _at_save_point: bool,
        cost: Option<u32>,
    ) -> Result<()> {
        let tech = data
            .techniques
            .get(usize::from(selected.technique))
            .context("technique definition was not prepared")?;
        let tech_text = data.technique_text(selected.technique)?;
        let caster = party
            .members
            .get(selected.character)
            .context("technique description member is absent")?;
        self.shadowed_text(&tech_text.name, [40., 322.], 28., 2.)?;
        self.text(&tech_text.description, [48., 358.], 20., WHITE)?;
        self.text("TP : ", [488., 336.], 16., GOLD)?;
        let x = 488. + self.text_width("TP : ", 16.)?;
        self.text(
            &cost.map_or_else(|| "--".into(), |value| value.to_string()),
            [x, 336.],
            16.,
            WHITE,
        )?;
        if tech.element != 0 {
            self.sprite(
                self.spec
                    .sprite(Sprite::Elements, usize::from(tech.element - 1))?,
                [456., 336.],
            );
        }
        if let Some(&uses) = caster
            .technique_uses
            .get(&selected.technique)
            .filter(|n| **n != 0)
        {
            // Draw each complete description below the next description during a crossfade,
            // before the usage frame and glyphs.
            let text_plane = self.plane;
            self.shade([548., 372., 620., 428.]);
            self.plane = text_plane + 1;
            self.frame([548., 372., 72., 56.])?;
            self.text(
                data.presentation
                    .labels
                    .get("tech_usage")
                    .context("technique usage label was not prepared")?,
                [552., 376.],
                16.,
                GOLD,
            )?;
            self.text(&uses.to_string(), [560., 400.], 16., WHITE)?;
            self.plane = text_plane;
        }
        Ok(())
    }

    pub(crate) fn tech_target_page(&mut self, page: Page<'_>) -> Result<[f32; 2]> {
        let data = page.data;
        let party = page.party;
        let state = page.state;
        let all = page.tech_targets_all();
        let target = usize::from(party.formation[state.target] - 1);
        let member = &party.members[target];
        let stats = page.target_stats(self.stats)?;
        self.portrait(target, member, [32., 346.])?;
        self.text_size(page.character_name(target), [96., 332.], [20.; 2], WHITE)?;
        self.gauge(false, [224., 332.], [16., 20.], member.hp, stats.hp, Full)?;
        self.gauge(true, [408., 332.], [16., 20.], member.tp, stats.tp, Full)?;
        for (i, (key, value)) in [
            ("strength", stats.strength),
            (if target == 0 { "slash" } else { "attack" }, stats.slash),
            ("thrust", stats.thrust),
            ("defense", stats.defense),
            ("luck", stats.luck),
            ("accuracy", stats.accuracy),
            ("evasion", stats.evasion),
            ("intelligence", stats.intelligence),
        ]
        .into_iter()
        .enumerate()
        {
            if i == 2 && target != 0 {
                continue;
            }
            let [x, y] = [104. + (i % 4) as f32 * 128., 356. + (i / 4) as f32 * 24.];
            self.text_size(data.label(&format!("tech_{key}"))?, [x, y], [20.; 2], GOLD)?;
            self.text_size(&format!("{value:4}"), [x + 48., y], [16., 20.], WHITE)?;
        }
        self.plane = 6;
        self.opacity = 255;
        let label = data.label(if all {
            "tech_target_all"
        } else {
            "tech_target"
        })?;
        let width = self.text_width(label, 20.)?;
        let panel_width = if party.formation.len() > 4 {
            384.
        } else {
            192.
        };
        let panel_x = target_panel_x(party.formation.len());
        let label_y = 20.;
        self.shade([
            316. - width / 2.,
            label_y - 4.,
            324. + width / 2.,
            label_y + 32.,
        ]);
        self.shade([panel_x, 60., panel_x + panel_width, 324.]);
        self.plane = 7;
        self.frame([316. - width / 2., label_y - 4., width + 8., 32.])?;
        self.text_size(label, [320. - width / 2., label_y], [20.; 2], WHITE)?;
        if all {
            self.button(6, [320. - width / 2., label_y])?;
        }
        self.frame([panel_x, 60., panel_width, 264.])?;
        for (slot, &id) in party.formation.iter().enumerate() {
            let character = &party.members[usize::from(id - 1)];
            let [hp, tp] = character.maximum_vitals();
            let x = panel_x + (slot / 4) as f32 * 188.;
            let y = 62. + (slot % 4) as f32 * 65.;
            if all || slot == state.target {
                self.highlight([x + 16., y, 160., 64.], 255);
            }
            self.text(&(slot + 1).to_string(), [x, y + 20.], 16., WHITE)?;
            self.portrait(usize::from(id - 1), character, [x + 16., y])?;
            self.gauge(
                false,
                [x + 80., y + 7.],
                [16., 18.],
                character.hp,
                hp,
                Stacked,
            )?;
            self.gauge(
                true,
                [x + 80., y + 39.],
                [16., 18.],
                character.tp,
                tp,
                Stacked,
            )?;
        }
        self.opacity = 255;
        Ok(target_cursor_page(page, state.target))
    }

    pub(super) fn tech_popup(&mut self, menu: &Menu) -> Result<Option<[f32; 2]>> {
        self.tech_popup_page(menu.tech_page())
    }

    pub(crate) fn tech_popup_page(&mut self, page: Page<'_>) -> Result<Option<[f32; 2]>> {
        let data = page.data;
        let state = page.state;
        let cannot_forget = state.focus == Focus::CannotForget;
        if !cannot_forget && !matches!(state.focus, Focus::Forget { .. }) {
            return Ok(None);
        }
        self.plane = 6;
        self.opacity = 255;
        self.quad_role(
            DrawRole::Background,
            FONT,
            self.screen,
            [0.5; 4],
            [0., 0., 0., 0.5],
        );
        self.opacity = 255;
        if cannot_forget {
            let text = data.label("tech_cannot_forget")?;
            let width = self.text_width(text, 32.)?;
            let x = ((640. - width) / 2.).floor();
            self.shade([x - 12., 196., x + width + 12., 252.]);
            self.plane = 7;
            self.colored_frame([x - 12., 196., width + 24., 56.], false, self.popup_color())?;
            self.text_size(text, [x, 208.], [32.; 2], WHITE)?;
            self.opacity = 255;
            return Ok(None);
        }
        let yes = matches!(state.focus, Focus::Forget { yes: true });
        self.shade([52., 99., 592., 353.]);
        self.plane = 7;
        self.colored_frame([52., 99., 536., 250.], false, self.popup_color())?;
        let list = page.technique_list();
        let selected = list.get(state.row).context("missing technique to forget")?;
        let tech = &data.techniques[usize::from(*selected)];
        let tech_text = data.technique_text(*selected)?;
        self.text(
            &tech_text.name,
            [
                ((640. - self.text_width(&tech_text.name, 24.)?) / 2.).floor(),
                103.,
            ],
            24.,
            WHITE,
        )?;
        let label = data.label("tech_related")?;
        let width = label
            .lines()
            .map(|s| self.text_width(s, 24.))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(0., f32::max);
        self.text(label, [((640. - width) / 2.).floor(), 127.], 24., GOLD)?;
        let y = 127. + label.lines().count() as f32 * (24. + LINE_SPACING);
        for (i, id) in tech
            .alternatives
            .iter()
            .filter(|id| **id != 0 && list.contains(id))
            .enumerate()
        {
            self.text(
                &data.technique_text(*id)?.name,
                [56. + (i % 2) as f32 * 264., y + (i / 2) as f32 * 24.],
                20.,
                WHITE,
            )?;
        }
        self.text(data.label("tech_forget_warning")?, [68., y + 72.], 24., 2)?;
        let label = data.label("tech_forget_confirm")?;
        self.text(
            label,
            [
                ((640. - self.text_width(label, 24.)?) / 2.).floor(),
                y + 120.,
            ],
            24.,
            WHITE,
        )?;
        let width = self.text_width(menu_label(&self.spec.labels, "yes")?, 24.)?;
        let x =
            ((640. - width - 24. - self.text_width(menu_label(&self.spec.labels, "no")?, 24.)?)
                / 2.)
                .floor();
        let anchor = [x + if yes { 0. } else { width + 24. }, y + 152.];
        for (i, key) in ["yes", "no"].into_iter().enumerate() {
            let label = menu_label(&self.spec.labels, key)?;
            let x = x + if i == 0 { 0. } else { width + 24. };
            if yes == (i == 0) {
                self.highlight([x, y + 144., self.text_width(label, 24.)?, 24.], 255);
            }
            self.text(label, [x, y + 144.], 24., WHITE)?;
        }
        self.opacity = 255;
        Ok(Some(anchor))
    }
}

#[cfg(test)]
mod binding_tests {
    use super::super::test_support::Fixture;
    use super::*;
    use resonance_content::input::{InputChord, PhysicalInput};
    use resonance_events::party::Party;
    use resonance_game::menu::{
        MenuAction,
        techniques::{Context, Tech},
    };

    #[test]
    #[ignore = "requires locally cooked menus; exercises recording, replacement and drawing"]
    fn arte_list_records_one_combo_and_confirms_transfers_across_characters() -> Result<()> {
        let mut fixture = Fixture::load()?;
        let connected = [false; 4];
        let context = Context::Field {
            at_save_point: false,
            connected: &connected,
        };
        let mut tech = Tech::opening(0, context, &fixture.party, &fixture.session, &fixture.data)?;
        assert_eq!(tech.focus, Focus::List);
        let id = tech
            .page(&fixture.party, &fixture.session, &fixture.data, context)
            .selected_technique()
            .unwrap()
            .technique;
        let record =
            |tech: &mut Tech, party: &mut Party, inputs: Vec<PhysicalInput>| -> Result<()> {
                tech.step(
                    Some(MenuAction::Confirm),
                    party,
                    &fixture.session,
                    &fixture.data,
                    context,
                )?;
                assert_eq!(tech.focus, Focus::List);
                assert!(tech.capture.is_some());
                tech.capture_physical(party, &Default::default());
                tech.capture_physical(party, &inputs.into_iter().collect());
                tech.capture_physical(party, &Default::default());
                Ok(())
            };
        record(
            &mut tech,
            &mut fixture.party,
            vec![PhysicalInput::Key("Escape".into())],
        )?;
        assert_eq!(tech.binding_message.as_deref(), Some("Combo assigned."));
        let before = fixture.party.members[0].arte_bindings.clone();
        record(
            &mut tech,
            &mut fixture.party,
            vec![PhysicalInput::Key("Space".into())],
        )?;
        assert_eq!(tech.focus, Focus::BindingError);
        assert!(tech.binding_error.as_ref().unwrap().contains("Attack"));
        let row = tech.row;
        tech.step(
            Some(MenuAction::Down),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        assert_eq!(tech.focus, Focus::BindingError);
        assert_eq!(tech.row, row);
        let mut drawing = fixture.drawing(0);
        let anchor = drawing.techniques_page(tech.page(
            &fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        ))?;
        drawing.cursor_trail(anchor, 255);
        fixture.export_drawing(&drawing, "RESONANCE_COMBAT_CONFLICT_PREVIEW")?;
        drop(drawing);
        tech.step(
            Some(MenuAction::Cancel),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        assert_eq!(fixture.party.members[0].arte_bindings, before);
        record(
            &mut tech,
            &mut fixture.party,
            vec![
                PhysicalInput::Key("ShiftLeft".into()),
                PhysicalInput::Key("Tab".into()),
            ],
        )?;
        assert_eq!(tech.focus, Focus::BindingError);
        assert!(tech.binding_error.as_ref().unwrap().contains("overlay"));
        let mut drawing = fixture.drawing(0);
        let anchor = drawing.techniques_page(tech.page(
            &fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        ))?;
        drawing.cursor_trail(anchor, 255);
        fixture.export_drawing(&drawing, "RESONANCE_BINDING_ERROR_PREVIEW")?;
        drop(drawing);
        tech.step(
            Some(MenuAction::Confirm),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        use resonance_content::input::AxisDirection;
        record(
            &mut tech,
            &mut fixture.party,
            vec![
                PhysicalInput::Button("South".into()),
                PhysicalInput::Axis(AxisDirection::LeftRight),
            ],
        )?;
        assert_eq!(fixture.party.members[0].arte_bindings.len(), 1);
        let chord = InputChord(vec![
            PhysicalInput::Key("Enter".into()),
            PhysicalInput::Key("KeyW".into()),
        ]);
        record(&mut tech, &mut fixture.party, chord.0.clone())?;
        assert_eq!(fixture.party.members[0].arte_bindings.len(), 1);
        assert!(
            fixture.party.members[0].arte_bindings[0]
                .chord
                .same_inputs(&chord)
        );
        let mut drawing = fixture.drawing(0);
        let anchor = drawing.techniques_page(tech.page(
            &fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        ))?;
        assert!(
            drawing
                .batches
                .keys()
                .any(|key| key.2 == MaterialKey::InputIcons)
        );
        drawing.cursor_trail(anchor, 255);
        fixture.export_drawing(&drawing, "RESONANCE_ARTES_PREVIEW")?;
        drop(drawing);

        // A collision on another member requires an explicit decision.
        let owner = fixture.party.arte_binding_owner(&chord).unwrap();
        let mut other = Tech::opening(1, context, &fixture.party, &fixture.session, &fixture.data)?;
        let destination = other
            .page(&fixture.party, &fixture.session, &fixture.data, context)
            .selected_technique()
            .unwrap();
        let before = serde_json::to_value(&fixture.party.members)?;
        record(&mut other, &mut fixture.party, chord.0.clone())?;
        assert_eq!(other.focus, Focus::ReplaceBinding { yes: false });
        assert_eq!(other.pending_binding.as_ref().unwrap().previous, owner);
        let mut drawing = fixture.drawing(0);
        let anchor = drawing.techniques_page(other.page(
            &fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        ))?;
        drawing.cursor_trail(anchor, 255);
        fixture.export_drawing(&drawing, "RESONANCE_COMBO_POPUP_PREVIEW")?;
        drop(drawing);
        other.step(
            Some(MenuAction::Cancel),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        assert_eq!(serde_json::to_value(&fixture.party.members)?, before);
        record(&mut other, &mut fixture.party, chord.0.clone())?;
        other.step(
            Some(MenuAction::Confirm),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        assert_eq!(
            serde_json::to_value(&fixture.party.members)?,
            before,
            "default No preserves the original owner"
        );
        record(&mut other, &mut fixture.party, chord.0.clone())?;
        other.step(
            Some(MenuAction::Right),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        assert_eq!(other.focus, Focus::ReplaceBinding { yes: true });
        let result = other.step(
            Some(MenuAction::Confirm),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        assert!(result.changed);
        assert!(fixture.party.members[0].arte_bindings.is_empty());
        assert_eq!(fixture.party.arte_binding_owner(&chord), Some(destination));
        other.step(
            Some(MenuAction::Details),
            &mut fixture.party,
            &fixture.session,
            &fixture.data,
            context,
        )?;
        assert!(fixture.party.arte_binding_owner(&chord).is_none());
        assert!(fixture.party.members[0].techniques.contains(&id));
        Ok(())
    }
}
