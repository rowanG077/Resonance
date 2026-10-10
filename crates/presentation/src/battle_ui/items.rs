//! Shared inventory and native scan feedback from immutable battle snapshots.
use super::*;
use resonance_battle::{ActorId, Affinity, Cue};
use resonance_game::battle::command::ListFrame;

const SCAN_LIFETIME: u32 = 4 * 60;
const SCAN_WIDTH: f32 = 256.;
// The first affinity is neutral; the remaining entries follow Element::ALL.
const ELEMENT_NAMES: [&str; 9] = [
    "Neutral",
    "Water",
    "Wind",
    "Fire",
    "Earth",
    "Lightning",
    "Ice",
    "Light",
    "Darkness",
];

#[derive(Clone, Copy)]
pub(super) struct Scan {
    target: ActorId,
    age: u32,
}

pub(super) fn validate_font(font: &BitmapFont) -> Result<()> {
    for text in ELEMENT_NAMES
        .into_iter()
        .chain(["SCAN: HP TP / 0123456789 All affinities normal Weak Resists Absorbs Immune"])
    {
        font.validate_text(text)?;
    }
    Ok(())
}

impl Artwork {
    pub(super) fn advance_scan(&mut self, frame: &BattleFrame, elapsed: u32) {
        if frame.recognized_result.is_some() {
            self.scan = None;
            return;
        }
        if let Some(scan) = &mut self.scan {
            scan.age += elapsed;
            if scan.age >= SCAN_LIFETIME {
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

    pub(super) fn scan_batches(&self, frame: &BattleFrame) -> Result<[Batch; 2]> {
        let mut panels = Batch::default();
        let mut text = Batch::default();
        let Some(scan) = self.scan else {
            return Ok([panels, text]);
        };
        let target = frame
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
        let rows: Vec<_> = ELEMENT_NAMES
            .into_iter()
            .zip(target.equipment.affinities)
            .filter_map(|(name, affinity)| {
                let (label, color) = match affinity {
                    Affinity::Normal => return None,
                    Affinity::Weak => ("Weak", [128, 112, 64, 255]),
                    Affinity::Resistant => ("Resists", [80, 110, 128, 255]),
                    Affinity::Absorb => ("Absorbs", [80, 128, 96, 255]),
                    Affinity::Immune => ("Immune", [100, 104, 110, 255]),
                };
                Some((name, label, color))
            })
            .collect();
        let [x, y] = [640. - SCAN_WIDTH - 8., 8.];
        let row_height = 20.;
        let affinities_y = y + 80.;
        let bottom = affinities_y + rows.len().max(1) as f32 * row_height + 8.;
        panels.quad(
            [x, y, x + SCAN_WIDTH, bottom],
            [0.5; 4],
            [0.04, 0.05, 0.08, 0.96],
        );
        let white = [128, 128, 128, 255];
        text::fit(
            &mut text,
            &self.font,
            &format!("SCAN: {name}"),
            [x + 8., y + 8., SCAN_WIDTH - 16., 20.],
            [128, 112, 64, 255],
        )?;
        for (row, caption) in [
            format!("HP {} / {}", target.hp, target.equipment.max_hp),
            format!("TP {} / {}", target.tp, target.equipment.max_tp),
        ]
        .iter()
        .enumerate()
        {
            text::fit(
                &mut text,
                &self.font,
                caption,
                [x + 8., y + 34. + row as f32 * 22., SCAN_WIDTH - 16., 18.],
                white,
            )?;
        }
        if rows.is_empty() {
            text::fit(
                &mut text,
                &self.font,
                "All affinities normal",
                [x + 8., affinities_y, SCAN_WIDTH - 16., 18.],
                white,
            )?;
        }
        for (row, &(element, affinity, color)) in rows.iter().enumerate() {
            let y = affinities_y + row as f32 * row_height;
            text::fit(
                &mut text,
                &self.font,
                element,
                [x + 8., y, 124., 18.],
                white,
            )?;
            text::fit(
                &mut text,
                &self.font,
                affinity,
                [x + 140., y, 108., 18.],
                color,
            )?;
        }
        Ok([panels, text])
    }

    pub(super) fn render_scan(
        &mut self,
        frame: &BattleFrame,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let batches = self.scan_batches(frame)?;
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
        self.notice_request(actor, &name, duration, frame)
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
