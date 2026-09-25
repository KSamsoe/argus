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
//!
//! Each component's schema version is written beside it (format version
//! 2; version 1, written before save games, reads as "the live version").
//! A save game (§6, phase 3 item 0) read by a newer build meets components
//! whose version went up: `decode_state_with` hands each such row, as field
//! values by the names it was saved with, to a [`Migrate`] (the script host,
//! for components declared in Luau), and writes what comes back into the
//! live fields. Every version change is reported, migrated or not.

use crate::codec::{Reader, Writer};
use crate::entity::{Entity, Slot};
use crate::error::{codes, ArgusError, Result};
use crate::intern::{Interner, StrId};
use crate::schema::{ComponentId, ComponentSchema};
use crate::value::{FieldType, Value};
use crate::world::World;
use std::collections::HashMap;

const TAG: &[u8; 4] = b"ARGW";
const VERSION: u32 = 2;

/// One saved row of a component, as field values by name.
pub type Row = Vec<(String, Value)>;

/// What brings rows saved by an older version of a component up to the
/// live one (§6 "Save games": "a component's schema file may declare a
/// migrate function from older versions").
pub trait Migrate {
    /// Whether `component` declares a migration.
    fn declares(&self, component: &str) -> bool;
    /// Rewrite `row`, saved at version `from`, into the fields of `live`
    /// (its schema now). Fields the row still names that `live` lacks are
    /// dropped; fields `live` has that the row lacks take their defaults.
    fn migrate(&mut self, live: &ComponentSchema, from: u32, row: &mut Row) -> Result<()>;
}

/// A component whose version changed since the save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Migrated {
    /// The component.
    pub component: String,
    /// The version saved.
    pub from: u32,
    /// The live version.
    pub to: u32,
    /// Rows brought up.
    pub rows: usize,
    /// By its declared migration (else field by field, by name).
    pub declared: bool,
    /// Saved fields no live field took (renamed without a migration, or
    /// dropped).
    pub left: Vec<String>,
}

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
    /// Components saved at an older version, brought up (§6: "Health
    /// migrated v2→v3 for 1,204 entities").
    pub migrated: Vec<Migrated>,
    /// Components saved at a newer version than this build has: read
    /// field by field, which a save game refuses.
    pub newer: Vec<(String, u32, u32)>,
}

struct Saved {
    name: String,
    /// `None` in a version 1 world.
    version: Option<u32>,
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
    /// Rows through the declared migration, value by value.
    Migrate {
        id: ComponentId,
        from: u32,
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
            w.u32(s.version);
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
        self.decode_state_with(bytes, None)
    }

    /// `decode_state`, with rows of components saved at an older version
    /// brought up by `migrate` where it declares a migration for them.
    pub fn decode_state_with(
        &mut self,
        bytes: &[u8],
        mut migrate: Option<&mut dyn Migrate>,
    ) -> Result<StateReport> {
        let mut r = Reader::new(bytes, "world state");
        let format = r.magic(TAG, VERSION)?;
        let frame = r.u64()?;
        let mut saved = Vec::new();
        for _ in 0..r.count()? {
            let name = r.str()?;
            let version = if format >= 2 { Some(r.u32()?) } else { None };
            let size = r.count()?;
            let mut fields = Vec::new();
            for _ in 0..r.count()? {
                let n = r.str()?;
                let ty = r.str()?;
                let ty = FieldType::parse(&ty)
                    .ok_or_else(|| r.bad(format!("unknown field type {ty:?}")))?;
                fields.push((n, ty, r.count()?));
            }
            saved.push(Saved {
                name,
                version,
                size,
                fields,
            });
        }
        let mut report = StateReport::default();
        let declares = |n: &str| migrate.as_ref().is_some_and(|m| m.declares(n));
        let plans: Vec<Plan> = saved
            .iter()
            .map(|s| self.plan(s, declares(&s.name), &mut report))
            .collect();
        let mut rows_by = vec![0usize; saved.len()];
        let mut left_by: Vec<Vec<String>> = vec![Vec::new(); saved.len()];

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
                    Plan::Copy(id) | Plan::Fields { id, .. } | Plan::Migrate { id, .. } => {
                        Some((*id, k))
                    }
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
                    rows_by[c] += 1;
                    let bytes = match &plans[c] {
                        Plan::Copy(_) => {
                            let mut b = src.to_vec();
                            translate(&mut b, reg.schema(id), &table);
                            b
                        }
                        Plan::Fields { copy, default, .. } => {
                            let mut b = default.clone();
                            for &(to, from, n) in copy {
                                b[to..to + n].copy_from_slice(&src[from..from + n]);
                            }
                            translate(&mut b, reg.schema(id), &table);
                            b
                        }
                        Plan::Migrate { from, .. } => {
                            let m = migrate.as_deref_mut().expect("planned with a migrator");
                            let schema = reg.schema(id);
                            let what = |err: ArgusError| {
                                in_migration(err, &saved[c].name, *from, schema.version, *e)
                            };
                            let mut values = read_row(&saved[c], src, &table, &self.strings);
                            m.migrate(schema, *from, &mut values).map_err(what)?;
                            write_row(schema, values, &self.strings, &mut left_by[c])
                                .map_err(what)?
                        }
                        Plan::Drop => unreachable!("dropped components are filtered out"),
                    };
                    arch.columns[i].push(&bytes);
                }
            }
        }
        if let Some(i) = slots.iter().zip(&placed).position(|(s, p)| s.alive && !p) {
            return Err(r.bad(format!("entity slot {i} is alive but in no archetype")));
        }
        for (c, s) in saved.iter().enumerate() {
            let (Some(from), Some(id)) = (s.version, self.registry.id(&s.name)) else {
                continue;
            };
            let live = self.registry.schema(id);
            if from >= live.version {
                continue;
            }
            let declared = matches!(plans[c], Plan::Migrate { .. });
            let mut left = if declared {
                std::mem::take(&mut left_by[c])
            } else {
                s.fields
                    .iter()
                    .filter(|(n, ty, _)| !live.fields.iter().any(|f| f.name == *n && f.ty == *ty))
                    .map(|(n, _, _)| n.clone())
                    .collect()
            };
            left.sort();
            left.dedup();
            report.migrated.push(Migrated {
                component: s.name.clone(),
                from,
                to: live.version,
                rows: rows_by[c],
                declared,
                left,
            });
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
        self.structure = crate::world::next_structure();
        self.rebuild_broadphase();
        Ok(report)
    }

    fn plan(&self, s: &Saved, declares: bool, report: &mut StateReport) -> Plan {
        let Some(id) = self.registry.id(&s.name) else {
            report.dropped.push(s.name.clone());
            return Plan::Drop;
        };
        let live = self.registry.schema(id);
        if let Some(v) = s.version {
            if v < live.version && declares {
                return Plan::Migrate { id, from: v };
            }
            if v > live.version {
                report.newer.push((s.name.clone(), v, live.version));
            }
        }
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

/// A saved row as values by the names it was saved with, its strings as
/// text.
fn read_row(saved: &Saved, src: &[u8], table: &[StrId], strings: &Interner) -> Row {
    saved
        .fields
        .iter()
        .map(|(name, ty, off)| {
            let b = &src[*off..*off + ty.size()];
            let v = match ty {
                FieldType::Str | FieldType::Ref => {
                    let i = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                    let text = strings
                        .get(table.get(i).copied().unwrap_or(StrId::EMPTY))
                        .to_string();
                    if *ty == FieldType::Str {
                        Value::Str(text)
                    } else {
                        Value::Ref(text)
                    }
                }
                // SAFETY: `b` holds exactly `ty.size()` bytes, and `read`
                // reads them unaligned.
                _ => unsafe { ty.read(b.as_ptr(), strings) },
            };
            (name.clone(), v)
        })
        .collect()
}

/// A live row from migrated values: fields the row names are written
/// (coerced to the live type), the rest keep their defaults, and names no
/// live field has are collected in `left`.
fn write_row(
    schema: &ComponentSchema,
    values: Row,
    strings: &Interner,
    left: &mut Vec<String>,
) -> Result<Vec<u8>> {
    let mut b = schema.default_row(strings);
    for (name, v) in values {
        let Some(f) = schema.field(&name) else {
            left.push(name);
            continue;
        };
        let v = v
            .coerce(f.ty)
            .map_err(|e| ArgusError::new(e.code, format!("{name}: {}", e.message)))?;
        // SAFETY: the row is the schema's size and `f` is one of its fields.
        unsafe { f.ty.write(b[f.offset..].as_mut_ptr(), &v, strings)? };
    }
    Ok(b)
}

/// A migration's error, saying which component, versions, and entity.
fn in_migration(e: ArgusError, component: &str, from: u32, to: u32, entity: Entity) -> ArgusError {
    ArgusError::new(
        codes::BAD_STATE,
        format!(
            "{component} v{from}→v{to} on {entity}: {}",
            e.message
        ),
    )
    .hint("The component's migrate (in its schema file) returns its fields as they are now; a field whose type changed needs a value of the new type.")
    .at(e.r#ref.clone())
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
