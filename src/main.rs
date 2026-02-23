mod aquacomputer;
mod config;
mod control;
mod logging;
mod notify;
mod sensors;
mod signal;

use aquacomputer::{AqcDevice, D5NEXT, QUADRO};
use hidapi::HidApi;
use notify::TempStatus;
use std::{
    sync::atomic::Ordering,
    thread,
    time::Duration,
};
use tracing::{error, info, warn};

fn main() {
    let _log_guard = logging::init();

    let config = config::load("/etc/cooling-control/config.json").unwrap_or_else(|e| {
        warn!("Config load error: {e} — using defaults");
        serde_json::from_value(serde_json::json!({
            "monitoring":          { "interval": 2.0, "history_size": 10, "smoothing_factor": 0.2 },
            "fan_curve":           { "radiator_profile":    [20,20,30,40,35,60,40,80,45,100],
                                     "motherboard_profile": [30,30,40,50,50,70,60,85,70,100] },
            "pump_curve":          { "profile": [30,5,40,25,50,60,60,85,70,100] },
            "hardware":            { "quadro_device": "auto", "d5_device": "auto" },
            "temperature_limits":  { "cpu_max":95.0, "gpu_max":90.0,
                                     "coolant_max":50.0, "motherboard_max":80.0 }
        }))
        .expect("hardcoded defaults must always deserialise")
    });

    let running = signal::install();
    let sensor_paths = sensors::SensorPaths::discover();
    let nvml_ctx = sensors::NvmlContext::init();

    let api_opt: Option<HidApi> = match HidApi::new() {
        Ok(api) => Some(api),
        Err(e) => {
            error!("HID API init failed: {e} — hardware control disabled");
            None
        }
    };

    let mut quadro: Option<AqcDevice> = None;
    let mut d5next: Option<AqcDevice> = None;

    if let Some(ref api) = api_opt {
        match AqcDevice::open(api, &QUADRO) {
            Ok(dev) => { info!("Aquacomputer Quadro opened"); quadro = Some(dev); }
            Err(e)  => warn!("{e} — radiator and NIC fan control disabled"),
        }
        match AqcDevice::open(api, &D5NEXT) {
            Ok(dev) => { info!("Aquacomputer D5 Next opened"); d5next = Some(dev); }
            Err(e)  => warn!("{e} — pump control disabled"),
        }
    }

    let mut smoothing = control::SmoothingState::new();
    let alpha  = config.monitoring.smoothing_factor;
    let limits = &config.temperature_limits;

    info!("Starting temperature monitoring");

    while running.load(Ordering::Relaxed) {
        // ── Sensor reads ─────────────────────────────────────────────────────
        let raw_cpu     = sensors::read_cpu(&sensor_paths);
        let raw_gpu     = nvml_ctx.as_ref().and_then(|n| n.gpu_temp());
        let raw_coolant = sensors::read_coolant(&sensor_paths);
        let raw_nic     = sensors::read_nic(&sensor_paths);

        // ── Smoothing ────────────────────────────────────────────────────────
        // CPU/GPU use alpha*0.5 (extra damping for noisy sensors).
        let cpu     = raw_cpu    .map(|t| smoothing.cpu        .update(t, alpha * 0.5));
        let gpu     = raw_gpu    .map(|t| smoothing.gpu        .update(t, alpha * 0.5));
        let coolant = raw_coolant.map(|t| smoothing.coolant    .update(t, alpha));
        let nic     = raw_nic    .map(|t| smoothing.motherboard.update(t, alpha));

        // ── Log ──────────────────────────────────────────────────────────────
        info!(
            "Temps - CPU: {}, GPU: {}, Coolant: {}, MB: {}",
            fmt_temp(cpu), fmt_temp(gpu), fmt_temp(coolant), fmt_temp(nic),
        );

        // ── Fault / missing-sensor safeguard ─────────────────────────────────
        // If any primary control sensor is unavailable we can't make an
        // informed speed decision.  Log it and fall through: the control
        // sections below substitute 100% for every None input, ensuring fans
        // and pump run at full speed rather than staying at a stale value.
        for (sensor, val, impact) in [
            ("CPU",         cpu,     "fans/pump defaulting to 100%"),
            ("GPU",         gpu,     "pump defaulting to 100%"),
            ("Coolant",     coolant, "radiator fans defaulting to 100%"),
            ("Motherboard", nic,     "motherboard fan defaulting to 100%"),
        ] {
            if val.is_none() {
                let msg = format!("{sensor} temperature unavailable — {impact}");
                warn!("{msg}");
                notify::desktop("Sensor fault: liquidctl-monitor", &msg, "critical");
            }
        }

        // ── Temperature threshold checks ──────────────────────────────────────
        let checks: &[(&'static str, Option<f64>, f64)] = &[
            ("CPU",         cpu,     limits.cpu_max),
            ("GPU",         gpu,     limits.gpu_max),
            ("Coolant",     coolant, limits.coolant_max),
            ("Motherboard", nic,     limits.motherboard_max),
        ];

        let mut critical: Option<String> = None;

        for &(sensor, temp_opt, limit) in checks {
            let Some(temp) = temp_opt else { continue };
            match notify::check(sensor, temp, limit) {
                TempStatus::Ok => {}

                TempStatus::Warning { sensor, temp, limit } => {
                    let msg = format!(
                        "{sensor} temperature {temp:.1}°C is approaching limit {limit:.1}°C \
                         ({:.0}%)",
                        temp / limit * 100.0
                    );
                    warn!("{msg}");
                    notify::desktop(
                        &format!("Temperature warning: {sensor}"),
                        &msg,
                        "critical",
                    );
                }

                TempStatus::Critical { sensor, temp, limit } => {
                    // Record the first critical breach; we'll handle it after
                    // this loop so all warnings are still emitted first.
                    critical.get_or_insert_with(|| {
                        format!(
                            "{sensor} temperature {temp:.1}°C has reached limit {limit:.1}°C"
                        )
                    });
                }
            }
        }

        // If any sensor is critical: ramp everything to 100%, then shut down.
        if let Some(ref reason) = critical {
            error!("Critical temperature limit reached — setting all cooling to 100% and powering off");
            if let Some(ref mut q) = quadro {
                // Empty speeds slice → every channel gets the fallback (100%).
                let _ = q.set_speeds(&[], 100);
            }
            if let Some(ref mut d) = d5next {
                let _ = d.set_speeds(&[], 100);
            }
            thread::sleep(Duration::from_millis(400)); // let USB writes land
            notify::emergency_shutdown(reason);
            break; // reached only if poweroff is slow
        }

        // ── Quadro: radiator fans (1+2), motherboard fan (3), fan4 default ───
        if let Some(ref mut q) = quadro {
            let fan1_2 = coolant
                .map(|c| control::interpolate(&config.fan_curve.radiator_profile, c))
                .unwrap_or(100);
            let fan3 = nic
                .map(|n| control::interpolate(&config.fan_curve.motherboard_profile, n))
                .unwrap_or(100);

            match q.set_speeds(&[("fan1", fan1_2), ("fan2", fan1_2), ("fan3", fan3)], 100) {
                Ok(()) => {
                    if coolant.is_some() {
                        info!("Set radiator fans (1+2) to {}% for {}", fan1_2, fmt_temp(coolant));
                    }
                    if nic.is_some() {
                        info!("Set motherboard fan (3) to {}% for {}", fan3, fmt_temp(nic));
                    }
                }
                Err(e) => warn!("Quadro set_speeds: {e}"),
            }

            thread::sleep(Duration::from_millis(200));
        }

        // ── D5 Next: pump (max CPU/GPU temp), D5 fan default ─────────────────
        if let Some(ref mut d) = d5next {
            let pump = match (cpu, gpu) {
                (Some(c), Some(g)) => {
                    let max_temp = c.max(g);
                    let speed = control::interpolate(&config.pump_curve.profile, max_temp);
                    info!(
                        "Set pump to {}% for max temp {:.1}°C (CPU: {}, GPU: {})",
                        speed, max_temp, fmt_temp(cpu), fmt_temp(gpu)
                    );
                    speed
                }
                _ => 100,
            };

            if let Err(e) = d.set_speeds(&[("pump", pump)], 100) {
                warn!("D5 Next set_speeds: {e}");
            }
        }

        thread::sleep(Duration::from_secs_f64(config.monitoring.interval));
    }

    info!("Shutdown complete");
}

fn fmt_temp(t: Option<f64>) -> String {
    match t {
        Some(v) => format!("{v:.1}°C"),
        None    => "N/A".to_string(),
    }
}
