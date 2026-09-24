//! A world written to bytes and read back (phase 2 item 2): the world part
//! of a keyframe on disk, which is what lets a mark outlive the daemon.
//!
//! What is written is the world exactly: every archetype in order with its
//! rows in order and its columns as bytes, every entity slot with its
//! generation and the free list in order, the string table, and the
//! authored ids. Iteration order is part of determinism, so a read-back
//! world iterates as the saved one did.
//!
//! It reads into a world that already has its components registered (a
//! scene just loaded, by the same code or a newer one). Components are
//! matched by name. One whose fields are laid out as they were is copied as
//! bytes; one whose fields changed (a script added or dropped one) is copied
//! field by field, new fields taking their defaults; one that no longer
//! exists is dropped. Strings are interned into the reading world's table
//! and every string field is translated, because that world's table was
//! built by its own load and may number them differently. The events queue
//! is not saved: a keyframe's events are already in the log.

use crate::codec::{Reader, Writer};
use crate::entity::{Entity, Slot};
use crate::error::Result;
use crate::intern::StrId;
use crate::schema::{ComponentId, ComponentSchema};
use crate::value::{FieldType, Value};
use crate::world::World;
use std::collections::HashMap;

const TAG: &[u8; 4] = b"ARGW";
const VERSION: u32 = 1;

/// What reading a world back did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateReport {
    /// Live entities read.
    pub entities: usize,
    /// Components whose fields changed since the save, copied field by
    /// field (new fields take their defaults).
    pub remapped: Vec<String>,
    /// Components the save had that this world does not, dropped.
    pub dropped: Vec<String>,
}

struct Saved {
    name: String,
    size: usize,
    fields: Vec<(String, FieldType, usize)>,
}

/// How one saved component becomes a row of a live one.
enum Plan {
    Drop,
    Copy(ComponentId),
    Fields {
        id: ComponentId,
        /// (live offset, saved offset, size) of each field both have.
        copy: Vec<(usize, usize, usize)>,
        default: Vec<u8>,
    },
}

impl World {
    /// The world as bytes: see the module docs.
    pub fn encode_state(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.magic(TAG, VERSION);
        w.u64(self.frame);
        let schemas = self.registry.schemas();
        w.count(schemas.len());
        for s in schemas {
            w.str(&s.name);
            w.count(s.size);
            w.count(s.fields.len());
            for f in &s.fields {
                w.str(&f.name);
                w.str(f.ty.name());
                w.count(f.offset);
            }
        }
        let strings = self.strings.all();
        w.count(strings.len());
        for s in &strings {
            w.str(s);
        }
        w.count(self.entities.slots.len());
        for s in &self.entities.slots {
            w.u32(s.generation);
            w.u8(s.alive as u8 | (s.authored as u8) << 1);
        }
        w.count(self.entities.free.len());
        for &i in &self.entities.free {
            w.u32(i);
        }
        w.count(self.archetypes.len());
        for a in &self.archetypes {
            w.count(a.components.len());
            for c in &a.components {
                w.u32(c.0);
            }
            w.count(a.entities.len());
            for e in &a.entities {
                w.u64(e.to_bits());
            }
            for col in &a.columns {
                w.bytes(col.bytes());
            }
        }
        let mut names: Vec<_> = self.names.iter().collect();
        names.sort();
        w.count(names.len());
        for (n, e) in names {
            w.str(n);
            w.u64(e.to_bits());
        }
        let mut ids: Vec<_> = self.ids.iter().collect();
        ids.sort();
        w.count(ids.len());
        for (i, n) in ids {
            w.u32(*i);
            w.str(n);
        }
        let mut short: Vec<_> = self.short.iter().collect();
        short.sort();
        w.count(short.len());
        for (n, es) in short {
            w.str(n);
            w.count(es.len());
            for e in es {
                w.u64(e.to_bits());
            }
        }
        w.into_bytes()
    }

    /// Replace this world's state with saved bytes, keeping its registry.
    /// On an error the world is unchanged, but for strings added to its
    /// table (which only grows).
    pub fn decode_state(&mut self, bytes: &[u8]) -> Result<StateReport> {
        let mut r = Reader::new(bytes, "world state");
        r.magic(TAG, VERSION)?;
        let frame = r.u64()?;
        let mut saved = Vec::new();
        for _ in 0..r.count()? {
            let name = r.str()?;
            let size = r.count()?;
            let mut fields = Vec::new();
            for _ in 0..r.count()? {
                let n = r.str()?;
                let ty = r.str()?;
                let ty = FieldType::parse(&ty)
                    .ok_or_else(|| r.bad(format!("unknown field type {ty:?}")))?;
                fields.push((n, ty, r.count()?));
            }
            saved.push(Saved { name, size, fields });
        }
        let mut report = StateReport::default();
        let plans: Vec<Plan> = saved.iter().map(|s| self.plan(s, &mut report)).collect();

        let mut table = Vec::new();
        for _ in 0..r.count()? {
            table.push(self.strings.intern(&r.str()?));
        }
        let mut slots = Vec::new();
        for _ in 0..r.count()? {
            let generation = r.u32()?;
            let flags = r.u8()?;
            slots.push(Slot {
                generation,
                alive: flags & 1 != 0,
                authored: flags & 2 != 0,
                archetype: 0,
                row: 0,
            });
        }
        let mut free = Vec::new();
        for _ in 0..r.count()? {
            free.push(r.u32()?);
        }

        // Rebuild the archetypes in the saved order. Two saved ones that
        // differ only by a dropped component become one, rows appended in
        // order.
        let mut next = World::new();
        next.registry = self.registry.clone();
        let mut placed = vec![false; slots.len()];
        for _ in 0..r.count()? {
            let n = r.count()?;
            let mut comps = Vec::with_capacity(n);
            for _ in 0..n {
                let i = r.u32()? as usize;
                if i >= plans.len() {
                    return Err(r.bad(format!("component {i} of {}", plans.len())));
                }
                comps.push(i);
            }
            let rows = r.rows()?;
            let mut ents = Vec::with_capacity(rows.min(r.remaining() / 8));
            for _ in 0..rows {
                ents.push(Entity::from_bits(r.u64()?));
            }
            let mut cols = Vec::with_capacity(comps.len());
            for &c in &comps {
                let b = r.bytes()?;
                if b.len() != rows * saved[c].size {
                    return Err(r.bad(format!(
                        "{} has {} bytes for {rows} rows of {}",
                        saved[c].name,
                        b.len(),
                        saved[c].size
                    )));
                }
                cols.push(b);
            }
            // The live components this archetype's rows carry.
            let mut live: Vec<(ComponentId, usize)> = comps
                .iter()
                .enumerate()
                .filter_map(|(k, &c)| match &plans[c] {
                    Plan::Drop => None,
                    Plan::Copy(id) | Plan::Fields { id, .. } => Some((*id, k)),
                })
                .collect();
            live.sort();
            live.dedup_by_key(|(id, _)| *id);
            let ids: Vec<ComponentId> = live.iter().map(|(id, _)| *id).collect();
            let aid = next.archetype_for(ids);
            for (row, e) in ents.iter().enumerate() {
                let slot = slots
                    .get_mut(e.index() as usize)
                    .filter(|s| s.alive && s.generation == e.generation())
                    .ok_or_else(|| r.bad(format!("row {row} holds {e}, which is not alive")))?;
                if placed[e.index() as usize] {
                    return Err(r.bad(format!("{e} is stored twice")));
                }
                placed[e.index() as usize] = true;
                let arch = &mut next.archetypes[aid.0 as usize];
                let reg = &next.registry;
                slot.archetype = aid.0;
                slot.row = arch.entities.len() as u32;
                arch.entities.push(*e);
                for (i, &(id, k)) in live.iter().enumerate() {
                    let c = comps[k];
                    let size = saved[c].size;
                    let src = &cols[k][row * size..(row + 1) * size];
                    let mut bytes = match &plans[c] {
                        Plan::Copy(_) => src.to_vec(),
                        Plan::Fields { copy, default, .. } => {
                            let mut b = default.clone();
                            for &(to, from, n) in copy {
                                b[to..to + n].copy_from_slice(&src[from..from + n]);
                            }
                            b
                        }
                        Plan::Drop => unreachable!("dropped components are filtered out"),
                    };
                    translate(&mut bytes, reg.schema(id), &table);
                    arch.columns[i].push(&bytes);
                }
            }
        }
        if let Some(i) = slots.iter().zip(&placed).position(|(s, p)| s.alive && !p) {
            return Err(r.bad(format!("entity slot {i} is alive but in no archetype")));
        }
        let mut names = HashMap::new();
        for _ in 0..r.count()? {
            let n = r.str()?;
            names.insert(n, Entity::from_bits(r.u64()?));
        }
        let mut ids = HashMap::new();
        for _ in 0..r.count()? {
            let i = r.u32()?;
            ids.insert(i, r.str()?);
        }
        let mut short = HashMap::new();
        for _ in 0..r.count()? {
            let n = r.str()?;
            let mut es = Vec::new();
            for _ in 0..r.count()? {
                es.push(Entity::from_bits(r.u64()?));
            }
            short.insert(n, es);
        }
        if r.remaining() != 0 {
            return Err(r.bad(format!("{} bytes after the end", r.remaining())));
        }

        report.entities = slots.iter().filter(|s| s.alive).count();
        self.entities.alive = report.entities;
        self.entities.slots = slots;
        self.entities.free = free;
        self.archetypes = next.archetypes;
        self.archetype_map = next.archetype_map;
        self.names = names;
        self.ids = ids;
        self.short = short;
        self.frame = frame;
        self.events.clear();
        self.emitter = None;
        self.rebuild_broadphase();
        Ok(report)
    }

    fn plan(&self, s: &Saved, report: &mut StateReport) -> Plan {
        let Some(id) = self.registry.id(&s.name) else {
            report.dropped.push(s.name.clone());
            return Plan::Drop;
        };
        let live = self.registry.schema(id);
        let same = live.size == s.size
            && live.fields.len() == s.fields.len()
            && live
                .fields
                .iter()
                .zip(&s.fields)
                .all(|(a, (n, ty, off))| a.name == *n && a.ty == *ty && a.offset == *off);
        if same {
            return Plan::Copy(id);
        }
        report.remapped.push(s.name.clone());
        let copy = live
            .fields
            .iter()
            .filter_map(|f| {
                s.fields
                    .iter()
                    .find(|(n, ty, _)| *n == f.name && *ty == f.ty)
                    .map(|(_, _, off)| (f.offset, *off, f.ty.size()))
            })
            .collect();
        Plan::Fields {
            id,
            copy,
            default: live.default_row(&self.strings),
        }
    }

    /// A digest of the whole world: every field of every live entity, in
    /// entity order, components by name, floats to the bit, strings as
    /// text. Two worlds with the same digest are the same world, whichever
    /// process, platform, or string table they came from.
    pub fn digest(&self) -> u64 {
        fn fnv(h: &mut u64, bytes: &[u8]) {
            for b in bytes {
                *h ^= *b as u64;
                *h = h.wrapping_mul(0x100000001b3);
            }
        }
        let mut h = 0xcbf29ce484222325u64;
        let mut live: Vec<_> = self.live().collect();
        live.sort();
        for e in live {
            fnv(&mut h, e.to_string().as_bytes());
            let Ok(comps) = self.components_of(e) else {
                continue;
            };
            let mut comps: Vec<_> = comps.to_vec();
            comps.sort_by(|a, b| {
                self.registry
                    .schema(*a)
                    .name
                    .cmp(&self.registry.schema(*b).name)
            });
            for c in comps {
                let schema = self.registry.schema(c);
                fnv(&mut h, schema.name.as_bytes());
                let _ = self.with_row(e, c, |row| {
                    for f in &schema.fields {
                        if let Some(v) = self.decode_field(f, row) {
                            let s = match v {
                                Value::Float(x) => format!("{:016x}", x.to_bits()),
                                other => format!("{other:?}"),
                            };
                            fnv(&mut h, s.as_bytes());
                        }
                    }
                });
            }
        }
        h
    }
}

/// Point a row's string fields at the reading world's ids. (Rows are
/// native-endian; every target Argus builds for is little-endian.)
fn translate(row: &mut [u8], schema: &ComponentSchema, table: &[StrId]) {
    for f in &schema.fields {
        if matches!(f.ty, FieldType::Str | FieldType::Ref) {
            let b = &mut row[f.offset..f.offset + 4];
            let saved = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
            let live = table.get(saved).copied().unwrap_or(StrId::EMPTY);
            b.copy_from_slice(&live.0.to_le_bytes());
        }
    }
}
