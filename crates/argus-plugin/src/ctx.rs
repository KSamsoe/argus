//! What a plugin system sees: a [`Ctx`] for one run, its values, and the
//! errors it answers with.

use crate::abi::{self, Str};
use crate::view::{Fetch, View};
use core::ffi::c_void;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

/// What a system answers.
pub type Result<T = (), E = Error> = core::result::Result<T, E>;

/// A system's error. Returned from its run, it stops the system with this
/// message (the engine's own when the engine refused a call), as a fault
/// at the system's `file:line`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// `None`: the engine refused a call and has the message.
    pub(crate) message: Option<String>,
}

impl Error {
    /// An error with a message.
    pub fn new(message: impl Into<String>) -> Error {
        Error {
            message: Some(message.into()),
        }
    }

    pub(crate) fn engine() -> Error {
        Error { message: None }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(
            self.message
                .as_deref()
                .unwrap_or("the engine refused a call"),
        )
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Error {
        Error::new(s)
    }
}

impl From<String> for Error {
    fn from(s: String) -> Error {
        Error::new(s)
    }
}

/// An entity: 32 bits of index, 32 of generation, as the engine's.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct Entity(pub u64);

impl Entity {
    /// No entity.
    pub const NULL: Entity = Entity(u64::MAX);
    /// Slot index.
    pub const fn index(self) -> u32 {
        self.0 as u32
    }
    /// Generation.
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
}

impl Default for Entity {
    fn default() -> Self {
        Entity::NULL
    }
}

/// A string's id in the world's table (a `string` field).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct StrId(pub u32);

/// A def, asset, or prefab reference (a `ref` field): a string's id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct Ref(pub StrId);

/// Two f32s, as the engine's `vec2`. Arithmetic is component by component
/// in f32, as Luau's vectors do it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[repr(C)]
pub struct Vec2 {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
}

impl Vec2 {
    /// (0, 0).
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    /// A vector.
    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
    /// x² + y².
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
    /// The length, as Luau's `vector.magnitude` computes it.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }
    /// x·x' + y·y'.
    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }
    /// The unit vector, or zero.
    pub fn normalized(self) -> Vec2 {
        let l = self.length();
        if l > 0.0 {
            self / l
        } else {
            Vec2::ZERO
        }
    }
}

impl core::ops::Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl core::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl core::ops::Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f32) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}
impl core::ops::Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, k: f32) -> Vec2 {
        Vec2::new(self.x / k, self.y / k)
    }
}
impl core::ops::Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}
impl core::ops::AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}
impl core::ops::SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}
impl core::ops::MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, k: f32) {
        *self = *self * k;
    }
}

/// An event's field, or a spawned entity's.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Value<'a> {
    /// A bool.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    UInt(u64),
    /// A float.
    Float(f64),
    /// A vector.
    Vec2(Vec2),
    /// Text.
    Str(&'a str),
    /// An entity.
    Entity(Entity),
}

macro_rules! value_from {
    ($($t:ty => $v:ident as $c:ty),*) => {$(
        impl From<$t> for Value<'_> {
            fn from(x: $t) -> Self {
                Value::$v(x as $c)
            }
        }
    )*};
}
value_from!(i32 => Int as i64, i64 => Int as i64, u8 => UInt as u64, u32 => UInt as u64,
    u64 => UInt as u64, usize => UInt as u64, f32 => Float as f64, f64 => Float as f64);

impl From<bool> for Value<'_> {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<Vec2> for Value<'_> {
    fn from(v: Vec2) -> Self {
        Value::Vec2(v)
    }
}
impl From<Entity> for Value<'_> {
    fn from(e: Entity) -> Self {
        Value::Entity(e)
    }
}
impl<'a> From<&'a str> for Value<'a> {
    fn from(s: &'a str) -> Self {
        Value::Str(s)
    }
}

impl Value<'_> {
    pub(crate) fn raw(&self) -> abi::Value {
        let mut v = abi::Value {
            ty: 0,
            i: 0,
            u: 0,
            f: 0.0,
            x: 0.0,
            y: 0.0,
            s: Str::EMPTY,
        };
        match *self {
            Value::Bool(b) => {
                v.ty = abi::ty::BOOL;
                v.u = b as u64;
            }
            Value::Int(i) => {
                v.ty = abi::ty::I64;
                v.i = i;
            }
            Value::UInt(u) => {
                v.ty = abi::ty::U64;
                v.u = u;
            }
            Value::Float(f) => {
                v.ty = abi::ty::F64;
                v.f = f;
            }
            Value::Vec2(p) => {
                v.ty = abi::ty::VEC2;
                v.x = p.x;
                v.y = p.y;
            }
            Value::Str(s) => {
                v.ty = abi::ty::STR;
                v.s = Str::new(s);
            }
            Value::Entity(e) => {
                v.ty = abi::ty::ENTITY;
                v.u = e.0;
            }
        }
        v
    }
}

fn fields(fields: &[(&str, Value<'_>)]) -> Vec<abi::Field> {
    fields
        .iter()
        .map(|(n, v)| abi::Field {
            name: Str::new(n),
            value: v.raw(),
        })
        .collect()
}

/// One run of one system: the world as its declaration lets it see it.
///
/// Everything here lives as long as the run, and no longer: a column, a
/// row, or the context itself cannot be kept for the next tick (§7, a
/// system holds no references across frames). State that must last is in
/// components.
pub struct Ctx<'a> {
    raw: *mut abi::RunCtx,
    _run: PhantomData<&'a mut abi::RunCtx>,
}

impl<'a> Ctx<'a> {
    /// # Safety
    /// `raw` must be the engine's, for the length of `'a`.
    pub(crate) unsafe fn from_raw(raw: *mut abi::RunCtx) -> Ctx<'a> {
        Ctx {
            raw,
            _run: PhantomData,
        }
    }

    fn api(&self) -> &abi::HostApi {
        // SAFETY: the engine's table lives as long as the engine.
        unsafe { &*(*self.raw).api }
    }

    fn host(&self) -> *mut c_void {
        // SAFETY: valid for the run.
        unsafe { (*self.raw).host }
    }

    fn ok(status: u32) -> Result {
        if status == abi::status::OK {
            Ok(())
        } else {
            Err(Error::engine())
        }
    }

    /// Tick number on the system's clock.
    pub fn tick(&self) -> u64 {
        // SAFETY: valid for the run.
        unsafe { (*self.raw).tick }
    }

    /// Sim time in seconds: tick × timestep, never the wall clock.
    pub fn time(&self) -> f64 {
        // SAFETY: valid for the run.
        unsafe { (*self.raw).time }
    }

    /// The timestep in seconds.
    pub fn dt(&self) -> f64 {
        // SAFETY: valid for the run.
        unsafe { (*self.raw).dt }
    }

    /// The continuous clock's speed as the tick began (ticks a frame).
    pub fn speed(&self) -> f64 {
        // SAFETY: valid for the run.
        unsafe { (*self.raw).speed }
    }

    /// Whether the tick is frozen (a hit-stop); only `unfrozen` systems run.
    pub fn frozen(&self) -> bool {
        // SAFETY: valid for the run.
        unsafe { (*self.raw).frozen != 0 }
    }

    pub(crate) fn fail(&self, message: &str) {
        // SAFETY: the engine copies the message before it returns.
        unsafe { (self.api().fail)(self.host(), Str::new(message)) }
    }

    /// A component's id, by name.
    pub fn component(&self, name: &str) -> Result<u32> {
        let mut out = 0u32;
        // SAFETY: `out` is written before the call returns.
        Self::ok(unsafe { (self.api().component)(self.host(), Str::new(name), &mut out) })?;
        Ok(out)
    }

    pub(crate) fn column<T: View>(&self, write: bool) -> Result<abi::Column> {
        Ok(abi::Column {
            component: self.component(T::NAME)?,
            write: write as u32,
            size: core::mem::size_of::<T>(),
            fingerprint: T::FINGERPRINT,
        })
    }

    /// Visit every entity with the fetched components: `(&Enemy, &mut
    /// Transform)` reads Enemy and writes Transform, in the engine's row
    /// order. Each must be in the system's declaration.
    pub fn each<F: Fetch>(&self, f: impl FnMut(Entity, F::Item<'_>)) -> Result {
        self.query::<F>().each(f)
    }

    /// A query to narrow before visiting: `ctx.query::<(&Enemy,)>().without::<Dead>()`.
    pub fn query<F: Fetch>(&self) -> Query<'_, 'a, F> {
        Query {
            ctx: self,
            without: Vec::new(),
            _f: PhantomData,
        }
    }

    /// Copy out an entity's component; a declared read.
    pub fn get<T: View>(&self, e: Entity) -> Result<T> {
        let col = self.column::<T>(false)?;
        let mut out = MaybeUninit::<T>::uninit();
        // SAFETY: the engine writes size_of::<T>() bytes of a row whose
        // layout it checked against T's.
        Self::ok(unsafe { (self.api().read)(self.host(), e.0, &col, out.as_mut_ptr().cast()) })?;
        // SAFETY: written in full above.
        Ok(unsafe { out.assume_init() })
    }

    /// Write an entity's component; a declared write.
    pub fn set<T: View>(&self, e: Entity, value: T) -> Result {
        let col = self.column::<T>(true)?;
        // SAFETY: the engine copies size_of::<T>() bytes from `value`.
        Self::ok(unsafe { (self.api().write)(self.host(), e.0, &col, (&value as *const T).cast()) })
    }

    /// Change an entity's component in place; a declared write.
    pub fn update<T: View, R>(&self, e: Entity, f: impl FnOnce(&mut T) -> R) -> Result<R> {
        let mut v = self.get::<T>(e)?;
        let r = f(&mut v);
        self.set(e, v)?;
        Ok(r)
    }

    /// Whether the entity has the component (no declaration needed).
    pub fn has<T: View>(&self, e: Entity) -> bool {
        self.has_named(e, T::NAME)
    }

    /// [`Ctx::has`] by the component's name.
    pub fn has_named(&self, e: Entity, component: &str) -> bool {
        match self.component(component) {
            // SAFETY: plain values.
            Ok(c) => unsafe { (self.api().has)(self.host(), e.0, c) != 0 },
            Err(_) => false,
        }
    }

    /// Whether the entity is alive.
    pub fn alive(&self, e: Entity) -> bool {
        // SAFETY: plain values.
        unsafe { (self.api().alive)(self.host(), e.0) != 0 }
    }

    /// An authored entity by `@id` (or `e:n`).
    pub fn entity(&self, r: &str) -> Result<Entity> {
        let mut out = 0u64;
        // SAFETY: `out` is written before the call returns.
        Self::ok(unsafe { (self.api().entity)(self.host(), Str::new(r), &mut out) })?;
        Ok(Entity(out))
    }

    /// Emit a gameplay event (§8). It joins the log when the run ends.
    pub fn emit(&self, name: &str, fields: &[(&str, Value<'_>)]) -> Result {
        let f = self::fields(fields);
        // SAFETY: the engine copies the name and fields before it returns.
        Self::ok(unsafe { (self.api().emit)(self.host(), Str::new(name), f.as_ptr(), f.len()) })
    }

    /// Spawn an entity with these components (each declared written or
    /// spawned) and `Component.field` values, when the run ends.
    pub fn spawn(&self, components: &[&str], values: &[(&str, Value<'_>)]) -> Result {
        let ids = components
            .iter()
            .map(|c| self.component(c))
            .collect::<Result<Vec<u32>>>()?;
        let f = self::fields(values);
        // SAFETY: the engine copies everything before it returns.
        Self::ok(unsafe {
            (self.api().spawn)(self.host(), ids.as_ptr(), ids.len(), f.as_ptr(), f.len())
        })
    }

    /// Despawn an entity, when the run ends.
    pub fn despawn(&self, e: Entity) -> Result {
        // SAFETY: plain values.
        Self::ok(unsafe { (self.api().despawn)(self.host(), e.0) })
    }

    fn draw(&self, op: u32, a: f64, b: f64) -> Result<f64> {
        let mut out = 0.0;
        // SAFETY: `out` is written before the call returns.
        Self::ok(unsafe { (self.api().rng)(self.host(), op, a, b, &mut out) })?;
        Ok(out)
    }

    /// The system's random stream (§6): the engine's own generator, so
    /// each draw is the one a Luau system makes with `q:rng()`.
    pub fn rng(&self) -> Rng<'_, 'a> {
        Rng { ctx: self }
    }

    /// Live entities within `r` of a point, nearest first, from the shared
    /// broadphase (§12).
    pub fn within(&self, x: f32, y: f32, r: f32) -> Result<Vec<(Entity, f32)>> {
        unsafe extern "C" fn found(user: *mut c_void, e: u64, d: f32) {
            // SAFETY: `user` is the vector below, for the call.
            let out = unsafe { &mut *(user as *mut Vec<(Entity, f32)>) };
            out.push((Entity(e), d));
        }
        let mut out: Vec<(Entity, f32)> = Vec::new();
        // SAFETY: `found` only pushes into `out`, which outlives the call.
        Self::ok(unsafe {
            (self.api().within)(
                self.host(),
                x,
                y,
                r,
                found,
                (&mut out as *mut Vec<(Entity, f32)>).cast(),
            )
        })?;
        Ok(out)
    }

    /// A string field's text.
    pub fn string(&self, id: StrId) -> Result<&str> {
        let mut out = Str::EMPTY;
        // SAFETY: the engine's table keeps every string for the world's
        // life, which outlasts the run this borrow is tied to.
        Self::ok(unsafe { (self.api().string)(self.host(), id.0, &mut out) })?;
        // SAFETY: as above.
        Ok(unsafe { out.as_str() })
    }

    /// A string's id, added to the table when new (a parallel system may
    /// only find one the world holds).
    pub fn intern(&self, s: &str) -> Result<StrId> {
        let mut out = 0u32;
        // SAFETY: `out` is written before the call returns.
        Self::ok(unsafe { (self.api().intern)(self.host(), Str::new(s), &mut out) })?;
        Ok(StrId(out))
    }
}

/// A system's random stream ([`Ctx::rng`]).
pub struct Rng<'c, 'a> {
    ctx: &'c Ctx<'a>,
}

impl Rng<'_, '_> {
    /// A u32.
    pub fn u32(&self) -> Result<u32> {
        Ok(self.ctx.draw(abi::rng_op::U32, 0.0, 0.0)? as u32)
    }
    /// A float in [0, 1).
    pub fn f32(&self) -> Result<f32> {
        Ok(self.ctx.draw(abi::rng_op::F32, 0.0, 0.0)? as f32)
    }
    /// A float in [0, 1), 53 bits.
    pub fn f64(&self) -> Result<f64> {
        self.ctx.draw(abi::rng_op::F64, 0.0, 0.0)
    }
    /// An integer in [0, n).
    pub fn below(&self, n: u32) -> Result<u32> {
        Ok(self.ctx.draw(abi::rng_op::BELOW, n as f64, 0.0)? as u32)
    }
    /// An integer from lo to hi inclusive.
    pub fn range(&self, lo: i64, hi: i64) -> Result<i64> {
        Ok(self.ctx.draw(abi::rng_op::RANGE, lo as f64, hi as f64)? as i64)
    }
    /// True with probability p.
    pub fn chance(&self, p: f32) -> Result<bool> {
        Ok(self.ctx.draw(abi::rng_op::CHANCE, p as f64, 0.0)? != 0.0)
    }
}

/// A query being narrowed ([`Ctx::query`]).
pub struct Query<'c, 'a, F> {
    ctx: &'c Ctx<'a>,
    without: Vec<&'static str>,
    _f: PhantomData<F>,
}

impl<F: Fetch> Query<'_, '_, F> {
    /// Leave out entities with this component.
    pub fn without<T: View>(mut self) -> Self {
        self.without.push(T::NAME);
        self
    }

    /// Leave out entities with the named component.
    pub fn without_named(mut self, component: &'static str) -> Self {
        self.without.push(component);
        self
    }

    /// Visit every match.
    pub fn each<G: FnMut(Entity, F::Item<'_>)>(self, mut f: G) -> Result {
        struct Visit<'g, G> {
            f: &'g mut G,
            panic: Option<Box<dyn std::any::Any + Send>>,
        }
        unsafe extern "C" fn chunk<F: Fetch, G: FnMut(Entity, F::Item<'_>)>(
            user: *mut c_void,
            chunk: *const abi::Chunk,
        ) {
            // SAFETY: `user` is the Visit below, for the call.
            let v = unsafe { &mut *(user as *mut Visit<'_, G>) };
            if v.panic.is_some() {
                return;
            }
            // SAFETY: the engine gives one valid chunk per call, its
            // columns borrowed as asked until the call returns.
            let c = unsafe { &*chunk };
            let f = &mut *v.f;
            // A panic may not unwind through the engine: it is kept and
            // raised again once the engine's call returns.
            let r = catch_unwind(AssertUnwindSafe(|| {
                for row in 0..c.len {
                    // SAFETY: row < len; columns as above.
                    let (e, item) =
                        unsafe { (Entity(*c.entities.add(row)), F::item(c.columns, row)) };
                    f(e, item);
                }
            }));
            if let Err(p) = r {
                v.panic = Some(p);
            }
        }
        let mut cols = Vec::with_capacity(F::LEN);
        F::columns(self.ctx, &mut cols)?;
        let without = self
            .without
            .iter()
            .map(|n| self.ctx.component(n))
            .collect::<Result<Vec<u32>>>()?;
        let q = abi::Query {
            columns: cols.as_ptr(),
            len: cols.len(),
            without: without.as_ptr(),
            without_len: without.len(),
        };
        let mut visit = Visit {
            f: &mut f,
            panic: None,
        };
        // SAFETY: `chunk` matches F's columns, built above in order.
        let status = unsafe {
            (self.ctx.api().each)(
                self.ctx.host(),
                &q,
                chunk::<F, G>,
                (&mut visit as *mut Visit<'_, G>).cast(),
            )
        };
        if let Some(p) = visit.panic {
            resume_unwind(p);
        }
        Ctx::ok(status)
    }
}
