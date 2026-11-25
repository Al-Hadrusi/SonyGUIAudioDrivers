//! An in-process headset emulator that speaks the real wire protocol. It lets
//! the app be used and tested without hardware.

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use super::{DeviceInfo, FormFactor, Transport};
use crate::protocol::message::*;
use crate::protocol::{DataType, Frame, FrameDecoder};

pub fn demo_devices() -> Vec<DeviceInfo> {
    ["WH-1000XM5", "WF-1000XM5"]
        .into_iter()
        .map(|m| DeviceInfo {
            address: format!("demo:{m}"),
            name: m.to_string(),
            connected: false,
        })
        .collect()
}

pub struct SimulatedHeadset {
    form: FormFactor,
    decoder: FrameDecoder,
    outbox: VecDeque<u8>,
    seq: u8,
    ambient: AmbientSound,
    eq: Equalizer,
    custom: [[i8; 6]; 3],
    upsampling: bool,
    battery: [u8; 3],
    last_drain: Instant,
    powered_off: bool,
}

impl SimulatedHeadset {
    pub fn new(name: &str) -> Self {
        Self {
            form: FormFactor::from_name(name),
            decoder: FrameDecoder::new(),
            outbox: VecDeque::new(),
            seq: 0,
            ambient: AmbientSound::default(),
            eq: Equalizer::default(),
            custom: [[0; 6]; 3],
            upsampling: true,
            battery: [80, 75, 60],
            last_drain: Instant::now(),
            powered_off: false,
        }
    }

    fn emit(&mut self, n: Notification) {
        let frame = Frame::new(DataType::DataMdr, self.seq, n.payload());
        self.seq ^= 1;
        self.outbox.extend(frame.encode());
    }

    fn battery_notifications(&self) -> Vec<Notification> {
        match self.form {
            FormFactor::Headphones => vec![Notification::Battery {
                kind: BatteryKind::Single,
                left: Battery {
                    level: self.battery[0],
                    charging: false,
                },
                right: None,
            }],
            FormFactor::Earbuds => vec![
                Notification::Battery {
                    kind: BatteryKind::Dual,
                    left: Battery {
                        level: self.battery[0],
                        charging: false,
                    },
                    right: Some(Battery {
                        level: self.battery[1],
                        charging: false,
                    }),
                },
                Notification::Battery {
                    kind: BatteryKind::Case,
                    left: Battery {
                        level: self.battery[2],
                        charging: true,
                    },
                    right: None,
                },
            ],
        }
    }

    fn handle_request(&mut self, payload: &[u8]) {
        let Some(&opcode) = payload.first() else {
            return;
        };
        match opcode {
            op::INIT_REQUEST => self.emit(Notification::InitReply),
            op::FW_VERSION_GET => self.emit(Notification::FirmwareVersion("2.1.0 (demo)".into())),
            op::BATTERY_GET => {
                for n in self.battery_notifications() {
                    self.emit(n);
                }
            }
            op::CODEC_GET => self.emit(Notification::Codec(Codec::Ldac)),
            op::AMBIENT_GET => self.emit(Notification::AmbientSound(self.ambient)),
            op::AMBIENT_SET => {
                let mut p = payload.to_vec();
                p[0] = op::AMBIENT_NOTIFY;
                if let Ok(Notification::AmbientSound(mut a)) = Notification::parse(&p) {
                    if a.mode != AmbientMode::AmbientSound {
                        a.level = self.ambient.level; // level is remembered
                    }
                    self.ambient = a;
                    // Like the real devices, only notify when the adjustment completed.
                    if payload.get(2) != Some(&0x10) {
                        self.emit(Notification::AmbientSound(self.ambient));
                    }
                }
            }
            op::EQ_GET => self.emit(Notification::Equalizer(self.eq)),
            op::EQ_SET => {
                let mut p = payload.to_vec();
                p[0] = op::EQ_NOTIFY;
                if let Ok(Notification::Equalizer(mut eq)) = Notification::parse(&p) {
                    let slot = match eq.preset {
                        EqPreset::Manual => Some(0),
                        EqPreset::Custom1 => Some(1),
                        EqPreset::Custom2 => Some(2),
                        _ => None,
                    };
                    match slot {
                        Some(i) if payload.len() >= 10 => self.custom[i] = eq.bands,
                        Some(i) => eq.bands = self.custom[i],
                        None => eq.bands = eq.preset.default_bands(),
                    }
                    self.eq = eq;
                    self.emit(Notification::Equalizer(eq));
                }
            }
            op::AUDIO_UPSAMPLING_GET => self.emit(Notification::AudioUpsampling(self.upsampling)),
            op::AUDIO_UPSAMPLING_SET => {
                self.upsampling = payload.get(3) == Some(&1);
                self.emit(Notification::AudioUpsampling(self.upsampling));
            }
            op::POWER_OFF => self.powered_off = true,
            _ => {} // Accepted silently (system settings etc.)
        }
    }

    fn tick(&mut self) {
        if self.last_drain.elapsed() >= Duration::from_secs(60) {
            self.last_drain = Instant::now();
            for b in &mut self.battery[..2] {
                *b = b.saturating_sub(1).max(5);
            }
            for n in self.battery_notifications() {
                self.emit(n);
            }
        }
    }
}

impl Transport for SimulatedHeadset {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.powered_off {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "headset powered off",
            ));
        }
        for frame in self.decoder.push(bytes).into_iter().flatten() {
            if frame.data_type == DataType::Ack {
                continue;
            }
            self.outbox.extend(Frame::ack_for(frame.seq).encode());
            self.handle_request(&frame.payload);
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.powered_off && self.outbox.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "headset powered off",
            ));
        }
        self.tick();
        if self.outbox.is_empty() {
            std::thread::sleep(timeout.min(Duration::from_millis(20)));
            return Ok(0);
        }
        let n = buf.len().min(self.outbox.len());
        for (dst, src) in buf.iter_mut().zip(self.outbox.drain(..n)) {
            *dst = src;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Session;

    #[test]
    fn session_talks_to_simulator() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WF-1000XM5")));
        s.send(&Request::Init).unwrap();
        s.send(&Request::GetBattery(BatteryKind::Dual)).unwrap();
        let target = AmbientSound {
            mode: AmbientMode::AmbientSound,
            level: 9,
            focus_on_voice: true,
        };
        s.send(&Request::SetAmbientSound {
            value: target,
            adjusting: false,
        })
        .unwrap();
        s.send(&Request::SetEqualizer(Equalizer {
            preset: EqPreset::Custom1,
            bands: [1, 2, 3, 4, 5, 6],
        }))
        .unwrap();

        let mut got = Vec::new();
        for _ in 0..10 {
            got.extend(s.poll(Duration::from_millis(10)).unwrap());
        }
        assert!(got.contains(&Notification::InitReply));
        assert!(got.iter().any(|n| matches!(
            n,
            Notification::Battery {
                kind: BatteryKind::Dual,
                ..
            }
        )));
        assert!(got.contains(&Notification::AmbientSound(target)));
        assert!(got.contains(&Notification::Equalizer(Equalizer {
            preset: EqPreset::Custom1,
            bands: [1, 2, 3, 4, 5, 6]
        })));
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn power_off_closes_connection() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WH-1000XM5")));
        s.send(&Request::PowerOff).unwrap();
        assert!(
            s.poll(Duration::from_millis(10)).is_err()
                || s.poll(Duration::from_millis(10)).is_err()
        );
    }
}
