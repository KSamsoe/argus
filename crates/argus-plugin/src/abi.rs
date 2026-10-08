//! What crosses between the engine and a plugin's library: plain C structs
//! and functions, nothing else. The two sides are built separately (the
//! plugin by its own cargo, maybe by another compiler of the same Rust), so
//! no type of either side's own, a `World` or a `Vec`, ever crosses.
//!
//! A library exports one function, [`ENTRY`], returning its
//! [`PluginDecl`]: its systems and the component layouts it reads columns
//! as. The engine checks both against the game, then calls a system's
//! [`RunFn`] once a tick with a [`RunCtx`], whose [`HostApi`] is the only
//! way back into the world.

use core::ffi::c_void;

/// The version of this boundary. A library built against another one is
/// refused at load, with a hint to rebuild it.
pub const ABI_VERSION: u32 = 1;

/// The one symbol a plugin library exports: an [`EntryFn`].
pub const ENTRY: &str = "argus_plugin_v1";

/// UTF-8 text, borrowed for as long as the call it is passed to (or, in a
/// [`PluginDecl`], for as long as the library is loaded).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Str {
    /// First byte.
    pub ptr: *const u8,
    /// Length in bytes.
    pub len: usize,
}

impl Str {
    /// Empty text.
    pub const EMPTY: Str = Str {
        ptr: core::ptr::NonNull::<u8>::dangling().as_ptr(),
        len: 0,
    };

    /// Borrow a `&str`.
    pub const fn new(s: &str) -> Str {
        Str {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// The text.
    ///
    /// # Safety
    /// `ptr` must point at `len` bytes of UTF-8 that outlive `'a`.
    pub unsafe fn as_str<'a>(self) -> &'a str {
        if self.len == 0 {
            return "";
        }
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(self.ptr, self.len))
    }
}

/// A list of [`Str`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Strs {
    /// First item.
    pub ptr: *const Str,
    /// Count.
    pub len: usize,
}

impl Strs {
    /// The items.
    ///
    /// # Safety
    /// `ptr` must point at `len` items that outlive `'a`.
    pub unsafe fn as_slice<'a>(self) -> &'a [Str] {
        if self.len == 0 {
            return &[];
        }
        core::slice::from_raw_parts(self.ptr, self.len)
    }
}

/// Field types, numbered as the engine's `FieldType`.
pub mod ty {
    /// 1 byte, 0 or 1.
    pub const BOOL: u32 = 1;
    /// Unsigned 8-bit.
    pub const U8: u32 = 2;
    /// Unsigned 16-bit.
    pub const U16: u32 = 3;
    /// Unsigned 32-bit.
    pub const U32: u32 = 4;
    /// Unsigned 64-bit.
    pub const U64: u32 = 5;
    /// Signed 8-bit.
    pub const I8: u32 = 6;
    /// Signed 16-bit.
    pub const I16: u32 = 7;
    /// Signed 32-bit.
    pub const I32: u32 = 8;
    /// Signed 64-bit.
    pub const I64: u32 = 9;
    /// 32-bit float.
    pub const F32: u32 = 10;
    /// 64-bit float.
    pub const F64: u32 = 11;
    /// Two f32s.
    pub const VEC2: u32 = 12;
    /// A string's id in the world's table.
    pub const STR: u32 = 13;
    /// An entity.
    pub const ENTITY: u32 = 14;
    /// A def, asset, or prefab reference: stored as a string's id.
    pub const REF: u32 = 15;

    /// The type's name, as a component declaration writes it.
    pub fn name(t: u32) -> &'static str {
        match t {
            BOOL => "bool",
            U8 => "u8",
            U16 => "u16",
            U32 => "u32",
            U64 => "u64",
            I8 => "i8",
            I16 => "i16",
            I32 => "i32",
            I64 => "i64",
            F32 => "f32",
            F64 => "f64",
            VEC2 => "vec2",
            STR => "string",
            ENTITY => "entity",
            REF => "ref",
            _ => "unknown",
        }
    }
}

/// One field of a component as the plugin lays it out.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FieldLayout {
    /// Name.
    pub name: Str,
    /// A [`ty`] number.
    pub ty: u32,
    /// Byte offset in the row.
    pub offset: usize,
}

/// A component as the plugin reads its column: the engine checks it against
/// the game's declaration before the plugin's systems run.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComponentLayout {
    /// The component's name.
    pub name: Str,
    /// Row size.
    pub size: usize,
    /// Row alignment.
    pub align: usize,
    /// Fields, in row order.
    pub fields: *const FieldLayout,
    /// Field count.
    pub fields_len: usize,
    /// Where the plugin declares it: `src/lib.rs:12`.
    pub source: Str,
}

/// A layout's fingerprint: what a column fetch carries so the engine can
/// refuse a view that does not match the game's component (FNV-1a over the
/// size, the alignment, and each field's name, type, and offset).
pub fn fingerprint<'a>(
    size: usize,
    align: usize,
    fields: impl IntoIterator<Item = (&'a str, u32, usize)>,
) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut put = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    put(&(size as u64).to_le_bytes());
    put(&(align as u64).to_le_bytes());
    for (name, ty, offset) in fields {
        put(name.as_bytes());
        put(&[0]);
        put(&ty.to_le_bytes());
        put(&(offset as u64).to_le_bytes());
    }
    h
}

/// System flags.
pub mod flags {
    /// May run in a batch beside other parallel systems (on threads).
    pub const PARALLEL: u32 = 1;
    /// Also runs once when a scene loads.
    pub const ON_LOAD: u32 = 2;
    /// Runs first in its stage.
    pub const FIRST: u32 = 4;
    /// Runs last in its stage.
    pub const LAST: u32 = 8;
    /// Runs while the sim is frozen.
    pub const UNFROZEN: u32 = 16;
}

/// Clock bindings.
pub mod clock {
    /// The scene's driving clock.
    pub const DRIVING: u32 = 0;
    /// The continuous clock.
    pub const CONTINUOUS: u32 = 1;
    /// The discrete clock.
    pub const DISCRETE: u32 = 2;
}

/// What a call answers.
pub mod status {
    /// Done.
    pub const OK: u32 = 0;
    /// Refused or failed; the message was given to [`super::HostApi::fail`]
    /// or is the engine's own.
    pub const ERR: u32 = 1;
    /// The system panicked; the plugin caught it.
    pub const PANIC: u32 = 2;
}

/// A system's body.
pub type RunFn = unsafe extern "C" fn(ctx: *mut RunCtx, user: *const c_void) -> u32;

/// One system, declared as a Luau system is (§19: a system moves from Luau
/// to Rust without anything around it changing).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SystemDecl {
    /// Unique name, dotted: `sim.chase`.
    pub name: Str,
    /// Stage: `input`, `sim`, `late`, `hierarchy`, or `present`.
    pub stage: Str,
    /// What it does.
    pub doc: Str,
    /// Where it is declared: `src/lib.rs:40`.
    pub source: Str,
    /// Systems of its stage it runs after.
    pub after: Strs,
    /// Components it reads.
    pub reads: Strs,
    /// Components it writes.
    pub writes: Strs,
    /// Components it spawns with, inserts, or removes.
    pub spawns: Strs,
    /// Its random stream, empty for none.
    pub rng: Str,
    /// [`flags`].
    pub flags: u32,
    /// A [`clock`] binding.
    pub clock: u32,
    /// Its body.
    pub run: RunFn,
    /// Passed back to `run`.
    pub user: *const c_void,
}

/// What a library declares.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PluginDecl {
    /// [`ABI_VERSION`] as the library was built.
    pub abi: u32,
    /// The crate's name.
    pub name: Str,
    /// Its systems.
    pub systems: *const SystemDecl,
    /// System count.
    pub systems_len: usize,
    /// The component layouts it reads columns as.
    pub components: *const ComponentLayout,
    /// Layout count.
    pub components_len: usize,
}

/// The [`ENTRY`] function.
pub type EntryFn = unsafe extern "C" fn() -> *const PluginDecl;

/// One run of one system.
#[repr(C)]
pub struct RunCtx {
    /// The way back into the world.
    pub api: *const HostApi,
    /// The engine's side of this run; passed to every [`HostApi`] call.
    pub host: *mut c_void,
    /// Tick number on the system's clock.
    pub tick: u64,
    /// Sim time on that clock, in seconds.
    pub time: f64,
    /// Its timestep.
    pub dt: f64,
    /// The continuous clock's speed as the tick began.
    pub speed: f64,
    /// 1 while the tick is frozen (a hit-stop).
    pub frozen: u32,
}

/// One column a fetch asks for.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Column {
    /// The component's id ([`HostApi::component`]).
    pub component: u32,
    /// 1 to write it.
    pub write: u32,
    /// The plugin's row size.
    pub size: usize,
    /// The plugin's layout [`fingerprint`].
    pub fingerprint: u64,
}

/// A query: the entities that have every column's component and none of
/// `without`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Query {
    /// Columns fetched.
    pub columns: *const Column,
    /// Column count.
    pub len: usize,
    /// Components the entities must not have.
    pub without: *const u32,
    /// Count.
    pub without_len: usize,
}

/// One archetype's rows, as a query visits it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Chunk {
    /// Rows.
    pub len: usize,
    /// The rows' entities.
    pub entities: *const u64,
    /// One pointer per column asked for, to its first row.
    pub columns: *const *mut u8,
}

/// Called with each chunk a query visits.
pub type ChunkFn = unsafe extern "C" fn(user: *mut c_void, chunk: *const Chunk);

/// Called with each entity a spatial query finds.
pub type NearFn = unsafe extern "C" fn(user: *mut c_void, entity: u64, distance: f32);

/// A value: an event's field, a spawn's field.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Value {
    /// A [`ty`] number: `BOOL` (in `u`), `I64` (in `i`), `U64` (in `u`),
    /// `F64` (in `f`), `VEC2` (`x`, `y`), `STR` (`s`), `ENTITY` (in `u`).
    pub ty: u32,
    /// Signed integers.
    pub i: i64,
    /// Unsigned integers, bools, entities.
    pub u: u64,
    /// Floats.
    pub f: f64,
    /// A vector's x.
    pub x: f32,
    /// A vector's y.
    pub y: f32,
    /// Text.
    pub s: Str,
}

/// A named value.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// `name`, or `Component.field` in a spawn.
    pub name: Str,
    /// The value.
    pub value: Value,
}

/// Random draws, made with the engine's own stream so a draw is the one a
/// Luau system makes.
pub mod rng_op {
    /// A u32.
    pub const U32: u32 = 0;
    /// A float in [0, 1), 24 bits.
    pub const F32: u32 = 1;
    /// A float in [0, 1), 53 bits.
    pub const F64: u32 = 2;
    /// An integer below `a`.
    pub const BELOW: u32 = 3;
    /// An integer from `a` to `b` inclusive.
    pub const RANGE: u32 = 4;
    /// True with probability `a` (1 or 0).
    pub const CHANCE: u32 = 5;
}

/// The engine's functions, every one taking [`RunCtx::host`] first and
/// answering a [`status`]. A refusal's message is the engine's: the system
/// that returns its error is stopped with it.
#[repr(C)]
pub struct HostApi {
    /// [`ABI_VERSION`].
    pub abi: u32,
    /// A component's id by name.
    pub component: unsafe extern "C" fn(host: *mut c_void, name: Str, out: *mut u32) -> u32,
    /// Visit a query's chunks, its columns borrowed for the visit.
    pub each: unsafe extern "C" fn(
        host: *mut c_void,
        query: *const Query,
        f: ChunkFn,
        user: *mut c_void,
    ) -> u32,
    /// Copy one entity's row out.
    pub read: unsafe extern "C" fn(
        host: *mut c_void,
        entity: u64,
        col: *const Column,
        out: *mut u8,
    ) -> u32,
    /// Copy one entity's row in.
    pub write: unsafe extern "C" fn(
        host: *mut c_void,
        entity: u64,
        col: *const Column,
        src: *const u8,
    ) -> u32,
    /// 1 if the entity has the component.
    pub has: unsafe extern "C" fn(host: *mut c_void, entity: u64, component: u32) -> u32,
    /// 1 if the entity is alive.
    pub alive: unsafe extern "C" fn(host: *mut c_void, entity: u64) -> u32,
    /// Resolve `@id` or `e:n`.
    pub entity: unsafe extern "C" fn(host: *mut c_void, r: Str, out: *mut u64) -> u32,
    /// Emit a gameplay event, when the system's run ends.
    pub emit:
        unsafe extern "C" fn(host: *mut c_void, name: Str, fields: *const Field, n: usize) -> u32,
    /// Spawn an entity with components and `Component.field` values, when
    /// the system's run ends.
    pub spawn: unsafe extern "C" fn(
        host: *mut c_void,
        components: *const u32,
        n: usize,
        fields: *const Field,
        nf: usize,
    ) -> u32,
    /// Despawn an entity, when the system's run ends.
    pub despawn: unsafe extern "C" fn(host: *mut c_void, entity: u64) -> u32,
    /// Draw from the system's stream ([`rng_op`]).
    pub rng: unsafe extern "C" fn(host: *mut c_void, op: u32, a: f64, b: f64, out: *mut f64) -> u32,
    /// Live entities within `r` of a point, nearest first.
    pub within: unsafe extern "C" fn(
        host: *mut c_void,
        x: f32,
        y: f32,
        r: f32,
        f: NearFn,
        user: *mut c_void,
    ) -> u32,
    /// A string by id; the text lives as long as the world's table.
    pub string: unsafe extern "C" fn(host: *mut c_void, id: u32, out: *mut Str) -> u32,
    /// A string's id, added when new (refused in a parallel batch).
    pub intern: unsafe extern "C" fn(host: *mut c_void, s: Str, out: *mut u32) -> u32,
    /// The plugin's own error message for this run.
    pub fail: unsafe extern "C" fn(host: *mut c_void, message: Str),
}
