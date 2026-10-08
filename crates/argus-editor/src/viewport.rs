//! The viewport (§11: "the viewport subscribes to frames"; phase 4 item
//! 1). The daemon draws the game into a file both processes map
//! (`render.stream`), and each `render.frame` notification names the slot
//! to copy. The person plays in it: while it has focus, keys, the mouse,
//! and typed text go to the daemon as the window's device events
//! (`input.device`), so the game's bindings, F8's marks, and F9's
//! proposals work as they do in the game window, and the input log holds
//! actions, not keys. A click while the game is not playing, or Alt and a
//! click while it is, selects what is under it, from the semantic frame.

use argus_client::frames::{FrameInfo, FrameReader};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use eframe::{egui_wgpu, wgpu};
use serde_json::{json, Value as Json};
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a new panel size waits before the stream is asked for it.
const RESIZE_AFTER: Duration = Duration::from_millis(200);

/// What a click selected.
#[derive(Clone, Debug, PartialEq)]
pub struct Selected {
    /// The entity, as the protocol names it (`@player`, `e:42`).
    pub id: String,
    /// What the semantic frame calls it.
    pub name: String,
    /// Where it is in the frame, in pixels: x0, y0, x1, y1.
    pub bounds: [f64; 4],
}

/// The window's GPU, when the editor runs in one: a frame goes from the
/// shared file straight into a texture of its own, with no work per pixel
/// (found when Kevin first played in the editor: egui's own texture path
/// turned each frame's million pixels into colours one by one, most of a
/// debug build's frame).
pub struct Gpu {
    state: egui_wgpu::RenderState,
    texture: Option<(wgpu::Texture, egui::TextureId, (u32, u32))>,
}

impl Gpu {
    /// The window's renderer (eframe's `CreationContext::wgpu_render_state`).
    pub fn new(state: egui_wgpu::RenderState) -> Gpu {
        Gpu {
            state,
            texture: None,
        }
    }

    /// Put a frame's pixels in the texture, made or remade at its size.
    fn upload(&mut self, info: FrameInfo, px: &[u8]) -> egui::TextureId {
        let size = (info.width, info.height);
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        if self.texture.as_ref().is_none_or(|t| t.2 != size) {
            let tex = self.state.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("argus-viewport"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // The daemon's offscreen format: sRGB bytes, as egui's own.
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let mut r = self.state.renderer.write();
            let id = match self.texture.take() {
                Some((_, id, _)) => {
                    r.update_egui_texture_from_wgpu_texture(
                        &self.state.device,
                        &view,
                        wgpu::FilterMode::Nearest,
                        id,
                    );
                    id
                }
                None => {
                    r.register_native_texture(&self.state.device, &view, wgpu::FilterMode::Nearest)
                }
            };
            self.texture = Some((tex, id, size));
        }
        let (tex, id, _) = self.texture.as_ref().expect("just made");
        self.state.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            px,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size.0),
                rows_per_image: Some(size.1),
            },
            extent,
        );
        *id
    }
}

/// The viewport's state.
#[derive(Default)]
pub struct Viewport {
    reader: Option<FrameReader>,
    latest: Option<Json>,
    shown_seq: u64,
    /// The window's GPU, when there is one; else egui's own texture.
    pub gpu: Option<Gpu>,
    texture: Option<egui::TextureHandle>,
    texture_id: Option<egui::TextureId>,
    buf: Vec<u8>,
    /// Frames shown so far.
    pub frames_shown: u64,
    /// The frame on screen.
    pub info: Option<FrameInfo>,
    /// The size the panel wants, in pixels.
    pub want: (u32, u32),
    asked: Option<(u32, u32)>,
    size_since: Option<Instant>,
    /// Keys and the mouse go to the game.
    pub focused: bool,
    modifiers: egui::Modifiers,
    held: Vec<&'static str>,
    pointer: Option<[f32; 2]>,
    /// Where the frame is drawn, in points.
    pub image_rect: Option<Rect>,
    /// What a click selected.
    pub selected: Option<Selected>,
    /// What the frame on screen shows, in world units (`render.frame`'s
    /// `view`: centre, half extents, roll).
    pub view: Option<Json>,
    /// The pointer paints (the Tiles tab has a tool): presses and drags go
    /// to the painter, not the game.
    pub paint: bool,
    painting: bool,
    /// How the game is shown: as it is (empty), or as a colour-blind eye
    /// sees it, by the daemon's name (`render.stream`'s `see_as`; phase 5
    /// item 2), to check that it reads without guessing.
    pub see_as: &'static str,
}

/// The kinds of colour vision the viewport can show the game as, as the
/// daemon names them.
pub const SEE_AS: [&str; 5] = [
    "normal",
    "protanopia",
    "deuteranopia",
    "tritanopia",
    "achromatopsia",
];

/// What the viewport wants sent.
pub enum Send {
    /// `render.stream` at this size.
    Stream(u32, u32),
    /// `input.device` with these events and this text.
    Input(Vec<Json>, Vec<String>),
    /// `obs.frame` (semantic) to find what is at this pixel.
    Pick([f64; 2], u32, u32),
    /// The pointer, painting, in the frame's pixels.
    Paint(Paint),
}

/// The pointer in paint mode, in the frame's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// Over the frame, not pressed.
    Hover([f32; 2]),
    /// Pressed: a stroke begins.
    Down([f32; 2]),
    /// Dragged.
    Move([f32; 2]),
    /// Let go: the stroke ends.
    Up([f32; 2]),
}

impl Viewport {
    /// A `render.frame` notification: the newest frame to show.
    pub fn on_frame(&mut self, params: Json) {
        self.latest = Some(params);
    }

    /// The connection is new: the stream must be asked for again.
    pub fn reset(&mut self) {
        self.asked = None;
        self.latest = None;
        self.reader = None;
        self.shown_seq = 0;
        self.focused = false;
        self.held.clear();
    }

    /// Copy the newest frame into the texture, if there is one not shown.
    fn upload(&mut self, ctx: &egui::Context) {
        // A window that does not paint (minimized) submits nothing to the
        // GPU, and every frame written to the texture waits in its queue
        // until it does (found by Kevin's hand check: the editor left
        // minimized grew to 11 GB, and fell back when it painted). The
        // newest frame goes up when the window paints again.
        if ctx.input(|i| i.viewport().minimized == Some(true)) {
            return;
        }
        let Some(n) = &self.latest else {
            return;
        };
        let (Some(slot), Some(seq), Some(path)) =
            (n["slot"].as_u64(), n["seq"].as_u64(), n["path"].as_str())
        else {
            return;
        };
        if seq <= self.shown_seq
            && self
                .reader
                .as_ref()
                .is_some_and(|r| r.path() == Path::new(path))
        {
            return;
        }
        if self
            .reader
            .as_ref()
            .is_none_or(|r| r.path() != Path::new(path))
        {
            self.reader = FrameReader::open(Path::new(path)).ok();
            self.shown_seq = 0;
        }
        let Some(r) = &self.reader else {
            return;
        };
        let Some(info) = r.read(slot as u32, seq, &mut self.buf) else {
            // Overwritten already: the next notification brings a newer one.
            return;
        };
        if let Some(g) = &mut self.gpu {
            self.texture_id = Some(g.upload(info, &self.buf));
        } else {
            // Without the window's GPU (the headless tests): egui's texture.
            // The frames are opaque, so their bytes are already the
            // premultiplied colours egui keeps.
            let img = egui::ColorImage::from_rgba_premultiplied(
                [info.width as usize, info.height as usize],
                &self.buf,
            );
            match &mut self.texture {
                Some(t) => t.set(img, egui::TextureOptions::NEAREST),
                None => {
                    self.texture =
                        Some(ctx.load_texture("argus-viewport", img, egui::TextureOptions::NEAREST))
                }
            }
            self.texture_id = self.texture.as_ref().map(|t| t.id());
        }
        self.shown_seq = seq;
        self.frames_shown += 1;
        self.info = Some(info);
        self.view = self.latest.as_ref().and_then(|n| n.get("view").cloned());
    }

    /// Draw the viewport and turn what the person does in it into calls.
    /// `playing`: the game is running, so a plain click goes to it.
    pub fn ui(&mut self, ui: &mut egui::Ui, up: bool, playing: bool) -> Vec<Send> {
        let mut out = Vec::new();
        ui.horizontal(|ui| {
            match &self.selected {
                Some(s) => {
                    ui.label(format!("Selected {}", s.id));
                    if !s.name.is_empty() {
                        ui.weak(&s.name);
                    }
                }
                None => {
                    ui.weak("Nothing selected");
                }
            }
            ui.separator();
            ui.weak(if self.focused {
                "Keys go to the game; click outside it to stop"
            } else if playing {
                "Click the game to play; Alt and a click selects"
            } else {
                "A click selects; Play, then click the game to play"
            });
            if let Some(i) = &self.info {
                ui.weak(format!("{}x{}  tick {}", i.width, i.height, i.tick));
            }
            ui.separator();
            let before = self.see_as;
            egui::ComboBox::from_label("See as")
                .selected_text(if self.see_as.is_empty() {
                    "normal"
                } else {
                    self.see_as
                })
                .show_ui(ui, |ui| {
                    for k in SEE_AS {
                        let v = if k == "normal" { "" } else { k };
                        ui.selectable_value(&mut self.see_as, v, k);
                    }
                });
            if self.see_as != before {
                // The stream is asked for again, drawn the new way.
                self.asked = None;
            }
        });
        let avail = ui.available_size().max(Vec2::splat(16.0));
        let ppp = ui.ctx().pixels_per_point();
        self.want = ((avail.x * ppp) as u32, (avail.y * ppp) as u32);
        let now = Instant::now();
        if up && self.asked != Some(self.want) {
            let since = *self.size_since.get_or_insert(now);
            if self.asked.is_none() || now.duration_since(since) >= RESIZE_AFTER {
                out.push(Send::Stream(self.want.0, self.want.1));
                self.asked = Some(self.want);
                self.size_since = None;
            } else {
                ui.ctx().request_repaint_after(RESIZE_AFTER);
            }
        } else {
            self.size_since = None;
        }
        self.upload(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(avail, Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(12));
        let image = match (self.texture_id, &self.info) {
            (Some(t), Some(i)) => {
                // The frame at its size, fitted into the panel and centred.
                let size = Vec2::new(i.width as f32, i.height as f32) / ppp;
                let scale = (rect.width() / size.x).min(rect.height() / size.y).min(1.0);
                let r = Rect::from_center_size(rect.center(), size * scale);
                painter.image(
                    t,
                    r,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                if self.focused {
                    painter.rect_stroke(
                        r,
                        0.0,
                        egui::Stroke::new(1.0_f32, ui.visuals().selection.stroke.color),
                        egui::StrokeKind::Outside,
                    );
                }
                Some(r)
            }
            _ => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    if up {
                        "waiting for frames"
                    } else {
                        "no daemon"
                    },
                    egui::FontId::proportional(14.0),
                    Color32::GRAY,
                );
                None
            }
        };
        self.image_rect = image;
        if let (Some(img), Some(info)) = (image, self.info) {
            out.extend(self.input(ui, &response, img, info, playing));
        }
        if self.focused {
            // Tab, the arrows, and Escape are the game's while it has focus.
            ui.memory_mut(|m| {
                m.request_focus(response.id);
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }
        out
    }

    /// The person's input this frame, as device events for the game.
    fn input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        img: Rect,
        info: FrameInfo,
        playing: bool,
    ) -> Vec<Send> {
        let mut out = Vec::new();
        let (events, modifiers) = ui.input(|i| (i.events.clone(), i.modifiers));
        let to_px = |p: Pos2| -> [f32; 2] {
            let x = (p.x - img.min.x) / img.width() * info.width as f32;
            let y = (p.y - img.min.y) / img.height() * info.height as f32;
            [
                x.clamp(0.0, info.width as f32 - 1.0),
                y.clamp(0.0, info.height as f32 - 1.0),
            ]
        };
        let mut evs: Vec<Json> = Vec::new();
        let mut text: Vec<String> = Vec::new();
        let mut moved: Option<[f32; 2]> = None;
        if self.paint {
            // The painter has the pointer; the game has nothing while it does.
            if self.focused {
                self.blur(&mut evs);
                out.push(Send::Input(evs, Vec::new()));
            }
            let mut last = self.pointer;
            for e in &events {
                match e {
                    egui::Event::PointerMoved(p) if img.contains(*p) || self.painting => {
                        let at = to_px(*p);
                        last = Some(at);
                        out.push(Send::Paint(if self.painting {
                            Paint::Move(at)
                        } else {
                            Paint::Hover(at)
                        }));
                    }
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } if img.contains(*pos) => {
                        let at = to_px(*pos);
                        last = Some(at);
                        self.painting = true;
                        out.push(Send::Paint(Paint::Down(at)));
                    }
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    } if self.painting => {
                        let at = to_px(*pos);
                        self.painting = false;
                        out.push(Send::Paint(Paint::Up(at)));
                    }
                    _ => {}
                }
            }
            self.pointer = last;
            return out;
        }
        self.painting = false;
        for e in &events {
            match e {
                egui::Event::PointerMoved(p)
                    if img.contains(*p) && (self.focused || response.hovered()) =>
                {
                    moved = Some(to_px(*p));
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers,
                } => {
                    if !img.contains(*pos) {
                        if self.focused {
                            self.blur(&mut evs);
                        }
                        continue;
                    }
                    if modifiers.alt || !playing {
                        let [x, y] = to_px(*pos);
                        out.push(Send::Pick([x as f64, y as f64], info.width, info.height));
                        continue;
                    }
                    self.focused = true;
                    if let Some(b) = mouse_name(*button) {
                        let at = to_px(*pos);
                        evs.push(json!({ "pointer": at }));
                        self.pointer = Some(at);
                        evs.push(json!({ "mouse": b, "pressed": true }));
                        self.held.push(b);
                    }
                }
                egui::Event::PointerButton {
                    button,
                    pressed: false,
                    ..
                } => {
                    if let Some(b) = mouse_name(*button) {
                        if let Some(i) = self.held.iter().position(|h| *h == b) {
                            self.held.remove(i);
                            evs.push(json!({ "mouse": b, "pressed": false }));
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    ..
                } if self.focused && !repeat => {
                    if let Some(name) = key_name(physical_key.unwrap_or(*key)) {
                        evs.push(json!({ "key": name, "pressed": pressed }));
                    }
                }
                egui::Event::Text(t) if self.focused => text.push(t.clone()),
                egui::Event::WindowFocused(false) if self.focused => self.blur(&mut evs),
                _ => {}
            }
        }
        if self.focused {
            for (on, was, name) in [
                (modifiers.shift, self.modifiers.shift, "ShiftLeft"),
                (modifiers.ctrl, self.modifiers.ctrl, "ControlLeft"),
                (modifiers.alt, self.modifiers.alt, "AltLeft"),
            ] {
                if on != was {
                    evs.push(json!({ "key": name, "pressed": on }));
                }
            }
        }
        self.modifiers = modifiers;
        if let Some(at) = moved {
            if self.pointer != Some(at) {
                self.pointer = Some(at);
                // Before the rest, so a click lands where the pointer is.
                evs.insert(0, json!({ "pointer": at }));
            }
        }
        if !evs.is_empty() || !text.is_empty() {
            out.push(Send::Input(evs, text));
        }
        out
    }

    /// Focus leaves: everything held goes up, as when a window loses focus.
    fn blur(&mut self, evs: &mut Vec<Json>) {
        self.focused = false;
        self.held.clear();
        evs.push(json!({ "release_all": true }));
    }

    /// What `obs.frame` says is at the pixel: the smallest thing there, the
    /// UI above the world.
    pub fn pick(&mut self, frame: &Json, at: [f64; 2]) {
        let inside = |b: &Json| -> Option<([f64; 4], f64)> {
            let v: Vec<f64> = b.as_array()?.iter().filter_map(Json::as_f64).collect();
            let b = [*v.first()?, *v.get(1)?, *v.get(2)?, *v.get(3)?];
            let hit = at[0] >= b[0] && at[0] <= b[2] && at[1] >= b[1] && at[1] <= b[3];
            hit.then_some((b, (b[2] - b[0]) * (b[3] - b[1])))
        };
        let mut best: Option<(f64, Selected)> = None;
        let mut consider = |id: &Json, name: &str, bounds: &Json, bias: f64| {
            let (Some(id), Some((b, area))) = (id.as_str(), inside(bounds)) else {
                return;
            };
            let score = area * bias;
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((
                    score,
                    Selected {
                        id: id.to_string(),
                        name: name.to_string(),
                        bounds: b,
                    },
                ));
            }
        };
        for u in frame["ui"].as_array().into_iter().flatten() {
            consider(
                &u["id"],
                u["text"].as_str().unwrap_or(""),
                &u["bounds"],
                0.5,
            );
        }
        for r in frame["entities"].as_array().into_iter().flatten() {
            consider(&r[0], r[1].as_str().unwrap_or(""), &r[3], 1.0);
        }
        self.selected = best.map(|(_, s)| s);
    }
}

/// egui's mouse button, as the window names it.
fn mouse_name(b: egui::PointerButton) -> Option<&'static str> {
    Some(match b {
        egui::PointerButton::Primary => "left",
        egui::PointerButton::Secondary => "right",
        egui::PointerButton::Middle => "middle",
        egui::PointerButton::Extra1 => "back",
        egui::PointerButton::Extra2 => "forward",
    })
}

/// egui's key, as the window names it (winit's physical key codes: `KeyW`,
/// `Digit1`, `ArrowLeft`, `F8`), which the game's bindings and the daemon's
/// F8, F9, and F10 read.
pub fn key_name(k: egui::Key) -> Option<&'static str> {
    use egui::Key as K;
    Some(match k {
        K::ArrowDown => "ArrowDown",
        K::ArrowLeft => "ArrowLeft",
        K::ArrowRight => "ArrowRight",
        K::ArrowUp => "ArrowUp",
        K::Escape => "Escape",
        K::Tab => "Tab",
        K::Backspace => "Backspace",
        K::Enter => "Enter",
        K::Space => "Space",
        K::Insert => "Insert",
        K::Delete => "Delete",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "PageUp",
        K::PageDown => "PageDown",
        K::Comma => "Comma",
        K::Backslash => "Backslash",
        K::Slash => "Slash",
        K::OpenBracket => "BracketLeft",
        K::CloseBracket => "BracketRight",
        K::Backtick => "Backquote",
        K::Minus => "Minus",
        K::Period => "Period",
        K::Equals => "Equal",
        K::Semicolon => "Semicolon",
        K::Quote => "Quote",
        K::Num0 => "Digit0",
        K::Num1 => "Digit1",
        K::Num2 => "Digit2",
        K::Num3 => "Digit3",
        K::Num4 => "Digit4",
        K::Num5 => "Digit5",
        K::Num6 => "Digit6",
        K::Num7 => "Digit7",
        K::Num8 => "Digit8",
        K::Num9 => "Digit9",
        K::A => "KeyA",
        K::B => "KeyB",
        K::C => "KeyC",
        K::D => "KeyD",
        K::E => "KeyE",
        K::F => "KeyF",
        K::G => "KeyG",
        K::H => "KeyH",
        K::I => "KeyI",
        K::J => "KeyJ",
        K::K => "KeyK",
        K::L => "KeyL",
        K::M => "KeyM",
        K::N => "KeyN",
        K::O => "KeyO",
        K::P => "KeyP",
        K::Q => "KeyQ",
        K::R => "KeyR",
        K::S => "KeyS",
        K::T => "KeyT",
        K::U => "KeyU",
        K::V => "KeyV",
        K::W => "KeyW",
        K::X => "KeyX",
        K::Y => "KeyY",
        K::Z => "KeyZ",
        K::F1 => "F1",
        K::F2 => "F2",
        K::F3 => "F3",
        K::F4 => "F4",
        K::F5 => "F5",
        K::F6 => "F6",
        K::F7 => "F7",
        K::F8 => "F8",
        K::F9 => "F9",
        K::F10 => "F10",
        K::F11 => "F11",
        K::F12 => "F12",
        _ => return None,
    })
}
