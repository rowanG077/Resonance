//! 57718 initialization, 564D4 numeric visits and 54BDC card progression.
use super::*;

pub(super) struct Results {
    pub age: u32,
    pub positions: [[i16; 2]; 6],
    pub strips: [[i16; 4]; 6],
    pub opacity: [u8; 6],
    pub enabled: [bool; 6],
    pub experience: i32,
    pub bonus: i32,
    pub grade: i16,
    pub items: Vec<(String, bool)>,
    pub notices: Vec<(u8, String)>,
    pub item_window: [i16; 4],
    pub notice_windows: [[i16; 4]; 2],
    pub selector: usize,
    pub notice: usize,
    pub ready: bool,
    arrived: bool,
    mode: u8,
    counter: i32,
    last_age: Option<u32>,
    sounds: Vec<u16>,
}
impl Results {
    pub fn new(
        art: &Art,
        experience: i32,
        bonus: i32,
        combo: u16,
        gald: u32,
        items: Vec<(String, bool)>,
        notices: Vec<(u8, String)>,
    ) -> Self {
        let rows = (items.len() as i16 - 1) >> 1;
        Self {
            age: 0,
            positions: art.results.positions,
            strips: art.results.strips,
            opacity: [0; 6],
            enabled: [
                experience != 0,
                bonus != 0,
                combo >= 2,
                gald != 0,
                true,
                true,
            ],
            experience,
            bonus,
            grade: 0,
            item_window: [
                656,
                352 - rows * 26,
                if items.len() < 2 { 260 } else { 520 },
                rows * 26 + 36,
            ],
            items,
            notices,
            notice_windows: [[-640, 352, 520, 36], [736, 352, 520, 36]],
            selector: 0,
            notice: 0,
            ready: false,
            arrived: false,
            mode: 0,
            counter: 0,
            last_age: None,
            sounds: Vec::new(),
        }
    }
    /// The game supplies each result callback visit. Rendering never advances it.
    pub fn update(
        &mut self,
        age: u32,
        confirm: bool,
        target_exp: i32,
        target_grade: i16,
    ) -> Result<()> {
        ensure!(
            self.last_age.is_none_or(|last| age == last + 1),
            "result HUD callbacks must be consecutive"
        );
        self.last_age = Some(age);
        self.age = age;
        if age > 20 && age < 140 {
            for row in 0..3 {
                if age > 20 + row as u32 * 15 {
                    self.strips[row][0] = (self.strips[row][0] + 16).min(896);
                    self.strips[row][2] = (self.strips[row][2] - 16).max(-256);
                    self.opacity[row] = self.opacity[row].saturating_add(16);
                }
            }
        }
        if age > 40 && age < 160 {
            for row in 0..3 {
                if age > 78 + row as u32 * 6 {
                    self.positions[row][0] = (self.positions[row][0] - 12).max(56);
                }
                if age > 70 + row as u32 * 15 {
                    self.strips[row + 3][0] = (self.strips[row + 3][0] + 28).min(896);
                    self.strips[row + 3][2] = (self.strips[row + 3][2] - 28).max(-256);
                }
                if age > 85 + row as u32 * 15 {
                    self.opacity[row + 3] = self.opacity[row + 3].saturating_add(16);
                }
            }
        }
        // 56BD8: confirmation acts on the preceding visit's arrived bit.
        if confirm && self.arrived && self.notice + 1 < self.notices.len() {
            self.sounds.extend([1, 38]);
            self.arrived = false;
            self.notice_windows[self.selector] = [736, 352, 520, 36];
            self.selector ^= 1;
            self.notice += 1;
            self.counter = 60;
            self.mode = 2;
        }
        if age >= 150 {
            // Preserve the original signed-16 stores and its asymmetric +1
            // branch (56E50..56EF4); this is not a generic tween.
            if i32::from(self.grade) - 10 < i32::from(target_grade) {
                self.grade = self.grade.wrapping_add(10);
            }
            if i32::from(self.grade) + 10 > i32::from(target_grade) {
                self.grade = self.grade.wrapping_sub(10);
            }
            if self.grade < target_grade {
                self.grade = self.grade.wrapping_add(1);
            }
            if self.grade > target_grade {
                self.grade = self.grade.wrapping_add(1);
            }
        }
        self.cards(age);
        if age >= 150 {
            if self.bonus > 0 {
                self.bonus -= 1;
                if self.bonus / 10 != 0 {
                    self.bonus -= 10;
                }
                if self.bonus / 100 != 0 {
                    self.bonus -= 100;
                }
            }
            if self.experience < target_exp {
                self.experience += 1;
                if (target_exp - self.experience) / 10 != 0 {
                    self.experience += 10;
                }
                if (target_exp - self.experience) / 100 != 0 {
                    self.experience += 100;
                }
            }
        }
        Ok(())
    }
    fn cards(&mut self, age: u32) {
        if age <= 60 {
            return;
        }
        match self.mode {
            0 => {
                if !self.items.is_empty() {
                    self.item_window[0] -= 32;
                    let target = 96 + if self.items.len() < 2 { 260 } else { 0 };
                    if self.item_window[0] >= target {
                        return;
                    }
                    self.item_window[0] = target;
                    self.mode = 1;
                    self.counter = -30;
                } else {
                    self.mode = 2;
                }
                if self.notices.is_empty() {
                    self.ready = true;
                }
            }
            1 => {
                if self.counter >= 60 {
                    self.mode = 2;
                } else {
                    self.counter += 1;
                }
            }
            2 => {
                if self.notice >= self.notices.len() {
                    return;
                }
                if self.counter >= 60 {
                    self.counter = 0;
                    self.mode = 3;
                } else {
                    self.counter += 1;
                }
            }
            3 => {
                if !self.items.is_empty() {
                    self.item_window[0] = (self.item_window[0] - 32).max(-640);
                }
                self.notice_windows[self.selector][0] =
                    (self.notice_windows[self.selector][0] - 32).max(-640);
                let incoming = &mut self.notice_windows[self.selector ^ 1][0];
                *incoming -= 32;
                if *incoming >= 96 {
                    return;
                }
                *incoming = 96;
                self.arrived = true;
                self.ready = self.notice + 1 == self.notices.len();
            }
            _ => {}
        }
    }
    pub fn windows(&self) -> Vec<[i16; 4]> {
        let mut out = Vec::new();
        if !self.items.is_empty() {
            out.push(self.item_window);
        }
        for i in 0..2 {
            let slot = self.notice as isize + i as isize - 1;
            if slot >= 0 && (slot as usize) < self.notices.len() {
                out.push(self.notice_windows[i]);
            }
        }
        out
    }
}

impl Artwork {
    pub fn take_result_sounds(&mut self) -> Vec<u16> {
        self.results
            .as_mut()
            .map_or_else(Vec::new, |state| std::mem::take(&mut state.sounds))
    }
    pub fn results_ready(&self) -> bool {
        self.results.as_ref().is_some_and(|state| state.ready)
    }
    pub fn update_results(
        &mut self,
        age: u32,
        model: &resonance_game::battle::results::Results,
        confirm: bool,
    ) -> Result<()> {
        use resonance_game::battle::results::ResultNotice;
        if self.results.is_none() {
            ensure!(
                model.overflow.len() == model.rewards.items.len(),
                "result item colors differ from rewards"
            );
            let items = model
                .rewards
                .items
                .iter()
                .zip(&model.overflow)
                .map(|(award, &overflow)| {
                    let name = &self
                        .menu_data
                        .items
                        .get(usize::from(award.item))
                        .context("unknown result item")?
                        .name;
                    Ok((
                        if award.count > 1 {
                            format!("{name}  {}", award.count)
                        } else {
                            name.clone()
                        },
                        overflow,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let notices = model
                .notices
                .iter()
                .map(|notice| {
                    let (character, format, args, actor_name) = match *notice {
                        ResultNotice::Technique {
                            character,
                            technique,
                        } => (
                            character,
                            &self.art.results.notice_formats[0],
                            vec![
                                self.menu_data
                                    .techniques
                                    .get(usize::from(technique))
                                    .context("unknown result technique")?
                                    .name
                                    .as_str(),
                            ],
                            true,
                        ),
                        ResultNotice::Title { character, title } => (
                            character,
                            &self.art.results.notice_formats[1],
                            vec![
                                self.menu_data
                                    .titles
                                    .get(usize::from(
                                        character
                                            .checked_sub(1)
                                            .context("invalid title character")?,
                                    ))
                                    .and_then(|titles| {
                                        title
                                            .checked_sub(1)
                                            .and_then(|title| titles.get(usize::from(title)))
                                    })
                                    .context("unknown result title")?
                                    .name
                                    .as_str(),
                            ],
                            false,
                        ),
                        ResultNotice::CompoundEx { character } => {
                            (character, &self.art.results.notice_formats[2], vec![], true)
                        }
                    };
                    let name = model
                        .character_names
                        .get(usize::from(
                            character
                                .checked_sub(1)
                                .context("invalid result character")?,
                        ))
                        .context("invalid result character")?;
                    let mut text = if actor_name {
                        format.replacen("%s", name, 1)
                    } else {
                        format.clone()
                    };
                    for value in args {
                        text = text.replacen("%s", value, 1);
                    }
                    Ok((character, text))
                })
                .collect::<Result<Vec<_>>>()?;
            self.results = Some(Results::new(
                &self.art,
                model.rewards.experience as i32,
                model.rewards.combo_experience as i32,
                model.maximum_combo,
                model.rewards.gald,
                items,
                notices,
            ));
        }
        self.results.as_mut().unwrap().update(
            age,
            confirm,
            model
                .rewards
                .experience
                .wrapping_add(model.rewards.combo_experience) as i32,
            model.grade as i16,
        )?;
        self.result_values = Some(model.clone());
        Ok(())
    }
    pub(super) fn render_results(
        &mut self,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let Some(state) = &self.results else {
            return Ok(());
        };
        let model = self
            .result_values
            .as_ref()
            .context("missing result values")?;
        self.windows.render_windows(
            &state.windows(),
            self.art.results.window_y_scale,
            &self.font,
            &self.dialogue,
            &self.settings,
            commands,
            meshes,
        )?;
        let mut batches: Vec<_> = (0..TRANSITION).map(|_| Batch::default()).collect();
        for &[x, y, right, _] in &state.strips[..3] {
            let [x, y, right] = [x, y + 18, right].map(f32::from);
            if x != right {
                ribbon(
                    &mut batches[RESULT_SOLID],
                    [x, y, right, y + 20.],
                    self.art.results.strip_color,
                );
            }
        }
        let frames = model.combat_ticks;
        let hundredths = (frames % 60) * 100 / 60;
        let minutes = frames / 3600 % 100;
        let seconds = frames / 60 % 60;
        let grade = i32::from(state.grade).abs();
        let lines = [
            format!("EXP   {:7}", state.experience),
            format!("BONUS +{:6}", state.bonus),
            format!("MAX  {:4} HIT", model.maximum_combo),
            format!("GALD  {:8}", model.rewards.gald),
            format!("TIME  {minutes:02}'{seconds:02}\"{hundredths:02}"),
            format!(
                "GRADE   {}{:2}.{}{}",
                if state.grade < 0 { '-' } else { '+' },
                grade / 100,
                grade / 10 % 10,
                grade % 10
            ),
        ];
        for (row, line) in lines.iter().enumerate() {
            if !state.enabled[row] {
                continue;
            }
            let [x, y] = state.positions[row].map(f32::from);
            let alpha = state.opacity[row];
            let shadow: [[u8; 4]; 2] = self.art.results.shadows[..2].try_into().unwrap();
            let shadow = shadow.map(|mut color: [u8; 4]| {
                color[3] = (alpha >> 1) + 32;
                color
            });
            glyphs(
                &mut batches[RESULT_FONT],
                &self.art,
                line,
                [x, y + 10.],
                [20., 16.],
                40.,
                18.,
                shadow,
                Some((tick, alpha >> 1)),
            )?;
            let colors = self.art.results.colors[row].map(|mut color| {
                color[3] = alpha;
                color
            });
            glyphs(
                &mut batches[RESULT_FONT],
                &self.art,
                line,
                [x, y],
                [20., 28.],
                14.,
                18.,
                colors,
                Some((tick, alpha >> 1)),
            )?;
            if row >= 3 || state.age >= 20 + row as u32 * 15 {
                let [left, y, right, _] = state.strips[row].map(f32::from);
                for x in [left, right] {
                    quad(
                        &mut batches[RESULT_BLOCK],
                        [x - 224., y, x, y + 40.],
                        [0.5; 4],
                        0.,
                        self.art.results.item_colors[..4].try_into().unwrap(),
                    );
                    // Native 55AC8/55BAC holds the second palette pointer fixed.
                    quad(
                        &mut batches[RESULT_BLOCK],
                        [x, y, x + 224., y + 40.],
                        [0.5; 4],
                        0.,
                        self.art.results.item_colors[1..].try_into().unwrap(),
                    );
                }
            }
        }
        if !state.items.is_empty() {
            let [x, y, _, _] = state.item_window.map(f32::from);
            heading(
                &mut batches[CARD_FONT],
                &self.art,
                &self.art.results.headings[0],
                [x, y],
                12.,
                tick,
            )?;
            for (index, (text, overflow)) in state.items.iter().enumerate() {
                dol_text(
                    &mut batches[DOL_FONT],
                    &self.font,
                    text,
                    [
                        x + 8. + (index & 1) as f32 * 264.,
                        y + 10. + (index >> 1) as f32 * 26.,
                    ],
                    [18., 24.],
                    18.,
                    1.,
                    if *overflow {
                        [128, 80, 80, 255]
                    } else {
                        [128, 128, 128, 255]
                    },
                )?;
            }
        }
        for info in 0..2 {
            let index = state.notice as isize + info as isize - 1;
            if index < 0 {
                continue;
            }
            let Some((character, text)) = state.notices.get(index as usize) else {
                continue;
            };
            let [x, y, _, _] = state.notice_windows[info ^ state.selector].map(f32::from);
            let icon = &self.art.results.character_icons[usize::from(*character - 1)];
            let [u, v, w, h] = icon.rect.map(|v| v as f32);
            quad(
                &mut batches[ICONS + usize::from(*character - 1)],
                [x + 8., y + 5., x + 8. + w, y + 5. + h],
                [u, v, u + w, v + h],
                0.,
                [[128, 128, 128, 255]; 4],
            );
            dol_text(
                &mut batches[DOL_FONT],
                &self.font,
                text,
                [x + 8. + w, y + 14.],
                [16., 20.],
                16.,
                0.,
                [128, 128, 128, 255],
            )?;
            heading(
                &mut batches[CARD_FONT],
                &self.art,
                &self.art.results.headings[3],
                [x, y],
                14.,
                tick,
            )?;
        }
        if state.notice + 1 < state.notices.len() {
            let [x, y, _, _] = state.notice_windows[state.selector ^ 1].map(f32::from);
            let sprite = &self.art.results.next_button[((tick / 10 * 16) & 32) as usize / 32];
            let [u, v, w, h] = sprite.rect.map(|v| v as f32);
            quad(
                &mut batches[NEXT],
                [x + 480., y + 12., x + 504., y + 36.],
                [u, v, u + w, v + h],
                0.,
                [[128, 128, 128, 255]; 4],
            );
        }
        for (index, batch) in batches
            .iter_mut()
            .enumerate()
            .take(TRANSITION)
            .skip(RESULT_SOLID)
        {
            let mut batch = std::mem::take(batch);
            if matches!(index, RESULT_FONT | CARD_FONT) && batch.secondary_uv.is_empty() {
                batch.secondary_uv = vec![[-1.; 2]; batch.positions.len()];
            }
            let visible = !batch.indices.is_empty();
            self.layers[index].update_mesh(batch, self.sizes[index], meshes)?;
            self.layers[index].show(visible, commands);
        }
        Ok(())
    }
}

fn heading(
    batch: &mut Batch,
    art: &Art,
    text: &str,
    [x, y]: [f32; 2],
    advance: f32,
    tick: u32,
) -> Result<()> {
    glyphs(
        batch,
        art,
        text,
        [x + 10., y - 8.],
        [16., 16.],
        0.,
        advance,
        [art.results.shadows[2]; 2],
        None,
    )?;
    glyphs(
        batch,
        art,
        text,
        [x + 8., y - 10.],
        [16., 16.],
        0.,
        advance,
        art.results.heading_colors,
        Some((tick, 128)),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn glyphs(
    batch: &mut Batch,
    art: &Art,
    text: &str,
    [mut x, y]: [f32; 2],
    [width, height]: [f32; 2],
    skew: f32,
    advance: f32,
    colors: [[u8; 4]; 2],
    overlay: Option<(u32, u8)>,
) -> Result<()> {
    for character in text.chars() {
        if character != ' ' {
            let glyph = art
                .font
                .glyphs
                .get(&character)
                .context("missing battle result glyph")?;
            let [mut u, mut v, w, h] = glyph.rect.map(|v| v as f32);
            if overlay.is_some() && ('!'..='/').contains(&character) {
                [u, v] = art.overlay_punctuation[character as usize - '!' as usize].map(f32::from);
            }
            let rect = [x, y, x + width, y + height];
            let uv = [u, v, u + w, v + h];
            glyph_quad(batch, rect, uv, skew, colors, overlay);
        }
        x += advance;
    }
    Ok(())
}

fn glyph_quad(
    batch: &mut Batch,
    rect: [f32; 4],
    uv: [f32; 4],
    skew: f32,
    colors: [[u8; 4]; 2],
    overlay: Option<(u32, u8)>,
) {
    quad(
        batch,
        rect,
        uv,
        skew,
        [colors[0], colors[0], colors[1], colors[1]],
    );
    if let Some((tick, alpha)) = overlay {
        // Interleave passes per glyph: italic glyphs overlap their neighbours.
        quad(batch, rect, uv, skew, [[128, 128, 128, alpha]; 4]);
        batch.secondary_uv.resize(batch.positions.len(), [-1.; 2]);
        let start = batch.secondary_uv.len() - 4;
        let top = (tick & 31) as f32 / 512.;
        let bottom = top + 16. / 512.;
        batch.secondary_uv[start..].copy_from_slice(&[
            [384. / 512., top],
            [416. / 512., top],
            [416. / 512., bottom],
            [384. / 512., bottom],
        ]);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dol_text(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [mut x, y]: [f32; 2],
    [width, height]: [f32; 2],
    advance: f32,
    skew: f32,
    color: [u8; 4],
) -> Result<()> {
    for ch in text.chars() {
        let glyph = font
            .glyphs
            .get(&ch)
            .context("missing result item/name glyph")?;
        let [u, v, w, h] = glyph.rect.map(|v| v as f32);
        quad(
            batch,
            [x, y, x + width, y + height],
            [u, v, u + w, v + h],
            skew,
            [color; 4],
        );
        x += (glyph.advance as f32 / 24. * advance).trunc();
    }
    Ok(())
}

fn ribbon(batch: &mut Batch, [x, y, right, bottom]: [f32; 4], color: [u8; 4]) {
    let mut clear = color;
    clear[3] = 0;
    for (rect, colors) in [
        ([x, y, right, bottom], [color; 4]),
        ([x, y - 4., right, y], [clear, clear, color, color]),
        (
            [x, bottom, right, bottom + 4.],
            [color, color, clear, clear],
        ),
        ([x - 4., y, x, bottom], [clear, color, clear, color]),
        ([right, y, right + 4., bottom], [color, clear, color, clear]),
    ] {
        quad(batch, rect, [0.5; 4], 0., colors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn controller(items: usize, notices: usize) -> Results {
        Results {
            age: 0,
            positions: [[0; 2]; 6],
            strips: [[0; 4]; 6],
            opacity: [0; 6],
            enabled: [true; 6],
            experience: 0,
            bonus: 0,
            grade: 0,
            items: vec![(String::new(), false); items],
            notices: vec![(1, String::new()); notices],
            item_window: [656, 352, 520, 36],
            notice_windows: [[-640, 352, 520, 36], [736, 352, 520, 36]],
            selector: 0,
            notice: 0,
            ready: false,
            arrived: false,
            mode: 0,
            counter: 0,
            last_age: None,
            sounds: Vec::new(),
        }
    }
    #[test]
    fn layered_glyph_keeps_ordered_overlapping_passes_and_secondary_samples() {
        let mut batch = Batch::default();
        for x in [56., 74.] {
            glyph_quad(
                &mut batch,
                [x, 20., x + 20., 48.],
                [0., 0., 16., 23.],
                14.,
                [[128, 80, 40, 200]; 2],
                Some((31, 100)),
            );
        }
        assert_eq!(batch.positions.len(), 16);
        assert_eq!(batch.positions[..4], batch.positions[4..8]);
        assert_eq!(batch.positions[8..12], batch.positions[12..]);
        assert!(
            batch.secondary_uv[..4]
                .iter()
                .chain(&batch.secondary_uv[8..12])
                .all(|uv| *uv == [-1.; 2])
        );
        assert_eq!(batch.secondary_uv[4], [0.75, 31. / 512.]);
        assert_eq!(batch.secondary_uv[6], [416. / 512., 47. / 512.]);
        assert_eq!(batch.colors[4][3], 100. / 255.);
        assert_eq!(batch.indices[6..12], [4, 7, 5, 5, 7, 6]);
        assert!(
            batch
                .mesh([512, 512])
                .attribute(Mesh::ATTRIBUTE_UV_1)
                .is_some()
        );
    }

    #[test]
    fn readiness_requires_final_card_arrival_and_confirm_advances_cards() {
        let mut state = controller(0, 2);
        for age in 0..300 {
            state.update(age, false, 0, 0).unwrap();
        }
        assert!(state.arrived);
        assert!(!state.ready);
        assert_eq!(state.notice, 0);
        state.update(300, true, 0, 0).unwrap();
        assert_eq!(state.notice, 1);
        assert_eq!(state.sounds, [1, 38]);
        assert!(!state.ready);
        for age in 301..322 {
            state.update(age, false, 0, 0).unwrap();
        }
        assert!(state.ready);
        assert_eq!(state.notice_windows[state.selector ^ 1][0], 96);
    }
    #[test]
    fn item_ready_uses_strict_crossing_and_no_items_visits_61() {
        let mut empty = controller(0, 0);
        empty.cards(60);
        assert!(!empty.ready);
        empty.cards(61);
        assert!(empty.ready);
        let mut items = controller(2, 0);
        for age in 61..78 {
            items.cards(age);
        }
        assert_eq!(items.item_window[0], 112);
        assert!(!items.ready);
        items.cards(78);
        assert!(items.ready);
        assert_eq!(items.item_window[0], 96);
    }
}
