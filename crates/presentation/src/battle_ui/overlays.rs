//! Actor labels and screen feedback owned by presentation.
use super::{Batch, BitmapFont, text};
use anyhow::Result;
use resonance_battle::conditions::ConditionLabel;

const HOLD_TICKS: u32 = 60;
const FADE_TICKS: u64 = 20;
const TEXT_SIZE: [f32; 2] = [10., 16.];
const PADDING: f32 = 4.;
const MAX_WIDTH: f32 = 240.;

#[derive(Clone, Copy, Debug)]
pub(super) enum LabelKind {
    Applied,
    OverLimit,
    ExSkill,
    StatusDown,
    Custom,
    EquipmentEffect,
    Revive,
}

pub(super) struct ActorLabel {
    pub position: [f32; 3],
    kind: LabelKind,
    custom_text: Option<String>,
    age: u64,
    duration: u32,
}

pub(super) fn condition(
    labels: &mut Vec<(usize, ActorLabel)>,
    owner: usize,
    kind: ConditionLabel,
    position: [f32; 3],
) {
    let kind = match kind {
        ConditionLabel::Applied => LabelKind::Applied,
        ConditionLabel::StatusDown => LabelKind::StatusDown,
        ConditionLabel::ExSkillEffect => LabelKind::ExSkill,
        ConditionLabel::EquipmentEffect => LabelKind::EquipmentEffect,
        ConditionLabel::Revive => LabelKind::Revive,
    };
    request_at(labels, owner, kind, position)
}

fn replace(labels: &mut Vec<(usize, ActorLabel)>, owner: usize, replacement: ActorLabel) {
    if let Some((_, label)) = labels.iter_mut().find(|(id, _)| *id == owner) {
        *label = replacement;
    } else {
        labels.push((owner, replacement));
        labels.sort_by_key(|(id, _)| *id);
    }
}

pub(super) fn request_at(
    labels: &mut Vec<(usize, ActorLabel)>,
    owner: usize,
    kind: LabelKind,
    position: [f32; 3],
) {
    replace(labels, owner, ActorLabel::new(kind, position))
}

pub(super) fn request_custom(
    labels: &mut Vec<(usize, ActorLabel)>,
    owner: usize,
    lines: [String; 2],
    position: [f32; 3],
    duration: u32,
) {
    if duration == 0 {
        labels.retain(|(id, _)| *id != owner);
        return;
    }
    let mut label = ActorLabel::new(LabelKind::Custom, position);
    label.duration = duration;
    label.custom_text = Some(lines.join(" "));
    replace(labels, owner, label)
}

impl ActorLabel {
    pub fn new(kind: LabelKind, position: [f32; 3]) -> Self {
        Self {
            position,
            kind,
            custom_text: None,
            age: 0,
            duration: HOLD_TICKS,
        }
    }

    pub fn advance(&mut self, elapsed: u32) {
        self.age = self.age.saturating_add(u64::from(elapsed));
    }

    fn alpha(&self) -> u8 {
        let fading = self.age.saturating_sub(u64::from(self.duration));
        (255 * FADE_TICKS.saturating_sub(fading) / FADE_TICKS) as u8
    }

    pub fn visible(&self) -> bool {
        self.alpha() != 0
    }

    fn text(&self) -> &str {
        self.custom_text.as_deref().unwrap_or(match self.kind {
            LabelKind::Applied => "STATUS EFFECT",
            LabelKind::OverLimit => "OVER LIMIT",
            LabelKind::ExSkill => "EX SKILL",
            LabelKind::StatusDown => "STATUS DOWN",
            LabelKind::EquipmentEffect => "EQUIPMENT EFFECT",
            LabelKind::Revive => "REVIVED",
            LabelKind::Custom => "",
        })
    }

    pub fn draw(
        &self,
        font: &BitmapFont,
        projected: [f32; 2],
        solid: &mut Batch,
        text_batch: &mut Batch,
    ) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }
        let text = self.text();
        let columns = text.chars().count().max(1) as f32;
        let width = TEXT_SIZE[0].min(MAX_WIDTH / columns);
        let size = [columns * width + PADDING * 2., TEXT_SIZE[1] + PADDING * 2.];
        let [x, y] = super::floating_origin([projected[0], projected[1] - 24.], size);
        let alpha = self.alpha();
        solid.quad(
            [x, y, x + size[0], y + size[1]],
            [0.5; 4],
            [0.04, 0.05, 0.08, f32::from(alpha) / 255. * 0.85],
        );
        text::glyphs(
            text_batch,
            font,
            text,
            [x + PADDING, y + PADDING],
            [width, TEXT_SIZE[1]],
            [128, 128, 128, alpha],
        )
    }
}

/// A brief wash over the scene, below the HUD. Drawing never advances it.
#[derive(Clone, Copy)]
pub(super) struct Flash {
    pub age: u32,
    pub color: [f32; 3],
}

impl Flash {
    pub fn alpha(&self) -> f32 {
        const DURATION: u32 = 24;
        const OPACITY: f32 = 0.18;
        let remaining = DURATION.saturating_sub(self.age);
        OPACITY * (remaining as f32 / DURATION as f32).powi(2)
    }

    pub fn draw(&self, batch: &mut Batch) {
        let alpha = self.alpha();
        if alpha > 0. {
            let [r, g, b] = self.color;
            batch.quad([0., 0., 640., 480.], [0.5; 4], [r, g, b, alpha]);
        }
    }
}

impl super::Artwork {
    pub(crate) fn overlays_diagnostic(&self) -> serde_json::Value {
        serde_json::json!({
            "flash": self.flash.map(|flash| serde_json::json!({
                "age": flash.age, "color": flash.color, "alpha": flash.alpha(),
            })),
            "actor_labels": self.labels.iter().map(|(actor, label)| serde_json::json!({
                "actor": actor, "kind": format!("{:?}", label.kind), "position": label.position,
                "age": label.age, "duration": label.duration,
                "alpha": label.alpha(), "text": label.text(),
            })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_replace_cancel_and_expire() {
        let mut labels = Vec::new();
        condition(&mut labels, 2, ConditionLabel::Applied, [1., 2., 3.]);
        labels[0].1.advance(0);
        assert!(labels[0].1.visible());
        request_at(&mut labels, 2, LabelKind::OverLimit, [4., 5., 6.]);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].1.position, [4., 5., 6.]);
        assert_eq!(labels[0].1.text(), "OVER LIMIT");
        labels[0].1.advance(HOLD_TICKS);
        assert!(labels[0].1.visible());
        labels[0].1.advance(FADE_TICKS as u32);
        assert!(!labels[0].1.visible());
        request_custom(
            &mut labels,
            2,
            ["CUSTOM".into(), "LABEL".into()],
            [0.; 3],
            10,
        );
        assert_eq!(labels[0].1.text(), "CUSTOM LABEL");
        request_custom(&mut labels, 2, Default::default(), [0.; 3], 0);
        assert!(labels.is_empty());
    }
}
