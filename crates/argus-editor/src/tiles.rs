//! Tilemap painting (§12: tiles are "editable through the protocol at the
//! rect level, so an agent can carve a room without touching pixels and a
//! human can paint in the editor and both land in the same chunk"; phase 4
//! item 7). The Tiles tab picks a map placed in the scene
//! (`scene.tiles.list`), a layer, a tool (pencil, rectangle, pick) and a
//! brush (a tile of the layer's tileset, one of its auto-tiling terrains,
//! or the eraser); the pointer paints in the viewport, the stroke shows at
//! once over the game, and letting go is one transaction by the person
//! (`edit.tiles` ops, one undo, one row in the change feed). The grid, the
//! map's outline, and what bodies hit (`scene.tiles.collision`) can show
//! over the game; a new map can be made in any scene.

use crate::viewport::Paint;
use argus_client::frames::FrameInfo;
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Stroke, TextureHandle, Vec2};
use serde_json::{json, Value as Json};
use std::collections::{BTreeSet, HashMap};

/// A tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Paint cell by cell as the pointer moves.
    Pencil,
    /// Fill the rectangle dragged out.
    Rect,
    /// Take the tile under the pointer as the brush.
    Pick,
}

/// What the tools lay down.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    /// A tile id: frame id - 1 of the layer's tileset.
    Tile(u64),
    /// An auto-tiling terrain of the layer's tileset.
    Terrain(String),
    /// Clear.
    Erase,
}

/// What the panel asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum TileDo {
    /// A stroke is over: one transaction of these ops, with this note.
    Edit(String, Vec<Json>),
    /// Read this cell for the pick tool (`scene.tiles.get`).
    Pick(Json),
    /// Make a tilemap (`scene.tiles.create`), then place it (a spawn).
    Create(Json, String, Vec<Json>),
}

/// A point on screen and in the world, for one frame.
#[derive(Clone, Copy, Debug)]
pub struct Mapping {
    img: Rect,
    size: [f32; 2],
    center: [f32; 2],
    half: [f32; 2],
    angle: f32,
}

impl Mapping {
    /// From where the frame is drawn, its size, and its view.
    pub fn new(img: Rect, info: FrameInfo, view: &Json) -> Option<Mapping> {
        let v2 = |v: &Json| Some([v[0].as_f64()? as f32, v[1].as_f64()? as f32]);
        Some(Mapping {
            img,
            size: [info.width as f32, info.height as f32],
            center: v2(&view["center"])?,
            half: v2(&view["half"])?,
            angle: view["angle"].as_f64().unwrap_or(0.0) as f32,
        })
    }

    /// A frame pixel (y down) in the world (y up).
    pub fn world_of_px(&self, px: [f32; 2]) -> [f32; 2] {
        let vx = (px[0] / self.size[0] - 0.5) * 2.0 * self.half[0];
        let vy = (0.5 - px[1] / self.size[1]) * 2.0 * self.half[1];
        let (s, c) = self.angle.sin_cos();
        [
            self.center[0] + vx * c - vy * s,
            self.center[1] + vx * s + vy * c,
        ]
    }

    /// A place in the world on screen, in points.
    pub fn screen_of_world(&self, w: [f32; 2]) -> Pos2 {
        let (dx, dy) = (w[0] - self.center[0], w[1] - self.center[1]);
        let (s, c) = self.angle.sin_cos();
        let (vx, vy) = (dx * c + dy * s, -dx * s + dy * c);
        let px = vx / (2.0 * self.half[0]) * self.size[0] + self.size[0] / 2.0;
        let py = self.size[1] / 2.0 - vy / (2.0 * self.half[1]) * self.size[1];
        Pos2::new(
            self.img.min.x + px / self.size[0] * self.img.width(),
            self.img.min.y + py / self.size[1] * self.img.height(),
        )
    }
}

/// A map's size in cells, its tile size, and where tile (0, 0)'s top-left
/// corner is.
fn geometry(map: &Json) -> Option<(i64, i64, f32, [f32; 2])> {
    Some((
        map["width"].as_i64()?,
        map["height"].as_i64()?,
        map["tile_size"].as_f64()? as f32,
        [
            map["origin"][0].as_f64()? as f32,
            map["origin"][1].as_f64()? as f32,
        ],
    ))
}

/// The cell a world point is in, if it is on the map.
pub fn cell_of(map: &Json, w: [f32; 2]) -> Option<[i64; 2]> {
    let (width, height, ts, o) = geometry(map)?;
    let x = ((w[0] - o[0]) / ts).floor() as i64;
    let y = ((o[1] - w[1]) / ts).floor() as i64;
    (x >= 0 && y >= 0 && x < width && y < height).then_some([x, y])
}

/// A cell's corners in the world: top-left and bottom-right.
pub fn cell_world(map: &Json, c: [i64; 2]) -> Option<([f32; 2], [f32; 2])> {
    let (_, _, ts, o) = geometry(map)?;
    let x0 = o[0] + c[0] as f32 * ts;
    let y0 = o[1] - c[1] as f32 * ts;
    Some(([x0, y0], [x0 + ts, y0 - ts]))
}

/// Cells in rows of consecutive runs: (x, y, w), row by row.
pub fn runs(cells: &BTreeSet<(i64, i64)>) -> Vec<(i64, i64, i64)> {
    let mut out: Vec<(i64, i64, i64)> = Vec::new();
    for &(y, x) in cells {
        match out.last_mut() {
            Some((lx, ly, lw)) if *ly == y && *lx + *lw == x => *lw += 1,
            _ => out.push((x, y, 1)),
        }
    }
    out
}

/// One `edit.tiles` op for a rect with a brush.
pub fn op(file: &str, layer: &str, rect: [i64; 4], brush: &Brush) -> Json {
    let mut o = json!({
        "op": "tiles", "file": file, "layer": layer,
        "rect": { "x": rect[0], "y": rect[1], "w": rect[2], "h": rect[3] },
    });
    match brush {
        Brush::Tile(t) => o["tile"] = json!(t),
        Brush::Terrain(t) => o["terrain"] = json!(t),
        Brush::Erase => o["erase"] = json!(true),
    }
    o
}

/// A tileset's frames, cut from its texture.
struct Tileset {
    images: Vec<ColorImage>,
    textures: Vec<Option<TextureHandle>>,
    props: Vec<Json>,
}

/// A new map's settings.
pub struct NewMap {
    /// Project-relative `*.tilemap.toml`.
    pub file: String,
    /// Size in cells.
    pub width: u32,
    pub height: u32,
    /// World units a tile.
    pub tile_size: f32,
    /// Its one layer's name, tileset, and whether bodies hit it.
    pub layer: String,
    pub tileset: String,
    pub collision: bool,
    /// The room it goes in, in a room scene.
    pub room: String,
}

impl Default for NewMap {
    fn default() -> NewMap {
        NewMap {
            file: String::new(),
            width: 32,
            height: 16,
            tile_size: 1.0,
            layer: "ground".into(),
            tileset: String::new(),
            collision: true,
            room: String::new(),
        }
    }
}

/// The panel's state.
pub struct Tiles {
    /// `scene.tiles.list`'s rows.
    pub maps: Vec<Json>,
    /// The list has come at least once.
    pub loaded: bool,
    /// The map painted: its file.
    pub map: Option<String>,
    /// The layer painted.
    pub layer: Option<String>,
    pub tool: Tool,
    pub brush: Brush,
    /// Draw the cells' grid over the game.
    pub show_grid: bool,
    /// Draw what bodies hit over the game.
    pub show_collision: bool,
    /// `scene.tiles.collision` for what is on screen.
    pub collision: Json,
    /// The cell under the pointer, and what is in it.
    pub hover: Option<[i64; 2]>,
    pub hover_tile: Option<u64>,
    /// The stroke in progress (pencil cells, or the rectangle's corners),
    /// drawn over the game until its transaction lands.
    pub stroke: BTreeSet<(i64, i64)>,
    start: Option<[i64; 2]>,
    end: Option<[i64; 2]>,
    /// The `argus edit` line of the stroke whose commit is on its way.
    pub pending: Option<String>,
    pending_cells: BTreeSet<(i64, i64)>,
    pending_brush: Option<Brush>,
    /// Sprite assets that can be tilesets (`asset.list`).
    pub sprites: Vec<String>,
    /// Settings for a new map.
    pub new_map: NewMap,
    tilesets: HashMap<String, Tileset>,
    asked: BTreeSet<String>,
}

impl Default for Tiles {
    fn default() -> Tiles {
        Tiles {
            maps: Vec::new(),
            loaded: false,
            map: None,
            layer: None,
            tool: Tool::Pencil,
            brush: Brush::Tile(1),
            show_grid: true,
            show_collision: false,
            collision: Json::Null,
            hover: None,
            hover_tile: None,
            stroke: BTreeSet::new(),
            start: None,
            end: None,
            pending: None,
            pending_cells: BTreeSet::new(),
            pending_brush: None,
            sprites: Vec::new(),
            new_map: NewMap::default(),
            tilesets: HashMap::new(),
            asked: BTreeSet::new(),
        }
    }
}

impl Tiles {
    /// `scene.tiles.list`'s answer.
    pub fn take_maps(&mut self, v: &Json) {
        self.maps = v["rows"].as_array().cloned().unwrap_or_default();
        self.loaded = true;
        if self
            .map
            .as_ref()
            .is_none_or(|m| !self.maps.iter().any(|r| r["file"] == m.as_str()))
        {
            self.map = self
                .maps
                .first()
                .and_then(|r| r["file"].as_str().map(str::to_string));
            self.layer = None;
        }
        if let Some(m) = self.current().cloned() {
            let layers: Vec<String> = m["layers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l["name"].as_str().map(str::to_string))
                .collect();
            if self.layer.as_ref().is_none_or(|l| !layers.contains(l)) {
                self.layer = layers.first().cloned();
            }
        }
    }

    /// The map painted.
    pub fn current(&self) -> Option<&Json> {
        let m = self.map.as_deref()?;
        self.maps.iter().find(|r| r["file"] == m)
    }

    /// The layer painted, as the map's index has it.
    pub fn current_layer(&self) -> Option<&Json> {
        let l = self.layer.as_deref()?;
        self.current()?["layers"]
            .as_array()?
            .iter()
            .find(|x| x["name"] == l)
    }

    /// Whether the pointer paints: a map and a layer are chosen.
    pub fn ready(&self) -> bool {
        self.current_layer().is_some()
    }

    /// Tilesets whose frames the palette still needs (`asset.get`).
    pub fn wanted_tilesets(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for m in &self.maps {
            for l in m["layers"].as_array().into_iter().flatten() {
                if let Some(ts) = l["tileset"].as_str() {
                    if !self.asked.contains(ts) {
                        out.push(ts.to_string());
                    }
                }
            }
        }
        for t in &out {
            self.asked.insert(t.clone());
        }
        out
    }

    /// `asset.get`'s answer for a tileset: its frames, cut from the texture
    /// the daemon imported (the editor runs beside it and reads the file).
    pub fn take_tileset(&mut self, name: &str, v: &Json) {
        let frames: Vec<&Json> = v["frames"].as_array().into_iter().flatten().collect();
        let sheet = v["texture"]
            .as_str()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| crate::agent::decode(&b).ok())
            .and_then(|mut f| (!f.is_empty()).then(|| f.swap_remove(0)));
        let mut images = Vec::new();
        let mut props = Vec::new();
        for f in &frames {
            let r: Vec<usize> = f["rect"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_u64().map(|x| x as usize))
                .collect();
            let img = match (&sheet, r.as_slice()) {
                (Some(s), [x, y, w, h]) if x + w <= s.size[0] && y + h <= s.size[1] => {
                    let mut out = ColorImage::new([*w, *h], Color32::TRANSPARENT);
                    for yy in 0..*h {
                        for xx in 0..*w {
                            out.pixels[yy * w + xx] = s.pixels[(y + yy) * s.size[0] + x + xx];
                        }
                    }
                    out
                }
                _ => ColorImage::new([1, 1], Color32::from_gray(128)),
            };
            images.push(img);
            props.push(f["properties"].clone());
        }
        let n = images.len();
        self.tilesets.insert(
            name.to_string(),
            Tileset {
                images,
                textures: vec![None; n],
                props,
            },
        );
    }

    /// Frames of a tileset, once read.
    pub fn tileset_frames(&self, name: &str) -> usize {
        self.tilesets.get(name).map_or(0, |t| t.images.len())
    }

    fn texture(&mut self, ctx: &egui::Context, ts: &str, i: usize) -> Option<egui::TextureId> {
        let t = self.tilesets.get_mut(ts)?;
        let img = t.images.get(i)?.clone();
        let tex = t.textures.get_mut(i)?;
        let tex = tex.get_or_insert_with(|| {
            ctx.load_texture(format!("tile {ts} {i}"), img, egui::TextureOptions::NEAREST)
        });
        Some(tex.id())
    }

    /// The cell a frame pixel is in, on the map painted.
    pub fn cell_at(&self, m: &Mapping, px: [f32; 2]) -> Option<[i64; 2]> {
        cell_of(self.current()?, m.world_of_px(px))
    }

    /// Where a cell's centre is on screen (for a test's pointer, and the
    /// overlay).
    pub fn screen_of_cell(&self, m: &Mapping, c: [i64; 2]) -> Option<Pos2> {
        let (a, b) = cell_world(self.current()?, c)?;
        Some(m.screen_of_world([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]))
    }

    /// The pointer in the viewport, painting.
    pub fn on_paint(&mut self, p: Paint, m: &Mapping) -> Vec<TileDo> {
        let mut out = Vec::new();
        let (Some(map), Some(layer)) = (self.current().cloned(), self.layer.clone()) else {
            return out;
        };
        let file = map["file"].as_str().unwrap_or("").to_string();
        match p {
            Paint::Hover(px) => {
                let c = self.cell_at(m, px);
                if c != self.hover {
                    self.hover = c;
                    self.hover_tile = None;
                }
            }
            Paint::Down(px) => {
                let c = self.cell_at(m, px);
                self.hover = c;
                let Some(c) = c else { return out };
                match self.tool {
                    Tool::Pick => out.push(TileDo::Pick(json!({
                        "file": file, "layer": layer, "rect": { "x": c[0], "y": c[1], "w": 1, "h": 1 },
                    }))),
                    Tool::Pencil => {
                        self.stroke.clear();
                        self.stroke.insert((c[1], c[0]));
                        self.start = Some(c);
                    }
                    Tool::Rect => {
                        self.start = Some(c);
                        self.end = Some(c);
                    }
                }
            }
            Paint::Move(px) => {
                let Some(c) = self.cell_at(m, px) else {
                    return out;
                };
                self.hover = Some(c);
                match self.tool {
                    Tool::Pencil if self.start.is_some() => {
                        // Every cell on the way from the last, so a quick
                        // stroke leaves no gaps.
                        let last = self.end.or(self.start).unwrap_or(c);
                        let n = (c[0] - last[0]).abs().max((c[1] - last[1]).abs()).max(1);
                        for i in 1..=n {
                            let x = last[0] + (c[0] - last[0]) * i / n;
                            let y = last[1] + (c[1] - last[1]) * i / n;
                            self.stroke.insert((y, x));
                        }
                        self.end = Some(c);
                    }
                    Tool::Rect if self.start.is_some() => self.end = Some(c),
                    _ => {}
                }
            }
            Paint::Up(px) => {
                if let Some(c) = self.cell_at(m, px) {
                    if self.tool == Tool::Rect && self.start.is_some() {
                        self.end = Some(c);
                    }
                }
                let what = match &self.brush {
                    Brush::Tile(t) => format!("tile {t}"),
                    Brush::Terrain(t) => format!("terrain {t}"),
                    Brush::Erase => "the eraser".to_string(),
                };
                let (ops, cells): (Vec<Json>, BTreeSet<(i64, i64)>) = match self.tool {
                    Tool::Pencil if self.start.is_some() => (
                        runs(&self.stroke)
                            .into_iter()
                            .map(|(x, y, w)| op(&file, &layer, [x, y, w, 1], &self.brush))
                            .collect(),
                        std::mem::take(&mut self.stroke),
                    ),
                    Tool::Rect => match (self.start, self.end) {
                        (Some(a), Some(b)) => {
                            let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
                            let (w, h) = ((a[0] - b[0]).abs() + 1, (a[1] - b[1]).abs() + 1);
                            let mut cells = BTreeSet::new();
                            for y in y0..y0 + h {
                                for x in x0..x0 + w {
                                    cells.insert((y, x));
                                }
                            }
                            (vec![op(&file, &layer, [x0, y0, w, h], &self.brush)], cells)
                        }
                        _ => (Vec::new(), BTreeSet::new()),
                    },
                    _ => (Vec::new(), BTreeSet::new()),
                };
                self.start = None;
                self.end = None;
                self.stroke.clear();
                if !ops.is_empty() {
                    let note = format!("painted {} cells of {layer} with {what}", cells.len());
                    self.pending_cells = cells;
                    self.pending_brush = Some(self.brush.clone());
                    out.push(TileDo::Edit(note, ops));
                }
            }
        }
        out
    }

    /// The stroke's transaction was sent as this line.
    pub fn sent(&mut self, line: String) {
        self.pending = Some(line);
    }

    /// A transaction landed: if it was the stroke's, the game draws it now.
    pub fn landed(&mut self, line: &str) {
        if self.pending.as_deref() == Some(line) {
            self.pending = None;
            self.pending_cells.clear();
            self.pending_brush = None;
        }
    }

    /// The pick tool's answer: the tile in the cell is the brush.
    pub fn take_pick(&mut self, v: &Json) {
        if let Some(t) = v["cells"][0].as_u64() {
            self.brush = if t == 0 { Brush::Erase } else { Brush::Tile(t) };
            self.tool = Tool::Pencil;
        }
    }

    /// The cells the screen shows of the map painted, clipped to it.
    pub fn visible_rect(&self, m: &Mapping) -> Option<[i64; 4]> {
        let map = self.current()?;
        let (w, h, ts, o) = geometry(map)?;
        let corners = [
            m.world_of_px([0.0, 0.0]),
            m.world_of_px([m.size[0], 0.0]),
            m.world_of_px([0.0, m.size[1]]),
            m.world_of_px(m.size),
        ];
        let xs = corners.map(|c| ((c[0] - o[0]) / ts).floor() as i64);
        let ys = corners.map(|c| ((o[1] - c[1]) / ts).floor() as i64);
        let (x0, x1) = (
            xs.iter().min()?.max(&0).to_owned(),
            (xs.iter().max()? + 1).min(w),
        );
        let (y0, y1) = (
            ys.iter().min()?.max(&0).to_owned(),
            (ys.iter().max()? + 1).min(h),
        );
        (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
    }

    /// Over the game: the map's outline, its grid, what bodies hit, the
    /// stroke, and the cell under the pointer.
    pub fn overlay(&mut self, ui: &egui::Ui, m: &Mapping) {
        let Some(map) = self.current().cloned() else {
            return;
        };
        let p = ui.painter_at(m.img);
        let cell_rect = |c: [i64; 2]| -> Option<Rect> {
            let (a, b) = cell_world(&map, c)?;
            Some(Rect::from_two_pos(
                m.screen_of_world(a),
                m.screen_of_world(b),
            ))
        };
        let (w, h, _, _) = geometry(&map).unwrap_or((0, 0, 1.0, [0.0, 0.0]));
        if let (Some(a), Some(b)) = (cell_rect([0, 0]), cell_rect([w - 1, h - 1])) {
            p.rect_stroke(
                a.union(b),
                0.0,
                Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(120, 190, 255, 200)),
                egui::StrokeKind::Middle,
            );
        }
        let vis = self.visible_rect(m);
        if let (true, Some(r), Some(c0)) = (self.show_grid, vis, cell_rect([0, 0])) {
            if c0.width() >= 6.0 {
                let line = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
                for x in r[0]..=r[0] + r[2] {
                    if let (Some(a), Some(b)) =
                        (cell_rect([x, r[1]]), cell_rect([x, r[1] + r[3] - 1]))
                    {
                        p.line_segment([a.left_top(), b.left_bottom()], line);
                    }
                }
                for y in r[1]..=r[1] + r[3] {
                    if let (Some(a), Some(b)) =
                        (cell_rect([r[0], y]), cell_rect([r[0] + r[2] - 1, y]))
                    {
                        p.line_segment([a.left_top(), b.right_top()], line);
                    }
                }
            }
        }
        if self.show_collision {
            if let Some(rows) = self.collision["rows"].as_array() {
                let (x0, y0) = (
                    self.collision["rect"]["x"].as_i64().unwrap_or(0),
                    self.collision["rect"]["y"].as_i64().unwrap_or(0),
                );
                for (dy, row) in rows.iter().enumerate() {
                    for (dx, ch) in row.as_str().unwrap_or("").chars().enumerate() {
                        let Some(r) = cell_rect([x0 + dx as i64, y0 + dy as i64]) else {
                            continue;
                        };
                        match ch {
                            '#' => {
                                p.rect_filled(
                                    r,
                                    0.0,
                                    Color32::from_rgba_unmultiplied(235, 80, 70, 70),
                                );
                            }
                            '_' => {
                                p.rect_filled(
                                    Rect::from_min_max(Pos2::new(r.left(), r.center().y), r.max),
                                    0.0,
                                    Color32::from_rgba_unmultiplied(235, 80, 70, 70),
                                );
                            }
                            '-' => {
                                p.line_segment(
                                    [r.left_top(), r.right_top()],
                                    Stroke::new(2.0_f32, Color32::from_rgb(240, 200, 60)),
                                );
                            }
                            '/' | '\\' => {
                                let pts = if ch == '/' {
                                    vec![r.left_bottom(), r.right_bottom(), r.right_top()]
                                } else {
                                    vec![r.left_bottom(), r.right_bottom(), r.left_top()]
                                };
                                p.add(egui::Shape::convex_polygon(
                                    pts,
                                    Color32::from_rgba_unmultiplied(240, 150, 60, 80),
                                    Stroke::NONE,
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // The stroke, before and while its transaction goes: the brush's
        // tile where it will be.
        let brush = self
            .pending_brush
            .clone()
            .unwrap_or_else(|| self.brush.clone());
        let mut cells: Vec<(i64, i64)> = self.pending_cells.iter().copied().collect();
        cells.extend(self.stroke.iter().copied());
        if let (Tool::Rect, Some(a), Some(b)) = (self.tool, self.start, self.end) {
            for y in a[1].min(b[1])..=a[1].max(b[1]) {
                for x in a[0].min(b[0])..=a[0].max(b[0]) {
                    cells.push((y, x));
                }
            }
        }
        let tileset = self
            .current_layer()
            .and_then(|l| l["tileset"].as_str().map(str::to_string));
        for (y, x) in cells {
            let Some(r) = cell_rect([x, y]) else { continue };
            match &brush {
                Brush::Tile(t) => {
                    let tex = tileset
                        .as_deref()
                        .and_then(|ts| self.texture(ui.ctx(), ts, (*t as usize).saturating_sub(1)));
                    match tex {
                        Some(tex) => {
                            p.image(
                                tex,
                                r,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::from_white_alpha(200),
                            );
                        }
                        None => {
                            p.rect_filled(
                                r,
                                0.0,
                                Color32::from_rgba_unmultiplied(120, 190, 255, 90),
                            );
                        }
                    }
                }
                Brush::Terrain(_) => {
                    p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(110, 200, 120, 110));
                }
                Brush::Erase => {
                    p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
                }
            }
        }
        if let Some(r) = self.hover.and_then(cell_rect) {
            p.rect_stroke(
                r,
                0.0,
                Stroke::new(1.5_f32, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
    }

    /// Draw the tab; what the person asked for.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        scene: &str,
        rooms: &[String],
        view_top_left: Option<[f32; 2]>,
    ) -> Vec<TileDo> {
        let mut out = Vec::new();
        if !self.loaded {
            // Before the list: the new map's section opens only for a scene
            // with none.
            ui.weak("Reading the maps...");
            return out;
        }
        egui::ScrollArea::vertical()
            .id_salt("tiles")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.maps.is_empty() {
                    ui.weak("No tilemap is placed in this scene. Make one below.");
                } else {
                    self.map_ui(ui);
                    ui.separator();
                    self.tools_ui(ui);
                    ui.separator();
                    self.palette_ui(ui);
                    ui.separator();
                    self.status_ui(ui);
                }
                ui.separator();
                self.new_map_ui(ui, scene, rooms, view_top_left, &mut out);
            });
        out
    }

    fn map_ui(&mut self, ui: &mut egui::Ui) {
        let files: Vec<String> = self
            .maps
            .iter()
            .filter_map(|r| r["file"].as_str().map(str::to_string))
            .collect();
        egui::ComboBox::new("tile-map", "Map")
            .selected_text(self.map.clone().unwrap_or_default())
            .show_ui(ui, |ui| {
                for f in files {
                    if ui
                        .selectable_label(self.map.as_deref() == Some(f.as_str()), &f)
                        .clicked()
                    {
                        self.map = Some(f);
                        self.layer = None;
                    }
                }
            });
        if let Some(m) = self.current().cloned() {
            ui.weak(format!(
                "{} x {} cells of {} units, placed by {}",
                m["width"],
                m["height"],
                m["tile_size"],
                m["entity"].as_str().unwrap_or("?")
            ));
            if self.layer.is_none() {
                self.layer = m["layers"][0]["name"].as_str().map(str::to_string);
            }
            ui.label("Layer");
            for l in m["layers"].as_array().into_iter().flatten() {
                let name = l["name"].as_str().unwrap_or("?").to_string();
                let mut text = format!(
                    "{name}  ({})",
                    l["tileset"].as_str().unwrap_or("no tileset")
                );
                if l["collision"] == true {
                    text.push_str(", bodies hit it");
                }
                if ui
                    .selectable_label(self.layer.as_deref() == Some(name.as_str()), text)
                    .clicked()
                {
                    self.layer = Some(name);
                    self.brush = Brush::Tile(1);
                }
            }
        }
    }

    fn tools_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Pencil, "Pencil")
                .on_hover_text("Paint cell by cell (B)");
            ui.selectable_value(&mut self.tool, Tool::Rect, "Rectangle")
                .on_hover_text("Fill the rectangle you drag out (R)");
            ui.selectable_value(&mut self.tool, Tool::Pick, "Pick")
                .on_hover_text("Take the tile under the pointer (I)");
            ui.separator();
            if ui
                .selectable_label(self.brush == Brush::Erase, "Eraser")
                .on_hover_text("Clear cells (E)")
                .clicked()
            {
                self.brush = Brush::Erase;
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_grid, "Grid");
            ui.checkbox(&mut self.show_collision, "What bodies hit");
        });
    }

    fn palette_ui(&mut self, ui: &mut egui::Ui) {
        let Some(l) = self.current_layer().cloned() else {
            return;
        };
        let terrains: Vec<String> = l["terrains"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect();
        if !terrains.is_empty() {
            ui.label("Terrains (auto-tiled)");
            ui.horizontal_wrapped(|ui| {
                for t in terrains {
                    let on = self.brush == Brush::Terrain(t.clone());
                    if ui.selectable_label(on, &t).clicked() {
                        self.brush = Brush::Terrain(t);
                    }
                }
            });
        }
        let Some(ts) = l["tileset"].as_str().map(str::to_string) else {
            ui.weak("This layer has no tileset.");
            return;
        };
        let n = self.tileset_frames(&ts);
        ui.label(format!("Tiles of {ts}"));
        if n == 0 {
            ui.weak("Reading the tileset...");
            return;
        }
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(2.0);
            for i in 0..n {
                let id = i as u64 + 1;
                let Some(tex) = self.texture(&ctx, &ts, i) else {
                    continue;
                };
                let on = self.brush == Brush::Tile(id);
                let props = self
                    .tilesets
                    .get(&ts)
                    .and_then(|t| t.props.get(i))
                    .cloned()
                    .unwrap_or(Json::Null);
                let mut tip = format!("tile {id}");
                if let Some(o) = props.as_object().filter(|o| !o.is_empty()) {
                    let words: Vec<String> = o.iter().map(|(k, v)| format!("{k} = {v}")).collect();
                    tip.push_str(&format!(": {}", words.join(", ")));
                }
                let b = egui::Button::image(egui::Image::new((tex, Vec2::splat(28.0))))
                    .selected(on)
                    .frame(true);
                let resp = ui.add(b).on_hover_text(tip);
                // Named as a person would say it (and a test clicks it).
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        on,
                        format!("tile {id}"),
                    )
                });
                if resp.clicked() {
                    self.brush = Brush::Tile(id);
                    if self.tool == Tool::Pick {
                        self.tool = Tool::Pencil;
                    }
                }
            }
        });
    }

    fn status_ui(&self, ui: &mut egui::Ui) {
        let brush = match &self.brush {
            Brush::Tile(t) => format!("tile {t}"),
            Brush::Terrain(t) => format!("terrain {t}"),
            Brush::Erase => "the eraser".into(),
        };
        ui.label(format!(
            "{} with {brush}",
            match self.tool {
                Tool::Pencil => "Pencil",
                Tool::Rect => "Rectangle",
                Tool::Pick => "Pick",
            }
        ));
        match self.hover {
            Some(c) => {
                let what = match self.hover_tile {
                    Some(0) => ", empty".to_string(),
                    Some(t) => format!(", tile {t}"),
                    None => String::new(),
                };
                ui.weak(format!("cell ({}, {}){what}", c[0], c[1]));
            }
            None => {
                ui.weak("Point at the map in the game to paint. Ctrl+Z takes a stroke back.");
            }
        }
    }

    fn new_map_ui(
        &mut self,
        ui: &mut egui::Ui,
        scene: &str,
        rooms: &[String],
        top_left: Option<[f32; 2]>,
        out: &mut Vec<TileDo>,
    ) {
        egui::CollapsingHeader::new("New tilemap")
            .id_salt("new-map")
            .default_open(self.maps.is_empty())
            .show(ui, |ui| {
                let nm = &mut self.new_map;
                if nm.file.is_empty() {
                    nm.file = format!("scenes/{scene}/paint.tilemap.toml");
                }
                if nm.tileset.is_empty() {
                    if let Some(s) = self.sprites.first() {
                        nm.tileset = s.clone();
                    }
                }
                if !rooms.is_empty() && !rooms.contains(&nm.room) {
                    nm.room = rooms[0].clone();
                }
                ui.horizontal(|ui| {
                    ui.label("File");
                    ui.add(egui::TextEdit::singleline(&mut nm.file).desired_width(230.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Cells");
                    ui.add(egui::DragValue::new(&mut nm.width).range(1..=4096));
                    ui.label("by");
                    ui.add(egui::DragValue::new(&mut nm.height).range(1..=4096));
                    ui.label("of");
                    ui.add(egui::DragValue::new(&mut nm.tile_size).range(0.05..=64.0).speed(0.05));
                    ui.label("units");
                });
                ui.horizontal(|ui| {
                    ui.label("Layer");
                    ui.add(egui::TextEdit::singleline(&mut nm.layer).desired_width(90.0));
                    ui.checkbox(&mut nm.collision, "bodies hit it");
                });
                egui::ComboBox::new("new-map-tileset", "Tileset")
                    .selected_text(nm.tileset.clone())
                    .show_ui(ui, |ui| {
                        for s in &self.sprites {
                            ui.selectable_value(&mut nm.tileset, s.clone(), s);
                        }
                    });
                if !rooms.is_empty() {
                    egui::ComboBox::new("new-map-room", "Room")
                        .selected_text(nm.room.clone())
                        .show_ui(ui, |ui| {
                            for r in rooms {
                                ui.selectable_value(&mut nm.room, r.clone(), r);
                            }
                        });
                }
                let ok = nm.file.ends_with(".tilemap.toml") && !nm.layer.is_empty() && !nm.tileset.is_empty();
                if ui
                    .add_enabled(ok, egui::Button::new("Create, its corner at the view's top left"))
                    .clicked()
                {
                    let corner = top_left.unwrap_or([0.0, 0.0]);
                    let snap = |v: f32| (v / nm.tile_size).floor() * nm.tile_size;
                    let create = json!({
                        "file": nm.file, "width": nm.width, "height": nm.height, "tile_size": nm.tile_size,
                        "layers": [{ "name": nm.layer, "tileset": nm.tileset, "z": 0, "collision": nm.collision }],
                    });
                    let stem = nm
                        .file
                        .rsplit('/')
                        .next()
                        .unwrap_or("tiles")
                        .trim_end_matches(".tilemap.toml")
                        .to_string();
                    // scene and room are the scene's own documents.
                    let stem = if stem == "scene" || stem == "room" {
                        format!("{stem}-map")
                    } else {
                        stem
                    };
                    let mut spawn = json!({
                        "op": "spawn", "id": stem,
                        "components": {
                            "Tilemap": { "map": nm.file },
                            "Transform": { "pos": [snap(corner[0]), -snap(-corner[1])] },
                        },
                    });
                    if !rooms.is_empty() {
                        spawn["room"] = json!(nm.room);
                    }
                    out.push(TileDo::Create(create, format!("a new tilemap, {}", nm.file), vec![spawn]));
                }
                if !ok {
                    ui.weak("A file ending .tilemap.toml, a layer name, and a sprite for its tiles.");
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> Json {
        json!({ "file": "m.tilemap.toml", "width": 10, "height": 5, "tile_size": 2.0, "origin": [4.0, 6.0],
                "layers": [{ "name": "ground", "tileset": "tiles/rock", "terrains": ["rock"] }] })
    }

    #[test]
    fn cells_and_the_world_meet_rows_going_down() {
        let m = map();
        assert_eq!(cell_of(&m, [4.1, 5.9]), Some([0, 0]));
        assert_eq!(cell_of(&m, [7.0, 2.5]), Some([1, 1]));
        assert_eq!(cell_of(&m, [3.9, 5.0]), None, "left of the map");
        assert_eq!(cell_of(&m, [4.5, -5.0]), None, "below it");
        assert_eq!(cell_world(&m, [1, 1]), Some(([6.0, 4.0], [8.0, 2.0])));
    }

    #[test]
    fn a_frame_pixel_and_the_world_round_trip() {
        let img = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(400.0, 200.0));
        let info = FrameInfo {
            tick: 0,
            frame: 0,
            width: 800,
            height: 400,
        };
        let m = Mapping::new(
            img,
            info,
            &json!({ "center": [10.0, 5.0], "half": [20.0, 10.0], "angle": 0.0 }),
        )
        .unwrap();
        assert_eq!(m.world_of_px([400.0, 200.0]), [10.0, 5.0]);
        assert_eq!(
            m.world_of_px([0.0, 0.0]),
            [-10.0, 15.0],
            "the top left is up and left"
        );
        let p = m.screen_of_world([-10.0, 15.0]);
        assert_eq!((p.x, p.y), (100.0, 50.0));
    }

    #[test]
    fn a_stroke_is_runs_of_a_row() {
        let cells: BTreeSet<(i64, i64)> = [(0, 1), (0, 2), (0, 3), (0, 5), (1, 2)]
            .into_iter()
            .collect();
        assert_eq!(runs(&cells), vec![(1, 0, 3), (5, 0, 1), (2, 1, 1)]);
        assert_eq!(
            op("m", "ground", [1, 0, 3, 1], &Brush::Terrain("rock".into())),
            json!({ "op": "tiles", "file": "m", "layer": "ground", "rect": { "x": 1, "y": 0, "w": 3, "h": 1 }, "terrain": "rock" })
        );
    }
}
