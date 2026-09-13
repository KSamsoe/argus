//! Structural events the world emits (§3 "emitting an event", §8 L2). The
//! daemon's `obs.events` feed starts from this queue; the queue is part of a
//! snapshot (§6) so a restored world carries its pending events.

use crate::entity::Entity;
use crate::schema::ComponentId;

/// What happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// An entity was created.
    Spawned {
        /// The entity.
        entity: Entity,
        /// Its stable string id, if authored.
        id: Option<String>,
    },
    /// An entity was destroyed.
    Despawned {
        /// The entity (handle is now dead).
        entity: Entity,
        /// Its string id, if it was authored.
        id: Option<String>,
    },
    /// A component was added to an entity.
    ComponentAdded {
        /// Entity.
        entity: Entity,
        /// Component.
        component: ComponentId,
    },
    /// A component was removed from an entity.
    ComponentRemoved {
        /// Entity.
        entity: Entity,
        /// Component.
        component: ComponentId,
    },
    /// An entity-reference field pointed at a dead entity and was set to
    /// [`Entity::NULL`] during end-of-frame maintenance (§3 "Relations").
    DanglingRefNulled {
        /// Entity holding the reference.
        entity: Entity,
        /// Component holding the field.
        component: ComponentId,
        /// Field name.
        field: String,
        /// The dead entity that was referenced.
        target: Entity,
    },
    /// A `Parent` chain loops back on itself. The entity is treated as a root
    /// for that frame and the loop is reported instead of hanging (§2).
    HierarchyCycle {
        /// An entity on the cycle.
        entity: Entity,
    },
    /// A component schema was registered.
    ComponentRegistered {
        /// The new component.
        component: ComponentId,
        /// Its name.
        name: String,
    },
}

/// An event tagged with the frame it happened on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// Frame counter of the world when the event was emitted.
    pub frame: u64,
    /// The event.
    pub event: Event,
}
