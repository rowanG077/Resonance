use super::catalogue::{Catalogue, ImageBinding};
use super::constructors::{Blend, Constructors};
use super::*;
use serde::{Deserialize, Serialize};

/// Texture bindings retain native image roles until attached to physical images.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Atlas {
    Effect(u8),
    Status,
}

impl AsRef<str> for Atlas {
    fn as_ref(&self) -> &str {
        match self {
            Self::Effect(_) => "effects",
            Self::Status => "status",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Recipe {
    pub archive: String,
    pub catalogue: Catalogue,
    pub constructors: Constructors,
    pub effects: FieldEffects<Atlas>,
    pub shadow: resonance_content::field::ContactShadow<Atlas>,
    pub blink: resonance_content::effect::BlinkCycle,
    pub particles: BTreeMap<i32, resonance_content::effect::FlutterRecipe<Atlas>>,
}

impl Recipe {
    pub fn read(executable: &[u8]) -> Result<Self> {
        let catalogue = Catalogue::read(executable)?;
        let constructors = Constructors::read(executable, &catalogue)?;
        Ok(Self {
            archive: crate::all_assets::roles::effects_declaration(executable)?,
            effects: effects(executable, &catalogue, &constructors)?,
            shadow: crate::field_shadow::read(executable)?,
            blink: blink(executable)?,
            particles: particles(executable, &catalogue, &constructors)?,
            catalogue,
            constructors,
        })
    }
}

fn blink(executable: &[u8]) -> Result<resonance_content::effect::BlinkCycle> {
    let word = |address| -> Result<u32> {
        Ok(u32::from_be_bytes(
            dol::slice(executable, address, 4)?.try_into()?,
        ))
    };
    // The initializer selects a sequence entry and randomizes its elapsed time.
    let entry = word(0x8001D4A8)?;
    let spread = word(0x8001D4CC)?;
    ensure!(
        entry >> 16 == 0x3800 && spread >> 16 == 0x1C00,
        "unexpected blink initializer"
    );
    let mut frames = Vec::new();
    let mut initial_tick = None;
    let table = dol::slice(executable, 0x801E3840, 20)?;
    ensure!(
        table[16..] == [0xFD, 0, 0, 1],
        "unexpected blink loop terminator"
    );
    for (index, row) in table[..16].chunks_exact(4).enumerate() {
        if index == (entry & 0xFFFF) as usize {
            initial_tick = Some(frames.len() as u16);
        }
        let ticks = usize::from(u16::from_be_bytes([row[2], row[3]])) + 1;
        ensure!(
            row[0] < 16 && row[1] == 0 && ticks <= 1024,
            "invalid blink frame"
        );
        frames.extend(std::iter::repeat_n(row[0], ticks));
    }
    let blink = resonance_content::effect::BlinkCycle {
        frames,
        initial_tick: initial_tick.ok_or_else(|| anyhow::anyhow!("invalid initial blink entry"))?,
        initial_spread: spread as u16,
    };
    blink.validate()?;
    Ok(blink)
}

fn sprite(
    catalogue: &Catalogue,
    constructors: &Constructors,
    kind: u16,
) -> Result<SpriteRecipe<Atlas>> {
    let constructor = constructors
        .kinds
        .get(&i32::from(kind))
        .context("missing particle constructor")?;
    let entry = catalogue.at_offset(
        constructor
            .sequence
            .context("particle needs an animation sequence")?,
    )?;
    ensure!(
        entry.frames.len() == 1,
        "sprite needs an animation controller"
    );
    let index = match entry.image {
        ImageBinding::Shared(index) => index,
        ImageBinding::Fallback(_) => 0,
        _ => anyhow::bail!("sprite needs a dynamic texture"),
    };
    let [u, v] = entry.frames[0].origin;
    let [width, height] = entry.dimensions;
    Ok(SpriteRecipe {
        texture: Atlas::Effect(index),
        uv: [
            u,
            v,
            u.wrapping_add(width.wrapping_sub(1)),
            v.wrapping_add(height.wrapping_sub(1)),
        ]
        .map(|v| f32::from(v) / 256.),
        additive: constructor.blend.unwrap_or(constructors.fresh_slot.blend) == Blend::Additive,
    })
}

fn effects(
    executable: &[u8],
    catalogue: &Catalogue,
    constructors: &Constructors,
) -> Result<FieldEffects<Atlas>> {
    let sprite = |kind| sprite(catalogue, constructors, kind);
    let value = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(executable, address, 4)?.try_into()?,
        ))
    };
    let effects = FieldEffects {
        version: 5,
        emote_texture: Atlas::Effect(1),
        status_texture: Atlas::Status,
        paralysis: EmoteTrack {
            anchor: dol::text(executable, 0x8017A498)?,
            missing_anchor_offset: [0.; 3],
            rotation: resonance_content::effect::EmoteRotation::Fixed,
            phase_count: 1,
            intro: Vec::new(),
            cycle: [16., 0.]
                .into_iter()
                .map(|y| {
                    Ok(vec![Sprite {
                        offset: [0., 0., value(0x8035AFD8)?],
                        size: [72., 24.],
                        uv: [137., y, 184., y + 15.].map(|v| v / 256.),
                        rotation: 0.,
                        vertical_anchor: VerticalAnchor::Center,
                        alpha: 255,
                    }])
                })
                .collect::<Result<_>>()?,
        },
        sprites: [0, 8, 10]
            .into_iter()
            .map(|kind| Ok((kind, sprite(kind)?)))
            .collect::<Result<_>>()?,
        refraction: resonance_content::effect::RefractionRecipe {
            sprite: sprite(27)?,
            displacement: [value(0x801E3828)? * 2., value(0x801E3838)? * 2.],
        },
        emotes: emotes::read(executable)?,
        // Each mouth frame lasts duration + 1 updates; 0xFD loops the sequence.
        // The dialogue player enables the sequence during text reveal and speech.
        mouth_cycle: {
            let table = dol::slice(executable, 0x801E3854, 12)?;
            ensure!(table[8] == 0xFD, "unexpected mouth cycle terminator");
            let mut frames = Vec::new();
            for row in table[..8].chunks_exact(4) {
                let ticks = usize::from(u16::from_be_bytes([row[2], row[3]])) + 1;
                ensure!(ticks <= 120 && row[0] < 8, "invalid mouth cycle frame");
                frames.extend(std::iter::repeat_n(row[0], ticks));
            }
            frames
        },
    };
    effects.validate()?;
    Ok(effects)
}

fn particles(
    executable: &[u8],
    catalogue: &Catalogue,
    constructors: &Constructors,
) -> Result<BTreeMap<i32, resonance_content::effect::FlutterRecipe<Atlas>>> {
    use resonance_content::effect::FlutterRecipe;
    let float = |at| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(executable, at, 4)?.try_into()?,
        ))
    };
    let sprite = sprite(catalogue, constructors, 25)?;
    let recipe = FlutterRecipe {
        texture: sprite.texture,
        uv: sprite.uv,
        aspect_ratio: float(0x8035C1C0)?,
        palette: constructors.palette.clone(),
        fall_speed: f64::from_be_bytes(dol::slice(executable, 0x8035C1C8, 8)?.try_into()?) as f32
            * float(0x8035C224)?,
        fall_variation: float(0x8035C224)? / float(0x8035C220)?,
        spin: f64::from_be_bytes(dol::slice(executable, 0x8035C228, 8)?.try_into()?) as f32,
    };
    recipe.validate()?;
    Ok([(25, recipe)].into())
}
