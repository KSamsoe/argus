//! The engine's side of a plugin system's run: [`Call`], and the
//! [`HostApi`] table every call into the world goes through.
//!
//! A call is checked as the engine's own contexts check a Rust system's:
//! a column fetched must be declared (read, or written to write it), and
//! must be laid out as the game's component (its fingerprint); a stream
//! must be declared to draw from. A refusal is recorded and answered with
//! `ERR`; a system that returns it is stopped with the engine's message.
//!
//! Events, spawns, and despawns are kept in the call and applied when the
//! run returns, through the system's own context, so a plugin system
//! behaves the same alone and in a parallel batch, and no part of the
//! world is changed through two paths at once.

use argus_ecs::{codes as ecs_codes, Access, ComponentId, Entity, StrId, Value, Vec2, World};
use argus_plugin::abi::{self, HostApi, Str};
use argus_sim::Pcg32;
use core::ffi::c_void;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// A change kept for the end of the run.
pub(crate) enum Command {
    Spawn {
        comps: Vec<ComponentId>,
        fields: Vec<(String, Value)>,
    },
    Despawn(Entity),
}

/// Events kept for the end of the run: name and fields.
pub(crate) type Events = Vec<(String, Vec<(String, Value)>)>;

/// One run, from the engine's side.
pub(crate) struct Call<'r> {
    pub(crate) world: &'r World,
    pub(crate) access: &'r Access,
    pub(crate) spawns: &'r [ComponentId],
    pub(crate) name: &'r str,
    rng: RefCell<Option<&'r mut Pcg32>>,
    pub(crate) events: RefCell<Events>,
    pub(crate) commands: RefCell<Vec<Command>>,
    /// The engine's last refusal.
    pub(crate) refused: RefCell<Option<String>>,
    /// The plugin's own message.
    pub(crate) failed: RefCell<Option<String>>,
}

impl<'r> Call<'r> {
    pub(crate) fn new(
        world: &'r World,
        access: &'r Access,
        spawns: &'r [ComponentId],
        rng: Option<&'r mut Pcg32>,
        name: &'r str,
    ) -> Call<'r> {
        Call {
            world,
            access,
            spawns,
            name,
            rng: RefCell::new(rng),
            events: RefCell::new(Vec::new()),
            commands: RefCell::new(Vec::new()),
            refused: RefCell::new(None),
            failed: RefCell::new(None),
        }
    }

    fn refuse(&self, message: String) -> u32 {
        *self.refused.borrow_mut() = Some(message);
        abi::status::ERR
    }

    fn component(&self, c: u32) -> Result<ComponentId, String> {
        if (c as usize) < self.world.registry().len() {
            Ok(ComponentId(c))
        } else {
            Err(format!("{c} is not a component id"))
        }
    }

    fn cname(&self, c: ComponentId) -> &str {
        &self.world.registry().schema(c).name
    }

    fn violation(&self, c: ComponentId, write: bool) -> String {
        let verb = if write { "writes" } else { "reads" };
        format!(
            "system {} {verb} {} without declaring it (code {})",
            self.name,
            self.cname(c),
            ecs_codes::ACCESS_VIOLATION
        )
    }

    /// A column as asked for: declared, and laid out as the game's.
    fn column(&self, col: &abi::Column) -> Result<ComponentId, String> {
        let c = self.component(col.component)?;
        let write = col.write != 0;
        let ok = if write {
            self.access.allows_write(c)
        } else {
            self.access.allows_read(c)
        };
        if !ok {
            return Err(self.violation(c, write));
        }
        let schema = self.world.registry().schema(c);
        if col.size != schema.size || col.fingerprint != fingerprint(schema) {
            return Err(format!(
                "the plugin reads {} laid out otherwise than the game declares it ({}); declare it field for field as the game does",
                schema.name,
                describe(schema)
            ));
        }
        Ok(c)
    }

    /// Run the system's body.
    pub(crate) fn run(&self, run: abi::RunFn, user: usize, t: (u64, f64, f64, f64, bool)) -> u32 {
        let mut raw = abi::RunCtx {
            api: &API,
            host: self as *const Call<'_> as *mut c_void,
            tick: t.0,
            time: t.1,
            dt: t.2,
            speed: t.3,
            frozen: t.4 as u32,
        };
        // SAFETY: the plugin's run function, from a library kept loaded by
        // the system holding it; `raw` lives for the call.
        unsafe { run(&mut raw, user as *const c_void) }
    }

    /// The run's message, for a status other than OK.
    pub(crate) fn message(&self, status: u32) -> String {
        if let Some(m) = self.failed.borrow().clone() {
            return m;
        }
        if let Some(m) = self.refused.borrow().clone() {
            return m;
        }
        match status {
            abi::status::PANIC => "panic".to_string(),
            _ => "the system failed".to_string(),
        }
    }
}

/// A schema's fingerprint, as a plugin computes its layout's.
pub(crate) fn fingerprint(s: &argus_ecs::ComponentSchema) -> u64 {
    abi::fingerprint(
        s.size,
        s.align,
        s.fields
            .iter()
            .map(|f| (f.name.as_str(), ty_code(f.ty), f.offset)),
    )
}

/// A schema's layout in words, for a refusal.
pub(crate) fn describe(s: &argus_ecs::ComponentSchema) -> String {
    let fields: Vec<String> = s
        .fields
        .iter()
        .map(|f| format!("{} {} at {}", f.name, f.ty, f.offset))
        .collect();
    format!("{} bytes: {}", s.size, fields.join(", "))
}

/// The ABI's number for a field type.
pub(crate) fn ty_code(t: argus_ecs::FieldType) -> u32 {
    use argus_ecs::FieldType as F;
    match t {
        F::Bool => abi::ty::BOOL,
        F::U8 => abi::ty::U8,
        F::U16 => abi::ty::U16,
        F::U32 => abi::ty::U32,
        F::U64 => abi::ty::U64,
        F::I8 => abi::ty::I8,
        F::I16 => abi::ty::I16,
        F::I32 => abi::ty::I32,
        F::I64 => abi::ty::I64,
        F::F32 => abi::ty::F32,
        F::F64 => abi::ty::F64,
        F::Vec2 => abi::ty::VEC2,
        F::Str => abi::ty::STR,
        F::Entity => abi::ty::ENTITY,
        F::Ref => abi::ty::REF,
    }
}

fn value(v: &abi::Value) -> Result<Value, String> {
    Ok(match v.ty {
        abi::ty::BOOL => Value::Bool(v.u != 0),
        abi::ty::I64 => Value::Int(v.i),
        abi::ty::U64 => Value::UInt(v.u),
        abi::ty::F64 => Value::Float(v.f),
        abi::ty::VEC2 => Value::Vec2(Vec2::new(v.x, v.y)),
        // SAFETY: the plugin's text, alive for the call.
        abi::ty::STR => Value::Str(unsafe { v.s.as_str() }.to_string()),
        abi::ty::ENTITY => Value::Entity(Entity::from_bits(v.u)),
        t => return Err(format!("a value of type {t} is not one a plugin can send")),
    })
}

fn fields(f: *const abi::Field, n: usize) -> Result<Vec<(String, Value)>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: the plugin's fields, alive for the call.
    let f = unsafe { std::slice::from_raw_parts(f, n) };
    f.iter()
        // SAFETY: as above.
        .map(|f| Ok((unsafe { f.name.as_str() }.to_string(), value(&f.value)?)))
        .collect()
}

/// Run `f` on the call behind `host`, never letting a panic out.
fn guard(host: *mut c_void, f: impl FnOnce(&Call<'_>) -> u32) -> u32 {
    // SAFETY: `host` is the Call a run passed, alive for the run.
    let c = unsafe { &*(host as *const Call<'_>) };
    match catch_unwind(AssertUnwindSafe(|| f(c))) {
        Ok(s) => s,
        Err(_) => c.refuse("the engine failed inside a plugin's call".to_string()),
    }
}

unsafe extern "C" fn component(host: *mut c_void, name: Str, out: *mut u32) -> u32 {
    guard(host, |c| {
        // SAFETY: the plugin's text, for the call.
        let n = unsafe { name.as_str() };
        match c.world.component_id(n) {
            Some(id) => {
                // SAFETY: the plugin's out pointer.
                unsafe { *out = id.0 };
                abi::status::OK
            }
            None => c.refuse(format!("the game has no component {n:?}")),
        }
    })
}

unsafe extern "C" fn each(
    host: *mut c_void,
    query: *const abi::Query,
    f: abi::ChunkFn,
    user: *mut c_void,
) -> u32 {
    guard(host, |c| {
        // SAFETY: the plugin's query, for the call.
        let q = unsafe { &*query };
        let cols = if q.len == 0 {
            &[][..]
        } else {
            // SAFETY: as above.
            unsafe { std::slice::from_raw_parts(q.columns, q.len) }
        };
        let without = if q.without_len == 0 {
            &[][..]
        } else {
            // SAFETY: as above.
            unsafe { std::slice::from_raw_parts(q.without, q.without_len) }
        };
        let mut ids = Vec::with_capacity(cols.len());
        for col in cols {
            match c.column(col) {
                Ok(id) => ids.push(id),
                Err(m) => return c.refuse(m),
            }
        }
        let mut query = c.world.query().access_ref(c.access);
        for &id in &ids {
            query = query.has(id);
        }
        for &w in without {
            match c.component(w) {
                Ok(id) => query = query.not(id),
                Err(m) => return c.refuse(m),
            }
        }
        let mut ptrs: Vec<*mut u8> = vec![std::ptr::null_mut(); ids.len()];
        let r = query.for_each_view(|view| {
            let mut held = Vec::with_capacity(ids.len());
            for (k, (&id, col)) in ids.iter().zip(cols).enumerate() {
                let d = if col.write != 0 {
                    view.dyn_column_mut(id)?
                } else {
                    view.dyn_column(id)?
                };
                ptrs[k] = d.as_ptr();
                held.push(d);
            }
            let chunk = abi::Chunk {
                len: view.len(),
                entities: view.entities().as_ptr() as *const u64,
                columns: ptrs.as_ptr(),
            };
            // SAFETY: the plugin's visit, with columns borrowed as asked
            // until `held` drops after it.
            unsafe { f(user, &chunk) };
            drop(held);
            Ok(())
        });
        match r {
            Ok(()) => abi::status::OK,
            Err(e) => c.refuse(e.message),
        }
    })
}

unsafe extern "C" fn read(host: *mut c_void, e: u64, col: *const abi::Column, out: *mut u8) -> u32 {
    guard(host, |c| {
        // SAFETY: the plugin's column, for the call.
        let col = unsafe { &*col };
        let id = match c.column(&abi::Column { write: 0, ..*col }) {
            Ok(id) => id,
            Err(m) => return c.refuse(m),
        };
        let r = c.world.with_row(Entity::from_bits(e), id, |row| {
            // SAFETY: `out` has room for the row (sizes checked above).
            unsafe { std::ptr::copy_nonoverlapping(row.as_ptr(), out, row.len()) }
        });
        match r {
            Ok(()) => abi::status::OK,
            Err(e) => c.refuse(e.message),
        }
    })
}

unsafe extern "C" fn write(
    host: *mut c_void,
    e: u64,
    col: *const abi::Column,
    src: *const u8,
) -> u32 {
    guard(host, |c| {
        // SAFETY: the plugin's column, for the call.
        let col = unsafe { &*col };
        let id = match c.column(&abi::Column { write: 1, ..*col }) {
            Ok(id) => id,
            Err(m) => return c.refuse(m),
        };
        let r = c.world.with_row_mut(Entity::from_bits(e), id, |row| {
            // SAFETY: `src` holds a row of this layout (checked above).
            unsafe { std::ptr::copy_nonoverlapping(src, row.as_mut_ptr(), row.len()) }
        });
        match r {
            Ok(()) => abi::status::OK,
            Err(e) => c.refuse(e.message),
        }
    })
}

unsafe extern "C" fn has(host: *mut c_void, e: u64, comp: u32) -> u32 {
    guard(host, |c| match c.component(comp) {
        Ok(id) => c.world.has(Entity::from_bits(e), id) as u32,
        Err(_) => 0,
    })
}

unsafe extern "C" fn alive(host: *mut c_void, e: u64) -> u32 {
    guard(host, |c| c.world.is_alive(Entity::from_bits(e)) as u32)
}

unsafe extern "C" fn entity(host: *mut c_void, r: Str, out: *mut u64) -> u32 {
    guard(host, |c| {
        // SAFETY: the plugin's text, for the call.
        match c.world.entity(unsafe { r.as_str() }) {
            Ok(e) => {
                // SAFETY: the plugin's out pointer.
                unsafe { *out = e.to_bits() };
                abi::status::OK
            }
            Err(e) => c.refuse(e.message),
        }
    })
}

unsafe extern "C" fn emit(host: *mut c_void, name: Str, f: *const abi::Field, n: usize) -> u32 {
    guard(host, |c| match fields(f, n) {
        Ok(fields) => {
            // SAFETY: the plugin's text, for the call.
            let name = unsafe { name.as_str() }.to_string();
            c.events.borrow_mut().push((name, fields));
            abi::status::OK
        }
        Err(m) => c.refuse(m),
    })
}

unsafe extern "C" fn spawn(
    host: *mut c_void,
    comps: *const u32,
    n: usize,
    f: *const abi::Field,
    nf: usize,
) -> u32 {
    guard(host, |c| {
        let raw = if n == 0 {
            &[][..]
        } else {
            // SAFETY: the plugin's ids, for the call.
            unsafe { std::slice::from_raw_parts(comps, n) }
        };
        let mut ids = Vec::with_capacity(n);
        for &r in raw {
            let id = match c.component(r) {
                Ok(id) => id,
                Err(m) => return c.refuse(m),
            };
            if !c.access.allows_write(id) && !c.spawns.contains(&id) {
                return c.refuse(format!(
                    "system {} spawns {} without declaring it: add it to the system's spawns",
                    c.name,
                    c.cname(id)
                ));
            }
            ids.push(id);
        }
        let fields = match fields(f, nf) {
            Ok(f) => f,
            Err(m) => return c.refuse(m),
        };
        for (path, v) in &fields {
            let checked = c.world.registry().resolve_path(path).and_then(|(id, fs)| {
                if !ids.contains(&id) {
                    return Err(argus_ecs::ArgusError::new(
                        ecs_codes::MISSING_COMPONENT,
                        format!("{path} is not a field of the components spawned"),
                    ));
                }
                c.world.registry().schema(id).check(fs, v.clone())
            });
            if let Err(e) = checked {
                return c.refuse(e.message);
            }
        }
        c.commands
            .borrow_mut()
            .push(Command::Spawn { comps: ids, fields });
        abi::status::OK
    })
}

unsafe extern "C" fn despawn(host: *mut c_void, e: u64) -> u32 {
    guard(host, |c| {
        let e = Entity::from_bits(e);
        if !c.world.is_alive(e) {
            return c.refuse(format!("{e} is not alive"));
        }
        c.commands.borrow_mut().push(Command::Despawn(e));
        abi::status::OK
    })
}

unsafe extern "C" fn rng(host: *mut c_void, op: u32, a: f64, b: f64, out: *mut f64) -> u32 {
    guard(host, |c| {
        let mut slot = c.rng.borrow_mut();
        let Some(r) = slot.as_deref_mut() else {
            drop(slot);
            return c.refuse(format!(
                "system {} uses randomness without declaring a stream: give it .rng(\"name\")",
                c.name
            ));
        };
        let v = match op {
            abi::rng_op::U32 => r.next_u32() as f64,
            abi::rng_op::F32 => r.next_f32() as f64,
            abi::rng_op::F64 => r.next_f64(),
            abi::rng_op::BELOW => r.below(a as u32) as f64,
            abi::rng_op::RANGE => r.range(a as i64, b as i64) as f64,
            abi::rng_op::CHANCE => r.chance(a as f32) as u32 as f64,
            other => {
                drop(slot);
                return c.refuse(format!("{other} is not a draw"));
            }
        };
        // SAFETY: the plugin's out pointer.
        unsafe { *out = v };
        abi::status::OK
    })
}

unsafe extern "C" fn within(
    host: *mut c_void,
    x: f32,
    y: f32,
    r: f32,
    f: abi::NearFn,
    user: *mut c_void,
) -> u32 {
    guard(host, |c| {
        for (e, d) in c.world.within(x, y, r) {
            // SAFETY: the plugin's callback, for the call.
            unsafe { f(user, e.to_bits(), d) };
        }
        abi::status::OK
    })
}

unsafe extern "C" fn string(host: *mut c_void, id: u32, out: *mut Str) -> u32 {
    guard(host, |c| {
        let s = c.world.strings().get(StrId(id));
        // The table holds every string for the world's life, shared and
        // never moved, so the text outlives the guard.
        // SAFETY: the plugin's out pointer.
        unsafe { *out = Str::new(&s) };
        abi::status::OK
    })
}

unsafe extern "C" fn intern(host: *mut c_void, s: Str, out: *mut u32) -> u32 {
    guard(host, |c| {
        // SAFETY: the plugin's text, for the call.
        match c.world.strings().try_intern(unsafe { s.as_str() }) {
            Ok(id) => {
                // SAFETY: the plugin's out pointer.
                unsafe { *out = id.0 };
                abi::status::OK
            }
            Err(e) => c.refuse(e.message),
        }
    })
}

unsafe extern "C" fn fail(host: *mut c_void, message: Str) {
    guard(host, |c| {
        // SAFETY: the plugin's text, for the call.
        *c.failed.borrow_mut() = Some(unsafe { message.as_str() }.to_string());
        abi::status::OK
    });
}

pub(crate) static API: HostApi = HostApi {
    abi: abi::ABI_VERSION,
    component,
    each,
    read,
    write,
    has,
    alive,
    entity,
    emit,
    spawn,
    despawn,
    rng,
    within,
    string,
    intern,
    fail,
};
