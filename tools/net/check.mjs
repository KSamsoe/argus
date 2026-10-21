// A real match between hosts (docs/multiplayer-checklist.md items 3 and 4):
// a relay on this machine, the game's web build in headless Chrome, and the
// other player a desktop build with no window, or a second page:
//
//   node tools/net/check.mjs --argus <argus.exe> --site <web build> [--game <desktop exe>] [--ticks 600] [--lag ms] [--jitter ms]
//
// With --game the page hosts and the desktop game joins by the code
// (`--join <code> --headless <ticks>`); without it, two pages play. The
// pages press keys (slot 1's: WASD, F, Space) on their own schedule. It
// fails when any player falls out of step, or the players agreed on too
// few checks. --lag and --jitter make the relay's link slow on purpose.
// Prints one JSON line. Node 24, no npm packages.
import { spawn } from "node:child_process";
import { mkdtempSync, existsSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const argus = opt("--argus");
const site = opt("--site");
const game = opt("--game");
const ticks = Number(opt("--ticks", "600"));
const lag = opt("--lag", "0");
const jitter = opt("--jitter", "0");
if (!argus || !site) {
  console.error("usage: check.mjs --argus <argus.exe> --site <web build> [--game <desktop exe>] [--ticks 600]");
  process.exit(2);
}
const chrome = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const freePort = () =>
  new Promise((ok) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const port = s.address().port;
      s.close(() => ok(port));
    });
  });

const kids = [];
const profiles = [];

// A page in a headless Chrome of its own, through the DevTools protocol.
async function openPage(url) {
  const profile = mkdtempSync(join(tmpdir(), "argus-net-"));
  profiles.push(profile);
  const browser = spawn(
    chrome,
    [
      "--headless=new",
      "--remote-debugging-port=0",
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--mute-audio",
      "--disable-background-timer-throttling",
      "--disable-renderer-backgrounding",
      "--disable-backgrounding-occluded-windows",
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  kids.push(browser);
  let ws = null;
  let next = 1;
  const waiting = new Map();
  for (let i = 0; i < 100 && !ws; i++) {
    await sleep(100);
    const port = join(profile, "DevToolsActivePort");
    if (!existsSync(port)) continue;
    const list = await (await fetch(`http://127.0.0.1:${readFileSync(port, "utf8").split("\n")[0].trim()}/json/list`)).json().catch(() => []);
    const page = list.find((t) => t.type === "page");
    if (!page) continue;
    const sock = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise((ok, fail) => ((sock.onopen = ok), (sock.onerror = () => fail(new Error("DevTools did not open")))));
    sock.onmessage = (e) => {
      const m = JSON.parse(e.data);
      const w = m.id && waiting.get(m.id);
      if (w) waiting.delete(m.id), m.error ? w.fail(new Error(m.error.message)) : w.ok(m.result);
    };
    ws = sock;
  }
  if (!ws) throw new Error("Chrome did not start");
  const send = (method, params = {}) =>
    new Promise((ok, fail) => {
      const id = next++;
      const t = setTimeout(() => (waiting.delete(id), fail(new Error(`${method}: no answer`))), 60000);
      waiting.set(id, { ok: (v) => (clearTimeout(t), ok(v)), fail: (e) => (clearTimeout(t), fail(e)) });
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expr) => {
    const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  };
  await send("Runtime.enable");
  await send("Page.navigate", { url });
  for (let i = 0; i < 300 && !(await evaluate("!!(window.argus && window.argus.net)").catch(() => false)); i++) await sleep(100);
  return { evaluate, net: async () => await evaluate("window.argus.net()") };
}

// What a page's player does: walks one way and the other, attacks, dashes.
// `t` counts in threes, so every moment named is a multiple of 3.
async function press(page, t) {
  const at = t % 240;
  const keys = [];
  if (at === 0) keys.push(["KeyD", true]);
  if (at === 60) keys.push(["KeyD", false], ["KeyA", true]);
  if (at === 120) keys.push(["KeyA", false], ["KeyW", true]);
  if (at === 150) keys.push(["KeyW", false]);
  if (at % 45 === 9) keys.push(["KeyF", true]);
  if (at % 45 === 15) keys.push(["KeyF", false]);
  if (at === 201) keys.push(["Space", true]);
  if (at === 207) keys.push(["Space", false]);
  for (const [k, down] of keys) await page.evaluate(`window.argus.player.key(${JSON.stringify(k)}, ${down})`);
}

const report = { ok: false, mode: game ? "page and desktop" : "two pages", lag: Number(lag), jitter: Number(jitter) };
let failed = false;
try {
  // The relay and the site.
  const rp = await freePort();
  const relayUrl = `ws://127.0.0.1:${rp}`;
  const relay = spawn(argus, ["relay", `:${rp}`, `lag=${lag}`, `jitter=${jitter}`], { stdio: "ignore" });
  kids.push(relay);
  const http = await freePort();
  kids.push(spawn(process.execPath, ["tools/web/serve.mjs", site, String(http), "127.0.0.1"], { stdio: "ignore" }));
  await sleep(800);
  const base = `http://127.0.0.1:${http}/?relay=${encodeURIComponent(relayUrl)}`;
  const host = await openPage(`${base}&host=2`);
  let code = null;
  for (let i = 0; i < 200 && !code; i++) {
    const n = await host.net();
    if (n.state === "room" && n.code) code = n.code;
    else await sleep(50);
  }
  if (!code) throw new Error(`the page never had a room: ${JSON.stringify(await host.net())}`);
  report.room = code;
  // The other player.
  let desktop = null;
  let desktopOut = "";
  let guest = null;
  if (game) {
    desktop = spawn(game, ["--relay", relayUrl, "--join", code, "--headless", String(ticks)], { stdio: ["ignore", "pipe", "pipe"] });
    kids.push(desktop);
    desktop.stdout.on("data", (d) => (desktopOut += d));
    desktop.stderr.on("data", (d) => (desktopOut += d));
  } else {
    guest = await openPage(`${base}&room=${code}`);
  }
  const pages = guest ? [host, guest] : [host];
  // Play: each page presses its keys while the match runs.
  const seen = pages.map(() => ({ max_checks: 0, rollbacks: 0, states: new Set() }));
  const started = Date.now();
  let done = false;
  let t = 0;
  while (!done && Date.now() - started < 120000) {
    for (const [i, p] of pages.entries()) {
      const n = await p.net();
      seen[i].states.add(n.state);
      if (n.why && n.why.includes("out of step")) throw new Error(`page ${i + 1}: ${n.why}`);
      if (n.stats) {
        seen[i].max_checks = Math.max(seen[i].max_checks, n.stats.checks);
        seen[i].rollbacks = n.stats.rollbacks;
        seen[i].tick = n.tick;
      }
      if (n.state === "playing") await press(p, t + i * 99);
    }
    t += 3;
    if (desktop && desktop.exitCode !== null) done = true;
    if (!desktop && seen.every((s) => s.max_checks >= Math.floor(ticks / 60) - 1)) done = true;
    await sleep(50);
  }
  report.pages = seen.map((s) => ({ checks: s.max_checks, rollbacks: s.rollbacks, tick: s.tick, states: [...s.states] }));
  if (desktop) {
    for (let i = 0; i < 40 && desktop.exitCode === null; i++) await sleep(100);
    const lines = desktopOut.trim().split("\n").filter((l) => l.startsWith("{"));
    const last = lines.length ? JSON.parse(lines.at(-1)) : null;
    report.desktop = { exit: desktop.exitCode, last, out: lines.length ? undefined : desktopOut.slice(-400) };
    if (desktop.exitCode !== 0 || !last?.checks) throw new Error(`the desktop game: ${desktopOut.slice(-400)}`);
    if (last.checks < Math.floor(ticks / 60) - 2) throw new Error(`the desktop agreed on only ${last.checks} checks`);
  } else if (!done) {
    throw new Error("the pages' match did not reach its checks in two minutes");
  }
  if (seen[0].max_checks < 3) throw new Error(`the page agreed on only ${seen[0].max_checks} checks`);
  report.ok = true;
} catch (e) {
  failed = true;
  report.error = String(e.stack ?? e);
} finally {
  for (const k of kids) {
    try {
      k.kill();
    } catch {}
  }
  await sleep(500);
  for (const p of profiles) {
    try {
      rmSync(p, { recursive: true, force: true });
    } catch {}
  }
  console.log(JSON.stringify(report));
  process.exit(failed ? 1 : 0);
}
