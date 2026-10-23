//! Shared inventory and authored enemy inspection from immutable battle snapshots.
use super::*;
use resonance_battle::{ActorId, Affinity, Cue};
use resonance_game::battle::command::{Frame, ListFrame, View};

const SCAN_HOLD: u32 = 240;
const SCAN_SLIDE: u32 = 15;
const SCAN_WIDTH: f32 = 176.;
const INSPECTION_LINES: usize = 12;
const INSPECTION_GLYPHS: usize = INSPECTION_LINES + 1;
const SCAN_LAYERS: usize = INSPECTION_GLYPHS + 1;
const AFFINITY_ICON_ADVANCES: [f32; 9] = [24., 24., 24., 24., 21., 17., 14.5, 13.5, 12.];
const INSPECTION_VITALS: [[[u8; 4]; 2]; 2] = [
    [[128, 128, 128, 255], [128, 64, 64, 255]],
    [[128, 128, 128, 255], [64, 64, 128, 255]],
];
const INSPECTION_SHADOW: [u8; 4] = [16, 16, 64, 255];

#[derive(Clone, Copy)]
pub(super) struct Scan {
    target: ActorId,
    age: u32,
}

pub(super) fn layers(
    art: &Art,
    palette: &Palette,
    materials: &mut Assets<Surface>,
    mut load: impl FnMut(&resonance_content::font::UiTexture, bool, bool) -> Handle<Image>,
) -> Vec<HudLayer> {
    let mut definitions = vec![
        palette.font.clone().at(Depth::Scan),
        palette.main_font.clone().at(Depth::Scan),
    ];
    for (index, texture) in art.feedback.scan_icons.iter().enumerate() {
        let image = load(texture, false, true);
        let mut definition =
            LayerDefinition::new(image, [texture.width, texture.height], Depth::Scan);
        // Later symbols cover earlier ones without overtaking the inspection text.
        definition.depth += (index + 1) as f32 / (art.feedback.scan_icons.len() + 1) as f32;
        // Element symbols stay crisp; the final weakness/resistance headings are smooth.
        definitions.push(if index < AFFINITY_ICON_ADVANCES.len() {
            definition.nearest()
        } else {
            definition
        });
    }
    definitions.push(palette.solid.clone().at(Depth::Scan));
    definitions.push(
        palette
            .font
            .clone()
            .at(Depth::Scan)
            .nearest()
            .offset_depth(1.),
    );
    palette.layers(definitions, materials)
}

fn inspection_target(frame: &BattleFrame, command: Option<&Frame>) -> Option<ActorId> {
    if frame.recognized_result.is_some() {
        return None;
    }
    match command.map(|command| &command.view) {
        Some(View::Enemy { target }) => Some(*target),
        Some(View::TechTarget { target }) => *target,
        Some(_) => None,
        None => frame
            .target_selector
            .and_then(|owner| frame.targets.get(owner.index()).copied().flatten()),
    }
}

fn inspection_line(batch: &mut Batch, from: [f32; 2], to: [f32; 2], color: [u8; 4]) {
    let delta = Vec2::from_array(to) - Vec2::from_array(from);
    let normal = Vec2::new(-delta.y, delta.x).normalize_or_zero() * 1.5;
    let start = batch.positions.len();
    batch.quad([0.; 4], [0.5; 4], hud_color(color));
    for (position, point) in batch.positions[start..].iter_mut().zip([
        Vec2::from_array(from) - normal,
        Vec2::from_array(to) - normal,
        Vec2::from_array(to) + normal,
        Vec2::from_array(from) + normal,
    ]) {
        *position = [point.x - 320., 240. - point.y, 0.];
    }
}

/// Selection uses a callout beside the actor; the timed item receipt is a separate panel.
fn draw_inspection(
    frame: &BattleFrame,
    target: ActorId,
    anchor: [f32; 3],
    presentation_tick: u64,
    font: &BitmapFont,
    batches: &mut [Batch; SCAN_LAYERS],
) -> Result<()> {
    let actor = frame
        .actors
        .get(target.index())
        .context("inspection target is not in battle frame")?;
    let Some(camera) = frame.camera.filter(|_| {
        actor.available()
            && actor.hp > 0
            && !frame
                .models
                .iter()
                .any(|model| model.actor == target && !model.visible)
    }) else {
        return Ok(());
    };
    let forward: f32 = (0..3)
        .map(|axis| (anchor[axis] - camera.eye[axis]) * (camera.focus[axis] - camera.eye[axis]))
        .sum();
    if forward <= 0. {
        return Ok(());
    }
    let [x, scene_y] = resonance_battle::project_screen_point(camera, anchor);
    let y = scene_y * 480. / 448. + 16.;
    let ground_x =
        resonance_battle::project_screen_point(camera, [actor.position[0], 0., actor.position[2]])
            [0];
    let direction = if ground_x <= 400. { 1. } else { -1. };
    let phase = ((presentation_tick * 4) % 360) as f32;
    let color = [
        (128. + 95. * phase.to_radians().sin()) as u8,
        (128. + 95. * phase.to_radians().cos()) as u8,
        (128. + 95. * (phase * 2.).to_radians().sin()) as u8,
        160,
    ];
    for (side, left) in [x - 25., x + 1.].into_iter().enumerate() {
        let shade = 96 + (((presentation_tick >> 4) as usize + side) % 2) as u8 * 96;
        let u = 256. + side as f32 * 24.;
        batches[INSPECTION_GLYPHS].quad(
            [left, y - 66., left + 24., y - 42.],
            [u, 0., u + 24., 24.],
            hud_color([shade, shade, shade, 255]),
        );
    }
    let corners = [
        [x - 40., y - 40.],
        [x + 40., y - 40.],
        [x + 40., y + 40.],
        [x - 40., y + 40.],
    ];
    for edge in 0..4 {
        inspection_line(
            &mut batches[INSPECTION_LINES],
            corners[edge],
            corners[(edge + 1) % 4],
            color,
        );
    }
    if !frame.scanned_enemies.contains(target) {
        return Ok(());
    }
    let joint = [x + 56. * direction, y - 16.];
    let end = [joint[0] + 96. * direction, joint[1]];
    for (from, to) in [([x + 40. * direction, y], joint), (joint, end)] {
        inspection_line(&mut batches[INSPECTION_LINES], from, to, color);
    }
    let text_x = joint[0].min(end[0]) - 8.;
    for (row, caption) in [
        format!("{:5}/{}", actor.hp, actor.equipment.max_hp),
        format!("{:5}/{}", actor.tp, actor.equipment.max_tp),
    ]
    .into_iter()
    .enumerate()
    {
        let text_y = joint[1] - 24. + row as f32 * 28.;
        for (offset, colors) in [(2., [INSPECTION_SHADOW; 2]), (0., INSPECTION_VITALS[row])] {
            text::italic(
                &mut batches[INSPECTION_GLYPHS],
                font,
                &caption,
                [text_x + offset, text_y + offset],
                [16., 18.],
                12.,
                6.,
                colors,
            )?;
        }
    }
    Ok(())
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
    pub fn inspection_target(
        &self,
        frame: &BattleFrame,
        command: Option<&Frame>,
    ) -> Option<ActorId> {
        inspection_target(frame, command)
    }

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
            .find(|enemy| enemy.actor == scan.target)
            .context("scan enemy name was not prepared")?
            .name;
        let [x, y, _, _] = scan_rect(scan, actor);
        let mut left = x + 4.;
        for character in name.chars() {
            let glyph = self
                .font
                .glyphs
                .get(&character)
                .context("missing scan name glyph")?;
            if character != ' ' {
                let [u, v, width, height] = glyph.rect.map(|value| value as f32);
                batches[1].quad(
                    [left, y + 4., left + 16., y + 26.],
                    [u, v, u + width, v + height],
                    hud_color([128, 128, 128, 255]),
                );
            }
            left += (glyph.advance * 15 / 24) as f32;
        }
        for (row, caption) in [
            format!("{:5}/{}", actor.hp, actor.equipment.max_hp),
            format!("{:5}/{}", actor.tp, actor.equipment.max_tp),
        ]
        .iter()
        .enumerate()
        {
            text::italic(
                &mut batches[0],
                &self.art.font,
                caption,
                [x + 8., y + 30. + row as f32 * 24.],
                [14., 18.],
                12.,
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
            let advance = AFFINITY_ICON_ADVANCES[elements.len() - 1];
            for (column, element) in elements.into_iter().enumerate() {
                let left = x + 32. + (column as f32 * advance).floor();
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
        inspection: Option<(ActorId, [f32; 3])>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let scan = if frame.recognized_result.is_some() || command.is_some() {
            None
        } else {
            self.scan
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
        let mut batches = self.draw_scan(frame, scan)?;
        if let Some((target, anchor)) = inspection {
            draw_inspection(
                frame,
                target,
                anchor,
                self.numbers.presentation_tick,
                &self.art.font,
                &mut batches,
            )?;
        }
        for (layer, batch) in self.ui.scan.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
    pub fn item_notice_request(
        &mut self,
        actor: ActorId,
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

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_game::battle::command::Command;

    #[test]
    fn inspection_tracks_manual_and_command_targets_and_dismisses() -> Result<()> {
        let owner = ActorId::from_index(0)?;
        let first = ActorId::from_index(1)?;
        let second = ActorId::from_index(2)?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.targets = vec![Some(first), None, None];
        assert_eq!(inspection_target(&frame, None), None);
        frame.target_selector = Some(owner);
        assert_eq!(inspection_target(&frame, None), Some(first));
        frame.targets[0] = Some(second);
        assert_eq!(inspection_target(&frame, None), Some(second));
        let mut command = Frame {
            actor: owner,
            selected: Command::Items,
            enabled: 0xff,
            controller: resonance_battle::ControllerPort::One,
            connected: [true, false, false, false],
            view: View::Enemy { target: first },
            escape_requested: false,
        };
        assert_eq!(inspection_target(&frame, Some(&command)), Some(first));
        command.view = View::TechTarget { target: None };
        assert_eq!(inspection_target(&frame, Some(&command)), None);
        command.view = View::Strip;
        assert_eq!(inspection_target(&frame, Some(&command)), None);
        frame.target_selector = None;
        assert_eq!(inspection_target(&frame, None), None);
        frame.target_selector = Some(owner);
        frame.recognized_result = Some(resonance_battle::BattleResult::Victory);
        assert_eq!(inspection_target(&frame, None), None);
        Ok(())
    }

    #[test]
    #[ignore = "requires the battle font; CPU target inspection drawing only"]
    fn scanned_target_adds_floating_vitals_without_an_elemental_panel() -> Result<()> {
        let font = fixtures::battle_font()?;
        let target = ActorId::from_index(0)?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        let draw = |frame: &BattleFrame| -> Result<[Batch; SCAN_LAYERS]> {
            let mut batches = std::array::from_fn(|_| Batch::default());
            draw_inspection(
                frame,
                target,
                frame.actors[0].target_center(),
                0,
                &font,
                &mut batches,
            )?;
            Ok(batches)
        };
        let unscanned = draw(&frame)?;
        assert_eq!(
            unscanned[INSPECTION_GLYPHS].positions.len(),
            8,
            "two target chevrons"
        );
        assert_eq!(unscanned[INSPECTION_LINES].positions.len(), 16);
        frame
            .scanned_enemies
            .insert(resonance_battle::ActorId::from_index(0).unwrap());
        let scanned = draw(&frame)?;
        assert!(
            scanned[INSPECTION_GLYPHS].positions.len() > 8,
            "revealed HP and TP"
        );
        assert_eq!(scanned[INSPECTION_LINES].positions.len(), 24);
        assert!(
            scanned[..INSPECTION_LINES]
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        assert_eq!(
            scanned[INSPECTION_GLYPHS].positions[..8],
            unscanned[INSPECTION_GLYPHS].positions
        );
        frame.actors[0].hp -= 1;
        let damaged = draw(&frame)?;
        assert!(
            damaged[INSPECTION_GLYPHS] != scanned[INSPECTION_GLYPHS],
            "vitals stay live"
        );
        assert!(damaged[INSPECTION_LINES] == scanned[INSPECTION_LINES]);
        frame.actors[0].position[0] += 10.;
        assert!(
            draw(&frame)? != damaged,
            "callout follows the selected actor"
        );
        frame.actors[0].hp = 0;
        assert!(draw(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        Ok(())
    }
}
