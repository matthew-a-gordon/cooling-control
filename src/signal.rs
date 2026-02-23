use signal_hook::{
    consts::signal::{SIGINT, SIGTERM},
    iterator::Signals,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Spawn a background thread that watches for SIGTERM/SIGINT and flips the
/// returned flag to `false`.  The main loop polls this flag each iteration.
pub fn install() -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    std::thread::spawn(move || {
        let mut sigs = Signals::new([SIGTERM, SIGINT]).expect("failed to register signals");
        for _ in sigs.forever() {
            r.store(false, Ordering::SeqCst);
            break;
        }
    });
    running
}
