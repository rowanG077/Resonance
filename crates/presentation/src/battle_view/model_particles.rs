//! Shared model recipes and reusable render objects for live particles.
use super::*;
use resonance_battle::ParticleFrame;

// Optional components cannot appear in an immutable try_query: an entirely
// rigid asset world has never registered SkinnedMesh, so that query is absent.
type SourceMeshes = (Entity, &'static Mesh3d, &'static MaterialSlot);

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(crate) struct Stats {
    pub templates: usize,
    pub active_particles: usize,
    pub active_draws: usize,
    pub active_joints: usize,
    pub retained_draws: usize,
    pub retained_joints: usize,
    pub created_draws: usize,
    pub created_joints: usize,
}

struct Source {
    spec: Arc<PreviewPart>,
    gltf: Handle<Gltf>,
    textures: Vec<Handle<Image>>,
    bone_offset: usize,
}

#[derive(Clone, Copy)]
struct Pose {
    bone: Option<usize>,
    suffix: Mat4,
}

impl Pose {
    fn sample(self, world: Mat4, bones: &[Matrix]) -> Result<GlobalTransform> {
        let bone = self.bone.map_or(Ok(Mat4::IDENTITY), |index| {
            bones
                .get(index)
                .map(Mat4::from_cols_array_2d)
                .context("model particle draw bone outside sampled pose")
        })?;
        let matrix = world * bone * self.suffix;
        ensure!(matrix.is_finite(), "invalid model particle transform");
        Ok(GlobalTransform::from(bevy::math::Affine3A::from_mat4(
            matrix,
        )))
    }
}

struct Skin {
    inverse_bindposes: Handle<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
    joints: Vec<usize>,
}

struct DrawRecipe {
    mesh: Handle<Mesh>,
    pose: Pose,
    skin: Option<Skin>,
    base: TitleSurface,
    styles: BTreeMap<ModelStyle, TitleSurface>,
    order: u32,
}

struct Template {
    sources: Vec<Source>,
    required_bones: usize,
    suppressed: BTreeSet<usize>,
    joints: Vec<Pose>,
    draws: Vec<DrawRecipe>,
    ready: bool,
    disabled: bool,
}

struct Draw {
    entity: Entity,
    material: Handle<TitleSurface>,
}

#[derive(Default)]
struct Instance {
    key: Option<(ParticleId, u32)>,
    draws: Vec<usize>,
    joints: Vec<usize>,
    style: Option<ModelStyle>,
}

#[derive(Default)]
pub(super) struct Pool {
    templates: BTreeMap<u32, Template>,
    instances: Vec<Instance>,
    draws: Vec<Draw>,
    joints: Vec<Entity>,
    free_draws: Vec<usize>,
    free_joints: Vec<usize>,
    warm_draws: Vec<Entity>,
    warm_joints: Vec<Entity>,
    warm_materials: Vec<Handle<TitleSurface>>,
    held_pose: Vec<(Entity, GlobalTransform)>,
}

impl Pool {
    pub fn load(
        &mut self,
        resource: u32,
        parts: Vec<Part>,
        bone_offsets: Vec<usize>,
        suppressed: BTreeSet<usize>,
    ) -> Result<()> {
        ensure!(
            !self.templates.contains_key(&resource),
            "duplicate model particle template"
        );
        ensure!(
            parts.len() == bone_offsets.len(),
            "particle layer bone mappings differ"
        );
        let required_bones = parts
            .iter()
            .zip(&bone_offsets)
            .map(|(part, offset)| offset + part.spec.scene.bone_names.len())
            .max()
            .unwrap_or(0);
        self.templates.insert(
            resource,
            Template {
                sources: parts
                    .into_iter()
                    .zip(bone_offsets)
                    .map(|(part, bone_offset)| Source {
                        spec: part.spec,
                        gltf: part.gltf,
                        textures: part.textures,
                        bone_offset,
                    })
                    .collect(),
                required_bones,
                suppressed,
                joints: Vec::new(),
                draws: Vec::new(),
                ready: false,
                disabled: false,
            },
        );
        Ok(())
    }

    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.templates
            .values()
            .flat_map(|t| &t.sources)
            .flat_map(|s| &s.textures)
    }

    pub fn images_mut(&mut self) -> impl Iterator<Item = &mut Handle<Image>> {
        self.templates
            .values_mut()
            .flat_map(|t| &mut t.sources)
            .flat_map(|s| &mut s.textures)
    }

    pub fn warm_draws(&self) -> impl Iterator<Item = Entity> + '_ {
        self.warm_draws.iter().copied()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        server: &AssetServer,
        assets: &mut AssetsForView,
        sampled: &mut crate::scene::SampledImages,
        toon: &Handle<Image>,
        styles: &BTreeSet<ModelStyle>,
        diagnostics: &Diagnostics,
    ) -> Result<bool> {
        for (&resource, template) in &mut self.templates {
            if template.ready {
                continue;
            }
            let result = template.compile(server, assets, sampled, toon, styles);
            match diagnostics.attempt(
                &format!("battle model particle template {resource}"),
                result,
            )? {
                None => {
                    template.ready = true;
                    template.disabled = true;
                }
                Some(false) => continue,
                Some(true) => {
                    let joints: Vec<_> = template
                        .joints
                        .iter()
                        .map(|pose| {
                            let entity = commands
                                .spawn((
                                    Transform::default(),
                                    GlobalTransform::from(bevy::math::Affine3A::from_mat4(
                                        pose.suffix,
                                    )),
                                ))
                                .id();
                            self.warm_joints.push(entity);
                            entity
                        })
                        .collect();
                    // Every actual mesh/image/material binding is submitted, not merely
                    // one representative of each pipeline's vertex layout.
                    for draw in &template.draws {
                        for surface in std::iter::once(&draw.base).chain(draw.styles.values()) {
                            let material = assets.surfaces.add(surface.clone());
                            let mut entity = commands.spawn((
                                Mesh3d(draw.mesh.clone()),
                                MeshMaterial3d(material.clone()),
                                Transform::default(),
                                GlobalTransform::from(bevy::math::Affine3A::from_mat4(
                                    draw.pose.suffix,
                                )),
                                Visibility::Inherited,
                                NoFrustumCulling,
                                RenderLayers::layer(WARM_LAYER),
                                DrawOrder(Layer::Effects, 0, draw.order),
                            ));
                            if let Some(skin) = &draw.skin {
                                entity.insert(SkinnedMesh {
                                    inverse_bindposes: skin.inverse_bindposes.clone(),
                                    joints: skin
                                        .joints
                                        .iter()
                                        .map(|&joint| joints[joint])
                                        .collect(),
                                });
                            }
                            self.warm_draws.push(entity.id());
                            self.warm_materials.push(material);
                        }
                    }
                    template.ready = true;
                }
            }
        }
        Ok(self.templates.values().all(|template| template.ready))
    }

    pub fn activate(&mut self, commands: &mut Commands) {
        for entity in self.warm_draws.drain(..).chain(self.warm_joints.drain(..)) {
            commands.entity(entity).despawn();
        }
        self.warm_materials.clear();
    }

    pub fn retain(
        &mut self,
        particles: &[ParticleFrame],
        commands: &mut Commands,
        surfaces: &mut Assets<TitleSurface>,
    ) {
        for instance in &mut self.instances {
            let Some(key) = instance.key else {
                continue;
            };
            if particles
                .iter()
                .any(|p| p.model.as_ref().is_some_and(|m| (p.id, m.resource) == key))
            {
                continue;
            }
            for index in instance.draws.drain(..) {
                let draw = &self.draws[index];
                commands
                    .entity(draw.entity)
                    .insert((Visibility::Hidden, NoCpuCulling))
                    .remove::<(Mesh3d, SkinnedMesh, MeshMaterial3d<TitleSurface>)>();
                if let Some(mut surface) = surfaces.get_mut(&draw.material) {
                    *surface = TitleSurface::default();
                }
                self.free_draws.push(index);
            }
            self.free_joints.append(&mut instance.joints);
            instance.key = None;
            instance.style = None;
        }
    }

    pub fn hide(&self, particle: ParticleId, commands: &mut Commands) {
        for instance in self
            .instances
            .iter()
            .filter(|instance| instance.key.is_some_and(|key| key.0 == particle))
        {
            for &index in &instance.draws {
                commands
                    .entity(self.draws[index].entity)
                    .insert(Visibility::Hidden);
            }
        }
    }

    pub fn clear_pose(&mut self) {
        self.held_pose.clear();
    }
    pub fn held_pose(&self) -> &[(Entity, GlobalTransform)] {
        &self.held_pose
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &mut self,
        particle: &ParticleFrame,
        style: ModelStyle,
        layer: Layer,
        order: usize,
        light: [f32; 3],
        commands: &mut Commands,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        let model = particle
            .model
            .as_ref()
            .context("particle has no model pose")?;
        let template = self
            .templates
            .get(&model.resource)
            .context("unprepared model particle template")?;
        ensure!(template.ready, "model particle template is not ready");
        if template.disabled {
            return Ok(());
        }
        ensure!(
            model.bones.len() >= template.required_bones,
            "model particle pose differs from template skeleton"
        );
        let key = (particle.id, model.resource);
        let index = if let Some(index) = self
            .instances
            .iter()
            .position(|instance| instance.key == Some(key))
        {
            index
        } else {
            let index = if let Some(index) = self
                .instances
                .iter()
                .position(|instance| instance.key.is_none())
            {
                index
            } else {
                self.instances.push(Instance::default());
                self.instances.len() - 1
            };
            let instance = &mut self.instances[index];
            for _ in &template.joints {
                let slot = self.free_joints.pop().unwrap_or_else(|| {
                    self.joints.push(
                        commands
                            .spawn((Transform::default(), GlobalTransform::default()))
                            .id(),
                    );
                    self.joints.len() - 1
                });
                instance.joints.push(slot);
            }
            for _ in template.draws.iter() {
                let slot = self.free_draws.pop().unwrap_or_else(|| {
                    self.draws.push(Draw {
                        entity: commands
                            .spawn((
                                Transform::default(),
                                GlobalTransform::default(),
                                Visibility::Hidden,
                                NoFrustumCulling,
                                RenderLayers::layer(LAYER),
                            ))
                            .id(),
                        material: surfaces.add(TitleSurface::default()),
                    });
                    self.draws.len() - 1
                });
                instance.draws.push(slot);
            }
            instance.key = Some(key);
            index
        };
        let instance = &mut self.instances[index];
        let world = Mat4::from_cols_array_2d(&model.world);
        for (pose, &index) in template.joints.iter().zip(&instance.joints) {
            let entity = self.joints[index];
            let pose = pose.sample(world, &model.bones)?;
            commands.entity(entity).insert(pose);
            self.held_pose.push((entity, pose));
        }
        for (recipe, &index) in template.draws.iter().zip(&instance.draws) {
            let draw = &self.draws[index];
            let surface = recipe
                .styles
                .get(&style)
                .context("unprepared model particle style")?;
            if instance.style != Some(style) {
                *surfaces
                    .get_mut(&draw.material)
                    .context("missing particle material slot")? = surface.clone();
                let mut entity = commands.entity(draw.entity);
                entity
                    .insert((
                        Mesh3d(recipe.mesh.clone()),
                        MeshMaterial3d(draw.material.clone()),
                        RenderLayers::layer(LAYER),
                    ))
                    .remove::<NoCpuCulling>();
                if let Some(skin) = &recipe.skin {
                    entity.insert(SkinnedMesh {
                        inverse_bindposes: skin.inverse_bindposes.clone(),
                        joints: skin
                            .joints
                            .iter()
                            .map(|&joint| self.joints[instance.joints[joint]])
                            .collect(),
                    });
                } else {
                    entity.remove::<SkinnedMesh>();
                }
            }
            appearance(&draw.material, surface, particle, style, light, surfaces)?;
            let pose = recipe.pose.sample(world, &model.bones)?;
            commands.entity(draw.entity).insert((
                pose,
                Visibility::Inherited,
                DrawOrder(layer, order, recipe.order),
            ));
            self.held_pose.push((draw.entity, pose));
        }
        instance.style = Some(style);
        Ok(())
    }

    pub fn stats(&self) -> Stats {
        let active = self
            .instances
            .iter()
            .filter(|instance| instance.key.is_some());
        Stats {
            templates: self.templates.len(),
            active_particles: active.clone().count(),
            active_draws: active.clone().map(|instance| instance.draws.len()).sum(),
            active_joints: active.map(|instance| instance.joints.len()).sum(),
            retained_draws: self.draws.len(),
            retained_joints: self.joints.len(),
            created_draws: self.draws.len(),
            created_joints: self.joints.len(),
        }
    }

    pub fn despawn(self, commands: &mut Commands) {
        for entity in self
            .draws
            .into_iter()
            .map(|draw| draw.entity)
            .chain(self.joints)
            .chain(self.warm_draws)
            .chain(self.warm_joints)
        {
            commands.entity(entity).despawn();
        }
    }
}

impl Template {
    fn compile(
        &mut self,
        server: &AssetServer,
        assets: &mut AssetsForView,
        sampled: &mut crate::scene::SampledImages,
        toon: &Handle<Image>,
        styles: &BTreeSet<ModelStyle>,
    ) -> Result<bool> {
        for source in &self.sources {
            check_load(server, source.gltf.id().untyped())?;
            if !server.is_loaded_with_dependencies(source.gltf.id()) {
                return Ok(false);
            }
        }
        let mut draws = Vec::new();
        let mut joints = Vec::new();
        for source in &self.sources {
            let gltf = assets
                .gltfs
                .get(&source.gltf)
                .context("missing loaded particle model")?;
            ensure!(
                gltf.meshes.len() == source.spec.scene.materials.len(),
                "particle material count differs from geometry"
            );
            let materials = surface_recipes(
                &source.spec,
                &source.textures,
                &mut assets.images,
                sampled,
                toon,
                true,
                None,
            )?;
            let scene = assets
                .scenes
                .get(gltf.scenes.first().context("model particle has no scene")?)
                .context("missing loaded particle scene")?;
            let world = &scene.world;
            let mut bone_indices = BTreeMap::new();
            let mut bones = vec![false; source.spec.scene.bone_names.len()];
            if let Some(mut query) = world.try_query::<(Entity, &bevy::gltf::GltfExtras)>() {
                for (entity, extras) in query.iter(world) {
                    let extras: serde_json::Value = serde_json::from_str(&extras.value)?;
                    let Some(index) = extras.get("resonance_bone") else {
                        continue;
                    };
                    let index = index.as_u64().context("invalid cooked bone index")? as usize;
                    let present = bones
                        .get_mut(index)
                        .context("particle bone outside skeleton")?;
                    ensure!(!*present, "duplicate particle bone index");
                    *present = true;
                    bone_indices.insert(entity, source.bone_offset + index);
                }
            }
            ensure!(
                bones.iter().all(|present| *present),
                "missing particle skeleton bone"
            );
            let mut paths = BTreeMap::new();
            let mut joint_indices = BTreeMap::new();
            let mut source_draw_count = 0;
            if let Some(mut query) = world.try_query::<SourceMeshes>() {
                for (entity, mesh, material) in query.iter(world) {
                    source_draw_count += 1;
                    let skin = world.get::<SkinnedMesh>(entity);
                    let index = material.index(materials.len())?;
                    let pose = source_pose(
                        entity,
                        world,
                        &bone_indices,
                        &mut paths,
                        &mut BTreeSet::new(),
                    )?;
                    let base = materials[index].clone();
                    validate_draw_mesh(
                        assets
                            .meshes
                            .get(&mesh.0)
                            .context("missing particle mesh")?,
                        &base,
                        skin.is_some(),
                    )?;
                    if pose
                        .bone
                        .is_some_and(|bone| self.suppressed.contains(&bone))
                    {
                        continue;
                    }
                    let skin = skin
                        .map(|skin| -> Result<Skin> {
                            let binds = assets
                                .inverse_bindposes
                                .get(&skin.inverse_bindposes)
                                .context("missing particle inverse bind poses")?;
                            ensure!(
                                binds.len() == skin.joints.len(),
                                "particle skin binding count differs"
                            );
                            let mut indices = Vec::with_capacity(skin.joints.len());
                            for &entity in &skin.joints {
                                let index = if let Some(&index) = joint_indices.get(&entity) {
                                    index
                                } else {
                                    let pose = source_pose(
                                        entity,
                                        world,
                                        &bone_indices,
                                        &mut paths,
                                        &mut BTreeSet::new(),
                                    )?;
                                    let index = joints.len();
                                    joints.push(pose);
                                    joint_indices.insert(entity, index);
                                    index
                                };
                                indices.push(index);
                            }
                            Ok(Skin {
                                inverse_bindposes: skin.inverse_bindposes.clone(),
                                joints: indices,
                            })
                        })
                        .transpose()?;
                    let variants = styles
                        .iter()
                        .map(|&style| {
                            let mut surface = base.clone();
                            surface.toon_ramp =
                                (style.lit && !surface.constant_color && surface.color.is_some())
                                    .then(|| toon.clone());
                            surface.blend = Some(style.blend);
                            surface.depth_test = style.depth_test;
                            surface.depth_write = style.depth_write;
                            surface.depth_equal = true;
                            if !surface.constant_color {
                                surface.cull = if style.cull_back {
                                    resonance_content::CullFace::Back
                                } else {
                                    resonance_content::CullFace::None
                                };
                            }
                            (style, surface)
                        })
                        .collect();
                    draws.push(DrawRecipe {
                        mesh: mesh.0.clone(),
                        pose,
                        skin,
                        base,
                        styles: variants,
                        order: source.spec.scene.materials[index].draw_order,
                    });
                }
            }
            ensure!(
                source_draw_count > 0 || gltf.meshes.is_empty(),
                "particle scene has no declared mesh draws"
            );
        }
        // A model may contain only attachment transforms, or have every draw
        // deliberately suppressed. Its sampled pose can still drive children.
        self.draws = draws;
        self.joints = joints;
        Ok(true)
    }
}

fn source_pose(
    entity: Entity,
    world: &World,
    bones: &BTreeMap<Entity, usize>,
    paths: &mut BTreeMap<Entity, Pose>,
    visiting: &mut BTreeSet<Entity>,
) -> Result<Pose> {
    if let Some(&pose) = paths.get(&entity) {
        return Ok(pose);
    }
    ensure!(visiting.insert(entity), "cyclic model particle transforms");
    let pose = if let Some(&bone) = bones.get(&entity) {
        Pose {
            bone: Some(bone),
            suffix: Mat4::IDENTITY,
        }
    } else {
        let local = world
            .get::<Transform>(entity)
            .context("particle node has no transform")?
            .to_matrix();
        let parent = world
            .get::<ChildOf>(entity)
            .map(|parent| source_pose(parent.parent(), world, bones, paths, visiting))
            .transpose()?
            .unwrap_or(Pose {
                bone: None,
                suffix: Mat4::IDENTITY,
            });
        Pose {
            bone: parent.bone,
            suffix: parent.suffix * local,
        }
    };
    visiting.remove(&entity);
    paths.insert(entity, pose);
    Ok(pose)
}

fn appearance(
    handle: &Handle<TitleSurface>,
    base: &TitleSurface,
    particle: &ParticleFrame,
    style: ModelStyle,
    light: [f32; 3],
    surfaces: &mut Assets<TitleSurface>,
) -> Result<()> {
    let color = particle.state.colors[0].map(|value| value as u8);
    let rgb = Vec3::new(
        f32::from(color[0]),
        f32::from(color[1]),
        f32::from(color[2]),
    ) / 64.;
    let (ambient, tint, field_light) = if style.lit && base.toon_ramp.is_some() {
        (
            rgb,
            base.tint * Vec4::new(1., 1., 1., f32::from(color[3]) / 255.),
            Vec3::from_array(light).extend(192.),
        )
    } else {
        (
            Vec3::ONE,
            base.tint * rgb.extend(f32::from(color[3]) / 255.),
            Vec4::ZERO,
        )
    };
    let uv = model_uv(particle.state.uv);
    let uv_offsets = base.uv_offsets + Vec4::new(uv[0], uv[1], uv[0], uv[1]);
    let surface = surfaces
        .get(handle)
        .context("missing model particle material")?;
    if surface.ambient_scale != ambient
        || surface.tint != tint
        || surface.field_light != field_light
        || surface.uv_offsets != uv_offsets
    {
        let mut surface = surfaces.get_mut(handle).unwrap();
        surface.ambient_scale = ambient;
        surface.tint = tint;
        surface.field_light = field_light;
        surface.uv_offsets = uv_offsets;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn particles(count: usize) -> Result<Vec<ParticleFrame>> {
        use resonance_battle::{
            EffectAppearance, EffectBank, EffectDefinition, Effects, ParticleDefinition,
        };
        use resonance_content::battle_effect::{
            EffectOperation, ScheduledEvent, declaration::Declaration,
        };
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/masked-particle.json"))?;
        let declaration: Declaration = serde_json::from_value(fixture["declaration"].clone())?;
        let particle = ParticleDefinition {
            resource: 1,
            member: 0,
            model: None,
            data: declaration.particle()?,
        };
        let birth = ScheduledEvent {
            at: 0,
            operation: EffectOperation::Spawn {
                particle: 0,
                blend: None,
                palette: None,
                birth: Box::default(),
            },
        };
        let definition = EffectDefinition {
            events: vec![birth; count],
            particles: [(0, Arc::new(particle))].into(),
            sounds: BTreeMap::new(),
        };
        let diagnostics = Diagnostics::new(true);
        let bank = EffectBank::new(
            1,
            BTreeMap::new(),
            [(0, Arc::new(definition))].into(),
            &diagnostics,
        )?;
        let mut effects = Effects::new(vec![bank], None, 1, diagnostics)?.with_ambient(Some((
            EffectAppearance {
                resource: 1,
                member: 0,
            },
            u16::MAX,
        )))?;
        let frame = BattleFrame::default();
        effects.advance(&frame, false)?;
        effects.advance(&frame, false)?;
        let particles = effects.frames();
        ensure!(particles.len() == count, "expected every fixture particle");
        Ok(particles)
    }

    fn template(style: ModelStyle, skinned: bool) -> Template {
        let pose = Pose {
            bone: skinned.then_some(0),
            suffix: Mat4::IDENTITY,
        };
        let surface = TitleSurface {
            color: skinned.then(Handle::default),
            blend: Some(style.blend),
            cull: resonance_content::CullFace::None,
            ..default()
        };
        Template {
            sources: vec![],
            required_bones: usize::from(skinned),
            suppressed: BTreeSet::new(),
            joints: if skinned { vec![pose] } else { vec![] },
            draws: vec![DrawRecipe {
                mesh: Handle::default(),
                pose,
                skin: skinned.then(|| Skin {
                    inverse_bindposes: Handle::default(),
                    joints: vec![0],
                }),
                base: surface.clone(),
                styles: [(style, surface)].into(),
                order: 7,
            }],
            ready: true,
            disabled: false,
        }
    }

    #[test]
    fn overlapping_models_reuse_global_slots_and_publish_the_birth_pose() -> Result<()> {
        let style = ModelStyle {
            lit: false,
            blend: Blend::Additive,
            depth_test: true,
            depth_write: false,
            cull_back: false,
        };
        let mut pool = Pool::default();
        pool.templates.insert(10, template(style, true));
        pool.templates.insert(11, template(style, false));
        let mut particles = particles(2)?;
        for (index, particle) in particles.iter_mut().enumerate() {
            particle.model = Some(resonance_battle::EffectModelFrame {
                resource: 10,
                clip: None,
                frame: 0.,
                world: Mat4::IDENTITY.to_cols_array_2d(),
                bones: Arc::new(vec![
                    Mat4::from_translation(Vec3::new(index as f32 * 20., 0., 0.))
                        .to_cols_array_2d(),
                ]),
            });
            particle.state.colors[0] = [64, 64, 64, 100 + index as i16];
            particle.state.uv = [index as i16, 0, 2, 2];
        }
        let mut world = World::new();
        let initial_entities: BTreeSet<_> = world.query::<Entity>().iter(&world).collect();
        let mut surfaces = Assets::<TitleSurface>::default();
        for (order, particle) in particles.iter().enumerate() {
            pool.apply(
                particle,
                style,
                Layer::Effects,
                order,
                [0.; 3],
                &mut world.commands(),
                &mut surfaces,
            )?;
        }
        assert_eq!(pool.held_pose.len(), 4);
        let draws: Vec<_> = pool.draws.iter().map(|draw| draw.entity).collect();
        // These entities have not entered the World yet, but their birth poses
        // are already retained for publication and paused-frame restoration.
        assert!(world.get::<GlobalTransform>(draws[0]).is_none());
        world.flush();
        assert_eq!(pool.stats().active_particles, 2);
        assert_eq!(pool.stats().retained_draws, 2);
        assert_eq!(pool.stats().retained_joints, 2);
        assert_eq!(
            world
                .get::<GlobalTransform>(draws[1])
                .unwrap()
                .translation()
                .x,
            20.
        );
        let first = surfaces.get(&pool.draws[0].material).unwrap();
        let second = surfaces.get(&pool.draws[1].material).unwrap();
        assert_ne!(first.tint, second.tint);
        assert_ne!(first.uv_offsets, second.uv_offsets);
        assert_eq!(pool.templates[&10].draws[0].styles[&style].tint, Vec4::ONE);
        let a = world.get::<SkinnedMesh>(draws[0]).unwrap();
        let b = world.get::<SkinnedMesh>(draws[1]).unwrap();
        assert_ne!(a.joints, b.joints);
        assert_eq!(world.get::<DrawOrder>(draws[1]).unwrap().1, 1);
        for &(entity, pose) in pool.held_pose() {
            assert_eq!(*world.get::<GlobalTransform>(entity).unwrap(), pose);
        }

        pool.retain(&[], &mut world.commands(), &mut surfaces);
        world.flush();
        assert_eq!(pool.stats().active_draws, 0);
        assert!(
            pool.draws
                .iter()
                .all(|draw| surfaces.get(&draw.material).unwrap().color.is_none())
        );
        // Rebind a previous skin draw to an unrelated rigid model in the same
        // update as retirement. Neither the draw pool nor materials grow.
        particles[0].model.as_mut().unwrap().resource = 11;
        particles[0].model.as_mut().unwrap().bones = Arc::new(vec![]);
        particles[0].state.uv = [0; 4];
        pool.clear_pose();
        pool.apply(
            &particles[0],
            style,
            Layer::Effects,
            4,
            [0.; 3],
            &mut world.commands(),
            &mut surfaces,
        )?;
        world.flush();
        let active = pool
            .instances
            .iter()
            .find(|instance| instance.key.is_some())
            .unwrap();
        let draw = &pool.draws[active.draws[0]];
        assert!(world.get::<SkinnedMesh>(draw.entity).is_none());
        assert_eq!(
            world.get::<Visibility>(draw.entity),
            Some(&Visibility::Inherited)
        );
        assert_eq!(surfaces.get(&draw.material).unwrap().uv_offsets, Vec4::ZERO);
        assert!(surfaces.get(&draw.material).unwrap().color.is_none());
        assert_eq!(pool.stats().retained_draws, 2);
        assert_eq!(surfaces.len(), 2);
        pool.hide(particles[0].id, &mut world.commands());
        world.flush();
        assert_eq!(
            world.get::<Visibility>(draw.entity),
            Some(&Visibility::Hidden)
        );
        let mut hidden = template(style, false);
        hidden.draws.clear();
        pool.templates.insert(12, hidden);
        particles[0].model.as_mut().unwrap().resource = 12;
        pool.retain(&particles[..1], &mut world.commands(), &mut surfaces);
        pool.apply(
            &particles[0],
            style,
            Layer::Effects,
            0,
            [0.; 3],
            &mut world.commands(),
            &mut surfaces,
        )?;
        assert_eq!(pool.stats().active_particles, 1);
        assert_eq!(pool.stats().active_draws, 0);
        assert_eq!(pool.stats().active_joints, 0);

        // Dense overlaps grow the reusable pool instead of dropping model draws.
        particles[0].model.as_mut().unwrap().resource = 11;
        let overlapping: Vec<_> = self::particles(300)?
            .into_iter()
            .map(|particle| ParticleFrame {
                id: particle.id,
                ..particles[0].clone()
            })
            .collect();
        for _ in 0..2 {
            pool.retain(&[], &mut world.commands(), &mut surfaces);
            pool.clear_pose();
            for (order, particle) in overlapping.iter().enumerate() {
                pool.apply(
                    particle,
                    style,
                    Layer::Effects,
                    order,
                    [0.; 3],
                    &mut world.commands(),
                    &mut surfaces,
                )?;
            }
            world.flush();
            assert_eq!(pool.stats().active_particles, overlapping.len());
            assert_eq!(pool.stats().active_draws, overlapping.len());
            assert_eq!(pool.stats().created_draws, overlapping.len());
            assert_eq!(surfaces.len(), overlapping.len());
        }
        let owned: Vec<_> = pool
            .draws
            .iter()
            .map(|draw| draw.entity)
            .chain(pool.joints.iter().copied())
            .collect();
        pool.despawn(&mut world.commands());
        world.flush();
        let remaining: BTreeSet<_> = world.query::<Entity>().iter(&world).collect();
        assert_eq!(remaining, initial_entities);
        assert!(
            owned
                .iter()
                .all(|&entity| !world.entities().contains_spawned(entity))
        );
        Ok(())
    }

    #[test]
    fn rigid_source_draws_do_not_require_a_registered_skin_component() {
        let mut world = World::new();
        let entity = world.spawn((Mesh3d::default(), MaterialSlot(0))).id();
        assert!(world.components().component_id::<SkinnedMesh>().is_none());
        let mut query = world.try_query::<SourceMeshes>().expect("rigid mesh query");
        let draws: Vec<_> = query.iter(&world).map(|(entity, _, _)| entity).collect();
        assert_eq!(draws, [entity]);
        assert!(world.get::<SkinnedMesh>(entity).is_none());
    }

    #[test]
    fn flat_pose_preserves_affine_bones_and_independent_child_draws() -> Result<()> {
        let mut world = World::new();
        let root = world.spawn(Transform::from_xyz(100., 0., 0.)).id();
        let bone = world.spawn((Transform::default(), ChildOf(root))).id();
        let suffix = Transform::from_xyz(2., 3., 4.);
        let draw = world.spawn((suffix, ChildOf(bone))).id();
        let child_bone = world.spawn((Transform::default(), ChildOf(draw))).id();
        let child_draw = world
            .spawn((Transform::default(), ChildOf(child_bone)))
            .id();
        let indices = [(bone, 0), (child_bone, 1)].into();
        let mut paths = BTreeMap::new();
        let pose = source_pose(draw, &world, &indices, &mut paths, &mut BTreeSet::new())?;
        let child = source_pose(
            child_draw,
            &world,
            &indices,
            &mut paths,
            &mut BTreeSet::new(),
        )?;
        assert_eq!(pose.bone, Some(0));
        assert_eq!(child.bone, Some(1));
        let affine = Mat4::from_cols(Vec4::ZERO, Vec4::new(0.5, 1., 0., 0.), Vec4::Z, Vec4::W);
        let global = Mat4::from_translation(Vec3::new(8., 9., 10.));
        let sampled = pose.sample(
            global,
            &[affine.to_cols_array_2d(), Mat4::IDENTITY.to_cols_array_2d()],
        )?;
        assert_eq!(sampled.to_matrix(), global * affine * suffix.to_matrix());
        assert_eq!(
            child
                .sample(
                    global,
                    &[affine.to_cols_array_2d(), Mat4::IDENTITY.to_cols_array_2d()]
                )?
                .to_matrix(),
            global
        );
        Ok(())
    }
}
