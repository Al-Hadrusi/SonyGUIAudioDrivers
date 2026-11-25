//! Real Bluetooth transport for Windows 10/11, via the WinRT RFCOMM API.
//!
//! Sony headsets expose their control channel as an RFCOMM service with a
//! vendor UUID (the same ones as on Linux). Windows lists paired devices that
//! offer it; we connect a stream socket to that service.

use std::io;
use std::time::{Duration, Instant};

use windows::Devices::Bluetooth::BluetoothConnectionStatus;
use windows::Devices::Bluetooth::Rfcomm::{RfcommDeviceService, RfcommServiceId};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};
use windows::core::{GUID, HSTRING};
use windows_future::{AsyncStatus, IAsyncOperation};

use super::{DeviceInfo, Transport};

/// Control service used by WH-1000XM3/XM4, WF-1000XM3/XM4, LinkBuds, ...
const SONY_UUID_V1: u128 = 0x96CC203E_5068_46AD_B32D_E316F5E069BA;
/// Control service used by newer models (WH-1000XM5/XM6, WF-1000XM5, ...).
const SONY_UUID_V2: u128 = 0x956C7B26_D49A_4BA8_B03F_B17D393CB6E2;

/// Read size per `LoadAsync`.
const CHUNK: u32 = 1024;

fn io_err(e: windows::core::Error) -> io::Error {
    io::Error::other(e.message())
}

/// Paired devices that offer a Sony control service.
pub fn list_paired() -> io::Result<Vec<DeviceInfo>> {
    let mut out: Vec<DeviceInfo> = Vec::new();
    for uuid in [SONY_UUID_V2, SONY_UUID_V1] {
        let service_id = RfcommServiceId::FromUuid(GUID::from_u128(uuid)).map_err(io_err)?;
        let selector = RfcommDeviceService::GetDeviceSelector(&service_id).map_err(io_err)?;
        let found = DeviceInformation::FindAllAsyncAqsFilter(&selector)
            .and_then(|op| op.join())
            .map_err(io_err)?;
        for info in found {
            let Ok(id) = info.Id() else { continue };
            // The entry describes the service; ask for the device behind it.
            let device = RfcommDeviceService::FromIdAsync(&id)
                .and_then(|op| op.join())
                .and_then(|service| service.Device());
            let (name, connected) = match &device {
                Ok(d) => (
                    d.Name().map(|n| n.to_string()).unwrap_or_default(),
                    d.ConnectionStatus().ok() == Some(BluetoothConnectionStatus::Connected),
                ),
                Err(_) => (
                    info.Name().map(|n| n.to_string()).unwrap_or_default(),
                    false,
                ),
            };
            let name = if name.is_empty() {
                "Sony headset".to_string()
            } else {
                name
            };
            // A headset offering both services is listed once (V2 first).
            if out.iter().all(|d| d.name != name) {
                out.push(DeviceInfo {
                    address: id.to_string(),
                    name,
                    connected,
                });
            }
        }
    }
    Ok(out)
}

/// RFCOMM stream to a headset's control service.
pub struct BluetoothTransport {
    _socket: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
    /// A read that timed out is kept and awaited by the next `read`, so no
    /// bytes are lost by cancelling it.
    pending: Option<IAsyncOperation<u32>>,
}

impl BluetoothTransport {
    /// `address` is the service id returned by [`list_paired`].
    pub fn connect(address: &str) -> io::Result<Self> {
        let service = RfcommDeviceService::FromIdAsync(&HSTRING::from(address))
            .and_then(|op| op.join())
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "headset not available ({}). Is it on and connected?",
                        e.message()
                    ),
                )
            })?;
        let socket = StreamSocket::new().map_err(io_err)?;
        socket
            .ConnectAsync(
                &service.ConnectionHostName().map_err(io_err)?,
                &service.ConnectionServiceName().map_err(io_err)?,
            )
            .and_then(|op| op.join())
            .map_err(io_err)?;
        let reader =
            DataReader::CreateDataReader(&socket.InputStream().map_err(io_err)?).map_err(io_err)?;
        // Return as soon as some bytes arrived instead of waiting for CHUNK.
        reader
            .SetInputStreamOptions(InputStreamOptions::Partial)
            .map_err(io_err)?;
        let writer = DataWriter::CreateDataWriter(&socket.OutputStream().map_err(io_err)?)
            .map_err(io_err)?;
        Ok(Self {
            _socket: socket,
            reader,
            writer,
            pending: None,
        })
    }
}

impl Transport for BluetoothTransport {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.WriteBytes(bytes).map_err(io_err)?;
        self.writer
            .StoreAsync()
            .and_then(|op| op.join())
            .map_err(io_err)?;
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.pending.is_none() && self.buffered()? > 0 {
            return self.take_buffered(buf);
        }
        let op = match self.pending.take() {
            Some(op) => op,
            None => self.reader.LoadAsync(CHUNK).map_err(io_err)?,
        };
        let deadline = Instant::now() + timeout;
        loop {
            match op.Status().map_err(io_err)? {
                AsyncStatus::Started => {
                    if Instant::now() >= deadline {
                        self.pending = Some(op);
                        return Ok(0);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                AsyncStatus::Completed => break,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "headset disconnected",
                    ));
                }
            }
        }
        let loaded = op.GetResults().map_err(io_err)?;
        if loaded == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "headset disconnected",
            ));
        }
        self.take_buffered(buf)
    }
}

impl BluetoothTransport {
    fn buffered(&self) -> io::Result<usize> {
        Ok(usize::try_from(self.reader.UnconsumedBufferLength().map_err(io_err)?).unwrap_or(0))
    }

    /// Hand out what fits; the rest stays buffered in the reader.
    fn take_buffered(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.buffered()?.min(buf.len());
        let Some(dst) = buf.get_mut(..n) else {
            return Ok(0);
        };
        self.reader.ReadBytes(dst).map_err(io_err)?;
        Ok(n)
    }
}
