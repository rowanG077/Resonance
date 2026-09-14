//! Capture-only extension of Bevy's highest SMAA preset.
use bevy::{prelude::*, shader::Shader};

const LIBRARY: &str = "embedded://bevy_anti_alias/smaa/smaa.wgsl";
const ULTRA: &str = "const SMAA_THRESHOLD: f32 = 0.05;\nconst SMAA_MAX_SEARCH_STEPS: u32 = 32u;\nconst SMAA_MAX_SEARCH_STEPS_DIAG: u32 = 16u;";
const CAPTURE: &str = "const SMAA_THRESHOLD: f32 = 0.025;\nconst SMAA_MAX_SEARCH_STEPS: u32 = 64u;\nconst SMAA_MAX_SEARCH_STEPS_DIAG: u32 = 20u;";

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
    let source = shader.source.as_str();
    if source.contains(CAPTURE) {
        return;
    }
    assert!(
        source.contains(ULTRA),
        "Bevy SMAA Ultra changed; review the capture preset"
    );
    let source = source.replacen(ULTRA, CAPTURE, 1);
    *shaders.get_mut(&library.0).unwrap() = Shader::from_wgsl(source, LIBRARY);
    info!("Capture SMAA: threshold 0.025, search steps 64/20");
}
