//! Physical AFS member order binds numeric voice IDs to shared cooked streams.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceArchive {
    pub version: u32,
    pub source_sha256: String,
    /// The member's position is its original archive ID, including empty streams.
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub metadata: String,
    pub kind: MemberKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    Stream,
    /// A recognized source stream with an authored frame count of zero.
    Empty,
}

impl VoiceArchive {
    pub const VERSION: u32 = 2;

    pub fn path(source_sha256: &str) -> Result<String> {
        ensure!(
            source_sha256.len() == 64 && source_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid voice archive digest"
        );
        Ok(format!("audio/archives/{source_sha256}/archive.json"))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == Self::VERSION && self.members.len() <= 65536,
            "invalid voice archive version or member count"
        );
        Self::path(&self.source_sha256)?;
        for (id, member) in self.members.iter().enumerate() {
            ensure!(
                !member.name.is_empty()
                    && member.name.len() <= 32
                    && !member.name.contains(['/', '\\'])
                    && !member.name.chars().any(char::is_control),
                "invalid voice archive member name"
            );
            crate::validate_asset_path(&member.metadata)?;
            ensure!(
                member.metadata == format!("audio/archives/{}/{id}.json", self.source_sha256),
                "voice archive member references another source"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_preserves_empty_slots_and_rejects_unsafe_references() {
        let mut archive = VoiceArchive {
            version: VoiceArchive::VERSION,
            source_sha256: "a".repeat(64),
            members: [MemberKind::Stream, MemberKind::Empty, MemberKind::Stream]
                .into_iter()
                .enumerate()
                .map(|(id, kind)| Member {
                    name: format!("{id}.adx"),
                    metadata: format!("audio/archives/{}/{id}.json", "a".repeat(64)),
                    kind,
                })
                .collect(),
        };
        archive.validate().unwrap();
        let restored: VoiceArchive =
            serde_json::from_slice(&serde_json::to_vec(&archive).unwrap()).unwrap();
        assert_eq!(restored.members[1].kind, MemberKind::Empty);
        assert_eq!(restored.members[2].name, "2.adx");
        assert_eq!(
            VoiceArchive::path(&archive.source_sha256).unwrap(),
            format!("audio/archives/{}/archive.json", "a".repeat(64))
        );
        archive.members[1].metadata = "../elsewhere.json".into();
        assert!(archive.validate().is_err());
        archive.members.clear();
        archive.validate().unwrap();
        archive.source_sha256 = "../unsafe".into();
        assert!(archive.validate().is_err());
    }
}
