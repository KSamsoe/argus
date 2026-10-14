//! Issue 23 through the daemon: text typed into a text box comes down the
//! window's path (`input.device`'s `text`), is journaled, and a seek back
//! and forward types it again; a script reads `Actions.typed`; and the
//! semantic frame names the box with its text.

mod common;

use common::*;
use serde_json::{json, Value as Json};
use std::path::Path;

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"typing\"\nclock = \"continuous@60\"\ndefault_scene = \"main\"\nplugins = []\n",
    );
    write(
        root,
        "input.toml",
        r#"[actions.ui_move]
type = "axis2"
[actions.ui_move.bindings]
keyboard = ["arrows"]
[actions.ui_confirm]
type = "button"
[actions.ui_confirm.bindings]
keyboard = ["enter"]
[actions.ui_cancel]
type = "button"
[actions.ui_cancel.bindings]
keyboard = ["escape"]
[actions.ui_click]
type = "button"
[actions.ui_click.bindings]
mouse = ["left"]
[actions.jump]
type = "button"
[actions.jump.bindings]
keyboard = ["space"]
[slots.1]
devices = ["keyboard", "mouse"]
"#,
    );
    write(
        root,
        "components/said.luau",
        r#"component "Said" {
  doc = "What the player sent, and what the slot typed last",
  line = field.str {},
  typed = field.str {},
}
"#,
    );
    write(
        root,
        "systems/said.luau",
        r#"system "sim.said" {
  stage = "sim",
  reads = { "Actions" },
  writes = { "Said" },
  doc = "Keeps the last line sent, and what was typed each tick.",
  run = function(q)
    for _, s in q:each("Said") do
      for _, ev in ipairs(q:events("said")) do s.line = ev.text end
      for _, a in q:each("Actions") do
        if a.typed ~= "" then s.typed = a.typed end
      end
    end
  end,
}
"#,
    );
    write(
        root,
        "scenes/main/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/main/player.toml",
        "[PlayerSlot]\nslot = 1\n\n[Actions]\n\n[Said]\n",
    );
    write(root, "scenes/main/ui.toml", "[UiRoot]\n\n[UiNode]\n");
    write(
        root,
        "scenes/main/field.toml",
        "[Parent]\nentity = \"@ui\"\n\n[UiNode]\nsize = [300.0, 24.0]\n\n[UiButton]\n\n[UiText]\n\n[UiTextBox]\nhint = \"Say something\"\nevent = \"said\"\n",
    );
}

fn field(d: &mut argus_daemon::Daemon, path: &str) -> Json {
    let q = ok(
        d,
        "world.query",
        json!({ "select": [path], "where": { "has": [path.split('.').next().unwrap()] } }),
    );
    q["rows"][0][0].clone()
}

#[test]
fn typed_text_is_journaled_and_a_seek_types_it_again() {
    let t = temp("typing");
    project(t.path());
    let mut d = open(t.path());
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    let frame = ok(&mut d, "obs.frame", json!({ "format": "semantic" }));
    let ui = frame["ui"].as_array().unwrap();
    let tb = ui
        .iter()
        .find(|u| u["textbox"] == true)
        .unwrap_or_else(|| panic!("{frame}"));
    assert_eq!(tb["text"], "Say something", "the hint, not a caret");

    // Pressed (it has the focus), it types; then a word and Enter.
    ok(
        &mut d,
        "input.inject",
        json!({ "action": "ui_confirm", "frames": 1 }),
    );
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    assert_eq!(field(&mut d, "UiTextBox.typing"), true);
    ok(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "Space", "pressed": true }], "text": ["hello there"] }),
    );
    ok(&mut d, "sim.step", json!({ "n": 1 }));
    assert_eq!(field(&mut d, "UiTextBox.text"), "hello there");
    assert_eq!(
        field(&mut d, "Actions.jump"),
        false,
        "space typed, it did not jump"
    );
    assert_eq!(
        field(&mut d, "Said.typed"),
        "hello there",
        "a script reads Actions.typed"
    );
    let typing_frame = ok(&mut d, "sim.status", json!({}))["frame"]
        .as_u64()
        .unwrap();
    ok(
        &mut d,
        "input.device",
        json!({ "events": [{ "key": "Space", "pressed": false }], "text": ["!\r"] }),
    );
    ok(&mut d, "sim.step", json!({ "n": 2 }));
    assert_eq!(field(&mut d, "Said.line"), "hello there!");
    assert_eq!(field(&mut d, "UiTextBox.text"), "", "sent and emptied");
    let end = ok(&mut d, "sim.status", json!({}))["frame"]
        .as_u64()
        .unwrap();

    // Back to before any of it, and forward: the journal types it again.
    ok(&mut d, "sim.seek", json!({ "frame": 1 }));
    assert_eq!(field(&mut d, "Said.line"), "");
    ok(&mut d, "sim.seek", json!({ "frame": typing_frame }));
    assert_eq!(field(&mut d, "UiTextBox.text"), "hello there");
    ok(&mut d, "sim.seek", json!({ "frame": end }));
    assert_eq!(field(&mut d, "Said.line"), "hello there!");
}
