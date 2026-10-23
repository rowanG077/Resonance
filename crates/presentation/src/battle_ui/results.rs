//! Final rewards and paged notices. Gameplay owns result timing and commits.
use super::*;
use resonance_game::battle::{
    party::Character,
    results::{ResultNotice, Results as Model},
};

const ITEMS_PER_PAGE: usize = 2;
const STAT_ADVANCE: f32 = 18.;
const PAGE_SLIDE_TICKS: u32 = 20;
const BONUS_TRANSFER_DELAY: u32 = 150;
// Large awards transfer quickly, then slow down near the final total.
const BONUS_TRANSFER_STEPS: [u32; 3] = [111, 11, 1];

struct Row {
    text: String,
    overflow: bool,
    character: Option<Character>,
}

pub(super) struct Results {
    pub(super) model: Model,
    rows: Vec<Row>,
    item_count: usize,
    chef: Option<String>,
    page: usize,
    slide_age: Option<u32>,
    slide_width: f32,
    sounds: Vec<u16>,
    age: u32,
    remaining_bonus: u32,
}
impl Results {
    fn pages(&self) -> usize {
        (self.item_count.div_ceil(ITEMS_PER_PAGE) + self.rows.len() - self.item_count).max(1)
    }
    fn page_rows(&self, page: usize) -> (&[Row], bool) {
        let item_pages = self.item_count.div_ceil(ITEMS_PER_PAGE);
        if page < item_pages {
            let first = page * ITEMS_PER_PAGE;
            (
                &self.rows[first..(first + ITEMS_PER_PAGE).min(self.item_count)],
                true,
            )
        } else {
            let first = self.item_count + page - item_pages;
            (&self.rows[first..(first + 1).min(self.rows.len())], false)
        }
    }
    fn ready(&self) -> bool {
        self.slide_age.is_none() && self.page + 1 == self.pages()
    }
    fn visible_pages(&self) -> impl Iterator<Item = (usize, f32)> {
        let travel = self.slide_age.map(|age| {
            age.min(PAGE_SLIDE_TICKS) as f32 / PAGE_SLIDE_TICKS as f32 * self.slide_width
        });
        travel
            .map(|distance| (self.page - 1, -distance))
            .into_iter()
            .chain(std::iter::once((
                self.page,
                travel.map_or(0., |distance| self.slide_width - distance),
            )))
    }
    pub(super) fn page_characters(&self) -> impl Iterator<Item = (Character, f32)> {
        self.visible_pages().filter_map(|(page, offset)| {
            self.page_rows(page)
                .0
                .first()
                .and_then(|row| row.character)
                .map(|character| (character, offset))
        })
    }
    fn experience(&self) -> u32 {
        self.model
            .rewards
            .experience
            .saturating_add(self.model.rewards.combo_experience - self.remaining_bonus)
    }
    fn advance(&mut self, elapsed: u32) {
        for _ in 0..elapsed {
            if self.age >= BONUS_TRANSFER_DELAY
                && let Some(&step) = BONUS_TRANSFER_STEPS
                    .iter()
                    .find(|&&step| step <= self.remaining_bonus)
            {
                self.remaining_bonus -= step;
            }
            self.age = self.age.saturating_add(1);
        }
        if let Some(age) = &mut self.slide_age {
            *age = age.saturating_add(elapsed);
            if *age > PAGE_SLIDE_TICKS {
                self.slide_age = None;
            }
        }
    }
    fn next_page(&mut self) {
        if self.slide_age.is_none() && self.page + 1 < self.pages() {
            self.page += 1;
            self.slide_age = Some(0);
            self.sounds.push(1);
        }
    }
}

fn result_text(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [x, y]: [f32; 2],
    colors: [[u8; 4]; 2],
) -> Result<()> {
    for (offset, gradient) in [(2., [[16, 16, 24, 220]; 2]), (0., colors)] {
        text::italic(
            batch,
            font,
            text,
            [x + offset, y + offset],
            [20., 28.],
            STAT_ADVANCE,
            12.,
            gradient,
        )
        .map_err(|_| anyhow::anyhow!("missing result glyph"))?;
    }
    Ok(())
}

impl Artwork {
    pub fn take_result_sounds(&mut self) -> Vec<u16> {
        self.results
            .as_mut()
            .map_or_else(Vec::new, |state| std::mem::take(&mut state.sounds))
    }
    pub(crate) fn results_diagnostic(&self) -> Option<serde_json::Value> {
        let state = self.results.as_ref()?;
        let rows: Vec<_> = state
            .rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "text": row.text, "overflow": row.overflow,
                    "character": row.character.map(|character| character as u8),
                })
            })
            .collect();
        Some(
            serde_json::json!({"page": state.page, "pages": state.pages(), "ready": state.ready(), "rows": rows, "chef": state.chef, "slide_age": state.slide_age, "age": state.age, "experience": state.experience(), "bonus": state.remaining_bonus}),
        )
    }
    pub fn results_on_last_page(&self) -> bool {
        self.results.as_ref().is_none_or(Results::ready)
    }
    pub fn synchronize_results(
        &mut self,
        model: &Model,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<()> {
        let prepared = self.prepare_results(model);
        if prepared.is_err() {
            self.results = None;
        }
        diagnostics.attempt("battle result display", prepared)?;
        Ok(())
    }
    fn prepare_results(&mut self, model: &Model) -> Result<()> {
        ensure!(
            model.overflow.len() == model.rewards.items.len(),
            "result item colors differ from rewards"
        );
        let mut rows = Vec::new();
        for (award, &overflow) in model.rewards.items.iter().zip(&model.overflow) {
            let name = &self.menu_data.item_text(award.item)?.name;
            rows.push(Row {
                text: if award.count == 1 {
                    name.clone()
                } else {
                    format!("{name} x{}", award.count)
                },
                overflow,
                character: None,
            });
        }
        for notice in &model.notices {
            let (character, message) = match *notice {
                ResultNotice::Level { character, level } => {
                    (character, format!("Level up: {level}"))
                }
                ResultNotice::TpRecovery { .. } => continue,
                ResultNotice::MaximumVital {
                    character,
                    vital,
                    amount,
                } => {
                    use resonance_game::battle::rewards::MaximumVital;
                    (
                        character,
                        format!(
                            "Max {} +{amount}",
                            match vital {
                                MaximumVital::Hp => "HP",
                                MaximumVital::Tp => "TP",
                            }
                        ),
                    )
                }
                ResultNotice::HappinessExperience { character, amount } => {
                    (character, format!("Bonus EXP +{amount}"))
                }
                ResultNotice::HappinessGald { character, amount } => {
                    (character, format!("Bonus Gald +{amount}"))
                }
                ResultNotice::Technique {
                    character,
                    technique,
                } => (
                    character,
                    format!(
                        "Learned {}",
                        self.menu_data.technique_text(technique.0)?.name
                    ),
                ),
                ResultNotice::Title { character, title } => (
                    character,
                    format!(
                        "Earned the title, \"{}\"",
                        self.menu_data.title_text(character as u8, title)?.name
                    ),
                ),
                ResultNotice::CompoundEx { character } => {
                    (character, "Discovered a compound EX skill".into())
                }
                ResultNotice::Cooking {
                    character,
                    recipe,
                    success,
                } => (
                    character,
                    format!(
                        "{} {}",
                        if success { "Cooked" } else { "Failed to cook" },
                        self.menu_data
                            .cooking_text()?
                            .recipe(usize::from(recipe))?
                            .name
                    ),
                ),
            };
            rows.push(Row {
                text: message,
                overflow: false,
                character: Some(character),
            });
        }
        let chef = model
            .cook_prompt
            .map(|character| model.character_names[character.index()].clone());
        for text in rows.iter().map(|row| &row.text).chain(chef.iter()) {
            self.font.validate_text(text)?;
        }
        self.font.validate_text("Cook")?;
        self.art.font.validate_text(
            "EXP BONUS MAX HIT GALD TIME GRADE ITEM(S) FOUND INFO +-.\"'0123456789",
        )?;
        let previous_notices = self
            .results
            .as_ref()
            .map_or(0, |state| state.model.notices.len());
        let sounds = model
            .notices
            .iter()
            .skip(previous_notices)
            .filter_map(|notice| match notice {
                ResultNotice::Cooking { success, .. } => Some(if *success { 6 } else { 7 }),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(state) = &mut self.results {
            state.model = model.clone();
            state.rows = rows;
            state.item_count = model.rewards.items.len();
            state.chef = chef;
            state.page = state.page.min(state.pages() - 1);
            state.sounds.extend(sounds);
        } else {
            self.results = Some(Results {
                model: model.clone(),
                rows,
                item_count: model.rewards.items.len(),
                chef,
                page: 0,
                slide_age: None,
                slide_width: self.resolution.ui_size().x,
                sounds,
                age: 0,
                remaining_bonus: model.rewards.combo_experience,
            });
        }
        Ok(())
    }
    pub fn next_result_page(&mut self) {
        if let Some(state) = &mut self.results {
            state.next_page();
        }
    }
    pub(super) fn advance_results(&mut self, elapsed: u32) {
        if let Some(state) = &mut self.results {
            state.advance(elapsed);
        }
    }
    fn draw_results(&self) -> Result<[Batch; 3]> {
        let mut batches: [Batch; 3] = Default::default();
        let Some(state) = &self.results else {
            return Ok(batches);
        };
        let model = &state.model;
        for top in [28., 64., 100.] {
            batches[0].quad(
                [0., top, 640., top + 24.],
                [0.5; 4],
                [0.28, 0.28, 0.72, 0.6],
            );
        }
        let [red, blue, heading] = self.art.result_colors;
        for (label, position, color) in [
            ("EXP", [56., 20.], red),
            ("BONUS", [56., 56.], red),
            ("MAX", [56., 92.], red),
            ("GALD", [332., 20.], blue),
            ("TIME", [332., 56.], blue),
            ("GRADE", [332., 92.], blue),
        ] {
            result_text(&mut batches[2], &self.art.font, label, position, color)?;
        }
        let ticks = model.combat_ticks;
        let grade = model.grade.unsigned_abs();
        for (value, right, top, color) in [
            (state.experience().to_string(), 290., 20., red),
            (format!("+{}", state.remaining_bonus), 290., 56., red),
            (format!("{} HIT", model.maximum_combo), 290., 92., red),
            (model.rewards.gald.to_string(), 586., 20., blue),
            (
                format!(
                    "{:02}'{:02}\"{:02}",
                    ticks / 3600,
                    ticks / 60 % 60,
                    ticks % 60 * 100 / 60
                ),
                586.,
                56.,
                blue,
            ),
            (
                format!(
                    "{}{}.{:02}",
                    if model.grade < 0 { '-' } else { '+' },
                    grade / 100,
                    grade % 100
                ),
                586.,
                92.,
                blue,
            ),
        ] {
            result_text(
                &mut batches[2],
                &self.art.font,
                &value,
                [right - value.len() as f32 * STAT_ADVANCE, top],
                color,
            )?;
        }
        for (page, offset) in state.visible_pages() {
            let (rows, items) = state.page_rows(page);
            if rows.is_empty() {
                continue;
            }
            text::glyph_gradient(
                &mut batches[2],
                &self.art.font,
                if items { "ITEM(S) FOUND" } else { "INFO" },
                [(if items { 98. } else { 108. }) + offset, 340.],
                [13., 18.],
                heading,
            )?;
            for (column, row) in rows.iter().enumerate() {
                text::fit(
                    &mut batches[1],
                    &self.font,
                    &row.text,
                    [
                        (if items {
                            108. + column as f32 * 260.
                        } else {
                            136.
                        }) + offset,
                        362.,
                        if items { 242. } else { 472. },
                        24.,
                    ],
                    if row.overflow {
                        [128, 85, 70, 255]
                    } else {
                        [128, 128, 128, 255]
                    },
                )?;
            }
        }
        if state.chef.is_some() {
            batches[2].quad([508., 418., 532., 450.], [456., 64., 476., 96.], [1.; 4]);
            text::fit(
                &mut batches[1],
                &self.font,
                "Cook",
                [536., 420., 84., 32.],
                [128, 128, 128, 255],
            )?;
        }
        Ok(batches)
    }
    pub(super) fn render_results(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let result = (|| {
            let batches = self.draw_results()?;
            if let Some(state) = &self.results {
                self.windows.render_battle_results_panel(
                    state.visible_pages().filter_map(|(page, offset)| {
                        (!state.page_rows(page).0.is_empty()).then_some(offset)
                    }),
                    &self.font,
                    &self.dialogue,
                    &self.settings,
                    commands,
                    meshes,
                )?;
            }
            for (layer, batch) in self.ui.results.iter_mut().zip(batches) {
                layer.upload(batch, commands, meshes)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.results = None;
            self.windows.clear_page(commands);
            for layer in &mut self.ui.results {
                layer.show(false, commands);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_pages_slide_together_and_cannot_skip_unread_notices() {
        use resonance_game::battle::rewards::{ItemAward, Rewards};
        let mut state = Results {
            model: Model {
                rewards: Rewards {
                    experience: 0,
                    combo_experience: 0,
                    gald: 0,
                    items: vec![ItemAward {
                        item: 1,
                        count: 1,
                        discoveries: Vec::new(),
                    }],
                },
                grade: 0,
                maximum_combo: 0,
                combat_ticks: 0,
                character_names: Default::default(),
                notices: vec![ResultNotice::Cooking {
                    character: Character::Lloyd,
                    recipe: 0,
                    success: true,
                }],
                cook_prompt: None,
                overflow: vec![false],
            },
            rows: vec![
                Row {
                    text: "Apple Gel".into(),
                    overflow: false,
                    character: None,
                },
                Row {
                    text: "Cooked Sandwich".into(),
                    overflow: false,
                    character: Some(Character::Lloyd),
                },
            ],
            item_count: 1,
            chef: None,
            page: 0,
            slide_age: None,
            slide_width: 640.,
            sounds: Vec::new(),
            age: 0,
            remaining_bonus: 0,
        };
        state.model.rewards.experience = 100;
        state.model.rewards.combo_experience = 123;
        state.remaining_bonus = 123;
        state.advance(150);
        assert_eq!((state.experience(), state.remaining_bonus), (100, 123));
        state.advance(0);
        assert_eq!((state.experience(), state.remaining_bonus), (100, 123));
        for expected in [(211, 12), (222, 1), (223, 0), (223, 0)] {
            state.advance(1);
            assert_eq!((state.experience(), state.remaining_bonus), expected);
        }
        assert_eq!(
            (
                state.model.rewards.experience,
                state.model.rewards.combo_experience
            ),
            (100, 123)
        );
        for width in [640., 480. * 16. / 9.] {
            state.page = 0;
            state.slide_width = width;
            state.sounds.clear();
            state.next_page();
            assert!(!state.ready());
            assert_eq!(
                state.visible_pages().collect::<Vec<_>>(),
                [(0, 0.), (1, width)]
            );
            state.next_page();
            state.advance(0);
            assert_eq!(state.slide_age, Some(0));
            assert_eq!(state.sounds, [1]);
            state.advance(10);
            assert_eq!(
                state.visible_pages().collect::<Vec<_>>(),
                [(0, -width / 2.), (1, width / 2.)]
            );
            assert_eq!(
                state.page_characters().collect::<Vec<_>>(),
                [(Character::Lloyd, width / 2.)]
            );
            state.next_page();
            state.advance(10);
            assert!(!state.ready());
            assert_eq!(
                state.visible_pages().collect::<Vec<_>>(),
                [(0, -width), (1, 0.)]
            );
            // Both pages start/end outside the complete viewport, including sidebands.
            assert!(96. + width > 320. + width / 2.);
            assert!(616. - width < 320. - width / 2.);
            state.advance(1);
            assert!(state.ready());
            assert_eq!(state.visible_pages().collect::<Vec<_>>(), [(1, 0.)]);
            state.next_page();
            assert_eq!(state.sounds, [1]);
            assert_eq!(state.model.rewards.items[0].count, 1);
        }
    }

    #[test]
    #[ignore = "requires current font and item/technique catalogues; CPU drawing only"]
    fn result_pages_fit_literal_names_and_cooking_and_failed_displays_can_finish() -> Result<()> {
        use resonance_content::diagnostics::Diagnostics;
        use resonance_game::battle::rewards::{ItemAward, MaximumVital, Rewards};
        let mut model = Model {
            rewards: Rewards {
                experience: 100,
                combo_experience: 30,
                gald: 50,
                items: vec![ItemAward {
                    item: 1,
                    count: 2,
                    discoveries: Vec::new(),
                }],
            },
            grade: -25,
            maximum_combo: 2,
            combat_ticks: 120,
            character_names: std::array::from_fn(|_| "R%s".into()),
            notices: vec![
                ResultNotice::Technique {
                    character: Character::Regal,
                    technique: resonance_battle::TechniqueId(201),
                },
                ResultNotice::MaximumVital {
                    character: Character::Lloyd,
                    vital: MaximumVital::Hp,
                    amount: 5,
                },
                ResultNotice::HappinessExperience {
                    character: Character::Genis,
                    amount: 100,
                },
                ResultNotice::HappinessGald {
                    character: Character::Raine,
                    amount: 200,
                },
                ResultNotice::CompoundEx {
                    character: Character::Kratos,
                },
                ResultNotice::Level {
                    character: Character::Lloyd,
                    level: 29,
                },
                ResultNotice::TpRecovery {
                    character: Character::Lloyd,
                    amount: 1,
                },
                ResultNotice::MaximumVital {
                    character: Character::Lloyd,
                    vital: MaximumVital::Tp,
                    amount: 5,
                },
            ],
            cook_prompt: Some(Character::Regal),
            overflow: vec![true],
        };
        let (mut hud, materials, _app) = test_artwork()?;
        let diagnostics = Diagnostics::new(true);
        hud.synchronize_results(&model, &diagnostics)?;
        assert_eq!(hud.results.as_ref().unwrap().rows[1].text, "Learned Mirage");
        assert!(hud.results.as_ref().unwrap().rows[0].overflow);
        assert!(!hud.results_on_last_page());
        let pages = hud.results.as_ref().unwrap().pages();
        for _ in 1..pages {
            hud.next_result_page();
            hud.advance_results(PAGE_SLIDE_TICKS + 1);
        }
        assert!(hud.results_on_last_page());
        assert_eq!(hud.take_result_sounds(), vec![1; pages - 1]);
        model.notices.push(ResultNotice::Cooking {
            character: Character::Regal,
            recipe: 0,
            success: false,
        });
        model.cook_prompt = None;
        hud.synchronize_results(&model, &diagnostics)?;
        assert!(!hud.results_on_last_page());
        assert_eq!(hud.take_result_sounds(), [7]);
        hud.synchronize_results(&model, &diagnostics)?;
        assert!(hud.take_result_sounds().is_empty());
        hud.next_result_page();
        assert!(!hud.results_on_last_page());
        hud.advance_results(PAGE_SLIDE_TICKS + 1);
        assert!(hud.results_on_last_page());
        assert!(
            hud.results
                .as_ref()
                .unwrap()
                .rows
                .last()
                .unwrap()
                .text
                .starts_with("Failed to cook ")
        );
        model.maximum_combo = u16::MAX;
        hud.synchronize_results(&model, &diagnostics)?;
        for page in 0..hud.results.as_ref().unwrap().pages() {
            hud.results.as_mut().unwrap().page = page;
            let draw = hud.draw_results()?;
            assert!(draw.iter().all(|b| !b.indices.is_empty()));
            assert!(
                draw.iter()
                    .flat_map(|b| &b.positions)
                    .all(|p| (-320. ..=320.).contains(&p[0]) && (-240. ..=240.).contains(&p[1]))
            );
        }
        let mut world = World::new();
        world.insert_resource(materials);
        let mut meshes = Assets::<Mesh>::default();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        hud.prepare(
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
            crate::Resolution::default(),
        );
        queue.apply(&mut world);
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            model.cook_prompt = Some(Character::Regal);
            let chef = std::mem::replace(
                &mut model.character_names[Character::Regal.index()],
                "\u{10ffff}".into(),
            );
            assert_eq!(
                hud.synchronize_results(&model, &diagnostics).is_err(),
                paranoid
            );
            assert!(
                diagnostics.has_errors() && hud.results.is_none() && hud.results_on_last_page()
            );
            model.character_names[Character::Regal.index()] = chef;
            hud.synchronize_results(&model, &Diagnostics::new(true))?;
            hud.render_results(&mut Commands::new(&mut queue, &world), &mut meshes)?;
            queue.apply(&mut world);
            let glyph = hud.font.glyphs.remove(&'C').unwrap();
            let rendered = hud.render_results(&mut Commands::new(&mut queue, &world), &mut meshes);
            queue.apply(&mut world);
            assert_eq!(
                diagnostics.attempt("battle HUD", rendered).is_err(),
                paranoid
            );
            assert!(hud.results.is_none() && hud.results_on_last_page());
            assert!(hud.ui.results.iter().all(|layer| {
                world.get::<Visibility>(layer.rendered.as_ref().unwrap().entity)
                    == Some(&Visibility::Hidden)
            }));
            hud.font.glyphs.insert('C', glyph);
        }
        Ok(())
    }
}
