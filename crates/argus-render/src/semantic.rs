//! The semantic frame (§8 L4): what the player sees, as data, in about
//! 3 KB. The camera's rect; every visible entity with its bounds in the
//! frame's pixels, the notable ones one by one and the rest in groups;
//! HUD text; the effects on screen. A [`Describe`] on an entity is how a
//! game says what matters about it: a name, a role, and a text whose
//! `{Component.field}` holes are filled in each frame. The same frame is a
//! screen-reader feed (§12): its lines come in reading order, HUD first.
//!
//! Nothing here touches the GPU, so a machine with no adapter builds it.

use crate::Sprite;
use argus_ecs::{Component, ComponentId, Entity, GlobalTransform, StrId, Transform, Value, World};
use argus_particles::ParticleStore;
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// What an entity says about itself in the semantic frame.
#[derive(Component, Clone, Copy, Debug, PartialEq, Default)]
#[repr(C)]
#[argus(
    doc = "What the semantic frame (obs.frame format semantic), and so a screen reader, says about this entity: a name, a role, and a text with {Component.field} filled in each frame"
)]
pub struct Describe {
    /// What it is, in a word or two.
    #[argus(doc = "What it is in a word or two (goblin, health bar); empty for its id")]
    pub name: StrId,
    /// What it says.
    #[argus(
        doc = "What it says: {Component.field} is this entity's field, {@id:Component.field} a named entity's, filled in each frame"
    )]
    pub text: StrId,
    /// hud, dialogue, menu, or empty for a thing in the world.
    #[argus(
        doc = "hud (on the screen, listed first), dialogue, menu, or empty for a thing in the world; an entity with no Transform is on the screen whatever its role"
    )]
    pub role: StrId,
    /// Reading order.
    #[argus(doc = "Reading order: lower first, then top to bottom, left to right")]
    pub order: i32,
    /// Leave it out.
    #[argus(doc = "Leave it out of the frame")]
    pub hidden: bool,
    /// Draw the text for the player too.
    #[argus(
        doc = "Draw the text in the game too, in the pixel font: over the entity, or at the top of the screen for the HUD (until the UI tree, phase 3)"
    )]
    pub shown: bool,
}

/// What to build a frame for.
pub struct FrameSpec<'a> {
    /// Pixel size the bounds are in (the PNG's, if one is drawn).
    pub width: u32,
    /// Pixel size.
    pub height: u32,
    /// Entities listed one by one; the rest are grouped.
    pub limit: usize,
    /// Groups listed; the rest are counted in `omitted`.
    pub groups: usize,
    /// For sprite pivots.
    pub assets: Option<&'a argus_assets::Importer>,
    /// For the particles on screen.
    pub particles: Option<&'a ParticleStore>,
    /// The view to use (a drawn frame's), else the drawing camera's now.
    pub view: Option<View>,
}

/// A view: centre, half extents in world units, roll in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Centre.
    pub center: [f32; 2],
    /// Half extents.
    pub half: [f32; 2],
    /// Roll.
    pub angle: f32,
}

/// One entity the frame lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    /// The entity.
    pub entity: Entity,
    /// Its protocol spelling: `@player` or `e:12`.
    pub id: String,
    /// Its `Describe.name`.
    pub name: String,
    /// Its `Describe.role`.
    pub role: String,
    /// Its `Describe.text`, filled in.
    pub text: String,
    /// Pixel bounds in the frame: x0, y0, x1, y1 (y down), clipped to it.
    pub bounds: [i32; 4],
    /// Draw order.
    pub z: f32,
    /// How it is drawn: `sprites/hero#3`, `circle #E8703F`, `+material`.
    pub look: String,
    /// `Describe.order`.
    pub order: i32,
}

/// Entities of one kind, counted.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// What they are: their game components (`Enemy+Health`), else their name or look.
    pub what: String,
    /// How many are on screen.
    pub count: usize,
    /// Pixel bounds around all of them.
    pub bounds: [i32; 4],
    /// How the first is drawn.
    pub look: String,
    /// The first few, by id.
    pub ids: Vec<String>,
}

/// What the player sees, as data.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticFrame {
    /// Pixel size.
    pub size: [u32; 2],
    /// The camera that draws, by id.
    pub camera: Option<String>,
    /// The view.
    pub view: View,
    /// The world rect it shows (its bounding box when rolled).
    pub rect: [f32; 4],
    /// What is on the screen, not in the world, in reading order.
    pub hud: Vec<Seen>,
    /// The notable entities (described or named), in reading order.
    pub entities: Vec<Seen>,
    /// Everything else, by kind, most first.
    pub groups: Vec<Group>,
    /// Shake, particles, and materials on screen.
    pub effects: Vec<Json>,
    /// Entities and groups past the limits.
    pub omitted: usize,
    /// Text holes that named nothing.
    pub problems: Vec<String>,
}

impl SemanticFrame {
    /// As the protocol returns it: entities as rows under columns, to keep
    /// it near 3 KB.
    pub fn to_json(&self) -> Json {
        let row = |s: &Seen| json!([s.id, s.name, s.text, s.bounds, round(s.z), s.look]);
        let line = |s: &Seen| {
            let mut o = json!({ "id": s.id, "text": s.text });
            if !s.name.is_empty() {
                o["name"] = json!(s.name);
            }
            if !s.role.is_empty() {
                o["role"] = json!(s.role);
            }
            if s.bounds != [0; 4] {
                o["bounds"] = json!(s.bounds);
            }
            o
        };
        json!({
            "format": "semantic",
            "size": self.size,
            "camera": {
                "id": self.camera,
                "rect": self.rect.map(round),
                "angle": round(self.view.angle),
            },
            "hud": self.hud.iter().map(line).collect::<Vec<_>>(),
            "columns": ["id", "name", "text", "bounds", "z", "look"],
            "entities": self.entities.iter().map(row).collect::<Vec<_>>(),
            "groups": self.groups.iter().map(|g| json!({
                "what": g.what, "count": g.count, "bounds": g.bounds, "look": g.look, "ids": g.ids,
            })).collect::<Vec<_>>(),
            "effects": self.effects,
            "omitted": self.omitted,
            "problems": self.problems,
        })
    }
}

fn round(x: f32) -> f64 {
    ((x as f64) * 100.0).round() / 100.0
}

/// The protocol's spelling of an entity: `@id` for a named one, else `e:N`.
pub fn entity_id(world: &World, e: Entity) -> String {
    match world.string_id(e) {
        Some(id) => format!("@{id}"),
        None => e.to_string(),
    }
}

/// A value as text: whole numbers bare, others to two places, vectors as
/// `(x, y)`, entities by id.
fn show(world: &World, v: &Value) -> String {
    let num = |f: f64| {
        if f.fract() == 0.0 && f.abs() < 1e15 {
            format!("{}", f as i64)
        } else {
            let s = format!("{f:.2}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    };
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => num(*f),
        Value::Vec2(p) => format!("({}, {})", num(p.x as f64), num(p.y as f64)),
        Value::Str(s) | Value::Ref(s) => s.clone(),
        Value::Entity(e) => entity_id(world, *e),
    }
}

/// The drawing camera's post effects doing something (phase 2 item 6).
fn post_effects(world: &World, cam: Entity, height: u32, out: &mut Vec<Json>) {
    use argus_post::*;
    let c = entity_id(world, cam);
    let color = |id: argus_ecs::StrId, default: &str| {
        let s = world.strings().resolve(id);
        if s.is_empty() {
            default.to_string()
        } else {
            s
        }
    };
    if let Ok(s) = world.get::<PostShake>(cam) {
        if s.level > 0.0 {
            let px = |v: f32| (v * height as f32).round() + 0.0;
            out.push(json!({ "kind": "screen shake", "camera": c, "trauma": round(s.level), "offset_px": [px(s.offset.x), px(s.offset.y)] }));
        }
    }
    if let Ok(h) = world.get::<PostHitStop>(cam) {
        if h.left > 0 {
            out.push(json!({ "kind": "hit-stop", "camera": c, "ticks_left": h.left, "scale": round(h.scale) }));
        }
    }
    if let Ok(f) = world.get::<PostFlash>(cam) {
        if f.amount > 0.0 {
            out.push(json!({ "kind": "flash", "camera": c, "amount": round(f.amount), "color": color(f.color, "#FFFFFF") }));
        }
    }
    if let Ok(f) = world.get::<PostFade>(cam) {
        if f.amount > 0.0 {
            out.push(json!({ "kind": "fade", "camera": c, "amount": round(f.amount), "color": color(f.color, "#000000") }));
        }
    }
    if let Ok(v) = world.get::<PostVignette>(cam) {
        if v.amount > 0.0 {
            out.push(json!({ "kind": "vignette", "camera": c, "amount": round(v.amount) }));
        }
    }
    if let Ok(a) = world.get::<PostAberration>(cam) {
        if a.amount > 0.0 {
            out.push(json!({ "kind": "aberration", "camera": c, "amount": round(a.amount) }));
        }
    }
    if let Ok(p) = world.get::<PostPalette>(cam) {
        if p.amount > 0.0 {
            out.push(json!({ "kind": "palette", "camera": c, "amount": round(p.amount), "colors": color(p.colors, DEFAULT_PALETTE) }));
        }
    }
}

/// Fill a text's `{Component.field}` and `{@id:Component.field}` holes. A
/// hole that names nothing reads `{?...}` and is reported.
pub fn fill(world: &World, e: Entity, text: &str, problems: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let hole = &rest[i + 1..i + j];
        let (who, path) = match hole.strip_prefix('@').and_then(|h| h.split_once(':')) {
            Some((id, p)) => (world.entity(&format!("@{id}")).ok(), p),
            None => (Some(e), hole),
        };
        let got = who.ok_or_else(|| format!("no entity {}", hole.split(':').next().unwrap_or("")));
        match got.and_then(|w| world.get_field(w, path).map_err(|e| e.message)) {
            Ok(v) => out.push_str(&show(world, &v)),
            Err(why) => {
                out.push_str(&format!("{{?{hole}}}"));
                let p = format!("{}: {{{hole}}}: {why}", entity_id(world, e));
                if !problems.contains(&p) {
                    problems.push(p);
                }
            }
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

const TEXT_CAP: usize = 200;

fn cap(mut s: String) -> String {
    if s.chars().count() > TEXT_CAP {
        s = s.chars().take(TEXT_CAP - 1).collect();
        s.push('…');
    }
    s
}

/// A candidate: an entity drawn, or described, in view.
struct Cand {
    e: Entity,
    bounds: [i32; 4],
    z: f32,
    /// Shared: each distinct look is built once, not once an entity.
    look: Rc<str>,
}

/// Build the frame.
pub fn semantic(world: &World, spec: &FrameSpec) -> SemanticFrame {
    let (w, h) = (spec.width.max(1) as f32, spec.height.max(1) as f32);
    let aspect = w / h;
    let mut camera = None;
    let view = match spec.view {
        Some(v) => {
            camera = argus_camera::current(world).map(|c| entity_id(world, c));
            v
        }
        None => match argus_camera::view(world, aspect) {
            Some(v) => {
                camera = Some(entity_id(world, v.camera));
                View {
                    center: v.center,
                    half: v.half,
                    angle: v.angle,
                }
            }
            None => View {
                center: [0.0, 0.0],
                half: [20.0 * aspect, 20.0],
                angle: 0.0,
            },
        },
    };
    let (sn, cs) = view.angle.sin_cos();
    let to_px = |x: f32, y: f32| {
        let (dx, dy) = (x - view.center[0], y - view.center[1]);
        let (vx, vy) = (dx * cs + dy * sn, -dx * sn + dy * cs);
        (
            vx / (2.0 * view.half[0]) * w + w / 2.0,
            h / 2.0 - vy / (2.0 * view.half[1]) * h,
        )
    };
    let clip = |b: [f32; 4]| -> Option<[i32; 4]> {
        if b[2] < 0.0 || b[3] < 0.0 || b[0] > w || b[1] > h {
            return None;
        }
        Some([
            b[0].max(0.0).floor() as i32,
            b[1].max(0.0).floor() as i32,
            b[2].min(w).ceil() as i32,
            b[3].min(h).ceil() as i32,
        ])
    };
    let (rs, rc) = (sn.abs(), cs.abs());
    let bx = [
        rc * view.half[0] + rs * view.half[1],
        rs * view.half[0] + rc * view.half[1],
    ];
    let rect = [
        view.center[0] - bx[0],
        view.center[1] - bx[1],
        view.center[0] + bx[0],
        view.center[1] + bx[1],
    ];
    let strings = world.strings();

    // Every visible sprite on screen.
    let mut cands: Vec<Cand> = Vec::new();
    let mut at: HashMap<Entity, usize> = HashMap::new();
    let pivot_of = |atlas: StrId, frame: u32| -> [f32; 2] {
        if atlas == StrId::EMPTY {
            return [0.5, 0.5];
        }
        let name = strings.resolve(atlas);
        spec.assets
            .and_then(|a| a.sprites().find(|(n, _)| **n == name).map(|(_, s)| s))
            .and_then(|s| {
                s.frames
                    .get(frame as usize)
                    .or(s.frames.first())
                    .map(|f| f.pivot)
            })
            .unwrap_or([0.5, 0.5])
    };
    let mut pivots: HashMap<(StrId, u32), [f32; 2]> = HashMap::new();
    let mut looks: HashMap<(StrId, u32, StrId, StrId, StrId), Rc<str>> = HashMap::new();
    let mut add =
        |e: Entity, pos: [f32; 2], rot: f32, scale: [f32; 2], s: &Sprite, cands: &mut Vec<Cand>| {
            if !s.visible {
                return;
            }
            let p = *pivots
                .entry((s.atlas, s.frame))
                .or_insert_with(|| pivot_of(s.atlas, s.frame));
            let (sw, sh) = (s.size.x * scale[0], s.size.y * scale[1]);
            let (rsn, rcs) = rot.sin_cos();
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let (lx, ly) = ((u - p[0]) * sw, (v - p[1]) * sh);
                let (x, y) = to_px(pos[0] + lx * rcs - ly * rsn, pos[1] + lx * rsn + ly * rcs);
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
            let Some(bounds) = clip(b) else { return };
            let key = (s.atlas, s.frame, s.kind, s.color, s.material);
            let look = looks
                .entry(key)
                .or_insert_with(|| {
                    let mut look = if s.atlas == StrId::EMPTY {
                        let kind = strings.resolve(s.kind);
                        let kind = if kind.is_empty() {
                            "rect".to_string()
                        } else {
                            kind
                        };
                        let color = strings.resolve(s.color);
                        if color.is_empty() {
                            kind
                        } else {
                            format!("{kind} {color}")
                        }
                    } else {
                        format!("{}#{}", strings.resolve(s.atlas), s.frame)
                    };
                    if s.material != StrId::EMPTY {
                        look.push_str(&format!(" +{}", strings.resolve(s.material)));
                    }
                    Rc::from(look)
                })
                .clone();
            cands.push(Cand {
                e,
                bounds,
                z: s.z,
                look,
            });
        };
    let sp = world.component_id_of::<Sprite>();
    let tr = world.component_id_of::<Transform>();
    let gt = world.component_id_of::<GlobalTransform>();
    if let (Some(sp), Some(tr)) = (sp, tr) {
        let mut q = world.query().has(sp).has(tr);
        if let Some(gt) = gt {
            q = q.not(gt);
        }
        let _ = q.each::<(&Transform, &Sprite)>(|e, (t, s)| {
            add(
                e,
                [t.pos.x, t.pos.y],
                t.rot,
                [t.scale.x, t.scale.y],
                s,
                &mut cands,
            )
        });
        if let Some(gt) = gt {
            let _ = world
                .query()
                .has(sp)
                .has(gt)
                .each::<(&GlobalTransform, &Sprite)>(|e, (g, s)| {
                    add(
                        e,
                        [g.pos.x, g.pos.y],
                        g.rot,
                        [g.scale.x, g.scale.y],
                        s,
                        &mut cands,
                    )
                });
        }
    }
    for (i, c) in cands.iter().enumerate() {
        at.insert(c.e, i);
    }

    // Described entities: on the screen (HUD), or in the world at a point.
    let mut problems = Vec::new();
    let mut hud: Vec<Seen> = Vec::new();
    let mut described: HashMap<Entity, Describe> = HashMap::new();
    if let Some(dc) = world.component_id_of::<Describe>() {
        let mut rows: Vec<(Entity, Describe)> = Vec::new();
        let _ = world
            .query()
            .has(dc)
            .each::<&Describe>(|e, d| rows.push((e, *d)));
        for (e, d) in rows {
            if d.hidden {
                continue;
            }
            let role = strings.resolve(d.role);
            let pos = world
                .get::<GlobalTransform>(e)
                .map(|g| [g.pos.x, g.pos.y])
                .or_else(|_| world.get::<Transform>(e).map(|t| [t.pos.x, t.pos.y]))
                .ok();
            let on_screen = role == "hud" || pos.is_none();
            if on_screen {
                let bounds = at.get(&e).map(|&i| cands[i].bounds).unwrap_or([0; 4]);
                hud.push(Seen {
                    entity: e,
                    id: entity_id(world, e),
                    name: strings.resolve(d.name),
                    role,
                    text: cap(fill(world, e, &strings.resolve(d.text), &mut problems)),
                    bounds,
                    z: 0.0,
                    look: String::new(),
                    order: d.order,
                });
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(slot) = at.entry(e) {
                let p = pos.expect("in the world");
                let (x, y) = to_px(p[0], p[1]);
                if let Some(bounds) = clip([x, y, x, y]) {
                    slot.insert(cands.len());
                    cands.push(Cand {
                        e,
                        bounds,
                        z: 0.0,
                        look: Rc::from(""),
                    });
                }
            }
            described.insert(e, d);
        }
    }
    let read = |a: &Seen, b: &Seen| {
        (a.order, a.bounds[1], a.bounds[0], a.id.clone()).cmp(&(
            b.order,
            b.bounds[1],
            b.bounds[0],
            b.id.clone(),
        ))
    };
    hud.sort_by(read);

    // One by one: the described, and anything that is the only one of its
    // kind on screen (the player, a boss). Everything else by kind: an id
    // says an entity is stable, not that it matters, and every entity a
    // scene file places has one.
    // Kinds are labelled once an archetype, looked up without copying it.
    let mut label_of: HashMap<Vec<ComponentId>, Rc<str>> = HashMap::new();
    let mut label = |e: Entity, look: &Rc<str>| -> Rc<str> {
        let comps: &[ComponentId] = world.components_of(e).unwrap_or(&[]);
        let l = match label_of.get(comps) {
            Some(l) => l.clone(),
            None => {
                let l: Rc<str> = Rc::from(
                    comps
                        .iter()
                        .map(|&c| world.registry().schema(c))
                        .filter(|s| s.rust_type.is_none())
                        .map(|s| s.name.clone())
                        .collect::<Vec<_>>()
                        .join("+"),
                );
                label_of.insert(comps.to_vec(), l.clone());
                l
            }
        };
        if l.is_empty() {
            look.clone()
        } else {
            l
        }
    };
    let seen = |c: &Cand, d: Describe, problems: &mut Vec<String>| Seen {
        entity: c.e,
        id: entity_id(world, c.e),
        name: strings.resolve(d.name),
        role: strings.resolve(d.role),
        text: cap(fill(world, c.e, &strings.resolve(d.text), problems)),
        bounds: c.bounds,
        z: c.z,
        look: c.look.to_string(),
        order: d.order,
    };
    let mut notable: Vec<Seen> = Vec::new();
    let mut by_kind: HashMap<Rc<str>, Vec<usize>> = HashMap::new();
    for (i, c) in cands.iter().enumerate() {
        if hud.iter().any(|s| s.entity == c.e) {
            continue;
        }
        match described.get(&c.e) {
            Some(d) => notable.push(seen(c, *d, &mut problems)),
            None => by_kind.entry(label(c.e, &c.look)).or_default().push(i),
        }
    }
    let mut kinds: BTreeMap<Rc<str>, Vec<usize>> = BTreeMap::new();
    for (what, members) in by_kind {
        if members.len() == 1 {
            notable.push(seen(&cands[members[0]], Describe::default(), &mut problems));
        } else {
            kinds.insert(what, members);
        }
    }
    notable.sort_by(read);
    // Past the limit, the rest join their kind.
    for s in notable
        .drain(spec.limit.min(notable.len())..)
        .collect::<Vec<_>>()
    {
        let i = at[&s.entity];
        let what = if s.name.is_empty() {
            label(s.entity, &cands[i].look)
        } else {
            Rc::from(s.name.as_str())
        };
        kinds.entry(what).or_default().push(i);
    }
    let mut groups: Vec<Group> = kinds
        .into_iter()
        .map(|(what, members)| {
            let first = &cands[members[0]];
            let mut b = first.bounds;
            for &i in &members {
                let c = cands[i].bounds;
                b = [
                    b[0].min(c[0]),
                    b[1].min(c[1]),
                    b[2].max(c[2]),
                    b[3].max(c[3]),
                ];
            }
            Group {
                what: what.to_string(),
                count: members.len(),
                bounds: b,
                look: first.look.to_string(),
                ids: members
                    .iter()
                    .take(3)
                    .map(|&i| entity_id(world, cands[i].e))
                    .collect(),
            }
        })
        .collect();
    groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.what.cmp(&b.what)));
    let mut omitted = 0;
    if groups.len() > spec.groups {
        omitted += groups.len() - spec.groups;
        groups.truncate(spec.groups);
    }

    // Effects: the camera's shake, particles, and materials on screen.
    let mut effects = Vec::new();
    if let Some(cam) = argus_camera::current(world) {
        if let Ok(sh) = world.get::<argus_camera::CameraShake>(cam) {
            if sh.trauma > 0.0 {
                effects.push(json!({ "kind": "shake", "camera": entity_id(world, cam), "trauma": round(sh.trauma), "offset": [round(sh.offset.x), round(sh.offset.y)], "angle": round(sh.angle) }));
            }
        }
        post_effects(world, cam, spec.height, &mut effects);
    }
    if let Some(store) = spec.particles {
        for (e, pool) in &store.pools {
            let local = pool.effect.spec.space == argus_assets::Space::Local;
            let o = if local { pool.origin } else { [0.0, 0.0] };
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            let mut n = 0;
            for (x, y) in pool.x().iter().zip(pool.y()) {
                let (px, py) = to_px(x + o[0], y + o[1]);
                if px >= 0.0 && py >= 0.0 && px <= w && py <= h {
                    n += 1;
                    b = [b[0].min(px), b[1].min(py), b[2].max(px), b[3].max(py)];
                }
            }
            if n > 0 {
                effects.push(json!({ "kind": "particles", "effect": pool.effect.name, "emitter": entity_id(world, *e), "count": n, "bounds": clip(b) }));
            }
        }
    }
    let mut per_look: HashMap<*const u8, (Rc<str>, usize)> = HashMap::new();
    for c in &cands {
        per_look
            .entry(c.look.as_ptr())
            .or_insert_with(|| (c.look.clone(), 0))
            .1 += 1;
    }
    let mut mats: BTreeMap<String, usize> = BTreeMap::new();
    for (look, n) in per_look.into_values() {
        if let Some(m) = look.split(" +").nth(1) {
            *mats.entry(m.to_string()).or_default() += n;
        }
    }
    for (m, n) in mats {
        effects.push(json!({ "kind": "material", "material": m, "sprites": n }));
    }

    SemanticFrame {
        size: [spec.width, spec.height],
        camera,
        view,
        rect,
        hud,
        entities: notable,
        groups,
        effects,
        omitted,
        problems,
    }
}
