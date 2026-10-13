//! Presentation-owned damage numbers, recovery notices, and gauge trails.
use super::{Art, Batch, party, text};
use anyhow::{Context, Result};
use resonance_battle::{Affinity, BattleFrame, Cue, GuardResult, RecoveryKind, Side};

const MAX_FLOATING: usize = 4;
const FLOATING_HOLD: u32 = 35;
const FADE_STEP: u32 = 8;
const FADE_DURATION: u32 = 255_u32.div_ceil(FADE_STEP);
const FLOATING_LIFETIME: u32 = FLOATING_HOLD + FADE_DURATION;
const RECOVERY_HOLD: u32 = 20;
const RECOVERY_LIFETIME: u32 = RECOVERY_HOLD + FADE_DURATION;

#[derive(Default)]
pub(super) struct Numbers {
    pub(super) actors: Vec<ActorNumbers>,
}

#[derive(Default)]
pub(super) struct ActorNumbers {
    pub(super) trails: [i16; 2],
    floating: Vec<FloatingNumber>,
    next_slot: usize,
    recovery: [Option<RecoveryNumber>; 2],
}

struct FloatingNumber {
    value: i32,
    style: NumberStyle,
    age: u32,
    origin: [f32; 3],
    slot: usize,
}

#[derive(Clone, Copy)]
enum NumberStyle {
    Normal,
    Guarded,
    Emphasized,
    Recovery(RecoveryKind),
}

struct RecoveryNumber {
    value: i32,
    age: u32,
}

impl ActorNumbers {
    fn advance(&mut self, current: [i16; 2], elapsed: u32) {
        for (trail, value) in self.trails.iter_mut().zip(current) {
            *trail = trail.saturating_sub(elapsed.min(100) as i16).max(value);
        }
        self.floating.retain_mut(|number| {
            number.age = number.age.saturating_add(elapsed);
            number.age < FLOATING_LIFETIME
        });
        for number in &mut self.recovery {
            if let Some(active) = number {
                active.age = active.age.saturating_add(elapsed);
                if active.age >= RECOVERY_LIFETIME {
                    *number = None;
                }
            }
        }
    }

    fn float(&mut self, value: i32, style: NumberStyle, origin: [f32; 3]) {
        if self.floating.iter().all(|hit| hit.age >= FLOATING_HOLD) {
            self.next_slot = 0;
        }
        let slot = self.next_slot;
        self.next_slot = (slot + 1) % MAX_FLOATING;
        self.floating.retain(|hit| hit.slot != slot);
        self.floating.push(FloatingNumber {
            value: value.max(0),
            style,
            age: 0,
            origin,
            slot,
        });
    }

    fn recover(&mut self, kind: RecoveryKind, value: i32, origin: [f32; 3], side: Side) {
        self.float(value, NumberStyle::Recovery(kind), origin);
        if side != Side::Party {
            return;
        }
        let slot = &mut self.recovery[kind as usize];
        let value = slot
            .as_ref()
            .map_or(value, |old| old.value.saturating_add(value));
        *slot = Some(RecoveryNumber { value, age: 0 });
    }
}

impl Numbers {
    pub(super) fn advance(&mut self, frame: &BattleFrame, elapsed: u32) {
        self.actors
            .resize_with(frame.actors.len(), ActorNumbers::default);
        for (numbers, actor) in self.actors.iter_mut().zip(&frame.actors) {
            numbers.advance([actor.hp_percent(), actor.tp_percent()], elapsed);
        }
        for cue in &frame.cues {
            match *cue {
                Cue::Hit { actor, result, .. }
                    if frame.recognized_result.is_none() && result.is_damage() =>
                {
                    let victim = &frame.actors[actor.index()];
                    let style = if victim.hp <= 0 {
                        NumberStyle::Normal
                    } else if result.guard != GuardResult::None
                        || result.affinity == Affinity::Resistant
                    {
                        NumberStyle::Guarded
                    } else if result.critical || result.affinity == Affinity::Weak {
                        NumberStyle::Emphasized
                    } else {
                        NumberStyle::Normal
                    };
                    self.actors[actor.index()].float(result.amount, style, victim.effect_origin());
                }
                Cue::IncidentalDamage { actor, amount } => {
                    self.actors[actor.index()].float(
                        amount,
                        NumberStyle::Normal,
                        frame.actors[actor.index()].effect_origin(),
                    );
                }
                Cue::Recovered {
                    actor,
                    kind,
                    nominal,
                    ..
                } => {
                    let recipient = &frame.actors[actor.index()];
                    self.actors[actor.index()].recover(
                        kind,
                        nominal,
                        recipient.effect_origin(),
                        recipient.side,
                    );
                }
                _ => {}
            }
        }
    }
}

fn alpha(age: u32, hold: u32) -> u8 {
    255_u32.saturating_sub(age.saturating_sub(hold) * FADE_STEP) as u8
}

fn draw_floating(
    batch: &mut Batch,
    art: &Art,
    hit: &FloatingNumber,
    side: Side,
    projected: [f32; 2],
) -> Result<()> {
    // Expand on contact, settle in six updates, then drift right while fading.
    let pulse = 12_u32.saturating_sub(hit.age * 2);
    let (size, mut colors) = match hit.style {
        NumberStyle::Recovery(kind) => (art.damage.sizes[0], art.recovery_colors[kind as usize]),
        style => (
            art.damage.sizes[match style {
                NumberStyle::Guarded => 1,
                NumberStyle::Emphasized => 2,
                _ => 0,
            }],
            art.damage.colors[usize::from(side == Side::Enemy)],
        ),
    };
    let size = size.map(|value| f32::from(value) + 2. * pulse as f32);
    for color in &mut colors {
        color[3] = alpha(hit.age, FLOATING_HOLD).saturating_sub((pulse * 16) as u8);
    }
    let trailing_digits = hit.value.checked_ilog10().unwrap_or(0);
    let slot = hit.slot as f32;
    let position = [
        projected[0] + 8. - slot * 8. + hit.age.saturating_sub(FLOATING_HOLD) as f32
            - trailing_digits as f32 * 14.,
        projected[1] - slot * 12. - 32.,
    ];
    text::italic(
        batch,
        &art.font,
        &hit.value.to_string(),
        position,
        size,
        size[0],
        10. + pulse as f32,
        colors,
    )
}

fn recovery(
    batch: &mut Batch,
    art: &Art,
    number: &RecoveryNumber,
    slot: usize,
    row: usize,
) -> Result<()> {
    let [right, y] = party::recovery_position(slot, row);
    let age = number.age as f32;
    // Horizontal arc with an initial speed of 1.5 pixels per update.
    let drift = ((24. * age - age * (age - 1.) / 2.) / 16.).floor();
    let mut colors = art.recovery_colors[row];
    for color in &mut colors {
        color[3] = alpha(number.age, RECOVERY_HOLD);
    }
    // Draw right to left so italic digits overlap in the correct order.
    let mut value = number.value.max(0) as u32;
    let mut index = 0;
    loop {
        text::italic(
            batch,
            &art.font,
            &(value % 10).to_string(),
            [right + drift - 16. - index as f32 * 13., y],
            [16., 24.],
            16.,
            12.,
            colors,
        )?;
        value /= 10;
        if value == 0 {
            break;
        }
        index += 1;
    }
    Ok(())
}

impl super::Artwork {
    pub(super) fn render_numbers(
        &mut self,
        frame: &BattleFrame,
        commands: &mut super::Commands,
        meshes: &mut super::Assets<super::Mesh>,
    ) -> Result<()> {
        let mut floating = Batch::default();
        let mut recovered = Batch::default();
        let mut party_slot = 0;
        for (actor, numbers) in frame.actors.iter().zip(&self.numbers.actors) {
            for hit in &numbers.floating {
                let camera = frame
                    .camera
                    .context("floating number requires battle camera")?;
                let [x, scene_y] = resonance_battle::project_screen_point(camera, hit.origin);
                // Floating feedback shares the scene's expanded display, like target markers.
                let projected = [
                    x,
                    scene_y * resonance_content::HEIGHT as f32
                        / resonance_content::SCENE_HEIGHT as f32,
                ];
                draw_floating(&mut floating, &self.art, hit, actor.side, projected)?;
            }
            if actor.side == Side::Party {
                for (row, number) in numbers.recovery.iter().enumerate() {
                    if let Some(number) = number {
                        recovery(&mut recovered, &self.art, number, party_slot, row)?;
                    }
                }
                party_slot += 1;
            }
        }
        self.ui.floating.upload(floating, commands, meshes)?;
        self.ui.recovery.upload(recovered, commands, meshes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_hits_keep_four_anchors_and_expire_with_their_animation() {
        let mut numbers = ActorNumbers::default();
        for value in 1..=5 {
            numbers.float(value, NumberStyle::Normal, [value as f32, 50., 0.]);
        }
        assert_eq!(
            numbers.floating.iter().map(|n| n.value).collect::<Vec<_>>(),
            [2, 3, 4, 5]
        );
        assert_eq!(numbers.floating.last().unwrap().origin, [5., 50., 0.]);
        numbers.advance([50, 25], 0);
        assert!(numbers.floating.iter().all(|n| n.age == 0));
        numbers.advance([50, 25], FLOATING_HOLD);
        assert_eq!(alpha(FLOATING_HOLD, FLOATING_HOLD), 255);
        assert_eq!(alpha(FLOATING_HOLD + 1, FLOATING_HOLD), 247);
        numbers.float(6, NumberStyle::Normal, [10., 50., 0.]);
        assert_eq!(numbers.floating.last().unwrap().slot, 0);
        numbers.advance([50, 25], FLOATING_LIFETIME);
        assert!(numbers.floating.is_empty());
    }

    #[test]
    fn recovery_accumulates_until_expiry_and_trails_follow_vitals() {
        let mut numbers = ActorNumbers::default();
        numbers.recover(RecoveryKind::Hp, 12, [10., 80., 0.], Side::Party);
        numbers.recover(RecoveryKind::Hp, 7, [20., 80., 0.], Side::Party);
        numbers.advance([100, 80], 0);
        assert_eq!(numbers.recovery[0].as_ref().unwrap().value, 19);
        assert_eq!(
            numbers.floating.iter().map(|n| n.value).collect::<Vec<_>>(),
            [12, 7]
        );
        assert_eq!(numbers.floating[1].origin, [20., 80., 0.]);
        assert!(matches!(
            numbers.floating[1].style,
            NumberStyle::Recovery(RecoveryKind::Hp)
        ));
        numbers.advance([40, 30], 10);
        assert_eq!(numbers.trails, [90, 70]);
        numbers.advance([40, 30], 100);
        assert_eq!(numbers.trails, [40, 30]);
        assert!(numbers.recovery[0].is_none());
        assert!(numbers.floating.is_empty());
        numbers.recover(RecoveryKind::Hp, 5, [0.; 3], Side::Party);
        assert_eq!(numbers.recovery[0].as_ref().unwrap().value, 5);
        numbers.recover(RecoveryKind::Tp, 0, [0.; 3], Side::Enemy);
        assert!(numbers.recovery[1].is_none());
        let number = numbers.floating.last().unwrap();
        assert_eq!(number.value, 0);
        assert!(matches!(
            number.style,
            NumberStyle::Recovery(RecoveryKind::Tp)
        ));
    }
}
