//! Shared mixer settings. Places own selections and optional local acoustics;
//! the library owns its volume curve and music-driven reverb changes once.
use super::MusicReverbs;
use crate::{library::Snapshot, preparation::JSON_LIMIT};
use anyhow::Result;
use serde::{Deserialize, Serialize};

mod document;

pub const PATH: &str = "audio/mix.json";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "document::Document")]
pub struct Mix {
    pub music_reverbs: MusicReverbs,
    pub voice_gains: Vec<f32>,
    pub field_reverbs: [[f32; 5]; 2],
    pub battle_reverbs: [[f32; 5]; 2],
}

impl Serialize for Mix {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        document::Document::from(self.clone()).serialize(serializer)
    }
}

/// Prepared settings for one context; never serialized into each field.
#[derive(Debug, Clone)]
pub struct Environment {
    pub music_reverbs: MusicReverbs,
    pub voice_gains: [f32; 128],
    pub reverbs: [[f32; 5]; 2],
}

impl Mix {
    pub fn read(snapshot: &mut Snapshot) -> Result<Self> {
        snapshot.json(PATH, JSON_LIMIT)
    }

    pub fn validate(&self) -> Result<()> {
        self.music_reverbs.validate()?;
        super::validate_voice_gains(&self.voice_gains)?;
        validate_reverbs(self.field_reverbs)?;
        validate_reverbs(self.battle_reverbs)
    }

    pub fn environment(self, reverbs: [[f32; 5]; 2]) -> Result<Environment> {
        validate_reverbs(reverbs)?;
        Ok(Environment {
            music_reverbs: self.music_reverbs,
            voice_gains: super::validate_voice_gains(&self.voice_gains)?,
            reverbs,
        })
    }
}

impl Environment {
    pub fn program_reverbs(&self, music: Option<i16>) -> Result<[[f32; 5]; 2]> {
        let mut parameters = self.reverbs;
        if let Some(id) = music
            && let Some(selected) = self.music_reverbs.for_song(id)?
        {
            parameters[0] = selected;
        }
        Ok(parameters)
    }
}

fn validate_reverbs(reverbs: [[f32; 5]; 2]) -> Result<()> {
    for parameters in reverbs {
        MusicReverbs::validate_preset(parameters)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
