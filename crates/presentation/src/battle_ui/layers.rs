use super::*;

/// Back-to-front HUD order. Shared menu planes start in the Menus band.
#[derive(Clone, Copy)]
#[repr(u16)]
pub(super) enum Depth {
    Flash = 0,
    Labels = 10,
    Combo = 20,
    Meters = 30,
    Party = 40,
    Vitals = 50,
    Names = 60,
    Notice = 70,
    Recovery = 80,
    Enemies = 90,
    Selector = 100,
    Scan = 110,
    Commands = 190,
    Menus = 200,
    Results = 310,
    GameOver = 800,
    Transition = 900,
}
impl Depth {
    pub(super) fn value(self) -> f32 {
        self as u16 as f32
    }
}

#[derive(Clone)]
pub(super) struct LayerDefinition {
    pub(super) image: Handle<Image>,
    pub(super) size: [u32; 2],
    pub(super) depth: f32,
    pub(super) nearest: bool,
}
impl LayerDefinition {
    pub(super) fn new(image: Handle<Image>, size: [u32; 2], depth: Depth) -> Self {
        Self {
            image,
            size,
            depth: depth.value(),
            nearest: false,
        }
    }
    pub(super) fn foreground(mut self) -> Self {
        self.depth += 1.;
        self
    }
    pub(super) fn nearest(mut self) -> Self {
        self.nearest = true;
        self
    }
    pub(super) fn at(mut self, depth: Depth) -> Self {
        self.depth = depth.value();
        self
    }
}

pub(super) struct HudLayer {
    pub(super) definition: LayerDefinition,
    pub(super) material: Handle<Surface>,
    pub(super) rendered: Option<Layer>,
}
impl HudLayer {
    pub(super) fn load(
        definition: LayerDefinition,
        nearest: &Handle<Image>,
        materials: &mut Assets<Surface>,
    ) -> Self {
        let image = &definition.image;
        let material = materials.add(Surface {
            source: image.clone(),
            sampling: if definition.nearest {
                nearest.clone()
            } else {
                image.clone()
            },
            frame_mask: image.clone(),
            color_mask: image.clone(),
            coverage: Coverage::default(),
            additive: false,
            red_channel: false,
            opaque: false,
        });
        Self {
            definition,
            material,
            rendered: None,
        }
    }
    pub(super) fn prepare(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if self.rendered.is_some() {
            return;
        }
        let mut batch = Batch::default();
        batch.quad([0., 0., 1., 1.], [0., 0., 1., 1.], [1.; 4]);
        let mesh = meshes.add(batch.mesh([1, 1]));
        let transform = Transform::from_xyz(0., 0., self.definition.depth);
        let entity = commands
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(self.material.clone()),
                transform,
                GlobalTransform::from(transform),
                Visibility::Hidden,
            ))
            .id();
        self.rendered = Some(Layer {
            entity,
            mesh,
            material: self.material.clone(),
            uploaded: None,
            visible: false,
        });
    }
    pub(super) fn upload(
        &mut self,
        batch: Batch,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let visible = !batch.indices.is_empty();
        let layer = self
            .rendered
            .as_mut()
            .context("battle HUD layer was not prepared")?;
        layer.update_mesh(batch, self.definition.size, meshes)?;
        layer.show(visible, commands);
        Ok(())
    }
    pub(super) fn show(&mut self, visible: bool, commands: &mut Commands) {
        if let Some(layer) = &mut self.rendered {
            layer.show(visible, commands);
        }
    }
}

pub(super) struct Palette {
    pub(super) solid: LayerDefinition,
    pub(super) font: LayerDefinition,
    pub(super) main_font: LayerDefinition,
    pub(super) nearest: Handle<Image>,
}
impl Palette {
    pub(super) fn layer(
        &self,
        definition: LayerDefinition,
        materials: &mut Assets<Surface>,
    ) -> HudLayer {
        HudLayer::load(definition, &self.nearest, materials)
    }
    pub(super) fn layers(
        &self,
        definitions: Vec<LayerDefinition>,
        materials: &mut Assets<Surface>,
    ) -> Vec<HudLayer> {
        definitions
            .into_iter()
            .map(|definition| self.layer(definition, materials))
            .collect()
    }
}

pub(super) struct UiLayers {
    pub(super) flash: HudLayer,
    pub(super) portraits: Vec<HudLayer>,
    pub(super) gauges: HudLayer,
    pub(super) font: HudLayer,
    pub(super) party_names: HudLayer,
    pub(super) commands: [HudLayer; 2],
    pub(super) results: [HudLayer; 2],
    pub(super) notice: [HudLayer; 2],
    pub(super) labels: [HudLayer; 2],
    pub(super) floating: HudLayer,
    pub(super) recovery: HudLayer,
    pub(super) selector: [HudLayer; 2],
    pub(super) scan: [HudLayer; 2],
    pub(super) transition: HudLayer,
}
impl UiLayers {
    pub(super) fn iter(&self) -> impl Iterator<Item = &HudLayer> {
        self.portraits
            .iter()
            .chain(&self.commands)
            .chain(&self.results)
            .chain(&self.notice)
            .chain(&self.labels)
            .chain(&self.selector)
            .chain(&self.scan)
            .chain([
                &self.flash,
                &self.gauges,
                &self.font,
                &self.party_names,
                &self.floating,
                &self.recovery,
                &self.transition,
            ])
    }
    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut HudLayer> {
        self.portraits
            .iter_mut()
            .chain(&mut self.commands)
            .chain(&mut self.results)
            .chain(&mut self.notice)
            .chain(&mut self.labels)
            .chain(&mut self.selector)
            .chain(&mut self.scan)
            .chain([
                &mut self.flash,
                &mut self.gauges,
                &mut self.font,
                &mut self.party_names,
                &mut self.floating,
                &mut self.recovery,
                &mut self.transition,
            ])
    }
}
