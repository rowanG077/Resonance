//! Shared model-view state for catalogue menus.
use super::*;
use resonance_content::model_preview::ModelPreview;

/// Page movement and the independently fading, retained catalogue model.
#[derive(Default, serde::Serialize)]
pub struct View {
    pub page_fade: u8,
    pub page_closing: bool,
    pub model_row: usize,
    pub model_opacity: u8,
    pub animation_tick: u32,
    pub model_started: bool,
}
impl View {
    pub(super) fn open() -> Self {
        Self {
            page_fade: 231 - MAIN_SLIDE_STEP,
            ..Default::default()
        }
    }
    pub(super) fn advance_model(&mut self, row: usize) -> bool {
        if self.model_row != row && self.model_opacity != 0 {
            self.model_opacity = self.model_opacity.saturating_sub(32);
            return true;
        }
        if !self.model_started || self.model_row != row {
            self.model_row = row;
            self.animation_tick = 0;
            self.model_started = true;
        } else {
            self.model_opacity = self.model_opacity.saturating_add(32);
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewId {
    Monster(u8),
    Figurine(u16),
}

pub struct Preview<'a> {
    pub id: PreviewId,
    pub model: &'a ModelPreview,
    pub yaw: f32,
    pub distance: f32,
    pub animation_tick: u32,
    pub opacity: u8,
    pub page_fade: u8,
}
impl Preview<'_> {
    pub fn sample(&self, duration: u32) -> u32 {
        animation_sample(self.animation_tick, duration)
    }
}
pub fn animation_sample(tick: u32, duration: u32) -> u32 {
    if tick > duration && duration > 0 {
        (tick - 1) % duration + 1
    } else {
        tick
    }
}
impl Menu {
    pub(super) fn advance_catalogue(&mut self) -> bool {
        let view = match self.page {
            Page::Monsters => &mut self.monsters.view,
            Page::Figurines => &mut self.figurines.view,
            _ => unreachable!(),
        };
        if view.page_fade == 255 {
            self.page = Page::Items;
            self.open_items();
            self.fade_item_description();
            return true;
        }
        if view.page_closing {
            view.page_fade = view.page_fade.saturating_add(MAIN_SLIDE_STEP);
            view.model_opacity = view.model_opacity.saturating_sub(MAIN_SLIDE_STEP);
            if view.page_fade == 255 {
                view.page_closing = false;
                view.model_opacity = 0;
            }
        } else {
            view.page_fade = view.page_fade.saturating_sub(MAIN_SLIDE_STEP);
        }
        false
    }
    pub(super) fn step_catalogue_animation(&mut self) {
        let view = match self.page {
            Page::Monsters if self.monsters.view.model_started => &mut self.monsters.view,
            Page::Figurines if self.figurines.view.model_opacity != 0 => &mut self.figurines.view,
            _ => return,
        };
        view.animation_tick = view.animation_tick.wrapping_add(1);
    }
    pub fn preview(&self) -> Option<Preview<'_>> {
        match self.page {
            Page::Monsters => self.displayed_monster().map(|(record, _)| Preview {
                id: PreviewId::Monster(record.id),
                model: &record.preview,
                yaw: self.monsters.yaw,
                distance: self.monsters.distance,
                animation_tick: self.monsters.view.animation_tick,
                opacity: self.monsters.view.model_opacity,
                page_fade: self.monsters.view.page_fade,
            }),
            Page::Figurines => self
                .figurine_records()
                .get(self.figurines.view.model_row)
                .map(|record| Preview {
                    id: PreviewId::Figurine(record.id),
                    model: &record.preview,
                    yaw: 330.,
                    distance: 1200.,
                    animation_tick: self.figurines.view.animation_tick,
                    opacity: self.figurines.view.model_opacity,
                    page_fade: self.figurines.view.page_fade,
                }),
            _ => None,
        }
    }
    pub fn register_preview(&mut self, id: PreviewId, tick: u32) -> bool {
        if !self.preview().is_some_and(|p| {
            p.id == id
                && p.model.parts.iter().any(|part| {
                    part.scene
                        .clips
                        .first()
                        .is_some_and(|c| tick <= c.duration_ticks())
                })
        }) {
            return false;
        }
        match id {
            PreviewId::Monster(_) => self.monsters.view.animation_tick = tick,
            PreviewId::Figurine(_) => self.figurines.view.animation_tick = tick,
        }
        true
    }
}
