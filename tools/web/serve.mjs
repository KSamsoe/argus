// Serve a built site, and take the reports its players send:
//
//   node tools/web/serve.mjs [site] [port] [host] [marks-dir]
//
// Defaults: target-web/site, 8765, 127.0.0.1, <site>/../marks. The host may
// be several addresses, comma-separated: on klab, 127.0.0.1 for Tailscale's
// HTTPS proxy, which players reach, and the tailnet address, which
// `argus mark pull` reaches over plain HTTP.
//
//   GET  /...           the site's files, brotli-compressed for a browser
//                       that takes it (phase 5 item 0: the page cold-loads
//                       in under 5 s on a 20 Mbit link, section 17)
//   POST /marks         a report from the page (a replay file, ARGR), at
//                       most 8 MB; stored as <marks-dir>/<id>.argr
//   GET  /marks         {marks: [{id, bytes, at}]}, oldest first
//   GET  /marks/<id>    one report's bytes
//
// `argus mark pull url=http://host:port/` fetches the reports into a
// project's mark queue (phase 2 item 7).
import { createServer } from "node:http";
import { brotliCompressSync, constants as zlib } from "node:zlib";
import { readFile, writeFile, readdir, stat, mkdir } from "node:fs/promises";
import { extname, join, normalize, resolve, dirname } from "node:path";
import { randomBytes } from "node:crypto";

const root = process.argv[2] ?? "target-web/site";
const port = Number(process.argv[3] ?? 8765);
const hosts = (process.argv[4] ?? "127.0.0.1").split(",").filter(Boolean);
const marks = resolve(process.argv[5] ?? join(dirname(resolve(root)), "marks"));
const MAX = 8 << 20;
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".pack": "application/octet-stream",
  ".json": "application/json",
};

const send = (res, code, body, type = "application/json") => {
  const b = typeof body === "string" || body instanceof Uint8Array ? body : JSON.stringify(body);
  res.writeHead(code, { "content-type": type, "content-length": Buffer.byteLength(b), "cache-control": "no-store" });
  res.end(b);
};

async function takeReport(req, res) {
  const parts = [];
  let n = 0;
  for await (const c of req) {
    n += c.length;
    if (n > MAX) return send(res, 413, { error: "a report is at most 8 MB" });
    parts.push(c);
  }
  const body = Buffer.concat(parts);
  if (body.subarray(0, 4).toString("latin1") !== "ARGR") return send(res, 400, { error: "not a report (a replay file starts ARGR)" });
  await mkdir(marks, { recursive: true });
  const id = `${Date.now()}-${randomBytes(3).toString("hex")}`;
  await writeFile(join(marks, `${id}.argr`), body);
  console.log(`report ${id}: ${body.length} bytes`);
  send(res, 201, { id });
}

async function listReports(res) {
  let names = [];
  try {
    names = (await readdir(marks)).filter((n) => n.endsWith(".argr")).sort();
  } catch {}
  const rows = [];
  for (const n of names) {
    const s = await stat(join(marks, n));
    rows.push({ id: n.slice(0, -5), bytes: s.size, at: s.mtime.toISOString() });
  }
  send(res, 200, { marks: rows });
}

// Each file compressed once, while it is the same file.
const compressed = new Map();
async function brotli(file) {
  const s = await stat(file);
  const key = `${s.mtimeMs}:${s.size}`;
  const have = compressed.get(file);
  if (have?.key === key) return have.body;
  const body = brotliCompressSync(await readFile(file), {
    params: { [zlib.BROTLI_PARAM_QUALITY]: 9, [zlib.BROTLI_PARAM_SIZE_HINT]: s.size },
  });
  compressed.set(file, { key, body });
  return body;
}

const handle = async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
  try {
    if (path === "/marks" && req.method === "POST") return await takeReport(req, res);
    if (path === "/marks" && req.method === "GET") return await listReports(res);
    const m = path.match(/^\/marks\/([0-9a-z-]+)$/);
    if (m && req.method === "GET") {
      return send(res, 200, await readFile(join(marks, `${m[1]}.argr`)), "application/octet-stream");
    }
    if (req.method !== "GET") return send(res, 405, { error: "GET, or POST /marks" });
    const file = normalize(join(root, path.endsWith("/") ? path + "index.html" : path));
    if (!file.startsWith(normalize(root))) return send(res, 403, { error: "outside the site" });
    const type = types[extname(file)] ?? "application/octet-stream";
    if (/\bbr\b/.test(req.headers["accept-encoding"] ?? "")) {
      const body = await brotli(file);
      res.writeHead(200, { "content-type": type, "content-encoding": "br", "content-length": body.length, "cache-control": "no-store", vary: "accept-encoding" });
      return res.end(body);
    }
    send(res, 200, await readFile(file), type);
  } catch {
    send(res, 404, { error: "not found" });
  }
};

for (const host of hosts) {
  createServer(handle).listen(port, host, () =>
    console.log(`serving ${root} on http://${host}:${port}/, reports into ${marks}`),
  );
}
