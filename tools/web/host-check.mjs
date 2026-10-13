// Check the host channel in a real page (host channel gate): a project's
// web/game.js is loaded by a web build, a message it sends reaches a
// script, and the script's q:send reaches it back.
//
//   node tools/web/host-check.mjs [argus] [out-dir]
//
// Makes its own project from the roguelike template, with a system that
// answers each message and a web/game.js that sends one, builds it with
// `argus build --target web`, serves it, and opens it in headless Chrome
// over the DevTools protocol. It exits 1 when anything failed. Node 24, no
// npm packages.
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const argus = resolve(process.argv[2] ?? "target/release/argus.exe");
const out = resolve(process.argv[3] ?? "target-web/host-check");
const chrome = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(out, { recursive: true });
const report = { stage: "making the project" };
const finish = (code) => {
  writeFileSync(join(out, "report.json"), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  process.exit(code);
};

// The project: the roguelike, a system that answers, a page script that asks.
const project = mkdtempSync(join(tmpdir(), "argus-host-check-"));
cpSync("templates/roguelike", project, { recursive: true });
rmSync(join(project, "template.toml"), { force: true });
writeFileSync(
  join(project, "systems/echo.luau"),
  `return system "sim.echo" {
  stage = "sim",
  unfrozen = true,
  writes = { "UiText" },
  doc = "Writes each host message on the message line and sends it back, longer.",
  run = function(q)
    for _, ev in ipairs(q:events("host.message")) do
      q:entity("@messages"):get("UiText").text = ev.text
      q:send("echo", ev.channel .. ":" .. ev.text .. "!")
    end
  end,
}
`,
);
mkdirSync(join(project, "web"));
writeFileSync(
  join(project, "web/game.js"),
  `// Sends the game a message and keeps what it sends back, for the check.
const { player } = window.argus;
const turns = player.message("gm.ruling", "from the page");
window.hostCheck = { turns, sent: JSON.parse(player.take_sent()) };
`,
);

report.stage = "building for the web";
const built = spawnSync(argus, ["build", "--target", "web"], { cwd: project, encoding: "utf8", timeout: 40 * 60 * 1000 });
report.build = { status: built.status, tail: (built.stdout + built.stderr).split("\n").slice(-8).join("\n") };
const site = join(project, "dist", "web");
if (built.status !== 0 || !existsSync(join(site, "web", "game.js"))) {
  report.error = "the web build failed, or left web/game.js out";
  finish(1);
}
report.packs = JSON.parse(readFileSync(join(site, "data", "packs.json"), "utf8"));
if (report.packs.script !== "web/game.js") {
  report.error = "packs.json does not name the page script";
  finish(1);
}

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
const browser = spawn(
  chrome,
  ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--enable-unsafe-webgpu", "--mute-audio", "about:blank"],
  { stdio: ["ignore", chromeLog, chromeLog] },
);
// Chrome holds its profile until it has gone, so wait for it, then retry.
const cleanup = async () => {
  server.kill();
  if (browser.exitCode === null) {
    const gone = new Promise((r) => browser.once("exit", r));
    browser.kill();
    await Promise.race([gone, sleep(10000)]);
  }
  for (const dir of [profile, project]) {
    for (let i = 0; i < 20; i++) {
      try {
        rmSync(dir, { recursive: true, force: true });
        break;
      } catch {
        await sleep(250);
      }
    }
  }
};

let ws;
let nextId = 1;
const waiting = new Map();
const logs = [];
function send(method, params = {}) {
  const id = nextId++;
  return new Promise((ok, fail) => {
    const timer = setTimeout(() => fail(new Error(`${method}: no answer in 60 s`)), 60000);
    waiting.set(id, { ok: (v) => (clearTimeout(timer), ok(v)), fail: (e) => (clearTimeout(timer), fail(e)) });
    ws.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expr) {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(`${expr}: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
  return r.result.value;
}

try {
  report.stage = "starting Chrome";
  for (let i = 0; i < 200 && !ws; i++) {
    await sleep(100);
    const file = join(profile, "DevToolsActivePort");
    if (!existsSync(file)) continue;
    const port = readFileSync(file, "utf8").split("\n")[0].trim();
    const page = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((t) => t.type === "page");
    if (!page) continue;
    const sock = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise((ok, fail) => {
      sock.onopen = ok;
      sock.onerror = () => fail(new Error("the DevTools socket did not open"));
    });
    ws = sock;
  }
  if (!ws) throw new Error("Chrome did not start");
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && waiting.has(msg.id)) {
      const w = waiting.get(msg.id);
      waiting.delete(msg.id);
      if (msg.error) w.fail(new Error(JSON.stringify(msg.error)));
      else w.ok(msg.result);
    } else if (msg.method === "Runtime.consoleAPICalled") {
      logs.push(msg.params.args.map((a) => a.value ?? a.description).join(" "));
    }
  };
  await send("Runtime.enable");
  await send("Page.enable");

  report.stage = "opening the page";
  await send("Page.navigate", { url: `http://127.0.0.1:${HTTP}/` });
  for (let i = 0; i < 300; i++) {
    await sleep(100);
    if (await evaluate("!!window.hostCheck").catch(() => false)) break;
  }
  report.page = await evaluate("window.hostCheck ?? null");
  report.line = await evaluate("argus.player.semantic()").then((s) => JSON.parse(s)).then((s) => JSON.stringify(s).includes("from the page")).catch((e) => String(e));
  report.console = logs.slice(-10);
  // The report form waits for F8: shown from the start, it was the page's
  // own style beating its hidden attribute (issue 3).
  report.report_form_shown = await evaluate("getComputedStyle(document.getElementById('report')).display !== 'none'").catch((e) => String(e));
  const sent = report.page?.sent ?? [];
  const failures = [];
  if (report.report_form_shown !== false) failures.push(`the report form shows before F8 (${report.report_form_shown})`);
  if (!report.page) failures.push("web/game.js did not run");
  else {
    if (report.page.turns !== 1) failures.push(`the message took ${report.page.turns} turns, not 1`);
    if (!(sent.length === 1 && sent[0].channel === "echo" && sent[0].text === "gm.ruling:from the page!")) {
      failures.push(`the game's answer did not come back: ${JSON.stringify(sent)}`);
    }
  }
  report.failures = failures;
  report.stage = "done";
  await cleanup();
  finish(failures.length ? 1 : 0);
} catch (e) {
  report.error = String(e);
  report.console = logs.slice(-10);
  await cleanup();
  finish(1);
}
