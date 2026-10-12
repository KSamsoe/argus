//! The post stack (§13, phase 2 item 6). Screen-space effects are passes a
//! game enables with components on its camera, every setting a tune field
//! (§10), run in order after the world is drawn:
//!
//! - [`PostShake`]: the picture shakes, trauma raised by an event and
//!   decaying, shaped by a curve (the camera's own shake moves the view in
//!   the world; this one moves the screen);
//! - [`PostFlash`]: the screen flashes a colour on an event and fades back;
//! - [`PostFade`]: the screen fades to a colour (a room transition, tweened);
//! - [`PostVignette`], [`PostAberration`], [`PostPalette`]: darkened edges,
//!   colour fringes, a limited palette;
//! - [`PostCrt`]: an old screen's scanlines, curve, bleed, and phosphor
//!   mask (phase 5 item 3).
//!
//! Flash, fade, and vignette amounts are shares of what a person sees (the
//! pass works in sRGB), not of linear light: a flash at 0.1 is a tenth of
//! the way to its colour on a dark screen as on a bright one.
//!
//! And [`PostHitStop`]: on its event the game freezes for a few ticks (or runs
//! slowly) while the post effects play on. Hit-flash, hit-stop, and shake
//! are the three systems that make an impact feel heavy, tuned together in
//! one session. Each listens for a game event by name (`on`, `hit` when
//! empty); an `amount` field on the event scales flash and shake.
//!
//! Everything here is world state, advanced by two systems on the sim's
//! ticks, so a replay, a seek, and a mark reproduce it: `post.hitstop`,
//! first in the input stage, freezes the rest of a tick while a stop lasts
//! (`SystemCtx::freeze`: the tick still counts; input waits and is applied
//! after); `post.update`, last in the late stage, starts effects on this
//! tick's events and decays them. Both run while frozen. On a discrete clock
//! the effects step with turns. The renderer draws the drawing camera's
//! passes (argus-render); the semantic frame lists them.

#![warn(missing_docs)]

use argus_ecs::{Component, ComponentId, Event, Result, StrId, Value, Vec2, World};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};

/// The event a pass or a hit-stop listens for when `on` is empty.
pub const DEFAULT_EVENT: &str = "hit";

/// Screen shake: the picture moves, trauma raised by an event and decaying.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Screen shake, a post pass: trauma raised by an event (on, hit when empty) and decaying, the shake it gives shaped by curve; the picture moves, not the view"
)]
pub struct PostShake {
    /// The event that raises trauma.
    #[argus(doc = "The event that raises trauma; empty for hit")]
    pub on: StrId,
    /// Trauma an event adds.
    #[argus(
        doc = "Trauma an event adds (times its amount field, if any)",
        range(0, 1),
        tune
    )]
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
    /// Largest offset, a share of the screen's height.
    #[argus(
        doc = "Largest offset at full shake, as a share of the screen's height (0.02 is 2%)",
        tune
    )]
    pub max_offset: Vec2,
    /// How fast it moves.
    #[argus(doc = "How fast the shake moves, in Hz", range(0, 1000), tune)]
    pub frequency: f32,
    /// Trauma now.
    #[argus(doc = "Trauma now, 0 to 1 (written by post.update)", range(0, 1))]
    pub level: f32,
    /// The picture's offset this tick.
    #[argus(
        doc = "The picture's offset this tick, as a share of the screen's height (written by post.update)"
    )]
    pub offset: Vec2,
    /// Place in the stack.
    #[argus(doc = "Place in the stack: lower runs first")]
    pub order: i32,
}

impl Default for PostShake {
    fn default() -> Self {
        PostShake {
            on: StrId::EMPTY,
            trauma: 0.4,
            decay: 2.0,
            curve: 2.0,
            max_offset: Vec2::new(0.015, 0.015),
            frequency: 25.0,
            level: 0.0,
            offset: Vec2::ZERO,
            order: 10,
        }
    }
}

/// Hit-flash: the screen flashes a colour on an event and fades back.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Hit-flash, a post pass: on an event (on, hit when empty) the screen takes a colour at peak strength, fading back at decay per second"
)]
pub struct PostFlash {
    /// The event that flashes.
    #[argus(doc = "The event that flashes; empty for hit")]
    pub on: StrId,
    /// Colour.
    #[argus(doc = "The flash's colour, #RRGGBB; empty for white", tune)]
    pub color: StrId,
    /// Strength at the flash.
    #[argus(
        doc = "Strength at the flash, 0 to 1: the share of the way to the colour as it looks, 0.1 a tenth (times the event's amount field, if any)",
        range(0, 1),
        tune
    )]
    pub peak: f32,
    /// Strength lost per second.
    #[argus(doc = "Strength lost per second", range(0, 100), tune)]
    pub decay: f32,
    /// Strength now.
    #[argus(doc = "Strength now, 0 to 1 (written by post.update)", range(0, 1))]
    pub amount: f32,
    /// Place in the stack.
    #[argus(doc = "Place in the stack: lower runs first")]
    pub order: i32,
}

impl Default for PostFlash {
    fn default() -> Self {
        PostFlash {
            on: StrId::EMPTY,
            color: StrId::EMPTY,
            peak: 0.6,
            decay: 6.0,
            amount: 0.0,
            order: 40,
        }
    }
}

/// Hit-stop: the game freezes for a few ticks after its event. Named with
/// the passes it sits beside on the camera; `HitStop` itself is left to
/// games (§3's first example declares one from script).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Hit-stop: after its event (on, hit when empty) the game freezes for frames ticks, or runs at scale, while the post effects play on; input waits and is applied after"
)]
pub struct PostHitStop {
    /// The event that stops the game.
    #[argus(doc = "The event that stops the game; empty for hit")]
    pub on: StrId,
    /// How long.
    #[argus(doc = "Ticks the game stops for", range(0, 120), tune)]
    pub frames: u32,
    /// Time scale while stopped.
    #[argus(
        doc = "Time scale while stopped: 0 stops, 0.25 runs one tick in four",
        range(0, 1),
        tune
    )]
    pub scale: f32,
    /// Ticks left.
    #[argus(doc = "Ticks left of this stop (written by post.update and post.hitstop)")]
    pub left: u32,
    /// The part of a tick carried at a scale above 0.
    #[argus(doc = "The part of a tick carried at a scale above 0 (written by post.hitstop)")]
    pub carry: f32,
}

impl Default for PostHitStop {
    fn default() -> Self {
        PostHitStop {
            on: StrId::EMPTY,
            frames: 4,
            scale: 0.0,
            left: 0,
            carry: 0.0,
        }
    }
}

/// A fade to a colour: room transitions, tweened.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Fade, a post pass: the screen goes toward a colour by amount (a room transition tweens it: q:tween(camera, \"PostFade.amount\", {to = 1, duration = 0.2}))"
)]
pub struct PostFade {
    /// Colour.
    #[argus(doc = "The colour faded to, #RRGGBB; empty for black", tune)]
    pub color: StrId,
    /// How far.
    #[argus(
        doc = "How far, 0 (none) to 1 (all colour), as it looks: 0.5 is half way",
        range(0, 1),
        tune
    )]
    pub amount: f32,
    /// Place in the stack.
    #[argus(doc = "Place in the stack: lower runs first")]
    pub order: i32,
}

impl Default for PostFade {
    fn default() -> Self {
        PostFade {
            color: StrId::EMPTY,
            amount: 0.0,
            order: 60,
        }
    }
}

/// An old screen: scanlines, a curved glass, colours bleeding sideways,
/// and the phosphor mask (phase 5 item 3).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "CRT, a post pass: the picture as an old screen shows it, with scanlines, a curved glass, colours bleeding sideways, and a phosphor mask"
)]
pub struct PostCrt {
    /// How much.
    #[argus(
        doc = "How much, 0 (the picture) to 1 (the old screen)",
        range(0, 1),
        tune
    )]
    pub amount: f32,
    /// How dark between the lines.
    #[argus(doc = "How dark between the scanlines, 0 to 1", range(0, 1), tune)]
    pub scanlines: f32,
    /// How many lines.
    #[argus(
        doc = "Scanlines across the screen's height; 0 for one every three pixels",
        range(0, 4000),
        tune
    )]
    pub lines: f32,
    /// The glass's curve.
    #[argus(
        doc = "How curved the glass is: the corners bend in by this share (0.1 is a gentle curve)",
        range(0, 0.5),
        tune
    )]
    pub curvature: f32,
    /// Sideways bleed.
    #[argus(
        doc = "How far colours bleed sideways, in screen pixels",
        range(0, 8),
        tune
    )]
    pub bleed: f32,
    /// The phosphor mask.
    #[argus(
        doc = "How strong the phosphor mask is: each pixel column leans red, green, or blue",
        range(0, 1),
        tune
    )]
    pub mask: f32,
    /// Place in the stack.
    #[argus(doc = "Place in the stack: lower runs first")]
    pub order: i32,
}

impl Default for PostCrt {
    fn default() -> Self {
        PostCrt {
            amount: 1.0,
            scanlines: 0.35,
            lines: 0.0,
            curvature: 0.08,
            bleed: 1.0,
            mask: 0.15,
            order: 70,
        }
    }
}

/// Darkened edges.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(doc = "Vignette, a post pass: the screen darkens toward its edges")]
pub struct PostVignette {
    /// How dark at the edges.
    #[argus(
        doc = "How dark at the edges, 0 to 1, as it looks: 0.25 takes a quarter of the brightness away",
        range(0, 1),
        tune
    )]
    pub amount: f32,
    /// Where darkening starts.
    #[argus(
        doc = "Distance from the centre where it starts, a share of the screen's height",
        range(0, 2),
        tune
    )]
    pub radius: f32,
    /// How gradual.
    #[argus(doc = "How gradual, the same units", range(0.001, 2), tune)]
    pub softness: f32,
    /// Place in the stack.
    #[argus(doc = "Place in the stack: lower runs first")]
    pub order: i32,
}

impl Default for PostVignette {
    fn default() -> Self {
        PostVignette {
            amount: 0.4,
            radius: 0.45,
            softness: 0.45,
            order: 50,
        }
    }
}

/// Colour fringes toward the edges.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(doc = "Chromatic aberration, a post pass: red and blue drift apart toward the edges")]
pub struct PostAberration {
    /// How far apart at the edges.
    #[argus(
        doc = "How far apart at the corners, a share of the screen's height",
        range(0, 0.1),
        tune
    )]
    pub amount: f32,
    /// Place in the stack.
    #[argus(doc = "Place in the stack: lower runs first")]
    pub order: i32,
}

impl Default for PostAberration {
    fn default() -> Self {
        PostAberration {
            amount: 0.004,
            order: 20,
        }
    }
}

/// A limited palette.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Palette, a post pass: each pixel takes the palette colour for its brightness, dark to light (up to 8 colours)"
)]
pub struct PostPalette {
    /// The palette.
    #[argus(
        doc = "Colours dark to light, #RRGGBB separated by spaces; empty for four greens",
        tune
    )]
    pub colors: StrId,
    /// How much.
    #[argus(
        doc = "How much, 0 (the picture) to 1 (the palette only)",
        range(0, 1),
        tune
    )]
    pub amount: f32,
    /// Place in the stack.
    #[argus(doc = "Place in the stack: lower runs first")]
    pub order: i32,
}

impl Default for PostPalette {
    fn default() -> Self {
        PostPalette {
            colors: StrId::EMPTY,
            amount: 1.0,
            order: 30,
        }
    }
}

/// The palette an empty `PostPalette.colors` means.
pub const DEFAULT_PALETTE: &str = "#0F380F #306230 #8BAC0F #9BBC0F";

/// Register the components.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<PostShake>()?;
    world.register::<PostFlash>()?;
    world.register::<PostHitStop>()?;
    world.register::<PostFade>()?;
    world.register::<PostVignette>()?;
    world.register::<PostAberration>()?;
    world.register::<PostPalette>()?;
    world.register::<PostCrt>()?;
    Ok(())
}

/// Register the components and add `post.hitstop` (first in the input
/// stage) and `post.update` (last in the late stage), both running while
/// the game is frozen.
pub fn install(world: &mut World, sched: &mut Scheduler) -> Result<()> {
    register(world)?;
    let id = |w: &World, n: &str| w.component_id(n).expect("registered");
    let (shake, flash, stop) = (
        id(world, "PostShake"),
        id(world, "PostFlash"),
        id(world, "PostHitStop"),
    );
    sched.add_system(
        SystemDesc::new("post.hitstop", Stage::Input)
            .first()
            .unfrozen()
            .writes(stop)
            .source("argus-post:hitstop")
            .doc("Freezes the rest of the tick while a hit-stop lasts (the tick still counts; input waits), or lets one tick in 1/scale through."),
        Box::new(move |ctx| hitstop(ctx, stop)),
    )?;
    sched.add_system(
        SystemDesc::new("post.update", Stage::Late)
            .last()
            .unfrozen()
            .writes(shake)
            .writes(flash)
            .writes(stop)
            .source("argus-post:update")
            .doc("Starts screen shake, hit-flash, and hit-stop on this tick's events (on, hit when empty; an amount field scales them) and decays shake and flash."),
        Box::new(move |ctx| update(ctx, shake, flash, stop)),
    )?;
    Ok(())
}

fn hitstop(ctx: &mut SystemCtx<'_>, stop: ComponentId) -> Result<()> {
    let ents = ctx.query().has(stop).entities();
    let mut freeze = false;
    for e in ents {
        let mut h: PostHitStop = ctx.get(e)?;
        if h.left == 0 {
            continue;
        }
        h.left -= 1;
        h.carry += h.scale.clamp(0.0, 1.0);
        if h.carry >= 1.0 {
            h.carry -= 1.0;
        } else {
            freeze = true;
        }
        if h.left == 0 {
            h.carry = 0.0;
        }
        ctx.update::<PostHitStop, _>(e, |x| *x = h)?;
    }
    if freeze {
        ctx.freeze();
    }
    Ok(())
}

/// How strongly each of this tick's events named `name` hits: its amount
/// field, or 1.
fn fired(ctx: &SystemCtx<'_>, name: &str) -> Vec<f32> {
    ctx.events_now(name)
        .map(|r| match &r.event {
            Event::Game { fields, .. } => fields
                .iter()
                .find(|(k, _)| k == "amount")
                .and_then(|(_, v)| match v {
                    Value::Float(f) => Some(*f as f32),
                    Value::Int(i) => Some(*i as f32),
                    Value::UInt(u) => Some(*u as f32),
                    _ => None,
                })
                .unwrap_or(1.0),
            _ => 1.0,
        })
        .collect()
}

fn event_name(ctx: &SystemCtx<'_>, on: StrId) -> String {
    let s = ctx.world().strings().resolve(on);
    if s.is_empty() {
        DEFAULT_EVENT.to_string()
    } else {
        s
    }
}

fn update(
    ctx: &mut SystemCtx<'_>,
    shake: ComponentId,
    flash: ComponentId,
    stop: ComponentId,
) -> Result<()> {
    let dt = ctx.dt as f32;
    let time = ctx.time as f32;
    for e in ctx.query().has(flash).entities() {
        let mut f: PostFlash = ctx.get(e)?;
        let hits = fired(ctx, &event_name(ctx, f.on));
        let before = f;
        f.amount = (f.amount - f.decay * dt).max(0.0);
        for a in hits {
            f.amount = f.amount.max((f.peak * a).clamp(0.0, 1.0));
        }
        if f != before {
            ctx.update::<PostFlash, _>(e, |x| *x = f)?;
        }
    }
    for e in ctx.query().has(shake).entities() {
        let mut s: PostShake = ctx.get(e)?;
        let hits = fired(ctx, &event_name(ctx, s.on));
        let before = s;
        s.level = (s.level - s.decay * dt).max(0.0);
        for a in hits {
            s.level = (s.level + s.trauma * a).clamp(0.0, 1.0);
        }
        let k = s.level.powf(s.curve.max(0.01));
        let t = time * s.frequency;
        let seed = e.index().wrapping_mul(7);
        s.offset = if k > 0.0 {
            Vec2::new(
                s.max_offset.x * k * noise(seed, t),
                s.max_offset.y * k * noise(seed + 1, t),
            )
        } else {
            Vec2::ZERO
        };
        if s != before {
            ctx.update::<PostShake, _>(e, |x| *x = s)?;
        }
    }
    for e in ctx.query().has(stop).entities() {
        let h: PostHitStop = ctx.get(e)?;
        if !fired(ctx, &event_name(ctx, h.on)).is_empty() && h.frames > 0 {
            ctx.update::<PostHitStop, _>(e, |x| {
                x.left = x.left.max(x.frames);
                x.carry = 0.0;
            })?;
        }
    }
    Ok(())
}

fn hash(seed: u32, i: i64) -> f32 {
    let mut x = (i as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Smooth value noise in -1..1: the same seed and time give the same value,
/// so a replay shakes the same way, and no gameplay random stream is used.
fn noise(seed: u32, t: f32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(seed, i as i64);
    let b = hash(seed, i as i64 + 1);
    a + (b - a) * u
}
