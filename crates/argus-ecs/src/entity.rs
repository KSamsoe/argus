//! Entity ids (§3): a 64-bit value made of a 32-bit index and a 32-bit
//! generation. Slots are recycled; the generation makes a stale handle fail
//! `is_alive` instead of aliasing whatever now lives in the slot.

use std::fmt;
use std::str::FromStr;

/// A handle to an entity. `Copy`, 8 bytes, and stored directly in component
/// fields when a component holds a reference (§3 "Relations").
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Entity(u64);

impl Entity {
    /// The null reference. Dangling references are set to this at frame end.
    pub const NULL: Entity = Entity(u64::MAX);

    /// Build from index and generation.
    pub const fn from_parts(index: u32, generation: u32) -> Entity {
        Entity(((generation as u64) << 32) | index as u64)
    }
    /// Slot index.
    pub const fn index(self) -> u32 {
        self.0 as u32
    }
    /// Slot generation at the time this handle was issued.
    pub const fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
    /// Raw 64-bit form.
    pub const fn to_bits(self) -> u64 {
        self.0
    }
    /// From raw 64-bit form.
    pub const fn from_bits(bits: u64) -> Entity {
        Entity(bits)
    }
    /// True for [`Entity::NULL`].
    pub const fn is_null(self) -> bool {
        self.0 == u64::MAX
    }
}

impl Default for Entity {
    fn default() -> Self {
        Entity::NULL
    }
}

/// Display uses the protocol form from §5: `e:4011`. When the slot has been
/// recycled the generation is appended (`e:4011.2`) so the text still names
/// exactly one entity. `e:null` is the null reference.
impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            write!(f, "e:null")
        } else if self.generation() == 0 {
            write!(f, "e:{}", self.index())
        } else {
            write!(f, "e:{}.{}", self.index(), self.generation())
        }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Entity {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let body = s
            .strip_prefix("e:")
            .ok_or_else(|| format!("entity refs look like e:4011, got {s:?}"))?;
        if body == "null" {
            return Ok(Entity::NULL);
        }
        let (idx, gen) = match body.split_once('.') {
            Some((i, g)) => (i, g),
            None => (body, "0"),
        };
        let index: u32 = idx
            .parse()
            .map_err(|_| format!("bad entity index in {s:?}"))?;
        let generation: u32 = gen
            .parse()
            .map_err(|_| format!("bad entity generation in {s:?}"))?;
        Ok(Entity::from_parts(index, generation))
    }
}

/// Per-slot bookkeeping. `archetype`/`row` locate the entity's storage.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub generation: u32,
    pub alive: bool,
    pub authored: bool,
    pub archetype: u32,
    pub row: u32,
}

/// Slot allocator with a free list and generation counters.
#[derive(Clone, Debug, Default)]
pub(crate) struct Entities {
    slots: Vec<Slot>,
    free: Vec<u32>,
    alive: usize,
}

impl Entities {
    pub fn alloc(&mut self, authored: bool) -> Entity {
        self.alive += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.alive = true;
            slot.authored = authored;
            slot.archetype = 0;
            slot.row = 0;
            return Entity::from_parts(index, slot.generation);
        }
        let index = self.slots.len() as u32;
        self.slots.push(Slot {
            generation: 0,
            alive: true,
            authored,
            archetype: 0,
            row: 0,
        });
        Entity::from_parts(index, 0)
    }

    /// Frees the slot. Returns false if the handle was not alive.
    pub fn free(&mut self, e: Entity) -> bool {
        match self.slots.get_mut(e.index() as usize) {
            Some(slot) if slot.alive && slot.generation == e.generation() => {
                slot.alive = false;
                slot.generation = slot.generation.wrapping_add(1);
                self.free.push(e.index());
                self.alive -= 1;
                true
            }
            _ => false,
        }
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        self.slot(e).is_some()
    }

    pub fn slot(&self, e: Entity) -> Option<&Slot> {
        self.slots
            .get(e.index() as usize)
            .filter(|s| s.alive && s.generation == e.generation())
    }

    pub fn slot_mut(&mut self, e: Entity) -> Option<&mut Slot> {
        self.slots
            .get_mut(e.index() as usize)
            .filter(|s| s.alive && s.generation == e.generation())
    }

    pub fn alive_count(&self) -> usize {
        self.alive
    }
}
