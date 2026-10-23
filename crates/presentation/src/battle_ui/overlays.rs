//! Actor labels and screen feedback owned by presentation.
use super::{Batch, BitmapFont, text};
use anyhow::Result;
use resonance_battle::{ActorId, conditions::ConditionLabel};
use std::collections::BTreeMap;

const HOLD_TICKS: u32 = 60;
const LABEL_FADE_STEP: u64 = 8;
const CONDITION_HOLD_TICKS: u32 = 45;
const COLLAPSE_TICKS: u64 = 12;

#[derive(Clone, Copy, Debug)]
pub(super) enum LabelKind {
    Applied,
    OverLimit,
    GuardBreak,
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
    labels: &mut BTreeMap<ActorId, ActorLabel>,
    owner: ActorId,
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
        labels.insert(owner, label);
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

pub(super) fn request_at(
    labels: &mut BTreeMap<ActorId, ActorLabel>,
    owner: ActorId,
    kind: LabelKind,
    position: [f32; 3],
) {
    labels.insert(owner, ActorLabel::new(kind, position));
}

pub(super) fn request_custom(
    labels: &mut BTreeMap<ActorId, ActorLabel>,
    owner: ActorId,
    lines: [String; 2],
    position: [f32; 3],
    duration: u32,
) {
    if duration == 0 {
        labels.remove(&owner);
        return;
    }
    let mut label = ActorLabel::new(LabelKind::Custom, position);
    label.duration = duration;
    label.lines = lines;
    labels.insert(owner, label);
}

impl ActorLabel {
    pub fn new(kind: LabelKind, position: [f32; 3]) -> Self {
        Self {
            position,
            kind,
            lines: match kind {
                LabelKind::OverLimit => ["OVER", "LIMIT"],
                LabelKind::GuardBreak => ["GUARD", "BREAK"],
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
                LabelKind::Custom => HOLD_TICKS,
                _ => CONDITION_HOLD_TICKS,
            },
        }
    }

    pub fn advance(&mut self, elapsed: u32) {
        self.age = self.age.saturating_add(u64::from(elapsed));
    }

    fn alpha(&self) -> u8 {
        let fading = self.age.saturating_sub(u64::from(self.duration));
        255_u64.saturating_sub(fading.saturating_mul(LABEL_FADE_STEP)) as u8
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
        const ALIGNMENT_CELL: f32 = 12.;
        const ROW_ADVANCE: f32 = 22.;
        const BASELINE_ABOVE_ANCHOR: f32 = 10.;
        const ECHO_TICKS: u64 = 8;
        const ECHO_STEP: f32 = 4.;
        const ECHO_START_ALPHA: u64 = 128;
        const STRIP_PADDING: [f32; 2] = [24., 8.];
        const STRIP_EXTRA_WIDTH: f32 = 40.;
        const STRIP_GROWTH: f32 = 4.;
        let columns = lines.each_ref().map(|line| line.chars().count());
        let alignment = [columns[0].div_ceil(2), columns[1].saturating_sub(1) / 2];
        let [x, y] = projected.map(f32::trunc);
        let strip_width = columns[0] as f32 * ALIGNMENT_CELL + STRIP_EXTRA_WIDTH;
        let collapse = self
            .age
            .saturating_sub(u64::from(self.duration))
            .min(COLLAPSE_TICKS) as f32;
        let height = GLYPH[1] * (1. - collapse / COLLAPSE_TICKS as f32);
        let skew = SKEW + GLYPH[1] - height;
        let mut colors = stat_colors;
        for color in &mut colors {
            color[3] = self.alpha();
        }
        for (row, line) in lines.iter().enumerate() {
            let baseline = y - BASELINE_ABOVE_ANCHOR + row as f32 * ROW_ADVANCE;
            let top = baseline - height;
            let text_left = x - alignment[row] as f32 * ALIGNMENT_CELL;
            let left = text_left - STRIP_PADDING[row];
            let expansion = collapse * STRIP_GROWTH;
            let strip = if row == 0 {
                [left - expansion, left + strip_width]
            } else {
                [left, left + strip_width + expansion]
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
                skew,
                backdrop,
            );
            text::italic(
                text_batch,
                font,
                line,
                [text_left, baseline - height],
                [GLYPH[0], height],
                ADVANCE,
                skew,
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
    pub(super) fn advance_labels(&mut self, clock: resonance_battle::BattleClock, elapsed: u32) {
        let activation_hold = matches!(
            clock,
            resonance_battle::BattleClock::TimedHold | resonance_battle::BattleClock::Rescue(_)
        );
        self.labels.retain(|owner, label| {
            let rescue =
                matches!(clock, resonance_battle::BattleClock::Rescue(actor) if actor == *owner);
            label.advance(
                if rescue || (activation_hold && matches!(label.kind, LabelKind::OverLimit)) {
                    1
                } else {
                    elapsed
                },
            );
            label.visible()
        });
    }

    pub(crate) fn overlays_diagnostic(&self) -> serde_json::Value {
        serde_json::json!({
            "actor_labels": self.labels.iter().map(|(actor, label)| serde_json::json!({
                "actor": actor.index(), "kind": format!("{:?}", label.kind), "position": label.position,
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
        let mut labels = BTreeMap::new();
        let owner = ActorId::from_index(2).unwrap();
        condition(&mut labels, owner, ConditionLabel::Applied, [1., 2., 3.]);
        labels.get_mut(&owner).unwrap().advance(0);
        assert!(labels[&owner].visible());
        request_at(&mut labels, owner, LabelKind::OverLimit, [4., 5., 6.]);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[&owner].position, [4., 5., 6.]);
        assert_eq!(labels[&owner].text(), "OVER LIMIT");
        labels.get_mut(&owner).unwrap().advance(HOLD_TICKS);
        assert!(labels[&owner].visible());
        labels.get_mut(&owner).unwrap().advance(HOLD_TICKS);
        assert!(!labels[&owner].visible());
        request_custom(
            &mut labels,
            owner,
            ["CUSTOM".into(), "LABEL".into()],
            [0.; 3],
            10,
        );
        assert_eq!(labels[&owner].text(), "CUSTOM LABEL");
        assert_eq!(labels[&owner].lines, ["CUSTOM", "LABEL"]);
        request_custom(&mut labels, owner, Default::default(), [0.; 3], 0);
        assert!(labels.is_empty());
    }
}
