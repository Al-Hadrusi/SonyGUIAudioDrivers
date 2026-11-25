//! Background connection thread. The GUI never blocks on Bluetooth I/O: it
//! sends [`Command`]s and receives [`Event`]s through channels.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::message::*;
use crate::protocol::{Session, SessionError};
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
        dialect: Dialect,
    },
    /// The headset revealed more about its protocol (e.g. wind noise support).
    DialectChanged(Dialect),
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

fn forward(
    session: &mut Session,
    events: &Sender<Event>,
    wait: Duration,
) -> Result<Vec<Notification>, String> {
    let updates = session.poll(wait).map_err(|e| e.to_string())?;
    for n in &updates {
        let _ = events.send(Event::Update(n.clone()));
    }
    Ok(updates)
}

/// Send a request, treating "unsupported" and "no ACK" as non-fatal.
fn try_send(session: &mut Session, request: &Request) -> Result<bool, String> {
    match session.send(request) {
        Ok(()) => Ok(true),
        Err(SessionError::Io(e)) => Err(e.to_string()),
        Err(_) => Ok(false),
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

    match session.send(&Request::Init) {
        Ok(()) => {}
        Err(SessionError::NoAck(_)) => {
            return Err("The headset did not answer on its control channel. Try reconnecting it in your Bluetooth settings.".into());
        }
        Err(e) => return Err(e.to_string()),
    }
    // Wait for the init reply: it tells us which protocol generation to use.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut got_init = false;
    while !got_init && Instant::now() < deadline {
        let updates = session
            .poll(Duration::from_millis(50))
            .map_err(|e| e.to_string())?;
        got_init = updates
            .iter()
            .any(|n| matches!(n, Notification::InitReply(_)));
    }
    if session.dialect().is_v2() {
        // Earbuds usually use the ambient sub-type with wind noise reduction.
        let ambient = if form == FormFactor::Earbuds {
            AmbientType::WithWind
        } else {
            AmbientType::Basic
        };
        session.set_dialect(Dialect::v2(ambient));
    }
    let mut dialect = session.dialect();
    let _ = events.send(Event::Connected {
        device,
        form,
        dialect,
    });
    wake();

    let mut initial = match form {
        FormFactor::Headphones => vec![Request::GetBattery(BatteryKind::Single)],
        FormFactor::Earbuds if dialect.is_v2() => vec![
            Request::GetBattery(BatteryKind::Dual),
            Request::GetBattery(BatteryKind::Dual2),
            Request::GetBattery(BatteryKind::Case),
        ],
        FormFactor::Earbuds => vec![
            Request::GetBattery(BatteryKind::Dual),
            Request::GetBattery(BatteryKind::Case),
        ],
    };
    initial.extend([
        Request::GetAmbientSound,
        Request::GetCodec,
        Request::GetEqualizer,
        Request::GetFirmwareVersion,
        Request::GetAudioUpsampling,
        Request::GetBgmMode,
        Request::GetCinemaMode,
        Request::GetSurround,
        Request::GetSpeakToChat,
        Request::GetSpeakToChatConfig,
        Request::GetTouchSensor,
        Request::GetPauseWhenTakenOff,
        Request::GetAutoPowerOff,
    ]);
    // Not every model supports every query; missing answers are not fatal.
    let mut got_ambient = false;
    for request in &initial {
        try_send(&mut session, request)?;
        let updates = forward(&mut session, events, Duration::from_millis(1))?;
        got_ambient |= updates
            .iter()
            .any(|n| matches!(n, Notification::AmbientSound(_)));
        wake();
    }
    let updates = forward(&mut session, events, Duration::from_millis(300))?;
    got_ambient |= updates
        .iter()
        .any(|n| matches!(n, Notification::AmbientSound(_)));
    if !got_ambient && dialect.is_v2() {
        // Try the other ambient-sound sub-type (0x15 <-> 0x17).
        session.set_dialect(Dialect::v2(dialect.ambient.other()));
        try_send(&mut session, &Request::GetAmbientSound)?;
        let updates = forward(&mut session, events, Duration::from_millis(500))?;
        if !updates
            .iter()
            .any(|n| matches!(n, Notification::AmbientSound(_)))
        {
            session.set_dialect(dialect);
        }
    }
    wake();

    let mut warned = 0;
    loop {
        if session.dialect() != dialect {
            dialect = session.dialect();
            let _ = events.send(Event::DialectChanged(dialect));
            wake();
        }

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
                Err(SessionError::Io(e)) => return Err(e.to_string()),
                Err(e) => {
                    let _ = events.send(Event::Warning(e.to_string()));
                }
            }
        }

        if !forward(&mut session, events, Duration::from_millis(40))?.is_empty() {
            wake();
        }
        while warned < session.warnings.len() {
            let _ = events.send(Event::Warning(session.warnings[warned].clone()));
            warned += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                Ok(Event::Connected { form, dialect, .. }) => {
                    assert_eq!(form, FormFactor::Headphones);
                    assert!(dialect.is_v2());
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
