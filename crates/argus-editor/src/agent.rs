//! The agent panel (§11: "pending proposals, review requests, marks, the
//! change feed"; phase 4 item 4). What the agent is waiting on the person
//! for, and what it did:
//!
//! - Review requests (§10, `obs.review`): a question and the clip of its
//!   frames, replayed by the daemon from the keyframe ring; the person
//!   scrubs the clip frame by frame and answers with a key (1 to 9, a
//!   choice) or a sentence. The queue drains at their pace.
//! - Proposals (§10): try one (the game forks into A, as it is, and each
//!   variant, at the same moment), switch between them, keep one (the pick
//!   is the commit), or turn it down.
//! - The person's marks, waiting for the agent: their notes and frames.
//! - What the agent did: the change feed's rows by anyone else, newest
//!   first, as notes.
//!
//! The clips and frames are files the daemon's answers name; the editor
//! runs beside the daemon and reads them from disk, off the UI thread.

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};
use serde_json::{json, Value as Json};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

/// A picture read off disk: every frame of an animated PNG, or the one.
type Loaded = (String, Result<Vec<ColorImage>, String>);

/// Pictures by path, and the texture each shows.
struct Picture {
    frames: Vec<ColorImage>,
    texture: Option<TextureHandle>,
    shown: usize,
}

/// The clip of the review shown: where it is and whether it plays.
#[derive(Clone, Debug, Default)]
pub struct Scrub {
    /// The review's id.
    pub id: u64,
    /// The clip picture shown, from 0.
    pub at: usize,
    /// The clip plays on its own, looping.
    pub playing: bool,
    started: Option<(Instant, usize)>,
}

/// The panel's state.
pub struct Agent {
    /// `review.list`'s open rows, oldest first.
    pub reviews: Vec<Json>,
    /// `proposal.list`: `{pending, rows, trial}`.
    pub proposals: Json,
    /// `mark.list`'s open rows.
    pub marks: Vec<Json>,
    /// The review shown (its id), or the oldest.
    pub showing: Option<u64>,
    /// The shown review's clip.
    pub scrub: Scrub,
    /// The sentence being written for the review shown.
    pub answer: String,
    /// Notes being written for marks, by id.
    pub mark_notes: HashMap<u64, String>,
    pictures: HashMap<String, Picture>,
    asked: HashSet<String>,
    failed: HashMap<String, String>,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
}

impl Default for Agent {
    fn default() -> Agent {
        let (tx, rx) = channel();
        Agent {
            reviews: Vec::new(),
            proposals: Json::Null,
            marks: Vec::new(),
            showing: None,
            scrub: Scrub::default(),
            answer: String::new(),
            mark_notes: HashMap::new(),
            pictures: HashMap::new(),
            asked: HashSet::new(),
            failed: HashMap::new(),
            tx,
            rx,
        }
    }
}

/// Every frame of a PNG, animated or not, as egui images.
pub fn decode(bytes: &[u8]) -> Result<Vec<ColorImage>, String> {
    let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut r = d.read_info().map_err(|e| e.to_string())?;
    let n = r
        .info()
        .animation_control()
        .map_or(1, |a| a.num_frames.max(1));
    let mut out = Vec::new();
    let mut buf = vec![0; r.output_buffer_size()];
    for _ in 0..n {
        let f = match r.next_frame(&mut buf) {
            Ok(f) => f,
            Err(_) if !out.is_empty() => break,
            Err(e) => return Err(e.to_string()),
        };
        let px = &buf[..f.buffer_size()];
        let (w, h) = (f.width as usize, f.height as usize);
        let img = match f.color_type {
            png::ColorType::Rgba => ColorImage::from_rgba_unmultiplied([w, h], px),
            png::ColorType::GrayscaleAlpha => ColorImage::from_rgba_unmultiplied(
                [w, h],
                &px.chunks(2)
                    .flat_map(|c| [c[0], c[0], c[0], c[1]])
                    .collect::<Vec<u8>>(),
            ),
            other => return Err(format!("a PNG of {other:?}")),
        };
        out.push(img);
    }
    Ok(out)
}

impl Agent {
    /// What waits for the person: questions and proposals.
    pub fn waiting(&self) -> usize {
        self.reviews.len() + self.proposals["pending"].as_u64().unwrap_or(0) as usize
    }

    /// The bar's line about it, or nothing.
    pub fn waiting_line(&self) -> Option<String> {
        let (q, p) = (
            self.reviews.len(),
            self.proposals["pending"].as_u64().unwrap_or(0),
        );
        let mut parts = Vec::new();
        if q > 0 {
            parts.push(format!("{q} question{}", if q == 1 { "" } else { "s" }));
        }
        if p > 0 {
            parts.push(format!("{p} proposal{}", if p == 1 { "" } else { "s" }));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// `review.list`'s answer.
    pub fn take_reviews(&mut self, v: &Json) {
        self.reviews = v["rows"].as_array().cloned().unwrap_or_default();
        if self
            .showing
            .is_none_or(|id| !self.reviews.iter().any(|r| r["id"] == id))
        {
            self.showing = self.reviews.first().and_then(|r| r["id"].as_u64());
        }
    }

    /// `mark.list`'s answer.
    pub fn take_marks(&mut self, v: &Json) {
        self.marks = v["rows"].as_array().cloned().unwrap_or_default();
        let open: HashSet<u64> = self.marks.iter().filter_map(|m| m["id"].as_u64()).collect();
        self.mark_notes.retain(|id, _| open.contains(id));
    }

    /// The review shown.
    pub fn shown(&self) -> Option<&Json> {
        let id = self.showing?;
        self.reviews.iter().find(|r| r["id"] == id)
    }

    /// Frames of the shown review's clip, once read.
    pub fn clip_frames(&self) -> usize {
        self.shown()
            .and_then(|r| r["clip"].as_str())
            .and_then(|p| self.pictures.get(p))
            .map_or(0, |p| p.frames.len())
    }

    /// The frame of the run a clip picture shows.
    pub fn frame_of(r: &Json, i: usize) -> u64 {
        let (from, to) = (
            r["from"].as_u64().unwrap_or(0),
            r["to"].as_u64().unwrap_or(0),
        );
        let every = r["every"].as_u64().unwrap_or(1).max(1);
        (from + i as u64 * every).min(to)
    }

    /// Take in pictures read, and ask for the ones needed.
    fn load(&mut self, ctx: &egui::Context, path: &str) {
        while let Ok((p, r)) = self.rx.try_recv() {
            match r {
                Ok(frames) => {
                    self.pictures.insert(
                        p,
                        Picture {
                            frames,
                            texture: None,
                            shown: usize::MAX,
                        },
                    );
                }
                Err(e) => {
                    self.failed.insert(p, e);
                }
            }
        }
        if self.asked.insert(path.to_string()) {
            let (tx, c, p) = (self.tx.clone(), ctx.clone(), path.to_string());
            std::thread::spawn(move || {
                let r = std::fs::read(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|b| decode(&b));
                let _ = tx.send((p, r));
                c.request_repaint();
            });
        }
    }

    /// A picture's texture showing frame `i`, once read.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        i: usize,
    ) -> Option<(egui::TextureId, [usize; 2])> {
        self.load(ctx, path);
        let p = self.pictures.get_mut(path)?;
        let i = i.min(p.frames.len().checked_sub(1)?);
        let img = &p.frames[i];
        let size = img.size;
        match &mut p.texture {
            Some(_) if p.shown == i => {}
            Some(t) => {
                t.set(img.clone(), TextureOptions::LINEAR);
                p.shown = i;
            }
            None => {
                p.texture = Some(ctx.load_texture(path, img.clone(), TextureOptions::LINEAR));
                p.shown = i;
            }
        }
        p.texture.as_ref().map(|t| (t.id(), size))
    }

    /// Only the pictures still shown are kept.
    fn forget(&mut self) {
        let mut keep: HashSet<String> = self
            .marks
            .iter()
            .filter_map(|m| m["picture"].as_str().map(str::to_string))
            .collect();
        if let Some(c) = self.shown().and_then(|r| r["clip"].as_str()) {
            keep.insert(c.to_string());
        }
        self.pictures.retain(|p, _| keep.contains(p));
        self.asked.retain(|p| keep.contains(p));
    }

    /// Answer the shown review with its choice `n` (from 1), as its key does.
    pub fn choice(&self, n: usize) -> Option<(String, Json)> {
        let r = self.shown()?;
        let opt = r["options"].as_array()?.get(n.checked_sub(1)?)?.as_str()?;
        Some((
            "review.answer".into(),
            json!({ "id": r["id"], "answer": opt }),
        ))
    }

    /// Draw it; the calls the person made, in order.
    pub fn ui(&mut self, ui: &mut egui::Ui, author: &str, feed: &[Json]) -> Vec<(String, Json)> {
        let mut out = Vec::new();
        self.forget();
        egui::ScrollArea::vertical()
            .id_salt("agent")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.reviews_ui(ui, &mut out);
                ui.add_space(6.0);
                self.proposals_ui(ui, author, &mut out);
                ui.add_space(6.0);
                self.marks_ui(ui, author, &mut out);
                ui.add_space(6.0);
                did_ui(ui, author, feed);
            });
        out
    }

    fn reviews_ui(&mut self, ui: &mut egui::Ui, out: &mut Vec<(String, Json)>) {
        let n = self.reviews.len();
        ui.strong(if n == 0 {
            "Questions: none waiting".to_string()
        } else {
            format!("Questions for you ({n})")
        });
        let Some(r) = self.shown().cloned() else {
            ui.weak("The agent asks here when a change needs your eye: obs.review.");
            return;
        };
        let id = r["id"].as_u64().unwrap_or(0);
        if self.scrub.id != id {
            self.scrub = Scrub {
                id,
                ..Scrub::default()
            };
            self.answer.clear();
        }
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(r["asked_by"].as_str().unwrap_or("?"))
                        .strong()
                        .color(Color32::from_rgb(240, 170, 60)),
                );
                ui.weak(format!("#{id}  frames {} to {}", r["from"], r["to"]));
            });
            ui.label(RichText::new(r["question"].as_str().unwrap_or("")).size(15.0));
            if let Some(note) = r["note"].as_str() {
                ui.weak(note);
            }
            self.clip_ui(ui, &r);
            // The choices, a key each.
            ui.horizontal_wrapped(|ui| {
                for (i, o) in r["options"].as_array().into_iter().flatten().enumerate() {
                    let o = o.as_str().unwrap_or("?");
                    if ui
                        .button(format!("{} {o}", i + 1))
                        .on_hover_text(format!("Answer \"{o}\" (key {})", i + 1))
                        .clicked()
                    {
                        out.push(("review.answer".into(), json!({ "id": id, "answer": o })));
                    }
                }
            });
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.answer)
                        .hint_text("or say it in a sentence")
                        .desired_width(ui.available_width() - 60.0),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let send = ui
                    .add_enabled(!self.answer.trim().is_empty(), egui::Button::new("Send"))
                    .clicked();
                if (send || enter) && !self.answer.trim().is_empty() {
                    out.push((
                        "review.answer".into(),
                        json!({ "id": id, "answer": self.answer.trim() }),
                    ));
                }
            });
        });
        // The rest of the queue, one line each.
        for q in self.reviews.iter().filter(|q| q["id"] != id) {
            let qid = q["id"].as_u64().unwrap_or(0);
            if ui
                .selectable_label(
                    false,
                    format!("#{qid} {}", q["question"].as_str().unwrap_or("")),
                )
                .on_hover_text("Show this one")
                .clicked()
            {
                self.showing = Some(qid);
            }
        }
    }

    fn clip_ui(&mut self, ui: &mut egui::Ui, r: &Json) {
        let Some(path) = r["clip"].as_str().map(str::to_string) else {
            ui.weak(format!(
                "no clip: {}",
                r["clip_skipped"].as_str().unwrap_or("none was drawn")
            ));
            return;
        };
        if let Some(e) = self.failed.get(&path) {
            ui.colored_label(ui.visuals().error_fg_color, format!("the clip: {e}"));
            return;
        }
        let n = self.pictures.get(&path).map_or(0, |p| p.frames.len());
        // Playing: the picture the time since Play says, looping.
        if self.scrub.playing && n > 0 {
            let fps = r["fps"].as_f64().unwrap_or(30.0).max(1.0);
            let (t0, a0) = *self
                .scrub
                .started
                .get_or_insert((Instant::now(), self.scrub.at));
            self.scrub.at = (a0 + (t0.elapsed().as_secs_f64() * fps) as usize) % n;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(1.0 / fps));
        }
        let ctx = ui.ctx().clone();
        match self.texture(&ctx, &path, self.scrub.at) {
            Some((tex, [w, h])) => {
                let fit = ui.available_width().min(w as f32);
                let size = egui::vec2(fit, fit * h as f32 / w.max(1) as f32);
                ui.image((tex, size));
            }
            None => {
                ui.add_sized(
                    egui::vec2(ui.available_width(), 120.0),
                    egui::Spinner::new(),
                );
            }
        }
        if n == 0 {
            return;
        }
        ui.horizontal(|ui| {
            let play = if self.scrub.playing {
                "Pause"
            } else {
                "Play clip"
            };
            if ui.button(play).clicked() {
                self.scrub.playing = !self.scrub.playing;
                self.scrub.started = self
                    .scrub
                    .playing
                    .then(|| (Instant::now(), self.scrub.at % n));
            }
            let mut at = self.scrub.at.min(n - 1);
            let frame = Agent::frame_of(r, at);
            let s = ui.add(
                egui::Slider::new(&mut at, 0..=n - 1)
                    .show_value(false)
                    .text(format!("frame {frame}")),
            );
            if s.changed() {
                self.scrub.at = at;
                self.scrub.playing = false;
                self.scrub.started = None;
            }
        });
    }

    fn proposals_ui(&mut self, ui: &mut egui::Ui, author: &str, out: &mut Vec<(String, Json)>) {
        let rows: Vec<Json> = self.proposals["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        ui.strong(if rows.is_empty() {
            "Proposals: none waiting".to_string()
        } else {
            format!("Proposals to try ({})", rows.len())
        });
        if rows.is_empty() {
            ui.weak("A change the agent wants you to feel before it lands.");
            return;
        }
        let trial = self.proposals["trial"].clone();
        for p in &rows {
            let id = p["id"].as_u64().unwrap_or(0);
            ui.push_id(("proposal", id), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(p["author"].as_str().unwrap_or("?"))
                                .strong()
                                .color(Color32::from_rgb(240, 170, 60)),
                        );
                        ui.weak(format!("#{id}"));
                        ui.label(p["note"].as_str().unwrap_or(""));
                    });
                    let variants: Vec<Json> =
                        p["variants"].as_array().cloned().unwrap_or_default();
                    let trying = trial["proposal"] == id;
                    let showing = trial["showing"].as_str().unwrap_or("A").to_string();
                    let mut labels = vec![("A".to_string(), "as it is".to_string())];
                    for v in &variants {
                        labels.push((
                            v["label"].as_str().unwrap_or("?").to_string(),
                            v["note"].as_str().unwrap_or("").to_string(),
                        ));
                    }
                    if trying {
                        ui.horizontal_wrapped(|ui| {
                            for (l, note) in &labels {
                                let text = if note.is_empty() {
                                    l.clone()
                                } else {
                                    format!("{l}: {note}")
                                };
                                if ui.selectable_label(*l == showing, text).clicked()
                                    && *l != showing
                                {
                                    out.push(("proposal.show".into(), json!({ "variant": l })));
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            let keep = if showing == "A" {
                                "Keep the game as it is".to_string()
                            } else {
                                format!("Keep {showing}")
                            };
                            if ui
                                .button(keep)
                                .on_hover_text("F10 in the game: the pick is the commit, by you")
                                .clicked()
                            {
                                out.push((
                                    "proposal.pick".into(),
                                    json!({ "id": id, "variant": showing, "author": author }),
                                ));
                            }
                            if ui
                                .button("Stop trying")
                                .on_hover_text("Back to A; the proposal waits on")
                                .clicked()
                            {
                                out.push(("proposal.stop".into(), json!({})));
                            }
                        });
                        ui.weak(format!(
                            "F9 in the game switches; switched {} times",
                            trial["toggles"].as_u64().unwrap_or(0)
                        ));
                    } else {
                        for (l, note) in labels.iter().skip(1) {
                            ui.weak(format!("{l}: {note}"));
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .button("Try it")
                                .on_hover_text("The game as it is (A) and each variant, at this moment; B first")
                                .clicked()
                            {
                                out.push(("proposal.try".into(), json!({ "id": id })));
                                if labels.len() > 1 {
                                    out.push((
                                        "proposal.show".into(),
                                        json!({ "variant": labels[1].0 }),
                                    ));
                                }
                            }
                            if ui.button("Turn down").clicked() {
                                out.push((
                                    "proposal.reject".into(),
                                    json!({ "id": id, "author": author }),
                                ));
                            }
                        });
                    }
                });
            });
        }
    }

    fn marks_ui(&mut self, ui: &mut egui::Ui, author: &str, out: &mut Vec<(String, Json)>) {
        ui.strong(if self.marks.is_empty() {
            "Your marks: none open".to_string()
        } else {
            format!("Your marks, waiting for the agent ({})", self.marks.len())
        });
        if self.marks.is_empty() {
            ui.weak("F8 in the game marks a moment, with a note.");
            return;
        }
        let ctx = ui.ctx().clone();
        for m in self.marks.clone() {
            let id = m["id"].as_u64().unwrap_or(0);
            ui.push_id(("mark", id), |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some((tex, [w, h])) = m["picture"]
                            .as_str()
                            .and_then(|p| self.texture(&ctx, p, 0))
                        {
                            let size = egui::vec2(96.0, 96.0 * h as f32 / w.max(1) as f32);
                            ui.image((tex, size));
                        }
                        ui.vertical(|ui| {
                            ui.weak(format!(
                                "#{id}  frame {}  {}",
                                m["frame"],
                                m["scene"].as_str().unwrap_or("")
                            ));
                            let note = m["note"].as_str().unwrap_or("");
                            let draft = self
                                .mark_notes
                                .entry(id)
                                .or_insert_with(|| note.to_string());
                            ui.add(
                                egui::TextEdit::singleline(draft)
                                    .hint_text("what was wrong")
                                    .desired_width(ui.available_width()),
                            );
                            let changed = draft.trim() != note;
                            let draft = draft.trim().to_string();
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add_enabled(changed, egui::Button::new("Save note"))
                                    .clicked()
                                {
                                    out.push((
                                        "mark.note".into(),
                                        json!({ "id": id, "note": draft }),
                                    ));
                                }
                                if ui
                                    .button("Reproduce")
                                    .on_hover_text("Load the moment: its scene, the seconds before it, paused at the mark")
                                    .clicked()
                                {
                                    out.push(("mark.open".into(), json!({ "id": id })));
                                }
                                if ui
                                    .button("Close")
                                    .on_hover_text("Done with it: it leaves the agent's queue")
                                    .clicked()
                                {
                                    out.push((
                                        "mark.close".into(),
                                        json!({ "id": id, "author": author }),
                                    ));
                                }
                                if ui
                                    .button("Take back")
                                    .on_hover_text("As if it was never marked: its files go")
                                    .clicked()
                                {
                                    out.push(("mark.cancel".into(), json!({ "id": id })));
                                }
                            });
                        });
                    });
                });
            });
        }
    }
}

/// What everyone else did, newest first, as notes: the feed "author-tagged,
/// so the human returning reads what the agent did as a sequence of notes".
fn did_ui(ui: &mut egui::Ui, author: &str, feed: &[Json]) {
    let rows: Vec<&Json> = feed
        .iter()
        .rev()
        .filter(|r| r["author"].as_str().is_some_and(|a| a != author))
        .take(30)
        .collect();
    ui.strong("What the agent did");
    if rows.is_empty() {
        ui.weak("Nothing yet: the change feed has only your changes.");
        return;
    }
    for r in rows {
        let id = r["id"].as_u64().unwrap_or(0);
        ui.push_id(("did", id), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(r["author"].as_str().unwrap_or("?"))
                        .strong()
                        .color(Color32::from_rgb(240, 170, 60)),
                );
                ui.weak(format!("#{id}  frame {}", r["frame"]));
                let note = r["note"].as_str().unwrap_or("");
                if note.is_empty() {
                    ui.label(
                        RichText::new(r["command"].as_str().unwrap_or(""))
                            .monospace()
                            .size(11.5),
                    );
                } else {
                    ui.label(note)
                        .on_hover_text(r["command"].as_str().unwrap_or(""));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_review_row_names_its_frames() {
        let r = json!({ "from": 280, "to": 400, "every": 2 });
        assert_eq!(Agent::frame_of(&r, 0), 280);
        assert_eq!(Agent::frame_of(&r, 60), 400);
        assert_eq!(Agent::frame_of(&r, 99), 400);
    }

    #[test]
    fn a_choice_is_its_key() {
        let mut a = Agent::default();
        a.take_reviews(&json!({ "rows": [
            { "id": 3, "question": "?", "options": ["yes", "no"] },
            { "id": 4, "question": "?", "options": ["a"] },
        ] }));
        assert_eq!(a.showing, Some(3), "the oldest");
        assert_eq!(
            a.choice(2),
            Some(("review.answer".into(), json!({ "id": 3, "answer": "no" })))
        );
        assert_eq!(a.choice(3), None);
        assert_eq!(a.choice(0), None);
        a.take_reviews(&json!({ "rows": [{ "id": 4, "question": "?", "options": ["a"] }] }));
        assert_eq!(a.showing, Some(4), "the next once it is answered");
        assert_eq!(a.waiting_line().as_deref(), Some("1 question"));
    }

    #[test]
    fn an_animated_png_decodes_to_every_frame() {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, 3, 2);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.set_animated(3, 0).unwrap();
            let mut w = e.write_header().unwrap();
            for i in 0..3u8 {
                let px: Vec<u8> = (0..6).flat_map(|_| [i * 80, i * 80, i * 80, 255]).collect();
                w.write_image_data(&px).unwrap();
            }
            w.finish().unwrap();
        }
        let f = decode(&out).unwrap();
        assert_eq!(f.len(), 3);
        assert_eq!(f[2].size, [3, 2]);
        assert_eq!(f[2].pixels[0].r(), 160);
    }
}
