use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Model,
    Camera,
    Overlay,
}
#[derive(Default)]
pub struct ResourceLibrary {
    pub blink: Option<resonance_content::effect::BlinkCycle>,
    pub menu_data: Option<std::sync::Arc<resonance_content::menu_data::MenuData>>,
    pub skits: Option<std::sync::Arc<resonance_content::skit::SkitCatalog>>,
    pub bindings: BTreeMap<i32, (ResourceKind, u32)>,
    pub models: BTreeMap<u32, ModelResource>,
    pub particles: BTreeMap<i32, ParticleKind>,
    pub messages: Vec<symphonia_script::message::Message>,
    pub actor_names: BTreeMap<i32, String>,
    pub text: std::sync::Arc<resonance_content::session::GameText>,
    pub movies: BTreeSet<u32>,
    pub locators: BTreeSet<i32>,
    pub session_data: Option<std::sync::Arc<resonance_content::session::SessionData>>,
    pub fields: BTreeSet<u32>,
    pub doors: Vec<resonance_content::field::Door>,
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
        self.bindings
            .get(&script_id)
            .filter(|(k, _)| *k == kind)
            .map(|(_, id)| *id)
            .ok_or_else(|| format!("unmapped {kind:?} resource {script_id}"))
    }
    pub fn model(&self, asset: u32) -> Option<&ModelResource> {
        self.models.get(&asset)
    }
}
#[derive(Default)]
pub struct ModelResource {
    pub has_eyes: bool,
    pub names: Vec<String>,
    pub hidden_nodes: BTreeSet<u16>,
    pub clips: BTreeMap<u16, AnimationClip>,
}
pub struct AnimationClip {
    pub duration_ticks: u32,
    pub attachments: BTreeMap<String, AttachmentTrack>,
}
impl From<&resonance_content::SceneClip> for AnimationClip {
    fn from(clip: &resonance_content::SceneClip) -> Self {
        Self {
            duration_ticks: clip.duration_ticks(),
            attachments: BTreeMap::new(),
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
pub enum AttachmentTrack {
    Constant([f32; 3]),
    Samples(Vec<[f32; 3]>),
}
impl AttachmentTrack {
    pub fn sample(&self, tick: u32) -> Option<[f32; 3]> {
        match self {
            Self::Constant(p) => Some(*p),
            Self::Samples(p) => p.get(tick as usize).copied(),
        }
    }
}
