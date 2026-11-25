pub mod app;
pub mod notify;
pub mod theme;
#[cfg(all(feature = "tray", target_os = "linux"))]
pub mod tray;
pub mod widgets;
