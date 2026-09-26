//! Original actor labels (219D8/65E24/664BC) and feedback controller (C5B4).
pub(super) use super::notice::{Notice, NoticeKind, NoticeOwner};
use super::{Art, Batch, quad, results};
use anyhow::{Result, ensure};
use resonance_battle::{Actor, HitResult};

/// Four party slots and eight enemy slots each own one source label record.
pub(super) const LABEL_CAPACITY: usize = 12;

#[derive(Clone, Copy)]
pub(super) enum LabelKind {
    Level,
    CompoundEx,
    Critical,
}

pub(super) struct ActorLabel {
    pub position: [f32; 3],
    kind: LabelKind,
    tick: u32,
    remaining: u16,
    alpha: u8,
    fade: u16,
    expansion: u8,
    previous: [i16; 2],
}

/// 219D8 replaces the actor's single record, including an earlier label's fade.
pub(super) fn request(
    labels: &mut Vec<(usize, ActorLabel)>,
    owner: usize,
    kind: LabelKind,
    actor: &Actor,
    tick: u32,
) -> Result<()> {
    ensure!(owner < LABEL_CAPACITY, "invalid actor label owner");
    if let Some((_, label)) = labels.iter_mut().find(|(id, _)| *id == owner) {
        label.restart(kind, actor, tick);
    } else {
        labels.push((owner, ActorLabel::new(kind, actor, tick)));
        labels.sort_by_key(|(id, _)| *id);
    }
    Ok(())
}

pub(super) fn contact(
    labels: &mut Vec<(usize, ActorLabel)>,
    owner: usize,
    actor: &Actor,
    hit: HitResult,
    tick: u32,
) -> Result<()> {
    // 3C85C..3C878: the resolved critical flag requests kind1 for45 visits.
    // The separately emitted common16 effect is an empty timeline.
    if hit.critical {
        request(labels, owner, LabelKind::Critical, actor, tick)?;
    }
    Ok(())
}

impl ActorLabel {
    pub fn new(kind: LabelKind, actor: &Actor, tick: u32) -> Self {
        // 219D8 uses the raw model profile point, without body scale or heading.
        let mut position =
            std::array::from_fn(|i| actor.position[i] + actor.body.center_offset[i] * 2.);
        // Only original kinds7/13 apply the result-label vertical adjustment.
        if !matches!(kind, LabelKind::Critical) {
            position[1] -= 8.;
        }
        Self {
            position,
            kind,
            tick,
            remaining: match kind {
                LabelKind::Level => 60,
                LabelKind::CompoundEx => 90,
                LabelKind::Critical => 45,
            },
            alpha: 255,
            fade: 0,
            expansion: 16,
            previous: [0; 2],
        }
    }

    /// 219D8 replaces the label but leaves its previously projected coordinate
    /// intact; an EX label can replace a level label during its fading tail.
    pub fn restart(&mut self, kind: LabelKind, actor: &Actor, tick: u32) {
        let previous = self.previous;
        *self = Self::new(kind, actor, tick);
        self.previous = previous;
    }

    pub fn advance(&mut self, tick: u32) {
        // 65E24 visits this before actor callbacks. A request emitted after the
        // shared world visit therefore retains its initial pose for that frame.
        let visits = tick
            .wrapping_sub(self.tick)
            .min(u32::from(self.remaining) + 32);
        for _ in 0..visits {
            if self.remaining != 0 {
                self.expansion = self.expansion.saturating_sub(2);
                self.remaining -= 1;
            } else if self.alpha != 0 {
                self.fade += 1;
                self.alpha = self.alpha.saturating_sub(8);
            }
        }
        self.tick = tick;
    }

    pub fn visible(&self) -> bool {
        self.alpha != 0
    }

    fn projected(&mut self, mut point: [i16; 2]) -> [i16; 2] {
        // 664BC stabilizes only result kinds7/13, not critical kind1.
        if !matches!(self.kind, LabelKind::Critical) {
            for (value, previous) in point.iter_mut().zip(self.previous) {
                if (i32::from(*value) - i32::from(previous)).abs() <= 1 {
                    *value = previous;
                }
            }
        }
        self.previous = point;
        point
    }

    /// `projected` is 53230's native viewport coordinate, truncated to signed
    /// halfwords. 664BC applies no 448-to-480 scale or display-border offset.
    pub fn draw(
        &mut self,
        art: &Art,
        projected: [i16; 2],
        solid: &mut Batch,
        font: &mut Batch,
    ) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }
        let projected = self.projected(projected);
        let lines = &art.overlays.actor_lines[self.kind as usize];
        let height = (24 - i32::from(self.fade) * 2).max(0);
        let skew = i32::from(self.fade) * 2 + 12;
        let width = lines[0].len() as i32 * 12;
        let centers = [
            ((lines[0].len() + 1) >> 1) as i32 * 12,
            ((lines[1].len() - 1) >> 1) as i32 * 12,
        ];
        let x = centers.map(|center| i32::from(projected[0]) - center);
        let y = [
            i32::from(projected[1]) - 30 + (20 - height),
            i32::from(projected[1]) - 8 + (20 - height),
        ];
        let mut colors = art.overlays.actor_colors;
        for color in &mut colors {
            color[3] = self.alpha;
        }
        let mut bars = art.overlays.actor_bars;
        for index in [2, 3, 6, 7] {
            bars[index][3] = self.alpha >> 1;
        }
        sprite_gradient(
            solid,
            [
                (x[0] - 24 - i32::from(self.fade) * 4) as f32,
                y[0] as f32,
                (width + 40 + i32::from(self.fade) * 4) as f32,
                height as f32,
            ],
            skew as f32,
            bars[..4].try_into().unwrap(),
        );
        sprite_gradient(
            solid,
            [
                (x[1] - 8) as f32,
                y[1] as f32,
                (width + 40 + i32::from(self.fade) * 4) as f32,
                height as f32,
            ],
            skew as f32,
            bars[4..].try_into().unwrap(),
        );
        for row in 0..2 {
            results::glyphs(
                font,
                art,
                &lines[row],
                [x[row] as f32, y[row] as f32],
                [18., height as f32],
                skew as f32,
                14.,
                colors,
                None,
            )?;
        }
        if self.expansion != 0 {
            let expansion = f32::from(self.expansion);
            for color in &mut colors {
                color[3] = 255 - self.expansion * 8;
            }
            for row in 0..2 {
                results::glyphs(
                    font,
                    art,
                    &lines[row],
                    [x[row] as f32, y[row] as f32],
                    [18. + expansion * 2., height as f32 + expansion * 2.],
                    skew as f32 + expansion,
                    14. + expansion * 2.,
                    colors,
                    None,
                )?;
            }
        }
        Ok(())
    }
}

fn sprite_gradient(batch: &mut Batch, [x, y, w, h]: [f32; 4], skew: f32, colors: [[u8; 4]; 4]) {
    quad(batch, [x, y, x + w, y + h], [0.5; 4], skew, colors);
}

/// Immutable renderer input. A generation begins with a capture-only draw;
/// repeated GPU draws at the same update must not recursively evolve feedback.
#[derive(Clone, Copy, Default)]
pub(crate) struct FeedbackFrame {
    pub generation: u64,
    pub update: u32,
    pub amount: u8,
    pub alpha: u8,
}

#[derive(Default)]
pub(super) struct Feedback {
    frame: FeedbackFrame,
    remaining: u16,
}

impl Feedback {
    pub fn start(&mut self, generation: u64, tick: u32) {
        self.frame = FeedbackFrame {
            generation,
            update: tick,
            amount: 0,
            alpha: 128,
        };
        self.remaining = 45;
    }

    /// C5B4 runs at the end of each shared world visit. Initialization may occur
    /// before or after that visit; the supplied source clock retains that order.
    pub fn advance(&mut self, tick: u32) {
        let visits = tick
            .wrapping_sub(self.frame.update)
            .min(u32::from(self.remaining) + 8);
        for _ in 0..visits {
            if self.remaining != 0 {
                self.frame.amount = (self.frame.amount + 1).min(8);
                self.remaining -= 1;
            } else {
                self.frame.amount = self.frame.amount.saturating_sub(1);
            }
        }
        self.frame.update = tick;
    }

    pub fn frame(&self) -> FeedbackFrame {
        self.frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_label_keeps_emission_pose_and_expires_after_hold_then_fade() {
        let mut label = ActorLabel {
            position: [0.; 3],
            kind: LabelKind::Level,
            tick: 100,
            remaining: 60,
            alpha: 255,
            fade: 0,
            expansion: 16,
            previous: [0; 2],
        };
        label.advance(100);
        assert_eq!(
            (label.remaining, label.expansion, label.alpha),
            (60, 16, 255)
        );
        label.advance(101);
        assert_eq!(
            (label.remaining, label.expansion, label.alpha),
            (59, 14, 255)
        );
        label.advance(160);
        assert_eq!(
            (label.remaining, label.expansion, label.alpha, label.fade),
            (0, 0, 255, 0)
        );
        label.advance(161);
        assert_eq!((label.alpha, label.fade), (247, 1));
        label.advance(191);
        assert!(label.visible());
        assert_eq!(label.alpha, 7);
        label.advance(192);
        assert!(!label.visible());
    }

    fn actor() -> Actor {
        Actor {
            side: resonance_battle::Side::Enemy,
            control: Default::default(),
            activity: Default::default(),
            availability: Default::default(),
            overlimit: 0,
            overlimit_active: false,
            guard: Default::default(),
            hp: 800,
            max_hp: 800,
            tp: 0,
            max_tp: 0,
            hud: Default::default(),
            luck: 0,
            stats: Default::default(),
            elements: Default::default(),
            affinities: [resonance_battle::Affinity::Normal; 9],
            attack_power: 100,
            physical_arte_boost: false,
            recovery: Default::default(),
            petrified: false,
            position: [100., 4., 20.],
            heading: 90.,
            facing_direction: [1., 0., 0.],
            effect_scale: 1.,
            framing: Default::default(),
            movement: Default::default(),
            hit_stop: 0,
            reaction: Default::default(),
            body: resonance_battle::Body {
                center_offset: [0., 50., 0.],
                ..Default::default()
            },
        }
    }

    fn hit(critical: bool) -> HitResult {
        HitResult {
            amount: 76,
            hp_change: -76,
            critical,
            affinity: resonance_battle::Affinity::Normal,
            guard: resonance_battle::GuardResult::None,
            auto_guard: false,
            armored: false,
            protection: resonance_battle::HitProtection::None,
        }
    }

    #[test]
    fn critical_label_uses_emission_pose_and_45_hud_visits_then_fade() -> Result<()> {
        let mut labels = Vec::new();
        let mut victim = actor();
        contact(&mut labels, 3, &victim, hit(true), 169)?;
        let label = &mut labels[0].1;
        assert_eq!(label.position, [100., 104., 20.]);
        assert_eq!(
            (label.remaining, label.expansion, label.alpha),
            (45, 16, 255)
        );
        label.advance(169);
        assert_eq!((label.remaining, label.expansion), (45, 16));
        victim.position[0] += 50.;
        label.advance(170);
        assert_eq!(label.position, [100., 104., 20.]);
        assert_eq!((label.remaining, label.expansion), (44, 14));
        label.advance(214);
        assert_eq!((label.remaining, label.fade, label.alpha), (0, 0, 255));
        label.advance(215);
        assert_eq!((label.fade, label.alpha), (1, 247));
        label.advance(246);
        assert!(!label.visible());
        Ok(())
    }

    #[test]
    fn resolved_critical_replaces_one_actor_record_without_result_projection_hold() -> Result<()> {
        let mut labels = Vec::new();
        let victim = actor();
        request(&mut labels, 3, LabelKind::Level, &victim, 1)?;
        assert_eq!(labels[0].1.position[1], 96.);
        assert_eq!(labels[0].1.projected([200, 100]), [200, 100]);
        assert_eq!(labels[0].1.projected([201, 99]), [200, 100]);
        // An ordinary contact leaves the currently displayed label intact.
        contact(&mut labels, 3, &victim, hit(false), 2)?;
        assert_eq!(labels[0].1.remaining, 60);
        contact(&mut labels, 3, &victim, hit(true), 2)?;
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].1.position[1], 104.);
        assert_eq!(labels[0].1.remaining, 45);
        assert_eq!(labels[0].1.previous, [200, 100]);
        assert_eq!(labels[0].1.projected([201, 99]), [201, 99]);
        labels[0].1.advance(20);
        contact(&mut labels, 3, &victim, hit(true), 20)?;
        assert_eq!((labels[0].1.remaining, labels[0].1.expansion), (45, 16));
        request(&mut labels, 3, LabelKind::CompoundEx, &victim, 20)?;
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].1.remaining, 90);
        assert_eq!(labels[0].1.projected([202, 98]), [201, 99]);
        // The source draws labels for every party/enemy actor, in roster order.
        for owner in (0..LABEL_CAPACITY).rev() {
            contact(&mut labels, owner, &victim, hit(true), 21)?;
        }
        assert_eq!(
            labels.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );
        assert!(contact(&mut labels, 12, &victim, hit(true), 21).is_err());
        assert_eq!(labels.len(), 12);
        Ok(())
    }

    #[test]
    fn feedback_grows_then_holds_before_eight_visit_release() {
        let mut feedback = Feedback::default();
        feedback.start(7, u32::MAX - 3);
        feedback.advance(u32::MAX - 3);
        assert_eq!(feedback.frame().amount, 0);
        feedback.advance(u32::MAX - 2);
        assert_eq!(feedback.frame().amount, 1);
        feedback.advance(41);
        assert_eq!((feedback.remaining, feedback.frame().amount), (0, 8));
        feedback.advance(42);
        assert_eq!(feedback.frame().amount, 7);
        feedback.advance(49);
        assert_eq!(feedback.frame().amount, 0);
        assert_eq!(feedback.frame().generation, 7);
    }
}
