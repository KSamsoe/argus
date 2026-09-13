//! Transform hierarchy (§3 "Transform hierarchy is first-class"). A `Parent`
//! component gives an entity a local transform relative to another;
//! [`propagate`] computes `GlobalTransform` for every entity that has one,
//! and runs in the dedicated `hierarchy` stage before `present`. Entities
//! without `Parent` are not visited beyond a straight copy, so Swarm's
//! bullets pay nothing.

use crate::entity::Entity;
use crate::error::Result;
use crate::event::Event;
use crate::value::Vec2;
use crate::world::World;
use crate::Component;
use std::collections::HashMap;

/// Local transform: position, rotation in radians, scale.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Position, rotation (radians) and scale. Local to the Parent if one is attached, else world space."
)]
pub struct Transform {
    /// Position.
    #[argus(doc = "Position in world units")]
    pub pos: Vec2,
    /// Rotation in radians.
    #[argus(doc = "Rotation in radians, counter-clockwise")]
    pub rot: f32,
    /// Scale.
    #[argus(doc = "Scale per axis; 1 is unscaled")]
    pub scale: Vec2,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            pos: Vec2::ZERO,
            rot: 0.0,
            scale: Vec2::ONE,
        }
    }
}

impl Transform {
    /// Translation only.
    pub fn at(x: f32, y: f32) -> Transform {
        Transform {
            pos: Vec2::new(x, y),
            ..Default::default()
        }
    }
}

/// World-space transform, written by [`propagate`]. Read-only for game code.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "World-space transform computed by the hierarchy stage. Do not write; edit Transform instead."
)]
pub struct GlobalTransform {
    /// World position.
    #[argus(doc = "World position")]
    pub pos: Vec2,
    /// World rotation.
    #[argus(doc = "World rotation in radians")]
    pub rot: f32,
    /// World scale.
    #[argus(doc = "World scale per axis")]
    pub scale: Vec2,
}

impl Default for GlobalTransform {
    fn default() -> Self {
        GlobalTransform {
            pos: Vec2::ZERO,
            rot: 0.0,
            scale: Vec2::ONE,
        }
    }
}

impl GlobalTransform {
    /// Compose a child's local transform under this world transform.
    pub fn compose(&self, local: &Transform) -> GlobalTransform {
        let (s, c) = self.rot.sin_cos();
        let sx = local.pos.x * self.scale.x;
        let sy = local.pos.y * self.scale.y;
        GlobalTransform {
            pos: Vec2::new(self.pos.x + sx * c - sy * s, self.pos.y + sx * s + sy * c),
            rot: self.rot + local.rot,
            scale: Vec2::new(self.scale.x * local.scale.x, self.scale.y * local.scale.y),
        }
    }
    /// A root's world transform is its local transform.
    pub fn from_local(local: &Transform) -> GlobalTransform {
        GlobalTransform {
            pos: local.pos,
            rot: local.rot,
            scale: local.scale,
        }
    }
}

/// Attaches an entity under another. A null or dead parent makes the entity
/// a root; dead parents are nulled by `World::maintain`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Parent entity. Transform becomes local to it. Nulled automatically if the parent despawns."
)]
pub struct Parent {
    /// The parent entity.
    #[argus(doc = "Parent entity reference")]
    pub entity: Entity,
}

/// Register `Transform`, `GlobalTransform`, and `Parent`.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Transform>()?;
    world.register::<GlobalTransform>()?;
    world.register::<Parent>()?;
    Ok(())
}

/// Compute `GlobalTransform` for every entity that has `Transform` and
/// `GlobalTransform`. Roots copy; children compose in depth order. Cycles
/// are reported with `Event::HierarchyCycle` and treated as roots.
pub fn propagate(world: &mut World) -> Result<()> {
    if world.component_id_of::<Parent>().is_none() {
        register(world)?;
    }

    // Roots: a straight copy, one archetype at a time.
    world
        .query()
        .with::<Transform>()
        .with::<GlobalTransform>()
        .without::<Parent>()
        .each::<(&Transform, &mut GlobalTransform)>(|_, (t, g)| {
            *g = GlobalTransform::from_local(t)
        })?;

    // Children: collect, order by depth, compose.
    let mut children: Vec<(Entity, Entity)> = Vec::new();
    world
        .query()
        .with::<Transform>()
        .with::<GlobalTransform>()
        .each::<&Parent>(|e, p| children.push((e, p.entity)))?;
    if children.is_empty() {
        return Ok(());
    }

    let parent_of: HashMap<Entity, Entity> = children.iter().copied().collect();
    let mut depth: HashMap<Entity, u32> = HashMap::with_capacity(children.len());
    let mut cycles: Vec<Entity> = Vec::new();
    let mut effective_parent: HashMap<Entity, Entity> = HashMap::with_capacity(children.len());

    for &(e, _) in &children {
        if depth.contains_key(&e) {
            continue;
        }
        // Walk up until a known depth, a root, or a repeat.
        let mut chain = vec![e];
        let mut cur = e;
        let base_depth;
        loop {
            let p = parent_of.get(&cur).copied().unwrap_or(Entity::NULL);
            let p_ok = !p.is_null() && world.is_alive(p) && world.has_type::<Transform>(p);
            if !p_ok {
                // cur is effectively a root child (parent missing or untransformed).
                base_depth = 0;
                break;
            }
            if let Some(&d) = depth.get(&p) {
                base_depth = d;
                break;
            }
            if !parent_of.contains_key(&p) {
                // p is a true root (has Transform, no Parent): depth 0.
                base_depth = 0;
                break;
            }
            if chain.contains(&p) {
                // Cycle: break it at `cur`, which becomes a root child.
                cycles.push(cur);
                effective_parent.insert(cur, Entity::NULL);
                base_depth = 0;
                break;
            }
            chain.push(p);
            cur = p;
        }
        // Assign depths down the chain: last pushed is nearest the base.
        let mut d = base_depth;
        for &x in chain.iter().rev() {
            d += 1;
            depth.insert(x, d);
        }
    }

    let mut order: Vec<(u32, Entity, Entity)> = children
        .iter()
        .map(|&(e, p)| (depth[&e], e, effective_parent.get(&e).copied().unwrap_or(p)))
        .collect();
    order.sort_by_key(|&(d, e, _)| (d, e));

    for (_, e, p) in order {
        let local = world.get::<Transform>(e)?;
        let parent_global = if !p.is_null() && world.is_alive(p) {
            match world.get::<GlobalTransform>(p) {
                Ok(g) => Some(g),
                Err(_) => world
                    .get::<Transform>(p)
                    .ok()
                    .map(|t| GlobalTransform::from_local(&t)),
            }
        } else {
            None
        };
        let g = match parent_global {
            Some(pg) => pg.compose(&local),
            None => GlobalTransform::from_local(&local),
        };
        world.update::<GlobalTransform, _>(e, |slot| *slot = g)?;
    }

    for e in cycles {
        world.emit(Event::HierarchyCycle { entity: e });
    }
    Ok(())
}
