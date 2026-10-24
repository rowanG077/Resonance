//! Translate verified native pattern rules and extract their referenced source tables.
use super::*;
use resonance_content::battle_scene::{
    CameraBounds, PatternContact, PatternOrigin, PatternSpell, SpellRecipe,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    provenance: String,
    lifetime: u16,
    origin: PatternOrigin,
    heading_offset: usize,
    startup_member: u16,
    startup_deferred: bool,
    contacts: Contacts,
    color_offset: usize,
    camera_offset: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contacts {
    first_tick: u16,
    period: u16,
    count: u16,
    points_offset: usize,
    projectile: u8,
    hit_variant: u8,
    effect_member: u16,
}

fn recipes() -> Result<BTreeMap<u16, Recipe>> {
    Ok(serde_json::from_str(include_str!("spell_recipes.json"))?)
}

fn read(module: &Rel, recipe: Recipe) -> Result<(PatternSpell, [u8; 3], CameraBounds)> {
    ensure!(
        !recipe.provenance.trim().is_empty() && recipe.provenance.len() <= 4096,
        "spell recipe source evidence is missing or unbounded"
    );
    let schedule = recipe.contacts;
    ensure!(
        (1..=64).contains(&schedule.count) && schedule.period > 0,
        "invalid authored pattern schedule"
    );
    let points = module
        .at((4, schedule.points_offset))?
        .get(..usize::from(schedule.count) * 12)
        .context("pattern points truncated")?;
    let contacts = points
        .chunks_exact(12)
        .enumerate()
        .map(|(index, point)| {
            Ok(PatternContact {
                tick: schedule
                    .first_tick
                    .checked_add(
                        schedule
                            .period
                            .checked_mul(index as u16)
                            .context("pattern contact interval overflow")?,
                    )
                    .context("pattern contact age overflow")?,
                projectile: schedule.projectile,
                hit_variant: schedule.hit_variant,
                effect_member: schedule.effect_member,
                offset: <[f32; 3]>::read(point, 0)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let pattern = PatternSpell {
        lifetime: recipe.lifetime,
        origin: recipe.origin,
        heading_offset: crate::read::f32(module.at((4, recipe.heading_offset))?, 0)?,
        startup_member: recipe.startup_member,
        startup_deferred: recipe.startup_deferred,
        contacts,
    };
    pattern.validate()?;
    let rgba = <[u8; 4]>::read(module.at((4, recipe.color_offset))?, 0)?;
    ensure!(rgba[3] == 255, "unsupported pattern scene tint alpha");
    let camera_bytes = module.at((4, recipe.camera_offset))?;
    let camera = CameraBounds {
        minimum_distance: crate::read::f32(camera_bytes, 0)?,
        minimum_elevation: crate::read::f32(camera_bytes, 4)?,
    };
    camera.validate()?;
    Ok((pattern, [rgba[0], rgba[1], rgba[2]], camera))
}

pub(super) fn bind(module: &Rel, id: u16, scene: &mut Scene) -> Result<bool> {
    let Some(recipe) = recipes()?.remove(&id) else {
        return Ok(false);
    };
    let (pattern, color, camera) = read(module, recipe)?;
    scene.spell_recipe = Some(SpellRecipe::FixedPattern(pattern));
    scene.stage_color = Some(color);
    scene.camera = Some(camera);
    // Each contact now owns its extracted point; there is no second point authority.
    scene.strike_points.clear();
    scene.recovery_radius = None;
    Ok(true)
}

#[test]
#[ignore = "reads original battle RELs from both extracted discs"]
fn original_ray_recipe_extracts_points_heading_and_scene_settings() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut declarations = Vec::new();
    for disc in ["disc1", "disc2"] {
        let module = Rel::read(&root.join(disc).join("files/US_r_Top2Btl.rel"))?;
        let (pattern, color, camera) = read(&module, recipes()?.remove(&252).unwrap())?;
        assert_eq!(pattern.lifetime, 230);
        assert_eq!(pattern.heading_offset, 20.);
        assert_eq!(color, [12; 3]);
        assert_eq!(
            camera,
            CameraBounds {
                minimum_distance: 3550.,
                minimum_elevation: 11.5
            }
        );
        assert_eq!(
            pattern.contacts.iter().map(|c| c.tick).collect::<Vec<_>>(),
            vec![60, 68, 76, 84, 92, 100, 108, 116, 124]
        );
        for (index, contact) in pattern.contacts.iter().enumerate() {
            assert_eq!(
                contact.offset,
                <[f32; 3]>::read(module.at((4, 0x7dac))?, index * 12)?
            );
        }
        declarations.push(serde_json::to_vec(&pattern)?);
    }
    assert_eq!(declarations[0], declarations[1]);
    Ok(())
}
