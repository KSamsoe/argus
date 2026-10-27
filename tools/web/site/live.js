// Hollow, live on the front page, and what an agent sees of it: the
// semantic frame `observe` gives, read from the running game. The game is
// the site's own web build of Hollow, in hollow/ beside this file
// (tools/web/publish-games.sh puts both there); nothing here is faked.
//
//   import { startGame } from "./live.js";
//   const game = await startGame(canvas, {
//     onFrame(frame) { ... },   // about ten times a second, while it runs
//     every: 6,                 // animation frames between two onFrame calls
//   });
//
// `frame` is the semantic frame as the agent reads it, parsed:
//   frame.entities  [{id, name, text, bounds, z, look}]: what is on screen
//                   (the agent gets these as rows under frame.columns;
//                   here each is an object)
//   frame.ui        [{id, text, bounds, button, focused, ...}]: UI text and
//                   menus (Hollow's line at the top is one)
//   frame.lighting  {ambient, level, shadows, lights: [{id, at: [x, y],
//                   color, intensity, radius_px, kind, flicker}]}
//   frame.camera    {id, angle, rect: the world rectangle in view}
//   frame.groups, frame.hud, frame.effects, frame.problems: often empty
//   frame.size      [w, h] of the picture the bounds and `at` are in
// plus what this file adds:
//   frame.tick      the game's tick (60 a second)
//   frame.digest    a hash of the whole world at that tick (16 hex digits):
//                   a replay reaches the same one at the same tick
//   frame.bytes     how big the JSON the agent gets is, in bytes
//   frame.json      that JSON, as the agent gets it
//   frame.box(b)    a bounds [x0, y0, x1, y1] as {x, y, w, h} in CSS
//                   pixels from the canvas's top left, to draw over it
//
// `game`:
//   game.player     the WebPlayer itself
//   game.frame()    the latest frame (or null before the first)
//   game.replay()   start the game again and play back every key pressed
//                   so far, tick for tick: the same seed and the same input
//                   give the same run (resolves when it has started)
//   game.replaying  true while a replay plays
//   game.stop()     stop drawing and let go of the input
//
// The game draws only while the canvas is on screen. Keys reach it while
// the canvas has the focus (a click on it gives it the focus; Tab moves on
// as usual). A finger on the canvas is the game's (its touch controls), so
// the canvas sets touch-action: none; leave room around it to scroll by.
// It plays no sound. If the browser cannot run it (no WebGPU or WebGL2, or
// WebAssembly refused), startGame rejects: show a still or a clip instead.
const GAME = new URL("./hollow/", import.meta.url);

// The build's player and its packs (data/packs.json lists them, as for the
// game's own page), loaded once.
let ready = null;
const load = async () => {
  ready ??= (async () => {
    const web = await import(new URL("pkg/argus_web.js", GAME).href);
    await web.default();
    const got = async (name) => {
      const r = await fetch(new URL(name, GAME));
      if (!r.ok) throw new Error(`the game's ${name} did not load (${r.status})`);
      return r;
    };
    const listed = await (await got("data/packs.json")).json();
    const packs = [];
    for (const name of listed.packs) packs.push(new Uint8Array(await (await got("data/" + name)).arrayBuffer()));
    return { web, packs };
  })();
  return ready;
};

export async function startGame(canvas, opts = {}) {
  const every = Math.max(1, opts.every ?? 6);
  const { web, packs } = await load();
  const fresh = async () => {
    const p = web.WebPlayer.from_packs(packs, undefined, undefined, undefined);
    p.use_renderer(await web.open_renderer(canvas, !!opts.webgl));
    return p;
  };
  let player = await fresh();
  canvas.style.touchAction = "none";
  if (!canvas.hasAttribute("tabindex")) canvas.tabIndex = 0;

  const game = { player, replaying: false, frame: () => last, replay, stop };
  let last = null;
  let stopped = false;
  let onScreen = true;
  let replayEnds = -1;

  // The keys the game holds down, as this page told it.
  const held = new Set();
  const keydown = (e) => {
    if (e.code === "Tab") return;
    e.preventDefault();
    if (game.replaying || e.repeat) return;
    held.add(e.code);
    player.key(e.code, true);
  };
  const keyup = (e) => {
    if (e.code === "Tab") return;
    e.preventDefault();
    held.delete(e.code);
    if (!game.replaying) player.key(e.code, false);
  };
  // A finger is the game's touch control; a mouse is its pointer.
  const fingers = new Set();
  const touchAt = (e, phase) => {
    const r = canvas.getBoundingClientRect();
    player.touch(e.pointerId, phase, e.clientX - r.left, e.clientY - r.top, r.width, r.height);
  };
  const point = (e) => {
    const r = canvas.getBoundingClientRect();
    player.pointer(e.clientX - r.left, e.clientY - r.top, r.width, r.height);
  };
  const buttons = ["left", "middle", "right"];
  const down = (e) => {
    canvas.focus();
    if (game.replaying) return;
    if (e.pointerType === "touch") {
      e.preventDefault();
      canvas.setPointerCapture?.(e.pointerId);
      fingers.add(e.pointerId);
      touchAt(e, "start");
      return;
    }
    point(e);
    if (buttons[e.button]) player.mouse(buttons[e.button], true);
  };
  const move = (e) => {
    if (game.replaying) return;
    if (e.pointerType === "touch") {
      if (fingers.has(e.pointerId)) touchAt(e, "move");
      return;
    }
    point(e);
  };
  const up = (e) => {
    if (game.replaying) return;
    if (e.pointerType === "touch") {
      if (fingers.delete(e.pointerId)) touchAt(e, "end");
      return;
    }
    point(e);
    if (buttons[e.button]) player.mouse(buttons[e.button], false);
  };
  const cancel = (e) => {
    if (e.pointerType === "touch" && fingers.delete(e.pointerId) && !game.replaying) touchAt(e, "cancel");
  };
  const blur = () => {
    held.clear();
    fingers.clear();
    if (!game.replaying) player.release_all();
  };
  const noMenu = (e) => e.preventDefault();
  const on = [
    ["keydown", keydown],
    ["keyup", keyup],
    ["pointerdown", down],
    ["pointermove", move],
    ["pointerup", up],
    ["pointercancel", cancel],
    ["blur", blur],
    ["contextmenu", noMenu],
  ];
  for (const [k, f] of on) canvas.addEventListener(k, f);

  // Draw only while the canvas is on screen.
  const seen = new IntersectionObserver((es) => {
    for (const e of es) onScreen = e.isIntersecting;
  });
  seen.observe(canvas);

  // The drawing buffer as big as the canvas is on the screen, at most two
  // screen pixels to a CSS pixel and about 1920 by 1080 in all.
  const fit = () => {
    const r = canvas.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) return;
    let k = Math.min(window.devicePixelRatio || 1, 2);
    const most = 1920 * 1080;
    if (r.width * r.height * k * k > most) k = Math.sqrt(most / (r.width * r.height));
    const w = Math.round(r.width * k);
    const h = Math.round(r.height * k);
    if (w === canvas.width && h === canvas.height) return;
    canvas.width = w;
    canvas.height = h;
    player.resize(w, h);
  };

  const read = () => {
    const json = player.semantic();
    const f = JSON.parse(json);
    // The agent gets each entity as a row under `columns`; here, objects.
    if (Array.isArray(f.columns) && Array.isArray(f.entities)) {
      f.entities = f.entities.map((r) => (Array.isArray(r) ? Object.fromEntries(f.columns.map((c, i) => [c, r[i]])) : r));
    }
    const s = JSON.parse(player.status());
    f.tick = s.tick;
    f.digest = s.digest;
    f.json = json;
    f.bytes = new TextEncoder().encode(json).length;
    const [w, h] = f.size ?? [canvas.width, canvas.height];
    f.box = (b) => {
      const r = canvas.getBoundingClientRect();
      const kx = r.width / w;
      const ky = r.height / h;
      return { x: b[0] * kx, y: b[1] * ky, w: (b[2] - b[0]) * kx, h: (b[3] - b[1]) * ky };
    };
    last = f;
    if (game.replaying && s.tick >= replayEnds) game.replaying = false;
    opts.onFrame?.(f);
  };

  let n = 0;
  const frame = (t) => {
    if (stopped) return;
    if (onScreen) {
      try {
        fit();
        player.frame(t, true);
        if (n++ % every === 0) read();
      } catch (e) {
        stopped = true;
        opts.onError?.(e);
        return;
      }
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  // The same run again: a new game from the same seed, given the input
  // log of this one from its first tick.
  async function replay() {
    const log = player.input_log();
    const s = JSON.parse(player.status());
    blur();
    const next = await fresh();
    next.replay_log(log);
    player.free();
    player = next;
    game.player = next;
    replayEnds = s.tick;
    game.replaying = true;
  }

  function stop() {
    stopped = true;
    seen.disconnect();
    for (const [k, f] of on) canvas.removeEventListener(k, f);
    blur();
  }

  return game;
}
