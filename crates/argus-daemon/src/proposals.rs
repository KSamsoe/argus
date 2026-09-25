//! Proposals and A/B by feel (§10, phase 2 item 4). The agent submits a
//! transaction as a proposal instead of committing it (`edit.propose`);
//! more transactions into the same proposal are its other variants. The
//! human plays and presses F9: each variant becomes a fork of the world at
//! that moment with its edits applied, and from then on every world takes
//! the same input at the same tick, so F9 switches between them at the same
//! point: A (the game as it is), B, C. F10 keeps the one showing. Keeping a
//! variant commits it, an ordinary commit in the feed that undo takes back;
//! keeping A turns the proposal down. Nobody reads a diff.
//!
//! A proposal is `.argus/proposals/<id>.json`: its author, note, scene,
//! variants (each the edit calls that make it and their summary), and what
//! became of it, with how often the human switched. It is kept on disk
//! because the agent proposes while the human is away, and the game the
//! human comes back to is another daemon.
//!
//! The worlds of a trial run in step: the one showing runs as the game
//! does; the others catch up behind it every frame (`sync_trial`), taking
//! the showing world's new input commands as their recorded future, with no
//! sound and no notifications. A seek or a scene load ends a trial.

use super::*;
use argus_input::JournalEntry;
use std::time::{Duration, Instant};

/// Variant labels after A, the world as it is.
const LABELS: &[u8] = b"BCDEFGHIJKLMNOPQRSTUVWXYZ";

/// A proposal being tried: its worlds and which one shows.
pub(super) struct Trial {
    /// The proposal.
    pub id: u64,
    /// Label and world name: "A" (the world as it was), then each variant.
    pub worlds: Vec<(String, String)>,
    /// The world showing, by index.
    pub showing: usize,
    /// Switches so far.
    pub toggles: u64,
    /// Each switch: the world shown and the tick, for the proposal's file
    /// (each world's event log holds only the switches made while it showed).
    pub shown: Vec<(String, u64)>,
}

fn dir(root: &Path) -> PathBuf {
    root.join(".argus").join("proposals")
}

fn file(root: &Path, id: u64) -> PathBuf {
    dir(root).join(format!("{id}.json"))
}

/// Proposal ids on disk, oldest first.
fn ids(root: &Path) -> Vec<u64> {
    let mut out: Vec<u64> = std::fs::read_dir(dir(root))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json")?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    out.sort_unstable();
    out
}

fn read(root: &Path, id: u64) -> Result<Json> {
    let text = std::fs::read_to_string(file(root, id)).map_err(|_| {
        ArgusError::new(codes::NO_PROPOSAL, format!("there is no proposal {id}"))
            .hint("proposal.list shows the proposals in .argus/proposals/.")
    })?;
    serde_json::from_str(&text)
        .map_err(|e| ArgusError::new(codes::BAD_PROPOSAL, format!("proposal {id}: {e}")))
}

fn write(root: &Path, p: &Json) -> Result<()> {
    let id = p["id"].as_u64().unwrap_or(0);
    std::fs::create_dir_all(dir(root))
        .and_then(|_| {
            std::fs::write(
                file(root, id),
                serde_json::to_string_pretty(p).expect("json") + "\n",
            )
        })
        .map_err(|e| ArgusError::new(codes::BAD_PROPOSAL, format!("proposal {id}: {e}")))
}

fn proposal_id(params: &Json) -> Result<u64> {
    match params.get("id") {
        Some(Json::Number(n)) => n.as_u64(),
        Some(Json::String(s)) => s.trim_start_matches('#').parse().ok(),
        _ => None,
    }
    .ok_or_else(|| bad_params("give id: the proposal's number, as proposal.list shows it"))
}

fn pending(id: u64, p: &Json) -> Result<()> {
    if p["status"] != "pending" {
        return Err(ArgusError::new(
            codes::BAD_PROPOSAL,
            format!(
                "proposal {id} is {}, not pending",
                p["status"].as_str().unwrap_or("?")
            ),
        ));
    }
    Ok(())
}

/// The oldest pending proposal.
fn first_pending(root: &Path) -> Option<Json> {
    ids(root)
        .into_iter()
        .filter_map(|id| read(root, id).ok())
        .find(|p| p["status"] == "pending")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A proposal as a list row.
fn row(p: &Json) -> Json {
    json!({
        "id": p["id"],
        "status": p["status"],
        "author": p["author"],
        "note": p["note"],
        "scene": p["scene"],
        "variants": p["variants"].as_array().map(|vs| vs.iter().map(|v| json!({
            "label": v["label"], "note": v["note"], "ops": v["ops"],
        })).collect::<Vec<_>>()),
        "picked": p["picked"],
        "made_at": p["made_at"],
    })
}

impl Daemon {
    /// `edit.propose`: the transaction as a proposal, or as another variant
    /// of one; nothing changes until it is picked. It is checked by
    /// applying it to a fork of the world now, which is then dropped.
    pub(super) fn edit_propose(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let tx = json::u64_param(params, "tx", 0)? as u32;
        let t = self.edits.take(tx)?;
        let calls: Vec<Json> = t
            .ops
            .iter()
            .map(|op| op.as_call().map(|(m, p)| json!([m, p])))
            .collect::<Option<_>>()
            .ok_or_else(|| {
                ArgusError::new(
                    codes::BAD_PROPOSAL,
                    "a proposal holds entity edits (set, add, remove, spawn, despawn); commit tile edits",
                )
            })?;
        if calls.is_empty() {
            return Err(
                ArgusError::new(codes::BAD_PROPOSAL, "the transaction is empty")
                    .hint("Queue edits with edit.set and the like before edit.propose."),
            );
        }
        let summaries: Vec<Json> = t.ops.iter().map(|o| o.summary()).collect();
        // Does it apply? On a fork of now, dropped after.
        let scratch = "proposal.check".to_string();
        if self.forks.contains_key(&scratch) {
            self.forks.remove(&scratch);
        }
        self.sim_fork(&json!({ "name": scratch }))?;
        let checked = self.preview(&scratch, &t.author, &calls);
        self.forks.remove(&scratch);
        checked.map_err(|e| {
            ArgusError::new(
                codes::BAD_PROPOSAL,
                format!("the proposal does not apply: {}", e.message),
            )
            .hint(e.hint)
        })?;

        let scene = self.session.as_ref().expect("session").scene_name.clone();
        let (id, mut p) = match params.get("proposal").filter(|v| !v.is_null()) {
            Some(v) => {
                let id = v
                    .as_u64()
                    .ok_or_else(|| bad_params("proposal must be a proposal's number"))?;
                let p = read(&root, id)?;
                pending(id, &p)?;
                (id, p)
            }
            None => {
                let id = ids(&root).last().map_or(1, |i| i + 1);
                let note = json::opt_str(params, "note")?
                    .unwrap_or(&t.note)
                    .to_string();
                (
                    id,
                    json!({
                        "id": id, "status": "pending", "author": t.author, "note": note,
                        "scene": scene, "made_at": unix_now(), "variants": [],
                    }),
                )
            }
        };
        let n = p["variants"].as_array().map_or(0, Vec::len);
        let label = LABELS
            .get(n)
            .map(|b| (*b as char).to_string())
            .ok_or_else(|| {
                ArgusError::new(codes::BAD_PROPOSAL, "a proposal holds at most 25 variants")
            })?;
        p["variants"].as_array_mut().expect("variants").push(json!({
            "label": label, "note": t.note, "author": t.author,
            "calls": calls, "ops": summaries,
        }));
        write(&root, &p)?;
        let touched: Vec<String> = summaries
            .iter()
            .filter_map(|s| s.get(1).and_then(Json::as_str).map(str::to_string))
            .collect();
        let note = format!("proposal {id} {label}: {}", t.note);
        let feed = self.feed_note(
            &t.author,
            &note,
            CommitKind::Proposed(id),
            summaries,
            touched,
        );
        self.emit_proposal("proposal.made", id, &label, &t.note);
        Ok(json!({ "proposal": id, "variant": label, "feed": feed, "summary": row(&p) }))
    }

    /// A feed row that changed nothing: a proposal made or turned down.
    fn feed_note(
        &mut self,
        author: &str,
        note: &str,
        kind: CommitKind,
        ops: Vec<Json>,
        touched: Vec<String>,
    ) -> Json {
        let id = self.edits.next_commit_id();
        let c = Commit {
            id,
            author: author.to_string(),
            note: note.to_string(),
            frame: self.world.frame(),
            kind,
            ops,
            inverse: Vec::new(),
            touched,
            files: Vec::new(),
        };
        let r = c.to_json();
        self.edits.feed.push(c);
        self.notify("edits", "edit.committed", r.clone());
        r
    }

    fn emit_proposal(&mut self, name: &str, id: u64, variant: &str, note: &str) {
        self.world.emit(Event::Game {
            name: name.to_string(),
            fields: vec![
                ("id".into(), Value::UInt(id)),
                ("variant".into(), Value::Str(variant.to_string())),
                ("note".into(), Value::Str(note.to_string())),
            ],
        });
        self.pump_events();
    }

    /// Run `f` against a parked world, swapped in for it.
    fn in_world<T>(&mut self, name: &str, f: impl FnOnce(&mut Daemon) -> Result<T>) -> Result<T> {
        let mut slot = self
            .forks
            .remove(name)
            .ok_or_else(|| self.unknown_fork(name))?;
        self.swap_slot(&mut slot);
        let was = std::mem::replace(&mut self.active, name.to_string());
        let r = f(self);
        self.active = was;
        self.swap_slot(&mut slot);
        self.forks.insert(name.to_string(), slot);
        r
    }

    /// The edit calls of a variant as ops, through the same queueing a
    /// transaction's edits take.
    fn ops_of(&mut self, author: &str, calls: &Json) -> Result<Vec<Op>> {
        let tx = self.edits.begin(author, "proposal");
        for c in calls.as_array().into_iter().flatten() {
            let method = c[0].as_str().unwrap_or_default();
            let mut p = c[1].clone();
            p["tx"] = json!(tx);
            if let Err(e) = self.edit_queue(method, &p) {
                let _ = self.edits.take(tx);
                return Err(e);
            }
        }
        Ok(self.edits.take(tx)?.ops)
    }

    /// Apply a variant to a parked world, in memory only (no file is
    /// written, no lock applies: it is a preview), and take a keyframe of
    /// the change so a replay never crosses it.
    fn preview(&mut self, name: &str, author: &str, calls: &[Json]) -> Result<()> {
        let ops = self.ops_of(author, &Json::Array(calls.to_vec()))?;
        self.in_world(name, |d| {
            let locks = std::mem::take(&mut d.edits.locks);
            d.live_edits = true;
            let mut r = Ok(());
            for op in &ops {
                if let Err(e) = d.apply_op(author, op, &mut BTreeSet::new(), &mut BTreeSet::new()) {
                    r = Err(e);
                    break;
                }
            }
            d.live_edits = false;
            d.edits.locks = locks;
            let _ = d.world.drain_events();
            let s = d.session.as_ref().expect("session");
            let k = s.sources.capture(&d.world, &d.sched, true);
            let mut ring = s.ring.borrow_mut();
            ring.push(k);
            ring.mutations = d.mutations;
            r
        })
    }

    /// `proposal.list`: pending proposals, oldest first (`all` adds the rest).
    pub(super) fn proposal_list(&self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let all = json::bool_param(params, "all", false)?;
        let mut rows = Vec::new();
        let mut pending_n = 0;
        for id in ids(&root) {
            let Ok(p) = read(&root, id) else { continue };
            if p["status"] == "pending" {
                pending_n += 1;
            }
            if all || p["status"] == "pending" {
                rows.push(row(&p));
            }
        }
        Ok(json!({ "pending": pending_n, "rows": rows, "trial": self.trial_json() }))
    }

    /// `proposal.get`: all of it.
    pub(super) fn proposal_get(&self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        read(&root, proposal_id(params)?)
    }

    fn trial_json(&self) -> Json {
        match &self.trial {
            None => Json::Null,
            Some(t) => json!({
                "proposal": t.id,
                "showing": t.worlds[t.showing].0,
                "worlds": t.worlds.iter().map(|(l, n)| json!({ "label": l, "world": n })).collect::<Vec<_>>(),
                "toggles": t.toggles,
            }),
        }
    }

    /// `proposal.try`: a fork of the world now for each variant, its edits
    /// applied; A stays the world in use. Ends any trial before.
    pub(super) fn proposal_try(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = proposal_id(params)?;
        self.start_trial(&root, id)?;
        Ok(json!({ "trial": self.trial_json() }))
    }

    fn start_trial(&mut self, root: &Path, id: u64) -> Result<()> {
        let p = read(root, id)?;
        pending(id, &p)?;
        let scene = self.session.as_ref().expect("session").scene_name.clone();
        if p["scene"] != json!(scene) {
            return Err(ArgusError::new(
                codes::BAD_PROPOSAL,
                format!(
                    "proposal {id} is for scene {}, and {scene} is loaded",
                    p["scene"]
                ),
            )
            .hint("scene.load it first."));
        }
        self.end_trial(true);
        let mut worlds = vec![("A".to_string(), self.active.clone())];
        for v in p["variants"].as_array().into_iter().flatten() {
            let label = v["label"].as_str().unwrap_or("?").to_string();
            let name = format!("proposal{id}{label}");
            self.forks.remove(&name);
            let made = self.sim_fork(&json!({ "name": name })).and_then(|_| {
                let calls = v["calls"].as_array().cloned().unwrap_or_default();
                self.preview(&name, v["author"].as_str().unwrap_or("agent"), &calls)
            });
            if let Err(e) = made {
                for (_, n) in &worlds[1..] {
                    self.forks.remove(n);
                }
                self.forks.remove(&name);
                return Err(e);
            }
            worlds.push((label, name));
        }
        self.trial = Some(Trial {
            id,
            worlds,
            showing: 0,
            toggles: 0,
            shown: Vec::new(),
        });
        self.emit_proposal("proposal.tried", id, "A", p["note"].as_str().unwrap_or(""));
        Ok(())
    }

    /// Bring every parked world of the trial to the tick of the one showing,
    /// with the same input: its new commands become theirs, and they step,
    /// quietly for the ears (no sound, no notifications).
    pub(super) fn sync_trial(&mut self) -> Result<()> {
        let Some(t) = &self.trial else {
            return Ok(());
        };
        let parked: Vec<String> = t
            .worlds
            .iter()
            .map(|(_, n)| n.clone())
            .filter(|n| *n != self.active)
            .collect();
        if parked.iter().any(|n| !self.forks.contains_key(n)) {
            // A world of the trial went (sim.fork drop): the trial is over.
            self.trial = None;
            return Ok(());
        }
        let target = self.sched.tick();
        let discrete = self.sched.driving().kind == ClockKind::Discrete;
        let input = self.session.as_ref().and_then(|s| s.input.clone());
        for name in parked {
            let have = self.forks[&name]
                .session
                .as_ref()
                .and_then(|s| s.input.as_ref())
                .map(|i| i.borrow().journal().len());
            let more: Vec<JournalEntry> = match (&input, have) {
                (Some(i), Some(have)) => i
                    .borrow()
                    .journal()
                    .get(have..)
                    .map(<[_]>::to_vec)
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            self.in_world(&name, |d| {
                if let Some(i) = d.session.as_ref().and_then(|s| s.input.clone()) {
                    i.borrow_mut().extend_journal(&more);
                }
                let n = target.saturating_sub(d.sched.tick());
                if n > 0 {
                    d.before_ticks();
                    d.backstage = true;
                    let r = if discrete {
                        d.sched.advance(&mut d.world, n).map(|_| ())
                    } else {
                        d.sched.step(&mut d.world, n).map(|_| ())
                    };
                    d.pump_ticks();
                    d.backstage = false;
                    r?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// `proposal.show {variant}`: make that world of the trial the one in
    /// use (what F9 does in the window).
    pub(super) fn proposal_show(&mut self, params: &Json) -> Result<Json> {
        let label = json::str_param(params, "variant")?.to_uppercase();
        let idx = self
            .trial
            .as_ref()
            .ok_or_else(|| {
                ArgusError::new(codes::BAD_PROPOSAL, "no proposal is being tried")
                    .hint("proposal.try {id} starts one.")
            })?
            .worlds
            .iter()
            .position(|(l, _)| *l == label)
            .ok_or_else(|| bad_params(format!("no variant {label} in this trial")))?;
        self.show_world(idx)?;
        Ok(json!({ "trial": self.trial_json() }))
    }

    /// Make `name` the world in use, carrying the play state over: a
    /// trial's worlds are one game to the human, so the one switched in
    /// plays, or waits paused, at the speed the one before it did. (A fork's
    /// own clock starts stopped, and a variant switched in that way stood
    /// still until A came back, found in the phase 2 gate.)
    fn switch_to(&mut self, name: &str) -> Result<()> {
        if name == self.active {
            return Ok(());
        }
        let (playing, paused, speed) = (
            self.sched.is_playing(),
            self.sched.is_paused(),
            self.sched.speed(),
        );
        self.sim_fork(&json!({ "name": name, "switch": true }))?;
        if playing {
            self.sched.play(&self.world);
            if paused {
                self.sched.pause();
            }
            if self.sched.driving().kind == ClockKind::Continuous {
                self.sched.set_speed(speed)?;
            }
        }
        Ok(())
    }

    fn show_world(&mut self, idx: usize) -> Result<()> {
        self.sync_trial()?;
        let Some(t) = &self.trial else {
            return Ok(());
        };
        let (label, name) = t.worlds[idx].clone();
        let id = t.id;
        if name != self.active {
            self.switch_to(&name)?;
            let now = self.sched.tick();
            if let Some(i) = self.session.as_ref().and_then(|s| s.input.clone()) {
                i.borrow_mut().issue_due(now);
            }
        }
        let tick = self.sched.tick();
        if let Some(t) = self.trial.as_mut() {
            t.showing = idx;
            t.toggles += 1;
            t.shown.push((label.clone(), tick));
        }
        self.emit_proposal("proposal.shown", id, &label, "");
        Ok(())
    }

    /// End the trial: the world showing stays (`keep_a`: A comes back
    /// first) and the others go; the world in use is "main" again.
    pub(super) fn end_trial(&mut self, keep_a: bool) {
        let Some(t) = self.trial.take() else {
            return;
        };
        if keep_a {
            let _ = self.switch_to(&t.worlds[0].1);
        }
        for (_, n) in &t.worlds {
            if *n != self.active {
                self.forks.remove(n);
            }
        }
        if self.active != "main" && !self.forks.contains_key("main") {
            self.active = "main".to_string();
        }
    }

    /// `proposal.stop`: stop trying, back to A; the proposal waits on.
    pub(super) fn proposal_stop(&mut self, _params: &Json) -> Result<Json> {
        self.sync_trial()?;
        self.end_trial(true);
        Ok(json!({ "trial": Json::Null }))
    }

    /// `proposal.pick {id, variant}`: commit that variant, an ordinary
    /// commit by the one who picked (so their own locks do not stand in
    /// the way; another's do). In a trial, the game goes on in the world
    /// that was picked when its edits are field writes, else in A.
    pub(super) fn proposal_pick(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = proposal_id(params)?;
        let label = json::str_param(params, "variant")?.to_uppercase();
        let by = json::opt_str(params, "author")?
            .unwrap_or("human")
            .to_string();
        if label == "A" {
            return self.reject(
                &root,
                id,
                &by,
                json::opt_str(params, "note")?.unwrap_or("kept as it is"),
            );
        }
        let mut p = read(&root, id)?;
        pending(id, &p)?;
        let v = p["variants"]
            .as_array()
            .and_then(|vs| vs.iter().find(|v| v["label"] == json!(label)))
            .cloned()
            .ok_or_else(|| bad_params(format!("proposal {id} has no variant {label}")))?;
        let ops = self.ops_of(&by, &v["calls"])?;
        let field_edits = ops.iter().all(Op::is_field_edit);
        // In a trial: commit in A, where the files and undo belong.
        let mut go_on_in = None;
        let mut toggles = 0;
        let mut shown = Vec::new();
        if let Some(t) = &self.trial {
            if t.id == id {
                toggles = t.toggles;
                shown = t.shown.clone();
                self.sync_trial()?;
                let t = self.trial.as_ref().expect("still trying");
                let a = t.worlds[0].1.clone();
                if field_edits {
                    go_on_in = t
                        .worlds
                        .iter()
                        .find(|(l, _)| *l == label)
                        .map(|(_, n)| n.clone());
                }
                self.switch_to(&a)?;
            }
        }
        let note = format!(
            "proposal {id} {label} picked by {by}: {}",
            v["note"]
                .as_str()
                .filter(|n| !n.is_empty())
                .unwrap_or(p["note"].as_str().unwrap_or(""))
        );
        let commit = self.apply_commit(&by, &note, ops, CommitKind::Normal)?;
        if let Some(name) = go_on_in {
            if self.forks.contains_key(&name) {
                self.switch_to(&name)?;
            }
        }
        self.end_trial(false);
        p["status"] = json!("picked");
        p["picked"] = json!({ "variant": label, "by": by, "at": unix_now(), "commit": commit["id"], "toggles": toggles, "shown": shown });
        write(&root, &p)?;
        self.emit_proposal(
            "proposal.picked",
            id,
            &label,
            v["note"].as_str().unwrap_or(""),
        );
        Ok(json!({ "proposal": row(&p), "commit": commit }))
    }

    /// `proposal.reject {id}`: turn it down; the world is as it was.
    pub(super) fn proposal_reject(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = proposal_id(params)?;
        let by = json::opt_str(params, "author")?
            .unwrap_or("human")
            .to_string();
        let note = json::opt_str(params, "note")?.unwrap_or("").to_string();
        self.reject(&root, id, &by, &note)
    }

    fn reject(&mut self, root: &Path, id: u64, by: &str, note: &str) -> Result<Json> {
        let mut p = read(root, id)?;
        pending(id, &p)?;
        let mut toggles = 0;
        let mut shown = Vec::new();
        if let Some(t) = self.trial.as_ref().filter(|t| t.id == id) {
            toggles = t.toggles;
            shown = t.shown.clone();
            self.sync_trial()?;
            self.end_trial(true);
        }
        p["status"] = json!("rejected");
        p["rejected"] =
            json!({ "by": by, "at": unix_now(), "note": note, "toggles": toggles, "shown": shown });
        write(root, &p)?;
        let feed_note = format!("proposal {id} turned down by {by}: {note}");
        self.feed_note(
            by,
            &feed_note,
            CommitKind::Rejected(id),
            Vec::new(),
            Vec::new(),
        );
        self.emit_proposal("proposal.rejected", id, "A", note);
        Ok(json!({ "proposal": row(&p) }))
    }

    /// F9 and F10 in the window, before the game hears them: F9 tries the
    /// oldest pending proposal, then switches to its next world (A, B, ...);
    /// F10 keeps the one showing (A turns the proposal down).
    pub(super) fn trial_keys(
        &mut self,
        evs: Vec<argus_input::DeviceInput>,
    ) -> Vec<argus_input::DeviceInput> {
        use argus_input::DeviceInput;
        let down = |k: &str| {
            evs.iter()
                .any(|e| matches!(e, DeviceInput::Key { name, pressed: true } if name == k))
        };
        let (next, keep) = (down("F9"), down("F10"));
        let rest: Vec<DeviceInput> = evs
            .iter()
            .filter(
                |e| !matches!(e, DeviceInput::Key { name, .. } if name == "F9" || name == "F10"),
            )
            .cloned()
            .collect();
        if !next && !keep {
            return rest;
        }
        let Some(root) = self.session.as_ref().map(|s| s.root.clone()) else {
            return rest;
        };
        // What came before the key in this batch happened before the switch.
        if !rest.is_empty() {
            self.device_input(&rest);
        }
        // The bar shows the trial at once, not an older message.
        self.toast = None;
        let said = if next {
            match &self.trial {
                Some(t) => {
                    let i = (t.showing + 1) % t.worlds.len();
                    self.show_world(i).err().map(|e| e.message)
                }
                None => match first_pending(&root) {
                    Some(p) => {
                        let id = p["id"].as_u64().unwrap_or(0);
                        self.start_trial(&root, id)
                            .and_then(|_| self.show_world(1))
                            .err()
                            .map(|e| e.message)
                    }
                    None => Some("NO PROPOSALS WAITING".to_string()),
                },
            }
        } else {
            match &self.trial {
                None => Some("NOTHING TO KEEP: F9 TRIES A PROPOSAL".to_string()),
                Some(t) => {
                    let (id, label) = (t.id, t.worlds[t.showing].0.clone());
                    match self.proposal_pick(&json!({ "id": id, "variant": label })) {
                        Ok(_) if label == "A" => Some(format!("KEPT A: PROPOSAL {id} TURNED DOWN")),
                        Ok(_) => Some(format!("KEPT {label}: PROPOSAL {id} COMMITTED")),
                        Err(e) => Some(e.message),
                    }
                }
            }
        };
        if let Some(text) = said {
            self.toast = Some((text, Instant::now() + Duration::from_millis(3000)));
        }
        vec![argus_input::DeviceInput::ReleaseAll]
    }

    /// The bar while a proposal is tried. Its first line is which world
    /// shows and the keys, which the bar never cuts; then what this world
    /// is (its variant's note, when it says more than the proposal's) and
    /// what the proposal is for.
    pub(super) fn trial_bar(&self) -> Option<String> {
        let t = self.trial.as_ref()?;
        let root = &self.session.as_ref()?.root;
        let p = read(root, t.id).ok()?;
        let (label, _) = &t.worlds[t.showing];
        let note = p["note"].as_str().unwrap_or("");
        let last = t.worlds.last().map(|(l, _)| l.as_str()).unwrap_or("B");
        let mut s = format!("PROPOSAL {}: {label} OF A-{last}", t.id);
        if label == "A" {
            s.push_str(" (AS IT IS)");
        }
        s.push_str("   F9 NEXT  F10 KEEP THIS\n");
        let what = p["variants"]
            .as_array()?
            .iter()
            .find(|v| v["label"] == json!(label))
            .and_then(|v| v["note"].as_str())
            .filter(|w| !w.is_empty() && *w != note);
        if let Some(w) = what {
            s.push_str(w);
            s.push_str("   ");
        }
        s.push_str(note);
        Some(s.trim_end().to_string())
    }

    /// Pending proposals, for the line the window shows when it opens.
    pub(super) fn waiting_line(&self) -> Option<String> {
        let root = &self.session.as_ref()?.root;
        let n = ids(root)
            .into_iter()
            .filter(|&id| read(root, id).is_ok_and(|p| p["status"] == "pending"))
            .count();
        (n > 0).then(|| {
            format!(
                "{n} PROPOSAL{} WAITING: F9 TO TRY",
                if n == 1 { "" } else { "S" }
            )
        })
    }
}
