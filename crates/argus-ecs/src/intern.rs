//! String interner. String and ref fields hold a 4-byte [`StrId`] into this
//! table so every column stays plain old data. The table is append-only and
//! deduplicated; it is cloned with the world on snapshot so a restored world
//! resolves the same ids.
//!
//! Known cost: a game that generates unbounded distinct strings at runtime
//! grows this table for the life of the world. Phase 0's reference games do
//! not; if Outpost does, the fix is a per-column arena with slot reuse.

use std::cell::RefCell;
use std::collections::HashMap;

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
    strings: Vec<String>,
    map: HashMap<String, StrId>,
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
        let mut map = HashMap::new();
        map.insert(String::new(), StrId::EMPTY);
        Interner {
            inner: RefCell::new(Inner {
                strings: vec![String::new()],
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
        inner.strings.push(s.to_string());
        inner.map.insert(s.to_string(), id);
        id
    }

    /// Every string, in id order.
    pub fn all(&self) -> Vec<String> {
        self.inner.borrow().strings.clone()
    }

    /// Resolve an id. Unknown ids resolve to the empty string rather than
    /// panicking, because a corrupt id must not take the daemon down (§2).
    pub fn get(&self, id: StrId) -> std::cell::Ref<'_, str> {
        std::cell::Ref::map(self.inner.borrow(), |i| {
            i.strings
                .get(id.0 as usize)
                .map(String::as_str)
                .unwrap_or("")
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

    /// Approximate bytes held, for snapshot size reporting.
    pub fn bytes(&self) -> usize {
        self.inner
            .borrow()
            .strings
            .iter()
            .map(|s| s.len() + 4)
            .sum()
    }
}
