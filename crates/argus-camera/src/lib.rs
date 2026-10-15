//! Argus cameras, phase 1 item 7 (§12 "Camera"): cameras are entities.
//!
//! A [`Camera`] frames the scene from its entity's position. One component
//! covers both configurations the design names: with a `target` it follows
//! (a dead zone the target moves in freely, lookahead in the direction the
//! target moves, smoothing, and the bounds of the room the target is in:
//! Hollow's rooms); with a pan or zoom action it is moved by a player
//! (Outpost's free pan-and-zoom); with neither it sits where its entity
//! is, which is how a camera on the player itself behaves. A
//! [`CameraShake`] beside it is trauma shake: a trauma value from 0 to 1
//! that gameplay raises (`q:emit("camera.shake", {amount = 0.4})`), shaped
//! by a curve, decaying over time. Every feel parameter is a tune field.
//!
//! The drawing camera is the active one with the highest `priority` (ties
//! go to the older entity). When that changes, `camera.cut {from, to}` is
//! an event, and the new camera's `blend` says how many seconds its view
//! takes to move there from what the last one showed (phase 3 item 5: a
//! timeline's camera cut glides when the cameras say so; 0 cuts at once).
//! `camera.update` runs last in the late stage, after gameplay and
//! physics, and writes each camera's `view_min` and `view_max`, the rect
//! it shows, blend included, which the semantic frame reports. Rendering
//! (present) only reads and interpolates.

#![warn(missing_docs)]

mod system;

use argus_ecs::{Component, Entity, GlobalTransform, Result, StrId, Transform, Vec2, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;

pub use system::SYSTEM;

/// The `device` action's value while a player plays by touch
/// (`argus_input::glyphs::Family::Touch`), which turns on
/// [`Camera::touch_offset`].
pub const TOUCH_DEVICE: f32 = 5.0;

/// A camera: the view from its entity's position, and how it moves.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A camera: frames the scene from its entity's position. With a target it follows (dead zone, lookahead, smoothing, room bounds); with a pan or zoom action a player moves it; with neither it stays where its entity is. The active camera with the highest priority draws"
)]
pub struct Camera {
    /// World units visible from bottom to top.
    #[argus(
        doc = "World units visible vertically (zoom)",
        range(0.01, 100000.0),
        tune
    )]
    pub height: f32,
    /// Whether this camera may draw.
    #[argus(doc = "Whether this camera may draw")]
    pub active: bool,
    /// Higher draws first.
    #[argus(doc = "Among active cameras the highest priority draws; ties go to the older entity")]
    pub priority: i32,
    /// Width over height of the view the game is designed for.
    #[argus(
        doc = "Width over height of the view, for room bounds and view_min/view_max",
        range(0.1, 10.0)
    )]
    pub aspect: f32,
    /// What it follows.
    #[argus(doc = "The entity it follows; none for a camera that stays or is panned")]
    pub target: Entity,
    /// Framing offset from the target.
    #[argus(
        doc = "Where the camera aims relative to the target, in world units (a little up shows more ahead of a jump)",
        tune
    )]
    pub offset: Vec2,
    /// Framing added while the player plays by touch.
    #[argus(
        doc = "Added to offset while the player plays by touch (the device action, of the camera's Actions or else its target's, is 5), and the room's bounds stretch by as much that way, so the target stands clear of the thumbs: [0, -4] draws it 4 units higher, over up to 4 units below the room's floor",
        tune
    )]
    pub touch_offset: Vec2,
    /// Half size of the dead zone.
    #[argus(
        doc = "Half size of the box around the aim the target moves in without moving the camera, in world units",
        tune
    )]
    pub dead_zone: Vec2,
    /// Seconds of target velocity to look ahead, per axis.
    #[argus(doc = "Seconds of the target's velocity to look ahead, per axis", tune)]
    pub lookahead: Vec2,
    /// Farthest lookahead, per axis.
    #[argus(doc = "Farthest lookahead in world units, per axis", tune)]
    pub lookahead_max: Vec2,
    /// Lookahead time constant.
    #[argus(
        doc = "Seconds the lookahead takes to settle (a time constant); 0 is instant",
        range(0, 10),
        tune
    )]
    pub lookahead_smoothing: f32,
    /// Follow time constant, per axis.
    #[argus(
        doc = "Seconds the camera takes to catch up, per axis (a time constant); 0 snaps",
        tune
    )]
    pub smoothing: Vec2,
    /// Keep the view inside the room's bounds.
    #[argus(doc = "Keep the view inside the bounds of the room (or scene) the target is in")]
    pub bounded: bool,
    /// Action that pans.
    #[argus(doc = "Action map name of a stick or axis that pans the camera; empty for none")]
    pub pan_action: StrId,
    /// Pan speed in view heights per second.
    #[argus(
        doc = "Pan speed in view heights per second, so a zoomed-out view pans as fast on screen",
        range(0, 100),
        tune
    )]
    pub pan_speed: f32,
    /// Action that zooms.
    #[argus(doc = "Action map name of an axis that zooms (positive zooms in); empty for none")]
    pub zoom_action: StrId,
    /// Zoom speed.
    #[argus(
        doc = "Zoom speed: the height changes by e to the power of this per second at full input",
        range(0, 100),
        tune
    )]
    pub zoom_speed: f32,
    /// Smallest height; 0 is no limit.
    #[argus(
        doc = "Smallest height zoom reaches; 0 is no limit",
        range(0, 100000),
        tune
    )]
    pub zoom_min: f32,
    /// Largest height; 0 is no limit.
    #[argus(
        doc = "Largest height zoom reaches; 0 is no limit",
        range(0, 100000),
        tune
    )]
    pub zoom_max: f32,
    /// The view's lower-left corner.
    #[argus(
        doc = "Lower-left corner of the rect it shows, shake included (written by camera.update)"
    )]
    pub view_min: Vec2,
    /// The view's upper-right corner.
    #[argus(
        doc = "Upper-right corner of the rect it shows, shake included (written by camera.update)"
    )]
    pub view_max: Vec2,
    /// The bounds it keeps to.
    #[argus(doc = "Name of the room whose bounds it keeps to (written by camera.update)")]
    pub zone: StrId,
    /// Current lookahead.
    #[argus(doc = "Current lookahead offset (written by camera.update)")]
    pub ahead: Vec2,
    /// Where the target was last tick.
    #[argus(doc = "Where the target was last tick, for its velocity (written by camera.update)")]
    pub target_last: Vec2,
    /// Following since the last tick.
    #[argus(doc = "Following the target since last tick; false snaps to it on the next tick")]
    pub tracking: bool,
    /// Seconds a blend into this camera takes.
    #[argus(
        doc = "Seconds the view takes to move from what the camera before showed when this one starts drawing (an ease in and out); 0 cuts",
        range(0, 60),
        tune
    )]
    pub blend: f32,
    /// Seconds of blend left.
    #[argus(doc = "Seconds of blend left (written by camera.update)")]
    pub blend_left: f32,
    /// Where the blend comes from.
    #[argus(doc = "Centre of the view the blend comes from (written by camera.update)")]
    pub blend_pos: Vec2,
    /// How tall the view the blend comes from was.
    #[argus(doc = "Height of the view the blend comes from (written by camera.update)")]
    pub blend_height: f32,
    /// It drew last tick.
    #[argus(doc = "It drew last tick (written by camera.update)")]
    pub drew: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            height: 40.0,
            active: true,
            priority: 0,
            aspect: 16.0 / 9.0,
            target: Entity::NULL,
            offset: Vec2::ZERO,
            touch_offset: Vec2::ZERO,
            dead_zone: Vec2::new(1.0, 1.0),
            lookahead: Vec2::new(0.25, 0.0),
            lookahead_max: Vec2::new(3.0, 0.0),
            lookahead_smoothing: 0.4,
            smoothing: Vec2::new(0.15, 0.2),
            bounded: true,
            pan_action: StrId::EMPTY,
            pan_speed: 1.0,
            zoom_action: StrId::EMPTY,
            zoom_speed: 1.5,
            zoom_min: 0.0,
            zoom_max: 0.0,
            view_min: Vec2::ZERO,
            view_max: Vec2::ZERO,
            zone: StrId::EMPTY,
            ahead: Vec2::ZERO,
            target_last: Vec2::ZERO,
            tracking: false,
            blend: 0.0,
            blend_left: 0.0,
            blend_pos: Vec2::ZERO,
            blend_height: 0.0,
            drew: false,
        }
    }
}

/// Trauma shake on a camera (§12 "shake as a first-class component with a
/// curve and a decay").
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Trauma shake: trauma from 0 to 1, raised by gameplay (the camera.shake event or a write), shaped by curve, decaying over time. The offset and angle move the view, not the camera's entity"
)]
pub struct CameraShake {
    /// Trauma, 0 to 1.
    #[argus(doc = "Current trauma, 0 to 1", range(0, 1))]
    pub trauma: f32,
    /// Trauma lost per second.
    #[argus(doc = "Trauma lost per second", range(0, 100), tune)]
    pub decay: f32,
    /// Exponent from trauma to shake.
    #[argus(
        doc = "Shake is trauma to this power: 2 or 3 keeps small hits small",
        range(0.1, 10),
        tune
    )]
    pub curve: f32,
    /// Largest offset.
    #[argus(doc = "Largest offset at full shake, in world units", tune)]
    pub max_offset: Vec2,
    /// Largest roll.
    #[argus(doc = "Largest roll at full shake, in radians", range(0, 3.2), tune)]
    pub max_angle: f32,
    /// How fast it moves.
    #[argus(doc = "How fast the shake moves, in Hz", range(0, 1000), tune)]
    pub frequency: f32,
    /// The view's offset this tick.
    #[argus(doc = "The view's offset this tick (written by camera.update)")]
    pub offset: Vec2,
    /// The view's roll this tick.
    #[argus(doc = "The view's roll this tick, in radians (written by camera.update)")]
    pub angle: f32,
}

impl Default for CameraShake {
    fn default() -> Self {
        CameraShake {
            trauma: 0.0,
            decay: 1.5,
            curve: 2.0,
            max_offset: Vec2::new(0.5, 0.5),
            max_angle: 0.05,
            frequency: 20.0,
            offset: Vec2::ZERO,
            angle: 0.0,
        }
    }
}

/// Named world rects a following camera keeps its view inside: every
/// room's `bounds` (a flat scene's own `bounds` as the room "").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Zones {
    /// Name and `[x0, y0, x1, y1]`.
    pub rects: Vec<(String, [f32; 4])>,
}

impl Zones {
    /// The first zone containing a point.
    pub fn find(&self, x: f32, y: f32) -> Option<usize> {
        self.rects
            .iter()
            .position(|(_, r)| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3])
    }
    /// A zone by name.
    pub fn named(&self, name: &str) -> Option<usize> {
        self.rects.iter().position(|(n, _)| n == name)
    }
}

/// What the drawing camera shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The camera.
    pub camera: Entity,
    /// Centre in world units, shake included.
    pub center: [f32; 2],
    /// Half extents in world units.
    pub half: [f32; 2],
    /// Roll in radians.
    pub angle: f32,
}

/// The drawing camera: the active one with the highest priority, the older
/// entity on a tie.
pub fn current(world: &World) -> Option<Entity> {
    let cam = world.component_id_of::<Camera>()?;
    let mut best: Option<(i32, Entity)> = None;
    for e in world.query().has(cam).entities() {
        let Ok(c) = world.get::<Camera>(e) else {
            continue;
        };
        if !c.active {
            continue;
        }
        if best.is_none_or(|(p, b)| c.priority > p || (c.priority == p && e < b)) {
            best = Some((c.priority, e));
        }
    }
    best.map(|(_, e)| e)
}

/// An entity's world position: `GlobalTransform` when it has one, else
/// `Transform`.
pub fn position(world: &World, e: Entity) -> Option<[f32; 2]> {
    if let Ok(g) = world.get::<GlobalTransform>(e) {
        return Some([g.pos.x, g.pos.y]);
    }
    world.get::<Transform>(e).ok().map(|t| [t.pos.x, t.pos.y])
}

/// The shake offset and roll on a camera, if it shakes.
pub fn shake(world: &World, e: Entity) -> ([f32; 2], f32) {
    match world.get::<CameraShake>(e) {
        Ok(s) => ([s.offset.x, s.offset.y], s.angle),
        Err(_) => ([0.0, 0.0], 0.0),
    }
}

/// What the drawing camera shows on a target `aspect` wide per unit of
/// height: its position plus shake. `None` without a camera.
pub fn view(world: &World, aspect: f32) -> Option<View> {
    let e = current(world)?;
    let c = world.get::<Camera>(e).ok()?;
    let p = position(world, e).unwrap_or([0.0, 0.0]);
    let (o, angle) = shake(world, e);
    let (center, height) = blended(&c, [p[0] + o[0], p[1] + o[1]], c.height);
    let h = height.max(0.01) * 0.5;
    Some(View {
        camera: e,
        center,
        half: [h * aspect, h],
        angle,
    })
}

/// How far a blend has come, 0 to 1, eased in and out; 1 when there is
/// none.
pub fn blend_progress(c: &Camera) -> f32 {
    if c.blend <= 0.0 || c.blend_left <= 0.0 {
        return 1.0;
    }
    let k = (1.0 - c.blend_left / c.blend).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// A camera's view centre and height with its blend: from where the blend
/// began towards its own.
pub fn blended(c: &Camera, center: [f32; 2], height: f32) -> ([f32; 2], f32) {
    let k = blend_progress(c);
    if k >= 1.0 {
        return (center, height);
    }
    let lerp = |a: f32, b: f32| a + (b - a) * k;
    (
        [
            lerp(c.blend_pos.x, center[0]),
            lerp(c.blend_pos.y, center[1]),
        ],
        lerp(c.blend_height, height),
    )
}

/// The colour behind everything, on the drawing camera's entity. Without
/// it a frame is cleared to the engine's dark grey, which showed as a band
/// around Delve's map and every template's floor.
#[derive(Component, Clone, Copy, Debug, PartialEq, Default)]
#[repr(C)]
#[argus(
    doc = "The colour behind everything, on the drawing camera's entity: what shows where nothing is drawn. Without it, the engine's dark grey"
)]
pub struct Background {
    /// The colour, #RRGGBB.
    #[argus(doc = "#RRGGBB; empty for the engine's dark grey", tune)]
    pub color: StrId,
}

/// The drawing camera's background colour, as written (#RRGGBB), if it has
/// one.
pub fn background(world: &World) -> Option<String> {
    world.component_id_of::<Background>()?;
    let b = world.get::<Background>(current(world)?).ok()?;
    let s = world.strings().resolve(b.color);
    (!s.is_empty()).then_some(s)
}

/// Register `Camera`, `CameraShake`, and `Background`.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Camera>()?;
    world.register::<CameraShake>()?;
    world.register::<Background>()?;
    Ok(())
}

/// Register the components and add `camera.update` to the end of the late
/// stage. `zones` holds the bounds cameras keep to; the daemon fills it
/// from the scene's rooms.
pub fn install(world: &mut World, sched: &mut Scheduler, zones: Rc<RefCell<Zones>>) -> Result<()> {
    register(world)?;
    system::install(world, sched, zones)
}
