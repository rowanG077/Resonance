//! Publish editable pixels without changing authored mip levels.
use super::*;
use resonance_asset_writer::texture;
use resonance_content::texture::{ColorSpace, Definition, Mips, VERSION};

#[cfg(test)]
mod tests;

impl Publisher<'_> {
    pub(super) fn image(&mut self, source: &str, name: &str) -> Result<String> {
        library::validate_path(source)?;
        library::validate_path(name)?;
        let image = format!("{name}.png");
        if source.ends_with(".png") {
            self.copy(source, &image)?;
            return Ok(image);
        }
        let definition = if source.ends_with(".texture.json") {
            let bytes = fs::read(self.source.join(source))?;
            let mut definition: Definition = serde_json::from_slice(&bytes)?;
            definition.validate()?;
            self.copy(&definition.image, &image)?;
            definition.image = image;
            if let Mips::Authored { levels } = &mut definition.mips {
                for (index, path) in levels.iter_mut().enumerate() {
                    let destination = format!("{name}-mip-{}.png", index + 1);
                    self.copy(path, &destination)?;
                    *path = destination;
                }
            }
            definition
        } else {
            ensure!(source.ends_with(".ktx2"), "unsupported texture {source}");
            let levels = texture::ktx2(&fs::read(self.source.join(source))?)
                .with_context(|| format!("publish pixels from {source}"))?;
            return self.image_levels(name, &levels);
        };
        let path = format!("{name}.texture.json");
        self.json(&path, &definition)?;
        Ok(path)
    }

    pub(super) fn image_levels(
        &mut self,
        name: &str,
        levels: &[texture::Pixels],
    ) -> Result<String> {
        library::validate_path(name)?;
        texture::validate_levels(levels)?;
        let image = format!("{name}.png");
        self.write(&image, &levels[0].png()?)?;
        if levels.len() == 1 {
            return Ok(image);
        }
        let mut paths = Vec::new();
        for (index, level) in levels.iter().enumerate().skip(1) {
            let path = format!("{name}-mip-{index}.png");
            self.write(&path, &level.png()?)?;
            paths.push(path);
        }
        let path = format!("{name}.texture.json");
        self.json(
            &path,
            &Definition {
                version: VERSION,
                image,
                color_space: ColorSpace::Linear,
                mips: Mips::Authored { levels: paths },
            },
        )?;
        Ok(path)
    }
}
