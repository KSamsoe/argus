//! The geometry under the queries (2D staples item 0): every shape is a
//! rounded convex polygon, the convex hull of a few points grown by a
//! radius. One point grown by a radius is a circle, two a capsule, four
//! corners with no radius a box, a tile's slope three or four. A shape
//! moving against another is a ray against their Minkowski sum (the target
//! plus the mover turned about its origin), which is another rounded
//! convex polygon; two shapes overlap when the origin is inside the sum of
//! one and the other turned about. So one ray test and one distance test
//! answer every query, exactly, with nothing but `+ - * /` and `sqrt`,
//! which every platform rounds the same way.

/// Distances closer than this are touching, not overlapping.
pub const EPS: f32 = 1e-4;

/// A point or a direction, y up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
}

impl V {
    /// Construct.
    pub const fn new(x: f32, y: f32) -> V {
        V { x, y }
    }
    /// The dot product.
    pub fn dot(self, o: V) -> f32 {
        self.x * o.x + self.y * o.y
    }
    /// The 2D cross product (z of the 3D one).
    pub fn cross(self, o: V) -> f32 {
        self.x * o.y - self.y * o.x
    }
    /// The length squared.
    pub fn len2(self) -> f32 {
        self.dot(self)
    }
    /// The length.
    pub fn len(self) -> f32 {
        self.len2().sqrt()
    }
    /// Of length 1, or zero when it has none.
    pub fn unit(self) -> V {
        let l = self.len();
        if l > 0.0 {
            self * (1.0 / l)
        } else {
            V::default()
        }
    }
}

impl std::ops::Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        V::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        V::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f32> for V {
    type Output = V;
    fn mul(self, s: f32) -> V {
        V::new(self.x * s, self.y * s)
    }
}

impl std::ops::Neg for V {
    type Output = V;
    fn neg(self) -> V {
        V::new(-self.x, -self.y)
    }
}

/// A rounded convex polygon: the hull of `pts` (counter-clockwise, no
/// three in a line) grown by `r`. One point is a circle, two a capsule.
#[derive(Clone, Debug, PartialEq)]
pub struct Round {
    /// The hull's corners, counter-clockwise.
    pub pts: Vec<V>,
    /// How far it is grown.
    pub r: f32,
}

impl Round {
    /// A circle.
    pub fn circle(c: V, r: f32) -> Round {
        Round { pts: vec![c], r }
    }
    /// A box from its centre and half extents.
    pub fn rect(c: V, hw: f32, hh: f32) -> Round {
        Round::hull(
            &[
                V::new(c.x - hw, c.y - hh),
                V::new(c.x + hw, c.y - hh),
                V::new(c.x + hw, c.y + hh),
                V::new(c.x - hw, c.y + hh),
            ],
            0.0,
        )
    }
    /// A capsule: the segment `a` to `b` grown by `r`.
    pub fn capsule(a: V, b: V, r: f32) -> Round {
        Round::hull(&[a, b], r)
    }
    /// The convex hull of `pts`, grown by `r`.
    pub fn hull(pts: &[V], r: f32) -> Round {
        Round {
            pts: hull(pts),
            r: r.max(0.0),
        }
    }
    /// Moved by `d`.
    pub fn shift(&self, d: V) -> Round {
        Round {
            pts: self.pts.iter().map(|p| *p + d).collect(),
            r: self.r,
        }
    }
    /// Turned half a circle about the origin.
    pub fn neg(&self) -> Round {
        Round::hull(&self.pts.iter().map(|p| -*p).collect::<Vec<_>>(), self.r)
    }
    /// The Minkowski sum: every point of one plus every point of the other.
    pub fn sum(&self, o: &Round) -> Round {
        let mut all = Vec::with_capacity(self.pts.len() * o.pts.len());
        for a in &self.pts {
            for b in &o.pts {
                all.push(*a + *b);
            }
        }
        Round::hull(&all, self.r + o.r)
    }
    /// The smallest box holding it: min and max corners.
    pub fn bounds(&self) -> (V, V) {
        let mut lo = V::new(f32::INFINITY, f32::INFINITY);
        let mut hi = V::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in &self.pts {
            lo = V::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = V::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        (
            V::new(lo.x - self.r, lo.y - self.r),
            V::new(hi.x + self.r, hi.y + self.r),
        )
    }
    /// How far `p` is from its hull, before the radius: below 0 inside a
    /// polygon, by how deep.
    pub fn hull_distance(&self, p: V) -> f32 {
        match self.pts.len() {
            0 => f32::INFINITY,
            1 => (p - self.pts[0]).len(),
            2 => seg_distance(p, self.pts[0], self.pts[1]),
            n => {
                let nearest = (0..n)
                    .map(|i| seg_distance(p, self.pts[i], self.pts[(i + 1) % n]))
                    .fold(f32::INFINITY, f32::min);
                let inside = (0..n).all(|i| {
                    let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
                    (b - a).cross(p - a) >= 0.0
                });
                if inside {
                    -nearest
                } else {
                    nearest
                }
            }
        }
    }
    /// How far `p` is from it: below 0 inside, by how much.
    pub fn distance(&self, p: V) -> f32 {
        self.hull_distance(p) - self.r
    }
    /// Whether it and `o` overlap by more than touching.
    pub fn overlaps(&self, o: &Round) -> bool {
        self.sum(&o.neg()).distance(V::default()) < -EPS
    }
}

/// How far `p` is from the segment `a` to `b`.
pub fn seg_distance(p: V, a: V, b: V) -> f32 {
    (p - closest_on_seg(p, a, b)).len()
}

fn closest_on_seg(p: V, a: V, b: V) -> V {
    let ab = b - a;
    let l2 = ab.len2();
    if l2 == 0.0 {
        return a;
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    a + ab * t
}

/// The convex hull, counter-clockwise from the lowest-then-leftmost
/// point, with points in a line and repeats left out (Andrew's monotone
/// chain). One distinct point gives one, points in a line their two ends.
pub fn hull(pts: &[V]) -> Vec<V> {
    let mut p: Vec<V> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup();
    if p.len() <= 2 {
        return p;
    }
    let mut lower: Vec<V> = Vec::new();
    for &q in &p {
        while lower.len() >= 2
            && (lower[lower.len() - 1] - lower[lower.len() - 2]).cross(q - lower[lower.len() - 2])
                <= 0.0
        {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<V> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2
            && (upper[upper.len() - 1] - upper[upper.len() - 2]).cross(q - upper[upper.len() - 2])
                <= 0.0
        {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    if lower.len() < 3 {
        // All in a line: its two ends.
        let (a, b) = (p[0], p[p.len() - 1]);
        return vec![a, b];
    }
    lower
}

/// Where a segment meets a shape: the fraction of the way along it, and
/// the shape's outward normal there (zero when it starts inside).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// 0 at the segment's start, 1 at its end.
    pub t: f32,
    /// The surface's outward normal, of length 1; zero from inside.
    pub normal: V,
}

/// The first point at which the segment from `o` to `o + d` enters
/// `shape`. A segment that starts inside it, deeper than touching, meets
/// it at once (`t` 0, a zero normal); one that starts touching it meets it
/// only by moving in.
pub fn ray(shape: &Round, o: V, d: V) -> Option<Contact> {
    if shape.pts.is_empty() {
        return None;
    }
    if shape.distance(o) < -EPS {
        return Some(Contact {
            t: 0.0,
            normal: V::default(),
        });
    }
    let mut best: Option<Contact> = None;
    let mut take = |c: Contact| {
        if (0.0..=1.0).contains(&c.t) && best.is_none_or(|b| c.t < b.t) {
            best = Some(c);
        }
    };
    let n = shape.pts.len();
    let r = shape.r;
    if r <= 0.0 {
        match n {
            1 => {}
            2 => {
                if let Some(c) = ray_segment(o, d, shape.pts[0], shape.pts[1]) {
                    take(c);
                }
            }
            _ => {
                if let Some(c) = ray_polygon(o, d, &shape.pts) {
                    take(c);
                }
            }
        }
        return best;
    }
    // Grown: the first point on its boundary is on an edge moved out by r,
    // or on a corner's circle.
    for &p in &shape.pts {
        if let Some(c) = ray_circle(o, d, p, r) {
            take(c);
        }
    }
    let edges: Vec<(V, V)> = match n {
        1 => Vec::new(),
        2 => vec![(shape.pts[0], shape.pts[1]), (shape.pts[1], shape.pts[0])],
        _ => (0..n)
            .map(|i| (shape.pts[i], shape.pts[(i + 1) % n]))
            .collect(),
    };
    for (a, b) in edges {
        let e = b - a;
        let out = V::new(e.y, -e.x).unit();
        let (a2, b2) = (a + out * r, b + out * r);
        if let Some(c) = ray_face(o, d, a2, b2, out) {
            take(c);
        }
    }
    best
}

/// The segment from `o` to `o + d` crossing the face `a`-`b` from its
/// outside (`out` the face's outward normal).
fn ray_face(o: V, d: V, a: V, b: V, out: V) -> Option<Contact> {
    let den = d.dot(out);
    if den >= 0.0 {
        return None;
    }
    let t = (a - o).dot(out) / den;
    let p = o + d * t;
    let e = b - a;
    let along = (p - a).dot(e);
    if along < 0.0 || along > e.len2() {
        return None;
    }
    Some(Contact { t, normal: out })
}

fn ray_circle(o: V, d: V, c: V, r: f32) -> Option<Contact> {
    let m = o - c;
    let a = d.len2();
    if a == 0.0 {
        return None;
    }
    let b = m.dot(d);
    let cc = m.len2() - r * r;
    let disc = b * b - a * cc;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / a;
    let p = o + d * t;
    Some(Contact {
        t,
        normal: (p - c).unit(),
    })
}

fn ray_segment(o: V, d: V, a: V, b: V) -> Option<Contact> {
    let e = b - a;
    let den = d.cross(e);
    if den == 0.0 {
        return None;
    }
    let w = a - o;
    let t = w.cross(e) / den;
    let u = w.cross(d) / den;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    // The side the segment came from.
    let n = V::new(e.y, -e.x).unit();
    let normal = if d.dot(n) < 0.0 { n } else { -n };
    Some(Contact { t, normal })
}

/// Cyrus-Beck: the segment clipped by each edge's half-plane.
fn ray_polygon(o: V, d: V, pts: &[V]) -> Option<Contact> {
    let n = pts.len();
    let (mut t_in, mut t_out) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut normal = V::default();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let e = b - a;
        let out = V::new(e.y, -e.x).unit();
        let num = (a - o).dot(out);
        let den = d.dot(out);
        if den == 0.0 {
            if num < 0.0 {
                return None;
            }
            continue;
        }
        let t = num / den;
        if den < 0.0 {
            if t > t_in {
                t_in = t;
                normal = out;
            }
        } else if t < t_out {
            t_out = t;
        }
        if t_in > t_out {
            return None;
        }
    }
    if t_in == f32::NEG_INFINITY {
        return None;
    }
    Some(Contact { t: t_in, normal })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn a_ray_meets_a_box_a_circle_and_a_capsule_where_it_should() {
        let b = Round::rect(V::new(5.0, 0.0), 1.0, 1.0);
        let c = ray(&b, V::new(0.0, 0.0), V::new(10.0, 0.0)).unwrap();
        assert!(close(c.t, 0.4) && c.normal == V::new(-1.0, 0.0), "{c:?}");
        assert!(
            ray(&b, V::new(0.0, 2.0), V::new(10.0, 0.0)).is_none(),
            "passes over"
        );
        let o = Round::circle(V::new(0.0, 5.0), 2.0);
        let c = ray(&o, V::new(0.0, 0.0), V::new(0.0, 10.0)).unwrap();
        assert!(close(c.t, 0.3) && close(c.normal.y, -1.0), "{c:?}");
        let k = Round::capsule(V::new(-3.0, 4.0), V::new(3.0, 4.0), 1.0);
        let c = ray(&k, V::new(1.0, 0.0), V::new(0.0, 10.0)).unwrap();
        assert!(
            close(c.t, 0.3) && close(c.normal.y, -1.0),
            "the flat side: {c:?}"
        );
        let c = ray(&k, V::new(5.0, 4.0), V::new(-10.0, 0.0)).unwrap();
        assert!(
            close(c.t, 0.1) && close(c.normal.x, 1.0),
            "the round end: {c:?}"
        );
        let inside = ray(&b, V::new(5.0, 0.0), V::new(1.0, 0.0)).unwrap();
        assert_eq!(inside.t, 0.0);
        assert_eq!(inside.normal, V::default());
    }

    #[test]
    fn a_moving_shape_is_a_ray_against_the_sum() {
        // A circle of radius 1 moving right at a box 1 wide whose face is at
        // x = 4: it stops with its edge at 4, its centre at 3.
        let target = Round::rect(V::new(5.0, 0.0), 1.0, 1.0);
        let mover = Round::circle(V::default(), 1.0);
        let c = ray(
            &target.sum(&mover.neg()),
            V::new(0.0, 0.0),
            V::new(10.0, 0.0),
        )
        .unwrap();
        assert!(close(c.t, 0.3), "{c:?}");
        // Past the box's corner, the circle's edge just clips it.
        let c = ray(
            &target.sum(&mover.neg()),
            V::new(0.0, 1.9),
            V::new(10.0, 0.0),
        );
        assert!(c.is_some_and(|c| c.t > 0.3 && c.t < 0.45), "{c:?}");
        assert!(ray(
            &target.sum(&mover.neg()),
            V::new(0.0, 2.1),
            V::new(10.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn overlap_and_distance_agree_with_touching() {
        let a = Round::rect(V::new(0.0, 0.0), 1.0, 1.0);
        let b = Round::rect(V::new(2.0, 0.0), 1.0, 1.0);
        assert!(!a.overlaps(&b), "touching faces do not overlap");
        assert!(a.overlaps(&Round::rect(V::new(1.9, 0.0), 1.0, 1.0)));
        assert!(Round::circle(V::new(2.0, 0.0), 1.5).overlaps(&a));
        assert!(!Round::circle(V::new(3.0, 3.0), 1.5).overlaps(&a));
        assert!(close(a.distance(V::new(3.0, 0.0)), 2.0));
        assert!(a.distance(V::default()) < 0.0);
        assert_eq!(
            hull(&[V::new(0.0, 0.0), V::new(1.0, 1.0), V::new(2.0, 2.0)]).len(),
            2
        );
        assert_eq!(
            hull(&[
                V::new(0.0, 0.0),
                V::new(2.0, 0.0),
                V::new(1.0, 0.5),
                V::new(2.0, 2.0),
                V::new(0.0, 2.0)
            ])
            .len(),
            4,
            "the inner point is not a corner"
        );
    }
}
