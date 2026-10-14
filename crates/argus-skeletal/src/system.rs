//! `skeletal.pose`: every skeleton, every tick, after `anim.update`.

use crate::pose;
use crate::{Skeleton, SkeletonPart};
use argus_anim::{skeleton_frames, AnimStore, Animator};
use argus_assets::spine::{Animation, SkeletonAsset};
use argus_ecs::{
    ComponentId, Entity, FieldSchema, GlobalTransform, Parent, Result, StrId, Transform, Value,
    Vec2, World,
};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The skeleton system's name.
pub const SYSTEM: &str = "skeletal.pose";

/// Draw order between one slot and the next in front.
const Z_STEP: f32 = 0.001;

struct Ids {
    skeleton: ComponentId,
    part: ComponentId,
    animator: Option<ComponentId>,
    transform: ComponentId,
    global: ComponentId,
    parent: ComponentId,
    sprite: Option<ComponentId>,
}

/// The Sprite fields a part's drawing sets.
struct SpriteFields {
    atlas: FieldSchema,
    frame: FieldSchema,
    size: FieldSchema,
    color: FieldSchema,
    z: FieldSchema,
    visible: FieldSchema,
    material: FieldSchema,
}

#[derive(Default)]
struct State {
    generation: u64,
    built: bool,
    /// The world's frame last run: a world restored (a seek) or switched
    /// (a fork) takes its string table with it, so ids kept from before
    /// may name nothing, or something else.
    frame: Option<u64>,
    /// Interned strings, resolved.
    names: HashMap<StrId, String>,
    /// An animator's (machine, state) to its clip.
    clips: HashMap<(StrId, StrId), StrId>,
    /// Strings the parts are given, interned.
    ids: HashMap<String, StrId>,
    /// Packed colours as Sprite.color strings.
    colors: HashMap<u32, StrId>,
    reported: HashSet<(String, String)>,
}

/// A slot's sprite as this tick draws it.
struct Out {
    skeleton: Entity,
    slot: u32,
    image: StrId,
    pos: Vec2,
    rot: f32,
    scale: Vec2,
    size: Vec2,
    rgba: u32,
    z: f32,
    visible: bool,
    material: StrId,
}

type Events = Vec<(String, Vec<(String, Value)>)>;

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<AnimStore>>,
) -> Result<()> {
    if world.component_id_of::<Parent>().is_none() {
        argus_ecs::hierarchy::register(world)?;
    }
    let ids = Ids {
        skeleton: world.component_id_of::<Skeleton>().expect("registered"),
        part: world.component_id_of::<SkeletonPart>().expect("registered"),
        animator: world.component_id_of::<Animator>(),
        transform: world.component_id_of::<Transform>().expect("registered"),
        global: world
            .component_id_of::<GlobalTransform>()
            .expect("registered"),
        parent: world.component_id_of::<Parent>().expect("registered"),
        sprite: world.component_id("Sprite"),
    };
    let fields = ids.sprite.and_then(|s| {
        let sc = world.registry().schema(s);
        Some(SpriteFields {
            atlas: sc.field("atlas")?.clone(),
            frame: sc.field("frame")?.clone(),
            size: sc.field("size")?.clone(),
            color: sc.field("color")?.clone(),
            z: sc.field("z")?.clone(),
            visible: sc.field("visible")?.clone(),
            material: sc.field("material")?.clone(),
        })
    });
    let mut desc = SystemDesc::new(SYSTEM, Stage::Late)
        .on_load()
        .between_turns()
        .after(argus_anim::SYSTEM)
        .writes(ids.skeleton)
        .writes(ids.part)
        .writes(ids.transform)
        .writes(ids.global)
        .writes(ids.parent)
        .source("argus-skeletal:pose")
        .doc("Poses every Skeleton: the animation its Animator's state plays (at the animator's frame) or its own, crossfades, IK, skin and slot timelines; fires the animation's Spine events; spawns, places, and despawns the part entities whose Sprites draw its slots.");
    if let Some(a) = ids.animator {
        desc = desc.reads(a);
    }
    if let Some(s) = ids.sprite {
        desc = desc.writes(s);
    }
    let mut st = State::default();
    sched.add_system(
        desc,
        Box::new(move |ctx| run(ctx, &mut st, &store, &ids, fields.as_ref())),
    )
}

fn resolve<'n>(names: &'n mut HashMap<StrId, String>, world: &World, id: StrId) -> &'n str {
    names
        .entry(id)
        .or_insert_with(|| world.strings().resolve(id))
}

fn intern(ids: &mut HashMap<String, StrId>, world: &World, s: &str) -> Result<StrId> {
    if let Some(&id) = ids.get(s) {
        return Ok(id);
    }
    let id = world.strings().try_intern(s)?;
    ids.insert(s.to_string(), id);
    Ok(id)
}

fn pack(c: [f32; 4]) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (b(c[0]) << 24) | (b(c[1]) << 16) | (b(c[2]) << 8) | b(c[3])
}

fn fire(e: Entity, name: &str, anim: &Animation, after: f32, upto: f32, out: &mut Events) {
    for ev in pose::events_in(anim, after, upto) {
        out.push((
            ev.name.clone(),
            vec![
                ("entity".into(), Value::Entity(e)),
                ("animation".into(), Value::Str(name.to_string())),
                ("time".into(), Value::Float(ev.time as f64)),
                ("int".into(), Value::Int(ev.int)),
                ("float".into(), Value::Float(ev.float as f64)),
                ("string".into(), Value::Str(ev.string.clone())),
            ],
        ));
    }
}

fn run(
    ctx: &mut SystemCtx<'_>,
    st: &mut State,
    store: &Rc<RefCell<AnimStore>>,
    ids: &Ids,
    sf: Option<&SpriteFields>,
) -> Result<()> {
    if !ctx
        .world()
        .archetypes()
        .iter()
        .any(|a| !a.is_empty() && (a.has(ids.skeleton) || a.has(ids.part)))
    {
        return Ok(());
    }
    let dt = ctx.dt as f32;
    let store = store.borrow();
    let frame = ctx.world().frame();
    let jumped = st.frame.is_none_or(|f| frame != f + 1);
    if jumped || !st.built || st.generation != store.generation {
        st.names.clear();
        st.clips.clear();
        st.ids.clear();
        st.colors.clear();
        st.generation = store.generation;
        st.built = true;
    }
    st.frame = Some(frame);
    let State {
        names,
        clips,
        ids: interned,
        colors,
        reported,
        ..
    } = st;
    let mut outs: Vec<Out> = Vec::new();
    let mut events: Events = Vec::new();
    let mut errors: Vec<(String, String)> = Vec::new();
    let world: &World = ctx.world();

    // Pose every skeleton: what plays, where it is in it, its events, and
    // its slots placed.
    world.query().has(ids.skeleton).for_each_view(|view| {
        let arch = view.archetype();
        let mut sks = view.column_mut::<Skeleton>()?;
        let anims = match ids.animator.filter(|a| arch.has(*a)) {
            Some(_) => Some(view.column::<Animator>()?),
            None => None,
        };
        let entities = view.entities();
        for row in 0..view.len() {
            let e = entities[row];
            let sk = &mut sks[row];
            let name = resolve(names, world, sk.skeleton);
            let Some((sk_name, asset)) = store.skeletons.get_key_value(name) else {
                if sk.skeleton != StrId::EMPTY {
                    errors.push((
                        name.to_string(),
                        format!("there is no skeleton asset named {name}"),
                    ));
                }
                sk.parts = 0;
                continue;
            };
            let asset: &SkeletonAsset = asset;
            let skin = if sk.skin == StrId::EMPTY {
                "default"
            } else {
                let s = resolve(names, world, sk.skin);
                match asset.skins.get_key_value(s) {
                    Some((k, _)) => k.as_str(),
                    None => {
                        errors.push((sk_name.clone(), format!("{sk_name} has no skin {s}")));
                        "default"
                    }
                }
            };
            let animation_of = |names: &mut HashMap<StrId, String>, id: StrId| {
                if id == StrId::EMPTY {
                    return None;
                }
                asset.animations.get_key_value(resolve(names, world, id))
            };

            // What plays and where: the animator's state, or its own (a
            // time a script made no number starts it again).
            if !sk.time.is_finite() {
                sk.time = 0.0;
            }
            let prev;
            let entry;
            let mut wrapped = false;
            let target;
            if let Some(a) = anims.as_ref().map(|c| &c[row]) {
                let key = (a.machine, a.state);
                let clip = match clips.get(&key) {
                    Some(&c) => c,
                    None => {
                        let machine = resolve(names, world, a.machine).to_string();
                        let state = resolve(names, world, a.state).to_string();
                        let c = match store.machines.get(&machine).and_then(|m| m.state(&state)) {
                            Some(s) => intern(interned, world, &s.clip)?,
                            None => StrId::EMPTY,
                        };
                        clips.insert(key, c);
                        c
                    }
                };
                target = animation_of(names, clip);
                let dur = target.map_or(0.0, |(_, an)| an.duration);
                let n = skeleton_frames(dur, asset.fps) as f32;
                let frac = if a.frame_len > 0.0 {
                    a.phase / a.frame_len
                } else if dur > 0.0 {
                    a.phase * n / dur
                } else {
                    0.0
                };
                let t = ((a.frame as f32 + frac.clamp(0.0, 1.0)) / n * dur).clamp(0.0, dur);
                entry = clip != sk.shown || a.time < sk.state_time;
                wrapped = !entry && a.loops > sk.loops;
                if entry
                    && a.blend_total > 0.0
                    && a.blend_left >= a.blend_total - 1e-6
                    && sk.shown != StrId::EMPTY
                {
                    sk.from = sk.shown;
                    sk.from_time = sk.time;
                }
                sk.mix = if a.blend_total > 0.0 {
                    (a.blend_left / a.blend_total).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                prev = sk.time;
                sk.time = t;
                sk.state_time = a.time;
                sk.loops = a.loops;
                sk.ended = a.ended;
                sk.animation = clip;
                sk.shown = clip;
            } else {
                target = animation_of(names, sk.animation);
                if sk.animation != StrId::EMPTY && target.is_none() {
                    let a = resolve(names, world, sk.animation).to_string();
                    errors.push((sk_name.clone(), format!("{sk_name} has no animation {a}")));
                }
                entry = sk.animation != sk.shown;
                if entry {
                    if sk.blend > 0.0 && sk.shown != StrId::EMPTY {
                        sk.from = sk.shown;
                        sk.from_time = sk.time;
                        sk.mix = 1.0;
                    } else {
                        sk.mix = 0.0;
                    }
                    sk.time = 0.0;
                    sk.loops = 0;
                    sk.ended = false;
                    prev = 0.0;
                } else {
                    prev = sk.time;
                    let step = dt * sk.speed.max(0.0);
                    if sk.mix > 0.0 {
                        sk.mix = if sk.blend > 0.0 {
                            (sk.mix - step / sk.blend).max(0.0)
                        } else {
                            0.0
                        };
                    }
                    let dur = target.map_or(0.0, |(_, an)| an.duration);
                    if target.is_some() && !(sk.ended && !sk.looping) {
                        sk.time += step;
                        if sk.time >= dur {
                            sk.ended = true;
                            if sk.looping && dur > 0.0 {
                                let k = (sk.time / dur).floor();
                                sk.time -= k * dur;
                                sk.loops += k as u32;
                                wrapped = true;
                            } else {
                                sk.time = dur;
                            }
                        }
                    }
                }
                sk.state_time = 0.0;
                sk.shown = sk.animation;
            }
            if sk.mix <= 0.0 {
                sk.from = StrId::EMPTY;
                sk.from_time = 0.0;
            }

            // Its events: every key the animation passed this tick.
            if let Some((an_name, an)) = target {
                if entry {
                    fire(e, an_name, an, -1.0, sk.time, &mut events);
                } else if wrapped {
                    fire(e, an_name, an, prev, an.duration, &mut events);
                    fire(e, an_name, an, -1.0, sk.time, &mut events);
                } else {
                    fire(e, an_name, an, prev, sk.time, &mut events);
                }
            }

            // The pose, mixed with the one it leaves, placed.
            let mut p = pose::setup(asset);
            if let Some((_, an)) = target {
                pose::apply(asset, an, sk.time, &mut p);
            }
            if sk.mix > 0.0 {
                if let Some((_, fa)) = animation_of(names, sk.from) {
                    let mut q = pose::setup(asset);
                    pose::apply(asset, fa, sk.from_time, &mut q);
                    pose::mix(&mut p, &q, sk.mix);
                }
            }
            let s = sk.scale;
            let mats = pose::world(asset, &p, sk.flip_x);
            for pl in pose::place_slots(asset, &p, &mats, skin) {
                let image = match pl.image {
                    Some(img) => intern(interned, world, img)?,
                    None => StrId::EMPTY,
                };
                let rgba = pack(pl.color);
                if rgba != 0xffff_ffff && !colors.contains_key(&rgba) {
                    let id = world.strings().try_intern(&format!("#{rgba:08x}"))?;
                    colors.insert(rgba, id);
                }
                outs.push(Out {
                    skeleton: e,
                    slot: pl.slot as u32,
                    image,
                    pos: Vec2::new(pl.pos[0] * s, pl.pos[1] * s),
                    rot: pl.rot,
                    scale: Vec2::new(pl.scale[0] * s, pl.scale[1] * s),
                    size: Vec2::new(pl.size[0], pl.size[1]),
                    rgba,
                    z: sk.z + pl.order as f32 * Z_STEP,
                    visible: sk.visible && pl.image.is_some(),
                    material: sk.material,
                });
            }
            sk.parts = asset.slots.len() as u32;
        }
        Ok(())
    })?;

    // Which parts are still wanted; the rest go, and the missing come.
    let index: HashMap<(Entity, u32), usize> = outs
        .iter()
        .enumerate()
        .map(|(i, o)| ((o.skeleton, o.slot), i))
        .collect();
    let mut claimed = vec![false; outs.len()];
    let mut extra: Vec<Entity> = Vec::new();
    world.query().has(ids.part).for_each_view(|view| {
        let parts = view.column::<SkeletonPart>()?;
        for (row, &e) in view.entities().iter().enumerate() {
            match index.get(&(parts[row].skeleton, parts[row].slot)) {
                Some(&i) if !claimed[i] => claimed[i] = true,
                _ => extra.push(e),
            }
        }
        Ok(())
    })?;
    for e in extra {
        ctx.despawn(e)?;
    }
    let mut comps = vec![ids.transform, ids.global, ids.parent, ids.part];
    comps.extend(ids.sprite);
    for (i, o) in outs.iter().enumerate() {
        if claimed[i] {
            continue;
        }
        let pe = ctx.spawn_with(&comps)?;
        let w = ctx.world();
        w.update::<Parent, _>(pe, |p| p.entity = o.skeleton)?;
        w.update::<SkeletonPart, _>(pe, |p| {
            p.skeleton = o.skeleton;
            p.slot = o.slot;
            p.rgba = 0xffff_ffff;
        })?;
    }

    // Every part placed and drawn as its slot is this tick.
    let world: &World = ctx.world();
    world.query().has(ids.part).for_each_view(|view| {
        let arch = view.archetype();
        let mut parts = view.column_mut::<SkeletonPart>()?;
        let mut ts = if arch.has(ids.transform) {
            Some(view.column_mut::<Transform>()?)
        } else {
            None
        };
        let sprites = match ids.sprite.filter(|s| arch.has(*s)) {
            Some(s) => Some(view.dyn_column_mut(s)?),
            None => None,
        };
        for row in 0..view.len() {
            let Some(&i) = index.get(&(parts[row].skeleton, parts[row].slot)) else {
                continue;
            };
            let o = &outs[i];
            if let Some(ts) = ts.as_mut() {
                ts[row] = Transform {
                    pos: o.pos,
                    rot: o.rot,
                    scale: o.scale,
                };
            }
            let (Some(sc), Some(f)) = (&sprites, sf) else {
                continue;
            };
            if sc.str_id_by(row, &f.atlas) != Some(o.image) {
                sc.set_str_id_by(row, &f.atlas, o.image)?;
                sc.set_by(row, &f.frame, Value::UInt(0))?;
            }
            if sc.str_id_by(row, &f.material) != Some(o.material) {
                sc.set_str_id_by(row, &f.material, o.material)?;
            }
            if parts[row].rgba != o.rgba {
                let id = colors.get(&o.rgba).copied().unwrap_or(StrId::EMPTY);
                sc.set_str_id_by(row, &f.color, id)?;
                parts[row].rgba = o.rgba;
            }
            sc.set_by(row, &f.size, Value::Vec2(o.size))?;
            sc.set_by(row, &f.z, Value::Float(o.z as f64))?;
            sc.set_by(row, &f.visible, Value::Bool(o.visible))?;
        }
        Ok(())
    })?;
    drop(store);

    for (skeleton, message) in errors {
        if reported.insert((skeleton.clone(), message.clone())) {
            events.push((
                "skeletal.error".into(),
                vec![
                    ("skeleton".into(), Value::Str(skeleton)),
                    ("message".into(), Value::Str(message)),
                ],
            ));
        }
    }
    for (n, f) in events {
        ctx.emit(n, f);
    }
    Ok(())
}
