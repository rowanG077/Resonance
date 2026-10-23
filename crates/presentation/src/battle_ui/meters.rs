//! Authored item timer, escape dial, and Unison gauge.
use super::*;
use resonance_battle::{MAX_ESCAPE_GAUGE, MAX_UNISON_GAUGE, item::ITEM_COOLDOWN_TICKS};

pub(super) const COUNT: usize = 7;

pub(super) fn draw(frame: &BattleFrame, art: &Art, batches: &mut [Batch]) -> Result<()> {
    if frame.recognized_result.is_some() {
        return Ok(());
    }
    if frame.item_cooldown > 0 {
        let remaining = frame.item_cooldown;
        let fraction = f32::from(remaining) / f32::from(ITEM_COOLDOWN_TICKS);
        party::slanted_quad(
            &mut batches[0],
            [-12., 68., 160., 84.],
            [0.5; 4],
            9.,
            [[0, 0, 0, 128]; 4],
        );
        party::slanted_quad(
            &mut batches[0],
            [-12., 70., -12. + (168. * fraction).floor(), 82.],
            [0.5; 4],
            8.,
            art.feedback.item_bar,
        );
        for (caption, position, advance) in [
            (art.feedback.item_label.clone(), [4., 58.], 11.),
            (
                format!("{}\"{:02}.", remaining / 60, remaining % 60 * 100 / 60),
                [104., 76.],
                12.,
            ),
        ] {
            text::italic(
                &mut batches[1],
                &art.font,
                &caption,
                position,
                [14., 16.],
                advance,
                8.,
                art.feedback.item_text,
            )?;
        }
    }
    if let Some(escape) = frame.escape
        && (escape.requested || escape.gauge > 0)
    {
        let fraction = (f32::from(escape.gauge) / f32::from(MAX_ESCAPE_GAUGE)).clamp(0., 1.);
        let right = frame.camera.is_some_and(|camera| {
            frame
                .actors
                .iter()
                .find(|actor| {
                    actor.side == Side::Party && actor.control != resonance_battle::Control::Auto
                })
                .is_some_and(|actor| {
                    resonance_battle::project_screen_point(
                        camera,
                        [actor.position[0], 0., actor.position[2]],
                    )[0] > 320.
                })
        });
        let [x, y] = art.feedback.escape_positions[usize::from(right)].map(f32::from);
        let width = (128. * fraction).floor();
        batches[3].quad(
            [x + 4., y + 4., x + 148., y + 84.],
            [304., 208., 448., 288.],
            [0., 0., 0., 0.5],
        );
        batches[5].quad(
            [x + 8., y, x + 8. + width, y + 80.],
            [312., 208., 312. + width, 288.],
            hud_color([128, 48, 48, 255]),
        );
        batches[3].quad([x, y, x + 144., y + 80.], [304., 208., 448., 288.], [1.; 4]);
        let needle = &mut batches[4];
        needle.quad(
            [x + 64., y + 28., x + 80., y + 60.],
            [448., 208., 464., 240.],
            [1.; 4],
        );
        let pivot = [x + 72. - 320., 240. - (y + 172.)];
        let angle = -(55. * fraction - 27.5).to_radians();
        let (sin, cos) = angle.sin_cos();
        for vertex in &mut needle.positions {
            let [dx, dy] = [vertex[0] - pivot[0], vertex[1] - pivot[1]];
            vertex[0] = pivot[0] + cos * dx - sin * dy;
            vertex[1] = pivot[1] + sin * dx + cos * dy;
        }
    }
    if frame.unison_available {
        let fraction = (f32::from(frame.unison_gauge) / f32::from(MAX_UNISON_GAUGE)).clamp(0., 1.);
        let ready = frame.unison_gauge >= MAX_UNISON_GAUGE;
        let wave = ((frame.update * if ready { 4 } else { 2 }) as f32)
            .to_radians()
            .sin();
        let pulse = (160. + 32. * wave) as u8;
        let color = if ready {
            [pulse, 32, 32, 208]
        } else {
            [32, 32, pulse, 208]
        };
        batches[0].quad(
            [16., 372., 16. + (340. * fraction).floor(), 388.],
            [0.5; 4],
            hud_color(color),
        );
        batches[6].quad(
            [3., 267., 371., 411.],
            [0., 368., 352., 512.],
            [0., 0., 0., 0.5],
        );
        batches[2].quad([0., 264., 368., 408.], [0., 368., 352., 512.], [1.; 4]);
        if ready {
            let alpha = (160. + 80. * ((frame.update * 6) as f32).to_radians().sin()) as u8;
            let mut colors = art.feedback.unison_text;
            for color in &mut colors {
                color[3] = alpha;
            }
            for (position, colors) in [
                ([162., 374.], [[0, 0, 0, alpha / 2]; 2]),
                ([160., 372.], colors),
            ] {
                text::italic(
                    &mut batches[1],
                    &art.font,
                    &art.feedback.unison_ready,
                    position,
                    [13., 16.],
                    10.,
                    8.,
                    colors,
                )?;
            }
        }
    }
    Ok(())
}
