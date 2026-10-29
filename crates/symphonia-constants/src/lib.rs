//! Known game identities, shared by Rust and cooked script definitions.
//!
//! These enums describe known values; asset mappings and script service boundaries
//! still accept raw IDs, including IDs added by mods. Resource names are initial
//! publication names, independent of localized captions and editable mappings.

use std::fmt;

/// Common metadata for generating definitions without a second list of values.
pub trait KnownId: Copy + 'static {
    const ALL: &'static [Self];
    const TYPE_NAME: &'static str;
    fn value(self) -> u32;
    fn name(self) -> &'static str;
    fn resource_name(self) -> Option<&'static str>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownId(pub &'static str, pub u32);
impl fmt::Display for UnknownId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {} {}", self.0, self.1)
    }
}
impl std::error::Error for UnknownId {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownName(pub &'static str, pub String);
impl fmt::Display for UnknownName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {} {:?}", self.0, self.1)
    }
}
impl std::error::Error for UnknownName {}

macro_rules! ids {
    ($(#[$attribute:meta])* pub enum $name:ident: $repr:ident {
        $($variant:ident = $value:expr $(=> $resource:expr)?),* $(,)?
    }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        $(#[$attribute])*
        #[repr($repr)]
        pub enum $name { $($variant = $value),* }

        impl $name {
            pub const ALL: [Self; <[()]>::len(&[$(ids!(@unit $variant)),*])] = [$(Self::$variant),*];
            pub const fn raw(self) -> $repr { self as $repr }
            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => stringify!($variant)),* }
            }
            pub const fn resource_name(self) -> Option<&'static str> {
                match self { $(Self::$variant => ids!(@resource $($resource)?)),* }
            }
        }
        impl crate::KnownId for $name {
            const ALL: &'static [Self] = &Self::ALL;
            const TYPE_NAME: &'static str = stringify!($name);
            fn value(self) -> u32 { self as u32 }
            fn name(self) -> &'static str { self.name() }
            fn resource_name(self) -> Option<&'static str> { self.resource_name() }
        }
        impl From<$name> for $repr {
            fn from(value: $name) -> Self { value.raw() }
        }
        impl TryFrom<$repr> for $name {
            type Error = crate::UnknownId;
            fn try_from(value: $repr) -> Result<Self, crate::UnknownId> {
                match value {
                    $(v if v == Self::$variant as $repr => Ok(Self::$variant),)*
                    _ => Err(crate::UnknownId(stringify!($name), value as u32)),
                }
            }
        }
        impl std::str::FromStr for $name {
            type Err = crate::UnknownName;
            fn from_str(value: &str) -> Result<Self, crate::UnknownName> {
                match value {
                    $(stringify!($variant) => Ok(Self::$variant),)*
                    _ => Err(crate::UnknownName(stringify!($name), value.into())),
                }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.name())
            }
        }
    };
    (@unit $variant:ident) => { () };
    (@resource $resource:expr) => { Some($resource) };
    (@resource) => { None };
}

mod arenas;
mod battle_scenes;
mod characters;
mod effects;
mod field_system;
mod fields;
mod figurines;
mod items;
mod monsters;
mod music;
mod story;
mod treasure;
mod world;
pub use arenas::*;
pub use battle_scenes::*;
pub use characters::*;
pub use effects::*;
pub use field_system::*;
pub use fields::*;
pub use figurines::*;
pub use items::*;
pub use monsters::*;
pub use music::*;
pub use story::*;
pub use treasure::*;
pub use world::*;

/// Semantic aliases used by model behaviors. Other node names come from the model.
pub const MODEL_NODES: &[(&str, &str)] = &[
    ("SwordDancerTail", "Bone_sippo01"),
    ("SwordDancerLeftWing", "Bone_hane01_L"),
    ("SwordDancerRightWing", "Bone_hane01_R"),
    ("FugitiveWings", "kk06_Hane"),
    ("YggdrasillRightHandAttachment", "kk00"),
];

impl PartyCharacterId {
    /// UI and publication order follows the same enum used by gameplay IDs.
    pub const RESOURCE_NAMES: [&'static str; Self::ALL.len()] = {
        let mut names = [""; Self::ALL.len()];
        let mut i = 0;
        while i < Self::ALL.len() {
            names[i] = Self::ALL[i].resource_name().unwrap();
            i += 1;
        }
        names
    };

    pub fn from_index(index: usize) -> Result<Self, UnknownId> {
        Self::ALL
            .get(index)
            .copied()
            .ok_or(UnknownId("party character index", index as u32))
    }
    pub const fn index(self) -> usize {
        self as usize - 1
    }
}
impl WorldId {
    pub const fn index(self) -> usize {
        self as usize
    }
}
impl MountId {
    pub const fn long_range(self) -> bool {
        !matches!(self, Self::Foot)
    }
    pub const fn airborne(self) -> bool {
        matches!(self, Self::Rheairds)
    }
}

impl Default for WorldInteraction {
    fn default() -> Self {
        Self::Active
    }
}
impl WorldInteraction {
    pub fn is_active(&self) -> bool {
        *self == Self::Active
    }
}

#[cfg(test)]
mod tests;
