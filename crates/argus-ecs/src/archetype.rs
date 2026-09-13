//! An archetype is the set of entities that have exactly the same set of
//! components, stored as one column per component plus an entity list (§3).
//! Adding or removing a component moves an entity to another archetype.

use crate::column::Column;
use crate::entity::Entity;
use crate::schema::{ComponentId, Registry};
use std::collections::HashMap;

/// Index of an archetype within a world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArchetypeId(pub u32);

/// Storage for one component set.
#[derive(Clone, Debug)]
pub struct Archetype {
    /// This archetype's id.
    pub id: ArchetypeId,
    /// Sorted component ids.
    pub components: Vec<ComponentId>,
    columns: Vec<Column>,
    entities: Vec<Entity>,
    index: HashMap<ComponentId, usize>,
}

impl Archetype {
    pub(crate) fn new(
        id: ArchetypeId,
        components: Vec<ComponentId>,
        registry: &Registry,
    ) -> Archetype {
        debug_assert!(components.windows(2).all(|w| w[0] < w[1]));
        let columns = components
            .iter()
            .map(|&c| {
                let s = registry.schema(c);
                Column::new(s.size, s.align)
            })
            .collect();
        let index = components
            .iter()
            .enumerate()
            .map(|(i, &c)| (c, i))
            .collect();
        Archetype {
            id,
            components,
            columns,
            entities: Vec::new(),
            index,
        }
    }

    /// Number of entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }
    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    /// Entities in row order.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }
    /// True if the archetype has the component.
    pub fn has(&self, c: ComponentId) -> bool {
        self.index.contains_key(&c)
    }
    /// Column for a component, if present.
    pub fn column(&self, c: ComponentId) -> Option<&Column> {
        self.index.get(&c).map(|&i| &self.columns[i])
    }
    /// Push a row. `rows[i]` supplies the bytes for `components[i]`.
    ///
    /// # Safety
    /// Each pointer must be valid for reads of that column's stride.
    pub(crate) unsafe fn push_row(&mut self, entity: Entity, rows: &[*const u8]) -> usize {
        debug_assert_eq!(rows.len(), self.columns.len());
        for (col, &src) in self.columns.iter_mut().zip(rows) {
            col.push_from(src);
        }
        self.entities.push(entity);
        self.entities.len() - 1
    }

    /// Remove a row by swap-remove. Returns the entity that moved into `row`,
    /// if any, so the caller can fix its location.
    pub(crate) fn swap_remove(&mut self, row: usize) -> Option<Entity> {
        for col in &mut self.columns {
            col.swap_remove(row);
        }
        self.entities.swap_remove(row);
        if row < self.entities.len() {
            Some(self.entities[row])
        } else {
            None
        }
    }

    /// Bytes held by all columns (capacity, not just live rows).
    pub fn bytes(&self) -> usize {
        self.columns
            .iter()
            .map(|c| c.capacity_bytes())
            .sum::<usize>()
            + self.entities.capacity() * 8
    }
}

/// Entity count and component names for one archetype — the rows of
/// `world.summary` and `prof.archetypes` (§5, §8 L0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchetypeStat {
    /// Archetype id.
    pub id: ArchetypeId,
    /// Component names, sorted by id.
    pub components: Vec<String>,
    /// Live entities.
    pub entities: usize,
    /// Approximate bytes allocated.
    pub bytes: usize,
}
