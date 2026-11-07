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
