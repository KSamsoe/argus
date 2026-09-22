//! Moving one body: the kinematic tier's rules (§12), in the solid/actor
//! model platformers use. Solids (moving platforms, doors) go where their
//! velocity takes them and carry or push the actors in the way; actors
//! (characters, walking enemies, bullets that stop at walls) move against
//! tiles and solids one axis at a time, x then y, never passing through a
//! wall however fast they go.
//!
//! An actor on the ground steps up ledges no higher than its step height,
//! follows the ground down slopes and small drops, and walks onto a slope
//! from level ground or level ground from a slope without popping. In the
//! air it gets two nudges of up to its corner correction: sideways around
//! a ceiling corner it bonks, and up onto a ledge its feet just miss.

use crate::env::{Aabb, Asker, Block, Env, Support, EPS};
use argus_ecs::{Entity, Vec2};

/// A body's tuning for the mover (from its CharacterController, else 0).
#[derive(Clone, Copy, Debug, Default)]
pub struct Params {
    /// Step-up height.
    pub step: f32,
    /// Corner correction distance.
    pub corner: f32,
}

/// One actor's state through a tick.
#[derive(Clone, Debug)]
pub struct Actor {
    /// The entity.
    pub e: Entity,
    /// Its box.
    pub b: Aabb,
    /// Filter and drop state.
    pub who: Asker,
    /// Tuning.
    pub p: Params,
    /// Velocity.
    pub vel: Vec2,
    /// On the ground.
    pub grounded: bool,
    /// What it stands on.
    pub support: Option<Support>,
    /// Pressed against a wall this tick: -1 left, 1 right.
    pub wall: i8,
    /// Bonked a ceiling this tick.
    pub ceiling: bool,
    /// Corner correction applied this tick (dx, dy).
    pub corrected: Option<[f32; 2]>,
    /// Squeezed by a solid it could not get out of the way of.
    pub crushed_by: Option<Entity>,
    /// Downward speed when it landed this tick.
    pub impact: f32,
}

impl Actor {
    /// How far the slope under it rises across half its width, plus over
    /// a move of `dx`: level ground or the next slope tile met that much
    /// above the feet is the slope going on, not a wall.
    fn rise(&self, dx: f32) -> f32 {
        self.support
            .map_or(0.0, |s| s.slope().abs() * (self.b.hw() + dx.abs()))
    }

    /// How far above the feet something may rise and still not be a wall
    /// for a move of `dx`.
    fn tol(&self, dx: f32) -> f32 {
        if self.grounded {
            self.p.step.max(self.rise(dx)) + EPS
        } else {
            EPS
        }
    }

    /// Slide sideways up to the first wall. Returns the distance moved and
    /// the wall's top if one stopped it.
    fn slide_x(&mut self, env: &mut Env, dx: f32, tol: f32) -> (f32, Option<f32>) {
        match env.wall(&self.b, dx, tol, &self.who) {
            Some((edge, top)) => {
                let lead = if dx > 0.0 { self.b.x1 } else { self.b.x0 };
                let m = if dx > 0.0 {
                    (edge - lead).clamp(0.0, dx)
                } else {
                    (edge - lead).clamp(dx, 0.0)
                };
                self.b = self.b.shift(m, 0.0);
                (m, Some(top))
            }
            None => {
                self.b = self.b.shift(dx, 0.0);
                (dx, None)
            }
        }
    }

    /// After a sideways move on the ground: lift onto a step or a rising
    /// slope, or settle onto a falling slope or a small drop; leave the
    /// ground if nothing is within reach. False if a step up has no
    /// headroom (then it was a wall).
    fn follow_ground(&mut self, env: &mut Env, dist: f32, tol: f32) -> bool {
        let feet = self.b.y0;
        let lo = feet - (self.p.step.max(dist) + 2.0 * EPS);
        let hi = feet + tol;
        match env.ground(&self.b, lo, hi, hi, feet, &self.who) {
            Some((h, sup)) => {
                if h > feet + EPS && env.ceiling(&self.b, h - feet, &self.who).is_some() {
                    return false;
                }
                self.b = self.b.shift(0.0, h - feet);
                self.support = Some(sup);
            }
            None => {
                self.grounded = false;
                self.support = None;
            }
        }
        true
    }

    /// Move sideways by `dx`. Returns the distance moved and whether a
    /// wall stopped it.
    pub fn move_x(&mut self, env: &mut Env, dx: f32) -> (f32, bool) {
        if dx == 0.0 {
            return (0.0, false);
        }
        let start = (self.b, self.grounded, self.support);
        let tol = self.tol(dx);
        let (mut moved, mut hit) = self.slide_x(env, dx, tol);
        if self.grounded {
            if !self.follow_ground(env, moved.abs(), tol) {
                // No room to step up: the ledge is a wall.
                (self.b, self.grounded, self.support) = start;
                let tol = self.rise(dx) + EPS;
                (moved, hit) = self.slide_x(env, dx, tol);
                self.follow_ground(env, moved.abs(), tol);
            }
        } else if let Some(top) = hit {
            // In the air, feet just below a ledge: up onto it.
            let lift = top - self.b.y0;
            if self.p.corner > 0.0
                && lift > EPS
                && lift <= self.p.corner + EPS
                && env.ceiling(&self.b, lift, &self.who).is_none()
            {
                let before = self.b;
                self.b = self.b.shift(0.0, lift);
                let (m2, hit2) = self.slide_x(env, dx - moved, EPS);
                if m2.abs() > EPS {
                    self.corrected = Some([0.0, lift]);
                    moved += m2;
                    hit = hit2;
                } else {
                    self.b = before;
                }
            }
        }
        if hit.is_some() {
            self.wall = if dx > 0.0 { 1 } else { -1 };
        }
        (moved, hit.is_some())
    }

    /// Nudge sideways around the corner of what it bonked, if no more than
    /// the corner correction and the way up is then clear.
    fn corner(&mut self, env: &mut Env, dy: f32, blocks: &[Block]) -> bool {
        let right = blocks
            .iter()
            .map(|b| b.x1)
            .fold(f32::NEG_INFINITY, f32::max)
            - self.b.x0;
        let left = self.b.x1 - blocks.iter().map(|b| b.x0).fold(f32::INFINITY, f32::min);
        let mut opts = [(right, 1.0f32), (left, -1.0f32)];
        let lean = if self.vel.x < 0.0 { -1.0 } else { 1.0 };
        opts.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap()
                .then(((b.1 == lean) as u8).cmp(&((a.1 == lean) as u8)))
        });
        for (need, dir) in opts {
            if need <= 0.0 || need > self.p.corner + EPS {
                continue;
            }
            let s = dir * need;
            if env.wall(&self.b, s, EPS, &self.who).is_some() {
                continue;
            }
            let shifted = self.b.shift(s, 0.0);
            if env.ceiling(&shifted, dy, &self.who).is_some() {
                continue;
            }
            self.b = shifted.shift(0.0, dy);
            self.corrected = Some([s, 0.0]);
            return true;
        }
        false
    }

    /// Move vertically by `dy`: land on floors, bonk ceilings. `carried`
    /// is a platform's carry, which keeps a rider on the ground as it rises.
    /// Returns the distance moved and whether something stopped it.
    pub fn move_y(&mut self, env: &mut Env, dy: f32, carried: bool) -> (f32, bool) {
        if dy < 0.0 {
            let feet = self.b.y0;
            // Falling on its own, a body that drifted sideways into a slope
            // in the air comes back up to its surface.
            let slope_hi = if carried {
                feet + EPS
            } else {
                self.b.cy().max(feet + EPS)
            };
            match env.ground(&self.b, feet + dy, feet + EPS, slope_hi, feet, &self.who) {
                Some((h, sup)) => {
                    self.b = self.b.shift(0.0, h - feet);
                    if !self.grounded {
                        self.impact = -self.vel.y;
                    }
                    self.grounded = true;
                    self.support = Some(sup);
                    (h - feet, true)
                }
                None => {
                    self.b = self.b.shift(0.0, dy);
                    self.grounded = false;
                    self.support = None;
                    (dy, false)
                }
            }
        } else if dy > 0.0 {
            let out = match env.ceiling(&self.b, dy, &self.who) {
                Some((bottom, blocks)) => {
                    if self.p.corner > 0.0 && !carried && self.corner(env, dy, &blocks) {
                        (dy, false)
                    } else {
                        let m = (bottom - self.b.y1).clamp(0.0, dy);
                        self.b = self.b.shift(0.0, m);
                        self.ceiling = true;
                        (m, true)
                    }
                }
                None => {
                    self.b = self.b.shift(0.0, dy);
                    (dy, false)
                }
            };
            if !carried {
                self.grounded = false;
                self.support = None;
            }
            out
        } else {
            if self.grounded {
                let feet = self.b.y0;
                match env.ground(
                    &self.b,
                    feet - 2.0 * EPS,
                    feet + EPS,
                    feet + EPS,
                    feet,
                    &self.who,
                ) {
                    Some((h, sup)) => {
                        self.b = self.b.shift(0.0, h - feet);
                        self.support = Some(sup);
                    }
                    None => {
                        self.grounded = false;
                        self.support = None;
                    }
                }
            }
            (0.0, false)
        }
    }
}

/// Move solid `si` by `(dx, dy)`, one axis at a time: its riders go with
/// it, and actors in its way are pushed; one it cannot push free is
/// crushed. `riders[i]` says actor i stood on it when the tick began.
pub fn move_solid(
    env: &mut Env,
    si: usize,
    dx: f32,
    dy: f32,
    actors: &mut [Actor],
    riders: &[bool],
) {
    let one_way = env.solids[si].one_way;
    let by = env.solids[si].e;
    if dx != 0.0 {
        env.solids[si].b = env.solids[si].b.shift(dx, 0.0);
        for (i, a) in actors.iter_mut().enumerate() {
            if riders[i] {
                a.move_x(env, dx);
            } else if !one_way && a.b.overlaps(&env.solids[si].b) {
                let s = env.solids[si].b;
                let need = if dx > 0.0 {
                    s.x1 - a.b.x0
                } else {
                    s.x0 - a.b.x1
                };
                let (m, _) = a.move_x(env, need);
                if (need - m).abs() > 10.0 * EPS {
                    a.crushed_by = Some(by);
                }
            }
        }
    }
    if dy != 0.0 {
        let old_top = env.solids[si].b.y1;
        env.solids[si].b = env.solids[si].b.shift(0.0, dy);
        let s = env.solids[si].b;
        for (i, a) in actors.iter_mut().enumerate() {
            let over_x = a.b.x0 < s.x1 - EPS && s.x0 < a.b.x1 - EPS;
            // A one-way platform rising under a body that was above it
            // picks it up, the way a full one does.
            let caught = one_way && dy > 0.0 && over_x && a.b.y0 >= old_top - EPS && a.b.y0 < s.y1;
            if riders[i] || caught {
                if dy > 0.0 {
                    let need = s.y1 - a.b.y0;
                    let (m, _) = a.move_y(env, need, true);
                    if (need - m).abs() > 10.0 * EPS {
                        a.crushed_by = Some(by);
                    }
                } else {
                    a.move_y(env, dy, true);
                }
                if !a.grounded || caught {
                    a.grounded = true;
                    a.support = Some(Support::Body { idx: si, one_way });
                }
            } else if !one_way && a.b.overlaps(&s) {
                let need = if dy > 0.0 {
                    s.y1 - a.b.y0
                } else {
                    s.y0 - a.b.y1
                };
                let (m, _) = a.move_y(env, need, dy > 0.0);
                if (need - m).abs() > 10.0 * EPS {
                    a.crushed_by = Some(by);
                } else if dy > 0.0 {
                    a.grounded = true;
                    a.support = Some(Support::Body { idx: si, one_way });
                }
            }
        }
    }
    env.solids[si].moved = [dx, dy];
}
