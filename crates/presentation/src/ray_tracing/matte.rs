//! Preserve Solari's ordinary materials while making zero-reflectance actors
//! diffuse-only, including at grazing angles and on indirect ray hits.
use bevy::{prelude::*, shader::Shader};

const LIBRARY: &str = "embedded://bevy_solari/scene/brdf.wgsl";
const SOURCE: &str = include_str!("brdf.wgsl");

#[derive(Resource)]
struct Library(Handle<Shader>);

pub(super) fn install(app: &mut App) {
    let handle = app.world().resource::<AssetServer>().load(LIBRARY);
    app.insert_resource(Library(handle))
        .add_systems(PostUpdate, replace.before(bevy::asset::AssetEventSystems));
}

fn replace(library: Res<Library>, mut shaders: ResMut<Assets<Shader>>) {
    let Some(shader) = shaders.get(&library.0) else {
        return;
    };
    if shader.source.as_str() == SOURCE {
        return;
    }
    // Replace the existing library handle before shader events reach the render
    // world. A second import-path registration would depend on asset load order.
    *shaders.get_mut(&library.0).unwrap() = Shader::from_wgsl(SOURCE, LIBRARY);
    info!("Classroom matte materials: diffuse-only Solari lighting enabled");
}
