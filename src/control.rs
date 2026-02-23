// ---------------------------------------------------------------------------
// Fan/pump curve interpolation
// ---------------------------------------------------------------------------

/// Interpolate a duty cycle (0–100) from a flat profile `[temp, duty, temp, duty, …]`.
///
/// Exact port of Python's `interpolate_curve`:
/// - below-range → clamp to first duty
/// - above-range → clamp to last duty
/// - in-range    → linear interpolation, then `as u8` (truncating, same as Python's `int()`)
pub fn interpolate(profile: &[f64], temp: f64) -> u8 {
    if profile.len() < 4 || profile.len() % 2 != 0 {
        return 50; // Safe fallback for malformed profiles
    }

    let mut points: Vec<(f64, f64)> = profile.chunks(2).map(|p| (p[0], p[1])).collect();
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    if temp <= points[0].0 {
        return points[0].1 as u8;
    }
    if temp >= points[points.len() - 1].0 {
        return points[points.len() - 1].1 as u8;
    }

    for window in points.windows(2) {
        let (temp1, duty1) = window[0];
        let (temp2, duty2) = window[1];
        if temp1 <= temp && temp <= temp2 {
            if (temp2 - temp1).abs() < f64::EPSILON {
                return duty1 as u8;
            }
            let ratio = (temp - temp1) / (temp2 - temp1);
            let duty = duty1 + ratio * (duty2 - duty1);
            return duty.clamp(0.0, 100.0) as u8;
        }
    }

    50 // Should be unreachable
}

// ---------------------------------------------------------------------------
// Exponential smoothing
// ---------------------------------------------------------------------------

/// Single-sensor exponential moving average.
///
/// First call returns the raw value unchanged (no history to blend with).
/// Subsequent calls: `smoothed = alpha * raw + (1 - alpha) * previous`.
pub struct SmoothedSensor {
    last_value: Option<f64>,
}

impl SmoothedSensor {
    pub fn new() -> Self {
        SmoothedSensor { last_value: None }
    }

    pub fn update(&mut self, raw: f64, alpha: f64) -> f64 {
        let smoothed = match self.last_value {
            None => raw,
            Some(prev) => alpha * raw + (1.0 - alpha) * prev,
        };
        self.last_value = Some(smoothed);
        smoothed
    }
}

// ---------------------------------------------------------------------------
// Per-sensor smoothing state
// ---------------------------------------------------------------------------

/// Holds one `SmoothedSensor` per sensor type.
///
/// CPU and GPU use `alpha * 0.5` (extra smoothing for noisy sensors).
/// Coolant and motherboard/NIC use `alpha` directly.
/// This replicates the Python code's inline halving of `smoothing_factor`
/// for the `'cpu'` and `'gpu'` sensor types.
pub struct SmoothingState {
    pub cpu: SmoothedSensor,
    pub gpu: SmoothedSensor,
    pub coolant: SmoothedSensor,
    pub motherboard: SmoothedSensor,
}

impl SmoothingState {
    pub fn new() -> Self {
        SmoothingState {
            cpu: SmoothedSensor::new(),
            gpu: SmoothedSensor::new(),
            coolant: SmoothedSensor::new(),
            motherboard: SmoothedSensor::new(),
        }
    }
}
