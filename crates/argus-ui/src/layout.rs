//! `ui.layout` (late stage, after the camera): every UI tree laid out on
//! the canvas, flexbox style, by taffy; each node's [`UiRect`] written.
//!
//! A tree's top is a node with no UI parent: it fills the canvas, or, with
//! a [`UiRoot`] that follows a world entity, takes its content's size and
//! sits at that entity's place on screen (its pivot on the point); an
//! absolute top takes its content's size and sits by anchor and pivot on
//! the canvas. Children are ordered by `UiNode.order`, then by entity. An
//! absolute node is taken out of the flow and placed by its anchor (a point
//! of its parent) and pivot (a point of itself). `offset` moves a node and
//! its subtree after layout. Hidden nodes take no space; opacity multiplies
//! down the tree; a clipping node's rect bounds what its descendants draw.
//!
//! Layout runs in the tick, on the fixed canvas of `ui.toml`, so it is part
//! of the deterministic world: a replay lays out the same, a test can
//! assert where a button is, and a pointer's position (in canvas pixels,
//! an input) hits the same button on every run.
//!
//! It reads each archetype's columns once (a lookup of a component a node
//! lacks would make an error to throw away), compares its keywords as
//! interned ids, and measures text without making its lines: §17 gives it
//! a millisecond for 2,000 nodes with text.

use crate::components::{UiButton, UiNode, UiRect, UiRoot, UiScroll, UiText};
use crate::text::{size_of, FontFace};
use crate::{Shared, UiSettings};
use argus_ecs::{
    ComponentId, Entity, GlobalTransform, Parent, Result, StrId, Transform, Vec2, World,
};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use taffy::prelude::*;
use taffy::{Overflow, Point};

/// The layout system's name.
pub const LAYOUT: &str = "ui.layout";

pub(crate) struct Ids {
    pub button: ComponentId,
    pub node: ComponentId,
    pub rect: ComponentId,
    pub root: ComponentId,
    pub text: ComponentId,
    pub parent: ComponentId,
    pub settings: ComponentId,
    pub scroll: ComponentId,
}

impl Ids {
    pub fn of(world: &World) -> Ids {
        Ids {
            button: world.component_id_of::<UiButton>().expect("registered"),
            node: world.component_id_of::<UiNode>().expect("registered"),
            rect: world.component_id_of::<UiRect>().expect("registered"),
            root: world.component_id_of::<UiRoot>().expect("registered"),
            text: world.component_id_of::<UiText>().expect("registered"),
            parent: world.component_id_of::<Parent>().expect("registered"),
            settings: world.component_id_of::<UiSettings>().expect("registered"),
            scroll: world.component_id_of::<UiScroll>().expect("registered"),
        }
    }
}

/// The layout's keywords, interned once.
struct Kw {
    row: StrId,
    start: StrId,
    center: StrId,
    end: StrId,
    stretch: StrId,
    between: StrId,
    around: StrId,
    evenly: StrId,
}

impl Kw {
    fn of(world: &World) -> Kw {
        let s = world.strings();
        Kw {
            row: s.intern("row"),
            start: s.intern("start"),
            center: s.intern("center"),
            end: s.intern("end"),
            stretch: s.intern("stretch"),
            between: s.intern("between"),
            around: s.intern("around"),
            evenly: s.intern("evenly"),
        }
    }
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    shared: Rc<RefCell<Shared>>,
) -> Result<()> {
    let ids = Ids::of(world);
    let kw = Kw::of(world);
    let mut desc = SystemDesc::new(LAYOUT, Stage::Late)
        .last()
        .on_load()
        .unfrozen()
        .reads(ids.node)
        .reads(ids.root)
        .reads(ids.parent)
        .reads(ids.settings)
        .writes(ids.rect)
        .writes(ids.text)
        .writes(ids.root)
        .writes(ids.button)
        .writes(ids.scroll)
        .source("argus-ui:layout")
        .doc("Lays out every UI tree on the canvas (ui.toml width and height), flexbox style: writes each node's UiRect (where it landed, its draw order, whether it shows, its opacity, its clip) and each text's shown words, holes filled. A tree hung from a world entity sits at its place on screen. A tree whose focused button cannot take focus now (hidden, disabled) focuses its first one that can, so a menu opened this tick shows its focus at once.");
    for t in [
        world.component_id_of::<Transform>(),
        world.component_id_of::<GlobalTransform>(),
    ]
    .into_iter()
    .flatten()
    {
        desc = desc.reads(t);
    }
    if let Some(cam) = world.component_id("Camera") {
        desc = desc.reads(cam);
        // After the camera has moved, when it runs here.
        if sched.is_enabled("camera.update").is_ok() {
            desc = desc.after("camera.update");
        }
    }
    let mut state = State::default();
    sched.add_system(
        desc,
        Box::new(move |ctx| {
            let sh = shared.borrow();
            layout(ctx, &sh, &ids, &kw, &mut state)
        }),
    )
}

/// The layout kept between ticks (a retained tree): taffy's nodes by entity
/// and what each was laid out from, so a tick marks only what changed and
/// taffy's cache answers for the rest. It is a cache: every node is
/// compared with the world every tick, so a seek, a load, or a fork that
/// replaces the world lays out the same as a fresh start.
struct State {
    tree: TaffyTree<Entity>,
    nodes: HashMap<Entity, NodeId>,
    prev: HashMap<Entity, Prev>,
    /// The fonts' generation it measured with.
    fonts: u64,
    /// The world's language at the last layout.
    lang: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            tree: TaffyTree::new(),
            nodes: HashMap::new(),
            prev: HashMap::new(),
            fonts: 0,
            lang: String::new(),
        }
    }
}

/// What a node was laid out from last tick.
struct Prev {
    node: UiNode,
    top: bool,
    fill: Option<(f32, f32)>,
    scroll: bool,
    kids: Vec<NodeId>,
    text: Option<TextKey>,
}

/// What a text's size depends on, and the text it came from.
#[derive(Clone)]
struct TextKey {
    raw: StrId,
    key: StrId,
    holes: bool,
    shown: Rc<str>,
    size: f32,
    font: StrId,
    wrap: bool,
}

impl PartialEq for TextKey {
    fn eq(&self, o: &Self) -> bool {
        (Rc::ptr_eq(&self.shown, &o.shown) || self.shown == o.shown)
            && self.size == o.size
            && self.font == o.font
            && self.wrap == o.wrap
    }
}

/// One node of the trees being laid out, with what it has.
struct Item {
    e: Entity,
    node: UiNode,
    parent: Option<Entity>,
    text: Option<UiText>,
    root: Option<UiRoot>,
    button: Option<UiButton>,
    rect: Option<UiRect>,
    scroll: Option<UiScroll>,
    kids: Vec<usize>,
    /// Its entry in the texts measured, for a text node.
    measured: Option<usize>,
    tid: Option<NodeId>,
}

/// A width to fit in (none: one line per paragraph) and the size measured.
type Measured = (Option<f32>, (f32, f32));

/// A text node's words, font, and size, and the sizes it was measured at.
struct TextItem {
    shown: Rc<str>,
    /// The words' id when they have no holes (the text itself).
    same: Option<StrId>,
    font: Rc<dyn FontFace>,
    size: f32,
    wrap: bool,
    /// The widest its words may be: the node's `max` width less its
    /// padding, when it has one. The layout measures a leaf before it
    /// clamps it, so wrapping must know this itself (found at the phase 5
    /// gate: a sign with a `max` width kept one line's height, and its
    /// other lines spilled out below it).
    cap: Option<f32>,
    memo: RefCell<Vec<Measured>>,
}

impl TextItem {
    fn size(&self, max: Option<f32>) -> (f32, f32) {
        let max = if self.wrap { max } else { None };
        if let Some((_, s)) = self.memo.borrow().iter().find(|(m, _)| *m == max) {
            return *s;
        }
        let s = size_of(self.font.as_ref(), &self.shown, self.size, max);
        self.memo.borrow_mut().push((max, s));
        s
    }
}

/// A node's taffy style: `top` for a tree's top (never absolutely placed to
/// taffy; it is placed by hand), `fill` the canvas a screen tree's top
/// fills unless it says otherwise.
fn style(kw: &Kw, n: &UiNode, top: bool, fill: Option<(f32, f32)>, scroll: bool) -> Style {
    let dim = |px: f32, share: f32| -> Dimension {
        if px > 0.0 {
            length(px)
        } else if share > 0.0 {
            percent(share)
        } else {
            auto()
        }
    };
    let bound = |px: f32| -> LengthPercentageAuto {
        if px > 0.0 {
            length(px)
        } else {
            auto()
        }
    };
    let align = |id: StrId| -> Option<AlignItems> {
        if id == kw.start {
            Some(AlignItems::FLEX_START)
        } else if id == kw.center {
            Some(AlignItems::CENTER)
        } else if id == kw.end {
            Some(AlignItems::FLEX_END)
        } else if id == kw.stretch {
            Some(AlignItems::STRETCH)
        } else {
            None
        }
    };
    let justify = if n.justify == kw.center {
        JustifyContent::CENTER
    } else if n.justify == kw.end {
        JustifyContent::FLEX_END
    } else if n.justify == kw.between {
        JustifyContent::SPACE_BETWEEN
    } else if n.justify == kw.around {
        JustifyContent::SPACE_AROUND
    } else if n.justify == kw.evenly {
        JustifyContent::SPACE_EVENLY
    } else {
        JustifyContent::FLEX_START
    };
    let mut st = Style {
        display: if n.visible {
            Display::Flex
        } else {
            Display::None
        },
        flex_direction: if n.direction == kw.row {
            FlexDirection::Row
        } else {
            FlexDirection::Column
        },
        justify_content: Some(justify),
        align_items: Some(align(n.align).unwrap_or(AlignItems::STRETCH)),
        align_self: align(n.align_self),
        flex_wrap: if n.wrap {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        gap: Size {
            width: length(n.gap),
            height: length(n.gap),
        },
        padding: Rect {
            left: length(n.pad.x),
            right: length(n.pad.x),
            top: length(n.pad.y),
            bottom: length(n.pad.y),
        },
        margin: Rect {
            left: length(n.margin.x),
            right: length(n.margin.x),
            top: length(n.margin.y),
            bottom: length(n.margin.y),
        },
        size: Size {
            width: dim(n.size.x, n.fill.x),
            height: dim(n.size.y, n.fill.y),
        },
        min_size: Size {
            width: bound(n.min.x),
            height: bound(n.min.y),
        },
        max_size: Size {
            width: bound(n.max.x),
            height: bound(n.max.y),
        },
        flex_grow: n.grow,
        flex_shrink: n.shrink,
        position: if n.absolute && !top {
            Position::Absolute
        } else {
            Position::Relative
        },
        // A scrolled node is as big as it is given, not as its content: its
        // content goes past it (issue 23).
        overflow: Point {
            x: Overflow::Visible,
            y: if scroll {
                Overflow::Scroll
            } else {
                Overflow::Visible
            },
        },
        scrollbar_width: 0.0,
        ..Style::DEFAULT
    };
    // A screen tree's top fills the canvas unless it says otherwise.
    if let Some((w, h)) = fill {
        if n.size.x <= 0.0 && n.fill.x <= 0.0 {
            st.size.width = length(w);
        }
        if n.size.y <= 0.0 && n.fill.y <= 0.0 {
            st.size.height = length(h);
        }
    }
    st
}

/// Where a world point is on the canvas, through the drawing camera; `None`
/// with no camera.
pub fn world_to_canvas(world: &World, p: [f32; 2], canvas: [f32; 2]) -> Option<[f32; 2]> {
    let v = argus_camera::view(world, canvas[0] / canvas[1])?;
    Some([
        ((p[0] - v.center[0]) / v.half[0] * 0.5 + 0.5) * canvas[0],
        (0.5 - (p[1] - v.center[1]) / v.half[1] * 0.5) * canvas[1],
    ])
}

/// Where a canvas point (the pointer's pixels) is in the world, through
/// the drawing camera: [`world_to_canvas`] the other way. `None` with no
/// camera.
pub fn canvas_to_world(world: &World, p: [f32; 2], canvas: [f32; 2]) -> Option<[f32; 2]> {
    let v = argus_camera::view(world, canvas[0] / canvas[1])?;
    Some([
        v.center[0] + (p[0] / canvas[0] - 0.5) * 2.0 * v.half[0],
        v.center[1] + (0.5 - p[1] / canvas[1]) * 2.0 * v.half[1],
    ])
}

fn position(world: &World, e: Entity) -> Option<[f32; 2]> {
    if let Ok(g) = world.get::<GlobalTransform>(e) {
        return Some([g.pos.x, g.pos.y]);
    }
    world.get::<Transform>(e).ok().map(|t| [t.pos.x, t.pos.y])
}

/// The text scale and palette in effect: the first `UiSettings`, else the
/// theme's.
pub(crate) fn settings(world: &World, shared: &Shared) -> (f32, bool) {
    let Some(id) = world.component_id_of::<UiSettings>() else {
        return (shared.theme.text_scale, shared.theme.high_contrast);
    };
    let mut all = world.query().has(id).entities();
    all.sort();
    match all.first().and_then(|e| world.get::<UiSettings>(*e).ok()) {
        Some(u) => (u.text_scale.max(0.1), u.high_contrast),
        None => (shared.theme.text_scale, shared.theme.high_contrast),
    }
}

/// Every UI node with what it has, read column by column.
fn gather(world: &World, ids: &Ids) -> Result<Vec<Item>> {
    let mut items: Vec<Item> = Vec::new();
    world.query().has(ids.node).for_each_view(|v| {
        let nodes = v.column::<UiNode>()?;
        let parents = v.column::<Parent>().ok();
        let texts = v.column::<UiText>().ok();
        let roots = v.column::<UiRoot>().ok();
        let buttons = v.column::<UiButton>().ok();
        let rects = v.column::<UiRect>().ok();
        let scrolls = v.column::<UiScroll>().ok();
        for (i, e) in v.entities().iter().enumerate() {
            items.push(Item {
                e: *e,
                node: nodes[i],
                parent: parents.as_ref().map(|p| p[i].entity),
                text: texts.as_ref().map(|t| t[i]),
                root: roots.as_ref().map(|r| r[i]),
                button: buttons.as_ref().map(|b| b[i]),
                rect: rects.as_ref().map(|r| r[i]),
                scroll: scrolls.as_ref().map(|s| s[i]),
                kids: Vec::new(),
                measured: None,
                tid: None,
            });
        }
        Ok(())
    })?;
    Ok(items)
}

fn layout(
    ctx: &mut SystemCtx<'_>,
    shared: &Shared,
    ids: &Ids,
    kw: &Kw,
    state: &mut State,
) -> Result<()> {
    // Fonts changed (an asset reimported): measure everything again.
    if state.fonts != shared.fonts.generation() {
        *state = State {
            fonts: shared.fonts.generation(),
            ..State::default()
        };
    }
    let world = ctx.world();
    let canvas = [shared.theme.width, shared.theme.height];
    let (scale, _) = settings(world, shared);
    let mut items = gather(world, ids)?;
    if items.is_empty() {
        return Ok(());
    }
    let index: HashMap<Entity, usize> = items.iter().enumerate().map(|(i, it)| (it.e, i)).collect();
    let mut tops = Vec::new();
    for i in 0..items.len() {
        match items[i].parent.and_then(|p| index.get(&p).copied()) {
            Some(p) if p != i => items[p].kids.push(i),
            _ => tops.push(i),
        }
    }
    let keys: Vec<(i32, Entity)> = items.iter().map(|it| (it.node.order, it.e)).collect();
    for it in items.iter_mut() {
        it.kids.sort_by_key(|&k| keys[k]);
    }
    // Trees in draw order: by the root's layer, then entity.
    tops.sort_by_key(|&i| (items[i].root.map_or(0, |r| r.layer), items[i].e));
    // A node under a hidden one (itself included) keeps last tick's words
    // while its text, its key, and the language are the same: they are
    // not read from the string tables again, nor their holes filled.
    // Found at the phase 3 gate: Outpost's inspector, closed, read its
    // seventy nodes' words every tick of the colony and doubled a debug
    // tick. A language chosen still reaches a hidden menu at once.
    let mut hidden = vec![false; items.len()];
    let mut stack: Vec<(usize, bool)> = tops.iter().map(|&t| (t, false)).collect();
    while let Some((i, above)) = stack.pop() {
        let h = above || !items[i].node.visible;
        hidden[i] = h;
        stack.extend(items[i].kids.iter().map(|&k| (k, h)));
    }

    // Texts, holes filled; fonts looked up once each. A keyed text is the
    // string table's in the world's language (phase 3 item 4), looked up
    // every layout, so a language chosen now shows now.
    let mut texts: Vec<TextItem> = Vec::new();
    let mut problems = Vec::new();
    let mut fonts: HashMap<StrId, Rc<dyn FontFace>> = HashMap::new();
    let lang_now = crate::strings::language(world);
    let lang_same = lang_now == state.lang;
    for (i, it) in items.iter_mut().enumerate() {
        let Some(t) = it.text else {
            continue;
        };
        let prev = state.prev.get(&it.e).and_then(|p| p.text.as_ref());
        // The words a hidden node keeps are the world's, which a seek or a
        // replay's keyframe puts back, not this cache's (phase 5 item 2: a
        // report that began after a controls page closed replayed its
        // labels as a fresh layout had last seen them).
        let unseen = prev
            .filter(|k| hidden[i] && lang_same && k.raw == t.text && k.key == t.key)
            .map(|k| {
                let world_shown = world.strings().resolve(t.shown);
                let s = if *k.shown == *world_shown {
                    k.shown.clone()
                } else {
                    Rc::from(world_shown.as_ref())
                };
                (s, (!k.holes).then_some(t.text))
            });
        // Words without holes are the text itself: resolved again only when
        // the text is another one.
        let kept = prev
            .filter(|k| t.key == StrId::EMPTY && k.raw == t.text && !k.holes)
            .map(|k| k.shown.clone());
        let (shown, same): (Rc<str>, Option<StrId>) = match (unseen, kept) {
            (Some((s, same)), _) => (s, same.filter(|_| t.key == StrId::EMPTY)),
            (None, Some(s)) => (s, Some(t.text)),
            (None, None) if t.key != StrId::EMPTY => {
                let raw = shared
                    .strings
                    .text(&lang_now, &world.strings().resolve(t.key));
                if raw.contains('{') {
                    (
                        Rc::from(argus_ecs::text::fill(world, it.e, &raw, &mut problems)),
                        None,
                    )
                } else {
                    (Rc::from(raw), None)
                }
            }
            (None, None) => {
                let raw = world.strings().resolve(t.text).to_string();
                if raw.contains('{') {
                    (
                        Rc::from(argus_ecs::text::fill(world, it.e, &raw, &mut problems)),
                        None,
                    )
                } else {
                    (Rc::from(raw), Some(t.text))
                }
            }
        };
        let size = if t.size > 0.0 {
            t.size
        } else {
            shared.theme.text_size
        } * scale;
        let font = fonts
            .entry(t.font)
            .or_insert_with(|| {
                shared
                    .fonts
                    .face(&world.strings().resolve(t.font), &shared.theme.font)
            })
            .clone();
        it.measured = Some(texts.len());
        texts.push(TextItem {
            shown,
            same,
            font,
            size,
            wrap: t.wrap,
            cap: (it.node.max.x > 0.0).then(|| (it.node.max.x - 2.0 * it.node.pad.x).max(0.0)),
            memo: RefCell::new(Vec::new()),
        });
    }

    // The kept taffy tree, brought up to the world: nodes gone are removed,
    // new ones made, and a node's style, children, or words marked only
    // when they changed.
    let tree_err = |e: taffy::TaffyError| {
        argus_ecs::ArgusError::new(argus_ecs::codes::BAD_STATE, format!("ui layout: {e}"))
    };
    let gone: Vec<Entity> = state
        .nodes
        .keys()
        .filter(|e| !index.contains_key(e))
        .copied()
        .collect();
    for e in gone {
        if let Some(n) = state.nodes.remove(&e) {
            state.tree.remove(n).map_err(tree_err)?;
        }
        state.prev.remove(&e);
    }
    for it in items.iter_mut() {
        let n = match state.nodes.get(&it.e) {
            Some(n) => *n,
            None => {
                let n = state.tree.new_leaf(Style::DEFAULT).map_err(tree_err)?;
                state.nodes.insert(it.e, n);
                n
            }
        };
        it.tid = Some(n);
    }
    let mut is_top = vec![false; items.len()];
    for &t in &tops {
        is_top[t] = true;
    }
    for (i, it) in items.iter().enumerate() {
        let n = it.tid.expect("a node");
        let top = is_top[i];
        let floating = it.root.is_some_and(|r| !r.follow.is_null()) || it.node.absolute;
        let fill = (top && !floating).then_some((canvas[0], canvas[1]));
        let text = it.measured.map(|m| TextKey {
            raw: it.text.map_or(StrId::EMPTY, |t| t.text),
            key: it.text.map_or(StrId::EMPTY, |t| t.key),
            holes: texts[m].same.is_none(),
            shown: texts[m].shown.clone(),
            size: texts[m].size,
            font: it.text.map_or(StrId::EMPTY, |t| t.font),
            wrap: texts[m].wrap,
        });
        let kid_node = |k: &usize| items[*k].tid.expect("a node");
        match state.prev.get_mut(&it.e) {
            // Compared in place: a tick where nothing changed allocates
            // nothing here.
            Some(p) => {
                let scroll = it.scroll.is_some();
                if p.node != it.node || p.top != top || p.fill != fill || p.scroll != scroll {
                    state
                        .tree
                        .set_style(n, style(kw, &it.node, top, fill, scroll))
                        .map_err(tree_err)?;
                    p.node = it.node;
                    p.top = top;
                    p.fill = fill;
                    p.scroll = scroll;
                }
                if !p.kids.iter().copied().eq(it.kids.iter().map(kid_node)) {
                    p.kids = it.kids.iter().map(kid_node).collect();
                    state.tree.set_children(n, &p.kids).map_err(tree_err)?;
                }
                if p.text.is_some() != text.is_some() {
                    state
                        .tree
                        .set_node_context(n, text.is_some().then_some(it.e))
                        .map_err(tree_err)?;
                }
                if p.text != text {
                    state.tree.mark_dirty(n).map_err(tree_err)?;
                }
                p.text = text;
            }
            None => {
                let kids: Vec<NodeId> = it.kids.iter().map(kid_node).collect();
                state
                    .tree
                    .set_style(n, style(kw, &it.node, top, fill, it.scroll.is_some()))
                    .map_err(tree_err)?;
                state.tree.set_children(n, &kids).map_err(tree_err)?;
                state
                    .tree
                    .set_node_context(n, text.is_some().then_some(it.e))
                    .map_err(tree_err)?;
                state.prev.insert(
                    it.e,
                    Prev {
                        node: it.node,
                        top,
                        fill,
                        scroll: it.scroll.is_some(),
                        kids,
                        text,
                    },
                );
            }
        }
    }
    let text_of: HashMap<Entity, &TextItem> = items
        .iter()
        .filter_map(|it| it.measured.map(|m| (it.e, &texts[m])))
        .collect();
    let tree = &mut state.tree;
    let mut out: Vec<(usize, UiRect)> = Vec::with_capacity(items.len());
    let mut scrolled: Vec<(usize, UiScroll)> = Vec::new();
    let mut tree_of: Vec<usize> = vec![usize::MAX; items.len()];
    let mut depth = 0u32;
    for &t in &tops {
        let follow = items[t].root.filter(|r| !r.follow.is_null());
        let id = items[t].tid.expect("a node");
        // A tree hung from the world takes its content's size.
        let space = if follow.is_some() {
            Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            }
        } else {
            Size {
                width: AvailableSpace::Definite(canvas[0]),
                height: AvailableSpace::Definite(canvas[1]),
            }
        };
        let text_of = &text_of;
        // Every leaf goes through taffy's leaf layout (its size, bounds,
        // and padding); a text leaf's content is its words.
        tree.compute_layout_with_measure(id, space, |input, _node, ctx, style| {
            let text = ctx.and_then(|e| text_of.get(e).copied());
            taffy::compute_leaf_layout(
                input,
                style,
                |_, _| 0.0,
                |known, avail| {
                    let Some(t) = text else {
                        return Size::ZERO;
                    };
                    let max = known.width.or(match avail.width {
                        AvailableSpace::Definite(w) => Some(w),
                        AvailableSpace::MinContent => Some(0.0),
                        AvailableSpace::MaxContent => None,
                    });
                    let max = match (max, t.cap) {
                        (Some(m), Some(c)) => Some(m.min(c)),
                        (None, c) => c,
                        (m, None) => m,
                    };
                    // Whole pixels up, so rounding the layout never leaves
                    // the words less room than they measured.
                    let (w, h) = t.size(max);
                    Size {
                        width: known.width.unwrap_or(w.ceil()),
                        height: known.height.unwrap_or(h),
                    }
                },
            )
        })
        .map_err(tree_err)?;

        // Where the tree's top sits.
        let size = tree.layout(id).map(|l| l.size).unwrap_or(Size::ZERO);
        let (origin, shown) = match follow {
            Some(r) => {
                let at = position(world, r.follow)
                    .map(|p| [p[0] + r.follow_offset.x, p[1] + r.follow_offset.y])
                    .and_then(|p| world_to_canvas(world, p, canvas));
                let pv = items[t].node.pivot;
                match at {
                    Some(c) => ([c[0] - pv.x * size.width, c[1] - pv.y * size.height], true),
                    None => ([0.0, 0.0], false),
                }
            }
            None if items[t].node.absolute => {
                let (a, pv) = (items[t].node.anchor, items[t].node.pivot);
                (
                    [
                        a.x * canvas[0] - pv.x * size.width,
                        a.y * canvas[1] - pv.y * size.height,
                    ],
                    true,
                )
            }
            None => ([0.0, 0.0], true),
        };
        let parent = Frame {
            pos: [origin[0], origin[1]],
            size: [canvas[0], canvas[1]],
            shown,
            alpha: 1.0,
            clip: [0.0, 0.0, canvas[0], canvas[1]],
        };
        let first = out.len();
        place(
            tree,
            &items,
            t,
            &parent,
            true,
            &mut depth,
            &mut out,
            &mut scrolled,
        );
        for (i, _) in &out[first..] {
            tree_of[*i] = t;
        }
    }

    // Where each scrolled node is now (issue 23).
    for (i, s) in scrolled {
        ctx.update::<UiScroll, _>(items[i].e, |x| *x = s)?;
    }
    // Write what changed, adding a UiRect where there is none.
    let mut rects: Vec<Option<UiRect>> = vec![None; items.len()];
    let mut updates = Vec::new();
    let mut inserts = Vec::new();
    for (i, r) in out {
        rects[i] = Some(r);
        match items[i].rect {
            Some(old) if old == r => {}
            Some(_) => updates.push((items[i].e, r)),
            None => inserts.push((items[i].e, r)),
        }
    }
    let mut shown_ids = Vec::new();
    for it in &items {
        let (Some(m), Some(t)) = (it.measured, it.text) else {
            continue;
        };
        let want = texts[m]
            .same
            .unwrap_or_else(|| world.strings().intern(&texts[m].shown));
        if t.shown != want {
            shown_ids.push((it.e, want));
        }
    }
    let focus = fix_focus(&items, &tree_of, &rects);
    for (e, r) in updates {
        ctx.update::<UiRect, _>(e, |x| *x = r)?;
    }
    for (e, r) in inserts {
        ctx.insert(e, r)?;
    }
    for (e, want) in shown_ids {
        ctx.update::<UiText, _>(e, |t| t.shown = want)?;
    }
    for (e, f) in focus.0 {
        ctx.update::<UiRoot, _>(e, |r| r.focus = f)?;
    }
    for (e, f) in focus.1 {
        ctx.update::<UiButton, _>(e, |b| b.focused = f)?;
    }
    *shared.problems.borrow_mut() = problems;
    state.lang = lang_now;
    Ok(())
}

/// Roots' and buttons' focus to write.
type FocusWrites = (Vec<(Entity, Entity)>, Vec<(Entity, bool)>);

/// Focus follows what can take it: in each interactive tree whose focused
/// button cannot take focus now (gone, hidden, disabled, clipped away), the
/// first that can takes it, in draw order; the buttons' flags follow.
fn fix_focus(items: &[Item], tree_of: &[usize], rects: &[Option<UiRect>]) -> FocusWrites {
    let usable = |i: usize| -> bool {
        let (Some(r), Some(b)) = (rects[i], items[i].button) else {
            return false;
        };
        r.shown
            && !b.disabled
            && r.alpha > 0.0
            && r.pos.x + r.size.x > r.clip_min.x
            && r.pos.y + r.size.y > r.clip_min.y
            && r.pos.x < r.clip_max.x
            && r.pos.y < r.clip_max.y
    };
    // Each interactive tree's buttons, in draw order.
    let mut trees: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        let t = tree_of[i];
        if it.button.is_some() && t != usize::MAX && items[t].root.is_some_and(|r| r.interactive) {
            trees.entry(t).or_default().push(i);
        }
    }
    let mut roots = Vec::new();
    let mut buttons = Vec::new();
    let mut order: Vec<usize> = trees.keys().copied().collect();
    order.sort_unstable();
    for t in order {
        let mut mine = trees.remove(&t).unwrap_or_default();
        mine.sort_by_key(|&i| rects[i].map_or(u32::MAX, |r| r.depth));
        let root = items[t].root.expect("an interactive tree has a root");
        let keep = mine.iter().any(|&i| items[i].e == root.focus && usable(i));
        let focus = if keep {
            root.focus
        } else {
            mine.iter()
                .find(|&&i| usable(i))
                .map_or(Entity::NULL, |&i| items[i].e)
        };
        if focus != root.focus {
            roots.push((items[t].e, focus));
        }
        for &i in &mine {
            let want = items[i].e == focus;
            if items[i].button.map(|b| b.focused) != Some(want) {
                buttons.push((items[i].e, want));
            }
        }
    }
    (roots, buttons)
}

/// The frame a node is placed in: its parent's.
struct Frame {
    pos: [f32; 2],
    size: [f32; 2],
    shown: bool,
    alpha: f32,
    clip: [f32; 4],
}

#[allow(clippy::too_many_arguments)]
fn place(
    tree: &TaffyTree<Entity>,
    items: &[Item],
    i: usize,
    parent: &Frame,
    top: bool,
    depth: &mut u32,
    out: &mut Vec<(usize, UiRect)>,
    scrolled: &mut Vec<(usize, UiScroll)>,
) {
    let it = &items[i];
    let n = &it.node;
    let l = it.tid.and_then(|id| tree.layout(id).ok());
    let size = l.map_or([0.0, 0.0], |l| [l.size.width, l.size.height]);
    let mut pos = if top {
        parent.pos
    } else if n.absolute {
        [
            parent.pos[0] + n.anchor.x * parent.size[0] - n.pivot.x * size[0],
            parent.pos[1] + n.anchor.y * parent.size[1] - n.pivot.y * size[1],
        ]
    } else {
        let loc = l.map_or([0.0, 0.0], |l| [l.location.x, l.location.y]);
        [parent.pos[0] + loc[0], parent.pos[1] + loc[1]]
    };
    pos[0] += n.offset.x;
    pos[1] += n.offset.y;
    let shown = parent.shown && n.visible;
    let alpha = parent.alpha * n.opacity.clamp(0.0, 1.0);
    let rect = UiRect {
        pos: Vec2::new(pos[0], pos[1]),
        size: Vec2::new(size[0], size[1]),
        depth: *depth,
        shown,
        alpha,
        clip_min: Vec2::new(parent.clip[0], parent.clip[1]),
        clip_max: Vec2::new(parent.clip[2], parent.clip[3]),
    };
    *depth += 1;
    out.push((i, rect));
    let clip = if n.clip || it.scroll.is_some() {
        [
            parent.clip[0].max(pos[0]),
            parent.clip[1].max(pos[1]),
            parent.clip[2].min(pos[0] + size[0]),
            parent.clip[3].min(pos[1] + size[1]),
        ]
    } else {
        parent.clip
    };
    // A scrolled node's children move up by how far it is scrolled: kept at
    // the end while it was there and nobody moved it (issue 23).
    let mut inner = pos;
    if let Some(s) = it.scroll {
        let mut bottom: f32 = 0.0;
        for &k in &it.kids {
            let kid = &items[k];
            if kid.node.absolute || !kid.node.visible {
                continue;
            }
            if let Some(l) = kid.tid.and_then(|id| tree.layout(id).ok()) {
                bottom = bottom.max(l.location.y + l.size.height + kid.node.margin.y);
            }
        }
        let content = bottom + n.pad.y;
        let view = size[1];
        let max = (content - view).max(0.0);
        let y = if s.end && s.at_end && s.y == s.seen {
            max
        } else {
            s.y.clamp(0.0, max)
        };
        let want = UiScroll {
            y,
            content,
            view,
            at_end: y >= max - 0.5,
            seen: y,
            ..s
        };
        if want != s {
            scrolled.push((i, want));
        }
        inner[1] -= y;
    }
    let frame = Frame {
        pos: inner,
        size,
        shown,
        alpha,
        clip,
    };
    for &k in &it.kids {
        place(tree, items, k, &frame, false, depth, out, scrolled);
    }
}
