//! Prefab resolution with provenance (§3 "Prefabs and overrides"). An entity
//! document plus its prefab chain becomes a [`ResolvedEntity`]: every
//! component, every field, and every tag with where its value came from
//! (`own`, a prefab path, or the schema default). This is what
//! `world.get {resolved: true}` returns and what the loader instantiates.
//!
//! The rules, from the document:
//! - inheritance is per field, never per component;
//! - adding a component the base lacks is an override; removing one is an
//!   explicit `-Component` entry;
//! - children are addressed by path (`@door.east/light`) and overridden the
//!   same way.

use crate::defs::{locate, Defs};
use crate::doc::EntityDoc;
use crate::moves::{self, MoveRecord};
use crate::prefab::PrefabLibrary;
use crate::tomlv;
use argus_ecs::{ArgusError, FieldType, Registry, Result, Value};
use std::fmt;
use std::path::Path;

/// Where a value came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// Set in the entity's own file.
    Own,
    /// Inherited from this prefab (its reference path).
    Prefab(String),
    /// Nobody set it: the schema default.
    Default,
}

impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provenance::Own => f.write_str("own"),
            Provenance::Prefab(p) => f.write_str(p),
            Provenance::Default => f.write_str("default"),
        }
    }
}

/// One resolved field.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedField {
    /// Field name.
    pub name: String,
    /// Storage type.
    pub ty: FieldType,
    /// The value. Entity fields hold a `Value::Str` placeholder (`@id`,
    /// possibly room-qualified, or empty for null) until the scene spawns.
    pub value: Value,
    /// Where it came from.
    pub from: Provenance,
    /// `file:line` of the setting, or the schema's source for defaults.
    pub at: String,
}

/// One resolved component.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedComponent {
    /// Component name.
    pub name: String,
    /// Fields in schema order.
    pub fields: Vec<ResolvedField>,
    /// Which layer added the component.
    pub from: Provenance,
}

/// An entity with inheritance applied.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEntity {
    /// Full string id (`door.east`, `cavern-03/door.east`, `door.east/light`).
    pub id: String,
    /// The prefab the entity's own file names, if any.
    pub prefab: Option<String>,
    /// Prefab chain from nearest to root base.
    pub chain: Vec<String>,
    /// Tags with provenance.
    pub tags: Vec<(String, Provenance)>,
    /// Components in the order they were first added.
    pub components: Vec<ResolvedComponent>,
    /// Child entities.
    pub children: Vec<ResolvedEntity>,
    /// Project-relative file the entity's own layer lives in.
    pub file: String,
}

impl ResolvedEntity {
    /// A component by name.
    pub fn component(&self, name: &str) -> Option<&ResolvedComponent> {
        self.components.iter().find(|c| c.name == name)
    }

    /// A field by `Component.field` path.
    pub fn field(&self, path: &str) -> Option<&ResolvedField> {
        let (c, f) = path.split_once('.')?;
        self.component(c)?.fields.iter().find(|x| x.name == f)
    }

    /// True if any tag or component came from `prefab`, or it is in the chain.
    pub fn uses_prefab(&self, prefab: &str) -> bool {
        self.chain.iter().any(|p| p == prefab)
            || self.children.iter().any(|c| c.uses_prefab(prefab))
    }

    /// Pre-order walk: the entity, then its children, with each one's
    /// parent id.
    pub fn flatten(&self) -> Vec<(&ResolvedEntity, Option<&str>)> {
        let mut out = Vec::new();
        self.walk(None, &mut out);
        out
    }

    fn walk<'a>(
        &'a self,
        parent: Option<&'a str>,
        out: &mut Vec<(&'a ResolvedEntity, Option<&'a str>)>,
    ) {
        out.push((self, parent));
        for c in &self.children {
            c.walk(Some(&self.id), out);
        }
    }

    /// Find an entity in this tree by full id.
    pub fn find(&self, id: &str) -> Option<&ResolvedEntity> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }
}

/// Everything resolution needs to look at.
#[derive(Clone, Copy)]
pub struct ResolveCtx<'a> {
    /// Project root, for checking bare path references.
    pub root: &'a Path,
    /// Prefabs.
    pub prefabs: &'a PrefabLibrary,
    /// Def tables, for `prefix:key` references.
    pub defs: &'a Defs,
    /// The moves log, for missing-reference hints.
    pub moves: &'a [MoveRecord],
    /// Component schemas.
    pub registry: &'a Registry,
}

struct Layer<'a> {
    from: Provenance,
    doc: &'a EntityDoc,
}

/// Resolve `doc` (from `file`) as entity `id`. Structural problems (unknown
/// component or field, a value that cannot be stored, a broken prefab
/// chain) are the error; missing def or path references are storable and
/// go to `issues` so every one of them is reported at once.
pub fn resolve(
    ctx: &ResolveCtx<'_>,
    id: &str,
    doc: &EntityDoc,
    file: &str,
    issues: &mut Vec<ArgusError>,
) -> Result<ResolvedEntity> {
    let layers = layers_for(ctx, doc, &format!("{file}:1"))?;
    resolve_layers(ctx, id, layers, file, issues)
}

/// Own layer first, then the prefab chain.
fn layers_for<'a>(ctx: &ResolveCtx<'a>, doc: &'a EntityDoc, at: &str) -> Result<Vec<Layer<'a>>> {
    let mut layers = vec![Layer {
        from: Provenance::Own,
        doc,
    }];
    if let Some(p) = &doc.prefab {
        for pf in ctx.prefabs.chain(p, at, ctx.moves)? {
            layers.push(Layer {
                from: Provenance::Prefab(pf.path.clone()),
                doc: &pf.doc,
            });
        }
    }
    Ok(layers)
}

fn resolve_layers(
    ctx: &ResolveCtx<'_>,
    id: &str,
    layers: Vec<Layer<'_>>,
    file: &str,
    issues: &mut Vec<ArgusError>,
) -> Result<ResolvedEntity> {
    let mut comps: Vec<ResolvedComponent> = Vec::new();
    let mut tags: Vec<(String, Provenance)> = Vec::new();
    let mut child_names: Vec<String> = Vec::new();

    // Base first, own last, so nearer layers win.
    for layer in layers.iter().rev() {
        let d = layer.doc;
        for r in &d.removed {
            comps.retain(|c| &c.name != r);
        }
        for t in &d.removed_tags {
            tags.retain(|(n, _)| n != t);
        }
        for c in &d.removed_children {
            child_names.retain(|n| n != c);
        }
        for cd in &d.components {
            let cid = ctx.registry.id(&cd.name).ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::UNKNOWN_COMPONENT,
                    format!("no component named {:?}", cd.name),
                )
                .hint("world.describe lists registered components; script components must load before the scene.")
                .at(Some(cd.at.clone()))
            })?;
            let schema = ctx.registry.schema(cid);
            if !comps.iter().any(|c| c.name == cd.name) {
                comps.push(ResolvedComponent {
                    name: cd.name.clone(),
                    fields: Vec::new(),
                    from: layer.from.clone(),
                });
            }
            let comp = comps
                .iter_mut()
                .find(|c| c.name == cd.name)
                .expect("just ensured");
            for f in &cd.fields {
                let fs = schema.field(&f.name).ok_or_else(|| {
                    ArgusError::new(
                        argus_ecs::codes::UNKNOWN_FIELD,
                        format!("{} has no field {:?}", cd.name, f.name),
                    )
                    .hint(format!(
                        "Fields: {}",
                        schema
                            .fields
                            .iter()
                            .map(|x| x.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                    .at(Some(f.at.clone()))
                })?;
                let v = tomlv::to_value(&f.raw, fs.ty, &f.at)?;
                let v = match fs.ty {
                    FieldType::Entity => v,
                    _ => schema.check(fs, v).map_err(|e| locate(e, &f.at))?,
                };
                if let Value::Ref(s) = &v {
                    if let Err(e) = check_ref(ctx, s, &f.at) {
                        issues.push(e);
                    }
                }
                let rf = ResolvedField {
                    name: f.name.clone(),
                    ty: fs.ty,
                    value: v,
                    from: layer.from.clone(),
                    at: f.at.clone(),
                };
                match comp.fields.iter_mut().find(|x| x.name == f.name) {
                    Some(slot) => *slot = rf,
                    None => comp.fields.push(rf),
                }
            }
        }
        for t in &d.tags {
            if !tags.iter().any(|(n, _)| n == t) {
                tags.push((t.clone(), layer.from.clone()));
            }
        }
        for (name, _) in &d.children {
            if !child_names.contains(name) {
                child_names.push(name.clone());
            }
        }
    }

    // Fill defaults and put fields in schema order.
    for comp in &mut comps {
        let cid = ctx.registry.id(&comp.name).expect("checked above");
        let schema = ctx.registry.schema(cid);
        let set = std::mem::take(&mut comp.fields);
        for fs in &schema.fields {
            match set.iter().find(|x| x.name == fs.name) {
                Some(f) => comp.fields.push(f.clone()),
                None => comp.fields.push(ResolvedField {
                    name: fs.name.clone(),
                    ty: fs.ty,
                    value: fs.default.clone(),
                    from: Provenance::Default,
                    at: schema.source.clone().unwrap_or_default(),
                }),
            }
        }
    }

    // Children: every layer's document for that name, in the same order,
    // plus the child's own prefab chain if it declares one.
    let mut children = Vec::new();
    for name in child_names {
        let mut child_layers: Vec<Layer<'_>> = Vec::new();
        let mut child_prefab: Option<(&str, &Provenance)> = None;
        for layer in &layers {
            if let Some(cd) = layer.doc.child(&name) {
                if child_prefab.is_none() {
                    if let Some(p) = &cd.prefab {
                        child_prefab = Some((p, &layer.from));
                    }
                }
                child_layers.push(Layer {
                    from: layer.from.clone(),
                    doc: cd,
                });
            }
        }
        if let Some((p, _)) = child_prefab {
            for pf in ctx.prefabs.chain(p, &format!("{file}:1"), ctx.moves)? {
                child_layers.push(Layer {
                    from: Provenance::Prefab(pf.path.clone()),
                    doc: &pf.doc,
                });
            }
        }
        let cid = format!("{id}/{name}");
        children.push(resolve_layers(ctx, &cid, child_layers, file, issues)?);
    }

    let own = &layers[0];
    // Every prefab layer, in order. A child that exists only because a
    // prefab declares it has no own layer, so the first layer may be one.
    let chain: Vec<String> = layers
        .iter()
        .filter_map(|l| match &l.from {
            Provenance::Prefab(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    Ok(ResolvedEntity {
        id: id.to_string(),
        prefab: own.doc.prefab.clone(),
        chain,
        tags,
        components: comps,
        children,
        file: file.to_string(),
    })
}

/// A reference value must point at something: a def row for `prefix:key`
/// with a known prefix, a prefab or a project file for a bare path. Empty
/// is null. Unknown prefixes pass through (asset kinds arrive in phase 1).
pub fn check_ref(ctx: &ResolveCtx<'_>, s: &str, at: &str) -> Result<()> {
    if s.is_empty() {
        return Ok(());
    }
    if s.contains(':') {
        return ctx.defs.check_ref(s, at, ctx.moves);
    }
    if ctx.prefabs.get(s).is_some() || ctx.root.join(s).exists() {
        return Ok(());
    }
    Err(moves::missing_reference("path", s, at, ctx.moves))
}
