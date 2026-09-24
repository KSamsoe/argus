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
// hit-stop, marked inside the stop and replayed natively), and the same
// picture with WebGL2 forced. Node 24 and cargo, no npm packages.
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const site = process.argv[2] ?? "target-web/site";
const out = process.argv[3] ?? "target-web/check";
const project = process.argv[4] ?? "examples/hollow";
const chrome = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const HTTP = 8765;
const DEBUG = 9334;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(out, { recursive: true });

const server = spawn(process.execPath, ["tools/web/serve.mjs", site, String(HTTP)], { stdio: "ignore" });
const profile = mkdtempSync(join(tmpdir(), "argus-chrome-"));
const browser = spawn(chrome, [
  "--headless=new",
  `--remote-debugging-port=${DEBUG}`,
  `--user-data-dir=${profile}`,
  "--no-first-run",
  "--no-default-browser-check",
  "--enable-unsafe-webgpu",
  "--autoplay-policy=no-user-gesture-required",
  // The audio runs, but nothing reaches the speakers of whoever's machine this is.
  "--mute-audio",
  "about:blank",
], { stdio: "ignore" });

let ws, nextId = 1;
const waiting = new Map();
const logs = [];
function send(method, params = {}) {
  const id = nextId++;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((ok, fail) => waiting.set(id, { ok, fail, method }));
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
try {
  for (let i = 0; i < 100; i++) {
    await sleep(100);
    try {
      const list = await (await fetch(`http://127.0.0.1:${DEBUG}/json/list`)).json();
      const page = list.find((t) => t.type === "page");
      if (page) {
        ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((r) => (ws.onopen = r));
        break;
      }
    } catch {}
  }
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
    }
  };
  await send("Runtime.enable");
  await send("Log.enable");
  await send("Page.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: 1000, height: 700, deviceScaleFactor: 1, mobile: false });
  const base = `http://127.0.0.1:${HTTP}/`;

  // 1. The picture, as the browser picks its backend.
  let t = performance.now();
  await open(base + "?paused");
  report.open_ms = Math.round(performance.now() - t);
  report.webgpu_in_page = await evaluate("!!navigator.gpu");
  await sleep(1500);
  let s = await status();
  report.render = { backend: s.backend, frames: s.frames, drawn: s.drawn, tick: s.tick, screenshot: await screenshot("start.png") };

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
  }

  // 5. WebGL2 forced.
  await open(base + "?paused&webgl");
  await sleep(1500);
  s = await status();
  report.webgl = { backend: s.backend, frames: s.frames, drawn: s.drawn, screenshot: await screenshot("webgl.png") };
} catch (e) {
  report.error = String(e.stack ?? e);
} finally {
  report.console = logs.slice(-20);
  console.log(JSON.stringify(report, null, 2));
  try {
    await send("Browser.close");
  } catch {}
  browser.kill();
  server.kill();
  await sleep(500);
  try {
    rmSync(profile, { recursive: true, force: true });
  } catch {}
}
