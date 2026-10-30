//! Named authoring values for the two ordinary effect buses. DSP keeps arrays.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub coloration: f32,
    pub mix: f32,
    pub decay_seconds: f32,
    pub damping: f32,
    pub predelay_seconds: f32,
}
impl From<[f32; 5]> for Parameters {
    fn from([coloration, mix, decay_seconds, damping, predelay_seconds]: [f32; 5]) -> Self {
        Self {
            coloration,
            mix,
            decay_seconds,
            damping,
            predelay_seconds,
        }
    }
}
impl From<Parameters> for [f32; 5] {
    fn from(p: Parameters) -> Self {
        [
            p.coloration,
            p.mix,
            p.decay_seconds,
            p.damping,
            p.predelay_seconds,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Buses<T> {
    pub auxiliary_a: T,
    pub auxiliary_b: T,
}

pub mod optional {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        pair: &Option<[[f32; 5]; 2]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        pair.map(|[a, b]| Buses {
            auxiliary_a: Parameters::from(a),
            auxiliary_b: Parameters::from(b),
        })
        .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<[[f32; 5]; 2]>, D::Error> {
        Ok(Option::<Buses<Parameters>>::deserialize(deserializer)?
            .map(|b| [b.auxiliary_a.into(), b.auxiliary_b.into()]))
    }
}
