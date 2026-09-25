// The page around the web player: load the build's packs, make a renderer
// for the canvas, feed it keys, run a frame per animation frame, send
// reports, and keep the game's saves.
//
// Query parameters: scene, seed; lang (a language pack); webgl (force
// WebGL2); paused (draw but do not advance: how the headless checks step
// the sim exactly).
import init, { WebPlayer, open_renderer } from "./pkg/argus_web.js";

const q = new URLSearchParams(location.search);
const status = document.getElementById("status");
const said = document.getElementById("said");
const canvas = document.getElementById("game");
const form = document.getElementById("report");
const note = document.getElementById("note");

// A release build lists its packs in data/packs.json (argus build); the
// spike's page has one pack of every file.
async function loadPlayer() {
  const scene = q.get("scene") ?? undefined;
  const seed = q.has("seed") ? Number(q.get("seed")) : undefined;
  const listed = await fetch("data/packs.json")
    .then((r) => (r.ok ? r.json() : null))
    .catch(() => null);
  if (listed) {
    const packs = [];
    for (const name of listed.packs) {
      packs.push(new Uint8Array(await (await fetch("data/" + name)).arrayBuffer()));
    }
    return WebPlayer.from_packs(packs, q.get("lang") ?? undefined, scene, seed);
  }
  const pack = new Uint8Array(await (await fetch("game.pack")).arrayBuffer());
  return new WebPlayer(pack, scene, seed);
}

// A report goes to the server the page came from (tools/web/serve.mjs);
// where there is none, it is offered as a file instead.
async function send(bytes) {
  said.textContent = "Sending the report.";
  try {
    const r = await fetch("marks", { method: "POST", headers: { "content-type": "application/octet-stream" }, body: bytes });
    if (!r.ok) throw new Error(`the server said ${r.status}`);
    said.textContent = "Report sent. Thank you.";
  } catch (e) {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
    a.download = `report-${Date.now()}.argr`;
    a.textContent = "save the report";
    said.textContent = `The report could not be sent (${e.message}). You can `;
    said.append(a, " and send the file to the developer.");
  }
}

// Saves the game asks for (q:save) live in the browser's storage, so a
// reload keeps them; a load it asks for (q:load) finds one there (phase 3
// item 0). One IndexedDB database per game, a save per name.
function saveStore(game) {
  const opened = new Promise((ok, fail) => {
    const r = indexedDB.open(`argus:${game}`, 1);
    r.onupgradeneeded = () => r.result.createObjectStore("saves");
    r.onsuccess = () => ok(r.result);
    r.onerror = () => fail(r.error);
  });
  const use = async (mode, f) => {
    const db = await opened;
    return new Promise((ok, fail) => {
      const req = f(db.transaction("saves", mode).objectStore("saves"));
      req.onsuccess = () => ok(req.result);
      req.onerror = () => fail(req.error);
    });
  };
  return {
    put: (name, bytes) => use("readwrite", (s) => s.put(bytes, name)),
    get: (name) => use("readonly", (s) => s.get(name)),
    names: () => use("readonly", (s) => s.getAllKeys()),
  };
}

try {
  await init();
  const player = await loadPlayer();
  const store = saveStore(JSON.parse(player.status()).game);
  // After a frame: keep the saves it asked for, and do the loads.
  const savesAndLoads = () => {
    for (const [name, bytes] of player.take_saves()) {
      store.put(name, bytes).catch((e) => (said.textContent = `The game could not be saved: ${e}`));
    }
    for (const name of player.take_loads()) {
      store
        .get(name)
        .then((bytes) => {
          if (!bytes) throw new Error(`there is no save called ${name}`);
          player.load(bytes);
        })
        .catch((e) => (said.textContent = `The save could not be loaded: ${e.message ?? e}`));
    }
  };
  player.use_renderer(await open_renderer(canvas, q.has("webgl")));
  let run = !q.has("paused");

  // A browser lets a page make sound only once the player has pressed a
  // key or clicked, so the first of those opens the audio.
  let audio = false;
  const openAudio = () => {
    if (audio) return;
    audio = true;
    try {
      player.open_audio();
    } catch (e) {
      console.warn("no audio: " + e);
    }
  };

  // F8: pause and ask what happened.
  let wasRunning = run;
  const openReport = () => {
    wasRunning = run;
    run = false;
    player.release_all();
    form.hidden = false;
    note.value = "";
    note.focus();
  };
  const closeReport = () => {
    form.hidden = true;
    run = wasRunning;
    canvas.focus();
  };
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    const bytes = player.report(note.value.trim());
    closeReport();
    send(bytes);
  });
  document.getElementById("cancel").addEventListener("click", closeReport);
  note.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeReport();
  });

  canvas.addEventListener("pointerdown", openAudio);
  canvas.addEventListener("keydown", (e) => {
    openAudio();
    e.preventDefault();
    if (e.code === "F8") {
      if (!e.repeat) openReport();
      return;
    }
    if (!e.repeat) player.key(e.code, true);
  });
  canvas.addEventListener("keyup", (e) => {
    player.key(e.code, false);
    e.preventDefault();
  });
  canvas.addEventListener("blur", () => player.release_all());
  canvas.focus();

  // What the headless checks reach for.
  window.argus = { player, setRunning: (r) => (run = r), report: (n) => player.report(n), send, store, savesAndLoads };

  let n = 0;
  const frame = (t) => {
    try {
      player.frame(t, run);
      savesAndLoads();
    } catch (e) {
      status.textContent = "Stopped: " + e;
      return;
    }
    if (n++ % 30 === 0) {
      const s = JSON.parse(player.status());
      status.textContent = `${s.scene}  tick ${s.tick}  ${s.backend}  sprites ${s.drawn}  hero ${s.player?.map((v) => v.toFixed(2)).join(", ")}`;
    }
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
} catch (e) {
  status.textContent = "Could not start: " + e;
  console.error(e);
}
