//! A skeleton's pose, without the world: the setup pose, an animation
//! sampled over it, two poses mixed, bones to world matrices with Spine's
//! inheritance and IK, and each slot's attachment placed. The arithmetic
//! is Spine 4's (its runtimes are the reference for what a file means);
//! the code is the engine's own.

use argus_assets::spine::{
    Animation, BoneProp, Curve, EventKey, Inherit, Key, SkeletonAsset, SlotTimeline,
};

/// A bone's local transform: Spine's x, y, rotation (degrees), scale, and
/// shear (degrees).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Local {
    /// Position x relative to the parent.
    pub x: f32,
    /// Position y relative to the parent.
    pub y: f32,
    /// Degrees, counter-clockwise.
    pub rot: f32,
    /// Scale x.
    pub sx: f32,
    /// Scale y.
    pub sy: f32,
    /// Shear x, degrees.
    pub shx: f32,
    /// Shear y, degrees.
    pub shy: f32,
}

/// A 2D affine matrix: a point (x, y) goes to (a x + b y + tx, c x + d y +
/// ty).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat {
    /// Row 1, column 1.
    pub a: f32,
    /// Row 1, column 2.
    pub b: f32,
    /// Row 2, column 1.
    pub c: f32,
    /// Row 2, column 2.
    pub d: f32,
    /// Translation x.
    pub tx: f32,
    /// Translation y.
    pub ty: f32,
}

impl Mat {
    /// The skeleton's own frame: a negative x mirrors it, at the entity's
    /// origin.
    fn root(sx: f32, sy: f32) -> Mat {
        Mat {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

/// Where everything is at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose<'a> {
    /// Each bone's local transform.
    pub bones: Vec<Local>,
    /// Each slot's attachment name.
    pub attachments: Vec<Option<&'a str>>,
    /// Each slot's colour, RGBA 0 to 1.
    pub colors: Vec<[f32; 4]>,
    /// Slots in draw order, back first.
    pub order: Vec<usize>,
    /// Each IK constraint's mix and whether it bends positive.
    pub ik: Vec<(f32, bool)>,
}

/// The setup pose.
pub fn setup(sk: &SkeletonAsset) -> Pose<'_> {
    Pose {
        bones: sk
            .bones
            .iter()
            .map(|b| Local {
                x: b.x,
                y: b.y,
                rot: b.rotation,
                sx: b.scale_x,
                sy: b.scale_y,
                shx: b.shear_x,
                shy: b.shear_y,
            })
            .collect(),
        attachments: sk.slots.iter().map(|s| s.attachment.as_deref()).collect(),
        colors: sk.slots.iter().map(|s| s.color).collect(),
        order: (0..sk.slots.len()).collect(),
        ik: sk.ik.iter().map(|c| (c.mix, c.bend_positive)).collect(),
    }
}

fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, s: f32) -> f32 {
    let u = 1.0 - s;
    u * u * u * p0 + 3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s * p3
}

/// Value `i` between two keys at `t`.
fn between<const N: usize>(k0: &Key<N>, k1: &Key<N>, i: usize, t: f32) -> f32 {
    let (t0, v0, t1, v1) = (k0.time, k0.values[i], k1.time, k1.values[i]);
    match k0.curves[i] {
        Curve::Stepped => v0,
        Curve::Linear => {
            if t1 > t0 {
                v0 + (v1 - v0) * ((t - t0) / (t1 - t0))
            } else {
                v1
            }
        }
        Curve::Bezier([cx1, cy1, cx2, cy2]) => {
            // The curve's x is time: find where it is `t` (a fixed number
            // of halvings, so every run takes the same steps), then read
            // its y there.
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..24 {
                let mid = 0.5 * (lo + hi);
                if cubic(t0, cx1, cx2, t1, mid) < t {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            cubic(v0, cy1, cy2, v1, 0.5 * (lo + hi))
        }
    }
}

/// Value `i` of a timeline at `t`; `None` before its first key (the setup
/// pose holds there).
pub fn sample<const N: usize>(keys: &[Key<N>], t: f32, i: usize) -> Option<f32> {
    let first = keys.first()?;
    if t.is_nan() || t < first.time {
        return None;
    }
    let at = keys.partition_point(|k| k.time <= t) - 1;
    Some(match keys.get(at + 1) {
        Some(next) => between(&keys[at], next, i, t),
        None => keys[at].values[i],
    })
}

/// The last of `(time, value)` keys at or before `t`.
fn last_at<T>(keys: &[(f32, T)], t: f32) -> Option<&T> {
    let n = keys.partition_point(|k| k.0 <= t);
    n.checked_sub(1).map(|i| &keys[i].1)
}

/// An animation at `t` seconds, over a pose that starts as the setup.
pub fn apply<'a>(sk: &'a SkeletonAsset, anim: &'a Animation, t: f32, pose: &mut Pose<'a>) {
    for tl in &anim.bones {
        let Some(setup) = sk.bones.get(tl.bone) else {
            continue;
        };
        let b = &mut pose.bones[tl.bone];
        let v = |i| sample(&tl.keys, t, i);
        match tl.prop {
            BoneProp::Rotate => {
                if let Some(r) = v(0) {
                    b.rot = setup.rotation + r;
                }
            }
            BoneProp::Translate => {
                if let (Some(x), Some(y)) = (v(0), v(1)) {
                    b.x = setup.x + x;
                    b.y = setup.y + y;
                }
            }
            BoneProp::TranslateX => {
                if let Some(x) = v(0) {
                    b.x = setup.x + x;
                }
            }
            BoneProp::TranslateY => {
                if let Some(y) = v(0) {
                    b.y = setup.y + y;
                }
            }
            BoneProp::Scale => {
                if let (Some(x), Some(y)) = (v(0), v(1)) {
                    b.sx = setup.scale_x * x;
                    b.sy = setup.scale_y * y;
                }
            }
            BoneProp::ScaleX => {
                if let Some(x) = v(0) {
                    b.sx = setup.scale_x * x;
                }
            }
            BoneProp::ScaleY => {
                if let Some(y) = v(0) {
                    b.sy = setup.scale_y * y;
                }
            }
            BoneProp::Shear => {
                if let (Some(x), Some(y)) = (v(0), v(1)) {
                    b.shx = setup.shear_x + x;
                    b.shy = setup.shear_y + y;
                }
            }
            BoneProp::ShearX => {
                if let Some(x) = v(0) {
                    b.shx = setup.shear_x + x;
                }
            }
            BoneProp::ShearY => {
                if let Some(y) = v(0) {
                    b.shy = setup.shear_y + y;
                }
            }
        }
    }
    for tl in &anim.slots {
        match tl {
            SlotTimeline::Attachment { slot, keys } => {
                if let Some(a) = last_at(keys, t) {
                    pose.attachments[*slot] = a.as_deref();
                }
            }
            SlotTimeline::Color {
                slot,
                channels,
                keys,
            } => {
                for ch in channels.0..channels.1 {
                    if let Some(v) = sample(keys, t, ch) {
                        pose.colors[*slot][ch] = v.clamp(0.0, 1.0);
                    }
                }
            }
        }
    }
    if let Some(order) = last_at(&anim.draw_order, t) {
        pose.order = match order {
            Some(o) => o.clone(),
            None => (0..sk.slots.len()).collect(),
        };
    }
    for (ci, keys) in &anim.ik {
        if let (Some(mix), Some(bend)) = (sample(keys, t, 0), sample(keys, t, 1)) {
            pose.ik[*ci] = (mix, bend > 0.0);
        }
    }
}

/// Mix `from` into `pose` at weight `w` (1 is all `from`): bones and
/// colours blend, rotation the short way round; attachments and draw order
/// are `pose`'s.
pub fn mix(pose: &mut Pose<'_>, from: &Pose<'_>, w: f32) {
    let w = w.clamp(0.0, 1.0);
    if w <= 0.0 {
        return;
    }
    let lerp = |a: f32, b: f32| a + (b - a) * w;
    for (b, f) in pose.bones.iter_mut().zip(&from.bones) {
        b.x = lerp(b.x, f.x);
        b.y = lerp(b.y, f.y);
        let mut dr = f.rot - b.rot;
        dr -= 360.0 * (dr / 360.0).round();
        b.rot += dr * w;
        b.sx = lerp(b.sx, f.sx);
        b.sy = lerp(b.sy, f.sy);
        b.shx = lerp(b.shx, f.shx);
        b.shy = lerp(b.shy, f.shy);
    }
    for (c, f) in pose.colors.iter_mut().zip(&from.colors) {
        for i in 0..4 {
            c[i] = lerp(c[i], f[i]);
        }
    }
    for (k, f) in pose.ik.iter_mut().zip(&from.ik) {
        k.0 = lerp(k.0, f.0);
        if w > 0.5 {
            k.1 = f.1;
        }
    }
}

/// Events an animation fires in `(after, upto]`.
pub fn events_in(anim: &Animation, after: f32, upto: f32) -> impl Iterator<Item = &EventKey> {
    let lo = anim.events.partition_point(|e| e.time <= after);
    let hi = anim.events.partition_point(|e| e.time <= upto);
    anim.events[lo..hi.max(lo)].iter()
}

/// A bone's world matrix from its local transform and its parent's
/// (Spine's updateWorldTransform). `sx`, `sy` is the skeleton's scale.
fn place(inherit: Inherit, l: &Local, parent: Option<&Mat>, sx: f32, sy: f32) -> Mat {
    let rx = (l.rot + l.shx).to_radians();
    let ry = (l.rot + 90.0 + l.shy).to_radians();
    let Some(p) = parent else {
        return Mat {
            a: rx.cos() * l.sx * sx,
            b: ry.cos() * l.sy * sx,
            c: rx.sin() * l.sx * sy,
            d: ry.sin() * l.sy * sy,
            tx: l.x * sx,
            ty: l.y * sy,
        };
    };
    let (mut pa, mut pb, mut pc, mut pd) = (p.a, p.b, p.c, p.d);
    let tx = pa * l.x + pb * l.y + p.tx;
    let ty = pc * l.x + pd * l.y + p.ty;
    let (a, b, c, d) = match inherit {
        Inherit::Normal => {
            let (la, lb) = (rx.cos() * l.sx, ry.cos() * l.sy);
            let (lc, ld) = (rx.sin() * l.sx, ry.sin() * l.sy);
            return Mat {
                a: pa * la + pb * lc,
                b: pa * lb + pb * ld,
                c: pc * la + pd * lc,
                d: pc * lb + pd * ld,
                tx,
                ty,
            };
        }
        Inherit::OnlyTranslation => (
            rx.cos() * l.sx,
            ry.cos() * l.sy,
            rx.sin() * l.sx,
            ry.sin() * l.sy,
        ),
        Inherit::NoRotationOrReflection => {
            let mut s = pa * pa + pc * pc;
            let prx;
            if s > 0.0001 {
                s = (pa * pd - pb * pc).abs() / s;
                pa /= sx;
                pc /= sy;
                pb = pc * s;
                pd = pa * s;
                prx = pc.atan2(pa).to_degrees();
            } else {
                pa = 0.0;
                pc = 0.0;
                prx = 90.0 - pd.atan2(pb).to_degrees();
            }
            let rx = (l.rot + l.shx - prx).to_radians();
            let ry = (l.rot + l.shy - prx + 90.0).to_radians();
            let (la, lb) = (rx.cos() * l.sx, ry.cos() * l.sy);
            let (lc, ld) = (rx.sin() * l.sx, ry.sin() * l.sy);
            (
                pa * la - pb * lc,
                pa * lb - pb * ld,
                pc * la + pd * lc,
                pc * lb + pd * ld,
            )
        }
        Inherit::NoScale | Inherit::NoScaleOrReflection => {
            let r = l.rot.to_radians();
            let (sin, cos) = r.sin_cos();
            let mut za = (pa * cos + pb * sin) / sx;
            let mut zc = (pc * cos + pd * sin) / sy;
            let mut s = (za * za + zc * zc).sqrt();
            if s > 0.00001 {
                s = 1.0 / s;
            }
            za *= s;
            zc *= s;
            s = (za * za + zc * zc).sqrt();
            if inherit == Inherit::NoScale
                && ((pa * pd - pb * pc < 0.0) != ((sx < 0.0) != (sy < 0.0)))
            {
                s = -s;
            }
            let r = std::f32::consts::FRAC_PI_2 + zc.atan2(za);
            let (zb, zd) = (r.cos() * s, r.sin() * s);
            let shx = l.shx.to_radians();
            let shy = (90.0 + l.shy).to_radians();
            let (la, lb) = (shx.cos() * l.sx, shy.cos() * l.sy);
            let (lc, ld) = (shx.sin() * l.sx, shy.sin() * l.sy);
            (
                za * la + zb * lc,
                za * lb + zb * ld,
                zc * la + zd * lc,
                zc * lb + zd * ld,
            )
        }
    };
    Mat {
        a: a * sx,
        b: b * sx,
        c: c * sy,
        d: d * sy,
        tx,
        ty,
    }
}

fn wrap180(mut a: f32) -> f32 {
    if a > 180.0 {
        a -= 360.0;
    } else if a < -180.0 {
        a += 360.0;
    }
    a
}

/// The bones' world matrices in the skeleton's own units, mirrored when
/// `flip_x`, with every IK constraint applied in order. Spine's arithmetic
/// has thresholds in those units (a determinant under 0.0001 is
/// degenerate), so a scale to world units is applied to what this places,
/// not here.
pub fn world(sk: &SkeletonAsset, pose: &Pose<'_>, flip_x: bool) -> Vec<Mat> {
    let (sx, sy) = (if flip_x { -1.0 } else { 1.0 }, 1.0);
    let n = sk.bones.len();
    let mut local = pose.bones.clone();
    let mut mats: Vec<Mat> = Vec::with_capacity(n);
    for (i, b) in sk.bones.iter().enumerate() {
        let m = place(b.inherit, &local[i], b.parent.map(|p| &mats[p]), sx, sy);
        mats.push(m);
    }
    let root = Mat::root(sx, sy);
    for (ci, c) in sk.ik.iter().enumerate() {
        let (alpha, bend) = pose.ik[ci];
        if alpha <= 0.0 {
            continue;
        }
        let (tx, ty) = (mats[c.target].tx, mats[c.target].ty);
        match c.bones[..] {
            [b] => ik1(sk, &mut mats, &mut local, &root, b, tx, ty, alpha, sx, sy),
            [p, ch] => ik2(
                sk, &mut mats, &mut local, &root, p, ch, tx, ty, bend, alpha, sx, sy,
            ),
            _ => continue,
        }
        // Everything below the bent bones follows them.
        let mut moved = vec![false; n];
        for &b in &c.bones {
            moved[b] = true;
        }
        for i in 0..n {
            let Some(p) = sk.bones[i].parent else {
                continue;
            };
            if moved[p] && !c.bones.contains(&i) {
                moved[i] = true;
                mats[i] = place(sk.bones[i].inherit, &local[i], Some(&mats[p]), sx, sy);
            }
        }
    }
    mats
}

/// One bone pointed at a target (Spine's IkConstraint.apply, one bone).
#[allow(clippy::too_many_arguments)]
fn ik1(
    sk: &SkeletonAsset,
    mats: &mut [Mat],
    local: &mut [Local],
    root: &Mat,
    b: usize,
    target_x: f32,
    target_y: f32,
    alpha: f32,
    sx: f32,
    sy: f32,
) {
    let data = &sk.bones[b];
    let p = data.parent.map_or(*root, |i| mats[i]);
    let (pa, mut pb, pc, mut pd) = (p.a, p.b, p.c, p.d);
    let l = local[b];
    let mut rot = -l.shx - l.rot;
    let (tx, ty);
    if data.inherit == Inherit::OnlyTranslation {
        tx = (target_x - mats[b].tx) * sx.signum();
        ty = (target_y - mats[b].ty) * sy.signum();
    } else {
        if data.inherit == Inherit::NoRotationOrReflection {
            let s = (pa * pd - pb * pc).abs() / (pa * pa + pc * pc).max(0.0001);
            let sa = pa / sx;
            let sc = pc / sy;
            pb = -sc * s * sx;
            pd = sa * s * sy;
            rot += sc.atan2(sa).to_degrees();
        }
        let (x, y) = (target_x - p.tx, target_y - p.ty);
        let d = pa * pd - pb * pc;
        if d.abs() <= 0.0001 {
            tx = 0.0;
            ty = 0.0;
        } else {
            tx = (x * pd - y * pb) / d - l.x;
            ty = (y * pa - x * pc) / d - l.y;
        }
    }
    rot += ty.atan2(tx).to_degrees();
    if l.sx < 0.0 {
        rot += 180.0;
    }
    local[b].rot = l.rot + wrap180(rot) * alpha;
    mats[b] = place(data.inherit, &local[b], data.parent.map(|_| &p), sx, sy);
}

/// A parent and its child bent so the child's end reaches a target
/// (Spine's IkConstraint.apply, two bones; without softness or stretch).
#[allow(clippy::too_many_arguments)]
fn ik2(
    sk: &SkeletonAsset,
    mats: &mut [Mat],
    local: &mut [Local],
    root: &Mat,
    parent: usize,
    child: usize,
    target_x: f32,
    target_y: f32,
    bend_positive: bool,
    alpha: f32,
    sx: f32,
    sy: f32,
) {
    use std::f32::consts::PI;
    let (pd_, cd_) = (&sk.bones[parent], &sk.bones[child]);
    if pd_.inherit != Inherit::Normal || cd_.inherit != Inherit::Normal {
        return;
    }
    let bend = if bend_positive { 1.0 } else { -1.0 };
    let pl = local[parent];
    let cl = local[child];
    let (px, py) = (pl.x, pl.y);
    let (mut psx, mut psy) = (pl.sx, pl.sy);
    let mut csx = cl.sx;
    let (os1, mut s2) = if psx < 0.0 {
        psx = -psx;
        (180.0, -1.0)
    } else {
        (0.0, 1.0)
    };
    if psy < 0.0 {
        psy = -psy;
        s2 = -s2;
    }
    let os2 = if csx < 0.0 {
        csx = -csx;
        180.0
    } else {
        0.0
    };
    let pm = mats[parent];
    let cx = cl.x;
    let u = (psx - psy).abs() <= 0.0001;
    let (cy, cwx, cwy) = if !u {
        (0.0, pm.a * cx + pm.tx, pm.c * cx + pm.ty)
    } else {
        let cy = cl.y;
        (
            cy,
            pm.a * cx + pm.b * cy + pm.tx,
            pm.c * cx + pm.d * cy + pm.ty,
        )
    };
    let pp = pd_.parent.map_or(*root, |i| mats[i]);
    let (a, b, c, d) = (pp.a, pp.b, pp.c, pp.d);
    let det = a * d - b * c;
    let id = if det.abs() > 0.0001 { 1.0 / det } else { 0.0 };
    let (x, y) = (cwx - pp.tx, cwy - pp.ty);
    let dx = (x * d - y * b) * id - px;
    let dy = (y * a - x * c) * id - py;
    let l1 = (dx * dx + dy * dy).sqrt();
    let mut l2 = cd_.length * csx;
    if l1 < 0.0001 {
        ik1(
            sk, mats, local, root, parent, target_x, target_y, alpha, sx, sy,
        );
        local[child].rot = cl.rot;
        mats[child] = place(cd_.inherit, &local[child], Some(&mats[parent]), sx, sy);
        return;
    }
    let (x, y) = (target_x - pp.tx, target_y - pp.ty);
    let tx = (x * d - y * b) * id - px;
    let ty = (y * a - x * c) * id - py;
    let dd = tx * tx + ty * ty;
    let (a1, a2);
    if u {
        l2 *= psx;
        let mut cos = (dd - l1 * l1 - l2 * l2) / (2.0 * l1 * l2);
        let ang;
        if cos < -1.0 {
            cos = -1.0;
            ang = PI * bend;
        } else if cos > 1.0 {
            cos = 1.0;
            ang = 0.0;
        } else {
            ang = cos.acos() * bend;
        }
        a2 = ang;
        let aa = l1 + l2 * cos;
        let bb = l2 * a2.sin();
        a1 = (ty * aa - tx * bb).atan2(tx * aa + ty * bb);
    } else {
        let a = psx * l2;
        let b = psy * l2;
        let (aa, bb) = (a * a, b * b);
        let ta = ty.atan2(tx);
        let c = bb * l1 * l1 + aa * dd - aa * bb;
        let c1 = -2.0 * bb * l1;
        let c2 = bb - aa;
        let d = c1 * c1 - 4.0 * c2 * c;
        let mut solved = None;
        if d >= 0.0 {
            let mut q = d.sqrt();
            if c1 < 0.0 {
                q = -q;
            }
            q = -(c1 + q) * 0.5;
            let (r0, r1) = (q / c2, c / q);
            let r = if r0.abs() < r1.abs() { r0 } else { r1 };
            let rest = dd - r * r;
            if rest >= 0.0 {
                let y = rest.sqrt() * bend;
                solved = Some((ta - y.atan2(r), (y / psy).atan2((r - l1) / psx)));
            }
        }
        (a1, a2) = solved.unwrap_or_else(|| {
            let (mut min_a, mut min_x, mut min_y) = (PI, l1 - a, 0.0f32);
            let mut min_d = min_x * min_x;
            let (mut max_a, mut max_x, mut max_y) = (0.0f32, l1 + a, 0.0f32);
            let mut max_d = max_x * max_x;
            let cc = -a * l1 / (aa - bb);
            if (-1.0..=1.0).contains(&cc) {
                let cc = cc.acos();
                let x = a * cc.cos() + l1;
                let y = b * cc.sin();
                let dist = x * x + y * y;
                if dist < min_d {
                    (min_a, min_d, min_x, min_y) = (cc, dist, x, y);
                }
                if dist > max_d {
                    (max_a, max_d, max_x, max_y) = (cc, dist, x, y);
                }
            }
            if dd <= (min_d + max_d) * 0.5 {
                (ta - (min_y * bend).atan2(min_x), min_a * bend)
            } else {
                (ta - (max_y * bend).atan2(max_x), max_a * bend)
            }
        });
    }
    let os = cy.atan2(cx) * s2;
    let r1 = wrap180((a1 - os).to_degrees() + os1 - pl.rot);
    local[parent] = Local {
        rot: pl.rot + r1 * alpha,
        shx: 0.0,
        shy: 0.0,
        ..pl
    };
    mats[parent] = place(pd_.inherit, &local[parent], pd_.parent.map(|_| &pp), sx, sy);
    let r2 = wrap180(((a2 + os).to_degrees() - cl.shx) * s2 + os2 - cl.rot);
    local[child] = Local {
        y: cy,
        rot: cl.rot + r2 * alpha,
        ..cl
    };
    mats[child] = place(cd_.inherit, &local[child], Some(&mats[parent]), sx, sy);
}

/// One slot's image as drawn: where, turned and scaled how, at what size,
/// in what colour. `image` is `None` when the slot shows nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed<'a> {
    /// The slot.
    pub slot: usize,
    /// Its place in the draw order, back first.
    pub order: usize,
    /// The attachment's image (a sprite asset), if it shows one.
    pub image: Option<&'a str>,
    /// Centre, in the skeleton's frame.
    pub pos: [f32; 2],
    /// Radians.
    pub rot: f32,
    /// Scale (a negative y is a reflection).
    pub scale: [f32; 2],
    /// The attachment's size.
    pub size: [f32; 2],
    /// Slot colour times the attachment's.
    pub color: [f32; 4],
}

/// Every slot's attachment placed, in slot order. A matrix with shear is
/// drawn as its rotation and scale (a sprite has no shear).
pub fn place_slots<'a>(
    sk: &'a SkeletonAsset,
    pose: &Pose<'a>,
    mats: &[Mat],
    skin: &str,
) -> Vec<Placed<'a>> {
    let mut rank = vec![0usize; sk.slots.len()];
    for (i, &s) in pose.order.iter().enumerate() {
        if let Some(r) = rank.get_mut(s) {
            *r = i;
        }
    }
    sk.slots
        .iter()
        .enumerate()
        .map(|(si, slot)| {
            let att = pose.attachments[si].and_then(|name| sk.attachment(skin, si, name));
            let Some(att) = att else {
                return Placed {
                    slot: si,
                    order: rank[si],
                    image: None,
                    pos: [0.0; 2],
                    rot: 0.0,
                    scale: [1.0; 2],
                    size: [0.0; 2],
                    color: pose.colors[si],
                };
            };
            let m = &mats[slot.bone];
            let r = att.rotation.to_radians();
            let (sin, cos) = r.sin_cos();
            let (oa, ob) = (cos * att.scale_x, -sin * att.scale_y);
            let (oc, od) = (sin * att.scale_x, cos * att.scale_y);
            let a = m.a * oa + m.b * oc;
            let b = m.a * ob + m.b * od;
            let c = m.c * oa + m.d * oc;
            let d = m.c * ob + m.d * od;
            let x = m.a * att.x + m.b * att.y + m.tx;
            let y = m.c * att.x + m.d * att.y + m.ty;
            let sxx = (a * a + c * c).sqrt();
            let det = a * d - b * c;
            let col = pose.colors[si];
            Placed {
                slot: si,
                order: rank[si],
                image: Some(att.image.as_str()),
                pos: [x, y],
                rot: c.atan2(a),
                scale: [sxx, if sxx > 1e-6 { det / sxx } else { 0.0 }],
                size: [att.width, att.height],
                color: [
                    col[0] * att.color[0],
                    col[1] * att.color[1],
                    col[2] * att.color[2],
                    col[3] * att.color[3],
                ],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bezier_key_eases_and_ends_on_its_value() {
        let k0 = Key {
            time: 0.0,
            values: [0.0, 0.0],
            curves: [Curve::Bezier([0.5, 0.0, 0.5, 10.0]), Curve::Linear],
        };
        let k1 = Key {
            time: 1.0,
            values: [10.0, 0.0],
            curves: [Curve::Linear; 2],
        };
        let keys = [k0, k1];
        let mid = sample(&keys, 0.5, 0).unwrap();
        assert!((mid - 5.0).abs() < 1e-3, "{mid}");
        let early = sample(&keys, 0.2, 0).unwrap();
        assert!(early < 2.0, "an ease-in starts slow: {early}");
        assert_eq!(sample(&keys, 1.0, 0), Some(10.0));
        assert_eq!(sample(&keys, 5.0, 0), Some(10.0));
        assert_eq!(sample(&keys, -0.1, 0), None);
    }

    #[test]
    fn a_stepped_key_holds_until_the_next() {
        let keys = [
            Key {
                time: 0.0,
                values: [1.0, 0.0],
                curves: [Curve::Stepped; 2],
            },
            Key {
                time: 1.0,
                values: [3.0, 0.0],
                curves: [Curve::Linear; 2],
            },
        ];
        assert_eq!(sample(&keys, 0.99, 0), Some(1.0));
        assert_eq!(sample(&keys, 1.0, 0), Some(3.0));
    }

    #[test]
    fn a_child_takes_its_parents_rotation_and_scale() {
        let parent = Mat::root(1.0, 1.0);
        let p = place(
            Inherit::Normal,
            &Local {
                rot: 90.0,
                sx: 2.0,
                sy: 2.0,
                ..Local::default()
            },
            Some(&parent),
            1.0,
            1.0,
        );
        // A child 10 along the parent's x lands 20 up.
        let c = place(
            Inherit::Normal,
            &Local {
                x: 10.0,
                sx: 1.0,
                sy: 1.0,
                ..Local::default()
            },
            Some(&p),
            1.0,
            1.0,
        );
        assert!((c.tx).abs() < 1e-4 && (c.ty - 20.0).abs() < 1e-4, "{c:?}");
        // Only its position, with onlyTranslation.
        let t = place(
            Inherit::OnlyTranslation,
            &Local {
                x: 10.0,
                sx: 1.0,
                sy: 1.0,
                ..Local::default()
            },
            Some(&p),
            1.0,
            1.0,
        );
        assert!((t.a - 1.0).abs() < 1e-5 && t.c.abs() < 1e-5, "{t:?}");
    }
}
