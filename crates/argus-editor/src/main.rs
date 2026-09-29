//! `argus-editor`: the editor window (§11). `argus editor` starts it.

use argus_editor::{launch, Editor};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match launch::parse(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let socket = match launch::socket_for(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("argus-editor: {e}");
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("argus editor")
            .with_inner_size([1280.0, 800.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let author = args.author.clone();
    if let Err(e) = eframe::run_native(
        "argus editor",
        options,
        Box::new(move |_cc| Ok(Box::new(Editor::new(socket, author)))),
    ) {
        eprintln!("argus-editor: {e}");
        std::process::exit(1);
    }
}
