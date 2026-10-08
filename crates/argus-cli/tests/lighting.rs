//! Phase 5 item 3: a lit frame's lights read as lines, as the rest of the
//! semantic frame does.

use argus_cli::render;
use serde_json::json;

#[test]
fn a_lit_frame_says_its_ambient_and_each_light() {
    let text = render::result(&json!({
        "format": "semantic", "tick": 3, "size": [640, 360],
        "camera": { "id": "@camera", "rect": [0, 0, 16, 9], "angle": 0 },
        "hud": [], "columns": [], "entities": [], "groups": [], "effects": [],
        "lighting": {
            "ambient": "#101820", "level": 0.4, "shadows": true,
            "lights": [
                { "id": "@player", "kind": "point", "at": [320, 200], "radius_px": 140, "color": "#FFD8A0", "intensity": 1.2, "flicker": 0.1 },
                { "id": "@lamp", "kind": "cone", "at": [10, 20], "radius_px": 90, "color": "#FFFFFF", "intensity": 1, "direction": 270, "angle": 30, "shadows": false }
            ]
        },
        "omitted": 0, "problems": [], "bytes": 500
    }));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "Lit: ambient #101820 at 0.4, with shadows");
    assert_eq!(
        lines[2],
        "Light point @player at (320, 200), 140 px, #FFD8A0 x1.2, flickering 0.1"
    );
    assert_eq!(
        lines[3],
        "Light cone @lamp at (10, 20), 90 px, #FFFFFF x1, pointing 270 degrees, 30 each side, no shadows"
    );
}
