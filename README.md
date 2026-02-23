# cooling-control

> [!WARNING]
> This controls your PC's cooling. Take personal accountability for understanding what the software does and whether you're comfortable trusting your PC to it. I think it has reasonable safety features, but use at your own risk.

A single-binary Rust daemon that monitors CPU, GPU, coolant, and NIC temperatures
and controls an Aquacomputer Quadro fan controller and D5 Next pump/reservoir.

## How it works

| Layer | Role |
|---|---|
| **sysfs / k10temp** | Read CPU Tccd die temperatures |
| **sysfs / d5next** | Read coolant temperature (kernel hwmon driver) |
| **sysfs / NIC hwmon** | Read PHY + MAC temperatures |
| **NVML** | Read NVIDIA GPU temperature |
| **hidraw (HID feature reports)** | Write fan/pump speed commands to devices |

All sensor reads go through the kernel's hwmon interface — no libusb, no
exclusive device claims.  The daemon coexists peacefully with the
`aquacomputer_d5next` kernel driver.

## Hardware

- Aquacomputer Quadro — controls fan1 + fan2 (radiator), fan3 (NIC cooling)
- Aquacomputer D5 Next — controls pump speed, provides coolant temperature
- NVIDIA GPU — temperature read via NVML (optional; skipped gracefully if absent)

## Installation

```bash
# Build dependencies (Fedora/RHEL)
sudo dnf install systemd-devel

# Build and install
sudo ./install.sh
```

The script builds the Rust binary, copies it to `/opt/cooling-control/`,
installs the systemd unit, and creates a default config if one doesn't exist.

## Configuration

`/etc/cooling-control/config.json` — edited live, read at startup.

```jsonc
{
    "monitoring": {
        "interval": 2.0,         // seconds between control cycles
        "smoothing_factor": 0.2  // EMA alpha; CPU/GPU get ×0.5 for extra damping
    },
    "fan_curve": {
        // [temp_°C, duty_%, temp_°C, duty_%, …] — linearly interpolated
        "radiator_profile":    [20, 10, 30, 25, 35, 50, 40, 80, 45, 100],
        "motherboard_profile": [50, 25, 60, 50, 70, 100]
    },
    "pump_curve": {
        "profile": [30, 5, 40, 10, 50, 25, 60, 65, 70, 100]
    },
    "temperature_limits": {
        // At 90% of a limit: log WARN + desktop notification (urgency=normal)
        // At 100% of a limit: ramp all fans/pump to 100%, notify (urgency=critical), then poweroff
        "cpu_max": 95.0,
        "gpu_max": 90.0,
        "coolant_max": 50.0,
        "motherboard_max": 80.0
    }
}
```

### Control routing

| Fan/pump | Controlled by | Device |
|---|---|---|
| fan1 + fan2 | coolant temp | Quadro |
| fan3 | NIC (PHY/MAC) temp | Quadro |
| fan4 | not in config → 100% | Quadro |
| pump | max(CPU, GPU) temp | D5 Next |
| D5 fan | not in config → 100% | D5 Next |

If a temperature source is unavailable, its channel(s) fall back to 100% duty
(more cooling, never less).

## Service management

```bash
# View live logs
journalctl -u cooling-control -f

# Status
systemctl status cooling-control

# Restart after config change
systemctl restart cooling-control
```

### Debug / verbose logging

```bash
sudo RUST_LOG=debug /opt/cooling-control/cooling-control
```

## Manual test run

```bash
sudo ./target/release/cooling-control
```

Verify fan speeds respond by reading sysfs before and after startup:

```bash
# Quadro: fan1-4 PWM values (0–255)
cat /sys/class/hwmon/hwmon5/pwm{1,2,3,4}

# D5 Next: pump PWM value
cat /sys/class/hwmon/hwmon4/pwm1
```

## Uninstall

```bash
sudo systemctl stop cooling-control
sudo systemctl disable cooling-control
sudo rm /etc/systemd/system/cooling-control.service
sudo rm -rf /opt/cooling-control
sudo systemctl daemon-reload
```

Config (`/etc/cooling-control/`) and any logs are left in place.
