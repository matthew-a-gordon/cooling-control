use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub monitoring: Monitoring,
    pub fan_curve: FanCurve,
    pub pump_curve: PumpCurve,
    pub hardware: Hardware,
    pub temperature_limits: TemperatureLimits,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Monitoring {
    pub interval: f64,
    pub history_size: usize,
    pub smoothing_factor: f64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FanCurve {
    pub radiator_profile: Vec<f64>,
    pub motherboard_profile: Vec<f64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PumpCurve {
    pub profile: Vec<f64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Hardware {
    pub quadro_device: String,
    pub d5_device: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TemperatureLimits {
    pub cpu_max: f64,
    pub gpu_max: f64,
    pub coolant_max: f64,
    pub motherboard_max: f64,
}

fn default_value() -> serde_json::Value {
    serde_json::json!({
        "monitoring": {
            "interval": 2.0,
            "history_size": 10,
            "smoothing_factor": 0.2
        },
        "fan_curve": {
            "radiator_profile": [20, 20, 30, 40, 35, 60, 40, 80, 45, 100],
            "motherboard_profile": [30, 30, 40, 50, 50, 70, 60, 85, 70, 100]
        },
        "pump_curve": {
            "profile": [30, 5, 40, 25, 50, 60, 60, 85, 70, 100]
        },
        "hardware": {
            "quadro_device": "auto",
            "d5_device": "auto"
        },
        "temperature_limits": {
            "cpu_max": 95.0,
            "gpu_max": 90.0,
            "coolant_max": 50.0,
            "motherboard_max": 80.0
        }
    })
}

/// Load config from `path`, merging missing top-level keys from defaults.
/// Creates the file with defaults if it doesn't exist.
pub fn load(path: &str) -> Result<Config> {
    let config_path = Path::new(path);
    let defaults = default_value();

    if config_path.exists() {
        let content =
            std::fs::read_to_string(config_path).context("Failed to read config file")?;
        let mut value: serde_json::Value =
            serde_json::from_str(&content).context("Failed to parse config JSON")?;

        // Shallow-merge: copy any top-level keys present in defaults but absent in file.
        if let (Some(obj), Some(def_obj)) = (value.as_object_mut(), defaults.as_object()) {
            for (k, v) in def_obj {
                if !obj.contains_key(k) {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        serde_json::from_value(value).context("Config structure mismatch")
    } else {
        // Create default config file so the user can customise it.
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }
        let content =
            serde_json::to_string_pretty(&defaults).context("Failed to serialize defaults")?;
        std::fs::write(config_path, content).context("Failed to write default config")?;
        serde_json::from_value(defaults).context("Failed to deserialize default config")
    }
}
