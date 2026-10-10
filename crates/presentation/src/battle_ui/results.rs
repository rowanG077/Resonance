//! Final rewards and paged notices. Gameplay owns result timing and commits.
use super::*;
use resonance_game::battle::results::{ResultNotice, Results as Model};

const ROWS_PER_PAGE: usize = 3;

pub(super) struct Results {
    pub(super) model: Model,
    rows: Vec<(String, bool)>,
    chef: Option<String>,
    page: usize,
    sounds: Vec<u16>,
}
impl Results {
    fn pages(&self) -> usize {
        self.rows.len().div_ceil(ROWS_PER_PAGE).max(1)
    }
    fn ready(&self) -> bool {
        self.page + 1 == self.pages()
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
            rows.push((
                format!(
                    "Found: {name} x{}{}",
                    award.count,
                    if overflow { " (inventory full)" } else { "" }
                ),
                overflow,
            ));
        }
        for notice in &model.notices {
            let (character, message) = match *notice {
                ResultNotice::Level { character, level } => {
                    (character, format!("Level up: {level}"))
                }
                ResultNotice::TpRecovery { character, amount } => {
                    (character, format!("Recovered {amount} TP"))
                }
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
                        "Earned title: {}",
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
            rows.push((
                format!("{}: {message}", character_name(model, character)?),
                false,
            ));
        }
        let chef = model
            .cook_prompt
            .map(|character| character_name(model, character).map(str::to_owned))
            .transpose()?;
        for text in rows.iter().map(|(text, _)| text).chain(chef.iter()) {
            self.font.validate_text(text)?;
        }
        self.font.validate_text("TOTAL EXP COMBO BONUS MAX COMBO LOOT GALD TIME GRADE REWARDS Continue Cook: 0123456789+-.()/% ")?;
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
            state.chef = chef;
            state.page = state.page.min(state.pages() - 1);
            state.sounds.extend(sounds);
        } else {
            self.results = Some(Results {
                model: model.clone(),
                rows,
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
    fn draw_results(&self) -> Result<[Batch; 2]> {
        let mut batches: [Batch; 2] = Default::default();
        let Some(state) = &self.results else {
            return Ok(batches);
        };
        let model = &state.model;
        for rect in [[12., 16., 628., 120.], [12., 266., 628., 380.]] {
            batches[0].quad(rect, [0.5; 4], [0.04, 0.05, 0.08, 0.9]);
        }
        let frames = model.combat_ticks;
        let grade = model.grade.unsigned_abs();
        let lines = [
            format!(
                "TOTAL EXP {}",
                model
                    .rewards
                    .experience
                    .saturating_add(model.rewards.combo_experience)
            ),
            format!("GALD {}", model.rewards.gald),
            format!("COMBO BONUS +{}", model.rewards.combo_experience),
            format!(
                "TIME {:02}:{:02}.{:02}",
                frames / 3600,
                frames / 60 % 60,
                frames % 60 * 100 / 60
            ),
            format!(
                "MAX COMBO {} / LOOT +{}%",
                model.maximum_combo,
                resonance_game::battle::rewards::combo_drop_bonus(model.maximum_combo)
            ),
            format!(
                "GRADE {}{}.{:02}",
                if model.grade < 0 { '-' } else { '+' },
                grade / 100,
                grade % 100
            ),
        ];
        for (index, line) in lines.iter().enumerate() {
            text::fit(
                &mut batches[1],
                &self.font,
                line,
                [
                    24. + (index % 2) as f32 * 306.,
                    28. + (index / 2) as f32 * 30.,
                    286.,
                    22.,
                ],
                [128, 128, 128, 255],
            )?;
        }
        text::fit(
            &mut batches[1],
            &self.font,
            &format!("REWARDS  {}/{}", state.page + 1, state.pages()),
            [24., 272., 270., 16.],
            [120, 100, 50, 255],
        )?;
        for (index, (line, overflow)) in state
            .rows
            .iter()
            .skip(state.page * ROWS_PER_PAGE)
            .take(ROWS_PER_PAGE)
            .enumerate()
        {
            text::fit(
                &mut batches[1],
                &self.font,
                line,
                [24., 292. + index as f32 * 21., 592., 20.],
                if *overflow {
                    [128, 85, 70, 255]
                } else {
                    [128, 128, 128, 255]
                },
            )?;
        }
        text::fit(
            &mut batches[1],
            &self.font,
            "Continue",
            [24., 358., 170., 16.],
            [100, 100, 100, 255],
        )?;
        if let Some(chef) = &state.chef {
            text::fit(
                &mut batches[1],
                &self.font,
                &format!("Cook: {chef}"),
                [330., 358., 286., 16.],
                [100, 120, 128, 255],
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
            for (layer, batch) in self.ui.results.iter_mut().zip(batches) {
                layer.upload(batch, commands, meshes)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.results = None;
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
        assert_eq!(
            hud.results.as_ref().unwrap().rows[1].0,
            "R%s: Learned Mirage"
        );
        assert!(
            hud.results.as_ref().unwrap().rows[0]
                .0
                .contains("inventory full")
        );
        assert!(!hud.results_on_last_page());
        hud.next_result_page();
        assert!(!hud.results_on_last_page());
        hud.next_result_page();
        assert!(hud.results_on_last_page());
        assert_eq!(hud.take_result_sounds(), [1, 1]);
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
                .0
                .starts_with("R%s: Failed to cook ")
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
                    .all(|p| (-308. ..=308.).contains(&p[0]) && (-140. ..=224.).contains(&p[1]))
            );
        }
        let mut world = World::new();
        world.insert_resource(materials);
        let mut meshes = Assets::<Mesh>::default();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        hud.prepare(&mut Commands::new(&mut queue, &world), &mut meshes);
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
            let glyph = hud.font.glyphs.remove(&'T').unwrap();
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
            hud.font.glyphs.insert('T', glyph);
        }
        Ok(())
    }
}
