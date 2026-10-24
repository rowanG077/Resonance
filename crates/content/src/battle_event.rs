//! Authored battle scenes and the model poses they own while combat is suspended.
use crate::{ScenePart, animation::Skeleton};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod document;

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    /// Captured authored blink cycle for this event.
    #[serde(skip)]
    pub blink: Option<crate::effect::BlinkCycle>,
    pub script: String,
    pub messages: String,
    pub audio: crate::battle_audio::Selection,
    pub actors: BTreeMap<u32, Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub skeleton: Skeleton,
    pub parts: Vec<ScenePart>,
}
