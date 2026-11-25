//! Byte transports the protocol session can run over.

use std::io;
use std::time::Duration;

#[cfg(all(feature = "bluetooth", target_os = "linux"))]
pub mod bluetooth;
/// Windows: the same interface on top of WinRT.
#[cfg(all(feature = "bluetooth", windows))]
#[path = "windows_bt.rs"]
pub mod bluetooth;
pub mod simulator;

/// A bidirectional byte stream to a headset.
pub trait Transport: Send {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Read available bytes, waiting at most `timeout`. Returns `Ok(0)` on
    /// timeout; a closed connection is reported as an error.
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize>;
}

/// What kind of product we are talking to (drives battery display and the
/// illustration in the UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFactor {
    Headphones,
    Earbuds,
}

impl FormFactor {
    /// Guess from the advertised product name (e.g. "WH-1000XM5", "WF-1000XM4").
    pub fn from_name(name: &str) -> Self {
        let n = name.to_ascii_uppercase();
        if n.starts_with("WF-") || n.contains("LINKBUDS") || n.starts_with("LE_WF") {
            FormFactor::Earbuds
        } else {
            FormFactor::Headphones
        }
    }
}

/// A device the user can pick in the device list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Bluetooth address, or `"demo:<model>"` for simulated devices.
    pub address: String,
    pub name: String,
    pub connected: bool,
}

impl DeviceInfo {
    pub fn is_demo(&self) -> bool {
        self.address.starts_with("demo:")
    }
}

/// Devices currently available: paired Bluetooth headsets (when Bluetooth
/// support is compiled in), plus the simulated demo devices if `include_demo`.
pub fn list_devices(include_demo: bool) -> (Vec<DeviceInfo>, Option<String>) {
    let mut devices = Vec::new();
    #[cfg(all(feature = "bluetooth", any(target_os = "linux", windows)))]
    let error = match bluetooth::list_paired() {
        Ok(found) => {
            devices.extend(found);
            None
        }
        Err(e) => {
            let detail = e.to_string();
            Some(if detail.contains("D-Bus") || detail.contains("dbus") {
                "Bluetooth service (BlueZ) is not running or not reachable.".to_string()
            } else {
                format!("Bluetooth unavailable: {detail}")
            })
        }
    };
    #[cfg(not(all(feature = "bluetooth", any(target_os = "linux", windows))))]
    let error = Some("This build has no Bluetooth support (demo devices only).".to_string());
    if include_demo {
        devices.extend(simulator::demo_devices());
    }
    (devices, error)
}

/// Open a transport to the given device.
pub fn open(device: &DeviceInfo) -> io::Result<Box<dyn Transport>> {
    if device.is_demo() {
        return Ok(Box::new(simulator::SimulatedHeadset::new(&device.name)));
    }
    #[cfg(all(feature = "bluetooth", any(target_os = "linux", windows)))]
    {
        Ok(Box::new(bluetooth::BluetoothTransport::connect(
            &device.address,
        )?))
    }
    #[cfg(not(all(feature = "bluetooth", any(target_os = "linux", windows))))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Bluetooth support is not available in this build",
        ))
    }
}
