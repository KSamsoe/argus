//! Argus particles, phase 1 item 10 (§13 "Particles and VFX"): emitter
//! pools.
//!
//! A particle effect is an asset (`assets/fx/sparks.toml`, `type =
//! "emitter"`, see `argus_assets::effect`). An [`Emitter`] on an entity
//! plays one at the entity's position. Its particles are not entities: they
//! live in a [`Pool`], a structure of arrays the [`ParticleStore`] keeps per
//! emitter, and `particles.update` advances every pool in one tight loop
//! (position, velocity with gravity and drag, age) and emits new particles
//! from the rate, the opening burst, `Emitter.burst`, and `particles.burst`
//! events. The velocity curve is evaluated exactly in the loop; size,
//! rotation, and colour are baked into lookup tables once per import, and
//! the renderer reads a pool's arrays and the tables directly, one draw
//! per pool.
//!
//! Randomness comes from the system's own stream (`particles`) of the run's
//! seed, so a replay emits the same particles in the same places. The pools
//! are not in the world, but they feed it: `Emitter.live`, `particles.done`,
//! and `auto_despawn` follow the particles' deaths, so an effect's lifetime,
//! rate, burst, and duration steer ticks. A world snapshot alone starts with
//! empty pools; a keyframe and a save hold them, each by its effect's name,
//! and the particles put back play on under the effect as it is now.

#![warn(missing_docs)]

mod system;

use argus_assets::{ColorCurve, Curve, EmitterSpec, Importer};
use argus_ecs::codec::{Reader, Writer};
use argus_ecs::{Component, Entity, Result, StrId, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub use system::SYSTEM;

/// Samples in a baked curve.
pub const LUT: usize = 64;

/// A particle dies when this much of its life is used: a hair under 1, so
/// a lifetime of a whole number of ticks is not a tick longer for rounding.
const END: f32 = 1.0 - 1e-5;

/// Plays a particle effect at this entity's position.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A particle emitter: plays an effect asset (assets/<effect>.toml, type = \"emitter\") at this entity's position. Its particles live in a pool, not as entities; live counts them"
)]
pub struct Emitter {
    /// The effect asset.
    #[argus(doc = "The effect: an emitter asset name, such as fx/sparks")]
    pub effect: StrId,
    /// Emitting from the rate.
    #[argus(
        doc = "Playing: the opening burst and the rate run while true; particles already out live on when false"
    )]
    pub playing: bool,
    /// Rate multiplier.
    #[argus(doc = "Multiplies the effect's rate", range(0, 100), tune)]
    pub rate_scale: f32,
    /// A burst request.
    #[argus(
        doc = "Emit this many at once on the next tick (set it to fire a burst; particles.update zeroes it)",
        range(0, 1000000)
    )]
    pub burst: u32,
    /// Despawn when finished.
    #[argus(doc = "Despawn the entity once emission has ended and its last particle is gone")]
    pub auto_despawn: bool,
    /// Seconds played.
    #[argus(doc = "Seconds it has played since it started (written by particles.update)")]
    pub time: f32,
    /// Particles alive.
    #[argus(doc = "Particles alive (written by particles.update)")]
    pub live: u32,
    /// Particles emitted.
    #[argus(doc = "Particles emitted in all (written by particles.update)")]
    pub emitted: u32,
    /// Particles not emitted because the pool was full.
    #[argus(
        doc = "Particles not emitted because the pool was at the effect's max (written by particles.update)"
    )]
    pub dropped: u32,
    /// Owed fraction of a particle.
    #[argus(doc = "The fraction of a particle the rate owes (written by particles.update)")]
    pub carry: f32,
    /// Started.
    #[argus(
        doc = "It has started and fired its opening burst (written by particles.update); set false to play it again from the start"
    )]
    pub started: bool,
    /// Emission over.
    #[argus(
        doc = "Its duration has run out, or it is a burst with no rate (written by particles.update)"
    )]
    pub done: bool,
}

impl Default for Emitter {
    fn default() -> Self {
        Emitter {
            effect: StrId::EMPTY,
            playing: true,
            rate_scale: 1.0,
            burst: 0,
            auto_despawn: false,
            time: 0.0,
            live: 0,
            emitted: 0,
            dropped: 0,
            carry: 0.0,
            started: false,
            done: false,
        }
    }
}

/// An effect ready to run: its spec and its curves baked into tables.
#[derive(Clone, Debug)]
pub struct Effect {
    /// The asset name.
    pub name: String,
    /// What the file says.
    pub spec: EmitterSpec,
    /// Speed multiplier over life, or none when it is 1 throughout.
    pub velocity: Option<Ramps>,
    /// Size over life.
    pub size: [f32; LUT],
    /// Rotation over life, radians.
    pub rotation: [f32; LUT],
    /// Colour over life, linear RGBA.
    pub color: [[f32; 4]; LUT],
    /// The atlas particles draw from: the sprite, or rect or circle.
    pub atlas: String,
}

fn bake(c: &Curve, scale: f32) -> [f32; LUT] {
    let mut out = [0.0; LUT];
    for (i, o) in out.iter_mut().enumerate() {
        *o = c.at(i as f32 / (LUT - 1) as f32) * scale;
    }
    out
}

fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn bake_color(c: &ColorCurve) -> [[f32; 4]; LUT] {
    let pts: Vec<(f32, [f32; 4])> = c
        .points
        .iter()
        .map(|(t, b)| {
            (
                *t,
                [
                    linear(b[0]),
                    linear(b[1]),
                    linear(b[2]),
                    b[3] as f32 / 255.0,
                ],
            )
        })
        .collect();
    let mut out = [[0.0; 4]; LUT];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / (LUT - 1) as f32;
        *o = if t <= pts[0].0 {
            pts[0].1
        } else {
            let mut v = pts[pts.len() - 1].1;
            for w in pts.windows(2) {
                if t <= w[1].0 {
                    let k = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                    v = std::array::from_fn(|j| w[0].1[j] + (w[1].1[j] - w[0].1[j]) * k);
                    break;
                }
            }
            v
        };
    }
    out
}

impl Effect {
    /// Bake an imported effect.
    pub fn compile(name: &str, spec: &EmitterSpec) -> Effect {
        let flat = spec.velocity.points.iter().all(|p| p.1 == 1.0);
        Effect {
            name: name.to_string(),
            velocity: (!flat).then(|| Ramps::new(&spec.velocity)),
            size: bake(&spec.size, 1.0),
            rotation: bake(&spec.rotation, std::f32::consts::PI / 180.0),
            color: bake_color(&spec.color),
            atlas: spec.sprite.clone().unwrap_or_else(|| spec.particle.clone()),
            spec: spec.clone(),
        }
    }
}

/// The table index for a life fraction.
#[inline]
pub fn lut_index(t: f32) -> usize {
    ((t.clamp(0.0, 1.0) * (LUT - 1) as f32) + 0.5) as usize
}

/// A curve as its start value plus ramps: exact where a table rounds, and
/// branch-free, so the update loop still vectorises with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ramps {
    /// The value before the first point.
    pub base: f32,
    /// Each segment: start t, length in t, slope.
    pub ramps: Vec<[f32; 3]>,
}

#[inline(always)]
fn ramp(r: [f32; 3], t: f32) -> f32 {
    r[2] * (t - r[0]).max(0.0).min(r[1])
}

impl Ramps {
    /// From a curve's points. A step (two points at one t) is a ramp a
    /// millionth long.
    pub fn new(c: &Curve) -> Ramps {
        let ramps = c
            .points
            .windows(2)
            .filter(|w| w[1].1 != w[0].1)
            .map(|w| {
                let len = (w[1].0 - w[0].0).max(1e-6);
                [w[0].0, len, (w[1].1 - w[0].1) / len]
            })
            .collect();
        Ramps {
            base: c.points[0].1,
            ramps,
        }
    }

    /// The value at `t`.
    #[inline]
    pub fn at(&self, t: f32) -> f32 {
        self.base + self.ramps.iter().map(|r| ramp(*r, t)).sum::<f32>()
    }
}

/// One emitter's particles, as a structure of arrays, 24 bytes a particle,
/// oldest first. Positions are world space, or relative to the emitter for
/// a local effect. Where a particle was a tick ago is not stored: it is
/// its position less one step of its velocity (see [`Pool::speed_at`]).
#[derive(Clone, Debug)]
pub struct Pool {
    /// The effect they came from.
    pub effect: Rc<Effect>,
    // Slots before `head` are dead: particles mostly die oldest first, and
    // skipping a dead front is free where closing it would move every
    // survivor. The front is reclaimed once it outgrows the living.
    head: usize,
    x: Vec<f32>,
    y: Vec<f32>,
    vx: Vec<f32>,
    vy: Vec<f32>,
    life: Vec<f32>,
    rate: Vec<f32>,
    /// The emitter's position at this tick.
    pub origin: [f32; 2],
    /// The emitter's position at the tick before.
    pub origin_prev: [f32; 2],
    /// The sim tick it was last advanced in (the world is at the next).
    pub tick: u64,
    /// The seconds that tick lasted.
    pub dt: f32,
    announced: bool,
}

macro_rules! each_array {
    ($s:ident, $v:ident => $body:expr) => {
        for $v in [
            &mut $s.x,
            &mut $s.y,
            &mut $s.vx,
            &mut $s.vy,
            &mut $s.life,
            &mut $s.rate,
        ] {
            $body;
        }
    };
}

impl Pool {
    fn new(effect: Rc<Effect>, origin: [f32; 2]) -> Pool {
        Pool {
            effect,
            head: 0,
            x: Vec::new(),
            y: Vec::new(),
            vx: Vec::new(),
            vy: Vec::new(),
            life: Vec::new(),
            rate: Vec::new(),
            origin,
            origin_prev: origin,
            tick: 0,
            dt: 0.0,
            announced: false,
        }
    }

    /// Particles alive.
    pub fn len(&self) -> usize {
        self.x.len() - self.head
    }

    /// No particles.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Positions at this tick.
    pub fn x(&self) -> &[f32] {
        &self.x[self.head..]
    }

    /// Positions at this tick.
    pub fn y(&self) -> &[f32] {
        &self.y[self.head..]
    }

    /// Velocities.
    pub fn vx(&self) -> &[f32] {
        &self.vx[self.head..]
    }

    /// Velocities.
    pub fn vy(&self) -> &[f32] {
        &self.vy[self.head..]
    }

    /// Life used, 0 to 1.
    pub fn life(&self) -> &[f32] {
        &self.life[self.head..]
    }

    /// Life used per second (1 / lifetime).
    pub fn rate(&self) -> &[f32] {
        &self.rate[self.head..]
    }

    /// The speed multiplier the velocity curve gives at a life fraction:
    /// the last step moved a particle by `v * speed_at(life) * dt`, so
    /// that is how far back it was a tick ago.
    pub fn speed_at(&self, life: f32) -> f32 {
        self.effect.velocity.as_ref().map_or(1.0, |v| v.at(life))
    }

    /// The pool is at the effect's max.
    pub fn full(&self) -> bool {
        self.len() >= self.effect.spec.max as usize
    }

    /// Keep the oldest `n`.
    fn truncate(&mut self, n: usize) {
        let end = self.head + n;
        each_array!(self, v => v.truncate(end));
    }

    fn push(&mut self, p: [f32; 2], v: [f32; 2], life: f32, rate: f32) {
        self.x.push(p[0]);
        self.y.push(p[1]);
        self.vx.push(v[0]);
        self.vy.push(v[1]);
        self.life.push(life);
        self.rate.push(rate);
    }

    /// Advance every particle by `dt` and drop the dead, keeping the order
    /// they were born in (so overlapping particles never swap in the draw).
    fn step(&mut self, dt: f32) {
        let fx = &*self.effect;
        let damp = (-fx.spec.drag * dt).exp();
        let (gx, gy) = (fx.spec.gravity[0] * dt, fx.spec.gravity[1] * dt);
        self.dt = dt;
        let h = self.head;
        let mut dead = 0u32;
        // Every particle moves and ages in one loop over the arrays side by
        // side, with nothing in it to stop the compiler vectorising it.
        let rows = self.x[h..]
            .iter_mut()
            .zip(self.y[h..].iter_mut())
            .zip(self.vx[h..].iter_mut().zip(self.vy[h..].iter_mut()))
            .zip(self.life[h..].iter_mut().zip(self.rate[h..].iter()));
        macro_rules! body {
            ($k:expr) => {
                for (((x, y), (vx, vy)), (life, rate)) in rows {
                    let t = *life + *rate * dt;
                    let k = $k(t) * dt;
                    *vx = (*vx + gx) * damp;
                    *vy = (*vy + gy) * damp;
                    *x += *vx * k;
                    *y += *vy * k;
                    *life = t;
                    dead += (t >= END) as u32;
                }
            };
        }
        match fx.velocity.as_ref().map(|v| (v.base, v.ramps.as_slice())) {
            None => body!(|_t: f32| 1.0f32),
            Some((b, [])) => body!(|_t: f32| b),
            Some((b, &[r0])) => body!(|t: f32| b + ramp(r0, t)),
            Some((b, &[r0, r1])) => body!(|t: f32| b + ramp(r0, t) + ramp(r1, t)),
            Some((b, &[r0, r1, r2])) => {
                body!(|t: f32| b + ramp(r0, t) + ramp(r1, t) + ramp(r2, t))
            }
            Some((b, rs)) => body!(|t: f32| b + rs.iter().map(|r| ramp(*r, t)).sum::<f32>()),
        }
        if dead > 0 {
            self.bury(dead as usize);
        }
    }

    /// Drop `dead` particles, keeping the order of the rest.
    fn bury(&mut self, mut dead: usize) {
        let n = self.x.len();
        // A dead front is skipped.
        while dead > 0 && self.life[self.head] >= END {
            self.head += 1;
            dead -= 1;
        }
        if dead > 0 {
            // Holes further in: from the last hole back to the head, move
            // the living towards the back, a block at a time.
            let mut last = n - 1;
            while self.life[last] < END {
                last -= 1;
            }
            let (mut w, mut i) = (last + 1, last + 1);
            while i > self.head {
                while i > self.head && self.life[i - 1] >= END {
                    i -= 1;
                }
                let end = i;
                while i > self.head && self.life[i - 1] < END {
                    i -= 1;
                }
                let len = end - i;
                if len > 0 && end != w {
                    let start = i;
                    each_array!(self, v => v.copy_within(start..end, w - len));
                }
                w -= len;
            }
            self.head = w;
        }
        // Reclaim the dead front once it is as big as the living.
        if self.head > 0 && self.head * 2 >= self.x.len() {
            let h = self.head;
            each_array!(self, v => {
                v.copy_within(h.., 0);
                v.truncate(v.len() - h);
            });
            self.head = 0;
        }
    }
}

/// Every effect and every pool. Shared by `particles.update` (which writes
/// it) and whatever draws (which reads it).
#[derive(Default)]
pub struct ParticleStore {
    /// Effects by asset name.
    pub effects: BTreeMap<String, Rc<Effect>>,
    /// Pools by emitter, in entity order.
    pub pools: BTreeMap<Entity, Pool>,
    /// Bumped whenever the effects change.
    pub generation: u64,
    /// Effects tuned live, by asset name: played in place of their files'
    /// until taken off (phase 5 item 3).
    tuned: BTreeMap<String, EmitterSpec>,
}

impl ParticleStore {
    /// Write the pools for a keyframe on disk (phase 2 item 2): each
    /// emitter's effect by name and its particles.
    pub fn encode_pools(pools: &BTreeMap<Entity, Pool>, w: &mut Writer) {
        w.count(pools.len());
        for (e, p) in pools {
            w.u64(e.to_bits());
            w.str(&p.effect.name);
            w.count(p.head);
            w.count(p.x.len());
            for a in [&p.x, &p.y, &p.vx, &p.vy, &p.life, &p.rate] {
                for v in a {
                    w.f32(*v);
                }
            }
            for v in p.origin.iter().chain(&p.origin_prev) {
                w.f32(*v);
            }
            w.u64(p.tick);
            w.f32(p.dt);
            w.bool(p.announced);
        }
    }

    /// Read pools [`ParticleStore::encode_pools`] wrote, finding each
    /// effect by name among this store's. A pool whose effect is gone is
    /// dropped, and named in the second list.
    pub fn decode_pools(&self, r: &mut Reader) -> Result<(BTreeMap<Entity, Pool>, Vec<String>)> {
        let mut pools = BTreeMap::new();
        let mut missing = Vec::new();
        for _ in 0..r.count()? {
            let e = Entity::from_bits(r.u64()?);
            let name = r.str()?;
            let head = r.count()?;
            let n = r.count()?;
            let mut arrays: [Vec<f32>; 6] = Default::default();
            for a in &mut arrays {
                for _ in 0..n {
                    a.push(r.f32()?);
                }
            }
            let mut o = [0.0f32; 4];
            for v in &mut o {
                *v = r.f32()?;
            }
            let tick = r.u64()?;
            let dt = r.f32()?;
            let announced = r.bool()?;
            if head > n {
                return Err(r.bad(format!("a pool's head {head} is past its {n} particles")));
            }
            let Some(effect) = self.effects.get(&name) else {
                missing.push(name);
                continue;
            };
            let [x, y, vx, vy, life, rate] = arrays;
            pools.insert(
                e,
                Pool {
                    effect: effect.clone(),
                    head,
                    x,
                    y,
                    vx,
                    vy,
                    life,
                    rate,
                    origin: [o[0], o[1]],
                    origin_prev: [o[2], o[3]],
                    tick,
                    dt,
                    announced,
                },
            );
        }
        Ok((pools, missing))
    }

    /// Take every effect from an importer. An effect whose file now fails
    /// to import keeps its last good version; one tuned live keeps that.
    pub fn set_assets(&mut self, assets: &Importer) {
        let mut effects: BTreeMap<String, Rc<Effect>> = BTreeMap::new();
        for (name, spec) in assets.emitters() {
            let spec = self.tuned.get(name).unwrap_or(spec);
            match self.effects.get(name) {
                Some(old) if old.spec == *spec => {
                    effects.insert(name.clone(), old.clone());
                }
                _ => {
                    effects.insert(name.clone(), Rc::new(Effect::compile(name, spec)));
                }
            }
        }
        for (name, old) in &self.effects {
            if !effects.contains_key(name) && assets.failures().contains_key(name) {
                effects.insert(name.clone(), old.clone());
            }
        }
        let changed = effects.len() != self.effects.len()
            || effects
                .iter()
                .any(|(n, e)| self.effects.get(n).is_none_or(|o| !Rc::ptr_eq(o, e)));
        self.effects = effects;
        if changed || self.generation == 0 {
            self.generation += 1;
        }
    }

    /// Add or replace one effect (for tests and tools without an importer).
    pub fn set_effect(&mut self, name: &str, spec: &EmitterSpec) {
        self.effects
            .insert(name.to_string(), Rc::new(Effect::compile(name, spec)));
        self.generation += 1;
    }

    /// Play `spec` for the effect `name` in place of its file's from the
    /// next tick, the particles already out included, until
    /// [`ParticleStore::untune`]; an import in between keeps it (phase 5
    /// item 3: the tuning panel moves an effect live, and its file changes
    /// only when the session is kept).
    pub fn tune(&mut self, name: &str, spec: EmitterSpec) {
        self.set_effect(name, &spec);
        self.tuned.insert(name.to_string(), spec);
    }

    /// What `name` is tuned to, if it is.
    pub fn tuned(&self, name: &str) -> Option<&EmitterSpec> {
        self.tuned.get(name)
    }

    /// Take every tuned effect off: each plays its file's again, as the
    /// importer has it.
    pub fn untune(&mut self, assets: &Importer) {
        if self.tuned.is_empty() {
            return;
        }
        self.tuned.clear();
        self.set_assets(assets);
    }

    /// Particles alive in every pool.
    pub fn live(&self) -> usize {
        self.pools.values().map(Pool::len).sum()
    }

    /// One emitter's pool.
    pub fn pool(&self, e: Entity) -> Option<&Pool> {
        self.pools.get(&e)
    }
}

/// Register `Emitter`.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Emitter>()?;
    Ok(())
}

/// Register `Emitter` and add `particles.update`, last in the late stage.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    store: Rc<RefCell<ParticleStore>>,
) -> Result<()> {
    register(world)?;
    system::install(world, sched, store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A project directory removed when the test ends, passed or not.
    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn emitter(burst: u32) -> String {
        format!("type = \"emitter\"\nburst = {burst}\n")
    }

    #[test]
    fn a_tuned_effect_plays_until_taken_off_and_an_import_keeps_it() {
        let dir = Dir(std::env::temp_dir().join(format!("argus-tuned-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&dir.0);
        std::fs::create_dir_all(dir.0.join("assets/fx")).unwrap();
        std::fs::write(dir.0.join("assets/fx/a.toml"), emitter(4)).unwrap();
        std::fs::write(dir.0.join("assets/fx/b.toml"), emitter(1)).unwrap();
        let mut imp = Importer::new(&dir.0);
        imp.scan();
        let mut store = ParticleStore::default();
        store.set_assets(&imp);
        let g = store.generation;

        let (spec, _) = argus_assets::parse_emitter("assets/fx/a.toml", &emitter(9)).unwrap();
        store.tune("fx/a", spec);
        assert_eq!(store.effects["fx/a"].spec.burst, 9);
        assert_eq!(store.tuned("fx/a").map(|s| s.burst), Some(9));
        assert!(
            store.generation > g,
            "particles.update takes it on the next tick"
        );
        // Another file's import keeps the tuned one. Written at the same
        // length and given back its time, as two writes inside one step of
        // the clock are, it keeps its stamp: unread until touched.
        let b = dir.0.join("assets/fx/b.toml");
        let was = std::fs::metadata(&b).unwrap().modified().unwrap();
        std::fs::write(&b, emitter(2)).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&b)
            .unwrap()
            .set_modified(was)
            .unwrap();
        imp.scan();
        store.set_assets(&imp);
        assert_eq!(store.effects["fx/b"].spec.burst, 1, "the stamp hid it");
        imp.touch("fx/b");
        imp.scan();
        store.set_assets(&imp);
        assert_eq!(store.effects["fx/a"].spec.burst, 9);
        assert_eq!(store.effects["fx/b"].spec.burst, 2);
        // Taken off, the file's again.
        store.untune(&imp);
        assert_eq!(store.effects["fx/a"].spec.burst, 4);
        assert!(store.tuned("fx/a").is_none());
    }
}
