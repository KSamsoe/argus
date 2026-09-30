//! The inspector (§11: "the inspector is `world.describe` plus
//! `edit.set`"; phase 4 item 2). One entity: its components with their
//! live values, each field coloured by where its value comes from (§3:
//! "the editor's inspector shows the same provenance as a colour"): the
//! entity's own file, a prefab of its chain, or the schema's default. The
//! schema gives each field its widget, its range, and its doc. Every change
//! is a transaction by the person (§5): a field when its drag ends or its
//! text is entered, a component added or removed. Undo and redo are the
//! person's own; locks fence an entity, a component, or a field.

use eframe::egui::{self, Color32, RichText};
use serde_json::{json, Map, Value as Json};
use std::collections::{BTreeMap, HashMap};

/// What the inspector asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    /// A transaction by the person: its note and its ops (`{op, ...}` as
    /// the agent's `edit` tool has them).
    Edit(String, Vec<Json>),
    /// Lock entities or paths, with the note.
    Lock(Vec<String>, Vec<String>, String),
    /// Release a lock.
    Unlock(u64),
    /// Undo the person's last change.
    Undo,
    /// Redo it.
    Redo,
}

/// Where a value comes from, and its colour.
pub fn provenance_colour(from: &str) -> Color32 {
    match from {
        "own" => Color32::from_rgb(120, 175, 255),
        "default" => Color32::from_gray(140),
        _ => Color32::from_rgb(120, 200, 130),
    }
}

/// The inspector's state.
#[derive(Default)]
pub struct Inspector {
    /// The entity shown.
    pub entity: Option<String>,
    /// `world.get {resolved: true}` for it.
    pub got: Json,
    /// `world.get {resolved: false}`'s `overrides`: what its own file says,
    /// with the components and tags it takes away from its prefab.
    pub overrides: Json,
    /// Component schemas by name (`world.describe`).
    pub schemas: BTreeMap<String, Json>,
    /// Locks held (`edit.locks`).
    pub locks: Vec<Json>,
    /// How many entities share its prefab.
    pub instances: Option<(String, u64)>,
    /// Values a widget holds while the person changes them.
    editing: HashMap<String, Json>,
    /// Text a field holds while the person types.
    typing: HashMap<String, String>,
    add: String,
    /// The note a lock is taken with.
    pub lock_note: String,
}

impl Inspector {
    /// Show another entity.
    pub fn show(&mut self, entity: Option<String>) {
        if self.entity != entity {
            self.entity = entity;
            self.got = Json::Null;
            self.overrides = Json::Null;
            self.instances = None;
            self.editing.clear();
            self.typing.clear();
        }
    }

    /// A fresh `world.get` of the entity shown: the values are now the
    /// world's, so any the widgets held are let go.
    pub fn take(&mut self, got: Json) {
        if got["entity"].as_str().is_some()
            && self.entity.as_deref().is_some_and(|e| {
                got["entity"] == e || got["id"].as_str().is_some_and(|i| format!("@{i}") == e)
            })
        {
            self.got = got;
        }
    }

    /// The schemas from `world.describe`.
    pub fn take_schemas(&mut self, answer: &Json) {
        for r in answer["rows"].as_array().into_iter().flatten() {
            if let Some(n) = r["name"].as_str() {
                self.schemas.insert(n.to_string(), r.clone());
            }
        }
    }

    /// The prefab the entity is made from, if any.
    pub fn prefab(&self) -> Option<String> {
        self.got["resolved"]["prefab"].as_str().map(str::to_string)
    }

    /// The locks that cover this entity at `path` (`""`: the whole entity),
    /// as the daemon's `Lock::covers` has it: a lock names entities or none
    /// (all of them), and paths or none (every path); a component's path
    /// covers its fields.
    fn locks_on(&self, entity: &str, path: &str) -> Vec<Json> {
        let strs = |v: &Json| -> Vec<String> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        };
        self.locks
            .iter()
            .filter(|l| {
                let ents = strs(&l["entities"]);
                let paths = strs(&l["paths"]);
                let on_entity = ents.is_empty() || ents.iter().any(|e| e == entity);
                let on_path = paths.is_empty()
                    || paths.iter().any(|p| {
                        path == p
                            || path
                                .strip_prefix(p.as_str())
                                .is_some_and(|rest| rest.starts_with('.'))
                    });
                on_entity && on_path
            })
            .cloned()
            .collect()
    }

    /// Draw it; what the person did.
    pub fn ui(&mut self, ui: &mut egui::Ui, author: &str) -> Vec<Do> {
        let mut out = Vec::new();
        let Some(entity) = self.entity.clone() else {
            ui.weak("Select something: in the hierarchy, or click the game while it is paused (Alt and a click while it plays).");
            return out;
        };
        if self.got.is_null() {
            ui.weak(format!("{entity}: asking"));
            return out;
        }
        if let Some(e) = self.got.get("error") {
            ui.colored_label(ui.visuals().error_fg_color, e.to_string());
            return out;
        }
        let authored = self.got["authored"] == true;
        let resolved = self.got["resolved"].clone();
        ui.horizontal(|ui| {
            ui.heading(&entity);
            if ui
                .small_button("Undo")
                .on_hover_text("Your last change (Ctrl+Z); never the agent's")
                .clicked()
            {
                out.push(Do::Undo);
            }
            if ui
                .small_button("Redo")
                .on_hover_text("Ctrl+Shift+Z")
                .clicked()
            {
                out.push(Do::Redo);
            }
        });
        if authored {
            let file = resolved["file"].as_str().unwrap_or("");
            ui.weak(format!("saved in {file}"));
        } else {
            ui.weak("made at run time: changes are live only, in no file");
        }
        if let Some(p) = resolved["prefab"].as_str() {
            let chain: Vec<&str> = resolved["chain"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Json::as_str)
                .collect();
            let n = self
                .instances
                .as_ref()
                .filter(|(q, _)| q == p)
                .map(|(_, n)| match n {
                    1 => " (used by 1 entity)".to_string(),
                    n => format!(" (used by {n} entities)"),
                })
                .unwrap_or_default();
            ui.label(
                RichText::new(format!("prefab {}{n}", chain.join(" < ")))
                    .color(provenance_colour(p)),
            );
        }
        if !resolved.is_null() {
            // The colours' key: where each value comes from.
            ui.horizontal(|ui| {
                ui.weak("values from:");
                ui.label(RichText::new("its own file").color(provenance_colour("own")));
                ui.label(RichText::new("a prefab").color(provenance_colour("prefab")));
                ui.label(RichText::new("the default").color(provenance_colour("default")));
            });
        }
        let tags: Vec<String> = resolved["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                format!(
                    "{} ({})",
                    t[0].as_str().unwrap_or(""),
                    t[1].as_str().unwrap_or("")
                )
            })
            .collect();
        if !tags.is_empty() {
            ui.weak(format!("tags {}", tags.join(", ")));
        }
        let gone: Vec<String> = ["removed", "removed_tags"]
            .iter()
            .flat_map(|k| self.overrides[*k].as_array().cloned().unwrap_or_default())
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        if !gone.is_empty() {
            ui.label(
                RichText::new(format!("taken from its prefab: {}", gone.join(", ")))
                    .color(provenance_colour("own")),
            )
            .on_hover_text(
                "Its own file removes these from what the prefab gives it (-Component).",
            );
        }
        // Locks: the entity's, and taking one.
        ui.horizontal(|ui| {
            let on = self.locks_on(&entity, "");
            match on.first() {
                Some(l) => {
                    let who = l["author"].as_str().unwrap_or("?");
                    ui.colored_label(
                        Color32::from_rgb(240, 170, 60),
                        format!("locked by {who}: {}", l["note"].as_str().unwrap_or("")),
                    );
                    if who == author && ui.small_button("Unlock").clicked() {
                        out.push(Do::Unlock(l["id"].as_u64().unwrap_or(0)));
                    }
                }
                None => {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.lock_note)
                            .hint_text("why (for a lock)")
                            .desired_width(140.0),
                    );
                    if ui
                        .small_button("Lock")
                        .on_hover_text("No one else may change this entity")
                        .clicked()
                    {
                        out.push(Do::Lock(
                            vec![entity.clone()],
                            Vec::new(),
                            self.lock_note.clone(),
                        ));
                    }
                }
            }
        });
        ui.separator();
        let live: Map<String, Json> = self.got["components"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let mut order: Vec<String> = resolved["components"]
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        for c in live.keys() {
            if !order.contains(c) {
                order.push(c.clone());
            }
        }
        egui::ScrollArea::vertical()
            .id_salt("inspector")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for c in &order {
                    self.component(ui, &entity, c, &live, &resolved, author, &mut out);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let have: Vec<&String> = order.iter().collect();
                    egui::ComboBox::from_id_salt("inspector-add")
                        .selected_text(if self.add.is_empty() {
                            "a component"
                        } else {
                            self.add.as_str()
                        })
                        .show_ui(ui, |ui| {
                            for name in self.schemas.keys() {
                                if !have.contains(&name) {
                                    ui.selectable_value(&mut self.add, name.clone(), name);
                                }
                            }
                        });
                    if ui
                        .add_enabled(!self.add.is_empty(), egui::Button::new("Add"))
                        .clicked()
                    {
                        let c = std::mem::take(&mut self.add);
                        out.push(Do::Edit(
                            format!("add {c} to {entity}"),
                            vec![json!({ "op": "add", "entity": entity, "component": c })],
                        ));
                    }
                });
            });
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn component(
        &mut self,
        ui: &mut egui::Ui,
        entity: &str,
        c: &str,
        live: &Map<String, Json>,
        resolved: &Json,
        author: &str,
        out: &mut Vec<Do>,
    ) {
        let from = resolved["components"][c]["from"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let schema = self.schemas.get(c).cloned().unwrap_or(Json::Null);
        let values = live.get(c).cloned().unwrap_or(Json::Null);
        let header = if from.is_empty() {
            RichText::new(c).strong()
        } else {
            RichText::new(c).strong().color(provenance_colour(&from))
        };
        let id = ui.make_persistent_id(("component", c));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                ui.label(header).on_hover_text(match from.as_str() {
                    "" => "on this entity".to_string(),
                    "own" => "from its own file".to_string(),
                    p => format!("from the prefab {p}"),
                });
                // A lock on the whole entity is said once, above; here, the
                // ones that name this component (or one of its fields).
                let locked: Vec<Json> = self
                    .locks_on(entity, c)
                    .into_iter()
                    .filter(|l| l["paths"].as_array().is_some_and(|p| !p.is_empty()))
                    .collect();
                if let Some(l) = locked.first() {
                    let who = l["author"].as_str().unwrap_or("?");
                    ui.colored_label(Color32::from_rgb(240, 170, 60), format!("locked by {who}"));
                    if who == author && ui.small_button("Unlock").clicked() {
                        out.push(Do::Unlock(l["id"].as_u64().unwrap_or(0)));
                    }
                } else if ui.small_button("Lock").on_hover_text(format!("No one else may change {c}")).clicked() {
                    out.push(Do::Lock(vec![entity.to_string()], vec![c.to_string()], self.lock_note.clone()));
                }
                if ui.small_button("Remove").clicked() {
                    out.push(Do::Edit(
                        format!("remove {c} from {entity}"),
                        vec![json!({ "op": "remove", "entity": entity, "component": c })],
                    ));
                }
            })
            .body(|ui| {
                let fields: Vec<Json> = schema["fields"].as_array().cloned().unwrap_or_default();
                if fields.is_empty() {
                    if let Some(m) = values.as_object() {
                        for (k, v) in m {
                            ui.label(format!("{k} {v}"));
                        }
                    }
                    return;
                }
                egui::Grid::new(("fields", c)).num_columns(2).striped(true).show(ui, |ui| {
                    for f in &fields {
                        let name = f["name"].as_str().unwrap_or("?");
                        let path = format!("{c}.{name}");
                        let rf = &resolved["components"][c]["fields"][name];
                        let pfrom = rf["from"].as_str().unwrap_or(if from.is_empty() { "" } else { "default" });
                        let mut hover = String::new();
                        match pfrom {
                            "own" => hover.push_str("set in its own file"),
                            "default" => hover.push_str("the schema's default"),
                            "" => hover.push_str("on this entity"),
                            p => hover.push_str(&format!("from the prefab {p}")),
                        }
                        if let Some(at) = rf["at"].as_str() {
                            hover.push_str(&format!(" ({at})"));
                        }
                        if let Some(d) = f["doc"].as_str().filter(|d| !d.is_empty()) {
                            hover.push_str(&format!("\n{d}"));
                        }
                        if let Some(r) = f["range"].as_array() {
                            hover.push_str(&format!("\nrange {} to {}", r[0], r[1]));
                        }
                        if f["tune"] == true {
                            hover.push_str("\na tune field: the tuning panel's too");
                        }
                        let colour = if pfrom.is_empty() { ui.visuals().text_color() } else { provenance_colour(pfrom) };
                        // A lock that names this field itself.
                        let own_lock = self
                            .locks_on(entity, &path)
                            .into_iter()
                            .find(|l| l["paths"].as_array().is_some_and(|p| p.iter().any(|x| x == &json!(path))));
                        let mut text = RichText::new(name).color(colour);
                        if own_lock.is_some() {
                            text = text.underline();
                        }
                        let label = ui.label(text).on_hover_text(match &own_lock {
                            Some(l) => format!("{hover}\nlocked by {}: {}", l["author"].as_str().unwrap_or("?"), l["note"].as_str().unwrap_or("")),
                            None => format!("{hover}\nright-click to lock this field"),
                        });
                        label.context_menu(|ui| match &own_lock {
                            Some(l) if l["author"] == author => {
                                if ui.button("Unlock this field").clicked() {
                                    out.push(Do::Unlock(l["id"].as_u64().unwrap_or(0)));
                                    ui.close_menu();
                                }
                            }
                            Some(l) => {
                                ui.label(format!("locked by {}", l["author"].as_str().unwrap_or("?")));
                            }
                            None => {
                                if ui.button("Lock this field").clicked() {
                                    out.push(Do::Lock(vec![entity.to_string()], vec![path.clone()], self.lock_note.clone()));
                                    ui.close_menu();
                                }
                            }
                        });
                        let value = values.get(name).cloned().unwrap_or(Json::Null);
                        let ty = f["type"].as_str().unwrap_or("");
                        let range = f["range"].as_array().and_then(|r| Some((r.first()?.as_f64()?, r.get(1)?.as_f64()?)));
                        if let Some(v) = self.widget(ui, &path, ty, &value, range) {
                            out.push(Do::Edit(
                                format!("{path} on {entity}"),
                                vec![json!({ "op": "set", "entity": entity, "path": path, "value": v })],
                            ));
                        }
                        ui.end_row();
                    }
                });
            });
    }

    /// A field's widget by its type; the value to commit, once the person is
    /// done with it (a drag let go, text entered, a box ticked).
    fn widget(
        &mut self,
        ui: &mut egui::Ui,
        path: &str,
        ty: &str,
        value: &Json,
        range: Option<(f64, f64)>,
    ) -> Option<Json> {
        let held = self.editing.get(path).cloned();
        let now = held.clone().unwrap_or_else(|| value.clone());
        let done = |r: &egui::Response| r.drag_stopped() || r.lost_focus();
        match ty {
            "bool" => {
                let mut b = now.as_bool().unwrap_or(false);
                if ui.checkbox(&mut b, "").changed() {
                    return Some(json!(b));
                }
            }
            "f32" | "f64" => {
                let mut x = now.as_f64().unwrap_or(0.0);
                let speed = range.map_or(0.05, |(lo, hi)| ((hi - lo) / 400.0).max(0.001));
                let mut dv = egui::DragValue::new(&mut x).speed(speed).max_decimals(4);
                if let Some((lo, hi)) = range {
                    dv = dv.range(lo..=hi);
                }
                let r = ui.add(dv);
                if r.changed() {
                    self.editing.insert(path.to_string(), json!(x));
                }
                if done(&r) {
                    return self.release(path, value);
                }
            }
            "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" => {
                let mut x = now.as_i64().unwrap_or(0);
                let mut dv = egui::DragValue::new(&mut x).speed(0.1);
                let lo = if ty.starts_with('u') { 0.0 } else { f64::MIN };
                let (lo, hi) = range.unwrap_or((lo, f64::MAX));
                dv = dv.range(lo..=hi);
                let r = ui.add(dv);
                if r.changed() {
                    self.editing.insert(path.to_string(), json!(x));
                }
                if done(&r) {
                    return self.release(path, value);
                }
            }
            "vec2" => {
                let mut v = [
                    now[0].as_f64().unwrap_or(0.0),
                    now[1].as_f64().unwrap_or(0.0),
                ];
                let mut commit = false;
                ui.horizontal(|ui| {
                    for (i, x) in v.iter_mut().enumerate() {
                        let r = ui
                            .push_id(i, |ui| {
                                ui.add(egui::DragValue::new(x).speed(0.05).max_decimals(3))
                            })
                            .inner;
                        if done(&r) {
                            commit = true;
                        }
                    }
                });
                if json!(v) != now {
                    self.editing.insert(path.to_string(), json!(v));
                }
                if commit {
                    return self.release(path, value);
                }
            }
            "str" | "ref" | "entity" => {
                let text = self.typing.entry(path.to_string()).or_insert_with(|| {
                    now.as_str().map(str::to_string).unwrap_or_else(|| {
                        if now.is_null() {
                            String::new()
                        } else {
                            now.to_string()
                        }
                    })
                });
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(160.0));
                if r.lost_focus() {
                    let t = self.typing.remove(path).unwrap_or_default();
                    let v = if ty == "entity" && t.is_empty() {
                        Json::Null
                    } else {
                        json!(t)
                    };
                    if &v != value {
                        return Some(v);
                    }
                } else if !r.has_focus() {
                    // Not being typed in: follow the world.
                    self.typing.remove(path);
                }
            }
            _ => {
                ui.label(value.to_string());
            }
        }
        None
    }

    /// A widget let go: the value it holds, if it differs from the world's.
    fn release(&mut self, path: &str, value: &Json) -> Option<Json> {
        let v = self.editing.remove(path)?;
        (&v != value).then_some(v)
    }
}
