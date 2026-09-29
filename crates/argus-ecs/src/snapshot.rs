//! Snapshots (§2, §6): a copy of every SoA column plus the entity table,
//! string table, and event queue. Because columns are contiguous the copy is
//! a set of `memcpy`s; the §17 target is under 5 ms at 100k entities.
//! Entering play takes a snapshot and stopping restores it; `fork` gives a
//! second world for A/B comparison inside the same daemon.
//!
//! RNG stream states and the Luau heap are added to the snapshot by the
//! scheduler and scripting pieces; this crate's snapshot is the world's data.

use crate::world::World;

/// A frozen copy of a world.
#[derive(Clone, Debug)]
pub struct Snapshot {
    world: World,
}

impl Snapshot {
    /// A snapshot that is this world (read back from disk, say).
    pub fn of(world: World) -> Snapshot {
        Snapshot { world }
    }
    /// Frame counter at capture.
    pub fn frame(&self) -> u64 {
        self.world.frame()
    }
    /// Live entities at capture.
    pub fn entity_count(&self) -> usize {
        self.world.entity_count()
    }
    /// Approximate bytes held.
    pub fn bytes(&self) -> usize {
        self.world.bytes()
    }
    /// A new, independent world starting from this snapshot.
    pub fn fork(&self) -> World {
        self.world.clone()
    }
    /// Consume into a world without copying.
    pub fn into_world(self) -> World {
        self.world
    }
    /// Read-only view of the captured world.
    pub fn world(&self) -> &World {
        &self.world
    }
}

impl World {
    /// Capture the world.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            world: self.clone_state(),
        }
    }
    /// Replace this world's state with the snapshot's.
    pub fn restore(&mut self, snapshot: &Snapshot) {
        *self = snapshot.world.clone();
        self.structure = crate::world::next_structure();
    }
    /// A second world with identical state that evolves independently.
    pub fn fork(&self) -> World {
        let mut w = self.clone();
        w.structure = crate::world::next_structure();
        w
    }
}
