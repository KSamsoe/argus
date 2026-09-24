// The page around the web player: load the pack, make a renderer for the
// canvas, feed it keys, and run a frame per animation frame.
//
// Query parameters: scene, seed; webgl (force WebGL2); paused (draw but do
// not advance: how the headless checks step the sim exactly).
import init, { WebPlayer, open_renderer } from "./pkg/argus_web.js";

const q = new URLSearchParams(location.search);
const status = document.getElementById("status");
const canvas = document.getElementById("game");

try {
  await init();
  const pack = new Uint8Array(await (await fetch("game.pack")).arrayBuffer());
  const seed = q.has("seed") ? Number(q.get("seed")) : undefined;
  const player = new WebPlayer(pack, q.get("scene") ?? undefined, seed);
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
  canvas.addEventListener("pointerdown", openAudio);
  canvas.addEventListener("keydown", (e) => {
    openAudio();
    if (!e.repeat) player.key(e.code, true);
    e.preventDefault();
  });
  canvas.addEventListener("keyup", (e) => {
    player.key(e.code, false);
    e.preventDefault();
  });
  canvas.addEventListener("blur", () => player.release_all());
  canvas.focus();

  // What the headless checks reach for.
  window.argus = { player, setRunning: (r) => (run = r) };

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
