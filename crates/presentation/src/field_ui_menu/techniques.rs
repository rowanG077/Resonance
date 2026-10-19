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

    pub(super) fn technique_description(
        &mut self,
        party: &resonance_events::party::Party,
        data: &resonance_content::menu_data::MenuData,
        selected: resonance_events::party::TechniqueShortcut,
        at_save_point: bool,
    ) -> Result<()> {
        let cost = party
            .members
            .get(selected.character)
            .context("technique description member is absent")?
            .technique_cost(data, selected.technique, at_save_point);
        self.technique_description_cost(party, data, selected, at_save_point, Some(u32::from(cost)))
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
        let stats = member.stats(data);
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
