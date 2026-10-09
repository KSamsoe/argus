// The browser editor spike's check (phase 5 item 10), in headless Chrome
// over the DevTools protocol, with no window on anyone's screen:
//
//   node tools/web/editor-check.mjs [project] [out-dir]
//
// Starts the project's daemon serving the web client on this machine
// (argus daemon start --web), issues a review token, asks a review
// request with a clip, and opens the page as a phone would (390 x 844,
// touch): the page connects with the token from its address's fragment,
// shows the request and its clip, and a tap on a choice answers it, which
// the daemon then says was answered by the token's label. Then the
// viewport watches the game for a few seconds (frames a second, the time a
// frame takes), and a call's round trip is measured over the WebSocket and
// over the local socket, for the spike's write-up. It exits 1 when
// anything failed. Node 24 and the built CLI (target/debug/argus), no npm
// packages.
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, mkdirSync, openSync, readFileSync, existsSync } from "node:fs";
import { createServer, connect as netConnect } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const project = process.argv[2] ?? "examples/hollow";
const out = process.argv[3] ?? "target-web/editor-check";
const chrome = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const argus = process.env.ARGUS ?? (process.platform === "win32" ? "target/debug/argus.exe" : "target/debug/argus");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(out, { recursive: true });
const freePort = () =>
  new Promise((ok) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const port = s.address().port;
      s.close(() => ok(port));
    });
  });
const cli = (...args) => {
  const r = spawnSync(argus, ["--project", project, "--json", ...args], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`argus ${args.join(" ")}: ${r.stderr || r.stdout}`);
  return JSON.parse(r.stdout);
};

const report = {};
let stage = "starting the daemon";
const PORT = await freePort();
spawnSync(argus, ["--project", project, "daemon", "stop"], { encoding: "utf8" });
const started = spawnSync(argus, ["--project", project, "daemon", "start", "--web", `:${PORT}`], { encoding: "utf8" });
if (started.status !== 0) {
  console.log(JSON.stringify({ ok: false, error: started.stderr }, null, 2));
  process.exit(1);
}

const profile = mkdtempSync(join(tmpdir(), "argus-chrome-"));
const chromeLog = openSync(join(out, "chrome.log"), "w");
const browser = spawn(chrome, [
  "--headless=new",
  "--remote-debugging-port=0",
  `--user-data-dir=${profile}`,
  "--no-first-run",
  "--no-default-browser-check",
  "about:blank",
], { stdio: ["ignore", chromeLog, chromeLog] });
let chromeError = null;
browser.on("error", (e) => (chromeError = e));

let ws, nextId = 1;
const waiting = new Map();
const logs = [];
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
    waiting.set(id, { ok: done(ok), fail: done(fail) });
    ws.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(`${expr}: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
  return r.result.value;
}
async function until(expr, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await evaluate(expr).catch(() => false)) return true;
    await sleep(100);
  }
  throw new Error(`waited ${ms / 1000} s for ${expr}\n${logs.slice(-10).join("\n")}`);
}
async function screenshot(name) {
  const r = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(out, name), Buffer.from(r.data, "base64"));
  return join(out, name);
}
async function tap(selector) {
  const box = await evaluate(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
  for (const type of ["touchStart", "touchEnd"]) {
    await send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x: box[0], y: box[1] }] });
  }
}

// A call's round trip, many times, over the local socket (a named pipe
// here) and over the WebSocket the page uses: the spike's latency.
async function roundTrips(token, info, n = 200) {
  const timeIt = async (sendLine, next) => {
    const t = [];
    for (let i = 0; i < n; i++) {
      const at = performance.now();
      sendLine(JSON.stringify({ jsonrpc: "2.0", id: i + 1, method: "session.status", params: {} }));
      await next(i + 1);
      t.push(performance.now() - at);
    }
    t.sort((a, b) => a - b);
    return { mean_ms: +(t.reduce((a, b) => a + b) / n).toFixed(3), p95_ms: +t[Math.floor(n * 0.95)].toFixed(3) };
  };
  const pipe = process.platform === "win32" ? `\\\\.\\pipe\\${info.socket}` : info.socket;
  const sock = netConnect(pipe);
  await new Promise((ok, fail) => { sock.once("connect", ok); sock.once("error", fail); });
  let buf = "", resolve = null;
  sock.on("data", (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i);
      buf = buf.slice(i + 1);
      const m = JSON.parse(line);
      if (m.id && resolve) resolve(m);
    }
  });
  const local = await timeIt((l) => sock.write(l + "\n"), () => new Promise((ok) => (resolve = ok)));
  sock.end();
  const w = new WebSocket(`ws://127.0.0.1:${PORT}/ws`, ["argus", "t." + token]);
  let wres = null;
  w.onmessage = (e) => { const m = JSON.parse(e.data); if (m.id && wres) wres(m); };
  await new Promise((ok, fail) => { w.onopen = ok; w.onerror = () => fail(new Error("the WebSocket did not open")); });
  const web = await timeIt((l) => w.send(l), () => new Promise((ok) => (wres = ok)));
  w.close();
  return { local_socket: local, websocket: web, calls: n };
}

const watchdog = setTimeout(() => {
  report.error = `${stage}: the check took over five minutes`;
  finish();
}, 300000);

function finish() {
  clearTimeout(watchdog);
  const failures = [];
  if (report.error) failures.push(report.error.split("\n")[0]);
  if (report.review && !report.review.answered_by_the_page) failures.push("the tap did not answer the review");
  if (report.review && !report.review.clip_shown) failures.push("the clip did not show");
  if (report.watch && !(report.watch.frames_per_second > 0)) failures.push("no frames in the viewport");
  if (report.watch && !report.watch.header_tick_moved) failures.push("the header's tick did not move while watching");
  report.ok = failures.length === 0;
  report.failures = failures;
  console.log(JSON.stringify(report, null, 2));
  try { browser.kill(); } catch (e) {}
  spawnSync(argus, ["--project", project, "daemon", "stop"], { encoding: "utf8" });
  process.exit(report.ok ? 0 : 1);
}

try {
  stage = "issuing a token and asking a review";
  const info = JSON.parse(readFileSync(join(project, ".argus/daemon.json"), "utf8"));
  report.web = info.web;
  const tok = cli("token", "issue", "scope=review", "hours=1", "label=check").result ?? cli("token", "issue", "scope=review", "hours=1", "label=check");
  const token = tok.token;
  cli("sim", "step", "150");
  const asked = cli("obs", "review", "question=Does the hero's jump read as a jump?", "options=[yes, no, almost]", "software=true");
  report.asked = { id: asked.id ?? asked.result?.id };

  stage = "starting Chrome";
  for (let i = 0; i < 100 && !ws && !chromeError; i++) {
    await sleep(100);
    const file = join(profile, "DevToolsActivePort");
    if (!existsSync(file)) continue;
    const port = readFileSync(file, "utf8").split("\n")[0].trim();
    const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json().catch(() => []);
    const page = list.find((t) => t.type === "page");
    if (page) {
      const sock = new WebSocket(page.webSocketDebuggerUrl);
      await new Promise((ok, fail) => { sock.onopen = ok; sock.onerror = () => fail(new Error("DevTools did not open")); });
      sock.onmessage = (e) => {
        const m = JSON.parse(e.data);
        if (m.id && waiting.has(m.id)) {
          const w = waiting.get(m.id);
          waiting.delete(m.id);
          m.error ? w.fail(new Error(m.error.message)) : w.ok(m.result);
        } else if (m.method === "Runtime.consoleAPICalled") {
          logs.push(m.params.args.map((a) => a.value ?? a.description).join(" "));
        } else if (m.method === "Runtime.exceptionThrown") {
          logs.push("exception: " + (m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text));
        }
      };
      ws = sock;
    }
  }
  if (!ws) throw new Error("Chrome did not start: " + (chromeError?.message ?? "no DevTools port"));
  await send("Runtime.enable");
  await send("Page.enable");
  // A phone: its size, its density, its touch.
  await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 3, mobile: true });
  await send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });

  stage = "opening the page";
  const t0 = Date.now();
  await send("Page.navigate", { url: tok.url.replace(/^http:\/\/[^/]+/, `http://127.0.0.1:${PORT}`) });
  await until("document.getElementById('status').textContent.includes('review')");
  report.page = {
    connected_ms: Date.now() - t0,
    token_left_the_address: await evaluate("!location.hash.includes('t=')"),
    tune_hidden_for_review: await evaluate("document.querySelector('[data-tab=tune]').hidden"),
  };

  stage = "the review request";
  await until("document.querySelectorAll('#review-list .card').length > 0");
  await until("(() => { const i = document.querySelector('#review-list img.clip'); return i && i.complete && i.naturalWidth > 0; })()", 20000);
  report.review = {
    question: await evaluate("document.querySelector('#review-list h3').textContent"),
    choices: await evaluate("[...document.querySelectorAll('#review-list .choices button')].map(b => b.textContent)"),
    clip_shown: true,
    clip_size: await evaluate("(() => { const i = document.querySelector('#review-list img.clip'); return [i.naturalWidth, i.naturalHeight]; })()"),
  };
  report.screens = { review: await screenshot("review.png") };
  await tap("#review-list .choices button");
  await until("document.getElementById('toast').textContent.startsWith('Answered')");
  const list = cli("review", "list", "all=true");
  const rows = (list.rows ?? list.result?.rows ?? []);
  const row = rows.find((r) => r.id === report.asked.id) ?? rows[rows.length - 1];
  report.review.answer = row?.answer;
  report.review.answered_by = row?.answered_by;
  report.review.answered_by_the_page = row?.answered_by === "web:check" && row?.answer === "yes";

  stage = "watching";
  cli("sim", "play");
  await evaluate("document.querySelector('[data-tab=watch]').click()");
  const headerTick = () => evaluate("+(document.getElementById('status').textContent.match(/tick (\\d+)/) ?? [])[1] || 0");
  const tickBefore = await headerTick();
  await evaluate("document.getElementById('watch-toggle').click()");
  await sleep(6000);
  const fps = await evaluate("document.getElementById('fps').textContent");
  const tickAfter = await headerTick();
  report.watch = {
    shown: fps,
    frames_per_second: parseFloat(fps) || 0,
    frame_ms: parseFloat((fps.match(/a frame (\d+)/) ?? [])[1]) || null,
    header_tick: [tickBefore, tickAfter],
    header_tick_moved: tickAfter > tickBefore,
  };
  report.screens.watch = await screenshot("watch.png");
  await evaluate("document.getElementById('watch-toggle').click()");
  cli("sim", "pause");

  stage = "round trips";
  report.round_trips = await roundTrips(token, info);
  report.console = logs.slice(-10);
  cli("token", "revoke", `id=${tok.id}`);
} catch (e) {
  report.error = `${stage}: ${e.message}`;
}
finish();
