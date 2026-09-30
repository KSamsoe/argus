//! Review requests (§10: "The reverse channel: the agent asks for
//! judgement. `obs.review {clip, question}` queues a short clip and a
//! question ("does the enemy death read as a kill?") to the editor; the
//! human answers with one keystroke or a sentence. The queue is visible in
//! the editor's agent panel and drains at the human's pace. Because the
//! clip is rendered from a deterministic replay, the agent can attach the
//! exact frame range and the human can scrub it"; phase 4 item 4).
//!
//! A request is `.argus/reviews/<id>/`: `review.json` (the question, its
//! choices, the frame range, who asked, and the answer once there is one),
//! `clip.png` (the frames of the range replayed from the keyframe ring, an
//! animated PNG) and `clip-sheet.png` (eight of them in one picture, for a
//! reader that sees one). Asking and answering are events and rows in the
//! change feed; `obs.subscribe {topics: ["reviews"]}` streams both.

use super::*;

/// The most frames a clip holds: ten seconds at thirty a second.
const CLIP_MAX_FRAMES: u64 = 300;

fn reviews_dir(root: &Path) -> PathBuf {
    root.join(".argus").join("reviews")
}

fn review_dir(root: &Path, id: u64) -> PathBuf {
    reviews_dir(root).join(id.to_string())
}

/// Request ids on disk, oldest first.
fn ids(root: &Path) -> Vec<u64> {
    let mut out: Vec<u64> = std::fs::read_dir(reviews_dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
        .collect();
    out.sort_unstable();
    out
}

fn no_review(id: u64) -> ArgusError {
    ArgusError::new(codes::NO_REVIEW, format!("no review request {id}"))
        .hint("review.list lists the requests, open ones first.")
}

fn read_review(root: &Path, id: u64) -> Result<Json> {
    let text = std::fs::read_to_string(review_dir(root, id).join("review.json"))
        .map_err(|_| no_review(id))?;
    serde_json::from_str(&text)
        .map_err(|e| ArgusError::new(codes::BAD_REVIEW, format!("review {id}: {e}")))
}

fn write_review(root: &Path, id: u64, r: &Json) -> Result<()> {
    let dir = review_dir(root, id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| ArgusError::new(codes::BAD_REVIEW, format!("{}: {e}", dir.display())))?;
    let text = serde_json::to_string_pretty(r).expect("json") + "\n";
    std::fs::write(dir.join("review.json"), text)
        .map_err(|e| ArgusError::new(codes::BAD_REVIEW, format!("review {id}: {e}")))
}

fn review_id(params: &Json) -> Result<u64> {
    match params.get("id") {
        Some(Json::Number(n)) => n.as_u64(),
        Some(Json::String(s)) => s.trim_start_matches('#').parse().ok(),
        _ => None,
    }
    .ok_or_else(|| bad_params("give id: the request's number, as review.list shows it"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Daemon {
    /// `obs.review`: ask the human a question about a moment of this run,
    /// with its clip.
    pub(super) fn obs_review(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let question = json::str_param(params, "question")?.trim().to_string();
        if question.is_empty() {
            return Err(bad_params("the question is empty").hint(
                "Ask what the human can judge by feel: \"does the enemy death read as a kill?\"",
            ));
        }
        let options: Vec<String> = match params.get("options") {
            None | Some(Json::Null) => vec!["yes".into(), "no".into()],
            Some(_) => json::str_list(params, "options")?,
        };
        if options.len() > 9 {
            return Err(bad_params("at most nine choices: one key each, 1 to 9"));
        }
        self.before_ticks();
        let now = self.sched.tick();
        let driving = self.sched.driving().clone();
        let (_, clip_len, every, fps) = marks::spans(driving.kind, driving.hz());
        let (from, to) = match params.get("frames") {
            None | Some(Json::Null) => (now.saturating_sub(clip_len), now),
            Some(Json::Array(a)) if a.len() == 2 => match (a[0].as_u64(), a[1].as_u64()) {
                (Some(f), Some(t)) => (f, t),
                _ => return Err(bad_params("frames is [from, to], two frame numbers")),
            },
            Some(other) => return Err(bad_params(format!("frames is [from, to]; got {other}"))),
        };
        if from > to || to > now {
            return Err(bad_params(format!(
                "frames [{from}, {to}] must run forward and end by now (frame {now})"
            ))
            .hint("A clip of the last two seconds is the default: leave frames out."));
        }
        if (to - from) / every.max(1) > CLIP_MAX_FRAMES {
            return Err(bad_params(format!(
                "frames [{from}, {to}] is more than {CLIP_MAX_FRAMES} clip pictures"
            ))
            .hint("A question is about a moment: ten seconds at most."));
        }
        let (scene, seed, ring) = {
            let s = self.session.as_ref().expect("session");
            (s.scene_name.clone(), s.seed, s.ring.clone())
        };
        let start = ring.borrow().at_or_before(from).cloned().ok_or_else(|| {
            ArgusError::new(
                codes::BAD_REVIEW,
                format!("frame {from} is older than the keyframes this run keeps"),
            )
            .hint("sim.snapshot reports the ring: every frame of the last five minutes can be replayed.")
        })?;
        let id = ids(&root).last().map_or(1, |i| i + 1);
        let dir = review_dir(&root, id);
        std::fs::create_dir_all(&dir)
            .map_err(|e| ArgusError::new(codes::BAD_REVIEW, format!("{}: {e}", dir.display())))?;
        let software = json::bool_param(params, "software", false)?;
        let clip = self.review_clip(&dir, &start, from, to, every, fps, software);
        let mut r = json!({
            "id": id,
            "question": question,
            "options": options,
            "from": from,
            "to": to,
            "asked_by": self.key_author(),
            "asked_at": now_secs(),
            "status": "open",
            "scene": scene,
            "seed": seed,
            "clip": clip,
        });
        if let Some(n) = json::opt_str(params, "note")?.filter(|n| !n.is_empty()) {
            r["note"] = json!(n);
        }
        write_review(&root, id, &r)?;
        self.emit_review("review.asked", id, &question);
        let out = self.review_summary(&root, &r);
        self.notify("reviews", "review.asked", out.clone());
        Ok(out)
    }

    /// The clip of a request's frames, replayed from the ring.
    #[allow(clippy::too_many_arguments)]
    fn review_clip(
        &mut self,
        dir: &Path,
        start: &crate::ring::Keyframe,
        from: u64,
        to: u64,
        every: u64,
        fps: f64,
        software: bool,
    ) -> Json {
        #[cfg(feature = "render")]
        {
            let (cw, ch) = marks::clip_size(self.frame_size());
            self.refresh_assets();
            match self
                .render_clip(start, from, to, every, (cw, ch), software)
                .and_then(|frames| {
                    argus_render::save_apng(&frames, fps, &dir.join("clip.png"))?;
                    let labels: Vec<String> = (0..frames.len())
                        .map(|i| format!("{}", (from + i as u64 * every).min(to)))
                        .collect();
                    if let Some(sheet) = argus_render::contact_sheet(&frames, &labels) {
                        sheet.save_png(&dir.join("clip-sheet.png"))?;
                    }
                    Ok(frames.len())
                }) {
                Ok(n) => json!({
                    "file": "clip.png", "sheet": "clip-sheet.png", "frames": n,
                    "fps": (fps * 10.0).round() / 10.0, "every": every,
                    "width": cw, "height": ch, "from": from, "to": to,
                }),
                Err(e) => json!({ "skipped": e.message }),
            }
        }
        #[cfg(not(feature = "render"))]
        {
            let _ = (dir, start, from, to, every, fps, software);
            json!({ "skipped": "this daemon was built without the renderer" })
        }
    }

    /// A request as the queue shows it, its files as paths.
    fn review_summary(&self, root: &Path, r: &Json) -> Json {
        let id = r["id"].as_u64().unwrap_or(0);
        let dir = review_dir(root, id);
        let path = |f: &Json| {
            f.as_str().map(|f| {
                dir.join(f)
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_string()
            })
        };
        let mut out = json!({
            "id": id,
            "status": r["status"],
            "question": r["question"],
            "options": r["options"],
            "from": r["from"],
            "to": r["to"],
            "asked_by": r["asked_by"],
            "clip": path(&r["clip"]["file"]),
            "sheet": path(&r["clip"]["sheet"]),
            "frames": r["clip"]["frames"],
            "every": r["clip"]["every"],
            "fps": r["clip"]["fps"],
        });
        for k in [
            "note",
            "answer",
            "answered_by",
            "answered_at",
            "answered_frame",
        ] {
            if !r[k].is_null() {
                out[k] = r[k].clone();
            }
        }
        if let Some(why) = r["clip"]["skipped"].as_str() {
            out["clip_skipped"] = json!(why);
        }
        out
    }

    /// `review.list`: the queue, oldest first; with `all`, the answered too.
    pub(super) fn review_list(&self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let all = json::bool_param(params, "all", false)?;
        let (mut open, mut answered) = (0, 0);
        let mut rows = Vec::new();
        for id in ids(&root) {
            let Ok(r) = read_review(&root, id) else {
                continue;
            };
            let is_open = r["status"] == "open";
            if is_open {
                open += 1;
            } else {
                answered += 1;
            }
            if is_open || all {
                rows.push(self.review_summary(&root, &r));
            }
        }
        Ok(json!({ "open": open, "answered": answered, "rows": rows }))
    }

    /// `review.get`: one request in full.
    pub(super) fn review_get(&self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = review_id(params)?;
        let r = read_review(&root, id)?;
        let mut out = self.review_summary(&root, &r);
        out["scene"] = r["scene"].clone();
        out["seed"] = r["seed"].clone();
        out["asked_at"] = r["asked_at"].clone();
        Ok(out)
    }

    /// `review.answer`: the human's answer, a choice or a sentence.
    pub(super) fn review_answer(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let id = review_id(params)?;
        let answer = json::str_param(params, "answer")?.trim().to_string();
        if answer.is_empty() {
            return Err(bad_params("the answer is empty")
                .hint("One of the request's choices, or a sentence."));
        }
        let mut r = read_review(&root, id)?;
        if r["status"] != "open" {
            return Err(ArgusError::new(
                codes::BAD_REVIEW,
                format!(
                    "review {id} was answered by {}: {}",
                    r["answered_by"].as_str().unwrap_or("?"),
                    r["answer"].as_str().unwrap_or("")
                ),
            )
            .hint("A request is answered once; the agent asks again if it wants another look."));
        }
        r["status"] = json!("answered");
        r["answer"] = json!(answer);
        r["answered_by"] = json!(self.key_author());
        r["answered_at"] = json!(now_secs());
        r["answered_frame"] = json!(self.sched.tick());
        write_review(&root, id, &r)?;
        self.emit_review("review.answered", id, &answer);
        let out = self.review_summary(&root, &r);
        self.notify("reviews", "review.answered", out.clone());
        Ok(out)
    }

    fn emit_review(&mut self, name: &str, id: u64, text: &str) {
        self.world.emit(Event::Game {
            name: name.to_string(),
            fields: vec![
                ("id".into(), Value::UInt(id)),
                ("text".into(), Value::Str(text.to_string())),
            ],
        });
        self.pump_events();
    }
}
