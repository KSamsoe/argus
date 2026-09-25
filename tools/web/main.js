// The page around the web player: load the build's packs, make a renderer
// for the canvas, feed it keys, run a frame per animation frame, and send
// reports.
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

try {
  await init();
  const player = await loadPlayer();
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
  window.argus = { player, setRunning: (r) => (run = r), report: (n) => player.report(n), send };

  let n = 0;
  const frame = (t) => {
    try {
      player.frame(t, run);
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
