// The page around the web player: load the build's packs, make a renderer
// for the canvas, feed it keys, run a frame per animation frame, send
// reports, and keep the game's saves.
//
// Query parameters: scene, seed; lang (a language pack); webgl (force
// WebGL2); paused (draw but do not advance: how the headless checks step
// the sim exactly); audio_ms (the sound queue's depth); cpal (the sound
// through cpal's Web Audio output, not the AudioWorklet).
import init, { WebPlayer, open_renderer } from "./pkg/argus_web.js";

const q = new URLSearchParams(location.search);

// The sound's AudioWorklet (phase 5 item 10): plays the blocks the page
// posts, in order, on the browser's audio thread; counts the render quanta
// it could not fill once it has started (underruns); says how much it has
// played, which is how deep the page keeps the queue.
const WORKLET = `
class Out extends AudioWorkletProcessor {
  constructor() {
    super();
    this.q = [];
    this.at = 0;
    this.played = 0;
    this.under = 0;
    this.started = false;
    this.n = 0;
    this.port.onmessage = (e) => this.q.push(e.data);
  }
  process(inputs, outputs) {
    const [l, r] = outputs[0];
    let i = 0;
    while (i < l.length && this.q.length) {
      const b = this.q[0];
      const take = Math.min(l.length - i, (b.length - this.at) / 2);
      for (let k = 0; k < take; k++) {
        l[i + k] = b[this.at + 2 * k];
        if (r) r[i + k] = b[this.at + 2 * k + 1];
      }
      i += take;
      this.at += take * 2;
      this.played += take;
      if (this.at >= b.length) {
        this.q.shift();
        this.at = 0;
      }
    }
    if (i < l.length) {
      l.fill(0, i);
      if (r) r.fill(0, i);
      if (this.started) this.under++;
    }
    if (i > 0) this.started = true;
    if (++this.n % 8 === 0) this.port.postMessage({ played: this.played, under: this.under });
    return true;
  }
}
registerProcessor("argus-out", Out);
`;
const status = document.getElementById("status");
const said = document.getElementById("said");
const canvas = document.getElementById("game");
const form = document.getElementById("report");
const note = document.getElementById("note");

// The game's own page script (web/game.js in its project), when packs.json
// names one: imported once the player is ready (host channel item 2).
let pageScript = null;

// A release build lists its packs in data/packs.json (argus build); the
// spike's page has one pack of every file.
async function loadPlayer() {
  const scene = q.get("scene") ?? undefined;
  const seed = q.has("seed") ? Number(q.get("seed")) : undefined;
  const listed = await fetch("data/packs.json")
    .then((r) => (r.ok ? r.json() : null))
    .catch(() => null);
  if (listed) {
    pageScript = listed.script ?? null;
    const packs = [];
    for (const name of listed.packs) {
      packs.push(new Uint8Array(await (await fetch("data/" + name)).arrayBuffer()));
    }
    return WebPlayer.from_packs(packs, q.get("lang") ?? undefined, scene, seed);
  }
  const pack = new Uint8Array(await (await fetch("game.pack")).arrayBuffer());
  return new WebPlayer(pack, scene, seed);
}

// A report goes to the server the page came from (tools/web/serve.mjs);
// where there is none, it is offered as a file instead.
async function send(bytes) {
  said.textContent = "Sending the report.";
  try {
    const r = await fetch("marks", { method: "POST", headers: { "content-type": "application/octet-stream" }, body: bytes });
    if (!r.ok) throw new Error(`the server said ${r.status}`);
    said.textContent = "Report sent. Thank you.";
  } catch (e) {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
    a.download = `report-${Date.now()}.argr`;
    a.textContent = "save the report";
    said.textContent = `The report could not be sent (${e.message}). You can `;
    said.append(a, " and send the file to the developer.");
  }
}

// Saves the game asks for (q:save) live in the browser's storage, so a
// reload keeps them; a load it asks for (q:load) finds one there (phase 3
// item 0). One IndexedDB database per game, a save per name.
function saveStore(game) {
  const opened = new Promise((ok, fail) => {
    const r = indexedDB.open(`argus:${game}`, 1);
    r.onupgradeneeded = () => r.result.createObjectStore("saves");
    r.onsuccess = () => ok(r.result);
    r.onerror = () => fail(r.error);
  });
  const use = async (mode, f) => {
    const db = await opened;
    return new Promise((ok, fail) => {
      const req = f(db.transaction("saves", mode).objectStore("saves"));
      req.onsuccess = () => ok(req.result);
      req.onerror = () => fail(req.error);
    });
  };
  return {
    put: (name, bytes) => use("readwrite", (s) => s.put(bytes, name)),
    get: (name) => use("readonly", (s) => s.get(name)),
    names: () => use("readonly", (s) => s.getAllKeys()),
  };
}

try {
  await init();
  const player = await loadPlayer();
  const game = JSON.parse(player.status()).game;
  const store = saveStore(game);
  // The player's settings (phase 5 item 2): kept in the browser, applied
  // before the first frame, kept again when the player changes them.
  const settingsKey = `argus:${game}:settings`;
  const kept = (() => {
    try {
      return localStorage.getItem(settingsKey) ?? "";
    } catch {
      return "";
    }
  })();
  player.apply_settings(kept);
  const keepSettings = () => {
    const s = player.settings_changed();
    if (s === undefined) return;
    try {
      localStorage.setItem(settingsKey, s);
    } catch {}
  };
  // After a frame: keep the saves it asked for, and do the loads.
  const savesAndLoads = () => {
    for (const [name, bytes] of player.take_saves()) {
      store.put(name, bytes).catch((e) => (said.textContent = `The game could not be saved: ${e}`));
    }
    for (const name of player.take_loads()) {
      store
        .get(name)
        .then((bytes) => {
          if (!bytes) throw new Error(`there is no save called ${name}`);
          player.load(bytes);
        })
        .catch((e) => (said.textContent = `The save could not be loaded: ${e.message ?? e}`));
    }
  };
  player.use_renderer(await open_renderer(canvas, q.has("webgl")));
  let run = !q.has("paused");

  // A browser lets a page make sound only once the player has pressed a
  // key or clicked, so the first of those opens the audio. It plays on the
  // browser's audio thread, in an AudioWorklet, from a queue this page keeps
  // about 100 ms deep (?audio_ms= for another depth), so a slow frame here
  // does not starve it while the queue lasts (phase 5 item 10). Where there
  // is no AudioWorklet (a page over plain http from another machine is not
  // a secure context), or with ?cpal, cpal's Web Audio output plays it,
  // whose buffers this thread schedules.
  const sound = { mode: "none", rate: 0, depth_ms: Number(q.get("audio_ms") ?? 100), sent: 0, played: 0, under: 0 };
  let out = null;
  const feed = () => {
    if (!out) return;
    let need = Math.round((sound.rate * sound.depth_ms) / 1000) - (sound.sent - sound.played);
    while (need >= 128) {
      const n = Math.min(need - (need % 128), 2048);
      const block = player.audio_pull(n);
      out.port.postMessage(block, [block.buffer]);
      sound.sent += n;
      need -= n;
    }
  };
  const openWorklet = async () => {
    if (q.has("cpal")) throw new Error("the page asked for cpal");
    const ctx = new AudioContext({ latencyHint: "interactive" });
    try {
      if (!ctx.audioWorklet) throw new Error("not a secure context");
      const url = URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }));
      await ctx.audioWorklet.addModule(url);
      URL.revokeObjectURL(url);
      out = new AudioWorkletNode(ctx, "argus-out", { numberOfInputs: 0, outputChannelCount: [2] });
      out.port.onmessage = (e) => {
        sound.played = e.data.played;
        sound.under = e.data.under;
      };
      out.connect(ctx.destination);
      if (ctx.state !== "running") await ctx.resume();
    } catch (e) {
      out = null;
      ctx.close();
      throw e;
    }
    player.open_worklet(ctx.sampleRate);
    Object.assign(sound, { mode: "worklet", rate: ctx.sampleRate });
    feed();
    setInterval(feed, 10);
  };
  let audio = false;
  const openAudio = () => {
    if (audio) return;
    audio = true;
    openWorklet().catch((e) => {
      console.warn(`no AudioWorklet (${e.message ?? e}): cpal's Web Audio output instead`);
      try {
        player.open_audio();
        sound.mode = "cpal";
      } catch (e) {
        console.warn("no audio: " + e);
      }
    });
  };

  // F8: pause and ask what happened.
  let wasRunning = run;
  const openReport = () => {
    wasRunning = run;
    run = false;
    player.release_all();
    form.hidden = false;
    note.value = "";
    note.focus();
  };
  const closeReport = () => {
    form.hidden = true;
    run = wasRunning;
    canvas.focus();
  };
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    const bytes = player.report(note.value.trim());
    closeReport();
    send(bytes);
  });
  document.getElementById("cancel").addEventListener("click", closeReport);
  note.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeReport();
  });

  canvas.addEventListener("pointerdown", openAudio);
  canvas.addEventListener("keydown", (e) => {
    openAudio();
    e.preventDefault();
    if (e.code === "F8") {
      if (!e.repeat) openReport();
      return;
    }
    if (!e.repeat) player.key(e.code, true);
  });
  canvas.addEventListener("keyup", (e) => {
    player.key(e.code, false);
    e.preventDefault();
  });
  // The mouse: where it is over the canvas is the pointer on the UI
  // canvas, and its buttons are buttons (phase 3 item 1).
  const buttons = ["left", "middle", "right"];
  const point = (e) => {
    const r = canvas.getBoundingClientRect();
    player.pointer(e.clientX - r.left, e.clientY - r.top, r.width, r.height);
  };
  canvas.addEventListener("pointermove", point);
  canvas.addEventListener("pointerdown", (e) => {
    canvas.focus();
    point(e);
    if (buttons[e.button]) player.mouse(buttons[e.button], true);
  });
  canvas.addEventListener("pointerup", (e) => {
    point(e);
    if (buttons[e.button]) player.mouse(buttons[e.button], false);
  });
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("blur", () => player.release_all());
  canvas.focus();

  // What the headless checks reach for.
  // openReport is F8's form, for a page script whose own text box has the
  // keys (the game hears them only while the canvas has focus).
  window.argus = { player, setRunning: (r) => (run = r), report: (n) => player.report(n), openReport, send, store, savesAndLoads };
  // The sound's queue: how deep it is now, and the render quanta (128
  // frames each) the worklet could not fill since it started.
  window.argus.sound = () => ({ ...sound, queued_ms: sound.rate ? Math.round(((sound.sent - sound.played) * 1000) / sound.rate) : 0 });

  // The game's own page script, with window.argus set: what it sends the
  // game reaches the scripts with player.message, and what the game sends
  // its host it reads with player.take_sent (host channel items 0 to 2).
  if (pageScript) {
    import("./" + pageScript).catch((e) => console.error(`${pageScript}: ${e}`));
  }

  // The game's settings say whether it fills the screen (phase 3 item 1).
  // A browser allows that only shortly after a key press or a click, which
  // the one that pressed the setting's button is; asked once per change.
  let fullscreen = false;
  const fitScreen = () => {
    const want = player.fullscreen();
    if (want === fullscreen) return;
    fullscreen = want;
    if (want && !document.fullscreenElement) canvas.requestFullscreen?.().catch(() => {});
    if (!want && document.fullscreenElement) document.exitFullscreen?.().catch(() => {});
  };

  // The game for the browser's screen reader (phase 5 item 2, the
  // player's own screen reader): the canvas is an application, so keys go
  // to the game; the semantic frame's buttons are buttons here, off the
  // screen, that press the game's (the pointer to it and a click, input
  // the run records as it does a mouse's); and what changes is said: the
  // focus as it moves, and the HUD.
  canvas.setAttribute("aria-label", game);
  const reader = {
    buttons: document.getElementById("reader-buttons"),
    said: document.getElementById("reader-said"),
    hud: document.getElementById("reader-hud"),
    ui: "",
    focus: "",
    lines: "",
  };
  const pressFor = (u) => () => {
    const r = canvas.getBoundingClientRect();
    const x = ((u.bounds[0] + u.bounds[2]) / 2) * (r.width / canvas.width);
    const y = ((u.bounds[1] + u.bounds[3]) / 2) * (r.height / canvas.height);
    player.pointer(x, y, r.width, r.height);
    try {
      player.inject("ui_click", "true", 1);
    } catch {}
    canvas.focus();
  };
  const read = () => {
    const f = JSON.parse(player.semantic());
    const ui = f.ui ?? [];
    const shape = JSON.stringify(ui.map((u) => [u.id, u.text, !!u.button, !!u.disabled]));
    if (shape !== reader.ui) {
      reader.ui = shape;
      reader.buttons.replaceChildren(
        ...ui.map((u) => {
          const el = document.createElement(u.button ? "button" : "p");
          el.textContent = u.text;
          el.dataset.id = u.id;
          if (u.button) {
            el.disabled = !!u.disabled;
            el.addEventListener("click", pressFor(u));
          }
          return el;
        }),
      );
    }
    const focused = ui.find((u) => u.button && u.focused);
    const words = focused ? `${focused.text}, button` : "";
    if (words !== reader.focus) {
      reader.focus = words;
      if (words) reader.said.textContent = words;
    }
    const lines = (f.hud ?? []).map((h) => [h.name, h.text].filter(Boolean).join(": ")).join(". ");
    if (lines !== reader.lines) {
      reader.lines = lines;
      reader.hud.textContent = lines;
    }
  };
  window.argus.read = read;

  let n = 0;
  const frame = (t) => {
    try {
      player.frame(t, run);
      savesAndLoads();
      fitScreen();
      if (n % 30 === 0) keepSettings();
      if (n % 10 === 0) read();
    } catch (e) {
      status.textContent = "Stopped: " + e;
      return;
    }
    if (n++ % 30 === 0) {
      const s = JSON.parse(player.status());
      status.textContent = `${s.scene}  tick ${s.tick}  ${s.backend}  sprites ${s.drawn}  hero ${s.player?.map((v) => v.toFixed(2)).join(", ")}`;
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
} catch (e) {
  status.textContent = "Could not start: " + e;
  console.error(e);
}
