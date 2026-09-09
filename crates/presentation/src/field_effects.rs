//! Camera-facing sprites from cooked recipes and live event state.
use super::{
    field_audit::{Applied, Request},
    field_view::{ActorPart, State},
    materials::TitleSurface,
};
use anyhow::Result;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    image::{ImageLoaderSettings, ImageSampler},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    transform::helper::TransformHelper,
};
use resonance_content::effect::FieldEffects;
use std::{collections::BTreeMap, fs, path::Path};

const DUST: usize = 0;
const EMOTES: usize = 1;

#[derive(Component)]
pub(super) struct EffectDraw;

#[derive(Resource)]
pub(super) struct Artwork {
    spec: FieldEffects,
    textures: [Handle<Image>; 2],
    layers: [Option<(Entity, Handle<Mesh>)>; 2],
}
impl Artwork {
    pub fn mouth_frame(&self, age: u32) -> u8 {
        self.spec.mouth_cycle[age as usize % self.spec.mouth_cycle.len()]
    }
    pub fn load(root: &Path, path: &str, server: &AssetServer) -> Result<Self> {
        Self::load_with(root, path, server, None)
    }
    pub fn load_with(
        root: &Path,
        path: &str,
        server: &AssetServer,
        files: Option<&resonance_content::prepared::Files>,
    ) -> Result<Self> {
        let spec: FieldEffects = if let Some(files) = files {
            files.json(path)?
        } else {
            serde_json::from_slice(&fs::read(root.join(path))?)?
        };
        spec.validate()?;
        let textures = [&spec.dust_texture, &spec.emote_texture].map(|path| {
            server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| {
                    s.is_srgb = false;
                    s.sampler = ImageSampler::linear();
                })
                .load(path.clone())
        });
        Ok(Self {
            spec,
            textures,
            layers: [None, None],
        })
    }
    pub fn despawn(self, world: &mut World) {
        for (entity, _) in self.layers.into_iter().flatten() {
            world.despawn(entity);
        }
    }
    pub(super) fn prepare(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        surfaces: &mut Assets<TitleSurface>,
    ) {
        for index in 0..2 {
            if self.layers[index].is_some() {
                continue;
            }
            let mut batch = Batch::default();
            batch.sprite(
                Vec3::ZERO,
                Quat::IDENTITY,
                [1., 1.],
                [0., 0., 1., 1.],
                [1.; 4],
            );
            let mesh = meshes.add(batch.mesh());
            let surface = surfaces.add(TitleSurface {
                color: Some(self.textures[index].clone()),
                blend: true,
                // Head emotes ignore depth so hair cannot obscure them; dust tests depth.
                depth_test: index == DUST,
                depth_write: false,
                cull: resonance_content::CullFace::None,
                ..default()
            });
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(surface),
                    Transform::default(),
                    Visibility::Hidden,
                    NoFrustumCulling,
                    EffectDraw,
                    super::draw_order::DrawOrder((1 << 22) + index as u32),
                ))
                .id();
            self.layers[index] = Some((entity, mesh));
        }
    }
}

#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}
impl Batch {
    fn sprite(
        &mut self,
        center: Vec3,
        rotation: Quat,
        size: [f32; 2],
        uv: [f32; 4],
        color: [f32; 4],
    ) {
        let right = rotation * Vec3::X * (size[0] / 2.).trunc();
        let up = rotation * Vec3::Y * (size[1] / 2.).trunc();
        let base = self.positions.len() as u32;
        self.positions.extend(
            [
                center - right + up,
                center + right + up,
                center + right - up,
                center - right - up,
            ]
            .map(|v| v.to_array()),
        );
        self.uv.extend([
            [uv[0], uv[1]],
            [uv[2], uv[1]],
            [uv[2], uv[3]],
            [uv[0], uv[3]],
        ]);
        self.colors.extend([color; 4]);
        self.indices
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[allow(clippy::too_many_arguments)] // Cooked images, live state, current joint transforms, and sprite submission.
pub(super) fn render(
    mut commands: Commands,
    state: State,
    art: Res<Artwork>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    actors: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    names: Query<&Name>,
    helper: TransformHelper,
    mut applied: ResMut<Applied>,
) {
    if state.live.as_ref().is_some_and(|s| !s.ready_for_field) {
        return;
    }
    let world = &state.get().events.world;
    let Some(camera) = &world.field_camera else {
        return;
    };
    if art.textures.iter().any(|h| !images.contains(h)) {
        for &id in world.billboards.keys() {
            applied.loading(Request::Billboard(id));
        }
        for &id in world.emotes.keys() {
            applied.loading(Request::Emote(id));
        }
        return;
    }
    let camera = Transform::from_translation(Vec3::from_array(camera.position))
        .looking_at(Vec3::from_array(camera.target), Vec3::Z);
    let side = Vec3::new(camera.right().x, camera.right().y, 0.).normalize_or_zero();
    let forward = Vec3::Z.cross(side);
    let brightness = world.brightness();
    let mut batches: [Batch; 2] = std::array::from_fn(|_| Batch::default());
    for (&id, effect) in &world.billboards {
        if effect.recipe != 0 {
            continue;
        }
        let rotation = camera.rotation * Quat::from_rotation_z(effect.rotation[2].to_radians());
        // Dust colors use a gain of four.
        let rgb = effect.rgba[..3]
            .iter()
            .map(|v| (f32::from(*v) * 4. / 255.).min(1.) * brightness)
            .collect::<Vec<_>>();
        batches[DUST].sprite(
            Vec3::from_array(effect.position),
            rotation,
            effect.size,
            art.spec.dust_uv,
            [
                rgb[0],
                rgb[1],
                rgb[2],
                effect.alpha(world.tick).clamp(0., 255.) / 255.,
            ],
        );
        applied.ack(Request::Billboard(id));
    }
    let roots: BTreeMap<_, _> = actors
        .iter()
        .filter(|(_, p)| p.part == 0)
        .map(|(e, p)| (p.actor, (e, p)))
        .collect();
    for (&id, emote) in &world.emotes {
        let Some(track) = art.spec.emotes.get(&emote.kind) else {
            continue;
        };
        let Some((root, part)) = roots.get(&emote.actor) else {
            continue;
        };
        if !part.prepared {
            applied.loading(Request::Emote(id));
            continue;
        }
        let bone = children
            .iter_descendants(*root)
            .find(|e| names.get(*e).is_ok_and(|n| n.as_str() == track.anchor));
        let Some(bone) = bone else {
            continue;
        };
        let Ok(anchor) = helper.compute_global_transform(bone) else {
            continue;
        };
        for sprite in track.frame(world.tick.saturating_sub(emote.start_tick) as usize) {
            let [x, y, z] = std::array::from_fn(|i| sprite.offset[i] + emote.offset[i]);
            let center = anchor.translation() + side * x + forward * y + Vec3::Z * z;
            // Snap emote centers to whole world units; keep their rotated vertices
            // and the independently moving dust particles at full precision.
            let center = center.trunc();
            let rotation = camera.rotation * Quat::from_rotation_z(sprite.rotation.to_radians());
            batches[EMOTES].sprite(
                center,
                rotation,
                sprite.size,
                sprite.uv,
                [brightness, brightness, brightness, 1.],
            );
        }
        // The intro frame can deliberately contain no sprites; the track
        // has still been sampled and handled by this renderer.
        applied.ack(Request::Emote(id));
    }
    for (index, batch) in batches.into_iter().enumerate() {
        if batch.positions.is_empty() {
            if let Some((entity, _)) = &art.layers[index] {
                commands.entity(*entity).insert(Visibility::Hidden);
            }
            continue;
        }
        let mesh = batch.mesh();
        let (entity, handle) = art.layers[index]
            .as_ref()
            .expect("effect layer was not prepared");
        *meshes.get_mut(handle).expect("effect mesh is retained") = mesh;
        commands.entity(*entity).insert(Visibility::Inherited);
    }
}
