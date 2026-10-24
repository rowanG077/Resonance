use super::*;
use resonance_content::{credits, game_over};

impl Publisher<'_> {
    /// Compile native screen resources directly into their editable owners.
    pub fn screens(&mut self, extracted: &Path) -> Result<Vec<String>> {
        let (mut game_over, background) = crate::game_over::prepare(extracted)?;
        game_over.background.path = self.image_levels("ui/game-over/background", &background)?;
        self.json(game_over::document::PATH, &game_over)?;
        let mut paths = vec![game_over.background.path, game_over::document::PATH.into()];

        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let resources = crate::all_assets::credits::Resources::read(&executable)?;
        let read = |name: &str| -> Result<Vec<u8>> {
            let files = extracted.join("files");
            let path = crate::field_resources::resolve_path(&files, name)?;
            Ok(fs::read(files.join(path))?)
        };
        let program = crate::all_assets::credits::decode(&read(&resources.text)?)?;
        let images = crate::texture::decode_source(&read(&resources.pictures)?)?;
        let mut pictures = Vec::new();
        for (index, texture) in images.catalogue.textures.iter().enumerate() {
            let mut image = texture
                .as_ref()
                .context("missing credits picture")?
                .image(0)?;
            image.path = self.image_levels(
                &format!("ui/credits/pictures/picture-{index:02}"),
                &images.image_levels(index, 0)?,
            )?;
            paths.push(image.path.clone());
            pictures.push(image);
        }
        let music = "audio/music/credits/music.wav";
        self.check_spelling(music)?;
        let target = self.output.join(music);
        fs::create_dir_all(target.parent().context("credits music has no owner")?)?;
        let temporary = tempfile::NamedTempFile::new_in(target.parent().unwrap())?;
        let encoded = read(&resources.music)?;
        let format = crate::media::library::stream_format(&encoded)
            .context("unsupported credits recording")?;
        crate::media::field_audio::decode_voice_wave(temporary.path(), &encoded, format)?;
        let hash = crate::media::hash_file(temporary.path())?;
        if !self.already_published(music, &hash)? {
            crate::publication::install(temporary.path(), &target, &hash)?;
            self.remember(music, hash)?;
        }
        // This standalone recording plays at its native header clock.
        let definition = credits::document::Definition {
            version: credits::document::VERSION,
            program,
            pictures,
            music: credits::document::Recording {
                path: music.into(),
                sample_rate: None,
            },
            final_hold_ticks: 180,
        };
        self.json(credits::document::PATH, &definition)?;
        paths.extend([music.into(), credits::document::PATH.into()]);
        Ok(paths)
    }
}
