//! An in-process headset emulator that speaks the real wire protocol. It lets
//! the app be used and tested without hardware.
//!
//! * `WH-1000XM6` – protocol V2, with Listening Mode (Standard / BGM / Cinema)
//! * `WH-1000XM5` – protocol V2, ambient type `0x15`
//! * `WF-1000XM5` – protocol V2, ambient type `0x17` (wind noise reduction)
//! * `WH-1000XM4` – protocol V1, with virtual surround

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use super::{DeviceInfo, FormFactor, Transport};
use crate::protocol::message::*;
use crate::protocol::{DataType, Frame, FrameDecoder};

pub const DEMO_MODELS: [&str; 4] = ["WH-1000XM6", "WH-1000XM5", "WF-1000XM5", "WH-1000XM4"];

pub fn demo_devices() -> Vec<DeviceInfo> {
    DEMO_MODELS
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
    dialect: Dialect,
    decoder: FrameDecoder,
    outbox: VecDeque<u8>,
    seq: u8,
    ambient: AmbientSound,
    eq: Equalizer,
    custom: [[i8; 6]; 3],
    upsampling: bool,
    speak_to_chat: bool,
    stc: SpeakToChatConfig,
    touch: bool,
    pause: bool,
    auto_power_off: AutoPowerOff,
    battery: [u8; 3],
    last_drain: Instant,
    powered_off: bool,
    /// `None` when the model has no listening mode.
    listening: Option<(ListeningMode, RoomSize)>,
    /// `None` when the model has no virtual surround.
    surround: Option<Surround>,
    /// Every request received, for tests.
    pub received: Vec<Request>,
}

impl SimulatedHeadset {
    pub fn new(name: &str) -> Self {
        let upper = name.to_ascii_uppercase();
        let dialect = if upper.contains("XM3") || (upper.contains("XM4") && upper.starts_with("WH"))
        {
            Dialect::V1
        } else if upper.starts_with("WF") {
            Dialect::v2(AmbientType::WithWind)
        } else {
            Dialect::v2(AmbientType::Basic)
        };
        Self {
            form: FormFactor::from_name(name),
            dialect,
            decoder: FrameDecoder::new(),
            outbox: VecDeque::new(),
            seq: 0,
            ambient: AmbientSound::default(),
            eq: Equalizer::default(),
            custom: [[0; 6]; 3],
            upsampling: true,
            speak_to_chat: false,
            stc: SpeakToChatConfig::default(),
            touch: true,
            pause: true,
            auto_power_off: AutoPowerOff::Off,
            battery: [80, 75, 60],
            last_drain: Instant::now(),
            powered_off: false,
            listening: upper
                .contains("XM6")
                .then_some((ListeningMode::Standard, RoomSize::Small)),
            surround: (dialect == Dialect::V1).then_some(Surround::Off),
            received: Vec::new(),
        }
    }

    fn emit(&mut self, n: Notification) {
        let frame = Frame::new(DataType::DataMdr, self.seq, n.payload(self.dialect));
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
        // Real headsets silently ignore what they do not understand.
        let Some(request) = Request::decode(payload, self.dialect) else {
            return;
        };
        self.received.push(request.clone());
        match request {
            Request::Init => {
                let version = self.dialect.version;
                self.emit(Notification::InitReply(version));
            }
            Request::GetFirmwareVersion => self.emit(Notification::FirmwareVersion("2.1.0".into())),
            Request::GetBattery(kind) => {
                let wanted: Vec<_> = self
                    .battery_notifications()
                    .into_iter()
                    .filter(|n| matches!(n, Notification::Battery { kind: k, .. } if *k == kind || (k == &BatteryKind::Dual && kind == BatteryKind::Dual2)))
                    .collect();
                for n in wanted {
                    self.emit(n);
                }
            }
            Request::GetCodec => self.emit(Notification::Codec(Codec::Ldac)),
            Request::GetAmbientSound => self.emit(Notification::AmbientSound(self.ambient)),
            Request::SetAmbientSound {
                mut value,
                adjusting,
            } => {
                if value.mode != AmbientMode::AmbientSound && !self.dialect.is_v2() {
                    value.level = self.ambient.level; // V1 does not send the level
                }
                self.ambient = value;
                if !adjusting {
                    self.emit(Notification::AmbientSound(self.ambient));
                }
            }
            Request::GetEqualizer => self.emit(Notification::Equalizer(self.eq)),
            Request::SetEqualizerPreset(preset) => {
                let bands = match slot(preset) {
                    Some(i) => self.custom[i],
                    None => preset.default_bands(),
                };
                self.eq = Equalizer { preset, bands };
                self.emit(Notification::Equalizer(self.eq));
            }
            Request::SetEqualizerBands(eq) => {
                if let Some(i) = slot(eq.preset) {
                    self.custom[i] = eq.bands;
                }
                self.eq = eq;
                self.emit(Notification::Equalizer(self.eq));
            }
            Request::GetAudioUpsampling => {
                self.emit(Notification::AudioUpsampling(self.upsampling))
            }
            Request::SetAudioUpsampling(on) => {
                self.upsampling = on;
                self.emit(Notification::AudioUpsampling(on));
            }
            Request::GetSpeakToChat => self.emit(Notification::SpeakToChat(self.speak_to_chat)),
            Request::SetSpeakToChat(on) => {
                self.speak_to_chat = on;
                self.emit(Notification::SpeakToChat(on));
            }
            Request::GetSpeakToChatConfig => self.emit(Notification::SpeakToChatConfig(self.stc)),
            Request::SetSpeakToChatConfig(cfg) => {
                self.stc = cfg;
                self.emit(Notification::SpeakToChatConfig(cfg));
            }
            Request::GetTouchSensor => self.emit(Notification::TouchSensor(self.touch)),
            Request::SetTouchSensor(on) => {
                self.touch = on;
                self.emit(Notification::TouchSensor(on));
            }
            Request::GetPauseWhenTakenOff => self.emit(Notification::PauseWhenTakenOff(self.pause)),
            Request::SetPauseWhenTakenOff(on) => {
                self.pause = on;
                self.emit(Notification::PauseWhenTakenOff(on));
            }
            Request::GetAutoPowerOff => self.emit(Notification::AutoPowerOff(self.auto_power_off)),
            Request::SetAutoPowerOff(apo) => {
                self.auto_power_off = apo;
                self.emit(Notification::AutoPowerOff(apo));
            }
            Request::GetBgmMode | Request::GetCinemaMode => {
                if let Some((mode, room)) = self.listening {
                    if request == Request::GetBgmMode {
                        self.emit(Notification::BgmMode {
                            on: mode == ListeningMode::Bgm,
                            room,
                        });
                    } else {
                        self.emit(Notification::CinemaMode(mode == ListeningMode::Cinema));
                    }
                }
            }
            Request::SetBgmMode { on, room } => {
                if let Some((mode, _)) = self.listening {
                    let mode = if on {
                        ListeningMode::Bgm
                    } else if mode == ListeningMode::Bgm {
                        ListeningMode::Standard
                    } else {
                        mode
                    };
                    self.listening = Some((mode, room));
                    self.emit_listening();
                }
            }
            Request::SetCinemaMode(on) => {
                if let Some((mode, room)) = self.listening {
                    let mode = if on {
                        ListeningMode::Cinema
                    } else if mode == ListeningMode::Cinema {
                        ListeningMode::Standard
                    } else {
                        mode
                    };
                    self.listening = Some((mode, room));
                    self.emit_listening();
                }
            }
            Request::GetSurround => {
                if let Some(mode) = self.surround {
                    self.emit(Notification::Surround(mode));
                }
            }
            Request::SetSurround(mode) => {
                if self.surround.is_some() {
                    self.surround = Some(mode);
                    self.emit(Notification::Surround(mode));
                }
            }
            Request::PowerOff => self.powered_off = true,
        }
    }

    fn emit_listening(&mut self) {
        if let Some((mode, room)) = self.listening {
            self.emit(Notification::BgmMode {
                on: mode == ListeningMode::Bgm,
                room,
            });
            self.emit(Notification::CinemaMode(mode == ListeningMode::Cinema));
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

fn slot(preset: EqPreset) -> Option<usize> {
    match preset {
        EqPreset::Manual => Some(0),
        EqPreset::Custom1 => Some(1),
        EqPreset::Custom2 => Some(2),
        _ => None,
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

    fn drain(s: &mut Session) -> Vec<Notification> {
        let mut got = Vec::new();
        for _ in 0..10 {
            got.extend(s.poll(Duration::from_millis(10)).unwrap());
        }
        got
    }

    #[test]
    fn negotiates_v2_with_xm5_and_controls_it() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WH-1000XM5")));
        s.send(&Request::Init).unwrap();
        let got = drain(&mut s);
        assert!(got.contains(&Notification::InitReply(ProtocolVersion::V2)));
        assert!(s.dialect().is_v2());

        s.send(&Request::GetBattery(BatteryKind::Single)).unwrap();
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
        s.send(&Request::SetSpeakToChat(true)).unwrap();
        s.send(&Request::SetEqualizerPreset(EqPreset::BassBoost))
            .unwrap();
        let got = drain(&mut s);
        assert!(
            got.iter().any(|n| matches!(
                n,
                Notification::Battery {
                    kind: BatteryKind::Single,
                    ..
                }
            )),
            "{got:?}"
        );
        assert!(got.contains(&Notification::AmbientSound(target)), "{got:?}");
        assert!(got.contains(&Notification::SpeakToChat(true)), "{got:?}");
        assert!(
            got.iter().any(
                |n| matches!(n, Notification::Equalizer(e) if e.preset == EqPreset::BassBoost)
            )
        );
        assert!(matches!(
            s.send(&Request::SetTouchSensor(true)),
            Err(crate::protocol::SessionError::Unsupported(_))
        ));
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn xm6_listening_mode() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WH-1000XM6")));
        s.send(&Request::Init).unwrap();
        drain(&mut s);
        s.send(&Request::SetCinemaMode(true)).unwrap();
        assert!(drain(&mut s).contains(&Notification::CinemaMode(true)));
        s.send(&Request::SetBgmMode {
            on: true,
            room: RoomSize::Large,
        })
        .unwrap();
        let got = drain(&mut s);
        assert!(got.contains(&Notification::BgmMode {
            on: true,
            room: RoomSize::Large
        }));
        assert!(got.contains(&Notification::CinemaMode(false)));
    }

    #[test]
    fn xm5_has_no_listening_mode() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WH-1000XM5")));
        s.send(&Request::Init).unwrap();
        drain(&mut s);
        s.send(&Request::GetCinemaMode).unwrap();
        assert!(
            !drain(&mut s)
                .iter()
                .any(|n| matches!(n, Notification::CinemaMode(_)))
        );
    }

    #[test]
    fn negotiates_v1_with_xm4() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WH-1000XM4")));
        s.send(&Request::Init).unwrap();
        drain(&mut s);
        assert_eq!(s.dialect(), Dialect::V1);
        s.send(&Request::SetPauseWhenTakenOff(false)).unwrap();
        assert!(drain(&mut s).contains(&Notification::PauseWhenTakenOff(false)));
    }

    #[test]
    fn learns_ambient_type_of_earbuds() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WF-1000XM5")));
        s.send(&Request::Init).unwrap();
        drain(&mut s);
        // Asking with the wrong sub-type gets no answer...
        s.send(&Request::GetAmbientSound).unwrap();
        assert!(
            !drain(&mut s)
                .iter()
                .any(|n| matches!(n, Notification::AmbientSound(_)))
        );
        // ...the right one does, and the session remembers it.
        s.set_dialect(Dialect::v2(AmbientType::WithWind));
        s.send(&Request::GetAmbientSound).unwrap();
        assert!(
            drain(&mut s)
                .iter()
                .any(|n| matches!(n, Notification::AmbientSound(_)))
        );
        assert_eq!(s.dialect().ambient, AmbientType::WithWind);
    }

    #[test]
    fn power_off_closes_connection() {
        let mut s = Session::new(Box::new(SimulatedHeadset::new("WH-1000XM5")));
        s.send(&Request::Init).unwrap();
        drain(&mut s);
        s.send(&Request::PowerOff).unwrap();
        assert!(
            s.poll(Duration::from_millis(10)).is_err()
                || s.poll(Duration::from_millis(10)).is_err()
        );
    }
}
