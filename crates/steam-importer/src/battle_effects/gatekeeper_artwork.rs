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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Language;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; reads only the installed Gatekeeper effect source"]
    fn installed_gatekeeper_dome_restores_coverage_without_changing_other_artwork_or_clocks()
    -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let image = pe::Image::parse(&source.executable)?;
        ensure!(
            monsters::package(&image, 245)?.eq_ignore_ascii_case("ba09.pac"),
            "Gatekeeper owner changed"
        );
        let package = source.require("ba09.pac")?;
        let body = source
            .require(&preview_model::reference(&package, 0x18)?.context("missing owner body")?)?;
        let bank = decode(
            preview_model::member(&package, 0x1cc)?.context("missing effect bank")?,
            &model::Rig::read(&body)?.joint_names(),
            |stride, declaration| {
                if let Declaration::Particle { visual, .. } = declaration
                    && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
                {
                    *palette_set = stride;
                }
                Ok(())
            },
        )?;
        let before = serde_json::to_value(&bank)?;
        let dome = Dome::read(245, &package, &bank)?.context("missing dome style")?;
        assert!(Dome::read(210, &package, &bank)?.is_none());
        let descriptor = "ba09_pic003_pic000.totexb_d";
        let native = crate::texture::Descriptor::read(&source.require(descriptor)?)?;
        let decoded = native.decode(&source.require(&native.payload)?)?;
        let directory = effect_atlas::Directory::read(&image)?;
        let plan = effect_atlas::frames::Plan::read(&bank, dome.member)?;
        let pixels = plan.image(&decoded.levels[0], directory.get(descriptor), 9)?;
        assert!(pixels.rgba.chunks_exact(4).any(|p| p[0] > 64 && p[3] < 64));
        let mut styled = pixels.clone();
        dome.normalize(2, dome.member, 9, &mut styled);
        for (native, styled) in pixels.rgba.chunks_exact(4).zip(styled.rgba.chunks_exact(4)) {
            assert_eq!(&native[..3], &styled[..3]);
            assert_eq!(styled[3], 255);
        }
        for (slot, member, palette) in [
            (2, dome.member + 1, 9),
            (2, dome.member, 2),
            (14, dome.member, 9),
        ] {
            let mut other = pixels.clone();
            dome.normalize(slot, member, palette, &mut other);
            assert_eq!(other, pixels);
        }
        let mut black = Pixels {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 0],
        };
        dome.normalize(2, dome.member, 9, &mut black);
        assert_eq!(black.rgba, [0, 0, 0, 255]);
        assert_eq!(serde_json::to_value(&bank)?, before);
        Ok(())
    }
}
