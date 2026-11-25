//! Application-layer ("MDR" v1) messages: building requests and parsing the
//! replies / notifications sent back by the headset.
//!
//! The opcodes follow the publicly documented reverse-engineering work of the
//! open-source community (Gadgetbridge, SonyHeadphonesClient). Every parser is
//! bounds-checked: a malformed packet produces an error, never a panic.

use std::fmt;

/// Opcodes (first payload byte).
pub mod op {
    pub const INIT_REQUEST: u8 = 0x00;
    pub const INIT_REPLY: u8 = 0x01;
    pub const FW_VERSION_GET: u8 = 0x04;
    pub const FW_VERSION_RET: u8 = 0x05;
    pub const BATTERY_GET: u8 = 0x10;
    pub const BATTERY_RET: u8 = 0x11;
    pub const BATTERY_NOTIFY: u8 = 0x13;
    pub const CODEC_GET: u8 = 0x18;
    pub const CODEC_RET: u8 = 0x19;
    pub const CODEC_NOTIFY: u8 = 0x1B;
    pub const POWER_OFF: u8 = 0x22;
    pub const EQ_GET: u8 = 0x56;
    pub const EQ_RET: u8 = 0x57;
    pub const EQ_SET: u8 = 0x58;
    pub const EQ_NOTIFY: u8 = 0x59;
    pub const AMBIENT_GET: u8 = 0x66;
    pub const AMBIENT_RET: u8 = 0x67;
    pub const AMBIENT_SET: u8 = 0x68;
    pub const AMBIENT_NOTIFY: u8 = 0x69;
    pub const TOUCH_SENSOR_SET: u8 = 0xD8;
    pub const AUDIO_UPSAMPLING_GET: u8 = 0xE6;
    pub const AUDIO_UPSAMPLING_RET: u8 = 0xE7;
    pub const AUDIO_UPSAMPLING_SET: u8 = 0xE8;
    pub const AUDIO_UPSAMPLING_NOTIFY: u8 = 0xE9;
    pub const SYSTEM_SET: u8 = 0xF8;
    pub const SPEAK_TO_CHAT_CONFIG_SET: u8 = 0xFC;
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AmbientMode {
    #[default]
    NoiseCancelling,
    WindReduction,
    AmbientSound,
    Off,
}

/// Noise cancelling / ambient sound state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbientSound {
    pub mode: AmbientMode,
    /// Ambient sound level, 1..=20.
    pub level: u8,
    pub focus_on_voice: bool,
}

impl Default for AmbientSound {
    fn default() -> Self {
        Self {
            mode: AmbientMode::NoiseCancelling,
            level: 20,
            focus_on_voice: false,
        }
    }
}

pub const AMBIENT_LEVEL_MIN: u8 = 1;
pub const AMBIENT_LEVEL_MAX: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryKind {
    /// Over-ear headphones (single battery).
    Single,
    /// True-wireless earbuds (left + right).
    Dual,
    /// Charging case of true-wireless earbuds.
    Case,
}

impl BatteryKind {
    fn code(self) -> u8 {
        match self {
            BatteryKind::Single => 0x00,
            BatteryKind::Dual => 0x01,
            BatteryKind::Case => 0x02,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Battery {
    pub level: u8,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqPreset {
    Off,
    Bright,
    Excited,
    Mellow,
    Relaxed,
    Vocal,
    TrebleBoost,
    BassBoost,
    Speech,
    Manual,
    Custom1,
    Custom2,
    Unknown(u8),
}

impl EqPreset {
    pub const ALL: [EqPreset; 12] = [
        EqPreset::Off,
        EqPreset::Bright,
        EqPreset::Excited,
        EqPreset::Mellow,
        EqPreset::Relaxed,
        EqPreset::Vocal,
        EqPreset::TrebleBoost,
        EqPreset::BassBoost,
        EqPreset::Speech,
        EqPreset::Manual,
        EqPreset::Custom1,
        EqPreset::Custom2,
    ];

    pub fn code(self) -> u8 {
        match self {
            EqPreset::Off => 0x00,
            EqPreset::Bright => 0x10,
            EqPreset::Excited => 0x11,
            EqPreset::Mellow => 0x12,
            EqPreset::Relaxed => 0x13,
            EqPreset::Vocal => 0x14,
            EqPreset::TrebleBoost => 0x15,
            EqPreset::BassBoost => 0x16,
            EqPreset::Speech => 0x17,
            EqPreset::Manual => 0xA0,
            EqPreset::Custom1 => 0xA1,
            EqPreset::Custom2 => 0xA2,
            EqPreset::Unknown(c) => c,
        }
    }

    pub fn from_code(code: u8) -> Self {
        EqPreset::ALL
            .into_iter()
            .find(|p| p.code() == code)
            .unwrap_or(EqPreset::Unknown(code))
    }

    pub fn label(self) -> &'static str {
        match self {
            EqPreset::Off => "Off",
            EqPreset::Bright => "Bright",
            EqPreset::Excited => "Excited",
            EqPreset::Mellow => "Mellow",
            EqPreset::Relaxed => "Relaxed",
            EqPreset::Vocal => "Vocal",
            EqPreset::TrebleBoost => "Treble Boost",
            EqPreset::BassBoost => "Bass Boost",
            EqPreset::Speech => "Speech",
            EqPreset::Manual => "Manual",
            EqPreset::Custom1 => "Custom 1",
            EqPreset::Custom2 => "Custom 2",
            EqPreset::Unknown(_) => "Unknown",
        }
    }

    /// Whether the user can edit the bands of this preset.
    pub fn is_editable(self) -> bool {
        matches!(
            self,
            EqPreset::Manual | EqPreset::Custom1 | EqPreset::Custom2
        )
    }

    /// Approximate band curve of the built-in presets (used to preview them,
    /// and by the simulated device).
    pub fn default_bands(self) -> [i8; 6] {
        match self {
            EqPreset::Bright => [0, -1, 0, 2, 4, 5],
            EqPreset::Excited => [2, 3, 0, -1, 2, 4],
            EqPreset::Mellow => [1, 2, 1, 0, -2, -4],
            EqPreset::Relaxed => [0, -1, -2, -3, -4, -5],
            EqPreset::Vocal => [-2, -1, 3, 4, 2, 0],
            EqPreset::TrebleBoost => [0, 0, 0, 2, 5, 8],
            EqPreset::BassBoost => [6, 4, 1, 0, 0, 0],
            EqPreset::Speech => [-4, 0, 4, 4, 1, -2],
            _ => [0; 6],
        }
    }
}

/// Equalizer: preset + Clear Bass + 5 bands (400 Hz, 1 kHz, 2.5 kHz,
/// 6.3 kHz, 16 kHz). Every value is in the range -10..=10.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Equalizer {
    pub preset: EqPreset,
    /// `[clear_bass, 400, 1k, 2.5k, 6.3k, 16k]`
    pub bands: [i8; 6],
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            preset: EqPreset::Off,
            bands: [0; 6],
        }
    }
}

pub const EQ_BAND_LABELS: [&str; 6] = ["CLEAR\nBASS", "400", "1k", "2.5k", "6.3k", "16k"];
pub const EQ_MIN: i8 = -10;
pub const EQ_MAX: i8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Sbc,
    Aac,
    Ldac,
    AptX,
    AptXHd,
    Lc3,
    Unknown(u8),
}

impl Codec {
    pub fn from_code(code: u8) -> Self {
        match code {
            0x01 => Codec::Sbc,
            0x02 => Codec::Aac,
            0x10 => Codec::Ldac,
            0x20 => Codec::AptX,
            0x21 => Codec::AptXHd,
            0x30 => Codec::Lc3,
            c => Codec::Unknown(c),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Codec::Sbc => 0x01,
            Codec::Aac => 0x02,
            Codec::Ldac => 0x10,
            Codec::AptX => 0x20,
            Codec::AptXHd => 0x21,
            Codec::Lc3 => 0x30,
            Codec::Unknown(c) => c,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Codec::Sbc => "SBC",
            Codec::Aac => "AAC",
            Codec::Ldac => "LDAC",
            Codec::AptX => "aptX",
            Codec::AptXHd => "aptX HD",
            Codec::Lc3 => "LC3",
            Codec::Unknown(_) => "—",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AutoPowerOff {
    #[default]
    Off,
    After5Min,
    After30Min,
    After1Hour,
    After3Hours,
}

impl AutoPowerOff {
    pub const ALL: [AutoPowerOff; 5] = [
        AutoPowerOff::Off,
        AutoPowerOff::After5Min,
        AutoPowerOff::After30Min,
        AutoPowerOff::After1Hour,
        AutoPowerOff::After3Hours,
    ];

    fn codes(self) -> [u8; 2] {
        match self {
            AutoPowerOff::Off => [0x11, 0x00],
            AutoPowerOff::After5Min => [0x00, 0x00],
            AutoPowerOff::After30Min => [0x01, 0x01],
            AutoPowerOff::After1Hour => [0x02, 0x02],
            AutoPowerOff::After3Hours => [0x03, 0x03],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AutoPowerOff::Off => "Do not turn off",
            AutoPowerOff::After5Min => "After 5 minutes",
            AutoPowerOff::After30Min => "After 30 minutes",
            AutoPowerOff::After1Hour => "After 1 hour",
            AutoPowerOff::After3Hours => "After 3 hours",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StcSensitivity {
    #[default]
    Auto,
    High,
    Low,
}

impl StcSensitivity {
    pub const ALL: [StcSensitivity; 3] = [
        StcSensitivity::Auto,
        StcSensitivity::High,
        StcSensitivity::Low,
    ];
    fn code(self) -> u8 {
        match self {
            StcSensitivity::Auto => 0,
            StcSensitivity::High => 1,
            StcSensitivity::Low => 2,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            StcSensitivity::Auto => "Auto",
            StcSensitivity::High => "High",
            StcSensitivity::Low => "Low",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StcTimeout {
    Short,
    #[default]
    Standard,
    Long,
    Never,
}

impl StcTimeout {
    pub const ALL: [StcTimeout; 4] = [
        StcTimeout::Short,
        StcTimeout::Standard,
        StcTimeout::Long,
        StcTimeout::Never,
    ];
    fn code(self) -> u8 {
        match self {
            StcTimeout::Short => 0,
            StcTimeout::Standard => 1,
            StcTimeout::Long => 2,
            StcTimeout::Never => 3,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            StcTimeout::Short => "Short (approx. 15 sec.)",
            StcTimeout::Standard => "Standard (approx. 30 sec.)",
            StcTimeout::Long => "Long (approx. 1 min.)",
            StcTimeout::Never => "Do not end automatically",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SpeakToChatConfig {
    pub sensitivity: StcSensitivity,
    pub focus_on_voice: bool,
    pub timeout: StcTimeout,
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// A command sent to the headset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Init,
    GetFirmwareVersion,
    GetBattery(BatteryKind),
    GetCodec,
    GetAmbientSound,
    /// `adjusting = true` while a slider is still being dragged.
    SetAmbientSound {
        value: AmbientSound,
        adjusting: bool,
    },
    GetEqualizer,
    SetEqualizer(Equalizer),
    GetAudioUpsampling,
    SetAudioUpsampling(bool),
    SetSpeakToChat(bool),
    SetSpeakToChatConfig(SpeakToChatConfig),
    SetTouchSensor(bool),
    SetPauseWhenTakenOff(bool),
    SetAutoPowerOff(AutoPowerOff),
    PowerOff,
}

impl Request {
    /// Encode the request as an MDR payload.
    pub fn payload(&self) -> Vec<u8> {
        match self {
            Request::Init => vec![op::INIT_REQUEST, 0x00],
            Request::GetFirmwareVersion => vec![op::FW_VERSION_GET, 0x02],
            Request::GetBattery(kind) => vec![op::BATTERY_GET, kind.code()],
            Request::GetCodec => vec![op::CODEC_GET, 0x00],
            Request::GetAmbientSound => vec![op::AMBIENT_GET, 0x02],
            Request::SetAmbientSound { value, adjusting } => {
                let effect = match (value.mode, adjusting) {
                    (AmbientMode::Off, _) => 0x00,
                    (_, true) => 0x10,
                    (_, false) => 0x11,
                };
                let nc = match value.mode {
                    AmbientMode::NoiseCancelling => 0x02,
                    AmbientMode::WindReduction => 0x01,
                    AmbientMode::AmbientSound | AmbientMode::Off => 0x00,
                };
                let level = value.level.clamp(AMBIENT_LEVEL_MIN, AMBIENT_LEVEL_MAX);
                let level = if value.mode == AmbientMode::AmbientSound {
                    level
                } else {
                    0
                };
                vec![
                    op::AMBIENT_SET,
                    0x02, // inquired type: NC + ambient sound
                    effect,
                    0x02, // NC setting type: dual / single / off
                    nc,
                    0x01, // ASM setting type: level adjustment
                    u8::from(value.focus_on_voice),
                    level,
                ]
            }
            Request::GetEqualizer => vec![op::EQ_GET, 0x00],
            Request::SetEqualizer(eq) => {
                let mut p = vec![op::EQ_SET, 0x00, eq.preset.code(), 0x06];
                p.extend(eq.bands.iter().map(|b| encode_band(*b)));
                p
            }
            Request::GetAudioUpsampling => vec![op::AUDIO_UPSAMPLING_GET, 0x02],
            Request::SetAudioUpsampling(on) => {
                vec![op::AUDIO_UPSAMPLING_SET, 0x02, 0x00, u8::from(*on)]
            }
            Request::SetSpeakToChat(on) => vec![op::SYSTEM_SET, 0x0C, u8::from(*on), 0x01],
            Request::SetSpeakToChatConfig(cfg) => vec![
                op::SPEAK_TO_CHAT_CONFIG_SET,
                0x0C,
                0x00,
                cfg.sensitivity.code(),
                u8::from(cfg.focus_on_voice),
                cfg.timeout.code(),
            ],
            Request::SetTouchSensor(on) => vec![op::TOUCH_SENSOR_SET, 0xD2, 0x01, u8::from(*on)],
            Request::SetPauseWhenTakenOff(on) => vec![op::SYSTEM_SET, 0x01, u8::from(!*on)],
            Request::SetAutoPowerOff(apo) => {
                let [a, b] = apo.codes();
                vec![op::SYSTEM_SET, 0x04, 0x01, a, b]
            }
            Request::PowerOff => vec![op::POWER_OFF, 0x00, 0x01],
        }
    }
}

fn encode_band(v: i8) -> u8 {
    // Clamped to -10..=10, so the sum is always within 0..=20.
    u8::try_from(i16::from(v.clamp(EQ_MIN, EQ_MAX)) + 10).unwrap_or(10)
}

fn decode_band(v: u8) -> i8 {
    let v = i16::from(v) - 10;
    i8::try_from(v.clamp(i16::from(EQ_MIN), i16::from(EQ_MAX))).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Responses / notifications
// ---------------------------------------------------------------------------

/// Information reported by the headset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notification {
    InitReply,
    FirmwareVersion(String),
    Battery {
        kind: BatteryKind,
        left: Battery,
        right: Option<Battery>,
    },
    Codec(Codec),
    AmbientSound(AmbientSound),
    Equalizer(Equalizer),
    AudioUpsampling(bool),
    /// Valid frame we do not interpret (opcode + raw payload).
    Unknown(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    Truncated {
        opcode: u8,
        needed: usize,
        got: usize,
    },
    Invalid {
        opcode: u8,
        reason: &'static str,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty payload"),
            ParseError::Truncated {
                opcode,
                needed,
                got,
            } => {
                write!(
                    f,
                    "message {opcode:#04x} truncated: needed {needed} bytes, got {got}"
                )
            }
            ParseError::Invalid { opcode, reason } => {
                write!(f, "message {opcode:#04x} invalid: {reason}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

fn need(p: &[u8], n: usize) -> Result<(), ParseError> {
    if p.len() < n {
        Err(ParseError::Truncated {
            opcode: p.first().copied().unwrap_or(0),
            needed: n,
            got: p.len(),
        })
    } else {
        Ok(())
    }
}

fn battery(level: u8, charging: u8) -> Battery {
    Battery {
        level: level.min(100),
        charging: charging == 0x01,
    }
}

impl Notification {
    pub fn parse(p: &[u8]) -> Result<Self, ParseError> {
        let &opcode = p.first().ok_or(ParseError::Empty)?;
        match opcode {
            op::INIT_REPLY => Ok(Notification::InitReply),
            op::FW_VERSION_RET => {
                need(p, 3)?;
                let len = usize::from(p[2]);
                need(p, 3 + len)?;
                let text = String::from_utf8_lossy(&p[3..3 + len]).trim().to_string();
                Ok(Notification::FirmwareVersion(text))
            }
            op::BATTERY_RET | op::BATTERY_NOTIFY => {
                need(p, 4)?;
                match p[1] {
                    0x00 => Ok(Notification::Battery {
                        kind: BatteryKind::Single,
                        left: battery(p[2], p[3]),
                        right: None,
                    }),
                    0x01 => {
                        need(p, 6)?;
                        Ok(Notification::Battery {
                            kind: BatteryKind::Dual,
                            left: battery(p[2], p[3]),
                            right: Some(battery(p[4], p[5])),
                        })
                    }
                    0x02 => Ok(Notification::Battery {
                        kind: BatteryKind::Case,
                        left: battery(p[2], p[3]),
                        right: None,
                    }),
                    _ => Err(ParseError::Invalid {
                        opcode,
                        reason: "unknown battery type",
                    }),
                }
            }
            op::CODEC_RET | op::CODEC_NOTIFY => {
                need(p, 3)?;
                Ok(Notification::Codec(Codec::from_code(p[2])))
            }
            op::AMBIENT_RET | op::AMBIENT_NOTIFY => {
                need(p, 8)?;
                let effect = p[2];
                let nc = p[4];
                let mode = if effect == 0x00 {
                    AmbientMode::Off
                } else if nc == 0x02 || (p[3] == 0x00 && nc == 0x01) {
                    AmbientMode::NoiseCancelling
                } else if nc == 0x01 {
                    AmbientMode::WindReduction
                } else {
                    AmbientMode::AmbientSound
                };
                Ok(Notification::AmbientSound(AmbientSound {
                    mode,
                    level: p[7].clamp(AMBIENT_LEVEL_MIN, AMBIENT_LEVEL_MAX),
                    focus_on_voice: p[6] == 0x01,
                }))
            }
            op::EQ_RET | op::EQ_NOTIFY => {
                need(p, 3)?;
                let preset = EqPreset::from_code(p[2]);
                let mut bands = [0i8; 6];
                if p.len() >= 4 {
                    let count = usize::from(p[3]).min(6);
                    need(p, 4 + count)?;
                    for (dst, src) in bands.iter_mut().zip(&p[4..4 + count]) {
                        *dst = decode_band(*src);
                    }
                }
                Ok(Notification::Equalizer(Equalizer { preset, bands }))
            }
            op::AUDIO_UPSAMPLING_RET | op::AUDIO_UPSAMPLING_NOTIFY => {
                need(p, 4)?;
                Ok(Notification::AudioUpsampling(p[3] == 0x01))
            }
            _ => Ok(Notification::Unknown(p.to_vec())),
        }
    }

    /// Encode as the device would (used by the simulator and tests).
    pub fn payload(&self) -> Vec<u8> {
        match self {
            Notification::InitReply => vec![op::INIT_REPLY, 0x00, 0x00, 0x00],
            Notification::FirmwareVersion(v) => {
                let bytes = v.as_bytes();
                let len = bytes.len().min(255);
                let mut p = vec![op::FW_VERSION_RET, 0x02, u8::try_from(len).unwrap_or(255)];
                p.extend_from_slice(&bytes[..len]);
                p
            }
            Notification::Battery { kind, left, right } => {
                let mut p = vec![
                    op::BATTERY_NOTIFY,
                    kind.code(),
                    left.level,
                    u8::from(left.charging),
                ];
                if let Some(r) = right {
                    p.extend([r.level, u8::from(r.charging)]);
                }
                p
            }
            Notification::Codec(c) => vec![op::CODEC_NOTIFY, 0x00, c.code()],
            Notification::AmbientSound(a) => {
                let mut p = Request::SetAmbientSound {
                    value: *a,
                    adjusting: false,
                }
                .payload();
                p[0] = op::AMBIENT_NOTIFY;
                p[7] = a.level;
                p
            }
            Notification::Equalizer(eq) => {
                let mut p = Request::SetEqualizer(*eq).payload();
                p[0] = op::EQ_NOTIFY;
                p
            }
            Notification::AudioUpsampling(on) => {
                vec![op::AUDIO_UPSAMPLING_NOTIFY, 0x02, 0x00, u8::from(*on)]
            }
            Notification::Unknown(p) => p.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_payloads() {
        let nc = Request::SetAmbientSound {
            value: AmbientSound::default(),
            adjusting: false,
        };
        assert_eq!(
            nc.payload(),
            vec![0x68, 0x02, 0x11, 0x02, 0x02, 0x01, 0x00, 0x00]
        );

        let asm = Request::SetAmbientSound {
            value: AmbientSound {
                mode: AmbientMode::AmbientSound,
                level: 15,
                focus_on_voice: true,
            },
            adjusting: true,
        };
        assert_eq!(
            asm.payload(),
            vec![0x68, 0x02, 0x10, 0x02, 0x00, 0x01, 0x01, 15]
        );

        let off = Request::SetAmbientSound {
            value: AmbientSound {
                mode: AmbientMode::Off,
                level: 7,
                focus_on_voice: false,
            },
            adjusting: false,
        };
        assert_eq!(off.payload()[2], 0x00);
    }

    #[test]
    fn ambient_roundtrip() {
        for mode in [
            AmbientMode::NoiseCancelling,
            AmbientMode::WindReduction,
            AmbientMode::AmbientSound,
            AmbientMode::Off,
        ] {
            let a = AmbientSound {
                mode,
                level: 12,
                focus_on_voice: mode == AmbientMode::AmbientSound,
            };
            let parsed = Notification::parse(&Notification::AmbientSound(a).payload()).unwrap();
            assert_eq!(parsed, Notification::AmbientSound(a));
        }
    }

    #[test]
    fn equalizer_roundtrip_and_clamping() {
        let eq = Equalizer {
            preset: EqPreset::Custom1,
            bands: [10, -10, 0, 3, -4, 7],
        };
        let p = Request::SetEqualizer(eq).payload();
        assert_eq!(p, vec![0x58, 0x00, 0xA1, 0x06, 20, 0, 10, 13, 6, 17]);
        assert_eq!(
            Notification::parse(&Notification::Equalizer(eq).payload()).unwrap(),
            Notification::Equalizer(eq)
        );

        let wild = Request::SetEqualizer(Equalizer {
            preset: EqPreset::Manual,
            bands: [127, -128, 0, 0, 0, 0],
        });
        assert_eq!(&wild.payload()[4..6], &[20, 0]);
    }

    #[test]
    fn battery_parsing() {
        assert_eq!(
            Notification::parse(&[0x11, 0x00, 80, 0x01]).unwrap(),
            Notification::Battery {
                kind: BatteryKind::Single,
                left: Battery {
                    level: 80,
                    charging: true
                },
                right: None
            }
        );
        assert_eq!(
            Notification::parse(&[0x13, 0x01, 70, 0x00, 60, 0x01]).unwrap(),
            Notification::Battery {
                kind: BatteryKind::Dual,
                left: Battery {
                    level: 70,
                    charging: false
                },
                right: Some(Battery {
                    level: 60,
                    charging: true
                })
            }
        );
        // Out-of-range levels are clamped instead of trusted.
        assert!(matches!(
            Notification::parse(&[0x11, 0x00, 250, 0x00]).unwrap(),
            Notification::Battery {
                left: Battery { level: 100, .. },
                ..
            }
        ));
    }

    #[test]
    fn truncated_messages_are_errors_not_panics() {
        for len in 0..8 {
            for opcode in [0x05, 0x11, 0x13, 0x19, 0x57, 0x67, 0x69, 0xE7] {
                let mut p = vec![opcode];
                p.extend(std::iter::repeat_n(0x09, len));
                // Must never panic; errors are fine.
                let _ = Notification::parse(&p);
            }
        }
        assert_eq!(Notification::parse(&[]), Err(ParseError::Empty));
        assert!(matches!(
            Notification::parse(&[0x05, 0x02, 10, b'1']),
            Err(ParseError::Truncated { .. })
        ));
    }

    #[test]
    fn firmware_and_codec() {
        let n = Notification::FirmwareVersion("2.0.1".into());
        assert_eq!(Notification::parse(&n.payload()).unwrap(), n);
        assert_eq!(
            Notification::parse(&[0x19, 0x00, 0x10]).unwrap(),
            Notification::Codec(Codec::Ldac)
        );
    }

    #[test]
    fn eq_preset_codes_are_unique() {
        for p in EqPreset::ALL {
            assert_eq!(EqPreset::from_code(p.code()), p);
        }
    }
}
