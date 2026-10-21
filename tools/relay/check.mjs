// Check a running relay (docs/multiplayer-checklist.md item 2) the way a
// web page reaches one, with a WebSocket of Node's own:
//
//   node tools/relay/check.mjs ws://<address>:7879
//
// Hosts a room, joins it, sends a message each way and times the round
// trips, then leaves. Prints one JSON line; exits 1 if any step failed.
// Node 24, no npm packages.

const url = process.argv[2];
if (!url) {
  console.error("usage: check.mjs ws://<address>:<port>");
  process.exit(2);
}
const GAME = 0x52454c4159n; // "RELAY", a game no real one is

const u64 = (n) => {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, BigInt(n), true);
  return b;
};
const str = (s) => {
  const t = new TextEncoder().encode(s);
  const b = new Uint8Array(4 + t.length);
  new DataView(b.buffer).setUint32(0, t.length, true);
  b.set(t, 4);
  return b;
};
const cat = (...parts) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
};
const host = (players) => cat(Uint8Array.of(1, players), u64(GAME));
const join = (code) => cat(Uint8Array.of(2), str(code), u64(GAME), u64(0));
const send = (bytes) => cat(Uint8Array.of(3), u64(bytes.length), bytes);

function read(buf) {
  const b = new Uint8Array(buf);
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const text = (at) => {
    const n = v.getUint32(at, true);
    return [new TextDecoder().decode(b.subarray(at + 4, at + 4 + n)), at + 4 + n];
  };
  switch (b[0]) {
    case 11: {
      const [code, at] = text(1);
      return { kind: "room", code, slot: b[at], players: b[at + 1] };
    }
    case 12:
      return { kind: "present", slots: [...b.subarray(5, 5 + v.getUint32(1, true))] };
    case 13: {
      const n = Number(v.getBigUint64(2, true));
      return { kind: "from", slot: b[1], data: b.subarray(10, 10 + n) };
    }
    case 14:
      return { kind: "refused", why: text(1)[0] };
    default:
      return { kind: "unknown", byte: b[0] };
  }
}

function open() {
  return new Promise((ok, fail) => {
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    const queue = [];
    const waiting = [];
    ws.onmessage = (e) => {
      const m = read(e.data);
      const w = waiting.findIndex((x) => x.kind === m.kind || m.kind === "refused");
      if (w >= 0) waiting.splice(w, 1)[0].ok(m);
      else queue.push(m);
    };
    ws.onerror = () => fail(new Error(`cannot reach ${url}`));
    ws.onopen = () =>
      ok({
        ws,
        next: (kind, ms = 5000) =>
          new Promise((ok, fail) => {
            const i = queue.findIndex((m) => m.kind === kind || m.kind === "refused");
            if (i >= 0) return ok(queue.splice(i, 1)[0]);
            const t = setTimeout(() => fail(new Error(`no ${kind} in ${ms} ms`)), ms);
            waiting.push({ kind, ok: (m) => (clearTimeout(t), ok(m)) });
          }),
      });
  });
}

const report = { url, ok: false };
try {
  const a = await open();
  a.ws.send(host(2));
  const room = await a.next("room");
  if (room.kind !== "room") throw new Error(`refused: ${room.why}`);
  report.code = room.code;
  const b = await open();
  b.ws.send(join(room.code));
  const joined = await b.next("room");
  if (joined.kind !== "room") throw new Error(`join refused: ${joined.why}`);
  report.slots = [room.slot, joined.slot];
  const trips = [];
  for (let i = 0; i < 20; i++) {
    const t0 = performance.now();
    a.ws.send(send(Uint8Array.of(i)));
    const got = await b.next("from");
    b.ws.send(send(got.data));
    await a.next("from");
    trips.push(performance.now() - t0);
  }
  trips.sort((x, y) => x - y);
  report.round_trip_ms = { median: +trips[10].toFixed(1), worst: +trips[19].toFixed(1) };
  a.ws.close();
  b.ws.close();
  report.ok = true;
} catch (e) {
  report.error = e.message;
}
console.log(JSON.stringify(report));
process.exit(report.ok ? 0 : 1);
