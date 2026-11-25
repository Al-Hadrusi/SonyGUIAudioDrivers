//! Reliable request / acknowledge exchange on top of a byte transport.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use super::frame::{DataType, Frame, FrameDecoder};
use super::message::{Notification, Request};
use crate::transport::Transport;

const ACK_TIMEOUT: Duration = Duration::from_millis(1500);
const RETRIES: usize = 2;
const READ_SLICE: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    /// The headset never acknowledged a request.
    NoAck(Request),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "connection error: {e}"),
            SessionError::NoAck(r) => write!(f, "headset did not acknowledge {r:?}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        SessionError::Io(e)
    }
}

/// Owns a transport and implements the sequence-number / ACK protocol.
pub struct Session {
    transport: Box<dyn Transport>,
    decoder: FrameDecoder,
    seq: u8,
    pending: VecDeque<Notification>,
    acked: bool,
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
            warnings: Vec::new(),
        }
    }

    /// Send a request and wait for its acknowledgement. Notifications that
    /// arrive meanwhile are queued and returned by [`Session::poll`].
    pub fn send(&mut self, request: &Request) -> Result<(), SessionError> {
        let frame = Frame::new(DataType::DataMdr, self.seq, request.payload()).encode();
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
                match Notification::parse(&frame.payload) {
                    Ok(n) => self.pending.push_back(n),
                    Err(e) => self.warn(format!("unparsable message: {e}")),
                }
            }
            DataType::Other(code) => self.warn(format!("ignoring frame type {code:#04x}")),
        }
        Ok(())
    }

    fn warn(&mut self, w: String) {
        if self.warnings.len() < 100 {
            self.warnings.push(w);
        }
    }
}
