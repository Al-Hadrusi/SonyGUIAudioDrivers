//! Background connection thread. The GUI never blocks on Bluetooth I/O: it
//! sends [`Command`]s and receives [`Event`]s through channels.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::protocol::Session;
use crate::protocol::message::*;
use crate::transport::{self, DeviceInfo, FormFactor};

#[derive(Debug)]
pub enum Command {
    Send(Request),
    Disconnect,
}

#[derive(Debug)]
pub enum Event {
    Connected {
        device: DeviceInfo,
        form: FormFactor,
    },
    Update(Notification),
    /// Connection closed; `None` when the user asked for it.
    Disconnected(Option<String>),
    Warning(String),
}

/// Handle to a running connection.
pub struct Connection {
    tx: Sender<Command>,
    pub events: Receiver<Event>,
}

impl Connection {
    /// Connect in the background. `wake` is called whenever an event is
    /// queued (used to repaint the GUI).
    pub fn spawn(device: DeviceInfo, wake: impl Fn() + Send + 'static) -> Self {
        let (tx, cmd_rx) = mpsc::channel();
        let (ev_tx, events) = mpsc::channel();
        thread::Builder::new()
            .name("headset-connection".into())
            .spawn(move || {
                let reason = run(device, &cmd_rx, &ev_tx, &wake).err();
                let _ = ev_tx.send(Event::Disconnected(reason));
                wake();
            })
            .map(drop)
            .unwrap_or_else(|e| eprintln!("failed to start connection thread: {e}"));
        Self { tx, events }
    }

    pub fn send(&self, request: Request) {
        let _ = self.tx.send(Command::Send(request));
    }

    pub fn disconnect(&self) {
        let _ = self.tx.send(Command::Disconnect);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Disconnect);
    }
}

fn run(
    device: DeviceInfo,
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    wake: &dyn Fn(),
) -> Result<(), String> {
    let transport = transport::open(&device).map_err(|e| e.to_string())?;
    let mut session = Session::new(transport);
    let form = FormFactor::from_name(&device.name);

    session.send(&Request::Init).map_err(|e| e.to_string())?;
    let _ = events.send(Event::Connected { device, form });
    wake();

    let mut initial = vec![
        Request::GetFirmwareVersion,
        Request::GetCodec,
        Request::GetAmbientSound,
        Request::GetEqualizer,
        Request::GetAudioUpsampling,
    ];
    match form {
        FormFactor::Headphones => initial.insert(0, Request::GetBattery(BatteryKind::Single)),
        FormFactor::Earbuds => {
            initial.insert(0, Request::GetBattery(BatteryKind::Case));
            initial.insert(0, Request::GetBattery(BatteryKind::Dual));
        }
    }
    for request in &initial {
        // Not every model supports every query; a missing ACK is not fatal.
        if let Err(e) = session.send(request) {
            if let crate::protocol::SessionError::Io(_) = e {
                return Err(e.to_string());
            }
            let _ = events.send(Event::Warning(e.to_string()));
        }
    }

    let mut warned = 0;
    loop {
        // Coalesce queued commands: while a slider is dragged the GUI may
        // queue many ambient/EQ updates; only the latest of each kind matters.
        let mut batch: Vec<Request> = Vec::new();
        loop {
            match commands.try_recv() {
                Ok(Command::Send(r)) => {
                    batch.retain(|q| std::mem::discriminant(q) != std::mem::discriminant(&r));
                    batch.push(r);
                }
                Ok(Command::Disconnect) | Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => break,
            }
        }
        for r in batch {
            let power_off = r == Request::PowerOff;
            match session.send(&r) {
                Ok(()) if power_off => return Err("Headset powered off".into()),
                Ok(()) => {}
                Err(crate::protocol::SessionError::Io(e)) => return Err(e.to_string()),
                Err(e) => {
                    let _ = events.send(Event::Warning(e.to_string()));
                }
            }
        }

        let updates = session
            .poll(Duration::from_millis(40))
            .map_err(|e| e.to_string())?;
        let woke = !updates.is_empty();
        for n in updates {
            let _ = events.send(Event::Update(n));
        }
        while warned < session.warnings.len() {
            let _ = events.send(Event::Warning(session.warnings[warned].clone()));
            warned += 1;
        }
        if woke {
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn connection_roundtrip_with_simulated_headset() {
        let device = transport::simulator::demo_devices().remove(0);
        let conn = Connection::spawn(device, || {});
        let target = AmbientSound {
            mode: AmbientMode::AmbientSound,
            level: 5,
            focus_on_voice: false,
        };
        let mut connected = false;
        let mut sent = false;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match conn.events.recv_timeout(Duration::from_millis(100)) {
                Ok(Event::Connected { form, .. }) => {
                    assert_eq!(form, FormFactor::Headphones);
                    connected = true;
                }
                Ok(Event::Update(Notification::FirmwareVersion(_))) if !sent => {
                    conn.send(Request::SetAmbientSound {
                        value: target,
                        adjusting: false,
                    });
                    sent = true;
                }
                Ok(Event::Update(Notification::AmbientSound(a))) if sent && a == target => {
                    assert!(connected);
                    return;
                }
                Ok(Event::Disconnected(r)) => panic!("disconnected: {r:?}"),
                _ => {}
            }
        }
        panic!("did not receive the ambient sound update");
    }
}
