# Reaching players: checklist

Kevin, 2026-10-15: with 0.1.0 out and invitees about to make games on it, the next work is what their games need to reach people: touch, so a web build plays on a phone or a tablet; a game's own icon and name, and a Mac app a player double-clicks; rich text in the UI; and the small fixes found making 0.1.0. Chosen over online multiplayer and over another staples comparison. In order:

## 0. Small fixes found making 0.1.0

- **The web click carries its modifiers.** A browser's pointer and wheel events say which of Ctrl, Shift, Alt, and Meta are held. The web page passes them on, so a `ctrl+left` binding works even when Ctrl was pressed while something else on the page had the focus. Bring Your Own Game Master's page passes Ctrl in by hand today, because its own text box has the focus.
- **Undo a named edit.** `edit.undo` takes back the author's last transaction. It can also name one (`tx`), refused with the reason when later edits touched the same fields. An agent in a fresh project asked for this.

Built (2026-10-15). The page keeps the keys it has told the game are down, and on a pointer or wheel event passes any difference in the event's modifier flags on as those keys (`tools/web/main.js`); the host check presses Ctrl in a text box of the page script's and then clicks the game, and gets `ctrl+left`'s action alone (on the page before, a plain click). `edit.undo {tx}`, and the edit tool's `tx` with `undo`: each op says what it changes (`Op::scope`, a thing and a dotted path), and the later commits still in effect are found by walking the feed (an undo of one of them takes it out). A clash is error 4718, naming each commit in the way with its author and note. Tests: argus-daemon `tests/undo_named.rs` (a named undo, plain undo still the latest left, another author's change in the way and then undone, a component's removal meeting a field's edit).

## 1. Touch

- Touch points as device input: each finger's id, place, and phase, from the web page's pointer events (`pointerType` touch or pen) and from a touchscreen on the desktop. They are recorded in the input journal like every input, so replays, seeks, and reports match.
- With no touch controls declared, a tap is a left click at that place and a drag moves the pointer, so a game made for the mouse works under a finger: UI buttons, the engine's click, `q:screen_to_world`.
- Touch controls in `input.toml`: a button (a region of the screen bound to actions as a key is) and a stick (a region where a drag is an axis), placed in the canvas's units, anchored to a corner or an edge. The engine draws them over the game, in the theme's colours, unless the game draws its own. They show while the player is touching and hide again after a few seconds of keys or a mouse.
- The web page stops the browser from scrolling, zooming, and selecting under a finger, and fits the game to the screen in either orientation.
- Tests: the input crate (taps, drags, two fingers, a control pressed and let go), the web check in headless Chrome (dispatched touches play a game), and a replay of a touch run reaching the same world.

Built (2026-10-15). `DeviceInput::Touch {id, phase, x, y}` in UI canvas pixels, and `argus_input::touch`: `[touch.<name>]` parsed before the actions (kind, anchor, at, size, label, floating, draw), laid out on the canvas (`InputState::set_canvas`, the theme's), and `touch` bindings checked against the controls by kind. `Devices` keeps the fingers: one lands as new and takes its control when the devices are next resolved, with the layout known (the nearest in reach, a stick one finger at a time, a floating stick's half of the screen last); else the first is the mouse (`left` and the pointer, without counting as the mouse in use); else nothing. Touch resolves into the actions as every device does, so the journal records actions and replays need nothing new. `Family::Touch` (index 5): the `device` action, prompts (a control's label, or its action's name; a click is "Tap"), and the controls drawn only while it is the family in use (`Devices::touch_shown`). Found and fixed on the way: on a discrete clock `device` was scheduled as a press, so the first touch or pad press took a turn of its own and the device went back to the keyboard's the tick after; it is a state now, as the pointer is. Hosts: the web page (pointer events of type touch, captured from landing to lift; the canvas fits the screen at 16 by 9, with `touch-action: none`), the desktop window (winit's touches, and its mouse not heard while a finger is down and half a second after, since Windows sends a touch as the mouse too), the release game, and the daemon (`input.device`'s touch in canvas pixels, `input.devices`' touches, the controls drawn in the window, the stream, and pictures, and the semantic frame's `touch`). `t.input:touch` for scenarios. Tests: argus-input `tests/touch.rs`, argus-daemon `tests/touch.rs` (a stick walks the hero, a seek plays it again), the host check in headless Chrome (a finger on a touch button, a finger as the mouse, the semantic frame's list), and `formats.rs` holding the reference to the parser's keys and anchors.

## 2. A game's own icon and name

- `argus.toml`'s `[game]` names the game, its version, and its icon (a PNG of the art, square, at least 256 pixels).
- `argus build` puts them where each platform looks, with no compiler:
  - Windows: the icon and version inside the executable's resources, and the window's icon.
  - The web: the page's title, its icon, and a manifest, so "Add to Home Screen" on a phone gives the game's name and icon.
  - The Mac: an `.app` bundle with the icon and an `Info.plist`, signed ad hoc as the runtime is. A player double-clicks it. A download from the web is still stopped by Gatekeeper the first time (no Apple developer account signs it), and the release's README says how to open it.
  - Linux: the icon beside the executable and a `.desktop` file.
- Templates get an icon drawn by their art script, as their sprites are.
- Tests: the built executable's resources read back, the web page's manifest, the `.app`'s layout and plist checked on klab, and `argus build` with no icon still working (the engine's own).

## 3. Rich text

- `UiText` takes inline markup: a colour (a theme colour's name or `#RRGGBB`), bold and italic (from the SDF font), a size, an input glyph that follows the player's device (`[glyph=jump]`, as `InputGlyph` shows), and an inline sprite frame (`[img=icons:3]`). `[[` writes a bracket.
- Wrapping, alignment, holes (`{Component.field}`), and string tables work as now, markup inside them too.
- The semantic frame and the screen reader get the plain words, with the glyph as its label.
- Markup that does not read is shown as written, and reported once as a warning with the node and the place.
- Tests: layout (a line that wraps inside a coloured run, a glyph changing with the device), the semantic frame's plain text, and a picture golden.

## Docs

- Gameplay (touch), presentation (rich text), releasing (the icon, the Mac app, a phone), the references, and the file formats.

## Gate

- Tests for each part, and the games using them:
  - Hollow's web build played by touch, with a stick and a jump button, in headless Chrome and on a phone.
  - Every example game built with its own icon, and a Mac `.app` for each one that has a Mac release.
  - Delve's log in rich text: damage and healing in colour, and the menu's prompts with the player's own keys inline.
