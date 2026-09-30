//! The editor's one connection to the daemon (§11: "a separate process
//! that speaks only the protocol"). Requests go out without waiting; each
//! answer comes back tagged with what it was for, and notifications (the
//! change feed) are handed over as they arrive.

use argus_client::client::{Conn, Incoming};
use serde_json::{json, Value as Json};
use std::collections::HashMap;

/// What an answer is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Want {
    /// `session.identify`.
    Identify,
    /// `obs.subscribe` to the change feed.
    Subscribe,
    /// `session.status`, polled for the bar.
    Status,
    /// `world.summary`, polled for the overview.
    Summary,
    /// `edit.feed`, the rows from before this editor connected.
    Feed,
    /// Something the person did, as its `argus` line.
    Action(String),
    /// `render.stream`, the viewport's frames.
    Stream,
    /// `input.device`, the viewport's keys and mouse.
    Input,
    /// `obs.frame` (semantic), to find what is at this pixel.
    Pick([f64; 2]),
    /// A page for the hierarchy.
    Hier(crate::hierarchy::Ask),
    /// `world.get {resolved: true}` of the entity inspected.
    Get(String),
    /// `world.get` of its own file's overrides.
    Overrides(String),
    /// `world.describe`: every component's schema.
    Describe,
    /// `world.instances` of a prefab: how many entities share it.
    Instances(String),
    /// `edit.locks`.
    Locks,
    /// `edit.begin` of the person's transaction: its `argus` line and ops.
    EditBegin(String, Vec<Json>),
    /// One op queued in transaction `tx`.
    EditPart(u64),
    /// `edit.abort` of a transaction one of whose ops was refused.
    EditAbort,
    /// `tune.fields`, for the tuning panel.
    TuneFields,
    /// `tune.log` of the open session: its search paths.
    TuneLog,
    /// A slider's `tune.set`.
    TuneSet,
}

/// An answer: what it was for, and its result or error.
#[derive(Clone, Debug)]
pub struct Answer {
    /// What it was for.
    pub want: Want,
    /// `Ok(result)` or `Err(error)`: the protocol's `{code, message, data:
    /// {hint, ref}}`.
    pub result: Result<Json, Json>,
}

/// The connection's state.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Not connected yet.
    Idle,
    /// Up, as this author.
    Up,
    /// Closed, and why.
    Down(String),
}

/// One connection and the requests waiting on it.
pub struct Link {
    /// The daemon's socket name.
    pub socket: String,
    conn: Option<Conn>,
    pending: HashMap<u64, Want>,
    /// The connection's state.
    pub state: State,
}

impl Link {
    /// A link to this socket, not yet connected.
    pub fn new(socket: impl Into<String>) -> Link {
        Link {
            socket: socket.into(),
            conn: None,
            pending: HashMap::new(),
            state: State::Idle,
        }
    }

    /// Connect, say who this is, and subscribe to the change feed. `wake`
    /// runs whenever a message arrives.
    pub fn connect(&mut self, author: &str, wake: impl Fn() + Send + 'static) {
        match Conn::connect(&self.socket, wake) {
            Ok(c) => {
                self.conn = Some(c);
                self.pending.clear();
                self.state = State::Up;
                self.send(
                    "session.identify",
                    json!({ "author": author }),
                    Want::Identify,
                );
                self.send(
                    "obs.subscribe",
                    json!({ "topics": ["edits"] }),
                    Want::Subscribe,
                );
                self.send("edit.feed", json!({ "limit": 200 }), Want::Feed);
            }
            Err(e) => {
                self.state = State::Down(format!("cannot connect to {}: {e}", self.socket));
            }
        }
    }

    /// True while connected.
    pub fn is_up(&self) -> bool {
        self.state == State::Up
    }

    /// Is a request of this kind still waiting for its answer?
    pub fn waiting(&self, want: &Want) -> bool {
        self.pending.values().any(|w| w == want)
    }

    /// Send a request.
    pub fn send(&mut self, method: &str, params: Json, want: Want) {
        let Some(c) = self.conn.as_mut() else {
            return;
        };
        match c.send(method, params) {
            Ok(id) => {
                self.pending.insert(id, want);
            }
            Err(e) => {
                self.conn = None;
                self.state = State::Down(format!("the connection broke: {e}"));
            }
        }
    }

    /// Everything that arrived: answers, and the notifications' params.
    pub fn poll(&mut self) -> (Vec<Answer>, Vec<(String, Json)>) {
        let (mut answers, mut notes) = (Vec::new(), Vec::new());
        let Some(c) = self.conn.as_mut() else {
            return (answers, notes);
        };
        for i in c.poll() {
            match i {
                Incoming::Response(id, msg) => {
                    let Some(want) = self.pending.remove(&id) else {
                        continue;
                    };
                    let result = match msg.get("error") {
                        Some(e) => Err(e.clone()),
                        None => Ok(msg.get("result").cloned().unwrap_or(Json::Null)),
                    };
                    answers.push(Answer { want, result });
                }
                Incoming::Notification(n) => {
                    let method = n["method"].as_str().unwrap_or_default().to_string();
                    notes.push((method, n.get("params").cloned().unwrap_or(Json::Null)));
                }
                Incoming::Closed(why) => {
                    self.state = State::Down(why);
                }
            }
        }
        if !self.is_up() {
            self.conn = None;
            self.pending.clear();
        }
        (answers, notes)
    }
}
