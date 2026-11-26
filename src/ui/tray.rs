//! System tray icon (freedesktop StatusNotifierItem, shown by KDE, XFCE,
//! GNOME with the AppIndicator extension, waybar, ...).
//!
//! The tray runs on its own thread. It sends [`TrayCommand`]s to the app and
//! receives a [`TrayStatus`] snapshot whenever the headset state changes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use headphones_control::protocol::AmbientMode;
use ksni::blocking::TrayMethods;
use ksni::menu::{RadioGroup, RadioItem, StandardItem};
use ksni::{Icon, MenuItem, OfflineReason, ToolTip};

/// What the tray shows about the connected headset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayStatus {
    pub name: String,
    pub battery: Option<u8>,
    pub mode: AmbientMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ShowWindow,
    SetAmbient(AmbientMode),
    Quit,
}

const MODES: [(AmbientMode, &str); 3] = [
    (AmbientMode::NoiseCancelling, "Noise Canceling"),
    (AmbientMode::AmbientSound, "Ambient Sound"),
    (AmbientMode::Off, "Off"),
];

struct TrayModel {
    status: Option<TrayStatus>,
    tx: Sender<TrayCommand>,
    ctx: egui::Context,
    icon: Vec<Icon>,
    /// The plain app icon, used to redraw `icon` with a battery bar.
    base_icon: Vec<Icon>,
    available: Arc<AtomicBool>,
}

impl TrayModel {
    fn send(&self, cmd: TrayCommand) {
        let _ = self.tx.send(cmd);
        // Wake the app even while its window is hidden.
        self.ctx.request_repaint();
    }

    fn summary(&self) -> String {
        match &self.status {
            None => "Not connected".to_string(),
            Some(s) => {
                let mode = MODES
                    .iter()
                    .find(|(m, _)| *m == s.mode)
                    .map_or("Noise Canceling", |(_, l)| *l);
                match s.battery {
                    Some(b) => format!("{} · {b}% · {mode}", s.name),
                    None => format!("{} · {mode}", s.name),
                }
            }
        }
    }
}

impl ksni::Tray for TrayModel {
    fn id(&self) -> String {
        "headphones-control".into()
    }

    fn title(&self) -> String {
        "Headphones Control".into()
    }

    // No `icon_name`: Plasma prefers a theme icon name over the pixmap and
    // shows nothing when the name isn't installed (e.g. with `cargo run`).
    // The pixmap always works.

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Headphones Control".into(),
            // Tooltip descriptions may be rendered as markup; the device
            // name comes from Bluetooth.
            description: headphones_control::text::escape_markup(&self.summary()),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayCommand::ShowWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items = vec![
            StandardItem {
                label: self.summary(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ];
        if let Some(status) = &self.status {
            // Wind noise reduction counts as noise cancelling here.
            let mode = if status.mode == AmbientMode::WindReduction {
                AmbientMode::NoiseCancelling
            } else {
                status.mode
            };
            items.push(
                RadioGroup {
                    selected: MODES.iter().position(|(m, _)| *m == mode).unwrap_or(0),
                    select: Box::new(|tray: &mut Self, index| {
                        if let Some((mode, _)) = MODES.get(index) {
                            tray.send(TrayCommand::SetAmbient(*mode));
                        }
                    }),
                    options: MODES
                        .iter()
                        .map(|(_, label)| RadioItem {
                            label: (*label).into(),
                            ..Default::default()
                        })
                        .collect(),
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }
        items.push(
            StandardItem {
                label: "Open Headphones Control".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::ShowWindow)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        );
        items
    }

    fn watcher_online(&self) {
        self.available.store(true, Ordering::SeqCst);
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        self.available.store(false, Ordering::SeqCst);
        // Keep running: the tray host may come back (e.g. shell restart).
        true
    }
}

/// Handle owned by the app.
pub struct Tray {
    handle: ksni::blocking::Handle<TrayModel>,
    pub commands: Receiver<TrayCommand>,
    available: Arc<AtomicBool>,
    last: Option<TrayStatus>,
}

impl Tray {
    /// Start the tray. Fails when there is no session D-Bus or no tray host
    /// (the app then works without it).
    pub fn spawn(ctx: &egui::Context) -> Result<Self, String> {
        let (tx, commands) = mpsc::channel();
        // `spawn` only succeeds once a tray host (StatusNotifierWatcher) has
        // accepted the icon, so it starts out visible. `watcher_offline` /
        // `watcher_online` keep this up to date afterwards.
        let available = Arc::new(AtomicBool::new(true));
        let model = TrayModel {
            status: None,
            tx,
            ctx: ctx.clone(),
            icon: tray_icon(),
            base_icon: tray_icon(),
            available: Arc::clone(&available),
        };
        match model.spawn() {
            Ok(handle) => Ok(Self {
                handle,
                commands,
                available,
                last: None,
            }),
            Err(e) => {
                eprintln!("System tray unavailable: {e}");
                Err(format!("System tray unavailable: {e}"))
            }
        }
    }

    /// Whether a tray host is currently showing the icon (only then is it
    /// safe to hide the window instead of quitting).
    pub fn is_visible(&self) -> bool {
        self.available.load(Ordering::SeqCst) && !self.handle.is_closed()
    }

    pub fn set_status(&mut self, status: Option<TrayStatus>) {
        if self.last == status {
            return;
        }
        self.last.clone_from(&status);
        self.handle.update(move |m| {
            let battery = status.as_ref().and_then(|s| s.battery);
            m.icon = m
                .base_icon
                .iter()
                .map(|i| with_battery_bar(i, battery))
                .collect();
            m.status = status;
        });
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // Remove the icon from the panel when the app exits.
        let _ = self.handle.shutdown();
    }
}

/// The app icon in the ARGB32 (network byte order) format of the tray spec.
/// Draw a small battery bar along the bottom of the icon (green, then orange
/// below 30 %, red below 15 %).
fn with_battery_bar(icon: &Icon, level: Option<u8>) -> Icon {
    let mut out = Icon {
        width: icon.width,
        height: icon.height,
        data: icon.data.clone(),
    };
    let Some(level) = level else { return out };
    let (Ok(w), Ok(h)) = (usize::try_from(icon.width), usize::try_from(icon.height)) else {
        return out;
    };
    if w < 16 || h < 16 || out.data.len() != w * h * 4 {
        return out;
    }
    let (x0, x1) = (w / 10, w - w / 10);
    let (y0, y1) = (h - h / 6, h - h / 32);
    let filled = x0 + (x1 - x0) * usize::from(level.min(100)) / 100;
    let color: [u8; 4] = if level < 15 {
        [0xFF, 0xFF, 0x45, 0x3A]
    } else if level < 30 {
        [0xFF, 0xF5, 0xA3, 0x1B]
    } else {
        [0xFF, 0x4C, 0xD9, 0x64]
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let px = if x < filled {
                color
            } else {
                [0xFF, 0x30, 0x30, 0x35]
            };
            let i = (y * w + x) * 4;
            if let Some(dst) = out.data.get_mut(i..i + 4) {
                dst.copy_from_slice(&px);
            }
        }
    }
    out
}

fn tray_icon() -> Vec<Icon> {
    let Ok(img) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../assets/headphones-control-64.png"))
    else {
        return Vec::new();
    };
    let data = img
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect();
    let (Ok(width), Ok(height)) = (i32::try_from(img.width), i32::try_from(img.height)) else {
        return Vec::new();
    };
    vec![Icon {
        width,
        height,
        data,
    }]
}
