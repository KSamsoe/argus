//! Where a release build keeps what it writes: "a save location per platform
//! convention" (§15, phase 2 item 7). Saves (phase 3) and reports go under
//! it, never beside the executable, which may be read-only, and never in a
//! `.argus/` directory, which a release build does not have.
//!
//! - Windows: `%APPDATA%\<game>` (roaming application data);
//! - macOS: `~/Library/Application Support/<game>`;
//! - Linux and other Unix: `$XDG_DATA_HOME/<game>`, else
//!   `~/.local/share/<game>`.

use std::path::PathBuf;

/// The game's name as a folder name: letters, digits, spaces, `-` and `_`
/// kept, anything else `_`.
pub fn folder_name(game: &str) -> String {
    let s: String = game
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "argus-game".to_string()
    } else {
        s
    }
}

/// The platform's place for a game's data, from the environment given.
pub fn save_dir_in(game: &str, env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let name = folder_name(game);
    let nonempty = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        return nonempty("APPDATA").map(|d| d.join(name));
    }
    if cfg!(target_os = "macos") {
        return nonempty("HOME").map(|h| h.join("Library/Application Support").join(name));
    }
    nonempty("XDG_DATA_HOME")
        .or_else(|| nonempty("HOME").map(|h| h.join(".local/share")))
        .map(|d| d.join(name))
}

/// This machine's place for a game's data; `None` where there is none (a
/// browser, or an environment without a home).
pub fn save_dir(game: &str) -> Option<PathBuf> {
    save_dir_in(game, |k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_per_game_where_the_platform_keeps_them() {
        assert_eq!(folder_name("Hollow: Deeper!"), "Hollow_ Deeper_");
        assert_eq!(folder_name("  "), "argus-game");
        let env = |k: &str| match k {
            "APPDATA" => Some(r"C:\Users\p\AppData\Roaming".to_string()),
            "HOME" => Some("/home/p".to_string()),
            _ => None,
        };
        let d = save_dir_in("hollow", env).unwrap();
        if cfg!(windows) {
            assert_eq!(
                d,
                PathBuf::from(r"C:\Users\p\AppData\Roaming").join("hollow")
            );
        } else if cfg!(target_os = "macos") {
            assert_eq!(
                d,
                PathBuf::from("/home/p/Library/Application Support/hollow")
            );
        } else {
            assert_eq!(d, PathBuf::from("/home/p/.local/share/hollow"));
        }
        assert_eq!(save_dir_in("hollow", |_| None), None);
    }
}
