//! Small persistent settings file (`~/.config/headphones-control/settings`).
//!
//! The format is plain `key=value` lines so it can be read and edited by hand.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::transport::DeviceInfo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The headset the user connected to last (never a demo device).
    pub last_device: Option<DeviceInfo>,
    /// Connect to `last_device` automatically when it shows up.
    pub auto_connect: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            last_device: None,
            auto_connect: true,
        }
    }
}

impl Config {
    /// Default location: `$XDG_CONFIG_HOME/headphones-control/settings`, or
    /// `~/.config/headphones-control/settings`.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("headphones-control").join("settings"))
    }

    /// Load from the default location; missing or unreadable files give the defaults.
    pub fn load() -> Self {
        Self::default_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .map(|s| Self::parse(&s))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::default_path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?;
        self.save_to(&path)
    }

    /// Write atomically (temporary file + rename).
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, self.serialize())?;
        fs::rename(&tmp, path)
    }

    pub fn parse(text: &str) -> Self {
        let mut cfg = Self::default();
        let (mut address, mut name) = (None, None);
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().to_string();
            match key.trim() {
                "last_address" if !value.is_empty() => address = Some(value),
                "last_name" if !value.is_empty() => name = Some(value),
                "auto_connect" => cfg.auto_connect = value != "false",
                _ => {}
            }
        }
        if let Some(address) = address {
            let name = name.unwrap_or_else(|| address.clone());
            cfg.last_device = Some(DeviceInfo {
                address,
                name,
                connected: false,
            });
        }
        cfg
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("# Headphones Control settings\n");
        if let Some(d) = &self.last_device {
            // Names come from Bluetooth; keep them on one line.
            let name: String = d.name.chars().filter(|c| !c.is_control()).collect();
            out.push_str(&format!("last_address={}\nlast_name={}\n", d.address, name));
        }
        out.push_str(&format!("auto_connect={}\n", self.auto_connect));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cfg = Config {
            last_device: Some(DeviceInfo {
                address: "AA:BB:CC:DD:EE:FF".into(),
                name: "WH-1000XM6".into(),
                connected: false,
            }),
            auto_connect: false,
        };
        assert_eq!(Config::parse(&cfg.serialize()), cfg);
        assert_eq!(
            Config::parse(&Config::default().serialize()),
            Config::default()
        );
    }

    #[test]
    fn tolerates_garbage_and_newlines_in_names() {
        assert_eq!(
            Config::parse("nonsense\n=\nauto_connect"),
            Config::default()
        );
        let cfg = Config {
            last_device: Some(DeviceInfo {
                address: "A".into(),
                name: "evil\nauto_connect=false".into(),
                connected: false,
            }),
            auto_connect: true,
        };
        let parsed = Config::parse(&cfg.serialize());
        assert!(parsed.auto_connect);
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("hpc-test-{}", std::process::id()));
        let path = dir.join("settings");
        let cfg = Config {
            last_device: None,
            auto_connect: false,
        };
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), cfg);
        let _ = fs::remove_dir_all(&dir);
    }
}
