//! Control library for Sony Bluetooth headphones and earbuds.
//!
//! * [`protocol`] – wire framing, message encoding/decoding and the
//!   acknowledged request session.
//! * [`transport`] – byte transports: BlueZ RFCOMM (Linux) and a built-in
//!   simulated headset.
//! * [`worker`] – background connection thread used by the GUI.
//!
//! The crate contains no `unsafe` code; this is enforced by the compiler.

#![forbid(unsafe_code)]

pub mod battery_alert;
pub mod config;
pub mod desktop_entry;
pub mod protocol;
pub mod transport;
pub mod worker;
