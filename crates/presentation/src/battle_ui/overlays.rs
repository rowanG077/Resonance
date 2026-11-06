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

pub(super) struct TpWarning {
    remaining: u32,
}
impl TpWarning {
    fn new() -> Self {
        Self { remaining: 60 }
    }

    fn advance(&mut self, elapsed: u32) {
        self.remaining = self.remaining.saturating_sub(elapsed);
    }

    pub(super) fn draw(
        &self,
        font: &BitmapFont,
        [x, y]: [f32; 2],
        batch: &mut Batch,
    ) -> Result<()> {
        let [x, y] = [x.trunc(), y.trunc()];
        let color = if self.remaining & 4 == 0 {
            [128, 128, 128, 255]
        } else {
            [96, 96, 128, 255]
        };
        for (label, offset, size, skew) in [
            ("TP", [-28., 0.], [16., 20.], 8.),
            ("XX", [2., 6.], [14., 14.], 6.),
        ] {
            text::italic(
                batch,
                font,
                label,
                [x + offset[0], y + offset[1]],
                size,
                14.,
                skew,
                [color; 2],
            )?;
        }
        Ok(())
    }
}

pub(super) fn advance_tp_warnings(
    warnings: &mut BTreeMap<ActorId, TpWarning>,
    frame: &resonance_battle::BattleFrame,
    elapsed: u32,
) -> Result<()> {
    use resonance_battle::{Cue, Rejection, Side};
    if frame.recognized_result.is_some()
        || frame
            .cues
            .iter()
            .any(|cue| matches!(cue, Cue::CombatRetired))
    {
        warnings.clear();
        return Ok(());
    }
    warnings.retain(|_, warning| {
        warning.advance(elapsed);
        warning.remaining != 0
    });
    for cue in &frame.cues {
        if let Cue::Rejected {
            actor,
            reason: Rejection::InsufficientTp,
        } = cue
        {
            let owner = frame
                .actors
                .get(actor.index())
                .ok_or_else(|| anyhow::anyhow!("unknown TP warning actor"))?;
            if owner.side == Side::Party {
                warnings.insert(*actor, TpWarning::new());
            }
        }
    }
    Ok(())
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
    fn tp_warnings_filter_refresh_pause_and_expire_independently() -> Result<()> {
        use super::super::fixtures;
        use resonance_battle::{Cue, Rejection, Side};
        let mut warnings = BTreeMap::new();
        let actor = ActorId::from_index(0)?;
        let ally = ActorId::from_index(1)?;
        let enemy = ActorId::from_index(2)?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.actors.push(frame.actors[0].clone());
        frame.actors.push(frame.actors[0].clone());
        frame.actors[2].side = Side::Enemy;
        frame.cues.push(Cue::Rejected {
            actor,
            reason: Rejection::Busy,
        });
        frame.cues.push(Cue::Rejected {
            actor: enemy,
            reason: Rejection::InsufficientTp,
        });
        advance_tp_warnings(&mut warnings, &frame, 1)?;
        assert!(warnings.is_empty());
        frame.cues = [actor, ally]
            .map(|actor| Cue::Rejected {
                actor,
                reason: Rejection::InsufficientTp,
            })
            .into();
        advance_tp_warnings(&mut warnings, &frame, 1)?;
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[&actor].remaining, 60);
        frame.cues.truncate(1);
        advance_tp_warnings(&mut warnings, &frame, 59)?;
        assert_eq!(warnings[&actor].remaining, 60);
        assert_eq!(warnings[&ally].remaining, 1);
        frame.cues.clear();
        advance_tp_warnings(&mut warnings, &frame, 0)?;
        assert_eq!(warnings[&actor].remaining, 60);
        assert_eq!(warnings[&ally].remaining, 1);
        advance_tp_warnings(&mut warnings, &frame, 1)?;
        assert!(!warnings.contains_key(&ally));
        Ok(())
    }

    #[test]
    fn battle_end_clears_tp_warnings_even_with_a_rejection() -> Result<()> {
        use super::super::fixtures;
        use resonance_battle::{BattleResult, Cue, Rejection};
        let actor = ActorId::from_index(0)?;
        for retired in [false, true] {
            let mut warnings = BTreeMap::from([(actor, TpWarning::new())]);
            let mut frame = fixtures::frame(fixtures::actor(Default::default()));
            frame.cues.push(Cue::Rejected {
                actor,
                reason: Rejection::InsufficientTp,
            });
            if retired {
                frame.cues.push(Cue::CombatRetired);
            } else {
                frame.recognized_result = Some(BattleResult::Victory);
            }
            advance_tp_warnings(&mut warnings, &frame, 0)?;
            assert!(warnings.is_empty());
        }
        Ok(())
    }

    #[test]
    fn tp_warning_draws_fixed_cells_and_blinks() -> Result<()> {
        let font = BitmapFont {
            texture: String::new(),
            width: 32,
            height: 32,
            line_height: 20,
            glyphs: ['T', 'P', 'X']
                .map(|ch| {
                    (
                        ch,
                        resonance_content::font::Glyph {
                            rect: [0, 0, 10, 20],
                            advance: 10,
                        },
                    )
                })
                .into(),
        };
        let mut warning = TpWarning::new();
        let mut batch = Batch::default();
        warning.draw(&font, [100.9, 200.9], &mut batch)?;
        assert_eq!(batch.positions.len(), 16);
        assert_eq!(batch.positions[0][0] + 320., 80.);
        assert_eq!(batch.positions[8][0] + 320., 108.);
        let initial_colors = batch.colors.clone();
        warning.advance(4);
        let mut batch = Batch::default();
        warning.draw(&font, [100.9, 200.9], &mut batch)?;
        assert_ne!(batch.colors, initial_colors);
        warning.advance(4);
        let mut batch = Batch::default();
        warning.draw(&font, [100.9, 200.9], &mut batch)?;
        assert_eq!(batch.colors, initial_colors);
        Ok(())
    }

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
