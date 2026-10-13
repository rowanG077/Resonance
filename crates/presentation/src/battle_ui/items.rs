//! Shared inventory and authored enemy inspection from immutable battle snapshots.
use super::*;
use resonance_battle::{ActorId, Affinity, Cue};
use resonance_game::battle::command::{Frame, ListFrame, View};

const SCAN_HOLD: u32 = 240;
const SCAN_SLIDE: u32 = 15;
const SCAN_WIDTH: f32 = 176.;
const SCAN_LAYERS: usize = 12;

#[derive(Clone, Copy)]
pub(super) struct Scan {
    target: ActorId,
    age: u32,
}

pub(super) fn layers(
    art: &Art,
    palette: &Palette,
    materials: &mut Assets<Surface>,
    mut load: impl FnMut(&resonance_content::font::UiTexture, bool) -> Result<Option<Handle<Image>>>,
) -> Result<Vec<HudLayer>> {
    let mut definitions = vec![
        palette.font.clone().at(Depth::Scan),
        palette.main_font.clone().at(Depth::Scan),
    ];
    for texture in &art.feedback.scan_icons {
        let image = load(texture, false)?.context("battle inspection atlas is missing")?;
        definitions.push(LayerDefinition::new(
            image,
            [texture.width, texture.height],
            Depth::Scan,
        ));
    }
    Ok(palette.layers(definitions, materials))
}

fn scan_rect(scan: Scan, actor: &resonance_battle::ActorFrame) -> [f32; 4] {
    let weak = actor.equipment.affinities.contains(&Affinity::Weak);
    let resist = actor
        .equipment
        .affinities
        .iter()
        .any(|a| matches!(a, Affinity::Resistant | Affinity::Absorb | Affinity::Immune));
    let rows = usize::from(weak) + usize::from(resist);
    let x = if scan.age < SCAN_SLIDE {
        (640. - scan.age as f32 * 12.).max(464.)
    } else {
        464. + scan.age.saturating_sub(SCAN_SLIDE + SCAN_HOLD) as f32 * 12.
    };
    [
        x,
        376. - rows as f32 * 24.,
        SCAN_WIDTH,
        72. + rows as f32 * 24.,
    ]
}

impl Artwork {
    pub(super) fn advance_scan(&mut self, frame: &BattleFrame, elapsed: u32) {
        if frame.recognized_result.is_some() {
            self.scan = None;
            return;
        }
        if let Some(scan) = &mut self.scan {
            scan.age += elapsed;
            if scan.age >= SCAN_HOLD + SCAN_SLIDE * 2 {
                self.scan = None;
            }
        }
        for cue in &frame.cues {
            if let Cue::EnemyScanned { actor } = *cue {
                self.scan = Some(Scan {
                    target: actor,
                    age: 0,
                });
            }
        }
    }

    #[cfg(test)]
    pub(super) fn scan_batches(&self, frame: &BattleFrame) -> Result<[Batch; SCAN_LAYERS]> {
        self.draw_scan(frame, self.scan)
    }

    fn draw_scan(&self, frame: &BattleFrame, scan: Option<Scan>) -> Result<[Batch; SCAN_LAYERS]> {
        let mut batches = std::array::from_fn(|_| Batch::default());
        let Some(scan) = scan else {
            return Ok(batches);
        };
        let actor = frame
            .actors
            .get(scan.target.index())
            .context("scan target is not in battle frame")?;
        let name = &self
            .combat
            .enemies
            .iter()
            .find(|enemy| enemy.actor == scan.target.index())
            .context("scan enemy name was not prepared")?
            .name;
        let [x, y, width, _] = scan_rect(scan, actor);
        text::fit(
            &mut batches[1],
            &self.font,
            name,
            [x + 4., y + 4., width - 8., 22.],
            [128, 128, 128, 255],
        )?;
        for (row, caption) in [
            format!("{:5}/{}", actor.hp, actor.equipment.max_hp),
            format!("{:5}/{}", actor.tp, actor.equipment.max_tp),
        ]
        .iter()
        .enumerate()
        {
            let scale = ((width - 16.) / (caption.len() as f32 * 12. + 2.)).min(1.);
            text::italic(
                &mut batches[0],
                &self.art.font,
                caption,
                [x + 8., y + 30. + row as f32 * 24.],
                [14. * scale, 18. * scale],
                12. * scale,
                0.,
                self.art.feedback.scan_vitals[row],
            )?;
        }
        let mut top = y + 74.;
        for weak in [true, false] {
            let elements = actor
                .equipment
                .affinities
                .iter()
                .enumerate()
                .filter_map(|(element, affinity)| {
                    (if weak {
                        *affinity == Affinity::Weak
                    } else {
                        matches!(
                            affinity,
                            Affinity::Resistant | Affinity::Absorb | Affinity::Immune
                        )
                    })
                    .then_some(element)
                })
                .collect::<Vec<_>>();
            if elements.is_empty() {
                continue;
            }
            let u = if weak { 214. } else { 238. };
            batches[11].quad(
                [x + 4., top, x + 28., top + 24.],
                [u, 136., u + 24., 160.],
                [1.; 4],
            );
            let advance = if elements.len() <= 4 {
                24.
            } else {
                120. / elements.len() as f32
            };
            for (column, element) in elements.into_iter().enumerate() {
                let left = x + 32. + column as f32 * advance;
                let u = element as f32 * 24.;
                batches[element + 2].quad(
                    [left, top, left + 24., top + 24.],
                    [u, 136., u + 24., 160.],
                    [1.; 4],
                );
            }
            top += 24.;
        }
        Ok(batches)
    }

    pub(super) fn render_scan(
        &mut self,
        frame: &BattleFrame,
        command: Option<&Frame>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let selected = match command.map(|command| &command.view) {
            Some(View::Enemy { target }) => Some(*target),
            Some(View::TechTarget { target }) => *target,
            Some(_) => None,
            None => frame
                .target_selector
                .and_then(|owner| frame.targets[owner.index()]),
        };
        let selected = selected
            .filter(|target| frame.scanned_enemies & (1 << target.index()) != 0)
            .map(|target| Scan {
                target,
                age: SCAN_SLIDE,
            });
        let scan = if frame.recognized_result.is_some() {
            None
        } else if command.is_some() {
            selected
        } else {
            self.scan.or(selected)
        };
        if let Some(scan) = scan {
            self.windows.render_battle_scan_panel(
                scan_rect(scan, &frame.actors[scan.target.index()]),
                &self.font,
                &self.dialogue,
                &self.settings,
                commands,
                meshes,
            )?;
        }
        let batches = self.draw_scan(frame, scan)?;
        for (layer, batch) in self.ui.scan.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
    pub fn item_notice_request(
        &mut self,
        actor: usize,
        item: u16,
        duration: u16,
        frame: &BattleFrame,
    ) -> Result<()> {
        let name = self.menu_data.item_text(item)?.name.clone();
        self.notice_with_icon(actor, &name, duration, frame, notice::Icon::Item)
    }

    pub fn hide_combat_for_menu(&mut self, commands: &mut Commands) {
        for layer in self.layers_mut() {
            layer.show(false, commands);
        }
    }

    pub fn clear_menu(&mut self, commands: &mut Commands) {
        self.windows.clear_page(commands);
    }

    pub fn render_items(
        &mut self,
        frame: Option<&ListFrame>,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let Some(frame) = frame else {
            return Ok(());
        };
        self.windows.render_battle_items(
            frame,
            &self.menu_data,
            &self.font,
            &self.dialogue,
            &self.settings,
            tick,
            commands,
            meshes,
        )
    }
}
