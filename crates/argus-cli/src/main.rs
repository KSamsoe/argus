//! `argus`: see the crate docs. Exit codes: 0 ok, 1 the daemon answered
//! with an error, 2 usage, 3 no daemon.

/// Same allocator as argusd, so `argus daemon run` counts allocations for
/// `prof.alloc` like the standalone daemon does.
#[global_allocator]
static ALLOC: argus_sim::alloc::Counting = argus_sim::alloc::Counting;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = argus_cli::run(&argv, &mut stdout.lock(), &mut stderr.lock());
    std::process::exit(code);
}
