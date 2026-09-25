//! What the page calls (wasm only): open a build's packs, step, feed keys,
//! report, and draw into a canvas. The player keeps the bounded recorder
//! from the start (§15, phase 2 item 7), so the page's report key sends the
//! last thirty to sixty seconds as a replay file.

use crate::Player;
use serde_json::Value as Json;
use wasm_bindgen::prelude::*;

/// Where a pack is mounted.
const ROOT: &str = "/game";

fn js(e: argus_ecs::ArgusError) -> JsValue {
    JsValue::from_str(&format!("{} ({})", e.message, e.hint))
}

/// A panic says what it was in the console, not only "unreachable".
fn hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            web_sys::console::error_1(&format!("argus panicked: {info}").into());
        }));
    });
}

/// The player, as the page holds it.
#[wasm_bindgen]
pub struct WebPlayer {
    p: Player,
    renderer: Option<argus_render::Renderer>,
    backend: String,
    last_ms: Option<f64>,
    owed: f64,
    frames: u64,
    drawn: usize,
}

#[wasm_bindgen]
impl WebPlayer {
    /// Mount a pack (a release build's game pack, or the spike's pack of
    /// every file) and open its scene (default: `argus.toml`'s) with a seed
    /// (default: the scene's).
    #[wasm_bindgen(constructor)]
    pub fn new(
        pack: &[u8],
        scene: Option<String>,
        seed: Option<u32>,
    ) -> Result<WebPlayer, JsValue> {
        hook();
        let files = if pack.starts_with(b"ARGP") {
            let p = argus_player::pack::Pack::decode(pack).map_err(js)?;
            argus_player::pack::resolve(&[p], None).map_err(js)?.0
        } else {
            crate::unpack(pack).map_err(js)?
        };
        WebPlayer::open(files, scene, seed)
    }

    /// A release build's packs (`data/packs.json` lists them): the game, its
    /// patches, its languages; `lang` picks one.
    pub fn from_packs(
        packs: js_sys::Array,
        lang: Option<String>,
        scene: Option<String>,
        seed: Option<u32>,
    ) -> Result<WebPlayer, JsValue> {
        hook();
        let mut all = Vec::new();
        for v in packs.iter() {
            let bytes = js_sys::Uint8Array::new(&v).to_vec();
            all.push(argus_player::pack::Pack::decode(&bytes).map_err(js)?);
        }
        let (files, _) = argus_player::pack::resolve(&all, lang.as_deref()).map_err(js)?;
        WebPlayer::open(files, scene, seed)
    }

    fn open(
        files: Vec<(String, Vec<u8>)>,
        scene: Option<String>,
        seed: Option<u32>,
    ) -> Result<WebPlayer, JsValue> {
        argus_os::fs::mount(ROOT, files);
        let mut p =
            Player::open(ROOT.as_ref(), scene.as_deref(), seed.map(u64::from)).map_err(js)?;
        p.record();
        Ok(WebPlayer {
            p,
            renderer: None,
            backend: "none".into(),
            last_ms: None,
            owed: 0.0,
            frames: 0,
            drawn: 0,
        })
    }

    /// Run `n` ticks.
    pub fn step(&mut self, n: u32) -> Result<(), JsValue> {
        self.p.step(n as u64).map_err(js)
    }

    /// `input.inject`: `value` is JSON (`[1, 0]`, `true`, `0.5`); `frames`
    /// 0 holds until the next inject of the action.
    pub fn inject(&mut self, action: &str, value: &str, frames: u32) -> Result<(), JsValue> {
        let v: Json = serde_json::from_str(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let (frames, hold) = if frames == 0 {
            (None, true)
        } else {
            (Some(frames as u64), false)
        };
        self.p.inject(action, Some(&v), frames, hold).map_err(js)
    }

    /// A key, by `KeyboardEvent.code`.
    pub fn key(&mut self, code: &str, pressed: bool) -> u32 {
        self.p.key(code, pressed) as u32
    }

    /// The mouse at a place on the canvas element, in its CSS pixels, with
    /// the element's CSS size: the pointer, on the UI canvas (phase 3 item
    /// 1). Returns turns taken, as a key does.
    pub fn pointer(&mut self, x: f32, y: f32, css_w: f32, css_h: f32) -> u32 {
        let Some(r) = self.renderer.as_ref() else {
            return 0;
        };
        let (w, h) = r.gpu().size();
        // CSS pixels to the drawing buffer's, then onto the UI canvas.
        let (bx, by) = (x * w as f32 / css_w.max(1.0), y * h as f32 / css_h.max(1.0));
        let Some(fit) = r.ui_fit() else {
            return 0;
        };
        let [cx, cy] = fit.to_canvas([bx, by]);
        self.p
            .devices(&[argus_input::DeviceInput::Pointer { x: cx, y: cy }]) as u32
    }

    /// A mouse button (`left`, `right`, `middle`), down or up.
    pub fn mouse(&mut self, button: &str, pressed: bool) -> u32 {
        self.p.devices(&[argus_input::DeviceInput::Mouse {
            button: button.to_string(),
            pressed,
        }]) as u32
    }

    /// The semantic frame (§8 L4) of the canvas now, as JSON: what a
    /// screen reader reads, and where the check finds the UI's buttons and
    /// focus (phase 3 item 1).
    pub fn semantic(&self) -> String {
        let (w, h) = self
            .renderer
            .as_ref()
            .map(|r| r.gpu().size())
            .unwrap_or((960, 540));
        let canvas = {
            let sh = self.p.ui.borrow();
            [sh.theme.width, sh.theme.height]
        };
        let store = self.p.particles.borrow();
        let sf = argus_render::semantic::semantic(
            &self.p.world,
            &argus_render::FrameSpec {
                width: w,
                height: h,
                limit: 24,
                groups: 12,
                assets: Some(&self.p.assets),
                particles: Some(&store),
                view: None,
                ui_canvas: Some(canvas),
            },
        );
        sf.to_json().to_string()
    }

    /// Whether the game's settings ask for fullscreen
    /// (`UiSettings.fullscreen`): the page asks the browser for it, which a
    /// browser grants only shortly after a key press or a click, the one
    /// that pressed the setting's button.
    pub fn fullscreen(&self) -> bool {
        argus_ui::wants_fullscreen(&self.p.world)
    }

    /// The canvas lost focus: every key lets go.
    pub fn release_all(&mut self) {
        self.p.release_all();
    }

    /// The status line, as JSON: tick, entities, events, sounds, digests,
    /// the backend drawing, frames drawn.
    pub fn status(&self) -> String {
        let mut s = self.p.status();
        s["backend"] = self.backend.clone().into();
        s["frames"] = self.frames.into();
        s["drawn"] = self.drawn.into();
        s["player"] = match self.p.position("@player") {
            Some([x, y]) => serde_json::json!([x, y]),
            None => Json::Null,
        };
        if let Some(a) = &self.p.audio {
            s["audio"] = serde_json::json!({
                "device": a.device_name(),
                "rendered": a.rendered_frames(),
                "voices": a.voices(),
            });
        }
        s.to_string()
    }

    /// Open the browser's audio output (Web Audio). A browser lets a page
    /// make sound only after the player has pressed a key or clicked, so
    /// the page calls this from its first key or click. Says what opened.
    pub fn open_audio(&mut self) -> Result<String, JsValue> {
        let Some(a) = self.p.audio.as_mut() else {
            return Ok("this project has no audio".into());
        };
        a.open_device().map_err(js)?;
        Ok(a.device_name().unwrap_or_default())
    }

    /// Output levels since the last call, as JSON: the peak and RMS after
    /// the limiter, in dBFS.
    pub fn audio_levels(&self) -> String {
        match &self.p.audio {
            Some(a) => {
                let (_, peak, rms, _) = a.meters();
                serde_json::json!({ "peak_db": peak, "rms_db": rms }).to_string()
            }
            None => "null".into(),
        }
    }

    /// The input log so far (what a mark from the browser carries).
    pub fn input_log(&self) -> String {
        self.p.input_log().to_string()
    }

    /// A report of the last thirty to sixty seconds, with a note: a replay
    /// file (ARGR) the page posts to the server it came from, which
    /// `argus mark pull` turns into marks.
    pub fn report(&self, note: &str) -> Vec<u8> {
        self.p
            .report(note, "report", option_env!("ARGUS_CODE").unwrap_or("none"))
            .unwrap_or_default()
    }

    /// A mark of this moment, as JSON, with the build's code revision (the
    /// spike's: its whole input log from the load).
    pub fn mark(&self, note: &str) -> String {
        self.p
            .mark(note, option_env!("ARGUS_CODE").unwrap_or("none"))
            .to_string()
    }

    /// The saves the game asked for since the last call (`q:save`), as
    /// `[name, bytes]` pairs, which the page keeps in the browser's storage
    /// (IndexedDB; phase 3 item 0, decided with Kevin).
    pub fn take_saves(&mut self) -> js_sys::Array {
        let out = js_sys::Array::new();
        for (name, bytes) in self
            .p
            .take_saves(option_env!("ARGUS_CODE").unwrap_or("none"))
        {
            let pair = js_sys::Array::new();
            pair.push(&JsValue::from_str(&name));
            pair.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            out.push(&pair);
        }
        out
    }

    /// The saves the game asked to load since the last call (`q:load`), by
    /// name; the page finds each in its storage and hands it to `load`.
    pub fn take_loads(&mut self) -> js_sys::Array {
        self.p
            .take_loads()
            .into_iter()
            .map(|n| JsValue::from_str(&n))
            .collect()
    }

    /// Load a save's bytes; what was migrated, as JSON. On an error the game
    /// goes on as it was.
    pub fn load(&mut self, bytes: &[u8]) -> Result<String, JsValue> {
        self.p.load(bytes).map(|r| r.to_string()).map_err(js)
    }

    /// Use a renderer made by [`open_renderer`]; the pack's sprites load into it.
    pub fn use_renderer(&mut self, r: WebRenderer) -> Result<(), JsValue> {
        let mut renderer = r.renderer;
        renderer.load_catalog(&self.p.assets).map_err(js)?;
        self.backend = r.backend;
        self.renderer = Some(renderer);
        Ok(())
    }

    /// One animation frame at `now_ms` (`requestAnimationFrame`'s time):
    /// with `run`, the ticks the elapsed time owes on the continuous clock
    /// (at most five, so a stalled tab does not spiral); then a picture.
    /// Without it only the picture, which is how a check steps exactly.
    pub fn frame(&mut self, now_ms: f64, run: bool) -> Result<(), JsValue> {
        let hz = self.p.sched.driving().hz();
        if run && hz > 0.0 && self.p.sched.driving().kind == argus_sim::ClockKind::Continuous {
            let dt = self.last_ms.map(|l| (now_ms - l) / 1000.0).unwrap_or(0.0);
            let speed = self.p.sched.speed();
            self.owed = (self.owed + dt * hz * speed).min(5.0 * speed.max(1.0));
            let n = self.owed.floor();
            self.owed -= n;
            if n > 0.0 {
                self.p.step(n as u64).map_err(js)?;
            }
        }
        self.last_ms = Some(now_ms);
        let _ = self.p.sched.present(&mut self.p.world);
        let Some(r) = self.renderer.as_mut() else {
            return Ok(());
        };
        let (w, h) = r.gpu().size();
        let view = argus_render::view_for(&self.p.world, w, h);
        let extras = self.p.tile_extras(view);
        let alpha = self.owed.clamp(0.0, 1.0) as f32;
        r.set_time(self.p.sched.time() as f32);
        r.set_ui(Some(self.p.ui.clone()));
        let store = self.p.particles.borrow();
        let tick = self.p.sched.tick();
        let rep = r
            .render_frame(&self.p.world, tick, alpha, &extras, Some(&store))
            .map_err(js)?;
        self.drawn = rep.stats.sprites;
        self.frames += 1;
        Ok(())
    }
}

/// A renderer bound to a canvas: made asynchronously, since a browser
/// cannot block while the GPU is found.
#[wasm_bindgen]
pub struct WebRenderer {
    renderer: argus_render::Renderer,
    backend: String,
}

/// Make a renderer for a canvas: WebGPU where the browser has it, else
/// WebGL2; `webgl` forces WebGL2.
#[wasm_bindgen]
pub async fn open_renderer(
    canvas: web_sys::HtmlCanvasElement,
    webgl: bool,
) -> Result<WebRenderer, JsValue> {
    hook();
    let renderer = argus_render::Renderer::web(canvas, webgl)
        .await
        .map_err(js)?;
    let backend = renderer.gpu().adapter.clone();
    Ok(WebRenderer { renderer, backend })
}
