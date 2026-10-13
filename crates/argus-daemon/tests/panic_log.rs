//! A panic in the daemon goes to its log with the thread and a backtrace
//! (issue 10: a daemon left mid-test with nothing in its log). Its own test
//! file, since a panic hook is the whole process's.

mod common;

#[test]
fn a_panic_on_any_thread_is_written_to_the_daemon_log() {
    let t = common::temp("panic-log");
    std::fs::create_dir_all(t.path().join(".argus")).unwrap();
    argus_daemon::launch::log_panics(t.path());
    let worker = std::thread::Builder::new()
        .name("the-worker".to_string())
        .spawn(|| panic!("the test's own panic"))
        .unwrap();
    assert!(worker.join().is_err());
    let _ = std::panic::take_hook();
    let log = std::fs::read_to_string(t.path().join(".argus/daemon.log")).unwrap();
    assert!(log.contains("argusd: panicked (thread the-worker"), "{log}");
    assert!(log.contains("the test's own panic"), "{log}");
    assert!(log.contains("backtrace:"), "{log}");
    assert!(
        log.contains("panic_log"),
        "the backtrace names this test: {log}"
    );
}
