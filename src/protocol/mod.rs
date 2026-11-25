//! Sony headset control protocol (framing + messages + session handling).

pub mod frame;
pub mod message;
pub mod session;

pub use frame::{DataType, Frame, FrameDecoder, FrameError};
pub use message::*;
pub use session::{Session, SessionError};
