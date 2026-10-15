# Reaching players: checklist

Kevin, 2026-10-15: with 0.1.0 out and invitees about to make games on it, the next work is what their games need to reach people: touch, so a web build plays on a phone or a tablet; a game's own icon and name, and a Mac app a player double-clicks; rich text in the UI; and the small fixes found making 0.1.0. Chosen over online multiplayer and over another staples comparison. In order:

## 0. Small fixes found making 0.1.0

- **The web click carries its modifiers.** A browser's pointer and wheel events say which of Ctrl, Shift, Alt, and Meta are held. The web page passes them on, so a `ctrl+left` binding works even when Ctrl was pressed while something else on the page had the focus. Bring Your Own Game Master's page passes Ctrl in by hand today, because its own text box has the focus.
- **Undo a named edit.** `edit.undo` takes back the author's last transaction. It can also name one (`tx`), refused with the reason when later edits touched the same fields. An agent in a fresh project asked for this.

## 1. Touch

- Touch points as device input: each finger's id, place, and phase, from the web page's pointer events (`pointerType` touch or pen) and from a touchscreen on the desktop. They are recorded in the input journal like every input, so replays, seeks, and reports match.
- With no touch controls declared, a tap is a left click at that place and a drag moves the pointer, so a game made for the mouse works under a finger: UI buttons, the engine's click, `q:screen_to_world`.
- Touch controls in `input.toml`: a button (a region of the screen bound to actions as a key is) and a stick (a region where a drag is an axis), placed in the canvas's units, anchored to a corner or an edge. The engine draws them over the game, in the theme's colours, unless the game draws its own. They show while the player is touching and hide again after a few seconds of keys or a mouse.
- The web page stops the browser from scrolling, zooming, and selecting under a finger, and fits the game to the screen in either orientation.
- Tests: the input crate (taps, drags, two fingers, a control pressed and let go), the web check in headless Chrome (dispatched touches play a game), and a replay of a touch run reaching the same world.

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
