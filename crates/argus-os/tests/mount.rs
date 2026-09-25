//! A pack mounted on the desktop (phase 2 item 7): paths under its root are
//! memory, reads and writes alike, and nothing there reaches the disk;
//! every other path is still the disk.

use argus_os::fs;

#[test]
fn a_mounted_pack_is_memory_and_the_rest_is_the_disk() {
    let base = std::env::temp_dir().join(format!("argus-mount-{}", std::process::id()));
    let root = base.join("game");
    fs::mount(
        &root,
        vec![
            ("argus.toml".into(), b"name = \"x\"".to_vec()),
            ("scenes/a/scene.toml".into(), b"seed = 1".to_vec()),
            ("scenes/a/b.toml".into(), b"[Transform]".to_vec()),
        ],
    );
    assert_eq!(fs::read(root.join("argus.toml")).unwrap(), b"name = \"x\"");
    assert!(fs::is_dir(root.join("scenes")) && fs::is_file(root.join("scenes/a/b.toml")));
    let names: Vec<String> = fs::read_dir(root.join("scenes/a"))
        .unwrap()
        .into_iter()
        .map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["b.toml", "scene.toml"]);
    // A write under the root (the importer's cache) stays in memory.
    fs::create_dir_all(root.join(".argus/assets")).unwrap();
    fs::write(root.join(".argus/assets/x.png"), [1, 2, 3]).unwrap();
    assert_eq!(
        fs::read(root.join(".argus/assets/x.png")).unwrap(),
        [1, 2, 3]
    );
    assert!(!root.exists(), "nothing on disk");
    // Beside the root is the disk.
    std::fs::create_dir_all(&base).unwrap();
    fs::write(base.join("real.txt"), "disk").unwrap();
    assert_eq!(
        std::fs::read_to_string(base.join("real.txt")).unwrap(),
        "disk"
    );
    assert!(fs::read(root.join("missing")).is_err());
    std::fs::remove_dir_all(&base).unwrap();
}
