// Check the games site in headless Chrome, over the DevTools protocol, with
// no window on anyone's screen:
//
//   node tools/web/games-check.mjs <base url> <out-dir> [game ...]
//
// For each game (default hollow, delve, outpost) opens <base url><game>/,
// waits for the player, lets it run a second and a half, and reports the
// backend it drew with, whether its tick moved on, and a screenshot. Node 24
// (its WebSocket). The base url is the site served by serve.mjs, local or
// the public one.
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, existsSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const base = process.argv[2];
const out = process.argv[3];
const games = process.argv.slice(4).length ? process.argv.slice(4) : ["hollow", "delve", "outpost"];
if (!base || !out) {
  console.error("usage: node tools/web/games-check.mjs <base url> <out-dir> [game ...]");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const chrome = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const profile = mkdtempSync(join(tmpdir(), "argus-chrome-"));
const browser = spawn(chrome, [
  "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`,
  "--no-first-run", "--no-default-browser-check", "--enable-unsafe-webgpu", "--mute-audio",
  "--window-size=960,600", "about:blank",
], { stdio: "ignore" });

let ws, nextId = 1;
const waiting = new Map();
const logs = [];
function send(method, params = {}) {
  const id = nextId++;
  return new Promise((ok, fail) => {
    waiting.set(id, { ok, fail });
    ws.send(JSON.stringify({ id, method, params }));
    setTimeout(() => waiting.has(id) && (waiting.delete(id), fail(new Error(`${method}: no answer`))), 60000);
  });
}
async function evaluate(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
}

const report = { base, games: {} };
try {
  for (let i = 0; i < 100 && !ws; i++) {
    await sleep(100);
    const file = join(profile, "DevToolsActivePort");
    if (!existsSync(file)) continue;
    const port = readFileSync(file, "utf8").split("\n")[0].trim();
    const page = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((t) => t.type === "page");
    if (!page) continue;
    ws = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
  }
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && waiting.has(msg.id)) {
      const w = waiting.get(msg.id);
      waiting.delete(msg.id);
      msg.error ? w.fail(new Error(msg.error.message)) : w.ok(msg.result);
    } else if (msg.method === "Runtime.consoleAPICalled") {
      logs.push(msg.params.args.map((a) => a.value ?? a.description ?? "").join(" "));
    } else if (msg.method === "Runtime.exceptionThrown") {
      logs.push("exception: " + (msg.params.exceptionDetails.exception?.description ?? msg.params.exceptionDetails.text));
    }
  };
  await send("Runtime.enable");
  await send("Page.enable");
  for (const g of games) {
    logs.length = 0;
    const r = { url: `${base}${g}/` };
    report.games[g] = r;
    try {
      const t0 = Date.now();
      await send("Page.navigate", { url: r.url });
      let ready = false;
      for (let i = 0; i < 300 && !ready; i++) {
        await sleep(100);
        ready = await evaluate("!!window.argus || (document.getElementById('status')?.textContent ?? '').startsWith('Could not')").catch(() => false);
      }
      r.ready_ms = Date.now() - t0;
      r.status_text = await evaluate("document.getElementById('status')?.textContent ?? ''").catch(() => "");
      const first = JSON.parse(await evaluate("argus.player.status()"));
      await sleep(1500);
      let later = JSON.parse(await evaluate("argus.player.status()"));
      if (!(later.tick > first.tick)) {
        // A turn-based game (Delve) moves when a key is pressed: a step right.
        for (const type of ["keyDown", "keyUp"]) {
          await send("Input.dispatchKeyEvent", { type, code: "ArrowRight", key: "ArrowRight", windowsVirtualKeyCode: 39, nativeVirtualKeyCode: 39 });
        }
        await sleep(500);
        later = JSON.parse(await evaluate("argus.player.status()"));
        r.pressed = "ArrowRight";
      }
      r.backend = later.backend ?? first.backend ?? null;
      r.ticks = [first.tick, later.tick];
      r.runs = later.tick > first.tick;
      const shot = await send("Page.captureScreenshot", { format: "png" });
      r.screenshot = join(out, `${g}.png`);
      writeFileSync(r.screenshot, Buffer.from(shot.data, "base64"));
    } catch (e) {
      r.error = String(e.message ?? e);
    }
    r.console = logs.slice(-8);
    r.ok = !r.error && r.runs === true;
  }
} catch (e) {
  report.error = String(e.message ?? e);
}
report.ok = !report.error && Object.values(report.games).every((g) => g.ok);
console.log(JSON.stringify(report, null, 2));
browser.kill();
// The browser's profile: Windows keeps a killed Chrome's files open for a
// moment, so the removal tries again for a while.
try { rmSync(profile, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 }); } catch {}
process.exit(report.ok ? 0 : 1);
