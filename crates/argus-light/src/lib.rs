//! 2D lighting (§12, phase 5 item 3): "2D lighting as a render pass: point
//! and cone lights, occluders derived from tiles and sprites, normal-mapped
//! sprites optional; off by default and zero-cost when off."
//!
//! The components a game puts on its entities, and what the renderer needs
//! from the world to draw them (argus-render draws them):
//!
//! - [`Lighting`] on the drawing camera turns lighting on: the ambient
//!   light everything gets, and whether tiles cast shadows. Without it
//!   nothing here runs and nothing is drawn differently.
//! - [`Light`] on any entity: a point light, or a cone when its angle is
//!   under 180 degrees, with a colour, an intensity, a radius, and a falloff,
//!   casting shadows unless told not to.
//! - [`Occluder`] on an entity: a box (its sprite's, by default) that casts
//!   shadows. Solid tiles cast them too: every solid and slope cell of a
//!   tilemap's collision layers, the collision the physics uses.
//! - [`Emissive`] on a sprite: it shows at its own colour whatever the light.
//!
//! A shadow starts behind an occluder, on its edges that face away from the
//! light, so a wall the lantern shines on is lit and the cave beyond it is
//! dark. The components are registered with no systems: a game that does
//! not light its scenes pays nothing in its ticks.

#![warn(missing_docs)]

use argus_ecs::{Component, Entity, GlobalTransform, Result, StrId, Transform, Vec2, World};
use argus_tiles::{Collide, MapStore, Tilemap};

/// A light: a point light, or a cone when `angle` is under 180 degrees.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A 2D light (seen when the drawing camera has Lighting): a point light, or a cone when angle is under 180 degrees, at the entity's position plus offset, fading to nothing at radius, casting shadows from solid tiles and Occluders"
)]
pub struct Light {
    /// Its colour.
    #[argus(doc = "Colour, #RRGGBB; empty for white", tune)]
    pub color: StrId,
    /// How bright at its centre.
    #[argus(
        doc = "Brightness at its centre, 1 the colour as it is",
        range(0, 16),
        tune
    )]
    pub intensity: f32,
    /// Where it has faded to nothing.
    #[argus(
        doc = "World units at which it has faded to nothing",
        range(0, 10000),
        tune
    )]
    pub radius: f32,
    /// How it fades.
    #[argus(
        doc = "How it fades: brightness is (1 - distance / radius) to this power; 1 is linear, 2 softer at the edge",
        range(0.1, 8),
        tune
    )]
    pub falloff: f32,
    /// Half the cone's width.
    #[argus(
        doc = "Half the cone's width in degrees; 180 is a point light, shining every way",
        range(0, 180),
        tune
    )]
    pub angle: f32,
    /// How gradual the cone's edge is.
    #[argus(
        doc = "How much of the cone's half-width its edge fades over, 0 (sharp) to 1",
        range(0, 1),
        tune
    )]
    pub softness: f32,
    /// Where the cone points.
    #[argus(
        doc = "Where the cone points, in degrees from +x counterclockwise, added to the entity's rotation",
        tune
    )]
    pub direction: f32,
    /// From the entity's position.
    #[argus(doc = "Offset from the entity's position, in its frame (turned and flipped with it)")]
    pub offset: Vec2,
    /// Height above the picture, for normal-mapped sprites.
    #[argus(
        doc = "Height above the picture in world units, for normal-mapped sprites: low lights rake across their bumps",
        range(0, 1000),
        tune
    )]
    pub height: f32,
    /// How much it flickers.
    #[argus(
        doc = "How much its brightness flickers, 0 (steady) to 1, the same at the same sim time",
        range(0, 1),
        tune
    )]
    pub flicker: f32,
    /// Shadows.
    #[argus(doc = "Casts shadows from solid tiles and Occluders")]
    pub shadows: bool,
    /// Shining at all.
    #[argus(doc = "Shining at all")]
    pub on: bool,
}

impl Default for Light {
    fn default() -> Self {
        Light {
            color: StrId::EMPTY,
            intensity: 1.0,
            radius: 8.0,
            falloff: 2.0,
            angle: 180.0,
            softness: 0.2,
            direction: 0.0,
            offset: Vec2::ZERO,
            height: 1.0,
            flicker: 0.0,
            shadows: true,
            on: true,
        }
    }
}

/// Lighting for what a camera shows: its presence turns lighting on.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "2D lighting for what this camera shows (off without it): everything is lit by ambient times level, plus every Light in reach; the picture is multiplied by the light before the post passes"
)]
pub struct Lighting {
    /// The light everywhere.
    #[argus(doc = "The light everywhere, #RRGGBB; empty for none (black)", tune)]
    pub ambient: StrId,
    /// How much of it.
    #[argus(
        doc = "The ambient light's strength: 1 the colour as it is (a day's cycle tweens this)",
        range(0, 4),
        tune
    )]
    pub level: f32,
    /// Shadows at all.
    #[argus(doc = "Lights cast shadows (each Light may also turn its own off)")]
    pub shadows: bool,
    /// Solid tiles cast them.
    #[argus(doc = "Solid and slope tiles of every collision layer cast shadows")]
    pub tiles: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            ambient: StrId::EMPTY,
            level: 1.0,
            shadows: true,
            tiles: true,
        }
    }
}

/// A box that casts shadows.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Casts shadows from a box: the entity's Sprite's size by default, turned and scaled with the entity; its own Light and its parent's are not shadowed by it"
)]
pub struct Occluder {
    /// The box's size.
    #[argus(doc = "The box's size in world units; zero for the entity's Sprite.size")]
    pub size: Vec2,
    /// From the entity's position.
    #[argus(doc = "The box's centre from the entity's position, in its frame")]
    pub offset: Vec2,
    /// Casting at all.
    #[argus(doc = "Casting shadows at all")]
    pub on: bool,
}

impl Default for Occluder {
    fn default() -> Self {
        Occluder {
            size: Vec2::ZERO,
            offset: Vec2::ZERO,
            on: true,
        }
    }
}

/// A sprite that shows at its own colour whatever the light.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "The entity's Sprite shows at its own colour in the dark, by amount: a glowing gem, a lit window (a material may say emissive = true for every sprite and particle drawn with it)"
)]
pub struct Emissive {
    /// How much.
    #[argus(
        doc = "How much, 0 (lit like the rest) to 1 (its own colour)",
        range(0, 1),
        tune
    )]
    pub amount: f32,
}

impl Default for Emissive {
    fn default() -> Self {
        Emissive { amount: 1.0 }
    }
}

/// Register the components. No systems: lighting is the renderer's.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Light>()?;
    world.register::<Lighting>()?;
    world.register::<Occluder>()?;
    world.register::<Emissive>()?;
    world.register::<Tilemap>()?;
    Ok(())
}

/// The drawing camera's lighting, if it has any: lighting is on.
pub fn lighting(world: &World) -> Option<(Entity, Lighting)> {
    world.component_id_of::<Lighting>()?;
    let cam = argus_camera::current(world)?;
    world.get::<Lighting>(cam).ok().map(|l| (cam, l))
}

/// An entity's world position, rotation, and scale.
pub fn pose(world: &World, e: Entity) -> Option<([f32; 2], f32, [f32; 2])> {
    if let Ok(g) = world.get::<GlobalTransform>(e) {
        return Some(([g.pos.x, g.pos.y], g.rot, [g.scale.x, g.scale.y]));
    }
    world
        .get::<Transform>(e)
        .ok()
        .map(|t| ([t.pos.x, t.pos.y], t.rot, [t.scale.x, t.scale.y]))
}

/// A point in an entity's frame (turned and scaled with it) in the world.
pub fn place(pos: [f32; 2], rot: f32, scale: [f32; 2], local: Vec2) -> [f32; 2] {
    let (s, c) = rot.sin_cos();
    let (x, y) = (local.x * scale[0], local.y * scale[1]);
    [pos[0] + x * c - y * s, pos[1] + x * s + y * c]
}

/// Where a light is this tick, and where it points (radians).
pub fn light_at(world: &World, e: Entity, l: &Light) -> Option<([f32; 2], f32)> {
    let (pos, rot, scale) = pose(world, e)?;
    Some(aim(pos, rot, scale, flipped(world, e), l))
}

/// Whether an entity's sprite is mirrored.
pub fn flipped(world: &World, e: Entity) -> bool {
    world
        .get_field(e, "Sprite.flip_x")
        .is_ok_and(|v| v == argus_ecs::Value::Bool(true))
}

/// A light's place and direction (radians) from its entity's pose: a
/// sprite mirrored by its flip turns the light's offset and cone with it.
pub fn aim(pos: [f32; 2], rot: f32, scale: [f32; 2], flipped: bool, l: &Light) -> ([f32; 2], f32) {
    let (offset, dir) = if flipped {
        (
            Vec2::new(-l.offset.x, l.offset.y),
            std::f32::consts::PI - l.direction.to_radians(),
        )
    } else {
        (l.offset, l.direction.to_radians())
    };
    (place(pos, rot, scale, offset), rot + dir)
}

/// How bright a flickering light is now: 1 when steady, the same at the
/// same sim time, so a replay flickers as the run did.
pub fn flicker(e: Entity, seconds: f64, amount: f32) -> f32 {
    if amount <= 0.0 {
        return 1.0;
    }
    // Three incommensurate waves, phased by the entity: never periodic to
    // the eye, never random to a replay.
    let p = (e.index() as f64 * 1.618_034).fract() * std::f64::consts::TAU;
    let t = seconds;
    let w = (t * 7.3 + p).sin() * 0.5
        + (t * 13.1 + p * 2.0).sin() * 0.3
        + (t * 23.7 + p * 3.0).sin() * 0.2;
    (1.0 - amount * (0.5 + 0.5 * w as f32)).clamp(0.0, 1.0)
}

/// One edge of an occluder: from `a` to `b`, `normal` pointing out of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// One end.
    pub a: [f32; 2],
    /// The other end.
    pub b: [f32; 2],
    /// Out of the occluder, unit length.
    pub normal: [f32; 2],
}

/// The world rect in which solid tiles can shadow what this view shows:
/// around every shadow-casting light that reaches the view (`None` when
/// lighting is off, its tiles cast no shadows, or no light casts shadows
/// into the view).
pub fn reach(world: &World, center: [f32; 2], half: [f32; 2]) -> Option<[f32; 4]> {
    let (_, lit) = lighting(world)?;
    if !lit.shadows || !lit.tiles {
        return None;
    }
    let lc = world.component_id_of::<Light>()?;
    let mut out: Option<[f32; 4]> = None;
    for e in world.query().has(lc).entities() {
        let Ok(l) = world.get::<Light>(e) else {
            continue;
        };
        if !l.on || !l.shadows || l.radius <= 0.0 || l.intensity <= 0.0 {
            continue;
        }
        let Some((p, _)) = light_at(world, e, &l) else {
            continue;
        };
        let r = l.radius;
        if (p[0] - center[0]).abs() > half[0] + r || (p[1] - center[1]).abs() > half[1] + r {
            continue;
        }
        let b = [p[0] - r, p[1] - r, p[0] + r, p[1] + r];
        out = Some(match out {
            None => b,
            Some(o) => [
                o[0].min(b[0]),
                o[1].min(b[1]),
                o[2].max(b[2]),
                o[3].max(b[3]),
            ],
        });
    }
    out
}

/// The edges of every solid and slope cell of every tilemap's collision
/// layers inside a world rect (`x0, y0, x1, y1`, y up), where a cell meets
/// one that is not: each wall a few long edges, not one per tile.
pub fn tile_edges(world: &World, store: &mut MapStore, rect: [f32; 4]) -> Vec<Edge> {
    let mut out = Vec::new();
    let Some(tm) = world.component_id_of::<Tilemap>() else {
        return out;
    };
    let mut maps = Vec::new();
    for e in world.query().has(tm).entities() {
        let Ok(t) = world.get::<Tilemap>(e) else {
            continue;
        };
        let origin = pose(world, e).map_or([0.0, 0.0], |p| p.0);
        maps.push((world.strings().resolve(t.map), origin));
    }
    for (file, origin) in maps {
        let Some(g) = store.grid(&file, origin) else {
            continue;
        };
        let c0 = g.col(rect[0]).max(0);
        let c1 = g.col(rect[2]).min(g.width as i64 - 1);
        let r0 = g.row(rect[3]).max(0);
        let r1 = g.row(rect[1]).min(g.height as i64 - 1);
        if c1 < c0 || r1 < r0 {
            continue;
        }
        // The cells with a border of one, for the neighbours.
        let (w, h) = ((c1 - c0 + 3) as usize, (r1 - r0 + 3) as usize);
        let mut solid = vec![false; w * h];
        for r in 0..h {
            for c in 0..w {
                let k = store.collide(&g, c0 - 1 + c as i64, r0 - 1 + r as i64);
                solid[r * w + c] = matches!(k, Collide::Solid | Collide::Slope { .. });
            }
        }
        let at = |c: i64, r: i64| solid[((r - r0 + 1) as usize) * w + (c - c0 + 1) as usize];
        // Along each row boundary: a cell's top face (the cell above it
        // open) and its bottom face, merged into runs.
        for r in r0..=r1 + 1 {
            let y = g.top(r);
            for (normal, open) in [([0.0, 1.0], true), ([0.0, -1.0], false)] {
                let mut run: Option<i64> = None;
                for c in c0..=c1 + 1 {
                    let face = c <= c1 && {
                        let (below, above) = (at(c, r), at(c, r - 1));
                        if open {
                            below && !above
                        } else {
                            above && !below
                        }
                    };
                    match (face, run) {
                        (true, None) => run = Some(c),
                        (false, Some(s)) => {
                            out.push(Edge {
                                a: [g.left(s), y],
                                b: [g.left(c), y],
                                normal,
                            });
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        // Along each column boundary: a cell's left face and its right.
        for c in c0..=c1 + 1 {
            let x = g.left(c);
            for (normal, open) in [([-1.0, 0.0], true), ([1.0, 0.0], false)] {
                let mut run: Option<i64> = None;
                for r in r0..=r1 + 1 {
                    let face = r <= r1 && {
                        let (right, left) = (at(c, r), at(c - 1, r));
                        if open {
                            right && !left
                        } else {
                            left && !right
                        }
                    };
                    match (face, run) {
                        (true, None) => run = Some(r),
                        (false, Some(s)) => {
                            out.push(Edge {
                                a: [x, g.top(s)],
                                b: [x, g.top(r)],
                                normal,
                            });
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

/// The four edges of a box (centre, half size, rotation).
pub fn box_edges(center: [f32; 2], half: [f32; 2], rot: f32) -> [Edge; 4] {
    let (s, c) = rot.sin_cos();
    let at = |x: f32, y: f32| [center[0] + x * c - y * s, center[1] + x * s + y * c];
    let turn = |x: f32, y: f32| [x * c - y * s, x * s + y * c];
    let (hx, hy) = (half[0], half[1]);
    let (p00, p10, p11, p01) = (at(-hx, -hy), at(hx, -hy), at(hx, hy), at(-hx, hy));
    [
        Edge {
            a: p00,
            b: p10,
            normal: turn(0.0, -1.0),
        },
        Edge {
            a: p10,
            b: p11,
            normal: turn(1.0, 0.0),
        },
        Edge {
            a: p11,
            b: p01,
            normal: turn(0.0, 1.0),
        },
        Edge {
            a: p01,
            b: p00,
            normal: turn(-1.0, 0.0),
        },
    ]
}

/// Every Occluder's edges, with the entity casting them.
pub fn occluder_edges(world: &World) -> Vec<(Entity, Edge)> {
    let mut out = Vec::new();
    let Some(oc) = world.component_id_of::<Occluder>() else {
        return out;
    };
    for e in world.query().has(oc).entities() {
        let Ok(o) = world.get::<Occluder>(e) else {
            continue;
        };
        if !o.on {
            continue;
        }
        let Some((pos, rot, scale)) = pose(world, e) else {
            continue;
        };
        let size = if o.size == Vec2::ZERO {
            match world.get_field(e, "Sprite.size") {
                Ok(argus_ecs::Value::Vec2(v)) => v,
                _ => continue,
            }
        } else {
            o.size
        };
        let center = place(pos, rot, scale, o.offset);
        let half = [
            (size.x * scale[0]).abs() * 0.5,
            (size.y * scale[1]).abs() * 0.5,
        ];
        out.extend(box_edges(center, half, rot).into_iter().map(|x| (e, x)));
    }
    out
}

/// The shadow a light at `p` with `radius` casts from `edges`, as
/// triangles: each edge facing away from the light (the light is on its
/// inner side), within reach, swept away from the light past the radius.
pub fn shadow(
    p: [f32; 2],
    radius: f32,
    edges: impl Iterator<Item = Edge>,
    out: &mut Vec<[f32; 2]>,
) {
    let far = radius * 64.0 + 1.0;
    for e in edges {
        let m = [(e.a[0] + e.b[0]) * 0.5, (e.a[1] + e.b[1]) * 0.5];
        let facing = e.normal[0] * (p[0] - m[0]) + e.normal[1] * (p[1] - m[1]);
        if facing >= 0.0 {
            continue;
        }
        if distance_to_segment(p, e.a, e.b) > radius {
            continue;
        }
        let away = |q: [f32; 2]| {
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let d = (dx * dx + dy * dy).sqrt().max(1e-6);
            [p[0] + dx / d * far, p[1] + dy / d * far]
        };
        let (fa, fb) = (away(e.a), away(e.b));
        out.extend_from_slice(&[e.a, e.b, fb, e.a, fb, fa]);
    }
}

fn distance_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let len = abx * abx + aby * aby;
    let t = if len > 0.0 {
        (((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a[0] + abx * t - p[0], a[1] + aby * t - p[1]);
    (x * x + y * y).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_casts_from_its_far_side_only() {
        let edges = box_edges([0.0, 0.0], [1.0, 1.0], 0.0);
        let mut tris = Vec::new();
        // A light to the left sees only the left face: the other three
        // face away and cast, two triangles each.
        shadow([-4.0, 0.0], 10.0, edges.into_iter(), &mut tris);
        assert_eq!(tris.len(), 18, "{tris:?}");
        let near: Vec<_> = tris.chunks(6).map(|q| [q[0], q[1]]).collect();
        assert!(
            !near.iter().any(|e| e[0][0] == -1.0 && e[1][0] == -1.0),
            "the lit face casts nothing: {near:?}"
        );
        // Out of reach: none.
        tris.clear();
        shadow([-40.0, 0.0], 10.0, edges.into_iter(), &mut tris);
        assert!(tris.is_empty());
    }

    #[test]
    fn a_steady_light_does_not_flicker_and_a_flickering_one_repeats() {
        let mut w = World::new();
        let e = w.spawn();
        assert_eq!(flicker(e, 3.0, 0.0), 1.0);
        let a = flicker(e, 3.25, 0.5);
        assert_eq!(a, flicker(e, 3.25, 0.5));
        assert!((0.5..=1.0).contains(&a), "{a}");
        assert_ne!(a, flicker(e, 3.5, 0.5));
    }
}
