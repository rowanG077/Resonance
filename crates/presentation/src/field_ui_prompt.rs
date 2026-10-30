//! Field action hints reuse the prepared system atlas and bitmap font.
use super::*;

impl Artwork {
    pub(super) fn prepare_prompt(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<Surface>,
    ) {
        if !self.prompt_layers.is_empty() {
            return;
        }
        // The same atlas supplies sharp dialogue slices and filtered field hints.
        let mut atlas = materials.get(&self.surfaces[0]).unwrap().clone();
        atlas.sampling = self.images[9].clone();
        for (index, material) in [
            materials.add(atlas),
            self.surfaces[9].clone(),
            self.menu.input_material(),
        ]
        .into_iter()
        .enumerate()
        {
            let mut batch = Batch::default();
            batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
            let mesh = meshes.add(batch.mesh([1, 1]));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(
                        0.,
                        0.,
                        3. + if index == 1 {
                            0.03
                        } else {
                            index as f32 * 0.01
                        },
                    ),
                    Visibility::Hidden,
                ))
                .id();
            self.prompt_layers.push(Layer {
                entity,
                mesh,
                material,
                uploaded: None,
                visible: false,
            });
        }
    }

    pub(super) fn render_prompt(
        &mut self,
        session: &FieldSession,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        // Menus and skits retain the last field presentation, including its
        // notifications. Their input remains owned by the modal scene.
        if session.active_skit.is_some() || session.menu_is_open() {
            return Ok(());
        }
        let mut batches = [Batch::default(), Batch::default(), Batch::default()];
        if session.allow_incomplete_scripts
            && session
                .events
                .world
                .party
                .as_ref()
                .is_some_and(|p| p.travel.overworld.is_some())
        {
            let hint = if session.events.exploration_error.is_some() {
                "Exploration preview - Home / Start: Overworld"
            } else {
                "Home / Start: Return to overworld"
            };
            skit::centered(&mut batches[1], &self.font, hint, 16., 15., 0.9, true)?;
        }
        if let Some(prompt) = session.action_prompt() {
            let glyphs = self
                .menu
                .action_label(prompt.action)?
                .chars()
                .map(|c| {
                    self.font
                        .glyphs
                        .get(&c)
                        .with_context(|| format!("uncooked action prompt glyph {c:?}"))
                })
                .collect::<Result<Vec<_>>>()?;
            let width = glyphs.iter().map(|g| g.advance).sum::<u32>() as f32;
            let left = 608. - width;
            let right = (left + width * (f32::from(prompt.opacity) * 3. / 255.).min(1.)).trunc();
            let color = [1., 1., 1., f32::from(prompt.opacity) / 255.];
            batches[0].quad(
                [left - 12., 440., right, 464.],
                [208., 200., 216., 224.],
                color,
            );
            batches[0].quad(
                [right, 440., right + 16., 464.],
                [224., 200., 240., 224.],
                color,
            );
            let map = session
                .events
                .world
                .party
                .as_ref()
                .map(|p| p.settings.preferences.keymap.clone())
                .unwrap_or_default();
            let [u, v, w, h] = crate::input_icons::action_rect_state(
                self.menu.input_atlas(),
                &map,
                resonance_content::input::StandardAction::Confirm,
                session.effect_clock.tick() & 32 != 0,
                self.menu.prompt_device(),
            )
            .map(|v| v as f32);
            batches[2].quad(
                [left - 30., 428., left + 2., 460.],
                [u, v, u + w, v + h],
                color,
            );
            let mut x = left;
            for glyph in glyphs {
                let [u, v, w, h] = glyph.rect.map(|v| v as f32);
                batches[1].quad(
                    [x, 432., x + 24., 456.],
                    [u, v, u + w * 255. / 256., v + h * 255. / 256.],
                    [1., 1., 1., f32::from(prompt.text_opacity) / 255.],
                );
                x += glyph.advance as f32;
            }
        }
        draw_skit_prompt(
            &mut batches,
            &self.font,
            self.menu.input_atlas(),
            session.skit_prompt(),
            &session
                .events
                .world
                .party
                .as_ref()
                .map(|p| p.settings.preferences.keymap.clone())
                .unwrap_or_default(),
            session.effect_clock.tick() & 32 != 0,
            self.menu.prompt_device(),
        )?;
        self.upload_prompts(batches, commands, meshes)
    }
    pub(super) fn render_world_notifications(
        &mut self,
        session: &resonance_game::overworld::Session,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let mut batches = [Batch::default(), Batch::default(), Batch::default()];
        let prompt = if session.active_skit.is_none()
            && session.menu.is_none()
            && session.prompt().is_none()
        {
            session.skit_prompt()
        } else {
            None
        };
        draw_skit_prompt(
            &mut batches,
            &self.font,
            self.menu.input_atlas(),
            prompt,
            &session
                .events
                .world
                .party
                .as_ref()
                .map(|p| p.settings.preferences.keymap.clone())
                .unwrap_or_default(),
            session.events.tick() & 32 != 0,
            self.menu.prompt_device(),
        )?;
        self.upload_prompts(batches, commands, meshes)
    }
    fn upload_prompts(
        &mut self,
        batches: [Batch; 3],
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let texture = &self.spec.textures[0];
        for ((layer, batch), size) in self.prompt_layers.iter_mut().zip(batches).zip([
            [texture.width, texture.height],
            [self.font.width, self.font.height],
            self.menu.input_atlas().size,
        ]) {
            let visible = !batch.indices.is_empty();
            if visible {
                layer.update_mesh(batch, size, meshes)?;
            }
            layer.show(visible, commands);
        }
        Ok(())
    }
}

fn draw_skit_prompt(
    batches: &mut [Batch; 3],
    font: &BitmapFont,
    atlas: &crate::input_icons::Atlas,
    prompt: Option<resonance_game::field::SkitPrompt<'_>>,
    keymap: &resonance_content::input::Keymap,
    pressed: bool,
    device: crate::input_icons::PromptDevice,
) -> Result<()> {
    if let Some(prompt) = prompt {
        let [u, v, w, h] = crate::input_icons::action_rect_state(
            atlas,
            keymap,
            resonance_content::input::StandardAction::Skit,
            pressed,
            device,
        )
        .map(|v| v as f32);
        batches[2].quad(
            [12., 428., 44., 460.],
            [u, v, u + w, v + h],
            [1., 1., 1., f32::from(prompt.opacity) / 255.],
        );
        if let Some(input) = crate::input_icons::action_input(
            keymap,
            resonance_content::input::StandardAction::Skit,
            device,
        ) && !crate::input_icons::has_specific_icon(atlas, input)
        {
            let label = crate::input_icons::short_label(input);
            let width = (24. / label.len().max(1) as f32).min(10.);
            let mut x = 16.;
            for character in label.chars() {
                if let Some(glyph) = font.glyphs.get(&character) {
                    let [u, v, w, h] = glyph.rect.map(|v| v as f32);
                    batches[1].quad(
                        [x, 439., x + width, 451.],
                        [u, v, u + w, v + h],
                        [
                            if pressed { 0. } else { 1. },
                            if pressed { 0. } else { 1. },
                            if pressed { 0. } else { 1. },
                            f32::from(prompt.opacity) / 255.,
                        ],
                    );
                    x += width;
                }
            }
        }
        if prompt.title_visible {
            let mut x = 56.;
            for c in prompt.title.chars() {
                let glyph = font
                    .glyphs
                    .get(&c)
                    .with_context(|| format!("uncooked skit title glyph {c:?}"))?;
                let [u, v, w, h] = glyph.rect.map(|v| v as f32);
                batches[1].quad(
                    [x, 432., x + 20., 456.],
                    [u, v, u + w * 255. / 256., v + h * 255. / 256.],
                    [1., 1., 1., f32::from(prompt.text_opacity) / 255.],
                );
                x = (x + glyph.advance as f32 * (5. / 6.)).trunc()
                    + f32::from(resonance_content::font::is_single_byte(c));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::input::{Keymap, PhysicalInput, StandardAction};

    #[test]
    fn skit_notification_uses_the_configured_icon_and_modded_atlas_rectangle() -> Result<()> {
        let font = BitmapFont {
            texture: String::new(),
            width: 1,
            height: 1,
            line_height: 24,
            glyphs: Default::default(),
        };
        let mut map = Keymap::default();
        map.assign(StandardAction::Skit, PhysicalInput::Key("F11".into()));
        let mut atlas = crate::input_icons::default_atlas().clone();
        atlas.icons.insert("key_f11".into(), [12, 34, 256, 256]);
        atlas.pressed.insert("key_f11".into(), [512, 768, 256, 256]);
        // Remove controller alternatives so the check is independent of active device preference.
        map.bindings[StandardAction::Skit as usize].retain(PhysicalInput::keyboard);
        for (pressed, [u, v]) in [(false, [12., 34.]), (true, [512., 768.])] {
            let mut batches = [Batch::default(), Batch::default(), Batch::default()];
            draw_skit_prompt(
                &mut batches,
                &font,
                &atlas,
                Some(resonance_game::field::SkitPrompt {
                    id: 1,
                    title: "",
                    opacity: 255,
                    text_opacity: 255,
                    title_visible: false,
                }),
                &map,
                pressed,
                crate::input_icons::PromptDevice::Keyboard,
            )?;
            let mut expected = Batch::default();
            expected.quad([12., 428., 44., 460.], [u, v, u + 256., v + 256.], [1.; 4]);
            assert!(batches[2] == expected);
            assert!(batches[0].indices.is_empty(), "no legacy GameCube glyph");
        }
        Ok(())
    }
}
