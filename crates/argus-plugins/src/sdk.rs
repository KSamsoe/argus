//! The plugin SDK the engine ships (phase 5 item 6). Before it builds a
//! plugin, the engine writes the `argus-plugin` crate it was built with to
//! the project's `.argus/plugins/sdk/`, and a plugin depends on it there
//! (`argus-plugin = { path = "../../.argus/plugins/sdk" }`). So a plugin
//! is always built against the engine that loads it, whatever its ABI, and
//! a project copied anywhere still builds.

use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    ("Cargo.toml", include_str!("../../argus-plugin/Cargo.toml")),
    ("src/lib.rs", include_str!("../../argus-plugin/src/lib.rs")),
    ("src/abi.rs", include_str!("../../argus-plugin/src/abi.rs")),
    ("src/ctx.rs", include_str!("../../argus-plugin/src/ctx.rs")),
    (
        "src/decl.rs",
        include_str!("../../argus-plugin/src/decl.rs"),
    ),
    (
        "src/view.rs",
        include_str!("../../argus-plugin/src/view.rs"),
    ),
];

/// Where a project's copy of the SDK is.
pub fn dir(root: &Path) -> PathBuf {
    root.join(".argus").join("plugins").join("sdk")
}

/// Write the SDK into the project, each file only when it differs (so a
/// built plugin stays built).
pub fn write(root: &Path) -> std::io::Result<PathBuf> {
    let d = dir(root);
    for (rel, text) in FILES {
        let p = d.join(rel);
        if std::fs::read_to_string(&p).ok().as_deref() == Some(*text) {
            continue;
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, text)?;
    }
    Ok(d)
}
