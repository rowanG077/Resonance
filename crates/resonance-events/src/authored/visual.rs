//! Scoped visual recipes; choreography belongs to the authored task.
use super::effects::{CONTEXT, origin};
use super::projectile::{PROJECTILE, VECTOR, float, floats, projectile};
use super::*;
use crate::effect::{
    BillboardEffect, Fade, NEUTRAL_PALETTE, RefractionImage, RefractionPulse, SpriteOrientation,
};
use symphonia_script::authored::NativeVariant;

#[repr(i32)]
enum Image {
    Glow = 4,
    CameraDisc = 5,
    Trail = 22,
    Flame = 11,
    Blast = 12,
    Disc = 6,
    Star = 7,
    Orb = 10,
    ElectricArc = 14,
    Streak = 23,
    ElectricSpark = 42,
    Ring = 41,
}
const IMAGES: &[NativeVariant] = &[
    variant("Glow", Image::Glow as i32, &[]),
    variant("CameraDisc", Image::CameraDisc as i32, &[]),
    variant("Trail", Image::Trail as i32, &[]),
    variant("Flame", Image::Flame as i32, &[]),
    variant("Blast", Image::Blast as i32, &[]),
    variant("Disc", Image::Disc as i32, &[]),
    variant("Star", Image::Star as i32, &[]),
    variant("Orb", Image::Orb as i32, &[]),
    variant("ElectricArc", Image::ElectricArc as i32, &[]),
    variant("Streak", Image::Streak as i32, &[]),
    variant("ElectricSpark", Image::ElectricSpark as i32, &[]),
    variant("Ring", Image::Ring as i32, &[]),
];
const IMAGE: Type = Type::Enum {
    name: "game::effects::Image",
    variants: IMAGES,
};
pub(super) const FACING: Type = Type::Enum {
    name: "game::effects::Facing",
    variants: &[variant("Camera", 0, &[]), variant("World", 1, &[])],
};
const ANCHOR: Type = Type::Enum {
    name: "game::effects::Anchor",
    variants: &[
        variant("Center", 0, &[]),
        variant("Bottom", 1, &[]),
        variant("Top", 2, &[]),
        variant("UpperHalf", 3, &[]),
        variant("LowerHalf", 4, &[]),
    ],
};
pub(super) const BLEND: Type = Type::Enum {
    name: "game::effects::Blend",
    variants: &[
        variant("Alpha", 0, &[]),
        variant("Additive", 1, &[]),
        variant("Subtractive", 2, &[]),
    ],
};
pub(super) fn blend(value: i32) -> Result<u8, String> {
    match value {
        0..=2 => Ok(value as u8),
        _ => Err("invalid particle blend".into()),
    }
}
const FADE: Type = Type::Enum {
    name: "game::effects::Fade",
    variants: &[variant("Linear", 0, &[Type::F32]), variant("Tail", 1, &[])],
};
const SPRITE_BLEND: Type = Type::Enum {
    name: "game::effects::SpriteBlend",
    variants: &[variant("Recipe", 0, &[]), variant("Custom", 1, &[BLEND])],
};
const SPRITE: Type = Type::Record {
    name: "game::effects::Sprite",
    fields: &[
        field("image", IMAGE),
        field("facing", FACING),
        field("lifetime", Type::Ticks),
        field("width", Type::F32),
        field("height", Type::F32),
        field("growth", Type::F32),
        field("offset", VECTOR),
        field("rotation", VECTOR),
        field("velocity", VECTOR),
        field("inherit_velocity", Type::F32),
        field("palette", Type::I32),
        field("alpha", Type::I32),
        field("fade", FADE),
        field("angular_velocity", VECTOR),
        field("gravity", Type::F32),
        field("color", super::projectile::COLOR),
        field("anchor", ANCHOR),
        field("blend", SPRITE_BLEND),
        field("field_lighting", Type::Bool),
    ],
};
const DISTORTION_IMAGE: Type = Type::Enum {
    name: "game::effects::DistortionImage",
    variants: &[
        variant("Ripple", RefractionImage::Ripple as i32, &[]),
        variant("Air", RefractionImage::Air as i32, &[]),
    ],
};
const DISTORTION: Type = Type::Record {
    name: "game::effects::Distortion",
    fields: &[
        field("image", DISTORTION_IMAGE),
        field("facing", FACING),
        field("lifetime", Type::Ticks),
        field("size", Type::F32),
        field("growth", Type::F32),
        field("rotation", VECTOR),
        field("alpha", Type::F32),
        field("fade", FADE),
        field("offset", VECTOR),
    ],
};

pub(super) fn facing(value: i32) -> Result<SpriteOrientation, String> {
    match value {
        0 => Ok(SpriteOrientation::Camera),
        1 => Ok(SpriteOrientation::World),
        _ => Err("invalid sprite facing".into()),
    }
}
fn lifetime(value: i32) -> Result<u32, String> {
    u32::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| "nonpositive particle lifetime".into())
}
pub(super) const fn register(
    bindings: NativeBindings<FieldHost<'_>>,
) -> NativeBindings<FieldHost<'_>> {
    bindings
        .function(
            "game::effects::shadow",
            &[PROJECTILE, Type::F32, super::projectile::COLOR, Type::I32],
            None,
            false,
            |h, a, _| {
                projectile(h, a[0])?;
                let size = float(a[1])?;
                if size <= 0. {
                    return Err("invalid shadow size".into());
                }
                let rgba = super::projectile::color(a[2..6].try_into().unwrap())?;
                h.world.projectiles.get_mut(&a[0]).unwrap().shadow =
                    Some(crate::projectile::Shadow { size, rgba });
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::math::sin_degrees",
            &[Type::F32],
            Some(Type::F32),
            false,
            |_, a, _| {
                Ok(NativeResult::Continue(Some(
                    float(a[0])?.to_radians().sin().to_bits() as i32,
                )))
            },
        )
        .function(
            "game::math::cos_degrees",
            &[Type::F32],
            Some(Type::F32),
            false,
            |_, a, _| {
                Ok(NativeResult::Continue(Some(
                    float(a[0])?.to_radians().cos().to_bits() as i32,
                )))
            },
        )
        .function(
            "game::projectiles::velocity",
            &[PROJECTILE],
            Some(VECTOR),
            false,
            |h, a, _| {
                Ok(NativeResult::Values(
                    projectile(h, a[0])?
                        .velocity
                        .map(|v| v.to_bits() as i32)
                        .to_vec(),
                ))
            },
        )
        .function(
            "game::projectiles::chain_impulse",
            &[PROJECTILE, VECTOR, Type::F32],
            None,
            false,
            |h, a, _| {
                let shot = projectile(h, a[0])?;
                let f = floats::<4>(&a[1..])?;
                let source = shot.source;
                let instance = shot.source_instance;
                let impulse = crate::projectile::ChainImpulse {
                    acceleration: std::array::from_fn(|i| f[i] + shot.velocity[i] * f[3]),
                    operation: shot.operation.clone(),
                };
                if let Some(actor) = h.world.actors.get_mut(&source)
                    && actor.instance == instance
                {
                    // Controllers write after the source actor's model update.
                    actor.chain_impulses.insert(h.world.tick + 1, impulse);
                }
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::effects::sprite",
            &[CONTEXT, SPRITE],
            None,
            false,
            |h, a, _| {
                let &[
                    context,
                    image,
                    orientation,
                    life,
                    width,
                    height,
                    growth,
                    ox,
                    oy,
                    oz,
                    rx,
                    ry,
                    rz,
                    vx,
                    vy,
                    vz,
                    inherit,
                    palette,
                    alpha,
                    fade_kind,
                    fade_step,
                    ax,
                    ay,
                    az,
                    gravity,
                    red,
                    green,
                    blue,
                    anchor,
                    blend_kind,
                    blend_value,
                    lighting,
                ] = a
                else {
                    return Err("invalid sprite arguments".into());
                };
                let shot = origin(h, context)?;
                let recipe = match image {
                    id if IMAGES.iter().any(|variant| variant.tag == id) => id as u16,
                    _ => return Err("unsupported sprite image".into()),
                };
                let [width, height, growth] = floats(&[width, height, growth])?;
                let offset = floats::<3>(&[ox, oy, oz])?;
                let velocity = floats::<3>(&[vx, vy, vz])?;
                let inherit = float(inherit)?;
                let lifetime = lifetime(life)?;
                let fade = match fade_kind {
                    0 => Fade::Linear(float(fade_step)?),
                    1 => Fade::tail(lifetime),
                    _ => return Err("invalid particle fade".into()),
                };
                if width <= 0.
                    || height <= 0.
                    || !(0..resonance_content::effect::FIELD_PALETTE_COLORS as i32)
                        .contains(&palette)
                {
                    return Err("invalid sprite dimensions or palette".into());
                }
                let effect = BillboardEffect {
                    operation: Some(shot.operation.clone()),
                    field_lighting: lighting != 0,
                    recipe,
                    orientation: facing(orientation)?,
                    anchor: match anchor {
                        0 => resonance_content::effect::VerticalAnchor::Center,
                        1 => resonance_content::effect::VerticalAnchor::Bottom,
                        2 => resonance_content::effect::VerticalAnchor::Top,
                        3 => resonance_content::effect::VerticalAnchor::UpperHalf,
                        4 => resonance_content::effect::VerticalAnchor::LowerHalf,
                        _ => return Err("invalid sprite anchor".into()),
                    },
                    palette: Some(palette as u16),
                    born: h.world.tick,
                    lifetime,
                    position: std::array::from_fn(|i| shot.position[i] + offset[i]),
                    velocity: std::array::from_fn(|i| velocity[i] + shot.velocity[i] * inherit),
                    gravity: float(gravity)?,
                    rotation: floats(&[rx, ry, rz])?,
                    angular_velocity: floats(&[ax, ay, az])?,
                    size: [width, height],
                    size_delta: growth,
                    rgba: super::projectile::color(&[red, green, blue, alpha])?,
                    fade,
                    blend_mode: match blend_kind {
                        0 => None,
                        1 => Some(blend(blend_value)?),
                        _ => return Err("invalid sprite blend".into()),
                    },
                    ..Default::default()
                };
                h.world.emit_billboard(effect)?;
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::effects::distort",
            &[CONTEXT, DISTORTION],
            None,
            false,
            |h, a, _| {
                let &[
                    context,
                    image,
                    orientation,
                    life,
                    size,
                    growth,
                    rx,
                    ry,
                    rz,
                    alpha,
                    fade_kind,
                    fade_step,
                    ox,
                    oy,
                    oz,
                ] = a
                else {
                    return Err("invalid distortion arguments".into());
                };
                let shot = origin(h, context)?;
                let image = match image {
                    id if id == RefractionImage::Ripple as i32 => RefractionImage::Ripple,
                    id if id == RefractionImage::Air as i32 => RefractionImage::Air,
                    _ => return Err("invalid distortion image".into()),
                };
                let [size, growth, alpha] = floats(&[size, growth, alpha])?;
                let offset = floats::<3>(&[ox, oy, oz])?;
                if size <= 0. || !(0. ..=255.).contains(&alpha) {
                    return Err("invalid distortion dimensions or fade".into());
                }
                let lifetime = lifetime(life)?;
                let fade = match fade_kind {
                    0 => Fade::Linear(float(fade_step)?),
                    1 => Fade::tail(lifetime),
                    _ => return Err("invalid distortion fade".into()),
                };
                let effect = RefractionPulse {
                    operation: Some(shot.operation.clone()),
                    image,
                    palette: NEUTRAL_PALETTE,
                    orientation: facing(orientation)?,
                    rotation: floats(&[rx, ry, rz])?,
                    position: std::array::from_fn(|i| shot.position[i] + offset[i]),
                    born: h.world.tick,
                    lifetime: lifetime - 1,
                    size,
                    growth,
                    alpha,
                    fade,
                };
                h.world.emit_refraction(effect)?;
                Ok(NativeResult::Continue(None))
            },
        )
        .function(
            "game::projectiles::heading",
            &[PROJECTILE],
            Some(Type::F32),
            false,
            |h, a, _| {
                let v = projectile(h, a[0])?.velocity;
                Ok(NativeResult::Continue(Some(
                    v[0].atan2(-v[1]).to_degrees().to_bits() as i32,
                )))
            },
        )
}
