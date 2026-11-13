//! `argus.toml`'s `[net] delay`: the game's own input delay for a match.

use argus_scene::{ProjectConfig, NET_DELAY};

fn load(extra: &str) -> argus_ecs::Result<ProjectConfig> {
    let dir = std::env::temp_dir().join(format!(
        "argus-net-config-{}-{}",
        std::process::id(),
        extra.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("argus.toml"), format!("name = \"t\"\n{extra}")).unwrap();
    let r = ProjectConfig::load(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    r
}

#[test]
fn a_game_chooses_its_input_delay() {
    assert_eq!(load("").unwrap().net_delay, NET_DELAY);
    assert_eq!(load("[net]\ndelay = 4\n").unwrap().net_delay, 4);
    assert_eq!(load("[net]\ndelay = 0\n").unwrap().net_delay, 0);
    let e = load("[net]\ndelay = 31\n").unwrap_err();
    assert!(e.message.contains("0 to 30"), "{}", e.message);
    let e = load("[net]\ndelay = 4\nlag = 1\n").unwrap_err();
    assert!(e.message.contains("no key \"lag\""), "{}", e.message);
}
