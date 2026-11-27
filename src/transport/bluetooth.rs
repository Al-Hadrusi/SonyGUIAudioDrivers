//! Real Bluetooth transport for Linux, via BlueZ (the `bluer` crate).
//!
//! Sony headsets expose their control channel as an RFCOMM service with a
//! vendor UUID. We register a client profile for that UUID with BlueZ and ask
//! it to connect, which takes care of the SDP channel lookup for us.
//!
//! Set `SONY_RFCOMM_CHANNEL=<n>` to bypass SDP and connect to a fixed channel.

use std::io;
use std::str::FromStr;
use std::time::Duration;

use bluer::rfcomm::{Profile, ReqError, Role, Security, SecurityLevel, Socket, SocketAddr, Stream};
use bluer::{Address, Session, Uuid};
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;

use super::{DeviceInfo, Transport};

/// Control service used by WH-1000XM3/XM4, WF-1000XM3/XM4, LinkBuds, ...
pub const SONY_UUID_V1: Uuid = Uuid::from_u128(0x96CC203E_5068_46AD_B32D_E316F5E069BA);
/// Control service used by newer models (WH-1000XM5, WF-1000XM5, ...).
pub const SONY_UUID_V2: Uuid = Uuid::from_u128(0x956C7B26_D49A_4BA8_B03F_B17D393CB6E2);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Bluetooth names are at most 248 bytes; keep displayed names short.
const MAX_NAME_CHARS: usize = 64;

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn runtime() -> io::Result<Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

/// Paired devices that look like Sony headsets.
pub fn list_paired() -> io::Result<Vec<DeviceInfo>> {
    runtime()?.block_on(async {
        let session = Session::new().await.map_err(io_err)?;
        let adapter = session.default_adapter().await.map_err(io_err)?;
        let mut out = Vec::new();
        for addr in adapter.device_addresses().await.map_err(io_err)? {
            let dev = adapter.device(addr).map_err(io_err)?;
            if !dev.is_paired().await.unwrap_or(false) {
                continue;
            }
            let uuids = dev.uuids().await.ok().flatten().unwrap_or_default();
            let name = crate::text::sanitize(
                &dev.alias().await.unwrap_or_else(|_| addr.to_string()),
                MAX_NAME_CHARS,
            );
            let name = if name.is_empty() {
                addr.to_string()
            } else {
                name
            };
            let sony = uuids.contains(&SONY_UUID_V1) || uuids.contains(&SONY_UUID_V2);
            let looks_sony = ["WH-", "WF-", "WI-", "LINKBUDS", "ULT", "INZONE"]
                .iter()
                .any(|p| name.to_ascii_uppercase().contains(p));
            if sony || looks_sony {
                out.push(DeviceInfo {
                    address: addr.to_string(),
                    name,
                    connected: dev.is_connected().await.unwrap_or(false),
                });
            }
        }
        Ok(out)
    })
}

/// RFCOMM connection to a headset's control service.
pub struct BluetoothTransport {
    rt: Runtime,
    stream: Stream,
}

impl BluetoothTransport {
    pub fn connect(address: &str) -> io::Result<Self> {
        let addr = Address::from_str(address).map_err(io_err)?;
        let rt = runtime()?;
        let stream = rt.block_on(async {
            tokio::time::timeout(CONNECT_TIMEOUT, connect_stream(addr))
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out connecting to the headset",
                    )
                })?
        })?;
        Ok(Self { rt, stream })
    }
}

async fn connect_stream(addr: Address) -> io::Result<Stream> {
    if let Some(channel) = std::env::var("SONY_RFCOMM_CHANNEL")
        .ok()
        .and_then(|c| c.parse::<u8>().ok())
    {
        // Same requirement as the profile path below: an authenticated,
        // encrypted link to the paired headset only.
        let socket = Socket::new()?;
        socket.set_security(Security {
            level: SecurityLevel::Medium,
            key_size: 0,
        })?;
        return socket.connect(SocketAddr::new(addr, channel)).await;
    }

    let session = Session::new().await.map_err(io_err)?;
    let adapter = session.default_adapter().await.map_err(io_err)?;
    let device = adapter.device(addr).map_err(io_err)?;
    if !device.is_connected().await.unwrap_or(false) {
        device.connect().await.map_err(io_err)?;
    }
    let uuids = device.uuids().await.ok().flatten().unwrap_or_default();
    let candidates: Vec<Uuid> = if uuids.contains(&SONY_UUID_V2) && !uuids.contains(&SONY_UUID_V1) {
        vec![SONY_UUID_V2, SONY_UUID_V1]
    } else {
        vec![SONY_UUID_V1, SONY_UUID_V2]
    };

    let mut last_error = io::Error::new(
        io::ErrorKind::NotFound,
        "headset has no Sony control service",
    );
    for uuid in candidates {
        let profile = Profile {
            uuid,
            role: Some(Role::Client),
            // The headset is paired, so insist on an authenticated
            // (encrypted) link: an unauthenticated one could be a device
            // spoofing the headset's address.
            require_authentication: Some(true),
            require_authorization: Some(false),
            auto_connect: Some(false),
            ..Default::default()
        };
        let mut handle = match session.register_profile(profile).await {
            Ok(h) => h,
            Err(e) => {
                last_error = io_err(e);
                continue;
            }
        };
        let dev = device.clone();
        let connect = tokio::spawn(async move { dev.connect_profile(&uuid).await });
        // Only accept the connection from the headset we asked for: while the
        // profile is registered, BlueZ could hand us a connection from any
        // other device that offers the same service UUID.
        let request = tokio::time::timeout(Duration::from_secs(8), async {
            while let Some(req) = handle.next().await {
                if req.device() == addr {
                    return Some(req);
                }
                req.reject(ReqError::Rejected);
            }
            None
        })
        .await;
        match request {
            Ok(Some(req)) => {
                let stream = req.accept().map_err(io_err)?;
                // The accepted socket stays open after the profile handle is
                // dropped; the connect task finishes on its own.
                drop(connect);
                return Ok(stream);
            }
            _ => {
                if let Ok(Err(e)) = connect.await {
                    last_error = io_err(e);
                }
            }
        }
    }
    Err(last_error)
}

/// What the kernel reports about an open control connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkInfo {
    /// Address of the device at the other end of the socket.
    pub peer: String,
    /// Security level the link actually runs at (not just what was asked).
    pub security: SecurityLevel,
}

impl LinkInfo {
    /// The link is authenticated and encrypted (BlueZ "medium" or better).
    pub fn is_authenticated(&self) -> bool {
        self.security >= SecurityLevel::Medium
    }
}

impl BluetoothTransport {
    /// Peer address and security level of the open connection, as reported
    /// by the kernel. Used by `examples/bt_check.rs` to verify on real
    /// hardware that only the chosen headset, over an authenticated link,
    /// is accepted.
    pub fn link_info(&self) -> io::Result<LinkInfo> {
        let socket: &Socket = self.stream.as_ref();
        Ok(LinkInfo {
            peer: self.stream.peer_addr()?.addr.to_string(),
            security: socket.security()?.level,
        })
    }
}

impl Transport for BluetoothTransport {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let stream = &mut self.stream;
        self.rt.block_on(async { stream.write_all(bytes).await })
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let stream = &mut self.stream;
        self.rt.block_on(async {
            match tokio::time::timeout(timeout, stream.read(buf)).await {
                Err(_elapsed) => Ok(0),
                Ok(Ok(0)) => Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "headset disconnected",
                )),
                Ok(result) => result,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_medium_or_higher_counts_as_authenticated() {
        let link = |security| LinkInfo {
            peer: "AA:BB:CC:DD:EE:FF".into(),
            security,
        };
        assert!(!link(SecurityLevel::Sdp).is_authenticated());
        assert!(!link(SecurityLevel::Low).is_authenticated());
        assert!(link(SecurityLevel::Medium).is_authenticated());
        assert!(link(SecurityLevel::High).is_authenticated());
    }
}
