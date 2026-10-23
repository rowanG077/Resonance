//! Authored battle scenes and the model poses they own while combat is suspended.
use crate::{ScenePart, animation::Skeleton, field_preload::File};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub fn path(id: u16) -> String {
    format!("battle/events/{id:03}.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub script: String,
    pub messages: String,
    pub audio: crate::battle_audio::Selection,
    pub actors: BTreeMap<u32, Model>,
    pub files: BTreeMap<String, File>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub skeleton: Skeleton,
    pub parts: Vec<ScenePart>,
}
