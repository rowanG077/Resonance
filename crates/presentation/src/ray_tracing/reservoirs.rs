//! Preserve empty ReSTIR samples when translating last frame's light IDs.
use bevy::{prelude::*, shader::Shader};

const LIBRARY: &str = "embedded://bevy_solari/realtime/restir_di.wgsl";
const TRANSLATION: &str = "    // Check if the light selected in the previous frame no longer exists in the current frame (e.g. entity despawned)";
const GUARD: &str = "    // Empty history has NULL_LIGHT_ID, not an index into the light translation table.\n    if !reservoir_valid(temporal.reservoir) {\n        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), vec3(0.0));\n    }\n\n";

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
    if source.contains(GUARD) {
        return;
    }
    assert_eq!(
        source.matches(TRANSLATION).count(),
        1,
        "Bevy Solari light translation changed; review the empty-reservoir guard"
    );
    // Bevy 0.19.1 also reaches this translation after rejecting history for
    // disoccluded pixels. Translating NULL_LIGHT_ID reads beyond the table and
    // can turn its sentinel into a real light with an invalid triangle index.
    let source = source.replacen(TRANSLATION, &format!("{GUARD}{TRANSLATION}"), 1);
    *shaders.get_mut(&library.0).unwrap() = Shader::from_wgsl(source, LIBRARY);
    info!("Solari: empty temporal light samples retain their sentinel");
}
