//! The entity file (§4 "Entity files") and the prefab file (§3 "Prefabs and
//! overrides"), which share one shape:
//!
//! ```toml
//! prefab = "door/locked"      # optional base
//! tags   = ["door"]           # zero-size components
//! -tags  = ["decoy"]          # remove tags the base has
//! -Door  = true               # remove a component the base has (§3)
//! -children = ["light"]       # remove a child the base has
//!
//! [Transform]                 # a component; keys are field overrides
//! pos = [46.0, 8.0]
//!
//! [Door]                      # an empty table adds the component with defaults
//!
//! [children.light.Light]      # child entities, addressed as @id/light
//! intensity = 2.0
//! ```
//!
//! A document stores only what the file says. Inheritance and defaults are
//! applied in [`crate::resolve`]. Values stay as TOML until a schema types
//! them, so prefab files can be read before any world exists.

use crate::codes;
use crate::tomlv;
use argus_ecs::{ArgusError, Result, Value};
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, TableLike, Value as TValue};

/// One field override inside a component table.
#[derive(Clone, Debug)]
pub struct FieldDoc {
    /// Field name.
    pub name: String,
    /// The value as written.
    pub raw: TValue,
    /// `file:line` of the key.
    pub at: String,
}

/// One `[Component]` table.
#[derive(Clone, Debug)]
pub struct ComponentDoc {
    /// Component name.
    pub name: String,
    /// Field overrides in file order.
    pub fields: Vec<FieldDoc>,
    /// `file:line` of the table header.
    pub at: String,
}

/// A parsed entity or prefab file.
#[derive(Clone, Debug, Default)]
pub struct EntityDoc {
    /// Base prefab path (`door/locked`), if any.
    pub prefab: Option<String>,
    /// Tags added.
    pub tags: Vec<String>,
    /// Tags removed from the base.
    pub removed_tags: Vec<String>,
    /// Components with their overrides, in file order.
    pub components: Vec<ComponentDoc>,
    /// Components removed from the base (`-Door = true`).
    pub removed: Vec<String>,
    /// Child entities by name, in file order.
    pub children: Vec<(String, EntityDoc)>,
    /// Children removed from the base.
    pub removed_children: Vec<String>,
    /// `file:line` where this document starts.
    pub at: String,
}

impl EntityDoc {
    /// Parse a file's text. `file` is the project-relative path used in refs.
    pub fn parse(file: &str, text: &str) -> Result<EntityDoc> {
        let im = tomlv::parse(file, text)?;
        let mut doc = Self::from_table(file, text, im.as_table(), &format!("{file}:1"))?;
        doc.at = format!("{file}:1");
        Ok(doc)
    }

    fn from_table(file: &str, text: &str, t: &dyn TableLike, at: &str) -> Result<EntityDoc> {
        let mut doc = EntityDoc {
            at: at.to_string(),
            ..Default::default()
        };
        for (key, item) in t.iter() {
            let at = tomlv::item_ref(file, text, item);
            match key {
                "prefab" => {
                    doc.prefab = Some(
                        item.as_str()
                            .ok_or_else(|| {
                                ArgusError::new(
                                    codes::BAD_SCENE,
                                    "prefab must be a string path like \"door/locked\"",
                                )
                                .at(Some(at.clone()))
                            })?
                            .to_string(),
                    );
                }
                "tags" => doc.tags = tomlv::string_list(item, "tags", &at)?,
                "-tags" => doc.removed_tags = tomlv::string_list(item, "-tags", &at)?,
                "-children" => doc.removed_children = tomlv::string_list(item, "-children", &at)?,
                "children" => {
                    let ct = tomlv::table_like(item, "children", &at)?;
                    for (name, child) in ct.iter() {
                        let cat = tomlv::item_ref(file, text, child);
                        let ctab = tomlv::table_like(child, &format!("children.{name}"), &cat)?;
                        doc.children
                            .push((name.to_string(), Self::from_table(file, text, ctab, &cat)?));
                    }
                }
                k if k.starts_with('-') => {
                    let name = &k[1..];
                    if item.as_bool() != Some(true) {
                        return Err(ArgusError::new(
                            codes::BAD_SCENE,
                            format!("{k} must be `true` to remove the {name} component"),
                        )
                        .hint("Removal is an explicit entry so a reader sees it: `-Door = true`.")
                        .at(Some(at)));
                    }
                    doc.removed.push(name.to_string());
                }
                name => {
                    let ct = item.as_table_like().ok_or_else(|| {
                        ArgusError::new(
                            codes::BAD_SCENE,
                            format!("unexpected key {name:?}; components are tables"),
                        )
                        .hint("An entity file has prefab, tags, -Component entries, [Component] tables, and [children.name] tables.")
                        .at(Some(at.clone()))
                    })?;
                    let mut fields = Vec::new();
                    for (fname, fitem) in ct.iter() {
                        let fat = tomlv::item_ref(file, text, fitem);
                        let raw = fitem.as_value().ok_or_else(|| {
                            ArgusError::new(
                                codes::BAD_SCENE,
                                format!("{name}.{fname} must be a value, not a table"),
                            )
                            .hint(
                                "Fields are flat: numbers, strings, booleans, or [x, y] for vec2.",
                            )
                            .at(Some(fat.clone()))
                        })?;
                        fields.push(FieldDoc {
                            name: fname.to_string(),
                            raw: raw.clone(),
                            at: fat,
                        });
                    }
                    doc.components.push(ComponentDoc {
                        name: name.to_string(),
                        fields,
                        at,
                    });
                }
            }
        }
        Ok(doc)
    }

    /// Look up a component table.
    pub fn component(&self, name: &str) -> Option<&ComponentDoc> {
        self.components.iter().find(|c| c.name == name)
    }

    /// Look up a child by name.
    pub fn child(&self, name: &str) -> Option<&EntityDoc> {
        self.children
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d)
    }

    /// Follow a child path (`light/bulb`) from this document.
    pub fn child_path(&self, path: &str) -> Option<&EntityDoc> {
        let mut cur = self;
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            cur = cur.child(seg)?;
        }
        Some(cur)
    }

    /// Canonical TOML text for a new file: keys in the order the parser
    /// expects, one blank line between tables.
    pub fn to_toml_string(&self) -> Result<String> {
        let mut doc = DocumentMut::new();
        self.write_into(doc.as_table_mut())?;
        Ok(doc.to_string())
    }

    fn write_into(&self, t: &mut Table) -> Result<()> {
        if let Some(p) = &self.prefab {
            t["prefab"] = toml_edit::value(p.as_str());
        }
        if !self.tags.is_empty() {
            t["tags"] = toml_edit::value(toml_edit::Array::from_iter(
                self.tags.iter().map(String::as_str),
            ));
        }
        if !self.removed_tags.is_empty() {
            t["-tags"] = toml_edit::value(toml_edit::Array::from_iter(
                self.removed_tags.iter().map(String::as_str),
            ));
        }
        if !self.removed_children.is_empty() {
            t["-children"] = toml_edit::value(toml_edit::Array::from_iter(
                self.removed_children.iter().map(String::as_str),
            ));
        }
        for r in &self.removed {
            t[&format!("-{r}")] = toml_edit::value(true);
        }
        for c in &self.components {
            let mut ct = Table::new();
            for f in &c.fields {
                ct[&f.name] = Item::Value(f.raw.clone());
            }
            t[&c.name] = Item::Table(ct);
        }
        if !self.children.is_empty() {
            let mut children = Table::new();
            children.set_implicit(true);
            for (name, child) in &self.children {
                let mut ct = Table::new();
                child.write_into(&mut ct)?;
                if ct.is_empty() {
                    ct.set_implicit(false);
                }
                children[name] = Item::Table(ct);
            }
            t["children"] = Item::Table(children);
        }
        Ok(())
    }
}

/// Edit an entity file in place, preserving comments and formatting, then
/// re-parse it so the caller gets the new document. `f` receives the table
/// the `path` (a child path such as `light`, or empty for the root) refers to.
pub fn edit_file(
    path: &Path,
    rel: &str,
    child: &str,
    f: impl FnOnce(&mut Table) -> Result<()>,
) -> Result<EntityDoc> {
    let text = if argus_os::fs::exists(path) {
        tomlv::read(path)?
    } else {
        String::new()
    };
    let mut doc = tomlv::parse_mut(rel, &text)?;
    let mut t: &mut Table = doc.as_table_mut();
    for seg in child.split('/').filter(|s| !s.is_empty()) {
        let children = t
            .entry("children")
            .or_insert_with(|| {
                let mut c = Table::new();
                c.set_implicit(true);
                Item::Table(c)
            })
            .as_table_mut()
            .ok_or_else(|| {
                ArgusError::new(codes::BAD_SCENE, "children must be a table")
                    .at(Some(rel.to_string()))
            })?;
        t = children
            .entry(seg)
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| {
                ArgusError::new(codes::BAD_SCENE, format!("children.{seg} must be a table"))
                    .at(Some(rel.to_string()))
            })?;
    }
    f(t)?;
    let out = doc.to_string();
    if let Some(dir) = path.parent() {
        argus_os::fs::create_dir_all(dir).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("cannot create {}: {e}", dir.display()),
            )
        })?;
    }
    argus_os::fs::write(path, &out).map_err(|e| {
        ArgusError::new(
            codes::MISSING_FILE,
            format!("cannot write {}: {e}", path.display()),
        )
    })?;
    EntityDoc::parse(rel, &out)
}

/// Set one field override in a component table (creating the table).
pub fn set_override(t: &mut Table, component: &str, field: &str, value: &Value) -> Result<()> {
    let ct = t
        .entry(component)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| {
            ArgusError::new(
                codes::BAD_SCENE,
                format!("{component} is not a table in the file"),
            )
        })?;
    let mut new = tomlv::from_value(value)?;
    // Replacing in place keeps the line's spacing and trailing comment.
    if let Some(existing) = ct.get_mut(field).and_then(Item::as_value_mut) {
        *new.decor_mut() = existing.decor().clone();
        *existing = new;
    } else {
        ct.insert(field, Item::Value(new));
    }
    Ok(())
}

/// Remove one field override; the component table stays (an empty table
/// still means "has the component").
pub fn clear_override(t: &mut Table, component: &str, field: &str) {
    if let Some(ct) = t.get_mut(component).and_then(Item::as_table_like_mut) {
        ct.remove(field);
    }
}

/// Remove a component table entirely, and drop any `-Component` entry.
pub fn remove_component(t: &mut Table, component: &str) {
    t.remove(component);
    t.remove(&format!("-{component}"));
}

/// Mark a component as removed from the base (`-Component = true`), dropping
/// its overrides if any.
pub fn mark_removed(t: &mut Table, component: &str) {
    t.remove(component);
    t[&format!("-{component}")] = toml_edit::value(true);
}
