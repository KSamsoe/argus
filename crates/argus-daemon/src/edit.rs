//! Transactions, the change feed, undo per author, and locks (§5 "Shape of
//! an edit", "Undo is the change feed"; §10 "Locks and the change feed").
//! Applying operations to the world and the files lives in the daemon;
//! this module holds the data and the lock rule.

use crate::codes;
use argus_ecs::{ArgusError, Result};
use argus_scene::EntityDoc;
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;

/// One queued or inverse operation.
#[derive(Clone, Debug)]
pub enum Op {
    /// Write a field.
    Set {
        /// Entity ref.
        entity: String,
        /// `Component.field`.
        path: String,
        /// The value as JSON.
        value: Json,
    },
    /// Add a component with defaults.
    Add {
        /// Entity ref.
        entity: String,
        /// Component name.
        component: String,
    },
    /// Remove a component.
    Remove {
        /// Entity ref.
        entity: String,
        /// Component name.
        component: String,
    },
    /// Add a component and set its fields (the inverse of `Remove`).
    Restore {
        /// Entity ref.
        entity: String,
        /// Component name.
        component: String,
        /// Field values as JSON.
        fields: Vec<(String, Json)>,
    },
    /// Spawn an entity.
    Spawn {
        /// Authored id, or `None` for a runtime entity.
        id: Option<String>,
        /// Room for authored entities in a room scene.
        room: Option<String>,
        /// Prefab path.
        prefab: Option<String>,
        /// Tags.
        tags: Vec<String>,
        /// Components with field values.
        components: Vec<(String, Vec<(String, Json)>)>,
    },
    /// Recreate an authored entity from its former file (inverse of an
    /// authored `Despawn`).
    SpawnDoc {
        /// File stem.
        local: String,
        /// Room.
        room: Option<String>,
        /// The document as it was.
        doc: EntityDoc,
    },
    /// Despawn an entity.
    Despawn {
        /// Entity ref.
        entity: String,
    },
    /// Recreate a runtime entity's components (inverse of a runtime
    /// `Despawn`; the handle is new).
    Respawn {
        /// Tags.
        tags: Vec<String>,
        /// Components with field values.
        components: Vec<(String, Vec<(String, Json)>)>,
    },
    /// Write a rect of tiles (and the inverse of every tile edit).
    Tiles {
        /// Project-relative `*.tilemap.toml`.
        file: String,
        /// Layer name.
        layer: String,
        /// x, y, w, h.
        rect: [u32; 4],
        /// Row-major tile ids, `w * h` of them.
        cells: Vec<u32>,
    },
    /// Set a string in a language's table, or with `None` remove it (and
    /// the inverse of either).
    Text {
        /// Language code.
        lang: String,
        /// String key.
        key: String,
        /// The words, or `None` to remove the key.
        text: Option<String>,
    },
    /// Set one value of a def row (phase 4 item 3: a tuning session's
    /// commit), and the inverse of one.
    Def {
        /// The row, `prefix:key` (`item:meal`).
        def: String,
        /// The field.
        path: String,
        /// The value as JSON.
        value: Json,
    },
    /// Set one value in an asset's sidecar, or with `None` remove it
    /// (phase 4 item 8): its path in the file, `states.run.fps`.
    Asset {
        /// The asset's name (`anim/hero`).
        asset: String,
        /// Dotted keys, `[n]` for an element.
        path: String,
        /// The value as JSON, or `None` to take it out.
        value: Option<Json>,
    },
    /// A sidecar's whole text as it was: the inverse of an `Asset` op.
    AssetText {
        /// The asset's name.
        asset: String,
        /// The file's text.
        text: String,
    },
    /// Paint a terrain over a rect (or erase it) and re-pick the tiles
    /// around it by the tileset's auto-tiling rules.
    Terrain {
        /// Project-relative `*.tilemap.toml`.
        file: String,
        /// Layer name.
        layer: String,
        /// x, y, w, h.
        rect: [u32; 4],
        /// Terrain, or `None` to erase.
        terrain: Option<String>,
    },
}

impl Op {
    /// The protocol call that queues this op (`edit.set` and its params),
    /// for the ops a proposal can hold; `None` for tile edits and inverses.
    pub fn as_call(&self) -> Option<(&'static str, Json)> {
        Some(match self {
            Op::Set {
                entity,
                path,
                value,
            } => (
                "edit.set",
                json!({ "entity": entity, "path": path, "value": value }),
            ),
            Op::Add { entity, component } => (
                "edit.add",
                json!({ "entity": entity, "component": component }),
            ),
            Op::Remove { entity, component } => (
                "edit.remove",
                json!({ "entity": entity, "component": component }),
            ),
            Op::Spawn {
                id,
                room,
                prefab,
                tags,
                components,
            } => {
                let comps: serde_json::Map<String, Json> = components
                    .iter()
                    .map(|(c, fs)| {
                        let f: serde_json::Map<String, Json> =
                            fs.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                        (c.clone(), Json::Object(f))
                    })
                    .collect();
                (
                    "edit.spawn",
                    json!({ "id": id, "room": room, "prefab": prefab, "tags": tags, "components": comps }),
                )
            }
            Op::Despawn { entity } => ("edit.despawn", json!({ "entity": entity })),
            Op::Text { lang, key, text } => (
                "edit.text",
                json!({ "lang": lang, "key": key, "text": text }),
            ),
            Op::Def { def, path, value } => (
                "edit.def",
                json!({ "def": def, "path": path, "value": value }),
            ),
            Op::Asset { asset, path, value } => (
                "edit.asset",
                match value {
                    Some(v) => json!({ "asset": asset, "path": path, "value": v }),
                    None => json!({ "asset": asset, "path": path, "remove": true }),
                },
            ),
            _ => return None,
        })
    }

    /// A field write or a component added or removed: an op a world that
    /// has it already can take again (an A/B trial's picked world).
    pub fn is_field_edit(&self) -> bool {
        matches!(
            self,
            Op::Set { .. } | Op::Add { .. } | Op::Remove { .. } | Op::Def { .. }
        )
    }

    /// Compact description for the feed.
    pub fn summary(&self) -> Json {
        match self {
            Op::Set {
                entity,
                path,
                value,
            } => json!(["set", entity, path, value]),
            Op::Add { entity, component } => json!(["add", entity, component]),
            Op::Remove { entity, component } => json!(["remove", entity, component]),
            Op::Restore {
                entity, component, ..
            } => json!(["restore", entity, component]),
            Op::Spawn { id, prefab, .. } => json!(["spawn", id, prefab]),
            Op::SpawnDoc { local, room, .. } => json!(["spawn", local, room]),
            Op::Despawn { entity } => json!(["despawn", entity]),
            Op::Respawn { .. } => json!(["respawn"]),
            Op::Tiles {
                file, layer, rect, ..
            } => json!(["tiles", file, layer, rect]),
            Op::Text { lang, key, text } => json!(["text", lang, key, text]),
            Op::Def { def, path, value } => json!(["def", def, path, value]),
            Op::Asset { asset, path, value } => json!(["asset", asset, path, value]),
            Op::AssetText { asset, .. } => json!(["asset", asset, "(as it was)"]),
            Op::Terrain {
                file,
                layer,
                rect,
                terrain,
            } => json!(["terrain", file, layer, rect, terrain]),
        }
    }
}

/// A queued call as the `edit` tool's op: its params without `tx`, with
/// `op` the method's last word (`edit.set` is `{op: "set", ...}`).
pub fn tool_op(method: &str, params: &Json) -> Json {
    let mut m = match params {
        Json::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    m.remove("tx");
    let verb = method.strip_prefix("edit.").unwrap_or(method);
    m.insert("op".into(), json!(verb));
    Json::Object(m)
}

/// An open transaction.
#[derive(Clone, Debug)]
pub struct Tx {
    /// Id.
    pub id: u32,
    /// Author tag (`agent:claude`).
    pub author: String,
    /// Why.
    pub note: String,
    /// Queued operations.
    pub ops: Vec<Op>,
    /// The calls that queued them, as the `edit` tool's ops (`{op: "set",
    /// entity, path, value}`): the commit's command line (phase 4).
    pub calls: Vec<Json>,
}

/// Why a commit happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitKind {
    /// A committed transaction.
    Normal,
    /// The undo of a commit.
    Undo(u32),
    /// The redo of a commit.
    Redo(u32),
    /// A proposal made, or a variant added to one (phase 2 item 4): nothing
    /// changed yet; picking it is an ordinary commit.
    Proposed(u64),
    /// A proposal turned down.
    Rejected(u64),
    /// A call that changed something outside a transaction (phase 4): play,
    /// a seek, a slider, a mark's note. The row holds the call.
    Command,
}

/// One entry of the change feed.
#[derive(Clone, Debug)]
pub struct Commit {
    /// Id, increasing.
    pub id: u32,
    /// Author.
    pub author: String,
    /// Note.
    pub note: String,
    /// World frame at commit.
    pub frame: u64,
    /// Kind.
    pub kind: CommitKind,
    /// What was done, one summary per op.
    pub ops: Vec<Json>,
    /// Operations that undo it, in the order to apply them.
    pub inverse: Vec<Op>,
    /// Entities touched (protocol refs).
    pub touched: Vec<String>,
    /// Files written or deleted (project-relative).
    pub files: Vec<String>,
    /// The `argus` line that makes this change (phase 4, "show as
    /// command"): a commit as the `edit` tool with its ops, anything else
    /// as the call that did it. Empty for internal changes.
    pub command: String,
    /// A `Command` row's call: the method and its params.
    pub call: Option<(String, Json)>,
}

impl Commit {
    /// Feed row.
    pub fn to_json(&self) -> Json {
        let kind = match &self.kind {
            CommitKind::Normal => json!("commit"),
            CommitKind::Undo(of) => json!({ "undo": of }),
            CommitKind::Redo(of) => json!({ "redo": of }),
            CommitKind::Proposed(p) => json!({ "proposed": p }),
            CommitKind::Rejected(p) => json!({ "rejected": p }),
            CommitKind::Command => json!("command"),
        };
        let mut row = json!({
            "id": self.id,
            "author": self.author,
            "note": self.note,
            "frame": self.frame,
            "kind": kind,
            "touched": self.touched,
            "files": self.files,
            "ops": self.ops,
            "command": self.command,
        });
        if let Some((m, p)) = &self.call {
            row["method"] = json!(m);
            row["params"] = p.clone();
        }
        row
    }
}

/// A fence on entities or paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lock {
    /// Id.
    pub id: u32,
    /// Owner.
    pub author: String,
    /// Why ("camera feel is mine").
    pub note: String,
    /// Entity refs covered (empty: every entity).
    pub entities: Vec<String>,
    /// Paths covered, `Component` or `Component.field` (empty: every path).
    pub paths: Vec<String>,
}

impl Lock {
    /// Does this lock cover a write to `path` on `entity`?
    pub fn covers(&self, entity: &str, path: &str) -> bool {
        let entity_hit = self.entities.is_empty() || self.entities.iter().any(|e| e == entity);
        let path_hit = self.paths.is_empty()
            || self.paths.iter().any(|p| {
                path == p
                    || path
                        .strip_prefix(p.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            });
        entity_hit && path_hit
    }

    /// Feed row.
    pub fn to_json(&self) -> Json {
        json!({
            "id": self.id,
            "author": self.author,
            "note": self.note,
            "entities": self.entities,
            "paths": self.paths,
        })
    }
}

/// All editing state of a session.
#[derive(Debug, Default)]
pub struct Editing {
    next_tx: u32,
    next_commit: u32,
    next_lock: u32,
    /// Open transactions by id.
    pub open: BTreeMap<u32, Tx>,
    /// The change feed, oldest first.
    pub feed: Vec<Commit>,
    /// Undo stacks per author: commit ids, latest last.
    pub undo: BTreeMap<String, Vec<u32>>,
    /// Redo stacks per author.
    pub redo: BTreeMap<String, Vec<u32>>,
    /// Held locks.
    pub locks: Vec<Lock>,
}

impl Editing {
    /// Open a transaction.
    pub fn begin(&mut self, author: &str, note: &str) -> u32 {
        self.next_tx += 1;
        let id = self.next_tx;
        self.open.insert(
            id,
            Tx {
                id,
                author: author.to_string(),
                note: note.to_string(),
                ops: Vec::new(),
                calls: Vec::new(),
            },
        );
        id
    }

    /// The open transaction with this id.
    pub fn tx_mut(&mut self, id: u32) -> Result<&mut Tx> {
        self.open.get_mut(&id).ok_or_else(|| {
            ArgusError::new(codes::NO_TRANSACTION, format!("no open transaction {id}")).hint(
                "edit.begin {author, note} returns the tx to pass to edit.set and edit.commit.",
            )
        })
    }

    /// Take a transaction out (for commit or abort).
    pub fn take(&mut self, id: u32) -> Result<Tx> {
        self.open.remove(&id).ok_or_else(|| {
            ArgusError::new(codes::NO_TRANSACTION, format!("no open transaction {id}")).hint(
                "edit.begin {author, note} returns the tx to pass to edit.set and edit.commit.",
            )
        })
    }

    /// Next commit id.
    pub fn next_commit_id(&mut self) -> u32 {
        self.next_commit += 1;
        self.next_commit
    }

    /// A commit by id.
    pub fn commit(&self, id: u32) -> Option<&Commit> {
        self.feed.iter().find(|c| c.id == id)
    }

    /// Refuse a write by `author` into a path another author locked.
    pub fn check_lock(&self, author: &str, entity: &str, path: &str) -> Result<()> {
        if let Some(l) = self
            .locks
            .iter()
            .find(|l| l.author != author && l.covers(entity, path))
        {
            return Err(ArgusError::new(
                codes::LOCKED,
                format!(
                    "{path} on {entity} is locked by {} (lock {}): {}",
                    l.author, l.id, l.note
                ),
            )
            .hint("Ask the owner to edit.unlock it, or edit something else; edit.locks lists every lock."));
        }
        Ok(())
    }

    /// Take a lock.
    pub fn lock(
        &mut self,
        author: &str,
        note: &str,
        entities: Vec<String>,
        paths: Vec<String>,
    ) -> u32 {
        self.next_lock += 1;
        let id = self.next_lock;
        self.locks.push(Lock {
            id,
            author: author.to_string(),
            note: note.to_string(),
            entities,
            paths,
        });
        id
    }

    /// Release a lock you own.
    pub fn unlock(&mut self, author: &str, id: u32) -> Result<()> {
        let i = self
            .locks
            .iter()
            .position(|l| l.id == id)
            .ok_or_else(|| ArgusError::new(codes::BAD_LOCK, format!("no lock {id}")))?;
        if self.locks[i].author != author {
            return Err(ArgusError::new(
                codes::BAD_LOCK,
                format!(
                    "lock {id} belongs to {}, not {author}",
                    self.locks[i].author
                ),
            )
            .hint("Only the owner releases a lock."));
        }
        self.locks.remove(i);
        Ok(())
    }
}
