//! The UI's components (§12 "UI and text"): a tree of ordinary entities.
//! A [`UiRoot`] is a tree's top; every node has a [`UiNode`] (how it is laid
//! out) and, after `ui.layout`, a [`UiRect`] (where it landed); what a node
//! shows is a [`UiPanel`], a [`UiText`], or both; a [`UiButton`] can take
//! focus and be pressed. Children are entities with a `Parent`.
//!
//! Enumerations are strings (`direction = "row"`), so a file reads as it
//! lays out; sizes are vectors, so a tween can move them (§13: "UI slides
//! are tweens").

use argus_ecs::{Component, Entity, StrId, Vec2};

/// The top of a UI tree.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "The top of a UI tree: laid out on the UI canvas (ui.toml's width and height), or hung from a world entity (follow). Draw order among trees is layer; the tree takes focus moves and presses from one player slot's actions"
)]
pub struct UiRoot {
    /// Draw order among trees.
    #[argus(doc = "Draw order among trees: higher is drawn over lower", range(-1000, 1000))]
    pub layer: i32,
    /// Player slot.
    #[argus(
        doc = "The player slot whose actions move focus and press (0: slot 1)",
        range(0, 8)
    )]
    pub slot: u8,
    /// Whether it takes input.
    #[argus(doc = "Takes focus moves and presses (a HUD does not)")]
    pub interactive: bool,
    /// A world entity to hang from.
    #[argus(
        doc = "A world entity the tree hangs from (a sign's words, a health bar); null: the screen"
    )]
    pub follow: Entity,
    /// Offset from it.
    #[argus(doc = "Where on the followed entity, in world units from its position")]
    pub follow_offset: Vec2,
    /// The focused node.
    #[argus(doc = "The node with focus (written by ui.input; a script may set it)")]
    pub focus: Entity,
    /// Action that moves focus.
    #[argus(doc = "The axis2 action that moves focus (empty: ui_move)")]
    pub nav: StrId,
    /// Action that presses.
    #[argus(doc = "The button action that presses the focused node (empty: ui_confirm)")]
    pub confirm: StrId,
    /// Action that backs out.
    #[argus(doc = "The button action that backs out: ui.cancel is emitted (empty: ui_cancel)")]
    pub cancel: StrId,
    /// Action that points.
    #[argus(
        doc = "The pointer action: where the pointer is on the canvas; moving it focuses the button under it (empty: pointer)"
    )]
    pub pointer: StrId,
    /// Action that clicks.
    #[argus(doc = "The button action that presses the button under the pointer (empty: ui_click)")]
    pub click: StrId,
    /// Action that scrolls.
    #[argus(
        doc = "The axis action that scrolls the UiScroll node under the pointer, up positive: a mouse wheel bound with mouse = [\"wheel\"] (empty: ui_scroll)"
    )]
    pub scroll: StrId,
    /// Direction held.
    #[argus(doc = "The nav direction held last tick (written by ui.input)")]
    pub held: Vec2,
    /// Until the next repeat.
    #[argus(doc = "Seconds until a held direction moves focus again (written by ui.input)")]
    pub wait: f32,
    /// Where the pointer was.
    #[argus(doc = "Where the pointer was last tick (written by ui.input)")]
    pub pointer_at: Vec2,
}

impl Default for UiRoot {
    fn default() -> Self {
        UiRoot {
            layer: 0,
            slot: 0,
            interactive: true,
            follow: Entity::NULL,
            follow_offset: Vec2::ZERO,
            focus: Entity::NULL,
            nav: StrId::EMPTY,
            confirm: StrId::EMPTY,
            cancel: StrId::EMPTY,
            pointer: StrId::EMPTY,
            click: StrId::EMPTY,
            scroll: StrId::EMPTY,
            held: Vec2::ZERO,
            wait: 0.0,
            pointer_at: Vec2::ZERO,
        }
    }
}

/// How a node is laid out (a flexbox, §12).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "How a UI node is laid out, flexbox style: its children in a row or a column, aligned and spaced; its own size fixed (size), a share of its parent (fill), or its content's. absolute takes it out of the flow and places it by anchor and pivot"
)]
pub struct UiNode {
    /// Main axis.
    #[argus(doc = "Children in a column (default) or a row")]
    pub direction: StrId,
    /// Main-axis placement.
    #[argus(
        doc = "Children along the main axis: start (default), center, end, between, around, evenly"
    )]
    pub justify: StrId,
    /// Cross-axis placement.
    #[argus(doc = "Children across: stretch (default), start, center, end")]
    pub align: StrId,
    /// Own cross-axis placement.
    #[argus(
        doc = "This node across its parent, over the parent's align: start, center, end, stretch; empty: the parent's"
    )]
    pub align_self: StrId,
    /// Wrap children.
    #[argus(doc = "Children wrap onto more lines when they do not fit")]
    pub wrap: bool,
    /// Space between children.
    #[argus(doc = "Pixels between children", range(0, 10000), tune)]
    pub gap: f32,
    /// Padding.
    #[argus(doc = "Pixels inside the edge: [left and right, top and bottom]", tune)]
    pub pad: Vec2,
    /// Margin.
    #[argus(
        doc = "Pixels outside the edge: [left and right, top and bottom]",
        tune
    )]
    pub margin: Vec2,
    /// Fixed size.
    #[argus(
        doc = "Size in pixels per axis; 0: from fill, else from the content",
        tune
    )]
    pub size: Vec2,
    /// Share of the parent.
    #[argus(
        doc = "Size as a share of the parent per axis (1: all of it); 0: not used",
        tune
    )]
    pub fill: Vec2,
    /// Smallest size.
    #[argus(
        doc = "Smallest size in pixels per axis; 0: none. It takes the place of the content as the smallest, so it may shrink past its content: shrink = 0 keeps the content"
    )]
    pub min: Vec2,
    /// Largest size.
    #[argus(doc = "Largest size in pixels per axis; 0: none")]
    pub max: Vec2,
    /// Grow into free space.
    #[argus(
        doc = "Share of the free space along the parent's main axis it takes",
        range(0, 1000)
    )]
    pub grow: f32,
    /// Shrink when short of space.
    #[argus(
        doc = "How readily it shrinks when the parent is short of space",
        range(0, 1000)
    )]
    pub shrink: f32,
    /// Out of the flow.
    #[argus(doc = "Out of the flow, placed by anchor and pivot in its parent")]
    pub absolute: bool,
    /// Point of the parent.
    #[argus(
        doc = "Where in the parent an absolute node is placed: [0, 0] top left, [1, 1] bottom right"
    )]
    pub anchor: Vec2,
    /// Point of itself.
    #[argus(
        doc = "Which point of an absolute node sits at the anchor: [0, 0] its top left, [0.5, 0.5] its middle"
    )]
    pub pivot: Vec2,
    /// Moved after layout.
    #[argus(
        doc = "Pixels it and its subtree are moved after layout, without moving the others (a slide is a tween of it)",
        tune
    )]
    pub offset: Vec2,
    /// Order among siblings.
    #[argus(doc = "Order among its siblings: lower first; equal: the order they were made", range(-100000, 100000))]
    pub order: i32,
    /// Shown.
    #[argus(doc = "Shown and laid out; hidden takes no space")]
    pub visible: bool,
    /// Clip children.
    #[argus(doc = "What its children draw outside it is cut off")]
    pub clip: bool,
    /// Opacity.
    #[argus(doc = "Opacity of it and its subtree", range(0, 1), tune)]
    pub opacity: f32,
}

impl Default for UiNode {
    fn default() -> Self {
        UiNode {
            direction: StrId::EMPTY,
            justify: StrId::EMPTY,
            align: StrId::EMPTY,
            align_self: StrId::EMPTY,
            wrap: false,
            gap: 0.0,
            pad: Vec2::ZERO,
            margin: Vec2::ZERO,
            size: Vec2::ZERO,
            fill: Vec2::ZERO,
            min: Vec2::ZERO,
            max: Vec2::ZERO,
            grow: 0.0,
            shrink: 1.0,
            absolute: false,
            anchor: Vec2::ZERO,
            pivot: Vec2::ZERO,
            offset: Vec2::ZERO,
            order: 0,
            visible: true,
            clip: false,
            opacity: 1.0,
        }
    }
}

/// Where a node landed (written by `ui.layout`).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Where a UI node landed on the canvas, written by ui.layout: top-left pos and size in canvas pixels (y down), draw order, whether it shows, its opacity, and the clip in effect"
)]
pub struct UiRect {
    /// Top left.
    #[argus(doc = "Top left, in canvas pixels (y down)")]
    pub pos: Vec2,
    /// Size.
    #[argus(doc = "Width and height, in canvas pixels")]
    pub size: Vec2,
    /// Draw order.
    #[argus(doc = "Draw order over the whole canvas: higher is drawn later")]
    pub depth: u32,
    /// Shown.
    #[argus(doc = "Shown: it and every ancestor visible")]
    pub shown: bool,
    /// Opacity.
    #[argus(doc = "Opacity, its own times its ancestors'")]
    pub alpha: f32,
    /// Clip top left.
    #[argus(doc = "Top left of the clip in effect")]
    pub clip_min: Vec2,
    /// Clip bottom right.
    #[argus(doc = "Bottom right of the clip in effect")]
    pub clip_max: Vec2,
}

/// Words.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Words in a UI node, wrapped to its width: text, or a string table's (key, in the current language), which may hold {Component.field} and {@id:Component.field} holes, filled each layout. size, color, and font default to the theme's (ui.toml)"
)]
pub struct UiText {
    /// The words.
    #[argus(doc = "The words; {Component.field} and {@id:Component.field} are filled in")]
    pub text: StrId,
    /// A string's key.
    #[argus(
        doc = "A string table's key (strings/<language>.toml): when set, the words are the table's in UiSettings.language, falling back to the base language, and text is not used"
    )]
    pub key: StrId,
    /// Pixel size.
    #[argus(
        doc = "Text size in canvas pixels (the line's height before scaling); 0: the theme's",
        range(0, 1000),
        tune
    )]
    pub size: f32,
    /// Colour.
    #[argus(doc = "A theme colour's name or #RRGGBB(AA); empty: the theme's text colour")]
    pub color: StrId,
    /// Font.
    #[argus(
        doc = "A font asset's name; empty: the theme's font (the built-in pixel font when it names none)"
    )]
    pub font: StrId,
    /// Alignment.
    #[argus(doc = "left (default), center, or right")]
    pub align: StrId,
    /// Wrap.
    #[argus(doc = "Wrap at the node's width (else one line)")]
    pub wrap: bool,
    /// What shows.
    #[argus(doc = "The words as shown, holes filled (written by ui.layout)")]
    pub shown: StrId,
}

impl Default for UiText {
    fn default() -> Self {
        UiText {
            text: StrId::EMPTY,
            key: StrId::EMPTY,
            size: 0.0,
            color: StrId::EMPTY,
            font: StrId::EMPTY,
            align: StrId::EMPTY,
            wrap: true,
            shown: StrId::EMPTY,
        }
    }
}

/// A panel behind a node.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "What a UI node draws behind its content: a flat colour, an image (nine-sliced when slice is set, so its corners keep their size), and a border"
)]
pub struct UiPanel {
    /// Fill colour.
    #[argus(
        doc = "Fill: a theme colour's name or #RRGGBB(AA); empty: none (or white under an image)"
    )]
    pub color: StrId,
    /// Image.
    #[argus(doc = "A sprite asset drawn over the fill, tinted by it; empty: none")]
    pub image: StrId,
    /// Which frame of the image.
    #[argus(
        doc = "Which frame of the image, 0 first: a panel can be one frame of a sheet; past the last, the first"
    )]
    pub frame: u32,
    /// Nine-slice inset.
    #[argus(
        doc = "Nine-slice: pixels of the image's edge that keep their size (corners) while the middle stretches; 0: stretched whole"
    )]
    pub slice: f32,
    /// Image pixel size.
    #[argus(
        doc = "Canvas pixels per image pixel for the slices (0: 1)",
        range(0, 100)
    )]
    pub scale: f32,
    /// Border width.
    #[argus(doc = "Border width in canvas pixels; 0: none", range(0, 100), tune)]
    pub border: f32,
    /// Border colour.
    #[argus(doc = "Border colour; empty: the theme's border")]
    pub border_color: StrId,
}

/// Something to press.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A UI node that takes focus and is pressed: the focus moves between buttons with the tree's nav action (to the nearest in that direction, or up/down/left/right when set), and confirm or a pointer press presses the focused one, emitting ui.pressed {entity, event} and the button's event"
)]
pub struct UiButton {
    /// Event on press.
    #[argus(
        doc = "An event emitted when it is pressed, beside ui.pressed; empty: only ui.pressed"
    )]
    pub event: StrId,
    /// Not pressable.
    #[argus(doc = "Shown, but focus passes over it and it cannot be pressed")]
    pub disabled: bool,
    /// Has focus.
    #[argus(doc = "Has focus (written by ui.input)")]
    pub focused: bool,
    /// Under the pointer.
    #[argus(doc = "Under the pointer (written by ui.input)")]
    pub hovered: bool,
    /// Pressed this tick.
    #[argus(doc = "Pressed this tick (written by ui.input)")]
    pub pressed: bool,
    /// Up neighbour.
    #[argus(doc = "Where focus goes up; null: the nearest button above")]
    pub up: Entity,
    /// Down neighbour.
    #[argus(doc = "Where focus goes down; null: the nearest below")]
    pub down: Entity,
    /// Left neighbour.
    #[argus(doc = "Where focus goes left; null: the nearest to the left")]
    pub left: Entity,
    /// Right neighbour.
    #[argus(doc = "Where focus goes right; null: the nearest to the right")]
    pub right: Entity,
}

/// A box the player types into (issue 23).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A box the player types into, on a UI node with a UiButton (to be focused and pressed) and a UiText (which shows it): pressing it starts typing, Enter sends event and ui.submitted {entity, text}, Escape, the cancel action, or a press elsewhere stops. While it types, the keyboard moves no action (Actions.typing)"
)]
pub struct UiTextBox {
    /// What is in it.
    #[argus(doc = "What is in it (written as the player types; a script may set it)")]
    pub text: StrId,
    /// Shown while empty.
    #[argus(doc = "Shown while it is empty and not typing")]
    pub hint: StrId,
    /// The most characters.
    #[argus(doc = "The most characters it holds", range(1, 4096))]
    pub max: u32,
    /// Event on Enter.
    #[argus(
        doc = "An event emitted with {entity, text} when Enter sends the text, beside ui.submitted"
    )]
    pub event: StrId,
    /// Empty it after.
    #[argus(doc = "Empty it once the text is sent")]
    pub clear: bool,
    /// Keep typing after.
    #[argus(doc = "Keep typing after Enter: an input line that stays open")]
    pub keep: bool,
    /// Typing now.
    #[argus(doc = "Typing now (written by ui.input; a script sets it to start typing)")]
    pub typing: bool,
}

impl Default for UiTextBox {
    fn default() -> Self {
        UiTextBox {
            text: StrId::EMPTY,
            hint: StrId::EMPTY,
            max: 200,
            event: StrId::EMPTY,
            clear: true,
            keep: false,
            typing: false,
        }
    }
}

/// A node whose content scrolls (issue 23).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A UI node whose children are laid out at their own sizes and scrolled: what is outside its padding is cut off, and the padding stays put. Give it a size (size, fill, or grow), or a max for one that grows with its content up to it and then scrolls. With end, a node scrolled to the end stays at the end as content is added, as a log keeps its newest line in view; to brings a child into view; the mouse wheel over it scrolls it (the tree's scroll action)"
)]
pub struct UiScroll {
    /// How far it is scrolled.
    #[argus(
        doc = "How far it is scrolled, in canvas pixels from the top (a script may set it); the layout keeps it between 0 and content - view"
    )]
    pub y: f32,
    /// Stay at the end.
    #[argus(doc = "Scrolled to the end, it stays at the end as content is added")]
    pub end: bool,
    /// One step of the wheel.
    #[argus(doc = "Pixels one notch of the wheel scrolls", range(1, 1000))]
    pub step: f32,
    /// The content's height.
    #[argus(doc = "How tall its content is (written by ui.layout)")]
    pub content: f32,
    /// The view's height.
    #[argus(doc = "How tall the part shown is (written by ui.layout)")]
    pub view: f32,
    /// At the end.
    #[argus(doc = "Whether the end is in view (written by ui.layout)")]
    pub at_end: bool,
    /// Where layout left it.
    #[argus(doc = "The y ui.layout left it at; a y set since is honoured (written by ui.layout)")]
    pub seen: f32,
    /// A child to bring into view (issue 32).
    #[argus(
        doc = "A child to bring into view at the next layout: all of it if it fits (the least scroll that shows it), else its top at the view's top; the layout sets it back to none. A line written this tick is laid out this tick, so a script can set it as it adds the line"
    )]
    pub to: argus_ecs::Entity,
}

impl Default for UiScroll {
    fn default() -> Self {
        UiScroll {
            y: 0.0,
            end: true,
            step: 40.0,
            content: 0.0,
            view: 0.0,
            at_end: true,
            seen: 0.0,
            to: argus_ecs::Entity::NULL,
        }
    }
}
