use tracing::{error, warn};

// ---------------------------------------------------------------------------
// Desktop notifications
// ---------------------------------------------------------------------------

/// Send a desktop notification to every active user session.
///
/// This is called from a root-owned systemd service, so we can't use the
/// ambient `DBUS_SESSION_BUS_ADDRESS`.  Instead we iterate `/run/user/*/bus`
/// — the systemd user-session D-Bus sockets — and invoke `notify-send` once
/// per logged-in user.  Silently succeeds if no notification daemon is
/// running or `notify-send` is not installed.
///
/// `urgency` is passed directly to `notify-send --urgency`: "low" | "normal" | "critical"
pub fn desktop(summary: &str, body: &str, urgency: &str) {
    let Ok(entries) = std::fs::read_dir("/run/user") else {
        return;
    };
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let uid = fname.to_string_lossy();
        // Only process numeric UIDs (skip stray files).
        if !uid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let bus = format!("unix:path=/run/user/{uid}/bus");
        match std::process::Command::new("notify-send")
            .env("DBUS_SESSION_BUS_ADDRESS", &bus)
            .args([
                "--urgency",
                urgency,
                "--app-name",
                "liquidctl-monitor",
                summary,
                body,
            ])
            .status()
        {
            Ok(s) if !s.success() => warn!("notify-send exited {s} for uid {uid}"),
            Err(e) => warn!("notify-send unavailable: {e}"),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Temperature thresholds
// ---------------------------------------------------------------------------

pub enum TempStatus {
    Ok,
    /// >= 90 % of limit — issue a warning notification
    Warning { sensor: &'static str, temp: f64, limit: f64 },
    /// >= limit — trigger emergency response
    Critical { sensor: &'static str, temp: f64, limit: f64 },
}

/// Classify a temperature reading against its configured limit.
pub fn check(sensor: &'static str, temp: f64, limit: f64) -> TempStatus {
    if temp >= limit {
        TempStatus::Critical { sensor, temp, limit }
    } else if temp >= limit * 0.9 {
        TempStatus::Warning { sensor, temp, limit }
    } else {
        TempStatus::Ok
    }
}

// ---------------------------------------------------------------------------
// Emergency shutdown
// ---------------------------------------------------------------------------

/// Set a reason string, log it at ERROR, send a critical notification, then
/// issue an immediate `systemctl poweroff`.  This function does not return
/// under normal circumstances.
pub fn emergency_shutdown(reason: &str) {
    error!("EMERGENCY SHUTDOWN — {reason}");
    desktop(
        "liquidctl-monitor: EMERGENCY SHUTDOWN",
        reason,
        "critical",
    );
    let _ = std::process::Command::new("systemctl")
        .args(["poweroff", "--no-wall"])
        .status();
    // Give the OS a moment to process the poweroff request; loop as fallback.
    std::thread::sleep(std::time::Duration::from_secs(5));
    error!("systemctl poweroff did not terminate the process — killing");
    std::process::exit(1);
}
