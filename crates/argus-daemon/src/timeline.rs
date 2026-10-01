//! Timelines through the protocol (§13 "Tweens and sequencing", phase 3
//! item 5): `timeline.list` reads them (every timeline with its tracks and
//! cues in time order, and those playing with where they are), and
//! `timeline.seek` scrubs one: the run is replayed from a keyframe to the
//! first frame at which the timeline had reached a time, which is what the
//! editor's scrubber (phase 4) will drive.

use super::*;
use argus_timeline::table::{Kind, Who, ROLES};
use argus_timeline::Timeline;

/// A cue's time within a frame of this (sim time adds up float steps).
const EPS: f64 = 1e-4;

impl Daemon {
    pub(super) fn reload_timelines(&mut self) {
        if let Some(ses) = self.session.as_ref() {
            *ses.timelines.borrow_mut() =
                argus_player::read_timelines(&ses.project.defs, &self.world);
        }
    }

    /// `script.errors` rows for timeline rows that do not read.
    pub(super) fn timeline_errors(&self) -> Vec<Json> {
        let Some(ses) = self.session.as_ref() else {
            return Vec::new();
        };
        ses.timelines
            .borrow()
            .problems
            .iter()
            .map(|(at, msg)| {
                let (file, line) = match at.rsplit_once(':') {
                    Some((f, l)) if l.parse::<u32>().is_ok() => {
                        (f.to_string(), l.parse::<u32>().ok())
                    }
                    _ => (at.clone(), None),
                };
                json!({ "file": file, "line": line, "message": msg, "phase": "timeline", "ref": at })
            })
            .collect()
    }

    pub(super) fn timeline_list(&self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let only = json::opt_str(params, "name")?
            .map(|n| n.strip_prefix("timeline:").unwrap_or(n).to_string());
        let lines = ses.timelines.borrow();
        // A row's fields as its file writes them (the rest are defaults):
        // what an editor edits, and what a field left out means.
        let table = ses.project.defs.table(argus_timeline::table::TABLE);
        let written = |key: &str| -> Json {
            let mut out = serde_json::Map::new();
            if let Some((t, r)) = table.and_then(|t| t.get(key).map(|r| (t, r))) {
                for (i, f) in t.schema.fields.iter().enumerate() {
                    if f.name != t.key && r.given.get(i).copied().unwrap_or(false) {
                        out.insert(f.name.clone(), json::to_json(&self.world, &r.values[i]));
                    }
                }
            }
            Json::Object(out)
        };
        let who = |w: &Who| match w {
            Who::Role(i) => ROLES.get(*i).copied().unwrap_or("?").to_string(),
            Who::Id(id) => format!("@{id}"),
        };
        let mut timelines = Vec::new();
        for l in lines.lines() {
            if only.as_ref().is_some_and(|n| *n != l.key) {
                continue;
            }
            let tracks: Vec<Json> = l
                .tracks()
                .into_iter()
                .map(|(track, cues)| {
                    let rows: Vec<Json> = cues
                        .into_iter()
                        .map(|i| {
                            let c = &l.cues[i];
                            let what = match c.kind {
                                Kind::Tween => format!(
                                    "{} to {:?} over {}s{}",
                                    c.path,
                                    [c.to.x, c.to.y],
                                    c.duration,
                                    if c.easing.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" ({})", c.easing)
                                    }
                                ),
                                Kind::Anim => format!("state {}", c.state),
                                Kind::Camera if c.duration > 0.0 => {
                                    format!("draws for {}s", c.duration)
                                }
                                Kind::Camera => "draws to the end".to_string(),
                                Kind::Line => format!("dialogue:{}", c.line),
                                Kind::Event => format!("{} edits", c.effects.len()),
                            };
                            json!([
                                i,
                                c.key,
                                c.at,
                                c.end(),
                                c.kind.name(),
                                who(&c.who),
                                what,
                                c.wait,
                                c.at_ref,
                                written(&c.key)
                            ])
                        })
                        .collect();
                    json!({ "track": track, "cues": rows })
                })
                .collect();
            timelines.push(json!({
                "name": l.key,
                "length": l.length,
                "ref": l.at_ref,
                "cue_columns": ["n", "cue", "at", "end", "kind", "who", "what", "wait", "ref", "fields"],
                "tracks": tracks,
                "fields": written(&l.key),
            }));
        }
        let mut playing = Vec::new();
        if let Some(tid) = self.world.component_id_of::<Timeline>() {
            let mut ents = self.world.query().has(tid).entities();
            ents.sort();
            for e in ents {
                let t = self.world.get::<Timeline>(e)?;
                let name = self.world.strings().resolve(t.name).to_string();
                let name = name.strip_prefix("timeline:").unwrap_or(&name).to_string();
                if only.as_ref().is_some_and(|n| *n != name) {
                    continue;
                }
                playing.push(json!({
                    "timeline": json::entity_ref(&self.world, e),
                    "name": name,
                    "time": t.time,
                    "length": t.length,
                    "next": t.next,
                    "held": t.held,
                    "waiting": json::entity_ref(&self.world, t.waiting),
                    "began": t.began,
                    "actor": json::entity_ref(&self.world, t.actor),
                    "other": json::entity_ref(&self.world, t.other),
                }));
            }
        }
        let problems: Vec<Json> = lines
            .problems
            .iter()
            .map(|(at, m)| json!({ "ref": at, "message": m }))
            .collect();
        Ok(json!({
            "timelines": timelines,
            "playing": playing,
            "problems": problems,
            "kinds": ["tween", "anim", "camera", "line", "event"],
            "roles": ROLES,
            "easings": argus_tween::ease::NAMES,
        }))
    }

    /// The timelines' problems now, each a sentence with where.
    pub(super) fn timeline_problems(&self) -> Vec<String> {
        self.session
            .as_ref()
            .map(|s| {
                s.timelines
                    .borrow()
                    .problems
                    .iter()
                    .map(|(at, m)| format!("{m} ({at})"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The latest `timeline.started` in the log for an entity or a name:
    /// the timeline and the frame it began on.
    fn timeline_started(&self, e: Option<Entity>, name: Option<&str>) -> Option<(Entity, u64)> {
        self.events.records.iter().rev().find_map(|(_, r)| {
            let Event::Game { name: n, fields } = &r.event else {
                return None;
            };
            if n != "timeline.started" {
                return None;
            }
            let get = |k: &str| fields.iter().find(|f| f.0 == k).map(|f| &f.1);
            let Some(Value::Entity(t)) = get("timeline") else {
                return None;
            };
            let ok = match (e, name) {
                (Some(e), _) => *t == e,
                (None, Some(want)) => get("name") == Some(&Value::Str(want.to_string())),
                _ => false,
            };
            ok.then_some((*t, r.frame))
        })
    }

    /// `timeline.play`: play a timeline now (phase 4 item 9, the editor's
    /// Play), as a script does: a Timeline entity of the run, not of the
    /// scene's files, with its actor and other. A change between ticks.
    pub(super) fn timeline_play(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let name = json::str_param(params, "name")?;
        let short = name.strip_prefix("timeline:").unwrap_or(name).to_string();
        if self
            .session
            .as_ref()
            .expect("session")
            .timelines
            .borrow()
            .line(&short)
            .is_none()
        {
            return Err(
                ArgusError::new(codes::BAD_PARAMS, format!("no timeline {short:?}"))
                    .hint("timeline.list lists the game's timelines."),
            );
        }
        let actor = match json::opt_str(params, "actor")? {
            Some(a) => self.world.entity(a)?,
            None => Entity::NULL,
        };
        let other = match json::opt_str(params, "other")? {
            Some(a) => self.world.entity(a)?,
            None => Entity::NULL,
        };
        let speed = json::f64_param(params, "speed", 1.0)? as f32;
        argus_timeline::register(&mut self.world)?;
        let e = self.world.spawn();
        let name_id = self.world.strings().intern(&short);
        self.world.insert(
            e,
            Timeline {
                name: name_id,
                actor,
                other,
                speed,
                ..Timeline::default()
            },
        )?;
        self.mutations += 1;
        Ok(json!({ "timeline": json::entity_str(&self.world, e), "name": short }))
    }

    pub(super) fn timeline_seek(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let time = params
            .get("time")
            .and_then(Json::as_f64)
            .ok_or_else(|| bad_params("give time: seconds into the timeline"))?;
        let by_entity = json::opt_str(params, "timeline")?;
        let by_name = json::opt_str(params, "name")?
            .map(|n| n.strip_prefix("timeline:").unwrap_or(n).to_string());
        let e = match by_entity {
            Some(r) if r.starts_with("e:") => Some(
                r.parse::<Entity>()
                    .map_err(|m: String| ArgusError::new(argus_ecs::codes::BAD_ID, m))?,
            ),
            Some(r) => Some(self.world.entity(r)?),
            None => None,
        };
        if e.is_none() && by_name.is_none() {
            return Err(bad_params("give timeline (its entity) or name"));
        }
        // Where it began: the live one's own record, or the log's.
        let live = e.and_then(|e| {
            self.world
                .get::<Timeline>(e)
                .ok()
                .filter(|t| t.started)
                .map(|t| (e, t.began))
        });
        let (e, began) = match live.or_else(|| self.timeline_started(e, by_name.as_deref())) {
            Some(x) => x,
            None => {
                return Err(ArgusError::new(
                    codes::BAD_PARAMS,
                    match (&by_name, e) {
                        (Some(n), _) => format!("no timeline {n:?} has begun in this run's log"),
                        (None, Some(e)) => format!("{e} is no timeline that has begun in this run's log"),
                        _ => "no such timeline".to_string(),
                    },
                )
                .hint("Play it first (spawn a Timeline, run until timeline.started); a seek goes over frames already played."))
            }
        };
        // The tick before it began, to land on the first frame at the time;
        // but a timeline started between ticks (timeline.play, the editor's)
        // is in the keyframe its change made at that tick, which a replay
        // from before it would not make.
        let barrier = self
            .session
            .as_ref()
            .is_some_and(|s| s.ring.borrow().barrier_at(began));
        let start = if barrier {
            began
        } else {
            began.saturating_sub(1)
        };
        let mut seen = false;
        let mut stop = |w: &World| match w.get::<Timeline>(e) {
            Ok(t) if t.started => {
                seen = true;
                t.time as f64 >= time - EPS
            }
            Ok(_) => false,
            // Ended before it got there: the frame it ended.
            Err(_) => seen,
        };
        let mut out = self.seek_to(start, Some(&mut stop), false)?;
        out["timeline"] = json!(e.to_string());
        match self.world.get::<Timeline>(e) {
            Ok(t) => {
                out["time"] = json!(t.time);
                out["next"] = json!(t.next);
                out["held"] = json!(t.held);
                out["ended"] = json!(false);
            }
            Err(_) => {
                out["ended"] = json!(true);
            }
        }
        Ok(out)
    }
}
