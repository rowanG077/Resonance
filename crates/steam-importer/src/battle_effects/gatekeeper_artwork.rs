//! Restore Gatekeeper's fiery dome coverage in the GameCube presentation style.
//! Its Steam RGB artwork is retained; only this single-mask additive image's
//! alpha becomes opaque. Native births, geometry, UVs and RGBA rates stay intact.
use super::*;
use resonance_asset_writer::texture::Pixels;
use resonance_content::battle_effect::visual::ParticleShape;
use sha2::{Digest, Sha256};

pub(super) struct Dome {
    member: usize,
}

impl Dome {
    pub(super) fn read(id: u16, package: &[u8], definition: &Definition) -> Result<Option<Self>> {
        if id != 245 {
            return Ok(None);
        }
        let bank =
            preview_model::member(package, 0x1cc)?.context("missing Gatekeeper effect bank")?;
        // This source bank proves actor0 and all of its users: program4 births
        // the dome with its fixed selector9, without a palette override.
        ensure!(
            Sha256::digest(bank)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                == "74605a619cec5e0d182e2e579a4258a1a957c9239a079a099a05e0f3841a7a93"
                && preview_model::reference(package, 0x1d0)?
                    .is_some_and(|name| name.eq_ignore_ascii_case("ba09_pic003_pic000.totexb_d")),
            "Gatekeeper dome source changed"
        );
        let member = definition
            .template_names
            .iter()
            .position(|name| name == "additive-hemisphere")
            .context("missing Gatekeeper dome declaration")?;
        let Declaration::Particle { template, visual } = &definition.templates[member] else {
            anyhow::bail!("Gatekeeper dome declaration changed kind")
        };
        ensure!(
            visual.geometry == (ParticleShape::Hemisphere { columns: 16 })
                && visual.blend == 1
                && visual.texture
                    == (ParticleTexture::Atlas {
                        slot: 2,
                        dual: false,
                        palette_set: 0
                    })
                && template.state.palettes == [9, 0]
                && template.state.uv == [128, 128, 64, 64]
                && template.state.colors == [[128, 128, 128, 255], [0, 0, 0, 0]]
                && template.lifetime == Some(17)
                && template.fade_from == 1
                && template.fade == [0, 0, 0, 32],
            "Gatekeeper dome material changed"
        );
        Ok(Some(Self { member }))
    }

    pub(super) fn normalize(&self, slot: u8, member: usize, palette: u16, pixels: &mut Pixels) {
        if slot == 2 && member == self.member && palette == 9 {
            // GC's corresponding single color palette is opaque. Steam stores
            // almost identical fiery RGB with low alpha, attenuating the upper
            // volume in the shared material. Black additive texels stay black.
            for pixel in pixels.rgba.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
    }
}
