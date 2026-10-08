//! Argus UI, phase 3 item 1 (§12 "UI and text"): "a retained UI tree built
//! from the same entities and hierarchy as everything else, with a
//! flexbox-style layout system, bitmap and SDF text with shaping,
//! nine-slice panels, and input focus. Not a separate world: a menu is a
//! query away like anything else, and the semantic frame reads it
//! directly."
//!
//! A UI node is an entity with a [`UiNode`]; its children are entities with
//! a `Parent` that is a node. `ui.layout` (late stage) lays the trees out on
//! the canvas of `ui.toml` ([`Theme`]) and writes each node's [`UiRect`];
//! `ui.input` (sim stage, first) moves focus between [`UiButton`]s with a
//! player's actions and a pointer, and presses them, as events. What a node
//! shows is a [`UiPanel`] (a colour, an image, nine-sliced) and a
//! [`UiText`]; [`draw::draw_list`] turns the trees into quads for the
//! renderer, which draws them over the world and its post passes.

#![warn(missing_docs)]

pub mod components;
pub mod draw;
pub mod input;
pub mod layout;
pub mod strings;
pub mod text;
pub mod theme;

pub use components::{UiButton, UiNode, UiPanel, UiRect, UiRoot, UiText};
pub use input::INPUT;
pub use layout::{world_to_canvas, LAYOUT};
pub use strings::Strings;

/// Error codes this crate adds (57xx).
pub mod codes {
    /// A string table that does not read, or a key that cannot be written.
    pub const BAD_STRINGS: u32 = 5701;
}
pub use text::{FontFace, PixelFont, SdfFont, SdfGlyph, PIXEL_FONT};
pub use theme::{parse_color, Rgba, Theme};

use argus_ecs::{Component, Result, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A player's UI settings (§12 accessibility): "text scaling and
/// high-contrast palettes are UI theme tokens", and whether the window is
/// fullscreen. The first one in the world is in effect; with none, the
/// theme's. A game's settings screen writes it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A player's UI settings, the first one in the world in effect: every text size times text_scale, and the theme's high-contrast colours when high_contrast (section 12 accessibility)"
)]
pub struct UiSettings {
    /// Text scale.
    #[argus(doc = "Every text size times this", range(0.5, 4), tune)]
    pub text_scale: f32,
    /// High contrast.
    #[argus(doc = "Use the theme's high-contrast colours")]
    pub high_contrast: bool,
    /// Fullscreen.
    #[argus(
        doc = "The game's window fills the screen: the release host (desktop or web page) reads it each frame and applies it"
    )]
    pub fullscreen: bool,
    /// Language.
    #[argus(
        doc = "The language player-facing text is in: a string table's code (strings/<code>.toml); empty: the game's base language (argus.toml [strings] base)"
    )]
    pub language: argus_ecs::StrId,
}

impl Default for UiSettings {
    fn default() -> Self {
        UiSettings {
            text_scale: 1.0,
            high_contrast: false,
            fullscreen: false,
            language: argus_ecs::StrId::EMPTY,
        }
    }
}

/// The entity whose [`UiSettings`] the UI reads: the first one.
pub fn settings_entity(world: &World) -> Option<argus_ecs::Entity> {
    let id = world.component_id_of::<UiSettings>()?;
    let mut all = world.query().has(id).entities();
    all.sort();
    all.first().copied()
}

/// The [`UiSettings`] the UI reads, if the game has any (a settings page).
pub fn settings(world: &World) -> Option<UiSettings> {
    world.get::<UiSettings>(settings_entity(world)?).ok()
}

/// Whether the world's settings ask for fullscreen (the first
/// [`UiSettings`], as the rest of the UI reads them): what a release host
/// applies to its window each frame.
pub fn wants_fullscreen(world: &World) -> bool {
    settings(world).is_some_and(|s| s.fullscreen)
}

/// The fonts a project has, by asset name, and the built-in pixel font.
pub struct Fonts {
    pixel: Rc<dyn FontFace>,
    faces: HashMap<String, Rc<dyn FontFace>>,
    generation: u64,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts {
            pixel: Rc::new(PixelFont::default()),
            faces: HashMap::new(),
            generation: 0,
        }
    }
}

impl Fonts {
    /// Add (or replace) a font by its asset name.
    pub fn add(&mut self, name: &str, face: Rc<dyn FontFace>) {
        self.faces.insert(name.to_string(), face);
        self.generation += 1;
    }

    /// Drop the fonts `keep` says no to (a font asset gone).
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        let n = self.faces.len();
        self.faces.retain(|k, _| keep(k));
        if self.faces.len() != n {
            self.generation += 1;
        }
    }

    /// Changes with every font added, replaced, or dropped: what the layout
    /// measured with is stale when it moves.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The font `name`, else `fallback`, else the built-in pixel font.
    pub fn face(&self, name: &str, fallback: &str) -> Rc<dyn FontFace> {
        self.faces
            .get(name)
            .or_else(|| self.faces.get(fallback))
            .cloned()
            .unwrap_or_else(|| self.pixel.clone())
    }

    /// The names of the fonts it has.
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.faces.keys().map(String::as_str).collect();
        v.sort();
        v
    }
}

/// What the UI systems and the renderer share: the theme, the fonts, and
/// the last layout's problems (holes that named nothing).
#[derive(Default)]
pub struct Shared {
    /// `ui.toml`.
    pub theme: Theme,
    /// Fonts.
    pub fonts: Fonts,
    /// The last layout's problems.
    pub problems: RefCell<Vec<String>>,
    /// The project's string tables (phase 3 item 4): what a `UiText.key`
    /// and a script's `q:text` read.
    pub strings: Strings,
}

/// How the canvas sits in a window: scaled to fit whole, centred. The
/// renderer draws the UI through it and a host turns the mouse into canvas
/// pixels through it, so a click lands where the button is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// Window pixels per canvas pixel.
    pub scale: f32,
    /// Where the canvas's top left is in the window.
    pub offset: [f32; 2],
    /// The canvas size.
    pub canvas: [f32; 2],
}

impl Fit {
    /// The canvas `canvas` in a window `window` pixels big.
    pub fn new(canvas: [f32; 2], window: [f32; 2]) -> Fit {
        let scale = (window[0] / canvas[0]).min(window[1] / canvas[1]).max(1e-6);
        Fit {
            scale,
            offset: [
                (window[0] - canvas[0] * scale) * 0.5,
                (window[1] - canvas[1] * scale) * 0.5,
            ],
            canvas,
        }
    }

    /// A window point on the canvas.
    pub fn to_canvas(&self, p: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - self.offset[0]) / self.scale,
            (p[1] - self.offset[1]) / self.scale,
        ]
    }

    /// A canvas point in the window.
    pub fn to_window(&self, p: [f32; 2]) -> [f32; 2] {
        [
            p[0] * self.scale + self.offset[0],
            p[1] * self.scale + self.offset[1],
        ]
    }
}

/// Register the UI's components (for a world that only reads them).
pub fn register(world: &mut World) -> Result<()> {
    argus_ecs::hierarchy::register(world).ok();
    world.register::<UiRoot>()?;
    world.register::<UiNode>()?;
    world.register::<UiRect>()?;
    world.register::<UiText>()?;
    world.register::<UiPanel>()?;
    world.register::<UiButton>()?;
    world.register::<UiSettings>()?;
    Ok(())
}

/// Register the components and add `ui.input` (sim stage, first) and
/// `ui.layout` (late stage, after the camera). Call after the input map is
/// installed, so `ui.input` can read the players' `Actions`.
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    shared: Rc<RefCell<Shared>>,
) -> Result<()> {
    register(world)?;
    input::install(world, sched, shared.clone())?;
    layout::install(world, sched, shared)
}
