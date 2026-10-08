//! A plugin's declaration: its systems ([`System`]), declared as a Luau
//! system is, and the [`crate::plugin!`] macro that exports them.

use crate::abi::{self, Str, Strs};
use crate::ctx::{Ctx, Result};
use crate::view::View;
use core::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// A system's body.
pub type SystemFn = fn(&Ctx<'_>) -> Result;

/// One system, declared as a Luau system is: `system "sim.chase" { stage =
/// "sim", after = {...}, reads = {...}, writes = {...} }` is
/// `System::new("sim.chase", "sim").after("...").reads(&[...]).writes(&[...])`.
#[derive(Clone, Debug)]
pub struct System {
    name: &'static str,
    stage: &'static str,
    doc: &'static str,
    source: String,
    after: Vec<&'static str>,
    reads: Vec<&'static str>,
    writes: Vec<&'static str>,
    spawns: Vec<&'static str>,
    rng: &'static str,
    flags: u32,
    clock: u32,
    run: Option<SystemFn>,
}

impl System {
    /// A system in a stage (`input`, `sim`, `late`, `hierarchy`, `present`).
    #[track_caller]
    pub fn new(name: &'static str, stage: &'static str) -> System {
        let at = std::panic::Location::caller();
        System {
            name,
            stage,
            doc: "",
            source: format!("{}:{}", at.file().replace('\\', "/"), at.line()),
            after: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            spawns: Vec::new(),
            rng: "",
            flags: 0,
            clock: abi::clock::DRIVING,
            run: None,
        }
    }
    /// Run after another system of the stage.
    pub fn after(mut self, system: &'static str) -> Self {
        self.after.push(system);
        self
    }
    /// Components read.
    pub fn reads(mut self, components: &[&'static str]) -> Self {
        self.reads.extend_from_slice(components);
        self
    }
    /// Components written.
    pub fn writes(mut self, components: &[&'static str]) -> Self {
        self.writes.extend_from_slice(components);
        self
    }
    /// Components spawned with, inserted, or removed.
    pub fn spawns(mut self, components: &[&'static str]) -> Self {
        self.spawns.extend_from_slice(components);
        self
    }
    /// What it does.
    pub fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }
    /// Its random stream.
    pub fn rng(mut self, stream: &'static str) -> Self {
        self.rng = stream;
        self
    }
    /// May run at once with the parallel systems beside it that it does
    /// not conflict with, on other threads.
    pub fn parallel(mut self) -> Self {
        self.flags |= abi::flags::PARALLEL;
        self
    }
    /// Also run once when a scene loads.
    pub fn on_load(mut self) -> Self {
        self.flags |= abi::flags::ON_LOAD;
        self
    }
    /// Run first in the stage.
    pub fn first(mut self) -> Self {
        self.flags |= abi::flags::FIRST;
        self
    }
    /// Run last in the stage.
    pub fn last(mut self) -> Self {
        self.flags |= abi::flags::LAST;
        self
    }
    /// Run while the sim is frozen.
    pub fn unfrozen(mut self) -> Self {
        self.flags |= abi::flags::UNFROZEN;
        self
    }
    /// Run on the continuous clock's ticks, whatever drives the scene.
    pub fn continuous(mut self) -> Self {
        self.clock = abi::clock::CONTINUOUS;
        self
    }
    /// Run on the discrete clock's ticks, whatever drives the scene.
    pub fn discrete(mut self) -> Self {
        self.clock = abi::clock::DISCRETE;
        self
    }
    /// Its body.
    pub fn run(mut self, f: SystemFn) -> Self {
        self.run = Some(f);
        self
    }
}

/// A component layout to check at load: what [`crate::plugin!`] lists.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    name: &'static str,
    size: usize,
    align: usize,
    fields: &'static [(&'static str, u32, usize)],
    source: &'static str,
}

impl Layout {
    /// The layout of `T`.
    pub fn of<T: View>() -> Layout {
        Layout {
            name: T::NAME,
            size: core::mem::size_of::<T>(),
            align: core::mem::align_of::<T>(),
            fields: T::FIELDS,
            source: T::SOURCE,
        }
    }
}

/// The run function every system goes through: it calls the system's body
/// and catches a panic, which may not unwind into the engine.
unsafe extern "C" fn trampoline(raw: *mut abi::RunCtx, user: *const c_void) -> u32 {
    // SAFETY: `user` is the SystemFn the declaration stored.
    let f: SystemFn = unsafe { core::mem::transmute::<*const c_void, SystemFn>(user) };
    // SAFETY: the engine's run context, for this call.
    let ctx = unsafe { Ctx::from_raw(raw) };
    match catch_unwind(AssertUnwindSafe(|| f(&ctx))) {
        Ok(Ok(())) => abi::status::OK,
        Ok(Err(e)) => {
            if let Some(m) = &e.message {
                ctx.fail(m);
            }
            abi::status::ERR
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic with non-string payload".to_string());
            ctx.fail(&format!("panic: {msg}"));
            abi::status::PANIC
        }
    }
}

/// A declaration built once and kept for the library's life.
pub struct Built {
    _strings: Vec<String>,
    _lists: Vec<Vec<Str>>,
    _fields: Vec<Vec<abi::FieldLayout>>,
    _systems: Vec<abi::SystemDecl>,
    _layouts: Vec<abi::ComponentLayout>,
    decl: Box<abi::PluginDecl>,
}

// SAFETY: built once, never changed; the pointers are into its own heap.
unsafe impl Sync for Built {}
// SAFETY: as above.
unsafe impl Send for Built {}

impl Built {
    /// Build the declaration. Pointers taken here point into heap buffers
    /// that moving `Built` does not move.
    pub fn new(name: &'static str, layouts: Vec<Layout>, systems: Vec<System>) -> Built {
        let mut strings: Vec<String> = Vec::new();
        let mut lists: Vec<Vec<Str>> = Vec::new();
        let mut list = |items: &[&'static str]| -> Strs {
            let v: Vec<Str> = items.iter().map(|s| Str::new(s)).collect();
            let s = Strs {
                ptr: v.as_ptr(),
                len: v.len(),
            };
            lists.push(v);
            s
        };
        let mut sys = Vec::with_capacity(systems.len());
        for s in &systems {
            let after = list(&s.after);
            let reads = list(&s.reads);
            let writes = list(&s.writes);
            let spawns = list(&s.spawns);
            strings.push(s.source.clone());
            let source = Str::new(strings.last().expect("pushed"));
            let run = s.run.unwrap_or(|_| {
                Err(crate::ctx::Error::new(
                    "this system has no body: give it .run(f)",
                ))
            });
            sys.push(abi::SystemDecl {
                name: Str::new(s.name),
                stage: Str::new(s.stage),
                doc: Str::new(s.doc),
                source,
                after,
                reads,
                writes,
                spawns,
                rng: Str::new(s.rng),
                flags: s.flags,
                clock: s.clock,
                run: trampoline,
                user: run as *const c_void,
            });
        }
        let mut fields = Vec::with_capacity(layouts.len());
        let mut lays = Vec::with_capacity(layouts.len());
        for l in &layouts {
            let f: Vec<abi::FieldLayout> = l
                .fields
                .iter()
                .map(|(n, t, o)| abi::FieldLayout {
                    name: Str::new(n),
                    ty: *t,
                    offset: *o,
                })
                .collect();
            lays.push(abi::ComponentLayout {
                name: Str::new(l.name),
                size: l.size,
                align: l.align,
                fields: f.as_ptr(),
                fields_len: f.len(),
                source: Str::new(l.source),
            });
            fields.push(f);
        }
        let decl = Box::new(abi::PluginDecl {
            abi: abi::ABI_VERSION,
            name: Str::new(name),
            systems: sys.as_ptr(),
            systems_len: sys.len(),
            components: lays.as_ptr(),
            components_len: lays.len(),
        });
        Built {
            _strings: strings,
            _lists: lists,
            _fields: fields,
            _systems: sys,
            _layouts: lays,
            decl,
        }
    }

    /// The declaration.
    pub fn decl(&self) -> *const abi::PluginDecl {
        &*self.decl
    }
}

/// Export a plugin: the component layouts its systems read columns as,
/// and its systems. Written once, at the crate's root.
///
/// ```text
/// plugin! {
///     components: [Enemy, Player, Transform],
///     systems: [
///         System::new("sim.chase", "sim").after("sim.waves")
///             .reads(&["Enemy", "Transform", "Player"]).writes(&["Transform"])
///             .run(chase),
///     ],
/// }
/// ```
#[macro_export]
macro_rules! plugin {
    (components: [$($c:ty),* $(,)?], systems: [$($s:expr),* $(,)?] $(,)?) => {
        /// The plugin's entry, which the engine calls when it loads it.
        ///
        /// # Safety
        /// Called by the engine only.
        #[no_mangle]
        pub unsafe extern "C" fn argus_plugin_v1() -> *const $crate::abi::PluginDecl {
            static DECL: ::std::sync::OnceLock<$crate::decl::Built> = ::std::sync::OnceLock::new();
            DECL.get_or_init(|| {
                $crate::decl::Built::new(
                    env!("CARGO_PKG_NAME"),
                    vec![$($crate::decl::Layout::of::<$c>()),*],
                    vec![$($s),*],
                )
            })
            .decl()
        }
    };
}
