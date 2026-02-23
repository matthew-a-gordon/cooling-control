use anyhow::{anyhow, Result};
use crc::{Crc, CRC_16_USB};
use hidapi::{HidApi, HidDevice};
use tracing::debug;

// CRC engine computed entirely at compile time (Crc::new is const fn in crc v3).
const CRC_ENGINE: Crc<u16> = Crc::<u16>::new(&CRC_16_USB);

/// Aquacomputer USB vendor ID.
const AQC_VID: u16 = 0x0C70;

// ---------------------------------------------------------------------------
// Device descriptors
// ---------------------------------------------------------------------------

pub struct DeviceDescriptor {
    pub pid: u16,
    /// Total length of the feature report buffer (including the report-ID byte).
    pub ctrl_report_len: usize,
    /// Slice of (channel-name, base-offset) pairs.
    pub channels: &'static [(&'static str, usize)],
}

pub static QUADRO: DeviceDescriptor = DeviceDescriptor {
    pid: 0xF00D,
    ctrl_report_len: 0x3C1, // 961 bytes
    channels: &[
        ("fan1", 0x36),
        ("fan2", 0x8B),
        ("fan3", 0xE0),
        ("fan4", 0x135),
    ],
};

pub static D5NEXT: DeviceDescriptor = DeviceDescriptor {
    pid: 0xF00E,
    ctrl_report_len: 0x329, // 809 bytes
    channels: &[("fan", 0x41), ("pump", 0x96)],
};

// ---------------------------------------------------------------------------
// Device handle
// ---------------------------------------------------------------------------

pub struct AqcDevice {
    hid: HidDevice,
    desc: &'static DeviceDescriptor,
}

impl AqcDevice {
    /// Open the device described by `desc`.  Returns an error if the device is
    /// not present or cannot be opened (non-fatal; caller stores `Option`).
    pub fn open(api: &HidApi, desc: &'static DeviceDescriptor) -> Result<Self> {
        let hid = api
            .open(AQC_VID, desc.pid)
            .map_err(|e| anyhow!("HID open {:04x}:{:04x} failed: {e}", AQC_VID, desc.pid))?;
        Ok(AqcDevice { hid, desc })
    }

    /// Send a single feature report that sets every channel listed in `speeds`
    /// to fixed-percent mode.  Channels not listed default to `fallback_pct`
    /// (use 100 as a safe "full speed" default for any channel you don't
    /// actively control).
    ///
    /// # Why no GET step
    ///
    /// Aquacomputer devices don't implement `HID_REQ_GET_REPORT` for the
    /// control report (0x03) when the kernel `aquacomputer_d5next` driver is
    /// bound — the hidraw `HIDIOCGFEATURE` ioctl returns EINVAL.  The Python
    /// liquidctl driver side-steps this by using the libusb backend (exclusive
    /// device claim), which we deliberately avoid to coexist with the kernel
    /// driver's sysfs sensor interface.
    ///
    /// Sending a complete report without a prior GET is safe because we supply
    /// ALL channels in every write, so no channel is ever silently left in an
    /// unknown state.
    pub fn set_speeds(&mut self, speeds: &[(&str, u8)], fallback_pct: u8) -> Result<()> {
        let len = self.desc.ctrl_report_len;
        let mut buf = vec![0u8; len];
        buf[0] = 0x03; // Report ID

        // Set every channel the descriptor knows about.
        for (ch_name, base) in self.desc.channels {
            let duty = speeds
                .iter()
                .find(|(name, _)| *name == *ch_name)
                .map(|(_, d)| *d)
                .unwrap_or(fallback_pct);

            buf[*base] = 0x00; // Mode = fixed percent
            let speed_raw = (duty as u16) * 100;
            buf[*base + 1..*base + 3].copy_from_slice(&speed_raw.to_be_bytes());

            debug!("  {ch_name} → {duty}% (raw {speed_raw})");
        }

        // Recompute CRC-16/USB over buf[1..len-2], store BE u16 at buf[len-2..len].
        let crc_val = CRC_ENGINE.checksum(&buf[1..len - 2]);
        buf[len - 2..len].copy_from_slice(&crc_val.to_be_bytes());

        self.hid
            .send_feature_report(&buf)
            .map_err(|e| anyhow!("send_feature_report failed: {e}"))?;

        Ok(())
    }
}
