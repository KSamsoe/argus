//! The semantic frame as an accessibility tree (section 12: "the semantic
//! frame (section 8) is, without modification, a screen-reader feed: the
//! same call the agent uses to know what's on screen can drive a player's
//! narrator"; phase 5 item 2). Kevin chose the player's own screen reader:
//! the frame's UI becomes AccessKit nodes that Narrator, NVDA, JAWS, Orca,
//! or VoiceOver reads and moves through, in the voice and speed the player
//! set. Each button with its words and whether it is the focus, the other
//! words as labels, in the frame's order; the HUD's lines in a live region,
//! so a change is spoken. A screen reader's press or focus comes back as an
//! [`ActionRequest`] on a button's node, which the game turns into the
//! pointer and a click, recorded input like a mouse's.

use crate::semantic::{SemanticFrame, UiSeen};
use accesskit::{Action, ActionRequest, Live, Node, NodeId, Rect, Role, Tree, TreeUpdate};
use argus_ecs::Entity;

const ROOT: NodeId = NodeId(1);
const HUD: NodeId = NodeId(2);

/// A UI node's id in the tree: its entity's.
pub fn node_of(e: Entity) -> NodeId {
    NodeId(e.to_bits() & !(1 << 63))
}

/// A HUD line's: its entity's, kept apart from the UI's.
fn hud_node(e: Entity) -> NodeId {
    NodeId(e.to_bits() | (1 << 63))
}

fn rect(b: [i32; 4]) -> Rect {
    Rect::new(b[0] as f64, b[1] as f64, b[2] as f64, b[3] as f64)
}

/// The tree for a frame of the game called `name`, its bounds in the
/// frame's pixels (the window's).
pub fn tree(frame: &SemanticFrame, name: &str) -> TreeUpdate {
    let mut nodes = Vec::new();
    let mut root = Node::new(Role::Window);
    root.set_label(name);
    let mut kids = Vec::new();
    let mut focus = ROOT;
    if !frame.hud.is_empty() {
        let mut hud = Node::new(Role::Group);
        hud.set_label("HUD");
        hud.set_live(Live::Polite);
        let mut lines = Vec::new();
        for s in &frame.hud {
            let words = match (s.name.is_empty(), s.text.is_empty()) {
                (_, true) => s.name.clone(),
                (true, false) => s.text.clone(),
                (false, false) => format!("{}: {}", s.name, s.text),
            };
            let mut line = Node::new(Role::Label);
            line.set_label(words);
            line.set_bounds(rect(s.bounds));
            lines.push(hud_node(s.entity));
            nodes.push((hud_node(s.entity), line));
        }
        hud.set_children(lines);
        kids.push(HUD);
        nodes.push((HUD, hud));
    }
    for u in &frame.ui {
        let id = node_of(u.entity);
        kids.push(id);
        nodes.push((id, ui_node(u)));
        if u.button && u.focused {
            focus = id;
        }
    }
    root.set_children(kids);
    nodes.push((ROOT, root));
    let mut tree = Tree::new(ROOT);
    tree.app_name = Some(name.to_string());
    tree.toolkit_name = Some("Argus".to_string());
    TreeUpdate {
        nodes,
        tree: Some(tree),
        focus,
    }
}

fn ui_node(u: &UiSeen) -> Node {
    let mut n = Node::new(if u.button { Role::Button } else { Role::Label });
    n.set_label(u.text.clone());
    n.set_bounds(rect(u.bounds));
    if u.button {
        if u.disabled {
            n.set_disabled();
        } else {
            n.add_action(Action::Click);
            n.add_action(Action::Focus);
        }
    }
    n
}

/// What a screen reader asked of the frame's UI: the button it means, and
/// whether to press it (a focus only moves to it).
pub fn asked<'a>(req: &ActionRequest, ui: &'a [UiSeen]) -> Option<(&'a UiSeen, bool)> {
    let press = match req.action {
        Action::Click => true,
        Action::Focus => false,
        _ => return None,
    };
    ui.iter()
        .find(|u| u.button && !u.disabled && node_of(u.entity) == req.target)
        .map(|u| (u, press))
}
