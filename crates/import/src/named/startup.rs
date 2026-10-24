use super::*;
use resonance_content::{BootAssets, BootTexture, TitleTexture, startup};

impl Publisher<'_> {
    pub fn startup(&mut self) -> Result<()> {
        let extracted = self.audio_source()?.original.extracted().to_path_buf();
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let recipe = crate::scene::title::Recipe::read(&extracted, &executable)?;
        let images = recipe.images.textures(&extracted)?;
        let mut textures = Vec::new();
        for (index, texture) in images.catalogue.textures.iter().enumerate() {
            let image = texture.as_ref().context("invalid title image")?.image(0)?;
            let path = self.image_levels(
                &format!("ui/title/textures/image-{index:02}"),
                &images.image_levels(index, 0)?,
            )?;
            textures.push(TitleTexture {
                index,
                path,
                width: image.width,
                height: image.height,
            });
        }
        let source = fs::read(extracted.join("files/boot.tpl"))?;
        let logos = crate::tpl::decode(&source)?;
        ensure!(logos.len() == 4, "expected four original startup textures");
        let mut boot = BootAssets {
            version: 1,
            textures: Vec::new(),
        };
        for (index, (width, height, pixels)) in logos.into_iter().enumerate() {
            let background = pixels[..3].try_into()?;
            let path = self.image_levels(
                &format!("ui/boot/textures/logo-{index}"),
                &[resonance_asset_writer::texture::Pixels {
                    width,
                    height,
                    rgba: pixels,
                }],
            )?;
            boot.textures.push(BootTexture {
                path,
                width,
                height,
                background,
            });
        }
        boot.validate()?;
        self.json(startup::BOOT, &boot)?;

        // The title owns its native reverb/fade arrangement independently of the
        // field song that uses the same score.
        self.startup_audio()?;
        self.json(
            startup::PATH,
            &startup::Definition {
                version: startup::VERSION,
                day_field: 474,
                night_field: 531,
                textures,
                music: "audio/music/title/music.json".into(),
                cues: "audio/effects/menu/cues.json".into(),
            },
        )
    }
}
