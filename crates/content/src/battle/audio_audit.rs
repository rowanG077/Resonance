//! Trace authored requests separately from unresolved native callback requirements.
use super::*;
use crate::battle::{
    action_inventory::ActionInventory,
    action_program::{CommandDependency, CommandProgram},
    actions::HitRule,
    arte_inventory::{ArtePhaseInventory, NativeDispatch, Recovery},
    effect_inventory::{EffectSourceBank, EffectTimelineCommand},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAudit {
    pub requirements: Vec<AudioRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRequirement {
    pub context: AudioContext,
    pub tick: Option<u16>,
    pub request: Option<AudioRequest>,
    pub resolution: AudioResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioContext {
    Normal {
        character: u8,
        selection: u8,
    },
    Arte {
        native_id: u16,
        phase: u8,
        character: u8,
    },
    SharedArte {
        native_id: u16,
        phase: u8,
    },
    Cast {
        character: u8,
        native_id: u16,
    },
    NativeArte {
        native_id: u16,
    },
    Enemy {
        monster: u8,
        action: u8,
        recovery: bool,
    },
    Effect {
        bank: EffectSourceBank,
        program: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioResolution {
    Source { route: AudioRoute },
    Missing { reason: String },
    UnresolvedProgram { reason: String },
    NativeDependency { dependency: CommandDependency },
    NativeCallback,
    UnresolvedEffect { opcode: u8 },
    NoAuthoredProgram,
    NativeHitPolicy { flags: u16 },
}

impl AudioInventory {
    /// Source routing is necessary for closure, but does not claim a decoded or
    /// rendered cue has passed its independent executable/oracle case.
    pub fn routing_audit(&self, actions: &ActionInventory) -> AudioAudit {
        let mut audit = AudioAudit {
            requirements: Vec::new(),
        };
        for party in &actions.artes.party {
            let actor = AudioActor::Party(party.character);
            for normal in &party.normals {
                let context = AudioContext::Normal {
                    character: party.character,
                    selection: normal.selection,
                };
                audit.program(
                    self,
                    &context,
                    Some(actor),
                    &normal.programs.commands.recovery,
                );
                if let Recovery::Recovered(hits) = &normal.programs.hits.recovery {
                    for hit in hits {
                        audit.hit(self, &context, Some(hit.start), &hit.rule);
                    }
                }
            }
            for cast in &party.casting_voices {
                let context = AudioContext::Cast {
                    character: party.character,
                    native_id: cast.native_id,
                };
                for id in std::iter::once(cast.begin).chain(cast.release) {
                    audit.request(self, &context, None, AudioRequest::ActorVoice { actor, id });
                }
            }
        }
        for (native_id, phases) in actions
            .artes
            .martial
            .iter()
            .map(|arte| (arte.native_id, &arte.phases))
            .chain(
                actions
                    .artes
                    .native
                    .iter()
                    .map(|arte| (arte.native_id, &arte.bundle.phases)),
            )
        {
            audit.bundle(self, actions, native_id, phases);
        }
        for (native_id, dispatch) in actions
            .artes
            .native
            .iter()
            .map(|arte| (arte.native_id, &arte.dispatch))
            .chain(
                actions
                    .artes
                    .martial
                    .iter()
                    .map(|arte| (arte.native_id, &arte.dispatch)),
            )
        {
            let resolution = match dispatch {
                NativeDispatch::Absent => continue,
                NativeDispatch::Callbacks { .. } => AudioResolution::NativeCallback,
                NativeDispatch::Unresolved { diagnostic } => AudioResolution::UnresolvedProgram {
                    reason: diagnostic.clone(),
                },
            };
            audit.push(
                AudioContext::NativeArte { native_id },
                None,
                None,
                resolution,
            );
        }
        for package in &actions.enemies.packages {
            let actor = AudioActor::Enemy(package.monster);
            for action in &package.actions {
                let context = AudioContext::Enemy {
                    monster: package.monster,
                    action: action.id,
                    recovery: false,
                };
                if let Some(program) = &action.commands {
                    audit.commands(self, &context, Some(actor), program);
                } else {
                    audit.unresolved(
                        context.clone(),
                        "enemy action commands were not recovered".into(),
                    );
                }
                if let Some(hits) = &action.hits {
                    for hit in hits {
                        audit.hit(self, &context, Some(hit.start), &hit.rule);
                    }
                }
                let recovery = AudioContext::Enemy {
                    monster: package.monster,
                    action: action.id,
                    recovery: true,
                };
                if let Some(program) = &action.recovery_commands {
                    audit.commands(self, &recovery, Some(actor), program);
                } else if action.source.recovery_command_index.is_some() {
                    audit.unresolved(
                        recovery,
                        "enemy recovery commands were not recovered".into(),
                    );
                }
                if action.native_technique.is_some() {
                    let [mut begin, release] = action.cast_voices;
                    if begin == 0
                        && let Ok(NativeVoices::Voiced { base, .. }) = self.native_voices(actor)
                    {
                        begin = (base + 7) | 0x8000;
                    }
                    for id in [begin, release] {
                        audit.request(self, &context, None, AudioRequest::ActorVoice { actor, id });
                    }
                    audit.push(context, None, None, AudioResolution::NativeCallback);
                }
            }
        }
        for bank in &actions.effects.banks {
            for program in &bank.programs {
                let context = AudioContext::Effect {
                    bank: bank.id,
                    program: program.id,
                };
                for record in &program.timeline {
                    audit.effect(
                        self,
                        &context,
                        u16::try_from(record.tick).ok(),
                        &record.command,
                    );
                }
            }
        }
        audit
    }
}

impl AudioAudit {
    fn push(
        &mut self,
        context: AudioContext,
        tick: Option<u16>,
        request: Option<AudioRequest>,
        resolution: AudioResolution,
    ) {
        self.requirements.push(AudioRequirement {
            context,
            tick,
            request,
            resolution,
        });
    }
    fn request(
        &mut self,
        inventory: &AudioInventory,
        context: &AudioContext,
        tick: Option<u16>,
        request: AudioRequest,
    ) {
        let resolution = match inventory.resolve(request) {
            Ok(route) => AudioResolution::Source { route },
            Err(error) => AudioResolution::Missing {
                reason: format!("{error:#}"),
            },
        };
        self.push(context.clone(), tick, Some(request), resolution);
    }
    fn unresolved(&mut self, context: AudioContext, reason: String) {
        self.push(
            context,
            None,
            None,
            AudioResolution::UnresolvedProgram { reason },
        );
    }
    fn bundle(
        &mut self,
        inventory: &AudioInventory,
        actions: &ActionInventory,
        native_id: u16,
        phases: &Recovery<Vec<ArtePhaseInventory>>,
    ) {
        let owners: BTreeSet<_> = actions
            .artes
            .artes
            .iter()
            .filter(|arte| arte.native_id == native_id)
            .flat_map(|arte| arte.owners.iter().copied())
            .collect();
        match phases {
            Recovery::Recovered(phases) => {
                for phase in phases {
                    let hit_context = AudioContext::SharedArte {
                        native_id,
                        phase: phase.phase,
                    };
                    if let Recovery::Recovered(rules) = &phase.hit_rules.recovery {
                        for rule in rules {
                            self.hit(inventory, &hit_context, None, rule);
                        }
                    }
                    if let Recovery::Recovered(hits) = &phase.programs.hits.recovery {
                        for hit in hits {
                            self.hit(inventory, &hit_context, Some(hit.start), &hit.rule);
                        }
                    }
                    if owners.is_empty() {
                        self.program(
                            inventory,
                            &AudioContext::SharedArte {
                                native_id,
                                phase: phase.phase,
                            },
                            None,
                            &phase.programs.commands.recovery,
                        );
                    }
                    for &character in &owners {
                        let context = AudioContext::Arte {
                            native_id,
                            phase: phase.phase,
                            character,
                        };
                        self.program(
                            inventory,
                            &context,
                            Some(AudioActor::Party(character)),
                            &phase.programs.commands.recovery,
                        );
                    }
                }
            }
            Recovery::Unresolved { diagnostic, .. } => {
                self.unresolved(AudioContext::NativeArte { native_id }, diagnostic.clone())
            }
            Recovery::Absent => self.push(
                AudioContext::NativeArte { native_id },
                None,
                None,
                AudioResolution::NoAuthoredProgram,
            ),
        }
    }
    fn program(
        &mut self,
        inventory: &AudioInventory,
        context: &AudioContext,
        actor: Option<AudioActor>,
        program: &Recovery<CommandProgram>,
    ) {
        match program {
            Recovery::Absent => self.push(
                context.clone(),
                None,
                None,
                AudioResolution::NoAuthoredProgram,
            ),
            Recovery::Recovered(program) => self.commands(inventory, context, actor, program),
            Recovery::Unresolved { diagnostic, .. } => {
                self.unresolved(context.clone(), diagnostic.clone())
            }
        }
    }
    fn commands(
        &mut self,
        inventory: &AudioInventory,
        context: &AudioContext,
        actor: Option<AudioActor>,
        program: &CommandProgram,
    ) {
        for command in &program.commands {
            for &dependency in &command.dependencies {
                let request = match dependency {
                    CommandDependency::Sound { id } => AudioRequest::Sound { id },
                    CommandDependency::Voice { id, .. } => match actor {
                        Some(actor) => AudioRequest::ActorVoice { actor, id },
                        None => AudioRequest::Voice { id },
                    },
                    CommandDependency::NativeTechnique { .. }
                    | CommandDependency::TargetEvent { .. } => {
                        self.push(
                            context.clone(),
                            Some(command.tick),
                            None,
                            AudioResolution::NativeDependency { dependency },
                        );
                        continue;
                    }
                    _ => continue,
                };
                self.request(inventory, context, Some(command.tick), request);
            }
        }
    }
    fn effect(
        &mut self,
        inventory: &AudioInventory,
        context: &AudioContext,
        tick: Option<u16>,
        command: &EffectTimelineCommand,
    ) {
        match command {
            EffectTimelineCommand::Sound { sound, .. } => {
                self.request(inventory, context, tick, AudioRequest::Sound { id: *sound })
            }
            EffectTimelineCommand::Repeat { command, .. } => {
                self.effect(inventory, context, tick, command)
            }
            EffectTimelineCommand::Unresolved { opcode } => self.push(
                context.clone(),
                tick,
                None,
                AudioResolution::UnresolvedEffect { opcode: *opcode },
            ),
            _ => {}
        }
    }

    fn hit(
        &mut self,
        inventory: &AudioInventory,
        context: &AudioContext,
        tick: Option<u16>,
        rule: &HitRule,
    ) {
        // Zero here selects native weapon/material defaults; it is not an
        // authored silent request as zero in a direct sound command would be.
        if rule.sound != 0 {
            self.request(
                inventory,
                context,
                tick,
                AudioRequest::Sound { id: rule.sound },
            );
        }
        self.push(
            context.clone(),
            tick,
            None,
            AudioResolution::NativeHitPolicy { flags: rule.flags },
        );
    }
}
