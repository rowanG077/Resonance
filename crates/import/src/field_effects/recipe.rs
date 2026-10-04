use super::*;
use serde::{Deserialize, Serialize};

/// Texture bindings retain native image roles until attached to physical images.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub effects: FieldEffects<Atlas>,
    pub shadow: resonance_content::field::ContactShadow<Atlas>,
    pub blink: resonance_content::effect::BlinkCycle,
    pub particles: BTreeMap<i32, resonance_content::effect::FlutterRecipe<Atlas>>,
}

impl Recipe {
    pub fn read(executable: &[u8]) -> Result<Self> {
        Ok(Self {
            archive: crate::all_assets::roles::effects_declaration(executable)?,
            effects: effects(executable)?,
            shadow: crate::field_shadow::read(),
            blink: blink(executable)?,
            particles: particles(executable)?,
        })
    }
}

fn blink(executable: &[u8]) -> Result<resonance_content::effect::BlinkCycle> {
    let mut frames = Vec::new();
    let table = dol::slice(executable, 0x801E3840, 20)?;
    ensure!(
        table[16..] == [0xFD, 0, 0, 1],
        "unexpected blink loop terminator"
    );
    for row in table[..16].chunks_exact(4) {
        let ticks = usize::from(u16::from_be_bytes([row[2], row[3]])) + 1;
        ensure!(
            row[0] < 16 && row[1] == 0 && ticks <= 1024,
            "invalid blink frame"
        );
        frames.extend(std::iter::repeat_n(row[0], ticks));
    }
    let blink = resonance_content::effect::BlinkCycle { frames };
    blink.validate()?;
    Ok(blink)
}

fn palette(executable: &[u8]) -> Result<Vec<[u8; 4]>> {
    const PALETTE: u32 = 0x8020_A240;
    dol::slice(
        executable,
        PALETTE,
        resonance_content::effect::FIELD_PALETTE_COLORS * 4,
    )?
    .chunks_exact(4)
    .map(|row| Ok(row.try_into()?))
    .collect()
}

fn effects(executable: &[u8]) -> Result<FieldEffects<Atlas>> {
    let sprite = |kind| sprites::read(executable, kind);
    let value = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(executable, address, 4)?.try_into()?,
        ))
    };
    let effects = FieldEffects {
        version: resonance_content::effect::FIELD_EFFECTS_VERSION,
        emote_texture: Atlas::Effect(1),
        status_texture: Atlas::Status,
        paralysis: EmoteTrack {
            anchor: dol::text(executable, 0x8017A498)?,
            missing_anchor_offset: [0.; 3],
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
        palette: palette(executable)?,
        sprites: [
            0, 1, 4, 5, 6, 7, 8, 10, 11, 12, 14, 22, 23, 41, 42, 52, 53, 54, 68, 69,
        ]
        .into_iter()
        .map(|kind| Ok((kind, sprite(kind)?)))
        .chain(std::iter::once(Ok((
            resonance_content::effect::WING_SPARK_SPRITE,
            SpriteRecipe {
                texture: Atlas::Effect(0),
                uv: [16., 192., 31., 207.].map(|v| v / 256.),
                additive: false,
                frames: Vec::new(),
                repeat: false,
            },
        ))))
        .collect::<Result<_>>()?,
        refraction: resonance_content::effect::RefractionRecipe {
            sprite: sprite(27)?,
            displacement: [value(0x801E3828)? * 2., value(0x801E3838)? * 2.],
        },
        air_refraction: sprite(9)?,
        emotes: emotes::tracks(),
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
) -> Result<BTreeMap<i32, resonance_content::effect::FlutterRecipe<Atlas>>> {
    use resonance_content::effect::FlutterRecipe;
    let float = |at| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(executable, at, 4)?.try_into()?,
        ))
    };
    let sprite = sprites::read(executable, 25)?;
    let recipe = FlutterRecipe {
        texture: sprite.texture,
        uv: sprite.uv,
        aspect_ratio: float(0x8035C1C0)?,
        palette: palette(executable)?,
        fall_speed: f64::from_be_bytes(dol::slice(executable, 0x8035C1C8, 8)?.try_into()?) as f32
            * float(0x8035C224)?,
        fall_variation: float(0x8035C224)? / float(0x8035C220)?,
        spin: f64::from_be_bytes(dol::slice(executable, 0x8035C228, 8)?.try_into()?) as f32,
    };
    recipe.validate()?;
    Ok([(25, recipe)].into())
}
