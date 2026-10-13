//! Actor labels and screen feedback owned by presentation.
use super::{Batch, BitmapFont, text};
use anyhow::Result;
use resonance_battle::conditions::ConditionLabel;

const HOLD_TICKS: u32 = 60;
const FADE_TICKS: u64 = 20;
const CONDITION_HOLD_TICKS: u32 = 45;
const COLLAPSE_TICKS: u64 = 12;

#[derive(Clone, Copy, Debug)]
pub(super) enum LabelKind {
    Applied,
    OverLimit,
    ExSkill,
    StatusDown,
    Custom,
    StatUp,
    EquipmentEffect,
    Revive,
    RevivalEffect,
}

pub(super) struct ActorLabel {
    pub position: [f32; 3],
    kind: LabelKind,
    lines: [String; 2],
    age: u64,
    duration: u32,
}

pub(super) fn condition(
    labels: &mut Vec<(usize, ActorLabel)>,
    owner: usize,
    kind: ConditionLabel,
    position: [f32; 3],
) {
    if let ConditionLabel::StatUp(condition) = kind {
        use resonance_battle::conditions::Condition::*;
        let stat = match condition {
            AttackUp => "ATTACK",
            DefenseUp => "DEFENSE",
            AccuracyUp => "ACCURACY",
            MagicAttackUp => "MAGIC ATTACK",
            MagicDefenseUp => "MAGIC DEFENSE",
            _ => return,
        };
        let mut label = ActorLabel::new(LabelKind::StatUp, position);
        label.lines = [stat.into(), "UP".into()];
        label.duration = CONDITION_HOLD_TICKS;
        replace(labels, owner, label);
        return;
    }
    let kind = match kind {
        ConditionLabel::StatUp(_) => unreachable!(),
        ConditionLabel::Applied => LabelKind::Applied,
        ConditionLabel::StatusDown => LabelKind::StatusDown,
        ConditionLabel::ExSkillEffect => LabelKind::ExSkill,
        ConditionLabel::EquipmentEffect => LabelKind::EquipmentEffect,
        ConditionLabel::Revive => LabelKind::Revive,
        ConditionLabel::RevivalEffect => LabelKind::RevivalEffect,
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
    label.lines = lines;
    replace(labels, owner, label)
}

impl ActorLabel {
    pub fn new(kind: LabelKind, position: [f32; 3]) -> Self {
        Self {
            position,
            kind,
            lines: match kind {
                LabelKind::OverLimit => ["OVER", "LIMIT"],
                LabelKind::Applied => ["STATUS", "EFFECT"],
                LabelKind::ExSkill => ["EX SKILL", "EFFECT"],
                LabelKind::StatusDown => ["STATUS", "DOWN"],
                LabelKind::EquipmentEffect => ["ITEM", "EFFECT"],
                LabelKind::Revive => ["MAGIC", "EFFECT"],
                LabelKind::RevivalEffect => ["REVIVAL", "EFFECT"],
                LabelKind::Custom | LabelKind::StatUp => ["", ""],
            }
            .map(str::to_owned),
            age: 0,
            duration: match kind {
                LabelKind::OverLimit | LabelKind::Custom => HOLD_TICKS,
                _ => CONDITION_HOLD_TICKS,
            },
        }
    }

    pub fn advance(&mut self, elapsed: u32) {
        self.age = self.age.saturating_add(u64::from(elapsed));
    }

    fn alpha(&self) -> u8 {
        let fading = self.age.saturating_sub(u64::from(self.duration));
        if matches!(self.kind, LabelKind::Applied) {
            return if fading < COLLAPSE_TICKS { 255 } else { 0 };
        }
        (255 * FADE_TICKS.saturating_sub(fading) / FADE_TICKS) as u8
    }

    pub fn visible(&self) -> bool {
        self.alpha() != 0
    }

    fn text(&self) -> String {
        self.lines.join(" ")
    }

    pub fn draw(
        &self,
        font: &BitmapFont,
        stat_colors: [[u8; 4]; 2],
        stat_backdrops: [[[u8; 4]; 4]; 2],
        projected: [f32; 2],
        solid: &mut Batch,
        text_batch: &mut Batch,
    ) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }
        let lines = &self.lines;
        const GLYPH: [f32; 2] = [18., 24.];
        const ADVANCE: f32 = 14.;
        const SKEW: f32 = 12.;
        const ROW_OFFSET: [f32; 2] = [12., 22.];
        const ECHO_TICKS: u64 = 8;
        const ECHO_STEP: f32 = 4.;
        const ECHO_START_ALPHA: u64 = 128;
        const STRIP_WIDTH: f32 = 100.;
        const STRIP_OFFSET: f32 = 60.;
        const STRIP_SKEW: f32 = 12.;
        const STRIP_LEAD: f32 = 24.;
        const STRIP_GROWTH: f32 = 4.;
        let line_width =
            |columns: usize| columns.saturating_sub(1) as f32 * ADVANCE + GLYPH[0] + SKEW;
        let columns = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        let width = line_width(columns).max(STRIP_WIDTH);
        let [x, y] = super::floating_origin(
            [projected[0] - STRIP_LEAD, projected[1] - GLYPH[1]],
            [width + STRIP_OFFSET + STRIP_SKEW, ROW_OFFSET[1] + GLYPH[1]],
        );
        let right = x + width + STRIP_LEAD;
        let collapse = if matches!(self.kind, LabelKind::Applied) {
            self.age
                .saturating_sub(u64::from(self.duration))
                .min(COLLAPSE_TICKS) as f32
        } else {
            0.
        };
        let height = GLYPH[1] * (1. - collapse / COLLAPSE_TICKS as f32);
        let mut colors = stat_colors;
        for color in &mut colors {
            color[3] = self.alpha();
        }
        for (row, line) in lines.iter().enumerate() {
            let top = y + row as f32 * ROW_OFFSET[1];
            let baseline = top + GLYPH[1];
            let text_left = right - line_width(line.chars().count()) + row as f32 * ROW_OFFSET[0];
            let left = x + width - STRIP_WIDTH + row as f32 * STRIP_OFFSET;
            let expansion = collapse * STRIP_GROWTH;
            let strip = if row == 0 {
                [left - expansion, left + STRIP_WIDTH]
            } else {
                [left, left + STRIP_WIDTH + expansion]
            };
            let mut backdrop = stat_backdrops[row];
            for (corner, color) in backdrop.iter_mut().enumerate() {
                color[3] = if corner < 2 {
                    0
                } else {
                    (u16::from(self.alpha()) * 128 / 255) as u8
                };
            }
            super::party::slanted_quad(
                solid,
                [strip[0], baseline - height, strip[1], baseline],
                [0.5; 4],
                STRIP_SKEW,
                backdrop,
            );
            text::italic(
                text_batch,
                font,
                line,
                [text_left, baseline - height],
                [GLYPH[0], height],
                ADVANCE,
                SKEW,
                colors,
            )?;
            if self.age <= ECHO_TICKS {
                let extra = (ECHO_TICKS - self.age) as f32 * ECHO_STEP;
                let opacity =
                    (ECHO_START_ALPHA + ECHO_START_ALPHA * self.age / ECHO_TICKS).min(255) as u16;
                let mut echo_colors = colors;
                for color in &mut echo_colors {
                    color[3] = (u16::from(color[3]) * opacity / 255) as u8;
                }
                text::italic(
                    text_batch,
                    font,
                    line,
                    [text_left, top],
                    [GLYPH[0] + extra, GLYPH[1] + extra],
                    ADVANCE + extra,
                    (GLYPH[1] + extra) / 2.,
                    echo_colors,
                )?;
            }
        }
        Ok(())
    }
}

impl super::Artwork {
    pub(crate) fn overlays_diagnostic(&self) -> serde_json::Value {
        serde_json::json!({
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
        assert_eq!(labels[0].1.lines, ["CUSTOM", "LABEL"]);
        request_custom(&mut labels, 2, Default::default(), [0.; 3], 0);
        assert!(labels.is_empty());
    }
}
