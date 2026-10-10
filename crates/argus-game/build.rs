//! On Linux the loader looks for a program's libraries in the system's
//! places, not beside the program as Windows does. The Steam runtime ships
//! `libsteam_api.so` beside the game (distribution item 1), so the
//! executable tells the loader to look there too (`$ORIGIN`, its own
//! folder). Found by the first release's check: the Steam games' Linux
//! builds did not start from their release folders.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
