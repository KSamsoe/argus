//! `physics.rigid`: the dynamic tier's tick. In the sim stage, after the
//! kinematic move and before the overlap pass:
//!
//! 1. The solid tiles of every Tilemap become fixed colliders, rebuilt
//!    when a map's cells change.
//! 2. The kinematic tier's bodies become Rapier's kinematic bodies, moved
//!    to where `physics.move` left them, so they push rigid bodies.
//! 3. Rigid bodies enter, change, and leave in entity order; a velocity or
//!    a Transform that gameplay changed since the last tick is a throw or
//!    a teleport.
//! 4. Joints are built from their components.
//! 5. Rapier steps once, by the tick's dt.
//! 6. Positions, angles, velocities, and sleep are written back; joints
//!    past their break force break; contacts that began or ended are
//!    events, sorted.

use crate::store::{Body, Given, JointRec, MapRec, Proxy, RigidState, RigidStore, Wrote};
use crate::{Joint, RigidBody, RigidWorld, RIGID};
use argus_ecs::{
    ArgusError, ComponentId, Entity, GlobalTransform, Result, Transform, Value, Vec2, World,
};
use argus_physics::Collider;
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use argus_tiles::{MapStore, Tilemap};
use rapier2d::prelude::*;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

#[derive(Clone, Copy)]
struct Ids {
    rb: ComponentId,
    jt: ComponentId,
    rw: ComponentId,
    col: ComponentId,
    kb: ComponentId,
    tm: ComponentId,
    tr: ComponentId,
    gt: ComponentId,
    parent: ComponentId,
}

pub(crate) fn install(
    world: &mut World,
    sched: &mut Scheduler,
    tiles: Rc<RefCell<MapStore>>,
    store: Rc<RefCell<RigidStore>>,
) -> Result<()> {
    let id = |w: &World, n: &str| {
        w.component_id(n)
            .ok_or_else(|| ArgusError::new(1, format!("{n} is not registered")))
    };
    let ids = Ids {
        rb: id(world, "RigidBody")?,
        jt: id(world, "Joint")?,
        rw: id(world, "RigidWorld")?,
        col: id(world, "Collider")?,
        kb: id(world, "KinematicBody")?,
        tm: id(world, "Tilemap")?,
        tr: id(world, "Transform")?,
        gt: id(world, "GlobalTransform")?,
        parent: id(world, "Parent")?,
    };
    sched.add_system(
        SystemDesc::new(RIGID, Stage::Sim)
            .last()
            .after(argus_physics::MOVE)
            .reads(ids.col)
            .reads(ids.kb)
            .reads(ids.tm)
            .reads(ids.gt)
            .reads(ids.rw)
            .reads(ids.parent)
            .writes(ids.tr)
            .writes(ids.rb)
            .writes(ids.jt)
            .source("argus-rigid:step")
            .doc("Steps the rigid bodies (Rapier): the solid tiles and the kinematic bodies as what they hit, then gravity, contacts, and joints. Writes Transform and RigidBody, breaks joints past their break_force, and emits contact.begin, contact.end, and joint.broken."),
        Box::new(move |ctx| step(ctx, &tiles, &store, ids)),
    )?;
    // The overlap pass reports where the rigid bodies are now.
    sched.order_after(argus_physics::OVERLAP, RIGID)?;
    Ok(())
}

fn any_with(ctx: &SystemCtx<'_>, c: ComponentId) -> bool {
    ctx.world()
        .archetypes()
        .iter()
        .any(|a| !a.is_empty() && a.has(c))
}

fn v(p: Vec2) -> Vector {
    Vector::new(p.x, p.y)
}

fn groups(c: &Collider) -> InteractionGroups {
    InteractionGroups::new(
        Group::from_bits_retain(c.layer),
        Group::from_bits_retain(c.mask),
        InteractionTestMode::And,
    )
}

fn shape(c: &Collider) -> ColliderBuilder {
    let b = if c.radius > 0.0 {
        ColliderBuilder::ball(c.radius)
    } else {
        ColliderBuilder::cuboid(c.size.x.max(1e-4) * 0.5, c.size.y.max(1e-4) * 0.5)
    };
    b.translation(v(c.offset)).collision_groups(groups(c))
}

fn position(ctx: &SystemCtx<'_>, e: Entity, gt: ComponentId) -> [f32; 2] {
    if ctx.has(e, gt) {
        if let Ok(g) = ctx.get::<GlobalTransform>(e) {
            return [g.pos.x, g.pos.y];
        }
    }
    ctx.get::<Transform>(e)
        .map(|t| [t.pos.x, t.pos.y])
        .unwrap_or([0.0, 0.0])
}

fn step(
    ctx: &mut SystemCtx<'_>,
    tiles: &Rc<RefCell<MapStore>>,
    store: &Rc<RefCell<RigidStore>>,
    ids: Ids,
) -> Result<()> {
    let mut st = store.borrow_mut();
    // A scene with no rigid bodies pays this check and nothing else.
    if !any_with(ctx, ids.rb) && st.state.is_empty() {
        return Ok(());
    }
    let RigidStore {
        state: s,
        pipeline,
        ccd,
    } = &mut *st;
    let settings = ctx
        .query()
        .has(ids.rw)
        .entities()
        .first()
        .map(|&e| ctx.get::<RigidWorld>(e))
        .transpose()?
        .unwrap_or_default();
    s.gravity = [settings.gravity.x, settings.gravity.y];
    s.params.dt = ctx.dt as f32;
    s.params.num_solver_iterations = settings.iterations.max(1) as usize;

    sync_maps(ctx, tiles, s, ids)?;
    sync_proxies(ctx, s, ids)?;
    sync_bodies(ctx, s, ids)?;
    let refused = sync_joints(ctx, s, ids)?;
    for (e, why) in refused {
        ctx.emit(
            "joint.refused",
            vec![
                ("joint".into(), Value::Entity(e)),
                ("why".into(), Value::Str(why)),
            ],
        );
    }

    pipeline.step(
        Vector::new(s.gravity[0], s.gravity[1]),
        &s.params,
        &mut s.islands,
        &mut s.broad_phase,
        &mut s.narrow_phase,
        &mut s.bodies,
        &mut s.colliders,
        &mut s.impulse_joints,
        &mut s.multibody_joints,
        &mut s.soft_bodies,
        ccd,
        &(),
        &(),
    );

    write_back(ctx, s)?;
    break_joints(ctx, s)?;
    contacts(ctx, s);
    Ok(())
}

/// Step 1: every Tilemap's solid tiles, as one fixed body each.
fn sync_maps(
    ctx: &SystemCtx<'_>,
    tiles: &Rc<RefCell<MapStore>>,
    s: &mut RigidState,
    ids: Ids,
) -> Result<()> {
    let mut store = tiles.borrow_mut();
    let mut ents = ctx.query().has(ids.tm).entities();
    ents.sort();
    let mut seen = BTreeSet::new();
    for e in ents {
        let t: Tilemap = ctx.get(e)?;
        let file = ctx.world().strings().resolve(t.map);
        if file.is_empty() {
            continue;
        }
        let origin = position(ctx, e, ids.gt);
        let Some(grid) = store.grid(&file, origin) else {
            continue;
        };
        let Some(rev) = store.revision(&file) else {
            continue;
        };
        let k = e.to_bits();
        seen.insert(k);
        let key = (file, [origin[0].to_bits(), origin[1].to_bits()], rev);
        if s.maps.get(&k).is_some_and(|m| m.key == key) {
            continue;
        }
        if let Some(old) = s.maps.remove(&k) {
            remove_body(s, old.body);
        }
        let body = s
            .bodies
            .insert(RigidBodyBuilder::fixed().user_data(k as u128));
        for c in crate::tiles::colliders(&mut store, &grid) {
            s.colliders
                .insert_with_parent(c.user_data(k as u128), body, &mut s.bodies);
        }
        s.maps.insert(k, MapRec { body, key });
    }
    let gone: Vec<u64> = s
        .maps
        .keys()
        .filter(|k| !seen.contains(k))
        .copied()
        .collect();
    for k in gone {
        if let Some(m) = s.maps.remove(&k) {
            remove_body(s, m.body);
        }
    }
    Ok(())
}

fn remove_body(s: &mut RigidState, h: RigidBodyHandle) {
    s.bodies.remove(
        h,
        &mut s.islands,
        &mut s.colliders,
        &mut s.impulse_joints,
        &mut s.multibody_joints,
        &mut s.soft_bodies,
        true,
    );
}

/// Step 2: the kinematic tier's bodies, where `physics.move` put them.
fn sync_proxies(ctx: &SystemCtx<'_>, s: &mut RigidState, ids: Ids) -> Result<()> {
    let mut ents = ctx
        .query()
        .has(ids.kb)
        .has(ids.col)
        .has(ids.tr)
        .not(ids.rb)
        .entities();
    ents.sort();
    let mut seen = BTreeSet::new();
    for e in ents {
        let c: Collider = ctx.get(e)?;
        let t: Transform = ctx.get(e)?;
        let k = e.to_bits();
        seen.insert(k);
        let shape_key = [c.size.x, c.size.y, c.offset.x, c.offset.y];
        match s.proxies.get_mut(&k) {
            Some(p) => {
                if p.shape != shape_key {
                    if let Some(col) = s.colliders.get_mut(p.collider) {
                        col.set_shape(SharedShape::cuboid(
                            c.size.x.max(1e-4) * 0.5,
                            c.size.y.max(1e-4) * 0.5,
                        ));
                        col.set_translation_wrt_parent(v(c.offset));
                    }
                    p.shape = shape_key;
                }
                if let Some(b) = s.bodies.get_mut(p.body) {
                    b.set_next_kinematic_translation(v(t.pos));
                }
            }
            None => {
                let body = s.bodies.insert(
                    RigidBodyBuilder::kinematic_position_based()
                        .translation(v(t.pos))
                        .user_data(k as u128),
                );
                let boxed = Collider { radius: 0.0, ..c };
                let collider = s.colliders.insert_with_parent(
                    shape(&boxed).user_data(k as u128),
                    body,
                    &mut s.bodies,
                );
                s.proxies.insert(
                    k,
                    Proxy {
                        body,
                        collider,
                        shape: shape_key,
                    },
                );
            }
        }
    }
    let gone: Vec<u64> = s
        .proxies
        .keys()
        .filter(|k| !seen.contains(k))
        .copied()
        .collect();
    for k in gone {
        if let Some(p) = s.proxies.remove(&k) {
            remove_body(s, p.body);
        }
    }
    Ok(())
}

fn given(r: &RigidBody, c: &Collider) -> Given {
    Given {
        kind: if r.fixed {
            1
        } else if r.kinematic {
            2
        } else {
            0
        },
        gravity_scale: r.gravity_scale,
        damping: r.damping,
        angular_damping: r.angular_damping,
        lock_rotation: r.lock_rotation,
        ccd: r.ccd,
        can_sleep: r.can_sleep,
        size: [c.size.x, c.size.y],
        radius: c.radius,
        offset: [c.offset.x, c.offset.y],
        density: r.density,
        friction: r.friction,
        restitution: r.restitution,
        layer: c.layer,
        mask: c.mask,
    }
}

fn body_type(kind: u8) -> RigidBodyType {
    match kind {
        1 => RigidBodyType::Fixed,
        2 => RigidBodyType::KinematicVelocityBased,
        _ => RigidBodyType::Dynamic,
    }
}

fn wrote(t: &Transform, r: &RigidBody) -> Wrote {
    Wrote {
        pos: [t.pos.x, t.pos.y],
        rot: t.rot,
        vel: [r.velocity.x, r.velocity.y],
        spin: r.spin,
    }
}

fn add_body(s: &mut RigidState, k: u64, t: &Transform, r: &RigidBody, c: &Collider) -> Body {
    let g = given(r, c);
    let mut b = RigidBodyBuilder::new(body_type(g.kind))
        .translation(v(t.pos))
        .rotation(t.rot)
        .linvel(v(r.velocity))
        .angvel(r.spin)
        .gravity_scale(r.gravity_scale)
        .linear_damping(r.damping)
        .angular_damping(r.angular_damping)
        .ccd_enabled(r.ccd)
        .can_sleep(r.can_sleep)
        .user_data(k as u128);
    if r.lock_rotation {
        b = b.lock_rotations();
    }
    let body = s.bodies.insert(b);
    let collider = s.colliders.insert_with_parent(
        shape(c)
            .density(r.density.max(1e-3))
            .friction(r.friction)
            .restitution(r.restitution)
            .user_data(k as u128),
        body,
        &mut s.bodies,
    );
    Body {
        body,
        collider,
        wrote: wrote(t, r),
        given: g,
    }
}

/// Step 3: rigid bodies in, changed, and out, in entity order.
fn sync_bodies(ctx: &SystemCtx<'_>, s: &mut RigidState, ids: Ids) -> Result<()> {
    let mut ents = ctx
        .query()
        .has(ids.rb)
        .has(ids.col)
        .has(ids.tr)
        .not(ids.parent)
        .entities();
    ents.sort();
    let mut seen = BTreeSet::new();
    for e in ents {
        let r: RigidBody = ctx.get(e)?;
        let c: Collider = ctx.get(e)?;
        let t: Transform = ctx.get(e)?;
        let k = e.to_bits();
        seen.insert(k);
        let g = given(&r, &c);
        let Some(rec) = s.rigid.get(&k).cloned() else {
            let b = add_body(s, k, &t, &r, &c);
            s.rigid.insert(k, b);
            continue;
        };
        // What cannot change in place starts the body again where it is.
        if g.can_sleep != rec.given.can_sleep {
            remove_body(s, rec.body);
            let b = add_body(s, k, &t, &r, &c);
            s.rigid.insert(k, b);
            continue;
        }
        let Some(body) = s.bodies.get_mut(rec.body) else {
            continue;
        };
        if g.kind != rec.given.kind {
            body.set_body_type(body_type(g.kind), true);
        }
        if g.gravity_scale != rec.given.gravity_scale {
            body.set_gravity_scale(g.gravity_scale, true);
        }
        if g.damping != rec.given.damping {
            body.set_linear_damping(g.damping);
        }
        if g.angular_damping != rec.given.angular_damping {
            body.set_angular_damping(g.angular_damping);
        }
        if g.ccd != rec.given.ccd {
            body.enable_ccd(g.ccd);
        }
        if g.lock_rotation != rec.given.lock_rotation {
            body.lock_rotations(g.lock_rotation, true);
        }
        // A Transform or a velocity that is not what this tier wrote last
        // tick is gameplay's: a teleport, a throw.
        if [t.pos.x, t.pos.y] != rec.wrote.pos || t.rot != rec.wrote.rot {
            body.set_translation(v(t.pos), true);
            body.set_rotation(Rotation::new(t.rot), true);
        }
        if [r.velocity.x, r.velocity.y] != rec.wrote.vel {
            body.set_linvel(v(r.velocity), true);
        }
        if r.spin != rec.wrote.spin {
            body.set_angvel(r.spin, true);
        }
        let shaped = g.size != rec.given.size
            || g.radius != rec.given.radius
            || g.offset != rec.given.offset
            || g.layer != rec.given.layer
            || g.mask != rec.given.mask;
        if let Some(col) = s.colliders.get_mut(rec.collider) {
            if shaped {
                col.set_shape(if c.radius > 0.0 {
                    SharedShape::ball(c.radius)
                } else {
                    SharedShape::cuboid(c.size.x.max(1e-4) * 0.5, c.size.y.max(1e-4) * 0.5)
                });
                col.set_translation_wrt_parent(v(c.offset));
                col.set_collision_groups(groups(&c));
            }
            if g.density != rec.given.density {
                col.set_density(g.density.max(1e-3));
            }
            if g.friction != rec.given.friction {
                col.set_friction(g.friction);
            }
            if g.restitution != rec.given.restitution {
                col.set_restitution(g.restitution);
            }
        }
        if let Some(b) = s.rigid.get_mut(&k) {
            b.given = g;
        }
    }
    let gone: Vec<u64> = s
        .rigid
        .keys()
        .filter(|k| !seen.contains(k))
        .copied()
        .collect();
    for k in gone {
        if let Some(b) = s.rigid.remove(&k) {
            remove_body(s, b.body);
        }
    }
    Ok(())
}

fn bits(x: f32) -> u32 {
    x.to_bits()
}

/// Step 4: joints, built again when their component changes. Returns the
/// joints refused this tick, with why, the first time each is.
fn sync_joints(ctx: &SystemCtx<'_>, s: &mut RigidState, ids: Ids) -> Result<Vec<(Entity, String)>> {
    let mut ents = ctx.query().has(ids.jt).entities();
    ents.sort();
    let mut seen = BTreeSet::new();
    let mut refused = Vec::new();
    for e in ents {
        let j: Joint = ctx.get(e)?;
        let k = e.to_bits();
        let a = s.rigid.get(&j.a.to_bits()).map(|b| b.body);
        let b = if j.b == Entity::NULL {
            Some(
                *s.ground
                    .get_or_insert_with(|| s.bodies.insert(RigidBodyBuilder::fixed())),
            )
        } else {
            s.rigid.get(&j.b.to_bits()).map(|b| b.body)
        };
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        if j.broken {
            continue;
        }
        let kind = ctx.world().strings().resolve(j.kind);
        let key = vec![
            kind.len() as u32,
            kind.bytes()
                .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32)),
            j.a.index(),
            j.a.generation(),
            j.b.index(),
            j.b.generation(),
            bits(j.anchor_a.x),
            bits(j.anchor_a.y),
            bits(j.anchor_b.x),
            bits(j.anchor_b.y),
            bits(j.axis.x),
            bits(j.axis.y),
            bits(j.min),
            bits(j.max),
            bits(j.length),
            bits(j.stiffness),
            bits(j.damping),
            bits(j.motor_speed),
            bits(j.motor_force),
            bits(j.break_force),
            j.collide as u32,
        ];
        if let Some(rec) = s.joints.get(&k) {
            if rec.key == key && s.impulse_joints.get(rec.joint).is_some() {
                seen.insert(k);
                continue;
            }
            if rec.key == key && rec.joint == ImpulseJointHandle::invalid() {
                // Refused before, and nothing changed.
                seen.insert(k);
                continue;
            }
        }
        if let Some(old) = s.joints.remove(&k) {
            s.impulse_joints.remove(old.joint, true);
        }
        seen.insert(k);
        let (p1, p2) = (v(j.anchor_a), v(j.anchor_b));
        let limited = j.min < j.max;
        let motor = j.motor_force > 0.0;
        let joint: GenericJoint = match kind.as_str() {
            "revolute" | "" => {
                let mut b = RevoluteJointBuilder::new()
                    .local_anchor1(p1)
                    .local_anchor2(p2)
                    .contacts_enabled(j.collide);
                if limited {
                    b = b.limits([j.min, j.max]);
                }
                if motor {
                    b = b
                        .motor_velocity(j.motor_speed, j.damping.max(0.01))
                        .motor_max_force(j.motor_force);
                }
                b.build().into()
            }
            "fixed" => FixedJointBuilder::new()
                .local_anchor1(p1)
                .local_anchor2(p2)
                .contacts_enabled(j.collide)
                .build()
                .into(),
            "prismatic" => {
                let axis = if j.axis.x == 0.0 && j.axis.y == 0.0 {
                    Vector::new(1.0, 0.0)
                } else {
                    v(j.axis).normalize()
                };
                let mut b = PrismaticJointBuilder::new(axis)
                    .local_anchor1(p1)
                    .local_anchor2(p2)
                    .contacts_enabled(j.collide);
                if limited {
                    b = b.limits([j.min, j.max]);
                }
                if motor {
                    b = b
                        .motor_velocity(j.motor_speed, j.damping.max(0.01))
                        .motor_max_force(j.motor_force);
                }
                b.build().into()
            }
            "rope" => RopeJointBuilder::new(j.length.max(0.0))
                .local_anchor1(p1)
                .local_anchor2(p2)
                .contacts_enabled(j.collide)
                .build()
                .into(),
            "spring" => SpringJointBuilder::new(j.length.max(0.0), j.stiffness, j.damping)
                .local_anchor1(p1)
                .local_anchor2(p2)
                .contacts_enabled(j.collide)
                .build()
                .into(),
            other => {
                refused.push((
                    e,
                    format!("{other:?} is not a joint kind: revolute, fixed, prismatic, rope, or spring"),
                ));
                s.joints.insert(
                    k,
                    JointRec {
                        joint: ImpulseJointHandle::invalid(),
                        key,
                        break_force: 0.0,
                    },
                );
                continue;
            }
        };
        let handle = s.impulse_joints.insert(a, b, joint, true);
        s.joints.insert(
            k,
            JointRec {
                joint: handle,
                key,
                break_force: j.break_force,
            },
        );
    }
    let gone: Vec<u64> = s
        .joints
        .keys()
        .filter(|k| !seen.contains(k))
        .copied()
        .collect();
    for k in gone {
        if let Some(rec) = s.joints.remove(&k) {
            s.impulse_joints.remove(rec.joint, true);
        }
    }
    Ok(refused)
}

/// Step 6: where the step left every body that moves.
fn write_back(ctx: &SystemCtx<'_>, s: &mut RigidState) -> Result<()> {
    for (&k, rec) in s.rigid.iter_mut() {
        if rec.given.kind == 1 {
            continue;
        }
        let Some(b) = s.bodies.get(rec.body) else {
            continue;
        };
        let p = b.translation();
        let a = b.rotation().angle();
        let lv = b.linvel();
        let w = b.angvel();
        let asleep = b.is_sleeping();
        let now = Wrote {
            pos: [p.x, p.y],
            rot: a,
            vel: [lv.x, lv.y],
            spin: w,
        };
        // A body asleep where it was costs no writes.
        if asleep && now == rec.wrote {
            continue;
        }
        let e = Entity::from_bits(k);
        ctx.update::<Transform, _>(e, |t| {
            t.pos = Vec2::new(p.x, p.y);
            t.rot = a;
        })?;
        ctx.update::<RigidBody, _>(e, |r| {
            r.velocity = Vec2::new(lv.x, lv.y);
            r.spin = w;
            r.sleeping = asleep;
        })?;
        rec.wrote = now;
    }
    Ok(())
}

/// Joints whose last step's force passed their break force break.
fn break_joints(ctx: &mut SystemCtx<'_>, s: &mut RigidState) -> Result<()> {
    // Rapier keeps the impulse of a tick's last substep (it takes
    // `num_solver_iterations` of them), so a force is that over a
    // substep's time, not the tick's.
    let dt = (s.params.dt / s.params.num_solver_iterations.max(1) as f32).max(1e-9);
    let mut broke = Vec::new();
    for (&k, rec) in &s.joints {
        if rec.break_force <= 0.0 {
            continue;
        }
        let Some(j) = s.impulse_joints.get(rec.joint) else {
            continue;
        };
        // The joint's own rows, and its linear limits and motors, which
        // Rapier keeps apart (a rope pulls through its limit, a spring
        // through its motor).
        let i = j.impulses;
        let held: f32 = (0..2)
            .map(|a| j.data.limits[a].impulse.abs() + j.data.motors[a].impulse.abs())
            .sum();
        let force = ((i.x * i.x + i.y * i.y).sqrt() + held) / dt;
        if force > rec.break_force {
            broke.push((k, rec.joint, force));
        }
    }
    for (k, h, force) in broke {
        s.impulse_joints.remove(h, true);
        if let Some(rec) = s.joints.get_mut(&k) {
            rec.joint = ImpulseJointHandle::invalid();
        }
        let e = Entity::from_bits(k);
        let j = ctx.update::<Joint, _>(e, |j| {
            j.broken = true;
            *j
        })?;
        ctx.emit(
            "joint.broken",
            vec![
                ("joint".into(), Value::Entity(e)),
                ("a".into(), Value::Entity(j.a)),
                ("b".into(), Value::Entity(j.b)),
                ("force".into(), Value::Float(force as f64)),
            ],
        );
    }
    Ok(())
}

/// Contacts that began or ended, sorted, the lower entity first; a pair
/// counts when one of the two is a rigid body.
fn contacts(ctx: &mut SystemCtx<'_>, s: &mut RigidState) {
    let mut now: Vec<(u64, u64, f32)> = Vec::new();
    for pair in s.narrow_phase.contact_pairs() {
        if !pair.has_any_active_contact() {
            continue;
        }
        let (Some(c1), Some(c2)) = (
            s.colliders.get(pair.collider1),
            s.colliders.get(pair.collider2),
        ) else {
            continue;
        };
        let (u1, u2) = (c1.user_data as u64, c2.user_data as u64);
        if !s.rigid.contains_key(&u1) && !s.rigid.contains_key(&u2) {
            continue;
        }
        let (a, b) = if u1 <= u2 { (u1, u2) } else { (u2, u1) };
        now.push((a, b, pair.total_impulse_magnitude()));
    }
    now.sort_by(|x, y| (x.0, x.1).cmp(&(y.0, y.1)).then(y.2.total_cmp(&x.2)));
    now.dedup_by(|x, y| x.0 == y.0 && x.1 == y.1);
    let pairs: Vec<(u64, u64)> = now.iter().map(|p| (p.0, p.1)).collect();
    let (mut i, mut j) = (0, 0);
    let before = std::mem::take(&mut s.touching);
    while i < pairs.len() || j < before.len() {
        match (pairs.get(i), before.get(j)) {
            (Some(a), Some(b)) if a == b => {
                i += 1;
                j += 1;
            }
            (Some(a), b) if b.is_none_or(|b| a < b) => {
                ctx.emit(
                    "contact.begin",
                    vec![
                        ("entity".into(), Value::Entity(Entity::from_bits(a.0))),
                        ("other".into(), Value::Entity(Entity::from_bits(a.1))),
                        ("impulse".into(), Value::Float(now[i].2 as f64)),
                    ],
                );
                i += 1;
            }
            (_, Some(b)) => {
                ctx.emit(
                    "contact.end",
                    vec![
                        ("entity".into(), Value::Entity(Entity::from_bits(b.0))),
                        ("other".into(), Value::Entity(Entity::from_bits(b.1))),
                    ],
                );
                j += 1;
            }
            _ => unreachable!(),
        }
    }
    s.touching = pairs;
}
