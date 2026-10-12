//! Sprites made from a kind and a seed (asset generation, item 3): each
//! generator writes an art script (`assets/<name>.art.luau`), not a
//! picture, so what it makes is something to change. Creatures,
//! characters, and items come out as rows of text, one character a pixel,
//! under a palette of named colours; tiles, plants, and rocks as a few
//! lines of drawing with their seed and colours at the top. The same kind
//! and seed write the same script, and the script draws the same pixels
//! on every machine.

/// The kinds, each with what it is for.
pub const KINDS: [(&str, &str); 6] = [
    ("creature", "a monster or a critter, seen from the side: idle and walk frames"),
    ("character", "a person facing right: a standing frame and four of a walk"),
    ("item", "a thing to pick up: a potion, a gem, a coin, a key, a heart, an apple, a sword, a shield"),
    ("tile", "a ground tile that repeats seamlessly: grass, stone, dirt, sand, water, or planks, in four variants"),
    ("plant", "a bush or a tree, standing on its base: two frames of a sway"),
    ("rock", "a rock or a boulder, shaded"),
];

/// PCG32, as the engine's streams, for the generators' choices.
struct Rng(u64, u64);

impl Rng {
    fn new(seed: u64, stream: &str) -> Rng {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in stream.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        let mut r = Rng(0, (h << 1) | 1);
        r.next();
        r.0 = r.0.wrapping_add(seed);
        r.next();
        r
    }
    fn next(&mut self) -> u32 {
        let old = self.0;
        self.0 = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.1);
        let x = (((old >> 18) ^ old) >> 27) as u32;
        x.rotate_right((old >> 59) as u32)
    }
    fn float(&mut self) -> f64 {
        self.next() as f64 / 4_294_967_296.0
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.float()
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.float() * n as f64) as usize).min(n.saturating_sub(1))
    }
    fn chance(&mut self, p: f64) -> bool {
        self.float() < p
    }
    fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
        &of[self.below(of.len())]
    }
}

fn hsv(h: f64, s: f64, v: f64) -> String {
    let h = ((h % 360.0) + 360.0) % 360.0 / 60.0;
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let q = |u: f64| libm::round((u + m) * 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", q(r), q(g), q(b))
}

fn rows_lua(rows: &[String], indent: &str) -> String {
    rows.iter()
        .map(|r| format!("{indent}\"{r}\",\n"))
        .collect::<String>()
}

fn header(what: &str, name: &str, kind: &str, seed: u64, how: &str) -> String {
    format!(
        "-- {what}\n-- Made by: argus generate name={name} sprite={kind} seed={seed}\n-- {how}\n-- The game shows a change as soon as this file is saved. What the kit\n-- can draw: argus docs files section=art.luau\n\n"
    )
}

/// The art script of a sprite of `kind`, picked by `seed`, for the asset
/// `name`. `Err` names the kinds.
pub fn generate(kind: &str, seed: u64, name: &str) -> Result<String, String> {
    let mut r = Rng::new(seed, &format!("sprite.{kind}"));
    match kind {
        "creature" => Ok(creature(&mut r, name, seed)),
        "character" => Ok(character(&mut r, name, seed)),
        "item" => Ok(item(&mut r, name, seed)),
        "tile" => Ok(tile(&mut r, name, seed)),
        "plant" => Ok(plant(&mut r, name, seed)),
        "rock" => Ok(rock(&mut r, name, seed)),
        _ => {
            let names: Vec<&str> = KINDS.iter().map(|(k, _)| *k).collect();
            Err(format!(
                "there is no sprite kind {kind:?}; the kinds are {}",
                names.join(", ")
            ))
        }
    }
}

/// A creature in the manner of Dave Bollinger's pixel sprite generator: a
/// mask of where body may be, filled by chance, mirrored, its darker
/// underside, eyes, and feet that step.
fn creature(r: &mut Rng, name: &str, seed: u64) -> String {
    // The left half, 7 wide, 12 tall: 0 never, 1 maybe body, 2 body or
    // its dark side, 3 always body.
    const MASK: [[u8; 7]; 10] = [
        [0, 0, 0, 0, 0, 1, 1],
        [0, 0, 0, 0, 1, 1, 2],
        [0, 0, 0, 1, 1, 2, 3],
        [0, 0, 1, 1, 2, 3, 3],
        [0, 1, 1, 2, 3, 3, 3],
        [0, 1, 2, 2, 3, 3, 3],
        [0, 1, 2, 3, 3, 3, 3],
        [0, 1, 1, 2, 3, 3, 3],
        [0, 0, 1, 1, 2, 3, 3],
        [0, 0, 0, 1, 1, 2, 2],
    ];
    let mut half: Vec<Vec<char>> = MASK
        .iter()
        .enumerate()
        .map(|(y, row)| {
            row.iter()
                .map(|&m| match m {
                    0 => '.',
                    1 => {
                        if r.chance(0.55) {
                            'b'
                        } else {
                            '.'
                        }
                    }
                    2 if y >= 6 && r.chance(0.6) => 'd',
                    2 => 'b',
                    _ => 'b',
                })
                .collect()
        })
        .collect();
    // Eyes: one each side, on a body pixel in the upper middle.
    let ey = 3 + r.below(3);
    let ex = 4 + r.below(2);
    half[ey][ex] = 'e';
    if r.chance(0.5) && ey + 1 < half.len() {
        half[ey + 1][ex] = 'e';
    }
    // A spot or a stripe of the third colour.
    let sy = 5 + r.below(3);
    for cell in half[sy][3..7].iter_mut() {
        if *cell == 'b' && r.chance(0.5) {
            *cell = 's';
        }
    }
    let full: Vec<String> = half
        .iter()
        .map(|row| {
            let mut s: String = row.iter().collect();
            let mirror: String = row[..6].iter().rev().collect();
            s.push_str(&mirror);
            s
        })
        .collect();
    let feet_style = r.below(3);
    let (feet_a, feet_b) = match feet_style {
        0 => ("...dd...dd...", "....dd.dd...."),
        1 => ("..d..d.d..d..", "...d.d...d.d."),
        _ => ("....ddddd....", "...dd...dd..."),
    };
    let hue = r.range(0.0, 360.0);
    let shift = r.range(20.0, 60.0) * if r.chance(0.5) { 1.0 } else { -1.0 };
    let pal = [
        ("body", hsv(hue, r.range(0.45, 0.75), r.range(0.65, 0.85))),
        ("dark", hsv(hue + shift * 0.3, 0.7, 0.45)),
        ("spot", hsv(hue + shift, 0.6, 0.9)),
        ("eye", "#FFFFFF".to_string()),
        ("light", hsv(hue - 10.0, 0.3, 0.97)),
        ("line", hsv(hue, 0.6, 0.15)),
    ];
    let mut s = header(
        "A creature, seen from the side: frames 0 and 1 stand and breathe, 2 and 3 walk.",
        name,
        "creature",
        seed,
        "Its shape is the rows below (b body, d its dark side, s a spot, e an eye; a dot is empty).",
    );
    s.push_str("local pal = {\n");
    for (k, v) in &pal {
        s.push_str(&format!("  {k} = \"{v}\",\n"));
    }
    s.push_str("}\nlocal key = { b = pal.body, d = pal.dark, s = pal.spot, e = pal.eye }\n\n");
    s.push_str("-- The body, 13 wide (the right half mirrors the left).\nlocal body = {\n");
    s.push_str(&rows_lua(&full, "  "));
    s.push_str(&format!(
        "}}\n\n-- Its feet, a row under the body: standing, and the other step.\nlocal feet = {{ \"{feet_a}\", \"{feet_b}\" }}\n\n"
    ));
    s.push_str(
        r##"local function frame(rise, step)
  local c = art.canvas(15, 14)
  c:rows(1, 1 + rise, body, key)
  c:rows(1, 11, { feet[step] }, key)
  c:shade{ light = pal.light }
  c:outline(pal.line)
  return c
end

return art.sprite{
  pixels_per_unit = 14,
  pivot = { 0.5, 0 },
  clips = {
    idle = { frame(0, 1), frame(1, 1), fps = 3 },
    walk = { frame(0, 1), frame(0, 2), fps = 6 },
  },
}
"##,
    );
    s
}

/// A person in the shape of the blank template's player, with a hair
/// style and colours of their own.
fn character(r: &mut Rng, name: &str, seed: u64) -> String {
    let hair_styles: [[&str; 4]; 3] = [
        // Short.
        [
            "......ooooo.......",
            ".....oHHHHHo......",
            "....oHHHHHHHo.....",
            "....oHHHHHHHHo....",
        ],
        // A cap with a brim.
        [
            "......ooooo.......",
            ".....oCCCCCo......",
            "....oCCCCCCCo.....",
            "....oCCCCCCCCCo...",
        ],
        // Long, down the back.
        [
            ".....oooooo.......",
            "....oHHHHHHo......",
            "...oHHHHHHHHo.....",
            "...oHHHHHHHHHo....",
        ],
    ];
    let style = r.below(3);
    let mut top: Vec<String> = hair_styles[style].iter().map(|s| s.to_string()).collect();
    let face: [&str; 4] = if style == 2 {
        [
            "...ohHHHHSSSSo....",
            "...ohHHHSSSESo....",
            "...ohhHHSSSESo....",
            "....ohhHSSSSo.....",
        ]
    } else {
        [
            "....ohHHHSSSSo....",
            "....ohHHSSSESo....",
            "....ohhHSSSESo....",
            ".....ohhSSSSo.....",
        ]
    };
    top.extend(face.iter().map(|s| s.to_string()));
    top.extend(
        [
            ".....oYYooooYo....",
            "....oyYYYYYYYo....",
            "....oyYYYYYYYo....",
            "....oyYYYYYYYo....",
            ".....oBBBBBBo.....",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    let skins = [
        "#F5CDA4", "#E8B48A", "#C68E62", "#8D5A3B", "#5C3A26", "#F2D6C0",
    ];
    let hairs = [
        "#6B4630", "#2B2024", "#C9A15B", "#A0432E", "#5A5560", "#E0D8C8",
    ];
    let hue = r.range(0.0, 360.0);
    let hair = r.pick(&hairs).to_string();
    let pal = [
        ("ink", "#1B1A22".to_string()),
        ("hair", hair.clone()),
        ("hair_dark", "art.darken(pal.hair, 0.3)".to_string()),
        ("cap", hsv(hue + 150.0, 0.6, 0.7)),
        ("skin", r.pick(&skins).to_string()),
        ("shirt", hsv(hue, r.range(0.45, 0.7), r.range(0.75, 0.95))),
        ("shirt_dark", hsv(hue + 8.0, 0.7, 0.62)),
        ("belt", hsv(hue + 200.0, 0.25, 0.25)),
        ("leg", hsv(r.range(15.0, 40.0), 0.45, r.range(0.35, 0.5))),
        ("leg_far", "art.darken(pal.leg, 0.3)".to_string()),
    ];
    let mut s = header(
        "A person facing right: frame 0 stands; 1 to 4 are a step on each foot.",
        name,
        "character",
        seed,
        "Drawn as rows of text, one character a pixel: change a row or a colour.",
    );
    s.push_str("local pal = {}\n");
    for (k, v) in &pal {
        if v.starts_with("art.") {
            s.push_str(&format!("pal.{k} = {v}\n"));
        } else {
            s.push_str(&format!("pal.{k} = \"{v}\"\n"));
        }
    }
    s.push_str(
        "local key = {\n  o = pal.ink, E = pal.ink, H = pal.hair, h = pal.hair_dark, C = pal.cap,\n  S = pal.skin, Y = pal.shirt, y = pal.shirt_dark, B = pal.belt, L = pal.leg, f = pal.leg_far,\n}\n\n",
    );
    s.push_str("-- The head and shirt, the same in every frame.\nlocal top = {\n");
    s.push_str(&rows_lua(&top, "  "));
    s.push_str(
        r##"}

-- The near arm, swung against the near leg: {x, y, colour}.
local arms = {
  mid = { { 9, 9, "y" }, { 9, 10, "y" }, { 9, 11, "S" } },
  fwd = { { 10, 9, "y" }, { 11, 10, "y" }, { 12, 11, "S" } },
  back = { { 8, 9, "y" }, { 7, 10, "y" }, { 6, 11, "S" } },
}

-- The legs: three rows under the shirt.
local legs = {
  stand = { ".......ff.LL......", ".......ff.LL......", ".......oo.oo......" },
  stride = { ".......ff.LL......", "......ff...LL.....", ".....oo.....oo...." },
  pass = { "........fLL.......", "........fLL.......", "........ooo......." },
  stride2 = { ".......LL.ff......", "......LL...ff.....", ".....oo.....oo...." },
}

local function frame(leg, arm, sink)
  local c = art.canvas(18, 18)
  for x = 3, 14 do -- a soft shadow under the feet
    c:pixel(x, 16, art.alpha("#000000", (x >= 4 and x <= 13) and 0.43 or 0.2))
  end
  c:rows(0, 13, legs[leg], key)
  c:rows(0, sink, top, key)
  for _, p in arms[arm] do
    c:pixel(p[1], p[2] + sink, key[p[3]])
  end
  return c
end

return art.sprite{
  pixels_per_unit = 9,
  pivot = { 0.5, 0.1 },
  frames = {
    frame("stand", "mid", 0),
    frame("stride", "back", 1),
    frame("pass", "mid", 0),
    frame("stride2", "fwd", 1),
    frame("pass", "mid", 0),
  },
  clips = {
    idle = { 0 },
    walk = { 1, 2, 3, 4, fps = 10 },
  },
}
"##,
    );
    s
}

/// A thing to pick up, from a small book of shapes, in colours of its own.
fn item(r: &mut Rng, name: &str, seed: u64) -> String {
    // o outline, a main, b its dark, c its light, w a white glint, g glass,
    // m metal, n dark metal, k a cork or a handle.
    let book: [(&str, &str, [&str; 12]); 8] = [
        (
            "potion",
            "a potion in a round bottle",
            [
                "....oooo....",
                "....okko....",
                "....oggo....",
                "...oggggo...",
                "..oggggggo..",
                ".oaaaaaaaao.",
                ".oacaaaaabo.",
                ".oaaaaaaabo.",
                ".oaaaaaabbo.",
                "..oaaabbbo..",
                "...oooooo...",
                "............",
            ],
        ),
        (
            "gem",
            "a cut gem",
            [
                "............",
                "...oooooo...",
                "..ocwccaao..",
                ".occcaaaabo.",
                ".oaaaaaabbo.",
                "..oaaaabbo..",
                "...oaabbo...",
                "....oabo....",
                ".....oo.....",
                "............",
                "............",
                "............",
            ],
        ),
        (
            "coin",
            "a coin",
            [
                "............",
                "....oooo....",
                "...occaao...",
                "..ocwaaabo..",
                "..ocaooabo..",
                "..ocaoaabo..",
                "..ocaooabo..",
                "..oaaaabbo..",
                "...oabbbo...",
                "....oooo....",
                "............",
                "............",
            ],
        ),
        (
            "key",
            "a key",
            [
                "............",
                "..oooo......",
                ".occaao.....",
                ".oco.ao.....",
                ".oaaabo.....",
                "..oabboooooo",
                "...oaaaaaaao",
                "....oooaoaoo",
                "......oo.o..",
                "............",
                "............",
                "............",
            ],
        ),
        (
            "heart",
            "a heart",
            [
                "............",
                "..ooo..ooo..",
                ".ocwao.aaao.",
                ".ocaaaaaabo.",
                ".oaaaaaaabo.",
                "..oaaaaabo..",
                "...oaaabo...",
                "....oabo....",
                ".....oo.....",
                "............",
                "............",
                "............",
            ],
        ),
        (
            "apple",
            "an apple",
            [
                "......k.....",
                ".....ko.....",
                "..ooo.ooo...",
                ".ocwaaaaao..",
                ".ocaaaaaabo.",
                ".oaaaaaaabo.",
                ".oaaaaaabbo.",
                "..oaaaabbo..",
                "...oabbbo...",
                "....oooo....",
                "............",
                "............",
            ],
        ),
        (
            "sword",
            "a sword",
            [
                "..........oo",
                ".........owo",
                "........omo.",
                ".......omo..",
                "......omo...",
                ".....omo....",
                "..o.omo.....",
                "..oonoo.....",
                "...onno.....",
                "..okoono....",
                ".oko..o.....",
                ".oo.........",
            ],
        ),
        (
            "shield",
            "a shield",
            [
                "............",
                ".oooooooooo.",
                ".omnmmmmmno.",
                ".omaaaaaano.",
                ".omacaaaano.",
                ".omaaaaabno.",
                "..omaaabno..",
                "..omaaabno..",
                "...omabno...",
                "....omno....",
                ".....oo.....",
                "............",
            ],
        ),
    ];
    let (what, about, rows) = r.pick(&book);
    let hue = r.range(0.0, 360.0);
    let main = match *what {
        "coin" | "key" => hsv(r.range(40.0, 52.0), 0.75, 0.95),
        "heart" | "apple" => hsv(r.range(352.0, 368.0), 0.75, 0.9),
        _ => hsv(hue, r.range(0.55, 0.8), r.range(0.75, 0.95)),
    };
    let mut s = header(
        &format!("{}, to pick up.", capital(about)),
        name,
        "item",
        seed,
        "Drawn as rows (o outline, a its colour, b dark, c light, w a glint, g glass, m metal, k a cork or handle).",
    );
    s.push_str(&format!(
        "local pal = {{ main = \"{main}\", line = \"#1B1A22\", glass = \"#BFE6F2\", metal = \"#C9CED6\", metal_dark = \"#7A808C\", cork = \"#8A5A3A\" }}\n"
    ));
    s.push_str(
        "local key = {\n  o = pal.line, a = pal.main, b = art.darken(pal.main, 0.35), c = art.lighten(pal.main, 0.35),\n  w = \"#FFFFFF\", g = pal.glass, m = pal.metal, n = pal.metal_dark, k = pal.cork,\n}\n\n",
    );
    s.push_str(&format!("-- {}.\nlocal rows = {{\n", capital(about)));
    let rows: Vec<String> = rows.iter().map(|s| s.to_string()).collect();
    s.push_str(&rows_lua(&rows, "  "));
    s.push_str(
        r##"}

local c = art.canvas(12, 12)
c:rows(0, 0, rows, key)
-- A second frame with the glint gone, for a shine that blinks.
local d = c:copy()
d:recolor({ ["#FFFFFF"] = key.c })

return art.sprite{
  pixels_per_unit = 12,
  clips = { shine = { c, d, c, c, fps = 4 } },
}
"##,
    );
    s
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// A ground tile that repeats with no seam: smooth noise that wraps, in a
/// material's colours, four variants.
fn tile(r: &mut Rng, name: &str, seed: u64) -> String {
    let materials: [(&str, [&str; 4], &str); 6] = [
        (
            "grass",
            ["#3E7A3A", "#4E8C42", "#5FA04C", "#79B85A"],
            "blades: specks of light green",
        ),
        (
            "stone",
            ["#4A4F58", "#575D67", "#646B76", "#767E8A"],
            "cracks: dark lines",
        ),
        (
            "dirt",
            ["#5C3F2A", "#6B4A31", "#7A563A", "#8C6646"],
            "pebbles: light specks",
        ),
        (
            "sand",
            ["#C9A86A", "#D5B677", "#E0C488", "#E9D29C"],
            "ripples: soft bands",
        ),
        (
            "water",
            ["#1F4E7A", "#255C8C", "#2E6CA0", "#4A8CC0"],
            "glints: white specks",
        ),
        (
            "planks",
            ["#6E4A2C", "#7D5634", "#8A603A", "#9C6E44"],
            "boards: dark seams",
        ),
    ];
    let (mat, cols, about) = r.pick(&materials);
    let cols: Vec<String> = cols.iter().map(|c| c.to_string()).collect();
    let mut s = header(
        &format!(
            "A {mat} tile, 16 x 16, that repeats with no seam; four variants (frames 0 to 3)."
        ),
        name,
        "tile",
        seed,
        &format!("Smooth noise that wraps at the edges, in the colours below, with {about}."),
    );
    s.push_str(&format!(
        "local SEED = {} -- another number, other variants\nlocal SIZE = 16\nlocal colours = {{ \"{}\", \"{}\", \"{}\", \"{}\" }} -- dark to light\n\n",
        r.next() % 1000,
        cols[0],
        cols[1],
        cols[2],
        cols[3]
    ));
    s.push_str(
        r##"-- Noise that wraps: four samples blended, so the right edge meets the
-- left and the bottom the top.
local function wrapped(x, y, scale, seed)
  local u, v = x / SIZE, y / SIZE
  local n = function(a, b)
    return art.noise(a / scale, b / scale, seed)
  end
  return n(x, y) * (1 - u) * (1 - v) + n(x - SIZE, y) * u * (1 - v)
    + n(x, y - SIZE) * (1 - u) * v + n(x - SIZE, y - SIZE) * u * v
end

local function variant(k)
  local c = art.canvas(SIZE, SIZE)
  local r = art.rng(SEED + k)
  for y = 0, SIZE - 1 do
    for x = 0, SIZE - 1 do
      local n = wrapped(x, y, 4, SEED + k) * 0.75 + wrapped(x, y, 1.5, SEED + k + 50) * 0.25
      local i = math.clamp(math.floor(n * 4 * 1.15 - 0.3) + 1, 1, 4)
      c:pixel(x, y, colours[i])
    end
  end
"##,
    );
    let detail = match *mat {
        "grass" => "  c:speckle({ \"#8FCB6A\", \"#A5D97C\" }, 0.06, SEED + k)\n",
        "stone" => "  for _ = 1, 2 do -- cracks, wrapping at the edges as the noise does\n    local x, y, len, dy = r:int(0, SIZE - 1), r:int(0, SIZE - 1), r:int(3, 6), r:int(-1, 1)\n    for i = 0, len - 1 do\n      c:pixel((x + i) % SIZE, (y + math.floor(dy * i / 2)) % SIZE, \"#33373F\")\n    end\n  end\n",
        "dirt" => "  c:speckle({ \"#A07B58\", \"#4A3222\" }, 0.05, SEED + k)\n",
        "sand" => "  for x = 0, SIZE - 1 do\n    local y = (math.floor(x / 3) + k * 4) % SIZE\n    c:pixel(x, y, \"#EFDDB0\")\n  end\n",
        "water" => "  c:speckle({ \"#E6F4FF\" }, 0.025, SEED + k)\n",
        _ => "  for y = 3, SIZE - 1, 4 do\n    c:line(0, y, SIZE - 1, y, \"#4A301C\")\n  end\n  c:pixel(r:int(0, SIZE - 1), r:int(0, 3) * 4 + 1, \"#4A301C\")\n",
    };
    s.push_str(detail);
    s.push_str(
        r##"  return c
end

return art.sprite{
  pixels_per_unit = 16,
  frames = { variant(0), variant(1), variant(2), variant(3) },
}
"##,
    );
    s
}

/// A bush or a tree: a crown of leaves from noise on a trunk, lit from the
/// top left, two frames of a sway.
fn plant(r: &mut Rng, name: &str, seed: u64) -> String {
    let tree = r.chance(0.5);
    let hue = r.range(85.0, 140.0);
    let leaf = hsv(hue, r.range(0.45, 0.7), r.range(0.5, 0.7));
    let leaf_dark = hsv(hue + 10.0, 0.7, 0.35);
    let leaf_light = hsv(hue - 15.0, 0.45, 0.85);
    let flowers = r.chance(0.4);
    let flower = hsv(r.range(0.0, 360.0), 0.5, 0.95);
    let mut s = header(
        &format!(
            "A {}, standing on its base: two frames of a sway.",
            if tree { "tree" } else { "bush" }
        ),
        name,
        "plant",
        seed,
        "A crown of leaves from noise on a trunk: change its sizes or colours below.",
    );
    s.push_str(&format!(
        "local SEED = {}\nlocal W, H = {}, {} -- its picture\nlocal CROWN = {{ x = {}, y = {}, rx = {}, ry = {} }} -- the leaves\nlocal TRUNK = {{ w = {}, h = {} }} -- 0 for a bush\nlocal pal = {{ leaf = \"{leaf}\", leaf_dark = \"{leaf_dark}\", leaf_light = \"{leaf_light}\", bark = \"#5A3C26\", bark_dark = \"#3E2818\", line = \"#16201A\", flower = \"{flower}\" }}\nlocal FLOWERS = {}\n\n",
        r.next() % 1000,
        if tree { 20 } else { 16 },
        if tree { 26 } else { 13 },
        if tree { 9.5 } else { 7.5 },
        if tree { 9 } else { 6 },
        if tree { 8 } else { 6 },
        if tree { 7 } else { 5 },
        if tree { 3 } else { 0 },
        if tree { 10 } else { 0 },
        if flowers { 0.05 } else { 0.0 },
    ));
    s.push_str(
        r##"local function frame(lean)
  local c = art.canvas(W, H)
  if TRUNK.w > 0 then
    local x = math.floor(W / 2 - TRUNK.w / 2)
    c:rect(x, H - TRUNK.h, TRUNK.w, TRUNK.h, pal.bark)
    c:rect(x + TRUNK.w - 1, H - TRUNK.h, 1, TRUNK.h, pal.bark_dark)
  end
  -- The crown: an ellipse whose edge noise frays, leaning with the wind.
  for y = 0, H - 1 do
    for x = 0, W - 1 do
      local dx = (x - CROWN.x - lean * (1 - y / H)) / CROWN.rx
      local dy = (y - CROWN.y) / CROWN.ry
      local d = math.sqrt(dx * dx + dy * dy)
      local n = art.noise(x / 3, y / 3, SEED)
      if d < 0.75 + n * 0.45 then
        local shade = art.noise(x / 2.5 + 7, y / 2.5, SEED + 1) + (dx + dy) * 0.35
        c:pixel(x, y, shade > 0.85 and pal.leaf_dark or (shade < 0.25 and pal.leaf_light or pal.leaf))
      end
    end
  end
  if FLOWERS > 0 then
    c:speckle({ pal.flower }, FLOWERS, SEED + 2)
  end
  c:outline(pal.line)
  return c
end

return art.sprite{
  pixels_per_unit = 12,
  pivot = { 0.5, 0 },
  clips = { sway = { frame(0), frame(1), fps = 2 } },
}
"##,
    );
    s
}

/// A rock: a blob from noise inside an ellipse, shaded from the top left,
/// with a crack or two.
fn rock(r: &mut Rng, name: &str, seed: u64) -> String {
    let warm = r.chance(0.4);
    let (base, dark, light) = if warm {
        ("#8A7A68", "#5E5246", "#B3A592")
    } else {
        ("#6E737C", "#4A4E56", "#9BA1AB")
    };
    let big = r.chance(0.5);
    let mut s = header(
        &format!(
            "A {}, shaded from the top left.",
            if big { "boulder" } else { "rock" }
        ),
        name,
        "rock",
        seed,
        "A blob from noise inside an ellipse: change its size, seed, or colours below.",
    );
    s.push_str(&format!(
        "local SEED = {}\nlocal W, H = {}, {}\nlocal pal = {{ stone = \"{base}\", dark = \"{dark}\", light = \"{light}\", line = \"#1C1E22\", moss = \"#5A7A3E\" }}\nlocal MOSS = {}\n\n",
        r.next() % 1000,
        if big { 20 } else { 13 },
        if big { 15 } else { 10 },
        if r.chance(0.35) { 0.08 } else { 0.0 },
    ));
    s.push_str(
        r##"local c = art.canvas(W, H)
for y = 0, H - 1 do
  for x = 0, W - 1 do
    local dx, dy = (x - (W - 1) / 2) / (W / 2 - 1), (y - (H - 1) / 2 - 0.5) / (H / 2 - 1)
    local d = dx * dx + dy * dy * 1.2
    if d < 0.8 + art.noise(x / 3, y / 3, SEED) * 0.35 then
      local light = -dx * 0.5 - dy * 0.7 + (art.noise(x / 2, y / 2, SEED + 1) - 0.5) * 0.6
      c:pixel(x, y, light > 0.35 and pal.light or (light < -0.35 and pal.dark or pal.stone))
    end
  end
end
-- A crack.
local r = art.rng(SEED)
local x, y = r:int(math.floor(W / 3), math.floor(W * 2 / 3)), r:int(math.floor(H / 3), math.floor(H / 2))
for _ = 1, r:int(2, 4) do
  local nx, ny = x + r:int(-1, 1), y + 1
  if c:get(nx, ny) and c:get(nx, ny) ~= "#00000000" then
    c:line(x, y, nx, ny, pal.dark)
  end
  x, y = nx, ny
end
if MOSS > 0 then
  c:noise(0, 0, W, math.floor(H / 3), { pal.moss, pal.stone, pal.light }, 2, SEED + 3, true)
end
c:outline(pal.line)

return art.sprite{
  pixels_per_unit = 12,
  pivot = { 0.5, 0.1 },
  frames = { c },
}
"##,
    );
    s
}
