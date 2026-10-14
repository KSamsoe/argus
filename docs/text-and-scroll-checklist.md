# Text entry and scrolling: checklist

Kevin, 2026-10-13: the engine's UI has no text box and no scrolling (GitHub issue 23). Typed characters reach only the engine's own prompts (the F8 note, a release build's report); they are not delivered to scripts or recorded, so Bring Your Own Game Master, whose whole input is a typed sentence, uses an HTML element on its web page, which the desktop build, the editor, and the daemon do not have. A log is a clipped node of wrapped text that the script trims and lays out by hand.

## 0. Typed text

- What a player types is input like a key: the window, the web page, the editor's viewport, and `input.device`'s `text` hand it to the game, and the input journal records it (`Command::Text`), so a replay, a mark, and a report type it again. Backspace, Enter, and Escape come as their control characters.
- A slot's typed text for the tick is `Actions.typed`, a string a script can read (a console, a chat line). While `Actions.typing` is true for a slot, its keyboard types instead of driving actions: no key it presses moves an action, and what was held is let go. The gamepad and the mouse work as before.

**Built (2026-10-13).** `DeviceInput::Text` and `DeviceInput::Wheel` in argus-input. The window, the release game, the daemon's window and `input.device`, the web page (`player.text`, `player.wheel`), and the editor's viewport all feed text in. Text goes to a slot only while it is typing (so ordinary play journals no character per key), as `Command::Text`; until its tick it waits among the messages on a channel no game can name, so a keyframe keeps it as it keeps them. While a slot types, its keyboard resolves as nothing held, which lets go of what was held; when it stops, the keys still down (the Enter that sent the line) are ignored until let go, or they would press the button the focus is on. `typed` and `typing` are the engine's names in an action map, as `device` is. In a turn-based game, a tick whose input was only the UI's (characters with no line sent, the wheel, the pointer, and taps going back to rest) is frozen by `input.actions`, from the journal, so a seek replays it frozen: it takes no turn, and the systems that run `unfrozen` (the UI's) see it. Enter, which sends the line, is a turn.

## 1. The text box

- `UiTextBox` on a UI node with a `UiButton` (to be focused and pressed) and a `UiText` (to show it): `text`, `hint` (shown while empty), `max` characters, `event` (sent with the text on Enter), `clear` (empty it after), `keep` (keep typing after Enter, for an input line that is always open), and `typing`.
- Pressing it starts typing; Enter sends `event` (and `ui.submitted`) with `{entity, text}`; Escape, the cancel action, or a click elsewhere stops. A script starts it with `typing = true`. While it types, its `UiText` shows the text and a caret; empty and not typing, the hint.
- The semantic frame and the screen reader name it as a text box with its text.

**Built (2026-10-13).** `UiTextBox` in argus-ui, handled by `ui.input`: it sets `Actions.typing` while a box types (and lets it go when one stops, leaving a game's own use of it alone), takes `Actions.typed` in, and writes what the box's `UiText` shows. The pointer passing over another button does not take the focus from a box that types; the focus moving off it (a pad's nav) stops it; a box a script set typing takes the focus. The semantic frame marks it `textbox` with its text or hint (not the caret), and the screen reader gets a text input with that value.

## 2. Scrolling

- `UiScroll` on a UI node: its children are laid out at their own sizes, what is outside the node is clipped, and `y` is how far it is scrolled. With `end` (the default), a node scrolled to the end stays there as content is added: a log keeps the newest line in view. `content` and `at_end` say how tall the content is and whether the end is in view.
- The mouse wheel scrolls the node under the pointer (`mouse = ["wheel"]` on an `axis` action, the UI's `scroll` action, `ui_scroll` by default); a script sets `y`.

**Built (2026-10-13).** `UiScroll` in argus-ui. Layout gives the node taffy's scroll overflow (so it is as big as it is given, not as its content), clips it, measures its content from its children, and moves them up by `y`. It stays at the end while it was there and `y` is where layout left it (`seen`), so a script or the wheel moving `y` takes it off the end, and reaching the end again follows. The wheel reaches the game as an axis bound to `mouse = ["wheel"]`: a notch is a one-tick inject of 1 or -1, journaled as one. `UiRoot.scroll` names the action (`ui_scroll`). Lines scrolled out of view are out of the semantic frame, since their bounds are clipped.

## 3. Docs

- The presentation chapter (a text box, a log), the input reference (`wheel`, `Actions.typed`, `Actions.typing`), the components reference, and the templates' input files.

**Done (2026-10-13).** The presentation chapter has "A text box" and "A log that scrolls" and the two components in its table; the `input.toml` reference has the `wheel` binding and the engine's `typed` and `typing`; `input.device` takes `{wheel}` and documents `text`; the components reference comes from their docs. No template has a menu with a log or a box, so their input files are as they were.

## Gate

- Tests: typed text journaled and replayed; keys withheld while typing and held ones let go; a text box typed into, sent, cleared, stopped by Escape and by a click; a log that keeps its newest line in view, scrolled back by the wheel, staying where it was put while lines are added, and going back to following at the end.
- One example uses each: Delve's message log on `UiScroll`, and a text box where a game would have one. (Done for the log. No example has a natural place for a box: Delve's new run could take a typed seed, but scripts cannot seed the random streams. The box is held by its tests, and Bring Your Own Game Master is its first user.)
- Then Bring Your Own Game Master's own box and log are the game's to do.
