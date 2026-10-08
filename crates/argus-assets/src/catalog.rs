//! The sprite catalog a release pack carries (§15 "a release pack is built
//! from the import cache, not the source tree", phase 2 item 7): what the
//! importer made of each sprite, so a release build draws the cached texture
//! with its frames, clips, and tiles, with neither the source image nor the
//! sidecar in the pack and no import at start. Sounds, animators, emitters,
//! and materials are small text and import from their sidecars as usual.

use crate::{
    codes, Asset, AutoMode, AutoTileSpec, Clip, Direction, Entry, Frame, Importer, Shape,
    SpriteAsset,
};
use argus_ecs::layers::{self, Change};
use argus_ecs::{ArgusError, Layers, Result};
use serde_json::{json, Map, Value as Json};
use std::collections::BTreeMap;
use std::path::Path;

/// The catalog's path in a pack.
pub const CATALOG: &str = "argus.cache/catalog.json";
/// Where a pack keeps the cached textures: `argus.cache/textures/<name>.png`.
pub const TEXTURES: &str = "argus.cache/textures";

impl Direction {
    /// From its written name.
    pub fn parse(s: &str) -> Option<Direction> {
        Some(match s {
            "forward" => Direction::Forward,
            "reverse" => Direction::Reverse,
            "pingpong" => Direction::PingPong,
            _ => return None,
        })
    }
}

impl AutoMode {
    /// From its written name.
    pub fn parse(s: &str) -> Option<AutoMode> {
        Some(match s {
            "single" => AutoMode::Single,
            "4bit" => AutoMode::FourBit,
            "blob47" => AutoMode::Blob47,
            _ => return None,
        })
    }
}

/// One sprite as the catalog carries it; `texture` is the texture's path in
/// the pack, `normal` its normal map's.
fn sprite_json(e: &Entry, s: &SpriteAsset, texture: &str, normal: Option<&str>) -> Json {
    let frames: Vec<Json> = s
        .frames
        .iter()
        .map(|f| {
            json!({ "rect": f.rect, "pivot": f.pivot, "duration_ms": f.duration_ms, "properties": f.properties })
        })
        .collect();
    let clips: Map<String, Json> = s
        .clips
        .iter()
        .map(|(n, c)| {
            (
                n.clone(),
                json!({ "frames": c.frames, "fps": c.fps, "loop": c.looping, "direction": c.direction.name() }),
            )
        })
        .collect();
    let autotile: Map<String, Json> = s
        .autotile
        .iter()
        .map(|(n, a)| {
            (
                n.clone(),
                json!({ "mode": a.mode.name(), "tiles": a.tiles, "edges": a.connect_edges, "at": a.at }),
            )
        })
        .collect();
    json!({
        "name": e.name,
        "sidecar": e.sidecar,
        "hash": format!("{:016x}", e.hash),
        "source": s.source,
        "size": [s.width, s.height],
        "texture": texture,
        "texture_size": s.texture_size,
        "normal": normal,
        "pixels_per_unit": s.pixels_per_unit,
        "frames": frames,
        "clips": clips,
        "shape": s.shape.as_ref().map(|sh| json!({ "kind": sh.kind, "size": sh.size, "color": sh.color })),
        "autotile": autotile,
    })
}

fn bad(what: &str) -> ArgusError {
    ArgusError::new(
        codes::CACHE,
        format!("the pack's sprite catalog is damaged: {what}"),
    )
    .hint("Rebuild the game with `argus build`.")
}

fn u32s<const N: usize>(j: &Json, what: &str) -> Result<[u32; N]> {
    let a = j
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| bad(what))?;
    let mut out = [0u32; N];
    for (o, v) in out.iter_mut().zip(a) {
        *o = v.as_u64().ok_or_else(|| bad(what))? as u32;
    }
    Ok(out)
}

fn f32s<const N: usize>(j: &Json, what: &str) -> Result<[f32; N]> {
    let a = j
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| bad(what))?;
    let mut out = [0f32; N];
    for (o, v) in out.iter_mut().zip(a) {
        *o = v.as_f64().ok_or_else(|| bad(what))? as f32;
    }
    Ok(out)
}

/// One sprite back from the catalog, its texture under `root`.
fn sprite_from(j: &Json, root: &Path) -> Result<Entry> {
    let name = j["name"]
        .as_str()
        .ok_or_else(|| bad("a sprite without a name"))?;
    let what = |f: &str| format!("{name}'s {f}");
    let [width, height] = u32s::<2>(&j["size"], &what("size"))?;
    let mut frames = Vec::new();
    for f in j["frames"].as_array().ok_or_else(|| bad(&what("frames")))? {
        let properties: BTreeMap<String, String> = f["properties"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        frames.push(Frame {
            rect: u32s::<4>(&f["rect"], &what("frame rect"))?,
            pivot: f32s::<2>(&f["pivot"], &what("frame pivot"))?,
            duration_ms: f["duration_ms"].as_u64().unwrap_or(100) as u32,
            properties,
        });
    }
    let mut clips = BTreeMap::new();
    for (n, c) in j["clips"].as_object().into_iter().flatten() {
        let frames = c["frames"]
            .as_array()
            .ok_or_else(|| bad(&what("clip")))?
            .iter()
            .map(|v| {
                v.as_u64()
                    .map(|x| x as u32)
                    .ok_or_else(|| bad(&what("clip")))
            })
            .collect::<Result<Vec<u32>>>()?;
        clips.insert(
            n.clone(),
            Clip {
                frames,
                fps: c["fps"].as_f64().map(|f| f as f32),
                looping: c["loop"].as_bool().unwrap_or(true),
                direction: c["direction"]
                    .as_str()
                    .and_then(Direction::parse)
                    .ok_or_else(|| bad(&what("clip direction")))?,
            },
        );
    }
    let mut autotile = BTreeMap::new();
    for (n, a) in j["autotile"].as_object().into_iter().flatten() {
        autotile.insert(
            n.clone(),
            AutoTileSpec {
                terrain: n.clone(),
                mode: a["mode"]
                    .as_str()
                    .and_then(AutoMode::parse)
                    .ok_or_else(|| bad(&what("auto-tiling mode")))?,
                tiles: a["tiles"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_u64().map(|x| x as u32))
                    .collect(),
                connect_edges: a["edges"].as_bool().unwrap_or(false),
                at: a["at"].as_str().unwrap_or_default().to_string(),
            },
        );
    }
    let shape = match &j["shape"] {
        Json::Null => None,
        s => Some(Shape {
            kind: s["kind"].as_str().unwrap_or("rect").to_string(),
            size: u32s::<2>(&s["size"], &what("shape size"))?,
            color: s["color"].as_str().unwrap_or("#FF00FF").to_string(),
        }),
    };
    let texture = j["texture"].as_str().ok_or_else(|| bad(&what("texture")))?;
    Ok(Entry {
        name: name.to_string(),
        sidecar: j["sidecar"].as_str().map(str::to_string),
        hash: j["hash"]
            .as_str()
            .and_then(|h| u64::from_str_radix(h, 16).ok())
            .unwrap_or(0),
        warnings: Vec::new(),
        asset: Asset::Sprite(SpriteAsset {
            source: j["source"].as_str().map(str::to_string),
            width,
            height,
            texture: root.join(texture),
            texture_size: u32s::<2>(&j["texture_size"], &what("texture size"))?,
            normal: j["normal"].as_str().map(|n| root.join(n)),
            frames,
            clips,
            pixels_per_unit: j["pixels_per_unit"].as_f64().unwrap_or(16.0) as f32,
            shape,
            autotile,
            aseprite: None,
        }),
        ms: 0.0,
    })
}

/// What a pack needs of the importer's sprites.
pub struct PackedSprites {
    /// The catalog, as `argus.cache/catalog.json` holds it.
    pub catalog: Json,
    /// Each cached texture: its path in the pack, and where it is now.
    pub textures: Vec<(String, std::path::PathBuf)>,
    /// The files under `assets/` the sprites were made from, which the pack
    /// leaves out (source images and sprite sidecars).
    pub sources: Vec<String>,
}

impl Importer {
    /// The sprites as a pack carries them (`argus build`): every imported
    /// sprite's cached texture and what the importer made of it.
    pub fn packed_sprites(&self) -> PackedSprites {
        let mut sprites = Vec::new();
        let mut textures = Vec::new();
        let mut sources = Vec::new();
        for (name, e) in &self.entries {
            let Asset::Sprite(s) = &e.asset else {
                continue;
            };
            let rel = format!("{TEXTURES}/{name}.png");
            let normal = s
                .normal
                .as_ref()
                .map(|n| (format!("{TEXTURES}/{name}.normal.png"), n));
            sprites.push(sprite_json(
                e,
                s,
                &rel,
                normal.as_ref().map(|n| n.0.as_str()),
            ));
            textures.push((rel, s.texture.clone()));
            if let Some((rel, path)) = normal {
                textures.push((rel, path.clone()));
            }
            sources.extend(s.source.iter().map(|p| format!("assets/{p}")));
            sources.extend(e.sidecar.iter().map(|p| format!("assets/{p}")));
        }
        PackedSprites {
            catalog: json!({ "importer": crate::IMPORTER_VERSION, "sprites": sprites }),
            textures,
            sources,
        }
    }

    /// An importer for a mounted pack (a release build): its sprites come
    /// from the catalog and their cached textures; every other asset is
    /// imported from its sidecar when [`scan`](Importer::scan) runs.
    pub fn packed(root: &Path) -> Result<Importer> {
        let mut imp = Importer::new(root);
        let text = argus_os::fs::read_to_string(root.join(CATALOG))
            .map_err(|e| ArgusError::new(codes::CACHE, format!("{CATALOG}: {e}")))?;
        let j: Json = serde_json::from_str(&text).map_err(|e| bad(&e.to_string()))?;
        for s in j["sprites"].as_array().ok_or_else(|| bad("no sprites"))? {
            let e = sprite_from(s, root)?;
            imp.packed.insert(e.name.clone());
            imp.entries.insert(e.name.clone(), e);
        }
        Ok(imp)
    }

    /// An importer for the game and its mods (phase 3 item 8) whose
    /// sprites come from each layer's catalog where it has one: a release
    /// build's game pack and its mod packs. A layer without one imports
    /// from its files when [`scan`](Importer::scan) runs. A mod's sprite
    /// follows the namespace rule: in its namespace it is added, with the
    /// name of one before it it replaces that one, otherwise it is a
    /// failed import.
    pub fn packed_layers(layers: &Layers) -> Result<Importer> {
        let mut imp = Importer::layered(layers);
        // Every asset name a layer before has, whether its catalog or its
        // files (a pack over a mod still a directory) say so.
        let mut known: std::collections::BTreeSet<String> = Default::default();
        for li in 0..imp.layers.len() {
            let mut files = BTreeMap::new();
            crate::walk(&imp.layers[li].dir, &imp.layers[li].dir, &mut files);
            let here: Vec<String> = crate::group_files(li, files.keys()).into_keys().collect();
            let root = imp.layers[li].root.clone();
            let path = root.join(CATALOG);
            if !argus_os::fs::is_file(&path) {
                known.extend(here);
                continue;
            }
            let shown = format!("{}{CATALOG}", layers.each()[li].prefix);
            let text = argus_os::fs::read_to_string(&path)
                .map_err(|e| ArgusError::new(codes::CACHE, format!("{shown}: {e}")))?;
            let j: Json = serde_json::from_str(&text).map_err(|e| bad(&e.to_string()))?;
            let id = imp.layers[li].id.clone();
            for s in j["sprites"].as_array().ok_or_else(|| bad("no sprites"))? {
                let e = sprite_from(s, &root)?;
                let name = e.name.clone();
                if let Some(id) = &id {
                    if known.contains(&name) {
                        let was = imp
                            .owners
                            .get(&name)
                            .map(|o| layers::dir_of(o))
                            .unwrap_or_default();
                        imp.changes
                            .push(Change::new("asset", &name, id, &was, shown.clone()));
                    } else if !layers::owns(id, &name) {
                        imp.packed_failed.push(crate::Failure {
                            name: name.clone(),
                            error: layers::outside(id, "asset", &name, shown.clone()),
                        });
                        continue;
                    }
                    imp.owners.insert(name.clone(), id.clone());
                }
                imp.packed.insert(name.clone());
                known.insert(name.clone());
                imp.entries.insert(name, e);
            }
            known.extend(here);
        }
        Ok(imp)
    }
}
