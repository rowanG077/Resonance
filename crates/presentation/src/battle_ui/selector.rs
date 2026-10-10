//! Native target outline and scanned vitals, projected from the presented camera.
use super::*;
use resonance_game::battle::command::{Frame, View};

const MARGIN: f32 = 8.;
const OUTLINE_WIDTH: f32 = 72.;
const VITALS_SIZE: [f32; 2] = [160., 52.];

pub(super) fn draw(
    frame: &BattleFrame,
    command: Option<&Frame>,
    font: &BitmapFont,
    show_vitals: bool,
) -> Result<[Batch; 2]> {
    let mut panels = Batch::default();
    let mut text = Batch::default();
    let target = match command.map(|command| &command.view) {
        Some(View::Enemy { target }) => Some(*target),
        Some(View::TechTarget { target }) => *target,
        Some(_) => None,
        None => frame
            .target_selector
            .and_then(|owner| frame.targets[owner.index()]),
    };
    let (Some(target), Some(camera)) = (target, frame.camera) else {
        return Ok([panels, text]);
    };
    let actor = frame
        .actors
        .get(target.index())
        .context("unknown selected target")?;
    if frame.recognized_result.is_some() {
        return Ok([panels, text]);
    }
    let center = actor.target_center();
    let forward: f32 = (0..3)
        .map(|axis| (center[axis] - camera.eye[axis]) * (camera.focus[axis] - camera.eye[axis]))
        .sum();
    if forward <= 0. {
        return Ok([panels, text]);
    }
    let [x, y] = resonance_battle::project_screen_point(camera, center);
    let [_, head] = resonance_battle::project_screen_point(
        camera,
        [actor.position[0], actor.body_top(), actor.position[2]],
    );
    let height = ((y - head).abs() * 2. + 16.).clamp(48., 160.);
    let left = (x - OUTLINE_WIDTH / 2.).clamp(MARGIN, 640. - MARGIN - OUTLINE_WIDTH);
    let top = (y - height / 2.).clamp(MARGIN, party::TOP - MARGIN - height);
    let right = left + OUTLINE_WIDTH;
    let bottom = top + height;
    for rect in [
        [left, top, right, top + 2.],
        [left, bottom - 2., right, bottom],
        [left, top, left + 2., bottom],
        [right - 2., top, right, bottom],
    ] {
        panels.quad(rect, [0.5; 4], [1., 0.8, 0.35, 1.]);
    }
    if show_vitals && frame.scanned_enemies & (1 << target.index()) != 0 {
        let [width, height] = VITALS_SIZE;
        let x = if x < 320. {
            right + MARGIN
        } else {
            left - MARGIN - width
        }
        .clamp(MARGIN, 640. - MARGIN - width);
        let y = top.clamp(MARGIN, party::TOP - MARGIN - height);
        panels.quad(
            [x, y, x + width, y + height],
            [0.5; 4],
            [0.04, 0.05, 0.08, 0.95],
        );
        for (row, caption) in [
            format!("HP {} / {}", actor.hp, actor.equipment.max_hp),
            format!("TP {} / {}", actor.tp, actor.equipment.max_tp),
        ]
        .iter()
        .enumerate()
        {
            text::fit(
                &mut text,
                font,
                caption,
                [x + 6., y + 6. + row as f32 * 22., width - 12., 18.],
                [128, 128, 128, 255],
            )?;
        }
    }
    Ok([panels, text])
}

impl Artwork {
    pub(super) fn render_selector(
        &mut self,
        frame: &BattleFrame,
        command: Option<&Frame>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let batches = draw(
            frame,
            command,
            &self.font,
            self.scan.is_none() || command.is_some(),
        )?;
        for (layer, batch) in self.ui.selector.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}
