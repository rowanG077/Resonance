//! Shared HUD display clocks and original intro, orders, radar and combo passes.
use super::*;
#[path = "combo.rs"]
mod combo;

const INTRO_PANEL: usize = 0;
const INTRO_TEXT: usize = 1;
const ORDERS_PANEL: usize = 2;
const ORDERS_TEXT: usize = 3;
const ORDERS_ARROWS: usize = 4;
const RADAR_NUMBERS: usize = 5;
const RADAR_PANEL: usize = 6;
const RADAR_TEXT: usize = 7;
const COMBO_PANEL: usize = 8;
const COMBO_TEXT: usize = 9;
const RADAR_EFFECT: usize = 10;
const RADAR_POINTER: usize = 11;
const RADAR_CONTROL: usize = 12;
const ENEMY_ICONS: usize = 16;

#[derive(Clone, Copy)]
struct IntroRow {
    x: i16,
    y: i16,
    age: i16,
    alpha: i16,
}
impl IntroRow {
    fn new(group: usize) -> Self {
        Self {
            x: 320,
            y: 8 + group as i16 * 28,
            age: -(group as i16 * 4),
            alpha: 160,
        }
    }
    fn advance(&mut self) {
        if (120..=190).contains(&self.age) {
            self.x += 28;
            self.alpha = (self.alpha - 4).max(0);
        }
        self.age = (self.age + 1).min(1024);
    }
}

pub(super) struct Hud {
    pub start: usize,
    enemies: Vec<EnemyHud>,
    groups: [Option<String>; 4],
    group_count: usize,
    icon_layers: [Option<usize>; 4],
    notice_start: usize,
    notices: [Option<overlays::Notice>; 2],
    latest_notice: usize,
    intro: [IntroRow; 4],
    intro_active: bool,
    tick: u32,
    orders_x: i16,
    orders_age: u16,
    orders_text: String,
    orders_kind: u8,
    radar_x: i16,
    radar_rotation: i32,
    radar_phases: Vec<u16>,
    combos: combo::Combos,
}

impl Hud {
    pub fn load(
        art: &Art,
        font: &BitmapFont,
        enemies: &[EnemyHud],
        images: &mut Vec<Handle<Image>>,
        sizes: &mut Vec<[u32; 2]>,
        load: impl Fn(String, bool) -> Handle<Image>,
    ) -> Result<Self> {
        let start = images.len();
        // All materials, including every possible radar pointer and cast glow,
        // exist before prepare() and the scene's residency seal.
        let solid = images[super::SOLID].clone();
        let dol = images[super::DOL_FONT].clone();
        let atlas = images[super::FONT].clone();
        for (image, size) in [
            (solid.clone(), [1, 1]),
            (dol.clone(), [font.width, font.height]),
            (solid.clone(), [1, 1]),
            (dol.clone(), [font.width, font.height]),
            (atlas.clone(), [art.font.width, art.font.height]),
            (atlas.clone(), [art.font.width, art.font.height]),
            (solid.clone(), [1, 1]),
            (dol, [font.width, font.height]),
            (solid, [1, 1]),
            (atlas, [art.font.width, art.font.height]),
        ] {
            images.push(image);
            sizes.push(size);
        }
        for texture in std::iter::once(&art.radar.effect.texture)
            .chain(std::iter::once(&art.markers.target_background.texture))
            .chain(&art.markers.target_foregrounds)
        {
            images.push(load(texture.path.clone(), false));
            sizes.push([texture.width, texture.height]);
        }
        let mut groups: [Option<String>; 4] = Default::default();
        let mut icon_layers = [None; 4];
        let group_count = enemies
            .iter()
            .map(|enemy| usize::from(enemy.group) + 1)
            .max()
            .unwrap_or(0);
        ensure!(group_count <= 4, "too many battle HUD enemy groups");
        for enemy in enemies {
            let group = usize::from(enemy.group);
            enemy.icon.validate()?;
            ensure!(
                enemy.icon.width >= 32 && enemy.icon.height >= 32,
                "enemy HUD icon is too small"
            );
            if let Some(name) = &groups[group] {
                ensure!(name == &enemy.name, "enemy HUD group names differ");
            } else {
                groups[group] = Some(enemy.name.clone());
                icon_layers[group] = Some(images.len());
                images.push(load(enemy.icon.path.clone(), false));
                sizes.push([enemy.icon.width, enemy.icon.height]);
            }
        }
        for (group, name) in groups.iter_mut().enumerate() {
            let count = enemies
                .iter()
                .filter(|enemy| usize::from(enemy.group) == group)
                .count();
            if count >= 2 {
                let text = name.as_mut().unwrap();
                ensure!(
                    art.intro.group_count_format.matches("%d").count() == 1,
                    "invalid enemy group format"
                );
                text.push_str(
                    &art.intro
                        .group_count_format
                        .replacen("%d", &count.to_string(), 1),
                );
            }
        }
        let notice_start = images.len();
        for _ in 0..2 {
            for original in [
                super::NOTICE_SHADOW,
                super::ICONS,
                super::NOTICE_BACKGROUND,
                super::NOTICE_TEXT,
                super::NOTICE_GENERIC,
            ] {
                images.push(images[original].clone());
                sizes.push(sizes[original]);
            }
            for sprite in &art.overlays.notice_symbols {
                images.push(load(sprite.texture.path.clone(), false));
                sizes.push([sprite.texture.width, sprite.texture.height]);
            }
        }
        Ok(Self {
            start,
            enemies: enemies.to_vec(),
            groups,
            group_count,
            icon_layers,
            notice_start,
            notices: Default::default(),
            latest_notice: 0,
            intro: std::array::from_fn(IntroRow::new),
            intro_active: group_count != 0,
            tick: 0,
            orders_x: -160,
            orders_age: 0,
            orders_text: art.orders.cancel_text.clone(),
            orders_kind: 0,
            radar_x: 640,
            radar_rotation: 5,
            radar_phases: vec![0; enemies.len()],
            combos: Default::default(),
        })
    }

    pub fn depth(&self, index: usize) -> Option<f32> {
        if index >= self.notice_start {
            return Some(120. + (index - self.notice_start) as f32);
        }
        let index = index.checked_sub(self.start)?;
        Some(match index {
            INTRO_PANEL => 200.,
            INTRO_TEXT => 201.,
            ORDERS_PANEL => 155.,
            ORDERS_TEXT => 156.,
            ORDERS_ARROWS => 157.,
            RADAR_NUMBERS => 171.,
            RADAR_PANEL => 176.,
            RADAR_TEXT => 177.,
            COMBO_PANEL => 94.,
            COMBO_TEXT => 95.,
            RADAR_EFFECT => 172.,
            RADAR_POINTER => 173.,
            RADAR_CONTROL..ENEMY_ICONS => 174.,
            _ => 170.,
        })
    }

    pub fn additive(&self, index: usize) -> bool {
        index == self.start + RADAR_EFFECT
    }

    pub fn nearest(&self, index: usize) -> bool {
        // 6A46C uses 4AEFC(..., 0) for enemy icons and target pointers.
        // 4B000/4B008 pass GX_NEAR for both filters. The casting halo uses
        // 4AEFC(..., 1), retaining its initialized linear sampler instead.
        // Each nine-layer notice row uses the same nearest atlas calls as
        // 68C90; only its fourth layer is the independently sampled DOL font.
        (self.start + RADAR_POINTER..self.notice_start).contains(&index)
            || ((self.notice_start..self.notice_start + 18).contains(&index)
                && (index - self.notice_start) % 9 != 3)
    }

    pub fn advance(&mut self, frame: &BattleFrame, art: &Art) -> Result<()> {
        if self.tick == frame.hud_update {
            return Ok(());
        }
        // Host GPU waits and repeated snapshots consume no display visits.
        self.tick = frame.hud_update;
        for notice in self.notices.iter_mut().flatten() {
            notice.advance(frame.hud_update, frame.hud_holds.notices);
        }
        // 71230 skips the intro visit under command bit0x10, including the
        // closing visit whose command callback is already absent at draw time.
        if self.intro_active && !frame.hud_holds.intro {
            for row in &mut self.intro[..self.group_count] {
                row.advance();
            }
            // 70AE4 checks this before the final row's increment.
            self.intro_active = self.intro[self.group_count - 1].age <= 191;
        }
        if self.orders_age < 10 || frame.hud_holds.notices {
            self.orders_x = (self.orders_x + 16).min(0);
        } else if self.orders_age >= 60 {
            let minimum = -56 - self.orders_text.chars().count() as i16 * 24;
            self.orders_x = (self.orders_x - 16).max(minimum);
        }
        self.orders_age = (self.orders_age + 1).min(160);
        self.radar_x = if frame.hud_holds.notices {
            (self.radar_x - 16).max(408)
        } else {
            (self.radar_x + 16).min(656)
        };
        self.radar_rotation += 5;
        if self.radar_rotation >= 720 {
            self.radar_rotation -= 360;
        }
        for (phase, enemy) in self.radar_phases.iter_mut().zip(&self.enemies) {
            if frame.actors[enemy.actor].available() {
                *phase = (*phase + 1) % 360;
            }
        }
        self.combos.advance(frame, art.combo.destinations)
    }

    fn draw(
        &mut self,
        frame: &BattleFrame,
        art: &Art,
        font: &BitmapFont,
        batch: &mut [Batch],
        results_active: bool,
    ) -> Result<()> {
        if self.intro_active {
            for (group, text) in self.groups.iter().enumerate() {
                let Some(text) = text else {
                    continue;
                };
                let row = self.intro[group];
                let mut colors = art.intro.panel_colors;
                for color in &mut colors {
                    color[3] = row.alpha as u8;
                }
                panel(
                    &mut batch[INTRO_PANEL],
                    [0., f32::from(row.y + 20), 640., 12.],
                    [0., 4., 0., 4.],
                    colors,
                    0.,
                );
                results::dol_text(
                    &mut batch[INTRO_TEXT],
                    font,
                    text,
                    [f32::from(row.x), f32::from(row.y + 4)],
                    [20., 26.],
                    20.,
                    10.,
                    art.intro.text_color,
                )?;
            }
        }
        if frame.recognized_result.is_none() {
            self.draw_orders(art, font, frame.hud_update, batch)?;
        }
        self.draw_radar(frame, art, font, batch)?;
        if !results_active && frame.target_selector.is_none() {
            let (left, right) = batch.split_at_mut(COMBO_TEXT);
            self.combos
                .draw(art, &mut left[COMBO_PANEL], &mut right[0])?;
        }
        self.combos.retain_body_positions(frame)?;
        Ok(())
    }

    fn draw_orders(
        &self,
        art: &Art,
        font: &BitmapFont,
        tick: u32,
        batch: &mut [Batch],
    ) -> Result<()> {
        let x = f32::from(self.orders_x);
        let width = text_width(font, &self.orders_text)? as f32;
        panel(
            &mut batch[ORDERS_PANEL],
            [x, 348., width + 20., 14.],
            [4.; 4],
            [art.orders.panel_colors[usize::from(self.orders_kind != 0)]; 4],
            0.,
        );
        results::dol_text(
            &mut batch[ORDERS_TEXT],
            font,
            &self.orders_text,
            [x + 23., 335.],
            [20., 26.],
            20.,
            0.,
            art.orders.shadow,
        )?;
        results::dol_text(
            &mut batch[ORDERS_TEXT],
            font,
            &self.orders_text,
            [x + 22., 334.],
            [20., 26.],
            20.,
            0.,
            [128, 128, 128, 255],
        )?;
        if tick & 8 == 0 {
            for (excluded, rect, uv) in [
                (
                    0,
                    [x + (width as i32 >> 1) as f32, 360., 32., 20.],
                    [224., 64., 24., -16.],
                ),
                (
                    2,
                    [x + (width as i32 >> 1) as f32, 316., 32., 20.],
                    [224., 48., 24., 16.],
                ),
                (1, [x, 334., 20., 28.], [208., 48., -16., 24.]),
                (3, [x + width, 334., 20., 28.], [192., 48., 16., 24.]),
            ] {
                if self.orders_kind != excluded {
                    sprite(&mut batch[ORDERS_ARROWS], rect, uv, [128, 128, 128, 255]);
                }
            }
        }
        Ok(())
    }

    fn draw_radar(
        &self,
        frame: &BattleFrame,
        art: &Art,
        font: &BitmapFont,
        batch: &mut [Batch],
    ) -> Result<()> {
        let lead = frame
            .actors
            .iter()
            .position(|actor| {
                actor.side == Side::Party && actor.control != resonance_battle::Control::Auto
            })
            .or_else(|| {
                frame
                    .actors
                    .iter()
                    .position(|actor| actor.side == Side::Party)
            });
        let target = lead
            .and_then(|lead| frame.targets[lead])
            .map(|actor| actor.index());
        let palette = if self.group_count == 1 {
            1
        } else if self.group_count <= 2 {
            0
        } else if self.enemies.len() <= 4 {
            2
        } else {
            1
        };
        let step = if palette == 2 {
            4
        } else {
            self.enemies.len().div_ceil(2)
        };
        let mut ordinals = art.radar.initial_ordinals;
        for (index, enemy) in self.enemies.iter().enumerate() {
            let actor = &frame.actors[enemy.actor];
            let group = usize::from(enemy.group);
            let ordinal = ordinals[group];
            ordinals[group] += 1;
            let [x, y] = radar_position(palette, step, index, group, ordinal);
            let bounce = actor.hud.portrait_bounce;
            let y = y - i32::from(bounce * (bounce & 1));
            if !actor.available() {
                continue;
            }
            let chanting =
                matches!(actor.activity, Activity::Casting { .. }) && !actor.hud.cast_released;
            let color = if chanting {
                art.radar.pulse_amplitude.mul_add(
                    art.sine[(frame.hud_update.wrapping_mul(32) % 360) as usize],
                    art.radar.shade_center,
                ) as i32 as u8
            } else {
                128
            };
            let selected = target == Some(enemy.actor);
            let size = if selected {
                (art.radar.target_size_center
                    + f64::from(
                        art.radar.target_size_amplitude
                            * art.sine[(frame.hud_update.wrapping_mul(6) % 360) as usize],
                    )) as i32
            } else {
                0
            };
            let half = size >> 1;
            let layer =
                self.icon_layers[group].context("enemy HUD group has no icon layer")? - self.start;
            sprite(
                &mut batch[layer],
                [
                    (x - half) as f32,
                    (y - half) as f32,
                    (32 + size) as f32,
                    (32 + size) as f32,
                ],
                [0., 0., 32., 32.],
                [color, color, color, 255],
            );
            if selected {
                rotate_last(
                    &mut batch[layer],
                    self.radar_rotation as f32,
                    0.,
                    &art.radar,
                );
            }
            ordinal_number(
                &mut batch[RADAR_NUMBERS],
                art,
                ordinal + 1,
                [x + 34, y + 20],
            );
            if chanting {
                let angle = i32::from(self.radar_phases[index]) * 12;
                let size = art.radar.effect_size_amplitude.mul_add(
                    art.sine[(angle % 360) as usize],
                    art.radar.effect_size_center,
                );
                let inset = (32. - size) * 0.5;
                let rect = [
                    (x as f32 + inset).trunc(),
                    (y as f32 + inset).trunc(),
                    size.trunc(),
                    size.trunc(),
                ];
                sprite(
                    &mut batch[RADAR_EFFECT],
                    rect,
                    art.radar.effect.rect.map(|v| v as f32),
                    [96, 96, 128, 192],
                );
                rotate_last(&mut batch[RADAR_EFFECT], 0., angle as f32, &art.radar);
            }
            if selected
                && lead.is_some_and(|lead| {
                    frame.actors[lead].hud.target_highlight != 0 || frame.target_selector.is_some()
                })
            {
                let y = y - 8 - half;
                sprite(
                    &mut batch[RADAR_POINTER],
                    [(x + 4) as f32, (y - 16) as f32, 28., 36.],
                    [0., 256., 48., 64.],
                    [0, 0, 0, 128],
                );
                sprite(
                    &mut batch[RADAR_POINTER],
                    [(x + 2) as f32, (y - 18) as f32, 28., 36.],
                    [0., 256., 48., 64.],
                    [128, 128, 128, 255],
                );
                let slot = usize::from(frame.actors[lead.unwrap()].hud.control_slot);
                sprite(
                    &mut batch[RADAR_CONTROL + slot],
                    [(x + 2) as f32, (y - 18) as f32, 28., 36.],
                    [48., 256., 48., 64.],
                    [128, 128, 128, 255],
                );
            }
        }
        if self.radar_x < 656 {
            let mut y = 354.;
            for (group, name) in self.groups.iter().enumerate() {
                let Some(name) = name else {
                    continue;
                };
                let mut color = [128, 128, 128, 255];
                if frame.target_selector.is_some()
                    && target.is_some_and(|target| {
                        self.enemies
                            .iter()
                            .any(|enemy| enemy.actor == target && usize::from(enemy.group) == group)
                    })
                {
                    let wave = art.sine[(frame.hud_update.wrapping_mul(8) % 360) as usize];
                    let shade = art
                        .radar
                        .pulse_amplitude
                        .mul_add(wave, art.radar.shade_center)
                        as i32 as u8;
                    color = [
                        shade,
                        shade,
                        (-art.radar.selection_color_amplitude)
                            .mul_add(wave, art.radar.selection_blue_center)
                            as i32 as u8,
                        255,
                    ];
                }
                let x = f32::from(self.radar_x);
                panel(
                    &mut batch[RADAR_PANEL],
                    [x - 8., y, 256., 12.],
                    [3.; 4],
                    art.radar.panel_colors,
                    4.,
                );
                results::dol_text(
                    &mut batch[RADAR_TEXT],
                    font,
                    name,
                    [x + 9., y - 9.],
                    [18., 22.],
                    18.,
                    0.,
                    art.radar.shadow,
                )?;
                results::dol_text(
                    &mut batch[RADAR_TEXT],
                    font,
                    name,
                    [x + 8., y - 10.],
                    [18., 22.],
                    18.,
                    0.,
                    color,
                )?;
                y -= 24.;
            }
        }
        Ok(())
    }
}

fn radar_position(palette: u8, step: usize, index: usize, group: usize, ordinal: u8) -> [i32; 2] {
    let (column, row) = if palette == 0 {
        (usize::from(ordinal), group)
    } else {
        (index % step, index / step)
    };
    [464 + column as i32 * 36, 388 + row as i32 * 36]
}

fn ordinal_number(batch: &mut Batch, art: &Art, value: u8, [mut x, y]: [i32; 2]) {
    let [u, v, w, h] = art.recovery.rect.map(f32::from);
    let mut value = u32::from(value);
    loop {
        x -= 12;
        quad(
            batch,
            [x as f32, y as f32, (x + 12) as f32, (y + 12) as f32],
            [
                u + (value % 10) as f32 * w,
                v,
                u + (value % 10 + 1) as f32 * w,
                v + h,
            ],
            2.,
            [art.radar.number_color; 4],
        );
        value /= 10;
        if value == 0 {
            break;
        }
    }
}

fn text_width(font: &BitmapFont, text: &str) -> Result<i32> {
    text.chars()
        .map(|character| {
            font.glyphs
                .get(&character)
                .map(|g| g.advance as i32)
                .context("missing HUD font glyph")
        })
        .sum()
}

fn sprite(batch: &mut Batch, [x, y, w, h]: [f32; 4], [u, v, tw, th]: [f32; 4], color: [u8; 4]) {
    quad(
        batch,
        [x, y, x + w, y + h],
        [u, v, u + tw, v + th],
        0.,
        [color; 4],
    );
}

/// 6553C's four independent feathered edges, in GX strip corner order.
fn panel(
    batch: &mut Batch,
    [x, y, w, h]: [f32; 4],
    [left, top, right, bottom]: [f32; 4],
    colors: [[u8; 4]; 4],
    skew: f32,
) {
    quad(batch, [x, y, x + w, y + h], [0.5; 4], skew, colors);
    let transparent = |mut color: [u8; 4]| {
        color[3] = 0;
        color
    };
    for (enabled, rect, color) in [
        (
            top,
            [x + skew, y - top, x + skew + w, y],
            [
                transparent(colors[0]),
                transparent(colors[1]),
                colors[0],
                colors[1],
            ],
        ),
        (
            bottom,
            [x - skew, y + h, x - skew + w, y + h + bottom],
            [
                colors[2],
                colors[3],
                transparent(colors[2]),
                transparent(colors[3]),
            ],
        ),
        (
            left,
            [x - left, y, x, y + h],
            [
                transparent(colors[0]),
                colors[0],
                transparent(colors[2]),
                colors[2],
            ],
        ),
        (
            right,
            [x + w, y, x + w + right, y + h],
            [
                colors[1],
                transparent(colors[1]),
                colors[3],
                transparent(colors[3]),
            ],
        ),
    ] {
        if enabled != 0. {
            quad(batch, rect, [0.5; 4], skew, color);
        }
    }
}

fn rotate_last(
    batch: &mut Batch,
    y_degrees: f32,
    z_degrees: f32,
    art: &resonance_content::battle_ui::Radar,
) {
    let start = batch.positions.len() - 4;
    let points = &mut batch.positions[start..];
    let center = [
        (points[0][0] + points[2][0]) * 0.5,
        (points[0][1] + points[2][1]) * 0.5,
    ];
    // 49FF4: Ry * Rz, original fixed UI Z=-0.5; result Z is restored.
    let radians = art.radians_per_degree;
    let (sy, cy) = (f64::from(y_degrees * radians)).sin_cos();
    let (sz, cz) = (f64::from(z_degrees * radians)).sin_cos();
    for point in points {
        let x = point[0] - center[0];
        let y = -(point[1] - center[1]);
        let rotated_x = (cz as f32).mul_add(x, -(sz as f32) * y);
        let rotated_y = (sz as f32).mul_add(x, (cz as f32) * y);
        point[0] = (cy as f32).mul_add(rotated_x, art.depth * sy as f32) + center[0];
        point[1] = center[1] - rotated_y;
    }
}

impl Artwork {
    pub fn combat_cues(&mut self, frame: &BattleFrame) -> Result<()> {
        self.combat.combos.emitted(frame)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn notice_request(
        &mut self,
        actor: usize,
        text: &str,
        duration: u16,
        kind: u8,
        frame: &BattleFrame,
        party_characters: &[u8],
    ) -> Result<()> {
        let actor_state = frame.actors.get(actor).context("unknown notice actor")?;
        let (side, owner) = if actor_state.side == Side::Party {
            let slot = frame.actors[..actor]
                .iter()
                .filter(|actor| actor.side == Side::Party)
                .count();
            (
                0,
                overlays::NoticeOwner::Party(
                    *party_characters
                        .get(slot)
                        .context("notice party character missing")?,
                ),
            )
        } else {
            let enemy = self
                .combat
                .enemies
                .iter()
                .find(|enemy| enemy.actor == actor)
                .context("notice enemy HUD missing")?;
            (1, overlays::NoticeOwner::Enemy(enemy.group))
        };
        ensure!(duration <= i16::MAX as u16, "invalid notice duration");
        self.combat.notices[side] = Some(overlays::Notice::request(
            text,
            Some(owner),
            side as u8,
            kind,
            duration as i16,
            &self.font,
            frame.hud_update,
        )?);
        self.combat.latest_notice = side;
        Ok(())
    }

    pub(super) fn render_combat(
        &mut self,
        frame: &BattleFrame,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let start = self.combat.start;
        let mut batches: Vec<_> = (start..self.surfaces.len())
            .map(|_| Batch::default())
            .collect();
        self.combat.draw(
            frame,
            &self.art,
            &self.font,
            &mut batches,
            self.results.is_some(),
        )?;
        for (row, side) in [self.combat.latest_notice ^ 1, self.combat.latest_notice]
            .into_iter()
            .enumerate()
        {
            // 6EEE4 suppresses 68C90 throughout the native results phase (5).
            if self.results.is_some() {
                continue;
            }
            let Some(notice) = &self.combat.notices[side] else {
                continue;
            };
            let color = if side == 0 {
                self.settings.colors.dialogue
            } else {
                self.settings.colors.choice
            };
            let draw = notice.draw_row(
                &self.art,
                &self.font,
                color,
                12. + row as f32 * 12.,
                row == 0,
            )?;
            let base = self.combat.notice_start + row * 9;
            let offset = base - start;
            batches[offset] = draw.shadow;
            if let Some((owner, batch)) = draw.owner {
                let material = match owner {
                    overlays::NoticeOwner::Party(character) => {
                        super::ICONS + usize::from(character - 1)
                    }
                    overlays::NoticeOwner::Enemy(group) => self.combat.icon_layers
                        [usize::from(group)]
                    .context("notice enemy icon missing")?,
                };
                self.sizes[base + 1] = self.sizes[material];
                commands
                    .entity(self.layers[base + 1].entity)
                    .insert(MeshMaterial2d(self.surfaces[material].clone()));
                batches[offset + 1] = batch;
            }
            batches[offset + 2] = draw.background;
            batches[offset + 3] = draw.text;
            batches[offset + 4] = draw.generic_icon;
            if let Some((kind, batch)) = draw.symbol {
                batches[offset + 5 + kind] = batch;
            }
        }
        for (offset, batch) in batches.into_iter().enumerate() {
            let index = start + offset;
            let visible = !batch.indices.is_empty();
            self.layers[index].update_mesh(batch, self.sizes[index], meshes)?;
            self.layers[index].show(visible, commands);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_intro_rows_stagger_four_visits_and_slide_after_age_119() {
        let mut rows = [IntroRow::new(0), IntroRow::new(1)];
        for _ in 0..120 {
            for row in &mut rows {
                row.advance();
            }
        }
        assert_eq!((rows[0].x, rows[0].alpha, rows[0].age), (320, 160, 120));
        assert_eq!((rows[1].x, rows[1].y, rows[1].age), (320, 36, 116));
        for row in &mut rows {
            row.advance();
        }
        assert_eq!((rows[0].x, rows[0].alpha), (348, 156));
        assert_eq!((rows[1].x, rows[1].alpha), (320, 160));
        for _ in 0..39 {
            rows[0].advance();
        }
        assert_eq!((rows[0].x, rows[0].alpha, rows[0].age), (1440, 0, 160));
    }

    #[test]
    fn radar_original_instructions_put_group_ordinals_on_horizontal_axis() {
        // 6A660..6A6C8, not the transposed coordinates in partial candidate C.
        assert_eq!(radar_position(0, 3, 0, 1, 0), [464, 424]);
        assert_eq!(radar_position(0, 3, 4, 0, 2), [536, 388]);
        assert_eq!(radar_position(1, 3, 4, 0, 0), [500, 424]);
        assert_eq!(radar_position(2, 4, 3, 3, 0), [572, 388]);
    }

    #[test]
    fn panel_feathers_follow_each_original_corner_and_skew() {
        let mut batch = Batch::default();
        panel(
            &mut batch,
            [100., 50., 120., 10.],
            [3.; 4],
            [[64, 32, 16, 128]; 4],
            5.,
        );
        assert_eq!(batch.positions.len(), 20);
        // Main strip top is sheared +5; its top feather starts at x+5,
        // then applies that same shear again at the feather's upper edge.
        assert_eq!(batch.positions[0], [-215., 190., 0.]);
        assert_eq!(batch.positions[4], [-210., 193., 0.]);
        assert_eq!(batch.colors[4][3], 0.);
        assert_eq!(batch.colors[6][3], 128. / 255.);
        assert_eq!(batch.colors[8][3], 128. / 255.);
        assert_eq!(batch.colors[10][3], 0.);
    }
}
