//! A job's logical binding and source dependencies identify its shared output.
use super::*;
use std::sync::OnceLock;

pub(crate) struct Identities<'a> {
    extracted: &'a Path,
    hashes: &'a BTreeMap<String, String>,
    rel: Rel,
    executable: String,
    toon: String,
    resources: crate::resource::Catalogue,
    owners: Vec<u16>,
    audio: OnceLock<std::result::Result<String, String>>,
    sources: Sources,
}

impl<'a> Identities<'a> {
    pub(crate) fn read(extracted: &'a Path, hashes: &'a BTreeMap<String, String>) -> Result<Self> {
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        Ok(Self {
            extracted,
            hashes,
            rel: Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?,
            executable: crate::digest(&executable),
            toon: crate::all_assets::roles::toon_path(extracted, &executable)?,
            resources: crate::resource::read(&executable)?,
            owners: crate::session::equipment_owners(
                &executable,
                &crate::item::read(&executable)?,
            )?,
            audio: OnceLock::new(),
            sources: Sources::read(extracted)?,
        })
    }

    pub(crate) fn sources(&self) -> &Sources {
        &self.sources
    }

    pub(crate) fn identity(&self, job: &Job) -> Result<String> {
        let mut inputs = if matches!(job, Job::Geometry { .. } | Job::UnavailablePartyBody { .. }) {
            Vec::new()
        } else {
            vec![self.file("US_r_Top2Btl.rel")?, self.executable.clone()]
        };
        match job {
            Job::Shared => {
                inputs.push(self.file(&self.sources.usual)?);
                inputs.push(self.file(&self.toon)?);
                for kind in PowWeapon::ALL {
                    inputs.push(self.archive(Archive::Magic, kind.native() - 200)?);
                }
            }
            Job::Archive { kind, id, range } => {
                inputs.push(self.member(self.sources.archive(*kind), range.clone())?);
                match kind {
                    Archive::Weapon => {
                        inputs.extend(self.weapon(weapon_item(*id))?);
                    }
                    Archive::Arena | Archive::Magic | Archive::Skill => {}
                }
            }
            Job::Enemy { range, .. } => {
                let bytes = read_range(
                    &self.extracted.join("files").join(&self.sources.enemy),
                    range.clone(),
                )?;
                inputs.push(crate::digest(&bytes));
                inputs.push(self.file(&self.sources.usual)?);
                if word(&compression::decode(&bytes)?, 0x1e4)? != 0 {
                    // Missing-object classification scans every physical sound pool,
                    // including the other enemy packages, before declaring absence.
                    inputs.push(self.audio()?);
                }
            }
            Job::Visual { asset, .. } => match *asset {
                Asset::Party { character, costume } => {
                    inputs.push(self.party(PartyResource::Body, character, costume)?);
                    inputs.push(self.party(PartyResource::BattleMotion, character, costume)?);
                    inputs
                        .push(self.file(&crate::all_assets::roles::victory_path(self.extracted)?)?);
                }
                Asset::WeaponMotions { item, costume } => {
                    inputs.push(self.file(self.sources.archive(Archive::Weapon))?);
                    inputs.push(self.owner(item).to_string());
                    inputs.push(self.party(PartyResource::BattleMotion, 3, costume)?);
                }
                _ => anyhow::bail!("unsupported standalone battle visual job {asset:?}"),
            },
            Job::Geometry { path } => {
                inputs.push(
                    self.file(
                        path.strip_prefix("files")?
                            .to_str()
                            .context("non-UTF8 battle resource")?,
                    )?,
                );
            }
            Job::UnavailablePartyBody { .. } => {}
        }
        key(job, &inputs)
    }

    fn file(&self, source: &str) -> Result<String> {
        self.hashes
            .get(source)
            .cloned()
            .with_context(|| format!("missing battle dependency hash {source}"))
    }

    fn member(&self, source: &str, range: Range<usize>) -> Result<String> {
        Ok(crate::digest(&read_range(
            &self.extracted.join("files").join(source),
            range,
        )?))
    }

    fn archive(&self, kind: Archive, id: u16) -> Result<String> {
        let source = self.sources.archive(kind);
        let ranges = super::super::archive_directories::read(
            &self.rel,
            super::super::embedded::Layout::RETAIL.archives,
            kind,
            source,
            self.extracted.join("files").join(source).metadata()?.len(),
        )?
        .into_ranges()?;
        let range = ranges
            .into_iter()
            .find_map(|(index, range)| (index == id).then_some(range))
            .with_context(|| format!("missing battle archive dependency {source}/{id}"))?;
        self.member(source, range)
    }

    fn party(&self, kind: PartyResource, character: u8, costume: Costume) -> Result<String> {
        let declared = self.resources.party(kind, character, costume as u8)?;
        let actual = crate::field_resources::find_path(&self.extracted.join("files"), declared)?
            .with_context(|| format!("missing battle resource {declared}"))?;
        self.file(&actual)
    }

    fn owner(&self, item: u16) -> u16 {
        self.owners.get(usize::from(item)).copied().unwrap_or(0)
    }

    fn audio(&self) -> Result<String> {
        self.audio
            .get_or_init(|| {
                (|| -> Result<String> {
                    let banks = super::super::audio::all::bank_sources(
                        self.extracted,
                        &fs::read(self.extracted.join("sys/main.dol"))?,
                    )?
                    .into_iter()
                    .map(|path| Ok((path.clone(), self.file(&path)?)))
                    .collect::<Result<Vec<_>>>()?;
                    Ok(crate::digest(&serde_json::to_vec(&(
                        self.file(&self.sources.enemy)?,
                        self.file(&crate::all_assets::voice_bank_path(self.extracted)?)?,
                        banks,
                    ))?))
                })()
                .map_err(|error| format!("{error:#}"))
            })
            .as_ref()
            .cloned()
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    fn weapon(&self, item: u16) -> Result<Vec<String>> {
        let owner = self.owner(item);
        let mut inputs = vec![self.file(&self.sources.usual)?, owner.to_string()];
        if owner & (1 << 2) != 0 {
            inputs.push(self.party(PartyResource::BattleMotion, 3, Costume::Standard)?);
        }
        Ok(inputs)
    }
}

fn key(job: &Job, inputs: &[String]) -> Result<String> {
    Ok(crate::digest(&serde_json::to_vec(&(
        "battle", job, inputs,
    ))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_identity_ignores_archive_placement_but_keeps_dependencies_and_binding() -> Result<()>
    {
        let first = Job::Archive {
            kind: Archive::Magic,
            id: 1,
            range: 0..10,
        };
        let relocated = Job::Archive {
            kind: Archive::Magic,
            id: 1,
            range: 20..30,
        };
        let other = Job::Archive {
            kind: Archive::Magic,
            id: 2,
            range: 0..10,
        };
        let inputs = [crate::digest(b"member"), crate::digest(b"metadata")];
        assert_eq!(key(&first, &inputs)?, key(&relocated, &inputs)?);
        assert_ne!(key(&first, &inputs)?, key(&other, &inputs)?);
        assert_ne!(
            key(&first, &inputs)?,
            key(&first, &[inputs[0].clone(), crate::digest(b"changed")])?
        );
        let absent = Job::UnavailablePartyBody {
            character: 1,
            costume: Costume::Standard,
            source: "missing.bin".into(),
        };
        let present = Job::Visual {
            asset: Asset::Party {
                character: 1,
                costume: Costume::Standard,
            },
            name: "party-1-costume-0".into(),
        };
        assert_ne!(key(&absent, &[])?, key(&present, &[])?);
        Ok(())
    }

    #[test]
    fn party_identity_tracks_motion_and_absence_without_hashing_unrelated_files() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("battle-job-identity"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files"))?;
            fs::write(root.join("files/body.bin"), b"body")?;
            fs::write(root.join("files/motion.bin"), b"motion")?;
            fs::write(root.join("files/victory.bin"), b"victory")?;
            let declaration = b"./victory.bin\0";
            let size = 0x1e4 + declaration.len();
            let mut module = vec![0; 0x100 + size];
            for (at, value) in [(12, 5u32), (16, 0x4c), (0x6c, 0x100), (0x70, size as u32)] {
                module[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            module[0x100 + 0x1e4..].copy_from_slice(declaration);
            fs::write(root.join("files/US_r_Top2Btl.rel"), &module)?;
            let mut hashes = BTreeMap::from([
                ("body.bin".into(), crate::digest(b"body")),
                ("motion.bin".into(), crate::digest(b"motion")),
                ("victory.bin".into(), crate::digest(b"victory")),
                ("US_r_Top2Btl.rel".into(), crate::digest(&module)),
            ]);
            let job = Job::Visual {
                asset: Asset::Party {
                    character: 1,
                    costume: Costume::Standard,
                },
                name: "party-1-costume-0".into(),
            };
            let identity = |hashes: &BTreeMap<String, String>| {
                Identities {
                    extracted: &root,
                    hashes,
                    rel: Rel {
                        bytes: vec![],
                        sections: vec![],
                        pointers: BTreeMap::new(),
                        local_targets: BTreeSet::new(),
                    },
                    executable: crate::digest(b"resource and ownership tables"),
                    toon: String::new(),
                    resources: crate::resource::Catalogue {
                        standalone: vec![],
                        groups: vec![],
                        party_bodies: vec![std::array::from_fn(|_| Some("body.bin".into()))],
                        party_battle_motions: vec![std::array::from_fn(|_| {
                            Some("motion.bin".into())
                        })],
                        party_field_motions: vec![],
                        field_services: vec![],
                    },
                    owners: vec![],
                    audio: OnceLock::new(),
                    sources: Sources {
                        usual: String::new(),
                        enemy: String::new(),
                        archives: std::array::from_fn(|_| String::new()),
                    },
                }
                .identity(&job)
            };
            let original = identity(&hashes)?;
            hashes.insert(
                "unrelated.map".into(),
                crate::digest(b"different disc content"),
            );
            assert_eq!(original, identity(&hashes)?);
            hashes.insert("victory.bin".into(), crate::digest(b"different victory"));
            assert_ne!(original, identity(&hashes)?);
            let original = identity(&hashes)?;
            hashes.insert("motion.bin".into(), crate::digest(b"different motion"));
            assert_ne!(original, identity(&hashes)?);
            fs::remove_file(root.join("files/body.bin"))?;
            assert!(identity(&hashes).is_err());
            Ok(())
        })();
        if root.exists() {
            fs::remove_dir_all(root)?;
        }
        result
    }
}
