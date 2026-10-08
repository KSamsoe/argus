// Check a built site in headless Chrome, over the DevTools protocol, with no
// window on anyone's screen:
//
//   node tools/web/check.mjs [site] [out-dir] [project]
//
// Serves the site, opens it in headless Chrome, and reports: the backend the
// renderer got and what it drew (with a screenshot), the run of Hollow's
// scenario stepped exactly in the page (its digests, to compare with the
// native player's), real key presses moving the hero and opening the audio,
// a mark of that moment replayed natively (the same world or not), the
// hero falling into the hall's pit (the post stack's flash, shake, and
// hit-stop, marked inside the stop and replayed natively), a report of it
// sent the way the page's F8 sends one, through the server and back, and
// replayed natively (phase 2 item 7), the UI (phase 3 item 1: the pause
// menu opened by a real key, a button focused by the real mouse and pressed
// by a click), and the same picture with WebGL2 forced. First, a cold load
// over a playtester's link (phase 5 item 0, section 17: under 5 s on 20
// Mbit). It exits 1 when anything failed (CI gates on it). It cannot hang
// or meet another run (phase 5 item 1: a CI run waited two hours here): its
// ports are free ones, every DevTools call has a minute, the whole check
// five, and Chrome's own output is kept in <out-dir>/chrome.log. Node 24
// and cargo, no npm packages.
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync, openSync, readFileSync, existsSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const site = process.argv[2] ?? "target-web/site";
const out = process.argv[3] ?? "target-web/check";
const project = process.argv[4] ?? "examples/hollow";
const chrome = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(out, { recursive: true });
// A port nobody is using, so a check never meets another one (the runner's
// and a person's) or a playtest server on the default port.
const freePort = () =>
  new Promise((ok) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const port = s.address().port;
      s.close(() => ok(port));
    });
  });
const HTTP = await freePort();

const server = spawn(process.execPath, ["tools/web/serve.mjs", site, String(HTTP), "127.0.0.1", join(out, "marks")], { stdio: "ignore" });
const profile = mkdtempSync(join(tmpdir(), "argus-chrome-"));
const chromeLog = openSync(join(out, "chrome.log"), "w");
// Chrome picks its own debugging port and writes it in the profile.
const browser = spawn(chrome, [
  "--headless=new",
  "--remote-debugging-port=0",
  `--user-data-dir=${profile}`,
  "--no-first-run",
  "--no-default-browser-check",
  "--enable-unsafe-webgpu",
  "--autoplay-policy=no-user-gesture-required",
  // The audio runs, but nothing reaches the speakers of whoever's machine this is.
  "--mute-audio",
  "about:blank",
], { stdio: ["ignore", chromeLog, chromeLog] });
// A Chrome that cannot start is a failed check with a report, not a crash.
let chromeError = null;
browser.on("error", (e) => (chromeError = e));

let ws, nextId = 1;
const waiting = new Map();
const logs = [];
let bytes = 0;
// Where the check is, for a report that stops part way.
let stage = "starting Chrome";
// A DevTools call: its answer, or an error after `ms` (Chrome gone quiet).
function send(method, params = {}, ms = 60000) {
  const id = nextId++;
  return new Promise((ok, fail) => {
    const timer = setTimeout(() => {
      waiting.delete(id);
      fail(new Error(`${method}: no answer from Chrome in ${ms / 1000} s`));
    }, ms);
    const done = (f) => (v) => {
      clearTimeout(timer);
      f(v);
    };
    waiting.set(id, { ok: done(ok), fail: done(fail), method });
    try {
      ws.send(JSON.stringify({ id, method, params }));
    } catch (e) {
      waiting.delete(id);
      done(fail)(e);
    }
  });
}
async function evaluate(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(`${expr}: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
  return r.result.value;
}
async function open(url) {
  logs.length = 0;
  await send("Page.navigate", { url });
  for (let i = 0; i < 200; i++) {
    await sleep(100);
    const ready = await evaluate("!!window.argus || document.getElementById('status')?.textContent.startsWith('Could not')").catch(() => false);
    if (ready) break;
  }
  const failed = await evaluate("document.getElementById('status').textContent");
  if (failed.startsWith("Could not")) throw new Error(failed + "\n" + logs.join("\n"));
}
const status = async () => JSON.parse(await evaluate("argus.player.status()"));
async function screenshot(name) {
  const r = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 12, width: 960, height: 540, scale: 1 } });
  writeFileSync(join(out, name), Buffer.from(r.data, "base64"));
  return join(out, name);
}
async function key(code, keyText, vk, down) {
  await send("Input.dispatchKeyEvent", { type: down ? "keyDown" : "keyUp", code, key: keyText, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk });
}

const report = {};
// The verdict: an error, a frame that drew nothing, a world or a digest
// that is not the same as the native player's, or a slow cold load.
function verdict() {
  report.console = logs.slice(-20);
  const failures = [];
  if (report.error) failures.push("error");
  if (!(report.cold_load?.ms <= report.cold_load?.bound_ms)) failures.push(`cold load ${report.cold_load?.ms} ms`);
  if (!(report.render?.drawn > 0)) failures.push("nothing drawn");
  if (report.webgl && !(report.webgl.drawn > 0)) failures.push("nothing drawn with WebGL2");
  if (report.reader) {
    if (!(report.reader.buttons?.length >= 4)) failures.push("the screen reader sees no menu");
    if (!report.reader.said) failures.push("the screen reader is not told the focus");
    if (!report.reader.pressed_controls) failures.push("a screen reader's press did not reach the game");
  }
  const walk = (v, at) => {
    if (v && typeof v === "object") {
      for (const [k, x] of Object.entries(v)) {
        if (k === "same" && x === false) failures.push(`${at}: not the same`);
        walk(x, at ? `${at}.${k}` : k);
      }
    }
  };
  walk(report, "");
  report.ok = failures.length === 0;
  report.failures = failures;
  process.exitCode = report.ok ? 0 : 1;
  console.log(JSON.stringify(report, null, 2));
}
// The whole check has five minutes; it usually takes under one.
const watchdog = setTimeout(() => {
  report.error = `${stage}: the check took over five minutes (see chrome.log)`;
  verdict();
  browser.kill();
  server.kill();
  process.exit(1);
}, 300000);
try {
  for (let i = 0; i < 100 && !ws && !chromeError; i++) {
    await sleep(100);
    try {
      const file = join(profile, "DevToolsActivePort");
      if (!existsSync(file)) continue;
      const port = readFileSync(file, "utf8").split("\n")[0].trim();
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find((t) => t.type === "page");
      if (page) {
        const sock = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((ok, fail) => {
          sock.onopen = ok;
          sock.onerror = () => fail(new Error("Chrome's DevTools socket did not open"));
          setTimeout(() => fail(new Error("Chrome's DevTools socket did not open in 10 s")), 10000);
        });
        ws = sock;
      }
    } catch {}
  }
  if (chromeError) throw new Error(`Chrome did not start: ${chromeError.message}`);
  if (!ws) throw new Error("Chrome did not start in 10 s (see chrome.log)");
  // Chrome gone: whatever waits on it fails now, not in a minute.
  ws.onclose = () => {
    for (const w of waiting.values()) w.fail(new Error(`${w.method}: Chrome closed the connection`));
    waiting.clear();
  };
  stage = "connecting";
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && waiting.has(msg.id)) {
      const w = waiting.get(msg.id);
      waiting.delete(msg.id);
      msg.error ? w.fail(new Error(`${w.method}: ${msg.error.message}`)) : w.ok(msg.result);
    } else if (msg.method === "Runtime.consoleAPICalled") {
      logs.push(msg.params.args.map((a) => a.value ?? a.description).join(" "));
    } else if (msg.method === "Log.entryAdded") {
      logs.push(msg.params.entry.text);
    } else if (msg.method === "Network.loadingFinished") {
      bytes += msg.params.encodedDataLength ?? 0;
    }
  };
  await send("Runtime.enable");
  await send("Log.enable");
  await send("Page.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: 1000, height: 700, deviceScaleFactor: 1, mobile: false });
  const base = `http://127.0.0.1:${HTTP}/`;

  stage = "0. the cold load";
  // 0. A cold load over a playtester's link (section 17: "Web build, cold
  //    load < 5 s on a 20 Mbit link"): a fresh profile has nothing cached,
  //    the cache is off, and the network is held to 20 Mbit down, 5 up, and
  //    40 ms each way, until the first frame is drawn.
  await send("Network.enable");
  await send("Network.setCacheDisabled", { cacheDisabled: true });
  await send("Network.emulateNetworkConditions", { offline: false, latency: 40, downloadThroughput: 20e6 / 8, uploadThroughput: 5e6 / 8 });
  bytes = 0;
  const t0 = performance.now();
  await open(base + "?paused");
  for (let i = 0; i < 200 && !((await status()).drawn > 0); i++) await sleep(25);
  report.cold_load = { ms: Math.round(performance.now() - t0), bytes, link: "20 Mbit/s down, 5 up, 40 ms", bound_ms: 5000 };
  await send("Network.emulateNetworkConditions", { offline: false, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  await send("Network.setCacheDisabled", { cacheDisabled: false });

  stage = "1. the picture";
  // 1. The picture, as the browser picks its backend.
  let t = performance.now();
  await open(base + "?paused");
  report.open_ms = Math.round(performance.now() - t);
  report.webgpu_in_page = await evaluate("!!navigator.gpu");
  await sleep(1500);
  let s = await status();
  report.render = { backend: s.backend, frames: s.frames, drawn: s.drawn, tick: s.tick, screenshot: await screenshot("start.png") };

  stage = "2. determinism";
  // 2. Determinism: Hollow's run scenario, stepped exactly.
  const lines = [await evaluate("argus.player.status()")];
  await evaluate(`argus.player.inject("move", "[1,0]", 450)`);
  t = performance.now();
  await evaluate("argus.player.step(118)");
  lines.push(await evaluate("argus.player.status()"));
  await evaluate(`argus.player.inject("jump", "true", 18)`);
  await evaluate("argus.player.step(330)");
  lines.push(await evaluate("argus.player.status()"));
  await evaluate("argus.player.step(600)");
  report.step_ms_per_tick = +((performance.now() - t) / 1048).toFixed(3);
  lines.push(await evaluate("argus.player.status()"));
  report.determinism = lines.map((l) => {
    const j = JSON.parse(l);
    return { tick: j.tick, digest: j.digest, events_digest: j.events_digest, sounds: j.sounds, player: j.player };
  });
  await sleep(300);
  report.hall_screenshot = await screenshot("hall.png");

  stage = "3. keys";
  // 3. Keys: the page running in real time, the right arrow held.
  await open(base);
  await evaluate("document.getElementById('game').focus()");
  await sleep(500);
  const before = (await status()).player;
  await key("ArrowRight", "ArrowRight", 39, true);
  await sleep(1500);
  await key("ArrowRight", "ArrowRight", 39, false);
  await sleep(300);
  s = await status();
  report.keys = { before, after: s.player, tick: s.tick, sounds: s.sounds, moved: s.player[0] - before[0] };
  // The first key press opened Web Audio; the mixer is pulled by it now.
  report.audio = { ...s.audio, levels: JSON.parse(await evaluate("argus.player.audio_levels()")) };
  // A mark of that moment: what the desktop replays (the mark_replay example).
  await evaluate("argus.setRunning(false)");
  const mark = await evaluate(`argus.player.mark("held the right arrow for a second and a half")`);
  writeFileSync(join(out, "mark.json"), mark);
  report.mark = { ...JSON.parse(mark), log: `${JSON.parse(mark).log.length} entries`, file: join(out, "mark.json") };
  // The desktop replays it from load: the same world at the same tick?
  const replay = spawnSync("cargo", ["run", "-q", "-p", "argus-web", "--example", "mark_replay", "--", project, join(out, "mark.json")], { encoding: "utf8" });
  report.mark.native = replay.stdout.trim() ? JSON.parse(replay.stdout) : { error: replay.stderr || String(replay.error) };
  report.keys_screenshot = await screenshot("keys.png");

  stage = "4. the post stack";
  // 4. The post stack (phase 2 item 6): the run goes on into the hall's pit,
  // whose fall flashes red, shakes, and stops the game for six ticks. Marked
  // inside the stop, where the flash, the shake, and the ticks left are all
  // world state the desktop must reach too.
  await open(base + "?paused");
  await sleep(500);
  await evaluate(`argus.player.inject("move", "[1,0]", 900)`);
  await evaluate("argus.player.step(118)");
  await evaluate(`argus.player.inject("jump", "true", 18)`);
  let last = (await status()).player;
  let fell = null;
  for (let i = 0; i < 900 && fell === null; i++) {
    await evaluate("argus.player.step(1)");
    s = await status();
    // Put back at the cave mouth: the tick of the fall.
    if (s.player[0] < last[0] - 10) fell = { tick: s.tick, at: last, back: s.player };
    last = s.player;
  }
  report.pit = { fell };
  if (fell) {
    const held = [];
    for (let i = 0; i < 2; i++) {
      await evaluate("argus.player.step(1)");
      held.push((await status()).player);
    }
    report.pit.held = held.every((p) => p[0] === fell.back[0] && p[1] === fell.back[1]);
    const pitMark = await evaluate(`argus.player.mark("in the pit's hit-stop")`);
    writeFileSync(join(out, "pit-mark.json"), pitMark);
    const r = spawnSync("cargo", ["run", "-q", "-p", "argus-web", "--example", "mark_replay", "--", project, join(out, "pit-mark.json")], { encoding: "utf8" });
    report.pit.native = r.stdout.trim() ? JSON.parse(r.stdout) : { error: r.stderr || String(r.error) };
    await sleep(300);
    report.pit.screenshot = await screenshot("pit.png");

    // The page's report key sends this: the recorder's replay file, posted
    // to the server the page came from; the desktop reproduces it.
    report.pit.sent = await evaluate(
      `argus.send(argus.report("fell in the pit (from the check)")).then(() => document.getElementById("said").textContent)`,
    );
    const listed = await (await fetch(`http://127.0.0.1:${HTTP}/marks`)).json();
    const last = listed.marks[listed.marks.length - 1];
    const body = Buffer.from(await (await fetch(`http://127.0.0.1:${HTTP}/marks/${last.id}`)).arrayBuffer());
    writeFileSync(join(out, "pit-report.argr"), body);
    const rr = spawnSync("cargo", ["run", "-q", "-p", "argus-web", "--example", "mark_replay", "--", project, join(out, "pit-report.argr")], { encoding: "utf8" });
    report.pit.report = { id: last.id, bytes: last.bytes, native: rr.stdout.trim() ? JSON.parse(rr.stdout) : { error: rr.stderr || String(rr.error) } };
  }

  stage = "5. the UI";
  // 5. The UI (phase 3 item 1): Escape opens the pause menu over the game,
  // drawn as on the desktop; the mouse over a button focuses it and a click
  // presses it, found through the semantic frame.
  await open(base);
  await evaluate("document.getElementById('game').focus()");
  await sleep(500);
  await key("Escape", "Escape", 27, true);
  await sleep(100);
  await key("Escape", "Escape", 27, false);
  await sleep(400);
  const ui = async () => JSON.parse(await evaluate("argus.player.semantic()")).ui;
  let seen = await ui();
  report.ui = { opened: seen.map((u) => `${u.id}${u.focused ? " (focused)" : ""}: ${u.text}`) };
  report.ui.screenshot = await screenshot("menu.png");
  const restart = seen.find((u) => u.id.endsWith("pause.restart"));
  if (restart) {
    // The frame's pixels to the page's: the canvas element's place and size.
    const box = JSON.parse(await evaluate("JSON.stringify(document.getElementById('game').getBoundingClientRect())"));
    const size = JSON.parse(await evaluate("JSON.stringify([document.getElementById('game').width, document.getElementById('game').height])"));
    const x = box.left + ((restart.bounds[0] + restart.bounds[2]) / 2) * (box.width / size[0]);
    const y = box.top + ((restart.bounds[1] + restart.bounds[3]) / 2) * (box.height / size[1]);
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
    await sleep(300);
    seen = await ui();
    report.ui.hovered = seen.find((u) => u.focused)?.id ?? null;
    const tick = (await status()).tick;
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
    await sleep(100);
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
    await sleep(400);
    seen = await ui();
    s = await status();
    report.ui.clicked = { menu_open: seen.some((u) => u.id.endsWith("pause.restart")), tick_before: tick, tick_after: s.tick, player: s.player };
  }
  // The same menu for a screen reader (phase 5 item 2): its buttons are
  // the page's, the focus is said, and pressing one there presses the
  // game's (here, Controls).
  await key("Escape", "Escape", 27, true);
  await sleep(100);
  await key("Escape", "Escape", 27, false);
  await sleep(600);
  const reader = async () =>
    JSON.parse(
      await evaluate(`JSON.stringify({
        buttons: [...document.querySelectorAll('#reader-buttons button')].map((b) => b.textContent),
        said: document.getElementById('reader-said').textContent,
        role: document.getElementById('game').getAttribute('role'),
      })`),
    );
  const readerBefore = await reader();
  await evaluate("[...document.querySelectorAll('#reader-buttons button')].find((b) => b.dataset.id.endsWith('pause.controls'))?.click()");
  await sleep(600);
  seen = await ui();
  const after = await reader();
  report.reader = {
    buttons: readerBefore.buttons,
    said: readerBefore.said,
    role: readerBefore.role,
    pressed_controls: seen.some((u) => u.id.endsWith("controls.jump") && u.focused),
    said_after: after.said,
  };

  stage = "6. WebGL2";
  // 6. WebGL2 forced.
  await open(base + "?paused&webgl");
  await sleep(1500);
  s = await status();
  report.webgl = { backend: s.backend, frames: s.frames, drawn: s.drawn, screenshot: await screenshot("webgl.png") };
} catch (e) {
  report.error = `${stage}: ${e.stack ?? e}`;
} finally {
  clearTimeout(watchdog);
  verdict();
  try {
    if (ws) await send("Browser.close", {}, 2000);
  } catch {}
  browser.kill();
  server.kill();
  await sleep(500);
  try {
    rmSync(profile, { recursive: true, force: true });
  } catch {}
}
