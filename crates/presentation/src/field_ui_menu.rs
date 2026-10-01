//! Prepared bitmap menu layers. Coordinates use the authored 640×448 canvas.
use super::*;
use GaugeLayout::{Compact, Full, Stacked, Unlabelled};
use bevy::camera::visibility::RenderLayers;
use resonance_content::{
    HEIGHT, SCENE_HEIGHT,
    menu::{MenuArt, Sprite},
};
use resonance_game::menu::{
    ConfirmationKind, MAIN_COLUMNS, MAIN_ENTRIES, Menu, Mode, Page, PopupContent, SLOTS_PER_BANK,
    Slot, SlotFocus, VISIBLE_SLOTS, cooking::Focus as CookingFocus,
    customize::Focus as CustomizeFocus, equipment::Focus as EquipmentFocus,
    ex_skills::Focus as ExFocus, items::Focus as ItemFocus, strategy::Focus as StrategyFocus,
    techniques::Focus as TechFocus, world_map::Focus as WorldMapFocus,
};
#[path = "field_ui_menu/collection.rs"]
mod collection;
#[path = "field_ui_menu/cooking.rs"]
mod cooking;
#[path = "field_ui_menu/crafting.rs"]
mod crafting;
#[path = "field_ui_menu/customize.rs"]
mod customize;
#[path = "field_ui_menu/equipment.rs"]
mod equipment;
#[path = "field_ui_menu/ex_skills.rs"]
mod ex_skills;
#[path = "field_ui_menu/figurines.rs"]
mod figurines;
#[path = "field_ui_menu/grade_shop.rs"]
mod grade_shop;
#[path = "field_ui_menu/items.rs"]
mod items;
#[path = "field_ui_menu/manual.rs"]
mod manual;
#[path = "field_ui_menu/monsters.rs"]
mod monsters;
#[path = "field_ui_menu/rename.rs"]
mod rename;
#[path = "field_ui_menu/shop.rs"]
mod shop;
#[path = "field_ui_menu/status.rs"]
mod status;
#[path = "field_ui_menu/strategy.rs"]
mod strategy;
#[path = "field_ui_menu/synopsis.rs"]
mod synopsis;
#[path = "field_ui_menu/techniques.rs"]
mod techniques;
#[cfg(test)]
#[path = "field_ui_menu/test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "field_ui_menu/tests.rs"]
mod tests;
#[path = "field_ui_menu/unison.rs"]
mod unison;
#[path = "field_ui_menu/world_map.rs"]
mod world_map;

const SCROLL_DOWN: usize = 16;
const SCROLL_UP: usize = 17;
const FONT: MaterialKey = MaterialKey::Font;
pub(super) fn model_preview_depth() -> f32 {
    DepthBand::Field.at(0, DrawRole::Text.depth() + 0.5)
}
const ATLAS: MaterialKey = MaterialKey::Texture(MenuArt::ATLAS_TEXTURE);
const PORTRAITS: usize = MenuArt::PORTRAIT_TEXTURES.start;
const WORLD_MAPS: usize = MenuArt::WORLD_MAP_TEXTURES.start;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MaterialKey {
    Texture(usize),
    Font,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DrawRole {
    Background,
    Frame(u8),
    Scroll,
    Text,
    Content,
    Cursor,
}
impl DrawRole {
    fn depth(self) -> f32 {
        match self {
            Self::Background => 0.,
            Self::Frame(piece) => 1. + f32::from(piece),
            Self::Scroll => 16.,
            Self::Text => 18.,
            Self::Content => 19.,
            Self::Cursor => 31.,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum DepthBand {
    Field,
    Battle,
}
impl DepthBand {
    pub(super) fn at(self, plane: usize, order: f32) -> f32 {
        let base = match self {
            Self::Field => 100.,
            Self::Battle => super::battle_ui::menu_depth(),
        };
        base + plane as f32 * 32. + order
    }
}
const WHITE: usize = 9;
const GOLD: usize = 8;
const DISABLED: usize = 7;
const LINE_SPACING: f32 = 2.;
const TICKS_PER_MINUTE: u64 = 60 * 60;

pub(super) enum Source<'a> {
    Title(Option<&'a Menu>),
    Field(&'a FieldSession),
    World(&'a resonance_game::overworld::Session, Option<&'a str>),
}

fn menu_label<'a>(labels: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    labels
        .get(key)
        .map(String::as_str)
        .with_context(|| format!("menu label {key:?} was not prepared"))
}

/// Lists scroll over five updates, including the extra row entering from above.
fn scroll_offset(phase: i8, row_height: i32) -> i32 {
    i32::from(phase) * row_height / 5 + if phase < 0 { row_height } else { 0 }
}

/// Once the retained content has faded out, the current content follows its page.
fn crossfade_opacity(blend: u8, page: u8) -> u8 {
    if blend == 255 { page } else { blend }
}

fn blink_opacity(tick: u32, period: u32, duration: u32) -> f32 {
    let phase = tick % period;
    let mut fade = if phase < duration {
        phase * 512 / duration
    } else {
        0
    };
    if fade >= 256 {
        fade = 511 - fade;
    }
    (255 - fade) as f32 / 255.
}

fn glyph_advance(character: char, advance: u32, width: f32) -> f32 {
    // Menus space full-width characters by one cell; Latin and half-width kana
    // retain the font's proportional advances.
    if resonance_content::font::is_single_byte(character) {
        (advance as f32 * width / 24.).trunc()
    } else {
        width
    }
}

#[derive(PartialEq, Eq)]
enum GaugeLayout {
    Unlabelled,
    Compact,
    Full,
    Stacked,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActivePage {
    Menu(Page),
    Shop,
    Crafting,
    WorldNotice,
}

pub(super) fn menu_image(server: &AssetServer, path: String, repeat: bool) -> Handle<Image> {
    server
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = false;
            let wrap = if repeat {
                ImageAddressMode::Repeat
            } else {
                ImageAddressMode::ClampToEdge
            };
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: wrap,
                address_mode_v: wrap,
                ..ImageSamplerDescriptor::linear()
            });
        })
        .load(path)
}

#[derive(Resource, Clone, Default, bevy::render::extract_resource::ExtractResource)]
pub(crate) struct MenuDraws(
    pub std::sync::Arc<std::sync::Mutex<crate::model_preview::gpu::Report>>,
);

pub(crate) fn install(app: &mut App) {
    app.add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
        MenuDraws,
    >::default());
    app.sub_app_mut(bevy::render::RenderApp).add_systems(
        bevy::render::Render,
        rendered.in_set(bevy::render::RenderSystems::Cleanup),
    );
}
fn rendered(
    draws: Option<Res<MenuDraws>>,
    phases: Res<
        bevy::render::render_phase::ViewSortedRenderPhases<
            bevy::core_pipeline::core_3d::Transparent3d,
        >,
    >,
    quads: Res<
        bevy::render::render_phase::ViewSortedRenderPhases<
            bevy::core_pipeline::core_2d::Transparent2d,
        >,
    >,
    cache: Res<bevy::render::render_resource::PipelineCache>,
    device: Res<bevy::render::renderer::RenderDevice>,
    queue: Res<bevy::render::renderer::RenderQueue>,
) {
    if let Some(draws) = draws {
        crate::model_preview::gpu::render_report(
            &mut draws.0.lock().unwrap(),
            &phases,
            &quads,
            &cache,
            &device,
            &queue,
        );
    }
}
impl MenuDraws {
    fn ready(&self) -> Result<bool> {
        let report = self.0.lock().unwrap();
        if let Some(error) = &report.error {
            anyhow::bail!("menu GPU draw failed: {error}");
        }
        Ok(report.armed && report.completed.load(std::sync::atomic::Ordering::Acquire))
    }
}

struct MenuMaterial {
    image: Handle<Image>,
    size: [u32; 2],
    material: Handle<Surface>,
    repeat: bool,
    pending: Option<String>,
}

pub(super) struct MenuArtwork {
    spec: MenuArt,
    server: AssetServer,
    draws: MenuDraws,
    experience: std::sync::Arc<[u32]>,
    materials: BTreeMap<MaterialKey, MenuMaterial>,
    pub layers: BTreeMap<(usize, DrawRole, MaterialKey), Layer>,
    band: DepthBand,
    active_page: Option<ActivePage>,
    items_animation: items::Animation,
    world_notice: WorldNotice,
}
/// Native world notices slide down from the top while their alpha approaches 255.
#[derive(Default)]
struct WorldNotice {
    message: Option<(String, bool)>,
    opacity: u8,
    tick: Option<u32>,
}
impl WorldNotice {
    fn step(&mut self, tick: u32, message: Option<(&str, bool)>) -> Option<(String, bool, u8)> {
        let elapsed = self
            .tick
            .map_or(1, |last| tick.saturating_sub(last))
            .min(16);
        self.tick = Some(tick);
        if let Some((text, acknowledgement)) = message {
            if self.message.as_ref().is_none_or(|(old, _)| old != text) {
                self.message = Some((text.to_owned(), acknowledgement));
                self.opacity = 0;
            }
            self.opacity = self.opacity.saturating_add((elapsed * 16).min(255) as u8);
        } else {
            self.opacity = self.opacity.saturating_sub((elapsed * 16).min(255) as u8);
        }
        (self.opacity > 0)
            .then(|| {
                self.message
                    .as_ref()
                    .map(|(s, a)| (s.clone(), *a, self.opacity))
            })
            .flatten()
    }
}
impl MenuArtwork {
    pub(super) fn world_map_art(&self, world: usize) -> (Handle<Surface>, [u32; 2]) {
        let material = &self.materials[&MaterialKey::Texture(WORLD_MAPS + world)];
        (material.material.clone(), material.size)
    }

    pub fn action_label(&self, action: resonance_game::field::FieldAction) -> Result<&str> {
        use resonance_game::field::FieldAction;
        menu_label(
            &self.spec.labels,
            match action {
                FieldAction::ToField => "to_field",
                FieldAction::Enter => "go_in",
                FieldAction::Talk => "talk",
                FieldAction::Shop => "shop",
                FieldAction::Examine => "examine",
                FieldAction::Open => "open",
                FieldAction::Climb => "climb",
                FieldAction::Descend => "descend",
                FieldAction::Jump => "jump",
                FieldAction::Rest => "rest",
                FieldAction::Leave => "go_out",
                FieldAction::Move => "move",
                FieldAction::Grab => "grab",
                FieldAction::Save => "save",
                FieldAction::Warp => "warp",
            },
        )
    }

    pub fn load(
        read: impl Fn(&str) -> Result<Vec<u8>>,
        experience: std::sync::Arc<[u32]>,
        server: &AssetServer,
        materials: &mut Assets<Surface>,
        font: (&Handle<Surface>, [u32; 2]),
        diagnostics: &resonance_content::diagnostics::Diagnostics,
    ) -> Result<Self> {
        let spec = MenuArt::decode(&read("ui/menu.json")?, diagnostics)?;
        spec.validate_structure()?;
        let mut prepared = BTreeMap::new();
        for (&index, texture) in &spec.textures {
            let image = Handle::default();
            let material = materials.add(Surface {
                source: image.clone(),
                sampling: image.clone(),
                frame_mask: image.clone(),
                color_mask: image.clone(),
                coverage: Coverage::default(),
                additive: false,
                red_channel: false,
                opaque: texture.opaque,
            });
            let key = MaterialKey::Texture(index);
            prepared.insert(
                key,
                MenuMaterial {
                    image,
                    size: [texture.width, texture.height],
                    material,
                    repeat: texture.repeat,
                    pending: Some(texture.path.clone()),
                },
            );
        }
        let (material, size) = font;
        prepared.insert(
            FONT,
            MenuMaterial {
                image: materials
                    .get(material)
                    .context("missing menu font material")?
                    .source
                    .clone(),
                size,
                material: material.clone(),
                repeat: true,
                pending: None,
            },
        );
        Ok(Self {
            spec,
            server: server.clone(),
            draws: MenuDraws::default(),
            experience,
            materials: prepared,
            layers: BTreeMap::new(),
            band: DepthBand::Field,
            active_page: None,
            items_animation: Default::default(),
            world_notice: Default::default(),
        })
    }
    pub fn ready(&self, images: &Assets<Image>) -> bool {
        self.active_page.is_some()
            && self.draws.ready().unwrap_or(false)
            && self.drawn_images().all(|image| images.contains(image.id()))
    }

    pub(super) fn drawn_images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.layers
            .iter()
            .filter(|(_, layer)| layer.visible)
            .map(|(&(_, _, material), _)| &self.materials[&material].image)
    }

    pub(super) fn published(&self, session: &FieldSession) -> bool {
        if session.menu.as_ref().is_some_and(|menu| menu.closed)
            || session.shop.as_ref().is_some_and(|shop| shop.closed)
        {
            return true;
        }
        (self.active_page.is_none() || self.draws.ready().unwrap_or(false))
            && self.active_page
                == session
                    .menu
                    .as_ref()
                    .map(|menu| ActivePage::Menu(menu.page))
                    .or_else(|| session.shop.as_ref().map(|_| ActivePage::Shop))
                    .or_else(|| session.crafting.as_ref().map(|_| ActivePage::Crafting))
    }

    /// Only textures referenced by this completed drawing admit the selected page.
    pub(super) fn drawn_images_ready(
        &self,
        images: &Assets<Image>,
        server: &AssetServer,
    ) -> Result<bool> {
        let mut drawn = self.drawn_images().peekable();
        if drawn.peek().is_none() {
            return Ok(true);
        }
        let mut ready = true;
        for image in drawn {
            ready &= crate::field_view::image_ready(server, images, image)?;
        }
        Ok(ready && self.draws.ready()?)
    }

    /// Entrance, panel and selection motion is complete; decorative pulses may continue.
    pub(super) fn visually_settled(&self, menu: &Menu, tick: u32) -> bool {
        if self.active_page != Some(ActivePage::Menu(menu.page))
            || !self.draws.ready().unwrap_or(false)
            || menu.busy
            || menu.main_animating()
        {
            return false;
        }
        match menu.page {
            Page::Items => self.items_animation.settled(menu.inventory.focus, tick),
            Page::Equip => {
                !menu.equipment.transition.animating()
                    && menu.equipment.scroll == 0
                    && menu.equipment.description_opacity == 255
            }
            Page::Strategy => {
                let state = &menu.strategy;
                !state.transition.animating()
                    && state.scroll == 0
                    && !state.preset_closing
                    && state.preset_opacity == if state.focus.preset() { 255 } else { 0 }
                    && state.rename_opacity
                        == if state.focus == StrategyFocus::Rename {
                            255
                        } else {
                            0
                        }
                    && (menu.strategy_page().description().is_none()
                        || state.description_opacity == 255)
            }
            Page::Status => !menu.status.animating() && menu.status.portrait_fade == 0,
            Page::Cooking => {
                let state = &menu.cooking;
                !state.transition.animating()
                    && state.scroll == 0
                    && state.description_fade == 0
                    && state
                        .popup
                        .as_ref()
                        .is_none_or(|popup| popup.active && popup.opacity == 255)
            }
            Page::Customize => {
                !menu.customize.transition.animating()
                    && menu.customize.scroll == 0
                    && menu.customize.color_scroll == 0
            }
            Page::Slots(_) => menu.popup.as_ref().is_none_or(|popup| popup.opacity == 255),
            _ => menu.foreground_fade() == 0,
        }
    }

    pub(super) fn prepare_windows(&mut self) {
        self.band = DepthBand::Battle;
    }
    #[cfg(test)]
    pub(super) fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.materials.values().map(|material| &material.image)
    }
    fn set_page(&mut self, page: Option<ActivePage>) {
        if self.active_page != page {
            self.items_animation = Default::default();
            self.active_page = page;
            *self.draws.0.lock().unwrap() = Default::default();
        }
    }

    pub(crate) fn close_page(&mut self) {
        self.set_page(None);
    }

    pub(crate) fn clear_page(&mut self, commands: &mut Commands) {
        self.close_page();
        for layer in self.layers.values_mut() {
            layer.show(false, commands);
            layer.uploaded = None;
        }
    }

    fn begin_drawing<'a>(
        &'a self,
        font: &'a BitmapFont,
        dialogue: &'a DialogueArt,
        preferences: Option<&'a resonance_content::menu_data::CustomizeSettings>,
        tick: u32,
    ) -> Result<Drawing<'a>> {
        self.spec
            .windows
            .get(&preferences.map_or(1, |settings| usize::from(settings.window)))
            .context("missing selected menu window")?;
        Ok(Drawing {
            screen: [0., 0., 640., 448.],
            spec: &self.spec,
            font,
            selection: &dialogue.selection,
            preferences,
            experience: &self.experience,
            tick,
            plane: 0,
            opacity: 255,
            offset: [0.; 2],
            batches: BTreeMap::new(),
        })
    }

    fn submit_drawing(
        &mut self,
        batches: DrawBatches,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        for (key, layer) in &mut self.layers {
            if batches
                .get(key)
                .is_none_or(|batch| batch.indices.is_empty())
            {
                layer.show(false, commands);
            }
        }
        for ((plane, role, texture), batch) in batches
            .into_iter()
            .filter(|(_, batch)| !batch.indices.is_empty())
        {
            if let MaterialKey::Texture(index) = texture {
                self.spec.texture(index)?;
            }
            let prepared = self
                .materials
                .get_mut(&texture)
                .context("missing selected menu material")?;
            if let Some(path) = prepared.pending.take() {
                prepared.image = menu_image(&self.server, path, prepared.repeat);
                let material = prepared.material.clone();
                let image = prepared.image.clone();
                commands.queue(move |world: &mut World| {
                    if let Some(mut surface) =
                        world.resource_mut::<Assets<Surface>>().get_mut(&material)
                    {
                        surface.source = image.clone();
                        surface.sampling = image.clone();
                        surface.frame_mask = image.clone();
                        surface.color_mask = image;
                    }
                });
            }
            let size = prepared.size;
            let layer = self
                .layers
                .entry((plane, role, texture))
                .or_insert_with(|| {
                    let mesh = meshes.add(batch.clone().mesh(size));
                    let material = prepared.material.clone();
                    let render_layers = match self.band {
                        DepthBand::Field => RenderLayers::default(),
                        DepthBand::Battle => RenderLayers::layer(crate::battle_view::LAYER),
                    };
                    let entity = commands
                        .spawn((
                            Mesh2d(mesh.clone()),
                            MeshMaterial2d(material.clone()),
                            Transform::from_xyz(0., 0., self.band.at(plane, role.depth())),
                            Visibility::Hidden,
                            render_layers,
                            // Sliding pages must submit their initially offscreen layers
                            // before the draw fence lets the opening animation advance.
                            bevy::camera::visibility::NoFrustumCulling,
                        ))
                        .id();
                    Layer {
                        entity,
                        mesh,
                        material,
                        uploaded: Some((batch.clone(), size)),
                        visible: false,
                    }
                });
            layer.update_mesh(batch, size, meshes)?;
            layer.show(true, commands);
        }
        let expected = self
            .layers
            .values()
            .filter(|layer| layer.visible)
            .map(|layer| bevy::render::sync_world::MainEntity::from(layer.entity))
            .collect();
        let mut report = self.draws.0.lock().unwrap();
        if report.expected != expected || !report.armed {
            *report = crate::model_preview::gpu::Report {
                armed: true,
                expected,
                ..Default::default()
            };
        }
        commands.insert_resource(self.draws.clone());
        Ok(())
    }

    /// Render the field-owned Tech page from a battle page borrow.  Both
    /// callers use the same `Drawing` methods and page state; battle only
    /// supplies a different backdrop and skips field-only input plumbing.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_battle_tech(
        &mut self,
        page: resonance_game::menu::techniques::Page<'_>,
        font: &BitmapFont,
        dialogue: &DialogueArt,
        preferences: &resonance_content::menu_data::CustomizeSettings,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.set_page(Some(ActivePage::Menu(Page::Tech)));
        let mut draw = self.begin_drawing(font, dialogue, Some(preferences), tick)?;
        draw.opacity = 255;
        draw.shade(draw.screen);
        draw.plane = 1;
        let anchor = draw.techniques_page(page)?;
        draw.tech_cursors_page(page);
        let modal = matches!(
            page.state.focus,
            TechFocus::CannotForget | TechFocus::Forget { .. }
        );
        let alpha = if modal { 127 } else { 255 };
        draw.cursor(anchor, alpha);
        if let Some(anchor) = draw.tech_popup_page(page)? {
            draw.cursor(anchor, 255);
        }
        let drawing = draw.batches;
        self.submit_drawing(drawing, commands, meshes)
    }

    #[allow(clippy::too_many_arguments)] // Compose shared artwork with the live viewport and clock.
    pub fn render(
        &mut self,
        source: Source<'_>,
        font: &BitmapFont,
        dialogue: &DialogueArt,
        presentation_tick: u32,
        resolution: super::super::Resolution,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let result = (|| {
            let (menu, shop) = match source {
                Source::Title(menu) => (menu, None),
                Source::Field(session) => (
                    session.menu.as_ref(),
                    session
                        .shop
                        .as_ref()
                        .zip(session.events.world.party.as_ref()),
                ),
                Source::World(session, _) => (session.menu.as_ref(), None),
            };
            let crafting = match source {
                Source::Field(session) => session
                    .crafting
                    .as_ref()
                    .zip(session.events.world.party.as_ref()),
                _ => None,
            };
            let notice = if let Source::World(session, message) = source {
                let acknowledgement = matches!(
                    session.prompt(),
                    Some(
                        resonance_game::overworld::Prompt::Item { .. }
                            | resonance_game::overworld::Prompt::Guidepost { .. }
                    )
                );
                self.world_notice
                    .step(session.events.tick(), message.map(|s| (s, acknowledgement)))
                    .filter(|_| session.active_skit.is_none() && session.menu.is_none())
            } else {
                None
            };
            if menu.is_none() && shop.is_none() && crafting.is_none() && notice.is_none() {
                self.clear_page(commands);
                return Ok(());
            }
            let [left, top, right, bottom] = resolution.ui_rect();
            let y_scale = SCENE_HEIGHT as f32 / HEIGHT as f32;
            self.set_page(
                menu.map(|menu| ActivePage::Menu(menu.page))
                    .or_else(|| shop.map(|_| ActivePage::Shop))
                    .or_else(|| crafting.map(|_| ActivePage::Crafting))
                    .or_else(|| notice.as_ref().map(|_| ActivePage::WorldNotice)),
            );
            let item_visual = menu.filter(|menu| menu.page == Page::Items).map(|menu| {
                self.items_animation
                    .sample(menu.inventory.focus, presentation_tick)
            });
            let preferences = menu
                .and_then(Menu::preferences)
                .or_else(|| shop.map(|(_, party)| &party.settings.preferences))
                .or_else(|| crafting.map(|(_, party)| &party.settings.preferences))
                .or_else(|| match source {
                    Source::World(session, _) => session
                        .events
                        .world
                        .party
                        .as_ref()
                        .map(|party| &party.settings.preferences),
                    _ => None,
                });
            let mut draw = self.begin_drawing(font, dialogue, preferences, presentation_tick)?;
            draw.screen = [left, top * y_scale, right, bottom * y_scale];
            draw.opacity = menu.map_or_else(
                || {
                    shop.map(|(s, _)| 255 - s.fade)
                        .or_else(|| crafting.map(|(c, _)| 255 - c.fade))
                        .unwrap_or(255)
                },
                |m| 255 - m.background_fade(),
            );
            if let Some(menu) = menu {
                if menu.checkpoint.is_none() {
                    draw.quad_role(
                        DrawRole::Background,
                        FONT,
                        draw.screen,
                        [0.5; 4],
                        [0., 0., 0., 1.],
                    );
                }
                draw.shade(draw.screen);
                draw.opacity = 255 - menu.foreground_fade();
                draw.plane = 1;
                let anchor = match menu.page {
                    Page::Slots(mode) => draw.slots(menu, mode)?,
                    Page::Main | Page::Party | Page::System | Page::Character(_) => {
                        draw.main(menu)?
                    }
                    Page::Status | Page::Titles => draw.status(menu)?,
                    Page::Rename => draw.rename(menu)?,
                    Page::Items => draw.items(menu, item_visual.unwrap())?,
                    Page::Collection => draw.collection(menu)?,
                    Page::WorldMap => draw.world_map(menu)?,
                    Page::Monsters => draw.monsters(menu)?,
                    Page::Figurines => draw.figurines(menu)?,
                    Page::Manual => draw.manual(menu)?,
                    Page::Equip => draw.equipment(menu)?,
                    Page::Tech => draw.techniques(menu)?,
                    Page::Unison => draw.unison(menu.unison_page(), menu.at_save_point)?,
                    Page::ExSkills => draw.ex_skills(menu)?,
                    Page::Strategy => draw.strategy(menu.strategy_page())?,
                    Page::Synopsis => draw.synopsis(menu)?,
                    Page::Cooking => draw.cooking(menu)?,
                    Page::Customize => draw.customize(menu)?,
                    Page::GradeShop => draw.grade_shop(menu)?,
                };
                let modal = menu.popup.is_some()
                    || match menu.page {
                        Page::ExSkills => matches!(menu.ex_skills.focus, ExFocus::Confirm { .. }),
                        Page::Cooking => menu.cooking.popup.is_some(),
                        Page::Synopsis => menu.synopsis.reading,
                        Page::Strategy => {
                            menu.strategy.focus.preset()
                                && (menu.strategy.focus == StrategyFocus::Rename
                                    || menu.strategy.rename_opacity != 0)
                        }
                        Page::Tech => matches!(
                            menu.tech.focus,
                            TechFocus::CannotForget | TechFocus::Forget { .. }
                        ),
                        _ => false,
                    };
                draw.plane = if modal { 1 } else { draw.plane.max(2) };
                match menu.page {
                    Page::Titles => {
                        draw.plane = 2;
                        draw.cursor([48., 96.], 127);
                        draw.plane = 4;
                    }
                    Page::Equip if menu.equipment.focus != EquipmentFocus::Character => {
                        let fade = u32::from(menu.equipment.transition.page_fade);
                        draw.cursor(
                            [32. - (fade * 308 / 256) as f32, 68.],
                            ((255 - fade) / 2) as u8,
                        );
                    }
                    Page::Items
                        if matches!(
                            menu.inventory.focus,
                            ItemFocus::Target | ItemFocus::Discard(_)
                        ) =>
                    {
                        let row = menu.inventory.row - menu.inventory.first;
                        draw.plane = 2;
                        draw.cursor(
                            [48. + (row % 2) as f32 * 296., 96. + (row / 2) as f32 * 26.],
                            127,
                        );
                        draw.plane = 3;
                    }
                    Page::Tech => draw.tech_cursors(menu),
                    Page::Unison => draw.unison_cursors(menu.unison_page()),
                    Page::ExSkills => draw.ex_cursors(menu),
                    Page::Strategy => draw.strategy_cursors(menu.strategy_page()),
                    Page::Cooking if menu.cooking.focus != CookingFocus::Header => draw.cursor(
                        menu.cooking.header_cursor(),
                        (255 - menu.cooking.transition.page_fade) >> 1,
                    ),
                    Page::Customize
                        if matches!(
                            menu.customize.focus,
                            CustomizeFocus::Colors
                                | CustomizeFocus::Volume
                                | CustomizeFocus::Controls
                        ) =>
                    {
                        draw.cursor(
                            menu.customize.options_cursor(),
                            (255 - menu.customize.transition.page_fade) >> 1,
                        )
                    }
                    _ => {}
                }
                let list = matches!(menu.page, Page::Slots(_)) && menu.focus == SlotFocus::List;
                if !menu.busy {
                    if list {
                        draw.cursor([336. + menu.bank as f32 * 144., 30.], 127);
                    }
                    let alpha = match menu.page {
                        Page::Rename if menu.rename.pending => 0,
                        Page::Rename => 255 - menu.rename.fade,
                        Page::Items if !modal => 255 - item_visual.unwrap().fade,
                        Page::Collection => 255 - menu.collection.page_fade,
                        Page::Manual => 255 - menu.manual.page_fade,
                        Page::Monsters => 255 - menu.monsters.view.page_fade,
                        Page::Figurines => 255 - menu.figurines.view.page_fade,
                        Page::Equip => 255 - menu.equipment.transition.page_fade,
                        Page::Customize => {
                            (255 - menu.customize.transition.page_fade)
                                >> u8::from(menu.customize.focus == CustomizeFocus::Position)
                        }
                        Page::System => menu.system_opacity,
                        Page::Cooking => {
                            (255 - menu.cooking.transition.page_fade) >> u8::from(modal)
                        }
                        Page::ExSkills => {
                            (255 - menu.ex_skills.transition.page_fade) >> u8::from(modal)
                        }
                        Page::Unison => 255,
                        Page::Tech if !modal => 255,
                        Page::WorldMap => match menu.world_map.focus {
                            WorldMapFocus::Locations => 255 - menu.world_map.page_fade,
                            WorldMapFocus::Shops => menu.world_map.shops_opacity,
                            WorldMapFocus::Items => menu.world_map.items_opacity,
                        },
                        Page::Titles => menu.status.title_opacity,
                        Page::Status => 255 - menu.status.page_fade,
                        Page::Strategy if menu.strategy.focus == StrategyFocus::Rename => 127,
                        Page::Strategy => 255,
                        // The list regains its active cursor while the popup fades away.
                        Page::Slots(_) if menu.confirmation.is_none() && menu.notice.is_none() => {
                            255
                        }
                        _ if modal || menu.confirmation.is_some() || menu.notice.is_some() => 127,
                        _ => 255,
                    };
                    let group_target = match menu.page {
                        Page::Tech => menu.tech_target_visible() && menu.tech_targets_all(),
                        Page::Items => {
                            menu.inventory.focus == ItemFocus::Target && menu.inventory.target_all
                        }
                        _ => false,
                    };
                    if group_target {
                        for slot in 0..menu.party().formation.len() {
                            let at = if menu.page == Page::Items {
                                items::target_cursor(menu, slot, item_visual.unwrap())
                            } else {
                                techniques::target_cursor(menu, slot)
                            };
                            draw.cursor(at, 255);
                        }
                    } else {
                        draw.cursor(anchor, alpha);
                    }
                }
                let anchor = if let Some(popup) = &menu.popup {
                    match &popup.content {
                        PopupContent::Confirmation { kind, bank, yes } => {
                            let keys = match kind {
                                ConfirmationKind::Save => ["confirm_save_a", "confirm_save_b"],
                                ConfirmationKind::Overwrite => {
                                    ["confirm_overwrite_a", "confirm_overwrite_b"]
                                }
                                ConfirmationKind::Load => ["confirm_load_a", "confirm_load_b"],
                            };
                            draw.popup(
                                menu_label(&self.spec.labels, keys[*bank])?,
                                Some(*yes),
                                popup.opacity,
                            )?
                        }
                        PopupContent::Notice(notice) => {
                            draw.popup(&wrap(notice, 32), None, popup.opacity)?
                        }
                    }
                } else {
                    match menu.page {
                        Page::Tech => draw.tech_popup(menu)?,
                        Page::ExSkills => draw.ex_popup(menu)?,
                        Page::Strategy => draw.strategy_rename(menu.strategy_page())?,
                        Page::Synopsis => {
                            draw.synopsis_text(menu)?;
                            None
                        }
                        Page::Cooking => {
                            draw.cooking_popup(menu)?;
                            None
                        }
                        _ => None,
                    }
                };
                if let Some(anchor) = anchor.filter(|_| !menu.busy) {
                    draw.cursor(
                        anchor,
                        menu.popup.as_ref().map_or_else(
                            || match menu.page {
                                Page::ExSkills => menu.ex_skills.popup_opacity,
                                _ => 255,
                            },
                            |popup| popup.opacity,
                        ),
                    );
                }
            } else if let Some((shop, party)) = shop {
                draw.shade(draw.screen);
                draw.plane = 1;
                let anchor = draw.shop(shop, party)?;
                draw.plane = 4;
                draw.cursor(anchor, 255 - shop.fade);
            } else if let Some((crafting, party)) = crafting {
                draw.shade(draw.screen);
                draw.plane = 1;
                let anchor = draw.crafting(crafting, party)?;
                draw.plane = 4;
                draw.cursor(anchor, 255 - crafting.fade);
            } else if let (Source::World(_, _), Some((message, acknowledgement, opacity))) =
                (source, notice)
            {
                draw.plane = 3;
                draw.opacity = opacity;
                let lines = draw.wrap_notice(&message, 24., 600.)?;
                let mut width: f32 = 0.;
                for line in &lines {
                    width = width.max(draw.text_width(line, 24.)?);
                }
                // A tight top notice, 12-pixel inset, 26-pixel
                // line pitch. Enter/Leave are separate button ribbons below it.
                let height = lines.len() as f32 * 26.;
                let top = 24. - ((height + 48.) * (255 - opacity) as f32 / 255.).trunc();
                let left = ((640. - width) / 2.).trunc();
                draw.colored_frame(
                    [left - 12., top - 12., width + 24., height + 24.],
                    false,
                    draw.popup_color(),
                )?;
                for (index, line) in lines.iter().enumerate() {
                    draw.text_size(line, [left, top + index as f32 * 26.], [24., 24.], WHITE)?;
                }
                if !acknowledgement {
                    draw.world_button(6, "Enter", [504., 76.])?;
                    draw.world_button(8, "Leave", [504., 104.])?;
                }
            }
            let drawing = draw.batches;
            self.submit_drawing(drawing, commands, meshes)
        })();
        if result.is_err() {
            self.clear_page(commands);
        }
        result
    }
}

type DrawBatches = BTreeMap<(usize, DrawRole, MaterialKey), Batch>;

struct Drawing<'a> {
    screen: [f32; 4],
    spec: &'a MenuArt,
    font: &'a BitmapFont,
    selection: &'a resonance_content::font::SelectionArt,
    preferences: Option<&'a resonance_content::menu_data::CustomizeSettings>,
    experience: &'a [u32],
    tick: u32,
    plane: usize,
    opacity: u8,
    offset: [f32; 2],
    batches: DrawBatches,
}
impl Drawing<'_> {
    fn batch(&mut self, texture: MaterialKey) -> &mut Batch {
        self.batch_role(DrawRole::Text, texture)
    }
    fn batch_role(&mut self, role: DrawRole, texture: MaterialKey) -> &mut Batch {
        self.batches.entry((self.plane, role, texture)).or_default()
    }

    fn vertex_counts(&self) -> BTreeMap<(usize, DrawRole, MaterialKey), usize> {
        self.batches
            .iter()
            .map(|(&key, batch)| (key, batch.positions.len()))
            .collect()
    }
    /// Clip only newly drawn vertices, in the authored menu coordinates.
    fn clip_rows(
        &mut self,
        starts: BTreeMap<(usize, DrawRole, MaterialKey), usize>,
        [top, bottom]: [f32; 2],
    ) {
        let [_, top, _, bottom] =
            super::super::ui_coordinates::overlay_rect([0., top, 640., bottom]);
        let center = resonance_content::HEIGHT as f32 / 2.;
        for (key, batch) in &mut self.batches {
            if key.1 == DrawRole::Cursor {
                continue;
            }
            batch.clip_vertical(
                starts.get(key).copied().unwrap_or(0),
                center - top,
                center - bottom,
            );
        }
    }

    fn window(&self) -> &resonance_content::menu::WindowArt {
        &self.spec.windows[&self.preferences.map_or(1, |s| usize::from(s.window))]
    }
    fn heading(&mut self, text: &str) -> Result<()> {
        let x = if let Some(index) = self.window().heading {
            self.spec.texture(index)?;
            self.quad_role(
                DrawRole::Frame(0),
                MaterialKey::Texture(index),
                [16., 16., 48., 48.],
                [0., 0., 32., 32.],
                [1.; 4],
            );
            48.
        } else {
            let width = self.text_width(text, 32.)?;
            self.quad(
                FONT,
                [7., 40., 25. + width, 49.],
                [0.5; 4],
                rgba([0, 0, 0, 128]),
            );
            self.quad(
                FONT,
                [8., 41., 24. + width, 48.],
                [0.5; 4],
                rgba(self.menu_color()),
            );
            16.
        };
        self.shadowed_text(text, [x, 16.], 32., 4.)
    }
    fn menu_color(&self) -> [u8; 4] {
        self.preferences.map_or(self.spec.fill, |s| s.colors.menu)
    }
    fn popup_color(&self) -> [u8; 4] {
        self.preferences
            .map_or(self.spec.popup_fill, |s| s.colors.popup)
    }
    fn cursor(&mut self, [x, y]: [f32; 2], alpha: u8) {
        let alpha = self.alpha([1., 1., 1., f32::from(alpha) / 255.])[3];
        let position = [x + self.offset[0], y + self.offset[1]];
        self.batch_role(DrawRole::Cursor, FONT)
            .cursor(position, alpha);
    }
    fn alpha(&self, mut color: [f32; 4]) -> [f32; 4] {
        if self.opacity != 255 {
            color[3] = ((color[3] * 255.).round() * f32::from(self.opacity) / 255.).floor() / 255.;
        }
        color
    }
    fn quad(&mut self, texture: MaterialKey, rect: [f32; 4], uv: [f32; 4], color: [f32; 4]) {
        self.quad_role(DrawRole::Text, texture, rect, uv, color);
    }
    fn quad_role(
        &mut self,
        role: DrawRole,
        texture: MaterialKey,
        rect: [f32; 4],
        uv: [f32; 4],
        color: [f32; 4],
    ) {
        let color = self.alpha(color);
        let rect = std::array::from_fn(|i| rect[i] + self.offset[i % 2]);
        self.batch_role(role, texture).quad(
            super::super::ui_coordinates::overlay_rect(rect),
            uv,
            color,
        );
    }
    fn shade(&mut self, rect: [f32; 4]) {
        let start = self.batch_role(DrawRole::Background, FONT).colors.len();
        let colors = self.preferences.map_or(self.spec.shade, |s| {
            [s.colors.shade_top, s.colors.shade_bottom]
        });
        self.quad_role(DrawRole::Background, FONT, rect, [0.5; 4], rgba(colors[0]));
        let lower = self.alpha(rgba(colors[1]));
        self.batch_role(DrawRole::Background, FONT).colors[start + 2..start + 4].fill(lower);
    }
    /// Fit the message and choices to the canvas, with a dimmed menu below.
    fn popup(
        &mut self,
        message: &str,
        choice: Option<bool>,
        opacity: u8,
    ) -> Result<Option<[f32; 2]>> {
        self.popup_layout(message, choice, opacity, None)
    }
    fn popup_layout(
        &mut self,
        message: &str,
        choice: Option<bool>,
        opacity: u8,
        choice_layout: Option<[f32; 3]>,
    ) -> Result<Option<[f32; 2]>> {
        const FONT_SIZE: f32 = 24.;
        const PADDING: f32 = 12.;
        let choice_spacing = choice_layout.map_or(FONT_SIZE, |layout| layout[2]);
        let mut width: f32 = 0.;
        let mut height = 0.;
        for line in message.split('\n') {
            width = width.max(self.text_width(line, FONT_SIZE)?);
            height += FONT_SIZE + 2.;
        }
        let choices = if choice.is_some() {
            [
                menu_label(&self.spec.labels, "yes")?,
                menu_label(&self.spec.labels, "no")?,
            ]
        } else {
            [""; 2]
        };
        if choice.is_some() {
            height += choice_spacing * 2.;
            for text in choices {
                width = width.max(self.text_width(text, FONT_SIZE)?);
            }
        }
        let [x, y] = [
            ((640. - width) / 2.).trunc(),
            ((448. - height) / 2.).trunc(),
        ];
        self.plane = 2;
        self.quad(
            FONT,
            self.screen,
            [0.5; 4],
            [0., 0., 0., f32::from(opacity >> 1) / 255.],
        );
        self.opacity = opacity;
        self.shade([
            x - PADDING,
            y - PADDING,
            x + width + PADDING,
            y + height + PADDING,
        ]);
        self.plane = 3;
        self.colored_frame(
            [
                x - PADDING,
                y - PADDING,
                width + PADDING * 2.,
                height + PADDING * 2.,
            ],
            false,
            self.popup_color(),
        )?;
        self.text_size(message, [x, y], [FONT_SIZE; 2], WHITE)?;
        let Some(yes) = choice else {
            self.opacity = 255;
            return Ok(None);
        };
        let left = choice_layout.map_or(
            x + ((width - self.text_width(choices[0], FONT_SIZE)?) / 2.).trunc(),
            |layout| layout[0],
        );
        let top = y + height - choice_spacing * 2.;
        let selected = usize::from(!yes);
        let selected_y = top + selected as f32 * choice_spacing;
        self.highlight(
            [
                left,
                selected_y,
                choice_layout.map_or(self.text_width(choices[selected], FONT_SIZE)?, |layout| {
                    layout[1]
                }),
                FONT_SIZE,
            ],
            255,
        );
        for (index, text) in choices.into_iter().enumerate() {
            self.text_size(
                text,
                [left, top + index as f32 * choice_spacing],
                [FONT_SIZE; 2],
                WHITE,
            )?;
        }
        self.opacity = 255;
        Ok(Some([left, selected_y + 8.]))
    }
    fn frame(&mut self, rect: [f32; 4]) -> Result<()> {
        self.framed(rect, false)
    }
    fn framed(&mut self, rect: [f32; 4], ornament: bool) -> Result<()> {
        self.colored_frame(rect, ornament, self.menu_color())
    }
    fn party_color(&self, slot: usize) -> [u8; 4] {
        let mut color = self.menu_color();
        if slot >= resonance_game::menu::VISIBLE_PARTY {
            color[..3].iter_mut().for_each(|channel| *channel /= 2);
        }
        color
    }
    fn colored_frame(
        &mut self,
        [x, y, w, h]: [f32; 4],
        ornament: bool,
        color: [u8; 4],
    ) -> Result<()> {
        self.frame_detail([x, y, w, h], ornament, color, false)
    }
    fn frame_detail(
        &mut self,
        [x, y, w, h]: [f32; 4],
        ornament: bool,
        color: [u8; 4],
        decorated_left: bool,
    ) -> Result<()> {
        let window =
            self.spec.windows[&self.preferences.map_or(1, |s| usize::from(s.window))].clone();
        let pattern = *window
            .patterns
            .get(self.preferences.map_or(5, |s| usize::from(s.background)))
            .context("invalid selected background")?;
        self.spec.texture(pattern)?;
        let pattern = MaterialKey::Texture(pattern);
        let Some(slices) = window.slices else {
            let outset = f32::from(window.outset);
            let rect = [x - outset, y - outset, x + w + outset, y + h + outset];
            let start = self.batch_role(DrawRole::Background, pattern).colors.len();
            self.quad_role(
                DrawRole::Background,
                pattern,
                rect,
                [0., 0., w + outset * 2., h + outset * 2.],
                rgba(color),
            );
            for (index, level) in [4u16, 3, 2, 3].into_iter().enumerate() {
                let tint = std::array::from_fn(|channel| {
                    if channel == 3 {
                        color[3]
                    } else {
                        (u16::from(color[channel]) * level).div_ceil(4) as u8
                    }
                });
                self.batch_role(DrawRole::Background, pattern).colors[start + index] =
                    self.alpha(rgba(tint));
            }
            let [left, top, right, bottom] = rect;
            for (rect, shade) in [
                ([left, top, right, y], 128),
                ([left, top, x, bottom], 128),
                ([x + w, top, right, bottom], 16),
                ([left, y + h, right, bottom], 16),
            ] {
                self.quad_role(
                    DrawRole::Frame(0),
                    FONT,
                    rect,
                    [0.5; 4],
                    rgba([shade, shade, shade, color[3] >> 1]),
                );
            }
            return Ok(());
        };
        self.quad_role(
            DrawRole::Background,
            pattern,
            [x, y, x + w, y + h],
            [0., 0., w, h],
            rgba(color),
        );
        let outset = f32::from(window.outset);
        const JOIN: f32 = 16.;
        let flourish_outset = window.flourish_outset.map(f32::from);
        for texture in 1..=8 {
            if decorated_left && matches!(texture, 3 | 5 | 7) {
                continue;
            }
            let texture = if texture == 8 && ornament { 9 } else { texture };
            let image = self.spec.texture(slices[texture])?;
            let (tw, th) = (image.width as f32, image.height as f32);
            let (rect, uv) = match texture {
                1 | 2 => {
                    let join = if decorated_left {
                        f32::from(window.left_joins[texture - 1])
                    } else {
                        JOIN
                    };
                    let top = if texture == 1 {
                        y - outset
                    } else {
                        y + h + outset - th
                    };
                    (
                        [x + join, top, x + w - JOIN, top + th],
                        [0., 0., w - join - JOIN, th],
                    )
                }
                3 | 4 => {
                    let left = if texture == 3 {
                        x - outset
                    } else {
                        x + w + outset - tw
                    };
                    ([left, y + JOIN, left + tw, y + h - JOIN], [0., 0., tw, h])
                }
                _ => {
                    let left = if texture == 9 {
                        x + w + flourish_outset[0] - tw
                    } else if texture % 2 == 0 {
                        x + w + outset - tw
                    } else {
                        x - outset
                    };
                    let top = if texture >= 7 {
                        y + h + outset - th
                    } else {
                        y - outset
                    };
                    ([left, top, left + tw, top + th], [0., 0., tw, th])
                }
            };
            self.quad_role(
                DrawRole::Frame(texture as u8),
                MaterialKey::Texture(slices[texture]),
                rect,
                uv,
                [1.; 4],
            );
        }
        if decorated_left {
            for texture in [10, 11] {
                let image = self.spec.texture(slices[texture])?;
                let (tw, th) = (image.width as f32, image.height as f32);
                let top = if texture == 10 {
                    y - flourish_outset[1]
                } else {
                    y + h + outset - th
                };
                let left = x - if texture == 10 {
                    flourish_outset[0]
                } else {
                    f32::from(window.foot_outset)
                };
                self.quad_role(
                    DrawRole::Frame(texture as u8),
                    MaterialKey::Texture(slices[texture]),
                    [left, top, left + tw, top + th],
                    [0., 0., tw, th],
                    [1.; 4],
                );
            }
            let top = y + f32::from(window.left_strip[0]);
            let bottom = y + h - f32::from(window.left_strip[1]);
            if bottom > top {
                self.quad_role(
                    DrawRole::Frame(3),
                    MaterialKey::Texture(slices[3]),
                    [
                        x - outset,
                        top,
                        x - outset + self.spec.texture(slices[3])?.width as f32,
                        bottom,
                    ],
                    [
                        0.,
                        0.,
                        self.spec.texture(slices[3])?.width as f32,
                        bottom - top,
                    ],
                    [1.; 4],
                );
            }
        }
        Ok(())
    }
    fn button(&mut self, id: usize, position: [f32; 2]) -> Result<()> {
        let tick = self.tick;
        let index = match id {
            32 => {
                if tick % 60 < 30 {
                    4
                } else {
                    3
                }
            }
            6 | 8 | 10 | 12 | 14 | 16 | 18 | 23 => id - usize::from(tick % 40 >= 20),
            33..=36 => {
                if tick % 40 < 20 {
                    [1, 2, 4, 3][id - 33]
                } else {
                    0
                }
            }
            37 | 38 => {
                if tick % 40 < 20 {
                    id - 12
                } else {
                    24
                }
            }
            _ => id,
        };
        let sprite = self.spec.sprite(Sprite::Buttons, index)?;
        self.sprite(sprite, position);
        Ok(())
    }
    fn world_button(&mut self, button: usize, label: &str, [x, y]: [f32; 2]) -> Result<()> {
        let width = self.text_width(label, 24.)?;
        let left = self.spec.sprite(Sprite::Buttons, 19)?;
        let middle = self.spec.sprite(Sprite::Buttons, 20)?;
        let right = self.spec.sprite(Sprite::Buttons, 21)?;
        let start = x + 4.;
        let center = start + left[2] as f32;
        self.sprite(left, [start, y + 6.]);
        self.sprite_rect(
            middle,
            [center, y + 6., center + width, y + 6. + middle[3] as f32],
            [1.; 4],
        );
        self.sprite(right, [center + width, y + 6.]);
        self.button(button, [x, y])?;
        // Atlas layers follow the font within a plane. Native text is drawn
        // after its ribbon, so put the caption on the next plane.
        self.plane += 1;
        let result = self.text_size(label, [x + 24., y - 2.], [24.; 2], WHITE);
        self.plane -= 1;
        result
    }
    fn sprite(&mut self, rect: [u32; 4], [x, y]: [f32; 2]) {
        self.sprite_color(rect, [x, y], [1.; 4]);
    }
    fn sprite_rect(&mut self, sprite: [u32; 4], rect: [f32; 4], color: [f32; 4]) {
        self.quad_role(DrawRole::Content, ATLAS, rect, glyph_uv(sprite), color);
    }
    fn sprite_color(&mut self, rect: [u32; 4], [x, y]: [f32; 2], color: [f32; 4]) {
        self.sprite_rect(rect, [x, y, x + rect[2] as f32, y + rect[3] as f32], color);
    }
    fn portrait(
        &mut self,
        member: usize,
        status: &resonance_events::party::Member,
        [x, y]: [f32; 2],
    ) -> Result<()> {
        let knockout = status.knocked_out();
        let petrified = status.ailments.petrified && !knockout;
        let poisoned = status.ailments.poison.has_mild() || status.ailments.poison.has_severe();
        let rect = if petrified {
            self.spec.sprite(Sprite::PetrifiedPortraits, member)?
        } else {
            self.spec.sprite(Sprite::Portraits, member)?
        };
        let half = 128. / 255.;
        let tint = if knockout {
            [half, half, half, 1.]
        } else if poisoned && !petrified {
            [half, 1., half, 1.]
        } else {
            [1.; 4]
        };
        self.sprite_color(rect, [x, y], tint);
        if knockout || petrified {
            return Ok(());
        }
        if status.ailments.curse {
            self.sprite(self.spec.sprite(Sprite::ConditionIcons, 12)?, [x, y]);
            self.sprite_color(
                self.spec.sprite(Sprite::ConditionIcons, 13)?,
                [x, y],
                [1., 1., 1., blink_opacity(self.tick, 40, 15)],
            );
        } else if status.ailments.paralysis {
            let frame = if self.tick % 40 < 20 { 8 } else { 10 };
            self.sprite(self.spec.sprite(Sprite::ConditionIcons, frame)?, [x, y]);
            self.sprite(
                self.spec.sprite(Sprite::ConditionIcons, frame + 1)?,
                [x + 40., y],
            );
        } else if poisoned {
            let frame_ticks = if status.ailments.poison.has_severe() {
                4
            } else {
                8
            };
            let frame = (self.tick / frame_ticks % 4) as usize;
            self.sprite(
                self.spec.sprite(Sprite::ConditionIcons, frame)?,
                [x + 48., y],
            );
        }
        Ok(())
    }
    fn wrap_notice(&self, text: &str, cell: f32, maximum: f32) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        for paragraph in text.lines() {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let candidate = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if self.text_width(&candidate, cell)? + cell <= maximum {
                    line = candidate;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                for c in word.chars() {
                    let mut candidate = line.clone();
                    candidate.push(c);
                    if !line.is_empty() && self.text_width(&candidate, cell)? + cell > maximum {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(c);
                }
            }
            lines.push(line);
        }
        Ok(lines)
    }
    fn text_width(&self, text: &str, width: f32) -> Result<f32> {
        text.chars()
            .map(|c| {
                if c == ' ' {
                    Ok((width / 2.).trunc())
                } else {
                    self.font
                        .glyphs
                        .get(&c)
                        .with_context(|| format!("uncooked menu glyph {c:?}"))
                        .map(|g| glyph_advance(c, g.advance, width))
                }
            })
            .sum()
    }
    fn number(
        &mut self,
        value: u32,
        [right, y]: [f32; 2],
        size: [f32; 2],
        color: usize,
    ) -> Result<()> {
        let text = value.to_string();
        for (i, c) in text.chars().rev().enumerate() {
            self.text_size(
                &c.to_string(),
                [right - (i + 1) as f32 * size[0], y],
                size,
                color,
            )?;
        }
        Ok(())
    }
    fn currency(&mut self, value: u32, [right, y]: [f32; 2], size: [f32; 2]) -> Result<()> {
        for (i, digit) in value.to_string().chars().rev().enumerate() {
            let x = right - (i + 1) as f32 * size[0] - (i / 3) as f32 * size[0] / 2.;
            self.text_size(&digit.to_string(), [x, y], size, WHITE)?;
            if i > 0 && i.is_multiple_of(3) {
                self.text_size(",", [x + size[0], y], size, WHITE)?;
            }
        }
        Ok(())
    }
    fn time(
        &mut self,
        ticks: u64,
        [x, y]: [f32; 2],
        size: [f32; 2],
        color: usize,
        colon: bool,
    ) -> Result<()> {
        let minutes = ticks / TICKS_PER_MINUTE;
        self.number((minutes / 60) as u32, [x + size[0] * 3., y], size, color)?;
        if colon {
            self.text_size(":", [x + size[0] * 3., y], size, color)?;
        }
        self.number((minutes % 60) as u32, [x + size[0] * 6., y], size, color)?;
        if minutes % 60 < 10 {
            self.text_size("0", [x + size[0] * 4., y], size, color)?;
        }
        Ok(())
    }
    fn slanted(
        &mut self,
        role: DrawRole,
        texture: MaterialKey,
        rect: [f32; 4],
        uv: [f32; 4],
        colors: [[u8; 4]; 4],
        inset: f32,
    ) {
        self.quad_role(role, texture, rect, uv, [1.; 4]);
        let colors = colors.map(|color| self.alpha(rgba(color)));
        let batch = self.batch_role(role, texture);
        let start = batch.positions.len() - 4;
        batch.positions[start][0] += inset;
        batch.positions[start + 1][0] += inset;
        for (out, color) in batch.colors[start..].iter_mut().zip(colors) {
            *out = color;
        }
    }
    fn gauge_digit(
        &mut self,
        tile: u32,
        [x, y]: [f32; 2],
        [w, h]: [f32; 2],
        color: usize,
    ) -> Result<()> {
        let [u, v, _, th] = self.spec.sprite(Sprite::Numbers, 0)?;
        let [mut top, mut bottom] = *self
            .spec
            .sprites
            .number_colors
            .get(color)
            .context("missing menu number colors")?;
        top[3] = ((u16::from(top[3]) * 255) >> 8) as u8;
        bottom[3] = top[3];
        self.slanted(
            DrawRole::Content,
            ATLAS,
            [x, y + 1., x + w, y + h - 1.],
            [
                u as f32 + tile as f32 * 16.,
                v as f32,
                u as f32 + tile as f32 * 16. + 15.,
                v as f32 + th as f32,
            ],
            [top, top, bottom, bottom],
            8.,
        );
        Ok(())
    }
    fn gauge_number(
        &mut self,
        value: u16,
        [right, y]: [f32; 2],
        size: [f32; 2],
        color: usize,
    ) -> Result<()> {
        for (i, digit) in value.to_string().bytes().rev().enumerate() {
            self.gauge_digit(
                u32::from(digit - b'0'),
                [right - (i + 1) as f32 * size[0], y],
                size,
                color,
            )?;
        }
        Ok(())
    }
    fn gauge(
        &mut self,
        tp: bool,
        [x, y]: [f32; 2],
        [w, h]: [f32; 2],
        value: u16,
        maximum: u16,
        layout: GaugeLayout,
    ) -> Result<()> {
        let compact = layout != Full;
        let stacked = layout == Stacked;
        let shift = if stacked { (h / 2.).trunc() } else { 0. };
        let digits = [w, (h * 22. / 24.).trunc()];
        let top = y + h - digits[1] - shift;
        let left = if layout == Unlabelled { x } else { x + w * 2. };
        let width = w * if compact { 4. } else { 9. };
        let bar = [
            left,
            y + h - shift - 8.,
            left + width * f32::from(value) / f32::from(maximum.max(1)),
            y + h - shift,
        ];
        let mut shadow = *self
            .spec
            .sprites
            .bar_colors
            .get(2)
            .context("missing menu bar colors")?;
        for color in &mut shadow {
            color[3] = ((u16::from(color[3]) * 127) >> 8) as u8;
        }
        self.slanted(
            DrawRole::Text,
            FONT,
            [
                left + 1.,
                y + h - shift - 7.,
                left + width + 1.,
                y + h - shift + 1.,
            ],
            [0.5; 4],
            shadow,
            8.,
        );
        let mut colors = *self
            .spec
            .sprites
            .bar_colors
            .get(usize::from(tp))
            .context("missing menu bar colors")?;
        for color in &mut colors {
            color[3] = ((u16::from(color[3]) * 255) >> 8) as u8;
        }
        self.slanted(DrawRole::Text, FONT, bar, [0.5; 4], colors, 8.);
        let color = if value == 0 {
            15
        } else if u32::from(value) * 4 <= u32::from(maximum) {
            14
        } else {
            12
        };
        self.gauge_number(value, [left + w * 4. + 1., top], digits, 17)?;
        self.gauge_number(value, [left + w * 4., top], digits, color)?;
        if !compact {
            self.gauge_digit(11, [x + w * 6., top], digits, 11)?;
            self.gauge_number(maximum, [x + w * 11. + 1., top], digits, 17)?;
            self.gauge_number(maximum, [x + w * 11., top], digits, 12)?;
        }
        if stacked {
            let bottom = top + shift * 2. + if tp { 2. } else { 1. };
            self.quad(
                FONT,
                [left - 1., y + shift - 0.5, x + w * 6. + 9., y + shift + 2.5],
                [0.5; 4],
                [0., 0., 0., 1.],
            );
            self.quad(
                FONT,
                [left, y + shift + 0.5, x + w * 6. + 8., y + shift + 1.5],
                [0.5; 4],
                [1.; 4],
            );
            self.gauge_number(maximum, [left + w * 4. + 1., bottom], digits, 17)?;
            self.gauge_number(maximum, [left + w * 4., bottom], digits, 12)?;
        }
        if layout == Unlabelled {
            Ok(())
        } else {
            self.text_size(if tp { "TP" } else { "HP" }, [x, y], [w, h], GOLD)
        }
    }
    fn technique(&mut self, [x, y]: [f32; 2], balance: i8) -> Result<()> {
        self.sprite(self.spec.sprite(Sprite::Technique, 0)?, [x + 52., y]);
        self.sprite(
            self.spec
                .sprite(Sprite::Technique, if balance <= 0 { 4 } else { 2 })?,
            [x - 6., y],
        );
        self.sprite(
            self.spec
                .sprite(Sprite::Technique, if balance > 0 { 1 } else { 3 })?,
            [x + 114., y],
        );
        for i in 0..10 {
            let active = i <= usize::from(balance.unsigned_abs() / 10);
            let color = if !active { 3 } else { (i / 3).min(2) };
            let (left, right) = if balance <= 0 {
                (9 + color, 8)
            } else {
                (12, 5 + color)
            };
            self.sprite(
                self.spec.sprite(Sprite::Technique, left)?,
                [x + 50. - i as f32 * 5., y],
            );
            self.sprite(
                self.spec.sprite(Sprite::Technique, right)?,
                [x + 62. + i as f32 * 5., y],
            );
        }
        Ok(())
    }
    /// Standard menu glyphs are 24 pixels tall; width controls their proportions.
    fn text(&mut self, text: &str, position: [f32; 2], width: f32, color: usize) -> Result<()> {
        self.text_size(text, position, [width, 24.], color)
    }
    fn shadowed_text(
        &mut self,
        text: &str,
        [x, y]: [f32; 2],
        size: f32,
        shadow: f32,
    ) -> Result<()> {
        let opacity = self.opacity;
        self.opacity >>= 1;
        self.text_size(text, [x + shadow, y + shadow], [size; 2], 0)?;
        self.opacity = opacity;
        self.text_size(text, [x, y], [size; 2], WHITE)
    }
    fn text_size(
        &mut self,
        text: &str,
        [left, mut y]: [f32; 2],
        [width, height]: [f32; 2],
        color: usize,
    ) -> Result<()> {
        let color = rgba(
            *self
                .spec
                .palette
                .get(color)
                .context("unknown menu text color")?,
        );
        let mut x = left;
        for character in text.chars() {
            match character {
                '\n' => {
                    x = left;
                    y += height + LINE_SPACING;
                }
                ' ' => x += (width / 2.).trunc(),
                _ => {
                    let glyph = self
                        .font
                        .glyphs
                        .get(&character)
                        .with_context(|| format!("uncooked menu glyph {character:?}"))?;
                    let uv = glyph_uv(glyph.rect);
                    let advance = glyph_advance(character, glyph.advance, width);
                    self.quad(FONT, [x, y, x + width, y + height], uv, color);
                    x += advance;
                }
            }
        }
        Ok(())
    }
    fn highlight(&mut self, [x, y, width, height]: [f32; 4], alpha: u8) {
        let selected = self
            .preferences
            .map_or(self.selection.color, |s| s.colors.selection);
        let mut color = rgba(selected);
        color[3] = f32::from(u16::from(selected[3]) * u16::from(alpha) / 255) / 255.;
        if self.preferences.map_or(self.selection.mode, |s| s.window) == 0 {
            self.quad(
                FONT,
                [x, y + height - 1., x + width, y + height + 1.],
                [0.5; 4],
                color,
            );
        } else if height <= 32. {
            for (row, offset) in self.selection.row_offsets.into_iter().enumerate() {
                let top = y + ((height - 9.) / 2.).trunc() + 2. + row as f32 - 0.5;
                let offset = f32::from(offset);
                self.quad(
                    FONT,
                    [x + 8. - offset, top, x + width - 8. + offset, top + 1.],
                    [0.5; 4],
                    color,
                );
            }
        }
    }
    fn scroll_arrow(&mut self, texture: usize, [x, y]: [f32; 2]) -> Result<()> {
        let tick = self.tick;
        let image = self.spec.texture(texture)?;
        let (w, h) = (image.width as f32, image.height as f32);
        self.quad_role(
            DrawRole::Scroll,
            MaterialKey::Texture(texture),
            [x, y, x + w, y + h],
            [0., 0., w, h],
            [1., 1., 1., blink_opacity(tick, 40, 15)],
        );
        Ok(())
    }
    fn slots(&mut self, menu: &Menu, mode: Mode) -> Result<[f32; 2]> {
        self.heading(menu_label(
            &self.spec.labels,
            if mode == Mode::Save { "save" } else { "load" },
        )?)?;
        self.frame([324., 18., 296., 32.])?;
        for (bank, x) in [336., 480.].into_iter().enumerate() {
            if bank == menu.bank {
                self.highlight(
                    [x, 22., 120., 24.],
                    if menu.focus == SlotFocus::Bank {
                        255
                    } else {
                        127
                    },
                );
            }
            self.text(
                if bank == 0 { "Slot A" } else { "Slot B" },
                [x, 22.],
                24.,
                if bank == menu.bank { WHITE } else { DISABLED },
            )?;
        }
        let filled = !menu.busy
            && menu.focus == SlotFocus::List
            && matches!(menu.slots[menu.index()], Slot::Saved { .. });
        if !filled {
            self.frame([120., 60., 500., 306.])?;
        }
        for row in 0..VISIBLE_SLOTS {
            let y = 86. + row as f32 * 48.;
            self.frame([16., y, 92., 39.])?;
            self.text_size(
                &(menu.first_slot + row + 1).to_string(),
                [24., y + 2.],
                [16.; 2],
                GOLD,
            )?;
            if let Slot::Saved { played_ticks, .. } =
                &menu.slots[menu.bank * SLOTS_PER_BANK + menu.first_slot + row]
            {
                self.text_size(
                    &format!(
                        "{}:{:02}",
                        played_ticks / (60 * TICKS_PER_MINUTE),
                        played_ticks / TICKS_PER_MINUTE % 60
                    ),
                    [32., y + 18.],
                    [12., 20.],
                    WHITE,
                )?;
            }
        }
        self.frame([16., 376., 604., 56.])?;
        let bank = &menu.slots[menu.bank * SLOTS_PER_BANK..(menu.bank + 1) * SLOTS_PER_BANK];
        let total = bank
            .iter()
            .filter(|slot| matches!(slot, Slot::Saved { .. }))
            .count();
        let before = bank[..=menu.slot]
            .iter()
            .filter(|slot| matches!(slot, Slot::Saved { .. }))
            .count();
        let counter = if total == 0 {
            "-/-".into()
        } else {
            format!("{before}/{total}")
        };
        let width = counter
            .chars()
            .map(|c| {
                self.font
                    .glyphs
                    .get(&c)
                    .map_or(0., |g| (g.advance as f32 * 14. / 24.).trunc())
            })
            .sum::<f32>();
        self.text_size(&counter, [112. - width, 60.], [14., 16.], WHITE)?;
        if menu.first_slot > 0 {
            self.scroll_arrow(SCROLL_UP, [50., 70.])?;
        }
        if menu.first_slot + VISIBLE_SLOTS < SLOTS_PER_BANK {
            self.scroll_arrow(SCROLL_DOWN, [50., 358.])?;
        }
        if menu.busy {
            self.text("Please wait...", [264., 201.], 24., WHITE)?;
        } else if menu.focus == SlotFocus::List {
            match &menu.slots[menu.index()] {
                Slot::Empty => self.text(
                    menu_label(&self.spec.labels, "empty")?,
                    [310., 201.],
                    24.,
                    WHITE,
                )?,
                Slot::Invalid(_) => {
                    self.text("Cannot load this data", [236., 201.], 24., DISABLED)?
                }
                Slot::Saved {
                    played_ticks,
                    party,
                    ..
                } => {
                    self.party(party, true, 0, None)?;
                    for (key, position) in [
                        ("gald", [32., 380.]),
                        ("play_time", [32., 406.]),
                        ("encounters", [320., 380.]),
                        ("max_combo", [320., 406.]),
                    ] {
                        self.text(menu_label(&self.spec.labels, key)?, position, 20., GOLD)?;
                    }
                    self.currency(party.gald, [268., 380.], [16., 24.])?;
                    self.time(*played_ticks, [172., 406.], [16., 24.], WHITE, true)?;
                    for y in [380., 406.] {
                        self.number(0, [544., y], [16., 24.], WHITE)?;
                    }
                }
            }
        }
        Ok(if menu.focus == SlotFocus::Bank {
            [336. + menu.bank as f32 * 144., 30.]
        } else {
            [32., 110. + (menu.slot - menu.first_slot) as f32 * 48.]
        })
    }
    fn main(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let slide = |distance: u32| f32::from((u32::from(menu.main_fade) * distance / 256) as u16);
        let header_offset = [0., -slide(84)];
        self.offset = header_offset;
        self.frame([16., 16., 604., 60.])?;
        let mut anchor = [0.; 2];
        if menu.page == Page::Party {
            self.party_controls(menu)?;
        } else {
            for (index, (page, key)) in MAIN_ENTRIES.into_iter().enumerate() {
                let label = menu_label(&self.spec.labels, key)?;
                let position = [
                    76. + (index % MAIN_COLUMNS) as f32 * 120.
                        - (self.text_width(label, 23.)? / 2.).trunc(),
                    20. + (index / MAIN_COLUMNS) as f32 * 28.,
                ];
                if index == menu.selected && matches!(menu.page, Page::Main | Page::Character(_)) {
                    self.highlight(
                        [position[0], position[1], self.text_width(label, 23.)?, 24.],
                        255,
                    );
                    anchor = [
                        76. + (index % MAIN_COLUMNS) as f32 * 120.
                            - (self.text_width(label, 22.)? / 2.).trunc(),
                        position[1] + 8.,
                    ];
                }
                self.text(
                    label,
                    position,
                    23.,
                    if menu.main_entry_available(page) {
                        WHITE
                    } else {
                        DISABLED
                    },
                )?;
            }
        }
        if let Some(checkpoint) = &menu.checkpoint {
            self.offset = [-slide(440), 0.];
            let statistics = menu
                .resources
                .as_ref()
                .filter(|_| menu.party_statistics)
                .map(|r| r.data.as_ref());
            self.party(
                &checkpoint.progress().party,
                false,
                menu.first_character,
                statistics,
            )?;
            if menu.first_character > 0 {
                self.scroll_arrow(SCROLL_UP, [226., 77.])?;
            }
            if menu.first_character + resonance_game::menu::VISIBLE_PARTY
                < checkpoint.progress().party.formation.len()
            {
                self.scroll_arrow(SCROLL_DOWN, [226., 413.])?;
            }
            self.offset = [slide(170), 0.];
            self.framed([470., 85., 150., 335.], true)?;
            self.text(
                menu_label(&self.spec.labels, "gald")?,
                [478., 89.],
                20.,
                GOLD,
            )?;
            self.currency(checkpoint.progress().party.gald, [604., 113.], [14., 24.])?;
            self.text(
                menu_label(&self.spec.labels, "time")?,
                [478., 171.],
                20.,
                GOLD,
            )?;
            for (ticks, y, size, color) in [
                (menu.play_time.total(), 199., [16., 24.], WHITE),
                (menu.play_time.session(), 227., [16., 16.], 6),
            ] {
                self.time(ticks, [478., y], size, color, ticks % 60 > 30)?;
            }
            for (key, y) in [("encounter", 257.), ("combo", 343.)] {
                self.text(menu_label(&self.spec.labels, key)?, [478., y], 20., GOLD)?;
                self.number(0, [574., y + 28.], [16., 24.], WHITE)?;
            }
        }
        self.offset = header_offset;
        if menu.page == Page::System {
            let x = 444 + u32::from(255 - menu.system_opacity) * 196 / 256;
            let x = x + (640 - x) * u32::from(menu.main_fade) / 256;
            self.offset = [x as f32 - 444., 0.];
            self.opacity = if menu.system_closing || menu.system_opacity != 255 {
                menu.system_opacity
            } else {
                255 - menu.main_fade
            };
            self.plane = 2;
            self.shade([444., 48., 620., 136.]);
            self.plane = 3;
            self.frame([444., 48., 176., 88.])?;
            for (index, key) in ["save", "load", "customize"].into_iter().enumerate() {
                let label = menu_label(&self.spec.labels, key)?;
                let y = 52. + index as f32 * 28.;
                if index == menu.selected {
                    self.highlight([460., y, self.text_width(label, 24.)?, 24.], 255);
                }
                self.text(
                    label,
                    [460., y],
                    24.,
                    if index == 1
                        || index == 0 && menu.at_save_point
                        || index == 2 && menu.resources.is_some() && menu.checkpoint.is_some()
                    {
                        WHITE
                    } else {
                        DISABLED
                    },
                )?;
            }
            self.offset = [0.; 2];
            self.opacity = 255;
            Ok([x as f32 + 8., 52. + menu.selected as f32 * 28.])
        } else if matches!(menu.page, Page::Party | Page::Character(_)) {
            if matches!(menu.page, Page::Character(_)) {
                self.cursor(anchor, 127);
            }
            Ok([
                32.,
                133. + (menu.character - menu.first_character) as f32 * 86.,
            ])
        } else {
            Ok(anchor)
        }
    }
    fn party(
        &mut self,
        party: &resonance_events::party::Party,
        compact: bool,
        first: usize,
        statistics: Option<&resonance_content::menu_data::MenuData>,
    ) -> Result<()> {
        for (index, &id) in party
            .formation
            .iter()
            .enumerate()
            .skip(first)
            .take(resonance_game::menu::VISIBLE_PARTY)
        {
            let member = &party.members[usize::from(id - 1)];
            let [max_hp, max_tp] = member.maximum_vitals();
            let name = member
                .name
                .as_deref()
                .or_else(|| {
                    self.spec
                        .sprites
                        .names
                        .get(usize::from(id - 1))
                        .map(String::as_str)
                })
                .context("missing menu party name")?;
            let [x, y, w, h] = if compact {
                [120., 60. + (index - first) as f32 * 79., 245., 69.]
            } else {
                [16., 85. + (index - first) as f32 * 86., 444., 77.]
            };
            let color = if compact {
                self.menu_color()
            } else {
                self.party_color(index)
            };
            self.colored_frame([x, y, w, h], false, color)?;
            self.portrait(
                usize::from(id - 1),
                member,
                [x + 16., y + if compact { 3. } else { 5. }],
            )?;
            self.number(
                index as u32 + 1,
                [x + 16., y + if compact { 21. } else { 27. }],
                [16., 24.],
                if id == party.field_leader {
                    2
                } else if index < 4 {
                    WHITE
                } else {
                    DISABLED
                },
            )?;
            if id == party.field_leader {
                self.sprite(
                    self.spec.sprite(Sprite::Leader, 0)?,
                    [x, y - if compact { 2. } else { 0. }],
                );
            }
            if compact {
                self.text(name, [x + 80., y + 2.], 16., WHITE)?;
                self.text("Lv", [x + 176., y + 2.], 12., GOLD)?;
                self.number(
                    u32::from(member.level),
                    [x + 236., y + 2.],
                    [12., 24.],
                    WHITE,
                )?;
                self.gauge(
                    false,
                    [x + 148., y + 26.],
                    [14., 20.],
                    member.hp,
                    max_hp,
                    Compact,
                )?;
                self.gauge(
                    true,
                    [x + 148., y + 46.],
                    [14., 20.],
                    member.tp,
                    max_tp,
                    Compact,
                )?;
            } else {
                self.text(name, [x + 80., y], 24., WHITE)?;
                if let Some(data) = statistics {
                    let technique_type = if member.technique_balance > 0 {
                        &data.status_text()?.strike_type
                    } else {
                        &data.status_text()?.technical_type
                    };
                    self.text(technique_type, [x + 80., y + 26.], 16., WHITE)?;
                    let stats = member.stats(data);
                    for (row, columns) in [
                        [
                            (
                                if id == 1 {
                                    "party_slash"
                                } else {
                                    "party_attack"
                                },
                                stats.slash,
                            ),
                            ("party_thrust", stats.thrust),
                        ],
                        [("party_defense", stats.defense), ("party_luck", stats.luck)],
                        [
                            ("party_evasion", stats.evasion),
                            ("party_accuracy", stats.accuracy),
                        ],
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        for (col, (label, value)) in columns.into_iter().enumerate() {
                            if label == "party_thrust" && id != 1 {
                                continue;
                            }
                            let y = y + 2. + row as f32 * 24.;
                            self.text(
                                data.label(label)?,
                                [x + 236. + col as f32 * 96., y],
                                16.,
                                GOLD,
                            )?;
                            self.number(
                                u32::from(value),
                                [x + 324. + col as f32 * 104., y],
                                [16., 24.],
                                WHITE,
                            )?;
                        }
                    }
                    continue;
                }
                self.text("Lv", [x + 80., y + 50.], 16., GOLD)?;
                self.number(
                    u32::from(member.level),
                    [x + 160., y + 50.],
                    [16., 24.],
                    WHITE,
                )?;
                self.text(
                    menu_label(&self.spec.labels, "next")?,
                    [x + 204., y + 50.],
                    16.,
                    GOLD,
                )?;
                let next = self
                    .experience
                    .get(usize::from(member.level) + 1)
                    .copied()
                    .unwrap_or(member.experience)
                    .saturating_sub(member.experience);
                self.number(next, [x + 428., y + 50.], [16., 24.], WHITE)?;
                self.gauge(
                    false,
                    [x + 252., y + 2.],
                    [16., 24.],
                    member.hp,
                    max_hp,
                    Full,
                )?;
                self.gauge(
                    true,
                    [x + 252., y + 26.],
                    [16., 24.],
                    member.tp,
                    max_tp,
                    Full,
                )?;
                self.technique([x + 88., y + 26.], member.technique_balance)?;
            }
        }
        Ok(())
    }

    fn party_controls(&mut self, menu: &Menu) -> Result<()> {
        let party = &menu
            .checkpoint
            .as_ref()
            .context("party menu has no checkpoint")?
            .progress()
            .party;
        let labels = &menu
            .resources
            .as_ref()
            .context("party menu data is missing")?
            .data
            .presentation
            .labels;
        if let Some(origin) = menu.swap_character {
            let id = party.formation[origin];
            self.number(
                origin as u32 + 1,
                [32., 34.],
                [16., 24.],
                if id == party.field_leader {
                    2
                } else if origin < 4 {
                    WHITE
                } else {
                    DISABLED
                },
            )?;
            self.portrait(
                usize::from(id - 1),
                &party.members[usize::from(id - 1)],
                [32., 14.],
            )?;
            self.text(
                menu_label(labels, "party_swap_target")?,
                [112., 34.],
                24.,
                WHITE,
            )?;
        } else {
            let width = self
                .text_width(menu_label(labels, "party_leader")?, 24.)?
                .max(self.text_width(menu_label(labels, "party_swap")?, 24.)?);
            let x = 588. - width;
            for (label, button, y, color) in [
                (
                    "party_leader",
                    if party.leader_locked { 5 } else { 6 },
                    20.,
                    if party.leader_locked { DISABLED } else { WHITE },
                ),
                ("party_swap", 12, 48., WHITE),
            ] {
                self.sprite(self.spec.sprite(Sprite::Buttons, button)?, [x, y]);
                self.text(menu_label(labels, label)?, [x + 24., y], 24., color)?;
            }
        }
        Ok(())
    }
}
fn rgba(color: [u8; 4]) -> [f32; 4] {
    color.map(|v| f32::from(v) / 255.)
}
fn wrap(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut column = 0;
    for word in text.split_whitespace() {
        if column > 0 {
            if column + 1 + word.chars().count() > width {
                result.push('\n');
                column = 0;
            } else {
                result.push(' ');
                column += 1;
            }
        }
        result.push_str(word);
        column += word.chars().count();
    }
    result
}
