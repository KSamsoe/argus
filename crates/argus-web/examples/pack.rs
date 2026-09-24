//! Pack a project for the web player: `cargo run -p argus-web --example pack -- <project> <out.pack>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (dir, out) = match (a.get(1), a.get(2)) {
        (Some(d), Some(o)) => (d, o),
        _ => {
            eprintln!("usage: pack <project> <out.pack>");
            std::process::exit(2)
        }
    };
    let bytes = argus_web::pack(std::path::Path::new(dir)).unwrap_or_else(|e| {
        eprintln!("{}", e.message);
        std::process::exit(1)
    });
    std::fs::write(out, &bytes).unwrap();
    println!(
        "{} bytes, {} files",
        bytes.len(),
        argus_web::unpack(&bytes).unwrap().len()
    );
}
