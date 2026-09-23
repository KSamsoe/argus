//! Phase 1 item 11 through the protocol: sprites draw with a project
//! material and with the library's dissolve, which a script tweens; the
//! library is listed beside the project's materials; a shader that does not
//! compile comes back through script.errors with its line; and an edit
//! reloads in place (§13 "Materials and shaders", §7, §17).

mod common;

use common::*;
use serde_json::json;
use std::path::Path;

const GLOW: &str = "// A flat colour, brighter by power.\nfn material(in: MaterialInput, p: Params) -> vec4<f32> {\n    let c = sample_atlas(in, in.uv);\n    return vec4<f32>(p.tint.rgb * p.power, c.a);\n}\n";

fn project(root: &Path) {
    write(
        root,
        "argus.toml",
        "name = \"materials\"\nclock = \"continuous@60\"\ndefault_scene = \"room\"\nplugins = []\n",
    );
    write(root, "assets/materials/glow.wgsl", GLOW);
    write(
        root,
        "assets/materials/glow.toml",
        "type = \"material\"\ndoc = \"A flat glow\"\nblend = \"add\"\n\n[params.tint]\ntype = \"color\"\ndefault = \"#FF8000\"\n\n[params.power]\ntype = \"f32\"\ndefault = 1\nrange = [0, 4]\n",
    );
    write(
        root,
        "systems/fade.luau",
        r#"system "late.fade" {
  stage = "late",
  writes = { "Tween" },
  doc = "The ghost fades out from tick 5.",
  run = function(q)
    if q.tick == 5 then
      q:tween(q:entity("@ghost"), "Dissolve.amount", { to = 1, duration = 0.5, easing = "linear", on_complete = "ghost.gone" })
    end
  end,
}
"#,
    );
    write(
        root,
        "scenes/room/scene.toml",
        "clock = \"continuous@60\"\nseed = 1\n",
    );
    write(
        root,
        "scenes/room/camera.toml",
        "[Transform]\npos = [0.0, 0.0]\n\n[Camera]\nheight = 10.0\n",
    );
    write(
        root,
        "scenes/room/lamp.toml",
        "[Transform]\npos = [-2.0, 0.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [2.0, 2.0]\ncolor = \"#FFFFFF\"\nmaterial = \"materials/glow\"\n",
    );
    write(
        root,
        "scenes/room/ghost.toml",
        "[Transform]\npos = [2.0, 0.0]\n\n[Sprite]\nkind = \"rect\"\nsize = [2.0, 2.0]\ncolor = \"#FFFFFF\"\nmaterial = \"dissolve\"\n\n[Dissolve]\namount = 0.0\n",
    );
}

#[test]
fn materials_draw_reload_and_report_their_errors_with_lines() {
    let t = temp("materials");
    project(t.path());
    let mut d = open(t.path());

    // The project's material and the library's five.
    let list = ok(&mut d, "asset.list", json!({ "kind": "material" }));
    assert_eq!(list["total"], 6, "{list}");
    let names: Vec<&str> = list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[0].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"materials/glow") && names.contains(&"dissolve"),
        "{names:?}"
    );
    let glow = ok(&mut d, "asset.get", json!({ "name": "materials/glow" }));
    assert_eq!(glow["blend"], "add");
    assert_eq!(glow["params"][1]["name"], "power");
    let dissolve = ok(&mut d, "asset.get", json!({ "name": "dissolve" }));
    assert_eq!(dissolve["builtin"], true);
    assert_eq!(dissolve["params"][0]["from"], "Dissolve.amount");
    // The library's per-sprite components are components like any other.
    let desc = ok(&mut d, "world.describe", json!({ "component": "Dissolve" }));
    assert!(desc.to_string().contains("tween it"), "{desc}");

    // A frame draws both materials.
    if let Some(f) = frame(&mut d, json!({ "width": 100, "height": 100 })) {
        assert_eq!(f["materials"], 2, "glow and dissolve: {f}");
        assert_eq!(f["missing_materials"], json!([]));
        assert_eq!(f["material_problems"], json!([]));
    }

    // A script tweens the dissolve; the ghost is gone half a second later.
    ok(&mut d, "sim.step", json!({ "n": 20 }));
    let amount = ok(&mut d, "world.get", json!({ "entity": "@ghost" }))["components"]["Dissolve"]
        ["amount"]
        .as_f64()
        .unwrap();
    assert!(amount > 0.2 && amount < 0.8, "part way: {amount}");
    ok(&mut d, "sim.step", json!({ "n": 30 }));
    assert_eq!(
        ok(&mut d, "obs.events", json!({ "name": "ghost.gone" }))["total"],
        1
    );
    let w = ok(
        &mut d,
        "world.who_writes",
        json!({ "component": "Dissolve" }),
    );
    assert_eq!(w["systems"], json!(["tween.update"]));

    // A shader that does not compile: script.errors has it with its line.
    write(
        t.path(),
        "assets/materials/glow.wgsl",
        &GLOW.replace("p.tint.rgb * p.power", "p.tint.rgb * p.powr"),
    );
    let r = ok(&mut d, "asset.import", json!({}));
    assert_eq!(r["failed"][0]["name"], "materials/glow", "{r}");
    let errs = ok(&mut d, "script.errors", json!({}));
    assert_eq!(errs["total"], 1, "{errs}");
    let row = &errs["rows"][0];
    assert_eq!(row["phase"], "material");
    assert_eq!(row["file"], "assets/materials/glow.wgsl");
    assert_eq!(row["line"], 4, "{row}");
    assert!(row["message"].as_str().unwrap().contains("powr"), "{row}");
    // The last good version still draws.
    if let Some(f) = frame(&mut d, json!({ "width": 100, "height": 100 })) {
        assert_eq!(f["missing_materials"], json!([]), "{f}");
    }

    // Fixed, the error goes. Each edit reloads in place: the import checks
    // the shader, the renderer compiles it, and the next frame draws it.
    let mut best = f64::MAX;
    for power in ["2.0", "1.5", "1.0"] {
        write(
            t.path(),
            "assets/materials/glow.wgsl",
            &GLOW.replace(
                "p.tint.rgb * p.power",
                &format!("p.tint.rgb * p.power * {power}"),
            ),
        );
        let t0 = std::time::Instant::now();
        let r = ok(&mut d, "asset.import", json!({}));
        best = best.min(t0.elapsed().as_secs_f64() * 1e3);
        assert_eq!(r["imported"], json!(["materials/glow"]), "{r}");
    }
    assert_eq!(ok(&mut d, "script.errors", json!({}))["total"], 0);
    println!("material hot reload (import, check, GPU compile): {best:.1} ms");
    assert!(best < 100.0, "{best} ms");
    if let Some(f) = frame(&mut d, json!({ "width": 100, "height": 100 })) {
        assert_eq!(f["materials"], 2, "{f}");
    }
}
