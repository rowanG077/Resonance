//! Final rewards and paged notices. Gameplay owns result timing and commits.
use super::*;
use resonance_game::battle::results::{ResultNotice, Results as Model};

const ITEMS_PER_PAGE: usize = 2;
const STAT_ADVANCE: f32 = 18.;

#[derive(serde::Serialize)]
struct Row {
    text: String,
    overflow: bool,
    character: Option<u8>,
}

pub(super) struct Results {
    pub(super) model: Model,
    rows: Vec<Row>,
    item_count: usize,
    chef: Option<String>,
    page: usize,
    sounds: Vec<u16>,
}
impl Results {
    fn pages(&self) -> usize {
        (self.item_count.div_ceil(ITEMS_PER_PAGE) + self.rows.len() - self.item_count).max(1)
    }
    fn page_rows(&self) -> (&[Row], bool) {
        let item_pages = self.item_count.div_ceil(ITEMS_PER_PAGE);
        if self.page < item_pages {
            let first = self.page * ITEMS_PER_PAGE;
            (
                &self.rows[first..(first + ITEMS_PER_PAGE).min(self.item_count)],
                true,
            )
        } else {
            let first = self.item_count + self.page - item_pages;
            (&self.rows[first..(first + 1).min(self.rows.len())], false)
        }
    }
    fn ready(&self) -> bool {
        self.page + 1 == self.pages()
    }
    pub(super) fn page_character(&self) -> Option<u8> {
        self.page_rows().0.first().and_then(|row| row.character)
    }
}

fn character_name(model: &Model, character: u8) -> Result<&str> {
    model
        .character_names
        .get(usize::from(
            character
                .checked_sub(1)
                .context("invalid result character")?,
        ))
        .map(String::as_str)
        .context("invalid result character")
}

fn result_text(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [x, y]: [f32; 2],
    colors: [[u8; 4]; 2],
) -> Result<()> {
    for (offset, gradient) in [
        (2., [[16, 16, 24, 220]; 4]),
        (0., [colors[0], colors[0], colors[1], colors[1]]),
    ] {
        for (column, character) in text.chars().enumerate() {
            let glyph = font
                .glyphs
                .get(&character)
                .context("missing result glyph")?;
            let [u, v, w, h] = glyph.rect.map(|value| value as f32);
            let left = x + column as f32 * STAT_ADVANCE + offset;
            let top = y + offset;
            party::slanted_quad(
                batch,
                [left, top, left + 20., top + 28.],
                [u, v, u + w, v + h],
                12.,
                gradient,
            );
        }
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
        Some(
            serde_json::json!({"page": state.page, "pages": state.pages(), "ready": state.ready(), "rows": state.rows, "chef": state.chef}),
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
                    format!("Learned {}", self.menu_data.technique_text(technique)?.name),
                ),
                ResultNotice::Title { character, title } => (
                    character,
                    format!(
                        "Earned the title, \"{}\"",
                        self.menu_data.title_text(character, title)?.name
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
            character_name(model, character)?;
            rows.push(Row {
                text: message,
                overflow: false,
                character: Some(character),
            });
        }
        let chef = model
            .cook_prompt
            .map(|character| character_name(model, character).map(str::to_owned))
            .transpose()?;
        for text in rows.iter().map(|row| &row.text).chain(chef.iter()) {
            self.font.validate_text(text)?;
        }
        self.font.validate_text("Cook")?;
        self.art
            .font
            .validate_text("EXP GALD TIME GRADE ITEM(S) FOUND INFO +-.\"'0123456789")?;
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
                sounds,
            });
        }
        Ok(())
    }
    pub fn next_result_page(&mut self) {
        if let Some(state) = &mut self.results
            && !state.ready()
        {
            state.page += 1;
            state.sounds.push(1);
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
            ("GALD", [332., 20.], blue),
            ("TIME", [332., 56.], blue),
            ("GRADE", [332., 92.], blue),
        ] {
            result_text(&mut batches[2], &self.art.font, label, position, color)?;
        }
        let ticks = model.combat_ticks;
        let grade = model.grade.unsigned_abs();
        for (value, right, top, color) in [
            (
                model
                    .rewards
                    .experience
                    .saturating_add(model.rewards.combo_experience)
                    .to_string(),
                290.,
                20.,
                red,
            ),
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
        let (rows, items) = state.page_rows();
        if !rows.is_empty() {
            text::glyph_gradient(
                &mut batches[2],
                &self.art.font,
                if items { "ITEM(S) FOUND" } else { "INFO" },
                [if items { 98. } else { 108. }, 340.],
                [13., 18.],
                heading,
            )?;
            for (column, row) in rows.iter().enumerate() {
                text::fit(
                    &mut batches[1],
                    &self.font,
                    &row.text,
                    [
                        if items {
                            108. + column as f32 * 260.
                        } else {
                            136.
                        },
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
            if self
                .results
                .as_ref()
                .is_some_and(|state| !state.page_rows().0.is_empty())
            {
                self.windows.render_battle_results_panel(
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
                    character: 8,
                    technique: 201,
                },
                ResultNotice::MaximumVital {
                    character: 1,
                    vital: MaximumVital::Hp,
                    amount: 5,
                },
                ResultNotice::HappinessExperience {
                    character: 3,
                    amount: 100,
                },
                ResultNotice::HappinessGald {
                    character: 4,
                    amount: 200,
                },
                ResultNotice::CompoundEx { character: 9 },
                ResultNotice::Level {
                    character: 1,
                    level: 29,
                },
                ResultNotice::TpRecovery {
                    character: 1,
                    amount: 1,
                },
                ResultNotice::MaximumVital {
                    character: 1,
                    vital: MaximumVital::Tp,
                    amount: 5,
                },
            ],
            cook_prompt: Some(8),
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
        }
        assert!(hud.results_on_last_page());
        assert_eq!(hud.take_result_sounds(), vec![1; pages - 1]);
        model.notices.push(ResultNotice::Cooking {
            character: 8,
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
            model.cook_prompt = Some(0);
            assert_eq!(
                hud.synchronize_results(&model, &diagnostics).is_err(),
                paranoid
            );
            assert!(
                diagnostics.has_errors() && hud.results.is_none() && hud.results_on_last_page()
            );
            model.cook_prompt = Some(8);
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
