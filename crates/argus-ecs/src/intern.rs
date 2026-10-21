//! String interner. String and ref fields hold a 4-byte [`StrId`] into this
//! table so every column stays plain old data. The table is append-only and
//! deduplicated; it is cloned with the world on snapshot so a restored world
//! resolves the same ids.
//!
//! Known cost: a game that generates unbounded distinct strings at runtime
//! grows this table for the life of the world. Phase 0's reference games do
//! not; Outpost does (its HUD line, a new one twice a second), about 3,000
//! strings an hour of play. The strings are shared, not copied, when the
//! table is cloned (phase 3 gate): each keyframe cloned the whole table,
//! and at eight hours a ring of them was millions of small allocations to
//! make and free. A per-column arena with slot reuse is the fix for the
//! growth itself.
//!
//! The table is behind a lock so a world can be shared by a parallel batch
//! of systems (phase 5 item 5). While a batch runs the table is sealed: a
//! string it already holds resolves as always, and a new one is refused,
//! because the id a new string gets would depend on which thread asked
//! first, and ids are written into rows that a run's digest covers.

use crate::error::{codes, ArgusError, Result};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Handle into the interner. Id 0 is always the empty string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct StrId(pub u32);

impl StrId {
    /// The empty string.
    pub const EMPTY: StrId = StrId(0);
}

#[derive(Clone, Debug)]
struct Inner {
    strings: Vec<Arc<str>>,
    map: HashMap<Arc<str>, StrId>,
    /// Each string's [`text_hash`], by id: what the world's column digest
    /// reads for a string field, since two machines may number one string
    /// differently.
    hashes: Vec<u64>,
}

/// A string's own hash (FNV-1a, 64-bit), the same for its text in any
/// table.
pub fn text_hash(s: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Append-only deduplicating string table with interior mutability, so that
/// dynamic writes through a shared world view can intern new strings.
pub struct Interner {
    inner: RwLock<Inner>,
    /// Nonzero while a parallel batch runs ([`Interner::seal`]).
    sealed: AtomicUsize,
}

impl Clone for Interner {
    fn clone(&self) -> Interner {
        Interner {
            inner: RwLock::new(self.read().clone()),
            sealed: AtomicUsize::new(0),
        }
    }
}

impl std::fmt::Debug for Interner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interner")
            .field("len", &self.len())
            .finish()
    }
}

impl Default for Interner {
    fn default() -> Self {
        Interner::new()
    }
}

/// A resolved string, borrowed from the table.
pub struct StrRef<'a> {
    guard: RwLockReadGuard<'a, Inner>,
    id: usize,
}

impl Deref for StrRef<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        self.guard.strings.get(self.id).map(|s| &**s).unwrap_or("")
    }
}

impl std::fmt::Display for StrRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self)
    }
}

impl std::fmt::Debug for StrRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

impl Interner {
    /// New table containing only the empty string.
    pub fn new() -> Interner {
        let empty: Arc<str> = Arc::from("");
        let mut map = HashMap::new();
        map.insert(empty.clone(), StrId::EMPTY);
        Interner {
            inner: RwLock::new(Inner {
                strings: vec![empty],
                map,
                hashes: vec![text_hash("")],
            }),
            sealed: AtomicUsize::new(0),
        }
    }

    // A panic while the table is held cannot leave it half written (a push
    // and an insert of one string), so a poisoned lock is still a good table.
    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Return the id for `s`, adding it if new.
    ///
    /// # Panics
    /// If `s` is new while the table is sealed (a parallel system that
    /// interns a string the world has never held); use
    /// [`Interner::try_intern`] for an error instead.
    pub fn intern(&self, s: &str) -> StrId {
        match self.try_intern(s) {
            Ok(id) => id,
            Err(e) => panic!("{e}"),
        }
    }

    /// [`Interner::intern`], refusing a new string while the table is sealed.
    pub fn try_intern(&self, s: &str) -> Result<StrId> {
        if let Some(id) = self.lookup(s) {
            return Ok(id);
        }
        if self.is_sealed() {
            return Err(ArgusError::new(
                codes::SEALED_STRINGS,
                format!("a parallel system wrote the new string {s:?}"),
            )
            .hint("A parallel system may write only strings the world already holds; write new ones from a serial system, or spawn with them (a spawn's fields are applied after the batch)."));
        }
        let mut inner = self.write();
        // Another thread may have added it between the two locks.
        if let Some(id) = inner.map.get(s) {
            return Ok(*id);
        }
        let id = StrId(inner.strings.len() as u32);
        let s: Arc<str> = Arc::from(s);
        inner.hashes.push(text_hash(&s));
        inner.strings.push(s.clone());
        inner.map.insert(s, id);
        Ok(id)
    }

    /// The id of `s` if the table holds it; never adds.
    pub fn lookup(&self, s: &str) -> Option<StrId> {
        self.read().map.get(s).copied()
    }

    /// Refuse new strings until the matching [`Interner::unseal`]: the
    /// scheduler seals the table around a parallel batch, run on one thread
    /// or many, so a batch behaves the same either way.
    pub fn seal(&self) {
        self.sealed.fetch_add(1, Ordering::AcqRel);
    }

    /// Undo one [`Interner::seal`].
    pub fn unseal(&self) {
        self.sealed.fetch_sub(1, Ordering::AcqRel);
    }

    /// Is the table refusing new strings?
    pub fn is_sealed(&self) -> bool {
        self.sealed.load(Ordering::Acquire) > 0
    }

    /// Every string, in id order.
    pub fn all(&self) -> Vec<String> {
        self.read().strings.iter().map(|s| s.to_string()).collect()
    }

    /// Resolve an id. Unknown ids resolve to the empty string rather than
    /// panicking, because a corrupt id must not take the daemon down (§2).
    pub fn get(&self, id: StrId) -> StrRef<'_> {
        StrRef {
            guard: self.read(),
            id: id.0 as usize,
        }
    }

    /// Every string's [`text_hash`] by id, for `f` to read under one lock.
    pub fn with_hashes<R>(&self, f: impl FnOnce(&[u64]) -> R) -> R {
        f(&self.read().hashes)
    }

    /// Resolve to an owned string.
    pub fn resolve(&self, id: StrId) -> String {
        self.get(id).to_string()
    }

    /// Number of distinct strings, including the empty one.
    pub fn len(&self) -> usize {
        self.read().strings.len()
    }

    /// Always false: the empty string is always present.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Approximate bytes a copy of the table holds, for snapshot size
    /// reporting: its two tables of shared strings (the words themselves
    /// are shared between copies, not held by each).
    pub fn bytes(&self) -> usize {
        self.read().strings.len() * 56
    }
}
