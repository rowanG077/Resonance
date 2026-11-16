//! Original geometric formulas and cinematic framing, driven entirely by simulation state.
use super::*;
use resonance_battle::FatalColor;

pub(super) struct Prompt<'a> {
    pub label: &'a str,
    pub icon: Option<[u32; 4]>,
    pub batch: &'a mut Batch,
}

fn line(batch: &mut Batch, a: [f32; 2], b: [f32; 2], width: f32, color: [u8; 4]) {
    let delta = Vec2::from_array(b) - Vec2::from_array(a);
    let normal = Vec2::new(-delta.y, delta.x).normalize_or_zero() * width * 0.5;
    let a = Vec2::from_array(a);
    let b = Vec2::from_array(b);
    let start = batch.positions.len() as u32;
    for point in [a - normal, b - normal, b + normal, a + normal] {
        batch.positions.push([point.x - 320., 240. - point.y, 0.]);
    }
    batch.uv.extend([[0., 0.]; 4]);
    batch.colors.extend([hud_color(color); 4]);
    batch
        .indices
        .extend([start, start + 2, start + 1, start, start + 3, start + 2]);
}

fn ring(
    batch: &mut Batch,
    center: [f32; 2],
    radius: f32,
    rotation: f32,
    fraction: f32,
    color: [u8; 4],
) {
    for segment in 0..48 {
        if segment as f32 / 48. > fraction {
            break;
        }
        let point = |step: f32| {
            let angle = rotation + step / 48. * std::f32::consts::TAU;
            [
                center[0] + angle.cos() * radius,
                center[1] + angle.sin() * radius,
            ]
        };
        line(
            batch,
            point(segment as f32),
            point(segment as f32 + 0.8),
            2.,
            color,
        );
    }
}

fn symbol(batch: &mut Batch, center: [f32; 2], color: FatalColor, size: f32, tint: [u8; 4]) {
    let [x, y] = center;
    let points = match color {
        FatalColor::Blue => [
            [x - size, y + size * 0.4],
            [x, y - size],
            [x + size, y + size * 0.4],
        ],
        FatalColor::Red => [
            [x - size, y - size * 0.4],
            [x, y + size],
            [x + size, y - size * 0.4],
        ],
        FatalColor::Green => [
            [x - size * 0.4, y - size],
            [x + size, y],
            [x - size * 0.4, y + size],
        ],
    };
    line(batch, points[0], points[1], 3., tint);
    line(batch, points[1], points[2], 3., tint);
}

pub(super) fn draw(
    frame: &BattleFrame,
    anchors: &[ActorMarkers],
    prompt: Prompt<'_>,
    bounds: [f32; 4],
    font: &BitmapFont,
    solid: &mut Batch,
    labels: &mut Batch,
) -> Result<()> {
    let Some(fatal) = &frame.fatal_strikes else {
        return Ok(());
    };
    if frame.recognized_result.is_some() {
        return Ok(());
    }
    if let Some(scene) = &fatal.scene {
        let [left, top, right, bottom] = bounds;
        let age = f32::from(scene.age);
        let envelope = (age / 10.).min(1.) * (f32::from(scene.duration - scene.age) / 16.).min(1.);
        let height = 48. * envelope;
        solid.quad(
            [left, top, right, top + height],
            [0.; 4],
            hud_color([0, 0, 0, 255]),
        );
        solid.quad(
            [left, bottom - height, right, bottom],
            [0.; 4],
            hud_color([0, 0, 0, 255]),
        );
        let [r, g, b] = scene.color.rgb().map(|v| v / 2);
        let alpha = (envelope * 255.) as u8;
        let offset = (1. - (age / 14.).min(1.)) * 70.;
        text::italic(
            labels,
            font,
            "FATAL STRIKE",
            [28. - offset, 16.],
            [18., 23.],
            16.,
            5.,
            [[r, g, b, alpha], [128, 128, 128, alpha]],
        )?;
        if scene.chain > 1 {
            text::glyphs(
                labels,
                font,
                &format!("CHAIN {}", scene.chain),
                [480., 22.],
                [14., 17.],
                [r, g, b, alpha],
            )?;
        }
        let impact_age = scene.age.saturating_sub(scene.impact_at);
        if scene.age >= scene.impact_at && impact_age < 3 {
            solid.quad(
                [left, top + height, right, bottom - height],
                [0.; 4],
                hud_color([128, 128, 128, (3 - impact_age) as u8 * 22]),
            );
        }
        if scene.age >= scene.impact_at && impact_age < 9 {
            let alpha = (8 - impact_age) as u8 * 5;
            solid.quad(
                [left, top + height, right, bottom - height],
                [0.; 4],
                hud_color([r, g, b, alpha]),
            );
        }
        return Ok(());
    }
    let Some(camera) = frame.camera else {
        return Ok(());
    };
    for target in &fatal.targets {
        let Some(anchor) = anchors
            .get(target.actor.index())
            .and_then(|markers| markers.head.or(markers.target))
        else {
            continue;
        };
        let eye = Vec3::from_array(camera.eye);
        let forward = (Vec3::from_array(camera.focus) - eye).normalize_or_zero();
        if (Vec3::from_array(anchor) - eye).dot(forward) <= 0. {
            continue;
        }
        let [x, y] = resonance_battle::project_screen_point(camera, anchor);
        let y = y * 480. / 448. - 30.;
        if !(-80. ..720.).contains(&x) || !(-80. ..560.).contains(&y) {
            continue;
        }
        if let Some(opening) = target.opening {
            let [r, g, b] = opening.color.rgb().map(|value| value / 2);
            let birth = f32::from(90_u16.saturating_sub(opening.remaining));
            let enter = (birth / 12.).clamp(0., 1.);
            let reveal = 1. - (1. - enter).powi(3);
            let scale = reveal + (enter * std::f32::consts::PI).sin() * 0.18;
            let pulse = 1. + (f32::from(opening.remaining) * 0.22).sin() * 0.06;
            ring(
                solid,
                [x, y],
                31. * pulse * scale,
                frame.update as f32 * 0.025,
                1.,
                [r, g, b, 230],
            );
            ring(
                solid,
                [x, y],
                36. * scale,
                -std::f32::consts::FRAC_PI_2,
                f32::from(opening.remaining) / 90.,
                [r, g, b, 255],
            );
            symbol(
                solid,
                [x, y],
                opening.color,
                15. * scale,
                [r, g, b, (reveal * 255.) as u8],
            );
            if birth < 18. {
                let fade = (1. - birth / 18.).max(0.);
                ring(
                    solid,
                    [x, y],
                    28. + birth * 2.8,
                    0.,
                    1.,
                    [r, g, b, (fade * 220.) as u8],
                );
                for ray in 0..8 {
                    let angle = ray as f32 * std::f32::consts::FRAC_PI_4;
                    let direction = Vec2::new(angle.cos(), angle.sin());
                    let center = Vec2::new(x, y);
                    line(
                        solid,
                        (center + direction * (34. + birth * 1.7)).to_array(),
                        (center + direction * (45. + birth * 2.8)).to_array(),
                        2. * fade,
                        [128, 128, 128, (fade * 230.) as u8],
                    );
                }
            }
            let selected_by_player = frame.actors.iter().enumerate().any(|(index, actor)| {
                actor.side == Side::Party
                    && actor.control != resonance_battle::Control::Auto
                    && actor.available()
                    && frame.targets.get(index).copied().flatten() == Some(target.actor)
            });
            if !selected_by_player {
                continue;
            }
            if let Some([u, v, w, h]) = prompt.icon {
                solid.quad(
                    [x - 15., y + 37., x + 15., y + 66.],
                    [0.; 4],
                    hud_color([6, 9, 16, 200]),
                );
                prompt.batch.quad(
                    [x - 12., y + 39., x + 12., y + 63.],
                    [u as f32, v as f32, (u + w) as f32, (v + h) as f32],
                    [1.; 4],
                );
            } else {
                text::glyphs(
                    labels,
                    font,
                    prompt.label,
                    [x - prompt.label.len() as f32 * 4., y + 40.],
                    [8., 12.],
                    [128, 128, 128, 255],
                )?;
            }
        } else if frame
            .targets
            .iter()
            .flatten()
            .any(|actor| *actor == target.actor)
        {
            for color in FatalColor::ALL {
                let [r, g, b] = color.rgb().map(|value| value / 2);
                let yy = y + color.index() as f32 * 5.;
                let fraction = f32::from(target.remaining[color.index()])
                    / f32::from(target.capacity[color.index()]);
                solid.quad(
                    [x - 24., yy, x + 24., yy + 3.],
                    [0.; 4],
                    hud_color([10, 10, 18, 190]),
                );
                solid.quad(
                    [x - 24., yy, x - 24. + 48. * fraction, yy + 3.],
                    [0.; 4],
                    hud_color([r, g, b, 230]),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_strokes_face_the_hud_camera_in_every_direction() {
        for end in [[100., 0.], [0., 100.], [-100., 20.], [25., -80.]] {
            let mut batch = Batch::default();
            line(&mut batch, [0.; 2], end, 2., [128; 4]);
            for triangle in batch.indices.as_chunks::<3>().0 {
                let a = Vec3::from_array(batch.positions[triangle[0] as usize]);
                let b = Vec3::from_array(batch.positions[triangle[1] as usize]);
                let c = Vec3::from_array(batch.positions[triangle[2] as usize]);
                assert!((b - a).cross(c - a).z > 0., "stroke was back-face culled");
            }
        }
    }
}
