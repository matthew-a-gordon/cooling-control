use std::path::{Path, PathBuf};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Sensor path discovery (run once at startup)
// ---------------------------------------------------------------------------

pub struct SensorPaths {
    /// Directory of the k10temp hwmon entry (AMD CPU die temperatures).
    pub k10temp_dir: Option<PathBuf>,
    /// Path to the D5 Next coolant temperature input file.
    pub d5next_coolant: Option<PathBuf>,
    /// Directory of the NIC hwmon entry (PHY + MAC temperatures).
    pub nic_dir: Option<PathBuf>,
}

impl SensorPaths {
    pub fn discover() -> Self {
        let k10temp_dir = hwmon_dir_by_name("k10temp");
        if k10temp_dir.is_some() {
            debug!("k10temp hwmon found: {:?}", k10temp_dir);
        } else {
            warn!("k10temp hwmon not found — CPU temperature monitoring disabled");
        }

        let d5next_coolant = hwmon_dir_by_name("d5next").map(|d| d.join("temp1_input"));
        if d5next_coolant.is_some() {
            debug!("d5next hwmon found");
        } else {
            warn!("d5next hwmon not found — coolant temperature monitoring disabled");
        }

        let nic_dir = hwmon_dir_with_label_containing(&["PHY", "MAC"]);
        if nic_dir.is_some() {
            debug!("NIC hwmon found: {:?}", nic_dir);
        } else {
            warn!("NIC hwmon not found — motherboard fan control disabled");
        }

        SensorPaths {
            k10temp_dir,
            d5next_coolant,
            nic_dir,
        }
    }
}

/// Find the first hwmon directory whose `name` file equals `target`.
fn hwmon_dir_by_name(target: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir("/sys/class/hwmon").ok()? {
        let dir = entry.ok()?.path();
        if let Ok(n) = std::fs::read_to_string(dir.join("name")) {
            if n.trim() == target {
                return Some(dir);
            }
        }
    }
    None
}

/// Find the first hwmon directory that has a `temp*_label` file whose content
/// contains any of `needles` (case-sensitive).
fn hwmon_dir_with_label_containing(needles: &[&str]) -> Option<PathBuf> {
    for entry in std::fs::read_dir("/sys/class/hwmon").ok()? {
        let dir = entry.ok()?.path();
        if let Ok(sub_entries) = std::fs::read_dir(&dir) {
            for sub in sub_entries.flatten() {
                let fname = sub.file_name();
                let fname = fname.to_string_lossy();
                if fname.starts_with("temp") && fname.ends_with("_label") {
                    if let Ok(label) = std::fs::read_to_string(sub.path()) {
                        if needles.iter().any(|n| label.contains(n)) {
                            return Some(dir);
                        }
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Sensor read functions
// ---------------------------------------------------------------------------

/// Read max Tccd die temperature from k10temp hwmon.
/// Filters to 20–100°C to discard stale or erroneous readings.
pub fn read_cpu(paths: &SensorPaths) -> Option<f64> {
    let dir = paths.k10temp_dir.as_ref()?;
    let mut tccd_temps: Vec<f64> = Vec::new();

    let entries = std::fs::read_dir(dir)
        .map_err(|e| warn!("k10temp read_dir error: {e}"))
        .ok()?;

    for entry in entries.flatten() {
        let fname = entry.file_name();
        let fname = fname.to_string_lossy();
        if !fname.starts_with("temp") || !fname.ends_with("_label") {
            continue;
        }
        let label_path = entry.path();
        let label = match std::fs::read_to_string(&label_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !label.trim().starts_with("Tccd") {
            continue;
        }
        // Derive the corresponding _input path by replacing "_label" with "_input".
        let input_path = PathBuf::from(
            label_path
                .to_string_lossy()
                .replace("_label", "_input"),
        );
        if let Ok(raw) = std::fs::read_to_string(&input_path) {
            if let Ok(millic) = raw.trim().parse::<f64>() {
                let temp = millic / 1000.0;
                if (20.0..=100.0).contains(&temp) {
                    tccd_temps.push(temp);
                }
            }
        }
    }

    if tccd_temps.is_empty() {
        warn!("No valid Tccd readings from k10temp");
        return None;
    }
    tccd_temps.into_iter().reduce(f64::max)
}

/// Read coolant temperature from the D5 Next kernel driver (temp1_input).
pub fn read_coolant(paths: &SensorPaths) -> Option<f64> {
    let path = paths.d5next_coolant.as_ref()?;
    read_millic(path)
}

/// Read NIC PHY + MAC temperatures and return the maximum.
pub fn read_nic(paths: &SensorPaths) -> Option<f64> {
    let dir = paths.nic_dir.as_ref()?;
    let t1 = read_millic(&dir.join("temp1_input"));
    let t2 = read_millic(&dir.join("temp2_input"));
    match (t1, t2) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => {
            warn!("Could not read NIC temperature");
            None
        }
    }
}

/// Read a sysfs millidegree Celsius input file and convert to °C.
fn read_millic(path: &Path) -> Option<f64> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| warn!("sysfs read {:?}: {e}", path))
        .ok()?;
    raw.trim()
        .parse::<f64>()
        .map(|v| v / 1000.0)
        .map_err(|e| warn!("sysfs parse {:?}: {e}", path))
        .ok()
}

// ---------------------------------------------------------------------------
// NVML context for GPU temperature
// ---------------------------------------------------------------------------

pub struct NvmlContext {
    nvml: nvml_wrapper::Nvml,
}

impl NvmlContext {
    /// Initialise NVML. Returns `None` if the NVIDIA driver is not present.
    pub fn init() -> Option<Self> {
        match nvml_wrapper::Nvml::init() {
            Ok(nvml) => Some(NvmlContext { nvml }),
            Err(e) => {
                warn!("NVML init failed: {e} — GPU temperature monitoring disabled");
                None
            }
        }
    }

    /// Query GPU 0 temperature. Cheap per-call handle lookup; avoids lifetime issues.
    pub fn gpu_temp(&self) -> Option<f64> {
        use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
        let device = self
            .nvml
            .device_by_index(0)
            .map_err(|e| warn!("NVML device_by_index: {e}"))
            .ok()?;
        device
            .temperature(TemperatureSensor::Gpu)
            .map(|t| t as f64)
            .map_err(|e| warn!("NVML temperature: {e}"))
            .ok()
    }
}
