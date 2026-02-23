use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Opaque guard type; kept for API compatibility (nothing to drop for a
/// stdout-only subscriber, but callers bind `let _guard = logging::init()`).
pub struct LogGuard;

/// Initialise a tracing subscriber that writes to stdout.
///
/// # Systemd / journald note
/// The service file sets `StandardOutput=journal`, so everything written to
/// stdout is captured and indexed by journald automatically.  A separate file
/// appender at `/var/log/…` would duplicate every log line outside journald's
/// management and rotation — so we deliberately avoid it here.
///
/// Filter defaults to INFO; override with the `RUST_LOG` environment variable.
pub fn init() -> LogGuard {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_ansi(false)      // clean output for journald
                .with_target(false)
                .with_thread_ids(false),
        )
        .init();

    LogGuard
}
