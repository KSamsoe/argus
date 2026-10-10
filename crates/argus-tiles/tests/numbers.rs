//! A tilemap's numbers say what they are or are refused: `chunk = 32.0`
//! used to become the default 64 without a word, and a `tile_size` of 0
//! was taken (both found writing the file-format reference).

use argus_tiles::MapFile;
use std::path::PathBuf;

fn map(name: &str, keys: &str) -> (PathBuf, &'static str) {
    let dir =
        std::env::temp_dir().join(format!("argus-tiles-numbers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    let rel = "maps/m.tilemap.toml";
    std::fs::write(
        dir.join(rel),
        format!("kind = \"tilemap\"\nwidth = 4\nheight = 4\nchunks = \"m.chunks\"\nhash = \"fnv1a64:cbf29ce484222325\"\n{keys}\n\n[[layers]]\nname = \"ground\"\n"),
    )
    .unwrap();
    (dir, rel)
}

#[test]
fn a_whole_number_written_as_a_float_is_that_number() {
    let (dir, rel) = map("float", "chunk = 32.0");
    let m = MapFile::open(&dir, rel).unwrap();
    assert_eq!(m.index.chunk, 32);
}

#[test]
fn a_number_that_is_not_whole_is_refused_not_defaulted() {
    let (dir, rel) = map("half", "chunk = 32.5");
    let e = MapFile::open(&dir, rel).unwrap_err();
    assert!(
        e.message.contains("chunk") && e.message.contains("whole number"),
        "{e}"
    );
    let (dir, rel) = map("text", "cell = \"two\"");
    let e = MapFile::open(&dir, rel).unwrap_err();
    assert!(e.message.contains("cell"), "{e}");
}

#[test]
fn a_tile_size_must_be_more_than_nothing() {
    for bad in ["tile_size = 0", "tile_size = -1.5", "tile_size = \"big\""] {
        let (dir, rel) = map("size", bad);
        let e = MapFile::open(&dir, rel).unwrap_err();
        assert!(e.message.contains("tile_size"), "{bad}: {e}");
    }
    let (dir, rel) = map("size-ok", "tile_size = 0.5");
    assert_eq!(MapFile::open(&dir, rel).unwrap().index.tile_size, 0.5);
}
