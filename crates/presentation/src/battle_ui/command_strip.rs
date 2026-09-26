//! Top-level command strip: original 63F8..8054 and 8594..8614.
//! The actor-selection branches (state 2/3) are deliberately separate.
use super::*;
use resonance_content::battle_ui::CommandArt;
use resonance_game::battle::command::{Cursor, Frame};

const PER_COMMAND: usize = 7;
const PLATE_SHADOW: usize = 42;
const PLATE: usize = 43;
const TEXT: usize = 44;
const CURSOR_TRAIL: usize = 45;
const CURSOR_SHADOW: usize = 46;
const CURSOR: usize = 47;
const PLAYER: usize = 48;
const COUNT: usize = 49;

pub(super) struct Strip {
    start: usize,
    selected: u8,
}

impl Strip {
    pub fn load(
        art: &Art,
        font: &BitmapFont,
        images: &mut Vec<Handle<Image>>,
        sizes: &mut Vec<[u32; 2]>,
        load: impl Fn(String, bool) -> Handle<Image>,
    ) -> Self {
        let start = images.len();
        let main_font = images[DOL_FONT].clone();
        let battle_font = images[FONT].clone();
        let a = &art.commands;
        // Every selected/nonselected icon, plate and pointer is present at warmup.
        for icons in &a.icons {
            for image in std::iter::once(&a.shadow)
                .chain(std::iter::once(&a.background))
                .chain((0..4).map(|i| icons.get(i).unwrap_or(&a.background)))
                .chain(std::iter::once(&a.disabled))
            {
                images.push(load(image.path.clone(), false));
                sizes.push([image.width, image.height]);
            }
        }
        for image in [&a.shadow, &a.plate] {
            images.push(load(image.path.clone(), false));
            sizes.push([image.width, image.height]);
        }
        images.push(main_font);
        sizes.push([font.width, font.height]);
        for image in [&a.cursor, &a.cursor_shadow, &a.cursor] {
            images.push(load(image.path.clone(), false));
            sizes.push([image.width, image.height]);
        }
        images.push(battle_font);
        sizes.push([art.font.width, art.font.height]);
        Self { start, selected: 0 }
    }

    fn select(&mut self, selected: u8, layers: &[Layer], commands: &mut Commands) {
        if self.selected == selected {
            return;
        }
        for (index, layer) in layers.iter().enumerate().skip(self.start).take(COUNT) {
            let depth = self.depth(index, usize::from(selected)).unwrap();
            if self.depth(index, usize::from(self.selected)) == Some(depth) {
                continue;
            }
            let transform = Transform::from_xyz(0., 0., depth);
            // These HUD entities have no parent. Battle rendering occurs after
            // transform propagation: publish both samples for this draw.
            // Bevy 0.19 retains Transparent2d sort keys and only requeues mesh/
            // material changes. Mark this same handle changed, without touching
            // mesh data, so the new depth reaches that queue on this visit.
            commands.entity(layer.entity).insert((
                transform,
                GlobalTransform::from(transform),
                Mesh2d(layer.mesh.clone()),
            ));
        }
        self.selected = selected;
    }

    pub fn depth(&self, index: usize, selected: usize) -> Option<f32> {
        let i = index.checked_sub(self.start).filter(|i| *i < COUNT)?;
        let order = if i < PLATE_SHADOW {
            let command = i / PER_COMMAND;
            let rank = if command == selected {
                5
            } else {
                command - usize::from(command > selected)
            };
            let part = i % PER_COMMAND;
            if part < 2 {
                rank * 2 + part
            } else {
                12 + rank * 5 + part - 2
            }
        } else {
            i
        };
        // Whole command callback is drawn after ordinary HUD, before transition.
        Some(210. + order as f32 * 0.125)
    }
}

impl Artwork {
    pub fn render_commands(
        &mut self,
        state: Option<&Frame>,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        ensure!(
            self.layers.len() >= self.command_strip.start + COUNT
                && self.sizes.len() >= self.command_strip.start + COUNT,
            "battle command strip was not prepared"
        );
        let Some(state) = state else {
            for layer in
                &mut self.layers[self.command_strip.start..self.command_strip.start + COUNT]
            {
                layer.show(false, commands);
            }
            return Ok(());
        };
        let mut batches: [Batch; COUNT] = std::array::from_fn(|_| Batch::default());
        draw(state, tick, &self.art, &self.font, &mut batches)?;
        self.command_strip
            .select(state.selected, &self.layers, commands);
        for (i, batch) in batches.into_iter().enumerate() {
            let index = self.command_strip.start + i;
            let layer = &mut self.layers[index];
            layer.update_mesh(batch, self.sizes[index], meshes)?;
            layer.show(true, commands);
        }
        Ok(())
    }
}

fn draw(
    state: &Frame,
    tick: u32,
    art: &Art,
    font: &BitmapFont,
    batches: &mut [Batch; COUNT],
) -> Result<()> {
    ensure!(
        state.selected < 6 && state.controller < 4,
        "invalid command strip state"
    );
    let a = &art.commands;
    let sine = |phase: i32| art.sine[phase.rem_euclid(360) as usize];
    let selected_color = a.motion.selected_shade_amplitude.mul_add(
        sine((tick.wrapping_mul(16) % 360) as i32),
        a.motion.selected_shade_center,
    ) as u8;
    for command in 0..6 {
        let selected = command == usize::from(state.selected);
        let enabled = state.enabled & (1 << command) != 0;
        let inset = if selected { 8 } else { 0 };
        let x = 152 + 56 * command as i32 - inset;
        let y = 128 - inset;
        let size = 48 + 2 * inset;
        let base = PER_COMMAND * command;
        sprite(
            &mut batches[base],
            [x, y, size + 4, size + 4],
            [0, 160, 48, 48],
            a.shadow_color,
        );
        sprite(
            &mut batches[base + 1],
            [x, y, size, size],
            [0, 160, 48, 48],
            [128, 128, 128, 255],
        );
        let phase = if selected && enabled {
            state.animation
        } else {
            0
        };
        let color = if selected { selected_color } else { 128 };
        for layer in 0..a.icons[command].len() {
            let mut point = [x, y];
            let mut transform = None;
            let uv = match command {
                0 => {
                    if selected && layer != 0 {
                        let angle = phase
                            .wrapping_mul(6)
                            .wrapping_add(if layer == 1 { 180 } else { 0 })
                            .rem_euclid(360);
                        point[1] += (a.motion.bob_amplitude * sine(angle)) as i32;
                    }
                    if selected && layer == 0 {
                        transform = Some((phase as f32 * 8., [0, 0]));
                    }
                    [49 + 48 * layer as i32, 161, 46, 46]
                }
                1 => {
                    if selected && layer < 2 {
                        point[1] -= (a.motion.bob_amplitude
                            * sine(phase.wrapping_mul(6).rem_euclid(180)))
                            as i32;
                    }
                    let frame = if selected && layer == 2 {
                        ((tick / 6) % 3) as i32 * 48
                    } else {
                        0
                    };
                    [97 + 48 * layer as i32 + frame, 321, 46, 46]
                }
                2 => {
                    if layer == 3 && (!selected || ((phase + 30) / 18) & 1 != 0) {
                        continue;
                    }
                    if selected && layer == 2 {
                        point[1] += (a.motion.small_bob_amplitude
                            * sine(phase.wrapping_mul(8).rem_euclid(360)))
                            as i32;
                    }
                    if selected && layer == 0 {
                        point[0] -= 5;
                        transform = Some((
                            a.motion.strategy_rotation_amplitude
                                * sine(phase.wrapping_mul(10).rem_euclid(360)),
                            [2, 2],
                        ));
                    }
                    [193 + 48 * layer as i32, 161, 46, 46]
                }
                3 => {
                    if selected {
                        let angle = phase
                            .wrapping_mul(6)
                            .wrapping_add(if layer == 1 { 90 } else { 0 })
                            .rem_euclid(180);
                        point[1] += (a.motion.lift_amplitude * sine(angle)) as i32;
                    }
                    [385 + 48 * layer as i32, 161, 46, 46]
                }
                4 => {
                    if selected {
                        match layer {
                            0 => {
                                point[1] += (a.motion.lift_amplitude
                                    * sine(phase.wrapping_mul(6).rem_euclid(180)))
                                    as i32
                            }
                            1 => {
                                point[0] += 12;
                                point[1] += 24;
                                transform = Some((
                                    a.motion.item_rotation_amplitude
                                        * sine(phase.wrapping_mul(5).rem_euclid(360)),
                                    [-14, -24],
                                ));
                            }
                            2 => {
                                point[0] -= 10;
                                point[1] += 16
                                    + (a.motion.small_bob_amplitude
                                        * sine(phase.wrapping_mul(3).rem_euclid(360)))
                                        as i32;
                                transform = Some((
                                    a.motion.item_sway_amplitude
                                        * sine(phase.wrapping_mul(6).rem_euclid(360)),
                                    [8, -16],
                                ));
                            }
                            _ => unreachable!(),
                        }
                    }
                    [1 + 48 * layer as i32, 209, 46, 46]
                }
                5 => {
                    point[1] -= 8;
                    let angle = if selected {
                        phase
                            .wrapping_mul(8)
                            .wrapping_add(layer as i32 * 180)
                            .rem_euclid(360)
                    } else {
                        0
                    };
                    transform = Some((a.motion.escape_rotation_amplitude * sine(angle), [0, 8]));
                    [145, 209, 46, 46]
                }
                _ => unreachable!(),
            };
            let batch = &mut batches[base + 2 + layer];
            sprite(
                batch,
                [point[0], point[1], size, size],
                uv,
                [color, color, color, 255],
            );
            if let Some((z, offset)) = transform {
                transform_last(batch, a.motion.y_rotation, z, offset, &art.radar);
            }
        }
        if !enabled {
            sprite(
                &mut batches[base + 6],
                [x, y, size, size],
                [192, 256, 48, 48],
                [128, 128, 128, 255],
            );
        }
    }
    let text = &a.names[state.selected as usize];
    let width = text
        .chars()
        .map(|ch| {
            font.glyphs
                .get(&ch)
                .map(|g| g.advance as i32)
                .context("missing battle command main-font glyph")
        })
        .sum::<Result<i32>>()?;
    nameplate(
        &mut batches[PLATE_SHADOW],
        state.selected,
        width,
        -4,
        a.shadow_color,
    );
    nameplate(
        &mut batches[PLATE],
        state.selected,
        width,
        0,
        [128, 128, 128, 255],
    );
    let units = width / 24 + 1;
    results::dol_text(
        &mut batches[TEXT],
        font,
        text,
        [
            (176 + i32::from(state.selected) * 56 - (units / 2) * 24 - (units & 1) * 12) as f32,
            (76 - (a.motion.label_bob_amplitude * sine((tick.wrapping_mul(6) % 180) as i32)) as i32)
                as f32,
        ],
        [22., 32.],
        24.,
        0.,
        a.text_color,
    )?;
    cursor(&state.cursor, tick, a, &art.sine, batches);
    let player = a
        .player_format
        .replacen("%d", &(state.controller + 1).to_string(), 1);
    let x = f32::from(state.selected) * 56. + 162.;
    results::glyphs(
        &mut batches[PLAYER],
        art,
        &player,
        [x + 2., 186.],
        [20., 20.],
        2.,
        18.,
        [a.shadow_color; 2],
        None,
    )?;
    results::glyphs(
        &mut batches[PLAYER],
        art,
        &player,
        [x, 184.],
        [20., 20.],
        2.,
        18.,
        a.player_colors,
        None,
    )?;
    Ok(())
}

fn nameplate(batch: &mut Batch, selected: u8, width: i32, offset: i32, color: [u8; 4]) {
    let units = width / 24 + 1;
    let mut x = 136 + i32::from(selected) * 56
        - offset
        - if units > 2 { (units - 2) * 12 } else { 0 }
        - (24 - width % 24) / 2;
    let y = 80 - offset;
    sprite(batch, [x, y, 16, 40], [304, 0, 16, 40], color);
    x += 16;
    let mut i = 0;
    while i < units {
        if units & 1 == 0 && i == units / 2 - 1 {
            for u in [320, 332, 344, 320] {
                sprite(batch, [x, y, 12, 40], [u, 0, 12, 40], color);
                x += 12;
            }
            i += 2;
        } else {
            let middle = units == 3 && i == 1;
            for u in if middle { [332, 344] } else { [320, 320] } {
                sprite(batch, [x, y, 12, 40], [u, 0, 12, 40], color);
                x += 12;
            }
            i += 1;
        }
    }
    sprite(batch, [x, y, 16, 40], [368, 0, 16, 40], color);
}

fn cursor(
    cursor: &Cursor,
    tick: u32,
    art: &CommandArt,
    sine: &[f32],
    batches: &mut [Batch; COUNT],
) {
    cursor_trail(&mut batches[CURSOR_TRAIL], cursor);
    let bounce = (art.cursor_amplitude * sine[(tick.wrapping_mul(8) % 180) as usize]) as i32;
    let x = i32::from(cursor.current[0]) + bounce;
    let y = i32::from(cursor.current[1]) - bounce;
    sprite(
        &mut batches[CURSOR_SHADOW],
        [x + 4, y + 4, 32, 32],
        [160, 72, 32, 32],
        art.shadow_color,
    );
    sprite(
        &mut batches[CURSOR],
        [x, y, 32, 32],
        [160, 72, 32, 32],
        [128, 128, 128, 255],
    );
}

fn cursor_trail(batch: &mut Batch, cursor: &Cursor) {
    let mut point = cursor.current.map(i32::from);
    let step: [i32; 2] = std::array::from_fn(|i| (i32::from(cursor.previous[i]) - point[i]) / 8);
    for alpha in cursor.trail_alpha {
        if alpha != 0 {
            sprite(
                batch,
                [point[0], point[1], 32, 32],
                [160, 72, 32, 32],
                [128, 128, 128, alpha as u8],
            );
            for i in 0..2 {
                point[i] = i32::from((point[i] + step[i]) as i16);
            }
        }
    }
}

fn sprite(batch: &mut Batch, [x, y, w, h]: [i32; 4], [u, v, uw, vh]: [i32; 4], color: [u8; 4]) {
    quad(
        batch,
        [x as f32, y as f32, (x + w) as f32, (y + h) as f32],
        [u as f32, v as f32, (u + uw) as f32, (v + vh) as f32],
        0.,
        [color; 4],
    );
}

fn transform_last(
    batch: &mut Batch,
    y_degrees: f32,
    z_degrees: f32,
    offset: [i16; 2],
    art: &resonance_content::battle_ui::Radar,
) {
    let start = batch.positions.len() - 4;
    let points = &mut batch.positions[start..];
    let center = [
        (points[0][0] + points[2][0]) * 0.5,
        (points[0][1] + points[2][1]) * 0.5,
    ];
    let (sy, cy) = f64::from(y_degrees * art.radians_per_degree).sin_cos();
    let (sz, cz) = f64::from(z_degrees * art.radians_per_degree).sin_cos();
    for point in points {
        // 49FF4 composes Ry*Rz*translation before restoring source Z.
        let x = point[0] - center[0] + f32::from(offset[0]);
        let y = -(point[1] - center[1]) + f32::from(offset[1]);
        let rx = (cz as f32).mul_add(x, -(sz as f32) * y);
        let ry = (sz as f32).mul_add(x, (cz as f32) * y);
        point[0] = (cy as f32).mul_add(rx, art.depth * sy as f32) + center[0];
        point[1] = center[1] - ry;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_cursor_does_not_advance_across_zero_alpha_entries() {
        let mut batch = Batch::default();
        cursor_trail(
            &mut batch,
            &Cursor {
                current: [412, 172],
                previous: [132, 172],
                trail_alpha: [0, 0, 0, 0, 0, 0, 128, 112],
            },
        );
        assert_eq!(batch.positions.len(), 8);
        assert_eq!(batch.positions[0], [412. - 320., 240. - 172., 0.]);
        assert_eq!(batch.positions[4], [377. - 320., 240. - 172., 0.]);
        assert_eq!(batch.colors[0][3], 128. / 255.);
        assert_eq!(batch.colors[4][3], 112. / 255.);
    }

    #[test]
    fn source_even_nameplate_places_the_split_pointer_at_its_center() {
        // 7924..7F5C: width41 -> N2, remainder17; paired central cells.
        let mut batch = Batch::default();
        nameplate(&mut batch, 0, 41, 0, [128, 128, 128, 255]);
        assert_eq!(batch.positions.len(), 24);
        assert_eq!(batch.positions[0], [133. - 320., 240. - 80., 0.]);
        let u: Vec<_> = batch.uv.chunks_exact(4).map(|q| q[0][0]).collect();
        assert_eq!(u, [304., 320., 332., 344., 320., 368.]);
        assert_eq!(batch.positions[22][0], 213. - 320.);
    }

    #[test]
    fn source_three_cell_nameplate_uses_one_split_center_and_offset_shadow() {
        let mut batch = Batch::default();
        nameplate(&mut batch, 2, 61, -4, [0, 0, 0, 128]);
        assert_eq!(batch.positions[0], [235. - 320., 240. - 84., 0.]);
        let u: Vec<_> = batch.uv.chunks_exact(4).map(|q| q[0][0]).collect();
        assert_eq!(u, [304., 320., 320., 332., 344., 320., 320., 368.]);
    }

    #[test]
    fn selection_requeues_changed_depths_with_same_mesh_and_immediate_world_transform() {
        let mut world = World::new();
        let mut strip = Strip {
            start: 0,
            selected: 0,
        };
        let layers: Vec<_> = (0..COUNT)
            .map(|index| {
                let transform = Transform::from_xyz(0., 0., strip.depth(index, 0).unwrap());
                let mesh = Handle::<Mesh>::default();
                let entity = world
                    .spawn((
                        Mesh2d(mesh.clone()),
                        transform,
                        GlobalTransform::from(transform),
                    ))
                    .id();
                Layer {
                    entity,
                    mesh,
                    material: Handle::default(),
                    uploaded: None,
                    visible: false,
                }
            })
            .collect();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        // Includes unchanged frames, disabled U. Attack/Escape, a later
        // selection, and reopening at zero. No transform propagation occurs
        // between select() and the simulated material queue inspection.
        for selected in [0, 1, 1, 5, 5, 2, 0] {
            let previous = strip.selected;
            world.clear_trackers();
            strip.select(selected, &layers, &mut Commands::new(&mut queue, &world));
            queue.apply(&mut world);
            let changed: Vec<_> = world
                .query_filtered::<Entity, Changed<Mesh2d>>()
                .iter(&world)
                .collect();
            for (index, layer) in layers.iter().enumerate() {
                let depth = strip.depth(index, usize::from(selected)).unwrap();
                let moved = strip.depth(index, usize::from(previous)) != Some(depth);
                assert_eq!(changed.contains(&layer.entity), moved);
                assert_eq!(world.get::<Mesh2d>(layer.entity).unwrap().0, layer.mesh);
                assert_eq!(
                    world.get::<Transform>(layer.entity).unwrap().translation.z,
                    depth
                );
                assert_eq!(
                    world
                        .get::<GlobalTransform>(layer.entity)
                        .unwrap()
                        .translation()
                        .z,
                    depth
                );
                assert!(world.get::<ChildOf>(layer.entity).is_none());
            }
            for command in [1, 5] {
                let base = command * PER_COMMAND;
                let disabled_depth = world
                    .get::<GlobalTransform>(layers[base + 6].entity)
                    .unwrap()
                    .translation()
                    .z;
                for icon in 2..6 {
                    assert!(
                        world
                            .get::<GlobalTransform>(layers[base + icon].entity)
                            .unwrap()
                            .translation()
                            .z
                            < disabled_depth
                    );
                }
            }
        }
    }

    #[test]
    fn selected_command_draws_last_in_each_original_pass() {
        let strip = Strip {
            start: 100,
            selected: 0,
        };
        for selected in 0..6 {
            let selected_base = 100 + selected * PER_COMMAND;
            for other in (0..6).filter(|i| *i != selected) {
                assert!(
                    strip.depth(100 + other * PER_COMMAND + 1, selected)
                        < strip.depth(selected_base, selected)
                );
                assert!(
                    strip.depth(100 + other * PER_COMMAND + 6, selected)
                        < strip.depth(selected_base + 2, selected)
                );
            }
            assert!(strip.depth(selected_base + 1, selected) < strip.depth(102, selected));
            assert!(
                strip.depth(selected_base + 6, selected)
                    < strip.depth(100 + PLATE_SHADOW, selected)
            );
        }
    }
}
