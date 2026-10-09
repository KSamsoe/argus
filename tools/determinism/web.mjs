// The determinism spike's browser half (phase 5 item 11): play run files
// in headless Chrome, over the DevTools protocol, with no window on
// anyone's screen, and write the digests as the native probe does:
//
//   node tools/determinism/web.mjs <site> <run.json>... [--rows <tick>]
//   node tools/determinism/web.mjs <site> math          (the math probe)
//
// Each run gets a player of its own, made from the site's packs in the
// page, apart from the page's frame loop, its saves, and its settings, so
// nothing but the run file drives it. It writes
// target/determinism/web-<game>-<seed>.json (with --rows, the digest's
// lines at that tick, -rows-<tick>.txt). Node 24, no npm packages.
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, openSync, readFileSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, basename } from "node:path";

const args = process.argv.slice(2);
const rowsAt = args.includes("--rows") ? Number(args[args.indexOf("--rows") + 1]) : null;
const [site, ...files] = args.filter((a, i) => !a.startsWith("--") && args[i - 1] !== "--rows");
if (!site || !files.length) {
  console.error("usage: web.mjs <site> <run.json>... [--rows <tick>]");
  process.exit(2);
}
const out = "target/determinism";
mkdirSync(out, { recursive: true });
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

const HTTP = await freePort();
const server = spawn(process.execPath, ["tools/web/serve.mjs", site, String(HTTP), "127.0.0.1", join(out, "marks")], { stdio: "ignore" });
const profile = mkdtempSync(join(tmpdir(), "argus-chrome-"));
const browser = spawn(chrome, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--mute-audio", "about:blank"], {
  stdio: ["ignore", openSync(join(out, "chrome.log"), "w"), "ignore"],
});

let ws, nextId = 1;
const waiting = new Map();
function send(method, params = {}, ms = 600000) {
  const id = nextId++;
  return new Promise((ok, fail) => {
    const timer = setTimeout(() => (waiting.delete(id), fail(new Error(`${method}: no answer in ${ms / 1000} s`))), ms);
    waiting.set(id, { ok: (v) => (clearTimeout(timer), ok(v)), fail: (e) => (clearTimeout(timer), fail(e)) });
    ws.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
}

let failed = false;
try {
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
  await send("Runtime.enable");
  // The page only to load the module: paused, so its own player waits.
  await send("Page.navigate", { url: `http://127.0.0.1:${HTTP}/?paused` });
  for (let i = 0; i < 300 && !(await evaluate("!!window.argus").catch(() => false)); i++) await sleep(100);
  // `--math`: the math probe in this browser, not a run.
  if (files[0] === "math") {
    const r = JSON.parse(await evaluate(`import("./pkg/argus_web.js").then((m) => m.WebPlayer.math_probe(20000))`));
    r.platform = { os: "browser", arch: "wasm32", env: await evaluate(`navigator.userAgent.match(/Chrome\\/[\\d.]+/)?.[0] ?? ""`), from: "" };
    writeFileSync(join(out, "math-web.json"), JSON.stringify(r) + "\n");
    console.log("math: target/determinism/math-web.json");
    files.length = 0;
  }
  for (const file of files) {
    const run = JSON.parse(readFileSync(file, "utf8"));
    const result = JSON.parse(
      await evaluate(`(async () => {
        const run = ${JSON.stringify(run)};
        const rowsAt = ${JSON.stringify(rowsAt)};
        const m = await import("./pkg/argus_web.js");
        const listed = await (await fetch("data/packs.json")).json();
        const packs = [];
        for (const n of listed.packs) packs.push(new Uint8Array(await (await fetch("data/" + n)).arrayBuffer()));
        const t0 = performance.now();
        const p = m.WebPlayer.from_packs(packs, undefined, run.scene, run.seed);
        const open_ms = performance.now() - t0;
        const replayed = run.log.length ? p.replay_log(JSON.stringify(run.log)) : 0;
        const t1 = performance.now();
        const digests = JSON.parse(p.digests(rowsAt ?? run.ticks, run.every));
        const run_ms = performance.now() - t1;
        const rows = rowsAt === null ? null : p.digest_rows();
        const entities = JSON.parse(p.status()).entities;
        p.free();
        return JSON.stringify({ game: run.game, platform: { os: "browser", arch: "wasm32", env: navigator.userAgent.match(/Chrome\\/[\\d.]+/)?.[0] ?? "", from: "packs" },
          scene: run.scene, seed: run.seed, replayed, ticks: run.ticks, every: run.every, open_ms, run_ms, entities, digests, rows });
      })()`),
    );
    const name = `web-${result.game}-${result.seed}`;
    if (rowsAt !== null) {
      writeFileSync(join(out, `${name}-rows-${rowsAt}.txt`), result.rows + "\n");
      console.log(`${basename(file)}: ${result.rows.split("\n").length} rows at tick ${rowsAt}`);
    } else {
      delete result.rows;
      writeFileSync(join(out, `${name}.json`), JSON.stringify(result) + "\n");
      console.log(`${basename(file)}: ${result.digests.length} checkpoints in ${Math.round(result.run_ms)} ms, last ${JSON.stringify(result.digests.at(-1))}`);
    }
  }
} catch (e) {
  failed = true;
  console.error("web.mjs: " + (e.stack ?? e));
} finally {
  try {
    if (ws) await send("Browser.close", {}, 2000);
  } catch {}
  browser.kill();
  server.kill();
  await sleep(500);
  try {
    rmSync(profile, { recursive: true, force: true });
  } catch {}
  process.exit(failed ? 1 : 0);
}
