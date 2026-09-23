//! Machines compiled against one world: field paths resolved to columns,
//! state names interned, clips bound per sprite.

use crate::{AnimStore, SpriteClips};
use argus_assets::sidecar::Direction;
use argus_assets::{AnimatorSpec, Num, Op, Operand, TransitionSpec};
use argus_ecs::{ComponentId, FieldSchema, FieldType, Registry, StrId, World};
use std::collections::HashMap;

/// A field read each tick: which column, which field, which axis.
#[derive(Clone, Debug)]
pub(crate) struct FieldRef {
    /// Index into `Compiled::used`.
    pub slot: usize,
    pub field: FieldSchema,
    pub axis: Option<u8>,
}

#[derive(Clone, Debug)]
pub(crate) enum CNum {
    Value(f32),
    Field(FieldRef),
}

#[derive(Clone, Debug)]
pub(crate) enum COperand {
    Field(FieldRef, bool),
    End,
    Time,
    Frame,
    Loops,
}

#[derive(Clone, Debug)]
pub(crate) struct CTerm {
    pub not: bool,
    pub what: COperand,
    pub cmp: Option<(Op, f32)>,
}

#[derive(Clone, Debug)]
pub(crate) struct CTrans {
    pub to: usize,
    pub terms: Vec<CTerm>,
    pub window: Option<[u32; 2]>,
    pub blend: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct CEvent {
    pub frame: u32,
    pub name: String,
    pub sound: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CState {
    pub name: String,
    pub id: StrId,
    pub clip: String,
    pub looping: Option<bool>,
    pub fps: Option<CNum>,
    pub windup: u32,
    pub anticipation: Option<CNum>,
    pub hold: Option<CNum>,
    pub hold_frame: Option<u32>,
    pub events: Vec<CEvent>,
    pub trans: Vec<CTrans>,
}

#[derive(Clone, Debug)]
pub(crate) struct Machine {
    pub name: String,
    pub initial: usize,
    pub states: Vec<CState>,
    pub any: Vec<CTrans>,
    pub by_id: HashMap<StrId, usize>,
}

/// One state's clip for one sprite: atlas frames in play order and their
/// base durations in seconds.
#[derive(Clone, Debug)]
pub(crate) struct BState {
    pub frames: Vec<u32>,
    pub durs: Vec<f32>,
    pub looping: bool,
}

/// A machine bound to a sprite.
#[derive(Clone, Debug)]
pub(crate) struct Bound {
    pub states: Vec<BState>,
}

/// Every machine for this world, rebuilt when the store changes.
#[derive(Default)]
pub(crate) struct Compiled {
    pub generation: u64,
    pub machines: Vec<Machine>,
    pub by_name: HashMap<StrId, usize>,
    /// Components some condition or number reads.
    pub used: Vec<ComponentId>,
    pub bound: HashMap<(usize, StrId), Option<Bound>>,
    /// Problems to report once each: machine, message, at.
    pub errors: Vec<(String, String, String)>,
}

fn resolve(
    reg: &Registry,
    path: &str,
    used: &mut Vec<ComponentId>,
    forbid: &[Option<ComponentId>],
) -> Result<FieldRef, String> {
    let mut parts = path.split('.');
    let (c, f) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let axis = match parts.next() {
        Some("x") => Some(0),
        Some("y") => Some(1),
        Some(o) => return Err(format!("{path}: .{o} is not x or y")),
        None => None,
    };
    let id = reg
        .id(c)
        .ok_or_else(|| format!("{path}: there is no component {c:?}"))?;
    if forbid.contains(&Some(id)) {
        return Err(format!(
            "{path}: conditions read other components; for the state's own clock use time, frame, loops, or end"
        ));
    }
    let schema = reg.schema(id);
    let field = schema.field(f).cloned().ok_or_else(|| {
        let names: Vec<&str> = schema.fields.iter().map(|x| x.name.as_str()).collect();
        format!(
            "{path}: {c} has no field {f:?}; it has {}",
            names.join(", ")
        )
    })?;
    let numeric = !matches!(
        field.ty,
        FieldType::Str | FieldType::Ref | FieldType::Entity | FieldType::Vec2
    );
    match (field.ty, axis) {
        (FieldType::Vec2, None) => return Err(format!("{path} is a vec2; compare .x or .y")),
        (FieldType::Vec2, Some(_)) => {}
        (_, Some(_)) => return Err(format!("{path}: only a vec2 has .x and .y")),
        _ if !numeric => {
            return Err(format!(
                "{path} is a {} field; conditions compare numbers and bools",
                field.ty.name()
            ))
        }
        _ => {}
    }
    let slot = match used.iter().position(|&u| u == id) {
        Some(s) => s,
        None => {
            used.push(id);
            used.len() - 1
        }
    };
    Ok(FieldRef { slot, field, axis })
}

fn num(
    reg: &Registry,
    n: &Option<Num>,
    used: &mut Vec<ComponentId>,
    forbid: &[Option<ComponentId>],
) -> Result<Option<CNum>, String> {
    Ok(match n {
        None => None,
        Some(Num::Value(v)) => Some(CNum::Value(*v)),
        Some(Num::Field(p)) => Some(CNum::Field(resolve(reg, p, used, forbid)?)),
    })
}

fn trans(
    reg: &Registry,
    spec: &AnimatorSpec,
    t: &TransitionSpec,
    used: &mut Vec<ComponentId>,
    forbid: &[Option<ComponentId>],
) -> Result<CTrans, String> {
    let to = spec
        .states
        .iter()
        .position(|s| s.name == t.to)
        .ok_or_else(|| format!("to = {:?} is not a state", t.to))?;
    let mut terms = Vec::new();
    for term in &t.when.terms {
        let what = match &term.what {
            Operand::Field { path, abs } => {
                COperand::Field(resolve(reg, path, used, forbid)?, *abs)
            }
            Operand::End => COperand::End,
            Operand::Time => COperand::Time,
            Operand::Frame => COperand::Frame,
            Operand::Loops => COperand::Loops,
        };
        terms.push(CTerm {
            not: term.not,
            what,
            cmp: term.cmp,
        });
    }
    Ok(CTrans {
        to,
        terms,
        window: t.window,
        blend: t.blend,
    })
}

impl Compiled {
    /// Compile every machine in the store against the world. A machine
    /// that names something the world does not have is left out and
    /// reported.
    pub fn build(store: &AnimStore, world: &World, forbid: &[Option<ComponentId>]) -> Compiled {
        let reg = world.registry();
        let mut c = Compiled {
            generation: store.generation,
            ..Default::default()
        };
        for (name, spec) in &store.machines {
            let mut used = c.used.clone();
            let mut compile = || -> Result<Machine, (String, String)> {
                let mut states = Vec::new();
                for s in &spec.states {
                    let mut ts = Vec::new();
                    for t in &s.transitions {
                        ts.push(
                            trans(reg, spec, t, &mut used, forbid)
                                .map_err(|m| (m, t.at.clone()))?,
                        );
                    }
                    states.push(CState {
                        name: s.name.clone(),
                        id: world.strings().intern(&s.name),
                        clip: s.clip.clone(),
                        looping: s.looping,
                        fps: num(reg, &s.fps, &mut used, forbid).map_err(|m| (m, s.at.clone()))?,
                        windup: s.windup,
                        anticipation: num(reg, &s.anticipation, &mut used, forbid)
                            .map_err(|m| (m, s.at.clone()))?,
                        hold: num(reg, &s.hold, &mut used, forbid)
                            .map_err(|m| (m, s.at.clone()))?,
                        hold_frame: s.hold_frame,
                        events: s
                            .events
                            .iter()
                            .map(|e| CEvent {
                                frame: e.frame,
                                name: e.name.clone(),
                                sound: e.sound.clone(),
                            })
                            .collect(),
                        trans: ts,
                    });
                }
                let mut any = Vec::new();
                for t in &spec.any {
                    any.push(
                        trans(reg, spec, t, &mut used, forbid).map_err(|m| (m, t.at.clone()))?,
                    );
                }
                let initial = spec
                    .states
                    .iter()
                    .position(|s| s.name == spec.initial)
                    .unwrap_or(0);
                let by_id = states.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
                Ok(Machine {
                    name: name.clone(),
                    initial,
                    states,
                    any,
                    by_id,
                })
            };
            match compile() {
                Ok(m) => {
                    c.used = used;
                    c.by_name
                        .insert(world.strings().intern(name), c.machines.len());
                    c.machines.push(m);
                }
                Err((msg, at)) => c.errors.push((name.clone(), msg, at)),
            }
        }
        c
    }

    /// Bind machine `mi` to a sprite's clips, once. `None` (reported) when a
    /// clip is missing.
    pub fn bind(
        &mut self,
        mi: usize,
        atlas: StrId,
        world: &World,
        store: &AnimStore,
    ) -> Option<&Bound> {
        if !self.bound.contains_key(&(mi, atlas)) {
            let m = &self.machines[mi];
            let sprite_name = world.strings().resolve(atlas);
            let result = match store.sprites.get(&sprite_name) {
                None => {
                    self.errors.push((
                        m.name.clone(),
                        format!("sprite {sprite_name:?} is not an imported sprite asset, so its clips cannot play"),
                        String::new(),
                    ));
                    None
                }
                Some(sc) => bind_states(m, sc)
                    .map_err(|msg| {
                        self.errors.push((
                            m.name.clone(),
                            format!("{msg} (sprite {sprite_name})"),
                            String::new(),
                        ))
                    })
                    .ok(),
            };
            self.bound.insert((mi, atlas), result);
        }
        self.bound.get(&(mi, atlas)).and_then(|b| b.as_ref())
    }
}

fn bind_states(m: &Machine, sc: &SpriteClips) -> Result<Bound, String> {
    let mut states = Vec::new();
    for s in &m.states {
        let clip = sc.clips.get(&s.clip).ok_or_else(|| {
            let names: Vec<&str> = sc.clips.keys().map(String::as_str).collect();
            format!(
                "state {} plays clip {:?}, which the sprite does not have; it has {}",
                s.name,
                s.clip,
                names.join(", ")
            )
        })?;
        let mut frames = clip.frames.clone();
        match clip.direction {
            Direction::Forward => {}
            Direction::Reverse => frames.reverse(),
            Direction::PingPong => {
                let back: Vec<u32> = frames
                    .iter()
                    .rev()
                    .skip(1)
                    .take(frames.len().saturating_sub(2))
                    .copied()
                    .collect();
                frames.extend(back);
            }
        }
        if frames.is_empty() {
            return Err(format!("clip {:?} has no frames", s.clip));
        }
        let durs = frames
            .iter()
            .map(|&f| match clip.fps {
                Some(fps) if fps > 0.0 => 1.0 / fps,
                _ => sc.frame_ms.get(f as usize).copied().unwrap_or(100).max(1) as f32 / 1000.0,
            })
            .collect();
        states.push(BState {
            frames,
            durs,
            looping: s.looping.unwrap_or(clip.looping),
        });
    }
    Ok(Bound { states })
}
