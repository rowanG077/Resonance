//! Shared model recipes and reusable render objects for live particles.
use super::*;
use resonance_battle::ParticleFrame;
use resonance_battle::ResourceId;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum TemplateState {
    Pending,
    Ready,
    Disabled,
}

struct Template {
    sources: Vec<Source>,
    required_bones: usize,
    suppressed: BTreeSet<usize>,
    joints: Vec<Pose>,
    draws: Vec<DrawRecipe>,
    state: TemplateState,
}

struct Draw {
    entity: Entity,
    material: Handle<TitleSurface>,
}

#[derive(Default)]
struct Instance {
    key: Option<(ParticleId, ResourceId)>,
    draws: Vec<usize>,
    joints: Vec<usize>,
    style: Option<ModelStyle>,
}

#[derive(Default)]
pub(super) struct Pool {
    templates: BTreeMap<ResourceId, Template>,
    instances: Vec<Instance>,
    draws: Vec<Draw>,
    joints: Vec<Entity>,
    free_draws: Vec<usize>,
    free_joints: Vec<usize>,
    // The admission report stores IDs, so the scene must retain these bindings
    // through asynchronous pipeline preparation and its completion fence.
    warm_materials: Vec<Handle<TitleSurface>>,
    held_pose: Vec<(Entity, GlobalTransform)>,
}

impl Pool {
    pub fn load(
        &mut self,
        resource: ResourceId,
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
                state: TemplateState::Pending,
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

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        view: Entity,
        report: &mut gpu::Report,
        server: &AssetServer,
        assets: &mut AssetsForView,
        sampled: &mut crate::scene::SampledImages,
        toon: &Handle<Image>,
        styles: &BTreeSet<ModelStyle>,
        diagnostics: &Diagnostics,
    ) -> Result<bool> {
        for (&resource, template) in &mut self.templates {
            if template.state != TemplateState::Pending {
                continue;
            }
            let result = template.compile(server, assets, sampled, toon, styles);
            template.state = match diagnostics.attempt(
                &format!("battle model particle template {resource}"),
                result,
            )? {
                None => TemplateState::Disabled,
                Some(false) => continue,
                Some(true) => {
                    let mut variants = std::collections::HashMap::new();
                    for draw in &template.draws {
                        for surface in std::iter::once(&draw.base).chain(draw.styles.values()) {
                            let material = variants
                                .entry(crate::materials::SurfaceKey::from(surface))
                                .or_insert_with(|| {
                                    let material = assets.surfaces.add(surface.clone());
                                    self.warm_materials.push(material.clone());
                                    material
                                });
                            report.prepare(
                                view,
                                draw.mesh.id(),
                                material.id().untyped(),
                                false,
                                None,
                            );
                        }
                    }
                    TemplateState::Ready
                }
            };
        }
        Ok(self
            .templates
            .values()
            .all(|template| template.state != TemplateState::Pending))
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
        match template.state {
            TemplateState::Pending => anyhow::bail!("model particle template is not ready"),
            TemplateState::Disabled => return Ok(()),
            TemplateState::Ready => {}
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
            let layout = crate::model_asset::prepared(scene)?;
            ensure!(
                layout.bones.len() == source.spec.scene.bone_names.len(),
                "particle skeleton differs from declared bones"
            );
            let pose = |node: &crate::model_asset::Node| Pose {
                bone: node.bone.map(|bone| source.bone_offset + bone),
                suffix: node.suffix,
            };
            let mut joint_indices = BTreeMap::new();
            for node in &layout.nodes {
                let Some(draw) = &node.draw else { continue };
                let index = draw.slot.index(materials.len())?;
                let base = materials[index].clone();
                validate_draw_mesh(
                    assets
                        .meshes
                        .get(&draw.mesh)
                        .context("missing particle mesh")?,
                    &base,
                    draw.skin.is_some(),
                )?;
                let draw_pose = pose(node);
                if draw_pose
                    .bone
                    .is_some_and(|bone| self.suppressed.contains(&bone))
                {
                    continue;
                }
                let skin = draw
                    .skin
                    .as_ref()
                    .map(|skin| -> Result<_> {
                        ensure!(
                            assets
                                .inverse_bindposes
                                .get(&skin.inverse_bindposes)
                                .context("missing particle inverse bind poses")?
                                .len()
                                == skin.joints.len(),
                            "particle skin binding count differs"
                        );
                        let indices = skin
                            .joints
                            .iter()
                            .map(|&node| {
                                *joint_indices.entry(node).or_insert_with(|| {
                                    let index = joints.len();
                                    joints.push(pose(&layout.nodes[node]));
                                    index
                                })
                            })
                            .collect();
                        Ok(Skin {
                            inverse_bindposes: skin.inverse_bindposes.clone(),
                            joints: indices,
                        })
                    })
                    .transpose()?;
                draws.push(DrawRecipe {
                    mesh: draw.mesh.clone(),
                    pose: draw_pose,
                    skin,
                    base: base.clone(),
                    styles: styles
                        .iter()
                        .map(|&style| (style, style.surface(base.clone(), toon)))
                        .collect(),
                    order: source.spec.scene.materials[index].draw_order,
                });
            }
            ensure!(
                layout.nodes.iter().any(|node| node.draw.is_some()) || gltf.meshes.is_empty(),
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
            events: vec![birth; count].into(),
            particles: [(0, Arc::new(particle))].into(),
        };
        let diagnostics = Diagnostics::new(true);
        let bank = EffectBank::new(
            ResourceId(1),
            BTreeMap::new(),
            [(0, Arc::new(definition))].into(),
            &mut |_| Ok(None),
            &diagnostics,
        )?;
        let mut effects = Effects::new(vec![bank], None, 1, diagnostics)?.with_ambient(Some((
            EffectAppearance {
                resource: ResourceId(1),
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
            state: TemplateState::Ready,
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
        pool.templates.insert(ResourceId(10), template(style, true));
        pool.templates
            .insert(ResourceId(11), template(style, false));
        let mut particles = particles(2)?;
        for (index, particle) in particles.iter_mut().enumerate() {
            particle.model = Some(resonance_battle::EffectModelFrame {
                resource: ResourceId(10),
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
        assert_eq!(
            pool.templates[&ResourceId(10)].draws[0].styles[&style].tint,
            Vec4::ONE
        );
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
        particles[0].model.as_mut().unwrap().resource = ResourceId(11);
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
        pool.templates.insert(ResourceId(12), hidden);
        particles[0].model.as_mut().unwrap().resource = ResourceId(12);
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
        particles[0].model.as_mut().unwrap().resource = ResourceId(11);
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
}
