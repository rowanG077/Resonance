//! Bind field effects to shared textures and parsed animation recipes.
mod catalogue;
mod constructors;
mod emotes;
mod recipe;
#[cfg(test)]
mod tests;
use crate::{dol, write_atomic};
use anyhow::{Context, Result, ensure};
pub(crate) use recipe::Atlas;
use recipe::Recipe;
use resonance_content::effect::{
    BlinkCycle, EmoteTrack, FieldEffects, FlutterRecipe, RefractionRecipe, Sprite, SpriteRecipe,
    VerticalAnchor,
};
use resonance_content::field::ContactShadow;
use std::fs;
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook_recipe(executable: &[u8], output: &Path) -> Result<Vec<String>> {
    let path = "embedded/field-effects.json";
    write_atomic(
        &output.join(path),
        &serde_json::to_vec(&Recipe::read(executable)?)?,
    )?;
    Ok(vec![path.into()])
}

/// The native decompressor accepts exactly one CAB member, regardless of its name.
pub(crate) fn textures(
    extracted: &Path,
    output: &Path,
    declaration: &str,
) -> Result<(Vec<crate::texture::Texture>, String)> {
    let files = extracted.join("files");
    let path = crate::all_assets::roles::declared_path(&files, declaration)?;
    let bytes = fs::read(files.join(path))?;
    let hash = crate::digest(&bytes);
    let (_, payload) = crate::compression::cabinet(&bytes)?;
    let textures = crate::texture::decode_source(&payload)?
        .write(output)?
        .textures
        .into_iter()
        .map(|texture| texture.context("incomplete decoded effect texture"))
        .collect::<Result<_>>()?;
    Ok((textures, hash))
}

pub(crate) struct Prepared {
    pub path: String,
    pub files: Vec<String>,
    pub blink: BlinkCycle,
    pub shadow: ContactShadow,
    pub particles: BTreeMap<i32, FlutterRecipe>,
}

pub(crate) fn cook(extracted: &Path, output: &Path) -> Result<Prepared> {
    let recipe = Recipe::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    prepare(extracted, output, recipe)
}

fn prepare(extracted: &Path, output: &Path, recipe: Recipe) -> Result<Prepared> {
    recipe.catalogue.encode()?;
    let (textures, _) = textures(extracted, output, &recipe.archive)?;
    let status = crate::font::system_texture(extracted, output)?.path;
    let image = |atlas| -> Result<String> {
        match atlas {
            Atlas::Effect(index) => Ok(textures
                .get(usize::from(index))
                .context("missing field effect atlas")?
                .image(0)?
                .path),
            Atlas::Status => Ok(status.clone()),
        }
    };
    let sprite = |value: SpriteRecipe<Atlas>| -> Result<_> {
        Ok(SpriteRecipe {
            texture: image(value.texture)?,
            uv: value.uv,
            additive: value.additive,
        })
    };
    let source = recipe.shadow;
    let shadow = ContactShadow {
        texture: image(source.texture)?,
        uv_size: source.uv_size,
        half_size: source.half_size,
        height_offset: source.height_offset,
        alpha: source.alpha,
        anchor_node: source.anchor_node,
    };
    shadow.validate()?;
    let source = recipe.effects;
    let effects = FieldEffects {
        version: source.version,
        emote_texture: image(source.emote_texture)?,
        status_texture: image(source.status_texture)?,
        paralysis: source.paralysis,
        sprites: source
            .sprites
            .into_iter()
            .map(|(id, value)| Ok((id, sprite(value)?)))
            .collect::<Result<_>>()?,
        refraction: RefractionRecipe {
            sprite: sprite(source.refraction.sprite)?,
            displacement: source.refraction.displacement,
        },
        emotes: source.emotes,
        mouth_cycle: source.mouth_cycle,
    };
    effects.validate()?;
    recipe.blink.validate()?;
    let particles = recipe
        .particles
        .into_iter()
        .map(|(id, value)| {
            let recipe = FlutterRecipe {
                texture: image(value.texture)?,
                uv: value.uv,
                aspect_ratio: value.aspect_ratio,
                palette: value.palette,
                fall_speed: value.fall_speed,
                fall_variation: value.fall_variation,
                spin: value.spin,
            };
            recipe.validate()?;
            Ok((id, recipe))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let path = "effects/field.json";
    write_atomic(&output.join(path), &serde_json::to_vec_pretty(&effects)?)?;
    let files = [
        &effects.emote_texture,
        &effects.status_texture,
        &effects.refraction.sprite.texture,
        &shadow.texture,
    ]
    .into_iter()
    .chain(effects.sprites.values().map(|s| &s.texture))
    .chain(particles.values().map(|p| &p.texture))
    .cloned()
    .collect::<std::collections::BTreeSet<_>>()
    .into_iter()
    .collect();
    Ok(Prepared {
        path: path.into(),
        files,
        blink: recipe.blink,
        shadow,
        particles,
    })
}
