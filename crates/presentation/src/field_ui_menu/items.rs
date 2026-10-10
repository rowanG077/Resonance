use super::*;
use anyhow::ensure;
use resonance_content::menu_data::MenuSpan;
use resonance_game::battle::command::ListFrame;
use resonance_game::menu::items::{Description, Focus, ItemRow, VISIBLE_ITEMS};

const KEY_ITEM_CATEGORY: u8 = 45;

#[derive(Default)]
pub(super) struct Animation {
    opened: Option<u32>,
    target: Option<(Focus, u32)>,
}

#[derive(Clone, Copy)]
pub(super) struct Visual {
    pub fade: u8,
    target_opacity: u8,
    target_age: u32,
}

impl Animation {
    const SLIDE_TICKS: u32 = 8;

    pub fn settled(&self, focus: Focus, tick: u32) -> bool {
        self.opened
            .is_some_and(|opened| tick.wrapping_sub(opened) >= Self::SLIDE_TICKS)
            && if matches!(focus, Focus::Target | Focus::Transform(_)) {
                self.target.is_some_and(|(shown, started)| {
                    shown == focus && tick.wrapping_sub(started) >= Self::SLIDE_TICKS
                })
            } else {
                self.target.is_none()
            }
    }

    pub fn sample(&mut self, focus: Focus, tick: u32) -> Visual {
        // These transitions are purely visual; menu input always uses the current selection.
        let opacity = |age: u32| (age.min(Self::SLIDE_TICKS) * 255 / Self::SLIDE_TICKS) as u8;
        let opened = *self.opened.get_or_insert(tick);
        let target_age = if matches!(focus, Focus::Target | Focus::Transform(_)) {
            let (previous, started) = self.target.get_or_insert((focus, tick));
            if *previous != focus {
                *previous = focus;
                *started = tick;
            }
            tick.wrapping_sub(*started)
        } else {
            self.target = None;
            0
        };
        Visual {
            fade: 255 - opacity(tick.wrapping_sub(opened)),
            target_opacity: opacity(target_age),
            target_age,
        }
    }
}

#[derive(Debug, PartialEq)]
struct InventoryLayout {
    list_offset: f32,
    description_offset: f32,
    heading_offset: f32,
    first: usize,
    visible: usize,
    scroll_offset: i32,
    anchor: [f32; 2],
}

struct InventoryList<'a> {
    rows: &'a [ItemRow],
    selected: Option<usize>,
    first: usize,
    scroll: i8,
    fade: u8,
    selection_alpha: u8,
    description: Description,
    previous_description: Description,
    description_opacity: u8,
}

fn inventory_layout(list: &InventoryList<'_>) -> Result<InventoryLayout> {
    ensure!(
        list.selected
            .is_none_or(|selected| selected < list.rows.len() && list.first <= selected),
        "invalid inventory selection"
    );
    ensure!(
        list.first.is_multiple_of(2) && (-4..=4).contains(&list.scroll),
        "invalid inventory scroll"
    );
    let first = list
        .first
        .checked_sub(usize::from(list.scroll > 0) * 2)
        .context("inventory scroll precedes first row")?;
    let fade = u32::from(list.fade);
    let list_offset = (fade * 628 / 256) as f32;
    let cell = list
        .selected
        .unwrap_or(list.first)
        .saturating_sub(list.first);
    Ok(InventoryLayout {
        list_offset,
        description_offset: (fade * 120 / 256) as f32,
        heading_offset: -((fade * 76 / 256) as f32),
        first,
        visible: VISIBLE_ITEMS + usize::from(list.scroll != 0) * 2,
        scroll_offset: scroll_offset(list.scroll, 26),
        anchor: [
            48. + (cell % 2) as f32 * 296. + list_offset,
            96. + (cell / 2) as f32 * 26.,
        ],
    })
}

impl MenuArtwork {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_battle_items(
        &mut self,
        frame: &ListFrame,
        data: &resonance_content::menu_data::MenuData,
        font: &BitmapFont,
        dialogue: &DialogueArt,
        preferences: &resonance_content::menu_data::CustomizeSettings,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.set_page(Some(ActivePage::Menu(Page::Items)));
        let mut draw = self.begin_drawing(font, dialogue, Some(preferences), tick)?;
        draw.opacity = 255 - frame.fade;
        draw.shade(draw.screen);
        let selected = (!frame.rows.is_empty()).then_some(frame.selected);
        let list = InventoryList {
            rows: &frame.rows,
            selected,
            first: frame.first,
            scroll: frame.scroll,
            fade: frame.fade,
            selection_alpha: 255,
            description: frame
                .rows
                .get(frame.selected)
                .map_or(Description::None, |row| Description::Item(row.id)),
            previous_description: if frame.description_previous == 0 {
                Description::None
            } else {
                Description::Item(frame.description_previous)
            },
            description_opacity: 255 - frame.description_blend,
        };
        if let Some(anchor) = draw.inventory_list(&list, data)? {
            draw.cursor(anchor, 255);
        }
        let drawing = draw.batches;
        self.submit_drawing(drawing, commands, meshes)
    }
}

fn target_panel(menu: &Menu, opacity: u8) -> (f32, f32) {
    let count = menu.party().formation.len();
    let width = if count > 4 { 384 } else { 192 };
    let x = 620 - width;
    (
        (x + (648 - x) * u32::from(255 - opacity) / 256) as f32,
        width as f32,
    )
}

pub(super) fn target_cursor(menu: &Menu, slot: usize, visual: Visual) -> [f32; 2] {
    [
        target_panel(menu, visual.target_opacity).0 + 16. + (slot / 4) as f32 * 188.,
        110. + (slot % 4) as f32 * 65.,
    ]
}

impl Drawing<'_> {
    fn inventory_list(
        &mut self,
        list: &InventoryList<'_>,
        data: &resonance_content::menu_data::MenuData,
    ) -> Result<Option<[f32; 2]>> {
        let layout = inventory_layout(list)?;
        let opacity = 255 - list.fade;
        self.opacity = opacity;
        self.offset = [0., layout.heading_offset];
        self.heading(menu_label(&self.spec.labels, "items")?)?;
        self.offset = [layout.list_offset, 0.];
        self.framed([16., 60., 604., 266.], true)?;
        self.offset = [0., layout.description_offset];
        self.frame_detail([16., 336., 604., 92.], true, self.menu_color(), true)?;
        self.offset = [layout.list_offset, 0.];
        let counter = if list.rows.is_empty() {
            "---/---".into()
        } else {
            let row = list
                .selected
                .map_or_else(|| "-".into(), |row| (row + 1).to_string());
            format!("{row}/{}", list.rows.len())
        };
        self.text_size(&counter, [32., 64.], [16.; 2], WHITE)?;
        if list.selected.is_some() {
            self.highlight(
                [
                    layout.anchor[0] - layout.list_offset,
                    layout.anchor[1] - 8.,
                    240.,
                    24.,
                ],
                list.selection_alpha,
            );
        }
        let starts = self.vertex_counts();
        for (row, entry) in list
            .rows
            .iter()
            .skip(layout.first)
            .take(layout.visible)
            .enumerate()
        {
            let item = data
                .items
                .get(usize::from(entry.id))
                .context("inventory item was not prepared")?;
            let item_text = data.item_text(entry.id)?;
            let x = 48. + (row % 2) as f32 * 296.;
            let y = 88. + (row / 2) as f32 * 26. - layout.scroll_offset as f32;
            if item.category != 0 {
                let rect = self
                    .spec
                    .sprite(Sprite::Items, usize::from(item.category - 1))?;
                let alpha = if entry.urgent {
                    blink_opacity(self.tick, 60, 15)
                } else {
                    1.
                };
                self.sprite_rect(rect, [x, y, x + 24., y + 24.], [1., 1., 1., alpha]);
            }
            self.text(
                &item_text.name,
                [x + 24., y],
                16.,
                if entry.recent { 4 } else { WHITE },
            )?;
            if item.category != KEY_ITEM_CATEGORY {
                self.text(&format!(":{:2}", entry.count), [x + 220., y], 16., 5)?;
            }
        }
        self.clip_rows(starts, [88., 322.]);
        if list.first > 0 {
            self.scroll_arrow(SCROLL_UP, [306., 72.])?;
        }
        if list.first + VISIBLE_ITEMS < list.rows.len() {
            self.scroll_arrow(SCROLL_DOWN, [306., 314.])?;
        }
        self.offset = [0., layout.description_offset];
        if list.description != Description::None {
            self.opacity = 255 - list.description_opacity;
            self.item_description_content_data(data, list.previous_description)?;
            self.opacity = crossfade_opacity(list.description_opacity, opacity);
            self.item_description_content_data(data, list.description)?;
        }
        self.offset = [0.; 2];
        self.opacity = opacity;
        Ok(list.selected.map(|_| layout.anchor))
    }
    pub(super) fn items(&mut self, menu: &Menu, visual: Visual) -> Result<[f32; 2]> {
        let resources = menu
            .resources
            .as_ref()
            .context("inventory data was not prepared")?;
        let data = &resources.data;
        let inventory = &menu.inventory;
        let rows = menu.inventory_rows();
        let fade = u32::from(visual.fade);
        let list_offset = (fade * 628 / 256) as f32;
        let description_offset = (fade * 120 / 256) as f32;
        let list = InventoryList {
            rows: &rows,
            selected: (inventory.focus != Focus::Categories && !rows.is_empty())
                .then_some(inventory.row),
            first: inventory.first,
            scroll: 0,
            fade: visual.fade,
            selection_alpha: if matches!(inventory.focus, Focus::Target | Focus::Discard(_)) {
                127
            } else {
                255
            },
            description: menu.item_description(),
            previous_description: Description::None,
            description_opacity: 255,
        };
        let mut anchor = self
            .inventory_list(&list, data)?
            .unwrap_or([330. + inventory.category as f32 * 32. + list_offset, 68.]);
        self.offset = [list_offset, 0.];
        for index in 0..self.spec.sprites.group(Sprite::ItemTabs)?.len() {
            let rect = self.spec.sprite(Sprite::ItemTabs, index)?;
            let selected = index == inventory.category;
            let x = 330. + index as f32 * 32.;
            let y = if selected { 52. } else { 56. };
            let tint = if selected { 1. } else { 192. / 255. };
            self.sprite_rect(rect, [x, y, x + 32., y + 32.], [tint, tint, tint, 1.]);
        }
        if let Focus::Transform(bottle) = inventory.focus {
            self.item_transform_prompt(menu, bottle, list_offset, visual)?;
        }
        self.offset = [0., description_offset];
        if let Some(notice) = &inventory.notice {
            self.text(
                notice,
                [(320. - self.text_width(notice, 24.)? / 2.).floor(), 364.],
                24.,
                WHITE,
            )?;
            self.offset = [0.; 2];
            self.opacity = 255;
            return Ok(anchor);
        }
        if let Some(entry) = rows.get(inventory.row) {
            let id = entry.id;
            let item = &data.items[usize::from(id)];
            let item_text = data.item_text(id)?;
            match inventory.focus {
                Focus::Categories => {}
                Focus::Target => {
                    self.offset = [0.; 2];
                    self.item_target(menu, id, visual)?;
                    anchor = target_cursor(menu, inventory.target, visual);
                }
                Focus::Discard(yes) => {
                    let label = data
                        .label("confirm_discard")?
                        .replace("%s", &item_text.name);
                    self.text(
                        &label,
                        [320. - self.text_width(&label, 24.)? / 2., 340.],
                        24.,
                        WHITE,
                    )?;
                    for (index, key) in ["yes", "no"].into_iter().enumerate() {
                        let label = menu_label(&self.spec.labels, key)?;
                        let x = 320. - self.text_width(label, 24.)? / 2.;
                        let y = 372. + index as f32 * 28.;
                        if yes == (index == 0) {
                            self.highlight([x, y, self.text_width(label, 24.)?, 24.], 255);
                            anchor = [x, y + 8.];
                        }
                        self.text(label, [x, y], 24., WHITE)?;
                    }
                }
                _ => {
                    if inventory.focus == Focus::List && item.price != 0 {
                        self.offset = [0., -((fade * 56 / 256) as f32)];
                        let label = data.label("discard")?;
                        self.text(
                            label,
                            [624. - self.text_width(label, 24.)?, 24.],
                            24.,
                            WHITE,
                        )?;
                        self.sprite(
                            self.spec.sprite(
                                Sprite::Buttons,
                                if self.tick % 40 < 20 { 10 } else { 9 },
                            )?,
                            [600. - self.text_width(label, 24.)?, 24.],
                        );
                    }
                }
            }
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(anchor)
    }

    fn item_transform_prompt(
        &mut self,
        menu: &Menu,
        bottle: u16,
        list_offset: f32,
        visual: Visual,
    ) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let label = data.label("select_item")?;
        let width = self.text_width(label, 24.)?;
        let opacity = visual.target_opacity;
        let y = 20. - (u32::from(255 - opacity) * 28 / 256) as f32;
        let x = (320. - width / 2.).floor();
        self.offset = [0.; 2];
        self.opacity = opacity;
        self.frame([x - 4., y - 4., width + 8., 32.])?;
        self.text(label, [x, y], 24., WHITE)?;
        self.offset = [list_offset, 0.];
        let count = menu.party().items.get(&bottle).copied().unwrap_or(0);
        let mut x = 176.;
        for span in data.item_bottle_count()? {
            let MenuSpan::Text { text, color } = span else {
                anyhow::bail!("button in bottle count")
            };
            let text = text.replace("%d", &count.to_string());
            self.text(&text, [x, 60.], 16., usize::from(*color))?;
            x += self.text_width(&text, 16.)?;
        }
        self.opacity = 255 - visual.fade;
        Ok(())
    }

    pub(super) fn item_description_content(
        &mut self,
        menu: &Menu,
        description: Description,
    ) -> Result<()> {
        self.item_description_content_data(&menu.resources.as_ref().unwrap().data, description)
    }

    fn item_description_content_data(
        &mut self,
        data: &resonance_content::menu_data::MenuData,
        description: Description,
    ) -> Result<()> {
        match description {
            Description::None => Ok(()),
            Description::Item(id) => self.item_description_data(data, id),
            Description::Category(category) => {
                let label = data
                    .items_text()?
                    .inventory_categories
                    .get(category)
                    .context("unknown inventory category caption")?;
                self.text(
                    label,
                    [(306. - self.text_width(label, 24.)? / 2.).trunc(), 368.],
                    24.,
                    WHITE,
                )
            }
        }
    }

    pub(super) fn item_description(&mut self, menu: &Menu, id: u16) -> Result<()> {
        self.item_description_data(&menu.resources.as_ref().unwrap().data, id)
    }

    pub(super) fn item_description_data(
        &mut self,
        data: &resonance_content::menu_data::MenuData,
        id: u16,
    ) -> Result<()> {
        let item = data
            .items
            .get(usize::from(id))
            .context("item description was not prepared")?;
        let item_text = data.item_text(id)?;
        self.sprite_rect(
            self.spec.sprite(Sprite::ItemImages, usize::from(id))?,
            [36., 348., 100., 412.],
            [1.; 4],
        );
        self.text(&item_text.description, [120., 344.], 20., WHITE)?;
        self.text(&item_text.details, [120., 370.], 20., WHITE)?;
        let category = data
            .items_text()?
            .item_categories
            .get(usize::from(item.category))
            .context("unknown item category caption")?;
        self.text(
            category,
            [612. - self.text_width(category, 20.)?, 344.],
            20.,
            5,
        )
    }

    fn item_target(&mut self, menu: &Menu, id: u16, visual: Visual) -> Result<()> {
        let data = &menu.resources.as_ref().unwrap().data;
        let party = menu.party();
        let all = menu.inventory.target_all;
        let slot = if all {
            const GROUP_PREVIEW_TICKS: u32 = 120;
            (visual.target_age / GROUP_PREVIEW_TICKS) as usize % party.formation.len()
        } else {
            menu.inventory.target
        };
        let member_index = usize::from(party.formation[slot] - 1);
        let target = &party.members[member_index];
        let stats = target.stats(data);
        let opacity = visual.target_opacity;
        let base_opacity = self.opacity;
        self.opacity = opacity;
        self.portrait(member_index, target, [32., 350.])?;
        self.text(menu.character_name(member_index), [96., 336.], 24., WHITE)?;
        self.opacity = base_opacity;
        self.gauge(false, [248., 336.], [16., 24.], target.hp, stats.hp, Full)?;
        self.gauge(true, [432., 336.], [16., 24.], target.tp, stats.tp, Full)?;
        let definition = &menu.resources.as_ref().unwrap().session.items[usize::from(id)];
        let equipment = definition.equipment_kind.is_some();
        let preview = definition
            .equipment_kind
            .and_then(|kind| target.preferred_equipment_slot(kind))
            .filter(|_| definition.allowed_characters & (1 << member_index) != 0)
            .map(|slot| target.preview_equipment(data, slot, id));
        let values = if let Some(next) = preview {
            [
                (
                    if member_index == 0 {
                        "item_slash"
                    } else {
                        "item_attack"
                    },
                    stats.slash,
                    next.slash,
                ),
                ("item_thrust", stats.thrust, next.thrust),
                ("item_defense", stats.defense, next.defense),
                ("item_accuracy", stats.accuracy, next.accuracy),
                ("item_evasion", stats.evasion, next.evasion),
                ("item_intelligence", stats.intelligence, next.intelligence),
                ("item_luck", stats.luck, next.luck),
                ("", 0, 0),
            ]
        } else {
            [
                ("strength", stats.strength),
                (
                    if member_index == 0 {
                        "item_slash"
                    } else {
                        "item_attack"
                    },
                    stats.slash,
                ),
                ("item_thrust", stats.thrust),
                ("defense", stats.defense),
                ("luck", stats.luck),
                ("accuracy", stats.accuracy),
                ("evasion", stats.evasion),
                ("intelligence", stats.intelligence),
            ]
            .map(|(key, value)| (key, value, value))
        };
        for (index, (key, current, value)) in values
            .into_iter()
            .take(if equipment { 7 } else { 8 })
            .enumerate()
        {
            if equipment && preview.is_none()
                || index == if equipment { 1 } else { 2 } && member_index != 0
            {
                continue;
            }
            let cell = index + usize::from(equipment);
            let x = 104. + (cell % 4) as f32 * 128.;
            let y = 364. + (cell / 4) as f32 * 28.;
            self.opacity = opacity;
            self.text(
                data.label(key)?,
                [x, y],
                if equipment { 16. } else { 24. },
                GOLD,
            )?;
            let color = match value.cmp(&current) {
                std::cmp::Ordering::Greater => 4,
                std::cmp::Ordering::Less => 2,
                _ => WHITE,
            };
            self.opacity = if equipment { opacity } else { base_opacity };
            self.number(u32::from(value), [x + 112., y], [16., 24.], color)?;
        }
        if equipment && preview.is_none() {
            // Empty slots retain their category icon in the equipment summary.
            const SLOT_CATEGORIES: [usize; 6] = [20, 23, 27, 31, 35, 35];
            for (row, (slot, category)) in resonance_game::menu::equipment::SLOTS
                .into_iter()
                .zip(SLOT_CATEGORIES)
                .enumerate()
            {
                let x = 104. + (row % 3) as f32 * 162.;
                let y = 364. + (row / 3) as f32 * 28.;
                self.sprite_rect(
                    self.spec.sprite(Sprite::Items, category - 1)?,
                    [x, y, x + 24., y + 24.],
                    [1.; 4],
                );
                if target.equipment[slot] != 0 {
                    self.text(
                        &data.item_text(target.equipment[slot])?.name,
                        [x + 28., y],
                        13.,
                        WHITE,
                    )?;
                }
            }
        }
        self.opacity = opacity;
        self.plane = 2;
        let (panel_x, panel_width) = target_panel(menu, opacity);
        self.shade([panel_x, 60., panel_x + panel_width, 324.]);
        self.plane = 3;
        let label = data.label(if menu.inventory.target_equipment {
            "equip_target"
        } else {
            "select_target"
        })?;
        let spans = if all { data.item_group_prompt()? } else { &[] };
        let width = if all {
            spans
                .iter()
                .map(|span| match span {
                    MenuSpan::Text { text, .. } => self.text_width(text, 24.),
                    MenuSpan::Button { .. } => Ok(24.),
                })
                .sum::<Result<f32>>()?
        } else {
            self.text_width(label, 24.)?
        };
        let y = 20. - (u32::from(255 - opacity) * 28 / 256) as f32;
        self.frame([316. - width / 2., y - 4., width + 8., 32.])?;
        let mut x = 320. - width / 2.;
        if all {
            for span in spans {
                match span {
                    MenuSpan::Text { text, color } => {
                        self.text(text, [x, y], 24., usize::from(*color))?;
                        x += self.text_width(text, 24.)?;
                    }
                    MenuSpan::Button { sprite } => {
                        self.button(usize::from(*sprite), [x, y])?;
                        x += 24.;
                    }
                }
            }
        } else {
            self.text(label, [x, y], 24., WHITE)?;
        }
        self.frame([panel_x, 60., panel_width, 264.])?;
        for (index, &member) in party.formation.iter().enumerate() {
            let character = &party.members[usize::from(member - 1)];
            let stats = character.stats(data);
            let x = panel_x + (index / 4) as f32 * 188.;
            let y = 62. + (index % 4) as f32 * 65.;
            if index == menu.inventory.target || all {
                self.highlight([x + 16., y, 160., 64.], 255);
            }
            self.text(&(index + 1).to_string(), [x, y + 20.], 16., WHITE)?;
            self.portrait(usize::from(member - 1), character, [x + 16., y])?;
            self.item_equipment_marker(menu, usize::from(member - 1), id, [x + 56., y + 40.])?;
            self.gauge(
                false,
                [x + 80., y + 8.],
                [16., 16.],
                character.hp,
                stats.hp,
                Stacked,
            )?;
            self.gauge(
                true,
                [x + 80., y + 40.],
                [16., 16.],
                character.tp,
                stats.tp,
                Stacked,
            )?;
        }
        self.opacity = base_opacity;
        Ok(())
    }

    fn item_equipment_marker(
        &mut self,
        menu: &Menu,
        member: usize,
        item: u16,
        at: [f32; 2],
    ) -> Result<()> {
        self.equipment_marker_data(
            menu.resources.as_ref().unwrap(),
            menu.party(),
            member,
            item,
            at,
        )
    }

    pub(super) fn equipment_marker_data(
        &mut self,
        resources: &resonance_game::menu::Resources,
        party: &resonance_events::party::Party,
        member: usize,
        item: u16,
        at: [f32; 2],
    ) -> Result<()> {
        let definition = &resources.session.items[usize::from(item)];
        let Some(kind) = definition.equipment_kind else {
            return Ok(());
        };
        let character = &party.members[member];
        let (rect, duration, color) = if definition.allowed_characters & (1 << member) == 0 {
            (self.spec.sprite(Sprite::TechRanks, 1)?, 20, WHITE)
        } else if character.equipment.contains(&item) {
            (self.spec.sprite(Sprite::EquipmentMarkers, 7)?, 20, 4)
        } else if kind < 4 {
            let slot = character.preferred_equipment_slot(kind).unwrap();
            let stat = if kind == 0 { 0 } else { 2 };
            let old = &resources.data.items[usize::from(character.equipment[slot])];
            let new = &resources.data.items[usize::from(item)];
            let frame = match self.tick % 60 {
                9..15 => 0,
                15..21 => 1,
                _ => 2,
            };
            let (marker, duration) = match new.equipment_stats[stat].cmp(&old.equipment_stats[stat])
            {
                std::cmp::Ordering::Greater => (frame, 18),
                std::cmp::Ordering::Less => (frame + 3, 18),
                std::cmp::Ordering::Equal => (6, 20),
            };
            (
                self.spec.sprite(Sprite::EquipmentMarkers, marker)?,
                duration,
                WHITE,
            )
        } else {
            return Ok(());
        };
        let mut tint = rgba(self.spec.palette[color]);
        tint[3] *= blink_opacity(self.tick, 60, duration);
        self.sprite_color(rect, at, tint);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_item_visuals_follow_elapsed_time_and_current_focus() {
        let mut animation = Animation::default();
        assert!(!animation.settled(Focus::List, 100));
        assert_eq!(animation.sample(Focus::List, 100).fade, 255);
        assert!(!animation.settled(Focus::List, 107));
        assert_eq!(animation.sample(Focus::List, 108).fade, 0);
        assert!(animation.settled(Focus::List, 108));
        assert!(!animation.settled(Focus::Target, 108));
        assert_eq!(animation.sample(Focus::Target, 108).target_opacity, 0);
        assert_eq!(animation.sample(Focus::Target, 112).target_opacity, 127);
        assert!(!animation.settled(Focus::Target, 112));
        // Drawing again at the same time must not advance the slide.
        assert_eq!(animation.sample(Focus::Target, 112).target_opacity, 127);
        assert_eq!(animation.sample(Focus::Target, 116).target_opacity, 255);
        assert!(animation.settled(Focus::Target, 116));
        assert!(!animation.settled(Focus::List, 116));
        animation.sample(Focus::List, 116);
        assert!(animation.settled(Focus::List, 116));
        assert_eq!(
            animation.sample(Focus::Transform(22), 117).target_opacity,
            0
        );
        assert!(!animation.settled(Focus::Transform(22), 117));
        assert!(animation.settled(Focus::Transform(22), 125));
        assert!(!animation.settled(Focus::Transform(23), 125));
    }

    #[test]
    fn inventory_columns_fade_and_scroll() -> Result<()> {
        let rows = vec![
            ItemRow {
                id: 1,
                count: 3,
                recent: false,
                urgent: false
            };
            32
        ];
        let mut list = InventoryList {
            rows: &rows,
            selected: Some(17),
            first: 0,
            scroll: 0,
            fade: 0,
            selection_alpha: 255,
            description: Description::None,
            previous_description: Description::None,
            description_opacity: 255,
        };
        assert_eq!(inventory_layout(&list)?.anchor, [344., 304.]);
        list.selected = Some(18);
        list.first = 2;
        for scroll in [-4, -1, 1, 4] {
            list.scroll = scroll;
            let layout = inventory_layout(&list)?;
            assert_eq!(layout.anchor, [48., 304.]);
            assert_eq!(layout.visible, VISIBLE_ITEMS + 2);
            assert_eq!(layout.first, if scroll > 0 { 0 } else { 2 });
            assert!((1..26).contains(&layout.scroll_offset));
        }
        list.scroll = 0;
        list.fade = 255;
        let closed = inventory_layout(&list)?;
        assert!(closed.heading_offset < 0. && closed.description_offset > 0.);
        assert!(closed.list_offset > 600.);
        list.selected = None;
        list.rows = &[];
        list.first = 0;
        assert!(inventory_layout(&list).is_ok());
        list.selected = Some(0);
        assert!(inventory_layout(&list).is_err());
        Ok(())
    }
}
