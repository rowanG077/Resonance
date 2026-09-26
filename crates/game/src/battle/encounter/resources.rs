//! Resolve maintained source assets against this encounter's prepared roster.
use crate::battle::{self, BattleResources, EffectResource, MeleeResource, ProjectileResource};
use anyhow::{Context, Result, bail, ensure};
use resonance_battle::{
    ModelDefinition, MotionBinding, ParticleDefinition, SoundBinding, VoiceLine,
};
use resonance_content::prepared::Files;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) struct ActorResources {
    pub source: battle::model::ModelSource,
    pub model: Arc<ModelDefinition>,
    pub contacts: Vec<Vec<u16>>,
    pub death: [Option<MotionBinding>; 2],
}

pub(super) struct Resources<'a, S> {
    pub files: &'a Files,
    pub actors: &'a [ActorResources],
    pub common: EffectResource,
    pub techniques: EffectResource,
    pub enemy_effects: BTreeMap<u8, EffectResource>,
    pub selected: BTreeMap<u32, BTreeSet<u16>>,
    pub fire_ball: Option<u16>,
    pub performances: &'a [battle::victory::Performance],
    pub sound: S,
}

fn character(name: &str) -> Result<u8> {
    [
        "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
    ]
    .iter()
    .position(|&value| value == name)
    .map(|index| index as u8 + 1)
    .with_context(|| format!("unknown battle character {name}"))
}

impl<S> Resources<'_, S> {
    fn track(&mut self, request: &EffectResource) {
        self.selected
            .entry(request.resource)
            .or_default()
            .extend(&request.members);
    }

    fn party(&self, character: u8) -> Result<&ActorResources> {
        self.actors
            .iter()
            .find(|actor| {
                matches!(actor.source,
            battle::model::ModelSource::Party(id) if id == character)
            })
            .context("battle resource requires an absent party character")
    }

    fn enemy(&self, enemy: u8) -> Result<&ActorResources> {
        self.actors
            .iter()
            .find(|actor| {
                matches!(actor.source,
            battle::model::ModelSource::Enemy(id) if id == enemy)
            })
            .context("battle resource requires an absent enemy")
    }
}

impl<S: FnMut(battle::voice::Sound) -> Result<SoundBinding>> BattleResources for Resources<'_, S> {
    fn effects(&mut self) -> Vec<EffectResource> {
        let common = self.common.clone();
        self.track(&common);
        vec![common]
    }
    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        let id = path
            .strip_prefix("battle/sounds/common/")
            .context("unknown battle sound namespace")?
            .parse()?;
        (self.sound)(battle::voice::Sound::Cue(id))
    }

    fn voice(&mut self, path: &str) -> Result<Vec<Option<VoiceLine>>> {
        let sources: Vec<_> = self.actors.iter().map(|actor| actor.source).collect();
        let components: Vec<_> = path.split('/').collect();
        match components.as_slice() {
            ["battle", "voices", "absolute", line] => {
                battle::voice::absolute(self.files, &sources, line.parse()?, &mut self.sound)
            }
            ["battle", "voices", "relative", line] => {
                battle::voice::relative(self.files, &sources, line.parse()?, &mut self.sound)
            }
            ["battle", "voices", "techniques", name, technique, phase] => {
                self.party(character(name)?)?;
                let phase = match *phase {
                    "chant" => battle::voice::Phase::Chant,
                    "fallback" => battle::voice::Phase::Fallback,
                    "release" => battle::voice::Phase::Release,
                    "self" => battle::voice::Phase::SelfChant,
                    _ => bail!("unknown battle voice phase {phase}"),
                };
                battle::voice::technique(
                    self.files,
                    &sources,
                    technique.parse()?,
                    phase,
                    &mut self.sound,
                )
            }
            _ => bail!("unknown battle voice {path}"),
        }
    }

    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        if path.starts_with("battle/motions/victory/") {
            return battle::victory::motion(path, self.performances);
        }
        let components: Vec<_> = path.split('/').collect();
        let (actor, clip) = match components.as_slice() {
            ["battle", "motions", "enemies", id, clip] => (self.enemy(id.parse()?)?, clip),
            ["battle", "motions", name, clip] => (self.party(character(name)?)?, clip),
            _ => bail!("unknown battle motion {path}"),
        };
        let clip = clip.parse()?;
        ensure!(
            actor.model.motions.contains_key(&clip),
            "missing battle motion {path}"
        );
        Ok(MotionBinding {
            model: actor.model.resource,
            clip,
        })
    }

    fn optional_motion(&mut self, path: &str) -> Result<Vec<Option<MotionBinding>>> {
        if let Some(clip) = path.strip_prefix("battle/motions/control/") {
            let clip: u16 = clip.parse()?;
            ensure!(
                matches!(clip, 14 | 15 | 16 | 28),
                "unknown control motion {clip}"
            );
            return self
                .actors
                .iter()
                .map(|actor| {
                    if !matches!(actor.source, battle::model::ModelSource::Party(_)) {
                        return Ok(None);
                    }
                    ensure!(
                        actor.model.motions.contains_key(&clip),
                        "missing control motion {clip}"
                    );
                    Ok(Some(MotionBinding {
                        model: actor.model.resource,
                        clip,
                    }))
                })
                .collect();
        }
        let index = match path {
            battle::death::FALL => 0,
            battle::death::REST => 1,
            _ => bail!("unknown optional battle motion {path}"),
        };
        Ok(self.actors.iter().map(|actor| actor.death[index]).collect())
    }

    fn casting(&mut self, path: &str) -> Result<battle::casting::CastingResource> {
        let components: Vec<_> = path.split('/').collect();
        let ["battle", "casting", name, technique] = components.as_slice() else {
            bail!("unknown battle casting parameters {path}");
        };
        let character = character(name)?;
        let model = self.party(character)?.model.resource;
        Ok(battle::casting::CastingResource {
            character,
            technique: technique.parse()?,
            model,
            stored_scene: None,
        })
    }

    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        if path.starts_with("battle/melee/lloyd/") {
            battle::normal::lloyd_melee(path, &self.party(1)?.contacts)
        } else if path.starts_with("battle/melee/colette/") {
            battle::normal::colette_melee(path, &self.party(2)?.contacts)
        } else if path.starts_with("battle/melee/genis/") {
            battle::normal::genis_melee(path, &self.party(3)?.contacts)
        } else if path.starts_with("battle/melee/enemies/036/") {
            battle::enemy::zombie_melee(path, &self.enemy(36)?.contacts)
        } else if path.starts_with("battle/melee/enemies/049/") {
            battle::enemy::ghost_melee(path, &self.enemy(49)?.contacts)
        } else {
            bail!("unprepared battle contact {path}")
        }
    }

    fn weapon_flight(&mut self, path: &str) -> Result<battle::WeaponFlightResource> {
        self.party(2)?;
        battle::normal::colette_flight(path)
    }

    fn effect(&mut self, path: &str) -> Result<EffectResource> {
        let request = match path {
            "battle/effects/common" => self.common.clone(),
            "battle/effects/techniques/fire_ball" => battle::fire_ball::effect(&self.techniques),
            "battle/effects/techniques/demon_fang" => {
                let mut bank = self.techniques.clone();
                bank.members = vec![3];
                bank
            }
            _ => bail!("unprepared battle effect {path}"),
        };
        self.track(&request);
        Ok(request)
    }

    fn projectile(&mut self, path: &str) -> Result<ProjectileResource> {
        let request = match path {
            "battle/projectiles/techniques/3" => {
                battle::fire_ball::projectile(self.files, &self.techniques)
            }
            "battle/projectiles/martial/1" => {
                battle::martial::projectile(self.files, 1, &self.techniques)
            }
            "battle/projectiles/martial/35" => {
                battle::martial::projectile(self.files, 35, &self.techniques)
            }
            "battle/projectiles/enemies/049/0" => battle::enemy::ghost_projectile(
                path,
                self.enemy_effects
                    .get(&49)
                    .context("missing Ghost effect bank")?
                    .clone(),
                self.common.clone(),
            ),
            _ => bail!("unprepared battle projectile {path}"),
        }?;
        for effect in [
            &request.birth,
            &request.trail,
            &request.ground,
            &request.clash,
            &request.impact,
        ]
        .into_iter()
        .flatten()
        {
            self.track(effect);
        }
        Ok(request)
    }

    fn spell(&mut self, path: &str) -> Result<u16> {
        match path {
            "battle/spells/fire_ball" => {
                self.fire_ball.context("Fire Ball release is not prepared")
            }
            _ => bail!("unprepared battle spell {path}"),
        }
    }

    fn particle(&mut self, path: &str) -> Result<Arc<ParticleDefinition>> {
        bail!("unprepared battle particle template {path}")
    }
}
