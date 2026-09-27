//! Prepared overworld rules. Only data access and validated writes live here;
//! story thresholds, landmark IDs and event decisions live in world/rules.sym.
use super::{
    TileCoordinate, World,
    landmarks::{Appearance, Locations, Progress},
    travel::Mount,
};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{Interaction, Marker};
use resonance_events::PersistentState;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use symphonia_script::{
    Program,
    authored::{NativeDeclaration, ScriptState, Type},
};
use symphonia_script_compiler::SourceResolver;
use symphonia_script_tools::PreparationCache;
use symphonia_script_vm::{Host, Memory, NativeBindings, NativeResult, RunEvent, Vm};

#[repr(u8)]
enum Native {
    Global = 64,
    Flag,
    ItemCount,
    Visited,
    PartyAvailable,
    GuidepostCount,
    GuidepostLocation,
    GuidepostKnown,
    SetInteraction,
    HideMarker,
    SetModel,
    UnmodeledMarker,
    Blocked,
    AlternateTile,
}
impl Native {
    const fn declaration(self) -> NativeDeclaration {
        let (name, parameters, result): (_, &[Type], _) = match self {
            Self::Global => ("game::world::global", &[Type::I32], Some(Type::I32)),
            Self::Flag => ("game::world::flag", &[Type::I32], Some(Type::Bool)),
            Self::ItemCount => ("game::world::item_count", &[Type::I32], Some(Type::I32)),
            Self::Visited => ("game::world::visited", &[Type::I32], Some(Type::Bool)),
            Self::PartyAvailable => (
                "game::world::party_available",
                &[Type::I32],
                Some(Type::Bool),
            ),
            Self::GuidepostCount => ("game::world::guidepost_count", &[], Some(Type::I32)),
            Self::GuidepostLocation => (
                "game::world::guidepost_location",
                &[Type::I32],
                Some(Type::I32),
            ),
            Self::GuidepostKnown => (
                "game::world::guidepost_known",
                &[Type::I32],
                Some(Type::Bool),
            ),
            Self::SetInteraction => (
                "game::world::set_interaction",
                &[Type::I32, Type::I32],
                None,
            ),
            Self::HideMarker => ("game::world::hide_marker", &[Type::I32], None),
            Self::SetModel => ("game::world::set_model", &[Type::I32, Type::I32], None),
            Self::UnmodeledMarker => ("game::world::unmodeled_marker", &[Type::I32], None),
            Self::Blocked => ("game::world::blocked", &[Type::I32], Some(Type::Bool)),
            Self::AlternateTile => ("game::world::alternate_tile", &[Type::I32, Type::I32], None),
        };
        NativeDeclaration {
            name,
            opcode: self as u8,
            parameters,
            result,
            suspends: false,
        }
    }
}

enum RuleHost<'a> {
    Pure,
    Entry {
        progress: &'a Progress<'a>,
        state: &'a mut ScriptState,
    },
    Refresh {
        progress: &'a Progress<'a>,
        locations: &'a Locations,
        appearances: &'a mut BTreeMap<u16, Appearance>,
    },
    Terrain {
        progress: &'a Progress<'a>,
        locations: &'a Locations,
        alternates: &'a mut BTreeSet<TileCoordinate>,
    },
}

fn value(value: impl Into<i32>) -> Result<NativeResult, String> {
    Ok(NativeResult::Continue(Some(value.into())))
}
fn id(value: i32) -> Result<u16, String> {
    value.try_into().map_err(|_| "invalid world data ID".into())
}
impl RuleHost<'_> {
    fn progress(&self) -> Result<&Progress<'_>, String> {
        match self {
            Self::Entry { progress, .. }
            | Self::Refresh { progress, .. }
            | Self::Terrain { progress, .. } => Ok(progress),
            _ => Err("world progress unavailable".into()),
        }
    }
    fn locations(&self) -> Result<&Locations, String> {
        match self {
            Self::Refresh { locations, .. } | Self::Terrain { locations, .. } => Ok(locations),
            _ => Err("landmarks unavailable".into()),
        }
    }
    fn set(
        &mut self,
        id: i32,
        update: impl FnOnce(&mut Appearance),
    ) -> Result<NativeResult, String> {
        let Self::Refresh { appearances, .. } = self else {
            return Err("landmark writes require a refresh".into());
        };
        if let Some(appearance) = appearances.get_mut(&self::id(id)?) {
            update(appearance);
        }
        Ok(NativeResult::Continue(None))
    }
}
impl Host for RuleHost<'_> {
    fn load_state(&self, name: &str) -> Result<Option<i32>, String> {
        let state = match self {
            Self::Entry { state, .. } => &**state,
            _ => self.progress()?.script_state,
        };
        Ok(state.get(name).copied())
    }
    fn store_state(&mut self, name: &str, value: i32) -> Result<(), String> {
        let Self::Entry { state, .. } = self else {
            return Err("world state writes require an entry hook".into());
        };
        state.insert(name.into(), value);
        Ok(())
    }
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register_typed(Native::Global.declaration(), |h, a, _| {
            value(
                resonance_events::script_global(h.progress()?.memory, a[0])
                    .map_err(|e| e.to_string())?,
            )
        })
        .register_typed(Native::Flag.declaration(), |h, a, _| {
            value(h.progress()?.event_flags.contains(&id(a[0])?))
        })
        .register_typed(Native::ItemCount.declaration(), |h, a, _| {
            value(h.progress()?.items.get(&id(a[0])?).copied().unwrap_or(0))
        })
        .register_typed(Native::Visited.declaration(), |h, a, _| {
            value(h.progress()?.visited.contains(&id(a[0])?))
        })
        .register_typed(Native::PartyAvailable.declaration(), |h, a, _| {
            let progress = h.progress()?;
            value(
                h.locations()?
                    .definitions
                    .party_requirements
                    .get(&id(a[0])?)
                    .is_none_or(|c| progress.formation.contains(c)),
            )
        })
        .register_typed(Native::GuidepostCount.declaration(), |h, _, _| {
            value(h.locations()?.guideposts.len() as i32)
        })
        .register_typed(Native::GuidepostLocation.declaration(), |h, a, _| {
            value(
                h.locations()?
                    .guideposts
                    .get(a[0] as usize)
                    .ok_or("invalid guidepost index")?
                    .location,
            )
        })
        .register_typed(Native::GuidepostKnown.declaration(), |h, a, _| {
            let post = h
                .locations()?
                .guideposts
                .get(a[0] as usize)
                .ok_or("invalid guidepost index")?;
            value(
                h.progress()?.event_flags.contains(
                    &post.event_flags[0]
                        .ok_or("guidepost lacks discovery flag")?
                        .get(),
                ),
            )
        })
        .register_typed(Native::SetInteraction.declaration(), |h, a, _| {
            let interaction = match a[1] {
                0 => Interaction::Disabled,
                1 => Interaction::Active,
                2 => Interaction::Blocked,
                _ => return Err("invalid landmark interaction".into()),
            };
            h.set(a[0], |a| a.interaction = interaction)
        })
        .register_typed(Native::HideMarker.declaration(), |h, a, _| {
            h.set(a[0], |a| a.marker = Marker::None)
        })
        .register_typed(Native::SetModel.declaration(), |h, a, _| {
            if !(1..=17).contains(&a[1]) {
                return Err("invalid landmark model".into());
            }
            h.set(a[0], |appearance| {
                appearance.marker = Marker::Model { id: a[1] as u8 }
            })
        })
        .register_typed(Native::UnmodeledMarker.declaration(), |h, a, _| {
            h.set(a[0], |a| a.marker = Marker::Unmodeled)
        })
        .register_typed(Native::Blocked.declaration(), |h, a, _| {
            let id = id(a[0])?;
            let appearance = match h {
                RuleHost::Refresh { appearances, .. } => appearances.get(&id).copied(),
                _ => h.locations()?.appearance(id),
            };
            value(appearance.is_some_and(|a| a.interaction == Interaction::Blocked))
        })
        .register_typed(Native::AlternateTile.declaration(), |h, a, _| {
            let RuleHost::Terrain { alternates, .. } = h else {
                return Err("terrain writes require terrain selection".into());
            };
            let column = u8::try_from(a[0]).map_err(|_| "invalid terrain column")?;
            let row = u8::try_from(a[1]).map_err(|_| "invalid terrain row")?;
            alternates.insert(TileCoordinate::new(column, row).map_err(|e| e.to_string())?);
            Ok(NativeResult::Continue(None))
        });
}

pub fn native_declarations() -> Vec<NativeDeclaration> {
    RuleHost::AUTHORED_NATIVES.declarations().collect()
}

#[derive(Clone, Copy)]
enum Rule {
    Refresh,
    OnEnter,
    RecordsVisit,
    Music,
    TerrainVariants,
}
impl Rule {
    const ALL: [Self; 5] = [
        Self::Refresh,
        Self::OnEnter,
        Self::RecordsVisit,
        Self::Music,
        Self::TerrainVariants,
    ];
    fn signature(self) -> (&'static str, u16, u16) {
        match self {
            Self::Refresh => ("world::rules::refresh", 0, 0),
            Self::OnEnter => ("world::rules::on_enter", 1, 0),
            Self::RecordsVisit => ("world::rules::records_visit", 1, 1),
            Self::Music => ("world::rules::music", 2, 1),
            Self::TerrainVariants => ("world::rules::terrain_variants", 1, 0),
        }
    }
}

#[derive(Debug)]
pub struct Rules {
    program: Arc<Program>,
    entries: [u32; Rule::ALL.len()],
}
impl Rules {
    pub fn prepare(
        cache: &mut PreparationCache,
        sources: &impl SourceResolver,
    ) -> Result<Arc<Self>> {
        let generation = cache.prepare(["world::rules"], sources, &native_declarations())?;
        let module = generation
            .module("world::rules")
            .context("world rules missing")?;
        ensure!(module.assets.is_empty(), "world rules cannot load assets");
        let authored = module
            .program
            .authored()
            .context("world rules are not authored")?;
        ensure!(
            authored.texts.is_empty(),
            "world rules cannot display messages"
        );
        Vm::validate_bindings::<RuleHost>(&module.program)?;
        let mut entries = [0; Rule::ALL.len()];
        for rule in Rule::ALL {
            let (name, parameters, results) = rule.signature();
            let function = authored
                .functions
                .iter()
                .find(|f| f.name == name)
                .with_context(|| format!("missing {name}"))?;
            ensure!(
                !function.is_task
                    && function.parameters == parameters
                    && function.results == results,
                "invalid world rule signature: {name}"
            );
            entries[rule as usize] = function.entry;
        }
        Ok(Arc::new(Self {
            program: module.program.clone(),
            entries,
        }))
    }
    fn run<const N: usize>(
        &self,
        rule: Rule,
        args: &[i32],
        host: &mut RuleHost<'_>,
    ) -> Result<[i32; N]> {
        let mut vm = Vm::with_arguments(self.program.clone(), self.entries[rule as usize], args)?;
        let result = vm
            .run(host, &mut Memory::default(), 32_768)
            .with_context(|| format!("world rule {}", rule.signature().0))?;
        ensure!(
            result.event == RunEvent::Halted,
            "world rule unexpectedly suspended"
        );
        vm.result()
            .context("world rule did not return")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("world rule returned the wrong number of values"))
    }
    pub(super) fn on_enter(&self, world: World, persistent: &mut PersistentState) -> Result<()> {
        let progress = Progress::new(
            &persistent.memory,
            persistent
                .party
                .as_ref()
                .context("world party is missing")?,
            &persistent.event_flags,
            &persistent.script_state,
        );
        let mut state = persistent.script_state.clone();
        self.run::<0>(
            Rule::OnEnter,
            &[world.index() as i32],
            &mut RuleHost::Entry {
                progress: &progress,
                state: &mut state,
            },
        )?;
        persistent.script_state = state;
        Ok(())
    }
    pub(super) fn records_visit(&self, location: u16) -> Result<bool> {
        let [value] = self.run(Rule::RecordsVisit, &[location.into()], &mut RuleHost::Pure)?;
        Ok(value != 0)
    }
    pub(super) fn music(&self, mount: Mount, bank: i32) -> Result<u16> {
        let [value] = self.run(Rule::Music, &[mount as i32, bank], &mut RuleHost::Pure)?;
        value
            .try_into()
            .context("world music rule returned an invalid track")
    }
    pub(super) fn refresh(
        &self,
        locations: &Locations,
        progress: &Progress<'_>,
    ) -> Result<BTreeMap<u16, Appearance>> {
        let mut appearances = Locations::default_appearances(&locations.definitions);
        self.run::<0>(
            Rule::Refresh,
            &[],
            &mut RuleHost::Refresh {
                progress,
                locations,
                appearances: &mut appearances,
            },
        )?;
        Ok(appearances)
    }
    pub(super) fn terrain_variants(
        &self,
        locations: &Locations,
        progress: &Progress<'_>,
        world: World,
    ) -> Result<BTreeSet<TileCoordinate>> {
        let mut alternates = BTreeSet::new();
        self.run::<0>(
            Rule::TerrainVariants,
            &[world.index() as i32],
            &mut RuleHost::Terrain {
                progress,
                locations,
                alternates: &mut alternates,
            },
        )?;
        Ok(alternates)
    }
}

#[cfg(test)]
pub(crate) fn fixture() -> Arc<Rules> {
    static RULES: std::sync::OnceLock<Arc<Rules>> = std::sync::OnceLock::new();
    RULES
        .get_or_init(|| {
            let sources = symphonia_script_tools::SourceTree::load(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../scripts"
            ))
            .unwrap();
            Rules::prepare(&mut PreparationCache::default(), &sources).unwrap()
        })
        .clone()
}
