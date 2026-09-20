//! Direct physical battle asset cooking, independent of gameplay admission.
mod identities;
mod records;
mod sources;
pub(crate) use super::archive_directories::Archive;
use super::{actions::Rel, effect_program, effects, visual};
use crate::{
    all_assets::{Failure, geometry},
    compression,
    field::{MapArchive, sections},
    read::{u16 as half, u32 as word},
    resource::PartyResource,
};
use anyhow::{Context, Result, ensure};
pub(crate) use identities::Identities;
pub(crate) use records::attachment_recipe;
pub(crate) use records::cook_party_settings;
pub(crate) use records::{
    ActorSettings, CombatStats, EnemyResources, EnemyStatistics, EnemyVariant, EnemyVariants,
    NativeResources, PartySettings,
};
use resonance_content::{
    battle::{
        effect_inventory::EffectSourceBank as Bank, effect_program::ModelRef, unison::PowWeapon,
    },
    menu_data::Costume,
};
use serde::Serialize;
pub(super) use sources::SourceLayout;
pub(crate) use sources::Sources;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    ops::Range,
    path::{Path, PathBuf},
};
use visual::all::Asset;

pub(super) const SKILL_ARCHIVE_ROWS: usize = 4;

/// Small physical work units for the shared cook worker pool.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "job", rename_all = "snake_case")]
pub(crate) enum Job {
    Shared,
    Archive {
        kind: Archive,
        id: u16,
        #[serde(skip)]
        range: Range<usize>,
    },
    Enemy {
        id: u16,
        #[serde(skip)]
        range: Range<usize>,
    },
    Visual {
        asset: Asset,
        name: String,
    },
    Geometry {
        path: std::path::PathBuf,
    },
    UnavailablePartyBody {
        character: u8,
        costume: Costume,
        source: String,
    },
}

pub(crate) struct Discovery {
    pub jobs: Vec<Job>,
    /// Exclusive physical readers, including empty archives and failed discoveries.
    /// Party geometry remains available to the ordinary asset reader too.
    pub owned_sources: BTreeSet<String>,
}

pub(crate) fn jobs(
    extracted: &Path,
    sources: &Sources,
    failed: &mut impl FnMut(Failure),
) -> Discovery {
    let files = extracted.join("files");
    let usual = files.join(&sources.usual);
    let enemy = files.join(&sources.enemy);
    let archives = Archive::ALL.map(|kind| (kind, files.join(sources.archive(kind))));
    let mut discovery = Discovery {
        jobs: Vec::new(),
        owned_sources: sources.owned_paths(),
    };
    let result = (|| {
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let rel = Rel::read(&files.join("US_r_Top2Btl.rel"))?;
        discovery.jobs.push(Job::Shared);
        for (kind, path) in archives {
            let result = (|| {
                super::archive_directories::read(
                    &rel,
                    super::embedded::Layout::RETAIL.archives,
                    kind,
                    sources.archive(kind),
                    path.metadata()?.len(),
                )?
                .into_ranges()
            })();
            if let Some(ranges) = discovered(failed, &path.display().to_string(), result) {
                discovery
                    .jobs
                    .extend(
                        ranges
                            .into_iter()
                            .map(|(id, range)| Job::Archive { kind, id, range }),
                    );
            }
        }
        let result = (|| {
            let usual = fs::read(&usual)?;
            let directory = super::actions::member(&usual, 10)?;
            let offsets = directory
                .chunks_exact(4)
                .map(|v| word(v, 0))
                .collect::<Result<Vec<_>>>()?;
            physical_ranges(&offsets, 0, enemy.metadata()?.len())
        })();
        if let Some(ranges) = discovered(failed, &enemy.display().to_string(), result) {
            discovery.jobs.extend(
                ranges
                    .into_iter()
                    .map(|(id, range)| Job::Enemy { id, range }),
            );
        }
        party_jobs(extracted, &executable, &mut discovery.jobs, failed)
    })();
    discovered(failed, "battle archives", result);
    discovery
}

fn discovered<T>(failed: &mut impl FnMut(Failure), path: &str, result: Result<T>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            failed(Failure {
                path: path.into(),
                error: format!("{error:#}"),
            });
            None
        }
    }
}

/// One reusable backend set per global worker, with no resident decoded roster.
pub(crate) struct Worker<'a> {
    extracted: &'a Path,
    output: PathBuf,
    failures: Vec<Failure>,
    visual: Option<visual::all::Cooker<'a>>,
    effects: Option<effect_program::all::Cooker>,
    projectiles: Option<effects::all::Cooker>,
    audio: std::sync::Arc<super::audio::all::Pools>,
    sources: Sources,
}

impl<'a> Worker<'a> {
    pub(crate) fn new(
        extracted: &'a Path,
        output: &Path,
        audio: std::sync::Arc<super::audio::all::Pools>,
    ) -> Result<Self> {
        let mut worker = Self {
            extracted,
            output: output.into(),
            failures: Vec::new(),
            visual: None,
            effects: None,
            projectiles: None,
            audio,
            sources: Sources::read(extracted)?,
        };
        worker.visual = worker.attempt(
            "battle visual decoder",
            visual::all::Cooker::new(extracted, output),
        );
        worker.effects = worker.attempt(
            "battle effect decoder",
            effect_program::all::Cooker::read(extracted),
        );
        worker.projectiles = worker.attempt(
            "battle projectile decoder",
            effects::all::Cooker::read(extracted),
        );
        Ok(worker)
    }

    pub(crate) fn cook_into(&mut self, job: &Job, output: &Path) -> Vec<Failure> {
        self.output = output.into();
        if let Some(visual) = &mut self.visual {
            visual.set_output(output);
        }
        self.cook(job)
    }

    pub(crate) fn cook(&mut self, job: &Job) -> Vec<Failure> {
        let (name, result) = match job {
            Job::Shared => ("shared battle resources".into(), self.shared()),
            Job::Archive { kind, id, range } => {
                let name = format!("files/BTL/{}/{id}", kind.file());
                let result = (|| {
                    let bytes = read_range(
                        &self
                            .extracted
                            .join("files")
                            .join(self.sources.archive(*kind)),
                        range.clone(),
                    )?;
                    match kind {
                        Archive::Magic | Archive::Skill => self.native(*kind, *id, &bytes),
                        Archive::Arena => self.arena(*id, &bytes),
                        Archive::Weapon => {
                            let item = weapon_item(*id);
                            self.visual(Asset::Weapon(item), &format!("weapon-{item}"));
                            self.geometry(&bytes, &format!("battle/weapon/{id}"));
                            Ok(())
                        }
                    }
                })();
                (name, result)
            }
            Job::Enemy { id, range } => (format!("enemy-{id}"), self.enemy(*id, range.clone())),
            Job::Visual { asset, name } => {
                self.visual(*asset, name);
                (name.clone(), Ok(()))
            }
            Job::Geometry { path } => {
                let name = path.to_string_lossy().into_owned();
                let result = (|| {
                    let bytes = fs::read(self.extracted.join(path))?;
                    self.geometry(&bytes, &name);
                    Ok(())
                })();
                (name, result)
            }
            Job::UnavailablePartyBody {
                character,
                costume,
                source,
            } => {
                let name = format!("party-{character}-costume-{}/body", *costume as u8);
                self.record(&name, &serde_json::json!({
                    "character":character,"costume":*costume as u8,"source":source,"status":"not_shipped"
                }));
                (name, Ok(()))
            }
        };
        self.attempt(&name, result);
        std::mem::take(&mut self.failures)
    }

    fn shared(&mut self) -> Result<()> {
        self.visual(Asset::ToonRamp, "toon-ramp");
        self.visual(Asset::Shadow, "shadow");
        for kind in PowWeapon::ALL {
            self.visual(
                Asset::PowWeapon(kind),
                &format!("pow-weapon-{}", kind.native()),
            );
        }
        let usual = fs::read(self.extracted.join("files").join(&self.sources.usual))?;
        self.usual(&usual)
    }

    fn attempt<T>(&mut self, path: &str, result: Result<T>) -> Option<T> {
        discovered(&mut |failure| self.failures.push(failure), path, result)
    }

    fn record(&mut self, name: &str, value: &impl Serialize) {
        let path = self
            .output
            .join("battle/all")
            .join(name)
            .with_extension("json");
        let result = (|| {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = BufWriter::new(File::create(&path)?);
            serde_json::to_writer(&mut file, value)?;
            file.flush()?;
            Ok(())
        })();
        self.attempt(&path.display().to_string(), result);
    }

    fn visual(&mut self, asset: Asset, name: &str) {
        let Some(cooker) = &mut self.visual else {
            return;
        };
        let result = cooker.cook(asset);
        if let Some(cooked) = self.attempt(name, result) {
            self.record(&format!("visuals/{name}"), &cooked);
        }
    }

    fn geometry(&mut self, bytes: &[u8], name: &str) {
        self.geometry_with_palette(bytes, None, name);
    }

    fn geometry_with_palette(&mut self, bytes: &[u8], primary: Option<&[u8]>, name: &str) {
        let failures = &mut self.failures;
        let mut report = |path: &str, result: Result<()>| {
            if let Err(error) = result {
                failures.push(Failure {
                    path: path.into(),
                    error: format!("{error:#}"),
                });
            }
        };
        let recognized = if let Some(primary) = primary {
            geometry::cook_with_palette(bytes, primary, name, &self.output, &mut report)
        } else {
            geometry::cook(
                bytes,
                name,
                &self.output,
                None,
                geometry::Input::File,
                &mut report,
            )
        };
        if !recognized {
            report(
                name,
                Err(anyhow::anyhow!(
                    "unrecognized physical member ({} bytes)",
                    bytes.len()
                )),
            );
        }
    }

    fn bank(&mut self, bank: Bank, name: &str, particles: bool, projectiles: bool) {
        if particles && let Some(cooker) = &mut self.effects {
            let cooked = cooker.cook_bank(bank);
            if let Some(failure) = &cooked.failure {
                self.failures.push(Failure {
                    path: format!("{name}/effects"),
                    error: failure.error.clone(),
                });
            }
            for root in &cooked.roots {
                if let effect_program::all::RootStatus::Unsupported { failure } = &root.status {
                    self.failures.push(Failure {
                        path: format!("{name}/{:?}", root.root),
                        error: failure.error.clone(),
                    });
                }
            }
            // Authored programs and controllers are independent of runtime bindings.
            self.record(&format!("effects/{name}"), &cooked);
        }
        if projectiles && let Some(cooker) = &mut self.projectiles {
            let cooked = cooker.cook_bank(bank);
            if let Some(error) = &cooked.failure {
                self.failures.push(Failure {
                    path: format!("{name}/projectiles"),
                    error: error.clone(),
                });
            }
            for root in &cooked.roots {
                if let effects::all::ProjectileStatus::Unsupported { error } = &root.status {
                    self.failures.push(Failure {
                        path: format!("{name}/projectile-{}", root.id),
                        error: error.clone(),
                    });
                }
            }
            self.record(&format!("projectiles/{name}"), &cooked);
        }
    }

    fn usual(&mut self, bytes: &[u8]) -> Result<()> {
        let ranges = sections(bytes)?;
        self.bank(
            Bank::Common,
            "common",
            ranges.get(2).is_some_and(Option::is_some),
            false,
        );
        self.bank(
            Bank::Techniques,
            "techniques",
            ranges.get(3).is_some_and(Option::is_some),
            ranges.get(7).is_some_and(Option::is_some),
        );
        for (id, range) in ranges.iter().enumerate() {
            let Some(range) = range else { continue };
            // Effect and projectile records were decoded by their bank parsers.
            if matches!(id, 2 | 3 | 7) {
                continue;
            }
            let member = &bytes[range.clone()];
            if id == 9 {
                for (index, range) in sections(member)?.into_iter().enumerate() {
                    let Some(range) = range else { continue };
                    let source = &member[range];
                    let name = format!("usual/9/{index}");
                    if word(source, 12).ok() == Some(28) {
                        let result = super::action_program::compact_bundle(source);
                        if let Some(value) = self.attempt(&name, result) {
                            self.record(&name, &value);
                        }
                    } else {
                        self.geometry(source, &format!("battle/{name}"));
                    }
                }
                continue;
            }
            if matches!(id, 0 | 1 | 5 | 10..=13) {
                let result = records::usual_table(member, id);
                if let Some(value) = self.attempt(&format!("usual/{id}"), result) {
                    self.record(&format!("usual/{id}"), &value);
                }
                continue;
            }
            self.geometry(&bytes[range.clone()], &format!("battle/usual/{id}"));
            if id == 6 {
                let result = sections(&bytes[range.clone()]);
                if let Some(models) = self.attempt("usual model table", result) {
                    for (index, range) in models.iter().enumerate() {
                        if range.is_some() {
                            let index = u8::try_from(index + 1)?;
                            self.visual(
                                Asset::EffectModel(ModelRef::Common { index }),
                                &format!("common-model-{index}"),
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn native(&mut self, kind: Archive, id: u16, bytes: &[u8]) -> Result<()> {
        let (bank, name) = match kind {
            Archive::Magic => (Bank::Magic { package: id }, format!("magic-{id}")),
            Archive::Skill => (Bank::Skill { package: id }, format!("skill-{id}")),
            _ => unreachable!(),
        };
        let resources = NativeResources::read(bytes)?;
        self.record(&format!("{name}/resources"), &resources);
        self.bank(
            bank,
            &name,
            resources.effects != 0,
            resources.projectiles != 0,
        );
        for (index, offset) in resources.models.iter().enumerate() {
            if *offset != 0 {
                let index = index as u8;
                let model = match kind {
                    Archive::Magic => ModelRef::Magic { package: id, index },
                    Archive::Skill => ModelRef::Skill { package: id, index },
                    _ => unreachable!(),
                };
                self.visual(Asset::EffectModel(model), &format!("{name}-model-{index}"));
            }
        }
        // Fixed source header: EF1, TPL, ten models/outlines, forty clips,
        // projectiles and native callback resources. Decode every physical
        // member independently, including unused texture/animation members.
        self.pointer_geometry(
            bytes,
            &(4..NativeResources::BYTES).step_by(4).collect::<Vec<_>>(),
            NativeResources::BYTES,
            &name,
        );
        Ok(())
    }

    fn arena(&mut self, id: u16, bytes: &[u8]) -> Result<()> {
        self.visual(Asset::Arena(id), &format!("arena-{id}"));
        let decoded = MapArchive::decode(bytes)?;
        self.bank(
            Bank::Arena { arena: id },
            &format!("arena-{id}"),
            decoded.sections.get(9).is_some_and(Option::is_some),
            false,
        );
        for (section, range) in decoded.sections.iter().enumerate() {
            if section == 0 {
                let result = records::arena_metadata(decoded.section(0)?, decoded.sections.len());
                if let Some(metadata) = self.attempt(&format!("arena-{id}/settings"), result) {
                    self.record(&format!("arenas/{id}/settings"), &metadata);
                }
                continue;
            }
            if section != 9
                && let Some(range) = range
            {
                self.geometry(
                    &decoded.bytes[range.clone()],
                    &format!("battle/arena/{id}/{section}"),
                );
            }
        }
        Ok(())
    }

    fn enemy(&mut self, id: u16, range: Range<usize>) -> Result<()> {
        let path = self.extracted.join("files").join(&self.sources.enemy);
        let name = format!("enemy-{id}");
        let bytes = compression::decode(&read_range(&path, range)?)?;
        ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
        for (field, result) in records::enemy_sections(&bytes) {
            let label = format!("{name}/header-{field:x}");
            if let Some(record) = self.attempt(&label, result) {
                self.record(&label, &record);
                self.attempt(&label, records::unresolved_enemy_fields(&bytes, field));
            }
        }
        let audio = super::audio::all::cook_embedded_bank(
            self.extracted,
            &self.output,
            &bytes,
            &self.audio,
        );
        if let Some(members) = self.attempt(&format!("{name}/audio"), audio) {
            for (member, result) in members {
                self.attempt(&format!("{name}/audio/{member}"), result);
            }
        }
        self.pointer_geometry(
            &bytes,
            &(0x18..0x1e8).step_by(4).collect::<Vec<_>>(),
            0x1e8,
            &name,
        );
        self.bank(
            Bank::Enemy { monster: id },
            &name,
            word(&bytes, 0x1cc)? != 0,
            word(&bytes, 0x1c8)? != 0,
        );
        let monster = u8::try_from(id)?;
        self.visual(Asset::Enemy(monster), &name);
        let metadata = usize::from(half(&bytes, 4)?);
        let appearances = *bytes
            .get(metadata + 0xef)
            .context("missing enemy appearance count")?;
        for row in 0..appearances {
            self.visual(
                Asset::EnemyAppearance { monster, row },
                &format!("{name}-appearance-{row}"),
            );
        }
        let count = *bytes
            .get(metadata + 0x1e8)
            .context("missing enemy model count")?;
        ensure!(count <= 6, "enemy effect model count exceeds source slots");
        for index in 0..6_u8 {
            if word(&bytes, 0x180 + usize::from(index) * 4)? == 0 {
                continue;
            }
            self.visual(
                Asset::EffectModel(ModelRef::Enemy { monster, index }),
                &format!("{name}-model-{index}"),
            );
            for animation_model in 0..count {
                if word(&bytes, 0x1b0 + usize::from(animation_model) * 4)? != 0 {
                    self.visual(
                        Asset::EffectModel(ModelRef::EnemyAnimated {
                            monster,
                            index,
                            animation_model,
                        }),
                        &format!("{name}-model-{index}-animation-{animation_model}"),
                    );
                }
            }
        }
        Ok(())
    }

    fn pointer_geometry(&mut self, bytes: &[u8], fields: &[usize], header: usize, name: &str) {
        let mut starts = BTreeMap::new();
        for &field in fields {
            let label = format!("{name}/header-{field:x}");
            let result = word(bytes, field).and_then(|v| {
                let start = v as usize;
                ensure!(
                    start == 0 || (header..bytes.len()).contains(&start),
                    "member exceeds source package"
                );
                Ok(start)
            });
            if let Some(start) = self.attempt(&label, result)
                && start != 0
            {
                starts.entry(start).or_insert(field);
            }
        }
        for (&start, &field) in &starts {
            // Both package headers identify effect banks and projectile tables.
            if (header == 276 && matches!(field, 4 | 252))
                || (header == 0x1e8 && matches!(field, 0x1c8 | 0x1cc | 0x1e4))
            {
                continue;
            }
            let end = starts
                .range(start + 1..)
                .next()
                .map_or(bytes.len(), |(&end, _)| end);
            let animation = (header == 276 && (92..252).contains(&field))
                || (header == 0x1e8
                    && ((0x20..0x160).contains(&field) || (0x1b0..0x1c8).contains(&field)));
            if animation {
                let label = format!("{name}/member-{field:x}");
                let result = crate::animation::unbound_indexed(&bytes[start..end]);
                if let Some(value) = self.attempt(&label, result) {
                    self.record(&label, &value);
                }
                continue;
            }
            if header == 0x1e8 && field == 0x1d8 {
                let result = (|| {
                    let metadata = usize::from(half(bytes, 4)?);
                    let count = usize::from(
                        *bytes
                            .get(metadata + 0x1e6)
                            .context("missing enemy auxiliary instance count")?,
                    );
                    records::enemy_auxiliary(&bytes[start..end], count)
                })();
                if let Some(value) = self.attempt(&format!("{name}/auxiliary-models"), result) {
                    self.record(&format!("{name}/auxiliary-models"), &value);
                }
                continue;
            }
            if header == 0x1e8 && field == 0x1dc {
                let result = super::projectile_modifiers::authored(bytes, start..end);
                if let Some(value) = self.attempt(&format!("{name}/projectile-modifiers"), result) {
                    self.record(&format!("{name}/projectile-modifiers"), &value);
                }
                continue;
            }
            if header == 0x1e8 && field == 0x1e0 {
                let result = (|| {
                    let metadata = usize::from(half(bytes, 4)?);
                    let count = usize::from(
                        *bytes
                            .get(metadata + 0x1e7)
                            .context("missing enemy variant count")?,
                    );
                    records::enemy_variants(&bytes[start..end], count)
                })();
                if let Some(value) = self.attempt(&format!("{name}/variants"), result) {
                    self.record(&format!("{name}/variants"), &value);
                }
                continue;
            }
            let primary_field = if header == 276 && (52..=88).contains(&field) {
                Some(field - 40)
            } else if header == 0x1e8 && field == 0x1c {
                Some(0x18)
            } else if header == 0x1e8 && (0x198..0x1b0).contains(&field) {
                Some(field - 24)
            } else {
                None
            };
            let primary = primary_field
                .and_then(|field| word(bytes, field).ok())
                .and_then(|at| {
                    let start = at as usize;
                    if !starts.contains_key(&start) {
                        return None;
                    }
                    let end = starts
                        .range(start + 1..)
                        .next()
                        .map_or(bytes.len(), |(&end, _)| end);
                    bytes.get(start..end)
                });
            self.geometry_with_palette(
                &bytes[start..end],
                primary,
                &format!("battle/{name}/member-{field:x}"),
            );
        }
    }
}

fn weapon_item(slot: u16) -> u16 {
    slot + match slot {
        0..=138 => 135,
        139..=149 => 217,
        _ => 379,
    }
}

const COSTUMES: [Costume; 5] = [
    Costume::Standard,
    Costume::Variant1,
    Costume::Variant2,
    Costume::Story,
    Costume::Variant4,
];

fn party_jobs(
    extracted: &Path,
    executable: &[u8],
    jobs: &mut Vec<Job>,
    failed: &mut impl FnMut(Failure),
) -> Result<()> {
    let files = extracted.join("files");
    let resources = crate::resource::read(executable)?;
    let mut physical = BTreeSet::new();
    for character in 1..=9_u8 {
        for costume in COSTUMES {
            let mut shipped_body = true;
            for kind in [PartyResource::Body, PartyResource::BattleMotion] {
                let label = format!("party-{character}-costume-{}/{kind:?}", costume as u8);
                let result = (|| {
                    let name = resources.party(kind, character, costume as u8)?;
                    let actual = crate::field_resources::find_path(&files, name)?;
                    if matches!(kind, PartyResource::Body) && actual.is_none() {
                        shipped_body = false;
                        jobs.push(Job::UnavailablePartyBody {
                            character,
                            costume,
                            source: name.into(),
                        });
                        return Ok(());
                    }
                    let actual =
                        actual.with_context(|| format!("missing resource path {name:?}"))?;
                    physical.insert(std::path::PathBuf::from("files").join(actual));
                    Ok(())
                })();
                discovered(failed, &label, result);
            }
            if shipped_body {
                jobs.push(Job::Visual {
                    asset: Asset::Party { character, costume },
                    name: format!("party-{character}-costume-{}", costume as u8),
                });
            }
        }
    }
    let items = crate::item::read(executable)?;
    let owners = crate::session::equipment_owners(executable, &items)?;
    for (item, row) in items.iter().enumerate() {
        if !(13..=22).contains(&row.category) || owners[item] & (1 << 2) == 0 {
            continue;
        }
        for costume in COSTUMES {
            jobs.push(Job::Visual {
                asset: Asset::WeaponMotions {
                    item: item as u16,
                    costume,
                },
                name: format!("weapon-{item}-costume-{}", costume as u8),
            });
        }
    }
    if let Some(entries) = discovered(
        failed,
        "files/BTL",
        fs::read_dir(files.join("BTL")).map_err(Into::into),
    ) {
        for entry in entries {
            let Some(entry) = discovered(failed, "files/BTL", entry.map_err(Into::into)) else {
                continue;
            };
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cab"))
            {
                physical.insert(path.strip_prefix(extracted)?.to_owned());
            }
        }
    }
    jobs.extend(physical.into_iter().map(|path| Job::Geometry { path }));
    Ok(())
}

/// Skip null, end and alias table entries; each physical byte range is cooked once.
pub(crate) fn physical_ranges(
    offsets: &[u32],
    first: usize,
    length: u64,
) -> Result<Vec<(u16, Range<usize>)>> {
    let eof = offsets
        .iter()
        .position(|&v| u64::from(v) == length)
        .context("source archive has no EOF table entry")?;
    ensure!(eof >= first, "source archive ends before its first package");
    ensure!(
        offsets[eof + 1..].iter().all(|&v| v == 0),
        "nonempty source slot after archive EOF"
    );
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for id in first..eof {
        let start = offsets[id];
        if (start == 0 && id != first) || !seen.insert(start) {
            continue;
        }
        let end = offsets[id + 1..=eof]
            .iter()
            .copied()
            .find(|&v| v > start)
            .context("source package has no range end")?;
        ensure!(u64::from(end) <= length, "source package exceeds archive");
        result.push((id.try_into()?, start as usize..end as usize));
    }
    let mut ranges = result.iter().map(|(_, range)| range).collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    let mut cursor = 0;
    for range in ranges {
        ensure!(
            range.start == cursor,
            "archive coverage gap or overlap: expected offset {cursor:#x}, found {:#x}",
            range.start
        );
        cursor = range.end;
    }
    ensure!(cursor as u64 == length, "archive coverage stops before EOF");
    Ok(result)
}

pub(super) fn read_range(path: &Path, range: Range<usize>) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    ensure!(
        range.start < range.end && range.end as u64 <= file.metadata()?.len(),
        "invalid source package range"
    );
    file.seek(SeekFrom::Start(range.start as u64))?;
    let mut bytes = vec![0; range.len()];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_keeps_empty_and_failed_archive_ownership_separate_from_party_dependencies()
    -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("battle-discovery"));
        let result = (|| -> Result<()> {
            fs::create_dir_all(root.join("files/BTL"))?;
            fs::create_dir_all(root.join("sys"))?;
            fs::write(root.join("files/BTL/BTLbg.dat"), [])?;
            fs::write(root.join("sys/main.dol"), [])?;
            fs::write(root.join("files/US_r_Top2Btl.rel"), [])?;
            let mut failures = Vec::new();
            let sources = Sources {
                usual: "BTL/BTLusual.dat".into(),
                enemy: "BTL/BTLenemy.dat".into(),
                archives: Archive::ALL.map(|kind| format!("BTL/{}", kind.file())),
            };
            let mut discovery = jobs(&root, &sources, &mut |failure| failures.push(failure));
            assert_eq!(failures.len(), 1);
            assert!(discovery.jobs.is_empty());
            assert_eq!(discovery.owned_sources.len(), Archive::ALL.len() + 2);
            assert!(discovery.owned_sources.contains("BTL/BTLbg.dat"));
            assert!(discovery.owned_sources.contains("BTL/BTLenemy.dat"));
            assert!(discovery.owned_sources.contains("BTL/BTLusual.dat"));
            let body = Job::Geometry {
                path: "files/renamed-party.bin".into(),
            };
            assert_eq!(sources.source_paths(&body), ["BTL/BTLusual.dat"]);
            discovery.jobs.push(body);
            assert!(!discovery.owned_sources.contains("renamed-party.bin"));
            Ok(())
        })();
        let cleanup = fs::remove_dir_all(root);
        result?;
        cleanup?;
        Ok(())
    }

    #[test]
    fn physical_archives_require_complete_nonoverlapping_coverage() {
        assert_eq!(
            physical_ranges(&[0, 0, 10, 10, 20, 0], 1, 20).unwrap(),
            vec![(1, 0..10), (2, 10..20)]
        );
        assert!(physical_ranges(&[5, 10, 20], 0, 20).is_err());
        assert!(physical_ranges(&[0, 10, 5, 20], 0, 20).is_err());
    }

    #[test]
    #[ignore = "requires original battle REL; no asset conversion"]
    fn original_skill_archive_stops_before_native_dispatch() -> Result<()> {
        for disc in ["disc1", "disc2"] {
            let files = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../local/extracted")
                .join(disc)
                .join("files");
            let rel = Rel::read(&files.join("US_r_Top2Btl.rel"))?;
            let table = rel.at((5, 0x11c4))?;
            let offsets = (0..SKILL_ARCHIVE_ROWS)
                .map(|row| {
                    assert!(rel.pointer(5, 0x11c4 + row * 4).is_err());
                    word(table, row * 4)
                })
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(
                rel.pointer(5, 0x11c4 + SKILL_ARCHIVE_ROWS * 4)?,
                (1, 0x39974)
            );
            assert_eq!(
                physical_ranges(
                    &offsets,
                    1,
                    files.join("BTL/BTLskill.dat").metadata()?.len()
                )?,
                [(1, 0..401408), (2, 401408..477184)]
            );
        }
        Ok(())
    }
}
