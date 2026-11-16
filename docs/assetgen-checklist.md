# Making art and sound in Argus: checklist

Kevin, 2026-10-11: after the art passes' engine gaps, tools for generating assets, the built-in and free kind first (no account, no key), sprites and sound together. A competitor's assistant makes models, sound, and voices with paid services; this is the part of that every person Kevin invites can use on day one, and it keeps Argus's own rules: an asset is text an agent can read, change, and diff; the same text makes the same asset on every machine; a change is an edit in the change feed, with its author and note.

Today a template's art is a Python script (`tools/art.py`, 160 to 1,385 lines) that writes PNGs and sidecars, and every example game's sound is a `tone` in `audio.toml`. Changing a picture needs Python, which a newcomer on Windows may not have, and a re-run; and a tone is a wave, a slide, and a two-stage envelope, too little for most effects.

## 0. Sound effects from a recipe

- `Tone` grows the controls of a sound-effect synthesiser (after sfxr): the envelope's sustain and punch; a slide that itself changes (delta); vibrato; an arpeggio (a jump in pitch partway); the square's duty and its sweep; a repeat; a phaser; low- and high-pass filters with their sweeps. Every new control defaults to what a tone does now, so every existing tone (all four games' sounds and music) renders the same samples.
- A sound sidecar (`assets/sfx/jump.toml`, `type = "sound"`) holds a tone like `audio.toml` does; nothing new to learn.
- Deterministic everywhere: the synthesiser's sine and exponentials come from a pure-Rust math library, not the platform's, so a recipe is the same samples on Windows, Linux, and in the browser.

**Built (2026-10-11).** A tone takes `punch`, `slide_accel`, `jump` and `jump_ms`, `vibrato` and `vibrato_hz`, `duty` and `duty_sweep`, `repeat_ms`, `lowpass`, `lowpass_sweep`, `resonance`, `highpass`, `highpass_sweep`, `phaser_ms`, `phaser_sweep`, and `noise_pitch`, each off by default. A test holds the synthesiser to the 0.0.4 code: every tone without the new controls renders the same samples, bit for bit, but sines, which are libm's now and within 1e-6 of the platform's. `synth.rs` pins four generated tones' samples.

## 1. Sound generators

- `argus asset generate sound <name> kind=<kind> [seed=<n>]` (and the `generate` MCP tool) writes a recipe: `pickup`, `laser`, `explosion`, `powerup`, `hit`, `jump`, `blip`, `step`, `click`. The seed picks values within the kind's ranges, so `seed=2` is another jump; `from=<sound> vary=<share>` makes a variation of one you have.
- It is an edit (`edit.file`): in the change feed with its author and note, undone by `edit undo`.
- Checking without ears: `asset get` on a sound says its length, peak, and loudness, and how its pitch and brightness move over time, in words an agent can check ("rises from 330 Hz to 660 Hz over 0.18 s, then fades").

**Built (2026-10-11).** `asset.generate` makes the text (writing nothing); the `generate` tool (`argus generate name=sfx/zap sound=laser seed=3`) writes it as an edit with the command that made it at its head, refuses to write over a file unless `replace=true`, imports it, and answers with `audio.describe`'s measure. `audio.describe` works on any sound the mixer has, a file's or a tone's: length, peak, loudness, when it is loudest and when it has faded to a tenth, pitch every 40 ms by autocorrelation (a peak after a dip, so pitched noise is not taken for a note), zero crossings, and a sentence ("a note starting near 560 Hz at 0.00 s, ending near 826 Hz at 0.14 s (higher)"). Making the generators read well found two of their own faults: a laser's and a hit's slide took the pitch to nothing before the end (they slide to a share of where they began now), and footsteps were at -22 dB.

## 2. Art scripts

- `assets/<name>.art.luau`: a Luau file the importer runs to draw a sprite, in place of a PNG and its sidecar. It returns the picture and says its frames, clips, pivots, and pixels per unit; nothing is written beside it. It reloads while the game runs, as a game script does: change a colour, and the game shows it.
- A pixel-art kit for it: palettes of named colours; pixels, lines, rectangles, circles, ellipses, polygons, flood fill; an outline round what is drawn; shading from a light direction; dithering; seeded noise; mirroring; drawing one canvas into another; recolouring; frames packed into a sheet with their clips.
- Sandboxed: no files, no clock, no network. Deterministic: its noise and random numbers are the kit's own, and its math the pure-Rust library's, so a script draws the same pixels on every machine.
- Errors name the script's file and line, as a game script's do. `argus asset export <name>` writes the PNG for a person who wants the file.
- A release build packs what the script drew, as it packs a PNG: a player's machine never runs it.

**Built (2026-10-11).** `art.rs` in argus-assets: the VM, the kit (also `c:rows`, pixel art as text, which the templates' Python drew from), and the importer's half, sharing the PNG path's cache. A release pack leaves the script out, as it does a PNG's source. Live: a scratch game's slime turned from green to red as its script was saved, with the game running. Found on the way: a Spine skeleton's JSON was not among the files a scan compares, so a new export kept the old skeleton until a restart (fixed, with a test).

## 3. Sprite generators

- `argus asset generate sprite <name> kind=<kind> [seed=<n>] [size=<px>] [palette=<name>]` writes an art script, not a picture, so what it makes is code to change: `creature`, `character` (with idle and walk frames), `item`, `tile` (grass, stone, dirt, water, with edges for auto-tiling), `plant`, `rock`, `ship`.
- An agent looks at the result with `observe` (or `asset get` with a picture) and changes the script.

**Built (2026-10-11).** `artgen.rs`: creature (Bollinger-style masks, mirrored, idle and walk frames), character (the blank template's shape, a hair style and colours of its own), item (a book of eight shapes), tile (wrapping noise, six materials, four variants), plant, and rock. Each kind is pinned at seed 1. `asset.preview` draws any sprite's frames on a checkerboard, and the `generate` tool answers with it, so an agent over MCP sees the sprite in the answer.

## 4. The templates on art scripts

- The blank template's art first (160 lines), then the other four, so changing a template's art needs nothing but Argus. Each one draws the same pixels as its PNGs did, checked picture against picture, and plays as before.

**Done (2026-10-11).** Blank by hand (its player pixel for pixel; its animator says idle to walk with `or` and faces the player with `face`, so the system that only mirrored the sprite is gone), the other four by four agents in parallel, each held to a frame-by-frame comparison with the PNGs it replaced (`scratchpad/port/compare_sprite.py`). Everything the Python drew without chance is the same pixel for pixel; what it drew by chance uses `art.rng` by the same rules (the roguelike's agent swapped one generator into both and got 0 pixels differing across its 32 tiles). Every scenario passes as before, and outside the art only comments and READMEs changed. Each template's shared colours and helpers are one module, `assets/shared.lib.luau`.

The ports found, and the kit now has: frame ranges in clips (`from`, `to`); `c:alpha`; `art.rgba`; modules (`require`), with a change to one importing again every script that uses it; and two faults fixed. Luau compiled `math.sin` and the rest to its own built-ins, the platform's, past the libm functions, so a script could draw differently on another machine (now told `math` may change, and a test holds it to libm bit for bit). And art scripts ran again on every scene load and scenario run (the arena's tests went from 0.35 s to 1.75 s); what a script drew is kept beside its sheet now, keyed by its and its modules' bytes, and the arena's tests take 0.34 s.

Left as they are: `art.mix` rounds a half up where Python rounded to even (the ports carry a half-to-even helper where they matched Python to the pixel); the `^` operator is the platform's `pow` (the reference says to use `math.pow`); there is no `math.hypot`; the packer leaves no gap between frames (sampling holds inside a frame, so nothing bleeds). The example games' art (Hollow, Delve, Swarm, Outpost) is still Python.

## 5. Docs and the agent's side

- A manual chapter, "Making art and sound"; the reference's entries for the kit and the generators, generated as the rest is; `ARGUS.md` lists a project's art scripts and recipes.

## The gate

- A trial agent with only the docs, over MCP, gives a template a new enemy with a generated sprite (an animated one) and a generated hit sound, both changed from what the generators gave; the enemy shows and sounds in the game, and the scenarios pass.
- A recipe and an art script make the same bytes on Windows, on Linux (klab), and in a web build.
- Every existing tone renders the same samples as before; every earlier gate's tests still pass.

**Where it stands (2026-10-12).** The trial is done: an agent with only the docs and the `argus` command (an install's layout, its templates beside it) made a platformer and gave it a slime, its sprite from `generate sprite=creature` redrawn (a dome in three frames, purple, facing the way it moves) and its hit sound from `generate sound=hit` reshaped (longer, a wobble, a closing filter), with two scenarios that can fail; all six scenarios pass, in about eight minutes. Its gaps are answered (2ef4e55): `t:run_until`'s `where`, `t:log` lines in `argus test`, the sentence's brightness and clipping, the platformer's enemy idea, and one rule for editing files. Every tone renders as before (`synth.rs`), and the kit's, the generators', and four tones' outputs are pinned. On klab (Linux) the art and sound tests pass, all 18, every pin the same. A web build never runs an art script (a pack carries the sheets the scripts drew, and leaves their modules out), and its tones come from the same Rust synth, on libm, that the determinism spike found bit-identical in Chrome. A full CI passes; a run overnight read slow and long only because the laptop slept through it (1:11 to 9:01).
