use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Model,
    Animation,
    UnboundGeometry,
    Camera,
    Overlay,
}
#[derive(Default)]
pub struct MemoryCircleText {
    pub tutorial: Vec<resonance_content::font::TextSpan>,
    pub unlock: Vec<resonance_content::font::TextSpan>,
    pub no_gem: Vec<resonance_content::font::TextSpan>,
}
#[derive(Default)]
pub struct ResourceLibrary {
    pub texture_animations: Vec<resonance_content::field::FieldTextureAnimation>,
    pub field_wind: Option<resonance_content::field::FieldWind>,
    pub rising_light_destination: Option<[f32; 3]>,
    pub memory_circle_text: MemoryCircleText,
    pub station_script: Option<std::sync::Arc<symphonia_script::Program>>,
    pub blink: Option<resonance_content::effect::BlinkCycle>,
    pub menu_data: Option<std::sync::Arc<resonance_content::menu_data::MenuData>>,
    pub skits: Option<std::sync::Arc<resonance_content::skit::SkitCatalog>>,
    pub bindings: BTreeMap<i32, (ResourceKind, u32)>,
    pub models: BTreeMap<u32, ModelResource>,
    pub animations: BTreeMap<u32, BTreeMap<u16, AnimationClip>>,
    pub particles: BTreeMap<i32, ParticleKind>,
    pub messages: Vec<symphonia_script::message::Message>,
    pub actor_names: BTreeMap<i32, String>,
    pub text: std::sync::Arc<resonance_content::session::GameText>,
    pub movies: BTreeSet<u32>,
    pub locators: BTreeSet<i32>,
    pub session_data: Option<std::sync::Arc<resonance_content::session::SessionData>>,
    pub fields: BTreeSet<u32>,
    pub doors: Vec<resonance_content::field::Door>,
    pub camera_tracks: BTreeMap<u32, Vec<resonance_content::CameraKey>>,
}
pub enum ParticleKind {
    Glow,
    Flutter(resonance_content::effect::FlutterRecipe),
}
impl ResourceLibrary {
    pub fn names(&self, party: Option<&crate::party::Party>) -> BTreeMap<i32, String> {
        let mut names = self.actor_names.clone();
        names.extend(self.text.characters.clone());
        if let Some(party) = party {
            names.extend(
                party
                    .members
                    .iter()
                    .enumerate()
                    .filter_map(|(i, m)| m.name.as_ref().map(|name| (i as i32 + 1, name.clone()))),
            );
        }
        names
    }
    pub fn character_names() -> BTreeMap<i32, String> {
        [
            "Lloyd", "Colette", "Genis", "Raine", "Sheena", "Zelos", "Presea", "Regal", "Kratos",
        ]
        .into_iter()
        .enumerate()
        .map(|(i, name)| (i as i32 + 1, name.into()))
        .collect()
    }
    pub fn resolve(&self, script_id: i32, kind: ResourceKind) -> Result<u32, String> {
        if matches!(
            self.bindings.get(&script_id),
            Some((ResourceKind::UnboundGeometry, _))
        ) {
            return Err(format!(
                "geometry resource {script_id:#x} requires caller-supplied textures"
            ));
        }
        self.bindings
            .get(&script_id)
            .filter(|(k, _)| *k == kind)
            .map(|(_, id)| *id)
            .ok_or_else(|| format!("unmapped {kind:?} resource {script_id}"))
    }
    pub fn binding(&self, script_id: i32) -> Option<(ResourceKind, u32)> {
        self.animations
            .contains_key(&(script_id as u32))
            .then_some((ResourceKind::Animation, script_id as u32))
            .or_else(|| self.bindings.get(&script_id).copied())
    }
    pub fn model(&self, asset: u32) -> Option<&ModelResource> {
        self.models.get(&asset)
    }

    pub fn clips(
        &self,
        resource: u32,
        source: crate::animation::AnimationSource,
    ) -> Option<&BTreeMap<u16, AnimationClip>> {
        match source {
            crate::animation::AnimationSource::Model => {
                self.models.get(&resource).map(|model| &model.clips)
            }
            crate::animation::AnimationSource::Resource => self.animations.get(&resource),
        }
    }

    pub fn attachment_point(
        &self,
        actor: &crate::Actor,
        node: usize,
        tick: u32,
    ) -> Result<[f32; 3], String> {
        let model = self
            .model(actor.model_resource())
            .ok_or("actor model is missing")?;
        let name = model.names.get(node).ok_or("bone index is missing")?;
        let matrix = self
            .bone_matrix(actor, name, tick)
            .map_err(|e| format!("{e:#}"))?;
        let point = matrix[3][..3].try_into().unwrap();
        Ok(resonance_content::animation::transform_point(
            actor.local_matrix(),
            point,
        ))
    }

    /// Local node position, independent of the actor's placement and its parents.
    pub(crate) fn node_translation(
        &self,
        actor: &crate::Actor,
        name: &str,
        tick: u32,
    ) -> anyhow::Result<[f32; 3]> {
        use anyhow::Context;
        let model = self
            .model(actor.model_resource())
            .context("node model missing")?;
        let skeleton = model
            .attachments
            .skeleton
            .as_ref()
            .context("node skeleton missing")?;
        let bone = skeleton.bone(name).context("node missing")?;
        let bind = skeleton.bones[usize::from(bone)].bind;
        let mut position = bind.translation;
        for animation in actor
            .animation
            .iter()
            .chain(actor.scenery_animations.values())
            .rev()
        {
            let pose = self
                .attachment_animation_pose(actor, animation)
                .context("node animation pose missing")?;
            if let Some(track) = pose.motion.tracks.iter().find(|track| track.bone == bone) {
                let clip = self
                    .animation(animation)
                    .context("node animation missing")?;
                let elapsed = animation.sample(
                    tick,
                    model.attachment_pose_delay,
                    clip.duration_ticks as f32,
                );
                position = track.sample(pose.frame(elapsed), bind)?.translation;
                break;
            }
        }
        for adjustment in actor.appearance.bone_adjustments.values() {
            let matches = match &adjustment.bone {
                crate::BoneTarget::Name(target) => target == name,
                crate::BoneTarget::Index(target) => *target == bone,
            };
            if matches
                && adjustment
                    .translation
                    .as_ref()
                    .is_some_and(|translation| translation.absolute)
            {
                position = adjustment.translation(tick);
            }
        }
        Ok(position)
    }

    pub(crate) fn bone_matrix(
        &self,
        actor: &crate::Actor,
        name: &str,
        tick: u32,
    ) -> anyhow::Result<resonance_content::animation::Matrix> {
        use anyhow::Context;
        let model = self
            .model(actor.model_resource())
            .context("attachment model is missing")?;
        let skeleton = match actor
            .scenery_animations
            .values()
            .next()
            .or(actor.animation.as_ref())
        {
            Some(animation) => {
                &self
                    .attachment_animation_pose(actor, animation)
                    .context("attachment pose is not prepared")?
                    .skeleton
            }
            None => model
                .attachments
                .skeleton
                .as_ref()
                .context("attachment rest pose is not prepared")?,
        };
        let bone = skeleton.bone(name).context("attachment bone is missing")?;
        let layers = actor
            .animation
            .iter()
            .chain(actor.scenery_animations.values())
            .map(|animation| {
                let clip = self
                    .animation(animation)
                    .context("attachment clip is missing")?;
                let pose = self
                    .attachment_animation_pose(actor, animation)
                    .context("attachment pose is not prepared")?;
                let elapsed = animation.sample(
                    tick,
                    model.attachment_pose_delay,
                    clip.duration_ticks as f32,
                );
                Ok((pose.motion.as_ref(), pose.frame(elapsed)))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        skeleton.sample_adjusted_matrix(&layers, bone, &|index, bone, mut local| {
            use resonance_content::animation::{euler_rotation, pose::LocalPose};
            for adjustment in actor
                .appearance
                .bone_adjustments
                .values()
                .filter(|adjustment| match &adjustment.bone {
                    crate::BoneTarget::Name(name) => name == &bone.name,
                    crate::BoneTarget::Index(target) => *target == index,
                })
            {
                let angles = adjustment.sample(tick);
                let rotation = euler_rotation(angles);
                if adjustment.absolute_rotation {
                    let mut value = match local {
                        LocalPose::Trs(value) => value,
                        LocalPose::Affine(_) => bone.bind,
                    };
                    value.rotation = rotation;
                    local = LocalPose::Trs(value);
                } else if angles != [0.; 3] {
                    local = local.rotate(rotation);
                }
                if let Some(scale) = &adjustment.scale {
                    local = local.with_scale(scale.sample(tick, bone.bind.scale));
                }
                if adjustment
                    .translation
                    .as_ref()
                    .is_some_and(|translation| translation.absolute)
                {
                    let position = adjustment.translation(tick);
                    match &mut local {
                        LocalPose::Trs(value) => value.translation = position,
                        LocalPose::Affine(matrix) => matrix[3][..3].copy_from_slice(&position),
                    }
                }
            }
            local
        })
    }

    pub fn attachment_pose(&self, actor: &crate::Actor) -> Option<&AttachmentPose> {
        let animation = actor.animation.as_ref()?;
        self.attachment_animation_pose(actor, animation)
    }

    fn attachment_animation_pose(
        &self,
        actor: &crate::Actor,
        animation: &crate::Animation,
    ) -> Option<&AttachmentPose> {
        let resource = if animation.source == crate::animation::AnimationSource::Model
            && animation.resource == actor.resource
        {
            actor.model_resource()
        } else {
            animation.resource
        };
        self.model(actor.model_resource())?
            .attachments
            .clips
            .get(&(animation.source, resource, animation.slot))
            .or_else(|| self.animation(animation)?.attachments.as_ref())
    }

    pub fn animation(&self, animation: &crate::Animation) -> Option<&AnimationClip> {
        self.clips(animation.resource, animation.source)?
            .get(&animation.slot)
    }
}
#[derive(Default)]
pub struct ModelResource {
    pub collision: std::sync::Arc<resonance_content::field::ModelCollision>,
    pub has_eyes: bool,
    pub toon_lighting: bool,
    /// Attachment queries observe the scene's last evaluated model pose.
    pub attachment_pose_delay: u32,
    pub names: Vec<String>,
    pub hidden_nodes: BTreeSet<u16>,
    pub clips: BTreeMap<u16, AnimationClip>,
    /// Pose bindings belong to the receiving skeleton, including external motion banks.
    pub attachments: ModelAttachments,
}
#[derive(Clone, Default)]
pub struct ModelAttachments {
    pub skeleton: Option<std::sync::Arc<resonance_content::animation::Skeleton>>,
    pub mesh_bounds: Option<[[f32; 3]; 2]>,
    pub clips: BTreeMap<(crate::animation::AnimationSource, u32, u16), AttachmentPose>,
}
pub struct AnimationClip {
    pub duration_ticks: u32,
    pub attachments: Option<AttachmentPose>,
}
impl From<&resonance_content::SceneClip> for AnimationClip {
    fn from(clip: &resonance_content::SceneClip) -> Self {
        Self {
            duration_ticks: clip.duration_ticks(),
            attachments: None,
        }
    }
}
impl AnimationClip {
    pub fn sample_tick(&self, elapsed: u32, repeat: bool) -> u32 {
        if repeat && self.duration_ticks > 0 && elapsed > self.duration_ticks {
            let phase = elapsed % self.duration_ticks;
            if phase == 0 {
                self.duration_ticks
            } else {
                phase
            }
        } else {
            elapsed.min(self.duration_ticks)
        }
    }
}
#[derive(Clone)]
pub struct AttachmentPose {
    skeleton: std::sync::Arc<resonance_content::animation::Skeleton>,
    motion: std::sync::Arc<resonance_content::animation::Motion>,
}
impl AttachmentPose {
    pub fn new(
        skeleton: std::sync::Arc<resonance_content::animation::Skeleton>,
        motion: std::sync::Arc<resonance_content::animation::Motion>,
    ) -> anyhow::Result<Self> {
        skeleton.validate()?;
        motion.validate(&skeleton)?;
        Ok(Self { skeleton, motion })
    }

    pub fn sample(&self, name: &str, tick: f32) -> anyhow::Result<[f32; 3]> {
        self.sample_offset(name, tick, [0.; 3])
    }

    pub fn sample_offset(
        &self,
        name: &str,
        tick: f32,
        offset: [f32; 3],
    ) -> anyhow::Result<[f32; 3]> {
        let point =
            resonance_content::animation::transform_point(self.sample_matrix(name, tick)?, offset);
        anyhow::ensure!(
            point.iter().all(|v| v.is_finite()),
            "invalid attachment position"
        );
        Ok(point)
    }

    pub fn sample_matrix(
        &self,
        name: &str,
        tick: f32,
    ) -> anyhow::Result<resonance_content::animation::Matrix> {
        anyhow::ensure!(tick.is_finite() && tick >= 0., "invalid attachment time");
        let bone = self
            .skeleton
            .bone(name)
            .ok_or_else(|| anyhow::anyhow!("missing attachment bone {name}"))?;
        self.skeleton
            .sample_matrix(&self.motion, self.frame(tick), bone)
    }

    fn frame(&self, tick: f32) -> f32 {
        (tick * resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ)
            .min(self.motion.duration_frames)
    }
}
