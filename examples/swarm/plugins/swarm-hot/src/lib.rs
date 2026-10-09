//! Swarm's hot path (design §19: "Swarm's per-entity rules ... The
//! scheduler's write-set declarations make it possible to promote a Luau
//! system to Rust without changing anything around it").
//!
//! `sim.chase` was `systems/chase.luau`. Its declaration is the Luau one,
//! name, stage, order, reads, and writes; its arithmetic is the Luau one,
//! step for step (vectors in f32, the scale in doubles, as Luau's numbers
//! are), so the world it makes is the same to the bit. In the stress scene
//! it walks 50,000 enemies a tick in well under a millisecond, where the
//! Luau took 37.
//!
//! `sim.dash` is the dasher's rules. It was Luau first, and cost 6 ms a
//! tick at 5,000 dashers (a tenth of a 50,000 wave), against the 6 ms the
//! whole tick may take; here it is per-entity arithmetic like the chase.
//! Its looks, a colour per phase, stay Luau (systems/dash_look.luau), run
//! only for the dashers its events name.

use argus_plugin::prelude::*;

component! {
    /// components/swarm.luau's Enemy. `hold` is set by sim.dash while a
    /// dasher winds up, charges, or recovers: the chase leaves it alone then.
    pub struct Enemy { pub speed: f32, pub damage: i32, pub wave: u32, pub hold: bool }
}

component! {
    /// components/swarm.luau's Player.
    pub struct Player {
        pub speed: f32,
        pub fire_every: u8,
        pub bullets_per_burst: u8,
        pub cooldown: u8,
        pub aim: f32,
        pub iframes: u8,
        pub kills: u32,
        pub dead: bool,
    }
}

component! {
    /// components/swarm.luau's Dasher.
    pub struct Dasher {
        pub state: u8,
        pub timer: u32,
        pub every: u32,
        pub range: f32,
        pub windup: u32,
        pub charge_speed: f32,
        pub overshoot: f32,
        pub recover: u32,
        pub target: Vec2,
        pub dir: Vec2,
        pub left: f32,
    }
}

/// Dasher.state.
const WALK: u8 = 0;
const WINDUP: u8 = 1;
const CHARGE: u8 = 2;
const RECOVER: u8 = 3;

/// Its Transform.scale in each phase: it swells by SWELL over the wind-up.
const SWELL: f32 = 0.6;
const CHARGE_SCALE: f32 = 1.2;
const RECOVER_SCALE: f32 = 0.9;

/// Every enemy walks straight at the player, but for the ones held (a
/// dasher between its wind-up and the end of its recovery).
fn chase(ctx: &Ctx) -> Result {
    let pe = ctx.entity("@player")?;
    if ctx.get::<Player>(pe)?.dead {
        return Ok(());
    }
    let p = ctx.get::<Transform>(pe)?.pos;
    let dt = ctx.dt();
    ctx.each::<(&Enemy, &mut Transform)>(|_, (en, t)| {
        if en.hold {
            return;
        }
        // local d = p - t.pos; local len = vector.magnitude(d)
        let d = p - t.pos;
        let len = d.length();
        if len as f64 > 0.05 {
            // t.pos = t.pos + d * (en.speed * dt / len): the scale is a
            // Luau number (a double), made a float to scale the vector.
            let k = (en.speed as f64 * dt / len as f64) as f32;
            t.pos = t.pos + d * k;
        }
    })
}

/// A phase change, emitted once the walk over the dashers is done.
struct Change {
    name: &'static str,
    entity: Entity,
    at: Vec2,
    target: Vec2,
}

/// Every dasher: walks (the chase moves it) until its timer is out and the
/// player is in range; then, held out of the chase, winds up (stands and
/// swells), charges along the line fixed when the wind-up began, through
/// where the player was then and overshoot past it, and recovers standing.
fn dash(ctx: &Ctx) -> Result {
    let pe = ctx.entity("@player")?;
    if ctx.get::<Player>(pe)?.dead {
        return Ok(());
    }
    let p = ctx.get::<Transform>(pe)?.pos;
    let dt = ctx.dt() as f32;
    let mut changes: Vec<Change> = Vec::new();
    ctx.each::<(&mut Dasher, &mut Enemy, &mut Transform)>(|e, (da, en, t)| {
        match da.state {
            WALK => {
                if da.timer > 0 {
                    da.timer -= 1;
                } else {
                    let d = p - t.pos;
                    let len = d.length();
                    if len <= da.range {
                        // The wind-up: the line is fixed now, at the player's spot.
                        da.state = WINDUP;
                        da.timer = da.windup;
                        da.target = p;
                        da.dir = if len > 1e-6 { d * (1.0 / len) } else { Vec2::new(1.0, 0.0) };
                        da.left = len + da.overshoot;
                        en.hold = true;
                        changes.push(Change { name: "dash.windup", entity: e, at: t.pos, target: p });
                    }
                }
            }
            WINDUP => {
                da.timer = da.timer.saturating_sub(1);
                if da.timer > 0 {
                    let s = 1.0 + SWELL * (1.0 - da.timer as f32 / da.windup.max(1) as f32);
                    t.scale = Vec2::new(s, s);
                } else {
                    da.state = CHARGE;
                    t.scale = Vec2::new(CHARGE_SCALE, CHARGE_SCALE);
                    changes.push(Change { name: "dash.charge", entity: e, at: t.pos, target: da.target });
                }
            }
            RECOVER => {
                da.timer = da.timer.saturating_sub(1);
                if da.timer == 0 {
                    da.state = WALK;
                    da.timer = da.every;
                    en.hold = false;
                    t.scale = Vec2::new(1.0, 1.0);
                    changes.push(Change { name: "dash.recovered", entity: e, at: t.pos, target: da.target });
                }
            }
            _ => {}
        }
        // The charge moves from the tick it begins on.
        if da.state == CHARGE {
            let step = (da.charge_speed * dt).min(da.left);
            t.pos = t.pos + da.dir * step;
            da.left -= step;
            if da.left <= 1e-4 {
                da.left = 0.0;
                da.state = RECOVER;
                da.timer = da.recover;
                t.scale = Vec2::new(RECOVER_SCALE, RECOVER_SCALE);
                changes.push(Change { name: "dash.recover", entity: e, at: t.pos, target: da.target });
            }
        }
    })?;
    let tick = ctx.tick();
    for c in changes {
        ctx.emit(
            c.name,
            &[("entity", c.entity.into()), ("at", c.at.into()), ("target", c.target.into()), ("tick", tick.into())],
        )?;
    }
    Ok(())
}

plugin! {
    components: [Enemy, Player, Transform, Dasher],
    systems: [
        System::new("sim.dash", "sim")
            .after("sim.waves")
            .reads(&["Dasher", "Enemy", "Transform", "Player"])
            .writes(&["Dasher", "Enemy", "Transform"])
            .doc("Every dasher walks with the swarm until its timer is out and the player in range; then, held out of the chase, it winds up (stands and swells), charges along a line fixed at the wind-up's start through where the player was then, and recovers. Emits dash.windup, dash.charge, dash.recover, and dash.recovered {entity, at, target, tick} (Rust: plugins/swarm-hot).")
            .run(dash),
        System::new("sim.chase", "sim")
            .after("sim.waves")
            .after("sim.dash")
            .reads(&["Enemy", "Transform", "Player"])
            .writes(&["Transform"])
            .doc("Every enemy walks straight at the player, but for a dasher held by its dash (Rust: plugins/swarm-hot).")
            .run(chase),
    ],
}
