//! Lightning Spear's GC-style artwork uses an existing authored Steam fork.
//! Only its sprite binding changes; native geometry, births and clocks remain.
use super::*;
use resonance_content::battle_effect::{EffectTexture, ParticleGeometry};

const OWNER: &str = "battle/enemies/commander-knight-0103/effects";
const FORK_DESCRIPTOR: &str = "bh02_pic003_pic000.totexb_d";
const RIBBON: &str = "additive-billboard-ribbon";

fn native_ribbon(package: &[u8], member: usize, prefix: &[u8; 8], uv: [u16; 4]) -> Result<()> {
    let bank = preview_model::member(package, 0x1cc)?.context("missing lightning effect bank")?;
    let start = pe::word(bank, 8)? as usize + member * 360;
    let actor = bank
        .get(start..start + 360)
        .context("missing authored lightning ribbon")?;
    ensure!(
        actor.starts_with(prefix),
        "authored lightning actor changed"
    );
    for (axis, expected) in uv.into_iter().enumerate() {
        ensure!(
            pe::half(actor, 8 + axis * 2)? == expected,
            "authored lightning panel changed"
        );
    }
    Ok(())
}

pub(super) fn bind(
    source: &Source,
    library: &Path,
    image: &pe::Image<'_>,
    atlases: &effect_atlas::Directory,
    id: u16,
    definition: &mut Definition,
    textures: &mut BTreeMap<u8, Vec<EffectTexture>>,
) -> Result<()> {
    if id != 103 {
        return Ok(());
    }
    ensure!(
        monsters::package(image, id)?.eq_ignore_ascii_case("hum24.pac")
            && monsters::symbol(id) == "commander-knight-0103",
        "Lightning Spear owner changed"
    );
    native_ribbon(
        &source.require("hum24.pac")?,
        5,
        &[18, 1, 2, 6, 1, 0, 32, 0],
        [193, 64, 62, 64],
    )?;
    let member = definition
        .template_names
        .iter()
        .position(|name| name == RIBBON)
        .context("missing Lightning Spear ribbon")?;
    // The decoded pool and serialized BTreeMap have different member orders.
    // Bind the semantic declaration and verify its exact program users below.
    let uses = definition
        .programs
        .iter()
        .enumerate()
        .flat_map(|(program, events)| {
            events
                .iter()
                .filter_map(move |event| match &event.operation {
                    EffectOperation::Spawn { particle, .. } if usize::from(*particle) == member => {
                        Some((program, event.at))
                    }
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    ensure!(
        uses == [(6, 34), (6, 38), (6, 42)],
        "Lightning Spear ribbon users changed"
    );
    let Declaration::Particle { template, visual } = &definition.templates[member] else {
        anyhow::bail!("Lightning Spear ribbon changed kind")
    };
    ensure!(
        template.state.geometry
            == ParticleGeometry::Ribbon {
                length: 900.,
                width: 160.,
                jitter: 88.,
                phase: 6,
                phase_period: 0,
            }
            && template.state.uv == [1, 1, 62, 64]
            && template.state.palettes == [6, 1]
            && template.lifetime == Some(33)
            && template.fade == [8, 8, 8, 16]
            && matches!(
                visual.texture,
                ParticleTexture::Atlas {
                    slot: 2,
                    dual: false,
                    ..
                }
            ),
        "Lightning Spear ribbon settings changed"
    );

    // Kvar's native actor1 selects palette key4*32/16=8. Its three
    // identity-remapped 62x32 panels contain the authored purple-white fork.
    // Read the Steam resource directly so a selected103 cook does not depend
    // on Kvar having been cooked earlier or on any GameCube source assets.
    ensure!(
        monsters::package(image, 217)?.eq_ignore_ascii_case("bh02.pac"),
        "authored lightning artwork owner changed"
    );
    let donor_package = source.require("bh02.pac")?;
    ensure!(
        preview_model::reference(&donor_package, 0x1d0)?
            .is_some_and(|name| name.eq_ignore_ascii_case(FORK_DESCRIPTOR)),
        "authored lightning descriptor changed"
    );
    native_ribbon(
        &donor_package,
        1,
        &[18, 1, 2, 4, 2, 0, 32, 0],
        [1, 1, 62, 32],
    )?;
    let table = atlases
        .get(FORK_DESCRIPTOR)
        .context("missing authored lightning atlas map")?;
    for (index, top) in [(22, 1.), (23, 33.), (24, 65.)] {
        let row = table
            .rows
            .get(index)
            .context("missing lightning atlas cell")?;
        let rect = [1., top, 63., top + 32.];
        ensure!(
            row.palette == 8 && row.source == rect && row.packed == rect,
            "authored lightning atlas cell changed"
        );
    }
    let mut donor = definition.templates[member].clone();
    let Declaration::Particle { template, visual } = &mut donor else {
        unreachable!()
    };
    template.state.uv = [1, 1, 62, 32];
    template.state.palettes = [4, 2];
    visual.texture = ParticleTexture::Atlas {
        slot: 2,
        dual: false,
        palette_set: 32,
    };
    let mut artwork = Definition {
        programs: Vec::new(),
        template_names: vec![RIBBON.to_owned()],
        templates: vec![donor],
        artwork: None,
    };
    let fork = effect_atlas::cook(
        source,
        library,
        &mut artwork,
        FORK_DESCRIPTOR,
        Some(table),
        &format!("{OWNER}/style-lightning"),
        2,
    )?;
    let set = artwork.templates[0]
        .particle_visual()?
        .texture
        .palette_set();
    let selected = fork.images[fork.palette_image(set, 4)?].clone();
    ensure!(
        (selected.width, selected.height) == (64, 98),
        "authored lightning image extent changed"
    );
    let actor = textures
        .get_mut(&2)
        .and_then(|bank| bank.first_mut())
        .context("missing Lightning Spear actor atlas")?;
    let image_index = actor.images.len();
    let palette_set = u8::try_from(actor.palette_sets.len())?;
    actor.images.push(selected);
    // Keep target palettes6/1 intact; this dedicated set always selects the
    // one authored donor colorway, irrespective of its native selector4.
    actor.palette_sets.push(vec![Some(image_index); 256]);
    let Declaration::Particle { template, visual } = &mut definition.templates[member] else {
        unreachable!()
    };
    template.state.uv = artwork.templates[0].template()?.state.uv;
    visual.texture = ParticleTexture::Atlas {
        slot: 2,
        dual: false,
        palette_set,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Language;
    use std::{fs, path::PathBuf};

    fn actor_atlas(
        source: &Source,
        library: &Path,
        image: &pe::Image<'_>,
        atlases: &effect_atlas::Directory,
        id: u16,
    ) -> Result<(Definition, BTreeMap<u8, Vec<EffectTexture>>)> {
        let package = source.require(monsters::package(image, id)?)?;
        let body = source.require(
            &preview_model::reference(&package, 0x18)?.context("missing test owner body")?,
        )?;
        let bones = model::Rig::read(&body)?.joint_names();
        let mut definition = decode(
            preview_model::member(&package, 0x1cc)?.context("missing test effect bank")?,
            &bones,
            |stride, declaration| {
                if let Declaration::Particle { visual, .. } = declaration
                    && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
                {
                    *palette_set = stride;
                }
                Ok(())
            },
        )?;
        let descriptor =
            preview_model::reference(&package, 0x1d0)?.context("missing test atlas")?;
        let texture = effect_atlas::cook(
            source,
            library,
            &mut definition,
            &descriptor,
            atlases.get(&descriptor),
            &format!("battle/enemies/{}/effects", monsters::symbol(id)),
            2,
        )?;
        Ok((definition, BTreeMap::from([(2, vec![texture])])))
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks only103/217 sprite atlases into a temporary directory"]
    fn installed_lightning_spear_reuses_authored_fork_without_changing_execution() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let actions = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        )
        .join("battle/enemies/commander-knight-0103/enemy.json");
        let action_definition = fs::read(&actions)?;
        let image = pe::Image::parse(&source.executable)?;
        let atlases = effect_atlas::Directory::read(&image)?;
        let temporary = tempfile::tempdir()?;
        let library = temporary.path();
        let (mut definition, mut textures) = actor_atlas(&source, library, &image, &atlases, 103)?;
        let before = definition.clone();
        let member = before
            .template_names
            .iter()
            .position(|name| name == RIBBON)
            .context("missing test Lightning Spear ribbon")?;
        let before_textures = textures.clone();
        let old_images = textures[&2][0]
            .images
            .iter()
            .map(|image| fs::read(library.join(&image.path)))
            .collect::<std::io::Result<Vec<_>>>()?;

        bind(
            &source,
            library,
            &image,
            &atlases,
            102,
            &mut definition,
            &mut textures,
        )?;
        assert_eq!(definition.programs, before.programs);
        assert_eq!(definition.templates, before.templates);
        assert_eq!(textures, before_textures);
        bind(
            &source,
            library,
            &image,
            &atlases,
            103,
            &mut definition,
            &mut textures,
        )?;
        assert_eq!(definition.programs, before.programs);
        assert_eq!(definition.template_names, before.template_names);
        let mut expected = before.templates.clone();
        let Declaration::Particle { template, visual } = &mut expected[member] else {
            anyhow::bail!("missing test ribbon")
        };
        template.state.uv[3] = 32;
        visual.texture = ParticleTexture::Atlas {
            slot: 2,
            dual: false,
            palette_set: before_textures[&2][0].palette_sets.len().try_into()?,
        };
        // Equality covers all other declarations plus geometry, native fade,
        // lifetime, colors, phases, counts and every authored emission event.
        assert_eq!(definition.templates, expected);
        let actor = &textures[&2][0];
        let original = &before_textures[&2][0];
        assert_eq!(actor.sampler, original.sampler);
        assert_eq!(actor.images.len(), original.images.len() + 1);
        assert_eq!(&actor.images[..original.images.len()], &original.images);
        assert_eq!(
            &actor.palette_sets[..original.palette_sets.len()],
            &original.palette_sets
        );
        assert_eq!(actor.palette_sets.len(), original.palette_sets.len() + 1);
        let set = definition.templates[member]
            .particle_visual()?
            .texture
            .palette_set();
        let fork_index = actor.palette_image(set, 6)?;
        for selector in 0..=255 {
            assert_eq!(actor.palette_image(set, selector)?, fork_index);
        }
        let fork_bytes = fs::read(library.join(&actor.images[fork_index].path))?;
        let old_set = before.templates[member]
            .particle_visual()?
            .texture
            .palette_set();
        assert_ne!(fork_bytes, old_images[original.palette_image(old_set, 6)?]);
        for (image, bytes) in original.images.iter().zip(old_images) {
            assert_eq!(fs::read(library.join(&image.path))?, bytes);
        }

        // Independently cook Kvar's unmodified native bank, not an existing
        // cooked asset: the selected authored fork must be byte-identical.
        let (donor, donor_textures) = actor_atlas(&source, library, &image, &atlases, 217)?;
        let member = donor
            .template_names
            .iter()
            .position(|name| name == RIBBON)
            .context("missing native Kvar fork")?;
        let donor_set = donor.templates[member]
            .particle_visual()?
            .texture
            .palette_set();
        let texture = &donor_textures[&2][0];
        let donor_image = &texture.images[texture.palette_image(donor_set, 4)?];
        assert_eq!(fork_bytes, fs::read(library.join(&donor_image.path))?);
        assert_eq!(fs::read(actions)?, action_definition);
        Ok(())
    }
}
