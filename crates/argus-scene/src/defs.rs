//! Data tables (§4 "Data tables"): `defs/<table>.toml` holds rows keyed by
//! id, validated against a schema declared in script (`def "items" {...}`,
//! piece 3) or in Rust. Rows are referenced as `prefix:key` (`item:key.brass`)
//! and queried with `world.defs {table, where}`.
//!
//! ```toml
//! # defs/items.toml
//! ["key.brass"]
//! name   = "Brass key"
//! weight = 0.1
//! ```

use crate::codes;
use crate::moves::{self, MoveRecord};
use crate::tomlv;
use argus_ecs::layers::{self, Change};
use argus_ecs::{ArgusError, ComponentSchema, FieldType, Layers, Result, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// The schema of one table: row layout, key field, reference prefix.
#[derive(Clone, Debug, PartialEq)]
pub struct DefSchema {
    /// Row schema; `name` is the table name (`items`).
    pub schema: ComponentSchema,
    /// The field that holds the row key. Default `id`; if the schema does
    /// not declare it, the key is the table header only.
    pub key: String,
    /// Reference prefix (`item` in `item:key.brass`). Default: the table name.
    pub prefix: String,
}

impl DefSchema {
    /// Build with the defaults for key (`id`) and prefix (the table name).
    pub fn new(schema: ComponentSchema) -> DefSchema {
        let prefix = schema.name.clone();
        DefSchema {
            schema,
            key: "id".to_string(),
            prefix,
        }
    }

    /// Set the key field.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }

    /// Set the reference prefix.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }
}

/// One row.
#[derive(Clone, Debug, PartialEq)]
pub struct DefRow {
    /// Row key (the table header in the file).
    pub key: String,
    /// Values in schema field order.
    pub values: Vec<Value>,
    /// Beside each value: whether the row wrote it (the others are the
    /// schema's defaults), for a table whose reader treats a field left
    /// out differently from one written as its default.
    pub given: Vec<bool>,
    /// `file:line` of the row.
    pub at: String,
}

/// One loaded table.
#[derive(Clone, Debug, PartialEq)]
pub struct DefTable {
    /// Table name (`items`).
    pub name: String,
    /// Reference prefix (`item`).
    pub prefix: String,
    /// Key field name.
    pub key: String,
    /// Row schema.
    pub schema: ComponentSchema,
    /// Project-relative file, if one exists.
    pub file: Option<String>,
    /// Rows in file order.
    pub rows: Vec<DefRow>,
}

impl DefTable {
    /// A row by key.
    pub fn get(&self, key: &str) -> Option<&DefRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    /// Column index of a field.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.schema.fields.iter().position(|f| f.name == name)
    }

    /// Column names in order.
    pub fn columns(&self) -> Vec<&str> {
        self.schema.fields.iter().map(|f| f.name.as_str()).collect()
    }

    /// One value of a row by field name.
    pub fn value<'a>(&self, row: &'a DefRow, field: &str) -> Option<&'a Value> {
        self.field_index(field).and_then(|i| row.values.get(i))
    }

    /// Whether a row wrote a field rather than taking its default.
    pub fn given(&self, row: &DefRow, field: &str) -> bool {
        self.field_index(field)
            .and_then(|i| row.given.get(i).copied())
            .unwrap_or(false)
    }

    /// Rows where every `(field, value)` pair matches (values are coerced to
    /// the field's type first, so `1` matches an `f32` column holding `1.0`).
    pub fn query(&self, where_: &[(String, Value)]) -> Result<Vec<&DefRow>> {
        let mut conds = Vec::with_capacity(where_.len());
        for (field, v) in where_ {
            let i = self.field_index(field).ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::UNKNOWN_FIELD,
                    format!("{} has no field {field:?}", self.name),
                )
                .hint(format!("Fields: {}", self.columns().join(", ")))
            })?;
            let ty = self.schema.fields[i].ty;
            conds.push((i, v.clone().coerce(ty)?));
        }
        Ok(self
            .rows
            .iter()
            .filter(|r| conds.iter().all(|(i, v)| &r.values[*i] == v))
            .collect())
    }
}

/// All def tables of a project.
#[derive(Clone, Debug, Default)]
pub struct Defs {
    tables: BTreeMap<String, DefTable>,
    by_prefix: BTreeMap<String, String>,
}

impl Defs {
    /// No tables.
    pub fn empty() -> Defs {
        Defs::default()
    }

    /// Load `defs/` under `root` against the declared schemas. Problems are
    /// returned alongside the tables that did load: a bad row skips that
    /// row, a table without a schema is reported and skipped, a schema
    /// without a file is an empty table.
    pub fn load(
        root: &Path,
        schemas: &[DefSchema],
        moves_log: &[MoveRecord],
    ) -> (Defs, Vec<ArgusError>) {
        let (defs, errors, _) = Defs::load_layers(&Layers::game(root), schemas, moves_log);
        (defs, errors)
    }

    /// [`load`](Defs::load) for the game and its mods (phase 3 item 8):
    /// each table's rows from the game's `defs/<table>.toml`, then from each
    /// mod's in load order. A mod's row whose key is in its namespace
    /// (`lanterns.oil`) is added; one with the key of a row before it
    /// changes the fields it writes, the later mod winning, and is
    /// reported as a change; any other is refused.
    pub fn load_layers(
        layers: &Layers,
        schemas: &[DefSchema],
        moves_log: &[MoveRecord],
    ) -> (Defs, Vec<ArgusError>, Vec<Change>) {
        let root = layers.game.as_path();
        let mut changes = Vec::new();
        let mut defs = Defs::default();
        let mut errors = Vec::new();
        for s in schemas {
            defs.by_prefix
                .insert(s.prefix.clone(), s.schema.name.clone());
            defs.tables.insert(
                s.schema.name.clone(),
                DefTable {
                    name: s.schema.name.clone(),
                    prefix: s.prefix.clone(),
                    key: s.key.clone(),
                    schema: s.schema.clone(),
                    file: None,
                    rows: Vec::new(),
                },
            );
        }
        let mut files = Vec::new();
        for layer in layers.each() {
            let dir = layer.root.join("defs");
            let mut here: Vec<_> = argus_os::fs::read_dir(&dir)
                .map(|rd| {
                    rd.into_iter()
                        .filter(|e| !e.is_dir)
                        .map(|e| e.path)
                        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
                        .collect()
                })
                .unwrap_or_default();
            here.sort();
            files.extend(here.into_iter().map(|p| (layer.id.map(str::to_string), p)));
        }
        for (owner, path) in files {
            let rel = moves::rel(root, &path);
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let Some(table) = defs.tables.get_mut(&name) else {
                errors.push(
                    ArgusError::new(codes::BAD_DEF, format!("{rel} has no schema"))
                        .hint(format!(
                            "Declare `def \"{name}\" {{ key = \"id\", ... }}` in a defs/ or components/ Luau file so rows can be validated."
                        ))
                        .at(Some(format!("{rel}:1"))),
                );
                continue;
            };
            if owner.is_none() || table.file.is_none() {
                table.file = Some(rel.clone());
            }
            match tomlv::read(&path) {
                Ok(text) => match &owner {
                    None => load_rows(table, &rel, &text, &mut errors),
                    Some(id) => {
                        let mut mine = table.clone();
                        mine.rows.clear();
                        load_rows(&mut mine, &rel, &text, &mut errors);
                        lay_over(table, mine.rows, id, &mut errors, &mut changes);
                    }
                },
                Err(e) => errors.push(e),
            }
        }
        // Second pass: references between tables.
        let mut ref_errors = Vec::new();
        for t in defs.tables.values() {
            for r in &t.rows {
                for (f, v) in t.schema.fields.iter().zip(&r.values) {
                    if let (FieldType::Ref, Value::Ref(s)) = (f.ty, v) {
                        if let Err(e) = defs.check_ref(s, &r.at, moves_log) {
                            ref_errors.push(e);
                        }
                    }
                }
            }
        }
        errors.extend(ref_errors);
        (defs, errors, changes)
    }

    /// A table by name.
    pub fn table(&self, name: &str) -> Option<&DefTable> {
        self.tables.get(name)
    }

    /// All tables, by name.
    pub fn tables(&self) -> impl Iterator<Item = &DefTable> {
        self.tables.values()
    }

    /// A table by reference prefix.
    pub fn table_for_prefix(&self, prefix: &str) -> Option<&DefTable> {
        self.by_prefix.get(prefix).and_then(|n| self.tables.get(n))
    }

    /// True if `r` is `prefix:key` with a known prefix.
    pub fn is_def_ref(&self, r: &str) -> bool {
        r.split_once(':')
            .map(|(p, _)| self.by_prefix.contains_key(p))
            .unwrap_or(false)
    }

    /// Resolve `prefix:key` to its table and row.
    pub fn resolve(&self, r: &str) -> Option<(&DefTable, &DefRow)> {
        let (p, key) = r.split_once(':')?;
        let t = self.table_for_prefix(p)?;
        t.get(key).map(|row| (t, row))
    }

    /// One value of a row, by reference (`item:meal`) and field (phase 4
    /// item 3: the tuning panel reads def rows as it reads components).
    pub fn value_of(&self, r: &str, field: &str) -> Result<Value> {
        let (t, row) = self.resolve(r).ok_or_else(|| no_row(r))?;
        t.value(row, field)
            .cloned()
            .ok_or_else(|| no_field(t, field))
    }

    /// Set one value of a row in memory, checked against the table's schema
    /// (its type and range, as an entity's field is). The value it had.
    /// Scripts read rows through `q:def`, so a change reaches the running
    /// game once the rows are given to the VM again.
    pub fn set_value(&mut self, r: &str, field: &str, v: Value) -> Result<Value> {
        let (p, key) = r.split_once(':').ok_or_else(|| no_row(r))?;
        let name = self.by_prefix.get(p).cloned().ok_or_else(|| no_row(r))?;
        let t = self.tables.get_mut(&name).ok_or_else(|| no_row(r))?;
        let i = t.field_index(field).ok_or_else(|| no_field(t, field))?;
        if field == t.key {
            return Err(
                ArgusError::new(codes::BAD_DEF, format!("{r}: {field} is the row's key"))
                    .hint("asset.move renames a row and rewrites every reference to it."),
            );
        }
        let fs = t.schema.fields[i].clone();
        let v = t.schema.check(&fs, v)?;
        let row = t
            .rows
            .iter_mut()
            .find(|row| row.key == key)
            .ok_or_else(|| no_row(r))?;
        let old = std::mem::replace(&mut row.values[i], v);
        row.given[i] = true;
        Ok(old)
    }

    /// A def reference must point at a row; other reference kinds pass.
    pub fn check_ref(&self, r: &str, at: &str, moves_log: &[MoveRecord]) -> Result<()> {
        if self.is_def_ref(r) && self.resolve(r).is_none() {
            return Err(moves::missing_reference("def", r, at, moves_log));
        }
        Ok(())
    }
}

/// A mod's rows over a table's: a row with a key already there writes the
/// fields it gives over the old row's; a new key must be the mod's.
fn lay_over(
    table: &mut DefTable,
    rows: Vec<DefRow>,
    id: &str,
    errors: &mut Vec<ArgusError>,
    changes: &mut Vec<Change>,
) {
    for r in rows {
        if let Some(old) = table.rows.iter_mut().find(|o| o.key == r.key) {
            changes.push(Change::new(
                "def row",
                &format!("{}:{}", table.prefix, r.key),
                id,
                &old.at,
                r.at.clone(),
            ));
            for (i, g) in r.given.iter().enumerate() {
                if *g {
                    old.values[i] = r.values[i].clone();
                    old.given[i] = true;
                }
            }
            old.at = r.at;
        } else if layers::owns(id, &r.key) {
            table.rows.push(r);
        } else {
            errors.push(layers::outside(
                id,
                &format!("{} row", table.name),
                &r.key,
                r.at,
            ));
        }
    }
}

fn load_rows(table: &mut DefTable, rel: &str, text: &str, errors: &mut Vec<ArgusError>) {
    let im = match tomlv::parse(rel, text) {
        Ok(d) => d,
        Err(e) => {
            errors.push(e);
            return;
        }
    };
    for (key, item) in im.as_table().iter() {
        let at = tomlv::item_ref(rel, text, item);
        let Some(row) = item.as_table_like() else {
            errors.push(
                ArgusError::new(
                    codes::BAD_DEF,
                    format!(
                        "{}: {key:?} must be a row table like [\"{key}\"]",
                        table.name
                    ),
                )
                .at(Some(at)),
            );
            continue;
        };
        match parse_row(table, key, row, rel, text, &at) {
            Ok(r) => table.rows.push(r),
            Err(e) => errors.push(e),
        }
    }
}

fn no_row(r: &str) -> ArgusError {
    ArgusError::new(codes::MISSING_REFERENCE, format!("no def row {r:?}"))
        .hint("A def row is prefix:key (item:meal); world.defs lists the tables and their rows.")
}

fn no_field(t: &DefTable, field: &str) -> ArgusError {
    ArgusError::new(
        argus_ecs::codes::UNKNOWN_FIELD,
        format!("{} has no field {field:?}", t.name),
    )
    .hint(format!("Fields: {}", t.columns().join(", ")))
}

/// Write one value of a def row into its table's file (phase 4 item 3: a
/// tuning session's commit, `edit.def`), keeping the file's layout and
/// comments: the row's table gets the field, or its new value. A table
/// with no file gets `defs/<name>.toml`. The project-relative path written.
pub fn write_value(
    root: &Path,
    table: &DefTable,
    key: &str,
    field: &str,
    v: &Value,
) -> Result<String> {
    let rel = table
        .file
        .clone()
        .unwrap_or_else(|| format!("defs/{}.toml", table.name));
    let path = root.join(&rel);
    let text = if path.exists() {
        tomlv::read(&path)?
    } else {
        String::new()
    };
    let mut doc = tomlv::parse_mut(&rel, &text)?;
    let fs = table
        .schema
        .fields
        .iter()
        .find(|f| f.name == field)
        .ok_or_else(|| no_field(table, field))?;
    // An f32 is written through its shortest text: 1.2, not
    // 1.2000000476837158.
    let tv = match (fs.ty, v) {
        (FieldType::F32, Value::Float(f)) => {
            let short: f64 = format!("{}", *f as f32).parse().unwrap_or(*f);
            toml_edit::Value::from(short)
        }
        _ => tomlv::from_value(v)?,
    };
    if !doc.contains_key(key) {
        doc.insert(key, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    let row = doc[key]
        .as_table_like_mut()
        .ok_or_else(|| ArgusError::new(codes::BAD_DEF, format!("{rel}: {key} is not a table")))?;
    match row.get_mut(field) {
        Some(item) if item.is_value() => {
            // The old value's comment and spacing stay with the new one.
            let decor = item.as_value().map(|x| x.decor().clone());
            let mut nv = tv;
            if let Some(d) = decor {
                *nv.decor_mut() = d;
            }
            *item = toml_edit::Item::Value(nv);
        }
        _ => {
            row.insert(field, toml_edit::Item::Value(tv));
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| ArgusError::new(codes::MISSING_FILE, format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(&path, doc.to_string())
        .map_err(|e| ArgusError::new(codes::MISSING_FILE, format!("{rel}: {e}")))?;
    Ok(rel)
}

fn parse_row(
    table: &DefTable,
    key: &str,
    row: &dyn toml_edit::TableLike,
    rel: &str,
    text: &str,
    at: &str,
) -> Result<DefRow> {
    let schema = &table.schema;
    let mut values: Vec<Option<Value>> = vec![None; schema.fields.len()];
    for (fname, item) in row.iter() {
        let fat = tomlv::item_ref(rel, text, item);
        let (i, fs) = schema
            .fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == fname)
            .ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::UNKNOWN_FIELD,
                    format!("{}.{key} has no field {fname:?}", table.name),
                )
                .hint(format!("Fields: {}", table.columns().join(", ")))
                .at(Some(fat.clone()))
            })?;
        if fs.ty == FieldType::Entity {
            return Err(ArgusError::new(
                codes::BAD_DEF,
                format!(
                    "{}.{fname} is an entity field; def rows cannot reference entities",
                    table.name
                ),
            )
            .hint("Defs are scene-independent data. Use a str or ref field.")
            .at(Some(fat)));
        }
        let raw = item.as_value().ok_or_else(|| {
            ArgusError::new(
                codes::BAD_DEF,
                format!("{}.{key}.{fname} must be a value", table.name),
            )
            .at(Some(fat.clone()))
        })?;
        let v = tomlv::to_value(raw, fs.ty, &fat)?;
        let v = schema.check(fs, v).map_err(|e| locate(e, &fat))?;
        values[i] = Some(v);
    }
    // The key field, if declared, is the header.
    if let Some(i) = table.field_index(&table.key) {
        match &values[i] {
            Some(Value::Str(s)) if s != key => {
                return Err(ArgusError::new(
                    codes::BAD_DEF,
                    format!(
                        "{}.{key}: {} = {s:?} disagrees with the row header",
                        table.name, table.key
                    ),
                )
                .hint("The row header is the key; drop the explicit field or make them equal.")
                .at(Some(at.to_string())));
            }
            _ => values[i] = Some(Value::Str(key.to_string())),
        }
    }
    let given = values.iter().map(Option::is_some).collect();
    let values = values
        .into_iter()
        .zip(&schema.fields)
        .map(|(v, f)| v.unwrap_or_else(|| f.default.clone()))
        .collect();
    Ok(DefRow {
        key: key.to_string(),
        values,
        given,
        at: at.to_string(),
    })
}

/// Add the TOML location to an error that came from the schema (whose ref
/// is the schema's own file:line, as §5 shows).
pub(crate) fn locate(e: ArgusError, at: &str) -> ArgusError {
    if e.r#ref.is_none() {
        e.at(Some(at.to_string()))
    } else {
        ArgusError {
            message: format!("{} (in {at})", e.message),
            ..e
        }
    }
}
