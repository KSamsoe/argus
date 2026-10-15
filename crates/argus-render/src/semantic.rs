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
use argus_ecs::{Component, ComponentId, Entity, GlobalTransform, StrId, Transform, World};
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
    /// The UI canvas (ui.toml's width and height), when the world has a
    /// UI: its nodes are placed in the frame through it (phase 3 item 1).
    pub ui_canvas: Option<[f32; 2]>,
}

/// A UI node the frame lists (phase 3 item 1): words, or something to
/// press, in the order it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct UiSeen {
    /// The node.
    pub entity: Entity,
    /// Its protocol spelling.
    pub id: String,
    /// Its words (a button's: its own, else its children's).
    pub text: String,
    /// Pixel bounds in the frame: x0, y0, x1, y1 (y down), clipped to it.
    pub bounds: [i32; 4],
    /// A button.
    pub button: bool,
    /// Has focus.
    pub focused: bool,
    /// Cannot be pressed.
    pub disabled: bool,
    /// A text box (issue 23): `text` is what is in it, or its hint.
    pub textbox: bool,
    /// Its words were longer than the frame keeps (200 characters) and
    /// end with an ellipsis (issue 35).
    pub cut: bool,
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
    /// The UI's words and buttons, in draw order (phase 3 item 1: "the
    /// semantic frame reads it directly").
    pub ui: Vec<UiSeen>,
    /// The notable entities (described or named), in reading order.
    pub entities: Vec<Seen>,
    /// Everything else, by kind, most first.
    pub groups: Vec<Group>,
    /// Shake, particles, and materials on screen.
    pub effects: Vec<Json>,
    /// 2D lighting, when the drawing camera has it (phase 5 item 3): the
    /// ambient light and the lights reaching the view; `None` when off.
    pub lighting: Option<Json>,
    /// Entities and groups past the limits.
    pub omitted: usize,
    /// Text holes that named nothing.
    pub problems: Vec<String>,
    /// The shapes systems drew for debugging (2D staples item 1), when a
    /// host added them ([`SemanticFrame::with_debug`]).
    pub debug: Option<Json>,
    /// The touch controls drawn over the game (reaching players item 1),
    /// when a host added them ([`SemanticFrame::with_touch`]).
    pub touch: Option<Json>,
}

impl SemanticFrame {
    /// List the touch controls the engine draws (reaching players item 1)
    /// under `touch`, in this frame's pixels as `ui` places things: each
    /// one's name, kind, bounds, words, and whether a finger is on it.
    pub fn with_touch(&mut self, controls: &[argus_input::TouchShown], canvas: [f32; 2]) {
        if controls.is_empty() {
            return;
        }
        let fit = argus_ui::Fit::new(canvas, [self.size[0] as f32, self.size[1] as f32]);
        // A tenth of a pixel, rounded as the f64 JSON holds.
        let r = |v: f32| (f64::from(v) * 10.0).round() / 10.0;
        let rows: Vec<Json> = controls
            .iter()
            .map(|c| {
                let a = fit.to_window([c.centre.0 - c.radius, c.centre.1 - c.radius]);
                let b = fit.to_window([c.centre.0 + c.radius, c.centre.1 + c.radius]);
                let kind = match c.kind {
                    argus_input::TouchKind::Button => "button",
                    argus_input::TouchKind::Stick => "stick",
                };
                let mut o = json!({
                    "name": c.name,
                    "kind": kind,
                    "bounds": [r(a[0]), r(a[1]), r(b[0]), r(b[1])],
                    "pressed": c.pressed,
                });
                if !c.label.is_empty() {
                    o["label"] = json!(c.label);
                }
                o
            })
            .collect();
        self.touch = Some(Json::Array(rows));
    }

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
            "ui": self.ui.iter().map(|u| {
                let mut o = json!({ "id": u.id, "text": u.text, "bounds": u.bounds });
                if u.button {
                    o["button"] = json!(true);
                }
                if u.focused {
                    o["focused"] = json!(true);
                }
                if u.disabled {
                    o["disabled"] = json!(true);
                }
                if u.textbox {
                    o["textbox"] = json!(true);
                }
                if u.cut {
                    o["cut"] = json!(true);
                }
                o
            }).collect::<Vec<_>>(),
            "columns": ["id", "name", "text", "bounds", "z", "look"],
            "entities": self.entities.iter().map(row).collect::<Vec<_>>(),
            "groups": self.groups.iter().map(|g| json!({
                "what": g.what, "count": g.count, "bounds": g.bounds, "look": g.look, "ids": g.ids,
            })).collect::<Vec<_>>(),
            "effects": self.effects,
            "omitted": self.omitted,
            "problems": self.problems,
        })
        .as_object()
        .cloned()
        .map(|mut o| {
            if let Some(l) = &self.lighting {
                o.insert("lighting".into(), l.clone());
            }
            if let Some(d) = &self.debug {
                o.insert("debug".into(), d.clone());
            }
            if let Some(t) = &self.touch {
                o.insert("touch".into(), t.clone());
            }
            Json::Object(o)
        })
        .expect("an object")
    }
}

fn round(x: f32) -> f64 {
    ((x as f64) * 100.0).round() / 100.0
}

pub use argus_ecs::text::{entity_id, fill};

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
    if let Ok(k) = world.get::<PostCrt>(cam) {
        if k.amount > 0.0 {
            out.push(json!({ "kind": "crt", "camera": c, "amount": round(k.amount), "scanlines": round(k.scanlines), "curvature": round(k.curvature) }));
        }
    }
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
    // Sprites drawn as if far or near (2D staples item 3): where they are drawn.
    let mut far: HashMap<Entity, crate::Parallax> = HashMap::new();
    if let Some(pc) = world.component_id_of::<crate::Parallax>() {
        let _ = world
            .query()
            .has(pc)
            .each::<(&crate::Parallax,)>(|e, (p,)| {
                far.insert(e, *p);
            });
    }
    let mut add =
        |e: Entity, pos: [f32; 2], rot: f32, scale: [f32; 2], s: &Sprite, cands: &mut Vec<Cand>| {
            if !s.visible {
                return;
            }
            let pos = match far.get(&e) {
                Some(p) => {
                    let d = crate::parallax_shift(
                        view.center,
                        [p.factor.x, p.factor.y],
                        [p.origin.x, p.origin.y],
                    );
                    [pos[0] + d[0], pos[1] + d[1]]
                }
                None => pos,
            };
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

    let lighting = lighting(world, rect, &to_px, h);
    let ui = ui_seen(world, spec);
    // UI nodes laid out off the screen or past their parent (issue 31).
    if let Some(canvas) = spec.ui_canvas {
        for o in argus_ui::overflows(world, canvas).iter().take(8) {
            problems.push(o.line(&entity_id(world, o.entity)));
        }
    }
    SemanticFrame {
        size: [spec.width, spec.height],
        camera,
        view,
        rect,
        hud,
        ui,
        entities: notable,
        groups,
        effects,
        lighting,
        omitted,
        problems,
        debug: None,
        touch: None,
    }
}

/// Lights listed one by one; past this, counted.
const LIGHTS: usize = 16;

/// The drawing camera's 2D lighting, as the agent reads it: the ambient
/// light, and each light reaching the view (where on the screen, how far
/// it reaches in pixels, its colour and strength, a cone's direction).
fn lighting(
    world: &World,
    rect: [f32; 4],
    to_px: &dyn Fn(f32, f32) -> (f32, f32),
    h: f32,
) -> Option<Json> {
    let (_, lit) = argus_light::lighting(world)?;
    let words = |id: StrId, default: &str| {
        let s = world.strings().resolve(id);
        if s.is_empty() {
            default.to_string()
        } else {
            s
        }
    };
    let per_unit = h / (rect[3] - rect[1]).max(0.0001);
    let mut lights = Vec::new();
    let mut omitted = 0;
    if let Some(lc) = world.component_id_of::<argus_light::Light>() {
        for e in world.query().has(lc).entities() {
            let Ok(l) = world.get::<argus_light::Light>(e) else {
                continue;
            };
            if !l.on || l.radius <= 0.0 || l.intensity <= 0.0 {
                continue;
            }
            let Some((p, dir)) = argus_light::light_at(world, e, &l) else {
                continue;
            };
            let r = l.radius;
            if p[0] + r < rect[0] || p[0] - r > rect[2] || p[1] + r < rect[1] || p[1] - r > rect[3]
            {
                continue;
            }
            if lights.len() == LIGHTS {
                omitted += 1;
                continue;
            }
            let (x, y) = to_px(p[0], p[1]);
            let mut o = json!({
                "id": entity_id(world, e),
                "kind": if l.angle < 180.0 { "cone" } else { "point" },
                "at": [x.round(), y.round()],
                "radius_px": (r * per_unit).round(),
                "color": words(l.color, "#FFFFFF"),
                "intensity": round(l.intensity),
            });
            if l.angle < 180.0 {
                o["direction"] = json!(round(dir.to_degrees().rem_euclid(360.0)));
                o["angle"] = json!(round(l.angle));
            }
            if !(lit.shadows && l.shadows) {
                o["shadows"] = json!(false);
            }
            if l.flicker > 0.0 {
                o["flicker"] = json!(round(l.flicker));
            }
            lights.push(o);
        }
    }
    let mut out = json!({
        "ambient": words(lit.ambient, "#000000"),
        "level": round(lit.level),
        "shadows": lit.shadows,
        "lights": lights,
    });
    if lit.shadows && !lit.tiles {
        out["tiles_cast"] = json!(false);
    }
    if omitted > 0 {
        out["omitted"] = json!(omitted);
    }
    Some(out)
}

/// The UI's words and buttons, placed in the frame through the canvas. A
/// text inside a button is the button's words, not a line of its own.
fn ui_seen(world: &World, spec: &FrameSpec) -> Vec<UiSeen> {
    use argus_ui::{UiButton, UiRect, UiText, UiTextBox};
    let (Some(canvas), Some(rect_id)) = (spec.ui_canvas, world.component_id_of::<UiRect>()) else {
        return Vec::new();
    };
    let fit = argus_ui::Fit::new(canvas, [spec.width as f32, spec.height as f32]);
    let mut nodes: Vec<(u32, Entity, UiRect)> = Vec::new();
    for e in world.query().has(rect_id).entities() {
        if let Ok(r) = world.get::<UiRect>(e) {
            if r.shown && r.alpha > 0.0 {
                nodes.push((r.depth, e, r));
            }
        }
    }
    nodes.sort_by_key(|n| (n.0, n.1));
    // The plain words: rich text's tags left out, a key's words kept
    // (reaching players item 3), as the screen reader says them.
    let words = |e: Entity| {
        world
            .get::<UiText>(e)
            .map(|t| argus_ui::rich::plain(&world.strings().resolve(t.shown)))
            .unwrap_or_default()
    };
    // The button a node is inside, if any.
    let button_of = |e: Entity| -> Option<Entity> {
        let mut cur = e;
        for _ in 0..64 {
            let p = world.get::<argus_ecs::Parent>(cur).ok()?.entity;
            if p.is_null() {
                return None;
            }
            if world.get::<UiButton>(p).is_ok() {
                return Some(p);
            }
            cur = p;
        }
        None
    };
    let bounds = |r: &UiRect| {
        let lo = fit.to_window([r.pos.x.max(r.clip_min.x), r.pos.y.max(r.clip_min.y)]);
        let hi = fit.to_window([
            (r.pos.x + r.size.x).min(r.clip_max.x),
            (r.pos.y + r.size.y).min(r.clip_max.y),
        ]);
        let (w, h) = (spec.width as i32, spec.height as i32);
        [
            (lo[0].floor() as i32).clamp(0, w),
            (lo[1].floor() as i32).clamp(0, h),
            (hi[0].ceil() as i32).clamp(0, w),
            (hi[1].ceil() as i32).clamp(0, h),
        ]
    };
    let mut out: Vec<UiSeen> = Vec::new();
    let mut at: HashMap<Entity, usize> = HashMap::new();
    for (_, e, r) in &nodes {
        let b = world.get::<UiButton>(*e).ok();
        let text = words(*e);
        if b.is_none() {
            if text.is_empty() {
                continue;
            }
            if let Some(owner) = button_of(*e) {
                if let Some(&i) = at.get(&owner) {
                    let t = &mut out[i].text;
                    if !t.is_empty() {
                        t.push(' ');
                    }
                    t.push_str(&text);
                    continue;
                }
            }
        }
        let bounds = bounds(r);
        // Off the frame (a tree hung from something out of view): not seen.
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            continue;
        }
        at.insert(*e, out.len());
        // A text box says what is in it (or its hint), not its caret.
        let tb = world.get::<UiTextBox>(*e).ok();
        let text = match tb {
            Some(t) => {
                let s = world.strings().resolve(t.text).to_string();
                if s.is_empty() {
                    world.strings().resolve(t.hint).to_string()
                } else {
                    s
                }
            }
            None => text,
        };
        let cut = text.chars().count() > TEXT_CAP;
        out.push(UiSeen {
            entity: *e,
            id: entity_id(world, *e),
            cut,
            text: cap(text),
            bounds,
            button: b.is_some(),
            focused: b.is_some_and(|b| b.focused),
            disabled: b.is_some_and(|b| b.disabled),
            textbox: tb.is_some(),
        });
    }
    out
}
