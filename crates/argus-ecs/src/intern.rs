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

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

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
}

/// Append-only deduplicating string table with interior mutability, so that
/// dynamic writes through a shared world view can intern new strings.
#[derive(Clone, Debug)]
pub struct Interner {
    inner: RefCell<Inner>,
}

impl Default for Interner {
    fn default() -> Self {
        Interner::new()
    }
}

impl Interner {
    /// New table containing only the empty string.
    pub fn new() -> Interner {
        let empty: Arc<str> = Arc::from("");
        let mut map = HashMap::new();
        map.insert(empty.clone(), StrId::EMPTY);
        Interner {
            inner: RefCell::new(Inner {
                strings: vec![empty],
                map,
            }),
        }
    }

    /// Return the id for `s`, adding it if new.
    pub fn intern(&self, s: &str) -> StrId {
        if let Some(id) = self.inner.borrow().map.get(s) {
            return *id;
        }
        let mut inner = self.inner.borrow_mut();
        let id = StrId(inner.strings.len() as u32);
        let s: Arc<str> = Arc::from(s);
        inner.strings.push(s.clone());
        inner.map.insert(s, id);
        id
    }

    /// Every string, in id order.
    pub fn all(&self) -> Vec<String> {
        self.inner
            .borrow()
            .strings
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Resolve an id. Unknown ids resolve to the empty string rather than
    /// panicking, because a corrupt id must not take the daemon down (§2).
    pub fn get(&self, id: StrId) -> std::cell::Ref<'_, str> {
        std::cell::Ref::map(self.inner.borrow(), |i| {
            i.strings.get(id.0 as usize).map(|s| &**s).unwrap_or("")
        })
    }

    /// Resolve to an owned string.
    pub fn resolve(&self, id: StrId) -> String {
        self.get(id).to_string()
    }

    /// Number of distinct strings, including the empty one.
    pub fn len(&self) -> usize {
        self.inner.borrow().strings.len()
    }

    /// Always false: the empty string is always present.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Approximate bytes a copy of the table holds, for snapshot size
    /// reporting: its two tables of shared strings (the words themselves
    /// are shared between copies, not held by each).
    pub fn bytes(&self) -> usize {
        self.inner.borrow().strings.len() * 48
    }
}
