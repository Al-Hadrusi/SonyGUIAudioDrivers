//! Hardware check for the Bluetooth connection hardening. Run on a machine
//! with a paired headset:
//!
//! ```text
//! cargo run --release --example bt_check            # first paired headset
//! cargo run --release --example bt_check -- <ADDR>  # a specific one
//! ```
//!
//! It connects the way the app does, then checks what the operating system
//! reports about the link: on Linux the peer must be the requested headset
//! and the link must be authenticated (BlueZ security "medium" or better).
//! Finally it sends the protocol's init request and waits for the reply, to
//! show the headset really answers on that channel. Exit code 0 means every
//! check passed; anything else is a failure. Nothing is changed on the
//! headset (only read-only requests are sent).

use std::process::ExitCode;
use std::time::{Duration, Instant};

use headphones_control::protocol::{Notification, Request, Session};
use headphones_control::transport::{self, DeviceInfo};

fn pick_device() -> Result<DeviceInfo, String> {
    let (devices, error) = transport::list_devices(false);
    if let Some(e) = error {
        return Err(e);
    }
    for d in &devices {
        println!("paired: {} ({})", d.name, d.address);
    }
    match std::env::args().nth(1) {
        Some(addr) => devices
            .into_iter()
            .find(|d| d.address.eq_ignore_ascii_case(&addr))
            .ok_or_else(|| format!("{addr} is not a paired Sony headset")),
        None => devices
            .into_iter()
            .next()
            .ok_or_else(|| "no paired Sony headset found".to_string()),
    }
}

#[cfg(target_os = "linux")]
fn connect(device: &DeviceInfo) -> Result<Box<dyn transport::Transport>, String> {
    use headphones_control::transport::bluetooth::BluetoothTransport;
    let t = BluetoothTransport::connect(&device.address).map_err(|e| e.to_string())?;
    let link = t.link_info().map_err(|e| e.to_string())?;
    println!("link: peer {} security {:?}", link.peer, link.security);
    if !link.peer.eq_ignore_ascii_case(&device.address) {
        return Err(format!(
            "FAIL: connected to {} instead of {}",
            link.peer, device.address
        ));
    }
    if !link.is_authenticated() {
        return Err(format!(
            "FAIL: link security is {:?}, expected Medium or higher",
            link.security
        ));
    }
    println!("ok: peer is the chosen headset and the link is authenticated");
    Ok(Box::new(t))
}

#[cfg(not(target_os = "linux"))]
fn connect(device: &DeviceInfo) -> Result<Box<dyn transport::Transport>, String> {
    // Windows: the OS connects only to the selected paired device's service;
    // it does not expose the link security level, so only the protocol
    // check below applies.
    println!("note: link security can only be inspected on Linux");
    transport::open(device).map_err(|e| e.to_string())
}

fn run() -> Result<(), String> {
    let device = pick_device()?;
    println!("connecting to {} ({}) ...", device.name, device.address);
    let mut session = Session::new(connect(&device)?);
    session
        .send(&Request::Init)
        .map_err(|e| format!("FAIL: init: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let updates = session
            .poll(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        if let Some(Notification::InitReply(v)) = updates
            .iter()
            .find(|n| matches!(n, Notification::InitReply(_)))
        {
            println!("ok: headset answered (protocol {v:?})");
            return Ok(());
        }
    }
    Err("FAIL: no init reply from the headset".into())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("PASS");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
