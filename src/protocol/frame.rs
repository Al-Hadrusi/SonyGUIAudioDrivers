//! Link-layer framing used by Sony headphones over the RFCOMM serial channel.
//!
//! A frame on the wire looks like this:
//!
//! ```text
//! 0x3E | data type | seq | len (u32 BE) | payload ... | checksum | 0x3C
//! ```
//!
//! * The checksum is the wrapping sum of every byte between the start and
//!   checksum markers (data type, sequence number, length and payload).
//! * Any byte between the markers that collides with `0x3C`, `0x3D` or `0x3E`
//!   is escaped as `0x3D, byte & 0xEF`.

use std::fmt;

/// Start-of-frame marker.
pub const START: u8 = 0x3E;
/// End-of-frame marker.
pub const END: u8 = 0x3C;
/// Escape prefix.
pub const ESCAPE: u8 = 0x3D;
const ESCAPE_MASK: u8 = 0xEF;

/// Largest payload we accept. Real messages are a few hundred bytes at most;
/// the limit protects us from a corrupt length field.
pub const MAX_PAYLOAD: usize = 4096;

/// The `data type` byte of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DataType {
    /// Acknowledgement of a received frame (empty payload).
    Ack,
    /// A regular "MDR" command / reply.
    DataMdr,
    /// Secondary MDR channel used by some models for extra features.
    DataMdrNo2,
    /// Anything else, kept as-is.
    Other(u8),
}

impl DataType {
    pub fn code(self) -> u8 {
        match self {
            DataType::Ack => 0x01,
            DataType::DataMdr => 0x0C,
            DataType::DataMdrNo2 => 0x0E,
            DataType::Other(c) => c,
        }
    }

    pub fn from_code(code: u8) -> Self {
        match code {
            0x01 => DataType::Ack,
            0x0C => DataType::DataMdr,
            0x0E => DataType::DataMdrNo2,
            c => DataType::Other(c),
        }
    }
}

/// A decoded frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub data_type: DataType,
    pub seq: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(data_type: DataType, seq: u8, payload: Vec<u8>) -> Self {
        Self {
            data_type,
            seq,
            payload,
        }
    }

    /// An acknowledgement for a frame that was received with sequence `seq`.
    pub fn ack_for(seq: u8) -> Self {
        Self::new(DataType::Ack, 1u8.wrapping_sub(seq) & 1, Vec::new())
    }

    /// Serialise the frame (including markers and escaping).
    pub fn encode(&self) -> Vec<u8> {
        let len = u32::try_from(self.payload.len()).unwrap_or(u32::MAX);
        let mut body = Vec::with_capacity(7 + self.payload.len());
        body.push(self.data_type.code());
        body.push(self.seq);
        body.extend_from_slice(&len.to_be_bytes());
        body.extend_from_slice(&self.payload);
        let checksum = checksum(&body);
        body.push(checksum);

        let mut out = Vec::with_capacity(body.len() + 8);
        out.push(START);
        for b in body {
            if matches!(b, START | END | ESCAPE) {
                out.push(ESCAPE);
                out.push(b & ESCAPE_MASK);
            } else {
                out.push(b);
            }
        }
        out.push(END);
        out
    }
}

/// Wrapping byte sum.
pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))
}

/// Errors detected while decoding a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    TooShort(usize),
    LengthMismatch { declared: usize, actual: usize },
    BadChecksum { expected: u8, actual: u8 },
    TooLarge(usize),
    BadEscape,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::TooShort(n) => write!(f, "frame too short ({n} bytes)"),
            FrameError::LengthMismatch { declared, actual } => {
                write!(
                    f,
                    "length field says {declared} bytes but frame holds {actual}"
                )
            }
            FrameError::BadChecksum { expected, actual } => {
                write!(
                    f,
                    "bad checksum: expected {expected:#04x}, got {actual:#04x}"
                )
            }
            FrameError::TooLarge(n) => write!(f, "frame too large ({n} bytes)"),
            FrameError::BadEscape => write!(f, "invalid escape sequence"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Incremental decoder: feed it raw bytes as they arrive from the socket and
/// it yields complete frames.
#[derive(Default, Debug)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    in_frame: bool,
    escaping: bool,
    overflow: bool,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns every frame (or framing error) completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Frame, FrameError>> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == START {
                // A start marker always (re)starts a frame, resynchronising
                // after garbage.
                self.reset();
                self.in_frame = true;
                continue;
            }
            if !self.in_frame {
                continue;
            }
            if b == END {
                let result = if self.overflow {
                    Err(FrameError::TooLarge(self.buf.len()))
                } else if self.escaping {
                    Err(FrameError::BadEscape)
                } else {
                    parse_body(&self.buf)
                };
                out.push(result);
                self.reset();
                continue;
            }
            if self.escaping {
                self.escaping = false;
                self.store(b | !ESCAPE_MASK);
            } else if b == ESCAPE {
                self.escaping = true;
            } else {
                self.store(b);
            }
        }
        out
    }

    fn store(&mut self, b: u8) {
        if self.buf.len() >= MAX_PAYLOAD + 7 {
            self.overflow = true;
        } else {
            self.buf.push(b);
        }
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.in_frame = false;
        self.escaping = false;
        self.overflow = false;
    }
}

fn parse_body(body: &[u8]) -> Result<Frame, FrameError> {
    // type + seq + len(4) + checksum
    if body.len() < 7 {
        return Err(FrameError::TooShort(body.len()));
    }
    let (content, sum) = body.split_at(body.len() - 1);
    let expected = checksum(content);
    if expected != sum[0] {
        return Err(FrameError::BadChecksum {
            expected,
            actual: sum[0],
        });
    }
    let declared = u32::from_be_bytes([content[2], content[3], content[4], content[5]]) as usize;
    let payload = &content[6..];
    if declared != payload.len() {
        return Err(FrameError::LengthMismatch {
            declared,
            actual: payload.len(),
        });
    }
    Ok(Frame {
        data_type: DataType::from_code(content[0]),
        seq: content[1],
        payload: payload.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_simple() {
        let f = Frame::new(DataType::DataMdr, 0, vec![0x00, 0x00]);
        let bytes = f.encode();
        assert_eq!(
            bytes,
            vec![0x3E, 0x0C, 0x00, 0, 0, 0, 2, 0x00, 0x00, 0x0E, 0x3C]
        );
        let mut d = FrameDecoder::new();
        assert_eq!(d.push(&bytes), vec![Ok(f)]);
    }

    #[test]
    fn escapes_reserved_bytes() {
        let f = Frame::new(DataType::DataMdr, 1, vec![0x3C, 0x3D, 0x3E, 0x10]);
        let bytes = f.encode();
        // No reserved byte may appear between the markers.
        assert!(
            bytes[1..bytes.len() - 1]
                .iter()
                .all(|b| *b != START && *b != END)
        );
        let mut d = FrameDecoder::new();
        assert_eq!(d.push(&bytes), vec![Ok(f)]);
    }

    #[test]
    fn decodes_byte_by_byte_and_multiple_frames() {
        let a = Frame::new(DataType::DataMdr, 0, vec![0x66, 0x02]);
        let b = Frame::ack_for(0);
        let mut stream = vec![0xFF, 0x00]; // leading garbage
        stream.extend(a.encode());
        stream.extend(b.encode());
        let mut d = FrameDecoder::new();
        let mut got = Vec::new();
        for byte in stream {
            got.extend(d.push(&[byte]));
        }
        assert_eq!(got, vec![Ok(a), Ok(b)]);
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut bytes = Frame::new(DataType::DataMdr, 0, vec![1, 2, 3]).encode();
        let n = bytes.len();
        bytes[n - 2] ^= 0x01;
        let mut d = FrameDecoder::new();
        assert!(matches!(
            d.push(&bytes)[0],
            Err(FrameError::BadChecksum { .. })
        ));
    }

    #[test]
    fn ack_sequence_is_flipped() {
        assert_eq!(Frame::ack_for(0).seq, 1);
        assert_eq!(Frame::ack_for(1).seq, 0);
    }

    #[test]
    fn oversized_frames_are_rejected() {
        let mut d = FrameDecoder::new();
        let mut bytes = vec![START];
        bytes.extend(std::iter::repeat_n(0x01, MAX_PAYLOAD + 100));
        bytes.push(END);
        assert!(matches!(d.push(&bytes)[0], Err(FrameError::TooLarge(_))));
    }
}
