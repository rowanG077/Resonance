use crate::{
    data::{Art, clamp, load_visits, smooth},
    lighting::{LightState, Lighting},
    scene::{Actor, Prop, sample},
};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub duration: f64,
    pub growth: f64,
    pub start: f64,
}
#[derive(Clone, Default)]
pub(crate) struct SceneState {
    pub time: f64,
    pub finished: bool,
    pub actors: Vec<Actor>,
    pub props: Vec<Prop>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Visit {
    pub event: Event,
    pub actors: Vec<Vec<Actor>>,
    pub props: Vec<Vec<Prop>>,
    #[serde(default)]
    pub finale: bool,
}
impl Visit {
    pub fn end(&self) -> f64 {
        self.event.start + self.event.duration
    }
    pub fn sample(&self, time: f64) -> SceneState {
        let time = time.clamp(0., self.event.duration);
        let actors: Vec<_> = self.actors.iter().map(|keys| sample(keys, time)).collect();
        let props = self
            .props
            .iter()
            .map(|keys| {
                let mut p = sample(keys, time);
                if let Some(id) = &p.attach {
                    let a = actors.iter().find(|a| &a.id == id).expect("prop owner");
                    p.x = a.x + a.face() * p.x;
                    p.y += a.y;
                    p.face = Some(p.face.unwrap_or(a.face()));
                    p.actor = Some(a.clone());
                }
                p
            })
            .collect();
        SceneState {
            time,
            actors,
            props,
            finished: self.finale && time >= self.event.duration,
        }
    }
}
pub(crate) struct Timeline {
    pub visits: Vec<Visit>,
    pub duration: f64,
    pub growth_end: f64,
    growth: Vec<[f64; 2]>,
}
pub(crate) struct Moment {
    pub progress: f64,
    pub state: SceneState,
    pub event_time: f64,
    pub light: LightState,
}
impl Timeline {
    pub fn new(data: &Art) -> Result<Self, crate::Error> {
        let mut visits = load_visits()?;
        for visit in &mut visits {
            for keys in &mut visit.actors {
                for i in 1..keys.len() {
                    keys[i].travel = keys[i - 1].travel
                        + (keys[i].x - keys[i - 1].x).hypot(keys[i].y - keys[i - 1].y);
                    keys[i].pose_since = if keys[i].pose == keys[i - 1].pose {
                        keys[i - 1].pose_since
                    } else {
                        keys[i].t
                    };
                }
            }
            for keys in &mut visit.props {
                for p in keys {
                    p.event_id = visit.event.id.clone();
                }
            }
        }
        let mut growth = vec![[0., 0.]];
        growth.extend(
            visits
                .iter()
                .filter(|v| !v.finale)
                .map(|v| [v.event.start + v.event.duration / 2., v.event.growth]),
        );
        growth.push([data.growth_end, 1.]);
        Ok(Self {
            visits,
            duration: data.duration,
            growth_end: data.growth_end,
            growth,
        })
    }
    pub fn events(&self) -> Vec<Event> {
        self.visits.iter().map(|v| v.event.clone()).collect()
    }
    pub fn event(&self, id: &str) -> Option<&Visit> {
        self.visits.iter().find(|v| v.event.id == id)
    }
    pub fn actor_at(&self, event: &str, id: &str, time: f64) -> Actor {
        sample(
            self.event(event)
                .unwrap()
                .actors
                .iter()
                .find(|keys| keys[0].id == id)
                .unwrap(),
            time,
        )
    }
    pub fn progress(&self, time: f64) -> f64 {
        if time >= self.growth_end {
            return 1.;
        }
        let pair = self
            .growth
            .windows(2)
            .find(|p| time >= p[0][0] && time < p[1][0])
            .unwrap_or(&self.growth[..2]);
        pair[0][1]
            + (pair[1][1] - pair[0][1]) * clamp((time - pair[0][0]) / (pair[1][0] - pair[0][0]))
    }
    pub fn world(&self, time: f64, art: &Art) -> Vec<Prop> {
        art.world
            .iter()
            .filter_map(|keys| {
                let i = keys.partition_point(|p| p.t <= time);
                i.checked_sub(1).map(|i| keys[i].clone())
            })
            .collect()
    }
    pub fn light(&self, time: f64, art: &Art) -> LightState {
        LightState::dusk(
            smooth((time - self.growth_end) / art.dusk_duration),
            Lighting::Auto,
        )
    }

    pub fn with_world(&self, mut state: SceneState, time: f64, art: &Art) -> SceneState {
        let mut props = self.world(time, art);
        for p in state.props {
            replace_prop(&mut props, p);
        }
        state.props = props;
        state
    }
    pub fn at(&self, time: f64, art: &Art) -> Moment {
        let visit = self
            .visits
            .iter()
            .find(|v| time >= v.event.start && (time < v.end() || v.finale));
        let event_time = visit.map_or(0., |v| (time - v.event.start).min(v.event.duration));
        let state = visit.map_or_else(
            || SceneState {
                time,
                ..Default::default()
            },
            |v| v.sample(crate::exposure(event_time)),
        );
        Moment {
            progress: self.progress(time),
            state: self.with_world(state, time, art),
            event_time,
            light: self.light(time, art),
        }
    }
}
fn replace_prop(props: &mut Vec<Prop>, prop: Prop) {
    if let Some(old) = props.iter_mut().find(|p| p.id == prop.id) {
        *old = prop;
    } else {
        props.push(prop);
    }
}
