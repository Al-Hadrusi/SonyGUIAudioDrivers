//! Reliable request / acknowledge exchange on top of a byte transport.
//!
//! Set `SONY_DEBUG=1` to print every frame sent and received to stderr.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use super::frame::{DataType, Frame, FrameDecoder};
use super::message::{Dialect, Notification, ProtocolVersion, Request};
use crate::transport::Transport;

const ACK_TIMEOUT: Duration = Duration::from_millis(1000);
const RETRIES: usize = 1;
const READ_SLICE: Duration = Duration::from_millis(50);
/// Most notifications kept between two polls. A misbehaving headset that
/// floods us while we wait for an ACK cannot grow memory without bound.
const MAX_PENDING: usize = 512;

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    /// The headset never acknowledged a request.
    NoAck(Request),
    /// This headset generation has no such command.
    Unsupported(Request),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "connection error: {e}"),
            SessionError::NoAck(r) => write!(f, "headset did not acknowledge {r:?}"),
            SessionError::Unsupported(r) => write!(f, "not supported by this headset: {r:?}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        SessionError::Io(e)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Owns a transport and implements the sequence-number / ACK protocol.
pub struct Session {
    transport: Box<dyn Transport>,
    decoder: FrameDecoder,
    seq: u8,
    pending: VecDeque<Notification>,
    acked: bool,
    dialect: Dialect,
    debug: bool,
    /// Non-fatal problems (bad frames, unparsable messages) for diagnostics.
    pub warnings: Vec<String>,
}

impl Session {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            decoder: FrameDecoder::new(),
            seq: 0,
            pending: VecDeque::new(),
            acked: false,
            dialect: Dialect::default(),
            debug: std::env::var_os("SONY_DEBUG").is_some_and(|v| !v.is_empty() && v != "0"),
            warnings: Vec::new(),
        }
    }

    /// The protocol dialect negotiated so far (updated from the init reply
    /// and from ambient sound replies).
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    pub fn set_dialect(&mut self, dialect: Dialect) {
        self.dialect = dialect;
    }

    /// Send a request and wait for its acknowledgement. Notifications that
    /// arrive meanwhile are queued and returned by [`Session::poll`].
    pub fn send(&mut self, request: &Request) -> Result<(), SessionError> {
        let Some(payload) = request.payload(self.dialect) else {
            return Err(SessionError::Unsupported(request.clone()));
        };
        if self.debug {
            eprintln!("[sony] TX {request:?}: {}", hex(&payload));
        }
        let frame = Frame::new(request.data_type(), self.seq, payload).encode();
        for _ in 0..=RETRIES {
            self.acked = false;
            self.transport.write_all(&frame)?;
            let deadline = Instant::now() + ACK_TIMEOUT;
            while Instant::now() < deadline {
                self.pump(READ_SLICE)?;
                if self.acked {
                    return Ok(());
                }
            }
            if self.debug {
                eprintln!("[sony] no ACK, retrying");
            }
        }
        Err(SessionError::NoAck(request.clone()))
    }

    /// Read for up to `timeout` and return every notification received.
    pub fn poll(&mut self, timeout: Duration) -> Result<Vec<Notification>, SessionError> {
        if self.pending.is_empty() {
            self.pump(timeout)?;
        }
        Ok(self.pending.drain(..).collect())
    }

    fn pump(&mut self, timeout: Duration) -> Result<(), SessionError> {
        let mut buf = [0u8; 1024];
        let n = self.transport.read(&mut buf, timeout)?;
        let Some(chunk) = buf.get(..n) else {
            return Ok(());
        };
        for result in self.decoder.push(chunk) {
            match result {
                Ok(frame) => self.handle(frame)?,
                Err(e) => self.warn(format!("dropped frame: {e}")),
            }
        }
        Ok(())
    }

    fn handle(&mut self, frame: Frame) -> Result<(), SessionError> {
        match frame.data_type {
            DataType::Ack => {
                self.seq = frame.seq;
                self.acked = true;
            }
            DataType::DataMdr | DataType::DataMdrNo2 => {
                self.transport
                    .write_all(&Frame::ack_for(frame.seq).encode())?;
                let parsed = if frame.data_type == DataType::DataMdrNo2 {
                    Notification::parse_t2(&frame.payload, self.dialect)
                } else {
                    Notification::parse(&frame.payload, self.dialect)
                };
                if self.debug {
                    eprintln!("[sony] RX {}: {parsed:?}", hex(&frame.payload));
                }
                match parsed {
                    Ok(n) => {
                        self.learn(&n, &frame.payload);
                        if self.pending.len() >= MAX_PENDING {
                            self.pending.pop_front();
                        }
                        self.pending.push_back(n);
                    }
                    Err(e) => self.warn(format!("unparsable message: {e}")),
                }
            }
            DataType::Other(code) => self.warn(format!("ignoring frame type {code:#04x}")),
        }
        Ok(())
    }

    /// Adapt the dialect to what the headset tells us.
    fn learn(&mut self, n: &Notification, payload: &[u8]) {
        match n {
            Notification::InitReply(version) => {
                self.dialect.version = *version;
                if self.debug {
                    eprintln!("[sony] protocol version {version:?}");
                }
            }
            Notification::AmbientSound(_) if self.dialect.version == ProtocolVersion::V2 => {
                if let Some(t) = Notification::ambient_type_of(payload) {
                    self.dialect.ambient = t;
                }
            }
            _ => {}
        }
    }

    fn warn(&mut self, w: String) {
        if self.debug {
            eprintln!("[sony] warning: {w}");
        }
        if self.warnings.len() < 100 {
            self.warnings.push(w);
        }
    }
}
