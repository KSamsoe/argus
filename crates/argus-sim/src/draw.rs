//! Debug drawing (2D staples item 1): shapes a system draws in world units
//! (`q:draw_line` and the rest), kept for the frames until that system runs
//! again, so a turn-based game's shapes stay up between turns. The window,
//! the editor, and a picture show them over the world. They are not part
//! of the world: no digest, keyframe, replay, or save holds them, and a
//! host that leaves them off (a release build) keeps none, so a draw there
//! costs one check.

use std::collections::BTreeMap;

/// The most shapes one system keeps from a run; past it, its draws are
/// counted, not kept.
pub const MAX_SHAPES: usize = 4096;

/// What is drawn, in world units.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A line, with a head at `to` when `arrow`.
    Line {
        /// Where it starts.
        from: [f32; 2],
        /// Where it ends.
        to: [f32; 2],
        /// A head at `to`.
        arrow: bool,
    },
    /// A box.
    Box {
        /// Its centre.
        at: [f32; 2],
        /// Its whole width and height.
        size: [f32; 2],
        /// Filled, not outlined.
        fill: bool,
    },
    /// A circle.
    Circle {
        /// Its centre.
        at: [f32; 2],
        /// Its radius.
        radius: f32,
        /// Filled, not outlined.
        fill: bool,
    },
    /// Words centred on a point.
    Text {
        /// The point.
        at: [f32; 2],
        /// The words.
        text: String,
    },
}

/// One shape as a system drew it.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    /// The shape.
    pub shape: Shape,
    /// Its colour, sRGB and alpha, 0 to 255.
    pub color: [u8; 4],
    /// A line's thickness (an outline's too), or text's height, in UI
    /// canvas pixels, whatever the zoom.
    pub size: f32,
}

/// Every system's shapes, by system name.
#[derive(Debug, Default)]
pub struct Draws {
    on: bool,
    by: BTreeMap<String, Vec<Drawn>>,
    dropped: BTreeMap<String, usize>,
}

impl Draws {
    /// Whether shapes are kept: a host turns them on where someone looks
    /// (the daemon's window, editor, and pictures).
    pub fn on(&self) -> bool {
        self.on
    }

    /// Keep shapes, or stop and forget them.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.clear();
        }
    }

    /// `system` is about to run: what it drew last time goes.
    pub fn begin(&mut self, system: &str) {
        if !self.on {
            return;
        }
        if let Some(v) = self.by.get_mut(system) {
            v.clear();
        }
        self.dropped.remove(system);
    }

    /// `system` draws a shape; false when it is not kept (off, or past
    /// [`MAX_SHAPES`]).
    pub fn push(&mut self, system: &str, d: Drawn) -> bool {
        if !self.on {
            return false;
        }
        if !self.by.contains_key(system) {
            self.by.insert(system.to_string(), Vec::new());
        }
        let v = self.by.get_mut(system).expect("inserted");
        if v.len() >= MAX_SHAPES {
            *self.dropped.entry(system.to_string()).or_insert(0) += 1;
            return false;
        }
        v.push(d);
        true
    }

    /// Every shape, by the system that drew it, systems by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Drawn)> {
        self.by
            .iter()
            .flat_map(|(s, v)| v.iter().map(move |d| (s.as_str(), d)))
    }

    /// How many shapes there are.
    pub fn len(&self) -> usize {
        self.by.values().map(Vec::len).sum()
    }

    /// No shapes.
    pub fn is_empty(&self) -> bool {
        self.by.values().all(Vec::is_empty)
    }

    /// Shapes not kept for being past [`MAX_SHAPES`], by system.
    pub fn dropped(&self) -> &BTreeMap<String, usize> {
        &self.dropped
    }

    /// Forget `system`'s shapes: it was declared again, or is gone.
    pub fn forget(&mut self, system: &str) {
        self.by.remove(system);
        self.dropped.remove(system);
    }

    /// Forget every shape.
    pub fn clear(&mut self) {
        self.by.clear();
        self.dropped.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x: f32) -> Drawn {
        Drawn {
            shape: Shape::Line {
                from: [0.0, 0.0],
                to: [x, 0.0],
                arrow: false,
            },
            color: [255, 0, 0, 255],
            size: 2.0,
        }
    }

    #[test]
    fn a_systems_shapes_last_until_it_runs_again() {
        let mut d = Draws::default();
        d.set_on(true);
        d.begin("a");
        d.push("a", line(1.0));
        d.begin("b");
        d.push("b", line(2.0));
        assert_eq!(d.len(), 2);
        // a runs again and draws nothing: its line goes, b's stays.
        d.begin("a");
        assert_eq!(d.len(), 1);
        assert_eq!(d.iter().next().unwrap().0, "b");
    }

    #[test]
    fn off_keeps_nothing_and_a_system_is_capped() {
        let mut d = Draws::default();
        assert!(!d.push("a", line(1.0)));
        assert!(d.is_empty());
        d.set_on(true);
        for _ in 0..MAX_SHAPES + 3 {
            d.push("a", line(1.0));
        }
        assert_eq!(d.len(), MAX_SHAPES);
        assert_eq!(d.dropped()["a"], 3);
        d.set_on(false);
        assert!(d.is_empty());
    }
}
