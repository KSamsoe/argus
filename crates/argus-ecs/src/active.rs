//! Inactive entities (issue 18): an entity switched off stays in the world
//! (saved, in the digest, kept through a seek) but out of the game: a query
//! skips it unless it asks for inactive ones, so systems, scripts, drawing,
//! physics, the UI, and the semantic frame pass it by. Its children are
//! inactive with it. A game parks what it is not using (a pooled bullet, a
//! room the player is not in, a menu not open) and switches it on again.
//!
//! Two tags say it, so a query decides by archetype: [`Inactive`] on an
//! entity switched off itself, and [`InactiveUnder`] on one under an
//! inactive ancestor, which the engine keeps ([`World::settle_inactive`]).

use crate::entity::Entity;
use crate::error::{ArgusError, Result};
use crate::hierarchy::Parent;
use crate::world::World;
use crate::Component;
use std::collections::HashMap;

/// An entity switched off: out of every query that does not ask for it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Switched off: the entity stays in the world (saved, seekable) but queries, systems, drawing, physics, the UI, and the semantic frame skip it, and its children with it. A script switches it with e:set_active(false), and finds inactive entities with q:inactive."
)]
pub struct Inactive;

/// Under an inactive ancestor: kept by the engine, not written by a game.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Under an Inactive ancestor, so skipped as it is. The engine keeps it: set Inactive on the ancestor instead."
)]
pub struct InactiveUnder;

/// Register [`Inactive`] and [`InactiveUnder`].
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Inactive>()?;
    world.register::<InactiveUnder>()?;
    Ok(())
}

impl World {
    /// Switch an entity on or off (and, with it, everything under it).
    pub fn set_active(&mut self, e: Entity, active: bool) -> Result<()> {
        if !self.is_alive(e) {
            return Err(ArgusError::new(
                crate::codes::ENTITY_NOT_ALIVE,
                format!("{e} is not alive"),
            ));
        }
        register(self)?;
        let off = self.component_id_of::<Inactive>().expect("registered");
        if active {
            self.remove(e, off)?;
        } else {
            self.insert_default(e, off)?;
        }
        self.settle_inactive()
    }

    /// Whether an entity is in the game: alive, not switched off, and not
    /// under an entity that is.
    pub fn is_active(&self, e: Entity) -> bool {
        // Its archetype says, with no lookup by component: q:each asks
        // this of every entity it hands out.
        self.entities
            .slot(e)
            .is_some_and(|s| !self.archetypes[s.archetype as usize].is_inactive())
    }

    /// Whether the entity itself is switched off (not just under one that
    /// is).
    pub fn is_switched_off(&self, e: Entity) -> bool {
        self.registry
            .inactive_tags()
            .0
            .is_some_and(|c| self.has(e, c))
    }

    /// Put [`InactiveUnder`] on every entity under an inactive one, and take
    /// it off every other: after a switch, a reparenting, or a spawn under
    /// an inactive parent. Nothing to do, and no cost but two counts, in a
    /// world with no inactive entity.
    pub fn settle_inactive(&mut self) -> Result<()> {
        let (Some(off), under) = self.registry.inactive_tags() else {
            return Ok(());
        };
        // Every tick at the hierarchy stage: one pass over the archetypes'
        // flags when nothing is switched off.
        let any = self
            .archetypes
            .iter()
            .any(|a| a.is_inactive() && !a.is_empty());
        if !any {
            return Ok(());
        }
        let Some(parent_c) = self.component_id_of::<Parent>() else {
            return Ok(());
        };
        let mut parent_of: HashMap<Entity, Entity> = HashMap::new();
        self.query()
            .including_inactive()
            .has(parent_c)
            .each::<&Parent>(|e, p| {
                parent_of.insert(e, p.entity);
            })?;
        // Under an inactive ancestor: walk up, memoised, a cycle ending it.
        let mut memo: HashMap<Entity, bool> = HashMap::with_capacity(parent_of.len());
        let mut want: Vec<(Entity, bool)> = Vec::with_capacity(parent_of.len());
        for &e in parent_of.keys() {
            let mut chain = Vec::new();
            let mut cur = e;
            let result = loop {
                if let Some(&m) = memo.get(&cur) {
                    break m;
                }
                let Some(&p) = parent_of.get(&cur) else {
                    break false;
                };
                if p.is_null() || !self.is_alive(p) || chain.contains(&cur) {
                    break false;
                }
                chain.push(cur);
                if self.has(p, off) {
                    break true;
                }
                cur = p;
            };
            for c in chain {
                memo.insert(c, result);
            }
            want.push((e, *memo.get(&e).unwrap_or(&result)));
        }
        want.sort_by_key(|(e, _)| *e);
        let has_under: Vec<Entity> = match under {
            Some(u) => self.query().including_inactive().has(u).entities(),
            None => Vec::new(),
        };
        let mut changes: Vec<(Entity, bool)> = Vec::new();
        for (e, w) in &want {
            let now = under.is_some_and(|u| self.has(*e, u));
            if now != *w {
                changes.push((*e, *w));
            }
        }
        // Under one no longer: an entity that lost its Parent.
        for e in has_under {
            if !parent_of.contains_key(&e) {
                changes.push((e, false));
            }
        }
        if changes.is_empty() {
            return Ok(());
        }
        register(self)?;
        let u = self.component_id_of::<InactiveUnder>().expect("registered");
        for (e, w) in changes {
            if w {
                self.insert_default(e, u)?;
            } else {
                self.remove(e, u)?;
            }
        }
        Ok(())
    }
}
