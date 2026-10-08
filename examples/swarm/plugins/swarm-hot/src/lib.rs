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

use argus_plugin::prelude::*;

component! {
    /// components/swarm.luau's Enemy.
    pub struct Enemy { pub speed: f32, pub damage: i32, pub wave: u32 }
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

/// Every enemy walks straight at the player.
fn chase(ctx: &Ctx) -> Result {
    let pe = ctx.entity("@player")?;
    if ctx.get::<Player>(pe)?.dead {
        return Ok(());
    }
    let p = ctx.get::<Transform>(pe)?.pos;
    let dt = ctx.dt();
    ctx.each::<(&Enemy, &mut Transform)>(|_, (en, t)| {
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

plugin! {
    components: [Enemy, Player, Transform],
    systems: [
        System::new("sim.chase", "sim")
            .after("sim.waves")
            .reads(&["Enemy", "Transform", "Player"])
            .writes(&["Transform"])
            .doc("Every enemy walks straight at the player (Rust: plugins/swarm-hot).")
            .run(chase),
    ],
}
