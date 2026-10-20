//! `fs::write` replaces a file whole: a reader sees the old file or the new
//! one, never half of one, and nothing of the write is left beside it.

use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("argus-os-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_reader_never_sees_half_a_write() {
    let d = dir("whole");
    let p = d.join("table.toml");
    let (a, b) = (vec![b'a'; 64 * 1024], vec![b'b'; 64 * 1024]);
    argus_os::fs::write(&p, &a).unwrap();
    let writer = {
        let (p, a, b) = (p.clone(), a.clone(), b.clone());
        std::thread::spawn(move || {
            for i in 0..300 {
                argus_os::fs::write(&p, if i % 2 == 0 { &b } else { &a }).unwrap();
            }
        })
    };
    let mut reads = 0;
    while !writer.is_finished() {
        // A read may meet the move itself on Windows and fail; it never
        // reads a file cut short.
        if let Ok(got) = std::fs::read(&p) {
            assert!(got == a || got == b, "a read of {} bytes, neither", got.len());
            reads += 1;
        }
    }
    writer.join().unwrap();
    assert!(reads > 0);
    // Nothing left beside it.
    let names: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["table.toml"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_new_file_is_written_and_an_old_one_replaced() {
    let d = dir("replace");
    let p = d.join("a.txt");
    argus_os::fs::write(&p, "one").unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "one");
    argus_os::fs::write(&p, "two, longer").unwrap();
    argus_os::fs::write(&p, "3").unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "3");
    let _ = std::fs::remove_dir_all(&d);
}
