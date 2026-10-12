# Making art and sound in Argus: checklist

Kevin, 2026-10-11: after the art passes' engine gaps, tools for generating assets, the built-in and free kind first (no account, no key), sprites and sound together. A competitor's assistant makes models, sound, and voices with paid services; this is the part of that every person Kevin invites can use on day one, and it keeps Argus's own rules: an asset is text an agent can read, change, and diff; the same text makes the same asset on every machine; a change is an edit in the change feed, with its author and note.

Today a template's art is a Python script (`tools/art.py`, 160 to 1,385 lines) that writes PNGs and sidecars, and every example game's sound is a `tone` in `audio.toml`. Changing a picture needs Python, which a newcomer on Windows may not have, and a re-run; and a tone is a wave, a slide, and a two-stage envelope, too little for most effects.

## 0. Sound effects from a recipe

- `Tone` grows the controls of a sound-effect synthesiser (after sfxr): the envelope's sustain and punch; a slide that itself changes (delta); vibrato; an arpeggio (a jump in pitch partway); the square's duty and its sweep; a repeat; a phaser; low- and high-pass filters with their sweeps. Every new control defaults to what a tone does now, so every existing tone (all four games' sounds and music) renders the same samples.
- A sound sidecar (`assets/sfx/jump.toml`, `type = "sound"`) holds a tone like `audio.toml` does; nothing new to learn.
- Deterministic everywhere: the synthesiser's sine and exponentials come from a pure-Rust math library, not the platform's, so a recipe is the same samples on Windows, Linux, and in the browser.

## 1. Sound generators

- `argus asset generate sound <name> kind=<kind> [seed=<n>]` (and the `generate` MCP tool) writes a recipe: `pickup`, `laser`, `explosion`, `powerup`, `hit`, `jump`, `blip`, `step`, `click`. The seed picks values within the kind's ranges, so `seed=2` is another jump; `from=<sound> vary=<share>` makes a variation of one you have.
- It is an edit (`edit.file`): in the change feed with its author and note, undone by `edit undo`.
- Checking without ears: `asset get` on a sound says its length, peak, and loudness, and how its pitch and brightness move over time, in words an agent can check ("rises from 330 Hz to 660 Hz over 0.18 s, then fades").

## 2. Art scripts

- `assets/<name>.art.luau`: a Luau file the importer runs to draw a sprite, in place of a PNG and its sidecar. It returns the picture and says its frames, clips, pivots, and pixels per unit; nothing is written beside it. It reloads while the game runs, as a game script does: change a colour, and the game shows it.
- A pixel-art kit for it: palettes of named colours; pixels, lines, rectangles, circles, ellipses, polygons, flood fill; an outline round what is drawn; shading from a light direction; dithering; seeded noise; mirroring; drawing one canvas into another; recolouring; frames packed into a sheet with their clips.
- Sandboxed: no files, no clock, no network. Deterministic: its noise and random numbers are the kit's own, and its math the pure-Rust library's, so a script draws the same pixels on every machine.
- Errors name the script's file and line, as a game script's do. `argus asset export <name>` writes the PNG for a person who wants the file.
- A release build packs what the script drew, as it packs a PNG: a player's machine never runs it.

## 3. Sprite generators

- `argus asset generate sprite <name> kind=<kind> [seed=<n>] [size=<px>] [palette=<name>]` writes an art script, not a picture, so what it makes is code to change: `creature`, `character` (with idle and walk frames), `item`, `tile` (grass, stone, dirt, water, with edges for auto-tiling), `plant`, `rock`, `ship`.
- An agent looks at the result with `observe` (or `asset get` with a picture) and changes the script.

## 4. The templates on art scripts

- The blank template's art first (160 lines), then the other four, so changing a template's art needs nothing but Argus. Each one draws the same pixels as its PNGs did, checked picture against picture, and plays as before.

## 5. Docs and the agent's side

- A manual chapter, "Making art and sound"; the reference's entries for the kit and the generators, generated as the rest is; `ARGUS.md` lists a project's art scripts and recipes.

## The gate

- A trial agent with only the docs, over MCP, gives a template a new enemy with a generated sprite (an animated one) and a generated hit sound, both changed from what the generators gave; the enemy shows and sounds in the game, and the scenarios pass.
- A recipe and an art script make the same bytes on Windows, on Linux (klab), and in a web build.
- Every existing tone renders the same samples as before; every earlier gate's tests still pass.
