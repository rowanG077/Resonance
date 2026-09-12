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
        for (index, material) in [materials.add(atlas), self.surfaces[9].clone()]
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
                    Transform::from_xyz(0., 0., 3. + index as f32 * 0.01),
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
        if session.active_skit.is_some() || session.menu.is_some() || session.shop.is_some() {
            return Ok(());
        }
        let mut batches = [Batch::default(), Batch::default()];
        let button_highlight = session.effect_clock.tick() & 32 != 0;
        if let Some(prompt) = session.action_prompt() {
            let glyphs = self
                .menu
                .action_label(prompt.action)
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
            let v = if button_highlight { 152. } else { 176. };
            batches[0].quad(
                [left - 26., 432., left - 2., 456.],
                [192., v, 216., v + 24.],
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
        if let Some(prompt) = session.skit_prompt() {
            let v = if button_highlight { 49. } else { 73. };
            batches[0].quad(
                [16., 432., 40., 456.],
                [233., v, 255., v + 22.],
                [1., 1., 1., f32::from(prompt.opacity) / 255.],
            );
            if prompt.title_visible {
                let mut x = 56.;
                for c in prompt.title.chars() {
                    let glyph = self
                        .font
                        .glyphs
                        .get(&c)
                        .with_context(|| format!("uncooked skit title glyph {c:?}"))?;
                    let [u, v, w, h] = glyph.rect.map(|v| v as f32);
                    batches[1].quad(
                        [x, 432., x + 20., 456.],
                        [u, v, u + w * 255. / 256., v + h * 255. / 256.],
                        [1., 1., 1., f32::from(prompt.text_opacity) / 255.],
                    );
                    x = (x + glyph.advance as f32 * (5. / 6.)).trunc() + f32::from(c.is_ascii());
                }
            }
        }
        let texture = &self.spec.textures[0];
        for ((layer, batch), size) in self.prompt_layers.iter_mut().zip(batches).zip([
            [texture.width, texture.height],
            [self.font.width, self.font.height],
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
