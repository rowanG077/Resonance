//! Depth-tested, world-space finisher choreography. One prewarmed draw is reused;
//! all geometry is sampled from the published finisher clock and skeletal hand.
use super::*;
use crate::weapon_geometry::Vertices;
use resonance_battle::{FatalColor, FatalSceneFrame, FatalStyle};

#[derive(Default)]
pub(super) struct FatalEffects {
    draw: Option<(Entity, Handle<Mesh>)>,
}

impl FatalEffects {
    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        assets: &mut AssetsForView,
        camera: Entity,
        report: &mut gpu::Report,
    ) {
        if self.draw.is_some() {
            return;
        }
        let surface = assets.surfaces.add(TitleSurface {
            blend: Some(Blend::Additive),
            depth_write: false,
            depth_equal: true,
            cull: resonance_content::CullFace::None,
            ..default()
        });
        let mut vertices = Vertices::default();
        vertices.quad(
            [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::ONE],
            [[0.; 2]; 4],
            [[1.; 4]; 4],
        );
        let mesh = assets.meshes.add(vertices.mesh());
        report.prepare(camera, mesh.id(), surface.id().untyped(), false, None);
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(surface),
                Transform::default(),
                Visibility::Hidden,
                NoFrustumCulling,
                RenderLayers::layer(LAYER),
                DrawOrder(Layer::Effects, usize::MAX, 0),
            ))
            .id();
        self.draw = Some((entity, mesh));
    }

    pub fn apply(
        &self,
        frame: &BattleFrame,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let Some((entity, mesh)) = &self.draw else {
            return Ok(());
        };
        commands.entity(*entity).insert(Visibility::Hidden);
        let Some(scene) = frame.fatal_strikes.as_ref().and_then(|f| f.scene.as_ref()) else {
            return Ok(());
        };
        let vertices = geometry(frame, scene)?;
        if vertices.is_empty() {
            return Ok(());
        }
        *meshes
            .get_mut(mesh)
            .context("missing prepared Fatal Strike mesh")? = vertices.mesh();
        commands.entity(*entity).insert(Visibility::Inherited);
        Ok(())
    }

    pub fn despawn(self, commands: &mut Commands) {
        if let Some((entity, _)) = self.draw {
            commands.entity(entity).despawn();
        }
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (t * (t * 6. - 15.) + 10.)
}

struct Ink {
    vertices: Vertices,
    eye: Vec3,
    color: Vec3,
}

impl Ink {
    fn glow(&mut self, center: Vec3, u: Vec3, v: Vec3, radius: f32, alpha: f32) {
        if alpha <= 0.001 {
            return;
        }
        let color = |a: f32| [self.color.x, self.color.y, self.color.z, a * 0.5];
        for i in 0..48 {
            let point = |i: usize| {
                let t = i as f32 / 48. * std::f32::consts::TAU;
                center + (u * t.cos() + v * t.sin()) * radius
            };
            self.vertices.quad(
                [center, point(i), center, point(i + 1)],
                [[0.; 2]; 4],
                [color(alpha), color(0.), color(alpha), color(0.)],
            );
        }
    }

    /// A feathered ribbon has zero opacity at its edges, rather than a stack of
    /// opaque white quads. Its thickness is in world units and obeys real depth.
    fn ribbon(&mut self, a: Vec3, b: Vec3, width: f32, opacity: f32) {
        if opacity <= 0.001 || width <= 0.001 {
            return;
        }
        let direction = b - a;
        let normal = direction
            .cross(self.eye - (a + b) * 0.5)
            .normalize_or_zero()
            * width
            * 0.5;
        let c = |alpha: f32| [self.color.x, self.color.y, self.color.z, alpha * 0.5];
        let center = c(opacity);
        let edge = c(0.);
        self.vertices.quad(
            [a - normal, a, b - normal, b],
            [[0.; 2]; 4],
            [edge, center, edge, center],
        );
        self.vertices.quad(
            [a, a + normal, b, b + normal],
            [[0.; 2]; 4],
            [center, edge, center, edge],
        );
    }

    fn ring(
        &mut self,
        center: Vec3,
        u: Vec3,
        v: Vec3,
        radius: f32,
        width: f32,
        alpha: f32,
        phase: f32,
    ) {
        let point = |i: usize| {
            let angle = i as f32 / 80. * std::f32::consts::TAU + phase;
            center + (u * angle.cos() + v * angle.sin()) * radius
        };
        for i in 0..80 {
            self.ribbon(point(i), point(i + 1), width, alpha);
        }
    }

    fn seal(&mut self, center: Vec3, radius: f32, alpha: f32, rotation: f32) {
        self.ring(center, Vec3::X, Vec3::Z, radius, 3., alpha, 0.);
        self.ring(
            center,
            Vec3::X,
            Vec3::Z,
            radius * 0.83,
            1.6,
            alpha * 0.7,
            0.,
        );
        for i in 0..12 {
            let angle = i as f32 * std::f32::consts::TAU / 12. + rotation;
            let radial = Vec3::new(angle.cos(), 0., angle.sin());
            let tangent = Vec3::new(-angle.sin(), 0., angle.cos());
            let p = center + radial * radius * 0.91;
            self.ribbon(p - radial * 4., p + radial * 4., 2., alpha);
            self.ribbon(p, p + tangent * 3., 1.6, alpha * 0.8);
        }
        for i in 0..6 {
            let point = |i: usize| {
                let a = i as f32 * std::f32::consts::TAU / 6. + rotation;
                center + Vec3::new(a.cos(), 0., a.sin()) * radius * 0.68
            };
            self.ribbon(point(i), point(i + 2), 1.8, alpha * 0.55);
        }
    }
}

fn geometry(frame: &BattleFrame, scene: &FatalSceneFrame) -> Result<Vertices> {
    let victim = frame
        .actors
        .get(scene.target.index())
        .context("missing caster finisher target")?;
    let caster = frame
        .models
        .iter()
        .find(|m| m.actor == scene.performer)
        .context("missing caster finisher body")?;
    if !scene.style.caster() {
        return physical_geometry(frame, scene);
    }
    let bone = scene
        .emitter
        .and_then(|b| caster.bones.get(usize::from(b)))
        .context("missing caster finisher hand")?;
    let hand = (Mat4::from_cols_array_2d(&caster.world) * Mat4::from_cols_array_2d(bone))
        .transform_point3(Vec3::ZERO);
    let center = Vec3::from_array(victim.target_center());
    let floor = Vec3::new(center.x, victim.position[1] + 1.5, center.z);
    let camera = frame.camera.context("missing caster finisher camera")?;
    let eye = Vec3::from_array(camera.eye);
    let forward = (center - hand).normalize_or_zero();
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    let age = f32::from(scene.age);
    let impact = f32::from(scene.impact_at);
    let after = (age - impact).max(0.);
    let gather = ease((age - 8.) / 30.) * (1. - ease((age - impact) / 10.));
    let release = ease((age - impact + 8.) / 8.);
    let fade = 1. - ease(after / 24.);
    let accent = Vec3::from_array(scene.color.rgb().map(|b| f32::from(b) / 255.));
    let family = if scene.style == FatalStyle::Sacred {
        Vec3::new(1., 0.82, 0.38)
    } else {
        Vec3::new(0.36, 0.66, 1.)
    };
    let mut ink = Ink {
        vertices: Vertices::default(),
        eye,
        color: family.lerp(accent, 0.38),
    };
    // The spell is gathered in the actual palm, with particles spiralling inward.
    let hand_view = (eye - hand).normalize_or_zero();
    let hand_right = hand_view.cross(Vec3::Y).normalize_or_zero();
    let hand_up = hand_right.cross(hand_view).normalize_or_zero();
    let spell = hand + forward * 10.;
    let radius = 8. + gather * 11.;
    ink.glow(spell, hand_right, hand_up, radius * 1.7, gather * 0.32);
    let tint = ink.color;
    ink.color = Vec3::ONE;
    ink.glow(spell, hand_right, hand_up, 4. + gather * 3., gather * 0.85);
    ink.color = tint;
    ink.ring(
        spell,
        hand_right,
        hand_up,
        radius,
        2.5,
        gather * 0.8,
        age * 0.035,
    );
    ink.ring(
        spell,
        hand_right * 0.85 + hand_view * 0.5,
        hand_up,
        radius * 1.4,
        1.6,
        gather * 0.45,
        -age * 0.03,
    );
    for i in 0..10 {
        let angle = i as f32 * 2.39996 + age * 0.055;
        let fraction = (age * 0.023 + i as f32 * 0.1).fract();
        let direction = hand_right * angle.cos() + hand_up * angle.sin();
        let point = spell + direction * (18. + (1. - fraction) * 45.);
        ink.ribbon(
            point,
            point - direction * 5.,
            2.,
            gather * fraction * (1. - fraction) * 2.,
        );
    }
    let target_scale = (victim.body_radius() * 1.2).clamp(48., 150.);
    let seal = ease((age - 24.) / 24.) * fade;
    ink.seal(floor, target_scale, seal * 0.7, age * 0.008);
    match scene.color {
        FatalColor::Blue => {
            let height = (center.y - floor.y) * 2. + 120.;
            let growth = ease((age - impact + 5.) / 5.);
            for i in 0..7 {
                let angle = i as f32 * std::f32::consts::TAU / 7. + 0.3;
                let offset = Vec3::new(angle.cos(), 0., angle.sin()) * target_scale * 0.38;
                let start = floor + offset;
                ink.ribbon(
                    start,
                    start + Vec3::Y * height * growth,
                    8. + release * 12.,
                    release * fade * 0.7,
                );
                // Each ascending shard is staggered and has its own short tail.
                let rise = ((age - impact + 6. + i as f32 * 1.6) / 22.).clamp(0., 1.);
                let p = start + Vec3::Y * height * rise;
                ink.ribbon(p - Vec3::Y * 12., p, 3.5, release * fade);
            }
            for i in 0..3 {
                let y = height * ((after / 28. + i as f32 * 0.22).fract());
                ink.ring(
                    floor + Vec3::Y * y,
                    Vec3::X,
                    Vec3::Z,
                    target_scale * (0.45 + after / 36.),
                    2.,
                    release * fade * 0.5,
                    0.,
                );
            }
        }
        FatalColor::Red => {
            let above = center + Vec3::Y * 280.;
            ink.seal(
                above,
                target_scale * 0.68,
                ease((age - 30.) / 16.) * fade,
                -age * 0.015,
            );
            let point = above.lerp(center, release);
            ink.ribbon(above, point, 18. + release * 20., release * fade * 0.8);
            for i in 0..5 {
                let angle = i as f32 * std::f32::consts::TAU / 5.;
                let offset = Vec3::new(angle.cos(), 0., angle.sin()) * target_scale * 0.35;
                ink.ribbon(
                    above + offset,
                    point + offset * 0.2,
                    4.,
                    release * fade * 0.55,
                );
            }
        }
        FatalColor::Green => {
            let end = hand.lerp(center, release);
            ink.ribbon(hand, end, 18. + release * 12., release * fade * 0.85);
            for i in 0..3 {
                let angle = age * 0.09 + i as f32 * std::f32::consts::TAU / 3.;
                let offset = (right * angle.cos() + up * angle.sin()) * 12.;
                ink.ribbon(hand + offset, end + offset * 0.5, 4., release * fade * 0.8);
                let along = ((age - impact + 8.) / 10. + i as f32 * 0.22).clamp(0., 1.);
                ink.ring(
                    hand.lerp(center, along),
                    right,
                    up,
                    18. + after * 1.3,
                    2.,
                    release * fade * 0.6,
                    0.,
                );
            }
        }
    }
    if age >= impact {
        let view = (eye - center).normalize_or_zero();
        let u = view.cross(Vec3::Y).normalize_or_zero();
        let v = u.cross(view).normalize_or_zero();
        ink.ring(center, u, v, 12. + after * 5.5, 5. * fade, fade * 0.9, 0.);
        ink.ring(
            floor,
            Vec3::X,
            Vec3::Z,
            20. + after * 6.,
            3. * fade,
            fade * 0.6,
            0.,
        );
        for i in 0..24 {
            let angle = i as f32 * 2.39996;
            let direction =
                (u * angle.cos() + v * angle.sin() + view * ((i % 3) as f32 - 1.) * 0.3)
                    .normalize();
            let p = center + direction * (8. + after * (3. + (i % 5) as f32));
            ink.ribbon(p - direction * (5. + after * 0.4), p, 3. * fade, fade * 0.8);
        }
    }
    Ok(ink.vertices)
}

/// Short contact effects leave the body and its follow-through legible. Their
/// axes live in the action plane; rotating the camera never rotates the strike.
fn physical_geometry(frame: &BattleFrame, scene: &FatalSceneFrame) -> Result<Vertices> {
    let victim = &frame.actors[scene.target.index()];
    let hero = &frame.actors[scene.performer.index()];
    let center = scene
        .contact
        .filter(|_| scene.style != FatalStyle::Chakram && scene.age >= scene.impact_at)
        .and_then(|point| {
            frame
                .models
                .iter()
                .find(|model| model.actor == scene.performer)
                .map(|model| {
                    Mat4::from_cols_array_2d(&model.world).transform_point3(Vec3::from_array(point))
                })
        })
        .unwrap_or_else(|| Vec3::from_array(victim.target_center()));
    let floor = Vec3::new(center.x, victim.position[1] + 1.5, center.z);
    let forward = Vec3::from_array(hero.facing_direction).normalize_or(Vec3::X);
    let side = forward.cross(Vec3::Y).normalize_or(Vec3::Z);
    let eye = Vec3::from_array(frame.camera.context("finisher requires camera")?.eye);
    let age = f32::from(scene.age);
    let after = age - f32::from(scene.impact_at);
    let accent = Vec3::from_array(scene.color.rgb().map(|v| f32::from(v) / 255.));
    let family = match scene.style {
        FatalStyle::Seraph => Vec3::new(1., 0.87, 0.55),
        FatalStyle::Spellblade => Vec3::new(1., 0.64, 0.32),
        FatalStyle::Seals => Vec3::new(0.85, 0.46, 1.),
        _ => Vec3::ONE,
    };
    let mut ink = Ink {
        vertices: Vertices::default(),
        eye,
        color: accent.lerp(family, 0.3),
    };
    let charge = ease((age - 28.) / 18.) * (1. - ease(after.max(0.) / 5.));
    let radius = victim.body_radius().clamp(25., 100.);
    if scene.style == FatalStyle::Seals {
        let body = frame
            .models
            .iter()
            .find(|m| m.actor == scene.performer)
            .context("missing seal performer")?;
        let bone = scene
            .emitter
            .and_then(|b| body.bones.get(usize::from(b)))
            .context("missing talisman hand")?;
        let palm = (Mat4::from_cols_array_2d(&body.world) * Mat4::from_cols_array_2d(bone))
            .transform_point3(Vec3::ZERO);
        ink.ring(palm, side, Vec3::Y, 15., 2., charge * 0.85, -age * 0.03);
        for i in 0..4 {
            let a = i as f32 * std::f32::consts::FRAC_PI_2 + age * 0.015;
            let p = palm + (side * a.cos() + Vec3::Y * a.sin()) * 10.;
            ink.ribbon(
                p,
                p + (side * a.sin() - Vec3::Y * a.cos()) * 6.,
                1.8,
                charge,
            );
        }
        ink.seal(floor, radius * 1.35, charge * 0.65, 0.2);
    } else if scene.style == FatalStyle::Chakram {
        for weapon in frame
            .weapons
            .iter()
            .filter(|w| w.owner == scene.performer && w.slot.is_some_and(|s| s.is_primary()))
        {
            let point = Mat4::from_cols_array_2d(&weapon.world).transform_point3(Vec3::ZERO);
            if scene.age >= scene.impact_at.saturating_sub(12) && after < 32. {
                ink.ring(
                    point,
                    forward,
                    side,
                    16.,
                    1.8,
                    (1. - ease((after - 20.) / 12.)) * 0.6,
                    age * 0.2,
                );
            }
        }
    }
    if !(0. ..=22.).contains(&after) {
        return Ok(ink.vertices);
    }
    let fade = 1. - ease(after / 22.);
    let view = (eye - center).normalize_or_zero();
    let u = view.cross(Vec3::Y).normalize_or(Vec3::X);
    let v = u.cross(view).normalize_or(Vec3::Y);
    ink.glow(center, u, v, (18. + after * 4.) * fade, fade * 0.3);
    let direction = match scene.color {
        FatalColor::Blue => (forward * 0.38 + Vec3::Y).normalize(),
        FatalColor::Red => (forward * 0.38 - Vec3::Y).normalize(),
        FatalColor::Green => forward,
    };
    let broad = if scene.style == FatalStyle::HeavyAxe {
        1.5
    } else {
        1.
    };
    match scene.style {
        FatalStyle::Seals => {
            // The talisman stamps a square seal into the target and then breaks
            // into four strokes along the chosen formula direction.
            ink.ring(
                center,
                side,
                Vec3::Y,
                radius * (0.7 + after * 0.035),
                2.2,
                fade,
                0.,
            );
            for i in 0..4 {
                let a = std::f32::consts::FRAC_PI_4 + i as f32 * std::f32::consts::FRAC_PI_2;
                let b = a + std::f32::consts::FRAC_PI_2;
                let point =
                    |a: f32| center + (side * a.cos() + Vec3::Y * a.sin()) * (radius + after * 1.5);
                ink.ribbon(point(a), point(b), 3. * fade, fade);
            }
            ink.seal(floor, radius * 1.4 + after * 2., fade * 0.65, 0.2);
        }
        FatalStyle::Martial => {
            ink.ring(center, side, Vec3::Y, 8. + after * 6., 6. * fade, fade, 0.);
            for i in 0..7 {
                let a = (i as f32 - 3.) * 0.13;
                let p = center + side * a * 65.;
                ink.ribbon(
                    p - direction * (20. + after * 2.),
                    p + direction * (65. + after * 6.),
                    4. * fade,
                    fade * 0.8,
                );
            }
        }
        FatalStyle::Chakram => {
            ink.ring(
                center,
                side,
                Vec3::Y,
                radius + after * 4.,
                5. * fade,
                fade,
                0.,
            );
            ink.ring(
                center,
                forward,
                Vec3::Y,
                radius * 0.8 + after * 3.,
                3. * fade,
                fade * 0.7,
                0.,
            );
        }
        _ => {
            let count = if scene.style == FatalStyle::TwinBlades {
                2
            } else {
                1
            };
            for blade in 0..count {
                let sign = if blade == 0 { 1. } else { -1. };
                let axis = (direction + side * (sign * 0.22)).normalize();
                let normal = axis.cross(side).normalize_or(Vec3::Y);
                let reach = (radius * 1.4 + after * 5.) * broad;
                let start = center - axis * reach;
                let end = center + axis * reach;
                ink.ribbon(start, end, 18. * fade * broad, fade * 0.65);
                let color = ink.color;
                ink.color = Vec3::ONE;
                ink.ribbon(start, end, 3. * fade, fade);
                ink.color = color;
                // Curved edges give the cut a swept volume, rather than a flat
                // screen-space line painted over the character.
                let point = |i: usize| {
                    let t = i as f32 / 24.;
                    center
                        + axis * ((t - 0.5) * reach * 2.)
                        + normal * ((t * std::f32::consts::PI).sin() * reach * 0.18)
                };
                for i in 0..24 {
                    ink.ribbon(point(i), point(i + 1), 4. * fade, fade * 0.65)
                }
            }
            if matches!(scene.style, FatalStyle::Seraph | FatalStyle::Spellblade) {
                ink.ring(
                    center,
                    side,
                    Vec3::Y,
                    radius * 0.8 + after * 2.8,
                    2. * fade,
                    fade * 0.7,
                    0.,
                );
            }
        }
    }
    ink.ring(
        floor,
        Vec3::X,
        Vec3::Z,
        10. + after
            * (if scene.style == FatalStyle::HeavyAxe {
                7.
            } else {
                4.
            }),
        3. * fade,
        fade * 0.55,
        0.,
    );
    for i in 0..18 {
        let angle = i as f32 * 2.39996;
        let ray =
            (side * angle.cos() + Vec3::Y * angle.sin() + forward * ((i % 3) as f32 - 1.) * 0.35)
                .normalize();
        let p = center + ray * (8. + after * (3. + (i % 4) as f32));
        ink.ribbon(p - ray * (7. + after * 0.35), p, 3. * fade, fade * 0.85);
    }
    Ok(ink.vertices)
}
