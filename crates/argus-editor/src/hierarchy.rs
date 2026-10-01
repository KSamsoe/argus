//! The hierarchy (§11: "the hierarchy is `world.query`"; phase 4 item 2).
//! It never holds the whole scene (P2): the top level is a page of the
//! entities with no `Parent`, fifty at a time with the total said, and a
//! parent's children are a query of their own, made when it is opened.
//! Which entities have children is one grouped query (`group_by
//! Parent.entity`). A filter by component or room narrows the top level.

use serde_json::{json, Value as Json};
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

/// Rows a page asks for.
pub const PAGE: u64 = 50;
/// How often the hierarchy looks again while it is shown.
const REFRESH: Duration = Duration::from_secs(2);

/// One entity in the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Its reference (`@player`, `e:42`).
    pub id: String,
    /// Its room, in a scene of rooms.
    pub room: Option<String>,
    /// What `Describe.name` calls it, if anything.
    pub name: String,
}

/// A page of rows and where the rest are.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// Rows so far.
    pub rows: Vec<Row>,
    /// How many there are in all.
    pub total: u64,
    /// The cursor for the rest.
    pub next: Option<String>,
}

/// What the hierarchy wants asked.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// The top level, `limit` rows from `cursor` (appended when a cursor).
    Top(Option<String>, u64),
    /// A parent's children.
    Children(String),
    /// Which entities have children.
    Parents,
    /// The rooms, for the filter.
    Rooms,
}

/// The hierarchy's state.
#[derive(Default)]
pub struct Hierarchy {
    /// Only entities with this component (empty: all).
    pub has: String,
    /// Only entities in this room (empty: all).
    pub room: String,
    /// The top level.
    pub top: Page,
    /// Opened parents' children.
    pub children: HashMap<String, Page>,
    /// Parents open.
    pub open: BTreeSet<String>,
    /// Entities that have children.
    pub parents: BTreeSet<String>,
    /// The scene's rooms (loaded ones first).
    pub rooms: Vec<String>,
    /// Whether the entity carries a name (`Describe`).
    pub named: bool,
    last: Option<Instant>,
    asked_rooms: bool,
}

impl Hierarchy {
    /// The query for a page of the top level.
    pub fn top_query(&self, cursor: Option<&str>, limit: u64) -> Json {
        let mut w = json!({ "not": ["Parent"] });
        if !self.has.is_empty() {
            w["has"] = json!([self.has]);
        }
        if !self.room.is_empty() {
            w["room"] = json!(self.room);
        }
        let mut q = json!({ "select": self.select(), "where": w, "limit": limit });
        if let Some(c) = cursor {
            q["cursor"] = json!(c);
        }
        q
    }

    /// The query for a parent's children.
    pub fn children_query(&self, parent: &str) -> Json {
        json!({ "select": self.select(), "where": { "Parent.entity": parent }, "limit": 200 })
    }

    /// The query for which entities have children.
    pub fn parents_query() -> Json {
        json!({ "group_by": ["Parent.entity"], "select": ["count()"], "limit": 1000 })
    }

    fn select(&self) -> Json {
        if self.named {
            json!(["id", "room", "Describe.name"])
        } else {
            json!(["id", "room"])
        }
    }

    /// What is due to be asked: everything at first, then the pages shown,
    /// again every two seconds (or at once, after a change).
    pub fn due(&mut self, now: Instant) -> Vec<Ask> {
        let mut out = Vec::new();
        if !self.asked_rooms {
            self.asked_rooms = true;
            out.push(Ask::Rooms);
        }
        if self.last.is_some_and(|t| now.duration_since(t) < REFRESH) {
            return out;
        }
        self.last = Some(now);
        let shown = (self.top.rows.len() as u64).clamp(PAGE, 500);
        out.push(Ask::Top(None, shown));
        out.push(Ask::Parents);
        for p in &self.open {
            out.push(Ask::Children(p.clone()));
        }
        out
    }

    /// Look again at the next frame (a spawn, a despawn, a filter).
    pub fn stale(&mut self) {
        self.last = None;
    }

    /// A page of rows from `world.query`.
    pub fn page(answer: &Json) -> Page {
        let rows = answer["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| Row {
                id: r[0].as_str().unwrap_or("?").to_string(),
                room: r[1].as_str().map(str::to_string),
                name: r[2].as_str().unwrap_or("").to_string(),
            })
            .collect();
        Page {
            rows,
            total: answer["total"].as_u64().unwrap_or(0),
            next: answer["next"].as_str().map(str::to_string),
        }
    }

    /// An answer to one of its asks.
    pub fn take(&mut self, ask: &Ask, answer: &Json) {
        match ask {
            Ask::Top(Some(_), _) => {
                let mut p = Self::page(answer);
                self.top.rows.append(&mut p.rows);
                self.top.total = p.total;
                self.top.next = p.next;
            }
            Ask::Top(None, _) => self.top = Self::page(answer),
            Ask::Children(parent) => {
                self.children.insert(parent.clone(), Self::page(answer));
            }
            Ask::Parents => {
                self.parents = answer["rows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r[0].as_str().map(str::to_string))
                    .collect();
            }
            Ask::Rooms => {
                let mut loaded = Vec::new();
                let mut rest = Vec::new();
                for r in answer["rooms"].as_array().into_iter().flatten() {
                    let name = r["name"].as_str().unwrap_or_default().to_string();
                    if name.is_empty() {
                        continue;
                    }
                    if r["loaded"] == true {
                        loaded.push(name);
                    } else {
                        rest.push(name);
                    }
                }
                loaded.extend(rest);
                self.rooms = loaded;
            }
        }
    }

    /// Draw it. Returns the entity clicked, and what to ask now.
    pub fn ui(
        &mut self,
        ui: &mut eframe::egui::Ui,
        selected: Option<&str>,
        components: &[String],
    ) -> (Option<String>, Vec<Ask>) {
        use eframe::egui;
        let mut picked = None;
        let mut asks = Vec::new();
        ui.horizontal(|ui| {
            ui.strong("Hierarchy");
            ui.weak(format!("{} at the top", self.top.total));
        });
        ui.horizontal(|ui| {
            let before = (self.has.clone(), self.room.clone());
            egui::ComboBox::from_id_salt("hier-has")
                .selected_text(if self.has.is_empty() {
                    "any component"
                } else {
                    self.has.as_str()
                })
                .width(130.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.has, String::new(), "any component");
                    for c in components {
                        ui.selectable_value(&mut self.has, c.clone(), c);
                    }
                })
                .response
                .widget_info(|| named_combo("Only entities with"));
            if !self.rooms.is_empty() {
                egui::ComboBox::from_id_salt("hier-room")
                    .selected_text(if self.room.is_empty() {
                        "any room"
                    } else {
                        self.room.as_str()
                    })
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.room, String::new(), "any room");
                        for r in &self.rooms {
                            ui.selectable_value(&mut self.room, r.clone(), r);
                        }
                    })
                    .response
                    .widget_info(|| named_combo("Only the room"));
            }
            if (self.has.clone(), self.room.clone()) != before {
                self.top = Page::default();
                self.stale();
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("hierarchy")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let rows = self.top.rows.clone();
                for r in &rows {
                    self.row(ui, r, 0, selected, &mut picked, &mut asks);
                }
                if let Some(c) = self.top.next.clone() {
                    let left = self.top.total.saturating_sub(self.top.rows.len() as u64);
                    if ui.small_button(format!("{left} more")).clicked() {
                        asks.push(Ask::Top(Some(c), PAGE));
                    }
                }
            });
        (picked, asks)
    }

    fn row(
        &mut self,
        ui: &mut eframe::egui::Ui,
        r: &Row,
        depth: usize,
        selected: Option<&str>,
        picked: &mut Option<String>,
        asks: &mut Vec<Ask>,
    ) {
        use eframe::egui;
        let has_children = self.parents.contains(&r.id);
        let open = self.open.contains(&r.id);
        ui.push_id(&r.id, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                if has_children {
                    let arrow = if open { "v" } else { ">" };
                    if ui
                        .small_button(arrow)
                        .on_hover_text("Its children")
                        .clicked()
                    {
                        if open {
                            self.open.remove(&r.id);
                        } else {
                            self.open.insert(r.id.clone());
                            asks.push(Ask::Children(r.id.clone()));
                        }
                    }
                } else {
                    ui.add_space(18.0);
                }
                let mut text = egui::RichText::new(&r.id);
                if selected == Some(r.id.as_str()) {
                    text = text.strong();
                }
                let resp = ui.selectable_label(selected == Some(r.id.as_str()), text);
                if !r.name.is_empty() {
                    ui.weak(&r.name);
                }
                if let Some(room) = &r.room {
                    ui.weak(room);
                }
                if resp.clicked() {
                    *picked = Some(r.id.clone());
                }
            });
        });
        if open {
            if let Some(kids) = self.children.get(&r.id).cloned() {
                for k in &kids.rows {
                    self.row(ui, k, depth + 1, selected, picked, asks);
                }
                if kids.total > kids.rows.len() as u64 {
                    ui.weak(format!(
                        "{:indent$}and {} more",
                        "",
                        kids.total - kids.rows.len() as u64,
                        indent = (depth + 1) * 3
                    ));
                }
            }
        }
    }
}

/// A list's accessible name: its words say what it filters, not what it
/// holds now (a screen reader's, and the tests' way to it).
fn named_combo(name: &str) -> eframe::egui::WidgetInfo {
    eframe::egui::WidgetInfo::labeled(eframe::egui::WidgetType::ComboBox, true, name)
}
