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
    /// The player's settings (phase 5 item 2), once the page gave them.
    settings: Option<argus_player::settings::Store>,
    renderer: Option<argus_render::Renderer>,
    backend: String,
    last_ms: Option<f64>,
    owed: f64,
    frames: u64,
    drawn: usize,
    /// The game's code revision, which reports, marks, and saves record:
    /// its packs', since one prebuilt module plays any game (distribution
    /// item 1), else what the module was built for.
    code: String,
}

/// The module's own code (`argus build` sets it when it compiles one for a
/// game), when the packs name none.
fn built_for() -> String {
    option_env!("ARGUS_CODE").unwrap_or("none").to_string()
}

/// The code a pack report names, when it names one.
fn code_of(report: &Json) -> String {
    match report["code"].as_str() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => built_for(),
    }
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
        let (files, code) = if pack.starts_with(b"ARGP") {
            let p = argus_player::pack::Pack::decode(pack).map_err(js)?;
            let (files, report) = argus_player::pack::resolve(&[p], None).map_err(js)?;
            (files, code_of(&report))
        } else {
            (crate::unpack(pack).map_err(js)?, built_for())
        };
        WebPlayer::open(files, scene, seed, code)
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
        let (files, report) = argus_player::pack::resolve(&all, lang.as_deref()).map_err(js)?;
        WebPlayer::open(files, scene, seed, code_of(&report))
    }

    fn open(
        files: Vec<(String, Vec<u8>)>,
        scene: Option<String>,
        seed: Option<u32>,
        code: String,
    ) -> Result<WebPlayer, JsValue> {
        argus_os::fs::mount(ROOT, files);
        let mut p =
            Player::open(ROOT.as_ref(), scene.as_deref(), seed.map(u64::from)).map_err(js)?;
        p.record();
        Ok(WebPlayer {
            p,
            settings: None,
            renderer: None,
            backend: "none".into(),
            last_ms: None,
            owed: 0.0,
            frames: 0,
            drawn: 0,
            code,
        })
    }

    /// The player's settings as the page kept them (`settings.toml`'s text,
    /// empty for none), before the first frame: into the world and the
    /// map, and the recorder starts again so its first keyframe holds them
    /// (phase 5 item 2).
    pub fn apply_settings(&mut self, text: &str) {
        let text = (!text.trim().is_empty()).then_some(text);
        self.settings = Some(argus_player::settings::Store::open(&mut self.p, None, text));
        self.p.record();
    }

    /// The settings as the page should keep them, when the player changed
    /// them since the last call.
    pub fn settings_changed(&mut self) -> Option<String> {
        self.settings.as_mut()?.changed(&self.p)
    }

    /// Run `n` ticks.
    pub fn step(&mut self, n: u32) -> Result<(), JsValue> {
        self.p.step(n as u64).map_err(js)
    }

    /// Schedule an input log (`input_log`'s rows, as JSON) from now: how
    /// the determinism spike plays one run on every platform (phase 5
    /// item 11). Says how many rows were scheduled.
    pub fn replay_log(&mut self, log: &str) -> Result<u32, JsValue> {
        let v: Json = serde_json::from_str(log).map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.p.replay_log(&v).map(|n| n as u32).map_err(js)
    }

    /// Run `ticks` more, `every` at a time, and say the world's and the
    /// events' digests after each (`[[tick, world, events], ...]` as
    /// JSON): one call, not a call a checkpoint.
    pub fn digests(&mut self, ticks: u32, every: u32) -> Result<String, JsValue> {
        let every = every.max(1);
        let mut out = Vec::new();
        let mut done = 0;
        while done < ticks {
            let n = every.min(ticks - done);
            self.p.step(n as u64).map_err(js)?;
            done += n;
            out.push(serde_json::json!([
                self.p.tick(),
                format!("{:016x}", self.p.digest()),
                format!("{:016x}", self.p.events_digest()),
            ]));
        }
        Ok(Json::Array(out).to_string())
    }

    /// The world for a page script, as JSON: an entity (`@player`, `e:12`)
    /// as `{entity, components}`, one component (`path` "Health"), or one
    /// field ("Health.cur"); `path` "" for the whole entity (issue 26).
    pub fn get(&self, entity: &str, path: &str) -> Result<String, JsValue> {
        argus_player::read::get(&self.p.world, entity, path)
            .map(|j| j.to_string())
            .map_err(js)
    }

    /// Every entity with all these components (comma-separated, "Monster,
    /// Health"), oldest first, each with those components, as JSON.
    pub fn query(&self, components: &str) -> Result<String, JsValue> {
        argus_player::read::query(&self.p.world, components)
            .map(|j| j.to_string())
            .map_err(js)
    }

    /// What the world's digest reads, a line a field (see
    /// `Player::digest_rows`): the digest checks' own debug text, not for a
    /// game's page to read (`get` and `query` are).
    pub fn digest_rows(&self) -> String {
        self.p.digest_rows().join("\n")
    }

    /// The math probe (`argus_web::mathprobe`), `n` inputs a function, as
    /// JSON: this browser's results, to compare with a native run's.
    pub fn math_probe(n: u32) -> String {
        crate::mathprobe::math_probe(n as usize).to_string()
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

    /// Play the sound through the page's AudioWorklet (phase 5 item 10):
    /// the mixer at the AudioContext's rate, its blocks taken with
    /// `audio_pull` and posted to the worklet, whose own thread plays them
    /// from a queue, so a slow frame on the page does not starve the sound
    /// while the queue lasts. Says what opened.
    pub fn open_worklet(&mut self, rate: u32) -> String {
        match self.p.audio.as_mut() {
            Some(a) => {
                a.open_external(rate, "AudioWorklet");
                format!("AudioWorklet at {rate} Hz")
            }
            None => "this project has no audio".into(),
        }
    }

    /// The next `frames` of sound for the worklet, stereo interleaved.
    pub fn audio_pull(&mut self, frames: u32) -> Vec<f32> {
        let mut out = vec![0.0f32; frames.min(48_000) as usize * 2];
        if let Some(a) = &self.p.audio {
            a.pull(&mut out);
        }
        out
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
            .report(note, "report", &self.code)
            .unwrap_or_default()
    }

    /// A mark of this moment, as JSON, with the build's code revision (the
    /// spike's: its whole input log from the load).
    pub fn mark(&self, note: &str) -> String {
        self.p.mark(note, &self.code).to_string()
    }

    /// The saves the game asked for since the last call (`q:save`), as
    /// `[name, bytes]` pairs, which the page keeps in the browser's storage
    /// (IndexedDB; phase 3 item 0, decided with Kevin).
    pub fn take_saves(&mut self) -> js_sys::Array {
        let out = js_sys::Array::new();
        let code = self.code.clone();
        for (name, bytes) in self.p.take_saves(&code) {
            let pair = js_sys::Array::new();
            pair.push(&JsValue::from_str(&name));
            pair.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            out.push(&pair);
        }
        out
    }

    /// Text for the game's scripts, as a `host.message` event
    /// `{channel, text, slot}` (host channel item 0): recorded with the
    /// run's input, so a report carries it. On a discrete clock it is a
    /// turn, run inside this call: it returns 1, and `take_sent` has what
    /// the game answered straight after. On a continuous clock it returns
    /// 0, and the next tick the page runs has it.
    pub fn message(&mut self, channel: &str, text: &str) -> Result<u32, JsValue> {
        self.p.message(channel, text).map(|n| n as u32).map_err(js)
    }

    /// What the game sent its host since the last call (`q:send`), oldest
    /// first, as JSON `[{tick, channel, text}]` (host channel item 1).
    pub fn take_sent(&mut self) -> String {
        let rows: Vec<Json> = self
            .p
            .take_sent()
            .into_iter()
            .map(|(tick, channel, text)| serde_json::json!({ "tick": tick, "channel": channel, "text": text }))
            .collect();
        Json::Array(rows).to_string()
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
        r.set_occluders(self.p.tile_occluders(view));
        // On a discrete clock a frame draws the latest turn: no time owed
        // between turns to blend across. With the continuous clock's owed
        // time, 0 there, Delve drew a turn behind: a new direction showed
        // a step the old way first, and a new floor the hero where they
        // stood on the last one (found on the public games site,
        // 2026-10-10). As Scheduler::alpha has it.
        let alpha = match self.p.sched.driving().kind {
            argus_sim::ClockKind::Continuous => self.owed.clamp(0.0, 1.0) as f32,
            argus_sim::ClockKind::Discrete => 1.0,
        };
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
